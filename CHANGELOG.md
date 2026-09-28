# Changelog

## Unreleased — native Win32 cutover

- Replaced the Tauri v2 + WebView2 stack with a pure native Win32 rewrite
  (`windows-rs` + Direct2D/DirectWrite, single process, no WebView).
  See `docs/benchmark_report.md` (indicative, single-sample numbers) and
  `docs/benchmark_tauri_baseline.md` (archived baseline) for comparison.
- Polling is canonical in `docs/polling.md`, sourced from `start_polling`
  in `native/src/engine/fetcher.rs`.
- Legacy debug paths (`C:\sp_bench\`, `C:\sp_native\`) are debug-only and
  code-owned; direction is `%LOCALAPPDATA%/SportsPulse/logs`.
- Supply chain: `native/Cargo.lock` is tracked (SBOM source of truth for
  exact dependency versions); `native/deny.toml` holds the basic `cargo-deny`
  policy (check with `cargo deny --manifest-path native/Cargo.toml check`
  from the repo root); Dependabot scans `native/` weekly. Run `cargo audit` locally
  before release (CI runs it best-effort when installed).
- Tags/releases: no git tags are created by automation. The maintainer cuts
  tags manually (e.g. `git tag -a v0.1.0 -m "native Win32 cutover"`).
