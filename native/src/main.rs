//! SportsPulse — Pure Native Win32 Rewrite (windows-rs).
//! Ultra-lightweight system tray cricket & soccer scoreboard overlay with Direct2D/DirectWrite.

#![windows_subsystem = "windows"]
#![cfg(windows)]
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::*;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, ReleaseCapture, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
    VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, KillTimer, LoadCursorW,
    PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, HMENU, HTCAPTION, IDC_ARROW, MSG,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCLBUTTONDOWN, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub mod dashboard;
pub mod engine;
pub mod popup;
pub mod render;
pub mod tray;

use dashboard::{
    DashboardRenderer, DashboardSport, HitTarget, DASH_NORMAL_H, DASH_NORMAL_W,
    WM_APP_SELECT_MATCH, WM_APP_UNTRACK,
};
use engine::cache::ScoreCache;
use engine::events::{AppEvent, DiscoveredMatch};
use engine::match_state::ActiveMatchesState;
use engine::models::{MatchEventType, MatchScore, MatchStatus, SportType};
use popup::{MiniPopupWindow, POPUP_H, POPUP_W};
use render::Renderer;
use tray::{
    TrayIcon, ID_TRAY_OPEN_DASHBOARD, ID_TRAY_QUIT, ID_TRAY_TOGGLE_SCORE, ID_TRAY_UNTRACK_MATCH,
    WM_APP_TRAY,
};

pub const WM_APP_SCORE_UPDATE: u32 = WM_APP + 1;
pub const WM_APP_MATCH_EVENT: u32 = WM_APP + 3;
pub const WM_APP_MATCHES_DISCOVERED: u32 = WM_APP + 4;

const CLASS_NAME: PCWSTR = w!("SPNativeMain");
const SCORE_W: u32 = 540;
const SCORE_H: u32 = 200;
const SCORE_SOCCER_H: u32 = 96;
const SCORE_CRICKET_COMPACT_H: u32 = 140;
const HOTKEY_ID: i32 = 1;
const DASH_LOADER_TIMER: usize = 1;
const OVERLAY_FLASH_TIMER: usize = 2;

static HMAIN: AtomicUsize = AtomicUsize::new(0);

struct AppState {
    renderer: Renderer,
    tray: TrayIcon,
    popup_win: Option<MiniPopupWindow>,
    dash_hwnd: HWND,
    dash_renderer: Option<DashboardRenderer>,
    dash_matches: Vec<DiscoveredMatch>,
    dash_selected_id: Option<String>,
    cache: ScoreCache,
    match_state: ActiveMatchesState,
    dashboard_restore_rect: RECT,
}

unsafe fn work_area() -> RECT {
    let mut wa = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut wa as *mut RECT as *mut core::ffi::c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    wa
}

fn overlay_size_for(score: &Option<MatchScore>) -> (u32, u32) {
    match score {
        Some(s) if s.status != MatchStatus::NoMatch => match s.sport {
            SportType::Soccer => (SCORE_W, SCORE_SOCCER_H),
            SportType::Cricket => match s.status {
                MatchStatus::Live | MatchStatus::Break => (SCORE_W, SCORE_H),
                _ => (SCORE_W, SCORE_CRICKET_COMPACT_H),
            },
        },
        _ => (SCORE_W, SCORE_H),
    }
}

unsafe fn overlay_pos(w: u32, h: u32) -> POINT {
    let wa = work_area();
    POINT {
        x: wa.right - w as i32 - 12,
        y: wa.bottom - h as i32 - 12,
    }
}

unsafe fn bottom_right_score_point() -> POINT {
    overlay_pos(SCORE_W, SCORE_H)
}

/// Resize, pin to the work-area bottom-right (12px inset), and present the overlay.
unsafe fn place_and_present_overlay(hwnd: HWND, state: &mut AppState, score: &Option<MatchScore>) {
    let (w, h) = overlay_size_for(score);
    let pos = overlay_pos(w, h);
    let _ = SetWindowPos(
        hwnd,
        None,
        pos.x,
        pos.y,
        w as i32,
        h as i32,
        SWP_NOACTIVATE | SWP_NOZORDER,
    );
    let _ = state.renderer.resize(w, h);
    let _ = state.renderer.present(&pos, score);
}

unsafe fn present_overlay_now(hwnd: HWND, state: &mut AppState) {
    let score = state.cache.get();
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let pos = POINT {
        x: rect.left,
        y: rect.top,
    };
    let _ = state.renderer.present(&pos, &score);
}

unsafe fn hide_overlay(hwnd: HWND, state: &mut AppState) {
    let _ = KillTimer(hwnd, OVERLAY_FLASH_TIMER);
    state.renderer.set_event_flash(None);
    if let Some(popup) = state.popup_win.as_mut() {
        popup.hide();
    }
    let _ = ShowWindow(hwnd, SW_HIDE);
}

unsafe fn bottom_right_popup_point() -> POINT {
    let wa = work_area();
    POINT {
        x: wa.right - POPUP_W as i32 - 12,
        y: wa.bottom - SCORE_H as i32 - POPUP_H as i32 - 20,
    }
}

unsafe fn center_screen_point(w: u32, h: u32) -> POINT {
    let wa = work_area();
    POINT {
        x: wa.left + (wa.right - wa.left - w as i32) / 2,
        y: wa.top + (wa.bottom - wa.top - h as i32) / 2,
    }
}

unsafe fn show_overlay(hwnd: HWND, state: &mut AppState, score: &Option<MatchScore>) {
    if let Some(popup) = state.popup_win.as_mut() {
        popup.hide();
    }
    place_and_present_overlay(hwnd, state, score);
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
}

unsafe fn toggle_scoreboard(hwnd: HWND, state: &mut AppState) {
    let visible = IsWindowVisible(hwnd).as_bool();
    if visible {
        hide_overlay(hwnd, state);
    } else {
        let score = state.cache.get();
        show_overlay(hwnd, state, &score);
    }
}

/// Overlay-click toggle. May hide the dashboard. Tray "Open Dashboard" must never hide.
unsafe fn toggle_dashboard(state: &mut AppState) {
    if state.dash_hwnd.0.is_null() {
        return;
    }
    let visible = IsWindowVisible(state.dash_hwnd).as_bool();
    if visible {
        if IsIconic(state.dash_hwnd).as_bool() {
            let _ = ShowWindow(state.dash_hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(state.dash_hwnd);
        } else {
            let _ = KillTimer(state.dash_hwnd, DASH_LOADER_TIMER);
            let _ = ShowWindow(state.dash_hwnd, SW_HIDE);
        }
    } else {
        show_dashboard(state);
    }
}

/// Tray "Open Dashboard": always show + focus. Never hide.
unsafe fn show_dashboard(state: &mut AppState) {
    if state.dash_hwnd.0.is_null() {
        return;
    }
    if IsIconic(state.dash_hwnd).as_bool() {
        let _ = ShowWindow(state.dash_hwnd, SW_RESTORE);
    } else if !IsWindowVisible(state.dash_hwnd).as_bool() {
        present_dashboard_hwnd(state, state.dash_hwnd);
        let _ = ShowWindow(state.dash_hwnd, SW_SHOW);
    }
    let _ = SetForegroundWindow(state.dash_hwnd);
}

/// Full work-area maximize for the custom-rendered dashboard. This deliberately avoids
/// the previous down/up double-toggle and the arbitrary 40px gutter that made maximize
/// look broken. The dashboard preserves its last normal geometry for a faithful restore.
unsafe fn toggle_dashboard_maximize(state: &mut AppState) {
    let hwnd = state.dash_hwnd;
    let restore_rect = state.dashboard_restore_rect;
    let pos = {
        let Some(r) = state.dash_renderer.as_mut() else {
            return;
        };

        if !r.is_maximized {
            let mut current = RECT::default();
            let _ = GetWindowRect(hwnd, &mut current);
            state.dashboard_restore_rect = current;

            let wa = work_area();
            let width = wa.right - wa.left;
            let height = wa.bottom - wa.top;
            r.is_maximized = true;
            let _ = SetWindowPos(hwnd, None, wa.left, wa.top, width, height, SWP_NOACTIVATE);
            let _ = r.resize(width as u32, height as u32);
            POINT {
                x: wa.left,
                y: wa.top,
            }
        } else {
            let width = restore_rect.right - restore_rect.left;
            let height = restore_rect.bottom - restore_rect.top;
            r.is_maximized = false;
            let _ = SetWindowPos(
                hwnd,
                None,
                restore_rect.left,
                restore_rect.top,
                width,
                height,
                SWP_NOACTIVATE,
            );
            let _ = r.resize(width as u32, height as u32);
            POINT {
                x: restore_rect.left,
                y: restore_rect.top,
            }
        }
    };
    present_dashboard(state, pos);
}

/// Mirrors standard Windows behavior: pulling a maximized window from its title bar
/// restores its normal size beneath the pointer and immediately continues the drag.
unsafe fn dashboard_is_loading(state: &AppState) -> bool {
    !state
        .match_state
        .initial_fetch_completed
        .load(Ordering::Relaxed)
}

unsafe fn present_dashboard(state: &mut AppState, pos: POINT) {
    let loading = dashboard_is_loading(state);
    if let Some(r) = state.dash_renderer.as_mut() {
        let _ = r.present(&pos, &state.dash_matches, &state.dash_selected_id, loading);
    }
    if loading {
        let _ = SetTimer(state.dash_hwnd, DASH_LOADER_TIMER, 33, None);
    } else {
        let _ = KillTimer(state.dash_hwnd, DASH_LOADER_TIMER);
    }
}

unsafe fn present_dashboard_hwnd(state: &mut AppState, hwnd: HWND) {
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    present_dashboard(
        state,
        POINT {
            x: rect.left,
            y: rect.top,
        },
    );
}

unsafe fn restore_dashboard_for_drag(state: &mut AppState, grab_x: f32, grab_y: f32) {
    let hwnd = state.dash_hwnd;
    let restore = state.dashboard_restore_rect;
    let pos = {
        let Some(renderer) = state.dash_renderer.as_mut() else {
            return;
        };
        if !renderer.is_maximized {
            return;
        }

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let mut maximized = RECT::default();
        let _ = GetWindowRect(hwnd, &mut maximized);
        let max_width = (maximized.right - maximized.left).max(1) as f32;
        let normal_width = (restore.right - restore.left).max(1);
        let normal_height = (restore.bottom - restore.top).max(1);
        let pointer_ratio = (grab_x / max_width).clamp(0.12, 0.88);
        let new_left = cursor.x - (normal_width as f32 * pointer_ratio).round() as i32;
        let new_top = cursor.y - grab_y.round().clamp(12.0, 32.0) as i32;

        renderer.is_maximized = false;
        let _ = SetWindowPos(
            hwnd,
            None,
            new_left,
            new_top,
            normal_width,
            normal_height,
            SWP_NOACTIVATE,
        );
        let _ = renderer.resize(normal_width as u32, normal_height as u32);
        POINT {
            x: new_left,
            y: new_top,
        }
    };
    present_dashboard(state, pos);
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;

    match msg {
        WM_LBUTTONUP => {
            if let Some(state) = state_ptr.as_mut() {
                toggle_dashboard(state);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if let Some(state) = state_ptr.as_mut() {
                present_overlay_now(hwnd, state);
            }
            let _ = ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == OVERLAY_FLASH_TIMER {
                let _ = KillTimer(hwnd, OVERLAY_FLASH_TIMER);
                if let Some(state) = state_ptr.as_mut() {
                    if IsWindowVisible(hwnd).as_bool() {
                        present_overlay_now(hwnd, state);
                    }
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 == HOTKEY_ID as usize {
                if let Some(state) = state_ptr.as_mut() {
                    toggle_scoreboard(hwnd, state);
                }
            }
            LRESULT(0)
        }
        WM_APP_SCORE_UPDATE => {
            if let Some(state) = state_ptr.as_mut() {
                let score = state.cache.get();
                if IsWindowVisible(hwnd).as_bool() {
                    place_and_present_overlay(hwnd, state, &score);
                }
                if let Some(s) = &score {
                    let tip = format!("{} - {}", s.match_title, s.team1.score);
                    state.tray.update_tooltip(&tip);
                }
            }
            LRESULT(0)
        }
        WM_APP_MATCH_EVENT => {
            if let Some(state) = state_ptr.as_mut() {
                if let Some(event) = state.cache.get_latest_event() {
                    if IsWindowVisible(hwnd).as_bool() {
                        if let Some(popup) = state.popup_win.as_mut() {
                            popup.hide();
                        }
                        state.renderer.set_event_flash(Some(event.clone()));
                        present_overlay_now(hwnd, state);
                        let timeout_ms: u32 = if event.event_type == MatchEventType::Win {
                            8000
                        } else {
                            3000
                        };
                        let _ = SetTimer(hwnd, OVERLAY_FLASH_TIMER, timeout_ms, None);
                    } else {
                        let pos = bottom_right_popup_point();
                        if let Some(popup) = state.popup_win.as_mut() {
                            popup.show_event(event, pos);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_MATCHES_DISCOVERED => {
            if let Some(state) = state_ptr.as_mut() {
                let matches = state.match_state.active_matches.lock().ok().map(|active| {
                    active
                        .iter()
                        .map(|m| DiscoveredMatch {
                            sport: m.0.clone(),
                            series_id: m.1.clone(),
                            match_id: m.2.clone(),
                            title: m.3.clone(),
                            status: m.4.clone(),
                            league_name: m.5.clone(),
                            start_time: m.6.clone(),
                        })
                        .collect::<Vec<_>>()
                });
                if let Some(matches) = matches {
                    state.dash_matches = matches;
                    if IsWindowVisible(state.dash_hwnd).as_bool() {
                        present_dashboard_hwnd(state, state.dash_hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_SELECT_MATCH => {
            let idx = wparam.0;
            if let Some(state) = state_ptr.as_mut() {
                if idx < state.dash_matches.len() {
                    let m = state.dash_matches[idx].clone();
                    state.cache.clear();
                    state.renderer.set_event_flash(None);
                    state.match_state.select_match(
                        m.sport.clone(),
                        m.series_id.clone(),
                        m.match_id.clone(),
                    );
                    state.dash_selected_id = Some(m.match_id);

                    show_overlay(hwnd, state, &None);

                    if IsWindowVisible(state.dash_hwnd).as_bool() {
                        present_dashboard_hwnd(state, state.dash_hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_UNTRACK => {
            if let Some(state) = state_ptr.as_mut() {
                state.match_state.untrack_match();
                state.dash_selected_id = None;
                state.cache.clear();
                hide_overlay(hwnd, state);

                if IsWindowVisible(state.dash_hwnd).as_bool() {
                    present_dashboard_hwnd(state, state.dash_hwnd);
                }
            }
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let event = lparam.0 as u32;
            match event {
                windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP => {
                    if let Some(state) = state_ptr.as_mut() {
                        toggle_scoreboard(hwnd, state);
                    }
                }
                windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP => {
                    if let Some(state) = state_ptr.as_mut() {
                        state.tray.show_context_menu();
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            if let Some(state) = state_ptr.as_mut() {
                match id {
                    ID_TRAY_TOGGLE_SCORE => toggle_scoreboard(hwnd, state),
                    ID_TRAY_OPEN_DASHBOARD => show_dashboard(state),
                    ID_TRAY_UNTRACK_MATCH => {
                        let _ = PostMessageW(hwnd, WM_APP_UNTRACK, WPARAM(0), LPARAM(0));
                    }
                    ID_TRAY_QUIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// Dashboard Window Procedure
unsafe extern "system" fn dashboard_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let parent_hwnd = HWND(HMAIN.load(Ordering::Relaxed) as *mut _);
    let state_ptr = GetWindowLongPtrW(parent_hwnd, GWLP_USERDATA) as *mut AppState;

    match msg {
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;

            if let Some(state) = state_ptr.as_mut() {
                let hit = state
                    .dash_renderer
                    .as_ref()
                    .and_then(|r| r.hit_test(x, y, state.dash_matches.len()));
                if matches!(hit, Some(HitTarget::TitleBar)) {
                    restore_dashboard_for_drag(state, x, y);
                    let _ = ReleaseCapture();
                    let _ = SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        WPARAM(HTCAPTION as usize),
                        LPARAM(0),
                    );
                    return LRESULT(0);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_ptr.as_mut() {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = (lparam.0 >> 16) as i16 as f32;

                let hover_changed = if let Some(r) = state.dash_renderer.as_mut() {
                    let hit = r.hit_test(x, y, state.dash_matches.len());
                    let (hover_idx, action_idx, min_h, max_h, close_h, cricket_h, football_h) =
                        match hit {
                            Some(HitTarget::MinimizeButton) => {
                                (None, None, true, false, false, false, false)
                            }
                            Some(HitTarget::MaximizeButton) => {
                                (None, None, false, true, false, false, false)
                            }
                            Some(HitTarget::CloseButton) => {
                                (None, None, false, false, true, false, false)
                            }
                            Some(HitTarget::CricketTab) => {
                                (None, None, false, false, false, true, false)
                            }
                            Some(HitTarget::FootballTab) => {
                                (None, None, false, false, false, false, true)
                            }
                            Some(HitTarget::MatchItem(idx)) => {
                                (Some(idx), None, false, false, false, false, false)
                            }
                            Some(HitTarget::MatchAction(idx)) => {
                                (Some(idx), Some(idx), false, false, false, false, false)
                            }
                            _ => (None, None, false, false, false, false, false),
                        };

                    let changed = r.hover_index != hover_idx
                        || r.action_hover_index != action_idx
                        || r.min_hover != min_h
                        || r.max_hover != max_h
                        || r.close_hover != close_h
                        || r.cricket_hover != cricket_h
                        || r.football_hover != football_h;
                    if changed {
                        r.set_hover(
                            hover_idx, action_idx, min_h, max_h, close_h, cricket_h, football_h,
                        );
                    }
                    changed
                } else {
                    false
                };
                if hover_changed {
                    present_dashboard_hwnd(state, hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_ptr.as_mut() {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16;
                if let Some(r) = state.dash_renderer.as_mut() {
                    r.scroll_by(if delta < 0 { 1 } else { -1 }, state.dash_matches.len());
                }
                present_dashboard_hwnd(state, hwnd);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if let Some(state) = state_ptr.as_mut() {
                present_dashboard_hwnd(state, hwnd);
            }
            let _ = ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == DASH_LOADER_TIMER {
                if let Some(state) = state_ptr.as_mut() {
                    if dashboard_is_loading(state) {
                        present_dashboard_hwnd(state, hwnd);
                    } else {
                        let _ = KillTimer(hwnd, DASH_LOADER_TIMER);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;

            if let Some(state) = state_ptr.as_mut() {
                let hit = state
                    .dash_renderer
                    .as_ref()
                    .map(|r| r.hit_test(x, y, state.dash_matches.len()));
                match hit {
                    Some(Some(HitTarget::MinimizeButton)) => {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    Some(Some(HitTarget::MaximizeButton)) => {
                        toggle_dashboard_maximize(state);
                    }
                    Some(Some(HitTarget::CloseButton)) => {
                        let _ = KillTimer(hwnd, DASH_LOADER_TIMER);
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    Some(Some(HitTarget::CricketTab)) | Some(Some(HitTarget::FootballTab)) => {
                        let sport = if matches!(hit, Some(Some(HitTarget::CricketTab))) {
                            DashboardSport::Cricket
                        } else {
                            DashboardSport::Football
                        };
                        let changed = if let Some(r) = state.dash_renderer.as_mut() {
                            if r.active_sport != sport {
                                r.active_sport = sport;
                                r.scroll_offset = 0;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if changed {
                            present_dashboard_hwnd(state, hwnd);
                        }
                    }
                    Some(Some(HitTarget::MatchAction(idx))) => {
                        if idx < state.dash_matches.len() {
                            let selected = state
                                .dash_selected_id
                                .as_ref()
                                .is_some_and(|id| id == &state.dash_matches[idx].match_id);
                            if selected {
                                let _ =
                                    PostMessageW(parent_hwnd, WM_APP_UNTRACK, WPARAM(0), LPARAM(0));
                            } else {
                                let _ = PostMessageW(
                                    parent_hwnd,
                                    WM_APP_SELECT_MATCH,
                                    WPARAM(idx),
                                    LPARAM(0),
                                );
                            }
                        }
                    }
                    Some(Some(HitTarget::MatchItem(_))) => {
                        // Card body is hover-only. Track / untrack is the action button.
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = KillTimer(hwnd, DASH_LOADER_TIMER);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn spawn_engine_worker(
    cache: ScoreCache,
    match_state: ActiveMatchesState,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime build");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

        // Spawn fetcher loop
        let c = cache.clone();
        let ms = match_state.clone();
        rt.spawn(async move {
            engine::fetcher::start_polling(c, ms, tx).await;
        });

        // Event listener bridge to Win32 messages
        rt.block_on(async move {
            while let Some(ev) = rx.recv().await {
                let h = HMAIN.load(Ordering::Relaxed);
                if h != 0 {
                    let hwnd = HWND(h as *mut _);
                    match ev {
                        AppEvent::ScoreChanged(_) => {
                            let _ = unsafe {
                                PostMessageW(hwnd, WM_APP_SCORE_UPDATE, WPARAM(0), LPARAM(0))
                            };
                        }
                        AppEvent::MatchEvent(_) => {
                            let _ = unsafe {
                                PostMessageW(hwnd, WM_APP_MATCH_EVENT, WPARAM(0), LPARAM(0))
                            };
                        }
                        AppEvent::MatchesDiscovered(_) => {
                            let _ = unsafe {
                                PostMessageW(hwnd, WM_APP_MATCHES_DISCOVERED, WPARAM(0), LPARAM(0))
                            };
                        }
                    }
                }
            }
        });
    })
}

fn main() {
    unsafe {
        // COM STA Initialization
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );

        // DPI Awareness Context (PerMonitorV2)
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = GetModuleHandleW(None).expect("module handle");

        // 1. Register Main Scoreboard Window Class
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        // 2. Register Dashboard Window Class
        const DASH_CLASS: PCWSTR = w!("SPNativeDashboard");
        let wc_dash = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dashboard_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            lpszClassName: DASH_CLASS,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc_dash);

        // 3. Create Main Scoreboard Layered Window
        let score_pos = bottom_right_score_point();
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("SportsPulse"),
            WS_POPUP,
            score_pos.x,
            score_pos.y,
            SCORE_W as i32,
            SCORE_H as i32,
            None,
            HMENU::default(),
            hinstance,
            None,
        )
        .expect("create main window");

        HMAIN.store(hwnd.0 as usize, Ordering::Release);

        // 4. Create Dashboard Window
        let dash_pos = center_screen_point(DASH_NORMAL_W, DASH_NORMAL_H);
        let dash_hwnd = CreateWindowExW(
            // The discovery dashboard is a normal taskbar application while open.
            // The lightweight scoreboard itself remains a tray-only topmost overlay.
            WS_EX_LAYERED | WS_EX_APPWINDOW,
            DASH_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            dash_pos.x,
            dash_pos.y,
            DASH_NORMAL_W as i32,
            DASH_NORMAL_H as i32,
            None,
            HMENU::default(),
            hinstance,
            None,
        )
        .expect("create dashboard window");

        // 5. Create Components
        let renderer = Renderer::new(hwnd, SCORE_W, SCORE_H).expect("d2d scoreboard renderer");
        let dash_renderer = DashboardRenderer::new(dash_hwnd, DASH_NORMAL_W, DASH_NORMAL_H).ok();
        let popup_win = MiniPopupWindow::create().ok();
        let tray = TrayIcon::new(hwnd, "SportsPulse - Live Scores");

        let cache = ScoreCache::new();
        let match_state = ActiveMatchesState::new();

        let app_state = Box::new(AppState {
            renderer,
            tray,
            popup_win,
            dash_hwnd,
            dash_renderer,
            dash_matches: Vec::new(),
            dash_selected_id: None,
            cache: cache.clone(),
            match_state: match_state.clone(),
            dashboard_restore_rect: RECT {
                left: dash_pos.x,
                top: dash_pos.y,
                right: dash_pos.x + DASH_NORMAL_W as i32,
                bottom: dash_pos.y + DASH_NORMAL_H as i32,
            },
        });

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app_state) as isize);

        // 6. Register Global Hotkey Ctrl + Alt + Space
        let _ = RegisterHotKey(
            hwnd,
            HOTKEY_ID,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0),
            VK_SPACE.0 as u32,
        );

        // 7. Start with the dashboard visible (original Tauri: overlay hidden, dashboard shown).
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
        if let Some(state) = state_ptr.as_mut() {
            present_dashboard(state, dash_pos);
        }
        let _ = ShowWindow(dash_hwnd, SW_SHOW);
        let _ = SetForegroundWindow(dash_hwnd);

        // 8. Spawn Tokio Engine
        let engine_handle = spawn_engine_worker(cache, match_state);

        // 9. Main Win32 Message Pump
        let mut msg = MSG::default();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                break;
            }
        }

        HMAIN.store(0, Ordering::Release);
        drop(engine_handle);
    }
}
