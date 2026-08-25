//! SportsPulse — native Win32 rewrite (windows-rs)
//! M1: scoreboard window with Direct2D/DirectWrite rendering (static sample).

#![cfg(windows)]
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{HBRUSH, InvalidateRect, UpdateWindow, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW,
    PostMessageW, PostQuitMessage, RegisterClassExW, SetWindowTextW, ShowWindow,
    TranslateMessage, DestroyWindow, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GetWindowLongPtrW,
    HCURSOR, HMENU, IDC_ARROW, MSG, SW_SHOW, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_PAINT, WM_SIZE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW, GWLP_USERDATA, SetWindowLongPtrW,
};

mod render;
use render::{Renderer, SAMPLE};

const WM_APP_TICK: u32 = WM_APP + 1;
const CLASS_NAME: PCWSTR = PCWSTR::from_raw(class_name_wide().as_ptr());

static HMAIN: AtomicUsize = AtomicUsize::new(0);
static TICKS: AtomicU32 = AtomicU32::new(0);

const fn class_name_wide() -> [u16; 15] {
    let mut buf = [0u16; 15];
    let name: &[u16] = &[
        b'S' as u16, b'P' as u16, b'N' as u16, b'a' as u16, b't' as u16, b'i' as u16,
        b'v' as u16, b'e' as u16, b'M' as u16, b'a' as u16, b'i' as u16, b'n' as u16,
    ];
    let mut i = 0;
    while i < name.len() {
        buf[i] = name[i];
        i += 1;
    }
    buf
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Renderer;
            if !ptr.is_null() {
                let _ = (*ptr).draw(&SAMPLE);
            }
            let _ = ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_SIZE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Renderer;
            if !ptr.is_null() {
                let w = (lparam.0 & 0xFFFF) as u32;
                let h = ((lparam.0 >> 16) & 0xFFFF) as u32;
                if w > 0 && h > 0 {
                    let _ = (*ptr).resize(w, h);
                    let _ = (*ptr).draw(&SAMPLE);
                }
            }
            LRESULT(0)
        }
        WM_APP_TICK => {
            let n = TICKS.load(Ordering::Relaxed);
            let title = wide(&format!("SportsPulse Native [M1] — tick {n}"));
            let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn spawn_engine() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .expect("tokio runtime");
        rt.block_on(engine_loop());
    })
}

async fn engine_loop() {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        TICKS.fetch_add(1, Ordering::Relaxed);
        unsafe {
            let h = HMAIN.load(Ordering::Relaxed);
            if h != 0 {
                let _ = PostMessageW(HWND(h as *mut _), WM_APP_TICK, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn main() {
    unsafe {
        let hinstance = GetModuleHandleW(None).expect("module handle");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let title = wide("SportsPulse Native [M1]");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            356,
            146,
            None,
            HMENU::default(),
            hinstance,
            None,
        )
        .expect("create window");

        let renderer = Box::new(Renderer::new(hwnd, 340, 110).expect("d2d renderer"));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(renderer) as isize);

        HMAIN.store(hwnd.0 as usize, Ordering::Release);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = InvalidateRect(hwnd, None, false);
        let _ = UpdateWindow(hwnd);

        let engine = spawn_engine();

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
        drop(engine);
        let _ = RECT::default();
    }
}
