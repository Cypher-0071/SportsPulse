# SportsPulse — Tauri Baseline Benchmark

> Reference measurements of the current Tauri v2 implementation, taken on the
> `native-win32` branch point (base commit `b7900ef`) before the native
> `windows`-crate rewrite. All numbers below are single-sample, indicative
> measurements intended for head-to-head comparison, not statistical claims.

---

## Environment

| Component | Value |
|---|---|
| Host | Windows (driven from WSL2 Ubuntu via interop) |
| Build toolchain | VS 2022 Community 17.14.38, MSVC 14.44.35207, `stable-x86_64-pc-windows-msvc` (rustc 1.97.0) |
| Build time | `pnpm tauri build` release: 4m 49s (clean) |
| Source state | `main` @ `b7900ef` ("fix: updated the api calling") |

## Artifacts

| Artifact | Bytes |
|---|---|
| `sportspulse.exe` (release) | 13,426,176 |
| `SportsPulse_0.1.0_x64_en-US.msi` | 4,747,264 |
| `SportsPulse_0.1.0_x64-setup.exe` (NSIS) | 3,256,854 |

## Methodology

- **RAM attribution**: every process whose command line references Tauri's
  WebView2 user-data folder marker `com.sportspulse.app`, plus the root
  `sportspulse.exe`. WebView2 processes spawn asynchronously and are not
  always direct PPID children of the app, so parent-tree walking alone
  undercounts; command-line matching is authoritative here.
- **Metrics per process**: `PrivateMemorySize64` (private commit) and
  `WorkingSet64`, summed across the tree. Private commit is the headline
  number (shared pages excluded; physical-RAM attribution is fuzzy due to
  system-wide WebView2 sharing).
- **States**:
  - *Idle*: app launched, dashboard window open (per `tauri.conf.json`),
    scoreboard hidden, polling engine in discovery mode.
  - *Shown*: scoreboard toggled visible via the real global hotkey
    (`Ctrl+Alt+Space` sent through `SendKeys`), 4s settle.
- **Startup**: `Start-Process` → first top-level HWND belonging to the root
  PID (EnumWindows poll, 150ms cadence). Warm-ish: WebView2 user-data dir
  already cached from a previous run.
- **Verification**: bottom-right screen capture confirmed the dashboard
  rendering (match list + TRACK buttons) at measurement time.

## Results

| State | Private commit | Working set | Processes |
|---|---|---|---|
| Launch → first window | — | — | — |
| Idle (dashboard open, ~14s settle) | 284,045,312 B (~271 MB) | 574,021,632 B (~547 MB) | 9 |
| Idle (2nd sample, pre-fix attribution) | 8,450,048 B (root only — webviews not yet spawned) | 35,569,664 B | 1 |
| **Scoreboard shown** | **413,970,432 B (~395 MB)** | **832,679,936 B (~794 MB)** | 12 |
| First window latency (warm) | 442 ms | | |

### Largest single processes (shown state)

| Process | Private commit |
|---|---|
| msedgewebview2 (renderer) | 132,947,968 B (~127 MB) |
| msedgewebview2 (browser) | 67,026,944 B (~64 MB) |
| sportspulse.exe (host) | 9,109,504 B (~8.7 MB) |

## Observations

1. The Rust host process is tiny (~9 MB); **~98% of footprint is WebView2**.
2. Each visible window pulls additional renderer processes (9 → 12 procs when
   the scoreboard appears).
3. Working-set numbers are inflated by shared WebView2 pages; private commit
   is the fair cross-implementation metric.

## Reproduction

Scripts: `C:\sp_bench\measure.ps1` (`LAUNCH|SHOWN|DASH|KILL`), driven from WSL
via PowerShell interop. Build scratch dir: `C:\sp_baseline` (git-untracked).
