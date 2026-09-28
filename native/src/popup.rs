//! SportsPulse — Win11 Dark Theme Event Mini-Popup (Wickets, Boundaries, Goals).
//! Auto-dismisses after 8 seconds for Win events, 5 seconds otherwise, or on click.
//! Pixel flow: D2D -> WIC bitmap -> CopyPixels -> DIB -> UpdateLayeredWindow.

#![allow(dead_code)]

use std::cell::RefCell;
use std::marker::PhantomData;

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
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, GetWindowRect, KillTimer, LoadCursorW,
    RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, UpdateLayeredWindow,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, HMENU, HWND_TOPMOST, IDC_ARROW, SWP_NOACTIVATE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_CLOSE, WM_DESTROY, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::engine::models::{MatchEvent, MatchEventType};
use crate::render::{
    d2d_factory, dwrite_factory, has_word, high_contrast, software_rt_props, ui_text, EVENT_TTL,
    WIN_TTL,
};

pub const POPUP_W: u32 = 320;
pub const POPUP_H: u32 = 84;
const POPUP_CLASS: PCWSTR = w!("SPNativeMiniPopup");
const TIMER_AUTOHIDE_ID: usize = 1001;

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
    // Persistent pixel scratch buffer: avoids a per-present alloc + double copy.
    buf: Vec<u8>,
    // STA-bound COM/GDI state must never cross threads.
    _no_send: PhantomData<*const ()>,
}

impl PopupRenderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32) -> Result<Self> {
        // Shared process-lifetime factories (WIC stays per-renderer: fixed size, no resize).
        let factory = d2d_factory()?;
        let dwrite = dwrite_factory()?;
        let wicf: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

        let wic =
            wicf.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;

        let rt = factory.CreateWicBitmapRenderTarget(&wic, &software_rt_props())?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

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
        let hbmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if hbmp.is_invalid() {
            let _ = DeleteDC(mem_dc);
            return Err(Error::from_win32());
        }
        let old_bmp = SelectObject(mem_dc, hbmp);

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
            10.0,
            w!("en-us"),
        )?;
        fmt_emoji_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        fmt_emoji_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        fmt_emoji_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

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
            fmt_badge: mk_font(10.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_emoji: Fmt {
                fmt: fmt_emoji_fmt,
                buf: RefCell::new(Vec::new()),
            },
            fmt_title: mk_font(
                12.5,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            fmt_desc: mk_font(
                10.5,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            fmt_score: mk_font(
                11.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            last_event: None,
            buf: Vec::new(),
            _no_send: PhantomData,
        })
    }

    /// Force a full rebuild at the current size (D2DERR_RECREATE_TARGET
    /// recovery). The popup is fixed-size so there is no resize(); callers
    /// retry present() once after this returns Ok — the same policy as the
    /// overlay/dashboard outside-paint paths in main.rs.
    pub unsafe fn recreate(&mut self) -> Result<()> {
        // Park the previously selected bitmap; deleting a selected GDI object is a no-op leak.
        if !self.mem_dc.is_invalid() {
            let _ = SelectObject(self.mem_dc, self.old_bmp);
        }
        if !self.hbmp.is_invalid() {
            let _ = DeleteObject(self.hbmp);
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

        // Reuse mem_dc across rebuilds; recreate only if it was lost.
        if self.mem_dc.is_invalid() {
            let screen_dc = GetWindowDC(None);
            if screen_dc.is_invalid() {
                return Err(Error::from_win32());
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            let _ = ReleaseDC(None, screen_dc);
            if mem_dc.is_invalid() {
                return Err(Error::from_win32());
            }
            self.mem_dc = mem_dc;
        }

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = self.w;
        bmi.bmiHeader.biHeight = -self.h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(self.mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if hbmp.is_invalid() {
            return Err(Error::from_win32());
        }
        self.old_bmp = SelectObject(self.mem_dc, hbmp);

        // Rebuild every brush against the new render target (same palette as new()).
        self.bg_brush = rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?;
        self.border_brush = rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?;
        self.white_brush = rt.CreateSolidColorBrush(&color(0.96, 0.96, 0.98, 1.0), None)?;
        self.dim_brush = rt.CreateSolidColorBrush(&color(0.62, 0.62, 0.67, 1.0), None)?;
        self.subtle_brush = rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?;
        self.wicket_bg = rt.CreateSolidColorBrush(&color(0.600, 0.106, 0.106, 0.35), None)?;
        self.wicket_text = rt.CreateSolidColorBrush(&color(0.973, 0.294, 0.333, 1.0), None)?;
        self.redcard_bg = rt.CreateSolidColorBrush(&color(0.851, 0.016, 0.161, 0.40), None)?;
        self.redcard_text = rt.CreateSolidColorBrush(&color(1.0, 0.42, 0.454, 1.0), None)?;
        self.four_bg = rt.CreateSolidColorBrush(&color(0.012, 0.412, 0.631, 0.35), None)?;
        self.four_text = rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?;
        self.six_bg = rt.CreateSolidColorBrush(&color(0.082, 0.502, 0.239, 0.35), None)?;
        self.six_text = rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?;
        self.goal_bg = rt.CreateSolidColorBrush(&color(0.706, 0.325, 0.035, 0.35), None)?;
        self.goal_text = rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?;
        self.win_bg = rt.CreateSolidColorBrush(&color(0.450, 0.150, 0.750, 0.35), None)?;
        self.win_text = rt.CreateSolidColorBrush(&color(0.750, 0.450, 0.980, 1.0), None)?;

        self.wic = wic;
        self.rt = rt;
        self.hbmp = hbmp;
        self.bits = bits;
        // Force the persistent buffer back to the rebuilt footprint on next present.
        self.buf.clear();

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
        let (w, h) = (self.w as f32, self.h as f32);
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
            radiusX: 10.0,
            radiusY: 10.0,
        };
        self.rt.FillRoundedRectangle(&rr, &self.bg_brush);
        // HC: bg is already opaque; bump the border to 2px.
        let border_w = if high_contrast() { 2.0 } else { 1.0 };
        self.rt
            .DrawRoundedRectangle(&rr, &self.border_brush, border_w, None);

        let (badge_label, badge_bg, badge_text) = self.event_badge(event);

        // Top Row: Badge Pill on left, Current Score on right
        let badge_rect = D2D_RECT_F {
            left: 12.0,
            top: 10.0,
            right: 88.0,
            bottom: 28.0,
        };
        let badge_rr = D2D1_ROUNDED_RECT {
            rect: badge_rect,
            radiusX: 4.0,
            radiusY: 4.0,
        };
        self.rt.FillRoundedRectangle(&badge_rr, badge_bg);
        // Badge labels carry emoji (🏆/🟥/🏏/⚽/💥/⚡): must use the Emoji
        // family, never Segoe UI (tofu). ASCII fallback in Segoe UI Emoji
        // keeps the trailing label legible.
        self.fmt_emoji
            .text(&self.rt, badge_label, &badge_rect, badge_text);

        let score_rect = D2D_RECT_F {
            left: 96.0,
            top: 10.0,
            right: w - 12.0,
            bottom: 28.0,
        };
        self.fmt_score.text(
            &self.rt,
            &ui_text(&event.score, 24),
            &score_rect,
            &self.white_brush,
        );

        // Middle Row: Event Headline
        let title_rect = D2D_RECT_F {
            left: 12.0,
            top: 33.0,
            right: w - 12.0,
            bottom: 53.0,
        };
        self.fmt_title.text(
            &self.rt,
            &ui_text(&event.title, 48),
            &title_rect,
            &self.white_brush,
        );

        // Bottom Row: Description / Subtext
        let desc_rect = D2D_RECT_F {
            left: 12.0,
            top: 54.0,
            right: w - 12.0,
            bottom: 74.0,
        };
        let clean_desc = clean_event_detail(&event.description, event.event_type);
        self.fmt_desc.text(
            &self.rt,
            &ui_text(&clean_desc, 80),
            &desc_rect,
            &self.dim_brush,
        );

        self.rt.EndDraw(None, None)?;

        let row_pitch = (self.w * 4) as usize;
        let total_bytes = row_pitch * self.h as usize;
        // Persistent buffer: clear + resize instead of a per-present alloc.
        // CopyPixels fully overwrites it; a stride mismatch surfaces as Err below.
        debug_assert_eq!(row_pitch, self.w as usize * 4);
        self.buf.clear();
        self.buf.resize(total_bytes, 0);
        self.wic
            .CopyPixels(std::ptr::null(), row_pitch as u32, &mut self.buf)?;
        debug_assert_eq!(self.buf.len(), total_bytes);
        std::ptr::copy_nonoverlapping(self.buf.as_ptr(), self.bits as *mut u8, self.buf.len());

        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        UpdateLayeredWindow(
            self.hwnd,
            None,
            Some(pos),
            Some(&size),
            self.mem_dc,
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
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
    match msg {
        WM_PAINT => {
            // Layered windows must not rely on WM_PAINT for ULW, but RDP/UAC
            // can blank the surface — re-present the last event if we have one.
            let renderer_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupRenderer;
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
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
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_AUTOHIDE_ID {
                let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            // Click to dismiss
            let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // Esc dismisses when the popup has focus. Note: the popup is
            // intentionally WS_EX_NOACTIVATE (non-focusable by design, no tab
            // stops) so this is a backstop — click and the auto-hide timer are
            // the primary dismiss paths. Screen-reader users get the same
            // event via the overlay flash + tray tooltip.
            if wparam.0 == VK_ESCAPE.0 as usize {
                let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, TIMER_AUTOHIDE_ID);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl MiniPopupWindow {
    pub unsafe fn create() -> Result<Self> {
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
            POPUP_W as i32,
            POPUP_H as i32,
            None,
            HMENU::default(),
            hinstance,
            None,
        )?;

        let mut renderer = Box::new(PopupRenderer::new(hwnd, POPUP_W, POPUP_H)?);
        // Expose the renderer to popup_wnd_proc for WM_PAINT re-present.
        // The Box is heap-stable; the pointer stays valid for the window lifetime.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, renderer.as_mut() as *mut _ as isize);

        Ok(Self {
            hwnd,
            renderer,
            current_event: None,
        })
    }

    pub unsafe fn show_event(&mut self, event: MatchEvent, pos: POINT) {
        if let Err(e) = self.renderer.present(&pos, &event) {
            // P0-7: EndDraw RECREATE_TARGET (device lost) must recreate +
            // retry once, never be swallowed. Other errors stay best-effort.
            if e.code() == D2DERR_RECREATE_TARGET && self.renderer.recreate().is_ok() {
                let _ = self.renderer.present(&pos, &event);
            }
        }
        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            pos.x,
            pos.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let timeout_ms: u32 = if event.event_type == MatchEventType::Win {
            WIN_TTL.as_millis() as u32
        } else {
            EVENT_TTL.as_millis() as u32
        };
        let _ = SetTimer(self.hwnd, TIMER_AUTOHIDE_ID, timeout_ms, None);
        self.current_event = Some(event);
    }

    pub unsafe fn hide(&mut self) {
        let _ = KillTimer(self.hwnd, TIMER_AUTOHIDE_ID);
        let _ = ShowWindow(self.hwnd, SW_HIDE);
    }
}
