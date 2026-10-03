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

const MAX_BODY_BYTES: usize = 10_000_000;

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

/// P1-2 outage guard: true when at least one of the scoreboard GETs
/// returned usable JSON. All-failed means "no fresh data" — the caller
/// must keep the previous list and emit no event.
fn any_scoreboard_fetch_ok<T: AsRef<[bool]>>(flags: T) -> bool {
    flags.as_ref().iter().any(|&ok| ok)
}

/// `%Y%m` window for next-month league scoreboards, derived from the
/// first of next month (not `now + 20d`, which stays in-month early on).
fn next_month_ym_for_year_month(year: i32, month: u32) -> String {
    let (y, m) = if month >= 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    format!("{:04}{:02}", y, m)
}

fn next_month_ym(now: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Datelike;
    next_month_ym_for_year_month(now.year(), now.month())
}

/// Partial-outage merge: `None` means that sport's fetch group returned
/// no data, so keep the previous slice for that sport instead of wiping
/// it. `Some(vec)` replaces that sport's slice (even when empty).
fn merge_discovered_per_sport(
    prev: &[DiscoveredMatch],
    fresh_cricket: Option<Vec<DiscoveredMatch>>,
    fresh_soccer: Option<Vec<DiscoveredMatch>>,
) -> Vec<DiscoveredMatch> {
    let mut out = Vec::new();
    match fresh_cricket {
        Some(v) => out.extend(v),
        None => out.extend(
            prev.iter()
                .filter(|d| d.sport == SportType::Cricket)
                .cloned(),
        ),
    }
    match fresh_soccer {
        Some(v) => out.extend(v),
        None => out.extend(
            prev.iter()
                .filter(|d| d.sport == SportType::Soccer)
                .cloned(),
        ),
    }
    out
}

/// Identity projection for discovery re-fire: only (sport, series_id,
/// match_id, status) counts. Title/league/start-time text flips must not
/// re-present the dashboard every 60s.
fn discovery_identity_changed(prev: &[ActiveMatchEntry], next: &[ActiveMatchEntry]) -> bool {
    if prev.len() != next.len() {
        return true;
    }
    for (a, b) in prev.iter().zip(next.iter()) {
        if a.0 != b.0 || a.1 != b.1 || a.2 != b.2 || a.4 != b.4 {
            return true;
        }
    }
    false
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
    // Pre-buffer cap: skip oversized bodies without loading 10MB into RAM.
    // The post-buffer length check below stays as backstop (no header / lying header).
    if let Some(len) = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
    {
        if len > MAX_BODY_BYTES {
            #[cfg(debug_assertions)]
            eprintln!(
                "[WARN] scoreboard Content-Length too large ({} bytes), skipping",
                len
            );
            return None;
        }
    }
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
            let now = chrono::Utc::now();
            let today_str = now.format("%Y%m%d").to_string();
            // First-of-next-month `%Y%m` window for upcoming league fixtures.
            let next_month_window = next_month_ym(now);

            let cricket_default_url = "https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in";
            let cricket_today_url = format!("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in&dates={}", today_str);
            let soccer_global_url =
                "https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=soccer";

            let eng_next_url = format!(
                "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard?dates={}",
                next_month_window
            );
            let esp_next_url = format!(
                "https://site.api.espn.com/apis/site/v2/sports/soccer/esp.1/scoreboard?dates={}",
                next_month_window
            );
            let ita_next_url = format!(
                "https://site.api.espn.com/apis/site/v2/sports/soccer/ita.1/scoreboard?dates={}",
                next_month_window
            );
            let ger_next_url = format!(
                "https://site.api.espn.com/apis/site/v2/sports/soccer/ger.1/scoreboard?dates={}",
                next_month_window
            );

            let (
                cricket_default_json,
                cricket_today_json,
                soccer_global_json,
                ucl_json,
                uel_json,
                isl_json,
                eng_json,
                esp_json,
                ita_json,
                ger_json,
                fra_json,
                eng_next_json,
                esp_next_json,
                ita_next_json,
                ger_next_json,
            ) = tokio::join!(
                fetch_json_capped(&client, cricket_default_url),
                fetch_json_capped(&client, &cricket_today_url),
                fetch_json_capped(&client, soccer_global_url),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/uefa.champions/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/uefa.europa/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/ind.1/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/esp.1/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/ita.1/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/ger.1/scoreboard"),
                fetch_json_capped(&client, "https://site.api.espn.com/apis/site/v2/sports/soccer/fra.1/scoreboard"),
                fetch_json_capped(&client, &eng_next_url),
                fetch_json_capped(&client, &esp_next_url),
                fetch_json_capped(&client, &ita_next_url),
                fetch_json_capped(&client, &ger_next_url)
            );

            // Per-sport outage gates: only the sport whose fetch group
            // returned data replaces its slice. A soccer-only outage keeps
            // cricket entries (and vice versa) instead of wiping them.
            let cricket_ok = cricket_default_json.is_some() || cricket_today_json.is_some();
            let soccer_ok = any_scoreboard_fetch_ok([
                soccer_global_json.is_some(),
                ucl_json.is_some(),
                uel_json.is_some(),
                isl_json.is_some(),
                eng_json.is_some(),
                esp_json.is_some(),
                ita_json.is_some(),
                ger_json.is_some(),
                fra_json.is_some(),
                eng_next_json.is_some(),
                esp_next_json.is_some(),
                ita_next_json.is_some(),
                ger_next_json.is_some(),
            ]);
            let scoreboard_ok = cricket_ok || soccer_ok;
            if !scoreboard_ok {
                #[cfg(debug_assertions)]
                eprintln!("[WARN] all scoreboard fetches failed, keeping previous list");
                last_scoreboard_fetch = Some(std::time::Instant::now());
            } else {
                // 1. Cricket Scoreboards (only when this sport fetched OK).
                let fresh_cricket: Option<Vec<DiscoveredMatch>> = if cricket_ok {
                    let mut cricket_matches = Vec::new();
                    if let Some(json) = cricket_default_json.as_ref() {
                        cricket_matches.extend(parse_all_live_indian_matches(json));
                    }
                    if let Some(json) = cricket_today_json.as_ref() {
                        cricket_matches.extend(parse_all_live_indian_matches(json));
                    }

                    cricket_matches.sort_by(|a, b| a.1.cmp(&b.1));
                    cricket_matches.dedup_by_key(|m| m.1.clone());

                    Some(
                        cricket_matches
                            .into_iter()
                            .map(
                                |(series_id, match_id, title, status, league_name, start_time)| {
                                    DiscoveredMatch {
                                        sport: SportType::Cricket,
                                        series_id,
                                        match_id,
                                        title,
                                        status,
                                        league_name,
                                        start_time,
                                    }
                                },
                            )
                            .collect(),
                    )
                } else {
                    None
                };

                // 2. Soccer Scoreboards (only when this sport fetched OK).
                let fresh_soccer: Option<Vec<DiscoveredMatch>> = if soccer_ok {
                    let mut soccer_matches = Vec::new();
                    if let Some(json) = soccer_global_json.as_ref() {
                        soccer_matches.extend(parse_soccer_matches(json));
                    }
                    for json_opt in [
                        &ucl_json,
                        &uel_json,
                        &isl_json,
                        &eng_json,
                        &esp_json,
                        &ita_json,
                        &ger_json,
                        &fra_json,
                        &eng_next_json,
                        &esp_next_json,
                        &ita_next_json,
                        &ger_next_json,
                    ] {
                        if let Some(json) = json_opt.as_ref() {
                            soccer_matches.extend(parse_soccer_matches(json));
                        }
                    }

                    soccer_matches.sort_by(|a, b| a.1.cmp(&b.1));
                    soccer_matches.dedup_by_key(|m| m.1.clone());

                    Some(
                        soccer_matches
                            .into_iter()
                            .map(
                                |(series_id, match_id, title, status, league_name, start_time)| {
                                    DiscoveredMatch {
                                        sport: SportType::Soccer,
                                        series_id,
                                        match_id,
                                        title,
                                        status,
                                        league_name,
                                        start_time,
                                    }
                                },
                            )
                            .collect(),
                    )
                } else {
                    None
                };

                // Merge fresh slices over the previous list so the failed
                // sport's slice survives a partial outage.
                let prev_snapshot: Vec<DiscoveredMatch> = match_state
                    .active_matches
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .map(
                        |(sport, series_id, match_id, title, status, league_name, start_time)| {
                            DiscoveredMatch {
                                sport: *sport,
                                series_id: series_id.clone(),
                                match_id: match_id.clone(),
                                title: title.clone(),
                                status: status.clone(),
                                league_name: league_name.clone(),
                                start_time: start_time.clone(),
                            }
                        },
                    )
                    .collect();
                let discovered_matches =
                    merge_discovered_per_sport(&prev_snapshot, fresh_cricket, fresh_soccer);

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
                    // Identity compare (sport, series_id, match_id, status):
                    // title/league/start-time text flips update silently
                    // without re-presenting the dashboard every 60s.
                    let mut active_m = match_state
                        .active_matches
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if discovery_identity_changed(&active_m, &discovered_tuples) {
                        *active_m = discovered_tuples;
                        list_changed = true;
                    } else if *active_m != discovered_tuples {
                        *active_m = discovered_tuples;
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
            // Idle with no selection: wake every 30s; the 60s scoreboard gate
            // above still decides whether a fresh discovery fetch runs.
            sleep_duration = Duration::from_secs(30);
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

    fn test_discovered(sport: SportType, series: &str, id: &str) -> DiscoveredMatch {
        DiscoveredMatch {
            sport,
            series_id: series.to_string(),
            match_id: id.to_string(),
            title: "title".to_string(),
            status: "in".to_string(),
            league_name: "league".to_string(),
            start_time: "start".to_string(),
        }
    }

    #[test]
    fn test_partial_outage_keeps_failed_sport_slice() {
        // Soccer outage (fresh_soccer=None) preserves soccer entries while
        // cricket replaces its slice, and vice versa.
        let prev = vec![
            test_discovered(SportType::Cricket, "14135", "1413511"),
            test_discovered(SportType::Soccer, "eng.1", "700100"),
        ];
        let fresh_cricket = vec![test_discovered(SportType::Cricket, "14135", "1413512")];
        let merged = merge_discovered_per_sport(&prev, Some(fresh_cricket), None);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|d| d.match_id == "1413512"));
        assert!(merged.iter().any(|d| d.match_id == "700100"));

        let fresh_soccer = vec![test_discovered(SportType::Soccer, "eng.1", "700101")];
        let merged2 = merge_discovered_per_sport(&prev, None, Some(fresh_soccer));
        assert_eq!(merged2.len(), 2);
        assert!(merged2.iter().any(|d| d.match_id == "1413511"));
        assert!(merged2.iter().any(|d| d.match_id == "700101"));
    }

    #[test]
    fn test_next_month_ym_rolls_over_december() {
        assert_eq!(next_month_ym_for_year_month(2026, 1), "202602");
        assert_eq!(next_month_ym_for_year_month(2026, 11), "202612");
        assert_eq!(next_month_ym_for_year_month(2026, 12), "202701");
        // Early-month dates must still land in next month (not now+20d).
        assert_eq!(next_month_ym_for_year_month(2026, 9), "202610");
    }

    #[test]
    fn test_discovery_identity_ignores_text_flips() {
        // Same identity, different title text: no re-fire.
        let prev = vec![test_entry(SportType::Cricket, "14135", "1413511")];
        let mut same_identity = prev.clone();
        same_identity[0].3 = "new title text".to_string();
        same_identity[0].5 = "new league".to_string();
        assert!(!discovery_identity_changed(&prev, &same_identity));

        // Status flip is identity: re-fires.
        let mut status_flip = prev.clone();
        status_flip[0].4 = "pre".to_string();
        assert!(discovery_identity_changed(&prev, &status_flip));

        // Added/removed rows re-fire.
        assert!(discovery_identity_changed(&prev, &[]));
        let mut added = prev.clone();
        added.push(test_entry(SportType::Soccer, "eng.1", "700100"));
        assert!(discovery_identity_changed(&prev, &added));
    }

    #[tokio::test]
    async fn test_live_espn_soccer_fetch() {
        let client = Client::builder()
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url =
            "https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=soccer";
        let res = fetch_json_capped(&client, url).await;
        assert!(res.is_some(), "fetch_json_capped returned None");
        let matches = parse_soccer_matches(&res.unwrap());
        println!("PARSED_SOCCER_COUNT: {}", matches.len());
        for m in matches.iter().take(5) {
            println!(
                "SAMPLE_SOCCER: series={} id={} title='{}' status={} league='{}' start='{}'",
                m.0, m.1, m.2, m.3, m.4, m.5
            );
        }
        assert!(
            !matches.is_empty(),
            "parse_soccer_matches returned empty vec"
        );
    }
}
