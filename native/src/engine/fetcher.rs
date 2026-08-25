use std::sync::atomic::Ordering;
use std::time::Duration;

use reqwest::Client;

use super::cache::ScoreCache;
use super::events::{AppEvent, DiscoveredMatch};
use super::match_state::ActiveMatchesState;
use super::models::{MatchEvent, MatchEventType, MatchStatus};
use super::parser::{
    parse_all_live_indian_matches, parse_latest_event, parse_match_detail,
    parse_soccer_latest_event, parse_soccer_match_detail, parse_soccer_matches,
};

pub async fn start_polling(
    cache: ScoreCache,
    match_state: ActiveMatchesState,
    events: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("Accept", "*/*".parse().unwrap());
    headers.insert("Accept-Language", "en-US,en;q=0.9".parse().unwrap());
    headers.insert("Origin", "https://www.espn.in".parse().unwrap());
    headers.insert("Referer", "https://www.espn.in/cricket/".parse().unwrap());

    let client = Client::builder()
        .tcp_nodelay(true)
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| Client::new());

    let mut last_ball_id: Option<String> = None;
    let mut last_tracked_match_id: Option<String> = None;
    let mut last_completed_match_id: Option<String> = None;
    let mut last_scoreboard_fetch: Option<std::time::Instant> = None;

    loop {
        let mut sleep_duration = Duration::from_secs(300);

        // Fetch scoreboards if not fetched recently (every 60s)
        let should_fetch_scoreboard = last_scoreboard_fetch.map_or(true, |t| t.elapsed() >= Duration::from_secs(60));
        if should_fetch_scoreboard {
            let mut discovered_matches: Vec<DiscoveredMatch> = Vec::new();
            let today_str = chrono::Local::now().format("%Y%m%d").to_string();

            // 1. Fetch Cricket Scoreboards
            let mut cricket_matches = Vec::new();
            // Default (Live / Recent)
            if let Ok(resp) = client.get("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in").send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    cricket_matches.extend(parse_all_live_indian_matches(&json));
                }
            }
            // Today's Scheduled
            let cricket_today_url = format!("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in&dates={}", today_str);
            if let Ok(resp) = client.get(&cricket_today_url).send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    cricket_matches.extend(parse_all_live_indian_matches(&json));
                }
            }

            cricket_matches.sort_by_key(|m| m.1.clone());
            cricket_matches.dedup_by_key(|m| m.1.clone());

            for (series_id, match_id, title, status, league_name, start_time) in cricket_matches {
                discovered_matches.push(DiscoveredMatch {
                    sport: "cricket".to_string(),
                    series_id,
                    match_id,
                    title,
                    status,
                    league_name,
                    start_time,
                });
            }

            // 2. Fetch Soccer Scoreboards
            let mut soccer_matches = Vec::new();
            // Default (Live / Recent)
            if let Ok(resp) = client.get("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=soccer&region=in").send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    soccer_matches.extend(parse_soccer_matches(&json));
                }
            }
            // Today's Scheduled
            let soccer_today_url = format!("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=soccer&region=in&dates={}", today_str);
            if let Ok(resp) = client.get(&soccer_today_url).send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    soccer_matches.extend(parse_soccer_matches(&json));
                }
            }

            soccer_matches.sort_by_key(|m| m.1.clone());
            soccer_matches.dedup_by_key(|m| m.1.clone());

            for (series_id, match_id, title, status, league_name, start_time) in soccer_matches {
                discovered_matches.push(DiscoveredMatch {
                    sport: "soccer".to_string(),
                    series_id,
                    match_id,
                    title,
                    status,
                    league_name,
                    start_time,
                });
            }

            // Update active matches list (kept as tuple vec to avoid touching match_state.rs)
            let discovered_tuples: Vec<(String, String, String, String, String, String, String)> = discovered_matches
                .iter()
                .map(|d| {
                    (
                        d.sport.clone(),
                        d.series_id.clone(),
                        d.match_id.clone(),
                        d.title.clone(),
                        d.status.clone(),
                        d.league_name.clone(),
                        d.start_time.clone(),
                    )
                })
                .collect();

            let mut list_changed = false;
            if let Ok(mut active_m) = match_state.active_matches.lock() {
                if *active_m != discovered_tuples {
                    *active_m = discovered_tuples;
                    list_changed = true;
                }
            }

            if list_changed {
                eprintln!("[DEBUG] Discovered matches changed ({} entries)", discovered_matches.len());
                let _ = events.send(AppEvent::MatchesDiscovered(discovered_matches));
            }

            last_scoreboard_fetch = Some(std::time::Instant::now());
        }

        // Determine which match to track
        let match_to_track = match_state.selected_match.lock().ok().and_then(|s| s.clone());

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
                eprintln!("[DEBUG] Fetching: {}", detail_url);

                match client.get(&detail_url).send().await {
                    Ok(detail_resp) => {
                        let status_code = detail_resp.status();
                        if let Ok(detail_json) = detail_resp.json::<serde_json::Value>().await {
                            let parsed_score = if sport == "soccer" {
                                parse_soccer_match_detail(&detail_json, &series_id, &match_id)
                            } else {
                                parse_match_detail(&detail_json, &series_id, &match_id)
                            };
                            eprintln!("[DEBUG] HTTP {} | parse result: {}", status_code, parsed_score.is_some());

                            if let Some(score) = parsed_score {
                                cache.set(Some(score.clone()));
                                let _ = events.send(AppEvent::ScoreChanged(Box::new(score.clone())));

                                // Detect match change initialization for completed status
                                let is_first_fetch_for_match = last_tracked_match_id.as_ref() != Some(&match_id);
                                if is_first_fetch_for_match {
                                    last_tracked_match_id = Some(match_id.clone());
                                    last_ball_id = None;
                                    if score.status == MatchStatus::Completed {
                                        last_completed_match_id = Some(match_id.clone());
                                    } else {
                                        last_completed_match_id = None;
                                    }
                                }

                                // Check for win event (transition to Completed)
                                if score.status == MatchStatus::Completed && last_completed_match_id.as_ref() != Some(&match_id) {
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
                                            score: format!("{} vs {}", score.team1.abbreviation, score.team2.abbreviation),
                                            sport: sport.clone(),
                                        };
                                        cache.set_latest_event(Some(win_event.clone()));
                                        let _ = events.send(AppEvent::MatchEvent(Box::new(win_event)));
                                    }
                                }

                                // Detect match events (boundaries/wickets in cricket, goals/red cards in soccer)
                                let parsed_event = if sport == "soccer" {
                                    parse_soccer_latest_event(&detail_json, &mut last_ball_id)
                                } else {
                                    parse_latest_event(&detail_json, &mut last_ball_id)
                                };

                                if let Some(event) = parsed_event {
                                    cache.set_latest_event(Some(event.clone()));
                                    let _ = events.send(AppEvent::MatchEvent(Box::new(event)));
                                }

                                sleep_duration = match score.status {
                                    MatchStatus::Live => {
                                        if sport == "soccer" {
                                            Duration::from_secs(3)
                                        } else {
                                            // Set 10s polling rate for Test cricket (slower pace), 2s for T20/ODIs
                                            let is_test = score.match_title.to_lowercase().contains("test");
                                            if is_test {
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
                                cache.set(None);
                            }
                        }
                    }
                    Err(e) => eprintln!("Error fetching match details: {}", e),
                }
            }
        } else {
            cache.set(None);
            sleep_duration = Duration::from_secs(30); // Re-check scoreboard every 30s for new live matches
        }

        match_state.initial_fetch_completed.store(true, Ordering::Relaxed);

        tokio::select! {
            _ = tokio::time::sleep(sleep_duration) => {},
            _ = match_state.notify.notified() => {
                eprintln!("[DEBUG] Fetcher waken up by match selection change!");
            }
        }
    }
}
