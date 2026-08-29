//! SportsPulse — Win11 Dark Theme Direct2D/DirectWrite Layered Scoreboard Renderer.
//! Pixel flow: D2D -> WIC bitmap -> CopyPixels -> DIB -> UpdateLayeredWindow.
//! Windows 11 Fluent Geometry: SOLID opaque #202020 background, #2D2D2D surfaces, 16px corner radius, bold typography.
//! Overlay *logic* matches the original Tauri scoreboard in `src/main.js`.

#![allow(dead_code)]

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
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
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetWindowDC, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use crate::engine::models::{MatchEvent, MatchEventType, MatchScore, MatchStatus, SportType};

pub fn dbglog(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\sp_bench\\sp_debug.log")
    {
        let _ = writeln!(f, "{msg}");
    }
}

pub struct SampleScore {
    pub title: &'static str,
    pub t1_abbr: &'static str,
    pub t1_score: &'static str,
    pub t2_abbr: &'static str,
    pub t2_score: &'static str,
    pub status: &'static str,
    pub info: &'static str,
}

pub const SAMPLE: SampleScore = SampleScore {
    title: "IND vs AUS · 3rd T20I",
    t1_abbr: "IND",
    t1_score: "245/3",
    t2_abbr: "AUS",
    t2_score: "198/9",
    status: "LIVE",
    info: "42.3 ov · CRR 5.83",
};

#[inline]
pub fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// Original `cleanTitle`: split on '•' or ',' and take the first piece.
fn clean_title(title: &str) -> String {
    if title.is_empty() {
        return String::new();
    }
    title
        .split(|c| c == '•' || c == ',')
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
    if event.title.contains("GOAL") {
        FlashKind::Goal
    } else if event.title == "RED CARD!" {
        FlashKind::RedCard
    } else {
        match event.event_type {
            MatchEventType::Boundary => {
                if event.title.contains("SIX") {
                    FlashKind::Six
                } else {
                    FlashKind::Four
                }
            }
            MatchEventType::Wicket => FlashKind::Wicket,
            MatchEventType::Win => FlashKind::Win,
        }
    }
}

fn flash_ttl(event: &MatchEvent) -> Duration {
    if event.event_type == MatchEventType::Win {
        Duration::from_secs(8)
    } else {
        Duration::from_secs(3)
    }
}

struct FlashState {
    event: MatchEvent,
    started: Instant,
}

pub struct Fmt {
    pub fmt: IDWriteTextFormat,
}

impl Fmt {
    pub unsafe fn text(
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
}

impl Brushes {
    unsafe fn create(rt: &ID2D1RenderTarget) -> Result<Self> {
        Ok(Self {
            bg: rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?, // #202020 SOLID
            card_surface: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?, // #2D2D2D SOLID
            border: rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?, // #3E3E3E SOLID
            white: rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 1.0), None)?,        // #FFFFFF
            dim: rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?,       // #A6A6A6
            subtle: rt.CreateSolidColorBrush(&color(0.44, 0.44, 0.44, 1.0), None)?,    // #707070
            green_accent: rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?, // #22C55E
            green_badge_bg: rt.CreateSolidColorBrush(&color(0.055, 0.240, 0.110, 1.0), None)?, // #0E3B1C SOLID
            red_accent: rt.CreateSolidColorBrush(&color(0.973, 0.294, 0.333, 1.0), None)?, // #F84B55
            red_badge_bg: rt.CreateSolidColorBrush(&color(0.320, 0.098, 0.098, 1.0), None)?, // #521919 SOLID
            amber_accent: rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?, // #F59E0B
            amber_badge_bg: rt.CreateSolidColorBrush(&color(0.320, 0.180, 0.020, 1.0), None)?, // #522E05 SOLID
            blue_accent: rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?, // #38BDF8
            blue_badge_bg: rt.CreateSolidColorBrush(&color(0.020, 0.190, 0.300, 1.0), None)?, // #05304D SOLID
            purple_accent: rt.CreateSolidColorBrush(&color(0.750, 0.450, 0.980, 1.0), None)?,
            purple_badge_bg: rt.CreateSolidColorBrush(&color(0.280, 0.090, 0.420, 1.0), None)?,
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
                Ok(Fmt { fmt })
            };

        Ok(Self {
            title: mk(
                15.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            team_name: mk(24.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            team_name_right: mk(
                24.0,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            score_large: mk(38.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            score_medium: mk(28.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            score_center: mk(38.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            overs: mk(
                17.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            overs_right: mk(
                17.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            badge: mk(14.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            info: mk(
                15.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            center_dim: mk(
                17.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            no_match_title: mk(26.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            no_match_sub: mk(
                16.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            flash: mk(
                13.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
        })
    }
}

pub struct Renderer {
    hwnd: HWND,
    wic: IWICBitmap,
    rt: ID2D1RenderTarget,
    mem_dc: HDC,
    hbmp: HBITMAP,
    bits: *mut core::ffi::c_void,
    w: i32,
    h: i32,
    brushes: Brushes,
    formats: Formats,
    event_flash: Option<FlashState>,
}

impl Renderer {
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
        let formats = Formats::create(&dwrite)?;

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
            formats,
            event_flash: None,
        })
    }

    /// Rebuild the WIC bitmap / DIB / render target the same way DashboardRenderer::resize does.
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

    pub fn set_event_flash(&mut self, event: Option<MatchEvent>) {
        self.event_flash = event.map(|event| FlashState {
            event,
            started: Instant::now(),
        });
    }

    /// Present dynamic `Option<MatchScore>` with cricket/soccer layout and Win11 theme.
    pub unsafe fn present(&mut self, pos: &POINT, score: &Option<MatchScore>) -> Result<()> {
        if let Some(flash) = self.event_flash.as_ref() {
            if flash.started.elapsed() >= flash_ttl(&flash.event) {
                self.event_flash = None;
            }
        }

        let (w, h) = (self.w as f32, self.h as f32);
        let full = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };

        self.rt.BeginDraw();
        self.rt.Clear(None);

        // Win11 Top-Level Window (16px rounded corners)
        let rr = D2D1_ROUNDED_RECT {
            rect: full,
            radiusX: 16.0,
            radiusY: 16.0,
        };
        self.rt.FillRoundedRectangle(&rr, &self.brushes.bg);
        self.rt
            .DrawRoundedRectangle(&rr, &self.brushes.border, 1.2, None);

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

    unsafe fn render_cricket(&self, w: f32, h: f32, score: &MatchScore) {
        let show_stats = show_cricket_stats(score);
        let header_top = if h <= 150.0 { 10.0 } else { 14.0 };
        let header_bottom = header_top + 26.0;

        let title = display_match_title(score);
        let badge_left = self.render_status_badge(w, header_top, score);
        let title_rect = D2D_RECT_F {
            left: 26.0,
            top: header_top,
            right: badge_left - 10.0,
            bottom: header_bottom,
        };
        self.formats
            .title
            .text(&self.rt, &title, &title_rect, &self.brushes.dim);

        let stats_top = if show_stats { h - 48.0 } else { h + 8.0 };
        let body_bottom = if show_stats {
            stats_top - 4.0
        } else {
            h - 10.0
        };
        let name_top = header_bottom + 4.0;
        let name_bottom = name_top + 28.0;
        let remaining = (body_bottom - name_bottom).max(36.0);
        let score_h = if remaining > 70.0 {
            44.0
        } else {
            remaining.min(44.0).max(32.0)
        };
        let score_top = name_bottom;
        let score_bottom = score_top + score_h;
        let overs_top = score_bottom;
        let overs_bottom = (overs_top + 22.0).min(body_bottom);
        let half = w / 2.0;

        let t1_batting = score.batting_team == 1 || score.team1.is_batting;
        let t1_name = cricket_team_label(&score.team1.name, &score.team1.abbreviation, "T1");
        let t1_name_rect = D2D_RECT_F {
            left: 44.0,
            top: name_top,
            right: half - 16.0,
            bottom: name_bottom,
        };
        if t1_batting {
            let cy = (name_top + name_bottom) / 2.0;
            let dot = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: 30.0, y: cy },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            self.rt.FillEllipse(&dot, &self.brushes.green_accent);
        }
        self.formats.team_name.text(
            &self.rt,
            &t1_name,
            &t1_name_rect,
            if t1_batting {
                &self.brushes.white
            } else {
                &self.brushes.dim
            },
        );

        let t1_score_str = clean_score_string(&score.team1.score, SportType::Cricket);
        self.formats.score_large.text(
            &self.rt,
            &t1_score_str,
            &D2D_RECT_F {
                left: 26.0,
                top: score_top,
                right: half - 14.0,
                bottom: score_bottom,
            },
            &self.brushes.white,
        );

        if score.team1.overs > 0.0 && overs_bottom > overs_top + 8.0 {
            let overs_str = format!("({:.1} ov)", score.team1.overs);
            self.formats.overs.text(
                &self.rt,
                &overs_str,
                &D2D_RECT_F {
                    left: 26.0,
                    top: overs_top,
                    right: half - 14.0,
                    bottom: overs_bottom,
                },
                &self.brushes.dim,
            );
        }

        self.formats.center_dim.text(
            &self.rt,
            "vs",
            &D2D_RECT_F {
                left: half - 22.0,
                top: score_top,
                right: half + 22.0,
                bottom: score_bottom,
            },
            &self.brushes.subtle,
        );

        let t2_batting = score.batting_team == 2 || score.team2.is_batting;
        let t2_name = cricket_team_label(&score.team2.name, &score.team2.abbreviation, "T2");
        let t2_name_rect = D2D_RECT_F {
            left: half + 44.0,
            top: name_top,
            right: w - 26.0,
            bottom: name_bottom,
        };
        if t2_batting {
            let cy = (name_top + name_bottom) / 2.0;
            let dot = D2D1_ELLIPSE {
                point: D2D_POINT_2F {
                    x: half + 30.0,
                    y: cy,
                },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            self.rt.FillEllipse(&dot, &self.brushes.green_accent);
        }
        self.formats.team_name.text(
            &self.rt,
            &t2_name,
            &t2_name_rect,
            if t2_batting {
                &self.brushes.white
            } else {
                &self.brushes.dim
            },
        );

        let t2_score_str = clean_score_string(&score.team2.score, SportType::Cricket);
        self.formats.score_large.text(
            &self.rt,
            &t2_score_str,
            &D2D_RECT_F {
                left: half + 26.0,
                top: score_top,
                right: w - 26.0,
                bottom: score_bottom,
            },
            &self.brushes.white,
        );

        if score.team2.overs > 0.0 && overs_bottom > overs_top + 8.0 {
            let overs_str = format!("({:.1} ov)", score.team2.overs);
            self.formats.overs.text(
                &self.rt,
                &overs_str,
                &D2D_RECT_F {
                    left: half + 26.0,
                    top: overs_top,
                    right: w - 26.0,
                    bottom: overs_bottom,
                },
                &self.brushes.dim,
            );
        }

        if show_stats {
            let info_rect = D2D_RECT_F {
                left: 20.0,
                top: h - 48.0,
                right: w - 20.0,
                bottom: h - 14.0,
            };
            let info_rr = D2D1_ROUNDED_RECT {
                rect: info_rect,
                radiusX: 8.0,
                radiusY: 8.0,
            };
            self.rt
                .FillRoundedRectangle(&info_rr, &self.brushes.card_surface);

            let mut info_parts = Vec::new();
            info_parts.push(format!("CRR: {:.2}", score.crr));
            if let Some(rrr) = score.rrr {
                info_parts.push(format!("RRR: {:.2}", rrr));
            }
            if let Some(needed) = score.runs_needed {
                let chasing_abbr = if score.batting_team == 1 {
                    if score.team1.abbreviation.is_empty() {
                        &score.team1.name
                    } else {
                        &score.team1.abbreviation
                    }
                } else if score.team2.abbreviation.is_empty() {
                    &score.team2.name
                } else {
                    &score.team2.abbreviation
                };
                info_parts.push(format!("{chasing_abbr} need {needed}"));
            }
            if let Some(target) = score.target {
                info_parts.push(format!("Target: {target}"));
            }

            let info_str = info_parts.join("   ·   ");
            self.formats
                .info
                .text(&self.rt, &info_str, &info_rect, &self.brushes.dim);
        }
    }

    unsafe fn render_soccer(&self, w: f32, h: f32, score: &MatchScore) {
        let header_top = if h <= 110.0 { 8.0 } else { 14.0 };
        let header_bottom = header_top + 26.0;

        let title = display_match_title(score);
        let badge_left = self.render_status_badge(w, header_top, score);
        let title_rect = D2D_RECT_F {
            left: 26.0,
            top: header_top,
            right: badge_left - 10.0,
            bottom: header_bottom,
        };
        self.formats
            .title
            .text(&self.rt, &title, &title_rect, &self.brushes.dim);

        let row_top = header_bottom + 2.0;
        let row_bottom = h - 8.0;
        let half = w / 2.0;

        let t1_name = football_short_name(&score.team1.name, &score.team1.abbreviation, "T1");
        let t2_name = football_short_name(&score.team2.name, &score.team2.abbreviation, "T2");

        self.formats.team_name.text(
            &self.rt,
            &t1_name,
            &D2D_RECT_F {
                left: 26.0,
                top: row_top,
                right: half - 88.0,
                bottom: row_bottom,
            },
            &self.brushes.white,
        );
        self.formats.team_name_right.text(
            &self.rt,
            &t2_name,
            &D2D_RECT_F {
                left: half + 88.0,
                top: row_top,
                right: w - 26.0,
                bottom: row_bottom,
            },
            &self.brushes.white,
        );

        let t1_score = clean_score_string(&score.team1.score, SportType::Soccer);
        let t2_score = clean_score_string(&score.team2.score, SportType::Soccer);
        let score_pair = format!("{t1_score}  —  {t2_score}");
        self.formats.score_center.text(
            &self.rt,
            &score_pair,
            &D2D_RECT_F {
                left: half - 90.0,
                top: row_top,
                right: half + 90.0,
                bottom: row_bottom,
            },
            &self.brushes.white,
        );
    }

    unsafe fn render_no_match(&self, w: f32, h: f32) {
        let compact = h <= 120.0;
        self.formats.title.text(
            &self.rt,
            "SportsPulse",
            &D2D_RECT_F {
                left: 26.0,
                top: if compact { 8.0 } else { 16.0 },
                right: w - 26.0,
                bottom: if compact { 30.0 } else { 42.0 },
            },
            &self.brushes.subtle,
        );

        let msg_top = if compact { 32.0 } else { h * 0.38 };
        let msg_bottom = if compact {
            h - 8.0
        } else {
            (h * 0.38 + 48.0).min(h - 12.0)
        };
        let title_fmt = if compact {
            &self.formats.no_match_sub
        } else {
            &self.formats.no_match_title
        };
        title_fmt.text(
            &self.rt,
            "No match tracked",
            &D2D_RECT_F {
                left: 26.0,
                top: msg_top,
                right: w - 26.0,
                bottom: msg_bottom,
            },
            &self.brushes.white,
        );
    }

    /// Status drives color; clock only appends for Live. Never treat a custom label as live-green.
    /// Returns the left edge of the badge so the title can avoid overlapping it.
    unsafe fn render_status_badge(&self, w: f32, top: f32, score: &MatchScore) -> f32 {
        let (text, bg_brush, fg_brush) = if is_live_stale(score) {
            (
                "RECONNECTING".to_string(),
                &self.brushes.amber_badge_bg,
                &self.brushes.amber_accent,
            )
        } else {
            match score.status {
                MatchStatus::Live => {
                    let text = if score.sport == SportType::Soccer {
                        if let Some(clock) = soccer_clock(score) {
                            format!("LIVE · {clock}")
                        } else {
                            "LIVE".to_string()
                        }
                    } else {
                        "LIVE".to_string()
                    };
                    if score.sport == SportType::Soccer {
                        (text, &self.brushes.red_badge_bg, &self.brushes.red_accent)
                    } else {
                        (
                            text,
                            &self.brushes.green_badge_bg,
                            &self.brushes.green_accent,
                        )
                    }
                }
                MatchStatus::Break => {
                    let text = if score.sport == SportType::Soccer {
                        "HT"
                    } else {
                        "BREAK"
                    };
                    (
                        text.to_string(),
                        &self.brushes.amber_badge_bg,
                        &self.brushes.amber_accent,
                    )
                }
                MatchStatus::Scheduled => (
                    "UPCOMING".to_string(),
                    &self.brushes.blue_badge_bg,
                    &self.brushes.blue_accent,
                ),
                MatchStatus::Completed => {
                    let text = if score.sport == SportType::Soccer
                        && soccer_clock(score).is_some_and(|c| c.eq_ignore_ascii_case("FT"))
                    {
                        "FT"
                    } else {
                        "FINISHED"
                    };
                    (
                        text.to_string(),
                        &self.brushes.green_badge_bg,
                        &self.brushes.green_accent,
                    )
                }
                MatchStatus::NoMatch => (
                    "OFFLINE".to_string(),
                    &self.brushes.card_surface,
                    &self.brushes.subtle,
                ),
            }
        };

        let char_w = 8.4;
        let bw = (text.chars().count() as f32 * char_w + 20.0).clamp(72.0, 168.0);
        let badge_rect = D2D_RECT_F {
            left: w - 16.0 - bw,
            top,
            right: w - 16.0,
            bottom: top + 26.0,
        };
        let badge_rr = D2D1_ROUNDED_RECT {
            rect: badge_rect,
            radiusX: 8.0,
            radiusY: 8.0,
        };
        self.rt.FillRoundedRectangle(&badge_rr, bg_brush);
        self.formats
            .badge
            .text(&self.rt, &text, &badge_rect, fg_brush);
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
        let border_rr = D2D1_ROUNDED_RECT {
            rect: border,
            radiusX: 16.0,
            radiusY: 16.0,
        };
        self.rt.DrawRoundedRectangle(&border_rr, fg, 2.2, None);

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
        let line = if detail.is_empty() {
            event.title.clone()
        } else {
            format!("{}  {}", event.title, detail)
        };
        let text_rect = D2D_RECT_F {
            left: banner.left + 12.0,
            top: banner.top,
            right: banner.right - 12.0,
            bottom: banner.bottom,
        };
        self.formats.flash.text(&self.rt, &line, &text_rect, fg);
    }

    unsafe fn flush_to_layered_window(&self, pos: &POINT) -> Result<()> {
        let row = (self.w * 4) as usize;
        let mut buf = vec![0u8; row * self.h as usize];
        self.wic
            .CopyPixels(std::ptr::null(), row as u32, &mut buf)?;
        std::ptr::copy_nonoverlapping(buf.as_ptr(), self.bits as *mut u8, buf.len());

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
            if !self.hbmp.is_invalid() {
                let _ = DeleteObject(self.hbmp);
            }
            if !self.mem_dc.is_invalid() {
                let _ = DeleteDC(self.mem_dc);
            }
        }
    }
}
