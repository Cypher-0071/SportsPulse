//! Isolation test v2: A = normal paint (control, known visible)
//! C = SetLayeredWindowAttributes + GDI paint
//! D = ULW with explicit screen hdcDst + pptSrc(0,0)
//! E = ULW without AC_SRC_ALPHA (constant alpha only)

#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(non_snake_case)]

#[cfg(windows)]
use windows::core::*;
#[cfg(windows)]
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, FillRect, GetWindowDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBRUSH, HDC,
};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW,
    PostQuitMessage, RegisterClassExW, SetLayeredWindowAttributes, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HCURSOR, HMENU, IDC_ARROW,
    LWA_ALPHA, MSG, SW_SHOW, ULW_ALPHA, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_PAINT,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP,
};

#[cfg(windows)]
const W: i32 = 300;
#[cfg(windows)]
const H: i32 = 100;

/// Leak-once static wide string (P0-8): class/title pointers handed to Win32
/// must outlive the call. Intentional Box::leak — one small leak per distinct
/// dev-bin string, process-lifetime test tool.
#[cfg(windows)]
fn wide_static(s: &str) -> &'static [u16] {
    Box::leak(
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

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

#[cfg(windows)]
fn fill_dib_solid(bits: *mut core::ffi::c_void, r: u8, g: u8, b: u8, bottom_up: bool) {
    let row = (W * 4) as usize;
    let buf = unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, row * H as usize) };
    for y in 0..H as usize {
        for x in 0..W as usize {
            let dy = if bottom_up { H as usize - 1 - y } else { y };
            let i = dy * row + x * 4;
            buf[i] = b;
            buf[i + 1] = g;
            buf[i + 2] = r;
            buf[i + 3] = 255;
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn proc_c(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        // GDI paint: solid teal
        let hdc = windows::Win32::Graphics::Gdi::GetDC(hwnd);
        let brush = CreateSolidBrush(COLORREF(0x0088AA44)); // greenish
        let mut r = RECT {
            left: 0,
            top: 0,
            right: W,
            bottom: H,
        };
        FillRect(hdc, &mut r, brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(hwnd, hdc);
        let _ = ValidateRect(hwnd, None);
        return LRESULT(0);
    }
    match msg {
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

#[cfg(windows)]
unsafe extern "system" fn proc_def(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

#[cfg(windows)]
fn register(
    hinstance: windows::Win32::Foundation::HINSTANCE,
    name: &[u16],
    proc_fn: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> u16 {
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc_fn),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("C:\\sp_bench\\sptest_panic.log")
        {
            let _ = writeln!(
                f,
                "registered {} atom={}",
                String::from_utf16_lossy(name),
                atom
            );
        }
        atom
    }
}

#[cfg(windows)]
use windows::Win32::Foundation::SIZE;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::ValidateRect;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// P0-9: dev-bin fatal path — message box, then ExitProcess(1). No panics.
#[cfg(windows)]
fn fatal_bin(msg: &str) -> ! {
    MessageBoxW_simple(msg);
    unsafe { windows::Win32::System::Threading::ExitProcess(1) };
}

#[cfg(windows)]
fn main() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("C:\\sp_bench\\sptest_panic.log")
        {
            let _ = writeln!(f, "{info}");
        }
    }));
    unsafe {
        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => fatal_bin("sptest: GetModuleHandleW failed"),
        };
        let _atom_a = register(hinstance.into(), &wide_static("SPZ_A"), proc_def);
        let _atom_c = register(hinstance.into(), &wide_static("SPZ_C"), proc_c);
        let _atom_d = register(hinstance.into(), &wide_static("SPZ_D"), proc_def);
        let _atom_e = register(hinstance.into(), &wide_static("SPZ_E"), proc_def);

        let wa = work_area();
        let cy = (wa.top + wa.bottom) / 2 - H / 2;
        let cx = (wa.left + wa.right) / 2;
        // 4 windows in a row: A, C, D, E
        let xa = cx - 2 * W - 30;
        let xc = cx - W - 10;
        let xd = cx + 10;
        let xe = cx + W + 30;

        // ---- Mutation matrix: clone main.rs working pattern, mutate one var at a time ----
        use std::io::Write as IoWrite;
        let log = |msg: &str| {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("C:\\sp_bench\\sptest_panic.log")
            {
                let _ = writeln!(f, "{msg}");
            }
        };

        const M1_CLASS: [u16; 3] = [77, 49, 0]; // "M1\0"
        let wc_m1 = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc_def),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: PCWSTR(M1_CLASS.as_ptr()),
            ..Default::default()
        };
        let m1_atom = RegisterClassExW(&wc_m1);
        log(&format!("M1 class atom={m1_atom}"));

        // V1: exact main.rs pattern (static class, ex=0, OVERLAPPED, CW_USEDEFAULT)
        match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(M1_CLASS.as_ptr()),
            PCWSTR(wide_static("V1").as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(_) => log("V1 staticclass+ex0+overlapped+usedefault OK"),
            Err(e) => log(&format!("V1 FAILED: {e}")),
        }

        // V2: static class + TOPMOST + POPUP + coords
        match CreateWindowExW(
            WS_EX_TOPMOST,
            PCWSTR(M1_CLASS.as_ptr()),
            PCWSTR(wide_static("V2").as_ptr()),
            WS_POPUP,
            xa,
            cy,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(_) => log("V2 staticclass+topmost+popup+coords OK"),
            Err(e) => log(&format!("V2 FAILED: {e}")),
        }

        // V3: static class + ex0 + OVERLAPPED + CW_USEDEFAULT
        // (was wide() temp class; now leak-once wide_static — same coverage)
        match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(wide_static("SPZ_A").as_ptr()),
            PCWSTR(wide_static("V3").as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(_) => log("V3 tempclass+ex0+overlapped+usedefault OK"),
            Err(e) => log(&format!("V3 FAILED: {e}")),
        }

        // V4: static class + ex0 + OVERLAPPED + coords
        match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(M1_CLASS.as_ptr()),
            PCWSTR(wide_static("V4").as_ptr()),
            WS_OVERLAPPEDWINDOW,
            xa,
            cy,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(_) => log("V4 staticclass+ex0+overlapped+coords OK"),
            Err(e) => log(&format!("V4 FAILED: {e}")),
        }

        // panic!("matrix done - check log");

        // ---- C: SetLayeredWindowAttributes + GDI paint ----
        let hc = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST,
            PCWSTR(wide_static("SPZ_C").as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            xc,
            cy,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(w) => w,
            Err(_) => fatal_bin("sptest: window C creation failed"),
        };
        let _ = ShowWindow(hc, SW_SHOW);
        // paint content
        let hdc_c = GetDC(hc);
        let brush = CreateSolidBrush(COLORREF(0x0044AA88));
        let mut r = RECT {
            left: 0,
            top: 0,
            right: W,
            bottom: H,
        };
        FillRect(hdc_c, &mut r, brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
        ReleaseDC(hc, hdc_c);
        // constant alpha 255
        if let Err(e) = SetLayeredWindowAttributes(hc, COLORREF(0), 255, LWA_ALPHA) {
            MessageBoxW_simple(&format!("C SLWA failed: {e}"));
        }

        // ---- D: ULW with explicit screen hdcDst + pptSrc ----
        let hd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST,
            PCWSTR(wide_static("SPZ_D").as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            xd,
            cy,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(w) => w,
            Err(_) => fatal_bin("sptest: window D creation failed"),
        };
        let _ = ShowWindow(hd, SW_SHOW);
        let screen_dc = GetWindowDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = W;
        bmi.bmiHeader.biHeight = H; // bottom-up
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) => b,
            Err(_) => fatal_bin("sptest: DIB section D failed"),
        };
        // P0-7: park the previously selected bitmap; deleting a selected
        // GDI object is a no-op leak. Mem DCs live for process lifetime here.
        let _old_bmp_d = SelectObject(mem_dc, hbmp);
        fill_dib_solid(bits, 200, 60, 60, true); // blue-ish card
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pos = POINT { x: xd, y: cy };
        let size = SIZE { cx: W, cy: H };
        let src_pt = POINT { x: 0, y: 0 };
        match UpdateLayeredWindow(
            hd,
            screen_dc,
            Some(&pos),
            Some(&size),
            mem_dc,
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        ) {
            Ok(_) => {}
            Err(e) => MessageBoxW_simple(&format!("D ULW failed: {e}")),
        }

        // ---- E: ULW without AC_SRC_ALPHA (constant alpha via SourceConstantAlpha) ----
        let he = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST,
            PCWSTR(wide_static("SPZ_E").as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            xe,
            cy,
            W,
            H,
            None,
            HMENU::default(),
            hinstance,
            None,
        ) {
            Ok(w) => w,
            Err(_) => fatal_bin("sptest: window E creation failed"),
        };
        let _ = ShowWindow(he, SW_SHOW);
        let mem_dc2 = CreateCompatibleDC(screen_dc);
        let mut bits2: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp2 = match CreateDIBSection(mem_dc2, &bmi, DIB_RGB_COLORS, &mut bits2, None, 0) {
            Ok(b) => b,
            Err(_) => fatal_bin("sptest: DIB section E failed"),
        };
        // P0-7: park the previously selected bitmap (see D above).
        let _old_bmp_e = SelectObject(mem_dc2, hbmp2);
        fill_dib_solid(bits2, 60, 200, 60, true); // green card (RGB 60,200,60)
        let blend_e = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 220,
            AlphaFormat: 0, // NO per-pixel alpha
        };
        let pos_e = POINT { x: xe, y: cy };
        match UpdateLayeredWindow(
            he,
            HDC::default(),
            Some(&pos_e),
            Some(&size),
            mem_dc2,
            None,
            COLORREF(0),
            Some(&blend_e),
            ULW_ALPHA,
        ) {
            Ok(_) => {}
            Err(e) => MessageBoxW_simple(&format!("E ULW failed: {e}")),
        }

        let _ = (hbmp, hbmp2);
        // P0-7: balance GetWindowDC(None). Both ULWs above already consumed
        // screen_dc (D as explicit hdcDst); mem DCs stay alive for the windows.
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);

        let mut msg = MSG::default();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                break;
            }
        }
    }
}

#[cfg(windows)]
fn MessageBoxW_simple(s: &str) {
    unsafe {
        let text: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let title: Vec<u16> = b"sptest error\0"
            .to_vec()
            .iter()
            .map(|&b| b as u16)
            .collect();
        let _ = windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
            HWND::default(),
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            windows::Win32::UI::WindowsAndMessaging::MB_OK,
        );
    }
}

/// Non-Windows fallback (P0-9): dev bin is Win32-only, but must provide
/// `main` so `cargo check` passes on Linux.
#[cfg(not(windows))]
fn main() {
    eprintln!("sptest is Windows-only.");
}
