//! Direct2D + DirectWrite drawing into per-pixel-alpha layered windows.
//!
//! Everything is drawn in DIPs (the design's CSS pixels): the render target's DPI is `96 * scale`, so the numbers from the
//! Obsidian mockup are used as they are, and D2D scales strokes, text and blur. A [`Surface`] is one window's 32-bit
//! premultiplied DIB; [`Surface::present`] hands it to `UpdateLayeredWindow`, so transparent pixels click through.

use std::collections::HashMap;

thread_local! {
    static BENCH_END: std::cell::Cell<Option<std::time::Duration>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn bench_start() { BENCH_END.set(Some(std::time::Duration::ZERO)); }
pub(crate) fn bench_end() -> std::time::Duration { BENCH_END.replace(None).unwrap_or_default() }

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, CLSID_D2D1Shadow, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_PROPERTIES,
    D2D1_COLORMATRIX_PROP_COLOR_MATRIX, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    D2D1_LAYER_PARAMETERS1, D2D1_PROPERTY_TYPE_MATRIX_5X4, ID2D1Geometry, ID2D1Image, D2D1_CAP_STYLE_ROUND, D2D1_COMBINE_MODE_EXCLUDE,
    D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2,
    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINE_JOIN_ROUND,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_PROPERTY_TYPE_FLOAT, D2D1_PROPERTY_TYPE_VECTOR4, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION,
    D2D1_SHADOW_PROP_COLOR, D2D1_STROKE_STYLE_PROPERTIES1, D2D1_STROKE_TRANSFORM_TYPE_NORMAL, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, D2D1_PRIMITIVE_BLEND_COPY, D2D1_PRIMITIVE_BLEND_SOURCE_OVER, ID2D1Bitmap, ID2D1BitmapRenderTarget, ID2D1DCRenderTarget, ID2D1DeviceContext, ID2D1Factory1, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle1,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_FONT_AXIS_TAG_OPTICAL_SIZE, DWRITE_FONT_AXIS_TAG_WEIGHT, DWRITE_FONT_AXIS_VALUE, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC,
    DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, DWriteCreateFactory, IDWriteFactory, IDWriteFactory6,
    IDWriteFontCollection2, IDWriteFontSetBuilder1, IDWriteTextFormat, IDWriteTextLayout1,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt, CAPTUREBLT, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ROP_CODE, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeMedianCut,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::{HICON, ULW_ALPHA, UpdateLayeredWindow};
use windows::core::{HSTRING, IUnknown, Interface, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::svg::{self, Seg};

/// A straight-alpha colour (D2D premultiplies for the target).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0.0, 0.0, 0.0, 0.0);

    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `0xRRGGBB` as written in the design.
    #[must_use]
    pub const fn hex(rgb: u32) -> Self {
        Self::new(((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0, 1.0)
    }

    /// CSS `rgba(r, g, b, a)` as written in the design.
    #[must_use]
    pub const fn css(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a)
    }

    /// A GDI `COLORREF` (`0x00BBGGRR`, what `winrust_common::style::pal()` hands out).
    #[must_use]
    pub const fn colorref(c: u32) -> Self {
        Self::new((c & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, ((c >> 16) & 255) as f32 / 255.0, 1.0)
    }

    /// White at `a`: every hover and pressed fill of the design is `rgba(255,255,255,a)`.
    #[must_use]
    pub const fn white(a: f32) -> Self {
        Self::new(1.0, 1.0, 1.0, a)
    }

    #[must_use]
    pub const fn alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Opacity applied on top of the colour's own alpha.
    #[must_use]
    pub fn fade(self, k: f32) -> Self {
        Self { a: self.a * k.clamp(0.0, 1.0), ..self }
    }

    /// Linear blend; `t = 0` is `self`.
    #[must_use]
    pub fn mix(self, o: Rgba, t: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self::new(l(self.r, o.r), l(self.g, o.g), l(self.b, o.b), l(self.a, o.a))
    }

    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.r, g: self.g, b: self.b, a: self.a }
    }
}

/// A rectangle in DIPs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub fn cx(self) -> f32 {
        self.x + self.w / 2.0
    }

    #[must_use]
    pub fn cy(self) -> f32 {
        self.y + self.h / 2.0
    }

    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// Shrinks every side by `d` (negative grows).
    #[must_use]
    pub fn inset(self, d: f32) -> Self {
        Self::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }

    /// Scales around the centre, like CSS `transform: scale(k)`.
    #[must_use]
    pub fn scale(self, k: f32) -> Self {
        let (w, h) = (self.w * k, self.h * k);
        Self::new(self.cx() - w / 2.0, self.cy() - h / 2.0, w, h)
    }

    #[must_use]
    pub fn shift(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
}

/// Horizontal text alignment inside the given rectangle (text is always centred vertically).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// One text style of the design: size in px, CSS weight, letter spacing in em, CSS `font-variant-numeric: tabular-nums`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    pub size: f32,
    pub weight: u16,
    pub tracking: f32,
    pub tabular: bool,
}

impl Font {
    #[must_use]
    pub const fn new(size: f32, weight: u16) -> Self {
        Self { size, weight, tracking: 0.0, tabular: false }
    }

    #[must_use]
    pub const fn tracking(self, em: f32) -> Self {
        Self { tracking: em, ..self }
    }

    /// Digits of one width, so a ticking clock does not jitter.
    #[must_use]
    pub const fn tabular(self) -> Self {
        Self { tabular: true, ..self }
    }
}

/// The design's typeface, built in: a Latin + Cyrillic subset of Inter's variable font (SIL OFL 1.1, `fonts/OFL.txt`).
static INTER: &[u8] = include_bytes!("../../fonts/InterVariable.ttf");
const INTER_FAMILY: &str = "Inter Variable";

/// Fallbacks when the system cannot load a variable font from memory (DirectWrite before Windows 10 1709).
const FACES: [&str; 3] = ["Inter", "Segoe UI Variable Text", "Segoe UI"];

/// A window's backing store: a top-down 32-bit premultiplied DIB selected into its own memory DC.
pub struct Surface {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
}

impl Surface {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dc: HDC(std::ptr::null_mut()),
            bmp: HBITMAP(std::ptr::null_mut()),
            old: HGDIOBJ(std::ptr::null_mut()),
            bits: std::ptr::null_mut(),
            w: 0,
            h: 0,
        }
    }

    /// Size in device pixels.
    #[must_use]
    pub const fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    fn ensure(&mut self, w: i32, h: i32) -> bool {
        if self.w == w && self.h == h && !self.dc.is_invalid() {
            return true;
        }
        self.free();
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: plain GDI object creation; every handle is released in `free`.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return false;
            };
            self.old = SelectObject(dc, bmp.into());
            (self.dc, self.bmp, self.bits, self.w, self.h) = (dc, bmp, bits.cast(), w, h);
        }
        true
    }

    /// The last frame's premultiplied BGRA pixels, row by row (empty before the first paint).
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        if self.bits.is_null() {
            return &[];
        }
        // SAFETY: the DIB section owns `w * h` 32-bit pixels for as long as `self` keeps it.
        unsafe { std::slice::from_raw_parts(self.bits, (self.w * self.h) as usize) }
    }

    /// Copies a rectangle of the screen (device pixels, entirely on the screen, layered windows included) into this surface
    /// and makes every pixel opaque. This is what a panel's glass blurs.
    pub fn grab(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 || !self.ensure(w, h) {
            return false;
        }
        // SAFETY: the screen DC is released at once; the DIB belongs to `self` and holds `w * h` pixels.
        unsafe {
            let screen = GetDC(None);
            let ok = BitBlt(self.dc, 0, 0, w, h, Some(screen), x, y, ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0)).is_ok();
            ReleaseDC(None, screen);
            if ok {
                std::slice::from_raw_parts_mut(self.bits, (w * h) as usize).iter_mut().for_each(|p| *p |= 0xff00_0000);
            }
            ok
        }
    }

    fn free(&mut self) {
        if self.dc.is_invalid() {
            return;
        }
        let (dc, bmp, old) = (self.dc, self.bmp, self.old);
        // reset first: assigning a fresh `Self` would drop the old value and call this again
        (self.dc, self.bmp, self.old, self.bits, self.w, self.h) =
            (HDC(std::ptr::null_mut()), HBITMAP(std::ptr::null_mut()), HGDIOBJ(std::ptr::null_mut()), std::ptr::null_mut(), 0, 0);
        // SAFETY: the handles were created in `ensure` and are not used after this.
        unsafe {
            SelectObject(dc, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
        }
    }

    /// Puts the last painted frame on screen at `(x, y)` (device pixels) with a whole-window opacity; false if Windows refused.
    pub fn present(&self, hwnd: HWND, x: i32, y: i32, alpha: u8) -> bool {
        if self.dc.is_invalid() {
            return false;
        }
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
        // SAFETY: `self.dc` holds a live 32-bit DIB of the given size.
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&POINT { x, y }),
                Some(&SIZE { cx: self.w, cy: self.h }),
                Some(self.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
            .is_ok()
        }
    }
}

impl Default for Surface {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.free();
    }
}

/// A baked picture that stays in the render target's memory: the blurred desktop behind a panel. It belongs to the device
/// it was made on, so a panel makes a new one each time it opens.
pub struct Picture {
    bmp: ID2D1Bitmap,
    /// Size in DIPs.
    pub w: f32,
    pub h: f32,
}

/// CSS `transform: translate(dx, dy) scale(k)` around `(cx, cy)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub cx: f32,
    pub cy: f32,
    pub k: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Xf {
    pub const NONE: Xf = Xf { cx: 0.0, cy: 0.0, k: 1.0, dx: 0.0, dy: 0.0 };

    fn matrix(self) -> Matrix3x2 {
        Matrix3x2::translation(-self.cx, -self.cy) * Matrix3x2::scale(self.k, self.k) * Matrix3x2::translation(self.cx + self.dx, self.cy + self.dy)
    }
}

/// Resources bound to the D2D device; rebuilt together after `D2DERR_RECREATE_TARGET`.
struct Device {
    rt: ID2D1DCRenderTarget,
    ctx: ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
    icons: HashMap<usize, ID2D1Bitmap>,
    shadows: HashMap<[u32; 6], (ID2D1Bitmap, f32)>,
}

type LayoutKey = (String, u32, u16, u32, bool, u32, u32, u8, bool);

/// The renderer: factories, the device, and caches of geometry and text formats.
pub struct Gfx {
    factory: ID2D1Factory1,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    round: ID2D1StrokeStyle1,
    dev: Option<Device>,
    paths: HashMap<&'static str, Option<ID2D1PathGeometry>>,
    formats: HashMap<(u32, u16), IDWriteTextFormat>,
    layouts: HashMap<LayoutKey, IDWriteTextLayout1>,
    /// The built-in Inter, when the system can use it.
    inter: Option<(IDWriteFactory6, IDWriteFontCollection2)>,
    face: &'static str,
    scale: f32,
    software: bool,
}

impl Gfx {
    /// Needs COM initialised on this thread (WIC).
    pub fn new(scale: f32) -> Result<Self> {
        // SAFETY: factory creation has no preconditions beyond COM for WIC.
        unsafe {
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let round = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES1 {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                    transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL, // strokes scale with the icon, like SVG
                },
                None,
            )?;
            let (inter, face) = (inter(&dwrite), pick_face(&dwrite));
            Ok(Self { factory, dwrite, wic, round, dev: None, paths: HashMap::new(), formats: HashMap::new(), layouts: HashMap::new(), inter, face, scale, software: false })
        }
    }

    /// Device pixels per DIP.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// Select a target before creating a new popup's device-dependent backdrop.
    pub fn target_for(&mut self, w: i32, h: i32) {
        let software = i64::from(w) * i64::from(h) <= 2_000_000;
        if self.software != software {
            self.dev = None;
            self.software = software;
        }
    }

    /// Drops every cached icon bitmap (a panel that drew icons it does not own, whose handles may be reused).
    pub fn forget_icons(&mut self) {
        if let Some(d) = self.dev.as_mut() {
            d.icons.clear();
        }
    }

    /// DIPs to whole device pixels.
    #[must_use]
    pub fn px(&self, dip: f32) -> i32 {
        (dip * self.scale).round() as i32
    }

    fn device(&mut self) -> Result<&mut Device> {
        if self.dev.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: if self.software { D2D1_RENDER_TARGET_TYPE_SOFTWARE } else { D2D1_RENDER_TARGET_TYPE_DEFAULT },
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * self.scale,
                dpiY: 96.0 * self.scale,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            // SAFETY: valid properties; the objects are owned by `Device`.
            let dev = unsafe {
                let rt = self.factory.CreateDCRenderTarget(&props)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE); // ClearType needs an opaque target
                rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                let ctx = rt.cast::<ID2D1DeviceContext>()?;
                let brush = rt.CreateSolidColorBrush(&Rgba::TRANSPARENT.d2d(), None)?;
                Device { rt, ctx, brush, icons: HashMap::new(), shadows: HashMap::new() }
            };
            self.dev = Some(dev);
        }
        Ok(self.dev.as_mut().expect("device was just created"))
    }

    /// Paints one frame of `w`×`h` device pixels into `surf`; `draw` gets a cleared, transparent canvas in DIPs.
    pub fn paint(&mut self, surf: &mut Surface, w: i32, h: i32, draw: impl FnOnce(&mut Canvas<'_>)) -> Result<()> {
        if w <= 0 || h <= 0 || !surf.ensure(w, h) {
            return Ok(());
        }
        let rc = RECT { left: 0, top: 0, right: w, bottom: h };
        let rt = self.device()?.rt.clone();
        // SAFETY: the DIB stays selected into `surf.dc` for the whole frame.
        unsafe {
            rt.BindDC(surf.dc, &rc)?;
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2::identity());
            rt.Clear(Some(&Rgba::TRANSPARENT.d2d()));
        }
        draw(&mut Canvas { g: self });
        // SAFETY: matches the BeginDraw above.
        let started = BENCH_END.with(|b| b.get().map(|_| std::time::Instant::now()));
        let end = unsafe { rt.EndDraw(None, None) };
        if let Some(started) = started {
            BENCH_END.with(|b| b.set(b.get().map(|d| d + started.elapsed())));
        }
        if end.is_err() {
            self.dev = None; // device lost or target broken: rebuild on the next frame
        }
        end
    }

    fn path(&mut self, d: &'static str) -> Option<ID2D1PathGeometry> {
        if let Some(p) = self.paths.get(d) {
            return p.clone();
        }
        let geo = svg::parse(d).ok().and_then(|segs| build_path(&self.factory, &segs).ok());
        self.paths.insert(d, geo.clone());
        geo
    }

    /// Width of `s` in DIPs (no frame needed: the bar lays itself out before it paints).
    pub fn measure(&mut self, s: &str, f: Font) -> f32 {
        let Some(lay) = self.layout(s, f, 4096.0, f.size * 2.0, Align::Start, false) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: `lay` is live; `m` is a local out-param.
        unsafe { lay.GetMetrics(&mut m).map_or(0.0, |()| m.widthIncludingTrailingWhitespace) }
    }

    /// Lays out one line of text; `None` if the font could not be made.
    fn layout(&mut self, s: &str, f: Font, w: f32, h: f32, align: Align, trim: bool) -> Option<IDWriteTextLayout1> {
        let key = (s.to_owned(), f.size.to_bits(), f.weight, f.tracking.to_bits(), f.tabular, w.to_bits(), h.to_bits(), align as u8, trim);
        if let Some(layout) = self.layouts.get(&key) { return Some(layout.clone()); }
        let fmt = self.format(f)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: plain DirectWrite layout creation.
        unsafe {
            let lay = self.dwrite.CreateTextLayout(&wide, &fmt, w.max(1.0), h.max(1.0)).ok()?;
            let _ = lay.SetTextAlignment(match align {
                Align::Start => DWRITE_TEXT_ALIGNMENT(0),
                Align::End => DWRITE_TEXT_ALIGNMENT(1),
                Align::Center => DWRITE_TEXT_ALIGNMENT(2),
            });
            if trim {
                // what does not fit ends in an ellipsis (`text-overflow: ellipsis`)
                let sign = self.dwrite.CreateEllipsisTrimmingSign(&fmt).ok();
                let t = windows::Win32::Graphics::DirectWrite::DWRITE_TRIMMING { granularity: windows::Win32::Graphics::DirectWrite::DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
                let _ = lay.SetTrimming(&t, sign.as_ref());
            }
            let lay = lay.cast::<IDWriteTextLayout1>().ok()?;
            let all = DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 };
            if f.tracking != 0.0 {
                let _ = lay.SetCharacterSpacing(0.0, f.tracking * f.size, 0.0, all);
            }
            if f.tabular
                && let Ok(ty) = self.dwrite.CreateTypography()
            {
                let _ = ty.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 });
                let _ = lay.SetTypography(&ty, all);
            }
            if self.layouts.len() >= 2048 { self.layouts.clear(); }
            self.layouts.insert(key, lay.clone());
            Some(lay)
        }
    }

    /// Writes opaque premultiplied BGRA pixels (`w`×`h`) to a PNG file.
    pub fn save_png(&self, path: &str, w: u32, h: u32, px: &[u32]) -> Result<()> {
        use windows::Win32::Graphics::Imaging::{GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, WICBitmapEncoderNoCache};
        // SAFETY: a stream, an encoder and one frame, all owned by this call; `px` outlives WritePixels.
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path), 0x4000_0000)?; // GENERIC_WRITE
            let enc = self.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let (mut frame, mut props) = (None, None);
            enc.CreateNewFrame(&mut frame, &mut props)?;
            let frame = frame.ok_or_else(windows::core::Error::empty)?;
            frame.Initialize(props.as_ref())?;
            frame.SetSize(w, h)?;
            let mut fmt = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut fmt)?;
            let bytes = std::slice::from_raw_parts(px.as_ptr().cast::<u8>(), px.len() * 4);
            frame.WritePixels(h, w * 4, bytes)?;
            frame.Commit()?;
            enc.Commit()
        }
    }

    fn format(&mut self, f: Font) -> Option<IDWriteTextFormat> {
        let key = (f.size.to_bits(), f.weight);
        if let Some(t) = self.formats.get(&key) {
            return Some(t.clone());
        }
        // SAFETY: plain DirectWrite object creation.
        let t: IDWriteTextFormat = unsafe {
            match &self.inter {
                // the axes set by hand, as a browser does: the CSS weight, and the optical size following the text size
                // (`font-optical-sizing: auto`; Inter's opsz axis spans 14..32)
                Some((f6, coll)) => {
                    let axes = [
                        DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT, value: f32::from(f.weight) },
                        DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_OPTICAL_SIZE, value: f.size.clamp(14.0, 32.0) },
                    ];
                    f6.CreateTextFormat(&HSTRING::from(INTER_FAMILY), coll, &axes, f.size, w!("en-us")).ok()?.cast().ok()?
                }
                None => self
                    .dwrite
                    .CreateTextFormat(
                        &HSTRING::from(self.face),
                        None,
                        DWRITE_FONT_WEIGHT(i32::from(f.weight)),
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        f.size,
                        w!("en-us"),
                    )
                    .ok()?,
            }
        };
        // SAFETY: `t` is a live text format.
        unsafe {
            let _ = t.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = t.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        self.formats.insert(key, t.clone());
        Some(t)
    }
}

/// The built-in Inter as a font collection of its own; `None` where DirectWrite cannot load a variable font from memory.
fn inter(dw: &IDWriteFactory) -> Option<(IDWriteFactory6, IDWriteFontCollection2)> {
    // SAFETY: the font data is 'static, so the in-memory file reference never outlives it.
    unsafe {
        let f6 = dw.cast::<IDWriteFactory6>().ok()?;
        let loader = f6.CreateInMemoryFontFileLoader().ok()?;
        f6.RegisterFontFileLoader(&loader).ok()?;
        let file = loader.CreateInMemoryFontFileReference(&f6, INTER.as_ptr().cast(), INTER.len() as u32, None::<&IUnknown>).ok()?;
        let builder = f6.CreateFontSetBuilder().ok()?;
        let from_memory: &IDWriteFontSetBuilder1 = &builder; // Builder2's own AddFontFile takes a path
        from_memory.AddFontFile(&file).ok()?;
        let set = builder.CreateFontSet().ok()?;
        let coll = f6.CreateFontCollectionFromFontSet(&set, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC).ok()?;
        Some((f6, coll))
    }
}

/// `Inter` when installed, else the Windows UI font (used only without the built-in Inter).
fn pick_face(dw: &IDWriteFactory) -> &'static str {
    let mut coll = None;
    // SAFETY: reads the system font collection.
    unsafe {
        if dw.GetSystemFontCollection(&mut coll, false).is_err() {
            return FACES[2];
        }
    }
    let Some(coll) = coll else { return FACES[2] };
    for face in FACES {
        let (mut idx, mut found) = (0u32, windows::core::BOOL(0));
        // SAFETY: `coll` is live; out-params are locals.
        if unsafe { coll.FindFamilyName(&HSTRING::from(face), &mut idx, &mut found) }.is_ok() && found.as_bool() {
            return face;
        }
    }
    FACES[2]
}

fn build_path(factory: &ID2D1Factory1, segs: &[Seg]) -> Result<ID2D1PathGeometry> {
    let v = |p: svg::Pt| Vector2 { X: p.x, Y: p.y };
    // SAFETY: a fresh geometry is filled through its own sink and closed once.
    unsafe {
        let geo = factory.CreatePathGeometry()?;
        let sink = geo.Open()?;
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        let mut open = false;
        for s in segs {
            match *s {
                Seg::Move(p) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(v(p), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                }
                Seg::Line(p) if open => sink.AddLine(v(p)),
                Seg::Cubic(a, b, c) if open => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: v(a), point2: v(b), point3: v(c) }),
                Seg::Close if open => {
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    open = false;
                }
                _ => {}
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
        geo.cast()
    }
}

/// Drawing operations for one frame. Coordinates and sizes are DIPs.
pub struct Canvas<'a> {
    g: &'a mut Gfx,
}

impl Canvas<'_> {
    fn dev(&mut self) -> &mut Device {
        self.g.dev.as_mut().expect("a frame always has a device")
    }

    fn brush(&mut self, c: Rgba) -> ID2D1SolidColorBrush {
        let b = self.dev().brush.clone();
        // SAFETY: setting a brush colour.
        unsafe { b.SetColor(&c.d2d()) };
        b
    }

    fn rt(&mut self) -> ID2D1RenderTarget {
        self.dev().rt.clone().into()
    }

    /// Device pixels per DIP.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.g.scale
    }

    /// Fills a rounded rectangle (`r = 0` is square).
    pub fn fill(&mut self, r: Rect, radius: f32, c: Rgba) {
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let (rt, b) = (self.rt(), self.brush(c));
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            if radius > 0.0 {
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &b);
            } else {
                rt.FillRectangle(&r.d2d(), &b);
            }
        }
    }

    /// CSS `box-shadow: inset 0 0 0 <w>px <c>`: a border drawn inside `r`.
    pub fn ring(&mut self, r: Rect, radius: f32, w: f32, c: Rgba) {
        if c.a <= 0.0 {
            return;
        }
        let half = w / 2.0;
        let rr = r.inset(half);
        let (rt, b) = (self.rt(), self.brush(c));
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: rr.d2d(), radiusX: (radius - half).max(0.0), radiusY: (radius - half).max(0.0) },
                &b,
                w,
                None,
            );
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Rgba) {
        let (rt, b) = (self.rt(), self.brush(c));
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, &b, w, None) };
    }

    /// The part of a disc (centre, radius) that lies inside the rounded rectangle `r`: the `clip-path: circle(..)` fill of an
    /// `overflow: hidden` tile (quick settings' tiles fill from the chip outwards).
    pub fn fill_disc_in(&mut self, r: Rect, radius: f32, (cx, cy): (f32, f32), disc: f32, c: Rgba) {
        if c.a <= 0.0 || disc <= 0.0 {
            return;
        }
        let (rt, b) = (self.rt(), self.brush(c));
        let f = &self.g.factory;
        // SAFETY: geometry creation on the factory; the result is drawn inside BeginDraw/EndDraw.
        unsafe {
            let Ok(rr) = f.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }) else { return };
            let Ok(el) = f.CreateEllipseGeometry(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: disc, radiusY: disc }) else { return };
            let Ok(out) = f.CreatePathGeometry() else { return };
            let Ok(sink) = out.Open() else { return };
            let combined = rr.CombineWithGeometry(&el, windows::Win32::Graphics::Direct2D::D2D1_COMBINE_MODE_INTERSECT, None, 0.1, &sink);
            if combined.is_ok() && sink.Close().is_ok() {
                rt.FillGeometry(&out, &b, None);
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let (rt, b) = (self.rt(), self.brush(c));
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, &b);
        }
    }

    /// An arc of a circle, stroked `w` wide with round caps (SVG `stroke-linecap: round`): from `start` degrees (0 is three
    /// o'clock, clockwise is positive) over `sweep` degrees, at most one turn. The power dialog's countdown ring and spinner.
    pub fn arc(&mut self, (cx, cy): (f32, f32), r: f32, start: f32, sweep: f32, w: f32, c: Rgba) {
        if c.a <= 0.0 || sweep <= 0.0 {
            return;
        }
        // cubic pieces of at most a quarter turn: the control arms are 4/3 tan(a/4) of the radius long
        let sweep = sweep.min(360.0);
        let n = (sweep / 90.0).ceil() as usize;
        let step = (sweep / n as f32).to_radians();
        let arm = 4.0 / 3.0 * (step / 4.0).tan() * r;
        let at = |a: f32| svg::Pt { x: cx + r * a.cos(), y: cy + r * a.sin() };
        let a0 = start.to_radians();
        let mut segs = vec![Seg::Move(at(a0))];
        for i in 0..n {
            let (p, q) = (a0 + step * i as f32, a0 + step * (i + 1) as f32);
            let (from, to) = (at(p), at(q));
            let c1 = svg::Pt { x: from.x - arm * p.sin(), y: from.y + arm * p.cos() };
            let c2 = svg::Pt { x: to.x + arm * q.sin(), y: to.y - arm * q.cos() };
            segs.push(Seg::Cubic(c1, c2, to));
        }
        let Ok(geo) = build_path(&self.g.factory, &segs) else { return };
        let (rt, b, round) = (self.rt(), self.brush(c), self.g.round.clone());
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { rt.DrawGeometry(&geo, &b, w, &round) };
    }

    /// A 1-px separator that fades out at both ends, like the design's
    /// `linear-gradient(transparent, c 30%, c 70%, transparent)`; `vertical` runs top to bottom.
    pub fn fade_line(&mut self, r: Rect, vertical: bool, edge: f32, c: Rgba) {
        let rt = self.rt();
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: c.alpha(0.0).d2d() },
            D2D1_GRADIENT_STOP { position: edge, color: c.d2d() },
            D2D1_GRADIENT_STOP { position: 1.0 - edge, color: c.d2d() },
            D2D1_GRADIENT_STOP { position: 1.0, color: c.alpha(0.0).d2d() },
        ];
        let (start, end) = if vertical {
            (Vector2 { X: r.cx(), Y: r.y }, Vector2 { X: r.cx(), Y: r.y + r.h })
        } else {
            (Vector2 { X: r.x, Y: r.cy() }, Vector2 { X: r.x + r.w, Y: r.cy() })
        };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            let Ok(coll) = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: start, endPoint: end };
            if let Ok(b) = rt.CreateLinearGradientBrush(&props, None, &coll) {
                rt.FillRectangle(&r.d2d(), &b);
            }
        }
    }

    /// A rounded rectangle filled with a vertical gradient, CSS `linear-gradient(180deg, top, bottom)`.
    pub fn gradient(&mut self, r: Rect, radius: f32, top: Rgba, bottom: Rgba) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let rt = self.rt();
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: top.d2d() }, D2D1_GRADIENT_STOP { position: 1.0, color: bottom.d2d() }];
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            let Ok(coll) = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: r.cx(), Y: r.y }, endPoint: Vector2 { X: r.cx(), Y: r.y + r.h } };
            if let Ok(b) = rt.CreateLinearGradientBrush(&props, None, &coll) {
                rt.FillRoundedRectangle(&rr, &b);
            }
        }
    }

    /// A rounded rectangle with a radial gradient, CSS `radial-gradient(rx ry at (cx, cy), c, transparent <end>)`: `c` at the
    /// centre fading out to nothing at `end` (0..1 of the radius) and staying clear beyond.
    pub fn radial(&mut self, r: Rect, radius: f32, (cx, cy): (f32, f32), (rx, ry): (f32, f32), c: Rgba, end: f32) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let rt = self.rt();
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: c.d2d() },
            D2D1_GRADIENT_STOP { position: end, color: c.alpha(0.0).d2d() },
            D2D1_GRADIENT_STOP { position: 1.0, color: c.alpha(0.0).d2d() },
        ];
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            let Ok(coll) = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 { X: cx, Y: cy },
                gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                radiusX: rx,
                radiusY: ry,
            };
            if let Ok(b) = rt.CreateRadialGradientBrush(&props, None, &coll) {
                rt.FillRoundedRectangle(&rr, &b);
            }
        }
    }

    /// Strokes a 24×24 design icon (SVG path data) into a `size`-DIP square centred at `(cx, cy)`, with round caps
    /// and joins; `stroke` is the SVG `stroke-width` in icon units. `turn` rotates it in degrees around its centre.
    pub fn icon(&mut self, d: &'static str, (cx, cy): (f32, f32), size: f32, stroke: f32, c: Rgba, turn: f32) {
        let Some(geo) = self.g.path(d) else { return };
        let k = size / 24.0;
        let m = Matrix3x2::translation(-12.0, -12.0)
            * Matrix3x2::rotation(turn)
            * Matrix3x2::scale(k, k)
            * Matrix3x2::translation(cx, cy);
        let (rt, b, round) = (self.rt(), self.brush(c), self.g.round.clone());
        let cur = self.world();
        // SAFETY: drawing inside BeginDraw/EndDraw; the transform is restored.
        unsafe {
            rt.SetTransform(&(m * cur));
            rt.DrawGeometry(&geo, &b, stroke, &round);
            rt.SetTransform(&cur);
        }
    }

    /// Fills a 24×24 design path (no stroke) into a `size`-DIP square centred at `(cx, cy)`.
    pub fn glyph(&mut self, d: &'static str, (cx, cy): (f32, f32), size: f32, c: Rgba) {
        let Some(geo) = self.g.path(d) else { return };
        let k = size / 24.0;
        let m = Matrix3x2::translation(-12.0, -12.0) * Matrix3x2::scale(k, k) * Matrix3x2::translation(cx, cy);
        let (rt, b) = (self.rt(), self.brush(c));
        let cur = self.world();
        // SAFETY: drawing inside BeginDraw/EndDraw; the transform is restored.
        unsafe {
            rt.SetTransform(&(m * cur));
            rt.FillGeometry(&geo, &b, None);
            rt.SetTransform(&cur);
        }
    }

    /// Width of `s` in DIPs.
    pub fn measure(&mut self, s: &str, f: Font) -> f32 {
        self.g.measure(s, f)
    }

    /// One line of text, vertically centred in `r`.
    pub fn text(&mut self, s: &str, r: Rect, f: Font, c: Rgba, align: Align) {
        self.draw_text(s, r, f, c, align, false);
    }

    /// Like [`Canvas::text`], and what does not fit in `r` ends in an ellipsis.
    pub fn text_fit(&mut self, s: &str, r: Rect, f: Font, c: Rgba, align: Align) {
        self.draw_text(s, r, f, c, align, true);
    }

    fn draw_text(&mut self, s: &str, r: Rect, f: Font, c: Rgba, align: Align, trim: bool) {
        if c.a <= 0.0 {
            return;
        }
        let Some(lay) = self.g.layout(s, f, r.w, r.h, align, trim) else { return };
        let (rt, b) = (self.rt(), self.brush(c));
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { rt.DrawTextLayout(Vector2 { X: r.x, Y: r.y }, &lay, &b, D2D1_DRAW_TEXT_OPTIONS_NONE) };
    }

    /// CSS `box-shadow: inset 0 <dy>px 0 <c>` on a rounded rectangle: the strip of the box that a copy of itself moved by `dy`
    /// leaves uncovered (`dy > 0` lights the top edge, `dy < 0` the bottom one), following the corners.
    pub fn inset_edge(&mut self, r: Rect, radius: f32, dy: f32, c: Rgba) {
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let rounded = |r: Rect| D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        let (rt, b) = (self.rt(), self.brush(c));
        let f = &self.g.factory;
        // SAFETY: plain geometry work on the factory; the result is filled inside BeginDraw/EndDraw.
        unsafe {
            let strip = (|| -> Result<ID2D1PathGeometry> {
                let (a, moved) = (f.CreateRoundedRectangleGeometry(&rounded(r))?, f.CreateRoundedRectangleGeometry(&rounded(r.shift(0.0, dy)))?);
                let path = f.CreatePathGeometry()?;
                let sink = path.Open()?;
                a.CombineWithGeometry(&moved, D2D1_COMBINE_MODE_EXCLUDE, None, 0.1, &sink)?;
                sink.Close()?;
                path.cast()
            })();
            if let Ok(path) = strip {
                rt.FillGeometry(&path, &b, None);
            }
        }
    }

    fn bitmap_of(&mut self, icon: HICON) -> Option<ID2D1Bitmap> {
        let key = icon.0 as usize;
        if let Some(b) = self.dev().icons.get(&key) {
            return Some(b.clone());
        }
        let b = self.icon_bitmap(icon)?;
        self.dev().icons.insert(key, b.clone());
        Some(b)
    }

    /// Draws a Windows icon into `r` with high-quality scaling. Bitmaps are cached per `HICON` until [`Canvas::forget_icon`].
    pub fn hicon(&mut self, icon: HICON, r: Rect, opacity: f32) {
        if icon.is_invalid() {
            return;
        }
        let Some(bmp) = self.bitmap_of(icon) else { return };
        let ctx = self.dev().ctx.clone();
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            ctx.DrawBitmap(&bmp, Some(&r.d2d()), opacity, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
        }
    }

    /// CSS `filter: drop-shadow(0 <dy>px <blur>px <c>)` of an icon: its own silhouette, blurred. Cached per icon and size.
    pub fn icon_shadow(&mut self, icon: HICON, r: Rect, dy: f32, blur: f32, c: Rgba) {
        if icon.is_invalid() || c.a <= 0.0 || r.w <= 0.0 {
            return;
        }
        let (sigma, k) = (blur / 2.0, icon.0 as usize);
        let pad = (sigma * 3.0).ceil();
        let key = [k as u32, (k >> 32) as u32, r.w.to_bits(), sigma.to_bits(), c.a.to_bits(), 1];
        let cached = self.dev().shadows.get(&key).cloned();
        let bmp = match cached {
            Some((b, _)) => b,
            None => {
                let Some(src) = self.bitmap_of(icon) else { return };
                let Some(b) = self.shadow_of(r.w, r.h, sigma, pad, c, |t| {
                    let ctx = t.cast::<ID2D1DeviceContext>().ok()?;
                    let dst = Rect::new(pad, pad, r.w, r.h);
                    // SAFETY: drawing into the off-screen target between its BeginDraw and EndDraw.
                    unsafe { ctx.DrawBitmap(&src, Some(&dst.d2d()), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None) };
                    Some(())
                }) else {
                    return;
                };
                let dev = self.dev();
                if dev.shadows.len() > 16 {
                    dev.shadows.clear();
                }
                dev.shadows.insert(key, (b.clone(), pad));
                b
            }
        };
        let ctx = self.dev().ctx.clone();
        let dst = Rect::new(r.x - pad, r.y - pad + dy, r.w + 2.0 * pad, r.h + 2.0 * pad);
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { ctx.DrawBitmap(&bmp, Some(&dst.d2d()), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
    }

    /// Drops the cached bitmap of an icon that is about to be destroyed.
    pub fn forget_icon(&mut self, icon: HICON) {
        self.dev().icons.remove(&(icon.0 as usize));
    }

    fn icon_bitmap(&mut self, icon: HICON) -> Option<ID2D1Bitmap> {
        let rt = self.rt();
        // SAFETY: WIC reads the icon's pixels; the converter yields premultiplied BGRA for D2D.
        unsafe {
            let src = self.g.wic.CreateBitmapFromHICON(icon).ok()?;
            let conv = self.g.wic.CreateFormatConverter().ok()?;
            conv.Initialize(&src, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
                .ok()?;
            rt.CreateBitmapFromWicBitmap(&conv, None).ok()
        }
    }

    /// CSS `box-shadow: 0 <dy>px <blur>px <c>` under a rounded rectangle. The blurred shape is cached per size.
    pub fn shadow(&mut self, r: Rect, radius: f32, dy: f32, blur: f32, c: Rgba) {
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let sigma = blur / 2.0; // CSS: the blur radius is twice the Gaussian's standard deviation
        let pad = (sigma * 3.0).ceil();
        let key = [r.w.to_bits(), r.h.to_bits(), radius.to_bits(), sigma.to_bits(), c.a.to_bits(), (c.r + c.g * 2.0 + c.b * 4.0).to_bits()];
        let cached = self.dev().shadows.get(&key).cloned();
        let (bmp, pad) = match cached {
            Some(v) => v,
            None => {
                let Some(b) = self.shadow_bitmap(r.w, r.h, radius, sigma, pad, c) else { return };
                let dev = self.dev();
                if dev.shadows.len() > 8 {
                    dev.shadows.clear(); // sizes animate; keep only recent ones
                }
                dev.shadows.insert(key, (b.clone(), pad));
                (b, pad)
            }
        };
        let ctx = self.dev().ctx.clone();
        let dst = Rect::new(r.x - pad, r.y - pad + dy, r.w + 2.0 * pad, r.h + 2.0 * pad);
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            ctx.DrawBitmap(&bmp, Some(&dst.d2d()), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
        }
    }

    fn shadow_bitmap(&mut self, w: f32, h: f32, radius: f32, sigma: f32, pad: f32, c: Rgba) -> Option<ID2D1Bitmap> {
        self.shadow_of(w, h, sigma, pad, c, |shape| {
            // SAFETY: drawing into the off-screen target between its BeginDraw and EndDraw.
            unsafe {
                let black = shape.CreateSolidColorBrush(&Rgba::new(0.0, 0.0, 0.0, 1.0).d2d(), None).ok()?;
                let rr = D2D_RECT_F { left: pad, top: pad, right: pad + w, bottom: pad + h };
                shape.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rr, radiusX: radius, radiusY: radius }, &black);
            }
            Some(())
        })
    }

    /// Blurs whatever `shape` draws (in a `w`x`h` box at `(pad, pad)` of a transparent target) into a tinted shadow bitmap.
    fn shadow_of(
        &mut self,
        w: f32,
        h: f32,
        sigma: f32,
        pad: f32,
        c: Rgba,
        shape: impl FnOnce(&ID2D1BitmapRenderTarget) -> Option<()>,
    ) -> Option<ID2D1Bitmap> {
        let (rt, ctx) = (self.rt(), self.dev().ctx.clone());
        let size = D2D_SIZE_F { width: w + 2.0 * pad, height: h + 2.0 * pad };
        // SAFETY: an off-screen target is drawn and read back within this frame's device.
        unsafe {
            let target = rt.CreateCompatibleRenderTarget(Some(&size), None, None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE).ok()?;
            target.BeginDraw();
            target.Clear(Some(&Rgba::TRANSPARENT.d2d()));
            shape(&target)?;
            target.EndDraw(None, None).ok()?;
            let input = target.GetBitmap().ok()?;

            let fx = ctx.CreateEffect(&CLSID_D2D1Shadow).ok()?;
            fx.SetInput(0, &input, true);
            fx.SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &sigma.to_le_bytes()).ok()?;
            let col: Vec<u8> = [c.r, c.g, c.b, c.a].iter().flat_map(|v| v.to_le_bytes()).collect();
            fx.SetValue(D2D1_SHADOW_PROP_COLOR.0 as u32, D2D1_PROPERTY_TYPE_VECTOR4, &col).ok()?;
            let out = fx.GetOutput().ok()?;

            let baked = rt.CreateCompatibleRenderTarget(Some(&size), None, None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE).ok()?;
            let bctx = baked.cast::<ID2D1DeviceContext>().ok()?;
            bctx.BeginDraw();
            bctx.Clear(Some(&Rgba::TRANSPARENT.d2d()));
            bctx.DrawImage(&out, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            bctx.EndDraw(None, None).ok()?;
            baked.GetBitmap().ok()
        }
    }

    /// Cuts a rounded rectangle out of what is drawn so far (a CSS outer `box-shadow` is not painted under its own box).
    pub fn erase(&mut self, r: Rect, radius: f32) {
        let ctx = self.dev().ctx.clone();
        let b = self.brush(Rgba::TRANSPARENT);
        // SAFETY: drawing inside BeginDraw/EndDraw; the blend mode is restored.
        unsafe {
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
            ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &b);
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
    }

    /// Runs `f` with everything it draws clipped to `r`.
    pub fn clipped(&mut self, r: Rect, f: impl FnOnce(&mut Self)) {
        let rt = self.rt();
        // SAFETY: the clip is popped right after `f`.
        unsafe { rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        f(self);
        // SAFETY: matches the PushAxisAlignedClip above.
        unsafe { rt.PopAxisAlignedClip() };
    }

    /// Runs `f` with everything it draws faded to `opacity` (CSS `opacity` on a group).
    pub fn faded(&mut self, opacity: f32, f: impl FnOnce(&mut Self)) {
        if opacity >= 0.999 {
            return f(self);
        }
        if opacity <= 0.001 {
            return;
        }
        let ctx = self.dev().ctx.clone();
        let params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
            maskTransform: Matrix3x2::identity(),
            opacity,
            ..Default::default()
        };
        // SAFETY: the layer is popped right after `f`.
        unsafe { ctx.PushLayer(&params, None) };
        f(self);
        // SAFETY: matches the PushLayer above.
        unsafe { ctx.PopLayer() };
    }

    /// The transform in force (what enclosing [`Canvas::transformed`] calls built up).
    fn world(&mut self) -> Matrix3x2 {
        let rt = self.rt();
        let mut m = Matrix3x2::identity();
        // SAFETY: reading the target's transform into a local.
        unsafe { rt.GetTransform(&mut m) };
        m
    }

    /// Draws `f` scaled by `k` around `(cx, cy)` and moved by `(dx, dy)` — CSS `transform: translate() scale()`. It composes
    /// with an enclosing call, so a panel's pose and an item's own motion add up.
    pub fn transformed(&mut self, cx: f32, cy: f32, k: f32, dx: f32, dy: f32, f: impl FnOnce(&mut Self)) {
        let m = Xf { cx, cy, k, dx, dy }.matrix();
        let (rt, cur) = (self.rt(), self.world());
        // SAFETY: the enclosing transform is restored after `f`.
        unsafe { rt.SetTransform(&(m * cur)) };
        f(self);
        // SAFETY: restoring the enclosing transform.
        unsafe { rt.SetTransform(&cur) };
    }

    /// Bakes what is on the screen behind a panel: `px` (`w`×`h` device pixels, opaque) blurred with a Gaussian of
    /// `sigma` DIPs (CSS `blur(r)`: `r` is the standard deviation) and saturated by `saturate` (CSS `saturate(k)`).
    pub fn glass(&mut self, px: &[u32], w: i32, h: i32, sigma: f32, saturate: f32) -> Option<Picture> {
        if w <= 0 || h <= 0 || px.len() < (w * h) as usize {
            return None;
        }
        let (rt, ctx, s) = (self.rt(), self.dev().ctx.clone(), self.g.scale);
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * s,
            dpiY: 96.0 * s,
        };
        let size = D2D_SIZE_F { width: w as f32 / s, height: h as f32 / s };
        // SAFETY: the pixels are read once by CreateBitmap; the effects and the baked target live within this call.
        unsafe {
            let src = rt.CreateBitmap(D2D_SIZE_U { width: w as u32, height: h as u32 }, Some(px.as_ptr().cast()), (w * 4) as u32, &props).ok()?;
            let blur = ctx.CreateEffect(&CLSID_D2D1GaussianBlur).ok()?;
            blur.SetInput(0, &src, true);
            blur.SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &sigma.to_le_bytes()).ok()?;
            let mut out: ID2D1Image = blur.GetOutput().ok()?;
            if (saturate - 1.0).abs() > 1e-3 {
                // CSS saturate(s); the rows are the input channels (D2D multiplies a row vector)
                let (a, b, c) = (0.213, 0.715, 0.072);
                let m = [
                    a + (1.0 - a) * saturate, a - a * saturate, a - a * saturate, 0.0,
                    b - b * saturate, b + (1.0 - b) * saturate, b - b * saturate, 0.0,
                    c - c * saturate, c - c * saturate, c + (1.0 - c) * saturate, 0.0,
                    0.0, 0.0, 0.0, 1.0,
                    0.0, 0.0, 0.0, 0.0,
                ];
                let bytes: Vec<u8> = m.iter().flat_map(|v: &f32| v.to_le_bytes()).collect();
                let cm = ctx.CreateEffect(&CLSID_D2D1ColorMatrix).ok()?;
                cm.SetInput(0, &out, true);
                cm.SetValue(D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32, D2D1_PROPERTY_TYPE_MATRIX_5X4, &bytes).ok()?;
                out = cm.GetOutput().ok()?;
            }
            let baked = rt.CreateCompatibleRenderTarget(Some(&size), None, None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE).ok()?;
            let bctx = baked.cast::<ID2D1DeviceContext>().ok()?;
            bctx.BeginDraw();
            bctx.Clear(Some(&Rgba::TRANSPARENT.d2d()));
            bctx.DrawImage(&out, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            bctx.EndDraw(None, None).ok()?;
            Some(Picture { bmp: baked.GetBitmap().ok()?, w: size.width, h: size.height })
        }
    }

    /// An image file (JPEG, PNG, ...) scaled to cover `w` × `h` DIPs, as a picture to draw with [`Canvas::picture`]; it is
    /// centred, so what sticks out of the box is cut by the caller (`CSS object-fit: cover`). `None` if it cannot be read.
    pub fn image(&mut self, path: &str, w: f32, h: f32) -> Option<Picture> {
        use windows::Win32::Graphics::Imaging::{WICBitmapInterpolationModeFant, WICDecodeMetadataCacheOnDemand};
        let (rt, s) = (self.rt(), self.g.scale);
        // SAFETY: WIC decodes and scales into a bitmap that the converter hands to D2D; every object is local to this call.
        unsafe {
            let dec = self.g.wic.CreateDecoderFromFilename(&HSTRING::from(path), None, windows::Win32::Foundation::GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
            let frame = dec.GetFrame(0).ok()?;
            let (mut iw, mut ih) = (0u32, 0u32);
            frame.GetSize(&mut iw, &mut ih).ok()?;
            if iw == 0 || ih == 0 {
                return None;
            }
            let k = ((w * s) / iw as f32).max((h * s) / ih as f32);
            let (pw, ph) = (((iw as f32 * k).ceil() as u32).max(1), ((ih as f32 * k).ceil() as u32).max(1));
            let scaler = self.g.wic.CreateBitmapScaler().ok()?;
            scaler.Initialize(&frame, pw, ph, WICBitmapInterpolationModeFant).ok()?;
            let conv = self.g.wic.CreateFormatConverter().ok()?;
            conv.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
            let bmp = rt.CreateBitmapFromWicBitmap(&conv, None).ok()?;
            Some(Picture { bmp, w: pw as f32 / s, h: ph as f32 / s })
        }
    }

    /// Draws a baked picture with its top left corner at `(x, y)`, pixel for pixel.
    pub fn picture(&mut self, p: &Picture, x: f32, y: f32) {
        let ctx = self.dev().ctx.clone();
        let dst = Rect::new(x, y, p.w, p.h);
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { ctx.DrawBitmap(&p.bmp, Some(&dst.d2d()), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None) };
    }

    /// Runs `f` with everything it draws clipped to the rounded rectangle `r` as `xf` moves it (CSS `clip-path: inset(..
    /// round ..)` on an element with a transform). Call it with no transform in force: the shape is in window coordinates.
    pub fn masked(&mut self, r: Rect, radius: f32, xf: Xf, f: impl FnOnce(&mut Self)) {
        let ctx = self.dev().ctx.clone();
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius.max(0.0), radiusY: radius.max(0.0) };
        // SAFETY: plain geometry creation on the factory.
        let mask = unsafe {
            self.g
                .factory
                .CreateRoundedRectangleGeometry(&rr)
                .and_then(|geo| self.g.factory.CreateTransformedGeometry(&geo, &xf.matrix()))
                .and_then(|t| t.cast::<ID2D1Geometry>())
                .ok()
        };
        let Some(mask) = mask else { return f(self) };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
            geometricMask: std::mem::ManuallyDrop::new(Some(mask)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        // SAFETY: the layer is popped right after `f`; the mask reference we hold is released once it is gone.
        unsafe { ctx.PushLayer(&params, None) };
        f(self);
        // SAFETY: matches the PushLayer above; the geometry reference is dropped exactly once.
        unsafe {
            ctx.PopLayer();
            std::mem::ManuallyDrop::drop(&mut params.geometricMask);
        }
    }
}
