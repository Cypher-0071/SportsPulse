use std::time::Duration;
use reqwest::Client;
use tauri::{Emitter, Manager};
use tauri_plugin_positioner::{Position, WindowExt};
use crate::cache::ScoreCache;
use crate::parser::{parse_live_indian_match, parse_match_detail, parse_latest_event};
use crate::models::MatchStatus;

pub async fn start_polling(cache: ScoreCache, app_handle: tauri::AppHandle) {
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
                    if let Some((series_id, match_id)) = parse_live_indian_match(&json) {
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
                                            let _ = app_handle.emit("match-event", &event);
                                            if let Some(mini_window) = app_handle.get_webview_window("mini_popup") {
                                                let _ = mini_window.move_window(Position::BottomRight);
                                                let _ = mini_window.show();
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
