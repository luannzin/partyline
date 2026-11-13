//! The built-in "virtual LAN": iroh QUIC connections dialed by endpoint id (hole punching,
//! relay fallback), mDNS room discovery on the local network, and the pumps between a
//! connection and the session server.
use std::net::IpAddr;
use std::time::Duration;

use iroh::endpoint::{Connection, presets};
use iroh::endpoint_info::UserData;
use iroh::{Endpoint, EndpointId, TransportAddr};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::StreamExt;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::proto::{self, ClientMsg, Link, Pid, ServerMsg};
use crate::server::{Cmd, Handle};

pub const ALPN: &[u8] = b"partyline/1";
const TAG: &str = "partyline1:";

#[derive(Clone)]
pub struct Net {
    pub ep: Endpoint,
    mdns: MdnsAddressLookup,
}

/// A client's view of a session, identical for the host's own player and remote players.
pub struct Conn {
    pub me: Pid,
    pub tx: mpsc::UnboundedSender<ClientMsg>,
    pub rx: mpsc::Receiver<ServerMsg>,
}

#[derive(Clone, Debug)]
pub struct RoomAd {
    pub id: EndpointId,
    pub name: String,
    pub locked: bool,
}

pub enum Lan {
    Up(RoomAd),
    Down(EndpointId),
}

pub async fn start() -> anyhow::Result<Net> {
    let ep = Endpoint::builder(presets::N0).alpns(vec![ALPN.to_vec()]).bind().await?;
    let mdns = MdnsAddressLookup::builder().build(ep.id())?;
    ep.address_lookup()?.add(mdns.clone());
    Ok(Net { ep, mdns })
}

impl Net {
    pub fn code(&self) -> String {
        self.ep.id().to_string()
    }

    /// Stream of rooms advertised on the local network.
    pub fn scan(&self) -> mpsc::UnboundedReceiver<Lan> {
        let (tx, rx) = mpsc::unbounded_channel();
        let (mdns, me) = (self.mdns.clone(), self.ep.id());
        tokio::spawn(async move {
            let mut events = mdns.subscribe().await;
            while let Some(e) = events.next().await {
                let out = match e {
                    DiscoveryEvent::Discovered { endpoint_info, .. } if endpoint_info.endpoint_id != me => {
                        let Some(rest) = endpoint_info.data.user_data().and_then(|u| u.as_ref().strip_prefix(TAG)) else {
                            continue;
                        };
                        let locked = rest.starts_with('1');
                        let name: String = rest.chars().skip(1).take(24).collect();
                        Lan::Up(RoomAd { id: endpoint_info.endpoint_id, name, locked })
                    }
                    DiscoveryEvent::Expired { endpoint_id } => Lan::Down(endpoint_id),
                    _ => continue,
                };
                if tx.send(out).is_err() {
                    break;
                }
            }
        });
        rx
    }

    /// Advertise the room on the LAN and accept remote players into `server`.
    pub fn host(&self, room: &str, locked: bool, server: Handle) -> anyhow::Result<JoinHandle<()>> {
        let ad = format!("{TAG}{}{}", locked as u8, room.chars().take(24).collect::<String>());
        self.ep.set_user_data_for_address_lookup(Some(UserData::try_from(ad)?));
        let ep = self.ep.clone();
        Ok(tokio::spawn(async move {
            while let Some(inc) = ep.accept().await {
                let server = server.clone();
                tokio::spawn(async move {
                    if let Ok(conn) = inc.await {
                        let _ = serve(conn, server).await;
                    }
                });
            }
        }))
    }

    pub fn stop_hosting(&self) {
        self.ep.set_user_data_for_address_lookup(None);
    }

    pub async fn join(ep: Endpoint, id: EndpointId, name: String, password: String) -> anyhow::Result<Conn> {
        let conn = tokio::time::timeout(Duration::from_secs(15), ep.connect(id, ALPN))
            .await
            .map_err(|_| anyhow::anyhow!("Timed out reaching the room."))??;
        let (mut send, mut recv) = conn.open_bi().await?;
        proto::send(&mut send, &ClientMsg::Hello { version: env!("CARGO_PKG_VERSION").into(), name, password }).await?;
        let me = match proto::recv::<ServerMsg>(&mut recv).await? {
            ServerMsg::Welcome { you } => you,
            ServerMsg::Reject(why) => anyhow::bail!(why),
            ServerMsg::Frame { .. } => anyhow::bail!("Unexpected reply from host."),
        };
        let (tx, mut crx) = mpsc::unbounded_channel::<ClientMsg>();
        let (stx, rx) = mpsc::channel::<ServerMsg>(256);
        tokio::spawn(async move {
            while let Some(m) = crx.recv().await {
                if proto::send(&mut send, &m).await.is_err() {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            while let Ok(m) = proto::recv::<ServerMsg>(&mut recv).await {
                if stx.send(m).await.is_err() {
                    break;
                }
            }
            conn.close(0u8.into(), b"bye");
        });
        Ok(Conn { me, tx, rx })
    }
}

/// The host's own player joins its server through channels instead of the network.
pub async fn local(server: &Handle, name: String, password: String) -> anyhow::Result<Conn> {
    let (stx, rx) = mpsc::channel(256);
    let (reply, ok) = oneshot::channel();
    server.send(Cmd::Join { name, password, link: Link::Host, tx: stx, reply }).map_err(|_| anyhow::anyhow!("server gone"))?;
    let me = ok.await?.map_err(anyhow::Error::msg)?;
    let (tx, mut crx) = mpsc::unbounded_channel::<ClientMsg>();
    let server = server.clone();
    tokio::spawn(async move {
        while let Some(m) = crx.recv().await {
            if server.send(Cmd::Msg(me, m)).is_err() {
                break;
            }
        }
    });
    Ok(Conn { me, tx, rx })
}

async fn serve(conn: Connection, server: Handle) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;
    let ClientMsg::Hello { version, name, password } = proto::recv(&mut recv).await? else {
        anyhow::bail!("expected hello");
    };
    let (tx, mut rx) = mpsc::channel(64);
    let (reply, ok) = oneshot::channel();
    let (link, _) = link_of(&conn);
    let ours = env!("CARGO_PKG_VERSION");
    if version == ours {
        server.send(Cmd::Join { name, password, link, tx, reply })?;
    } else {
        let _ = reply.send(Err(format!(
            "This room runs PartyLine v{ours} and you have v{version}. Restart PartyLine to update it, or ask the host to."
        )));
    }
    let pid = match ok.await? {
        Ok(pid) => {
            proto::send(&mut send, &ServerMsg::Welcome { you: pid }).await?;
            pid
        }
        Err(why) => {
            proto::send(&mut send, &ServerMsg::Reject(why)).await?;
            send.finish()?;
            let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
            return Ok(());
        }
    };
    let mut writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if proto::send(&mut send, &m).await.is_err() {
                break;
            }
        }
    });
    let stats = {
        let (conn, server) = (conn.clone(), server.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let (link, ping) = link_of(&conn);
                if server.send(Cmd::Stats(pid, link, ping)).is_err() {
                    break;
                }
            }
        })
    };
    loop {
        tokio::select! {
            m = proto::recv::<ClientMsg>(&mut recv) => match m {
                Ok(m) => if server.send(Cmd::Msg(pid, m)).is_err() { break },
                Err(_) => break,
            },
            _ = &mut writer => break,
        }
    }
    let _ = server.send(Cmd::Leave(pid));
    writer.abort();
    stats.abort();
    conn.close(0u8.into(), b"bye");
    Ok(())
}

/// Which way packets currently flow, for the lobby badge.
fn link_of(conn: &Connection) -> (Link, u16) {
    let paths = conn.paths();
    let Some(p) = paths.iter().find(|p| p.is_selected()) else { return (Link::Relay, 0) };
    let ping = p.rtt().as_millis().min(9999) as u16;
    let link = match p.remote_addr() {
        TransportAddr::Ip(a) if is_lan(a.ip()) => Link::Lan,
        TransportAddr::Ip(_) => Link::Direct,
        _ => Link::Relay,
    };
    (link, ping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Phase;

    /// Real network: two endpoints on this machine, LAN discovery, join by endpoint id, frames flow.
    /// Run with `cargo test two_endpoints -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn two_endpoints_discover_join_and_play() {
        let (a, b) = (start().await.unwrap(), start().await.unwrap());
        let server = crate::server::spawn("test room".into(), a.code(), "pw".into(), 0);
        let _accept = a.host("test room", true, server.clone()).unwrap();
        let _host = local(&server, "host".into(), "pw".into()).await.unwrap();

        let mut lan = b.scan();
        let ad = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Some(Lan::Up(ad)) = lan.recv().await {
                    if ad.id == a.ep.id() {
                        return ad;
                    }
                }
            }
        })
        .await
        .expect("room never showed up on the LAN");
        assert_eq!(ad.name, "test room");
        assert!(ad.locked);

        let wrong = Net::join(b.ep.clone(), a.ep.id(), "guest".into(), "nope".into()).await;
        assert!(wrong.err().unwrap().to_string().contains("Wrong password"));

        let mut guest = Net::join(b.ep.clone(), a.ep.id(), "guest".into(), "pw".into()).await.unwrap();
        guest.tx.send(ClientMsg::Ready(true)).unwrap();
        let ok = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(m) = guest.rx.recv().await {
                if let ServerMsg::Frame { s, .. } = m {
                    let me = s.player(guest.me).cloned();
                    if s.players.len() == 2 && me.is_some_and(|p| p.ready) && matches!(s.phase, Phase::Lobby) {
                        return true;
                    }
                }
            }
            false
        })
        .await
        .unwrap();
        assert!(ok);
    }
}

fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
    }
}
