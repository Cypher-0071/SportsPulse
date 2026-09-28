# SportsPulse — Polling Intervals (canonical)

Single source of truth: `start_polling` in `native/src/engine/fetcher.rs`
(client timeouts, 60s discovery gate, per-status sleep). Historical frontend
numbers (`0.5s` in `docs/prd.md`, `1s` in `docs/implementation.md`,
`500ms/2000ms` web tick) are archived and no longer apply.
(Line numbers drift — anchor to `start_polling`, not to lines. Current
landmarks: client timeouts ~`fetcher.rs:92-98`, 60s gate ~`:118-120`,
per-status sleep ~`:378-397`, scoreboard-cap ~`:437-444`.)

| State | Interval | Code ref |
|---|---|---|
| Live T20 / ODI (cricket, tracked) | 2s | `start_polling` per-status match |
| Live Test (cricket, tracked) | 10s | `start_polling` per-status match |
| Live soccer (tracked) | 3s | `start_polling` per-status match |
| Break (drinks / lunch / interval) | 30s | `start_polling` per-status match |
| Scheduled / pre-match (tracked) | 30s | `start_polling` per-status match |
| Completed (tracked) | 300s | `start_polling` per-status match |
| Parse-fail / NoMatch (tracked) | 300s (default) | `start_polling` backoff/default |
| No selection (discovery only) | 30s | `start_polling` idle branch |
| Scoreboard discovery (all sports) | 60s gate | `start_polling` scoreboard gate |
| Completed re-poll / idle default | 300s | `start_polling` per-status match |

## Anti-ban rationale

- Browser-mimicking headers (`User-Agent` Chrome, `Accept`, `Accept-Language`,
  `Origin https://www.espn.in`, `Referer https://www.espn.in/cricket/`) so ESPN
  treats polls as regular page-adjacent traffic (`start_polling` header block).
- `tcp_nodelay(true)` + shared `reqwest::Client` (keep-alive) avoids repeated
  handshakes instead of hammering new connections.
- Respectful floor: fastest loop is 2s (live limited-overs); slower pace for
  Tests (10s) and idle/completed (300s) keeps request volume low.
- Discovery (4 scoreboard GETs) gated to 60s via `last_scoreboard_fetch`
  (`start_polling` scoreboard gate); detail fetch is one GET per tick for the single
  tracked match.
- Wake-up via `Notify` (`start_polling` select-wait at the loop tail) re-polls
  immediately on selection change instead of polling faster globally.

Constants live inline in `start_polling` (`native/src/engine/fetcher.rs`);
there is no separate polling-consts module. Methodology gaps (timeout, stale
keep-alive, backoff, ETag/gzip) are tracked in `docs/techdebt.md` (P0-3/P0-4/P0-10/P1).
