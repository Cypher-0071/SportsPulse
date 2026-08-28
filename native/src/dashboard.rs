//! SportsPulse — Win11 Dark Theme Match Selection Dashboard.
//! Win11-inspired match discovery surface with custom rendering and standard window behavior.

#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_STRETCH_NORMAL,
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
use chrono::{DateTime, FixedOffset};

// Spacious, large default dimensions
pub const DASH_NORMAL_W: u32 = 1120;
pub const DASH_NORMAL_H: u32 = 760;
pub const DASH_W: u32 = DASH_NORMAL_W;
pub const DASH_H: u32 = DASH_NORMAL_H;

pub const WM_APP_SELECT_MATCH: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub const WM_APP_UNTRACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;

pub const TITLE_BAR_HEIGHT: f32 = 48.0;
pub const ITEM_TOP_START: f32 = 76.0;
pub const ITEM_HEIGHT: f32 = 92.0;
pub const ITEM_SPACING: f32 = 12.0;
const CARD_LEFT: f32 = 32.0;
const CARD_RIGHT: f32 = 32.0;
const ACTION_W: f32 = 108.0;
const CONTENT_BOTTOM_GUTTER: f32 = 24.0;
const SCROLL_STEP: f32 = 72.0;

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
    MinimizeButton,
    MaximizeButton,
    CloseButton,
    TitleBar,
    MatchItem(usize),
    MatchAction(usize),
}

#[derive(Debug, Default)]
struct LeagueGroup {
    name: String,
    match_indices: Vec<usize>,
}

fn is_live_match(m: &DiscoveredMatch) -> bool {
    let status = m.status.trim().to_ascii_lowercase();
    status == "in" || status.contains("live") || status.contains("in progress")
}

fn push_to_league_group(groups: &mut Vec<LeagueGroup>, league_name: &str, index: usize) {
    let name = if league_name.trim().is_empty() { "Other fixtures" } else { league_name };
    if let Some(group) = groups.iter_mut().find(|group| group.name == name) {
        group.match_indices.push(index);
    } else {
        groups.push(LeagueGroup { name: name.to_owned(), match_indices: vec![index] });
    }
}

fn format_upcoming_time(value: &str) -> String {
    let offset = FixedOffset::east_opt(5 * 60 * 60 + 30 * 60).unwrap();
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&offset).format("%-d %b, %I:%M %p").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

struct Brushes {
    bg: ID2D1SolidColorBrush,
    header_bg: ID2D1SolidColorBrush,
    card_bg: ID2D1SolidColorBrush,
    card_hover: ID2D1SolidColorBrush,
    card_active: ID2D1SolidColorBrush,
    border: ID2D1SolidColorBrush,
    active_border: ID2D1SolidColorBrush,
    divider: ID2D1SolidColorBrush,
    white: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    subtle: ID2D1SolidColorBrush,
    icon_box_bg: ID2D1SolidColorBrush,
    action_bg: ID2D1SolidColorBrush,
    action_hover_bg: ID2D1SolidColorBrush,
    action_border: ID2D1SolidColorBrush,
    action_text: ID2D1SolidColorBrush,
    caption_btn_hover_bg: ID2D1SolidColorBrush,
    live_badge_bg: ID2D1SolidColorBrush,
    live_badge_text: ID2D1SolidColorBrush,
    amber_badge_bg: ID2D1SolidColorBrush,
    amber_badge_text: ID2D1SolidColorBrush,
    blue_badge_bg: ID2D1SolidColorBrush,
    blue_badge_text: ID2D1SolidColorBrush,
    final_badge_bg: ID2D1SolidColorBrush,
    final_badge_text: ID2D1SolidColorBrush,
    close_btn_hover_bg: ID2D1SolidColorBrush,
}

impl Brushes {
    unsafe fn create(rt: &ID2D1RenderTarget) -> Result<Self> {
        Ok(Self {
            bg:           rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?,  // #202020
            header_bg:    rt.CreateSolidColorBrush(&color(0.110, 0.110, 0.110, 1.0), None)?,  // #1C1C1C
            card_bg:      rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?,  // #2D2D2D
            card_hover:   rt.CreateSolidColorBrush(&color(0.230, 0.230, 0.230, 1.0), None)?,  // #3B3B3B
            card_active:  rt.CreateSolidColorBrush(&color(0.090, 0.220, 0.130, 1.0), None)?,  // #173821
            border:       rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?,  // #3E3E3E
            active_border:rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?,  // #22C55E
            divider:      rt.CreateSolidColorBrush(&color(0.210, 0.210, 0.210, 1.0), None)?,  // #353535
            white:        rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 1.0), None)?,        // #FFFFFF
            dim:          rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?,     // #A6A6A6
            subtle:       rt.CreateSolidColorBrush(&color(0.44, 0.44, 0.44, 1.0), None)?,     // #707070
            icon_box_bg:  rt.CreateSolidColorBrush(&color(0.145, 0.145, 0.145, 1.0), None)?,  // #252525
            action_bg:    rt.CreateSolidColorBrush(&color(0.130, 0.130, 0.130, 1.0), None)?,
            action_hover_bg: rt.CreateSolidColorBrush(&color(0.220, 0.220, 0.220, 1.0), None)?,
            action_border: rt.CreateSolidColorBrush(&color(0.270, 0.270, 0.270, 1.0), None)?,
            action_text:  rt.CreateSolidColorBrush(&color(0.82, 0.82, 0.82, 1.0), None)?,
            caption_btn_hover_bg: rt.CreateSolidColorBrush(&color(0.235, 0.235, 0.235, 1.0), None)?, // #3C3C3C

            live_badge_bg:   rt.CreateSolidColorBrush(&color(0.055, 0.240, 0.110, 1.0), None)?,
            live_badge_text: rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?,

            amber_badge_bg:   rt.CreateSolidColorBrush(&color(0.320, 0.180, 0.020, 1.0), None)?,
            amber_badge_text: rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?,

            blue_badge_bg:   rt.CreateSolidColorBrush(&color(0.020, 0.190, 0.300, 1.0), None)?,
            blue_badge_text: rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?,

            final_badge_bg:   rt.CreateSolidColorBrush(&color(0.165, 0.165, 0.165, 1.0), None)?,
            final_badge_text: rt.CreateSolidColorBrush(&color(0.58, 0.58, 0.58, 1.0), None)?,

            close_btn_hover_bg: rt.CreateSolidColorBrush(&color(0.910, 0.067, 0.137, 1.0), None)?,  // #E81123
        })
    }
}

pub struct DashboardRenderer {
    hwnd: HWND,
    wic: IWICBitmap,
    rt: ID2D1RenderTarget,
    mem_dc: HDC,
    hbmp: HBITMAP,
    bits: *mut core::ffi::c_void,
    pub w: i32,
    pub h: i32,
    brushes: Brushes,
    fmt_app_title: Fmt,
    fmt_eyebrow: Fmt,
    fmt_subtitle: Fmt,
    fmt_item_title: Fmt,
    fmt_item_sub: Fmt,
    fmt_badge: Fmt,
    fmt_icon: Fmt,
    fmt_caption_min: Fmt,
    fmt_caption_max: Fmt,
    fmt_caption_close: Fmt,
    pub hover_index: Option<usize>,
    pub action_hover_index: Option<usize>,
    pub min_hover: bool,
    pub max_hover: bool,
    pub close_hover: bool,
    pub is_maximized: bool,
    pub scroll_offset: usize,
    content_height: f32,
    card_layout: Vec<(usize, D2D_RECT_F)>,
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

        let brushes = Brushes::create(&rt)?;

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
            brushes,
            // Prominent, large, readable typography
            fmt_app_title: mk_font(21.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_eyebrow: mk_font(13.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_subtitle: mk_font(17.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_item_title: mk_font(22.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_item_sub: mk_font(17.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_badge: mk_font(14.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_icon: mk_font(30.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_caption_min: mk_font(20.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_caption_max: mk_font(17.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_caption_close: mk_font(18.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            hover_index: None,
            action_hover_index: None,
            min_hover: false,
            max_hover: false,
            close_hover: false,
            is_maximized: false,
            scroll_offset: 0,
            content_height: 0.0,
            card_layout: Vec::new(),
        })
    }

    pub unsafe fn resize(&mut self, new_w: u32, new_h: u32) -> Result<()> {
        if self.w == new_w as i32 && self.h == new_h as i32 {
            return Ok(());
        }

        if !self.hbmp.is_invalid() {
            let _ = DeleteObject(self.hbmp);
        }
        if !self.mem_dc.is_invalid() {
            let _ = DeleteDC(self.mem_dc);
        }

        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let wicf: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

        let wic = wicf.CreateBitmap(
            new_w,
            new_h,
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
        bmi.bmiHeader.biWidth = new_w as i32;
        bmi.bmiHeader.biHeight = -(new_h as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        SelectObject(mem_dc, hbmp);

        self.brushes = Brushes::create(&rt)?;
        self.wic = wic;
        self.rt = rt;
        self.mem_dc = mem_dc;
        self.hbmp = hbmp;
        self.bits = bits;
        self.w = new_w as i32;
        self.h = new_h as i32;

        Ok(())
    }

    pub fn hit_test(&self, x: f32, y: f32, _match_count: usize) -> Option<HitTarget> {
        let w = self.w as f32;

        // Caption Buttons in Title Bar (Right-aligned, 54px wide each)
        if y >= 0.0 && y <= TITLE_BAR_HEIGHT {
            // Standard Win11-sized caption targets, right aligned.
            if x >= (w - 46.0) && x <= w {
                return Some(HitTarget::CloseButton);
            }
            if x >= (w - 92.0) && x < (w - 46.0) {
                return Some(HitTarget::MaximizeButton);
            }
            if x >= (w - 138.0) && x < (w - 92.0) {
                return Some(HitTarget::MinimizeButton);
            }
            // Anywhere else in top bar is draggable
            return Some(HitTarget::TitleBar);
        }

        for (index, rect) in &self.card_layout {
            if x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom {
                if x >= rect.right - ACTION_W - 16.0 {
                    return Some(HitTarget::MatchAction(*index));
                }
                return Some(HitTarget::MatchItem(*index));
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
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
        self.card_layout.clear();
        self.rt.BeginDraw();
        self.rt.Clear(None);

        // Window surface: a quiet neutral foundation with a single restrained accent role.
        let full_rect = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
        let rr = D2D1_ROUNDED_RECT { rect: full_rect, radiusX: 16.0, radiusY: 16.0 };
        self.rt.FillRoundedRectangle(&rr, &self.brushes.bg);
        self.rt.DrawRoundedRectangle(&rr, &self.brushes.border, 1.2, None);

        // Title bar header.
        let header_rect = D2D_RECT_F { left: 1.0, top: 1.0, right: w - 1.0, bottom: TITLE_BAR_HEIGHT };
        let header_rr = D2D1_ROUNDED_RECT { rect: header_rect, radiusX: 15.0, radiusY: 15.0 };
        self.rt.FillRoundedRectangle(&header_rr, &self.brushes.header_bg);

        // Divider below title bar
        self.rt.DrawLine(
            windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: 0.0, y: TITLE_BAR_HEIGHT },
            windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: w, y: TITLE_BAR_HEIGHT },
            &self.brushes.divider,
            1.0,
            None,
        );

        let title_rect = D2D_RECT_F { left: 28.0, top: 0.0, right: w - 156.0, bottom: TITLE_BAR_HEIGHT };
        self.fmt_app_title.text(&self.rt, "SportsPulse", &title_rect, &self.brushes.white);

        // 3. Caption Buttons: Minimize (−), Maximize/Restore (□ / ❐), Close (✕)
        let btn_h = TITLE_BAR_HEIGHT;

        // Minimize button, drawn as geometry for consistent optical size at every DPI.
        let min_rect = D2D_RECT_F { left: w - 136.0, top: 4.0, right: w - 94.0, bottom: btn_h - 4.0 };
        let min_rr = D2D1_ROUNDED_RECT { rect: min_rect, radiusX: 7.0, radiusY: 7.0 };
        if self.min_hover {
            self.rt.FillRoundedRectangle(&min_rr, &self.brushes.caption_btn_hover_bg);
        }
        let min_brush = if self.min_hover { &self.brushes.white } else { &self.brushes.dim };
        self.rt.DrawLine(
            D2D_POINT_2F { x: w - 122.0, y: TITLE_BAR_HEIGHT / 2.0 },
            D2D_POINT_2F { x: w - 108.0, y: TITLE_BAR_HEIGHT / 2.0 },
            min_brush,
            2.0,
            None,
        );

        // Maximize / restore button, a crisp square rather than a small font glyph.
        let max_rect = D2D_RECT_F { left: w - 90.0, top: 4.0, right: w - 48.0, bottom: btn_h - 4.0 };
        let max_rr = D2D1_ROUNDED_RECT { rect: max_rect, radiusX: 7.0, radiusY: 7.0 };
        if self.max_hover {
            self.rt.FillRoundedRectangle(&max_rr, &self.brushes.caption_btn_hover_bg);
        }
        let max_brush = if self.max_hover { &self.brushes.white } else { &self.brushes.dim };
        let max_cx = w - 69.0;
        let max_cy = TITLE_BAR_HEIGHT / 2.0;
        self.rt.DrawRectangle(
            &D2D_RECT_F { left: max_cx - 6.0, top: max_cy - 6.0, right: max_cx + 6.0, bottom: max_cy + 6.0 },
            max_brush,
            1.6,
            None,
        );

        // Close Button (✕) with Windows 11 Red Hover
        let close_rect = D2D_RECT_F { left: w - 44.0, top: 4.0, right: w - 6.0, bottom: btn_h - 4.0 };
        let close_rr = D2D1_ROUNDED_RECT { rect: close_rect, radiusX: 8.0, radiusY: 8.0 };
        if self.close_hover {
            self.rt.FillRoundedRectangle(&close_rr, &self.brushes.close_btn_hover_bg);
            self.fmt_caption_close.text(&self.rt, "✕", &close_rect, &self.brushes.white);
        } else {
            self.fmt_caption_close.text(&self.rt, "✕", &close_rect, &self.brushes.dim);
        }

        // The content deliberately starts with live/upcoming state instead of repeating a
        // generic "match discovery" heading. It makes the next decision obvious at a glance.
        if matches.is_empty() {
            self.scroll_offset = 0;
            self.content_height = 0.0;
            let empty_rect = D2D_RECT_F { left: 32.0, top: 280.0, right: w - 32.0, bottom: 360.0 };
            self.fmt_subtitle.text(
                &self.rt,
                "No live matches detected currently. Polling feeds in background...",
                &empty_rect,
                &self.brushes.subtle,
            );
        } else {
            let mut live_groups = Vec::new();
            let mut upcoming_groups = Vec::new();
            for (index, m) in matches.iter().enumerate() {
                if is_live_match(m) {
                    push_to_league_group(&mut live_groups, &m.league_name, index);
                } else {
                    push_to_league_group(&mut upcoming_groups, &m.league_name, index);
                }
            }

            let max_cols = if w >= 1500.0 { 3 } else if w >= 900.0 { 2 } else { 1 };
            let gap = 16.0;
            let content_top = TITLE_BAR_HEIGHT + 16.0;
            let content_bottom = h - CONTENT_BOTTOM_GUTTER;
            let scroll_px = self.scroll_offset as f32 * SCROLL_STEP;
            let mut y = ITEM_TOP_START - scroll_px;
            let clip = D2D_RECT_F { left: 1.0, top: content_top, right: w - 1.0, bottom: content_bottom };
            self.rt.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            for (is_live, groups) in [(true, &live_groups), (false, &upcoming_groups)] {
                if groups.is_empty() {
                    continue;
                }

                let section_rect = D2D_RECT_F { left: CARD_LEFT, top: y, right: w - CARD_RIGHT, bottom: y + 24.0 };
                let section_label = if is_live { "●  LIVE MATCHES" } else { "◷  UPCOMING MATCHES" };
                let section_brush = if is_live { &self.brushes.live_badge_text } else { &self.brushes.blue_badge_text };
                self.fmt_eyebrow.text(&self.rt, section_label, &section_rect, section_brush);
                y += 34.0;

                for group in groups {
                    // The league marker is intentionally separate from its label. This avoids
                    // the bar colliding with the first letter at every window size.
                    let league_marker = D2D_RECT_F { left: CARD_LEFT, top: y + 4.0, right: CARD_LEFT + 3.0, bottom: y + 24.0 };
                    let league_rect = D2D_RECT_F { left: CARD_LEFT + 14.0, top: y, right: w - CARD_RIGHT, bottom: y + 28.0 };
                    let league_brush = if is_live { &self.brushes.live_badge_text } else { &self.brushes.blue_badge_text };
                    self.rt.FillRectangle(&league_marker, league_brush);
                    self.fmt_eyebrow.text(&self.rt, &group.name.to_uppercase(), &league_rect, &self.brushes.dim);
                    y += 34.0;

                    let group_cols = max_cols.min(group.match_indices.len().max(1));
                    let card_height = if is_live { 74.0 } else { ITEM_HEIGHT };
                    let card_width = (w - CARD_LEFT - CARD_RIGHT - gap * (group_cols as f32 - 1.0)) / group_cols as f32;
                    for (position, index) in group.match_indices.iter().copied().enumerate() {
                        let row = position / group_cols;
                        let column = position % group_cols;
                        let top = y + row as f32 * (card_height + ITEM_SPACING);
                        let left = CARD_LEFT + column as f32 * (card_width + gap);
                        let item_rect = D2D_RECT_F { left, top, right: left + card_width, bottom: top + card_height };
                        self.card_layout.push((index, item_rect));
                        let item_rr = D2D1_ROUNDED_RECT { rect: item_rect, radiusX: 10.0, radiusY: 10.0 };
                        let m = &matches[index];
                        let is_selected = selected_id.as_ref().map_or(false, |id| id == &m.match_id);
                        let is_hovered = self.hover_index == Some(index);

                        if is_selected {
                            self.rt.FillRoundedRectangle(&item_rr, &self.brushes.card_active);
                            self.rt.DrawRoundedRectangle(&item_rr, &self.brushes.active_border, 1.6, None);
                        } else if is_hovered {
                            self.rt.FillRoundedRectangle(&item_rr, &self.brushes.card_hover);
                            self.rt.DrawRoundedRectangle(&item_rr, &self.brushes.border, 1.2, None);
                        } else {
                            self.rt.FillRoundedRectangle(&item_rr, &self.brushes.card_bg);
                            self.rt.DrawRoundedRectangle(&item_rr, &self.brushes.border, 1.0, None);
                        }

                        let action_rect = D2D_RECT_F {
                            left: item_rect.right - ACTION_W - 16.0,
                            top: top + (card_height - 36.0) / 2.0,
                            right: item_rect.right - 16.0,
                            bottom: top + (card_height + 36.0) / 2.0,
                        };
                        let action_rr = D2D1_ROUNDED_RECT { rect: action_rect, radiusX: 7.0, radiusY: 7.0 };
                        let action_hovered = self.action_hover_index == Some(index);
                        self.rt.FillRoundedRectangle(
                            &action_rr,
                            if action_hovered { &self.brushes.action_hover_bg } else { &self.brushes.action_bg },
                        );
                        self.rt.DrawRoundedRectangle(&action_rr, &self.brushes.action_border, 1.0, None);
                        let action_label = if is_selected {
                            if action_hovered { "UNTRACK" } else { "TRACKING" }
                        } else { "TRACK" };
                        self.fmt_badge.text(&self.rt, action_label, &action_rect, &self.brushes.action_text);

                        let text_right = action_rect.left - 20.0;
                        if is_live {
                            let title_rect = D2D_RECT_F { left: left + 24.0, top: top + 18.0, right: text_right, bottom: top + 54.0 };
                            self.fmt_item_title.text(&self.rt, &m.title, &title_rect, &self.brushes.white);
                        } else {
                            let title_rect = D2D_RECT_F { left: left + 24.0, top: top + 14.0, right: text_right, bottom: top + 48.0 };
                            let time_rect = D2D_RECT_F { left: left + 24.0, top: top + 52.0, right: text_right, bottom: top + 78.0 };
                            self.fmt_item_title.text(&self.rt, &m.title, &title_rect, &self.brushes.white);
                            self.fmt_item_sub.text(&self.rt, &format_upcoming_time(&m.start_time), &time_rect, &self.brushes.dim);
                        }
                    }
                    let rows = (group.match_indices.len() + group_cols - 1) / group_cols;
                    y += rows as f32 * (card_height + ITEM_SPACING) + 22.0;
                }
                y += 14.0;
            }
            self.rt.PopAxisAlignedClip();

            self.content_height = (y + scroll_px - content_top).max(0.0);
            self.scroll_offset = self.scroll_offset.min(self.max_scroll());
            if self.max_scroll() > 0 {
                let track_top = content_top;
                let track_bottom = content_bottom;
                let track_height = (track_bottom - track_top).max(1.0);
                let viewport = (content_bottom - content_top).max(1.0);
                let thumb_height = (track_height * (viewport / self.content_height)).clamp(32.0, track_height);
                let thumb_top = track_top + (track_height - thumb_height)
                    * (self.scroll_offset as f32 / self.max_scroll().max(1) as f32);
                let track = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: w - 15.0, top: track_top, right: w - 10.0, bottom: track_bottom }, radiusX: 2.5, radiusY: 2.5 };
                let thumb = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: w - 15.0, top: thumb_top, right: w - 10.0, bottom: thumb_top + thumb_height }, radiusX: 2.5, radiusY: 2.5 };
                self.rt.FillRoundedRectangle(&track, &self.brushes.icon_box_bg);
                self.rt.FillRoundedRectangle(&thumb, &self.brushes.subtle);
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

    pub fn set_hover(
        &mut self,
        index: Option<usize>,
        action_index: Option<usize>,
        min: bool,
        max: bool,
        close: bool,
    ) {
        self.hover_index = index;
        self.action_hover_index = action_index;
        self.min_hover = min;
        self.max_hover = max;
        self.close_hover = close;
    }

    pub fn scroll_by(&mut self, rows: isize, _match_count: usize) {
        let max_offset = self.max_scroll();
        let next = self.scroll_offset as isize + rows;
        self.scroll_offset = next.clamp(0, max_offset as isize) as usize;
    }

    fn max_scroll(&self) -> usize {
        let viewport = (self.h as f32 - TITLE_BAR_HEIGHT - 16.0 - CONTENT_BOTTOM_GUTTER).max(1.0);
        ((self.content_height - viewport).max(0.0) / SCROLL_STEP).ceil() as usize
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
