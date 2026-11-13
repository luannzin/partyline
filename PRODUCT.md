# Product

<!-- impeccable:product-schema 1 -->

Working name: PartyLine (inferred, not confirmed by the user).

All facts below are inferred from the user's own brief in chat; the user explicitly waived a product interview ("no need to ask for PRODUCT, you already know").

## Platform

terminal (TUI). Not web, not native mobile. Renders in a truecolor terminal (Windows Terminal primary, any modern Unix terminal, later SSH guests).

## Stack

Rust, ratatui + crossterm, tachyonfx for effects, iroh for peer-to-peer networking. Chosen by the user in the original brief (Rust + ratatui) with networking delegated.

## Users

Groups of friends playing together, in the same room or remote. One person hosts, others join. Some watch as spectators, often on a TV in the room.

## Product Purpose

A competitive party game made of many short minigames, played entirely in the terminal. Create a room, friends join over LAN or internet with no server to set up, play a sequence of minigames or a tournament, crown a winner.

## Positioning

Radmin-style rooms (room name + password, LAN auto-discovery) built into the game itself, with no backend the players run. Terminal-native party game: no install of drivers, no accounts.

## Operating Context

LAN parties, couch sessions with a TV showing the spectator view, remote friends on voice chat. Keyboard only. Sessions of 15 to 60 minutes. Players vary in terminal size.

## Capabilities and Constraints

- Main menu: host, join, settings.
- Lobby: player list, ready state, party mode, spectators.
- Minigames: short (30 to 90 seconds), 2 to 8 players.
- Spectator can watch any player's view.
- Input is key presses only; key release is not reliable across terminals.
- Host-authoritative networking; latency up to ~150 ms must stay playable.
- Undecided: tournament brackets, board meta mode, SSH guest mode (later phases).

## Brand Commitments

User words: "extremely amazing", "revolutionary", "juiceness". Juice and game feel are the bar.

## Evidence on Hand

None. No logo, no assets, no existing art. Do not fabricate player counts, reviews, or claims.

## Product Principles

1. Juice is a feature: every action answers with motion, color, or sound.
2. Zero friction to play: joining a room takes seconds, no accounts.
3. Fair under latency: minigames designed so network delay does not decide winners.
4. Readable from the couch: the spectator view works on a TV across a room.
