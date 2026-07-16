use std::time::Duration;
use reqwest::Client;
use tauri::{Emitter, Manager};
use tauri_plugin_positioner::{Position, WindowExt};
use crate::cache::ScoreCache;
use crate::parser::{
    parse_all_live_indian_matches, parse_match_detail, parse_latest_event,
    parse_soccer_matches, parse_soccer_match_detail, parse_soccer_latest_event
};
use crate::models::MatchStatus;
use crate::match_state::ActiveMatchesState;

pub async fn start_polling(cache: ScoreCache, app_handle: tauri::AppHandle, match_state: ActiveMatchesState) {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("Accept", "application/json".parse().unwrap());
    headers.insert("Accept-Language", "en-US,en;q=0.9".parse().unwrap());
    headers.insert("Referer", "https://www.espn.com/cricket/".parse().unwrap());

    let client = Client::builder()
        .tcp_nodelay(true)
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| Client::new());

    let mut last_ball_id: Option<String> = None;
    let mut last_tracked_match_id: Option<String> = None;
    let mut last_completed_match_id: Option<String> = None;

    loop {
        let mut sleep_duration = Duration::from_secs(300);
        let mut discovered_matches = Vec::new();

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
            discovered_matches.push(("cricket".to_string(), series_id, match_id, title, status, league_name, start_time));
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
            discovered_matches.push(("soccer".to_string(), series_id, match_id, title, status, league_name, start_time));
        }

        // Update active matches list
        if let Ok(mut active_m) = match_state.active_matches.lock() {
            if *active_m != discovered_matches {
                *active_m = discovered_matches.clone();
            }
        }

        // Determine which match to track
        // Determine which match to track
        let match_to_track = match_state.selected_match.lock().ok().and_then(|s| s.clone());

        if let Some((sport, series_id, match_id)) = match_to_track {
            let is_already_completed = last_completed_match_id.as_ref() == Some(&match_id);

            if is_already_completed {
                // Already fetched completed match final score and cached it. No need to poll again.
                sleep_duration = Duration::from_secs(300);
            } else {
                let detail_url = format!(
                    "https://site.api.espn.com/apis/site/v2/sports/{}/{}/summary?event={}",
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

                                // Dynamically resize main window depending on the sport
                                if let Some(main_win) = app_handle.get_webview_window("main") {
                                    let size = if score.sport == crate::models::SportType::Soccer {
                                        tauri::LogicalSize::new(260.0, 40.0)
                                    } else if score.status == MatchStatus::Scheduled || score.status == MatchStatus::Completed {
                                        tauri::LogicalSize::new(340.0, 90.0)
                                    } else {
                                        tauri::LogicalSize::new(340.0, 110.0)
                                    };
                                    let _ = main_win.set_size(tauri::Size::Logical(size));
                                    let _ = main_win.move_window(Position::BottomRight);
                                }

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
                                        use crate::models::{MatchEvent, MatchEventType};
                                        let win_event = MatchEvent {
                                            event_type: MatchEventType::Win,
                                            title: "MATCH WON!".to_string(),
                                            description: format!("{} won the match!", w_name),
                                            score: format!("{} vs {}", score.team1.abbreviation, score.team2.abbreviation),
                                            sport: sport.clone(),
                                        };
                                        cache.set_latest_event(Some(win_event.clone()));
                                        let _ = app_handle.emit("match-event", &win_event);

                                        let main_visible = app_handle
                                            .get_webview_window("main")
                                            .and_then(|w| w.is_visible().ok())
                                            .unwrap_or(false);

                                        if !main_visible {
                                            if let Some(mini_window) = app_handle.get_webview_window("mini_popup") {
                                                let _ = mini_window.move_window(Position::BottomRight);
                                                let _ = mini_window.show();
                                                let _ = mini_window.set_focus();
                                                let mini_clone = mini_window.clone();
                                                tokio::spawn(async move {
                                                    tokio::time::sleep(Duration::from_secs(8)).await;
                                                    let _ = mini_clone.hide();
                                                });
                                            }
                                        }
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
                                    let _ = app_handle.emit("match-event", &event);

                                    let main_visible = app_handle
                                        .get_webview_window("main")
                                        .and_then(|w| w.is_visible().ok())
                                        .unwrap_or(false);

                                    if !main_visible {
                                        if let Some(mini_window) = app_handle.get_webview_window("mini_popup") {
                                            let _ = mini_window.move_window(Position::BottomRight);
                                            let _ = mini_window.show();
                                            let _ = mini_window.set_focus();
                                            let mini_clone = mini_window.clone();
                                            tokio::spawn(async move {
                                                tokio::time::sleep(Duration::from_secs(5)).await;
                                                let _ = mini_clone.hide();
                                            });
                                        }
                                    }
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
            if let Some(main_win) = app_handle.get_webview_window("main") {
                let _ = main_win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(340.0, 110.0)));
                let _ = main_win.move_window(Position::BottomRight);
            }
        }

        match_state.initial_fetch_completed.store(true, std::sync::atomic::Ordering::Relaxed);

        tokio::select! {
            _ = tokio::time::sleep(sleep_duration) => {},
            _ = match_state.notify.notified() => {
                eprintln!("[DEBUG] Fetcher waken up by match selection change!");
            }
        }
    }
}


