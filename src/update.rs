//! Self-update from GitHub Releases. An installed copy looks for a newer release once at start,
//! in the background; when there is one it downloads this platform's binary and swaps it in for
//! the running file (which keeps running). The next launch is the new version.
//! Trust is HTTPS to this repo's releases, the same as running the install script.
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

const REPO: &str = "luannzin/partyline";
/// This platform's binary on every release (named by .github/workflows/release.yml).
#[cfg(windows)]
const ASSET: &str = "partyline-windows-x86_64.exe";
#[cfg(target_os = "linux")]
const ASSET: &str = "partyline-linux-x86_64";
#[cfg(target_os = "macos")]
const ASSET: &str = "partyline-macos";

/// Starts the background check. The receiver gets the release tag once a newer version is in
/// place. Dev builds (anything under a cargo `target` folder) never update.
pub fn start(enabled: bool) -> Receiver<String> {
    let (tx, rx) = channel();
    let Ok(exe) = std::env::current_exe() else { return rx };
    let _ = std::fs::remove_file(old(&exe)); // left behind by the last update on Windows
    if !enabled || cfg!(debug_assertions) || exe.ancestors().any(|a| a.file_name().is_some_and(|n| n == "target")) {
        return rx;
    }
    std::thread::spawn(move || {
        let part = exe.with_extension("part");
        match update(&exe, &part, env!("CARGO_PKG_VERSION")) {
            Ok(Some(tag)) => {
                let _ = tx.send(tag);
            }
            Ok(None) => {}
            Err(_) => {
                let _ = std::fs::remove_file(&part);
            }
        }
    });
    rx
}

/// Replaces `exe` with the latest release's binary when that release is newer than `current`.
fn update(exe: &Path, part: &Path, current: &str) -> Result<Option<String>, Box<dyn Error>> {
    let agent = |redirects: u32| -> ureq::Agent {
        ureq::Agent::config_builder().max_redirects(redirects).timeout_global(Some(Duration::from_secs(600))).build().into()
    };
    // /releases/latest redirects to /releases/tag/<tag>: no API call, no rate limit.
    let latest = agent(0).get(format!("https://github.com/{REPO}/releases/latest")).call()?;
    let location = latest.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or_default();
    let tag = location.rsplit('/').next().unwrap_or_default().to_string();
    if !newer(&tag, current) {
        return Ok(None);
    }
    let mut res = agent(10).get(format!("https://github.com/{REPO}/releases/download/{tag}/{ASSET}")).call()?;
    io::copy(&mut res.body_mut().as_reader(), &mut std::fs::File::create(part)?)?;
    swap(part, exe)?;
    Ok(Some(tag))
}

/// Whether release tag `tag` ("v1.2.3") is a newer version than `current` ("1.2.0"). A tag that
/// isn't a plain version never is.
fn newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|n| n.parse::<u64>().ok()).collect::<Option<Vec<_>>>();
    match (tag.strip_prefix('v').and_then(parse), parse(current)) {
        (Some(new), Some(cur)) => new > cur,
        _ => false,
    }
}

fn old(exe: &Path) -> PathBuf {
    exe.with_extension("old")
}

#[cfg(unix)]
fn swap(new: &Path, exe: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))?;
    // The running process keeps the replaced file open; rename is atomic.
    std::fs::rename(new, exe)
}

#[cfg(windows)]
fn swap(new: &Path, exe: &Path) -> io::Result<()> {
    // A running exe can't be overwritten, but it can be renamed out of the way.
    std::fs::rename(exe, old(exe))?;
    std::fs::rename(new, exe).inspect_err(|_| {
        let _ = std::fs::rename(old(exe), exe);
    })
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    #[ignore = "downloads the latest release from GitHub"]
    fn swaps_in_the_latest_release() {
        let dir = std::env::temp_dir().join(format!("partyline-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(if cfg!(windows) { "partyline.exe" } else { "partyline" });
        std::fs::write(&exe, b"old build").unwrap();
        let tag = super::update(&exe, &exe.with_extension("part"), "0.0.1").unwrap().expect("a release exists");
        assert!(tag.starts_with('v'));
        assert!(std::fs::metadata(&exe).unwrap().len() > 1_000_000, "the release binary replaced the old file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_plain_newer_versions_update() {
        assert!(newer("v0.2.1", "0.2.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(!newer("v0.2.0", "0.2.0"));
        assert!(!newer("v0.1.9", "0.2.0"));
        assert!(!newer("v0.3.0-beta", "0.2.0"));
        assert!(!newer("", "0.2.0"));
        assert!(!newer("0.3.0", "0.2.0"));
    }
}
