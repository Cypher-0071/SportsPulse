//! SportsPulse — Win32 System Tray (Shell_NotifyIconW) and Context Menu.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::*;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyMenu, GetCursorPos,
    PostMessageW, RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenuEx, HICON, HMENU,
    LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_RIGHTALIGN, WM_NULL,
};

pub const WM_APP_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

/// Registered "TaskbarCreated" broadcast id. Explorer restarts send this;
/// the main wnd_proc re-ADDs the icon on receipt. 0 = registration failed.
pub static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

pub const ID_TRAY_OPEN_DASHBOARD: usize = 1001;
pub const ID_TRAY_TOGGLE_SCORE: usize = 1002;
pub const ID_TRAY_UNTRACK_MATCH: usize = 1003;
pub const ID_TRAY_QUIT: usize = 1004;

pub const ICON_ICO_DATA: &[u8] = include_bytes!("../assets/icon.ico");

/// Load the SportsPulse icon from embedded assets/icon.ico closest to `desired_size`.
pub unsafe fn load_app_icon(desired_size: i32) -> HICON {
    if ICON_ICO_DATA.len() < 6 {
        return HICON::default();
    }
    let count = u16::from_le_bytes([ICON_ICO_DATA[4], ICON_ICO_DATA[5]]) as usize;
    if count == 0 || ICON_ICO_DATA.len() < 6 + count * 16 {
        return HICON::default();
    }

    let mut best_index = 0;
    let mut best_diff = i32::MAX;

    for i in 0..count {
        let entry_offset = 6 + i * 16;
        let w_byte = ICON_ICO_DATA[entry_offset];
        let w = if w_byte == 0 { 256 } else { w_byte as i32 };
        let diff = (w - desired_size).abs();
        if diff < best_diff {
            best_diff = diff;
            best_index = i;
        }
    }

    let entry_offset = 6 + best_index * 16;
    let bytes_in_res = u32::from_le_bytes([
        ICON_ICO_DATA[entry_offset + 8],
        ICON_ICO_DATA[entry_offset + 9],
        ICON_ICO_DATA[entry_offset + 10],
        ICON_ICO_DATA[entry_offset + 11],
    ]) as usize;
    let image_offset = u32::from_le_bytes([
        ICON_ICO_DATA[entry_offset + 12],
        ICON_ICO_DATA[entry_offset + 13],
        ICON_ICO_DATA[entry_offset + 14],
        ICON_ICO_DATA[entry_offset + 15],
    ]) as usize;

    if image_offset + bytes_in_res > ICON_ICO_DATA.len() {
        return HICON::default();
    }

    let res_slice = &ICON_ICO_DATA[image_offset..image_offset + bytes_in_res];
    CreateIconFromResourceEx(
        res_slice,
        BOOL(1),
        0x00030000,
        desired_size,
        desired_size,
        LR_DEFAULTCOLOR,
    )
    .unwrap_or_default()
}

pub struct TrayIcon {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
}

fn to_wide_buf<const N: usize>(s: &str) -> [u16; N] {
    let mut buf = [0u16; N];
    let encoded: Vec<u16> = s.encode_utf16().collect();
    let len = encoded.len().min(N - 1);
    buf[..len].copy_from_slice(&encoded[..len]);
    buf
}

impl TrayIcon {
    pub unsafe fn new(hwnd: HWND, tooltip: &str, hicon: Option<HICON>) -> Self {
        // Broadcast id for Explorer-restart recovery (checked in the main wnd_proc).
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        TASKBAR_CREATED_MSG.store(taskbar_created, Ordering::Release);

        let icon = hicon.unwrap_or_else(|| load_app_icon(32));
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: icon,
            szTip: to_wide_buf::<128>(tooltip),
            ..Default::default()
        };

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        // Negotiate NOTIFYICON_VERSION_4 behavior (required post-ADD protocol).
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);

        Self { hwnd, nid }
    }

    /// Re-ADD the icon after an Explorer restart (TaskbarCreated broadcast).
    pub unsafe fn re_add(&self) {
        let _ = Shell_NotifyIconW(NIM_ADD, &self.nid);
        let mut nid = self.nid;
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }

    pub unsafe fn update_tooltip(&mut self, tooltip: &str) {
        self.nid.szTip = to_wide_buf::<128>(tooltip);
        self.nid.uFlags = NIF_TIP;
        let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
    }

    pub unsafe fn remove(&self) {
        // Explicit teardown for main WM_DESTROY/WM_NCDESTROY ordering.
        // Drop also calls this; a second NIM_DELETE is a harmless no-op.
        let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
    }

    pub unsafe fn show_context_menu(&self) {
        let hmenu: HMENU = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        let w_dash: Vec<u16> = "Open Dashboard\0".encode_utf16().collect();
        let w_toggle: Vec<u16> = "Toggle Scoreboard\0".encode_utf16().collect();
        let w_untrack: Vec<u16> = "Untrack Match\0".encode_utf16().collect();
        let w_quit: Vec<u16> = "Quit SportsPulse\0".encode_utf16().collect();

        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            ID_TRAY_OPEN_DASHBOARD,
            PCWSTR(w_dash.as_ptr()),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            ID_TRAY_TOGGLE_SCORE,
            PCWSTR(w_toggle.as_ptr()),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            ID_TRAY_UNTRACK_MATCH,
            PCWSTR(w_untrack.as_ptr()),
        );
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_QUIT, PCWSTR(w_quit.as_ptr()));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        // Required for popup menu to dismiss when clicking away
        let _ = SetForegroundWindow(self.hwnd);
        let _ = TrackPopupMenuEx(
            hmenu,
            (TPM_RIGHTALIGN | TPM_BOTTOMALIGN).0,
            pt.x,
            pt.y,
            self.hwnd,
            None,
        );
        let _ = DestroyMenu(hmenu);
        // Trailing WM_NULL: lets a stuck TrackPopupMenuEx return so the next
        // right-click re-opens the menu instead of hanging.
        let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}
