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

    loop {
        let mut sleep_duration = Duration::from_secs(300); // 5 minutes default

        match client.get("https://site.web.api.espn.com/apis/personalized/v2/scoreboard/header?sport=cricket&region=in")
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let discovered_matches = parse_all_live_indian_matches(&json);
                    
                    // Check if live matches list changed
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
                    let selected = {
                        if let Ok(sel) = match_state.selected_match.lock() {
                            sel.clone()
                        } else {
                            None
                        }
                    };

                    let match_to_track = selected.or_else(|| {
                        discovered_matches.first().map(|(series_id, match_id, _)| (series_id.clone(), match_id.clone()))
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
                                        println!("Fetched live score: {:?}", score);
                                        cache.set(Some(score.clone()));
                                        
                                        // Detect and handle new match events
                                        if let Some(event) = parse_latest_event(&detail_json, &mut last_ball_id) {
                                            println!("New match event: {:?}", event);
                                            cache.set_latest_event(Some(event.clone()));
                                            let _ = app_handle.emit("match-event", &event);
                                            if let Some(mini_window) = app_handle.get_webview_window("mini_popup") {
                                                let _ = mini_window.move_window(Position::BottomRight);
                                                let _ = mini_window.show();
                                                
                                                // Auto-hide the mini_popup window after 5 seconds from Rust side
                                                let mini_window_clone = mini_window.clone();
                                                tokio::spawn(async move {
                                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                                    let _ = mini_window_clone.hide();
                                                });
                                            }
                                        }
                                        
                                        sleep_duration = match score.status {
                                            MatchStatus::Live => Duration::from_millis(500),
                                            MatchStatus::Break => Duration::from_secs(30),
                                            MatchStatus::Scheduled => Duration::from_secs(300),
                                            MatchStatus::Completed => Duration::from_secs(300),
                                            MatchStatus::NoMatch => Duration::from_secs(300),
                                        };
                                    } else {
                                        println!("Failed to parse match details.");
                                        cache.set(None);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Error fetching match details: {}", e);
                            }
                        }
                    } else {
                        println!("No live Indian match found.");
                        cache.set(None);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error fetching scoreboard header: {}", e);
            }
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
        if let Ok(select_match_submenu) = Submenu::new(app_handle, "Select Match", true) {
            for (series_id, match_id, match_title) in matches {
                if let Ok(item) = MenuItem::with_id(
                    app_handle,
                    format!("match_{}_{}", series_id, match_id),
                    match_title,
                    true,
                    None::<&str>,
                ) {
                    let _ = select_match_submenu.append(&item);
                }
            }
            let _ = menu.append(&select_match_submenu);
        }
    }
    
    if let Some(tray) = app_handle.tray_by_id("main") {
        let _ = tray.set_menu(Some(menu));
    }
}
