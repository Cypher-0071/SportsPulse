//! SportsPulse — Win11 Dark Theme Direct2D/DirectWrite Layered Scoreboard Renderer.
//! Pixel flow: D2D -> WIC bitmap -> CopyPixels -> DIB -> UpdateLayeredWindow.
//! Windows 11 Fluent Geometry: SOLID opaque #202020 background, #2D2D2D surfaces, 16px corner radius, bold typography.
//! Overlay logic: cricket stacked scorecard vs football broadcast bar, status badges, CRR/RRR/Need/Target rules.

#![allow(dead_code)]

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::*;
use windows::Win32::Foundation::{BOOL, COLORREF, E_FAIL, HWND, POINT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetSysColor, GetWindowDC,
    ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, COLOR_WINDOW, COLOR_WINDOWTEXT, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, UpdateLayeredWindow, SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA,
};

use crate::engine::models::{
    MatchEvent, MatchEventType, MatchScore, MatchStatus, SportType, BATTING_TEAM1, BATTING_TEAM2,
};

pub fn dbglog(msg: &str) {
    // P0-7: debug-only logging. Release builds must not touch C:\ paths
    // from the render hot path.
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("C:\\sp_bench\\sp_debug.log")
        {
            let _ = writeln!(f, "{msg}");
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = msg;
}

#[inline]
pub fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// Global UI scale: the whole interface (windows, cards, fonts, popups)
/// renders at 70% of the design size. Applied at the DPI choke points —
/// window pixel sizes (via `scale_for_dpi` in main.rs) and the Direct2D
/// render-target DPI — so every surface shrinks uniformly without touching
/// individual layout constants. Pixel insets against screen edges stay 1:1.
pub const UI_SCALE: f32 = 0.7;

/// ESPN-fed string guard: pre-truncate with … (all formats are NO_WRAP +
/// CLIP) and wrap in U+2066..U+2069 isolates so mixed-script team/event names
/// never reorder surrounding UI text. Apply at every DrawText site fed by
/// ESPN data (pure function — no HWND needed).
pub fn ui_text(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    let truncated: String = if t.chars().count() > max_chars {
        let mut v: String = t.chars().take(max_chars.saturating_sub(1)).collect();
        v.push('…');
        v
    } else {
        t.to_owned()
    };
    format!("\u{2066}{truncated}\u{2069}")
}

/// Reduced-motion: true when the user disabled client-area animation
/// (SPI_GETCLIENTAREAANIMATION). Callers swap the spinner / flash timers for
/// a static ring + persistent card. Throttled to avoid per-frame syscalls.
pub fn reduced_motion() -> bool {
    static CACHE: AtomicU64 = AtomicU64::new(0);
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    };
    let packed = CACHE.load(Ordering::Relaxed);
    let last_time = packed >> 1;
    if now.saturating_sub(last_time) < 2 {
        return (packed & 1) != 0;
    }
    let res = unsafe {
        let mut enabled = BOOL(1);
        if SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            enabled.0 == 0
        } else {
            false
        }
    };
    CACHE.store((now << 1) | (if res { 1 } else { 0 }), Ordering::Relaxed);
    res
}

/// High-contrast: SPI_GETHIGHCONTRAST (HIGHCONTRASTF_ON), with a GetSysColor
/// black/white-inversion sniff as fallback. Callers switch to 2px borders
/// (surfaces are already opaque). Throttled to avoid per-frame syscalls.
pub fn high_contrast() -> bool {
    static CACHE: AtomicU64 = AtomicU64::new(0);
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    };
    let packed = CACHE.load(Ordering::Relaxed);
    let last_time = packed >> 1;
    if now.saturating_sub(last_time) < 2 {
        return (packed & 1) != 0;
    }
    let res = unsafe {
        #[repr(C)]
        struct RawHc {
            cb_size: u32,
            flags: u32,
            scheme: *mut u16,
        }
        const HCF_ON: u32 = 1; // HIGHCONTRASTF_ON
        let mut hc = RawHc {
            cb_size: std::mem::size_of::<RawHc>() as u32,
            flags: 0,
            scheme: std::ptr::null_mut(),
        };
        if SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cb_size,
            Some(&mut hc as *mut RawHc as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && hc.flags & HCF_ON != 0
        {
            true
        } else {
            let bg = GetSysColor(COLOR_WINDOW);
            let fg = GetSysColor(COLOR_WINDOWTEXT);
            (bg == 0x00FF_FFFF && fg == 0x0000_0000) || (bg == 0x0000_0000 && fg == 0x00FF_FFFF)
        }
    };
    CACHE.store((now << 1) | (if res { 1 } else { 0 }), Ordering::Relaxed);
    res
}

/// Shared flash TTLs (spec: WIN 8s, EVENT 5s). Single source of truth used by
/// the overlay flash timer, the popup auto-hide timer, and render expiry.
pub const WIN_TTL: Duration = Duration::from_secs(8);
pub const EVENT_TTL: Duration = Duration::from_secs(5);

/// Word-boundary match on an already-uppercased title. Avoids SHOUT/WING-style
/// substring false positives from naive `contains("OUT")` / `contains("WIN")`.
pub(crate) fn has_word(upper_title: &str, word: &str) -> bool {
    upper_title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == word)
}

/// Process-lifetime Direct2D / DirectWrite factories. Creating these per-resize
/// caused visible jank when the overlay hopped 96↔140↔200px heights.
/// NOTE: IWICImagingFactory has no Send+Sync impl in windows 0.58, so it cannot
/// live in a `static OnceLock`; each renderer keeps one WIC factory for its own
/// lifetime and reuses it across resizes instead.
static D2D_FACTORY: OnceLock<Option<ID2D1Factory>> = OnceLock::new();
static DWRITE_FACTORY: OnceLock<Option<IDWriteFactory>> = OnceLock::new();

pub(crate) fn d2d_factory() -> Result<ID2D1Factory> {
    D2D_FACTORY
        .get_or_init(|| unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok() })
        .clone()
        .ok_or_else(|| E_FAIL.into())
}

pub(crate) fn dwrite_factory() -> Result<IDWriteFactory> {
    DWRITE_FACTORY
        .get_or_init(|| unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok() })
        .clone()
        .ok_or_else(|| E_FAIL.into())
}

pub(crate) fn software_rt_props() -> D2D1_RENDER_TARGET_PROPERTIES {
    D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    }
}

/// Original `cleanTitle`: split on '•' or ',' and take the first piece.
fn clean_title(title: &str) -> String {
    if title.is_empty() {
        return String::new();
    }
    title
        .split(['•', ','])
        .next()
        .unwrap_or(title)
        .trim()
        .to_string()
}

fn display_match_title(score: &MatchScore) -> String {
    let raw = clean_title(&score.match_title);
    if score.sport == SportType::Soccer && raw == "Soccer Match" {
        "FOOTBALL".to_string()
    } else if raw.is_empty() {
        format!(
            "{} vs {}",
            score.team1.abbreviation, score.team2.abbreviation
        )
    } else {
        raw
    }
}

/// Original `cleanScoreString`: strip `, target N` / `target: N` from cricket scores.
fn clean_score_string(score_str: &str, sport: SportType) -> String {
    let raw = score_str.trim();
    if raw.is_empty() {
        return match sport {
            SportType::Soccer => "0".to_string(),
            SportType::Cricket => "Yet to bat".to_string(),
        };
    }

    let lower = raw.to_ascii_lowercase();
    let cut = if let Some(i) = find_target_suffix(&lower) {
        i
    } else {
        raw.len()
    };
    let mut cleaned = raw[..cut].trim().to_string();
    loop {
        let next = cleaned.replace("()", "").replace("( )", "");
        if next == cleaned {
            break;
        }
        cleaned = next.trim().to_string();
    }
    if cleaned.is_empty() {
        match sport {
            SportType::Soccer => "0".to_string(),
            SportType::Cricket => "Yet to bat".to_string(),
        }
    } else {
        cleaned
    }
}

/// Index of a `, target N` / `target: N` / `target N` suffix, if present.
fn find_target_suffix(lower: &str) -> Option<usize> {
    let bytes = lower.as_bytes();
    let needle = b"target";
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if bytes[i..].starts_with(needle) {
            let mut start = i;
            while start > 0 && bytes[start - 1].is_ascii_whitespace() {
                start -= 1;
            }
            if start > 0 && bytes[start - 1] == b',' {
                start -= 1;
                while start > 0 && bytes[start - 1].is_ascii_whitespace() {
                    start -= 1;
                }
            }
            let mut j = i + needle.len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b':' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
            }
            if j < bytes.len() && bytes[j].is_ascii_digit() {
                return Some(start);
            }
        }
        i += 1;
    }
    None
}

/// First 3 chars of team NAME (fallback abbreviation), uppercase.
fn football_short_name(name: &str, abbreviation: &str, fallback: &str) -> String {
    let src = if !name.trim().is_empty() {
        name.trim()
    } else if !abbreviation.trim().is_empty() {
        abbreviation.trim()
    } else {
        fallback
    };
    src.chars().take(3).collect::<String>().to_uppercase()
}

fn cricket_team_label(name: &str, abbreviation: &str, fallback: &str) -> String {
    if !abbreviation.trim().is_empty() {
        abbreviation.to_string()
    } else if !name.trim().is_empty() {
        name.to_string()
    } else {
        fallback.to_string()
    }
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_live_stale(score: &MatchScore) -> bool {
    score.status == MatchStatus::Live && unix_now_secs().saturating_sub(score.timestamp) > 15
}

fn soccer_clock(score: &MatchScore) -> Option<&str> {
    score
        .soccer_clock
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
}

fn show_cricket_stats(score: &MatchScore) -> bool {
    score.sport == SportType::Cricket
        && matches!(score.status, MatchStatus::Live | MatchStatus::Break)
}

/// Original `getCleanEventDetail`.
fn clean_event_detail(description: &str, event_type: MatchEventType) -> String {
    if description.is_empty() {
        return String::new();
    }
    if let Some(idx) = description.find(" Own Goal") {
        return description[..idx].trim().to_string();
    }
    if let Some(idx) = description.find(" Goal") {
        return description[..idx].trim().to_string();
    }
    if let Some(idx) = description.find(" Penalty") {
        return description[..idx].trim().to_string();
    }
    if let Some(idx) = description.find(" Red Card") {
        return description[..idx].trim().to_string();
    }
    if let Some(idx) = description.find(" Yellow Card") {
        return description[..idx].trim().to_string();
    }
    match event_type {
        MatchEventType::Wicket => {
            if let Some((head, _)) = description.split_once(':') {
                head.trim().to_string()
            } else {
                description.to_string()
            }
        }
        MatchEventType::Boundary => {
            if let Some((head, _)) = description.split_once(':') {
                head.trim().to_string()
            } else if let Some((_, after_to)) = description.split_once(" to ") {
                if let Some((head, _)) = after_to.split_once(',') {
                    head.trim().to_string()
                } else if after_to.is_empty() {
                    description.to_string()
                } else {
                    after_to.trim().to_string()
                }
            } else {
                description.to_string()
            }
        }
        MatchEventType::Win => description.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlashKind {
    Goal,
    RedCard,
    Six,
    Four,
    Wicket,
    Win,
}

fn classify_flash(event: &MatchEvent) -> FlashKind {
    // Uppercase once; event_type decides first, title words refine.
    // (Soccer GOAL arrives as Boundary/"GOAL!", RED CARD as Wicket/"RED CARD!".)
    let upper = event.title.to_uppercase();
    match event.event_type {
        MatchEventType::Boundary => {
            if has_word(&upper, "GOAL") {
                FlashKind::Goal
            } else if has_word(&upper, "SIX") {
                FlashKind::Six
            } else {
                FlashKind::Four
            }
        }
        MatchEventType::Wicket => {
            if has_word(&upper, "RED") && has_word(&upper, "CARD") {
                FlashKind::RedCard
            } else {
                FlashKind::Wicket
            }
        }
        MatchEventType::Win => FlashKind::Win,
    }
}

fn flash_ttl(event: &MatchEvent) -> Duration {
    if event.event_type == MatchEventType::Win {
        WIN_TTL
    } else {
        EVENT_TTL
    }
}

struct FlashState {
    event: MatchEvent,
    started: Instant,
}

pub struct Fmt {
    pub fmt: IDWriteTextFormat,
    // Persistent UTF-16 scratch buffer: avoids one Vec<u16> alloc per DrawText
    // (~10-30 draw calls per frame).
    buf: RefCell<Vec<u16>>,
}

impl Fmt {
    pub unsafe fn text(
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

pub struct Brushes {
    pub bg: ID2D1SolidColorBrush,
    pub card_surface: ID2D1SolidColorBrush,
    pub border: ID2D1SolidColorBrush,
    pub white: ID2D1SolidColorBrush,
    pub dim: ID2D1SolidColorBrush,
    pub subtle: ID2D1SolidColorBrush,
    pub green_accent: ID2D1SolidColorBrush,
    pub green_badge_bg: ID2D1SolidColorBrush,
    pub red_accent: ID2D1SolidColorBrush,
    pub red_badge_bg: ID2D1SolidColorBrush,
    pub amber_accent: ID2D1SolidColorBrush,
    pub amber_badge_bg: ID2D1SolidColorBrush,
    pub blue_accent: ID2D1SolidColorBrush,
    pub blue_badge_bg: ID2D1SolidColorBrush,
    pub purple_accent: ID2D1SolidColorBrush,
    pub purple_badge_bg: ID2D1SolidColorBrush,
    pub gold_accent: ID2D1SolidColorBrush,
}

impl Brushes {
    unsafe fn create(rt: &ID2D1RenderTarget) -> Result<Self> {
        Ok(Self {
            bg: rt.CreateSolidColorBrush(&color(0.110, 0.110, 0.110, 1.0), None)?, // #1C1C1C
            card_surface: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D
            border: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D
            white: rt.CreateSolidColorBrush(&color(0.910, 0.910, 0.925, 1.0), None)?,  // #E8E8EC
            dim: rt.CreateSolidColorBrush(&color(0.545, 0.561, 0.627, 1.0), None)?,    // #8B8FA0
            subtle: rt.CreateSolidColorBrush(&color(0.420, 0.435, 0.482, 1.0), None)?, // #6B6F7B
            green_accent: rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?, // #22C55E
            green_badge_bg: rt.CreateSolidColorBrush(&color(0.055, 0.240, 0.110, 0.4), None)?,
            red_accent: rt.CreateSolidColorBrush(&color(1.0, 0.29, 0.29, 1.0), None)?, // #FF4A4A
            red_badge_bg: rt.CreateSolidColorBrush(&color(1.0, 0.29, 0.29, 0.12), None)?,
            amber_accent: rt.CreateSolidColorBrush(&color(0.941, 0.706, 0.161, 1.0), None)?, // #F0B429
            amber_badge_bg: rt.CreateSolidColorBrush(&color(0.941, 0.706, 0.161, 0.15), None)?,
            blue_accent: rt.CreateSolidColorBrush(&color(0.353, 0.608, 0.835, 1.0), None)?, // #5A9BD5
            blue_badge_bg: rt.CreateSolidColorBrush(&color(0.353, 0.608, 0.835, 0.15), None)?,
            purple_accent: rt.CreateSolidColorBrush(&color(0.82, 0.60, 1.0, 1.0), None)?,
            purple_badge_bg: rt.CreateSolidColorBrush(&color(0.22, 0.07, 0.36, 1.0), None)?,
            gold_accent: rt.CreateSolidColorBrush(&color(0.941, 0.706, 0.161, 1.0), None)?, // #F0B429
        })
    }
}

pub struct Formats {
    pub title: Fmt,
    pub team_name: Fmt,
    pub team_name_right: Fmt,
    pub score_large: Fmt,
    pub score_medium: Fmt,
    pub score_center: Fmt,
    pub overs: Fmt,
    pub overs_right: Fmt,
    pub badge: Fmt,
    pub info: Fmt,
    pub center_dim: Fmt,
    pub no_match_title: Fmt,
    pub no_match_sub: Fmt,
    pub no_match_icon: Fmt,
    pub flash: Fmt,
}

impl Formats {
    unsafe fn create(dwrite: &IDWriteFactory) -> Result<Self> {
        let mk =
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
            title: mk(
                10.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            team_name: mk(13.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            team_name_right: mk(
                12.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            score_large: mk(
                13.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            score_medium: mk(
                12.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            score_center: mk(20.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            overs: mk(
                10.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            overs_right: mk(
                10.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            badge: mk(9.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            info: mk(
                10.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            center_dim: mk(
                10.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            no_match_title: mk(
                11.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            no_match_sub: mk(
                10.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            no_match_icon: Fmt {
                fmt: fmt_icon_fmt,
                buf: RefCell::new(Vec::new()),
            },
            flash: mk(
                10.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
        })
    }
}

pub struct Renderer {
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
    pub dpi: u32,
    brushes: Brushes,
    formats: Formats,
    event_flash: Option<FlashState>,
    // Persistent pixel scratch buffer: avoids a ~432KB alloc + double copy per present.
    buf: Vec<u8>,
    // Reused info-line parts (CRR/RRR/need/target, max 4 short Strings):
    // avoids a per-present Vec alloc on the live-tick path. Joined string is
    // still allocated per frame but bounded (< 200 chars, tiny vs the frame).
    info_parts_scratch: Vec<String>,
    // STA-bound COM/GDI state (HWND/HDC/raw bits) must never cross threads.
    _no_send: PhantomData<*const ()>,
}

impl Renderer {
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
        let formats = Formats::create(&dwrite)?;

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
            dpi: 96,
            brushes,
            formats,
            event_flash: None,
            buf: Vec::new(),
            info_parts_scratch: Vec::new(),
            _no_send: PhantomData,
        })
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        let d = (if dpi == 0 { 96.0 } else { dpi as f32 }) * UI_SCALE;
        unsafe {
            self.rt.SetDpi(d, d);
        }
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
        let d = (if self.dpi == 0 { 96.0 } else { self.dpi as f32 }) * UI_SCALE;
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

    pub fn set_event_flash(&mut self, event: Option<MatchEvent>) {
        self.event_flash = event.map(|event| FlashState {
            event,
            started: Instant::now(),
        });
    }

    /// Current render-target size (for resize-then-present debug asserts).
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    /// Present dynamic `Option<MatchScore>` with cricket/soccer layout and Win11 theme.
    pub unsafe fn present(&mut self, pos: &POINT, score: &Option<MatchScore>) -> Result<()> {
        if let Some(flash) = self.event_flash.as_ref() {
            if flash.started.elapsed() >= flash_ttl(&flash.event) {
                self.event_flash = None;
            }
        }

        let size = self.rt.GetSize();
        let (w, h) = (size.width, size.height);
        let full = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };

        self.rt.BeginDraw();
        self.rt.Clear(None);

        let card_radius = if let Some(score) = score {
            if score.sport == SportType::Soccer && score.status != MatchStatus::NoMatch {
                8.0
            } else {
                10.0
            }
        } else {
            10.0
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: full,
            radiusX: card_radius,
            radiusY: card_radius,
        };
        self.rt.FillRoundedRectangle(&rr, &self.brushes.bg);
        let border_w = if high_contrast() { 2.0 } else { 1.0 };
        self.rt
            .DrawRoundedRectangle(&rr, &self.brushes.border, border_w, None);

        let scoreboard_visible = matches!(
            score,
            Some(s) if s.status != MatchStatus::NoMatch
        );

        match score {
            Some(score) if score.status != MatchStatus::NoMatch => match score.sport {
                SportType::Cricket => self.render_cricket(w, h, score),
                SportType::Soccer => self.render_soccer(w, h, score),
            },
            _ => {
                self.render_no_match(w, h);
            }
        }

        if scoreboard_visible {
            if let Some(flash) = self.event_flash.as_ref() {
                let event = flash.event.clone();
                self.render_event_flash(w, h, &event);
            }
        }

        if let Err(e) = self.rt.EndDraw(None, None) {
            dbglog(&format!("EndDraw FAILED: {e}"));
            return Err(e);
        }

        self.flush_to_layered_window(pos)
    }

    unsafe fn render_cricket(&mut self, w: f32, h: f32, score: &MatchScore) {
        let show_stats = show_cricket_stats(score);
        let title = display_match_title(score).to_uppercase();

        // 1. Header (y: 10.0 to 24.0)
        let badge_left = self.render_status_badge(w, 10.0, score);
        let title_rect = D2D_RECT_F {
            left: 14.0,
            top: 10.0,
            right: badge_left - 8.0,
            bottom: 24.0,
        };
        self.formats.title.text(
            &self.rt,
            &ui_text(&title, 48),
            &title_rect,
            &self.brushes.subtle,
        );

        // 2. Stacked Team Rows
        // Row 1: Team 1 (y: 28.0 to 48.0)
        let t1_batting = score.batting_team == BATTING_TEAM1 || score.team1.is_batting;
        let t1_name_raw = cricket_team_label(&score.team1.name, &score.team1.abbreviation, "T1");
        let t1_score_str = clean_score_string(&score.team1.score, SportType::Cricket);

        let t1_score_rect = D2D_RECT_F {
            left: w * 0.45,
            top: 28.0,
            right: w - 14.0,
            bottom: 48.0,
        };
        self.formats.score_large.text(
            &self.rt,
            &ui_text(&t1_score_str, 24),
            &t1_score_rect,
            &self.brushes.white,
        );

        let t1_name_rect = D2D_RECT_F {
            left: 14.0,
            top: 28.0,
            right: t1_score_rect.left - 8.0,
            bottom: 48.0,
        };
        self.formats.team_name.text(
            &self.rt,
            &ui_text(&t1_name_raw, 24),
            &t1_name_rect,
            &self.brushes.white,
        );
        if t1_batting {
            let dot_x = 14.0
                + (t1_name_raw.chars().count() as f32 * 8.2).min(t1_name_rect.right - 20.0)
                + 8.0;
            let dot = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: dot_x, y: 38.0 },
                radiusX: 3.0,
                radiusY: 3.0,
            };
            self.rt.FillEllipse(&dot, &self.brushes.gold_accent);
        }

        // Row 2: Team 2 (y: 50.0 to 70.0)
        let t2_batting = score.batting_team == BATTING_TEAM2 || score.team2.is_batting;
        let t2_name_raw = cricket_team_label(&score.team2.name, &score.team2.abbreviation, "T2");
        let t2_score_str = clean_score_string(&score.team2.score, SportType::Cricket);

        let t2_score_rect = D2D_RECT_F {
            left: w * 0.45,
            top: 50.0,
            right: w - 14.0,
            bottom: 70.0,
        };
        self.formats.score_large.text(
            &self.rt,
            &ui_text(&t2_score_str, 24),
            &t2_score_rect,
            &self.brushes.white,
        );

        let t2_name_rect = D2D_RECT_F {
            left: 14.0,
            top: 50.0,
            right: t2_score_rect.left - 8.0,
            bottom: 70.0,
        };
        self.formats.team_name.text(
            &self.rt,
            &ui_text(&t2_name_raw, 24),
            &t2_name_rect,
            &self.brushes.white,
        );
        if t2_batting {
            let dot_x = 14.0
                + (t2_name_raw.chars().count() as f32 * 8.2).min(t2_name_rect.right - 20.0)
                + 8.0;
            let dot = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: dot_x, y: 60.0 },
                radiusX: 3.0,
                radiusY: 3.0,
            };
            self.rt.FillEllipse(&dot, &self.brushes.gold_accent);
        }

        // 3. Stats Bar (y: 74.0 to 104.0)
        if show_stats && h >= 95.0 {
            // Divider line
            self.rt.DrawLine(
                D2D_POINT_2F { x: 14.0, y: 74.0 },
                D2D_POINT_2F {
                    x: w - 14.0,
                    y: 74.0,
                },
                &self.brushes.border,
                1.0,
                None,
            );

            // Left stats: CRR / RRR / Need
            self.info_parts_scratch.clear();
            self.info_parts_scratch
                .push(format!("CRR {:.2}", score.crr));
            if let Some(rrr) = score.rrr {
                self.info_parts_scratch.push(format!("RRR {:.2}", rrr));
            }
            if let Some(needed) = score.runs_needed {
                self.info_parts_scratch.push(format!("Need {needed}"));
            }
            let left_text = self.info_parts_scratch.join(" · ");

            let target_w = if score.target.is_some() { 64.0 } else { 0.0 };
            let left_rect = D2D_RECT_F {
                left: 14.0,
                top: 76.0,
                right: w - 14.0 - target_w,
                bottom: h - 4.0,
            };
            self.formats.info.text(
                &self.rt,
                &ui_text(&left_text, 48),
                &left_rect,
                &self.brushes.dim,
            );

            // Right stat: TGT
            if let Some(tgt) = score.target {
                let tgt_str = format!("TGT {tgt}");
                let tgt_rect = D2D_RECT_F {
                    left: w - 14.0 - target_w,
                    top: 76.0,
                    right: w - 14.0,
                    bottom: h - 4.0,
                };
                self.formats
                    .overs_right
                    .text(&self.rt, &tgt_str, &tgt_rect, &self.brushes.dim);
            }
        }
    }

    unsafe fn render_soccer(&self, w: f32, h: f32, score: &MatchScore) {
        // Horizontal Broadcast TV Bar (340 x 40)
        let cx = w / 2.0;
        let cy = h / 2.0;

        // 1. Center Clock Pill
        let clock_str = soccer_clock(score).unwrap_or("-");
        let pill_w = 44.0;
        let pill_h = 20.0;
        let pill_rect = D2D_RECT_F {
            left: cx - pill_w / 2.0,
            top: cy - pill_h / 2.0,
            right: cx + pill_w / 2.0,
            bottom: cy + pill_h / 2.0,
        };
        let pill_rr = D2D1_ROUNDED_RECT {
            rect: pill_rect,
            radiusX: 4.0,
            radiusY: 4.0,
        };
        self.rt
            .FillRoundedRectangle(&pill_rr, &self.brushes.red_badge_bg);
        self.formats
            .badge
            .text(&self.rt, clock_str, &pill_rect, &self.brushes.red_accent);

        // 2. Scores
        let t1_score = clean_score_string(&score.team1.score, SportType::Soccer);
        let t2_score = clean_score_string(&score.team2.score, SportType::Soccer);

        let s1_rect = D2D_RECT_F {
            left: cx - pill_w / 2.0 - 34.0,
            top: 0.0,
            right: cx - pill_w / 2.0 - 4.0,
            bottom: h,
        };
        self.formats
            .score_center
            .text(&self.rt, &t1_score, &s1_rect, &self.brushes.white);

        let s2_rect = D2D_RECT_F {
            left: cx + pill_w / 2.0 + 4.0,
            top: 0.0,
            right: cx + pill_w / 2.0 + 34.0,
            bottom: h,
        };
        self.formats
            .score_center
            .text(&self.rt, &t2_score, &s2_rect, &self.brushes.white);

        // 3. Team Names
        let t1_name = football_short_name(&score.team1.name, &score.team1.abbreviation, "T1");
        let t2_name = football_short_name(&score.team2.name, &score.team2.abbreviation, "T2");

        let t1_rect = D2D_RECT_F {
            left: 14.0,
            top: 0.0,
            right: s1_rect.left - 6.0,
            bottom: h,
        };
        self.formats.team_name_right.text(
            &self.rt,
            &ui_text(&t1_name, 12),
            &t1_rect,
            &self.brushes.dim,
        );

        let t2_rect = D2D_RECT_F {
            left: s2_rect.right + 6.0,
            top: 0.0,
            right: w - 14.0,
            bottom: h,
        };
        self.formats.team_name.text(
            &self.rt,
            &ui_text(&t2_name, 12),
            &t2_rect,
            &self.brushes.dim,
        );
    }

    unsafe fn render_no_match(&self, w: f32, h: f32) {
        // Centered emoji 22px + "No match tracked" 11px
        let icon_h = 26.0;
        let text_h = 16.0;
        let gap = 6.0;
        let total_h = icon_h + gap + text_h;
        let start_y = (h - total_h) / 2.0;

        let icon_rect = D2D_RECT_F {
            left: 0.0,
            top: start_y,
            right: w,
            bottom: start_y + icon_h,
        };
        self.formats
            .no_match_icon
            .text(&self.rt, "🏏 ⚽", &icon_rect, &self.brushes.white);

        let text_rect = D2D_RECT_F {
            left: 0.0,
            top: start_y + icon_h + gap,
            right: w,
            bottom: start_y + total_h,
        };
        self.formats.no_match_title.text(
            &self.rt,
            "No match tracked",
            &text_rect,
            &self.brushes.dim,
        );
    }

    /// Status drives color; clock only appends for Live. Never treat a custom label as live-green.
    /// Returns the left edge of the badge so the title can avoid overlapping it.
    unsafe fn render_status_badge(&self, w: f32, top: f32, score: &MatchScore) -> f32 {
        let (text, bg_brush, fg_brush) = if is_live_stale(score) {
            (
                "RECONNECTING",
                &self.brushes.amber_badge_bg,
                &self.brushes.amber_accent,
            )
        } else {
            match score.status {
                MatchStatus::Live => ("LIVE", &self.brushes.red_badge_bg, &self.brushes.red_accent),
                MatchStatus::Break => (
                    "BREAK",
                    &self.brushes.amber_badge_bg,
                    &self.brushes.amber_accent,
                ),
                MatchStatus::Scheduled => (
                    "UPCOMING",
                    &self.brushes.blue_badge_bg,
                    &self.brushes.blue_accent,
                ),
                MatchStatus::Completed => {
                    ("FINISHED", &self.brushes.card_surface, &self.brushes.dim)
                }
                MatchStatus::NoMatch => {
                    ("OFFLINE", &self.brushes.card_surface, &self.brushes.subtle)
                }
            }
        };

        let bw = (text.chars().count() as f32 * 6.5 + 14.0).clamp(38.0, 72.0);
        let badge_rect = D2D_RECT_F {
            left: w - 14.0 - bw,
            top,
            right: w - 14.0,
            bottom: top + 16.0,
        };
        let badge_rr = D2D1_ROUNDED_RECT {
            rect: badge_rect,
            radiusX: 4.0,
            radiusY: 4.0,
        };
        self.rt.FillRoundedRectangle(&badge_rr, bg_brush);
        self.formats
            .badge
            .text(&self.rt, text, &badge_rect, fg_brush);
        badge_rect.left
    }

    unsafe fn render_event_flash(&self, w: f32, h: f32, event: &MatchEvent) {
        let kind = classify_flash(event);
        let (bg, fg) = match kind {
            FlashKind::Goal => (&self.brushes.amber_badge_bg, &self.brushes.amber_accent),
            FlashKind::Four => (&self.brushes.blue_badge_bg, &self.brushes.blue_accent),
            FlashKind::Six => (&self.brushes.green_badge_bg, &self.brushes.green_accent),
            FlashKind::Wicket | FlashKind::RedCard => {
                (&self.brushes.red_badge_bg, &self.brushes.red_accent)
            }
            FlashKind::Win => (&self.brushes.purple_badge_bg, &self.brushes.purple_accent),
        };

        let border = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        let flash_radius = if h <= 45.0 { 8.0 } else { 10.0 };
        let border_rr = D2D1_ROUNDED_RECT {
            rect: border,
            radiusX: flash_radius,
            radiusY: flash_radius,
        };
        self.rt.DrawRoundedRectangle(&border_rr, fg, 2.0, None);

        let banner_h = if h <= 110.0 { 26.0 } else { 32.0 };
        let banner = D2D_RECT_F {
            left: 12.0,
            top: h - banner_h - 10.0,
            right: w - 12.0,
            bottom: h - 10.0,
        };
        let banner_rr = D2D1_ROUNDED_RECT {
            rect: banner,
            radiusX: 8.0,
            radiusY: 8.0,
        };
        self.rt.FillRoundedRectangle(&banner_rr, bg);

        let detail = clean_event_detail(&event.description, event.event_type);
        // Icon twin for the color flash: glyphs picked from Segoe UI coverage
        // (the overlay has no emoji font; popup badges carry the emoji icons).
        let glyph = match kind {
            FlashKind::Goal => "●",
            FlashKind::Four => "▲",
            FlashKind::Six => "◆",
            FlashKind::Wicket => "✕",
            FlashKind::RedCard => "■",
            FlashKind::Win => "★",
        };
        let line = if detail.is_empty() {
            format!("{glyph} {}", event.title)
        } else {
            format!("{glyph} {}  {}", event.title, detail)
        };
        let text_rect = D2D_RECT_F {
            left: banner.left + 12.0,
            top: banner.top,
            right: banner.right - 12.0,
            bottom: banner.bottom,
        };
        self.formats
            .flash
            .text(&self.rt, &ui_text(&line, 80), &text_rect, fg);
    }

    unsafe fn flush_to_layered_window(&mut self, pos: &POINT) -> Result<()> {
        let row_pitch = self.w as usize * 4;
        let total_bytes = row_pitch * self.h as usize;
        debug_assert_eq!(row_pitch, self.w as usize * 4);
        let dib_slice = std::slice::from_raw_parts_mut(self.bits as *mut u8, total_bytes);
        self.wic
            .CopyPixels(std::ptr::null(), row_pitch as u32, dib_slice)?;

        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let ulw = UpdateLayeredWindow(
            self.hwnd,
            None,
            Some(pos),
            Some(&size),
            self.mem_dc,
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        if let Err(e) = ulw {
            dbglog(&format!("UpdateLayeredWindow error: {:?}", e));
            return Err(e);
        }

        Ok(())
    }
}

impl Drop for Renderer {
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
