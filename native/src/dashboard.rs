//! SportsPulse — Win11 Dark Theme Match Selection Dashboard.
//! Win11-inspired match discovery surface with custom rendering and standard window behavior.

#![allow(dead_code)]

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};

use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, GetWindowDC,
    MonitorFromWindow, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA, WHEEL_DELTA};

use crate::engine::events::DiscoveredMatch;
use crate::engine::models::SportType;
use crate::render::{
    d2d_factory, dwrite_factory, high_contrast, reduced_motion, software_rt_props, ui_text,
    UI_SCALE,
};
use chrono::{DateTime, FixedOffset};

pub const ICON_128_PNG: &[u8] = include_bytes!("../assets/icon-128.png");

fn create_logo_bitmap(
    wicf: &IWICImagingFactory,
    rt: &ID2D1RenderTarget,
) -> Option<ID2D1Bitmap> {
    unsafe {
        let stream = wicf.CreateStream().ok()?;
        stream.InitializeFromMemory(ICON_128_PNG).ok()?;
        let decoder = wicf
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let converter = wicf.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        rt.CreateBitmapFromWicBitmap(&converter, None).ok()
    }
}

// Spacious, responsive default dimensions
pub const DASH_NORMAL_W: u32 = 1120;
pub const DASH_NORMAL_H: u32 = 760;

pub const WM_APP_SELECT_MATCH: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub const WM_APP_UNTRACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;

pub const TITLE_BAR_HEIGHT: f32 = 48.0;
pub const PAGE_HEADER_HEIGHT: f32 = 68.0;
pub const ITEM_TOP_START: f32 = TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT + 16.0;
pub const ITEM_HEIGHT: f32 = 92.0;
pub const ITEM_SPACING: f32 = 12.0;
const CARD_LEFT: f32 = 32.0;
const CARD_RIGHT: f32 = 32.0;
const ACTION_W: f32 = 108.0;
const ACTION_H: f32 = 32.0;
const CONTENT_BOTTOM_GUTTER: f32 = 24.0;
const SCROLL_STEP: f32 = 72.0; // pixels per wheel notch
const SWITCHER_W: f32 = 278.0;
const SWITCHER_H: f32 = 43.0;
const EMPTY_CARD_H: f32 = 116.0;
const SPINNER_RADIUS: f32 = 16.0;

fn get_work_area_size(hwnd: HWND) -> (i32, i32) {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut mi).as_bool() {
            (
                mi.rcWork.right - mi.rcWork.left,
                mi.rcWork.bottom - mi.rcWork.top,
            )
        } else {
            (1920, 1080)
        }
    }
}

/// Set by main.rs when both global-hotkey registrations fail; present() then
/// draws a conflict toast in the title bar instead of running silently hotkey-less.
pub static HOTKEY_CONFLICT: AtomicBool = AtomicBool::new(false);

#[inline]
fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    MinimizeButton,
    MaximizeButton,
    CloseButton,
    CricketTab,
    FootballTab,
    TitleBar,
    Scrollbar,
    MatchItem(usize),
    MatchAction(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardSport {
    Cricket,
    Football,
}

impl DashboardSport {
    fn matches(self, sport: SportType) -> bool {
        matches!(
            (self, sport),
            (Self::Cricket, SportType::Cricket) | (Self::Football, SportType::Soccer)
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cricket => "cricket",
            Self::Football => "football",
        }
    }
}

#[derive(Debug, Clone)]
struct CachedCard {
    match_index: usize,
    formatted_time: String,
    title_text: String,
}

#[derive(Debug, Clone)]
struct CachedLeagueGroup {
    name: String,
    display_title: String,
    cards: Vec<CachedCard>,
}

struct GroupCache {
    sport: DashboardSport,
    matches_len: usize,
    first_match_id: String,
    last_match_id: String,
    live_groups: Vec<CachedLeagueGroup>,
    upcoming_groups: Vec<CachedLeagueGroup>,
}

fn is_live_match(m: &DiscoveredMatch) -> bool {
    let status = m.status.trim().to_ascii_lowercase();
    status == "in" || status.contains("live") || status.contains("in progress")
}

fn format_upcoming_time(value: &str) -> String {
    // Display-only IST conversion. ESPN event timestamps arrive as UTC RFC3339;
    // scoreboard `dates=` in engine/fetcher.rs (`start_polling`) is Utc-built
    // (chrono::Utc, engine-owned).
    // P0-9: never panic on a const offset — fall back to the raw string.
    let Some(offset) = FixedOffset::east_opt(5 * 60 * 60 + 30 * 60) else {
        return value.to_owned();
    };
    DateTime::parse_from_rfc3339(value)
        .map(|time| {
            time.with_timezone(&offset)
                .format("%-d %b, %I:%M %p IST")
                .to_string()
        })
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
    action_tracked_bg: ID2D1SolidColorBrush,
    action_tracked_border: ID2D1SolidColorBrush,
    action_tracked_text: ID2D1SolidColorBrush,
    action_danger_bg: ID2D1SolidColorBrush,
    action_danger_hover_bg: ID2D1SolidColorBrush,
    action_danger_border: ID2D1SolidColorBrush,
    action_danger_text: ID2D1SolidColorBrush,
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
    tab_active_bg: ID2D1SolidColorBrush,
    spinner_track: ID2D1SolidColorBrush,
    spinner_accent: ID2D1SolidColorBrush,
}

impl Brushes {
    unsafe fn create(rt: &ID2D1RenderTarget) -> Result<Self> {
        Ok(Self {
            bg: rt.CreateSolidColorBrush(&color(0.071, 0.071, 0.071, 1.0), None)?, // #121212
            header_bg: rt.CreateSolidColorBrush(&color(0.110, 0.110, 0.110, 1.0), None)?, // #1C1C1C
            card_bg: rt.CreateSolidColorBrush(&color(0.110, 0.110, 0.110, 1.0), None)?, // #1C1C1C
            card_hover: rt.CreateSolidColorBrush(&color(0.133, 0.133, 0.133, 1.0), None)?, // #222222
            card_active: rt.CreateSolidColorBrush(&color(0.090, 0.220, 0.130, 1.0), None)?, // #173821
            border: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D
            active_border: rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?, // #22C55E
            divider: rt.CreateSolidColorBrush(&color(0.118, 0.118, 0.141, 1.0), None)?, // #1E1E24
            white: rt.CreateSolidColorBrush(&color(0.910, 0.910, 0.925, 1.0), None)?,   // #E8E8EC
            dim: rt.CreateSolidColorBrush(&color(0.545, 0.561, 0.627, 1.0), None)?,     // #8B8FA0
            subtle: rt.CreateSolidColorBrush(&color(0.420, 0.435, 0.482, 1.0), None)?,  // #6B6F7B
            icon_box_bg: rt.CreateSolidColorBrush(&color(0.145, 0.145, 0.145, 1.0), None)?, // #252525
            action_bg: rt.CreateSolidColorBrush(&color(0.141, 0.141, 0.141, 1.0), None)?, // #242424
            action_hover_bg: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D
            action_border: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D
            action_text: rt.CreateSolidColorBrush(&color(0.910, 0.910, 0.925, 1.0), None)?, // #E8E8EC
            action_tracked_bg: rt.CreateSolidColorBrush(&color(0.455, 0.776, 0.616, 0.12), None)?,
            action_tracked_border: rt
                .CreateSolidColorBrush(&color(0.455, 0.776, 0.616, 0.25), None)?,
            action_tracked_text: rt
                .CreateSolidColorBrush(&color(0.455, 0.776, 0.616, 1.0), None)?, // #74C69D
            action_danger_bg: rt.CreateSolidColorBrush(&color(0.340, 0.082, 0.102, 1.0), None)?,
            action_danger_hover_bg: rt
                .CreateSolidColorBrush(&color(0.500, 0.090, 0.122, 1.0), None)?,
            action_danger_border: rt
                .CreateSolidColorBrush(&color(0.900, 0.250, 0.290, 1.0), None)?,
            action_danger_text: rt.CreateSolidColorBrush(&color(1.000, 0.850, 0.860, 1.0), None)?,
            caption_btn_hover_bg: rt
                .CreateSolidColorBrush(&color(0.235, 0.235, 0.235, 1.0), None)?, // #3C3C3C

            live_badge_bg: rt.CreateSolidColorBrush(&color(1.0, 0.29, 0.29, 0.12), None)?,
            live_badge_text: rt.CreateSolidColorBrush(&color(1.0, 0.29, 0.29, 1.0), None)?, // #FF4A4A

            amber_badge_bg: rt.CreateSolidColorBrush(&color(0.941, 0.706, 0.161, 0.15), None)?,
            amber_badge_text: rt.CreateSolidColorBrush(&color(0.941, 0.706, 0.161, 1.0), None)?, // #F0B429

            blue_badge_bg: rt.CreateSolidColorBrush(&color(0.353, 0.608, 0.835, 0.15), None)?,
            blue_badge_text: rt.CreateSolidColorBrush(&color(0.353, 0.608, 0.835, 1.0), None)?, // #5A9BD5

            final_badge_bg: rt.CreateSolidColorBrush(&color(0.165, 0.165, 0.165, 1.0), None)?,
            final_badge_text: rt.CreateSolidColorBrush(&color(0.58, 0.58, 0.58, 1.0), None)?,

            close_btn_hover_bg: rt.CreateSolidColorBrush(&color(0.910, 0.067, 0.137, 1.0), None)?, // #E81123
            // Solid segmented-control pill: clearly lifts the active sport tab
            // off the #252525 switcher container (card_hover #222222 was invisible).
            tab_active_bg: rt.CreateSolidColorBrush(&color(0.227, 0.227, 0.227, 1.0), None)?, // #3A3A3A
            spinner_track: rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 0.10), None)?,
            spinner_accent: rt.CreateSolidColorBrush(&color(0.353, 0.608, 0.835, 1.0), None)?, // #5A9BD5
        })
    }
}

pub struct DashboardRenderer {
    hwnd: HWND,
    wic_factory: IWICImagingFactory,
    wic: IWICBitmap,
    rt: ID2D1RenderTarget,
    mem_dc: HDC,
    hbmp: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut core::ffi::c_void,
    pub w: i32,
    pub h: i32,
    pub dpi: u32,
    logo_bitmap: Option<ID2D1Bitmap>,
    brushes: Brushes,
    fmt_app_title: Fmt,
    fmt_page_title: Fmt,
    fmt_eyebrow: Fmt,
    fmt_subtitle: Fmt,
    fmt_item_title: Fmt,
    fmt_item_sub: Fmt,
    fmt_badge: Fmt,
    fmt_action: Fmt,
    fmt_tab: Fmt,
    fmt_icon: Fmt,
    fmt_empty: Fmt,
    fmt_loading: Fmt,
    fmt_caption_min: Fmt,
    fmt_caption_max: Fmt,
    fmt_caption_close: Fmt,
    pub hover_index: Option<usize>,
    pub action_hover_index: Option<usize>,
    pub min_hover: bool,
    pub max_hover: bool,
    pub close_hover: bool,
    pub cricket_hover: bool,
    pub football_hover: bool,
    pub is_maximized: bool,
    /// Vertical scroll position in pixels (interpolated smoothly towards target_scroll_offset).
    pub scroll_offset: f32,
    /// Target scroll position in pixels (clamped to max_scroll()).
    pub target_scroll_offset: f32,
    /// Accumulated raw wheel delta.
    pub wheel_accum: f32,
    /// True while a TME_LEAVE track is armed (re-armed on next mouse move after leave).
    pub mouse_tracking: bool,
    pub active_sport: DashboardSport,
    content_height: f32,
    /// (global match index, card rect, exact 44px action-button rect).
    card_layout: Vec<(usize, D2D_RECT_F, D2D_RECT_F)>,
    /// Scrollbar hover (cursor over track) and active (thumb being dragged)
    /// states. Drive the thumb through subtle → dim → white so hover and
    /// grab are unmistakable. Set from the main wnd_proc hover/drag paths.
    pub scroll_hover: bool,
    pub scroll_active: bool,
    /// Last-painted scrollbar geometry in design DIPs (None when content fits).
    /// Hit-testing and thumb-dragging read these; refreshed every present().
    pub scrollbar_track: Option<D2D_RECT_F>,
    pub scrollbar_thumb: Option<D2D_RECT_F>,
    spinner_origin: Instant,
    // Persistent pixel scratch buffer: avoids a ~3.4MB alloc + double copy per present.
    buf: Vec<u8>,
    // Cached layout structure to avoid per-frame allocations during scrolling and hover.
    group_cache: Option<GroupCache>,
    // STA-bound COM/GDI state must never cross threads.
    _no_send: PhantomData<*const ()>,
}

impl DashboardRenderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32, dpi: u32) -> Result<Self> {
        let factory = d2d_factory()?;
        let dwrite = dwrite_factory()?;
        let wicf: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

        let wic =
            wicf.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;

        let rt = factory.CreateWicBitmapRenderTarget(&wic, &software_rt_props())?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let d = (if dpi == 0 { 96.0 } else { dpi as f32 }) * UI_SCALE;
        rt.SetDpi(d, d);

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

        let brushes = Brushes::create(&rt)?;
        let logo_bitmap = create_logo_bitmap(&wicf, &rt);

        let mk_font =
            |size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT| -> Result<Fmt> {
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
                Ok(Fmt {
                    fmt,
                    buf: RefCell::new(Vec::new()),
                })
            };

        // Fallback emoji icon font if logo bitmap fails
        let fmt_icon_fmt = dwrite.CreateTextFormat(
            w!("Segoe UI Emoji"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            24.0,
            w!("en-us"),
        )?;
        fmt_icon_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        fmt_icon_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        fmt_icon_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

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
            logo_bitmap,
            brushes,
            // Prominent, large, readable typography
            fmt_app_title: mk_font(16.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_page_title: mk_font(20.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_eyebrow: mk_font(16.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_subtitle: mk_font(
                17.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            fmt_item_title: mk_font(22.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            fmt_item_sub: mk_font(
                17.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            fmt_badge: mk_font(13.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_action: mk_font(13.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_tab: mk_font(15.6, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_icon: Fmt {
                fmt: fmt_icon_fmt,
                buf: RefCell::new(Vec::new()),
            },
            fmt_empty: mk_font(
                20.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            fmt_loading: mk_font(15.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_caption_min: mk_font(20.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_caption_max: mk_font(17.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            fmt_caption_close: mk_font(
                18.0,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            hover_index: None,
            action_hover_index: None,
            min_hover: false,
            max_hover: false,
            close_hover: false,
            cricket_hover: false,
            football_hover: false,
            is_maximized: false,
            scroll_offset: 0.0,
            target_scroll_offset: 0.0,
            wheel_accum: 0.0,
            mouse_tracking: false,
            active_sport: DashboardSport::Cricket,
            content_height: 0.0,
            card_layout: Vec::new(),
            scroll_hover: false,
            scroll_active: false,
            scrollbar_track: None,
            scrollbar_thumb: None,
            spinner_origin: Instant::now(),
            buf: Vec::new(),
            group_cache: None,
            _no_send: PhantomData,
        })
    }

    /// Rebuild only the WIC bitmap / render target / brushes / DIB on resize.
    /// Factories (D2D/DWrite globals, per-renderer WIC) and mem_dc are reused.
    pub unsafe fn resize(&mut self, new_w: u32, new_h: u32, dpi: u32) -> Result<()> {
        if self.w == new_w as i32 && self.h == new_h as i32 && self.dpi == dpi {
            return Ok(());
        }
        self.dpi = dpi;

        // Park the previously selected bitmap; deleting a selected GDI object is a no-op leak.
        if !self.mem_dc.is_invalid() {
            let _ = SelectObject(self.mem_dc, self.old_bmp);
        }
        if !self.hbmp.is_invalid() {
            let _ = DeleteObject(self.hbmp);
        }

        let factory = d2d_factory()?;
        let wic = self.wic_factory.CreateBitmap(
            new_w,
            new_h,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnDemand,
        )?;

        let rt = factory.CreateWicBitmapRenderTarget(&wic, &software_rt_props())?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let d = (if dpi == 0 { 96.0 } else { dpi as f32 }) * UI_SCALE;
        rt.SetDpi(d, d);

        // Reuse mem_dc across resizes; recreate only if it was lost.
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
        bmi.bmiHeader.biWidth = new_w as i32;
        bmi.bmiHeader.biHeight = -(new_h as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(self.mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if hbmp.is_invalid() {
            return Err(Error::from_win32());
        }
        self.old_bmp = SelectObject(self.mem_dc, hbmp);

        self.brushes = Brushes::create(&rt)?;
        self.logo_bitmap = create_logo_bitmap(&self.wic_factory, &rt);
        self.wic = wic;
        self.rt = rt;
        self.hbmp = hbmp;
        self.bits = bits;
        self.w = new_w as i32;
        self.h = new_h as i32;
        // Force the persistent buffer back to the new footprint on next present.
        self.buf.clear();

        Ok(())
    }

    /// Force a full rebuild at the current size (D2DERR_RECREATE_TARGET
    /// recovery). resize() intentionally no-ops on identical dims, so device-
    /// lost recovery needs this explicit path; caller retries present() once.
    pub unsafe fn recreate(&mut self) -> Result<()> {
        let (w, h) = (self.w, self.h);
        let dpi = self.dpi;
        self.w = 0;
        self.h = 0;
        self.resize(w as u32, h as u32, dpi)
    }

    fn page_content_top() -> f32 {
        TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT
    }

    /// Single source of truth for the three caption buttons, shared by draw
    /// and hit-test so the clickable rects exactly match the painted ones.
    /// Returns (minimize, maximize, close).
    pub fn caption_rects(w: f32) -> (D2D_RECT_F, D2D_RECT_F, D2D_RECT_F) {
        let top = 4.0;
        let bottom = TITLE_BAR_HEIGHT - 4.0;
        (
            D2D_RECT_F {
                left: w - 136.0,
                top,
                right: w - 94.0,
                bottom,
            },
            D2D_RECT_F {
                left: w - 90.0,
                top,
                right: w - 48.0,
                bottom,
            },
            D2D_RECT_F {
                left: w - 44.0,
                top,
                right: w - 6.0,
                bottom,
            },
        )
    }

    pub fn container_bounds(w: f32) -> (f32, f32) {
        let max_w: f32 = 1800.0;
        let target_w = (w * 0.94).min(max_w);
        let left = ((w - target_w) / 2.0).max(32.0);
        let right = w - left;
        (left, right)
    }

    fn sport_switcher_rects(&self) -> (D2D_RECT_F, D2D_RECT_F, D2D_RECT_F) {
        let size = unsafe { self.rt.GetSize() };
        let w = size.width;
        let (_, container_right) = Self::container_bounds(w);
        let cy = TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT / 2.0;
        let switcher = D2D_RECT_F {
            left: container_right - SWITCHER_W,
            top: cy - SWITCHER_H / 2.0,
            right: container_right,
            bottom: cy + SWITCHER_H / 2.0,
        };
        let mid = switcher.left + SWITCHER_W / 2.0;
        let cricket = D2D_RECT_F {
            left: switcher.left + 3.0,
            top: switcher.top + 3.0,
            right: mid - 2.0,
            bottom: switcher.bottom - 3.0,
        };
        let football = D2D_RECT_F {
            left: mid + 2.0,
            top: switcher.top + 3.0,
            right: switcher.right - 3.0,
            bottom: switcher.bottom - 3.0,
        };
        (switcher, cricket, football)
    }

    unsafe fn draw_spinner(&self, cx: f32, cy: f32) {
        let ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: cx, y: cy },
            radiusX: SPINNER_RADIUS,
            radiusY: SPINNER_RADIUS,
        };
        self.rt
            .DrawEllipse(&ellipse, &self.brushes.spinner_track, 3.5, None);

        if reduced_motion() {
            // Static ring when the user disabled animation: no rotation.
            self.rt
                .DrawEllipse(&ellipse, &self.brushes.spinner_accent, 3.6, None);
            return;
        }

        let t = self.spinner_origin.elapsed().as_secs_f32();
        // Gentle 0.6 rev/s: at the 50ms loader tick the arc advances ~11° per
        // frame instead of jumping ~43°, which reads as smooth rather than janky.
        let start = t * std::f32::consts::TAU * 0.6;
        let sweep = 1.85_f32;
        let steps = 28;
        for i in 0..steps {
            let a0 = start + sweep * (i as f32 / steps as f32);
            let a1 = start + sweep * ((i + 1) as f32 / steps as f32);
            self.rt.DrawLine(
                D2D_POINT_2F {
                    x: cx + SPINNER_RADIUS * a0.cos(),
                    y: cy + SPINNER_RADIUS * a0.sin(),
                },
                D2D_POINT_2F {
                    x: cx + SPINNER_RADIUS * a1.cos(),
                    y: cy + SPINNER_RADIUS * a1.sin(),
                },
                &self.brushes.spinner_accent,
                3.6,
                None,
            );
        }
    }

    /// Pixels → design-DIP divisor. The render target runs at
    /// monitor-DPI × UI_SCALE, so physical mouse pixels must be divided by
    /// (dpi/96 × UI_SCALE) to land in layout space. Single source of truth
    /// shared by hit_test() and scrollbar-dragging.
    pub fn dip_scale(&self) -> f32 {
        if self.dpi == 0 {
            UI_SCALE
        } else {
            self.dpi as f32 / 96.0 * UI_SCALE
        }
    }

    pub fn hit_test(&self, x_px: f32, y_px: f32, _match_count: usize) -> Option<HitTarget> {
        // Mouse arrives in physical pixels; the render target works in design
        // DIPs (RT DPI = monitor DPI × UI_SCALE), so divide by both.
        let scale = self.dip_scale();
        let x = x_px / scale;
        let y = y_px / scale;
        let size = unsafe { self.rt.GetSize() };
        let w = size.width;

        if (0.0..=TITLE_BAR_HEIGHT).contains(&y) {
            let (min_r, max_r, close_r) = Self::caption_rects(w);
            if x >= close_r.left && x <= w {
                return Some(HitTarget::CloseButton);
            }
            if x >= max_r.left && x < close_r.left {
                return Some(HitTarget::MaximizeButton);
            }
            if x >= min_r.left && x < max_r.left {
                return Some(HitTarget::MinimizeButton);
            }
            return Some(HitTarget::TitleBar);
        }

        let (switcher, _cricket, _football) = self.sport_switcher_rects();
        if y > TITLE_BAR_HEIGHT
            && y <= Self::page_content_top()
            && x >= switcher.left - 16.0
            && x <= switcher.right + 16.0
        {
            let mid = switcher.left + SWITCHER_W / 2.0;
            if x < mid {
                return Some(HitTarget::CricketTab);
            } else {
                return Some(HitTarget::FootballTab);
            }
        }

        let content_top = Self::page_content_top();
        let content_bottom = size.height - CONTENT_BOTTOM_GUTTER;
        // Scrollbar grabs before cards: the track sits clear of card columns.
        if let Some(track) = self.scrollbar_track {
            if x >= track.left && x <= track.right && y >= track.top && y <= track.bottom {
                return Some(HitTarget::Scrollbar);
            }
        }
        if y >= content_top && y <= content_bottom {
            // Action buttons hit first on their exact rects; card bodies second.
            for (index, _card, action) in &self.card_layout {
                if x >= action.left && x <= action.right && y >= action.top && y <= action.bottom {
                    return Some(HitTarget::MatchAction(*index));
                }
            }
            for (index, card, _action) in &self.card_layout {
                if x >= card.left && x <= card.right && y >= card.top && y <= card.bottom {
                    return Some(HitTarget::MatchItem(*index));
                }
            }
        }

        None
    }

    pub unsafe fn present(
        &mut self,
        pos: &POINT,
        matches: &[DiscoveredMatch],
        selected_id: &Option<String>,
        loading: bool,
    ) -> Result<()> {
        let size = unsafe { self.rt.GetSize() };
        let (w, h) = (size.width, size.height);
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());
        self.card_layout.clear();
        self.rt.BeginDraw();
        self.rt.Clear(None);

        // Surface detection: check both is_maximized flag and geometry vs monitor work area
        let (wa_w, wa_h) = get_work_area_size(self.hwnd);
        let is_max = self.is_maximized || (self.w >= wa_w - 4 && self.h >= wa_h - 4);

        // Window surface: crisp square (radius 0.0) when maximized, 10px rounded rect when normal.
        let full_rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        let radius = if is_max { 0.0 } else { 10.0 };
        if radius > 0.0 {
            let rr = D2D1_ROUNDED_RECT {
                rect: full_rect,
                radiusX: radius,
                radiusY: radius,
            };
            self.rt.FillRoundedRectangle(&rr, &self.brushes.bg);

            // Title bar header: rounded top corners matching window, flat bottom at TITLE_BAR_HEIGHT
            let header_clip = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: TITLE_BAR_HEIGHT,
            };
            self.rt
                .PushAxisAlignedClip(&header_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.rt.FillRoundedRectangle(&rr, &self.brushes.header_bg);
            self.rt.PopAxisAlignedClip();

            let border_w = if high_contrast() { 2.0 } else { 1.0 };
            self.rt
                .DrawRoundedRectangle(&rr, &self.brushes.border, border_w, None);
        } else {
            self.rt.FillRectangle(&full_rect, &self.brushes.bg);

            // Title bar header: crisp square
            let header_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: TITLE_BAR_HEIGHT,
            };
            self.rt.FillRectangle(&header_rect, &self.brushes.header_bg);

            if high_contrast() {
                self.rt
                    .DrawRectangle(&full_rect, &self.brushes.border, 2.0, None);
            }
        }

        // Divider below title bar
        self.rt.DrawLine(
            windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                x: 0.0,
                y: TITLE_BAR_HEIGHT,
            },
            windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                x: w,
                y: TITLE_BAR_HEIGHT,
            },
            &self.brushes.divider,
            1.0,
            None,
        );

        let title_rect = D2D_RECT_F {
            left: 28.0,
            top: 0.0,
            right: w - 148.0,
            bottom: TITLE_BAR_HEIGHT,
        };
        self.fmt_app_title.text(
            &self.rt,
            "SportsPulse Dashboard",
            &title_rect,
            &self.brushes.white,
        );

        // Hotkey-conflict toast (main.rs sets HOTKEY_CONFLICT when both
        // RegisterHotKey ids fail). Remap UI is out of scope here.
        if HOTKEY_CONFLICT.load(Ordering::Relaxed) {
            let warn_rect = D2D_RECT_F {
                left: w - 560.0,
                top: 0.0,
                right: w - 148.0,
                bottom: TITLE_BAR_HEIGHT,
            };
            self.fmt_item_sub.text(
                &self.rt,
                "⚠ Hotkey unavailable (conflict)",
                &warn_rect,
                &self.brushes.amber_badge_text,
            );
        }

        // 3. Caption Buttons: Minimize (−), Maximize/Restore (□ / ❐), Close (✕)
        // Rects come from caption_rects() — the same helper hit_test uses.
        let (min_rect, max_rect, close_rect) = Self::caption_rects(w);

        // Minimize button, drawn as geometry for consistent optical size at every DPI.
        let min_rr = D2D1_ROUNDED_RECT {
            rect: min_rect,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if self.min_hover {
            self.rt
                .FillRoundedRectangle(&min_rr, &self.brushes.caption_btn_hover_bg);
        }
        let min_brush = if self.min_hover {
            &self.brushes.white
        } else {
            &self.brushes.dim
        };
        self.rt.DrawLine(
            D2D_POINT_2F {
                x: w - 122.0,
                y: TITLE_BAR_HEIGHT / 2.0,
            },
            D2D_POINT_2F {
                x: w - 108.0,
                y: TITLE_BAR_HEIGHT / 2.0,
            },
            min_brush,
            2.0,
            None,
        );

        // Maximize / restore button, a crisp square rather than a small font glyph.
        let max_rr = D2D1_ROUNDED_RECT {
            rect: max_rect,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if self.max_hover {
            self.rt
                .FillRoundedRectangle(&max_rr, &self.brushes.caption_btn_hover_bg);
        }
        let max_brush = if self.max_hover {
            &self.brushes.white
        } else {
            &self.brushes.dim
        };
        let max_cx = w - 69.0;
        let max_cy = TITLE_BAR_HEIGHT / 2.0;
        if is_max {
            let back = D2D_RECT_F {
                left: max_cx - 4.0,
                top: max_cy - 6.0,
                right: max_cx + 6.0,
                bottom: max_cy + 4.0,
            };
            let front = D2D_RECT_F {
                left: max_cx - 6.0,
                top: max_cy - 4.0,
                right: max_cx + 4.0,
                bottom: max_cy + 6.0,
            };
            self.rt.DrawRectangle(&back, max_brush, 1.4, None);
            self.rt.FillRectangle(&front, &self.brushes.header_bg);
            self.rt.DrawRectangle(&front, max_brush, 1.4, None);
        } else {
            self.rt.DrawRectangle(
                &D2D_RECT_F {
                    left: max_cx - 6.0,
                    top: max_cy - 6.0,
                    right: max_cx + 6.0,
                    bottom: max_cy + 6.0,
                },
                max_brush,
                1.6,
                None,
            );
        }

        // Close Button (✕) with Windows 11 Red Hover
        let close_rr = D2D1_ROUNDED_RECT {
            rect: close_rect,
            radiusX: 8.0,
            radiusY: 8.0,
        };
        if self.close_hover {
            self.rt
                .FillRoundedRectangle(&close_rr, &self.brushes.close_btn_hover_bg);
            self.fmt_caption_close
                .text(&self.rt, "✕", &close_rect, &self.brushes.white);
        } else {
            self.fmt_caption_close
                .text(&self.rt, "✕", &close_rect, &self.brushes.dim);
        }

        // Page header: title on the left, cricket/football switcher on the right.
        let (container_left, container_right) = Self::container_bounds(w);
        let (switcher_rect, cricket_rect, football_rect) = self.sport_switcher_rects();

        let logo_size = 36.0;
        let icon_rect = D2D_RECT_F {
            left: container_left,
            top: TITLE_BAR_HEIGHT + (PAGE_HEADER_HEIGHT - logo_size) / 2.0,
            right: container_left + logo_size,
            bottom: TITLE_BAR_HEIGHT + (PAGE_HEADER_HEIGHT + logo_size) / 2.0,
        };
        if let Some(logo) = self.logo_bitmap.as_ref() {
            self.rt.DrawBitmap(
                logo,
                Some(&icon_rect),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        } else {
            self.fmt_icon
                .text(&self.rt, "🏏 ⚽", &icon_rect, &self.brushes.white);
        }
        let page_title_rect = D2D_RECT_F {
            left: container_left + logo_size + 14.0,
            top: TITLE_BAR_HEIGHT,
            right: switcher_rect.left - 16.0,
            bottom: TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT,
        };
        self.fmt_page_title.text(
            &self.rt,
            "SportsPulse Dashboard",
            &page_title_rect,
            &self.brushes.white,
        );

        let switcher_rr = D2D1_ROUNDED_RECT {
            rect: switcher_rect,
            radiusX: 8.0,
            radiusY: 8.0,
        };
        self.rt
            .FillRoundedRectangle(&switcher_rr, &self.brushes.icon_box_bg);
        self.rt
            .DrawRoundedRectangle(&switcher_rr, &self.brushes.border, 1.0, None);
        for (sport, rect, label, hovered) in [
            (
                DashboardSport::Cricket,
                cricket_rect,
                "CRICKET",
                self.cricket_hover,
            ),
            (
                DashboardSport::Football,
                football_rect,
                "FOOTBALL",
                self.football_hover,
            ),
        ] {
            if self.active_sport == sport {
                let tab_rr = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                self.rt
                    .FillRoundedRectangle(&tab_rr, &self.brushes.tab_active_bg);
                self.fmt_tab
                    .text(&self.rt, label, &rect, &self.brushes.white);
            } else if hovered {
                let tab_rr = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                self.rt
                    .FillRoundedRectangle(&tab_rr, &self.brushes.action_hover_bg);
                self.fmt_tab
                    .text(&self.rt, label, &rect, &self.brushes.white);
            } else {
                self.fmt_tab
                    .text(&self.rt, label, &rect, &self.brushes.dim);
            }
        }

        self.rt.DrawLine(
            D2D_POINT_2F {
                x: container_left,
                y: Self::page_content_top(),
            },
            D2D_POINT_2F {
                x: container_right,
                y: Self::page_content_top(),
            },
            &self.brushes.divider,
            1.0,
            None,
        );

        // Reused scratch buffers (fields): no per-present Vec collect.
        if loading {
            self.group_cache = None;
            self.scroll_offset = 0.0;
            self.target_scroll_offset = 0.0;
            self.content_height = 0.0;
            let content_top = Self::page_content_top();
            let cx = w / 2.0;
            let cy = content_top + (h - content_top) * 0.42;
            self.draw_spinner(cx, cy);
            let text_rect = D2D_RECT_F {
                left: 32.0,
                top: cy + SPINNER_RADIUS + 16.0,
                right: w - 32.0,
                bottom: cy + SPINNER_RADIUS + 48.0,
            };
            self.fmt_loading.text(
                &self.rt,
                "FETCHING MATCHES...",
                &text_rect,
                &self.brushes.subtle,
            );
        } else {
            let first_id = matches.first().map(|m| m.match_id.as_str()).unwrap_or("");
            let last_id = matches.last().map(|m| m.match_id.as_str()).unwrap_or("");
            let cache_valid = self.group_cache.as_ref().is_some_and(|c| {
                c.sport == self.active_sport
                    && c.matches_len == matches.len()
                    && c.first_match_id == first_id
                    && c.last_match_id == last_id
            });

            if !cache_valid {
                let sport = self.active_sport;
                let mut live_map: Vec<CachedLeagueGroup> = Vec::new();
                let mut upcoming_map: Vec<CachedLeagueGroup> = Vec::new();

                for (index, m) in matches.iter().enumerate() {
                    if !sport.matches(m.sport) {
                        continue;
                    }
                    let is_live = is_live_match(m);
                    let target_groups = if is_live {
                        &mut live_map
                    } else {
                        &mut upcoming_map
                    };
                    let league_name = if m.league_name.trim().is_empty() {
                        "Other Series"
                    } else {
                        m.league_name.trim()
                    };

                    let card = CachedCard {
                        match_index: index,
                        formatted_time: ui_text(&format_upcoming_time(&m.start_time), 40),
                        title_text: ui_text(&m.title, 64),
                    };

                    if let Some(grp) = target_groups.iter_mut().find(|g| g.name == league_name) {
                        grp.cards.push(card);
                    } else {
                        target_groups.push(CachedLeagueGroup {
                            name: league_name.to_string(),
                            display_title: ui_text(&league_name.to_uppercase(), 48),
                            cards: vec![card],
                        });
                    }
                }

                live_map.sort_by(|a, b| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                });
                upcoming_map.sort_by(|a, b| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                });

                self.group_cache = Some(GroupCache {
                    sport,
                    matches_len: matches.len(),
                    first_match_id: first_id.to_string(),
                    last_match_id: last_id.to_string(),
                    live_groups: live_map,
                    upcoming_groups: upcoming_map,
                });
            }

            let cache = self.group_cache.as_ref().unwrap();

            let gap = 14.0;
            let available_w = container_right - container_left;
            // Wide columns so 22pt titles (e.g. "Jammu & Kashmir v Rest of India")
            // clear the TRACK button without clipping: 1 column at normal width,
            // 3 across when maximized.
            let min_col_w = 560.0;
            let num_cols = ((available_w + gap) / (min_col_w + gap)).floor().max(1.0) as usize;
            let col_w = (available_w - (num_cols - 1) as f32 * gap) / num_cols as f32;
            let content_top = Self::page_content_top();
            let content_bottom = h - CONTENT_BOTTOM_GUTTER;
            let scroll_px = self.scroll_offset;
            let mut y = ITEM_TOP_START - scroll_px;
            let clip = D2D_RECT_F {
                left: 1.0,
                top: content_top + 1.0,
                right: w - 1.0,
                bottom: content_bottom,
            };
            self.rt
                .PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            for (is_live, groups) in [(true, &cache.live_groups), (false, &cache.upcoming_groups)] {
                let section_rect = D2D_RECT_F {
                    left: container_left,
                    top: y,
                    right: container_right,
                    bottom: y + 28.0,
                };
                let section_label = if is_live {
                    "●  LIVE MATCHES"
                } else {
                    "◷  UPCOMING MATCHES"
                };
                let section_brush = if is_live {
                    &self.brushes.live_badge_text
                } else {
                    &self.brushes.blue_badge_text
                };
                if section_rect.bottom >= content_top && section_rect.top <= content_bottom {
                    self.fmt_eyebrow
                        .text(&self.rt, section_label, &section_rect, section_brush);
                }
                y += 40.0;

                if groups.is_empty() {
                    let empty_rect = D2D_RECT_F {
                        left: container_left,
                        top: y,
                        right: container_right,
                        bottom: y + EMPTY_CARD_H,
                    };
                    if empty_rect.bottom >= content_top && empty_rect.top <= content_bottom {
                        let empty_rr = D2D1_ROUNDED_RECT {
                            rect: empty_rect,
                            radiusX: 10.0,
                            radiusY: 10.0,
                        };
                        self.rt
                            .FillRoundedRectangle(&empty_rr, &self.brushes.header_bg);
                        self.rt
                            .DrawRoundedRectangle(&empty_rr, &self.brushes.border, 1.0, None);
                        let empty_copy = if is_live {
                            "No live matches currently"
                        } else {
                            "No upcoming matches currently"
                        };
                        self.fmt_empty.text(
                            &self.rt,
                            empty_copy,
                            &empty_rect,
                            &self.brushes.subtle,
                        );
                    }
                    y += EMPTY_CARD_H + 28.0;
                    continue;
                }

                for group in groups {
                    let league_marker = D2D_RECT_F {
                        left: container_left,
                        top: y + 4.0,
                        right: container_left + 3.0,
                        bottom: y + 26.0,
                    };
                    let league_rect = D2D_RECT_F {
                        left: container_left + 16.0,
                        top: y,
                        right: container_right,
                        bottom: y + 30.0,
                    };
                    let league_brush = if is_live {
                        &self.brushes.live_badge_text
                    } else {
                        &self.brushes.blue_badge_text
                    };
                    if league_rect.bottom >= content_top && league_rect.top <= content_bottom {
                        self.rt.FillRectangle(&league_marker, league_brush);
                        self.fmt_eyebrow.text(
                            &self.rt,
                            &group.display_title,
                            &league_rect,
                            &self.brushes.dim,
                        );
                    }
                    y += 40.0;

                    let card_height = ITEM_HEIGHT;
                    for (position, card) in group.cards.iter().enumerate() {
                        let index = card.match_index;
                        let row = position / num_cols;
                        let column = position % num_cols;
                        let top = y + row as f32 * (card_height + ITEM_SPACING);
                        let left = container_left + column as f32 * (col_w + gap);
                        let item_rect = D2D_RECT_F {
                            left,
                            top,
                            right: left + col_w,
                            bottom: top + card_height,
                        };
                        let action_rect = D2D_RECT_F {
                            left: item_rect.right - ACTION_W - 16.0,
                            top: top + (card_height - ACTION_H) / 2.0,
                            right: item_rect.right - 16.0,
                            bottom: top + (card_height + ACTION_H) / 2.0,
                        };
                        // Viewport culling: only hit-test and render cards visible within viewport
                        if item_rect.bottom >= content_top && item_rect.top <= content_bottom {
                            self.card_layout.push((index, item_rect, action_rect));
                        } else {
                            continue;
                        }
                        let item_rr = D2D1_ROUNDED_RECT {
                            rect: item_rect,
                            radiusX: 8.0,
                            radiusY: 8.0,
                        };
                        let m = &matches[index];
                        let is_selected = selected_id.as_ref() == Some(&m.match_id);
                        let is_hovered = self.hover_index == Some(index);

                        if is_selected {
                            self.rt
                                .FillRoundedRectangle(&item_rr, &self.brushes.card_active);
                            self.rt.DrawRoundedRectangle(
                                &item_rr,
                                &self.brushes.active_border,
                                1.5,
                                None,
                            );
                        } else if is_hovered {
                            self.rt
                                .FillRoundedRectangle(&item_rr, &self.brushes.card_hover);
                            self.rt
                                .DrawRoundedRectangle(&item_rr, &self.brushes.border, 1.2, None);
                        } else {
                            self.rt
                                .FillRoundedRectangle(&item_rr, &self.brushes.card_bg);
                            self.rt
                                .DrawRoundedRectangle(&item_rr, &self.brushes.border, 1.0, None);
                        }

                        let action_rr = D2D1_ROUNDED_RECT {
                            rect: action_rect,
                            radiusX: 6.0,
                            radiusY: 6.0,
                        };
                        let action_hovered = self.action_hover_index == Some(index);
                        let (action_bg, action_border, action_text, action_label) = if is_selected {
                            if action_hovered {
                                (
                                    &self.brushes.action_danger_hover_bg,
                                    &self.brushes.action_danger_border,
                                    &self.brushes.action_danger_text,
                                    "UNTRACK",
                                )
                            } else {
                                (
                                    &self.brushes.action_tracked_bg,
                                    &self.brushes.action_tracked_border,
                                    &self.brushes.action_tracked_text,
                                    "TRACKED",
                                )
                            }
                        } else if action_hovered {
                            (
                                &self.brushes.action_hover_bg,
                                &self.brushes.border,
                                &self.brushes.white,
                                "TRACK",
                            )
                        } else {
                            (
                                &self.brushes.action_bg,
                                &self.brushes.action_border,
                                &self.brushes.action_text,
                                "TRACK",
                            )
                        };
                        self.rt.FillRoundedRectangle(&action_rr, action_bg);
                        self.rt
                            .DrawRoundedRectangle(&action_rr, action_border, 1.0, None);
                        self.fmt_action
                            .text(&self.rt, action_label, &action_rect, action_text);

                        let text_left = left + 16.0;
                        let text_right = action_rect.left - 12.0;
                        if is_live {
                            let title_rect = D2D_RECT_F {
                                left: text_left,
                                top,
                                right: text_right,
                                bottom: top + card_height,
                            };
                            self.fmt_item_title.text(
                                &self.rt,
                                &card.title_text,
                                &title_rect,
                                &self.brushes.white,
                            );
                        } else {
                            let title_rect = D2D_RECT_F {
                                left: text_left,
                                top: top + 14.0,
                                right: text_right,
                                bottom: top + 48.0,
                            };
                            let time_rect = D2D_RECT_F {
                                left: text_left,
                                top: top + 52.0,
                                right: text_right,
                                bottom: top + 78.0,
                            };
                            self.fmt_item_title.text(
                                &self.rt,
                                &card.title_text,
                                &title_rect,
                                &self.brushes.white,
                            );
                            self.fmt_item_sub.text(
                                &self.rt,
                                &card.formatted_time,
                                &time_rect,
                                &self.brushes.dim,
                            );
                        }
                    }
                    let rows = group.cards.len().div_ceil(num_cols);
                    y += rows as f32 * (card_height + ITEM_SPACING) + 16.0;
                }
                y += 14.0;
            }
            self.rt.PopAxisAlignedClip();

            self.content_height = (y + scroll_px - content_top).max(0.0);
            let max_sc = self.max_scroll();
            self.scroll_offset = self.scroll_offset.clamp(0.0, max_sc);
            if max_sc > 0.0 {
                let track_top = content_top;
                let track_bottom = content_bottom;
                let track_height = (track_bottom - track_top).max(1.0);
                let viewport = (content_bottom - content_top).max(1.0);
                let thumb_height =
                    (track_height * (viewport / self.content_height)).clamp(32.0, track_height);
                let thumb_top = track_top
                    + (track_height - thumb_height) * (self.scroll_offset / max_sc.max(1.0));
                // Stored for hit-testing + thumb-dragging (same rects as painted).
                let track_rect = D2D_RECT_F {
                    // 8px-wide scrollbar: minimum operable width.
                    left: w - 18.0,
                    top: track_top,
                    right: w - 10.0,
                    bottom: track_bottom,
                };
                let thumb_rect = D2D_RECT_F {
                    left: w - 18.0,
                    top: thumb_top,
                    right: w - 10.0,
                    bottom: thumb_top + thumb_height,
                };
                self.scrollbar_track = Some(track_rect);
                self.scrollbar_thumb = Some(thumb_rect);
                let track = D2D1_ROUNDED_RECT {
                    rect: track_rect,
                    radiusX: 2.5,
                    radiusY: 2.5,
                };
                let thumb = D2D1_ROUNDED_RECT {
                    rect: thumb_rect,
                    radiusX: 2.5,
                    radiusY: 2.5,
                };
                // Thumb brightens on hover (dim) and while dragged (white).
                let thumb_brush = if self.scroll_active {
                    &self.brushes.white
                } else if self.scroll_hover {
                    &self.brushes.dim
                } else {
                    &self.brushes.subtle
                };
                self.rt
                    .FillRoundedRectangle(&track, &self.brushes.icon_box_bg);
                self.rt.FillRoundedRectangle(&thumb, thumb_brush);
            } else {
                self.scrollbar_track = None;
                self.scrollbar_thumb = None;
            }
        }

        self.rt.EndDraw(None, None)?;

        let row_pitch = (self.w * 4) as usize;
        let total_bytes = row_pitch * self.h as usize;
        debug_assert_eq!(row_pitch, self.w as usize * 4);
        let dib_slice = std::slice::from_raw_parts_mut(self.bits as *mut u8, total_bytes);
        self.wic
            .CopyPixels(std::ptr::null(), row_pitch as u32, dib_slice)?;

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

    #[allow(clippy::too_many_arguments)]
    pub fn set_hover(
        &mut self,
        index: Option<usize>,
        action_index: Option<usize>,
        min: bool,
        max: bool,
        close: bool,
        cricket: bool,
        football: bool,
    ) {
        self.hover_index = index;
        self.action_hover_index = action_index;
        self.min_hover = min;
        self.max_hover = max;
        self.close_hover = close;
        self.cricket_hover = cricket;
        self.football_hover = football;
    }

    pub fn scroll_by(&mut self, notches: isize, _match_count: usize) {
        let px = notches as f32 * SCROLL_STEP;
        self.target_scroll_offset = (self.target_scroll_offset + px).clamp(0.0, self.max_scroll());
        self.scroll_offset = self.target_scroll_offset;
    }

    /// Accumulate raw wheel delta with proportional smooth scrolling.
    /// Returns true if scroll position target changed.
    pub fn accumulate_wheel(&mut self, delta: i32) -> bool {
        let notches = delta as f32 / WHEEL_DELTA as f32;
        let px = notches * SCROLL_STEP;
        let new_target = (self.target_scroll_offset - px).clamp(0.0, self.max_scroll());
        let changed = (new_target - self.target_scroll_offset).abs() > 0.01;
        self.target_scroll_offset = new_target;
        changed
    }

    /// Step scroll animation frame towards target. Returns true if still animating.
    pub fn step_scroll_animation(&mut self) -> bool {
        let diff = self.target_scroll_offset - self.scroll_offset;
        if diff.abs() < 0.5 {
            if (self.scroll_offset - self.target_scroll_offset).abs() > 0.001 {
                self.scroll_offset = self.target_scroll_offset;
                return true;
            }
            return false;
        }
        self.scroll_offset += diff * 0.35;
        true
    }

    /// Clear all hover state (called on WM_MOUSELEAVE).
    pub fn clear_hover(&mut self) {
        self.set_hover(None, None, false, false, false, false, false);
        self.scroll_hover = false;
        self.mouse_tracking = false;
    }

    pub fn max_scroll(&self) -> f32 {
        let size = unsafe { self.rt.GetSize() };
        let viewport = (size.height - Self::page_content_top() - CONTENT_BOTTOM_GUTTER).max(1.0);
        (self.content_height - viewport).max(0.0)
    }
}

impl Drop for DashboardRenderer {
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
