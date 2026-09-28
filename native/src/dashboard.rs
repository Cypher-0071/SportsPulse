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
    ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetWindowDC, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA, WHEEL_DELTA};

use crate::engine::events::DiscoveredMatch;
use crate::engine::models::SportType;
use crate::render::{
    d2d_factory, dwrite_factory, high_contrast, reduced_motion, software_rt_props, ui_text,
};
use chrono::{DateTime, FixedOffset};

// Spacious, large default dimensions
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
const CONTENT_BOTTOM_GUTTER: f32 = 24.0;
const SCROLL_STEP: f32 = 72.0; // pixels per wheel notch
const SWITCHER_W: f32 = 232.0;
const SWITCHER_H: f32 = 36.0;
const EMPTY_CARD_H: f32 = 84.0;
const SPINNER_RADIUS: f32 = 16.0;

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
        match (self, sport) {
            (Self::Cricket, SportType::Cricket) => true,
            (Self::Football, SportType::Soccer) => true,
            _ => false,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cricket => "cricket",
            Self::Football => "football",
        }
    }
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
    let name = if league_name.trim().is_empty() {
        "Other Series"
    } else {
        league_name
    };
    if let Some(group) = groups.iter_mut().find(|group| group.name == name) {
        group.match_indices.push(index);
    } else {
        groups.push(LeagueGroup {
            name: name.to_owned(),
            match_indices: vec![index],
        });
    }
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
    spinner_track: ID2D1SolidColorBrush,
    spinner_accent: ID2D1SolidColorBrush,
}

impl Brushes {
    unsafe fn create(rt: &ID2D1RenderTarget) -> Result<Self> {
        Ok(Self {
            bg: rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?, // #202020
            header_bg: rt.CreateSolidColorBrush(&color(0.110, 0.110, 0.110, 1.0), None)?, // #1C1C1C
            card_bg: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D
            card_hover: rt.CreateSolidColorBrush(&color(0.230, 0.230, 0.230, 1.0), None)?, // #3B3B3B
            card_active: rt.CreateSolidColorBrush(&color(0.090, 0.220, 0.130, 1.0), None)?, // #173821
            border: rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?, // #3E3E3E
            active_border: rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?, // #22C55E
            divider: rt.CreateSolidColorBrush(&color(0.210, 0.210, 0.210, 1.0), None)?, // #353535
            white: rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 1.0), None)?,         // #FFFFFF
            dim: rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?,        // #A6A6A6
            // Raised from #707070: subtle is body text (loading/empty states) and
            // must clear 4.5 like dim. Decorative uses (scrollbar thumb) inherit it.
            subtle: rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?, // #A6A6A6
            icon_box_bg: rt.CreateSolidColorBrush(&color(0.145, 0.145, 0.145, 1.0), None)?, // #252525
            action_bg: rt.CreateSolidColorBrush(&color(0.130, 0.130, 0.130, 1.0), None)?,
            action_hover_bg: rt.CreateSolidColorBrush(&color(0.220, 0.220, 0.220, 1.0), None)?,
            action_border: rt.CreateSolidColorBrush(&color(0.270, 0.270, 0.270, 1.0), None)?,
            action_text: rt.CreateSolidColorBrush(&color(0.82, 0.82, 0.82, 1.0), None)?,
            action_danger_bg: rt.CreateSolidColorBrush(&color(0.340, 0.082, 0.102, 1.0), None)?,
            action_danger_hover_bg: rt
                .CreateSolidColorBrush(&color(0.500, 0.090, 0.122, 1.0), None)?,
            action_danger_border: rt
                .CreateSolidColorBrush(&color(0.900, 0.250, 0.290, 1.0), None)?,
            action_danger_text: rt.CreateSolidColorBrush(&color(1.000, 0.850, 0.860, 1.0), None)?,
            caption_btn_hover_bg: rt
                .CreateSolidColorBrush(&color(0.235, 0.235, 0.235, 1.0), None)?, // #3C3C3C

            live_badge_bg: rt.CreateSolidColorBrush(&color(0.055, 0.240, 0.110, 1.0), None)?,
            live_badge_text: rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?,

            amber_badge_bg: rt.CreateSolidColorBrush(&color(0.320, 0.180, 0.020, 1.0), None)?,
            amber_badge_text: rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?,

            blue_badge_bg: rt.CreateSolidColorBrush(&color(0.020, 0.190, 0.300, 1.0), None)?,
            blue_badge_text: rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?,

            final_badge_bg: rt.CreateSolidColorBrush(&color(0.165, 0.165, 0.165, 1.0), None)?,
            final_badge_text: rt.CreateSolidColorBrush(&color(0.58, 0.58, 0.58, 1.0), None)?,

            close_btn_hover_bg: rt.CreateSolidColorBrush(&color(0.910, 0.067, 0.137, 1.0), None)?, // #E81123
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
    brushes: Brushes,
    fmt_app_title: Fmt,
    fmt_page_title: Fmt,
    fmt_eyebrow: Fmt,
    fmt_subtitle: Fmt,
    fmt_item_title: Fmt,
    fmt_item_sub: Fmt,
    fmt_badge: Fmt,
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
    /// Vertical scroll position in **pixels** (clamped to `max_scroll()`).
    pub scroll_offset: usize,
    /// Accumulated raw wheel delta; one notch (WHEEL_DELTA = 120) scrolls SCROLL_STEP px.
    pub wheel_accum: i32,
    /// True while a TME_LEAVE track is armed (re-armed on next mouse move after leave).
    pub mouse_tracking: bool,
    pub active_sport: DashboardSport,
    content_height: f32,
    /// (global match index, card rect, exact 44px action-button rect).
    card_layout: Vec<(usize, D2D_RECT_F, D2D_RECT_F)>,
    spinner_origin: Instant,
    // Persistent pixel scratch buffer: avoids a ~3.4MB alloc + double copy per present.
    buf: Vec<u8>,
    // Per-present grouping scratch: reused via mem::take + restore so the
    // 120ms loader tick never reallocs. Outer capacity stabilizes after the
    // first frames; inner per-league index Vecs stay bounded (league count
    // < 32, matches total < 512 — small vs the ~3.4MB frame buffer).
    // sport_filter_scratch holds global match indices for the active sport.
    sport_filter_scratch: Vec<usize>,
    live_groups_scratch: Vec<LeagueGroup>,
    upcoming_groups_scratch: Vec<LeagueGroup>,
    // STA-bound COM/GDI state must never cross threads.
    _no_send: PhantomData<*const ()>,
}

impl DashboardRenderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32) -> Result<Self> {
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

        let brushes = Brushes::create(&rt)?;

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

        // Emoji icon font: "Segoe UI Variable Display" has no 🏏/⚽ glyphs, so the
        // page-header icon gets an explicit Emoji family instead of tofu boxes.
        let fmt_icon_fmt = dwrite.CreateTextFormat(
            w!("Segoe UI Emoji"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            22.0,
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
            fmt_icon: Fmt {
                fmt: fmt_icon_fmt,
                buf: RefCell::new(Vec::new()),
            },
            fmt_empty: mk_font(
                16.0,
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
            scroll_offset: 0,
            wheel_accum: 0,
            mouse_tracking: false,
            active_sport: DashboardSport::Cricket,
            content_height: 0.0,
            card_layout: Vec::new(),
            spinner_origin: Instant::now(),
            buf: Vec::new(),
            sport_filter_scratch: Vec::new(),
            live_groups_scratch: Vec::new(),
            upcoming_groups_scratch: Vec::new(),
            _no_send: PhantomData,
        })
    }

    /// Rebuild only the WIC bitmap / render target / brushes / DIB on resize.
    /// Factories (D2D/DWrite globals, per-renderer WIC) and mem_dc are reused.
    pub unsafe fn resize(&mut self, new_w: u32, new_h: u32) -> Result<()> {
        if self.w == new_w as i32 && self.h == new_h as i32 {
            return Ok(());
        }

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
        self.w = 0;
        self.h = 0;
        self.resize(w as u32, h as u32)
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

    fn sport_switcher_rects(&self) -> (D2D_RECT_F, D2D_RECT_F, D2D_RECT_F) {
        let w = self.w as f32;
        let cy = TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT / 2.0;
        let switcher = D2D_RECT_F {
            left: w - CARD_RIGHT - SWITCHER_W,
            top: cy - SWITCHER_H / 2.0,
            right: w - CARD_RIGHT,
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
        let start = t * std::f32::consts::TAU * 0.95;
        let sweep = 1.85_f32;
        let steps = 20;
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

    pub fn hit_test(&self, x: f32, y: f32, _match_count: usize) -> Option<HitTarget> {
        let w = self.w as f32;

        if y >= 0.0 && y <= TITLE_BAR_HEIGHT {
            let (min_r, max_r, close_r) = Self::caption_rects(w);
            if x >= close_r.left && x <= close_r.right {
                return Some(HitTarget::CloseButton);
            }
            if x >= max_r.left && x < max_r.right {
                return Some(HitTarget::MaximizeButton);
            }
            if x >= min_r.left && x < min_r.right {
                return Some(HitTarget::MinimizeButton);
            }
            return Some(HitTarget::TitleBar);
        }

        let (_, cricket, football) = self.sport_switcher_rects();
        if y > TITLE_BAR_HEIGHT && y <= Self::page_content_top() {
            if x >= cricket.left && x <= cricket.right && y >= cricket.top && y <= cricket.bottom {
                return Some(HitTarget::CricketTab);
            }
            if x >= football.left
                && x <= football.right
                && y >= football.top
                && y <= football.bottom
            {
                return Some(HitTarget::FootballTab);
            }
            return None;
        }

        // Action buttons hit first on their exact 44px rects; card bodies second.
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

        None
    }

    pub unsafe fn present(
        &mut self,
        pos: &POINT,
        matches: &[DiscoveredMatch],
        selected_id: &Option<String>,
        loading: bool,
    ) -> Result<()> {
        let (w, h) = (self.w as f32, self.h as f32);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
        self.card_layout.clear();
        self.rt.BeginDraw();
        self.rt.Clear(None);

        // Window surface: a quiet neutral foundation with a single restrained accent role.
        let full_rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: full_rect,
            radiusX: 16.0,
            radiusY: 16.0,
        };
        self.rt.FillRoundedRectangle(&rr, &self.brushes.bg);
        // HC: surfaces are already opaque; bump the window border to 2px.
        let border_w = if high_contrast() { 2.0 } else { 1.2 };
        self.rt
            .DrawRoundedRectangle(&rr, &self.brushes.border, border_w, None);

        // Title bar header.
        let header_rect = D2D_RECT_F {
            left: 1.0,
            top: 1.0,
            right: w - 1.0,
            bottom: TITLE_BAR_HEIGHT,
        };
        let header_rr = D2D1_ROUNDED_RECT {
            rect: header_rect,
            radiusX: 15.0,
            radiusY: 15.0,
        };
        self.rt
            .FillRoundedRectangle(&header_rr, &self.brushes.header_bg);

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
        let icon_rect = D2D_RECT_F {
            left: CARD_LEFT,
            top: TITLE_BAR_HEIGHT + 14.0,
            right: CARD_LEFT + 52.0,
            bottom: TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT - 14.0,
        };
        self.fmt_icon
            .text(&self.rt, "🏏 ⚽", &icon_rect, &self.brushes.white);
        let page_title_rect = D2D_RECT_F {
            left: CARD_LEFT + 58.0,
            top: TITLE_BAR_HEIGHT,
            right: w - CARD_RIGHT - SWITCHER_W - 16.0,
            bottom: TITLE_BAR_HEIGHT + PAGE_HEADER_HEIGHT,
        };
        self.fmt_page_title.text(
            &self.rt,
            "SportsPulse Dashboard",
            &page_title_rect,
            &self.brushes.white,
        );

        let (switcher_rect, cricket_rect, football_rect) = self.sport_switcher_rects();
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
                    .FillRoundedRectangle(&tab_rr, &self.brushes.card_hover);
                self.fmt_badge
                    .text(&self.rt, label, &rect, &self.brushes.white);
            } else if hovered {
                let tab_rr = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                self.rt
                    .FillRoundedRectangle(&tab_rr, &self.brushes.action_bg);
                self.fmt_badge
                    .text(&self.rt, label, &rect, &self.brushes.white);
            } else {
                self.fmt_badge
                    .text(&self.rt, label, &rect, &self.brushes.dim);
            }
        }

        self.rt.DrawLine(
            D2D_POINT_2F {
                x: CARD_LEFT,
                y: Self::page_content_top(),
            },
            D2D_POINT_2F {
                x: w - CARD_RIGHT,
                y: Self::page_content_top(),
            },
            &self.brushes.divider,
            1.0,
            None,
        );

        // Reused scratch buffers (fields): no per-present Vec collect.
        // sport_filter holds global indices for the active sport; groups hold
        // borrowed league structure rebuilt each frame. Taken out via mem::take
        // so the draw loop below holds no &self borrow on them.
        let sport = self.active_sport;
        let mut sport_idx = std::mem::take(&mut self.sport_filter_scratch);
        sport_idx.clear();
        sport_idx.extend(
            matches
                .iter()
                .enumerate()
                .filter(|(_, m)| sport.matches(m.sport))
                .map(|(i, _)| i),
        );

        if loading {
            // Loader path needs no grouping: return the filter scratch
            // immediately so capacity is preserved for the next present.
            self.sport_filter_scratch = std::mem::take(&mut sport_idx);
            self.scroll_offset = 0;
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
            // Reused group buffers: clear (keeps outer capacity) and rebuild.
            // Inner per-league index Vecs are bounded (leagues < 32, total
            // matches < 512) — small vs the ~3.4MB frame buffer.
            let mut live_groups = std::mem::take(&mut self.live_groups_scratch);
            let mut upcoming_groups = std::mem::take(&mut self.upcoming_groups_scratch);
            live_groups.clear();
            upcoming_groups.clear();
            for index in &sport_idx {
                let index = *index;
                let m = &matches[index];
                if is_live_match(m) {
                    push_to_league_group(&mut live_groups, &m.league_name, index);
                } else {
                    push_to_league_group(&mut upcoming_groups, &m.league_name, index);
                }
            }
            // Filter scratch no longer needed: restore now (keeps capacity).
            self.sport_filter_scratch = sport_idx;
            live_groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            upcoming_groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

            let max_cols = if w >= 1500.0 {
                3
            } else if w >= 900.0 {
                2
            } else {
                1
            };
            let gap = 16.0;
            let content_top = Self::page_content_top();
            let content_bottom = h - CONTENT_BOTTOM_GUTTER;
            // scroll_offset is already in pixels.
            let scroll_px = self.scroll_offset as f32;
            let mut y = ITEM_TOP_START - scroll_px;
            let clip = D2D_RECT_F {
                left: 1.0,
                top: content_top + 1.0,
                right: w - 1.0,
                bottom: content_bottom,
            };
            self.rt
                .PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            for (is_live, groups) in [(true, &live_groups), (false, &upcoming_groups)] {
                let section_rect = D2D_RECT_F {
                    left: CARD_LEFT,
                    top: y,
                    right: w - CARD_RIGHT,
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
                self.fmt_eyebrow
                    .text(&self.rt, section_label, &section_rect, section_brush);
                y += 40.0;

                if groups.is_empty() {
                    let empty_rect = D2D_RECT_F {
                        left: CARD_LEFT,
                        top: y,
                        right: w - CARD_RIGHT,
                        bottom: y + EMPTY_CARD_H,
                    };
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
                    self.fmt_empty
                        .text(&self.rt, empty_copy, &empty_rect, &self.brushes.subtle);
                    y += EMPTY_CARD_H + 28.0;
                    continue;
                }

                for group in groups {
                    // The league marker is intentionally separate from its label. This avoids
                    // the bar colliding with the first letter at every window size.
                    let league_marker = D2D_RECT_F {
                        left: CARD_LEFT,
                        top: y + 4.0,
                        right: CARD_LEFT + 3.0,
                        bottom: y + 26.0,
                    };
                    let league_rect = D2D_RECT_F {
                        left: CARD_LEFT + 16.0,
                        top: y,
                        right: w - CARD_RIGHT,
                        bottom: y + 30.0,
                    };
                    let league_brush = if is_live {
                        &self.brushes.live_badge_text
                    } else {
                        &self.brushes.blue_badge_text
                    };
                    self.rt.FillRectangle(&league_marker, league_brush);
                    self.fmt_eyebrow.text(
                        &self.rt,
                        &ui_text(&group.name.to_uppercase(), 48),
                        &league_rect,
                        &self.brushes.dim,
                    );
                    y += 40.0;

                    let group_cols = max_cols.min(group.match_indices.len().max(1));
                    let card_height = if is_live { 74.0 } else { ITEM_HEIGHT };
                    let card_width = (w - CARD_LEFT - CARD_RIGHT - gap * (group_cols as f32 - 1.0))
                        / group_cols as f32;
                    for (position, index) in group.match_indices.iter().copied().enumerate() {
                        let row = position / group_cols;
                        let column = position % group_cols;
                        let top = y + row as f32 * (card_height + ITEM_SPACING);
                        let left = CARD_LEFT + column as f32 * (card_width + gap);
                        let item_rect = D2D_RECT_F {
                            left,
                            top,
                            right: left + card_width,
                            bottom: top + card_height,
                        };
                        // Exact 44px-tall action button rect (≥44px touch target),
                        // stored for hit-testing.
                        let action_rect = D2D_RECT_F {
                            left: item_rect.right - ACTION_W - 16.0,
                            top: top + (card_height - 44.0) / 2.0,
                            right: item_rect.right - 16.0,
                            bottom: top + (card_height + 44.0) / 2.0,
                        };
                        self.card_layout.push((index, item_rect, action_rect));
                        let item_rr = D2D1_ROUNDED_RECT {
                            rect: item_rect,
                            radiusX: 10.0,
                            radiusY: 10.0,
                        };
                        let m = &matches[index];
                        let is_selected =
                            selected_id.as_ref().map_or(false, |id| id == &m.match_id);
                        let is_hovered = self.hover_index == Some(index);

                        if is_selected {
                            self.rt
                                .FillRoundedRectangle(&item_rr, &self.brushes.card_active);
                            self.rt.DrawRoundedRectangle(
                                &item_rr,
                                &self.brushes.active_border,
                                1.6,
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
                            radiusX: 7.0,
                            radiusY: 7.0,
                        };
                        let action_hovered = self.action_hover_index == Some(index);
                        let (action_bg, action_border, action_text, action_label) = if is_selected {
                            (
                                if action_hovered {
                                    &self.brushes.action_danger_hover_bg
                                } else {
                                    &self.brushes.action_danger_bg
                                },
                                &self.brushes.action_danger_border,
                                &self.brushes.action_danger_text,
                                "UNTRACK",
                            )
                        } else {
                            (
                                if action_hovered {
                                    &self.brushes.action_hover_bg
                                } else {
                                    &self.brushes.action_bg
                                },
                                &self.brushes.action_border,
                                &self.brushes.action_text,
                                "TRACK",
                            )
                        };
                        self.rt.FillRoundedRectangle(&action_rr, action_bg);
                        self.rt
                            .DrawRoundedRectangle(&action_rr, action_border, 1.0, None);
                        self.fmt_badge
                            .text(&self.rt, action_label, &action_rect, action_text);

                        let text_right = action_rect.left - 20.0;
                        if is_live {
                            let title_rect = D2D_RECT_F {
                                left: left + 24.0,
                                top: top + 18.0,
                                right: text_right,
                                bottom: top + 54.0,
                            };
                            self.fmt_item_title.text(
                                &self.rt,
                                &ui_text(&m.title, 64),
                                &title_rect,
                                &self.brushes.white,
                            );
                        } else {
                            let title_rect = D2D_RECT_F {
                                left: left + 24.0,
                                top: top + 14.0,
                                right: text_right,
                                bottom: top + 48.0,
                            };
                            let time_rect = D2D_RECT_F {
                                left: left + 24.0,
                                top: top + 52.0,
                                right: text_right,
                                bottom: top + 78.0,
                            };
                            self.fmt_item_title.text(
                                &self.rt,
                                &ui_text(&m.title, 64),
                                &title_rect,
                                &self.brushes.white,
                            );
                            self.fmt_item_sub.text(
                                &self.rt,
                                &ui_text(&format_upcoming_time(&m.start_time), 40),
                                &time_rect,
                                &self.brushes.dim,
                            );
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
                let thumb_height =
                    (track_height * (viewport / self.content_height)).clamp(32.0, track_height);
                let thumb_top = track_top
                    + (track_height - thumb_height)
                        * (self.scroll_offset as f32 / self.max_scroll().max(1) as f32);
                let track = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        // 8px-wide scrollbar (was 5px): minimum operable width.
                        left: w - 18.0,
                        top: track_top,
                        right: w - 10.0,
                        bottom: track_bottom,
                    },
                    radiusX: 2.5,
                    radiusY: 2.5,
                };
                let thumb = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: w - 18.0,
                        top: thumb_top,
                        right: w - 10.0,
                        bottom: thumb_top + thumb_height,
                    },
                    radiusX: 2.5,
                    radiusY: 2.5,
                };
                self.rt
                    .FillRoundedRectangle(&track, &self.brushes.icon_box_bg);
                self.rt.FillRoundedRectangle(&thumb, &self.brushes.subtle);
            }
            // Return group buffers (capacity preserved for the next present).
            self.live_groups_scratch = std::mem::take(&mut live_groups);
            self.upcoming_groups_scratch = std::mem::take(&mut upcoming_groups);
        }

        self.rt.EndDraw(None, None)?;

        let row_pitch = (self.w * 4) as usize;
        let total_bytes = row_pitch * self.h as usize;
        // Persistent buffer: clear + resize instead of a ~3.4MB per-frame alloc.
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
        // Pixel-based: one wheel notch moves SCROLL_STEP px, clamped to max px.
        let max_offset = self.max_scroll();
        let next = self.scroll_offset as isize + notches * SCROLL_STEP as isize;
        self.scroll_offset = next.clamp(0, max_offset as isize) as usize;
    }

    /// Accumulate raw wheel delta; every full WHEEL_DELTA (120) emits one notch.
    /// High-resolution wheels deliver partial deltas that must not be dropped.
    pub fn accumulate_wheel(&mut self, delta: i32) {
        self.wheel_accum += delta;
        let step = WHEEL_DELTA as i32;
        while self.wheel_accum >= step {
            self.wheel_accum -= step;
            self.scroll_by(-1, 0);
        }
        while self.wheel_accum <= -step {
            self.wheel_accum += step;
            self.scroll_by(1, 0);
        }
    }

    /// Clear all hover state (called on WM_MOUSELEAVE).
    pub fn clear_hover(&mut self) {
        self.set_hover(None, None, false, false, false, false, false);
        self.mouse_tracking = false;
    }

    fn max_scroll(&self) -> usize {
        let viewport = (self.h as f32 - Self::page_content_top() - CONTENT_BOTTOM_GUTTER).max(1.0);
        (self.content_height - viewport).max(0.0).ceil() as usize
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
