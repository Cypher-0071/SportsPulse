//! SportsPulse — Win11 Dark Theme Direct2D/DirectWrite Layered Scoreboard Renderer.
//! Pixel flow: D2D -> WIC bitmap -> CopyPixels -> DIB -> UpdateLayeredWindow.
//! Windows 11 Fluent Geometry: SOLID opaque #202020 background, #2D2D2D surfaces, 16px corner radius, bold typography.

#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
    D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP,
    IDWriteFactory, IDWriteTextFormat,
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

use crate::engine::models::{MatchScore, MatchStatus, SportType};

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
}

impl Renderer {
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

        // Windows 11 Dark Theme — Solid opaque (a=1.0)
        let brushes = Brushes {
            bg:           rt.CreateSolidColorBrush(&color(0.125, 0.125, 0.125, 1.0), None)?,  // #202020 SOLID
            card_surface: rt.CreateSolidColorBrush(&color(0.176, 0.176, 0.176, 1.0), None)?,  // #2D2D2D SOLID
            border:       rt.CreateSolidColorBrush(&color(0.245, 0.245, 0.245, 1.0), None)?,  // #3E3E3E SOLID
            white:        rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 1.0), None)?,        // #FFFFFF
            dim:          rt.CreateSolidColorBrush(&color(0.65, 0.65, 0.65, 1.0), None)?,     // #A6A6A6
            subtle:       rt.CreateSolidColorBrush(&color(0.44, 0.44, 0.44, 1.0), None)?,     // #707070
            green_accent:   rt.CreateSolidColorBrush(&color(0.133, 0.773, 0.369, 1.0), None)?, // #22C55E
            green_badge_bg: rt.CreateSolidColorBrush(&color(0.055, 0.240, 0.110, 1.0), None)?, // #0E3B1C SOLID
            red_accent:     rt.CreateSolidColorBrush(&color(0.973, 0.294, 0.333, 1.0), None)?, // #F84B55
            red_badge_bg:   rt.CreateSolidColorBrush(&color(0.320, 0.098, 0.098, 1.0), None)?, // #521919 SOLID
            amber_accent:   rt.CreateSolidColorBrush(&color(0.961, 0.620, 0.043, 1.0), None)?, // #F59E0B
            amber_badge_bg: rt.CreateSolidColorBrush(&color(0.320, 0.180, 0.020, 1.0), None)?, // #522E05 SOLID
            blue_accent:    rt.CreateSolidColorBrush(&color(0.220, 0.741, 0.973, 1.0), None)?, // #38BDF8
            blue_badge_bg:  rt.CreateSolidColorBrush(&color(0.020, 0.190, 0.300, 1.0), None)?, // #05304D SOLID
        };

        let mk = |size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT| -> Result<Fmt> {
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

        // Bold, prominent typography
        let formats = Formats {
            title:          mk(15.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            team_name:      mk(24.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            team_name_right:mk(24.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_TRAILING)?,
            score_large:    mk(38.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            score_medium:   mk(28.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            score_center:   mk(38.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            overs:          mk(17.0, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            overs_right:    mk(17.0, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_TRAILING)?,
            badge:          mk(14.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            info:           mk(15.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            center_dim:     mk(17.0, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            no_match_title: mk(26.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            no_match_sub:   mk(16.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
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
            formats,
        })
    }

    /// Present dynamic `Option<MatchScore>` with cricket/soccer layout and Win11 theme.
    pub unsafe fn present(&self, pos: &POINT, score: &Option<MatchScore>) -> Result<()> {
        let (w, h) = (self.w as f32, self.h as f32);
        let full = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };

        self.rt.BeginDraw();
        self.rt.Clear(None);

        // Win11 Top-Level Window (16px rounded corners)
        let rr = D2D1_ROUNDED_RECT { rect: full, radiusX: 16.0, radiusY: 16.0 };
        self.rt.FillRoundedRectangle(&rr, &self.brushes.bg);
        self.rt.DrawRoundedRectangle(&rr, &self.brushes.border, 1.2, None);

        match score {
            Some(score) => {
                match score.status {
                    MatchStatus::NoMatch => {
                        self.render_no_match(w, h);
                    }
                    _ => {
                        match score.sport {
                            SportType::Cricket => self.render_cricket(w, h, score),
                            SportType::Soccer => self.render_soccer(w, h, score),
                        }
                    }
                }
            }
            None => {
                self.render_no_match(w, h);
            }
        }

        if let Err(e) = self.rt.EndDraw(None, None) {
            dbglog(&format!("EndDraw FAILED: {e}"));
            return Err(e);
        }

        self.flush_to_layered_window(pos)
    }

    unsafe fn render_cricket(&self, w: f32, h: f32, score: &MatchScore) {
        // 1. Top bar: Title + Status Badge
        let title_rect = D2D_RECT_F { left: 26.0, top: 14.0, right: w - 110.0, bottom: 40.0 };
        let display_title = if score.match_title.is_empty() {
            format!("{} vs {}", score.team1.abbreviation, score.team2.abbreviation)
        } else {
            score.match_title.clone()
        };
        self.formats.title.text(&self.rt, &display_title, &title_rect, &self.brushes.dim);

        self.render_status_badge(w, score.status, None);

        // 2. Middle section: Teams & Scores
        let half = w / 2.0;

        // Team 1 column (Left)
        let t1_batting = score.batting_team == 1 || score.team1.is_batting;
        let t1_name = if score.team1.abbreviation.is_empty() {
            &score.team1.name
        } else {
            &score.team1.abbreviation
        };

        let t1_name_rect = D2D_RECT_F { left: 44.0, top: 44.0, right: half - 16.0, bottom: 72.0 };
        if t1_batting {
            let dot = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: 30.0, y: 58.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            self.rt.FillEllipse(&dot, &self.brushes.green_accent);
        }
        self.formats.team_name.text(&self.rt, t1_name, &t1_name_rect, if t1_batting { &self.brushes.white } else { &self.brushes.dim });

        let t1_score_str = if score.team1.score.is_empty() {
            if score.team1.runs > 0 || score.team1.wickets > 0 {
                format!("{}/{}", score.team1.runs, score.team1.wickets)
            } else {
                "-".to_string()
            }
        } else {
            score.team1.score.clone()
        };
        self.formats.score_large.text(
            &self.rt,
            &t1_score_str,
            &D2D_RECT_F { left: 26.0, top: 74.0, right: half - 14.0, bottom: 118.0 },
            &self.brushes.white,
        );

        if score.team1.overs > 0.0 {
            let overs_str = format!("({:.1} ov)", score.team1.overs);
            self.formats.overs.text(
                &self.rt,
                &overs_str,
                &D2D_RECT_F { left: 26.0, top: 118.0, right: half - 14.0, bottom: 140.0 },
                &self.brushes.dim,
            );
        }

        // Center "vs" divider
        self.formats.center_dim.text(
            &self.rt,
            "vs",
            &D2D_RECT_F { left: half - 22.0, top: 76.0, right: half + 22.0, bottom: 112.0 },
            &self.brushes.subtle,
        );

        // Team 2 column (Right)
        let t2_batting = score.batting_team == 2 || score.team2.is_batting;
        let t2_name = if score.team2.abbreviation.is_empty() {
            &score.team2.name
        } else {
            &score.team2.abbreviation
        };

        let t2_name_rect = D2D_RECT_F { left: half + 44.0, top: 44.0, right: w - 26.0, bottom: 72.0 };
        if t2_batting {
            let dot = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: half + 30.0, y: 58.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            self.rt.FillEllipse(&dot, &self.brushes.green_accent);
        }
        self.formats.team_name.text(&self.rt, t2_name, &t2_name_rect, if t2_batting { &self.brushes.white } else { &self.brushes.dim });

        let t2_score_str = if score.team2.score.is_empty() {
            if score.team2.runs > 0 || score.team2.wickets > 0 {
                format!("{}/{}", score.team2.runs, score.team2.wickets)
            } else {
                "-".to_string()
            }
        } else {
            score.team2.score.clone()
        };
        self.formats.score_large.text(
            &self.rt,
            &t2_score_str,
            &D2D_RECT_F { left: half + 26.0, top: 74.0, right: w - 26.0, bottom: 118.0 },
            &self.brushes.white,
        );

        if score.team2.overs > 0.0 {
            let overs_str = format!("({:.1} ov)", score.team2.overs);
            self.formats.overs.text(
                &self.rt,
                &overs_str,
                &D2D_RECT_F { left: half + 26.0, top: 118.0, right: w - 26.0, bottom: 140.0 },
                &self.brushes.dim,
            );
        }

        // 3. Bottom Info Bar: CRR, RRR, Target, Runs Needed (8px radius)
        let info_rect = D2D_RECT_F { left: 20.0, top: h - 48.0, right: w - 20.0, bottom: h - 14.0 };
        let info_rr = D2D1_ROUNDED_RECT { rect: info_rect, radiusX: 8.0, radiusY: 8.0 };
        self.rt.FillRoundedRectangle(&info_rr, &self.brushes.card_surface);

        let mut info_parts = Vec::new();
        if score.crr > 0.0 {
            info_parts.push(format!("CRR: {:.2}", score.crr));
        }
        if let Some(rrr) = score.rrr {
            if rrr > 0.0 {
                info_parts.push(format!("RRR: {:.2}", rrr));
            }
        }
        if let Some(needed) = score.runs_needed {
            info_parts.push(format!("Need: {}", needed));
        } else if let Some(target) = score.target {
            info_parts.push(format!("Target: {}", target));
        }

        let info_str = if info_parts.is_empty() {
            match score.status {
                MatchStatus::Live => "Live match in progress".to_string(),
                MatchStatus::Break => "Innings Break".to_string(),
                MatchStatus::Completed => "Match Completed".to_string(),
                MatchStatus::Scheduled => "Match Scheduled".to_string(),
                MatchStatus::NoMatch => "".to_string(),
            }
        } else {
            info_parts.join("   ·   ")
        };

        self.formats.info.text(&self.rt, &info_str, &info_rect, &self.brushes.dim);
    }

    unsafe fn render_soccer(&self, w: f32, _h: f32, score: &MatchScore) {
        // 1. Top bar: Title + Clock Badge
        let title_rect = D2D_RECT_F { left: 26.0, top: 14.0, right: w - 110.0, bottom: 40.0 };
        self.formats.title.text(&self.rt, &score.match_title, &title_rect, &self.brushes.dim);

        let clock_label = score.soccer_clock.as_deref().unwrap_or("FT");
        self.render_status_badge(w, score.status, Some(clock_label));

        // 2. Middle Section: Teams & Scores
        let half = w / 2.0;

        let t1_name = if score.team1.abbreviation.is_empty() {
            &score.team1.name
        } else {
            &score.team1.abbreviation
        };
        self.formats.team_name.text(
            &self.rt,
            t1_name,
            &D2D_RECT_F { left: 30.0, top: 48.0, right: half - 70.0, bottom: 76.0 },
            &self.brushes.white,
        );

        let t2_name = if score.team2.abbreviation.is_empty() {
            &score.team2.name
        } else {
            &score.team2.abbreviation
        };
        self.formats.team_name_right.text(
            &self.rt,
            t2_name,
            &D2D_RECT_F { left: half + 70.0, top: 48.0, right: w - 30.0, bottom: 76.0 },
            &self.brushes.white,
        );

        let score_pair = format!("{}  —  {}", score.team1.runs, score.team2.runs);
        self.formats.score_center.text(
            &self.rt,
            &score_pair,
            &D2D_RECT_F { left: half - 100.0, top: 70.0, right: half + 100.0, bottom: 120.0 },
            &self.brushes.white,
        );

        // Match status is already communicated by the clock badge. Keeping the score area
        // clear makes the compact overlay calmer and avoids repeating "Live Match in Progress".
    }

    unsafe fn render_no_match(&self, w: f32, _h: f32) {
        self.formats.title.text(
            &self.rt,
            "SportsPulse",
            &D2D_RECT_F { left: 26.0, top: 16.0, right: w - 26.0, bottom: 42.0 },
            &self.brushes.subtle,
        );

        self.formats.no_match_title.text(
            &self.rt,
            "No Live Match Right Now",
            &D2D_RECT_F { left: 26.0, top: 82.0, right: w - 26.0, bottom: 122.0 },
            &self.brushes.white,
        );

    }

    unsafe fn render_status_badge(&self, w: f32, status: MatchStatus, custom_label: Option<&str>) {
        let badge_rect = D2D_RECT_F { left: w - 100.0, top: 14.0, right: w - 20.0, bottom: 42.0 };
        let badge_rr = D2D1_ROUNDED_RECT { rect: badge_rect, radiusX: 8.0, radiusY: 8.0 };

        let (text, bg_brush, fg_brush) = match (custom_label, status) {
            (Some(label), _) => (label, &self.brushes.green_badge_bg, &self.brushes.green_accent),
            (None, MatchStatus::Live) => ("● LIVE", &self.brushes.green_badge_bg, &self.brushes.green_accent),
            (None, MatchStatus::Break) => ("BREAK", &self.brushes.amber_badge_bg, &self.brushes.amber_accent),
            (None, MatchStatus::Scheduled) => ("UPCOMING", &self.brushes.blue_badge_bg, &self.brushes.blue_accent),
            (None, MatchStatus::Completed) => ("FINAL", &self.brushes.card_surface, &self.brushes.dim),
            (None, MatchStatus::NoMatch) => ("OFFLINE", &self.brushes.card_surface, &self.brushes.subtle),
        };

        self.rt.FillRoundedRectangle(&badge_rr, bg_brush);
        self.formats.badge.text(&self.rt, text, &badge_rect, fg_brush);
    }

    unsafe fn flush_to_layered_window(&self, pos: &POINT) -> Result<()> {
        let row = (self.w * 4) as usize;
        let mut buf = vec![0u8; row * self.h as usize];
        self.wic.CopyPixels(std::ptr::null(), row as u32, &mut buf)?;
        std::ptr::copy_nonoverlapping(buf.as_ptr(), self.bits as *mut u8, buf.len());

        let size = SIZE { cx: self.w, cy: self.h };
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
