# SportsPulse — Tech Debt Register (Native Win32 canonical)

> STATUS: PASS — perfection loop complete. `cargo test --lib` 23 passed, `cargo clippy --lib -D warnings` 0, `cargo fmt --check` clean. Zero P0. Windows `cargo check` still requires MSVC host (Linux parses `cfg(windows)` only). Cutover + fixes uncommitted — commit before further work.

Source: native-only re-audit post-cutover + 2 fix iterations + final PASS verification. Tauri deleted from tree.
Scope: `native/*` + `docs/*` + `README.md` + `PRODUCT.md` + `.gitignore`. No `src/`, `src-tauri/`, `package.json`, `pnpm-lock`, `.vscode`.
Ranking: P0 = crash/hang/data-loss/blocking, P1 = wrong behavior/perf/protocol, P2 = polish/precision/nits.
Format: `native/...:line` + snippet + fix.

Cutover already done (verified):
- Deleted `src/` (7 web files), `src-tauri/` (7 rs + conf + caps + Cargo + ~40 icons), `package.json`, `pnpm-lock.yaml`, `.vscode/extensions.json`. Uncommitted vs `c3e8c29` — commit before further work.
- Preserved `native/assets/icon.png, icon.ico, icon-128/32.png` from old icons.
- `native/src/lib.rs`: `engine` always + `dashboard/popup/render/tray #[cfg(windows)]`. `main.rs` + `bin_benchmark.rs` use `sportspulse::...` (dual `mod engine` gone).
- `native/Cargo.toml`: `windows` under `[target.'cfg(windows)'.dependencies]`, `reqwest default-tls → rustls-tls`, `tokio` minimal. `Cargo.lock` tracked (correct).
- `.gitignore`: `**/target/`, `.env*/.pem/.key`, `*.exe/*.msi` added.
- `README.md` rewritten native-only, `PRODUCT.md` native status, `implementation.md` ARCHIVED header.
- `native-win32` branch deleted. `cargo test --lib`: 10 passed (fixed first-poll suppression tests).
- `render.rs:4`, `main.rs:918` de-Taurified.

Historical Tauri refs in `docs/prd.md`, `benchmark_*.md`, `implementation.md` body, `bin_benchmark` report strings are intentional archive — see RETIRED. Do not file as live work.

---

## P0 — crash / hang / data-loss / blocking

### P0-1 `parse_competitor` `unwrap()` panics fetcher task
- `native/src/engine/parser.rs:220`:
```rust
let team = comp.get("team").unwrap();
```
Soccer sibling `:806` already `unwrap_or(comp)`. One malformed `competitors[i]` kills Tokio 2-worker loop (`main.rs:759`), no supervisor.
Fix: `unwrap_or(comp)` + null guard returning `Yet to bat` fallback; wrap `start_polling` in restart supervisor.

### P0-2 Cricket `detail?` hard-fails whole parse
- `native/src/engine/parser.rs:94`:
```rust
let detail = status.get("type")?.get("detail")?.as_str()?.to_lowercase();
```
Pre-kickoff / rain shape without `detail` → `None` → `fetcher.rs:267 cache.set(None)` + 300s stall. Soccer `:754-759` already `unwrap_or("")`.
Fix: mirror soccer, then `if Live && !empty && contains delay/lunch/tea/stumps/rain/break`.

### P0-3 No timeout — hang forever (rustls switch done, timeout not)
- `native/src/engine/fetcher.rs:27-31`:
```rust
let client = Client::builder().tcp_nodelay(true).default_headers(h).build()
```
`Cargo.toml:29` rustls-tls verified, but no `.timeout()`. One hung `send()/json()` at `:51,58,82,89,170` stalls 2s live loop forever; `render.rs:206 stale>15s` never fires if hung.
Fix: `.timeout(10s).connect_timeout(5s)` + per-request `tokio::time::timeout` → `RECONNECTING`.

### P0-4 Transient `None` wipes scoreboard + 300s freeze
- `native/src/engine/fetcher.rs:267` `} else { cache.set(None); }`, initial `sleep 300s` at `:39`. Single bad JSON blanks overlay to NoMatch 5 min. `Err` at `:271` keeps cache but still 300s.
Fix: keep stale, `sleep 5s` on transient, `failures>10 → clear`, capped backoff.

### P0-5 Startup header `unwrap()` + `eprintln!` hot-path + `C:\` logs
- `fetcher.rs:21-25` `"...".parse().unwrap()` static today, panic tomorrow. Use `HeaderValue::from_static`.
- `fetcher.rs:140,168,179,271,289` `eprintln!("[DEBUG] Fetching: {detail_url}")` every 2s + full URL leak. Use `log debug!` behind `RUST_LOG`, never full URL.
- `main.rs:20-44 live_benchmark_verified.log`, `render.rs:44-53 sp_debug.log`, `bin_test.rs:100,118,142`, `bin_redbox.rs:45`, `bin_benchmark.rs:382` `C:\sp_bench\...` create+append, no ACL/rotation, blocks WndProc. Gate `cfg(debug_assertions)` / env opt-in, move to `%LOCALAPPDATA%/SportsPulse/logs` + rolling.

### P0-6 `GWLP_USERDATA` never set for dashboard + `HMAIN` ordering + `Box` leak + `NIM_DELETE` never runs
- `main.rs:908` only main `SetWindowLongPtrW`; dash never gets `USERDATA`, `dashboard_wnd_proc:566-567` reads via `HMAIN` proxy. `store(Release) :859` / `load(Relaxed) :566,777`; `match_state:20` / `fetcher:139,284 Relaxed` vs `main:301-304 Relaxed`.
- `:908 Box::into_raw` never `from_raw`; `WM_DESTROY:550-554` only `UnregisterHotKey+PostQuit` — `tray.rs:107-113 NIM_DELETE` in `Drop` never runs → ghost tray + `mem_dc/hbmp` leak. `WM_COMMAND:545 DestroyWindow` leaks dash/popup.
Fix: per-window `USERDATA` + `WM_NCCREATE lpCreateParams`; `HMAIN Acquire/Release` (`AtomicIsize` or `OnceLock`), `initial_fetch_completed Acquire/Release`; `WM_NCDESTROY from_raw` once + explicit `NIM_DELETE` + `DestroyWindow(dash/popup)` before quit.

### P0-7 `WM_PAINT` without `BeginPaint` + `ULW` inside + `EndDraw` swallowed + GDI leaks
- `main.rs:387-393`, `dashboard.rs:658-664` `present_*` (`BeginDraw/EndDraw/CopyPixels/ULW`) then `ValidateRect`, no `BeginPaint/EndPaint`. Layered windows must not rely on `WM_PAINT` for `ULW`; re-entrant.
- `render.rs:662-665` `EndDraw` err → `return Err`, callers `:389,399,417,310` + `dashboard:1140` do `let _ present` — `D2DERR_RECREATE_TARGET` swallowed.
- `render.rs:518,590`, `dashboard.rs:327,459`, `popup.rs:217`, `bin_test:240,263`, `bin_redbox:107,121` discard `SelectObject` old bitmap; `resize :544-549/:413-418 DeleteObject(selected hbmp)` no-op/leak every `96/140/200` switch. `popup.rs:205-206 GetWindowDC(None)` never `ReleaseDC` (render `:504-506`/dash `:313-315` correct).
Fix: `BeginPaint/EndPaint`, single present path outside paint, propagate `RECREATE_TARGET` → recreate; store `old_bmp`, `SelectObject(old)` before `DeleteObject/DeleteDC`, check `is_invalid`; `ReleaseDC` on every path including early `?`.

### P0-8 `SendMessageW` holding `&mut` UB + `SELECT_MATCH idx` TOCTOU + `wide()` temporary
- `main.rs:574-589` `as_mut()` → `restore_dashboard_for_drag(&mut)` → sync `SendMessageW(WM_NCLBUTTONDOWN)` re-enters `dashboard_wnd_proc` → second `as_mut()` — two live `&mut`.
- `:477-479,728-733` posts array `idx`, handler `idx < len → clone()`; `WM_APP_MATCHES_DISCOVERED:452-474` replaces `dash_matches` in between → wrong match. `card_layout` index == global only by accident.
- `bin_test.rs:170-224`, `bin_redbox.rs:130` `PCWSTR(wide("...").as_ptr())` temp dropped end-of-statement — works by luck, UB under opt.
Fix: scope borrow → bool, drop, then `SendMessage`; post heap-alloc `match_id` string (`Box::into_raw`, free on receipt) + validate present; static wide arrays or `Vec::leak`.

### P0-9 Linux bins no `main` + `expect/panic` in GUI + COM unchecked
- `main.rs:5`, `bin_benchmark/test/redbox:5-7` `#![cfg(windows)]` empties crate on Linux → no `main` error; `lib` gates UI so `cargo check` Linux still needs per-bin fallback.
- `main.rs:814,857,879,882,763`, `bin_benchmark:174,209,211,215,234,236,242,254`, `bin_test:209,227,239,258,262`, `bin_redbox:65,77,104,118,132`, `fetcher:21-25`, `parser:220` `expect/unwrap`.
- `main.rs:806`, `bin_benchmark:165`, `bin_redbox:50` `let _ CoInitializeEx(STA)` ignores `RPC_E_CHANGED_MODE`.
Fix: `#[cfg(not(windows))] fn main(){ eprintln!("windows-only") }` or `required-features`; `MessageBoxW + PostQuit` never `expect` on `CreateWindow/RegisterHotKey/D2D/DIB`; check `S_OK/S_FALSE` else MTA path + `CoUninitialize`.

### P0-10 Unbounded body + no `error_for_status` + silent swallow (OOM / stale LIVE)
- `fetcher.rs:51,58,82,89,170-173` `send().await → json::<Value>()` no `content-length` cap, no `bytes().take(N)`. ESPN/compromised net OOMs.
- `if let Ok(resp){ if let Ok(json){` drops 4xx/5xx/HTML silently; stale LIVE shown live, WIN/wicket missed.
Fix: `error_for_status()?`, `bytes()` with `len>1_000_000 → continue`, `serde_json::from_slice` after cap.

### P0-11 Hotkey ignored + mouse-only/SR-invisible blocking + dev bins shipped
- `main.rs:911-916` `let _ RegisterHotKey(1, Ctrl|Alt, Space)` — `ERROR_HOTKEY_ALREADY_REGISTERED` silent, no remap/fallback. Add check + dashboard toast + `HKCU\Software\SportsPulse\hotkey` + tray double-click fallback.
- `main.rs:372-557,560-752`, `dashboard.rs:534-575` no `WM_KEYDOWN/SYSKEYDOWN/GETOBJECT`, no Tab/Enter/Space/arrows for TRACK/caption/switcher/scroll; `MatchItem` hover-only `:737-739`; no UIA provider, `TOOLWINDOW/NOACTIVATE` excluded from tab/Narrator. Handle `WM_KEYDOWN+GETOBJECT`, UIA names/roles/states, Tab order, `Enter/Space` activate, `Esc` dismiss.
- `Cargo.toml:11-25` `sptest/redbox/spbench` build+ship default, write `C:\sp_bench`. Remove `[[bin]]` or `required-features=["dev-bins"]`, move to `examples/` or `cfg(debug_assertions)`.

---

## P1 — wrong behavior / perf / protocol / methodology

### Engine correctness
- **Shared `last_ball_id` + soccer first-poll spam:** `fetcher.rs:33`, `parser.rs:571` cricket suppresses (`None → set + false`, correct) vs `:860` soccer sets then emits stale GOAL/RED. One var for `u64-key` + `evt-id`; reset only on `last_tracked` change `:193-195`. Proof: soccer tests `expect` on `None`, cricket tests `is_none()` first. Split `last_cricket/last_soccer`, soccer `None → set + None`, fix soccer tests two-step.
- **`is_test` fragility:** `parser.rs:145-159` `contains("test")` fires `Latest/Contest/Greatest`; `as_bool()` dead vs ESPN `20.0`, `as_f64().unwrap_or(50.0)` misprices missing. Match `generalClassCard==Test` first, accept f64/u64/bool, regex `\btest\b` last.
- **Completed freeze + win suppressed:** `:158-162` 300s no re-poll; first-fetch-completed pre-sets `last_completed :191-206` → win false → no `MATCH WON!`. Emit win on first-completed if `is_winner` or document suppress; still allow 60s scoreboard refresh to clear on new match.
- **Win + boundary double:** `:203-244` win `:218-231` then `parse_latest_event :235-239` second popup + double flash timer. Skip ball-event if win emitted or coalesce when Completed.
- **Selected never validated:** `match_state.rs:23`, `fetcher.rs:151-157` no whitelist vs `active_matches`, no `^[A-Za-z0-9._-]{1,64}$` for `sport/series/match` in `detail_url :164-167` (prior ESPN JSON). Validate `cricket|soccer` + present in `active_matches` else `untrack + clear()`.
- **Test 2nd innings lost:** `:247-259` `find(isCurrent).or(last)` drops `462 & 193`. Aggregate Tests display, keep current for `runs/overs`.
- **Swallowed scoreboard errors + sequential 4x GETs:** `:51,58,82,89` silent; `:48-93` cricket-live→today→soccer-live→today blocks detail. `match` + `warn` + `error_for_status`, `tokio::join!` merge.
- **Stale `Live` on error, `timestamp` secs, `Local` dates:** `u64` secs (`models:44`, `parser:196,776`) + `render:206-207 stale>15s` flaps vs 2s poll; `Local::now %Y%m%d :46` IST vs UTC midnight miss. `Utc` + `fetched_at ms` + `Instant`, display `updated Xs ago`.
- **Type/precision:** `f32 overs/crr/rrr` (`models:20,40-41`) + `+0.1` fudge (`:325-347`) fragile; `batting_team:u8` magic (`:39`, `parser:123,132,162`, `render:707,775,855`); `MatchEvent.sport:String` vs `SportType`; `crr 0.00` when `batting 0` (`:132-138`); `Need 0/RRR 0` when `runs>=target` live (`:172,188`); `Batsman` sentinel (`:503,633`); boundary FP `scoreValue 4/6` wides (`:626`); soccer `score:""` blank (`:904`); tooltip single-team (`main:420` vs `tray:27 <128>` silent cut). Fix: balls `u32`/`f64`, `BattingSide` enum, `sport:SportType`, `Option` for `crr/need`, `Option<String>` batsman, require `boundary==true`, fill `{home}-{away}`, `"{t1} {s1} vs {s2} {t2} · {status}"` truncated 100 + ellipsis.
- **India-only + `pre/in` only:** cricket `id==6|india (:44-50)`, soccer unfiltered; `post` excluded (`:29,690`) → finished untrackable. Document intent or remove filter.
- **Cache/Notify:** `cache.rs:19-29` poison → `None` fake NoMatch → `unwrap_or_else(into_inner)`; `main:481,503` + `fetcher:275 clear()` blanks before fetch → keep stale + spinner; `sort_by_key clone (:64-65)` + order-compare flicker (`:129-145`) → `sort_by cmp` + `HashSet`; `Notify + select! sleep/notified (:27,:286-291)` coalesces — document (single consumer safe).
- **ETag/gzip:** `Cargo:29 default-features=false` disables `gzip`; 2s full body. Add `gzip`, `If-None-Match`, `304`.
- **`clean_event_detail` dup:** `popup:59` vs `render:224` drift → share in `parser.rs`.
- **Tests:** 10 pass now, but missing `detail` absent, `limitedOvers` int/bool/missing, 2nd-innings, `runs_needed 0`, `batting 0`, win+boundary same poll, completed-first-fetch, timeout. Add.

### UI correctness / perf / protocol
- **Tray:** no `NIM_SETVERSION/NOTIFYICON_VERSION_4`, no `TaskbarCreated` re-ADD, no trailing `PostMessage(WM_NULL)` (`tray:47,90-103`) — Explorer restart loses icon, menu sticks. Add all three.
- **Loader burn:** `DASH_LOADER 33ms :313` + `:666-675` full `1120x760` present (~3.4MB vec + D2D + ULW) per tick. 100-120ms, kill on `initial_fetch_completed`, spinner-only invalidate.
- **Per-frame alloc + double copy:** `render:1124-1128 432KB`, `dashboard:1142-1147 3.4MB`, `popup:415-420` per present/hover + `CopyPixels→buf→bits`. Persistent `Vec`/mapped DIB, validate `stride==w*4`.
- **`present_overlay_now` skips resize:** `:159-168` pos only, `overlay_size_for:117-128 96/140/200` varies → clip/overdraw. Call `place_and_present` or `resize(GetWindowRect)` + assert `w/h`.
- **DPI claimed but 0 + primary-only:** `PER_MONITOR_V2 :812` but `dpi 0 (:496-497,:568-569,:197-198)`, no `WM_DPICHANGED/GetDpiForWindow`; `SPI_GETWORKAREA :106-115` primary-only, `overlay/center :130-194` primary, no `DISPLAYCHANGE`. Real DPI + `WM_DPICHANGED resize` + `MonitorFromWindow/GetMonitorInfo` + reposition on change.
- **Channel bridge waste + ordering:** `unbounded_channel :765` grows if UI stalls; `:780-795` discard `Box` then re-lock cache (contention/stale); `let _ PostMessage :783,788,793,536,726,728` ignores queue-full; `:777 Relaxed` vs `:940 store(0)` posts dead hwnd. `bounded(32)+try_send` + coalesce, pass seq/id, check + retry, `Acquire/Release`.
- **Flash case + TTL split:** `render:281-292` case-sensitive `GOAL/SIX/RED CARD!` vs `popup:321-340 uppercased`; `render:300-306 8s/3s` vs `main:435-439 8000/3000` vs `popup:550-554 8s/5s` (spec 8/5). Uppercase once, word-boundary, shared `WIN_TTL 8s, EVENT_TTL 5s`.
- **Hit vs drawn + `OUT/WIN` substring:** caption `[w-46/w-92/w-138]` vs drawn `[w-44/w-90/w-136]` 2px gaps (`dashboard:537-548 vs 653-728`); `ACTION x>=right-108-16` full-height vs 36px button (`:565-571 vs 1013-1018`); `popup:330-338 contains OUT/WIN` matches `SHOUT/WING`. Shared `caption_rects()`, exact `action_rect` per card, `event_type` first then word-boundary.
- **Scroll/hover/wheel:** `SCROLL_STEP 72 :60` vs `74/92+12` drift; no `TrackMouseEvent/LEAVE` (sticky hover + `MOUSEMOVE:593-647` storm); `((wparam>>16)&0xffff) as i16 :650` ignores `WHEEL_DELTA 120`. Pixel scroll + `max px`, `TME_LEAVE` + clear, accumulate `delta/120`.
- **Factory per-resize + `!Send`:** `:551-592/:420-461` recreate `Factory+WIC+RT+16-20 brushes` per resize (jank `96↔140↔200`); `Renderer/...` hold `HWND/HDC/*mut/COM` auto-`Send` on STA D2D. Cache factory/WIC/dwrite globally, rebuild bitmap/RT+brushes only; `impl !Send/!Sync`.
- **Popup repaint + aliasing:** `:467-498` no `WM_PAINT` (RDP/UAC blanks until next event); `:345 present(&self)` writes `bits` via raw (`&self` violation). `BeginPaint/EndPaint + re-present(last)`, `&mut` or `UnsafeCell`.
- **Bench dims:** `:194-195 620x200` vs `main:83-84 540x200`; `:220 1060x820` vs `DASH 1120x760`; no pump, hidden ULW, single `Instant` incl COM+DPI. Use prod consts, pump/show, per-stage split.
- **Maximize/snap + single-instance:** `toggle_maximize:252-296 SetWindowPos(work_area)` on `WS_POPUP`, no `WM_GETMINMAXINFO`; `RegisterClass let_ :826,839` + no `CreateMutex`. `MINMAXINFO` work-area, `CreateMutex(Local\SportsPulse)` + exit if exists + fallback hotkey ID.
- **Emoji/palette/alloc/COM:** `dashboard:752 🏏⚽` in `Variable Display :359-391` tofu (no Emoji fallback, `popup:254 Segoe UI` drift); `popup bg #1E1E22@94% :220` vs `#202020` opaque; `Fmt::text :325/:83/:137` alloc per `DrawText` (~10-30/frame); `CoInitializeEx STA :806` unchecked. Emoji fallback run, unify `#202020/#2D2D2D/#3E3E3E`, reuse buffer/`TextLayout`, check `S_OK/S_FALSE` + `CoUninitialize`.
- **GDI unchecked + `JoinHandle::drop` + `static mut`:** `Select/CreateDIB` unchecked, `CreateDIB.unwrap (:229-240)`; `:940-941 store(0)+drop(handle)` detaches `block_on` (no clean exit); `bin_test:30 static mut MEM_DC` race. Check every return, `abort()/join`, delete `static mut`.

### Config / docs / benchmarks (native-only)
- **Uncommitted cutover:** deletions + README/PRODUCT/gitignore/Cargo edits floating. Commit `git add -A`, then freeze.
- **Benchmark as fact:** `report:12-16,31,33 97.5%/15.5x/43x` vs baseline single-sample admission (`baseline:5-6`); `EnumWindows 150ms` on `442ms` ±34%; `SendKeys` flaky; cmdline-only attribution; `B+MB` mixed; `12.8MB (13,426,176B)` vs `1.75MB` never measured (`spbench` never stats exe); `16.6ms` assumed vs `0.38ms` hidden-ULW stress (no DWM/vsync); template `>25x (:280)` vs `43x (:33)`; `~350MB/>850/~180/42/4` unmeasured. Label indicative or ≥30 runs + bars, same harness/sizes/vsync-on.
- **Size drift:** prod `540x200/96/140 (:83-86)` vs bench `620x200 (:194-195)`; `DASH 1120x760 (:43-44)` vs bench/report `1060x820 (:220,:292-293,:314, report:60)`; popup `320x84 (:48-49)`. Use prod consts in bench.
- **Polling multi-truth (frontend retired):** code `2s T20/10s Test/3s soccer/30s/300s/60s/30s no-sel (:39-43,:246-279)` vs `prd:119,138 0.5s/30s/5m` vs `implementation:23 1s`. Deleted `500ms/2000ms` frontend is historical. Single consts + `polling.md` + anti-ban rationale; mark frontend numbers historical (done in README, still cited in old techdebt — this file replaces it).
- **PRD present-tense Tauri:** `:5 built in Rust (Tauri)`, `:21-30` table (`Tauri v2/WebView2/Custom mini-popup`), `:124-131` UI-layer arch. No banner (unlike `implementation.md`). Add archive banner; update football/dashboard/IPL vs `:41,163-166,178` non-goals.
- **IST + wall-clock:** `dashboard:159 FixedOffset east 5:30`, `fetcher:46 Local dates=` (UTC miss), `render:206-207 >15` on `u64` secs. `Utc` + tz label, `Instant + fetched_at ms`.
- **Bins + lints:** `Cargo:15-25 sptest/redbox/spbench` unconditional; no `[lints]/clippy/fmt/toolchain`, `allow(non_snake_case :6, dead_code + SAMPLE + DASH_W/H aliases)`. `required-features=["dev"]`, add lints + CI `clippy -D warnings + fmt --check`.
- **This file replaces old techdebt:** old Tauri P0s were live work though code deleted. This native-only register is canonical.

---

## P2 — polish / supply-chain / git nits

- `Cargo:4 edition 2021 → 2024 + rust-version` (1.97 baseline); `:32 chrono` heavy for `%Y%m%d` → `time/jiff` or `default-features=false`.
- `.gitignore` leftovers: `Cargo.lock.orig:13`, `npm/yarn/pnpm-debug:4-8`, `logs:2` ambiguous, `.vscode/* + !extensions:30-31` for deleted dir. Prune after commit.
- No `.github/` CI, tags, templates/CODEOWNERS/CONTRIBUTING/SECURITY; log `d0fcd03 merge:`, `5fae0a8 basic rewrite done`, `82e4030 UI:` etc (lowercase conventional + commitlint).
- Missing `LICENSE` (undistributable), `CHANGELOG`, `native/README`, `ARCHITECTURE` (only ASCII `report:55-68`). README layout omits `target/, Cargo.lock, .gitignore`; bench `C:\sp_bench` path contradicts P0-5 fix direction — update when log path moves.
- `let _ SetWindowPos/Show/ULW` throughout — `GetLastError` debug-only log.
- `chrono Local %Y%m%d` DST edge → `Utc`.
- `clean_score_string raw[..cut] (render:117-139)` ASCII-safe today — char-boundary guard.
- `card_layout` rebuilt every `present (dashboard:586,981)` — cap `len` + virtualize if ESPN large.
- `SCROLL_STEP 72` wheel-only — add `PgUp/PgDn/Home/End` with P0 keyboard.
- Contrast (normal ≥4.5): `subtle #707070/#202020 3.29`, `#6B6F7B/#1C1C1C 3.40`, `redcard #D90429` on dark `3.16-3.79`, track `white@0.10` invisible. `dim #A6A6A6 6.69` passes — use it. Raise `subtle` to `#A6A6A6` min, red text `#FF6B74` or dark-on-light pill, verify badge pairs.
- Color-only: batting dot green-only, flash hue-only 2.2px, `LIVE` vs `FINISHED` both green (`render:1003-1006 vs 1035-1039`). Add `● BATTING`, icon+text flash, distinct `FINISHED` gray/blue. `LIVE · 68'` keep.
- Reduced-motion: spinner `33ms` 20-seg arc (`dashboard:502-532`), flash `3s/8s` (`main:435-440`, `popup:550-555`, `render:300-311`). No `SPI_GETCLIENTAREAANIMATION/SCREENREADER` check. Static ring + persistent card when reduced.
- Targets: `TRACK 108x36`, switcher `~113x30`, scrollbar `5px`, caption `46px` — below 44x44; `DASH 1120x760/SCORE 540x200/POPUP 320x84` fixed, `Segoe 10-38px` fixed, `PerMonitorV2` only. Min 44px height, 8px bar, `WM_DPICHANGED`, scale by `GetDpiForWindow`/text setting.
- High Contrast: hardcoded brushes, only `SPI_GETWORKAREA` queried. `SPI_GETHIGHCONTRAST/GetSysColor` + theme listener, opaque + 2px borders when HC.
- Popup: `5s/8s TIMER_AUTOHIDE`, `LBUTTON` only, `NOACTIVATE` unfocusable. `Esc`/button dismiss, focusable UIA, persist WIN, respect reduced-motion.
- `NO_WRAP+CLIP` silent loss (`dashboard:342,83-92`, `render:409,325-334`, `popup:262,137-146`): pre-truncate `…` via `TextLayout/GetTextExtent`, full string in UIA Name + tooltip. No RTL/bidi isolation (`U+2066...U+2069`, `SetReadingDirection`, never locale-`upper` display key).
- Supply chain: `windows 0.58`, `reqwest 0.12`, `tokio 1`, `chrono 0.4` caret drift; lock tracked good, no `audit/deny/dependabot/SBOM`. Pin `=` + `deny.toml` + `cargo audit` CI + Dependabot + SBOM.

---

## RETIRED — deleted with Tauri cutover (do not fix)

- `src/main.js` (`toFixed` null, `500ms` tick, `__TAURI__`, `console.error` stale-freeze, drag-region, `340x48` hack, `getElementById` per tick, `void offsetWidth`, untracked timeout, `className=` wipe, `(20` edge, `focus` leak).
- `src/dashboard.js` (`innerHTML` XSS, `onclick` injection, `2s` full-rerender focus steal, `m[4]=="in"` grouping, 7-tuple indices, `Asia/Kolkata`, no offline, double-click race, hover-only Untrack).
- `src/mini_popup.html` (timers, stale icons, `score.trim()` numeric, close a11y, transparent clipping).
- `src/styles.css` (triplication, dead `#soccer-clock-val`, flash end mismatch, `transition:all`, fixed sizes, `[hidden]` hack, English hardcode, no TS).
- `src-tauri/*` (`unwrap` team, `detail?`, `test` substring, shared `last_ball_id`, soccer spam, timeout, wipe, headers, `Notify`, sleeps, dedup, backoff, `initial_fetch`, `timestamp`, commands, shortcut/tray duplication, `Local` dates, `f32`, hide timers, ETag, 3-vs-10 tests, `u8` batting, `Batsman`, `CRR 0`, `Need 0`, draw, validation, poison, `eprintln`, resize/move, sequential GETs, 7-tuple, magic URLs/999.../`Yet to bat`, `limitedOvers` types).
- `tauri.conf.json` (`csp:null`, `withGlobalTauri:true`, `capabilities` missing dashboard + plugin perms, `opener:default`, `targets:all`, icons vs disk, sizes `340x110/278x68/650x450`, `dashboard.visible:true`, missing `url/devUrl/before*`, title casing, `description/authors you`).
- `capabilities/default.json`, `positioner/global-shortcut/tray-icon` perms, `frontendDist:../src`, absolute `/main.js`, `package.json tauri-app/cli ^2/packageManager/engines`, `src-tauri/.gitignore/Cargo.lock/icons/*`, `select_match` string-SSRF shape (native is index-based — see P1 validation still live), Tauri `cache clear/Default` gap, shared-crate/symlink strategy (single `sportspulse` lib now — moot), `260x40` soccer card.
- Web a11y/CSS (`:focus-visible`, `tablist`, two `<main>`, live regions, `9-10px` type, `13px` root, emoji `aria-hidden`, `ul/li`, loader `role=status`, `type=button`, `color-scheme`, `abbr`, `title` tooltips).
- Frontend polling truths (`0.5s/1s/500ms/2000ms`) — historical. Code truth in `native/src/engine/fetcher.rs:39-43,246-279` is canonical.
- Old techdebt shared-crate plan — moot. `native/src/lib.rs` single lib is canonical.

---

## Fix-first order (native-only)

1. P0-3+P0-4+P0-10 (timeout 10s/5s + keep-stale 5s backoff + body cap 1MB + `error_for_status`) + P0-5 (drop `eprintln`/full URL, gate `C:\` logs, move to `%LOCALAPPDATA%`).
2. P0-1+P0-2 (competitor `unwrap_or` + `detail unwrap_or("")`) + engine validation (sport allowlist, `^[A-Za-z0-9._-]{1,64}$`, `active_matches` check).
3. P0-6+P0-7 (per-window `USERDATA`, `Acquire/Release`, `NCDESTROY from_raw` + `NIM_DELETE`, `BeginPaint/EndPaint`, old-bmp `SelectObject`, `ReleaseDC`).
4. P0-8 (drop `&mut` before `SendMessage`, heap-alloc `match_id`, static wide) + P0-9 (non-Windows stub, `MessageBox` not `expect`, `CoInitialize` check).
5. P0-11 (hotkey check + remap, UIA keyboard/SR, dev bins `required-features`) + P1 tray (`NIM_SETVERSION`, `TaskbarCreated`, `WM_NULL`) + channel (`bounded 32`, check `PostMessage`).
6. P1 engine spam/correctness (split ball IDs + soccer suppress + fix soccer tests, `is_test`, completed/win/double, 2nd innings, `Utc`, `Option` rates, tooltip) + UI perf (loader 100ms, persistent buf, resize-before-present, DPI/monitor, TTL consts, hit rects, scroll/mouse/wheel, factory cache, popup paint, bench consts, maximize/mutex, emoji/palette).
7. P1 docs/benchmarks (commit cutover, label indicative or ≥30 runs, prod consts in bench, `polling` consts, PRD banner, `required-features`, lints/CI) + P2 (edition 2024, `time`, prune gitignore, LICENSE/CHANGELOG/ARCHITECTURE, contrast/motion/targets/HC/popup/RTL/supply-chain).

---

## Peer-audit reconciliation — Gemini 3.8 Flash (High) + local B/C (native-only)

Source: Gemini 3.8 Flash (High) via Orca terminal `term_b0a3f702` (scrollback capture) + local opinions B + C (both PASS).
Scope stays native-only (`native/*` + `docs/*` + `README.md` + `PRODUCT.md`). No Tauri resurrection — RETIRED above remains canonical.

Findings mapped (Gemini IDs → this register / owner):

- P0-1 event cadence → engine correctness (shared `last_ball_id` / soccer first-poll spam). Owner: engine agent (fixed there).
- P1-1 overlay DPI snap-back (`WM_DPICHANGED` suggested rect then unscaled `540x200`) → fixed here: `native/src/main.rs` `scale_for_dpi` + `overlay_size_for(score, dpi)` + `place_and_present_overlay` uses `AppState.dpi` (`GetDpiForWindow`, refreshed on `WM_DPICHANGED`). Mirrors dashboard suggested-rect handling.
- P1-2 transient `None` wipes scoreboard → Owner: engine agent (keep-stale / backoff).
- P1-3 sequential 4x GETs block detail → Owner: engine agent (`tokio::join!` merge).
- 8 PASS areas confirmed by Gemini, matching this register: `unwrap` guards, request timeout, event dedup, `GWLP_USERDATA` per-window, GDI `SelectObject`/`ReleaseDC`, tray `NIM_SETVERSION`/`TaskbarCreated`/`WM_NULL`, single-source polling consts, dev-bins gating.

Unrecovered from scrollback: P1-4 + 4 P2s were still generating when captured — text not in scrollback, so not mapped 1:1 here. Covered by local opinions B + C (both PASS) with zero P0; no new native-only action filed on partial IDs. If the full Gemini dump is recovered later, map P1-4/P2s into P1/P2 above rather than appending a parallel list.
