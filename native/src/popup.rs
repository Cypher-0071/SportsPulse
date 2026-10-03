//! SportsPulse — Win11 Dark Theme Event Mini-Popup (Wickets, Boundaries, Goals).
//! Auto-dismisses after 8 seconds for Win events, 5 seconds otherwise, or on click.
//! Pixel flow: D2D -> WIC bitmap -> CopyPixels -> DIB -> UpdateLayeredWindow.

#![allow(dead_code)]

use std::cell::RefCell;
use std::marker::PhantomData;
use std::time::Instant;

use windows::core::*;
use windows::Win32::Foundation::{
    COLORREF, D2DERR_RECREATE_TARGET, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint,
    GetWindowDC, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer,
    LoadCursorW, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, HMENU, HWND_TOPMOST,
    IDC_ARROW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
    ULW_ALPHA, WM_CLOSE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_PAINT, WM_TIMER,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::engine::models::{MatchEvent, MatchEventType};
use crate::render::{
    d2d_factory, dwrite_factory, has_word, high_contrast, reduced_motion, software_rt_props,
    ui_text, EVENT_TTL, WIN_TTL,
};

// Live scorecard footprint in design DIPs (340x110): physical window and
// render-target DPI scale with the monitor, so popup and scoreboard always
// match pixel-for-pixel on the same display.
pub const POPUP_W: u32 = 340;
pub const POPUP_H: u32 = 110;
const POPUP_CLASS: PCWSTR = w!("SPNativeMiniPopup");
const TIMER_AUTOHIDE_ID: usize = 1001;
/// Reveal/timeout-bar repaint tick (60fps while visible).
const TIMER_ANIM_ID: usize = 1002;
/// Reveal length (ms) and slide distance (physical px) for the ease-out
/// entrance: the card settles DOWN into its resting spot while fading in.
/// From above keeps every frame inside the work area (the 12px bottom inset
/// leaves no room for a from-below rise without clipping behind the taskbar).
const REVEAL_MS: f32 = 240.0;
const REVEAL_LIFT_PX: f32 = 48.0;
/// Animation repaint cadence (ms).
const ANIM_MS: u32 = 16;

/// Shared dismiss helper: stop every timer, hide, and park animation state
/// so a later show_event starts clean. Click, Esc, timeout, and
/// MiniPopupWindow::hide funnel through here (WM_DESTROY kills its timers
/// inline; hide() additionally clears MiniPopupWindow::current_event).
unsafe fn dismiss_popup(hwnd: HWND) {
    let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
    let _ = KillTimer(hwnd, TIMER_ANIM_ID);
    let _ = ShowWindow(hwnd, SW_HIDE);
    let renderer_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupRenderer;
    if let Some(renderer) = renderer_ptr.as_mut() {
        renderer.reveal_start = None;
        renderer.shown_at = None;
        renderer.ttl_ms = 0;
        renderer.last_event = None;
    }
}

#[inline]
fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// Same rules as `src/main.js` `getCleanEventDetail`.
fn clean_event_detail(description: &str, event_type: MatchEventType) -> String {
    if description.is_empty() {
        return String::new();
    }

    for suffix in [
        " Own Goal",
        " Goal",
        " Penalty",
        " Red Card",
        " Yellow Card",
    ] {
        if description.contains(suffix) {
            return description
                .split(suffix)
                .next()
                .unwrap_or(description)
                .trim()
                .to_string();
        }
    }

    match event_type {
        MatchEventType::Wicket => {
            if description.contains(':') {
                description
                    .split(':')
                    .next()
                    .unwrap_or(description)
                    .trim()
                    .to_string()
            } else {
                description.to_string()
            }
        }
        MatchEventType::Boundary => {
            if description.contains(':') {
                return description
                    .split(':')
                    .next()
                    .unwrap_or(description)
                    .trim()
                    .to_string();
            }
            if description.contains(" to ") {
                let after_to = description.split(" to ").nth(1).unwrap_or("");
                if !after_to.is_empty() && after_to.contains(',') {
                    return after_to
                        .split(',')
                        .next()
                        .unwrap_or(after_to)
                        .trim()
                        .to_string();
                }
                return if after_to.is_empty() {
                    description.to_string()
                } else {
                    after_to.to_string()
                };
            }
            description.to_string()
        }
        MatchEventType::Win => description.to_string(),
    }
}

struct Fmt {
    fmt: IDWriteTextFormat,
    // Persistent UTF-16 scratch buffer: avoids one Vec<u16> alloc per DrawText.
    buf: RefCell<Vec<u16>>,
}

impl Fmt {
    unsafe fn text(
        &self,
        rt: &ID2D1RenderTarget,
        s: &str,
        rect: &D2D_RECT_F,
        brush: &ID2D1SolidColorBrush,
    ) {
        let mut buf = self.buf.borrow_mut();
        buf.clear();
        buf.extend(s.encode_utf16());
        rt.DrawText(
            buf.as_slice(),
            &self.fmt,
            rect,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

pub struct PopupRenderer {
    hwnd: HWND,
    wic_factory: IWICImagingFactory,
    wic: IWICBitmap,
    rt: ID2D1RenderTarget,
    mem_dc: HDC,
    hbmp: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut core::ffi::c_void,
    w: i32,
    h: i32,
    dpi: u32,
    bg_brush: ID2D1SolidColorBrush,
    border_brush: ID2D1SolidColorBrush,
    white_brush: ID2D1SolidColorBrush,
    dim_brush: ID2D1SolidColorBrush,
    subtle_brush: ID2D1SolidColorBrush,
    wicket_bg: ID2D1SolidColorBrush,
    wicket_text: ID2D1SolidColorBrush,
    redcard_bg: ID2D1SolidColorBrush,
    redcard_text: ID2D1SolidColorBrush,
    four_bg: ID2D1SolidColorBrush,
    four_text: ID2D1SolidColorBrush,
    six_bg: ID2D1SolidColorBrush,
    six_text: ID2D1SolidColorBrush,
    goal_bg: ID2D1SolidColorBrush,
    goal_text: ID2D1SolidColorBrush,
    win_bg: ID2D1SolidColorBrush,
    win_text: ID2D1SolidColorBrush,
    fmt_badge: Fmt,
    fmt_emoji: Fmt,
    fmt_title: Fmt,
    fmt_desc: Fmt,
    fmt_score: Fmt,
    // Last presented event, so WM_PAINT can re-present after RDP/UAC blanks.
    last_event: Option<MatchEvent>,
    /// Reveal/timeout animation state. `target` is the resting screen pos;
    /// `reveal_start` drives the 240ms ease-out slide+fade after show_event;
    /// `shown_at`/`ttl_ms` drive the accent timeout bar. All cleared on hide.
    pub target: POINT,
    pub reveal_start: Option<Instant>,
    pub shown_at: Option<Instant>,
    pub ttl_ms: u32,
    /// Re-entrancy guard: true while a &mut borrow is live across a
    /// synchronous Win32 call (ULW/SetWindowPos/ShowWindow). popup_wnd_proc
    /// must DefWindowProcW while set instead of re-borrowing the renderer.
    pub in_present: bool,
    // STA-bound COM/GDI state must never cross threads.
    _no_send: PhantomData<*const ()>,
}

impl PopupRenderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32, dpi: u32) -> Result<Self> {
        // Shared process-lifetime factories (WIC stays per-renderer: fixed size, no resize).
        let factory = d2d_factory()?;
        let dwrite = dwrite_factory()?;
        let wicf: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

        let wic =
            wicf.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;

        let rt = factory.CreateWicBitmapRenderTarget(&wic, &software_rt_props())?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        // Design-DIP mapping: the physical bitmap reports POPUP_W/H DIPs so
        // the interior layout renders at the monitor's DPI, exactly like the
        // scoreboard overlay it stacks against.
        let ddpi = if dpi == 0 { 96.0 } else { dpi as f32 };
        rt.SetDpi(ddpi, ddpi);

        // Fallible D2D/DWrite objects first: any ? below returns before any
        // GDI object exists, so late failures cannot leak DCs/bitmaps.
        // Unified palette: opaque #202020 bg + #3E3E3E border, shared with overlay/dashboard.
        let bg_brush = rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?;
        let border_brush = rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?;
        let white_brush = rt.CreateSolidColorBrush(&color(0.96, 0.96, 0.98, 1.0), None)?;
        let dim_brush = rt.CreateSolidColorBrush(&color(0.62, 0.62, 0.67, 1.0), None)?;
        // Raised to dim range (was 0.42): description text must clear 4.5.
        let subtle_brush = rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?;

        // Event Badge Colors:
        // Wicket: Soft Red
        let wicket_bg = rt.CreateSolidColorBrush(&color(0.600, 0.106, 0.106, 0.35), None)?;
        let wicket_text = rt.CreateSolidColorBrush(&color(0.973, 0.294, 0.333, 1.0), None)?;

        // Soccer red card: light red text on the dark-red pill (#FF6B74 clears
        // 4.5 on dark; the old #D90429 text did not).
        let redcard_bg = rt.CreateSolidColorBrush(&color(0.851, 0.016, 0.161, 0.40), None)?;
        let redcard_text = rt.CreateSolidColorBrush(&color(1.0, 0.42, 0.454, 1.0), None)?;

        // Boundary Four: Sky Blue / Cyan
        let four_bg = rt.CreateSolidColorBrush(&color(0.012, 0.412, 0.631, 0.35), None)?;
        let four_text = rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?;

        // Boundary Six: Win11 Emerald Green
        let six_bg = rt.CreateSolidColorBrush(&color(0.082, 0.502, 0.239, 0.35), None)?;
        let six_text = rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?;

        // Goal: Amber / Gold
        let goal_bg = rt.CreateSolidColorBrush(&color(0.706, 0.325, 0.035, 0.35), None)?;
        let goal_text = rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?;

        // Win: Purple / Indigo
        let win_bg = rt.CreateSolidColorBrush(&color(0.450, 0.150, 0.750, 0.35), None)?;
        let win_text = rt.CreateSolidColorBrush(&color(0.750, 0.450, 0.980, 1.0), None)?;

        let mk_font =
            |size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT| -> Result<Fmt> {
                let fmt = dwrite.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )?;
                fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                fmt.SetTextAlignment(align)?;
                fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                Ok(Fmt {
                    fmt,
                    buf: RefCell::new(Vec::new()),
                })
            };

        // Emoji badge font: "Segoe UI" has no 🏆/🟥/🏏/⚽/💥/⚡ glyphs, so badge
        // labels get an explicit Emoji family instead of tofu boxes.
        // Mirrors dashboard.rs fmt_icon (Segoe UI Emoji) pattern.
        let fmt_emoji_fmt = dwrite.CreateTextFormat(
            w!("Segoe UI Emoji"),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            14.0,
            w!("en-us"),
        )?;
        fmt_emoji_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        fmt_emoji_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        fmt_emoji_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

        // All remaining fallible formats up front too: after this point the
        // only ? left is the guarded CreateDIBSection below, so GDI objects
        // can never leak through a DWrite failure.
        let fmt_badge = mk_font(14.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let fmt_title = mk_font(
            15.2,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        )?;
        let fmt_desc = mk_font(
            13.0,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        )?;
        let fmt_score = mk_font(
            14.0,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
        )?;

        // GDI last: from here on the only ? is CreateDIBSection, guarded with
        // DeleteDC inline, so no failure path leaks.
        let screen_dc = GetWindowDC(None);
        if screen_dc.is_invalid() {
            return Err(Error::from_win32());
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        // Released before any fallible op below, so every early-`?` path is covered.
        let _ = ReleaseDC(None, screen_dc);
        if mem_dc.is_invalid() {
            return Err(Error::from_win32());
        }
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w as i32;
        bmi.bmiHeader.biHeight = -(h as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if w == 0 || h == 0 {
            let _ = DeleteDC(mem_dc);
            return Err(Error::from_win32());
        }
        let hbmp = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(h) => h,
            Err(e) => {
                let _ = DeleteDC(mem_dc);
                return Err(e);
            }
        };
        if hbmp.is_invalid() {
            let _ = DeleteDC(mem_dc);
            return Err(Error::from_win32());
        }
        if bits.is_null() {
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(mem_dc);
            return Err(Error::from_win32());
        }
        let old_bmp = SelectObject(mem_dc, hbmp);
        if old_bmp.is_invalid() {
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(mem_dc);
            return Err(Error::from_win32());
        }

        Ok(Self {
            hwnd,
            wic_factory: wicf,
            wic,
            rt,
            mem_dc,
            hbmp,
            old_bmp,
            bits,
            w: w as i32,
            h: h as i32,
            dpi,
            bg_brush,
            border_brush,
            white_brush,
            dim_brush,
            subtle_brush,
            wicket_bg,
            wicket_text,
            redcard_bg,
            redcard_text,
            four_bg,
            four_text,
            six_bg,
            six_text,
            goal_bg,
            goal_text,
            win_bg,
            win_text,
            fmt_badge,
            fmt_emoji: Fmt {
                fmt: fmt_emoji_fmt,
                buf: RefCell::new(Vec::new()),
            },
            fmt_title,
            fmt_desc,
            fmt_score,
            last_event: None,
            target: POINT { x: 0, y: 0 },
            reveal_start: None,
            shown_at: None,
            ttl_ms: 0,
            in_present: false,
            _no_send: PhantomData,
        })
    }

    /// Force a full rebuild at the current size (D2DERR_RECREATE_TARGET
    /// recovery). The popup is fixed-size so there is no resize(); callers
    /// retry present() once after this returns Ok — the same policy as the
    /// overlay/dashboard outside-paint paths in main.rs. Atomic: all fallible
    /// work happens in locals; self commits only after the last success. Any
    /// failure frees what was staged and leaves self untouched.
    pub unsafe fn recreate(&mut self) -> Result<()> {
        if self.w <= 0 || self.h <= 0 {
            return Err(Error::from_win32());
        }

        let factory = d2d_factory()?;
        let wic = self.wic_factory.CreateBitmap(
            self.w as u32,
            self.h as u32,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnDemand,
        )?;
        let rt = factory.CreateWicBitmapRenderTarget(&wic, &software_rt_props())?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let ddpi = if self.dpi == 0 { 96.0 } else { self.dpi as f32 };
        rt.SetDpi(ddpi, ddpi);

        // Resolve the DC to build against without touching self yet; a fresh
        // DC is staged only when the old one was lost.
        let mut staged_dc: Option<HDC> = None;
        let build_dc = if self.mem_dc.is_invalid() {
            let screen_dc = GetWindowDC(None);
            if screen_dc.is_invalid() {
                return Err(Error::from_win32());
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            let _ = ReleaseDC(None, screen_dc);
            if mem_dc.is_invalid() {
                return Err(Error::from_win32());
            }
            staged_dc = Some(mem_dc);
            mem_dc
        } else {
            self.mem_dc
        };

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = self.w;
        bmi.bmiHeader.biHeight = -self.h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp_result = CreateDIBSection(build_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        let (hbmp, new_bits) = match hbmp_result {
            Ok(h) if !h.is_invalid() && !bits.is_null() => (h, bits),
            Ok(h) => {
                if !h.is_invalid() {
                    let _ = DeleteObject(h);
                }
                if let Some(dc) = staged_dc {
                    let _ = DeleteDC(dc);
                }
                return Err(Error::from_win32());
            }
            Err(e) => {
                if let Some(dc) = staged_dc {
                    let _ = DeleteDC(dc);
                }
                return Err(e);
            }
        };

        // Rebuild every brush against the new target in locals (same palette
        // as new()). One failure frees the staged hbmp/DC; self stays intact.
        let brushes: Result<[ID2D1SolidColorBrush; 17]> = (|| {
            Ok([
                rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.96, 0.96, 0.98, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.62, 0.62, 0.67, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.600, 0.106, 0.106, 0.35), None)?,
                rt.CreateSolidColorBrush(&color(0.973, 0.294, 0.333, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.851, 0.016, 0.161, 0.40), None)?,
                rt.CreateSolidColorBrush(&color(1.0, 0.42, 0.454, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.012, 0.412, 0.631, 0.35), None)?,
                rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.082, 0.502, 0.239, 0.35), None)?,
                rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.706, 0.325, 0.035, 0.35), None)?,
                rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?,
                rt.CreateSolidColorBrush(&color(0.450, 0.150, 0.750, 0.35), None)?,
                rt.CreateSolidColorBrush(&color(0.750, 0.450, 0.980, 1.0), None)?,
            ])
        })();
        let [bg_brush, border_brush, white_brush, dim_brush, subtle_brush, wicket_bg, wicket_text, redcard_bg, redcard_text, four_bg, four_text, six_bg, six_text, goal_bg, goal_text, win_bg, win_text] =
            match brushes {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteObject(hbmp);
                    if let Some(dc) = staged_dc {
                        let _ = DeleteDC(dc);
                    }
                    return Err(e);
                }
            };

        // Commit only after the last fallible op succeeded. Park the old
        // bitmap first; deleting a selected GDI object is a no-op leak.
        if !self.mem_dc.is_invalid() {
            let _ = SelectObject(self.mem_dc, self.old_bmp);
        }
        if !self.hbmp.is_invalid() {
            let _ = DeleteObject(self.hbmp);
        }
        if let Some(dc) = staged_dc {
            self.mem_dc = dc;
        }
        self.old_bmp = SelectObject(self.mem_dc, hbmp);
        self.wic = wic;
        self.rt = rt;
        self.hbmp = hbmp;
        self.bits = new_bits;
        self.bg_brush = bg_brush;
        self.border_brush = border_brush;
        self.white_brush = white_brush;
        self.dim_brush = dim_brush;
        self.subtle_brush = subtle_brush;
        self.wicket_bg = wicket_bg;
        self.wicket_text = wicket_text;
        self.redcard_bg = redcard_bg;
        self.redcard_text = redcard_text;
        self.four_bg = four_bg;
        self.four_text = four_text;
        self.six_bg = six_bg;
        self.six_text = six_text;
        self.goal_bg = goal_bg;
        self.goal_text = goal_text;
        self.win_bg = win_bg;
        self.win_text = win_text;

        Ok(())
    }

    pub fn last_event(&self) -> Option<MatchEvent> {
        self.last_event.clone()
    }

    fn event_badge<'a>(
        &'a self,
        event: &MatchEvent,
    ) -> (
        &'static str,
        &'a ID2D1SolidColorBrush,
        &'a ID2D1SolidColorBrush,
    ) {
        // Uppercase once; event_type decides first, title words refine with
        // word boundaries (never naive OUT/WIN substrings: SHOUT/WING).
        let upper = event.title.to_uppercase();
        match event.event_type {
            MatchEventType::Win => ("🏆 RESULT", &self.win_bg, &self.win_text),
            MatchEventType::Wicket => {
                if has_word(&upper, "RED") && has_word(&upper, "CARD") {
                    ("🟥 CARD", &self.redcard_bg, &self.redcard_text)
                } else {
                    ("🏏 WICKET", &self.wicket_bg, &self.wicket_text)
                }
            }
            MatchEventType::Boundary => {
                if has_word(&upper, "GOAL") {
                    ("⚽ GOAL", &self.goal_bg, &self.goal_text)
                } else if has_word(&upper, "SIX") {
                    ("💥 SIX", &self.six_bg, &self.six_text)
                } else {
                    ("⚡ FOUR", &self.four_bg, &self.four_text)
                }
            }
        }
    }

    pub unsafe fn present(&mut self, pos: &POINT, event: &MatchEvent) -> Result<()> {
        self.last_event = Some(event.clone());
        // Layout in design DIPs (overlay pattern): the render target reports
        // POPUP_W/H DIPs at any monitor DPI, so the full card always lands
        // on-bitmap. Physical size stays on self.w/h for the ULW handoff.
        let dip = self.rt.GetSize();
        let (w, h) = (dip.width, dip.height);
        let now = Instant::now();

        // Reveal: 240ms ease-out-cubic settle-down + fade. While a reveal is
        // active the card draws at the stored resting target, so a
        // mid-animation WM_PAINT re-present lands on the same trajectory.
        let base = if self.reveal_start.is_some() {
            self.target
        } else {
            *pos
        };
        let (lift_px, alpha) = match self.reveal_start {
            Some(t0) if !reduced_motion() => {
                let t = (now - t0).as_secs_f32() / (REVEAL_MS / 1000.0);
                if t >= 1.0 {
                    self.reveal_start = None;
                    (0.0, 255)
                } else {
                    let e = 1.0 - (1.0 - t).powi(3);
                    ((1.0 - e) * REVEAL_LIFT_PX, (e * 255.0).round() as u8)
                }
            }
            _ => {
                self.reveal_start = None;
                (0.0, 255)
            }
        };
        self.rt.BeginDraw();
        self.rt.Clear(None);

        let full_rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: full_rect,
            radiusX: 12.0,
            radiusY: 12.0,
        };
        self.rt.FillRoundedRectangle(&rr, &self.bg_brush);
        // HC: bg is already opaque; bump the border to 2px.
        let border_w = if high_contrast() { 2.0 } else { 1.0 };
        self.rt
            .DrawRoundedRectangle(&rr, &self.border_brush, border_w, None);

        let (badge_label, badge_bg, badge_text) = self.event_badge(event);

        // Top Row: Badge Pill on left, Current Score on right
        let badge_rect = D2D_RECT_F {
            left: 14.0,
            top: 12.0,
            right: 114.0,
            bottom: 34.0,
        };
        let badge_rr = D2D1_ROUNDED_RECT {
            rect: badge_rect,
            radiusX: 5.0,
            radiusY: 5.0,
        };
        self.rt.FillRoundedRectangle(&badge_rr, badge_bg);
        // Badge labels carry emoji (🏆/🟥/🏏/⚽/💥/⚡): must use the Emoji
        // family, never Segoe UI (tofu).
        self.fmt_emoji
            .text(&self.rt, badge_label, &badge_rect, badge_text);

        let score_rect = D2D_RECT_F {
            left: 122.0,
            top: 12.0,
            right: w - 14.0,
            bottom: 34.0,
        };
        self.fmt_score.text(
            &self.rt,
            &ui_text(&event.score, 24),
            &score_rect,
            &self.white_brush,
        );

        // Middle Row: Event Headline
        let title_rect = D2D_RECT_F {
            left: 14.0,
            top: 38.0,
            right: w - 14.0,
            bottom: 66.0,
        };
        self.fmt_title.text(
            &self.rt,
            &ui_text(&event.title, 48),
            &title_rect,
            &self.white_brush,
        );

        // Bottom Row: Description / Subtext
        let desc_rect = D2D_RECT_F {
            left: 14.0,
            top: 66.0,
            right: w - 14.0,
            bottom: 90.0,
        };
        let clean_desc = clean_event_detail(&event.description, event.event_type);
        self.fmt_desc.text(
            &self.rt,
            &ui_text(&clean_desc, 80),
            &desc_rect,
            &self.dim_brush,
        );

        // Accent timeout bar: drains over the auto-hide TTL in the event's
        // own accent color. Static full bar under reduced-motion.
        if self.ttl_ms > 0 {
            let frac = match self.shown_at {
                Some(t0) if !reduced_motion() => {
                    (1.0 - (now - t0).as_secs_f32() / (self.ttl_ms as f32 / 1000.0))
                        .clamp(0.0, 1.0)
                }
                _ => 1.0,
            };
            let bar_w = (w - 24.0) * frac;
            if bar_w > 1.0 {
                self.rt.FillRectangle(
                    &D2D_RECT_F {
                        left: 12.0,
                        top: h - 9.0,
                        right: 12.0 + bar_w,
                        bottom: h - 6.0,
                    },
                    badge_text,
                );
            }
        }

        self.rt.EndDraw(None, None)?;

        if self.w <= 0 || self.h <= 0 || self.bits.is_null() {
            return Err(Error::from_win32());
        }
        let row_pitch = (self.w as usize)
            .checked_mul(4)
            .ok_or_else(Error::from_win32)?;
        let total_bytes = row_pitch
            .checked_mul(self.h as usize)
            .ok_or_else(Error::from_win32)?;
        if total_bytes == 0 {
            return Err(Error::from_win32());
        }
        let row_pitch_u32 = u32::try_from(row_pitch).map_err(|_| Error::from_win32())?;
        let dib_slice = std::slice::from_raw_parts_mut(self.bits as *mut u8, total_bytes);
        self.wic
            .CopyPixels(std::ptr::null(), row_pitch_u32, dib_slice)?;

        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let draw_pos = POINT {
            x: base.x,
            y: base.y - lift_px.round() as i32,
        };

        // Re-entrancy guard: ULW can synchronously re-enter popup_wnd_proc
        // while this &mut borrow is live.
        self.in_present = true;
        let ulw = UpdateLayeredWindow(
            self.hwnd,
            None,
            Some(&draw_pos),
            Some(&size),
            self.mem_dc,
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        self.in_present = false;
        ulw
    }
}

impl Drop for PopupRenderer {
    fn drop(&mut self) {
        unsafe {
            // Restore the previously selected bitmap before deleting ours.
            if !self.mem_dc.is_invalid() {
                let _ = SelectObject(self.mem_dc, self.old_bmp);
            }
            if !self.hbmp.is_invalid() {
                let _ = DeleteObject(self.hbmp);
            }
            if !self.mem_dc.is_invalid() {
                let _ = DeleteDC(self.mem_dc);
            }
        }
    }
}

pub struct MiniPopupWindow {
    pub hwnd: HWND,
    renderer: Box<PopupRenderer>,
    pub current_event: Option<MatchEvent>,
}

unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Re-entrancy guard: present()/show_event() hold a &mut renderer across
    // synchronous ULW/SetWindowPos/ShowWindow calls that can re-enter here.
    // DefWindowProcW instead of creating a second borrow (aliasing the live
    // &mut would be undefined behavior). Plain bool read via raw pointer.
    let outer_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupRenderer;
    if !outer_ptr.is_null() {
        let guarded = std::ptr::addr_of!((*outer_ptr).in_present).read();
        if guarded {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
    }
    match msg {
        WM_PAINT => {
            // Layered windows must not rely on WM_PAINT for ULW, but RDP/UAC
            // can blank the surface — re-present the last event if we have one.
            let renderer_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupRenderer;
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            // Dismiss hygiene: never re-present (revive the ULW surface) once
            // hidden; dismiss clears last_event/ttl anyway.
            if IsWindowVisible(hwnd).as_bool() {
                if let Some(renderer) = renderer_ptr.as_mut() {
                    if let Some(event) = renderer.last_event() {
                        let mut rect = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut rect);
                        let pos = POINT {
                            x: rect.left,
                            y: rect.top,
                        };
                        if let Err(e) = renderer.present(&pos, &event) {
                            // P0-7: EndDraw RECREATE_TARGET → recreate + retry once,
                            // same policy as the overlay/dashboard paths in main.rs.
                            if e.code() == D2DERR_RECREATE_TARGET && renderer.recreate().is_ok() {
                                let _ = renderer.present(&pos, &event);
                            }
                        }
                    }
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_AUTOHIDE_ID {
                dismiss_popup(hwnd);
                LRESULT(0)
            } else if wparam.0 == TIMER_ANIM_ID {
                // Reveal/timeout-bar frame: re-present the last event at the
                // current animation instant (same recreate policy as WM_PAINT).
                // Skip while hidden so a stray tick cannot revive a dismissed
                // popup.
                if !IsWindowVisible(hwnd).as_bool() {
                    return LRESULT(0);
                }
                let renderer_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupRenderer;
                if let Some(renderer) = renderer_ptr.as_mut() {
                    if let Some(event) = renderer.last_event() {
                        let target = renderer.target;
                        if let Err(e) = renderer.present(&target, &event) {
                            if e.code() == D2DERR_RECREATE_TARGET && renderer.recreate().is_ok() {
                                let _ = renderer.present(&target, &event);
                            }
                        }
                    }
                }
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            // Click to dismiss
            dismiss_popup(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // Esc dismisses when the popup has focus. Note: the popup is
            // intentionally WS_EX_NOACTIVATE (non-focusable by design, no tab
            // stops) so this is a backstop — click and the auto-hide timer are
            // the primary dismiss paths. Screen-reader users get the same
            // event via the overlay flash + tray tooltip.
            if wparam.0 == VK_ESCAPE.0 as usize {
                dismiss_popup(hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            dismiss_popup(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
            let _ = KillTimer(hwnd, TIMER_ANIM_ID);
            // The renderer Box is owned by MiniPopupWindow (freed with AppState);
            // clear the pointer so no post-destroy message can dereference it.
            // (main WM_DESTROY destroys this window before the Box is freed.)
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl MiniPopupWindow {
    pub unsafe fn create(w: u32, h: u32, dpi: u32) -> Result<Self> {
        // P0-9: never panic on GUI startup paths; propagate so main can
        // show MessageBoxW + ExitProcess(1).
        let hinstance = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(popup_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR::default()),
            lpszClassName: POPUP_CLASS,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            POPUP_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            w as i32,
            h as i32,
            None,
            HMENU::default(),
            hinstance,
            None,
        )?;

        let mut renderer = Box::new(PopupRenderer::new(hwnd, w, h, dpi)?);
        // Expose the renderer to popup_wnd_proc for WM_PAINT re-present.
        // The Box is heap-stable; the pointer stays valid for the window lifetime.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, renderer.as_mut() as *mut _ as isize);

        Ok(Self {
            hwnd,
            renderer,
            current_event: None,
        })
    }

    /// Re-sync the window + renderer to a (possibly new) monitor DPI before
    /// showing. The scoreboard overlay re-scales on WM_DPICHANGED; popups are
    /// transient, so they re-sync lazily here instead — same physical size as
    /// the overlay on whatever monitor hosts the main window. No-op when
    /// nothing changed.
    pub unsafe fn sync_size(&mut self, w: u32, h: u32, dpi: u32) {
        let r = &mut *self.renderer;
        if r.w == w as i32 && r.h == h as i32 && r.dpi == dpi {
            return;
        }
        // Atomicity: stage the new size, revert on failed rebuild so fields
        // never disagree with the live bitmap.
        let (old_w, old_h, old_dpi) = (r.w, r.h, r.dpi);
        r.w = w as i32;
        r.h = h as i32;
        r.dpi = dpi;
        if r.recreate().is_err() {
            r.w = old_w;
            r.h = old_h;
            r.dpi = old_dpi;
            return;
        }
        // Resize the window only after the bitmap rebuild succeeded.
        r.in_present = true;
        let _ = SetWindowPos(
            self.hwnd,
            None,
            0,
            0,
            w as i32,
            h as i32,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        r.in_present = false;
    }

    /// Re-present the current frame in place: no animation restart, no timer
    /// changes. Used after DPI/monitor-move re-syncs.
    pub unsafe fn repaint(&mut self) {
        let target = self.renderer.target;
        if let Some(event) = self.renderer.last_event() {
            if let Err(e) = self.renderer.present(&target, &event) {
                if e.code() == D2DERR_RECREATE_TARGET && self.renderer.recreate().is_ok() {
                    let _ = self.renderer.present(&target, &event);
                }
            }
        }
    }

    /// DPI/monitor-move caretaker for a visible popup: re-sync size, move to
    /// the fresh corner, repaint the current frame. No animation restart, no
    /// timer changes. Called from the main WM_DPICHANGED/WM_DISPLAYCHANGE
    /// paths; no-op when hidden or when nothing changed.
    pub unsafe fn relayout(&mut self, w: u32, h: u32, dpi: u32, pos: POINT) {
        self.sync_size(w, h, dpi);
        self.renderer.target = pos;
        self.repaint();
    }

    pub unsafe fn show_event(&mut self, event: MatchEvent, pos: POINT) {
        let timeout_ms: u32 = if event.event_type == MatchEventType::Win {
            WIN_TTL.as_millis() as u32
        } else {
            EVENT_TTL.as_millis() as u32
        };
        // Arm the entrance + timeout-bar animation before the first frame.
        // Reduced-motion: appear instantly with a static full bar.
        let now = Instant::now();
        self.renderer.target = pos;
        self.renderer.shown_at = Some(now);
        self.renderer.ttl_ms = timeout_ms;
        self.renderer.reveal_start = if reduced_motion() { None } else { Some(now) };
        // Z-order first (hidden, no move/size), then the first frame, then
        // show: the HWND never disagrees with the ULW surface, and no stale
        // bitmap flashes.
        self.renderer.in_present = true;
        let _ = SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        self.renderer.in_present = false;
        if let Err(e) = self.renderer.present(&pos, &event) {
            // P0-7: EndDraw RECREATE_TARGET (device lost) must recreate +
            // retry once, never be swallowed. Other errors stay best-effort.
            if e.code() == D2DERR_RECREATE_TARGET && self.renderer.recreate().is_ok() {
                let _ = self.renderer.present(&pos, &event);
            }
        }
        self.renderer.in_present = true;
        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        self.renderer.in_present = false;
        // Best-effort in release; fail loudly in debug so a lost autohide
        // tick (stuck-visible popup) can't hide silently.
        debug_assert_ne!(
            SetTimer(self.hwnd, TIMER_AUTOHIDE_ID, timeout_ms, None),
            0,
            "SetTimer failed"
        );
        if !reduced_motion() {
            debug_assert_ne!(
                SetTimer(self.hwnd, TIMER_ANIM_ID, ANIM_MS, None),
                0,
                "SetTimer failed"
            );
        }
        self.current_event = Some(event);
    }

    pub unsafe fn hide(&mut self) {
        // Guard the synchronous ShowWindow inside: a re-entrant proc must see
        // the flag and DefWindowProcW instead of re-borrowing the renderer.
        self.renderer.in_present = true;
        dismiss_popup(self.hwnd);
        self.renderer.in_present = false;
        self.current_event = None;
    }
}
