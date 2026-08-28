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
    RegisterHotKey, ReleaseCapture, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW,
    GetCursorPos, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, HMENU, HTCAPTION, IDC_ARROW, MSG,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT,
    WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub mod engine;
pub mod render;
pub mod popup;
pub mod dashboard;
pub mod tray;

use engine::cache::ScoreCache;
use engine::events::{AppEvent, DiscoveredMatch};
use engine::match_state::ActiveMatchesState;
use render::Renderer;
use popup::{MiniPopupWindow, POPUP_W, POPUP_H};
use dashboard::{DashboardRenderer, DashboardSport, DASH_NORMAL_W, DASH_NORMAL_H, HitTarget, WM_APP_SELECT_MATCH, WM_APP_UNTRACK};
use tray::{TrayIcon, WM_APP_TRAY, ID_TRAY_TOGGLE_SCORE, ID_TRAY_OPEN_DASHBOARD, ID_TRAY_UNTRACK_MATCH, ID_TRAY_QUIT};

pub const WM_APP_SCORE_UPDATE: u32 = WM_APP + 1;
pub const WM_APP_MATCH_EVENT: u32 = WM_APP + 3;
pub const WM_APP_MATCHES_DISCOVERED: u32 = WM_APP + 4;

const CLASS_NAME: PCWSTR = w!("SPNativeMain");
const SCORE_W: u32 = 540;
const SCORE_H: u32 = 200;
const HOTKEY_ID: i32 = 1;

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

unsafe fn bottom_right_score_point() -> POINT {
    let wa = work_area();
    POINT {
        x: wa.right - SCORE_W as i32 - 12,
        y: wa.bottom - SCORE_H as i32 - 12,
    }
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

unsafe fn toggle_scoreboard(hwnd: HWND, state: &mut AppState) {
    let visible = IsWindowVisible(hwnd).as_bool();
    if visible {
        let _ = ShowWindow(hwnd, SW_HIDE);
    } else {
        let pos = bottom_right_score_point();
        let score = state.cache.get();
        let _ = state.renderer.present(&pos, &score);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

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
            let _ = ShowWindow(state.dash_hwnd, SW_HIDE);
        }
    } else {
        let pos = center_screen_point(DASH_NORMAL_W, DASH_NORMAL_H);
        if let Some(r) = state.dash_renderer.as_mut() {
            let _ = r.present(&pos, &state.dash_matches, &state.dash_selected_id);
        }
        let _ = ShowWindow(state.dash_hwnd, SW_SHOW);
        let _ = SetForegroundWindow(state.dash_hwnd);
    }
}

/// Full work-area maximize for the custom-rendered dashboard. This deliberately avoids
/// the previous down/up double-toggle and the arbitrary 40px gutter that made maximize
/// look broken. The dashboard preserves its last normal geometry for a faithful restore.
unsafe fn toggle_dashboard_maximize(state: &mut AppState) {
    let hwnd = state.dash_hwnd;
    let restore_rect = state.dashboard_restore_rect;
    let Some(r) = state.dash_renderer.as_mut() else { return; };

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
        let _ = r.present(&POINT { x: wa.left, y: wa.top }, &state.dash_matches, &state.dash_selected_id);
    } else {
        let width = restore_rect.right - restore_rect.left;
        let height = restore_rect.bottom - restore_rect.top;
        r.is_maximized = false;
        let _ = SetWindowPos(hwnd, None, restore_rect.left, restore_rect.top, width, height, SWP_NOACTIVATE);
        let _ = r.resize(width as u32, height as u32);
        let _ = r.present(
            &POINT { x: restore_rect.left, y: restore_rect.top },
            &state.dash_matches,
            &state.dash_selected_id,
        );
    }
}

/// Mirrors standard Windows behavior: pulling a maximized window from its title bar
/// restores its normal size beneath the pointer and immediately continues the drag.
unsafe fn restore_dashboard_for_drag(state: &mut AppState, grab_x: f32, grab_y: f32) {
    let hwnd = state.dash_hwnd;
    let restore = state.dashboard_restore_rect;
    let Some(renderer) = state.dash_renderer.as_mut() else { return; };
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
    let _ = SetWindowPos(hwnd, None, new_left, new_top, normal_width, normal_height, SWP_NOACTIVATE);
    let _ = renderer.resize(normal_width as u32, normal_height as u32);
    let _ = renderer.present(
        &POINT { x: new_left, y: new_top },
        &state.dash_matches,
        &state.dash_selected_id,
    );
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
                let pos = bottom_right_score_point();
                let score = state.cache.get();
                let _ = state.renderer.present(&pos, &score);
            }
            let _ = ValidateRect(hwnd, None);
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
                    let pos = bottom_right_score_point();
                    let _ = state.renderer.present(&pos, &score);
                }
                // Update Tray Tooltip
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
                    let pos = bottom_right_popup_point();
                    if let Some(popup) = state.popup_win.as_mut() {
                        popup.show_event(event, pos);
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_MATCHES_DISCOVERED => {
            if let Some(state) = state_ptr.as_mut() {
                if let Ok(active) = state.match_state.active_matches.lock() {
                    let matches: Vec<DiscoveredMatch> = active.iter().map(|m| DiscoveredMatch {
                        sport: m.0.clone(),
                        series_id: m.1.clone(),
                        match_id: m.2.clone(),
                        title: m.3.clone(),
                        status: m.4.clone(),
                        league_name: m.5.clone(),
                        start_time: m.6.clone(),
                    }).collect();

                    state.dash_matches = matches;
                    if IsWindowVisible(state.dash_hwnd).as_bool() {
                        let mut rect = RECT::default();
                        let _ = GetWindowRect(state.dash_hwnd, &mut rect);
                        let pos = POINT { x: rect.left, y: rect.top };
                        if let Some(r) = state.dash_renderer.as_mut() {
                            let _ = r.present(&pos, &state.dash_matches, &state.dash_selected_id);
                        }
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
                    if let Ok(mut sel) = state.match_state.selected_match.lock() {
                        *sel = Some((m.sport.clone(), m.series_id.clone(), m.match_id.clone()));
                    }
                    state.dash_selected_id = Some(m.match_id);
                    state.match_state.notify.notify_one();

                    // Instantly show scoreboard
                    let pos = bottom_right_score_point();
                    let score = state.cache.get();
                    let _ = state.renderer.present(&pos, &score);
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

                    if IsWindowVisible(state.dash_hwnd).as_bool() {
                        let mut rect = RECT::default();
                        let _ = GetWindowRect(state.dash_hwnd, &mut rect);
                        if let Some(r) = state.dash_renderer.as_mut() {
                            let _ = r.present(
                                &POINT { x: rect.left, y: rect.top },
                                &state.dash_matches,
                                &state.dash_selected_id,
                            );
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_UNTRACK => {
            if let Some(state) = state_ptr.as_mut() {
                if let Ok(mut sel) = state.match_state.selected_match.lock() {
                    *sel = None;
                }
                state.dash_selected_id = None;
                state.cache.set(None);
                state.match_state.notify.notify_one();

                let pos = bottom_right_score_point();
                let _ = state.renderer.present(&pos, &None);

                if IsWindowVisible(state.dash_hwnd).as_bool() {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(state.dash_hwnd, &mut rect);
                    if let Some(r) = state.dash_renderer.as_mut() {
                        let _ = r.present(
                            &POINT { x: rect.left, y: rect.top },
                            &state.dash_matches,
                            &state.dash_selected_id,
                        );
                    }
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
                    ID_TRAY_OPEN_DASHBOARD => toggle_dashboard(state),
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
unsafe extern "system" fn dashboard_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let parent_hwnd = HWND(HMAIN.load(Ordering::Relaxed) as *mut _);
    let state_ptr = GetWindowLongPtrW(parent_hwnd, GWLP_USERDATA) as *mut AppState;

    match msg {
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;

            if let Some(state) = state_ptr.as_mut() {
                let hit = state.dash_renderer.as_ref()
                    .and_then(|r| r.hit_test(x, y, state.dash_matches.len()));
                if matches!(hit, Some(HitTarget::TitleBar)) {
                    restore_dashboard_for_drag(state, x, y);
                    let _ = ReleaseCapture();
                    let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                    return LRESULT(0);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_ptr.as_mut() {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = (lparam.0 >> 16) as i16 as f32;

                if let Some(r) = state.dash_renderer.as_mut() {
                    let hit = r.hit_test(x, y, state.dash_matches.len());
                    let (hover_idx, action_idx, min_h, max_h, close_h) = match hit {
                        Some(HitTarget::MinimizeButton) => (None, None, true, false, false),
                        Some(HitTarget::MaximizeButton) => (None, None, false, true, false),
                        Some(HitTarget::CloseButton) => (None, None, false, false, true),
                        Some(HitTarget::MatchItem(idx)) => (Some(idx), None, false, false, false),
                        Some(HitTarget::MatchAction(idx)) => (Some(idx), Some(idx), false, false, false),
                        _ => (None, None, false, false, false),
                    };

                    if r.hover_index != hover_idx || r.action_hover_index != action_idx || r.min_hover != min_h || r.max_hover != max_h || r.close_hover != close_h {
                        r.set_hover(hover_idx, action_idx, min_h, max_h, close_h);
                        let mut rect = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut rect);
                        let pos = POINT { x: rect.left, y: rect.top };
                        let _ = r.present(&pos, &state.dash_matches, &state.dash_selected_id);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_ptr.as_mut() {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16;
                if let Some(r) = state.dash_renderer.as_mut() {
                    r.scroll_by(if delta < 0 { 1 } else { -1 }, state.dash_matches.len());
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    let _ = r.present(
                        &POINT { x: rect.left, y: rect.top },
                        &state.dash_matches,
                        &state.dash_selected_id,
                    );
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if let Some(state) = state_ptr.as_mut() {
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let pos = POINT { x: rect.left, y: rect.top };
                if let Some(r) = state.dash_renderer.as_mut() {
                    let _ = r.present(&pos, &state.dash_matches, &state.dash_selected_id);
                }
            }
            let _ = ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;

            if let Some(state) = state_ptr.as_mut() {
                if let Some(r) = state.dash_renderer.as_mut() {
                    let hit = r.hit_test(x, y, state.dash_matches.len());
                    match hit {
                        Some(HitTarget::MinimizeButton) => {
                            let _ = ShowWindow(hwnd, SW_MINIMIZE);
                        }
                        Some(HitTarget::MaximizeButton) => {
                            toggle_dashboard_maximize(state);
                        }
                        Some(HitTarget::CloseButton) => {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }
                        Some(HitTarget::CricketTab) | Some(HitTarget::FootballTab) => {
                            let sport = if matches!(hit, Some(HitTarget::CricketTab)) {
                                DashboardSport::Cricket
                            } else {
                                DashboardSport::Football
                            };
                            if r.active_sport != sport {
                                r.active_sport = sport;
                                r.scroll_offset = 0;
                                let mut rect = RECT::default();
                                let _ = GetWindowRect(hwnd, &mut rect);
                                let _ = r.present(
                                    &POINT { x: rect.left, y: rect.top },
                                    &state.dash_matches,
                                    &state.dash_selected_id,
                                );
                            }
                        }
                        Some(HitTarget::MatchAction(idx)) => {
                            if idx < state.dash_matches.len() {
                                let selected = state.dash_selected_id.as_ref()
                                    .is_some_and(|id| id == &state.dash_matches[idx].match_id);
                                if selected {
                                    let _ = PostMessageW(parent_hwnd, WM_APP_UNTRACK, WPARAM(0), LPARAM(0));
                                } else {
                                    let _ = PostMessageW(parent_hwnd, WM_APP_SELECT_MATCH, WPARAM(idx), LPARAM(0));
                                }
                            }
                        }
                        Some(HitTarget::MatchItem(idx)) => {
                            if idx < state.dash_matches.len() {
                                let _ = PostMessageW(parent_hwnd, WM_APP_SELECT_MATCH, WPARAM(idx), LPARAM(0));
                            }
                        }
                        _ => {}
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
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
                            let _ = unsafe { PostMessageW(hwnd, WM_APP_SCORE_UPDATE, WPARAM(0), LPARAM(0)) };
                        }
                        AppEvent::MatchEvent(_) => {
                            let _ = unsafe { PostMessageW(hwnd, WM_APP_MATCH_EVENT, WPARAM(0), LPARAM(0)) };
                        }
                        AppEvent::MatchesDiscovered(_) => {
                            let _ = unsafe { PostMessageW(hwnd, WM_APP_MATCHES_DISCOVERED, WPARAM(0), LPARAM(0)) };
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

        // 7. Initial Scoreboard Paint (starts visible)
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
        if let Some(state) = state_ptr.as_mut() {
            let _ = state.renderer.present(&score_pos, &None);
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

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
