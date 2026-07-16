use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri_plugin_positioner::{Position, WindowExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

mod models;
mod cache;
mod parser;
mod fetcher;
mod match_state;


#[tauri::command]
fn get_score(state: tauri::State<'_, cache::ScoreCache>) -> Option<models::MatchScore> {
    state.get()
}

#[tauri::command]
fn get_latest_event(state: tauri::State<'_, cache::ScoreCache>) -> Option<models::MatchEvent> {
    state.get_latest_event()
}

#[tauri::command]
fn hide_mini_popup(app: tauri::AppHandle) {
    if let Some(mini_window) = app.get_webview_window("mini_popup") {
        let _ = mini_window.hide();
    }
}

#[tauri::command]
fn get_discovered_matches(state: tauri::State<'_, match_state::ActiveMatchesState>) -> Vec<(String, String, String, String, String, String, String)> {
    if let Ok(active) = state.active_matches.lock() {
        active.clone()
    } else {
        Vec::new()
    }
}

#[tauri::command]
fn select_match(sport: String, series_id: String, match_id: String, state: tauri::State<'_, match_state::ActiveMatchesState>) {
    if let Ok(mut sel) = state.selected_match.lock() {
        *sel = Some((sport, series_id, match_id));
    }
    state.notify.notify_one();
}

#[tauri::command]
fn untrack_match(state: tauri::State<'_, match_state::ActiveMatchesState>) {
    if let Ok(mut sel) = state.selected_match.lock() {
        *sel = None;
    }
    state.notify.notify_one();
}

#[tauri::command]
fn get_active_match(state: tauri::State<'_, match_state::ActiveMatchesState>) -> Option<(String, String, String)> {
    state.selected_match.lock().ok().and_then(|s| s.clone())
}

#[tauri::command]
fn is_initial_fetch_completed(state: tauri::State<'_, match_state::ActiveMatchesState>) -> bool {
    state.initial_fetch_completed.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cache = cache::ScoreCache::new();
    let match_state = match_state::ActiveMatchesState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    if let Some(window) = app.get_webview_window("main") {
                        let is_visible = window.is_visible().unwrap_or(false);
                        if is_visible {
                            let _ = window.hide();
                        } else {
                            // Position window at bottom-right of screen
                            let _ = window.move_window(Position::BottomRight);
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
            })
            .build())
        .manage(cache.clone())
        .manage(match_state.clone())
        .invoke_handler(tauri::generate_handler![
            get_score,
            get_latest_event,
            hide_mini_popup,
            get_discovered_matches,
            select_match,
            get_active_match,
            untrack_match,
            is_initial_fetch_completed
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "dashboard" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            // Register global shortcut Ctrl+Alt+Space
            if let Ok(shortcut) = "Ctrl+Alt+Space".parse::<Shortcut>() {
                let _ = app.global_shortcut().register(shortcut);
            }

            // Spawn the background fetcher thread
            tauri::async_runtime::spawn(fetcher::start_polling(cache, app.handle().clone(), match_state.clone()));

            // Create tray menu items
            let dash_i = MenuItem::with_id(app, "show_dashboard", "Open Dashboard", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&dash_i, &quit_i])?;

            // Create system tray icon with explicit ID "main"
            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show_dashboard" => {
                            if let Some(dash_win) = app.get_webview_window("dashboard") {
                                let _ = dash_win.show();
                                let _ = dash_win.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let is_visible = window.is_visible().unwrap_or(false);
                            if is_visible {
                                let _ = window.hide();
                            } else {
                                // Position window at bottom-right of screen
                                let _ = window.move_window(Position::BottomRight);
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


