//! SportsPulse — Pure Native Win32 Rewrite (windows-rs).
//! Ultra-lightweight system tray cricket & soccer scoreboard overlay with Direct2D/DirectWrite.

#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(non_snake_case)]

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(windows)]
use windows::core::*;
#[cfg(windows)]
use windows::Win32::Foundation::{
    GetLastError, D2DERR_RECREATE_TARGET, HWND, LPARAM, LRESULT, POINT, RECT, RPC_E_CHANGED_MODE,
    S_FALSE, S_OK, WPARAM,
};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MonitorFromWindow, ValidateRect, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
#[cfg(windows)]
use windows::Win32::System::Com::CoUninitialize;
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentProcess;
#[cfg(windows)]
use windows::Win32::System::Threading::{CreateMutexW, ExitProcess};
#[cfg(windows)]
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

#[cfg(windows)]
pub fn log_live_benchmark_sample(state_label: &str) {
    // Gated like render::dbglog: debug builds log to C:\sp_bench\, release
    // builds never touch C:\ unless explicitly opted in via SP_DEBUG=1.
    // Unconditional C:\ writes fail under standard-user ACLs and pollute prod.
    #[cfg(debug_assertions)]
    {
        unsafe {
            let mut pmc = PROCESS_MEMORY_COUNTERS_EX {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                ..Default::default()
            };
            let _ = GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut _, pmc.cb);
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("C:\\sp_bench\\live_benchmark_verified.log")
            {
                let priv_mb = pmc.PrivateUsage as f64 / (1024.0 * 1024.0);
                let ws_mb = pmc.WorkingSetSize as f64 / (1024.0 * 1024.0);
                let peak_mb = pmc.PeakWorkingSetSize as f64 / (1024.0 * 1024.0);
                let _ = writeln!(
                    f,
                    "STATE: {} | PrivateCommit: {} bytes ({:.2} MB) | WorkingSet: {} bytes ({:.2} MB) | PeakWS: {:.2} MB",
                    state_label, pmc.PrivateUsage, priv_mb, pmc.WorkingSetSize, ws_mb, peak_mb
                );
            }
        }
    }
    #[cfg(not(debug_assertions))]
    {
        // Opt-in only: SP_DEBUG=1 re-enables the C:\sp_bench\ sample for
        // field diagnostics. Default release is a strict no-op.
        if std::env::var_os("SP_DEBUG").is_some() {
            unsafe {
                let mut pmc = PROCESS_MEMORY_COUNTERS_EX::default();
                pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                let _ =
                    GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut _, pmc.cb);
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("C:\\sp_bench\\live_benchmark_verified.log")
                {
                    let priv_mb = pmc.PrivateUsage as f64 / (1024.0 * 1024.0);
                    let ws_mb = pmc.WorkingSetSize as f64 / (1024.0 * 1024.0);
                    let peak_mb = pmc.PeakWorkingSetSize as f64 / (1024.0 * 1024.0);
                    let _ = writeln!(
                        f,
                        "STATE: {} | PrivateCommit: {} bytes ({:.2} MB) | WorkingSet: {} bytes ({:.2} MB) | PeakWS: {:.2} MB",
                        state_label, pmc.PrivateUsage, priv_mb, pmc.WorkingSetSize, ws_mb, peak_mb
                    );
                }
            }
        } else {
            let _ = state_label;
        }
    }
}
#[cfg(windows)]
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, ReleaseCapture, SetCapture, TrackMouseEvent, UnregisterHotKey,
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, TME_LEAVE, TRACKMOUSEEVENT, VK_SPACE,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, KillTimer, LoadCursorW,
    MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    SystemParametersInfoW, TranslateMessage, CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, HCURSOR, HMENU, HTCAPTION, ICON_BIG, ICON_SMALL, IDC_ARROW, MB_ICONERROR, MB_OK,
    MINMAXINFO, MSG, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_MINIMIZE,
    SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_EXITSIZEMOVE,
    WM_GETMINMAXINFO, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_PAINT,
    WM_RBUTTONUP, WM_SETICON, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

#[cfg(windows)]
use sportspulse::dashboard::{
    DashboardRenderer, DashboardSport, HitTarget, DASH_NORMAL_H, DASH_NORMAL_W, HOTKEY_CONFLICT,
    WM_APP_SELECT_MATCH, WM_APP_UNTRACK,
};
#[cfg(windows)]
use sportspulse::engine::cache::ScoreCache;
#[cfg(windows)]
use sportspulse::engine::events::{AppEvent, DiscoveredMatch};
#[cfg(windows)]
use sportspulse::engine::match_state::ActiveMatchesState;
#[cfg(windows)]
use sportspulse::engine::models::{MatchEventType, MatchScore, MatchStatus, SportType};
#[cfg(windows)]
use sportspulse::popup::{MiniPopupWindow, POPUP_H, POPUP_W};
#[cfg(windows)]
use sportspulse::render::{reduced_motion, ui_text, Renderer, UI_SCALE, EVENT_TTL, WIN_TTL};
#[cfg(windows)]
use sportspulse::tray::{
    load_app_icon, TrayIcon, ID_TRAY_OPEN_DASHBOARD, ID_TRAY_QUIT, ID_TRAY_TOGGLE_SCORE,
    ID_TRAY_UNTRACK_MATCH, TASKBAR_CREATED_MSG, WM_APP_TRAY,
};

#[cfg(windows)]
pub const WM_APP_SCORE_UPDATE: u32 = WM_APP + 1;
#[cfg(windows)]
pub const WM_APP_MATCH_EVENT: u32 = WM_APP + 3;
#[cfg(windows)]
pub const WM_APP_MATCHES_DISCOVERED: u32 = WM_APP + 4;

#[cfg(windows)]
const CLASS_NAME: PCWSTR = w!("SPNativeMain");
#[cfg(windows)]
const SCORE_W: u32 = 340;
#[cfg(windows)]
const SCORE_H: u32 = 110;
#[cfg(windows)]
const SCORE_SOCCER_W: u32 = 340;
#[cfg(windows)]
const SCORE_SOCCER_H: u32 = 40;
#[cfg(windows)]
const SCORE_CRICKET_COMPACT_H: u32 = 80;
#[cfg(windows)]
const HOTKEY_ID: i32 = 1;
/// Fallback registration id when the primary hotkey id is taken.
#[cfg(windows)]
const HOTKEY_ID_FALLBACK: i32 = 2;
#[cfg(windows)]
const DASH_LOADER_TIMER: usize = 1;
#[cfg(windows)]
const OVERLAY_FLASH_TIMER: usize = 2;
/// Spinner tick slowed from 33ms: a full 1120x760 present per tick is ~3.4MB + D2D + ULW.
#[cfg(windows)]
const DASH_LOADER_MS: u32 = 120;
/// One-shot retry timers for bridge posts that hit a full message queue.
#[cfg(windows)]
const POST_RETRY_SCORE_TIMER: usize = 3;
#[cfg(windows)]
const POST_RETRY_EVENT_TIMER: usize = 4;
#[cfg(windows)]
const POST_RETRY_DISCOVERED_TIMER: usize = 5;
#[cfg(windows)]
const POST_RETRY_UNTRACK_TIMER: usize = 6;
#[cfg(windows)]
const DASH_SCROLL_TIMER: usize = 7;
#[cfg(windows)]
const DASH_SCROLL_MS: u32 = 16;
#[cfg(windows)]
const POST_RETRY_DELAY_MS: u32 = 50;

#[cfg(windows)]
static HMAIN: AtomicUsize = AtomicUsize::new(0);
/// Set only when this thread's CoInitializeEx returns S_OK (we own the COM
/// ref and must balance it with CoUninitialize on WM_DESTROY).
#[cfg(windows)]
static COM_NEEDS_UNINIT: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
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
    /// Real DPI from GetDpiForWindow (96 fallback); refreshed on WM_DPICHANGED.
    dpi: u32,
    /// Pending drag on maximized dashboard title bar: (grab_x, grab_y, start_cursor).
    title_drag_pending: Option<(f32, f32, POINT)>,
}

/// P0-9: GUI startup must never panic. Show a message box and terminate with
/// ExitProcess(1) instead of expect()/unwrap() on module/window/renderer setup.
#[cfg(windows)]
unsafe fn fatal_startup(msg: &str) -> ! {
    let text: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = "SportsPulse fatal error\0".encode_utf16().collect();
    let _ = MessageBoxW(
        HWND::default(),
        PCWSTR(text.as_ptr()),
        PCWSTR(caption.as_ptr()),
        MB_OK | MB_ICONERROR,
    );
    ExitProcess(1);
}

/// Primary-monitor work area. Only used before any window exists; afterwards
/// prefer `work_area_for(hwnd)` so secondary monitors get their own work area.
#[cfg(windows)]
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

/// Work area of the monitor containing `hwnd` (multi-monitor aware).
/// Falls back to the primary work area if the monitor query fails.
#[cfg(windows)]
unsafe fn work_area_for(hwnd: HWND) -> RECT {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        info.rcWork
    } else {
        work_area()
    }
}

#[cfg(windows)]
fn scale_for_dpi(px: u32, dpi: u32) -> u32 {
    // P1-1: MulDiv(px, dpi, 96) with rounding. dpi 0 (GetDpiForWindow failure) falls back to 96.
    // Truthful 1:1 scaling — used for the scoreboard overlay, event popups
    // (via POPUP_W/H), and the tray icon, which all render full-size.
    let dpi = if dpi == 0 { 96 } else { dpi };
    (((px as u64 * dpi as u64) + 48) / 96).max(1) as u32
}

/// Dashboard-only variant carrying the global UI_SCALE (dashboard renders
/// at 70% of design size; overlay and popups stay full-size).
fn scale_dash_for_dpi(px: u32, dpi: u32) -> u32 {
    let dpi = if dpi == 0 { 96 } else { dpi };
    ((px as f32 * dpi as f32 / 96.0 * UI_SCALE).round() as u32).max(1)
}

#[cfg(windows)]
fn overlay_size_for(score: &Option<MatchScore>, dpi: u32) -> (u32, u32) {
    // P1-1: base sizes are 96-DPI px; scale by dpi/96 so a WM_DPICHANGED
    // suggested rect is not snapped back to 96-DPI px by the caller.
    let (base_w, base_h) = match score {
        Some(s) if s.status != MatchStatus::NoMatch => match s.sport {
            SportType::Soccer => (SCORE_SOCCER_W, SCORE_SOCCER_H),
            SportType::Cricket => match s.status {
                MatchStatus::Live | MatchStatus::Break => (SCORE_W, SCORE_H),
                _ => (SCORE_W, SCORE_CRICKET_COMPACT_H),
            },
        },
        _ => (SCORE_W, SCORE_H),
    };
    (scale_for_dpi(base_w, dpi), scale_for_dpi(base_h, dpi))
}

#[cfg(windows)]
unsafe fn overlay_pos_for(hwnd: HWND, w: u32, h: u32) -> POINT {
    let wa = work_area_for(hwnd);
    POINT {
        x: wa.right - w as i32 - 12,
        y: wa.bottom - h as i32 - 12,
    }
}

/// Resize, pin to the monitor work-area bottom-right (12px inset), and present the overlay.
#[cfg(windows)]
unsafe fn place_and_present_overlay(hwnd: HWND, state: &mut AppState, score: &Option<MatchScore>) {
    // P1-1: DPI-scale the 96-DPI base size via AppState.dpi (GetDpiForWindow,
    // refreshed on WM_DPICHANGED). Unscaled here snaps a suggested-rect resize
    // straight back to 96-DPI px. Mirrors the dashboard suggested-rect path.
    let (w, h) = overlay_size_for(score, state.dpi);
    let pos = overlay_pos_for(hwnd, w, h);
    let _ = SetWindowPos(
        hwnd,
        None,
        pos.x,
        pos.y,
        w as i32,
        h as i32,
        SWP_NOACTIVATE | SWP_NOZORDER,
    );
    state.renderer.set_dpi(state.dpi);
    let _ = state.renderer.resize(w, h);
    debug_assert_eq!(state.renderer.size(), (w as i32, h as i32));
    if let Err(e) = state.renderer.present(&pos, score) {
        // P0-7: D2DERR_RECREATE_TARGET (device lost) must recreate + retry once,
        // never be swallowed. Other errors stay best-effort; the next tick repaints.
        if e.code() == D2DERR_RECREATE_TARGET && state.renderer.recreate().is_ok() {
            #[cfg(debug_assertions)]
            sportspulse::render::dbglog("overlay RECREATE_TARGET: recreated, retrying present");
            let _ = state.renderer.present(&pos, score);
        }
    }
}

/// Present at the current window rect. Resizes the renderer first so a
/// height hop (96/140/200) or DPI change never clips or overdraws.
#[cfg(windows)]
unsafe fn present_overlay_now(hwnd: HWND, state: &mut AppState) {
    let score = state.cache.get();
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let w = (rect.right - rect.left).max(1) as u32;
    let h = (rect.bottom - rect.top).max(1) as u32;
    state.renderer.set_dpi(state.dpi);
    let _ = state.renderer.resize(w, h);
    debug_assert_eq!(state.renderer.size(), (w as i32, h as i32));
    let pos = POINT {
        x: rect.left,
        y: rect.top,
    };
    if let Err(e) = state.renderer.present(&pos, &score) {
        // P0-7: same RECREATE_TARGET → recreate + retry-once policy as the
        // primary outside-paint path above.
        if e.code() == D2DERR_RECREATE_TARGET && state.renderer.recreate().is_ok() {
            #[cfg(debug_assertions)]
            sportspulse::render::dbglog("overlay(paint) RECREATE_TARGET: recreated, retrying");
            let _ = state.renderer.present(&pos, &score);
        }
    }
}

#[cfg(windows)]
unsafe fn hide_overlay(hwnd: HWND, state: &mut AppState) {
    let _ = KillTimer(hwnd, OVERLAY_FLASH_TIMER);
    state.renderer.set_event_flash(None);
    if let Some(popup) = state.popup_win.as_mut() {
        popup.hide();
    }
    let _ = ShowWindow(hwnd, SW_HIDE);
    log_live_benchmark_sample("Idle Background (Tray Only)");
}

#[cfg(windows)]
unsafe fn bottom_right_popup_point(hwnd: HWND) -> POINT {
    let wa = work_area_for(hwnd);
    POINT {
        x: wa.right - POPUP_W as i32 - 12,
        y: wa.bottom - POPUP_H as i32 - 12,
    }
}

#[cfg(windows)]
unsafe fn center_screen_point(hwnd: HWND, w: u32, h: u32) -> POINT {
    let wa = work_area_for(hwnd);
    POINT {
        x: wa.left + (wa.right - wa.left - w as i32) / 2,
        y: wa.top + (wa.bottom - wa.top - h as i32) / 2,
    }
}

#[cfg(windows)]
unsafe fn show_overlay(hwnd: HWND, state: &mut AppState, score: &Option<MatchScore>) {
    if let Some(popup) = state.popup_win.as_mut() {
        popup.hide();
    }
    place_and_present_overlay(hwnd, state, score);
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    log_live_benchmark_sample("Scoreboard Shown (Overlay Active)");
}

#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
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
    log_live_benchmark_sample("Dashboard Open (Match Discovery Active)");
}

/// Full work-area maximize for the custom-rendered dashboard. This deliberately avoids
/// the previous down/up double-toggle and the arbitrary 40px gutter that made maximize
/// look broken. The dashboard preserves its last normal geometry for a faithful restore.
#[cfg(windows)]
unsafe fn toggle_dashboard_maximize(state: &mut AppState) {
    let hwnd = state.dash_hwnd;
    let restore_rect = state.dashboard_restore_rect;
    let Some(r) = state.dash_renderer.as_mut() else {
        return;
    };

    if !r.is_maximized {
        let mut current = RECT::default();
        let _ = GetWindowRect(hwnd, &mut current);
        state.dashboard_restore_rect = current;

        let wa = work_area_for(hwnd);
        let width = wa.right - wa.left;
        let height = wa.bottom - wa.top;
        r.is_maximized = true;
        let _ = SetWindowPos(hwnd, None, wa.left, wa.top, width, height, SWP_NOACTIVATE);
    } else {
        let min_w = scale_dash_for_dpi(DASH_NORMAL_W, state.dpi) as i32;
        let min_h = scale_dash_for_dpi(DASH_NORMAL_H, state.dpi) as i32;
        let width = (restore_rect.right - restore_rect.left).max(min_w);
        let height = (restore_rect.bottom - restore_rect.top).max(min_h);
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
    }
}

/// Mirrors standard Windows behavior: pulling a maximized window from its title bar
/// restores its normal size beneath the pointer and immediately continues the drag.
#[cfg(windows)]
unsafe fn dashboard_is_loading(state: &AppState) -> bool {
    // Acquire pairs with the engine's Release store on first-fetch completion.
    !state
        .match_state
        .initial_fetch_completed
        .load(Ordering::Acquire)
}

#[cfg(windows)]
unsafe fn present_dashboard(state: &mut AppState, pos: POINT) {
    let loading = dashboard_is_loading(state);
    if let Some(r) = state.dash_renderer.as_mut() {
        if let Err(e) = r.present(&pos, &state.dash_matches, &state.dash_selected_id, loading) {
            // P0-7: propagate EndDraw RECREATE_TARGET → recreate + retry once.
            if e.code() == D2DERR_RECREATE_TARGET && r.recreate().is_ok() {
                #[cfg(debug_assertions)]
                sportspulse::render::dbglog("dashboard RECREATE_TARGET: recreated, retrying");
                let _ = r.present(&pos, &state.dash_matches, &state.dash_selected_id, loading);
            }
        }
    }
    if loading {
        let _ = SetTimer(state.dash_hwnd, DASH_LOADER_TIMER, DASH_LOADER_MS, None);
    } else {
        let _ = KillTimer(state.dash_hwnd, DASH_LOADER_TIMER);
    }
}

/// Same-thread post with one immediate retry if the queue is momentarily full,
/// then a one-shot timer fallback (same policy as `post_checked` below).
/// SAFETY: scalar payloads only (WPARAM/LPARAM by value). Never use for
/// heap-pointer payloads (WM_APP_SELECT_MATCH): a double-post would
/// double-free. The select path posts its Box::into_raw pointer manually.
#[cfg(windows)]
unsafe fn post_ui_msg(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    if PostMessageW(hwnd, msg, wparam, lparam).is_err()
        && PostMessageW(hwnd, msg, wparam, lparam).is_err()
    {
        let retry_timer = match msg {
            WM_APP_SCORE_UPDATE => POST_RETRY_SCORE_TIMER,
            WM_APP_MATCH_EVENT => POST_RETRY_EVENT_TIMER,
            WM_APP_MATCHES_DISCOVERED => POST_RETRY_DISCOVERED_TIMER,
            WM_APP_UNTRACK => POST_RETRY_UNTRACK_TIMER,
            _ => POST_RETRY_SCORE_TIMER,
        };
        let _ = SetTimer(hwnd, retry_timer, POST_RETRY_DELAY_MS, None);
    }
}

#[cfg(windows)]
unsafe fn present_dashboard_hwnd(state: &mut AppState, hwnd: HWND) {
    // Resize-before-present: a height/DPI hop without a matching renderer
    // resize clips or overdraws. Mirrors overlay present_overlay_now().
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let w = (rect.right - rect.left).max(1) as u32;
    let h = (rect.bottom - rect.top).max(1) as u32;
    let dpi = state.dpi;
    if let Some(r) = state.dash_renderer.as_mut() {
        let _ = r.resize(w, h, dpi);
        debug_assert_eq!((r.w, r.h), (w as i32, h as i32));
    }
    present_dashboard(
        state,
        POINT {
            x: rect.left,
            y: rect.top,
        },
    );
}

#[cfg(windows)]
unsafe fn restore_dashboard_for_drag(state: &mut AppState, grab_x: f32, grab_y: f32) {
    let hwnd = state.dash_hwnd;
    let restore = state.dashboard_restore_rect;
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
    let min_w = scale_dash_for_dpi(DASH_NORMAL_W, state.dpi) as i32;
    let min_h = scale_dash_for_dpi(DASH_NORMAL_H, state.dpi) as i32;
    let normal_width = (restore.right - restore.left).max(min_w);
    let normal_height = (restore.bottom - restore.top).max(min_h);
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
}

#[cfg(windows)]
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;

    // Explorer restart recovery: re-ADD the tray icon (TaskbarCreated broadcast).
    let taskbar_created = TASKBAR_CREATED_MSG.load(Ordering::Acquire);
    if taskbar_created != 0 && msg == taskbar_created {
        if let Some(state) = state_ptr.as_mut() {
            state.tray.re_add();
        }
        return LRESULT(0);
    }

    match msg {
        WM_NCCREATE => {
            // P0-6 backstop: honor a non-null lpCreateParams owner pointer.
            // Both windows are created with null lpParam today (AppState is
            // built after the HWNDs exist and shared via SetWindowLongPtrW),
            // so this never clears an already-stored pointer. Must return
            // TRUE or window creation fails.
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            if !cs.lpCreateParams.is_null() && GetWindowLongPtrW(hwnd, GWLP_USERDATA) == 0 {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            }
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_ptr.as_mut() {
                toggle_dashboard(state);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            // P0-7: layered-window ULW presentation stays on the outside-paint
            // paths (score/timer/event handlers). WM_PAINT only re-presents the
            // last frame for RDP/UAC-obscured windows and must bracket with
            // BeginPaint/EndPaint so the update region validates.
            let mut ps = PAINTSTRUCT::default();
            let _paint_dc = BeginPaint(hwnd, &mut ps);
            if let Some(state) = state_ptr.as_mut() {
                present_overlay_now(hwnd, state);
            }
            let _ = EndPaint(hwnd, &ps);
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
            } else if wparam.0 == POST_RETRY_SCORE_TIMER {
                // Single retry of a queue-full bridge post; handlers re-read fresh state.
                // If the retry also hits a full queue, re-arm (post_checked policy)
                // instead of silently dropping the update.
                let _ = KillTimer(hwnd, POST_RETRY_SCORE_TIMER);
                if PostMessageW(hwnd, WM_APP_SCORE_UPDATE, WPARAM(0), LPARAM(0)).is_err() {
                    let _ = SetTimer(hwnd, POST_RETRY_SCORE_TIMER, POST_RETRY_DELAY_MS, None);
                }
            } else if wparam.0 == POST_RETRY_EVENT_TIMER {
                let _ = KillTimer(hwnd, POST_RETRY_EVENT_TIMER);
                if PostMessageW(hwnd, WM_APP_MATCH_EVENT, WPARAM(0), LPARAM(0)).is_err() {
                    let _ = SetTimer(hwnd, POST_RETRY_EVENT_TIMER, POST_RETRY_DELAY_MS, None);
                }
            } else if wparam.0 == POST_RETRY_DISCOVERED_TIMER {
                let _ = KillTimer(hwnd, POST_RETRY_DISCOVERED_TIMER);
                if PostMessageW(hwnd, WM_APP_MATCHES_DISCOVERED, WPARAM(0), LPARAM(0)).is_err() {
                    let _ = SetTimer(hwnd, POST_RETRY_DISCOVERED_TIMER, POST_RETRY_DELAY_MS, None);
                }
            } else if wparam.0 == POST_RETRY_UNTRACK_TIMER {
                let _ = KillTimer(hwnd, POST_RETRY_UNTRACK_TIMER);
                if PostMessageW(hwnd, WM_APP_UNTRACK, WPARAM(0), LPARAM(0)).is_err() {
                    let _ = SetTimer(hwnd, POST_RETRY_UNTRACK_TIMER, POST_RETRY_DELAY_MS, None);
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // lparam points at the system-suggested rect for the new DPI.
            if let Some(state) = state_ptr.as_mut() {
                let dpi = GetDpiForWindow(hwnd);
                if dpi != 0 {
                    state.dpi = dpi;
                }
                let suggested = *(lparam.0 as *const RECT);
                let w = (suggested.right - suggested.left).max(1);
                let h = (suggested.bottom - suggested.top).max(1);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                place_and_present_overlay(hwnd, state, &state.cache.get());
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // Monitor topology changed: re-pin the overlay to the (possibly new) work area.
            if let Some(state) = state_ptr.as_mut() {
                place_and_present_overlay(hwnd, state, &state.cache.get());
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 == HOTKEY_ID as usize || wparam.0 == HOTKEY_ID_FALLBACK as usize {
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
                    // Full ESPN string for the tooltip (isolated + pre-truncated
                    // to the 128-char tip cap).
                    let tip = ui_text(&format!("{} — {}", s.match_title, s.team1.score), 110);
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
                            WIN_TTL.as_millis() as u32
                        } else {
                            EVENT_TTL.as_millis() as u32
                        };
                        // Reduced-motion: no flash timer — the in-card flash
                        // persists until the next event or hide.
                        if !reduced_motion() {
                            let _ = SetTimer(hwnd, OVERLAY_FLASH_TIMER, timeout_ms, None);
                        }
                    } else {
                        let pos = bottom_right_popup_point(hwnd);
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
                // Poison-recovering read: a panic elsewhere must not freeze
                // the dashboard on a stale list.
                let matches: Vec<DiscoveredMatch> = state
                    .match_state
                    .active_matches
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .map(|m| DiscoveredMatch {
                        sport: m.0,
                        series_id: m.1.clone(),
                        match_id: m.2.clone(),
                        title: m.3.clone(),
                        status: m.4.clone(),
                        league_name: m.5.clone(),
                        start_time: m.6.clone(),
                    })
                    .collect();
                state.dash_matches = matches;
                if IsWindowVisible(state.dash_hwnd).as_bool() {
                    present_dashboard_hwnd(state, state.dash_hwnd);
                }
            }
            LRESULT(0)
        }
        WM_APP_SELECT_MATCH => {
            // Stable selection by heap-allocated match_id (Box::into_raw at the
            // dashboard sender, freed here). Never an index: dash_matches can
            // be replaced by WM_APP_MATCHES_DISCOVERED between hit-test and
            // receipt, and an index would silently track the wrong match.
            let raw = wparam.0 as *mut String;
            if raw.is_null() {
                return LRESULT(0);
            }
            // SAFETY: sender is the same-UI-thread dashboard proc, which only
            // constructs this via Box::into_raw(Box::new(match_id)) and never
            // touches it after a successful PostMessageW. Reclaim exactly once
            // here (even when state is gone) so no path leaks.
            let owned: Box<String> = unsafe { Box::from_raw(raw) };
            if let Some(state) = state_ptr.as_mut() {
                let match_id = owned.as_str();
                if match_id.is_empty() {
                    return LRESULT(0);
                }
                // Resolve the stable id against the *current* list, then
                // require it to still be present in the engine's active list
                // with a usable status before acting on it.
                let current = state
                    .dash_matches
                    .iter()
                    .find(|m| m.match_id == match_id)
                    .cloned();
                if let Some(m) = current {
                    let still_active = state
                        .match_state
                        .active_matches
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .iter()
                        .any(|a| a.2 == m.match_id && !a.4.is_empty());
                    if !m.status.is_empty() && still_active {
                        state.cache.clear();
                        state.renderer.set_event_flash(None);
                        state.match_state.select_match(
                            m.sport,
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
            // NOTIFYICON_VERSION_4 packs event in LOWORD and icon ID in HIWORD.
            let event = (lparam.0 as u32) & 0xFFFF;
            match event {
                WM_LBUTTONUP | 0x0400 /* NIN_SELECT */ | 0x0401 /* NIN_KEYSELECT */ => {
                    if let Some(state) = state_ptr.as_mut() {
                        toggle_scoreboard(hwnd, state);
                    }
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    if let Some(state) = state_ptr.as_mut() {
                        state.tray.show_context_menu();
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if let Some(state) = state_ptr.as_mut() {
                match id {
                    ID_TRAY_TOGGLE_SCORE => toggle_scoreboard(hwnd, state),
                    ID_TRAY_OPEN_DASHBOARD => show_dashboard(state),
                    ID_TRAY_UNTRACK_MATCH => {
                        post_ui_msg(hwnd, WM_APP_UNTRACK, WPARAM(0), LPARAM(0));
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
            // P0-6 teardown order: unregister hotkeys, explicitly remove the
            // tray icon + destroy owned windows BEFORE PostQuitMessage (the
            // TrayIcon Drop in WM_NCDESTROY is the backstop, not the plan, so
            // no ghost icon survives a forced quit).
            let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
            let _ = UnregisterHotKey(hwnd, HOTKEY_ID_FALLBACK);
            if let Some(state) = state_ptr.as_mut() {
                state.tray.remove();
                if !state.dash_hwnd.0.is_null() {
                    let _ = DestroyWindow(state.dash_hwnd);
                }
                if let Some(popup) = state.popup_win.as_ref() {
                    if !popup.hwnd.0.is_null() {
                        let _ = DestroyWindow(popup.hwnd);
                    }
                }
                // P0-9 COM balance: only uninitialize what we initialized.
                if COM_NEEDS_UNINIT.swap(false, Ordering::Release) {
                    CoUninitialize();
                }
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // P0-6 single owner: the AppState Box is freed exactly once here.
            // dash_hwnd shares the same raw ptr (set at startup); it is never
            // freed through the dashboard proc. Null-check + clear both
            // USERDATA slots to guard against double-free.
            if !state_ptr.is_null() {
                let dash_hwnd = state_ptr
                    .as_ref()
                    .map(|s| s.dash_hwnd)
                    .unwrap_or(HWND::default());
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !dash_hwnd.0.is_null() {
                    SetWindowLongPtrW(dash_hwnd, GWLP_USERDATA, 0);
                }
                drop(Box::from_raw(state_ptr));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// Dashboard Window Procedure
#[cfg(windows)]
unsafe extern "system" fn dashboard_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // P0-6: prefer this window's own GWLP_USERDATA (set alongside the main
    // window to the same single-owner Box). Fall back to the HMAIN parent
    // proxy only when own USERDATA is still zero.
    let own = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    let state_ptr = if own != 0 {
        own as *mut AppState
    } else {
        let parent_hwnd = HWND(HMAIN.load(Ordering::Acquire) as *mut _);
        GetWindowLongPtrW(parent_hwnd, GWLP_USERDATA) as *mut AppState
    };
    // Parent handle for cross-window posts (dashboard → main). Always resolved
    // via HMAIN; never cached across calls.
    let parent_hwnd = HWND(HMAIN.load(Ordering::Acquire) as *mut _);

    match msg {
        WM_NCCREATE => {
            // Same backstop as the main proc: honor a non-null lpCreateParams
            // owner pointer without ever clearing an already-stored one.
            // Must return TRUE or window creation fails.
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            if !cs.lpCreateParams.is_null() && GetWindowLongPtrW(hwnd, GWLP_USERDATA) == 0 {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            }
            LRESULT(1)
        }
        WM_GETMINMAXINFO => {
            // Clamp maximized geometry to the monitor work area (WS_POPUP has no frame to do it).
            let mmi = &mut *(lparam.0 as *mut MINMAXINFO);
            let wa = work_area_for(hwnd);
            mmi.ptMaxPosition = POINT {
                x: wa.left,
                y: wa.top,
            };
            mmi.ptMaxSize = POINT {
                x: wa.right - wa.left,
                y: wa.bottom - wa.top,
            };
            mmi.ptMinTrackSize = POINT { x: 640, y: 480 };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // Adopt the system-suggested rect, resize the renderer, re-present.
            if let Some(state) = state_ptr.as_mut() {
                let dpi = (wparam.0 & 0xFFFF) as u32;
                let dpi = if dpi == 0 { GetDpiForWindow(hwnd) } else { dpi };
                let dpi = if dpi == 0 { 96 } else { dpi };
                state.dpi = dpi;
                let suggested = *(lparam.0 as *const RECT);
                let w = (suggested.right - suggested.left).max(1) as u32;
                let h = (suggested.bottom - suggested.top).max(1) as u32;
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    w as i32,
                    h as i32,
                    SWP_NOACTIVATE,
                );
                if let Some(r) = state.dash_renderer.as_mut() {
                    let _ = r.resize(w, h, dpi);
                }
                present_dashboard_hwnd(state, hwnd);
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // Monitor topology changed: keep maximized dashboards glued to the work area.
            if let Some(state) = state_ptr.as_mut() {
                if state.dash_renderer.as_ref().is_some_and(|r| r.is_maximized) {
                    let wa = work_area_for(hwnd);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        wa.left,
                        wa.top,
                        wa.right - wa.left,
                        wa.bottom - wa.top,
                        SWP_NOACTIVATE,
                    );
                    if let Some(r) = state.dash_renderer.as_mut() {
                        let _ = r.resize((wa.right - wa.left) as u32, (wa.bottom - wa.top) as u32, state.dpi);
                    }
                    present_dashboard_hwnd(state, hwnd);
                }
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let new_w = (lparam.0 & 0xFFFF) as u32;
            let new_h = (lparam.0 >> 16) as u32;
            if !IsIconic(hwnd).as_bool() && new_w > 0 && new_h > 0 {
                if let Some(state) = state_ptr.as_mut() {
                    let wa = work_area_for(hwnd);
                    let wa_w = (wa.right - wa.left) as u32;
                    let wa_h = (wa.bottom - wa.top) as u32;
                    let is_max = new_w >= wa_w.saturating_sub(4) && new_h >= wa_h.saturating_sub(4);
                    if let Some(r) = state.dash_renderer.as_mut() {
                        r.is_maximized = is_max;
                        let _ = r.resize(new_w, new_h, state.dpi);
                    }
                    present_dashboard_hwnd(state, hwnd);
                }
            }
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            if let Some(state) = state_ptr.as_mut() {
                let is_max = state.dash_renderer.as_ref().is_some_and(|r| r.is_maximized);
                if !is_max {
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_ok() {
                        state.dashboard_restore_rect = rect;
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;
            if let Some(state) = state_ptr.as_mut() {
                state.title_drag_pending = None;
                let _ = ReleaseCapture();
                let hit = state
                    .dash_renderer
                    .as_ref()
                    .and_then(|r| r.hit_test(x, y, state.dash_matches.len()));
                if matches!(hit, Some(HitTarget::TitleBar)) {
                    toggle_dashboard_maximize(state);
                    return LRESULT(0);
                }
            }
            LRESULT(0)
        }
        WM_NCLBUTTONDBLCLK => {
            if wparam.0 == HTCAPTION as usize {
                if let Some(state) = state_ptr.as_mut() {
                    state.title_drag_pending = None;
                    let _ = ReleaseCapture();
                    toggle_dashboard_maximize(state);
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;

            if let Some(state) = state_ptr.as_mut() {
                let hit = state
                    .dash_renderer
                    .as_ref()
                    .and_then(|r| r.hit_test(x, y, state.dash_matches.len()));
                if matches!(hit, Some(HitTarget::CricketTab))
                    || matches!(hit, Some(HitTarget::FootballTab))
                {
                    let sport = if matches!(hit, Some(HitTarget::CricketTab)) {
                        DashboardSport::Cricket
                    } else {
                        DashboardSport::Football
                    };
                    let changed = if let Some(r) = state.dash_renderer.as_mut() {
                        if r.active_sport != sport {
                            r.active_sport = sport;
                            r.scroll_offset = 0.0;
                            r.target_scroll_offset = 0.0;
                            let _ = KillTimer(hwnd, DASH_SCROLL_TIMER);
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
                    return LRESULT(0);
                }

                if matches!(hit, Some(HitTarget::TitleBar)) {
                    let is_max = state.dash_renderer.as_ref().is_some_and(|r| r.is_maximized);
                    if is_max {
                        let mut cur = POINT::default();
                        let _ = GetCursorPos(&mut cur);
                        state.title_drag_pending = Some((x, y, cur));
                        let _ = SetCapture(hwnd);
                        return LRESULT(0);
                    } else {
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
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_ptr.as_mut() {
                if let Some((grab_x, grab_y, start_cur)) = state.title_drag_pending {
                    let mut cur = POINT::default();
                    if GetCursorPos(&mut cur).is_ok()
                        && ((cur.x - start_cur.x).abs() > 4 || (cur.y - start_cur.y).abs() > 4)
                    {
                        state.title_drag_pending = None;
                        let _ = ReleaseCapture();
                        restore_dashboard_for_drag(state, grab_x, grab_y);
                        let _ = SendMessageW(
                            hwnd,
                            WM_NCLBUTTONDOWN,
                            WPARAM(HTCAPTION as usize),
                            LPARAM(0),
                        );
                        return LRESULT(0);
                    }
                }

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
                // Arm TME_LEAVE so a mouse-leave clears sticky hover (no MOUSEMOVE storm).
                if let Some(state) = state_ptr.as_mut() {
                    if let Some(r) = state.dash_renderer.as_mut() {
                        if !r.mouse_tracking {
                            let mut tme = TRACKMOUSEEVENT {
                                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: hwnd,
                                dwHoverTime: 0,
                            };
                            if TrackMouseEvent(&mut tme).is_ok() {
                                r.mouse_tracking = true;
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(state) = state_ptr.as_mut() {
                let had_hover = state.dash_renderer.as_ref().is_some_and(|r| {
                    r.hover_index.is_some()
                        || r.action_hover_index.is_some()
                        || r.min_hover
                        || r.max_hover
                        || r.close_hover
                        || r.cricket_hover
                        || r.football_hover
                });
                if let Some(r) = state.dash_renderer.as_mut() {
                    r.clear_hover();
                }
                if had_hover {
                    present_dashboard_hwnd(state, hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_ptr.as_mut() {
                // High-resolution wheels send partial deltas: accumulate to WHEEL_DELTA notches.
                let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
                let (target_changed, still_anim) = if let Some(r) = state.dash_renderer.as_mut() {
                    let changed = r.accumulate_wheel(delta);
                    if changed {
                        r.step_scroll_animation();
                    }
                    (
                        changed,
                        (r.scroll_offset - r.target_scroll_offset).abs() >= 0.5,
                    )
                } else {
                    (false, false)
                };
                if target_changed || still_anim {
                    if still_anim {
                        let _ = SetTimer(hwnd, DASH_SCROLL_TIMER, DASH_SCROLL_MS, None);
                    }
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let mut screen_rect = RECT::default();
                        if GetWindowRect(hwnd, &mut screen_rect).is_ok() {
                            let x = (pt.x - screen_rect.left) as f32;
                            let y = (pt.y - screen_rect.top) as f32;
                            if let Some(r) = state.dash_renderer.as_mut() {
                                let hit = r.hit_test(x, y, state.dash_matches.len());
                                let (
                                    hover_idx,
                                    action_idx,
                                    min_h,
                                    max_h,
                                    close_h,
                                    cricket_h,
                                    football_h,
                                ) = match hit {
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
                                r.set_hover(
                                    hover_idx, action_idx, min_h, max_h, close_h, cricket_h,
                                    football_h,
                                );
                            }
                        }
                    }
                    present_dashboard_hwnd(state, hwnd);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            // P0-7: same contract as the overlay proc — ULW stays on the
            // outside-paint paths; WM_PAINT re-presents under Begin/EndPaint.
            let mut ps = PAINTSTRUCT::default();
            let _paint_dc = BeginPaint(hwnd, &mut ps);
            if let Some(state) = state_ptr.as_mut() {
                present_dashboard_hwnd(state, hwnd);
            }
            let _ = EndPaint(hwnd, &ps);
            let _ = ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == DASH_LOADER_TIMER {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    let _ = KillTimer(hwnd, DASH_LOADER_TIMER);
                    return LRESULT(0);
                }
                if let Some(state) = state_ptr.as_mut() {
                    if dashboard_is_loading(state) {
                        present_dashboard_hwnd(state, hwnd);
                    } else {
                        let _ = KillTimer(hwnd, DASH_LOADER_TIMER);
                    }
                }
            } else if wparam.0 == DASH_SCROLL_TIMER {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    let _ = KillTimer(hwnd, DASH_SCROLL_TIMER);
                    return LRESULT(0);
                }
                if let Some(state) = state_ptr.as_mut() {
                    let still_animating = if let Some(r) = state.dash_renderer.as_mut() {
                        r.step_scroll_animation()
                    } else {
                        false
                    };
                    if !still_animating {
                        let _ = KillTimer(hwnd, DASH_SCROLL_TIMER);
                    }
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let mut screen_rect = RECT::default();
                        if GetWindowRect(hwnd, &mut screen_rect).is_ok() {
                            let x = (pt.x - screen_rect.left) as f32;
                            let y = (pt.y - screen_rect.top) as f32;
                            if let Some(r) = state.dash_renderer.as_mut() {
                                let hit = r.hit_test(x, y, state.dash_matches.len());
                                let (
                                    hover_idx,
                                    action_idx,
                                    min_h,
                                    max_h,
                                    close_h,
                                    cricket_h,
                                    football_h,
                                ) = match hit {
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
                                r.set_hover(
                                    hover_idx, action_idx, min_h, max_h, close_h, cricket_h,
                                    football_h,
                                );
                            }
                        }
                    }
                    present_dashboard_hwnd(state, hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = (lparam.0 >> 16) as i16 as f32;

            if let Some(state) = state_ptr.as_mut() {
                if state.title_drag_pending.is_some() {
                    state.title_drag_pending = None;
                    let _ = ReleaseCapture();
                }
                let hit = state
                    .dash_renderer
                    .as_ref()
                    .map(|r| r.hit_test(x, y, state.dash_matches.len()));
                match hit {
                    Some(Some(HitTarget::MinimizeButton)) => {
                        let _ = KillTimer(hwnd, DASH_SCROLL_TIMER);
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    Some(Some(HitTarget::MaximizeButton)) => {
                        toggle_dashboard_maximize(state);
                    }
                    Some(Some(HitTarget::CloseButton)) => {
                        let _ = KillTimer(hwnd, DASH_LOADER_TIMER);
                        let _ = KillTimer(hwnd, DASH_SCROLL_TIMER);
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
                                r.scroll_offset = 0.0;
                                r.target_scroll_offset = 0.0;
                                let _ = KillTimer(hwnd, DASH_SCROLL_TIMER);
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
                    Some(Some(HitTarget::MatchAction(idx)))
                    | Some(Some(HitTarget::MatchItem(idx)))
                        if idx < state.dash_matches.len() =>
                    {
                        // Stable select: post a heap-allocated match_id, never the
                        // hit-test index (TOCTOU across list refreshes). The main
                        // proc reclaims via Box::from_raw on receipt.
                        // PostMessageW is async (no re-entrancy), so posting while
                        // holding `state` is safe (unlike SendMessageW, cf. P0-8).
                        let match_id = state.dash_matches[idx].match_id.clone();
                        if !match_id.is_empty() {
                            let selected = state
                                .dash_selected_id
                                .as_ref()
                                .is_some_and(|id| id == &match_id);
                            if selected {
                                post_ui_msg(parent_hwnd, WM_APP_UNTRACK, WPARAM(0), LPARAM(0));
                            } else {
                                let raw = Box::into_raw(Box::new(match_id));
                                if PostMessageW(
                                    parent_hwnd,
                                    WM_APP_SELECT_MATCH,
                                    WPARAM(raw as usize),
                                    LPARAM(0),
                                )
                                .is_err()
                                {
                                    // Queue full: reclaim to avoid a leak; the
                                    // click is user-retryable. Never double-post
                                    // the same pointer (would double-free).
                                    unsafe {
                                        drop(Box::from_raw(raw));
                                    }
                                    #[cfg(debug_assertions)]
                                    sportspulse::render::dbglog(
                                        "select post queue-full, reclaimed (click retry)",
                                    );
                                }
                            }
                        }
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

#[cfg(windows)]
fn spawn_engine_worker(
    cache: ScoreCache,
    match_state: ActiveMatchesState,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // P0-9: a dead runtime must surface a message box, not a panic in a
        // worker thread (invisible under windows_subsystem).
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => unsafe { fatal_startup("tokio runtime build failed") },
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

        // Spawn fetcher loop
        let c = cache.clone();
        let ms = match_state.clone();
        rt.spawn(async move {
            sportspulse::engine::fetcher::start_polling(c, ms, tx).await;
        });

        // Event listener bridge to Win32 messages
        rt.block_on(async move {
            // NOTE: the engine-facing sender is unbounded (fetcher signature is owned
            // by the P0 agent). Coalescing happens here on receipt instead: bursts of
            // ScoreChanged collapse to the latest before a single PostMessageW, so a
            // stalled UI thread can never pile up stale score posts.
            while let Some(ev) = rx.recv().await {
                let h = HMAIN.load(Ordering::Acquire);
                if h != 0 {
                    let hwnd = HWND(h as *mut _);
                    match ev {
                        AppEvent::ScoreChanged(_) => {
                            // Drain any further queued ScoreChanged; only the latest matters.
                            while let Ok(next) = rx.try_recv() {
                                match next {
                                    AppEvent::ScoreChanged(_) => continue,
                                    // Non-score events must not be swallowed: post them now.
                                    AppEvent::MatchEvent(_) => unsafe {
                                        post_checked(
                                            hwnd,
                                            WM_APP_MATCH_EVENT,
                                            POST_RETRY_EVENT_TIMER,
                                        )
                                    },
                                    AppEvent::MatchesDiscovered(_) => unsafe {
                                        post_checked(
                                            hwnd,
                                            WM_APP_MATCHES_DISCOVERED,
                                            POST_RETRY_DISCOVERED_TIMER,
                                        )
                                    },
                                }
                            }
                            unsafe {
                                post_checked(hwnd, WM_APP_SCORE_UPDATE, POST_RETRY_SCORE_TIMER);
                            }
                        }
                        AppEvent::MatchEvent(_) => unsafe {
                            post_checked(hwnd, WM_APP_MATCH_EVENT, POST_RETRY_EVENT_TIMER)
                        },
                        AppEvent::MatchesDiscovered(_) => unsafe {
                            post_checked(
                                hwnd,
                                WM_APP_MATCHES_DISCOVERED,
                                POST_RETRY_DISCOVERED_TIMER,
                            )
                        },
                    }
                }
            }
        });
    })
}

/// Post from the engine bridge; on a queue-full failure arm a one-shot timer so
/// the UI thread retries the (idempotent — handlers re-read fresh state) message once.
#[cfg(windows)]
unsafe fn post_checked(hwnd: HWND, msg: u32, retry_timer: usize) {
    if PostMessageW(hwnd, msg, WPARAM(0), LPARAM(0)).is_err() {
        let _ = SetTimer(hwnd, retry_timer, POST_RETRY_DELAY_MS, None);
    }
}

#[cfg(windows)]
fn main() {
    unsafe {
        // Single instance: a second launch exits immediately (mutex held for process lifetime).
        let mutex_name: Vec<u16> = "Local\\SportsPulseSingleInstance\0"
            .encode_utf16()
            .collect();
        let _instance_mutex = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr()));
        if GetLastError() == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
            return;
        }

        // COM STA Initialization (P0-9): S_OK/S_FALSE both mean COM is usable
        // (S_FALSE = already initialized on this thread — do NOT uninitialize).
        // RPC_E_CHANGED_MODE = already initialized in another mode; anything
        // else is a degraded fallback — startup proceeds, never hard-blocks.
        let com_hr = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
        if com_hr == S_OK {
            COM_NEEDS_UNINIT.store(true, Ordering::Release);
        } else if com_hr == S_FALSE || com_hr == RPC_E_CHANGED_MODE {
            // Usable without owning a ref; WM_DESTROY must skip CoUninitialize.
        } else {
            // Degraded fallback: D2D/WIC may still work; continue startup.
        }

        // DPI Awareness Context (PerMonitorV2)
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => fatal_startup("GetModuleHandleW failed during startup"),
        };

        // 1. Register Main Scoreboard Window Class
        let icon_big = load_app_icon(32);
        let icon_small = load_app_icon(16);

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hIcon: icon_big,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            hIconSm: icon_small,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        // 2. Register Dashboard Window Class
        const DASH_CLASS: PCWSTR = w!("SPNativeDashboard");
        let wc_dash = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(dashboard_wnd_proc),
            hInstance: hinstance.into(),
            hIcon: icon_big,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            hIconSm: icon_small,
            lpszClassName: DASH_CLASS,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc_dash);

        // 3. Create Main Scoreboard Layered Window
        // No window exists yet, so seed from the primary work area; the overlay
        // re-pins itself per-monitor (work_area_for) on every present after this.
        let primary_wa = work_area();
        let score_pos = POINT {
            x: primary_wa.right - SCORE_W as i32 - 12,
            y: primary_wa.bottom - SCORE_H as i32 - 12,
        };
        let hwnd = match CreateWindowExW(
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
        ) {
            Ok(w) => w,
            Err(_) => fatal_startup("CreateWindowExW failed for main scoreboard window"),
        };

        HMAIN.store(hwnd.0 as usize, Ordering::Release);

        // Real DPI for this monitor (96 fallback); refreshed on WM_DPICHANGED.
        let dpi = GetDpiForWindow(hwnd);
        let dpi = if dpi == 0 { 96 } else { dpi };

        let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon_big.0 as isize));
        let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon_small.0 as isize));

        // 4. Create Dashboard Window scaled for monitor DPI
        let max_dash_w = ((primary_wa.right - primary_wa.left) - 64).max(640) as u32;
        let max_dash_h = ((primary_wa.bottom - primary_wa.top) - 64).max(480) as u32;
        let init_dash_w = scale_dash_for_dpi(DASH_NORMAL_W, dpi).min(max_dash_w);
        let init_dash_h = scale_dash_for_dpi(DASH_NORMAL_H, dpi).min(max_dash_h);
        let dash_pos = center_screen_point(hwnd, init_dash_w, init_dash_h);
        let dash_hwnd = match CreateWindowExW(
            // The discovery dashboard is a normal taskbar application while open.
            // The lightweight scoreboard itself remains a tray-only topmost overlay.
            WS_EX_LAYERED | WS_EX_APPWINDOW,
            DASH_CLASS,
            w!("SportsPulse Dashboard"),
            WS_POPUP,
            dash_pos.x,
            dash_pos.y,
            init_dash_w as i32,
            init_dash_h as i32,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(w) => w,
            Err(_) => fatal_startup("CreateWindowExW failed for dashboard window"),
        };

        let _ = SendMessageW(dash_hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon_big.0 as isize));
        let _ = SendMessageW(dash_hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon_small.0 as isize));

        // 5. Create Components
        let mut renderer = match Renderer::new(hwnd, SCORE_W, SCORE_H) {
            Ok(r) => r,
            Err(_) => fatal_startup("Direct2D scoreboard renderer init failed"),
        };
        renderer.set_dpi(dpi);

        let dash_renderer = DashboardRenderer::new(dash_hwnd, init_dash_w, init_dash_h, dpi).ok();
        let popup_win = MiniPopupWindow::create().ok();
        let tray_icon_size = scale_for_dpi(16, dpi) as i32;
        let tray_icon = load_app_icon(tray_icon_size);
        let tray = TrayIcon::new(hwnd, "SportsPulse - Live Scores", Some(tray_icon));

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
                right: dash_pos.x + init_dash_w as i32,
                bottom: dash_pos.y + init_dash_h as i32,
            },
            dpi,
            title_drag_pending: None,
        });

        // P0-6: single Box owner shared via raw ptr. Both windows point at the
        // same AppState; it is freed exactly once in main WM_NCDESTROY (never
        // through the dashboard proc). The dashboard prefers its own USERDATA
        // and falls back to the HMAIN parent proxy only when zero.
        let app_ptr = Box::into_raw(app_state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app_ptr as isize);
        SetWindowLongPtrW(dash_hwnd, GWLP_USERDATA, app_ptr as isize);

        // 6. Register Global Hotkey Ctrl + Alt + Space, with a fallback id when taken.
        // (A fallback *id* cannot beat a taken *chord*; ERROR_HOTKEY_ALREADY_REGISTERED
        // still needs a user-facing remap — P0-11 owns that. This at least retries
        // registration instead of silently running hotkey-less on id collision.)
        if RegisterHotKey(
            hwnd,
            HOTKEY_ID,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0),
            VK_SPACE.0 as u32,
        )
        .is_err()
        {
            #[cfg(debug_assertions)]
            sportspulse::render::dbglog("primary hotkey id taken, trying fallback id");
            if let Err(e) = RegisterHotKey(
                hwnd,
                HOTKEY_ID_FALLBACK,
                HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0),
                VK_SPACE.0 as u32,
            ) {
                // Non-fatal: app runs hotkey-less (tray + click still work).
                // Never let_ the second failure — it means the chord itself is
                // taken, which needs a user-facing remap (P0-11). Surface a
                // dashboard toast instead of failing silently.
                HOTKEY_CONFLICT.store(true, Ordering::Release);
                sportspulse::render::dbglog(&format!("fallback hotkey failed: {e:?}"));
            }
        }

        // 7. Start with the dashboard visible (overlay hidden until Track).
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
        // The engine thread owns the Tokio runtime and blocks on the event channel;
        // it cannot be joined from here (std threads have no abort). Detaching via
        // drop is teardown-safe: process exit reclaims the runtime and its sockets.
        drop(engine_handle);
    }
}

/// Non-Windows fallback (P0-9): the app is Win32/Direct2D-only, but every bin
/// must still provide `main` so `cargo check` passes on Linux. All Windows
/// code above is `#[cfg(windows)]`-gated (the old file-level `#![cfg(windows)]`
/// would have gated this stub out too, defeating it).
#[cfg(not(windows))]
fn main() {
    eprintln!("SportsPulse is Windows-only.");
}
