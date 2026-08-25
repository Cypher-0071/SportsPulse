//! M1: Direct2D + DirectWrite scoreboard renderer (static sample data).

use windows::core::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
    D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

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

fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

pub struct Renderer {
    rt: ID2D1HwndRenderTarget,
    bg: ID2D1SolidColorBrush,
    white: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    fmt_title: Fmt,
    fmt_score: Fmt,
    fmt_status: Fmt,
    fmt_info: Fmt,
}

struct Fmt {
    fmt: IDWriteTextFormat,
}

impl Fmt {
    unsafe fn text(&self, rt: &ID2D1HwndRenderTarget, s: &str, rect: &D2D_RECT_F, brush: &ID2D1SolidColorBrush) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        rt.DrawText(
            &wide,
            &self.fmt,
            rect,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

impl Renderer {
    pub unsafe fn new(hwnd: HWND, w: u32, h: u32) -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U { width: w, height: h },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let rt = factory.CreateHwndRenderTarget(&props, &hwnd_props)?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);

        let bg = rt.CreateSolidColorBrush(&color(0.055, 0.063, 0.082, 1.0), None)?;
        let white = rt.CreateSolidColorBrush(&color(0.96, 0.97, 0.98, 1.0), None)?;
        let dim = rt.CreateSolidColorBrush(&color(0.55, 0.58, 0.62, 1.0), None)?;
        let accent = rt.CreateSolidColorBrush(&color(0.16, 0.85, 0.45, 1.0), None)?;

        let mk = |size: f32, weight: DWRITE_FONT_WEIGHT, center: bool| -> Result<Fmt> {
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
            if center {
                fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            } else {
                fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
                fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            }
            Ok(Fmt { fmt })
        };

        Ok(Self {
            bg,
            white,
            dim,
            accent,
            fmt_title: mk(12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, true)?,
            fmt_score: mk(26.0, DWRITE_FONT_WEIGHT_BOLD, false)?,
            fmt_status: mk(11.0, DWRITE_FONT_WEIGHT_BOLD, true)?,
            fmt_info: mk(12.0, DWRITE_FONT_WEIGHT_NORMAL, true)?,
            rt,
        })
    }

    pub unsafe fn resize(&self, w: u32, h: u32) -> Result<()> {
        self.rt.Resize(&D2D_SIZE_U { width: w, height: h })
    }

    pub unsafe fn draw(&self, s: &SampleScore) -> Result<()> {
        let size = self.rt.GetSize();
        let w = size.width;
        let h = size.height;
        let full = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
        self.rt.BeginDraw();
        self.rt.FillRectangle(&full, &self.bg);

        self.fmt_title.text(&self.rt, s.title, &D2D_RECT_F { left: 0.0, top: 4.0, right: w, bottom: 22.0 }, &self.dim);

        let half = w / 2.0;
        let t1 = format!("{} {}", s.t1_abbr, s.t1_score);
        let t2 = format!("{} {}", s.t2_abbr, s.t2_score);
        self.fmt_score.text(&self.rt, &t1, &D2D_RECT_F { left: 12.0, top: 30.0, right: half - 36.0, bottom: 68.0 }, &self.white);
        self.fmt_score.text(&self.rt, &t2, &D2D_RECT_F { left: half + 36.0, top: 30.0, right: w - 12.0, bottom: 68.0 }, &self.white);

        self.fmt_status.text(&self.rt, s.status, &D2D_RECT_F { left: half - 30.0, top: 38.0, right: half + 30.0, bottom: 58.0 }, &self.accent);
        self.fmt_info.text(&self.rt, s.info, &D2D_RECT_F { left: 0.0, top: h - 24.0, right: w, bottom: h - 4.0 }, &self.dim);

        self.rt.EndDraw(None, None)
    }
}
