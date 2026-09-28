<p align="center">
  <img src="native/assets/icon.png" width="128" height="128" alt="SportsPulse Logo" />
</p>

<h1 align="center">SportsPulse — Native Win32</h1>

<p align="center">
  Lightweight, premium desktop scoreboard companion for real-time Cricket and Football (Soccer) match tracking. Pure Rust + Win32 + Direct2D/DirectWrite, single process, no WebView.
</p>

---

## ⚡ Features

- **Dynamic Scoreboard Overlay**: Stacked scorecard for Cricket, horizontal broadcast bar for Football.
- **Match Dashboard**: Browse and track live or upcoming Indian cricket matches, international series, and major football leagues. Native Direct2D window with hit-testing, scroll, maximize/restore.
- **Smart Polling Engine**: Scales intervals by game status (live T20/ODI ~2s, Test ~10s, soccer ~3s, break/scheduled 30s, completed/idle 300s, discovery 60s). See `docs/polling.md` (canonical table from `native/src/engine/fetcher.rs`).
- **OS Integration**:
  - **Global Shortcut**: `Ctrl+Alt+Space` via `RegisterHotKey` to toggle scoreboard.
  - **System Tray**: `Shell_NotifyIconW` with Toggle / Open Dashboard / Untrack / Quit, bottom-right placement via work-area.
  - **Event Popups**: Topmost `WS_EX_NOACTIVATE` mini-popup for wickets, boundaries, goals, red cards, wins (Win 8s, else 5s; in-card flash when overlay visible).

## 🛠️ Tech Stack — Win32 native only

- **Language**: Rust 2021, `windows 0.58` (`Direct2D`, `DirectWrite`, `WIC`, `Dxgi`, `Gdi`, `Shell`, `HiDpi Per-Monitor V2`)
- **Async**: Tokio `rt-multi-thread/time/sync/macros` (2-worker engine thread + `mpsc` → `PostMessageW` bridge)
- **HTTP**: Reqwest `json + rustls-tls`
- **Render**: `D2D → WIC PBGRA → CopyPixels → DIB → UpdateLayeredWindow(ULW_ALPHA)`, Segoe UI Variable, `#1E1E22 94%`, 10-16px rounding
- **API**: ESPN unofficial JSON (`site.web.api.espn.com/.../scoreboard/header`, `.../summary?event=`)

> Tauri v2 + WebView2 stack removed in native cutover (see `docs/benchmark_tauri_baseline.md` for archived baseline). Git history preserves it. No Node/pnpm required.

## 🚀 Getting Started (Windows)

### Prerequisites

- Rust stable (MSVC) + VS 2022 Build Tools + Windows 11 SDK
- No Node.js required

### Build & Run

```powershell
# from repo root
cargo build --manifest-path native/Cargo.toml --bin sportspulse
# release (size-optimized opt-z + lto + strip)
cargo build --manifest-path native/Cargo.toml --bin sportspulse --release
# run (tray icon appears, dashboard opens, overlay hidden until Track)
.\native\target\debug\sportspulse.exe
```

### Tests (engine is cross-platform)

```bash
# lib engine parser/cache tests run on Linux too (UI modules are #[cfg(windows)])
cargo test --manifest-path native/Cargo.toml --lib
```

### Benchmark

```powershell
cargo run --manifest-path native/Cargo.toml --features dev-bins --bin spbench --release
# writes C:\sp_bench\native_measured_report.md today (legacy path, code-owned);
# direction is %LOCALAPPDATA%/SportsPulse/logs (see docs/techdebt.md P0-5).
# see docs/benchmark_report.md for last published comparison
```

### Debug helpers (Windows only, dev-bins feature)

```powershell
cargo run --manifest-path native/Cargo.toml --features dev-bins --bin redbox
cargo run --manifest-path native/Cargo.toml --features dev-bins --bin sptest
```

## ⌨️ Global Shortcuts

- **Toggle Scoreboard**: `Ctrl+Alt+Space`

## 📁 Layout

```
LICENSE                        # MIT
rustfmt.toml                   # edition 2021, max_width 100
.gitignore                     # target/, *.log, secrets, *.exe/*.msi
native/
  assets/icon.png, icon.ico      # preserved from old src-tauri/icons
  Cargo.toml                     # dev-bins feature gates sptest/redbox/spbench
  Cargo.lock                     # tracked (pin drift, see techdebt P2 supply-chain)
  target/                        # build output (git-ignored)
  README.md                      # crate pointer (commands + doc links)
  src/main.rs                    # Win32 pump, tray, hotkey, overlay/dashboard orchestration (uses sportspulse lib)
  src/lib.rs                     # engine (always) + dashboard/popup/render/tray (#[cfg(windows)])
  src/render.rs                  # scoreboard Direct2D renderer
  src/dashboard.rs               # dashboard Direct2D renderer + hit-test
  src/popup.rs                   # event mini-popup
  src/tray.rs                    # Shell_NotifyIconW tray
  src/engine/models.rs, cache.rs, match_state.rs, events.rs, fetcher.rs, parser.rs
  src/bin_benchmark.rs           # spbench (uses sportspulse lib)
  src/bin_test.rs, bin_redbox.rs # ULW isolation probes (dev only)
docs/
  ARCHITECTURE.md                # ESPN → fetcher → cache/match_state → mpsc → PostMessage → render UI
  polling.md                     # canonical polling table (from engine/fetcher.rs) + anti-ban rationale
  prd.md                         # Phase-1 Tauri PRD (archived, football/dashboard added later)
  implementation.md              # Tauri phases 1-7 (archived) — native M0-M8 supersedes
  benchmark_tauri_baseline.md    # archived Tauri baseline (single-sample, WSL2 interop)
  benchmark_report.md            # Tauri vs native comparison (indicative, see techdebt for methodology gaps)
  techdebt.md                    # perfection audit register
```

## ⚠️ Notes

- Windows-only GUI. `native/src/*` UI modules are `#[cfg(windows)]`. Engine tests run anywhere.
- ESPN endpoint is unofficial. UA is browser-mimicking; expect rate-limit risk — see `docs/techdebt.md` P0-4/P1 for timeout/backoff work.
- Logs: release file logging under `C:\sp_bench\` is debug-only legacy — see techdebt P0-11, use `%LOCALAPPDATA%/SportsPulse/logs` next.
