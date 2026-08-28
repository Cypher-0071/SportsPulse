//! SportsPulse — Win32 System Tray (Shell_NotifyIconW) and Context Menu.

use windows::core::*;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
    TrackPopupMenuEx, HICON, HMENU, IDI_APPLICATION, MF_SEPARATOR, MF_STRING,
    TPM_BOTTOMALIGN, TPM_RIGHTALIGN,
};

pub const WM_APP_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

pub const ID_TRAY_TOGGLE_SCORE: usize = 1001;
pub const ID_TRAY_OPEN_DASHBOARD: usize = 1002;
pub const ID_TRAY_UNTRACK_MATCH: usize = 1003;
pub const ID_TRAY_QUIT: usize = 1004;

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
    pub unsafe fn new(hwnd: HWND, tooltip: &str) -> Self {
        let hicon: HICON = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = hicon;
        nid.szTip = to_wide_buf::<128>(tooltip);

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        Self { hwnd, nid }
    }

    pub unsafe fn update_tooltip(&mut self, tooltip: &str) {
        self.nid.szTip = to_wide_buf::<128>(tooltip);
        self.nid.uFlags = NIF_TIP;
        let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
    }

    pub unsafe fn show_context_menu(&self) {
        let hmenu: HMENU = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        let w_toggle: Vec<u16> = "Toggle Scoreboard\0".encode_utf16().collect();
        let w_dash: Vec<u16> = "Open Dashboard\0".encode_utf16().collect();
        let w_untrack: Vec<u16> = "Untrack Match\0".encode_utf16().collect();
        let w_quit: Vec<u16> = "Exit SportsPulse\0".encode_utf16().collect();

        let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_TOGGLE_SCORE, PCWSTR(w_toggle.as_ptr()));
        let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN_DASHBOARD, PCWSTR(w_dash.as_ptr()));
        let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_UNTRACK_MATCH, PCWSTR(w_untrack.as_ptr()));
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
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}
