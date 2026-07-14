# 🛠 SportsPulse — Development Implementation Plan

This document outlines the multi-phase implementation roadmap for building **SportsPulse**.

---

## Development Phases

### Phase 1 — Project Scaffold & Environment Setup
- [x] Initialize Tauri v2 project directly in the workspace root (`e:\Projects\Sports Extension`).
- [x] Configure `tauri.conf.json` for a tray-only, borderless, transparent window `main` with dimensions 340x140.
- [x] Set system tray configurations in `lib.rs` (with toggle visibility on click, quit menu, etc.).
- [x] Add project dependencies in `Cargo.toml` (e.g. `tokio` full, `reqwest` json, `tauri-plugin-positioner`, etc.).
- [x] Compile and verify dev startup compiles cleanly.

### Phase 2 — ESPN API Integration & Score Fetching
- [x] Define live score data models (`models.rs`): `MatchScore`, `TeamScore`, `MatchStatus`.
- [x] Implement in-memory cache system (`cache.rs`) to store live scores thread-safely.
- [x] Build ESPN Cricinfo parsing engine (`parser.rs`) for live matches, run rates, and target margins.
- [x] Set up background polling task (`fetcher.rs`) using HTTP clients with browser-mimicking headers and TCP Keep-Alive + NoDelay.
- [x] Implement adaptive polling timers (1s for live, 30s for breaks, 5m when idle).
- [x] Wire caching, polling startup loop, and `get_score` command in `lib.rs`.

### Phase 3 — Main Scoreboard Popup UI
- [x] Design layout structure in `index.html` (supporting team scores, live badges, CRR/RRR stats).
- [x] Style card with minimal flat dark-theme CSS in `styles.css`.
- [x] Write script in `main.js` to poll the Tauri backend every 500ms and update HTML components.
- [x] Verify release builds compile and bundle packages correctly.

### Phase 4 — Global Hotkey
- [x] Install and configure `tauri-plugin-global-shortcut` in `Cargo.toml` and `lib.rs`.
- [x] Register `Ctrl + Alt + Space` keyboard hook at startup.
- [x] Bind shortcut event to toggle window visibility (mirroring the tray click toggle behavior).

### Phase 5 — Auto Event Mini-Popup
- [x] Define Event models in Rust (Wickets, Boundaries) and parse them from Cricinfo feeds.
- [x] Add a secondary Tauri window (`mini_popup`) configured to slide out from the screen edge.
- [x] Write CSS and auto-dismiss timer logic (5 seconds) in the frontend for event notifications.
- [x] Conditionally show mini_popup only when main card is hidden; flash main card when visible.

### Phase 6 — Multiple Match Support
- [x] Update `parser.rs` to extract a list of all live Indian international matches.
- [x] Extend system tray context menu in `lib.rs` to display a list of active matches.
- [x] Store chosen match ID and adapt the background polling worker to track the chosen match.

### Phase 7 — Polish, Edge Cases & Testing
- [x] Handle offline and connection drop states gracefully.
- [x] Optimize memory footprint (target < 40MB active RAM).
- [x] Perform cross-verification of cricket matches.
