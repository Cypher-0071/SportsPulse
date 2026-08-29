//! SportsPulse — Win11 Dark Theme Event Mini-Popup (Wickets, Boundaries, Goals).
//! Auto-dismisses after 8 seconds for Win events, 5 seconds otherwise, or on click.
//! Pixel flow: D2D -> WIC bitmap -> CopyPixels -> DIB -> UpdateLayeredWindow.

#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetWindowDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, KillTimer, LoadCursorW, RegisterClassExW, SetTimer,
    SetWindowPos, ShowWindow, UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, HCURSOR, HMENU,
    HWND_TOPMOST, IDC_ARROW, SWP_NOACTIVATE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WM_CLOSE, WM_DESTROY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::engine::models::{MatchEvent, MatchEventType};

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
}

impl Fmt {
    unsafe fn text(
        &self,
        rt: &ID2D1RenderTarget,
        s: &str,
        rect: &D2D_RECT_F,
        brush: &ID2D1SolidColorBrush,
    ) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        rt.DrawText(
            &wide,
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
    wic: IWICBitmap,
    rt: ID2D1RenderTarget,
    mem_dc: HDC,
    hbmp: HBITMAP,
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
    fmt_title: Fmt,
    fmt_desc: Fmt,
    fmt_score: Fmt,
}

impl PopupRenderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32) -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let wicf: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

        let wic =
            wicf.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = factory.CreateWicBitmapRenderTarget(&wic, &props)?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

        let screen_dc = GetWindowDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w as i32;
        bmi.bmiHeader.biHeight = -(h as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        SelectObject(mem_dc, hbmp);

        // Windows 11 Dark Theme: #1E1E22 at 94% opacity
        let bg_brush = rt.CreateSolidColorBrush(&color(0.1176, 0.1176, 0.1333, 0.94), None)?;
        let border_brush = rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 0.09), None)?;
        let white_brush = rt.CreateSolidColorBrush(&color(0.96, 0.96, 0.98, 1.0), None)?;
        let dim_brush = rt.CreateSolidColorBrush(&color(0.62, 0.62, 0.67, 1.0), None)?;
        let subtle_brush = rt.CreateSolidColorBrush(&color(0.42, 0.42, 0.47, 1.0), None)?;

        // Event Badge Colors:
        // Wicket: Soft Red
        let wicket_bg = rt.CreateSolidColorBrush(&color(0.600, 0.106, 0.106, 0.35), None)?;
        let wicket_text = rt.CreateSolidColorBrush(&color(0.973, 0.294, 0.333, 1.0), None)?;

        // Soccer red card: original mini_popup #d90429
        let redcard_bg = rt.CreateSolidColorBrush(&color(0.851, 0.016, 0.161, 0.40), None)?;
        let redcard_text = rt.CreateSolidColorBrush(&color(0.851, 0.016, 0.161, 1.0), None)?;

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
                Ok(Fmt { fmt })
            };

        Ok(Self {
            hwnd,
            wic,
            rt,
            mem_dc,
            hbmp,
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
        })
    }

    fn event_badge<'a>(
        &'a self,
        event: &MatchEvent,
    ) -> (
        &'static str,
        &'a ID2D1SolidColorBrush,
        &'a ID2D1SolidColorBrush,
    ) {
        let upper_title = event.title.to_uppercase();
        if upper_title.contains("RED CARD") {
            ("🟥 CARD", &self.redcard_bg, &self.redcard_text)
        } else if upper_title.contains("GOAL") {
            ("⚽ GOAL", &self.goal_bg, &self.goal_text)
        } else if upper_title.contains("FOUR") {
            ("⚡ FOUR", &self.four_bg, &self.four_text)
        } else if upper_title.contains("SIX") {
            ("💥 SIX", &self.six_bg, &self.six_text)
        } else if event.event_type == MatchEventType::Wicket
            || upper_title.contains("WICKET")
            || upper_title.contains("OUT")
        {
            ("🏏 WICKET", &self.wicket_bg, &self.wicket_text)
        } else if event.event_type == MatchEventType::Win
            || upper_title.contains("WON")
            || upper_title.contains("WIN")
        {
            ("🏆 RESULT", &self.win_bg, &self.win_text)
        } else {
            ("EVENT", &self.six_bg, &self.six_text)
        }
    }

    pub unsafe fn present(&self, pos: &POINT, event: &MatchEvent) -> Result<()> {
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
        self.rt
            .DrawRoundedRectangle(&rr, &self.border_brush, 1.0, None);

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
        self.fmt_badge
            .text(&self.rt, badge_label, &badge_rect, badge_text);

        let score_rect = D2D_RECT_F {
            left: 96.0,
            top: 10.0,
            right: w - 12.0,
            bottom: 28.0,
        };
        self.fmt_score
            .text(&self.rt, &event.score, &score_rect, &self.white_brush);

        // Middle Row: Event Headline
        let title_rect = D2D_RECT_F {
            left: 12.0,
            top: 33.0,
            right: w - 12.0,
            bottom: 53.0,
        };
        self.fmt_title
            .text(&self.rt, &event.title, &title_rect, &self.white_brush);

        // Bottom Row: Description / Subtext
        let desc_rect = D2D_RECT_F {
            left: 12.0,
            top: 54.0,
            right: w - 12.0,
            bottom: 74.0,
        };
        let clean_desc = clean_event_detail(&event.description, event.event_type);
        self.fmt_desc
            .text(&self.rt, &clean_desc, &desc_rect, &self.dim_brush);

        self.rt.EndDraw(None, None)?;

        let row_pitch = (self.w * 4) as usize;
        let total_bytes = row_pitch * self.h as usize;
        let mut buf = vec![0u8; total_bytes];
        self.wic
            .CopyPixels(std::ptr::null(), row_pitch as u32, &mut buf)?;
        std::ptr::copy_nonoverlapping(buf.as_ptr(), self.bits as *mut u8, total_bytes);

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
        let hinstance = GetModuleHandleW(None).unwrap();
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

        let renderer = Box::new(PopupRenderer::new(hwnd, POPUP_W, POPUP_H)?);

        Ok(Self {
            hwnd,
            renderer,
            current_event: None,
        })
    }

    pub unsafe fn show_event(&mut self, event: MatchEvent, pos: POINT) {
        let _ = self.renderer.present(&pos, &event);
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
            8000
        } else {
            5000
        };
        let _ = SetTimer(self.hwnd, TIMER_AUTOHIDE_ID, timeout_ms, None);
        self.current_event = Some(event);
    }

    pub unsafe fn hide(&mut self) {
        let _ = KillTimer(self.hwnd, TIMER_AUTOHIDE_ID);
        let _ = ShowWindow(self.hwnd, SW_HIDE);
    }
}
