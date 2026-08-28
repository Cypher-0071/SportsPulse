//! Minimal ULW viability test: three small windows at top-left.
//! R1: ULW (red) then ShowWindow
//! R2: ShowWindow then ULW (lime)
//! R3: SetLayeredWindowAttributes + GDI paint (cyan)

#![windows_subsystem = "windows"]
#![cfg(windows)]
#![allow(non_snake_case)]

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, FillRect, GetWindowDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BI_RGB, DIB_RGB_COLORS, HDC, HBITMAP,
    AC_SRC_OVER, AC_SRC_ALPHA,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW,
    RegisterClassExW, ShowWindow, DestroyWindow, PostQuitMessage, TranslateMessage,
    HCURSOR, HMENU, IDC_ARROW, MSG, SW_SHOWNOACTIVATE, WS_POPUP,
    WS_EX_LAYERED, WS_EX_TOPMOST, UpdateLayeredWindow, ULW_ALPHA,
    SetLayeredWindowAttributes, LWA_ALPHA, WNDCLASSEXW, WM_CLOSE, WM_DESTROY,
};

const W: i32 = 260;
const H: i32 = 90;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn proc_def(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => { let _ = DestroyWindow(hwnd); LRESULT(0) }
        WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true)
            .open("C:\\sp_bench\\redbox_panic.log") {
            let _ = writeln!(f, "{info}");
        }
    }));
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let hinstance = GetModuleHandleW(None).unwrap();
        const RB_CLASS: [u16; 3] = [82, 66, 0]; // "RB\0"
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_def),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            lpszClassName: PCWSTR(RB_CLASS.as_ptr()),
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            let err = windows::Win32::Foundation::GetLastError();
            panic!("RegisterClassExW failed: {err:?}");
        }

        let mk_dib = |dc: HDC| -> (HBITMAP, *mut core::ffi::c_void) {
            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = W;
            bmi.bmiHeader.biHeight = H; // bottom-up
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB.0;
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let hbmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            (hbmp, bits)
        };

        let fill = |bits: *mut core::ffi::c_void, r: u8, g: u8, b: u8| {
            let row = (W * 4) as usize;
            let buf = std::slice::from_raw_parts_mut(bits as *mut u8, row * H as usize);
            for y in 0..H as usize {
                for x in 0..W as usize {
                    let i = y * row + x * 4;
                    buf[i] = b; buf[i + 1] = g; buf[i + 2] = r; buf[i + 3] = 255;
                }
            }
        };

        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8, BlendFlags: 0,
            SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8,
        };

        let screen_dc = GetWindowDC(None);
        let src_pt = POINT { x: 0, y: 0 };

        // R1: ULW then Show
        let h1 = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST, PCWSTR(RB_CLASS.as_ptr()), PCWSTR::null(),
            WS_POPUP, 100, 100, W, H, None, HMENU::default(), hinstance, None,
        ).expect("R1 create");
        let mem1 = CreateCompatibleDC(screen_dc);
        let (bmp1, bits1) = mk_dib(mem1);
        SelectObject(mem1, bmp1);
        fill(bits1, 255, 40, 40); // red
        let r1 = UpdateLayeredWindow(h1, HDC::default(), Some(&POINT { x: 100, y: 100 }),
            Some(&SIZE { cx: W, cy: H }), mem1, Some(&src_pt), COLORREF(0), Some(&blend), ULW_ALPHA);
        let _ = ShowWindow(h1, SW_SHOWNOACTIVATE);
        let _ = r1;

        // R2: Show then ULW
        let h2 = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST, PCWSTR(RB_CLASS.as_ptr()), PCWSTR::null(),
            WS_POPUP, 100, 240, W, H, None, HMENU::default(), hinstance, None,
        ).expect("R2 create");
        let mem2 = CreateCompatibleDC(screen_dc);
        let (bmp2, bits2) = mk_dib(mem2);
        SelectObject(mem2, bmp2);
        fill(bits2, 40, 255, 40); // lime
        let _ = ShowWindow(h2, SW_SHOWNOACTIVATE);
        let r2 = UpdateLayeredWindow(h2, HDC::default(), Some(&POINT { x: 100, y: 240 }),
            Some(&SIZE { cx: W, cy: H }), mem2, Some(&src_pt), COLORREF(0), Some(&blend), ULW_ALPHA);
        let _ = r2;

        // R3: SLWA + GDI paint
        let h3 = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST, PCWSTR(wide("RB").as_ptr()), PCWSTR::null(),
            WS_POPUP, 100, 380, W, H, None, HMENU::default(), hinstance, None,
        ).expect("R3 create");
        let hdc3 = windows::Win32::Graphics::Gdi::GetDC(h3);
        let brush = CreateSolidBrush(COLORREF(0x00FFFF00)); // cyan-ish (BGR: 00FFFF00 = B00? -> RGB(0,255,255) cyan)
        let mut r = RECT { left: 0, top: 0, right: W, bottom: H };
        FillRect(hdc3, &mut r, brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
        windows::Win32::Graphics::Gdi::ReleaseDC(h3, hdc3);
        let _ = ShowWindow(h3, SW_SHOWNOACTIVATE);
        let _ = SetLayeredWindowAttributes(h3, COLORREF(0), 255, LWA_ALPHA);

        let mut msg = MSG::default();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else { break; }
        }
    }
}
