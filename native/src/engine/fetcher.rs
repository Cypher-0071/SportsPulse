use std::sync::atomic::Ordering;
use std::time::Duration;

use reqwest::Client;

use super::cache::ScoreCache;
use super::events::{AppEvent, DiscoveredMatch};
use super::match_state::{ActiveMatchEntry, ActiveMatchesState};
use super::models::{MatchEvent, MatchEventType, MatchStatus, SportType, BATTING_NONE};
use super::parser::{
    parse_all_live_indian_matches, parse_latest_event, parse_match_detail,
    parse_soccer_latest_event, parse_soccer_match_detail, parse_soccer_matches,
};

const MAX_BODY_BYTES: usize = 1_000_000;

/// Consecutive *scoreboard* polls (60s cadence) a selection may be absent
/// before it counts as aged out. P0-1: this is deliberately a scoreboard
/// count, not a detail-tick count — 10 × 60s ≈ 10min grace.
const MAX_MISSING_SELECTION_CYCLES: u32 = 10;

/// True when the selection is absent from a non-empty scoreboard snapshot.
/// Empty snapshots never count as "gone" (total-outage guard, see P1-2).
fn selection_gone_from_scoreboard(
    active: &[ActiveMatchEntry],
    sport: &SportType,
    series_id: &str,
    match_id: &str,
) -> bool {
    !active.is_empty()
        && !active
            .iter()
            .any(|(sp, se, mi, ..)| sp == sport && se == series_id && mi == match_id)
}

/// P0-1 cadence gate: only a fresh scoreboard poll may advance the
/// missing-selection counter. Detail ticks (2s live) pass
/// `scoreboard_refreshed=false` and leave the counter untouched, so a
/// single dropped scoreboard poll can never untrack a live match.
fn next_missing_cycles(prev: u32, scoreboard_refreshed: bool, selection_gone: bool) -> u32 {
    if !scoreboard_refreshed {
        return prev;
    }
    if selection_gone {
        prev.saturating_add(1)
    } else {
        0
    }
}

fn should_evict_selection(missing_cycles: u32) -> bool {
    missing_cycles >= MAX_MISSING_SELECTION_CYCLES
}

/// P1-2 outage guard: true when at least one of the 4 scoreboard GETs
/// returned usable JSON. All-failed means "no fresh data" — the caller
/// must keep the previous list and emit no event.
fn any_scoreboard_fetch_ok(flags: [bool; 4]) -> bool {
    flags.iter().any(|&ok| ok)
}

fn capped_backoff(failures: u32) -> Duration {
    match failures {
        0 | 1 => Duration::from_secs(5),
        2 => Duration::from_secs(10),
        _ => Duration::from_secs(30),
    }
}

async fn fetch_json_capped(client: &Client, url: &str) -> Option<serde_json::Value> {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("[WARN] scoreboard request failed: {}", _e);
            return None;
        }
    };
    let resp = match resp.error_for_status() {
        Ok(r) => r,
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("[WARN] scoreboard HTTP error: {}", _e);
            return None;
        }
    };
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("[WARN] scoreboard body read failed: {}", _e);
            return None;
        }
    };
    if bytes.len() > MAX_BODY_BYTES {
        #[cfg(debug_assertions)]
        eprintln!(
            "[WARN] scoreboard body too large ({} bytes), skipping",
            bytes.len()
        );
        return None;
    }
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(v) => Some(v),
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("[WARN] scoreboard JSON parse failed: {}", _e);
            None
        }
    }
}

pub async fn start_polling(
    cache: ScoreCache,
    match_state: ActiveMatchesState,
    events: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "User-Agent",
        reqwest::header::HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"),
    );
    headers.insert("Accept", reqwest::header::HeaderValue::from_static("*/*"));
    headers.insert(
        "Accept-Language",
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        "Origin",
        reqwest::header::HeaderValue::from_static("https://www.espn.in"),
    );
    headers.insert(
        "Referer",
        reqwest::header::HeaderValue::from_static("https://www.espn.in/cricket/"),
    );

    let client = Client::builder()
        .tcp_nodelay(true)
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| {
            // Header map carried a bad value (should never happen with
            // statics). Retry without custom headers so the 10s/5s timeouts
            // are preserved; only then fall back to `Client::new()`.
            Client::builder()
                .tcp_nodelay(true)
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| Client::new())
        });

    let mut last_cricket_ball_id: Option<String> = None;
    let mut last_soccer_event_id: Option<String> = None;
    let mut last_tracked_match_id: Option<String> = None;
    let mut last_completed_match_id: Option<String> = None;
    let mut last_scoreboard_fetch: Option<std::time::Instant> = None;
    let mut consecutive_failures: u32 = 0;
    // Consecutive *scoreboard* polls where the selection was absent from
    // a non-empty snapshot. At threshold the selection is stale (aged
    // out) and is untracked + cleared instead of polling a dead URL.
    // P0-1: advanced only inside the 60s scoreboard gate below, never on
    // 2s detail ticks.
    let mut selected_missing_cycles: u32 = 0;

    loop {
        // Assigned on every path below (tracked / backoff / idle); no
        // default so a future path that forgets to set it fails to compile
        // instead of silently stalling the loop.
        let mut sleep_duration: Duration;

        // Fetch scoreboards if not fetched recently (every 60s)
        let should_fetch_scoreboard =
            last_scoreboard_fetch.map_or(true, |t| t.elapsed() >= Duration::from_secs(60));
        if should_fetch_scoreboard {
            let mut discovered_matches: Vec<DiscoveredMatch> = Vec::new();
            let today_str = chrono::Utc::now().format("%Y%m%d").to_string();

            let cricket_default_url = "https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in";
            let cricket_today_url = format!("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in&dates={}", today_str);
            let soccer_default_url = "https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=soccer&region=in";
            let soccer_today_url = format!("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=soccer&region=in&dates={}", today_str);

            // P1-3: the 4 scoreboard GETs run concurrently. Sequential
            // 4x10s timeouts stalled the loop up to 40s; `join!` caps the
            // worst case at ~10s. Shared `&client` borrows are safe:
            // `Client` is internally Arc'd and every future is read-only.
            let (cricket_default_json, cricket_today_json, soccer_default_json, soccer_today_json) = tokio::join!(
                fetch_json_capped(&client, cricket_default_url),
                fetch_json_capped(&client, &cricket_today_url),
                fetch_json_capped(&client, soccer_default_url),
                fetch_json_capped(&client, &soccer_today_url)
            );

            // P1-2: total-outage guard. All 4 failed means no fresh data:
            // keep the previous list and emit nothing so the dashboard
            // never flickers to empty.
            let scoreboard_ok = any_scoreboard_fetch_ok([
                cricket_default_json.is_some(),
                cricket_today_json.is_some(),
                soccer_default_json.is_some(),
                soccer_today_json.is_some(),
            ]);
            if !scoreboard_ok {
                #[cfg(debug_assertions)]
                eprintln!("[WARN] all scoreboard fetches failed, keeping previous list");
                last_scoreboard_fetch = Some(std::time::Instant::now());
            } else {
                // 1. Cricket Scoreboards
                let mut cricket_matches = Vec::new();
                if let Some(json) = cricket_default_json.as_ref() {
                    cricket_matches.extend(parse_all_live_indian_matches(json));
                }
                if let Some(json) = cricket_today_json.as_ref() {
                    cricket_matches.extend(parse_all_live_indian_matches(json));
                }

                cricket_matches.sort_by(|a, b| a.1.cmp(&b.1));
                cricket_matches.dedup_by_key(|m| m.1.clone());

                for (series_id, match_id, title, status, league_name, start_time) in cricket_matches
                {
                    discovered_matches.push(DiscoveredMatch {
                        sport: SportType::Cricket,
                        series_id,
                        match_id,
                        title,
                        status,
                        league_name,
                        start_time,
                    });
                }

                // 2. Soccer Scoreboards
                let mut soccer_matches = Vec::new();
                if let Some(json) = soccer_default_json.as_ref() {
                    soccer_matches.extend(parse_soccer_matches(json));
                }
                if let Some(json) = soccer_today_json.as_ref() {
                    soccer_matches.extend(parse_soccer_matches(json));
                }

                soccer_matches.sort_by(|a, b| a.1.cmp(&b.1));
                soccer_matches.dedup_by_key(|m| m.1.clone());

                for (series_id, match_id, title, status, league_name, start_time) in soccer_matches
                {
                    discovered_matches.push(DiscoveredMatch {
                        sport: SportType::Soccer,
                        series_id,
                        match_id,
                        title,
                        status,
                        league_name,
                        start_time,
                    });
                }

                // Update active matches list (typed `ActiveMatchEntry` vec).
                let discovered_tuples: Vec<ActiveMatchEntry> = discovered_matches
                    .iter()
                    .map(|d| {
                        (
                            d.sport,
                            d.series_id.clone(),
                            d.match_id.clone(),
                            d.title.clone(),
                            d.status.clone(),
                            d.league_name.clone(),
                            d.start_time.clone(),
                        )
                    })
                    .collect();

                let first_scoreboard = last_scoreboard_fetch.is_none();
                let mut list_changed = false;
                {
                    // Recover from a poisoned lock instead of dropping the update:
                    // a panic elsewhere must not freeze the dashboard list.
                    let mut active_m = match_state
                        .active_matches
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if *active_m != discovered_tuples {
                        *active_m = discovered_tuples;
                        list_changed = true;
                    }
                }

                if list_changed || first_scoreboard {
                    // Release pairs with the UI thread's Acquire load in
                    // `dashboard_is_loading` (main.rs); Relaxed would let the
                    // active_matches write stay invisible after the flag flips.
                    match_state
                        .initial_fetch_completed
                        .store(true, Ordering::Release);
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[DEBUG] Discovered matches changed ({} entries)",
                        discovered_matches.len()
                    );
                    let _ = events.send(AppEvent::MatchesDiscovered(discovered_matches));
                }

                // P0-1: stale-selection eviction lives strictly inside the
                // scoreboard gate on fresh data. Detail ticks (2s live)
                // never reach this code, so one dropped poll (~1 missing
                // cycle) cannot untrack a match with healthy detail
                // updates; eviction needs 10 straight missing polls.
                if let Some((sport, series_id, match_id)) = match_state.get_selected_match() {
                    // Poison-recovering read: a panic elsewhere must not
                    // pin the selection as forever-present (dead URL poll).
                    let selection_gone = {
                        let active = match_state
                            .active_matches
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        selection_gone_from_scoreboard(&active, &sport, &series_id, &match_id)
                    };
                    selected_missing_cycles =
                        next_missing_cycles(selected_missing_cycles, true, selection_gone);
                    if should_evict_selection(selected_missing_cycles) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[DEBUG] selection vanished from scoreboard, untracking: sport={} match_id={}",
                            sport, match_id
                        );
                        match_state.untrack_match();
                        cache.clear();
                        selected_missing_cycles = 0;
                        last_tracked_match_id = None;
                        last_completed_match_id = None;
                        last_cricket_ball_id = None;
                        last_soccer_event_id = None;
                    }
                } else {
                    selected_missing_cycles = 0;
                }

                last_scoreboard_fetch = Some(std::time::Instant::now());
            }
        }

        // Determine which match to track (`select_match` already validated
        // the sport/id shapes, so `detail_url` below is safe to build).
        // Re-read after the gate: a scoreboard-gated eviction above already
        // cleared the selection via `untrack_match`.
        let match_to_track = match_state.get_selected_match();

        if let Some((sport, series_id, match_id)) = match_to_track {
            let is_already_completed = last_completed_match_id.as_ref() == Some(&match_id);

            if is_already_completed {
                // Already fetched completed match final score and cached it. No need to poll again.
                sleep_duration = Duration::from_secs(300);
            } else {
                let detail_url = format!(
                    "https://site.web.api.espn.com/apis/site/v2/sports/{}/{}/summary?event={}",
                    sport, series_id, match_id
                );
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] Fetching: sport={} match_id={}", sport, match_id);

                match fetch_json_capped(&client, &detail_url).await {
                    Some(detail_json) => {
                        let parsed_score = if sport == SportType::Soccer {
                            parse_soccer_match_detail(&detail_json, &series_id, &match_id)
                        } else {
                            parse_match_detail(&detail_json, &series_id, &match_id)
                        };
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[DEBUG] sport={} match_id={} parse result: {}",
                            sport,
                            match_id,
                            parsed_score.is_some()
                        );

                        if let Some(mut score) = parsed_score {
                            consecutive_failures = 0;
                            // Innings-break polls report `crr 0.0` with nobody
                            // batting: keep the last good rate so the overlay
                            // never flashes `CRR: 0.00` mid-game.
                            let no_batting = score.batting_team == BATTING_NONE;
                            if no_batting && score.crr == 0.0 {
                                if let Some(prev) = cache.get() {
                                    if prev.match_id == score.match_id && prev.crr != 0.0 {
                                        score.crr = prev.crr;
                                    }
                                }
                            }
                            cache.set(Some(score.clone()));
                            let _ = events.send(AppEvent::ScoreChanged(Box::new(score.clone())));

                            // Detect match change initialization for completed status
                            let is_first_fetch_for_match =
                                last_tracked_match_id.as_ref() != Some(&match_id);
                            if is_first_fetch_for_match {
                                last_tracked_match_id = Some(match_id.clone());
                                last_cricket_ball_id = None;
                                last_soccer_event_id = None;
                                // Deliberately do NOT pre-mark completed here:
                                // the win check below emits on
                                // first-fetch-completed when a winner is set.
                                if score.status != MatchStatus::Completed {
                                    last_completed_match_id = None;
                                }
                            }

                            // Win event: transition to Completed, plus
                            // first-fetch-completed so tracking a match
                            // after full-time still pops `MATCH WON!`.
                            let mut win_emitted = false;
                            if score.status == MatchStatus::Completed
                                && last_completed_match_id.as_ref() != Some(&match_id)
                            {
                                last_completed_match_id = Some(match_id.clone());

                                let winner_name = if score.team1.is_winner {
                                    Some(score.team1.name.clone())
                                } else if score.team2.is_winner {
                                    Some(score.team2.name.clone())
                                } else {
                                    None
                                };

                                if let Some(w_name) = winner_name {
                                    let win_event = MatchEvent {
                                        event_type: MatchEventType::Win,
                                        title: "MATCH WON!".to_string(),
                                        description: format!("{} won the match!", w_name),
                                        score: format!(
                                            "{} vs {}",
                                            score.team1.abbreviation, score.team2.abbreviation
                                        ),
                                        sport,
                                    };
                                    cache.set_latest_event(Some(win_event.clone()));
                                    let _ = events.send(AppEvent::MatchEvent(Box::new(win_event)));
                                    win_emitted = true;
                                }
                            }

                            // Ball/key events, skipped when a win fired on
                            // the same poll (coalesce: one popup, no double
                            // flash timer).
                            if !win_emitted {
                                let parsed_event = if sport == SportType::Soccer {
                                    parse_soccer_latest_event(
                                        &detail_json,
                                        &mut last_soccer_event_id,
                                    )
                                } else {
                                    parse_latest_event(&detail_json, &mut last_cricket_ball_id)
                                };

                                if let Some(event) = parsed_event {
                                    cache.set_latest_event(Some(event.clone()));
                                    let _ = events.send(AppEvent::MatchEvent(Box::new(event)));
                                }
                            }

                            sleep_duration = match score.status {
                                MatchStatus::Live => {
                                    if sport == SportType::Soccer {
                                        Duration::from_secs(3)
                                    } else {
                                        // 10s for Tests (slower pace), 2s for T20/ODIs.
                                        // `is_test` rides on the parsed score so the
                                        // fetcher never re-derives it from the title.
                                        if score.is_test {
                                            Duration::from_secs(10)
                                        } else {
                                            Duration::from_secs(2)
                                        }
                                    }
                                }
                                MatchStatus::Break => Duration::from_secs(30),
                                MatchStatus::Scheduled => Duration::from_secs(30),
                                MatchStatus::Completed => Duration::from_secs(300),
                                MatchStatus::NoMatch => Duration::from_secs(300),
                            };
                        } else {
                            // Transient parse miss: keep stale cache, back off.
                            consecutive_failures += 1;
                            if consecutive_failures > 10 {
                                cache.set(None);
                            }
                            sleep_duration = capped_backoff(consecutive_failures);
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[WARN] sport={} match_id={} detail parse empty (failures={})",
                                sport, match_id, consecutive_failures
                            );
                        }
                    }
                    None => {
                        // Transport / HTTP / body-cap / JSON failure: keep stale, back off.
                        consecutive_failures += 1;
                        if consecutive_failures > 10 {
                            cache.set(None);
                        }
                        sleep_duration = capped_backoff(consecutive_failures);
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[WARN] sport={} match_id={} detail fetch failed (failures={})",
                            sport, match_id, consecutive_failures
                        );
                    }
                }
            }
        } else {
            cache.set(None);
            consecutive_failures = 0;
            last_tracked_match_id = None;
            last_completed_match_id = None;
            last_cricket_ball_id = None;
            last_soccer_event_id = None;
            sleep_duration = Duration::from_secs(30); // Re-check scoreboard every 30s for new live matches
        }

        // Cap the sleep so the 60s scoreboard refresh still runs while a
        // Completed/NoMatch detail idles at 300s.
        if let Some(last) = last_scoreboard_fetch {
            let until_scoreboard = Duration::from_secs(60).saturating_sub(last.elapsed());
            if !until_scoreboard.is_zero() && until_scoreboard < sleep_duration {
                sleep_duration = until_scoreboard;
            }
        }

        // Release pairs with the UI thread's Acquire load (see above).
        match_state
            .initial_fetch_completed
            .store(true, Ordering::Release);

        tokio::select! {
            _ = tokio::time::sleep(sleep_duration) => {},
            _ = match_state.notify.notified() => {
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] Fetcher waken up by match selection change!");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capped_backoff_steps() {
        assert_eq!(capped_backoff(0), Duration::from_secs(5));
        assert_eq!(capped_backoff(1), Duration::from_secs(5));
        assert_eq!(capped_backoff(2), Duration::from_secs(10));
        assert_eq!(capped_backoff(3), Duration::from_secs(30));
        assert_eq!(capped_backoff(10), Duration::from_secs(30));
        assert_eq!(capped_backoff(100), Duration::from_secs(30));
    }

    fn test_entry(sport: SportType, series: &str, id: &str) -> ActiveMatchEntry {
        (
            sport,
            series.to_string(),
            id.to_string(),
            "title".to_string(),
            "live".to_string(),
            "league".to_string(),
            "start".to_string(),
        )
    }

    #[test]
    fn test_detail_ticks_never_advance_missing_counter() {
        // P0-1 regression: 2s detail ticks must not tick the counter even
        // when the selection looks absent — only a fresh 60s scoreboard
        // poll may. Thirty live detail ticks leave the count at zero.
        let mut cycles = 0;
        for _ in 0..30 {
            cycles = next_missing_cycles(cycles, false, true);
        }
        assert_eq!(cycles, 0);
        assert!(!should_evict_selection(cycles));
    }

    #[test]
    fn test_transient_drop_keeps_tracking() {
        // P0-1: a single dropped scoreboard poll (absent once) advances to
        // 1 but must not evict; the next good poll resets to zero.
        let mut cycles = 0;
        cycles = next_missing_cycles(cycles, true, true);
        assert_eq!(cycles, 1);
        assert!(!should_evict_selection(cycles));

        cycles = next_missing_cycles(cycles, true, false);
        assert_eq!(cycles, 0);
        assert!(!should_evict_selection(cycles));
    }

    #[test]
    fn test_ten_straight_missing_scoreboards_evict() {
        // P0-1: eviction still works, but on scoreboard cadence — 10
        // straight missing 60s polls (≈10min), not 10 detail ticks (20s).
        let mut cycles = 0;
        for i in 1..=9 {
            cycles = next_missing_cycles(cycles, true, true);
            assert_eq!(cycles, i);
            assert!(!should_evict_selection(cycles));
        }
        cycles = next_missing_cycles(cycles, true, true);
        assert_eq!(cycles, MAX_MISSING_SELECTION_CYCLES);
        assert!(should_evict_selection(cycles));
    }

    #[test]
    fn test_empty_scoreboard_never_counts_as_gone() {
        // Empty snapshot = outage/loading, not "selection aged out".
        let empty: Vec<ActiveMatchEntry> = Vec::new();
        assert!(!selection_gone_from_scoreboard(
            &empty,
            &SportType::Cricket,
            "14135",
            "1413511"
        ));

        let active = vec![test_entry(SportType::Cricket, "14135", "1413511")];
        assert!(!selection_gone_from_scoreboard(
            &active,
            &SportType::Cricket,
            "14135",
            "1413511"
        ));
        assert!(selection_gone_from_scoreboard(
            &active,
            &SportType::Cricket,
            "14135",
            "9999999"
        ));
    }

    #[test]
    fn test_all_scoreboard_failed_skips_update() {
        // P1-2 outage guard: only all-failed skips the dashboard update.
        assert!(!any_scoreboard_fetch_ok([false, false, false, false]));
        assert!(any_scoreboard_fetch_ok([true, false, false, false]));
        assert!(any_scoreboard_fetch_ok([false, true, false, false]));
        assert!(any_scoreboard_fetch_ok([false, false, true, false]));
        assert!(any_scoreboard_fetch_ok([false, false, false, true]));
        assert!(any_scoreboard_fetch_ok([true, true, true, true]));
    }
}
