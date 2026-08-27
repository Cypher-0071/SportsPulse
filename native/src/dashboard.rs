//! SportsPulse — Win11 Dark Theme Match Selection Dashboard.
//! SOLID opaque #202020 background, #2D2D2D cards — no transparency.
//! Draggable title bar, close button, spacious card layout.

#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetWindowDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HDC, AC_SRC_ALPHA, AC_SRC_OVER,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap,
    IWICImagingFactory, WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use crate::engine::events::DiscoveredMatch;

pub const DASH_W: u32 = 720;
pub const DASH_H: u32 = 680;

pub const WM_APP_SELECT_MATCH: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub const WM_APP_UNTRACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;

pub const TITLE_BAR_HEIGHT: f32 = 52.0;
pub const ITEM_TOP_START: f32 = 100.0;
pub const ITEM_HEIGHT: f32 = 84.0;
pub const ITEM_SPACING: f32 = 10.0;

#[inline]
fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    CloseButton,
    UntrackButton,
    TitleBar,
    MatchItem(usize),
}

pub struct DashboardRenderer {
    hwnd: HWND,
    wic: IWICBitmap,
    rt: ID2D1RenderTarget,
    mem_dc: HDC,
    hbmp: HBITMAP,
    bits: *mut core::ffi::c_void,
    w: i32,
    h: i32,
    // All brushes — FULLY OPAQUE (alpha = 1.0) matching Win11 dark theme
    bg_brush: ID2D1SolidColorBrush,
    header_bg_brush: ID2D1SolidColorBrush,
    card_bg: ID2D1SolidColorBrush,
    card_hover: ID2D1SolidColorBrush,
    card_active: ID2D1SolidColorBrush,
    border_brush: ID2D1SolidColorBrush,
    active_border_brush: ID2D1SolidColorBrush,
    divider_brush: ID2D1SolidColorBrush,
    white_brush: ID2D1SolidColorBrush,
    dim_brush: ID2D1SolidColorBrush,
    subtle_brush: ID2D1SolidColorBrush,
    icon_box_bg: ID2D1SolidColorBrush,
    live_badge_bg: ID2D1SolidColorBrush,
    live_badge_text: ID2D1SolidColorBrush,
    amber_badge_bg: ID2D1SolidColorBrush,
    amber_badge_text: ID2D1SolidColorBrush,
    blue_badge_bg: ID2D1SolidColorBrush,
    blue_badge_text: ID2D1SolidColorBrush,
    final_badge_bg: ID2D1SolidColorBrush,
    final_badge_text: ID2D1SolidColorBrush,
    btn_untrack_bg: ID2D1SolidColorBrush,
    btn_untrack_border: ID2D1SolidColorBrush,
    btn_untrack_text: ID2D1SolidColorBrush,
    close_btn_hover_bg: ID2D1SolidColorBrush,
    fmt_app_title: Fmt,
    fmt_subtitle: Fmt,
    fmt_item_title: Fmt,
    fmt_item_sub: Fmt,
    fmt_badge: Fmt,
    fmt_btn: Fmt,
    fmt_icon: Fmt,
    fmt_close: Fmt,
    pub hover_index: Option<usize>,
    pub close_hover: bool,
    pub untrack_hover: bool,
}

impl DashboardRenderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32) -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let wicf: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

        let wic = wicf.CreateBitmap(
            w,
            h,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnDemand,
        )?;

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
        let _ = ReleaseDC(None, screen_dc);

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

        // ===========================================================
        // WINDOWS 11 DARK THEME — ALL FULLY OPAQUE (alpha = 1.0)
        // Exact same solid dark gray as Win11 Settings/File Explorer
        // ===========================================================
        let bg_brush           = rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?;  // #202020
        let header_bg_brush    = rt.CreateSolidColorBrush(&color(0.110, 0.110, 0.110, 1.0), None)?;  // #1C1C1C
        let card_bg            = rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?;  // #2D2D2D
        let card_hover         = rt.CreateSolidColorBrush(&color(0.220, 0.220, 0.220, 1.0), None)?;  // #383838
        let card_active        = rt.CreateSolidColorBrush(&color(0.090, 0.200, 0.130, 1.0), None)?;  // #173321
        let border_brush       = rt.CreateSolidColorBrush(&color(0.235, 0.235, 0.235, 1.0), None)?;  // #3C3C3C
        let active_border_brush= rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?;  // #22C55E
        let divider_brush      = rt.CreateSolidColorBrush(&color(0.200, 0.200, 0.200, 1.0), None)?;  // #333333
        let white_brush        = rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 1.0), None)?;        // #FFFFFF
        let dim_brush          = rt.CreateSolidColorBrush(&color(0.60, 0.60, 0.60, 1.0), None)?;     // #999999
        let subtle_brush       = rt.CreateSolidColorBrush(&color(0.40, 0.40, 0.40, 1.0), None)?;     // #666666
        let icon_box_bg        = rt.CreateSolidColorBrush(&color(0.153, 0.153, 0.153, 1.0), None)?;  // #272727

        let live_badge_bg      = rt.CreateSolidColorBrush(&color(0.055, 0.220, 0.110, 1.0), None)?;  // #0E3820
        let live_badge_text    = rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?;  // #22C55E

        let amber_badge_bg     = rt.CreateSolidColorBrush(&color(0.310, 0.180, 0.020, 1.0), None)?;  // #4F2E05
        let amber_badge_text   = rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?;  // #F59E0B

        let blue_badge_bg      = rt.CreateSolidColorBrush(&color(0.020, 0.180, 0.290, 1.0), None)?;  // #052E4A
        let blue_badge_text    = rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?;  // #38BDF8

        let final_badge_bg     = rt.CreateSolidColorBrush(&color(0.160, 0.160, 0.160, 1.0), None)?;  // #292929
        let final_badge_text   = rt.CreateSolidColorBrush(&color(0.50, 0.50, 0.50, 1.0), None)?;     // #808080

        let btn_untrack_bg     = rt.CreateSolidColorBrush(&color(0.247, 0.114, 0.114, 1.0), None)?;  // #3F1D1D
        let btn_untrack_border = rt.CreateSolidColorBrush(&color(0.400, 0.130, 0.130, 1.0), None)?;  // #662121
        let btn_untrack_text   = rt.CreateSolidColorBrush(&color(0.973, 0.443, 0.443, 1.0), None)?;  // #F87171

        let close_btn_hover_bg = rt.CreateSolidColorBrush(&color(0.910, 0.067, 0.137, 1.0), None)?;  // #E81123

        let mk_font = |size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT| -> Result<Fmt> {
            let fmt = dwrite.CreateTextFormat(
                w!("Segoe UI Variable Display"),
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
            header_bg_brush,
            card_bg,
            card_hover,
            card_active,
            border_brush,
            active_border_brush,
            divider_brush,
            white_brush,
            dim_brush,
            subtle_brush,
            icon_box_bg,
            live_badge_bg,
            live_badge_text,
            amber_badge_bg,
            amber_badge_text,
            blue_badge_bg,
            blue_badge_text,
            final_badge_bg,
            final_badge_text,
            btn_untrack_bg,
            btn_untrack_border,
            btn_untrack_text,
            close_btn_hover_bg,
            // Larger fonts for spacious modern UI
            fmt_app_title: mk_font(16.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_subtitle: mk_font(13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_item_title: mk_font(16.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_item_sub: mk_font(13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_badge: mk_font(12.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_btn: mk_font(13.0, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_icon: mk_font(22.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_close: mk_font(14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            hover_index: None,
            close_hover: false,
            untrack_hover: false,
        })
    }

    pub fn hit_test(&self, x: f32, y: f32, match_count: usize) -> Option<HitTarget> {
        let w = self.w as f32;

        // Close button (top right)
        if x >= (w - 52.0) && x <= (w - 12.0) && y >= 10.0 && y <= 42.0 {
            return Some(HitTarget::CloseButton);
        }

        // Untrack button
        if x >= (w - 190.0) && x <= (w - 62.0) && y >= 12.0 && y <= 40.0 {
            return Some(HitTarget::UntrackButton);
        }

        // Title bar drag area (top 52px excluding buttons)
        if y <= TITLE_BAR_HEIGHT {
            return Some(HitTarget::TitleBar);
        }

        // Match Cards
        if y >= ITEM_TOP_START {
            let relative_y = y - ITEM_TOP_START;
            let index = (relative_y / (ITEM_HEIGHT + ITEM_SPACING)) as usize;
            let card_y_within = relative_y % (ITEM_HEIGHT + ITEM_SPACING);

            if index < match_count && card_y_within <= ITEM_HEIGHT && x >= 20.0 && x <= (w - 20.0) {
                return Some(HitTarget::MatchItem(index));
            }
        }

        None
    }

    pub unsafe fn present(
        &mut self,
        pos: &POINT,
        matches: &[DiscoveredMatch],
        selected_id: &Option<String>,
    ) -> Result<()> {
        let (w, h) = (self.w as f32, self.h as f32);
        self.rt.BeginDraw();
        self.rt.Clear(None);

        // SOLID Win11 background (8px rounded corners)
        let full_rect = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
        let rr = D2D1_ROUNDED_RECT { rect: full_rect, radiusX: 8.0, radiusY: 8.0 };
        self.rt.FillRoundedRectangle(&rr, &self.bg_brush);
        self.rt.DrawRoundedRectangle(&rr, &self.border_brush, 1.0, None);

        // Title Bar Header
        let header_rect = D2D_RECT_F { left: 1.0, top: 1.0, right: w - 1.0, bottom: TITLE_BAR_HEIGHT };
        let header_rr = D2D1_ROUNDED_RECT { rect: header_rect, radiusX: 7.0, radiusY: 7.0 };
        self.rt.FillRoundedRectangle(&header_rr, &self.header_bg_brush);

        // Divider below title bar
        self.rt.DrawLine(
            windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: 0.0, y: TITLE_BAR_HEIGHT },
            windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: w, y: TITLE_BAR_HEIGHT },
            &self.divider_brush,
            1.0,
            None,
        );

        // App Title
        let title_rect = D2D_RECT_F { left: 22.0, top: 0.0, right: w - 200.0, bottom: TITLE_BAR_HEIGHT };
        self.fmt_app_title.text(&self.rt, "SportsPulse — Match Discovery", &title_rect, &self.white_brush);

        // Untrack Button
        let untrack_btn_rect = D2D_RECT_F { left: w - 190.0, top: 12.0, right: w - 62.0, bottom: 40.0 };
        let untrack_rr = D2D1_ROUNDED_RECT { rect: untrack_btn_rect, radiusX: 5.0, radiusY: 5.0 };
        self.rt.FillRoundedRectangle(&untrack_rr, &self.btn_untrack_bg);
        self.rt.DrawRoundedRectangle(&untrack_rr, &self.btn_untrack_border, 1.0, None);
        self.fmt_btn.text(&self.rt, "Untrack Active", &untrack_btn_rect, &self.btn_untrack_text);

        // Close Button (✕)
        let close_rect = D2D_RECT_F { left: w - 52.0, top: 10.0, right: w - 12.0, bottom: 42.0 };
        let close_rr = D2D1_ROUNDED_RECT { rect: close_rect, radiusX: 5.0, radiusY: 5.0 };
        if self.close_hover {
            self.rt.FillRoundedRectangle(&close_rr, &self.close_btn_hover_bg);
            self.fmt_close.text(&self.rt, "✕", &close_rect, &self.white_brush);
        } else {
            self.fmt_close.text(&self.rt, "✕", &close_rect, &self.dim_brush);
        }

        // Subtitle
        let subtitle_rect = D2D_RECT_F { left: 24.0, top: 60.0, right: w - 24.0, bottom: 86.0 };
        self.fmt_subtitle.text(
            &self.rt,
            "Select a live fixture below to stream real-time scores to your desktop overlay",
            &subtitle_rect,
            &self.dim_brush,
        );

        // Match Cards
        if matches.is_empty() {
            let empty_rect = D2D_RECT_F { left: 24.0, top: 220.0, right: w - 24.0, bottom: 280.0 };
            self.fmt_subtitle.text(
                &self.rt,
                "No live matches detected. Polling feeds in background...",
                &empty_rect,
                &self.subtle_brush,
            );
        } else {
            for (i, m) in matches.iter().enumerate().take(6) {
                let top = ITEM_TOP_START + i as f32 * (ITEM_HEIGHT + ITEM_SPACING);
                let item_rect = D2D_RECT_F { left: 20.0, top, right: w - 20.0, bottom: top + ITEM_HEIGHT };
                let item_rr = D2D1_ROUNDED_RECT { rect: item_rect, radiusX: 8.0, radiusY: 8.0 };

                let is_selected = selected_id.as_ref().map_or(false, |id| id == &m.match_id);
                let is_hovered = self.hover_index == Some(i);

                if is_selected {
                    self.rt.FillRoundedRectangle(&item_rr, &self.card_active);
                    self.rt.DrawRoundedRectangle(&item_rr, &self.active_border_brush, 2.0, None);
                } else if is_hovered {
                    self.rt.FillRoundedRectangle(&item_rr, &self.card_hover);
                    self.rt.DrawRoundedRectangle(&item_rr, &self.border_brush, 1.0, None);
                } else {
                    self.rt.FillRoundedRectangle(&item_rr, &self.card_bg);
                    self.rt.DrawRoundedRectangle(&item_rr, &self.border_brush, 1.0, None);
                }

                // Sport Icon Box (44x44 px)
                let icon_rect = D2D_RECT_F { left: 36.0, top: top + 20.0, right: 80.0, bottom: top + 64.0 };
                let icon_rr = D2D1_ROUNDED_RECT { rect: icon_rect, radiusX: 8.0, radiusY: 8.0 };
                self.rt.FillRoundedRectangle(&icon_rr, &self.icon_box_bg);
                self.rt.DrawRoundedRectangle(&icon_rr, &self.border_brush, 1.0, None);

                let icon_char = if m.sport == "soccer" { "⚽" } else { "🏏" };
                self.fmt_icon.text(&self.rt, icon_char, &icon_rect, &self.white_brush);

                // Match Title (16pt bold)
                let m_title_rect = D2D_RECT_F { left: 96.0, top: top + 16.0, right: w - 120.0, bottom: top + 44.0 };
                self.fmt_item_title.text(&self.rt, &m.title, &m_title_rect, &self.white_brush);

                // League / Start Time (13pt dim)
                let sub_text = if !m.league_name.is_empty() && !m.start_time.is_empty() {
                    format!("{} · {}", m.league_name, m.start_time)
                } else if !m.league_name.is_empty() {
                    m.league_name.clone()
                } else {
                    m.start_time.clone()
                };
                let m_sub_rect = D2D_RECT_F { left: 96.0, top: top + 46.0, right: w - 120.0, bottom: top + 70.0 };
                self.fmt_item_sub.text(&self.rt, &sub_text, &m_sub_rect, &self.dim_brush);

                // Status Badge (80x28 px)
                let status_upper = m.status.to_uppercase();
                let (badge_label, badge_bg, badge_fg) = if status_upper.contains("LIVE") || status_upper.contains("IN PROGRESS") {
                    ("● LIVE", &self.live_badge_bg, &self.live_badge_text)
                } else if status_upper.contains("BREAK") || status_upper.contains("TEA") || status_upper.contains("LUNCH") {
                    ("BREAK", &self.amber_badge_bg, &self.amber_badge_text)
                } else if status_upper.contains("SCHED") || status_upper.contains("UPCOMING") {
                    ("UPCOMING", &self.blue_badge_bg, &self.blue_badge_text)
                } else {
                    ("FINAL", &self.final_badge_bg, &self.final_badge_text)
                };

                let badge_rect = D2D_RECT_F { left: w - 108.0, top: top + 28.0, right: w - 30.0, bottom: top + 56.0 };
                let badge_rr = D2D1_ROUNDED_RECT { rect: badge_rect, radiusX: 5.0, radiusY: 5.0 };
                self.rt.FillRoundedRectangle(&badge_rr, badge_bg);
                self.fmt_badge.text(&self.rt, badge_label, &badge_rect, badge_fg);
            }
        }

        self.rt.EndDraw(None, None)?;

        let row_pitch = (self.w * 4) as usize;
        let total_bytes = row_pitch * self.h as usize;
        let mut buf = vec![0u8; total_bytes];
        self.wic.CopyPixels(std::ptr::null(), row_pitch as u32, &mut buf)?;
        std::ptr::copy_nonoverlapping(buf.as_ptr(), self.bits as *mut u8, total_bytes);

        let size = SIZE { cx: self.w, cy: self.h };
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

    pub fn set_hover(&mut self, index: Option<usize>, close: bool, untrack: bool) {
        self.hover_index = index;
        self.close_hover = close;
        self.untrack_hover = untrack;
    }
}

impl Drop for DashboardRenderer {
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
