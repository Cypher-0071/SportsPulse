# SportsPulse — Architecture (native Win32)

```
ESPN unofficial JSON (scoreboard/header + summary?event=)
        │  reqwest, browser-mimic headers, 2s–300s cadence (docs/polling.md)
        ▼
engine/fetcher.rs (Tokio 2-worker, start_polling loop)
   ├─► engine/parser.rs ──► ScoreCache (latest score + latest event)
   └─► ActiveMatchesState (discovered list + selection + Notify)
        │  tokio mpsc::UnboundedSender<AppEvent>
        ▼
src/main.rs bridge (ScoreChanged / MatchEvent / MatchesDiscovered)
        │  PostMessageW (WM_APP_SCORE_UPDATE / MATCH_EVENT / MATCHES_DISCOVERED)
        ▼
Win32 UI thread (message pump + WndProcs)
   ├─► render.rs      overlay scoreboard   (540×200 / 96 / 140, D2D→WIC→DIB→ULW)
   ├─► dashboard.rs   match dashboard      (1120×760, cards + hit-test + scroll)
   ├─► popup.rs       event mini-popup     (320×84, WIN 8s / event 5s)
   └─► tray.rs        tray + hotkey        (Shell_NotifyIconW, Ctrl+Alt+Space)
```

Sizes are prod consts (`main.rs:83-86`, `dashboard.rs:43-44`); `spbench`
measures these same sizes. Engine (`engine/*`) is cross-platform and unit
tested (`cargo test --lib`); UI modules are `#[cfg(windows)]`.

Legacy debug paths (`C:\sp_bench\`, `C:\sp_native\`) in dev bins and
debug-only logging are code-owned (UI agent scope — do not touch here);
direction is `%LOCALAPPDATA%/SportsPulse/logs` (see `docs/techdebt.md`
P0-5/P0-11 and `README.md`).

## Accessibility policy (docs only — UI agent owns code)

- Contrast: body text must meet ≥4.5 normal. Approved pair: `dim #A6A6A6`
  on dark passes; do NOT ship `subtle #707070` (`3.29`) or `#6B6F7B`
  (`3.40`) for text, red-card `#D90429` on dark (`3.16–3.79`) for text —
  use `#FF6B74` or dark-on-light pill instead. Verify badge pairs per
  `docs/techdebt.md` P2.
- State is never color-only: batting dot gets `● BATTING` text, flash gets
  icon+text, `LIVE` vs `FINISHED` get distinct treatment.
- Reduced motion: when `SPI_GETCLIENTAREAANIMATION` reports animation off
  (or screen reader detected), replace 33ms spinner + 3s/8s flash with a
  static ring and persistent card.
- Targets: interactive height ≥44px, scrollbar ≥8px; scale via
  `GetDpiForWindow` / `WM_DPICHANGED`. High-contrast: `SPI_GETHIGHCONTRAST`
  / `GetSysColor` theme listener, opaque surfaces + 2px borders when HC.
