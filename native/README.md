# SportsPulse — native crate

Native Win32 scoreboard. Full docs live in the repo-root `README.md`.

```powershell
# from repo root
cargo build --manifest-path native/Cargo.toml --bin sportspulse
cargo build --manifest-path native/Cargo.toml --bin sportspulse --release
cargo test --manifest-path native/Cargo.toml --lib
# dev-only probes (need --features dev-bins, Windows only)
cargo run --manifest-path native/Cargo.toml --features dev-bins --bin spbench
cargo run --manifest-path native/Cargo.toml --features dev-bins --bin sptest
cargo run --manifest-path native/Cargo.toml --features dev-bins --bin redbox
```

Layout: `src/main.rs` (Win32 pump) + `src/lib.rs`
(`engine` always; `dashboard`/`popup`/`render`/`tray` on Windows).
Polling truth: `../docs/polling.md`. Debt register: `../docs/techdebt.md`.
