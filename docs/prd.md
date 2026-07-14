# 🏏 SportsPulse — Product Requirements Document

## Overview

**SportsPulse** is a lightweight, always-on Windows system tray application built in **Rust (Tauri)** that delivers real-time Indian cricket scores with near-zero system resource usage. It is designed for users who want live match awareness without the overhead of a browser or heavy sports app — especially while gaming, coding, or doing any demanding task in parallel.

> **Core Philosophy:** Zero friction. Instant scores. Invisible footprint.

---

## Target User

- Sports enthusiast who always has a match running in the background mentally
- Power user doing demanding tasks (gaming, WSL dev, video editing) who can't afford RAM overhead
- Someone who currently opens Chrome just to check scores and wants a smarter solution

---

## Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust |
| App Framework | Tauri v2 |
| Async Runtime | Tokio |
| HTTP Client | Reqwest (with TCP Keep-Alive + NoDelay) |
| Score Source | ESPNcricinfo Internal JSON API (unofficial) |
| UI | HTML + CSS + Vanilla JS (via Tauri WebView2) |
| Notifications | Custom Tauri mini-popup (no Windows toast) |
| Theme | Dark (fixed) |

---

## Match Coverage

- **Scope:** All Indian international cricket matches
  - Test Matches
  - One Day Internationals (ODIs)
  - T20 Internationals
  - ICC Tournaments (World Cups, Champions Trophy, Asia Cup)
- **Excluded (Phase 1):** IPL, domestic cricket, non-Indian matches

---

## Core Features

### 1. System Tray Presence
- App lives silently in the Windows system tray
- Tray icon: **Clean cricket ball icon** (minimal, no text)
- Right-click tray menu:
  - Select active match (if multiple live)
  - Quit app

### 2. Main Scoreboard Popup
**Trigger methods (all three work):**
- Left-click on tray icon
- Keyboard shortcut: `Ctrl + Alt + Space`
- Auto-appears on key match events

**Appearance:**
- Bottom-right corner of screen (near tray)
- Dark theme, Google-style card layout
- Compact-medium size — all critical stats visible at a glance

**Content displayed:**
```

+------------------------------------------+
|  INDIA          245 / 3  (42.3 ov)       |
|  AUSTRALIA        0 / 0   (0.0 ov)       |
+------------------------------------------+
|  CRR: 5.76    RRR: 8.12    Need: 87      |
+------------------------------------------+

```

**Stats shown:**
- Both teams name + flag emoji
- Current batting team: Score / Wickets (Overs)
- Bowling team: Target or 0/0 if 1st innings
- CRR (Current Run Rate)
- RRR (Required Run Rate) — only 2nd innings
- Runs needed — only 2nd innings

### 3. Auto Event Mini-Popup
A **smaller, non-intrusive card** that auto-appears bottom-right for:
- **Wicket falls** — shows: who got out, how, score at fall
- **Boundary** (4 or 6) — shows: who hit it, current score
- **Over completion** — shows: over summary (runs, wickets in over)

**Behaviour:**
- Auto-dismisses after **5 seconds**
- Has a small X button to manually close instantly
- Does NOT stack — if a new event fires, replaces the old popup

### 4. No Live Match State
When no Indian international match is live:
- Tray icon remains (cricket ball)
- Clicking tray / pressing shortcut shows a simple card:
  ```
  No live match right now
  ```

### 5. Multiple Match Handling
- If multiple Indian matches are live simultaneously (rare):
  - Right-click tray → "Select Match" submenu
  - User picks which match to track
  - App shows only the selected match's scorecard

---

## Data Fetching Architecture

```
+--------------------------------------------------+
|              Rust Background Worker              |
|                                                  |
|  reqwest (TCP Keep-Alive + NoDelay)              |
|       down every 0.5s (live match)               |
|  ESPNcricinfo Internal JSON API                  |
|       down                                       |
|  Parse JSON -> Struct -> Write to in-memory cache|
+--------------------------------------------------+
                        down
+--------------------------------------------------+
|              Tauri UI Layer (WebView2)           |
|                                                  |
|  Reads from in-memory cache (instant)            |
|  Renders scoreboard popup                        |
|  Zero network wait on UI                         |
+--------------------------------------------------+
```

**Polling intervals:**

| State | Interval |
|---|---|
| Live match active | Every 0.5 seconds |
| Match in break (drinks, lunch, etc.) | Every 30 seconds |
| No live match | Every 5 minutes (check for match start) |

**Anti-block measures:**
- `User-Agent` header spoofed as Chrome browser
- `Accept`, `Referer` headers set to mimic real browser request
- TCP Keep-Alive to avoid repeated handshakes
- Respectful 0.5s minimum interval (never faster)

---

## Performance Targets

| Metric | Target |
|---|---|
| Idle RAM usage | < 25 MB |
| Active polling RAM | < 40 MB |
| CPU usage (idle polling) | < 0.1% |
| Score update latency | 1–5 seconds from real event |
| Popup open time | < 50ms (reads from cache, no network) |
| App startup time | < 500ms |

---

## Non-Goals (Phase 1)

- IPL / domestic cricket
- Football scores
- Historical stats or scorecards
- User accounts or login
- Windows toast notifications (custom popups instead)
- Mobile version
- PowerToys Command Palette plugin (Phase 2)
- Settings/config UI (hardcoded in Phase 1)

---

## Future Phases (Out of Scope Now)

- **Phase 2:** Football scores (EPL, UCL, La Liga) — Indian team matches + popular leagues
- **Phase 3:** PowerToys Command Palette plugin that talks to the running tray app
- **Phase 4:** IPL support + player-specific alerts ("Alert me when Kohli bats")
- **Phase 5:** Settings UI (customize shortcut, popup position, sports to track)
