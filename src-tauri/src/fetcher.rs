use std::time::Duration;
use reqwest::Client;
use tauri::{Emitter, Manager};
use tauri_plugin_positioner::{Position, WindowExt};
use crate::cache::ScoreCache;
use crate::parser::{parse_all_live_indian_matches, parse_match_detail, parse_latest_event};
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

        match client.get("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in")
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let discovered_matches = parse_all_live_indian_matches(&json);
                    
                    // Update active matches list & rebuild tray if changed
                    let mut matches_changed = false;
                    {
                        if let Ok(mut active_m) = match_state.active_matches.lock() {
                            if *active_m != discovered_matches {
                                *active_m = discovered_matches.clone();
                                matches_changed = true;
                            }
                        }
                    }
                    if matches_changed {
                        rebuild_tray_menu(&app_handle, &discovered_matches);
                    }

                    // Determine which match to track
                    let selected = match_state.selected_match.lock().ok().and_then(|s| s.clone());
                    let match_to_track = selected.or_else(|| {
                        discovered_matches.first().map(|(s, m, _)| (s.clone(), m.clone()))
                    });

                    if let Some((series_id, match_id)) = match_to_track {
                        let detail_url = format!(
                            "https://site.api.espn.com/apis/site/v2/sports/cricket/{}/summary?event={}",
                            series_id, match_id
                        );
                        
                        match client.get(&detail_url).send().await {
                            Ok(detail_resp) => {
                                if let Ok(detail_json) = detail_resp.json::<serde_json::Value>().await {
                                    if let Some(score) = parse_match_detail(&detail_json, &series_id, &match_id) {
                                        cache.set(Some(score.clone()));
                                        
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
                                                            tokio::time::sleep(Duration::from_secs(8)).await; // Win popup shows for 8s
                                                            let _ = mini_clone.hide();
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                        
                                        // Detect match events (boundaries, wickets)
                                        if let Some(event) = parse_latest_event(&detail_json, &mut last_ball_id) {
                                            cache.set_latest_event(Some(event.clone()));
                                            // Always emit to both windows — main.js uses it for in-card flash
                                            let _ = app_handle.emit("match-event", &event);
                                            
                                            // Only show mini_popup if main window is hidden
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
                                            MatchStatus::Live => Duration::from_secs(1),
                                            MatchStatus::Break => Duration::from_secs(30),
                                            MatchStatus::Scheduled => Duration::from_secs(300),
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
                    } else {
                        cache.set(None);
                    }
                }
            }
            Err(e) => eprintln!("Error fetching scoreboard header: {}", e),
        }

        tokio::time::sleep(sleep_duration).await;
    }
}

fn rebuild_tray_menu(app_handle: &tauri::AppHandle, matches: &[(String, String, String)]) {
    use tauri::menu::{Menu, MenuItem, Submenu};
    
    let quit_i = match MenuItem::with_id(app_handle, "quit", "Quit", true, None::<&str>) {
        Ok(item) => item,
        Err(_) => return,
    };
    
    let menu = match Menu::new(app_handle) {
        Ok(m) => m,
        Err(_) => return,
    };
    let _ = menu.append(&quit_i);
    
    if !matches.is_empty() {
        if let Ok(submenu) = Submenu::new(app_handle, "Select Match", true) {
            for (series_id, match_id, title) in matches {
                if let Ok(item) = MenuItem::with_id(
                    app_handle,
                    format!("match_{}_{}", series_id, match_id),
                    title,
                    true,
                    None::<&str>,
                ) {
                    let _ = submenu.append(&item);
                }
            }
            let _ = menu.append(&submenu);
        }
    }
    
    if let Some(tray) = app_handle.tray_by_id("main") {
        let _ = tray.set_menu(Some(menu));
    }
}
