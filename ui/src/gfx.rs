//! Direct2D + DirectWrite drawing, shown through DirectComposition (or, as a fallback, layered windows).
//!
//! Everything is drawn in DIPs (the design's CSS pixels): the context's DPI is `96 * scale`, so the numbers from the
//! Obsidian mockup are used as they are, and D2D scales strokes, text and blur.
//!
//! One D3D11 device, one D2D device and one DirectComposition device serve the whole thread (the adapter the bar's monitor is
//! plugged into; WARP if that fails). Every [`Gfx`] draws with a device context of its own on that device, so bitmaps and
//! pictures are shared and the bar, the panels, the tip and the toast cost one device, not four.
//!
//! A [`Surface`] is one window's backing store, in one of two forms:
//! * a DirectComposition surface ([`Surface::composed`], windows made with [`window_ex`]): D2D draws into it on the GPU,
//!   [`Surface::present`] sets it as the window's visual with a whole-window opacity, and nothing is copied. Such a window has no
//!   per-pixel hit test, so what must not catch the mouse is cut out of it with [`shape`] (a window region);
//! * a 32-bit premultiplied DIB ([`Surface::new`]): the frame is drawn on the GPU, read back, and [`Surface::present`] hands it to
//!   `UpdateLayeredWindow` (transparent pixels click through). `--render`, `--render-panel`, the screen grab behind a panel's
//!   glass and the fallback without DirectComposition (`WINRUST_ULW=1`, or no DComp on this machine) use this form.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static BENCH_END: std::cell::Cell<Option<std::time::Duration>> = const { std::cell::Cell::new(None) };
}

pub fn bench_start() { BENCH_END.set(Some(std::time::Duration::ZERO)); }
pub fn bench_end() -> std::time::Duration { BENCH_END.replace(None).unwrap_or_default() }

use windows::Win32::Foundation::{COLORREF, E_FAIL, HMODULE, HWND, POINT, SIZE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionSurface, IDCompositionTarget, IDCompositionVisual3};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIDevice, IDXGIDevice3, IDXGIFactory1};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, CLSID_D2D1Shadow, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_PROPERTIES,
    D2D1_COLORMATRIX_PROP_COLOR_MATRIX, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    D2D1_LAYER_PARAMETERS1, D2D1_PROPERTY_TYPE_MATRIX_5X4, ID2D1Geometry, ID2D1Image, D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_COMBINE_MODE_EXCLUDE, D2D1_COMBINE_MODE_INTERSECT,
    D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE, D2D1_DASH_STYLE_CUSTOM, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINE_JOIN_MITER, D2D1_LINE_JOIN_ROUND,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_PROPERTY_TYPE_FLOAT, D2D1_PROPERTY_TYPE_VECTOR4,
    D2D1_ROUNDED_RECT, D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION,
    D2D1_SHADOW_PROP_COLOR, D2D1_STROKE_STYLE_PROPERTIES1, D2D1_STROKE_TRANSFORM_TYPE_NORMAL, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, D2D1_PRIMITIVE_BLEND_COPY, D2D1_PRIMITIVE_BLEND_SOURCE_OVER, ID2D1Bitmap, ID2D1Bitmap1, ID2D1BitmapRenderTarget, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1GradientStopCollection, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle1, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_MAP_OPTIONS_READ,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_FONT_AXIS_TAG_OPTICAL_SIZE, DWRITE_FONT_AXIS_TAG_WEIGHT, DWRITE_FONT_AXIS_VALUE, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC,
    DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, DWRITE_HIT_TEST_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteFactory6,
    IDWriteFontCollection2, IDWriteFontSetBuilder1, IDWriteTextFormat, IDWriteTextLayout1,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt, CAPTUREBLT, CombineRgn, CreateCompatibleDC,
    CreateDIBSection, CreateRectRgn, CreateRoundRectRgn, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, HRGN,
    MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint, RGN_OR, ROP_CODE, ReleaseDC, SRCCOPY, SelectObject, SetWindowRgn,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeMedianCut,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HICON, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOREDRAW, SWP_NOZORDER, SetWindowPos, ULW_ALPHA, UpdateLayeredWindow,
    LWA_ALPHA, SetLayeredWindowAttributes, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{HSTRING, IUnknown, Interface, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::svg::{self, Seg};
use winrust_common::style;

/// A straight-alpha colour (D2D premultiplies for the target). A colour written as one of the design's dark literals (`css`, `hex`)
/// or as the ink (`white`) also remembers which colour token it is: while light mode is on, it is drawn as the token's light value
/// (`style::TOKENS`), with its own alpha. `r`, `g`, `b` are always the dark-mode value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    tok: u8,
    /// `0x0100_0000 | 0xRRGGBB`: the light-mode value an app wrote beside the dark one ([`Rgba::pair`]); 0 = none.
    light: u32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0.0, 0.0, 0.0, 0.0);

    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a, tok: 0, light: 0 }
    }

    /// An app's own colour as the mockup's CSS writes it: the dark value and the light one (`[data-theme=light]{--win:..}`),
    /// `0xRRGGBB` each. It needs no entry in `style::TOKENS`, so apps do not touch the bar's table.
    #[must_use]
    pub const fn pair(dark: u32, light: u32) -> Self {
        let c = Self::hex_plain(dark);
        Self { light: 0x0100_0000 | (light & 0x00ff_ffff), ..c }
    }

    const fn hex_plain(rgb: u32) -> Self {
        Self::new(((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0, 1.0)
    }

    /// `0xRRGGBB` as written in the design.
    #[must_use]
    pub const fn hex(rgb: u32) -> Self {
        let c = Self::new(((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0, 1.0);
        Self { tok: style::token((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8), ..c }
    }

    /// CSS `rgba(r, g, b, a)` as written in the design.
    #[must_use]
    pub const fn css(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { tok: style::token(r, g, b), ..Self::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a) }
    }

    /// A GDI `COLORREF` (`0x00BBGGRR`, what `winrust_common::style::pal()` hands out).
    #[must_use]
    pub const fn colorref(c: u32) -> Self {
        Self::new((c & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, ((c >> 16) & 255) as f32 / 255.0, 1.0)
    }

    /// The ink at `a`: every hover and pressed fill of the design is `rgba(var(--ink),a)`, white in dark mode, near-black in light.
    #[must_use]
    pub const fn white(a: f32) -> Self {
        Self { tok: style::INK_TOKEN, ..Self::new(1.0, 1.0, 1.0, a) }
    }

    /// Selected labels: white in dark mode, `--t1` in light (the design's `#fff` that became a text token).
    #[must_use]
    pub const fn strong(a: f32) -> Self {
        Self { tok: style::STRONG_TOKEN, ..Self::new(1.0, 1.0, 1.0, a) }
    }

    /// White that stays white in both modes (`#fff` on an accent fill, the recording's red): the design's few literal whites.
    #[must_use]
    pub const fn pure_white(a: f32) -> Self {
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

    /// Linear blend of what is drawn (the mode's values); `t = 0` is `self`.
    #[must_use]
    pub fn mix(self, o: Rgba, t: f32) -> Self {
        let (s, o) = (self.themed(), o.themed());
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self::new(l(s.r, o.r), l(s.g, o.g), l(s.b, o.b), l(s.a, o.a))
    }

    /// The colour as it is drawn now: a token's value in the current mode, the colour itself when it has none; no token left.
    #[must_use]
    pub fn themed(self) -> Self {
        if style::light() { self.in_light() } else { Self { tok: 0, light: 0, ..self } }
    }

    /// The colour in light mode (what `themed` gives while it is on).
    #[must_use]
    pub fn in_light(self) -> Self {
        if self.light != 0 {
            let v = self.light;
            return Self::new(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, self.a);
        }
        if self.tok == 0 {
            return self;
        }
        let v = style::TOKENS[self.tok as usize - 1].1;
        Self::new(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, self.a)
    }

    fn d2d(self) -> D2D1_COLOR_F {
        let c = self.themed();
        D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
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
static INTER: &[u8] = include_bytes!("../fonts/InterVariable.ttf");
const INTER_FAMILY: &str = "Inter Variable";

/// Fallbacks when the system cannot load a variable font from memory (DirectWrite before Windows 10 1709).
const FACES: [&str; 3] = ["Inter", "Segoe UI Variable Text", "Segoe UI"];

/// A window's backing store: a DirectComposition surface ([`Surface::composed`]) or a top-down 32-bit premultiplied DIB
/// selected into its own memory DC ([`Surface::new`]).
pub struct Surface {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
    /// The GPU side of painting a DIB.
    gpu: Option<Gpu>,
    comp: Option<Comp>,
    composed: bool,
}

/// What a DIB frame is drawn into on the GPU, and the CPU-readable copy it is read back through.
struct Gpu {
    target: ID2D1Bitmap1,
    copy: ID2D1Bitmap1,
    epoch: u32,
    w: i32,
    h: i32,
}

/// The DirectComposition surface of a window, and which window's visual shows it.
struct Comp {
    surf: IDCompositionSurface,
    w: i32,
    h: i32,
    epoch: u32,
    bound: Cell<Option<isize>>,
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
            gpu: None,
            comp: None,
            composed: false,
        }
    }

    /// The surface of a window made with [`window_ex`]: a DirectComposition surface while [`compose_windows`] has switched
    /// composition on, the DIB of a layered window otherwise.
    #[must_use]
    pub fn composed() -> Self {
        let mut s = Self::new();
        s.composed = composing();
        s
    }

    /// Size in device pixels.
    #[must_use]
    pub fn size(&self) -> (i32, i32) {
        self.comp.as_ref().map_or((self.w, self.h), |c| (c.w, c.h))
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

    /// The last frame's premultiplied BGRA pixels, row by row (empty before the first paint, and for a composed surface:
    /// those pixels live on the GPU).
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

    /// Whether what was painted into it is still there. A lost device takes a composed surface's pixels with it; whoever paints
    /// a surface only when its look changes must paint it again then.
    #[must_use]
    pub fn is_current(&self) -> bool {
        match &self.comp {
            Some(c) => current_dev().is_some_and(|d| d.epoch == c.epoch),
            None => !self.composed && !self.dc.is_invalid(),
        }
    }

    /// Gives the memory back (a screen grab that has been blurred into a picture is not needed any more).
    pub fn free(&mut self) {
        self.gpu = None;
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
    /// A composed surface also resizes the window to the frame; what reaches the screen is cut by the next [`commit`].
    pub fn present(&self, hwnd: HWND, x: i32, y: i32, alpha: u8) -> bool {
        if self.composed {
            return self.present_comp(hwnd, x, y, alpha);
        }
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

    /// Puts the last painted frame on a window that is not moved by it (an application window: its size is the user's, the
    /// surface follows it): only the visual's content is set. A DirectComposition target's origin is the window's client area,
    /// so a frame of the client's size fits. What reaches the screen is cut by the next [`commit`]. False for a DIB surface
    /// (see [`Surface::blit`]) or a lost device.
    pub fn show(&self, hwnd: HWND) -> bool {
        let Some(c) = self.comp.as_ref().filter(|_| self.composed) else { return false };
        let Some(dev) = current_dev().filter(|d| d.epoch == c.epoch) else { return false };
        let key = hwnd.0 as isize;
        let mut targets = dev.targets.borrow_mut();
        let tgt = match targets.entry(key) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => match make_target(&dev, hwnd) {
                Ok(t) => v.insert(t),
                Err(_) => return false,
            },
        };
        let mut ok = true;
        // SAFETY: calls on our own visual; the surface is alive for the whole call.
        unsafe {
            let id = c.surf.as_raw() as usize;
            if tgt.content != id {
                ok &= tgt.visual.SetContent(&c.surf).is_ok();
                tgt.content = id;
            }
        }
        c.bound.set(Some(key));
        ok
    }

    /// Copies the last painted DIB frame to a window's device context at `(x, y)` (the fallback without DirectComposition:
    /// a `WM_PAINT` of an ordinary window).
    pub fn blit(&self, hdc: HDC, x: i32, y: i32) -> bool {
        if self.dc.is_invalid() {
            return false;
        }
        // SAFETY: `self.dc` holds a live DIB of `w` by `h` pixels; `hdc` is the caller's.
        unsafe { BitBlt(hdc, x, y, self.w, self.h, Some(self.dc), 0, 0, SRCCOPY).is_ok() }
    }

    fn present_comp(&self, hwnd: HWND, x: i32, y: i32, alpha: u8) -> bool {
        let Some(c) = &self.comp else { return false };
        let Some(dev) = current_dev().filter(|d| d.epoch == c.epoch) else { return false };
        let key = hwnd.0 as isize;
        let mut targets = dev.targets.borrow_mut();
        let tgt = match targets.entry(key) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => match make_target(&dev, hwnd) {
                Ok(t) => v.insert(t),
                Err(_) => return false,
            },
        };
        let mut ok = true;
        // SAFETY: calls on our own visual and window; the surface is alive for the whole call.
        unsafe {
            let id = c.surf.as_raw() as usize;
            if tgt.content != id {
                ok &= tgt.visual.SetContent(&c.surf).is_ok();
                tgt.content = id;
            }
            if tgt.opacity != alpha {
                ok &= tgt.visual.SetOpacity2(f32::from(alpha) / 255.0).is_ok();
                tgt.opacity = alpha;
            }
            let rect = (x, y, c.w, c.h);
            if tgt.rect != rect {
                let _ = SetWindowPos(hwnd, None, x, y, c.w, c.h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOREDRAW);
                tgt.rect = rect;
            }
        }
        c.bound.set(Some(key));
        ok
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

impl Drop for Comp {
    /// A surface that goes away takes itself off its window's visual (the next [`commit`] shows it), so a closed panel gives
    /// its GPU memory back at once.
    fn drop(&mut self) {
        let Some(window) = self.bound.get() else { return };
        let id = self.surf.as_raw() as usize;
        let _ = DEV.try_with(|d| {
            let Ok(d) = d.try_borrow() else { return };
            let Some(dev) = d.as_ref().filter(|dev| dev.epoch == self.epoch) else { return };
            let Ok(mut targets) = dev.targets.try_borrow_mut() else { return };
            if let Some(t) = targets.get_mut(&window).filter(|t| t.content == id) {
                // SAFETY: our own visual.
                unsafe {
                    let _ = t.visual.SetContent(None::<&IUnknown>);
                }
                t.content = 0;
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the devices of the thread

/// What survives a lost device: factories, and what hangs on them.
struct Shared {
    factory: ID2D1Factory1,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    round: ID2D1StrokeStyle1,
    /// The built-in Inter, when the system can use it.
    inter: Option<(IDWriteFactory6, IDWriteFontCollection2)>,
    face: &'static str,
}

/// The D3D11, D2D and DirectComposition devices; replaced as a whole when the GPU is lost.
struct Dev {
    epoch: u32,
    d3d: ID3D11Device,
    d2d: ID2D1Device,
    dcomp: Option<IDCompositionDesktopDevice>,
    targets: RefCell<HashMap<isize, Tgt>>,
}

/// What a window of ours shows through DirectComposition, and what was last told to it (calls with nothing new are skipped).
struct Tgt {
    _target: IDCompositionTarget,
    visual: IDCompositionVisual3,
    content: usize,
    opacity: u8,
    rect: (i32, i32, i32, i32),
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
    static DEV: RefCell<Option<Rc<Dev>>> = const { RefCell::new(None) };
    static GEN: Cell<u32> = const { Cell::new(1) };
    static COMPOSE: Cell<bool> = const { Cell::new(false) };
    static SHAPES: RefCell<HashMap<isize, Option<Vec<Rgn>>>> = RefCell::new(HashMap::new());
}

fn shared() -> Result<Rc<Shared>> {
    if let Some(s) = SHARED.with_borrow(Clone::clone) {
        return Ok(s);
    }
    // SAFETY: factory creation has no preconditions beyond COM for WIC.
    let s = unsafe {
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
        Rc::new(Shared { factory, dwrite, wic, round, inter, face })
    };
    SHARED.set(Some(s.clone()));
    Ok(s)
}

fn current_dev() -> Option<Rc<Dev>> {
    DEV.with_borrow(Clone::clone)
}

/// The device of this thread, made on first use.
fn dev() -> Result<Rc<Dev>> {
    if let Some(d) = current_dev() {
        return Ok(d);
    }
    let sh = shared()?;
    let d = Rc::new(create_dev(&sh.factory, composing())?);
    DEV.set(Some(d.clone()));
    Ok(d)
}

/// The adapter the primary monitor (where the bar lives) is plugged into: frames for it need no copy between adapters.
fn primary_adapter() -> Option<IDXGIAdapter> {
    // SAFETY: plain DXGI enumeration; every object is local.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            let mut o = 0;
            while let Ok(out) = adapter.EnumOutputs(o) {
                if out.GetDesc().is_ok_and(|d| d.Monitor == monitor) {
                    return adapter.cast().ok();
                }
                o += 1;
            }
            i += 1;
        }
        None
    }
}

fn create_dev(factory: &ID2D1Factory1, compose: bool) -> Result<Dev> {
    let mut d3d = None;
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    // SAFETY: device creation with out-params that are locals; the first adapter that works wins, WARP is the last resort.
    unsafe {
        if let Some(a) = primary_adapter() {
            let _ = D3D11CreateDevice(&a, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None);
        }
        if d3d.is_none() {
            let _ = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None);
        }
        if d3d.is_none() {
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None)?;
        }
        let d3d = d3d.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let d2d = factory.CreateDevice(&d3d.cast::<IDXGIDevice>()?)?;
        let dcomp = if compose { Some(DCompositionCreateDevice2::<_, IDCompositionDesktopDevice>(&d2d)?) } else { None };
        let epoch = GEN.get();
        Ok(Dev { epoch, d3d, d2d, dcomp, targets: RefCell::new(HashMap::new()) })
    }
}

/// The GPU went away (or a test says so): every device-bound thing is rebuilt on its next frame.
pub fn lose_device() {
    GEN.set(GEN.get() + 1);
    DEV.set(None);
}

/// A failure that means the device is gone and must be made anew.
fn device_gone(e: &windows::core::Error) -> bool {
    // DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, D2DERR_RECREATE_TARGET
    matches!(e.code().0 as u32, 0x887A_0005 | 0x887A_0007 | 0x8899_000C)
}

/// Whether the frames of the bar and its panels go through DirectComposition.
#[must_use]
pub fn composing() -> bool {
    COMPOSE.get()
}

/// The same for the thread that paces the frames: a layered window costs a readback and a copy per frame, so frames for it come
/// at 60 Hz, not on every tick of the screen.
static COMPOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[must_use]
pub fn composed_anywhere() -> bool {
    COMPOSED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switches the windows to DirectComposition if this machine can (call it before any of them is made, on the UI thread).
/// `WINRUST_ULW=1` keeps the layered-window path; so does a machine where a DirectComposition target cannot be made.
pub fn compose_windows() -> bool {
    if std::env::var_os("WINRUST_ULW").is_some_and(|v| v != "0") {
        return false;
    }
    COMPOSE.set(true);
    if current_dev().is_some_and(|d| d.dcomp.is_none()) {
        lose_device(); // made before composition was asked for: made again with the composition device
    }
    let ok = dev().is_ok_and(|d| probe_target(&d));
    if !ok {
        COMPOSE.set(false);
        DEV.set(None);
    }
    COMPOSED.store(ok, std::sync::atomic::Ordering::Relaxed);
    ok
}

/// Surfaces made from now on are DIBs again (the self-test draws its reference with them).
pub fn compose_off() {
    COMPOSE.set(false);
}

/// Can a window of the kind we make take a DirectComposition target? Tried on a hidden 1x1 window.
fn probe_target(d: &Dev) -> bool {
    let Some(dc) = d.dcomp.as_ref() else { return false };
    // SAFETY: a window of the system's STATIC class, made and destroyed here.
    unsafe {
        let Ok(w) = CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW, w!("STATIC"), None, WS_POPUP, 0, 0, 1, 1, None, None, None, None) else { return false };
        let ok = dc.CreateTargetForHwnd(w, true).and_then(|t| dc.CreateVisual().and_then(|v| t.SetRoot(&v))).is_ok();
        let _ = DestroyWindow(w);
        ok
    }
}

/// The extended style a window of ours is made with: no redirection bitmap when composed (DirectComposition draws it), a
/// layered window otherwise.
#[must_use]
pub fn window_ex() -> u32 {
    (if composing() { WS_EX_NOREDIRECTIONBITMAP } else { WS_EX_LAYERED }).0
}

/// The extended style of a window that must not catch the mouse at all (a shadow, a scrim, the tip, the toast). A composed
/// window that is merely `WS_EX_TRANSPARENT` still takes the pointer from other processes' windows; layered too, and given a
/// full opacity with [`through`], it does not (the same as the layered window it replaces).
#[must_use]
pub fn window_ex_through() -> u32 {
    window_ex() | WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0
}

/// Makes a window made with [`window_ex_through`] live: once it is composed, a layered window shows nothing until it has its
/// attributes (and a layered window painted by `UpdateLayeredWindow` must not have them set, so the fallback is left alone).
pub fn through(hwnd: HWND) {
    if composing() {
        // SAFETY: a window of ours.
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        }
    }
}

fn make_target(dev: &Dev, hwnd: HWND) -> Result<Tgt> {
    let dc = dev.dcomp.as_ref().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    // SAFETY: a target on our own window and a visual that is its root.
    unsafe {
        let target = dc.CreateTargetForHwnd(hwnd, true)?;
        let visual = dc.CreateVisual()?;
        target.SetRoot(&visual)?;
        Ok(Tgt { _target: target, visual: visual.cast()?, content: 0, opacity: 255, rect: (i32::MIN, 0, 0, 0) })
    }
}

/// How much video memory this process uses on the device's adapter, MB: (local, non-local). For the benchmark and self-test.
pub fn gpu_memory() -> Option<(f64, f64)> {
    use windows::Win32::Graphics::Dxgi::{DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, IDXGIAdapter3};
    let d = current_dev()?;
    // SAFETY: plain queries on the device's adapter.
    unsafe {
        let a: IDXGIAdapter3 = d.d3d.cast::<IDXGIDevice>().ok()?.GetAdapter().ok()?.cast().ok()?;
        let use_of = |g| {
            let mut i = windows::Win32::Graphics::Dxgi::DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            a.QueryVideoMemoryInfo(0, g, &mut i).map(|()| i.CurrentUsage as f64 / 1_048_576.0)
        };
        Some((use_of(DXGI_MEMORY_SEGMENT_GROUP_LOCAL).ok()?, use_of(DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL).ok()?))
    }
}

/// Waits until the GPU has finished everything queued so far and says how long that took (the benchmark's measure of what a
/// frame still costs the GPU after the CPU is done with it). Never used by the bar itself.
pub fn gpu_wait() -> std::time::Duration {
    use windows::Win32::Graphics::Direct3D11::{D3D11_QUERY_DESC, D3D11_QUERY_EVENT, ID3D11Query};
    let Some(d) = current_dev() else { return std::time::Duration::ZERO };
    // SAFETY: an event query on the thread's device, polled with the raw vtable because `GetData` answers S_FALSE while the
    // work is still running and the bindings fold that into success.
    unsafe {
        let Ok(ctx) = d.d3d.GetImmediateContext() else { return std::time::Duration::ZERO };
        let mut q: Option<ID3D11Query> = None;
        if d.d3d.CreateQuery(&D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 }, Some(&mut q)).is_err() {
            return std::time::Duration::ZERO;
        }
        let Some(q) = q else { return std::time::Duration::ZERO };
        ctx.End(&q);
        ctx.Flush();
        let started = std::time::Instant::now();
        while (Interface::vtable(&ctx).GetData)(Interface::as_raw(&ctx), Interface::as_raw(&q), std::ptr::null_mut(), 0, 0).0 != 0 {
            std::hint::spin_loop();
        }
        started.elapsed()
    }
}

/// Gives back what the devices cache (the memory of panels that are closed): once a panel is gone.
pub fn trim() {
    let Some(d) = current_dev() else { return };
    // SAFETY: plain calls on the devices.
    unsafe {
        d.d2d.ClearResources(0);
        if let Ok(dxgi) = d.d3d.cast::<IDXGIDevice3>() {
            dxgi.Trim();
        }
    }
}

/// Everything that was painted and presented since the last call reaches the screen together. Once per frame of an owner.
pub fn commit() {
    let Some(d) = current_dev() else { return };
    let Some(dc) = d.dcomp.as_ref() else { return };
    // SAFETY: plain calls on the devices.
    let lost = unsafe { dc.Commit().is_err_and(|e| device_gone(&e)) || d.d3d.GetDeviceRemovedReason().is_err() };
    if lost {
        lose_device();
    }
}

/// A rectangle of a window's click region, device pixels in window coordinates; `r` rounds its corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgn {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub r: i32,
}

/// Cuts a composed window to the union of `parts` (what is outside neither shows nor catches the mouse) or, with `None`, gives
/// it back whole. A rounded part is one pixel wider on every side, so the smooth edge of what is drawn is not clipped. Only when
/// the shape changed. Layered windows (`WINRUST_ULW=1`) are cut too: their faint shadow pixels are not transparent enough to
/// click through, so a control with a wide shadow margin would swallow the clicks round it.
pub fn shape(hwnd: HWND, parts: Option<&[Rgn]>) {
    let (key, parts) = (hwnd.0 as isize, parts.filter(|p| !p.is_empty()).map(<[Rgn]>::to_vec));
    if SHAPES.with_borrow(|m| m.get(&key) == Some(&parts)) {
        return;
    }
    // SAFETY: regions are made here and handed to the window, which owns them from then on (freed again if it refuses).
    unsafe {
        let rgn = parts.as_deref().map(|p| {
            let mut acc = HRGN::default();
            for r in p {
                let part = if r.r > 0 { CreateRoundRectRgn(r.x - 1, r.y - 1, r.x + r.w + 2, r.y + r.h + 2, 2 * r.r + 2, 2 * r.r + 2) } else { CreateRectRgn(r.x, r.y, r.x + r.w, r.y + r.h) };
                if acc.is_invalid() {
                    acc = part;
                } else {
                    CombineRgn(Some(acc), Some(acc), Some(part), RGN_OR);
                    let _ = DeleteObject(part.into());
                }
            }
            acc
        });
        if SetWindowRgn(hwnd, rgn, false) == 0
            && let Some(r) = rgn
        {
            let _ = DeleteObject(r.into());
        }
    }
    SHAPES.with_borrow_mut(|m| m.insert(key, parts));
}

/// A baked picture that stays in the render target's memory: the blurred desktop behind a panel. It belongs to the device
/// it was made on, so a panel makes a new one each time it opens.
pub struct Picture {
    bmp: ID2D1Bitmap,
    /// The device it was made on: after a lost device it is not drawn.
    epoch: u32,
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

/// What one [`Gfx`] keeps on the device: its own device context (resources made on any context of the device are shared), the
/// context the frame in progress is drawn with, and the bitmaps cached for it. Rebuilt when the device is.
struct Device {
    epoch: u32,
    /// The context that makes resources and draws DIB frames.
    ctx: ID2D1DeviceContext,
    /// What `ctx` targets between DIB frames: an off-screen target made from a context takes its pixel format from the
    /// context's target, so it must always have one.
    idle: ID2D1Bitmap1,
    /// The context of the frame in progress: `ctx`, or the one a DirectComposition surface hands out.
    cur: ID2D1DeviceContext,
    /// Where the frame's origin is on its target (a DirectComposition surface sits somewhere inside a shared texture), DIPs.
    base: Matrix3x2,
    brush: ID2D1SolidColorBrush,
    icons: HashMap<usize, ID2D1Bitmap>,
    shadows: HashMap<[u32; 6], (ID2D1Bitmap, f32)>,
    /// The last number of `RETIRED` this device has applied to `icons` and `shadows`.
    retired: u64,
    /// Gradient stop collections by their stops (a separator's gradient is the same every frame).
    grads: HashMap<Vec<u32>, ID2D1GradientStopCollection>,
}

/// Icons destroyed by their owners, numbered in order. A handle value is reused by the next icon the system creates, so a cache
/// keyed by it must drop an icon's bitmaps before that: every device applies what it has not seen at its next icon lookup.
static RETIRED: std::sync::Mutex<Vec<(u64, usize)>> = std::sync::Mutex::new(Vec::new());

/// An icon is about to be destroyed (call this before `DestroyIcon`, from any thread): its cached bitmaps and shadow go.
pub fn retire_icon(icon: HICON) {
    let mut r = RETIRED.lock().unwrap();
    let n = r.last().map_or(1, |e| e.0 + 1);
    r.push((n, icon.0 as usize));
    if r.len() > 512 {
        r.remove(0);
    }
}

fn drop_icon(d: &mut Device, key: usize) {
    d.icons.remove(&key);
    d.shadows.retain(|k, _| !(k[5] == 1 && (k[0] as usize | (k[1] as usize) << 32) == key)); // the icon's silhouettes (`icon_shadow`)
}

/// How many text layouts a generation of the cache holds (see `Gfx::layouts`).
const LAYOUTS_PER_GENERATION: usize = 2048;

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
    /// Two generations of text layouts: when `layouts` fills up it becomes `layouts_old` and the older one is let go, so what a
    /// list scrolled past a moment ago is still there, and what is in use is never dropped all at once.
    layouts: HashMap<LayoutKey, IDWriteTextLayout1>,
    layouts_old: HashMap<LayoutKey, IDWriteTextLayout1>,
    /// The built-in Inter, when the system can use it.
    inter: Option<(IDWriteFactory6, IDWriteFontCollection2)>,
    face: &'static str,
    scale: f32,
}

impl Gfx {
    /// Needs COM initialised on this thread (WIC).
    pub fn new(scale: f32) -> Result<Self> {
        let s = shared()?;
        Ok(Self {
            factory: s.factory.clone(),
            dwrite: s.dwrite.clone(),
            wic: s.wic.clone(),
            round: s.round.clone(),
            dev: None,
            paths: HashMap::new(),
            formats: HashMap::new(),
            layouts: HashMap::new(),
            layouts_old: HashMap::new(),
            inter: s.inter.clone(),
            face: s.face,
            scale,
        })
    }

    /// Device pixels per DIP.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// The window moved to a screen of another DPI: the next frame is drawn at the new scale. What was made for the old one (the
    /// device context, the bitmaps cached for it) goes; layouts and paths are in DIPs and stay.
    pub fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.dev = None;
        }
    }

    /// Drops every cached icon bitmap (a panel that drew icons it does not own, whose handles may be reused).
    pub fn forget_icons(&mut self) {
        if let Some(d) = self.dev.as_mut() {
            d.icons.clear();
            d.shadows.retain(|k, _| k[5] != 1);
        }
    }

    /// DIPs to whole device pixels.
    #[must_use]
    pub fn px(&self, dip: f32) -> i32 {
        (dip * self.scale).round() as i32
    }

    fn device(&mut self) -> Result<&mut Device> {
        let d = dev()?;
        if self.dev.as_ref().is_none_or(|x| x.epoch != d.epoch) {
            // SAFETY: a context and a brush on the thread's device; the objects are owned by `Device`.
            let dev = unsafe {
                let ctx = d.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
                ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE); // ClearType needs an opaque target
                ctx.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                ctx.SetDpi(96.0 * self.scale, 96.0 * self.scale);
                let brush = ctx.CreateSolidColorBrush(&Rgba::TRANSPARENT.d2d(), None)?;
                let idle = ctx.CreateBitmap(
                    D2D_SIZE_U { width: 1, height: 1 },
                    None,
                    0,
                    &D2D1_BITMAP_PROPERTIES1 {
                        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                        ..Default::default()
                    },
                )?;
                ctx.SetTarget(&idle);
                Device { epoch: d.epoch, cur: ctx.clone(), ctx, idle, base: Matrix3x2::identity(), brush, icons: HashMap::new(), shadows: HashMap::new(), retired: 0, grads: HashMap::new() }
            };
            self.dev = Some(dev);
        }
        Ok(self.dev.as_mut().expect("device was just created"))
    }

    /// The generation of the device this renderer draws on now (a [`Picture`] of another one is not drawn).
    fn epoch(&mut self) -> u32 {
        self.device().map_or(0, |d| d.epoch)
    }

    /// Paints one frame of `w`×`h` device pixels into `surf`; `draw` gets a cleared, transparent canvas in DIPs.
    pub fn paint(&mut self, surf: &mut Surface, w: i32, h: i32, draw: impl FnOnce(&mut Canvas<'_>)) -> Result<()> {
        if w <= 0 || h <= 0 {
            return Ok(());
        }
        let r = if surf.composed { self.paint_comp(surf, w, h, draw) } else { self.paint_dib(surf, w, h, draw) };
        if let Err(e) = &r
            && device_gone(e)
        {
            lose_device();
        }
        r
    }

    /// A frame into the surface's DirectComposition texture: no copy at all.
    fn paint_comp(&mut self, surf: &mut Surface, w: i32, h: i32, draw: impl FnOnce(&mut Canvas<'_>)) -> Result<()> {
        let d = dev()?;
        let epoch = self.device()?.epoch;
        let dc = d.dcomp.as_ref().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        if surf.comp.as_ref().is_none_or(|c| c.w != w || c.h != h || c.epoch != epoch) {
            // SAFETY: surface creation on the composition device.
            let s = unsafe { dc.CreateSurface(w as u32, h as u32, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)? };
            if let Some(old) = surf.comp.replace(Comp { surf: s, w, h, epoch, bound: Cell::new(None) }) {
                old.bound.set(None); // the window keeps showing it until the new one is presented
            }
        }
        let comp = surf.comp.as_ref().expect("surface was just made");
        let s = self.scale;
        let mut off = POINT::default();
        // SAFETY: BeginDraw hands out the surface's context; everything drawn stays inside the clip until EndDraw.
        let ctx: ID2D1DeviceContext = unsafe { comp.surf.BeginDraw(None, &mut off)? };
        let base = Matrix3x2::translation(off.x as f32 / s, off.y as f32 / s);
        // SAFETY: setting the state of the context for this frame.
        unsafe {
            ctx.SetDpi(96.0 * s, 96.0 * s);
            ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            ctx.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            ctx.SetTransform(&base);
            ctx.PushAxisAlignedClip(&D2D_RECT_F { left: 0.0, top: 0.0, right: w as f32 / s, bottom: h as f32 / s }, D2D1_ANTIALIAS_MODE_ALIASED);
            ctx.Clear(Some(&Rgba::TRANSPARENT.d2d()));
        }
        {
            let dv = self.dev.as_mut().expect("device was just made");
            dv.cur = ctx.clone();
            dv.base = base;
        }
        draw(&mut Canvas { g: self });
        let started = BENCH_END.with(|b| b.get().map(|_| std::time::Instant::now()));
        // SAFETY: matches the BeginDraw above.
        let end = unsafe {
            ctx.PopAxisAlignedClip();
            comp.surf.EndDraw()
        };
        if let Some(started) = started {
            BENCH_END.with(|b| b.set(b.get().map(|d| d + started.elapsed())));
        }
        let dv = self.dev.as_mut().expect("device was just made");
        dv.cur = dv.ctx.clone();
        dv.base = Matrix3x2::identity();
        end
    }

    /// A frame drawn on the GPU into a bitmap, read back and copied into the surface's DIB.
    fn paint_dib(&mut self, surf: &mut Surface, w: i32, h: i32, draw: impl FnOnce(&mut Canvas<'_>)) -> Result<()> {
        if !surf.ensure(w, h) {
            return Ok(());
        }
        let ctx = self.device()?.ctx.clone();
        let epoch = self.dev.as_ref().map_or(0, |d| d.epoch);
        if surf.gpu.as_ref().is_none_or(|g| g.epoch != epoch || g.w != w || g.h != h) {
            let props = |bitmap_options| D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: bitmap_options,
                ..Default::default()
            };
            let size = D2D_SIZE_U { width: w as u32, height: h as u32 };
            // SAFETY: two bitmaps of the frame's size on the thread's device.
            surf.gpu = Some(unsafe {
                Gpu {
                    target: ctx.CreateBitmap(size, None, 0, &props(D2D1_BITMAP_OPTIONS_TARGET))?,
                    copy: ctx.CreateBitmap(size, None, 0, &props(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW))?,
                    epoch,
                    w,
                    h,
                }
            });
        }
        let gpu = surf.gpu.as_ref().expect("bitmaps were just made");
        // SAFETY: the target is ours alone for the frame.
        unsafe {
            ctx.SetTarget(&gpu.target);
            ctx.BeginDraw();
            ctx.SetTransform(&Matrix3x2::identity());
            ctx.Clear(Some(&Rgba::TRANSPARENT.d2d()));
        }
        draw(&mut Canvas { g: self });
        let started = BENCH_END.with(|b| b.get().map(|_| std::time::Instant::now()));
        // SAFETY: matches the BeginDraw above; the copy is mapped for reading only for the time of the row copy, into the
        // DIB that holds `w * h` pixels.
        let end = unsafe {
            let end = ctx.EndDraw(None, None);
            ctx.SetTarget(&self.dev.as_ref().expect("device was just made").idle);
            end.and_then(|()| {
                gpu.copy.CopyFromBitmap(None, &gpu.target, None)?;
                let m = gpu.copy.Map(D2D1_MAP_OPTIONS_READ)?;
                let row = w as usize * 4;
                for y in 0..h as usize {
                    std::ptr::copy_nonoverlapping(m.bits.add(y * m.pitch as usize), surf.bits.cast::<u8>().add(y * row), row);
                }
                gpu.copy.Unmap()
            })
        };
        if let Some(started) = started {
            BENCH_END.with(|b| b.set(b.get().map(|d| d + started.elapsed())));
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

    /// Keeps a layout in the younger generation; a full one becomes the older.
    fn remember(&mut self, key: LayoutKey, lay: IDWriteTextLayout1) {
        if self.layouts.len() >= LAYOUTS_PER_GENERATION {
            self.layouts_old = std::mem::take(&mut self.layouts);
        }
        self.layouts.insert(key, lay);
    }

    /// The caret position (a UTF-16 index into `s`) nearest to `x` DIPs from the start of the line, as a click puts it: the
    /// edges of clusters, never inside one.
    pub fn text_hit(&mut self, s: &str, f: Font, x: f32) -> usize {
        let Some(lay) = self.layout(s, f, 4096.0, f.size * 2.0, Align::Start, false) else { return 0 };
        let (mut trailing, mut inside) = (windows::core::BOOL(0), windows::core::BOOL(0));
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        // SAFETY: `lay` is live; the out-params are locals.
        unsafe {
            if lay.HitTestPoint(x, f.size, &mut trailing, &mut inside, &mut m).is_err() {
                return 0;
            }
        }
        (m.textPosition + if trailing.as_bool() { m.length } else { 0 }) as usize
    }

    /// Where the caret at the UTF-16 index `idx` of `s` stands: DIPs from the start of the line.
    pub fn text_x(&mut self, s: &str, f: Font, idx: usize) -> f32 {
        let Some(lay) = self.layout(s, f, 4096.0, f.size * 2.0, Align::Start, false) else { return 0.0 };
        let (mut x, mut y) = (0.0, 0.0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        // SAFETY: `lay` is live; the out-params are locals.
        unsafe { lay.HitTestTextPosition(idx as u32, false, &mut x, &mut y, &mut m).map_or(0.0, |()| x) }
    }

    /// Lays out one line of text; `None` if the font could not be made.
    fn layout(&mut self, s: &str, f: Font, w: f32, h: f32, align: Align, trim: bool) -> Option<IDWriteTextLayout1> {
        let key = (s.to_owned(), f.size.to_bits(), f.weight, f.tracking.to_bits(), f.tabular, w.to_bits(), h.to_bits(), align as u8, trim);
        if let Some(layout) = self.layouts.get(&key) { return Some(layout.clone()); }
        if let Some(layout) = self.layouts_old.remove(&key) {
            self.remember(key, layout.clone());
            return Some(layout);
        }
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
            self.remember(key, lay.clone());
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

    /// The gradient stop collection of stops (made once, kept: the same gradient is drawn every frame).
    fn stops(&mut self, stops: &[D2D1_GRADIENT_STOP]) -> Option<ID2D1GradientStopCollection> {
        let key: Vec<u32> = stops.iter().flat_map(|s| [s.position.to_bits(), s.color.r.to_bits(), s.color.g.to_bits(), s.color.b.to_bits(), s.color.a.to_bits()]).collect();
        if let Some(c) = self.dev().grads.get(&key) {
            return Some(c.clone());
        }
        let rt = self.own();
        // SAFETY: a stop collection on the thread's device.
        let coll = unsafe { rt.CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()? };
        let grads = &mut self.dev().grads;
        if grads.len() > 64 {
            grads.clear();
        }
        grads.insert(key, coll.clone());
        Some(coll)
    }

    /// The context of the frame in progress (what is drawn goes through it).
    fn rt(&mut self) -> ID2D1RenderTarget {
        self.dev().cur.clone().into()
    }

    /// The renderer's own context: it makes the bitmaps, effects and off-screen targets, which any context of the device draws.
    fn own(&mut self) -> ID2D1RenderTarget {
        self.dev().ctx.clone().into()
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

    /// A dashed border (CSS `border: <w>px dashed`) centred on the rectangle `r`'s edge; `dash` and `gap` are in units of `w`.
    pub fn dashed_rect(&mut self, r: Rect, w: f32, dash: f32, gap: f32, c: Rgba) {
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let (rt, b) = (self.rt(), self.brush(c));
        let props = D2D1_STROKE_STYLE_PROPERTIES1 {
            startCap: D2D1_CAP_STYLE_FLAT,
            endCap: D2D1_CAP_STYLE_FLAT,
            dashCap: D2D1_CAP_STYLE_FLAT,
            lineJoin: D2D1_LINE_JOIN_MITER,
            miterLimit: 10.0,
            dashStyle: D2D1_DASH_STYLE_CUSTOM,
            dashOffset: 0.0,
            transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
        };
        // SAFETY: a stroke style made and used inside BeginDraw/EndDraw.
        unsafe {
            if let Ok(style) = self.g.factory.CreateStrokeStyle(&props, Some(&[dash, gap])) {
                rt.DrawRectangle(&r.d2d(), &b, w, &style);
            }
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
            let Some(coll) = self.stops(&stops) else { return };
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
            let Some(coll) = self.stops(&stops) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: r.cx(), Y: r.y }, endPoint: Vector2 { X: r.cx(), Y: r.y + r.h } };
            if let Ok(b) = rt.CreateLinearGradientBrush(&props, None, &coll) {
                rt.FillRoundedRectangle(&rr, &b);
            }
        }
    }

    /// A rounded rectangle filled with a diagonal gradient, CSS `linear-gradient(135deg, from, to)` of a square.
    pub fn gradient_diagonal(&mut self, r: Rect, radius: f32, from: Rgba, to: Rgba) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let rt = self.rt();
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: from.d2d() }, D2D1_GRADIENT_STOP { position: 1.0, color: to.d2d() }];
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            let Some(coll) = self.stops(&stops) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: r.x, Y: r.y }, endPoint: Vector2 { X: r.x + r.w, Y: r.y + r.h } };
            if let Ok(b) = rt.CreateLinearGradientBrush(&props, None, &coll) {
                rt.FillRoundedRectangle(&rr, &b);
            }
        }
    }

    /// Like [`Canvas::gradient_diagonal`] with any number of stops (position 0..1, colour): CSS
    /// `linear-gradient(135deg, a, b 55%, c)` of a square.
    pub fn gradient_diagonal_stops(&mut self, r: Rect, radius: f32, stops: &[(f32, Rgba)]) {
        if r.w <= 0.0 || r.h <= 0.0 || stops.len() < 2 {
            return;
        }
        let rt = self.rt();
        let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(position, c)| D2D1_GRADIENT_STOP { position, color: c.d2d() }).collect();
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            let Some(coll) = self.stops(&stops) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: r.x, Y: r.y }, endPoint: Vector2 { X: r.x + r.w, Y: r.y + r.h } };
            if let Ok(b) = rt.CreateLinearGradientBrush(&props, None, &coll) {
                rt.FillRoundedRectangle(&rr, &b);
            }
        }
    }

    /// Makes what lies outside the rounded rectangle `r` (inside the box `r`) transparent, antialiased: the round corners of a
    /// window drawn square. `r` is where the corners are; nothing outside the box is touched.
    pub fn round_corners(&mut self, r: Rect, radius: f32) {
        if radius <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let f = &self.g.factory;
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: plain geometry work on the factory; the result is filled inside BeginDraw/EndDraw, the blend mode restored.
        unsafe {
            let corners = (|| -> Result<ID2D1PathGeometry> {
                let (all, round) = (f.CreateRectangleGeometry(&r.d2d())?, f.CreateRoundedRectangleGeometry(&rr)?);
                let path = f.CreatePathGeometry()?;
                let sink = path.Open()?;
                all.CombineWithGeometry(&round, D2D1_COMBINE_MODE_EXCLUDE, None, 0.1, &sink)?;
                sink.Close()?;
                path.cast()
            })();
            let Ok(shape) = corners else { return };
            let ctx = self.dev().cur.clone();
            let b = self.brush(Rgba::TRANSPARENT);
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
            ctx.FillGeometry(&shape, &b, None);
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
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
            let Some(coll) = self.stops(&stops) else { return };
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

    /// See [`Gfx::text_hit`].
    pub fn text_hit(&mut self, s: &str, f: Font, x: f32) -> usize {
        self.g.text_hit(s, f, x)
    }

    /// See [`Gfx::text_x`].
    pub fn text_x(&mut self, s: &str, f: Font, idx: usize) -> f32 {
        self.g.text_x(s, f, idx)
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

    /// Applies the icons retired since this device last looked.
    fn sync_retired(&mut self) {
        let dev = self.dev();
        let log = RETIRED.lock().unwrap();
        let latest = log.last().map_or(0, |e| e.0);
        if dev.retired == latest {
            return;
        }
        if log.first().is_some_and(|e| e.0 > dev.retired + 1) {
            dev.icons.clear(); // the log was trimmed past what this device saw
            dev.shadows.clear();
        }
        let seen = dev.retired;
        for &(_, h) in log.iter().filter(|e| e.0 > seen) {
            drop_icon(dev, h);
        }
        dev.retired = latest;
    }

    fn bitmap_of(&mut self, icon: HICON) -> Option<ID2D1Bitmap> {
        self.sync_retired();
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
        let ctx = self.dev().cur.clone();
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe {
            ctx.DrawBitmap(&bmp, Some(&r.d2d()), opacity, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
        }
    }

    /// Draw a window snapshot with cover fitting and rounded corners.
    pub fn hicon_cover(&mut self, icon: HICON, r: Rect, radius: f32) {
        use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BRUSH_PROPERTIES};
        if icon.is_invalid() { return; }
        let Some(bmp) = self.bitmap_of(icon) else { return };
        // SAFETY: the bitmap belongs to this render target and is used inside BeginDraw/EndDraw.
        unsafe {
            let size = bmp.GetSize();
            if size.width <= 0.0 || size.height <= 0.0 { return; }
            let k = (r.w / size.width).max(r.h / size.height);
            let props = D2D1_BITMAP_BRUSH_PROPERTIES { extendModeX: D2D1_EXTEND_MODE_CLAMP, extendModeY: D2D1_EXTEND_MODE_CLAMP, interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR };
            let brush = D2D1_BRUSH_PROPERTIES { opacity: 1.0, transform: Matrix3x2::scale(k, k) * Matrix3x2::translation(r.cx() - size.width * k / 2.0, r.cy() - size.height * k / 2.0) };
            let rt = self.rt();
            if let Ok(b) = rt.CreateBitmapBrush(&bmp, Some(&props), Some(&brush)) {
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &b);
            }
        }
    }

    /// CSS `filter: drop-shadow(0 <dy>px <blur>px <c>)` of an icon: its own silhouette, blurred. Cached per icon and size.
    pub fn icon_shadow(&mut self, icon: HICON, r: Rect, dy: f32, blur: f32, c: Rgba) {
        if icon.is_invalid() || c.a <= 0.0 || r.w <= 0.0 {
            return;
        }
        self.sync_retired();
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
        let ctx = self.dev().cur.clone();
        let dst = Rect::new(r.x - pad, r.y - pad + dy, r.w + 2.0 * pad, r.h + 2.0 * pad);
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { ctx.DrawBitmap(&bmp, Some(&dst.d2d()), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
    }

    /// Drops the cached bitmap and shadow of an icon that is about to be destroyed.
    pub fn forget_icon(&mut self, icon: HICON) {
        drop_icon(self.dev(), icon.0 as usize);
    }

    fn icon_bitmap(&mut self, icon: HICON) -> Option<ID2D1Bitmap> {
        let rt = self.own();
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
        let c = c.themed(); // the cache below is keyed by the colour that is drawn
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let sigma = blur / 2.0; // CSS: the blur radius is twice the Gaussian's standard deviation
        let pad = (sigma * 3.0).ceil();
        // A box big enough has the same shadow at any size: four corners as they are, edges and middle stretched. So one small
        // shadow per look, cut in nine, serves every size an animation passes through: no new bitmap and no new blur each
        // frame (those piled up in the device's memory while a panel's height moved).
        let k = pad + radius + pad; // a corner: the blur outside the edge, the rounding, the blur inside
        let dst = Rect::new(r.x - pad, r.y - pad + dy, r.w + 2.0 * pad, r.h + 2.0 * pad);
        if dst.w >= 2.0 * k + 2.0 && dst.h >= 2.0 * k + 2.0 {
            let t = 2.0 * (radius + pad) + 2.0; // the small box: its middle two DIPs are edge only
            let key = [t.to_bits(), t.to_bits(), radius.to_bits(), sigma.to_bits(), c.a.to_bits(), (c.r + c.g * 2.0 + c.b * 4.0).to_bits()];
            let bmp = match self.dev().shadows.get(&key).cloned() {
                Some((b, _)) => b,
                None => {
                    let Some(b) = self.shadow_bitmap(t, t, radius, sigma, pad, c) else { return };
                    let dev = self.dev();
                    if dev.shadows.len() > 8 {
                        dev.shadows.clear();
                    }
                    dev.shadows.insert(key, (b.clone(), pad));
                    b
                }
            };
            let s = self.g.scale;
            let snap = |v: f32| (v * s).round() / s; // the pieces meet on whole pixels: no seam
            let side = 2.0 * k + 2.0;
            let src = [0.0, k, k + 2.0, side];
            let xs = [snap(dst.x), snap(dst.x + k), snap(dst.x + dst.w - k), snap(dst.x + dst.w)];
            let ys = [snap(dst.y), snap(dst.y + k), snap(dst.y + dst.h - k), snap(dst.y + dst.h)];
            let ctx = self.dev().cur.clone();
            for j in 0..3 {
                for i in 0..3 {
                    let to = D2D_RECT_F { left: xs[i], top: ys[j], right: xs[i + 1], bottom: ys[j + 1] };
                    let from = D2D_RECT_F { left: src[i], top: src[j], right: src[i + 1], bottom: src[j + 1] };
                    if to.right > to.left && to.bottom > to.top {
                        // SAFETY: drawing inside BeginDraw/EndDraw.
                        unsafe { ctx.DrawBitmap(&bmp, Some(&to), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, Some(&from), None) };
                    }
                }
            }
            return;
        }
        let key = [r.w.to_bits(), r.h.to_bits(), radius.to_bits(), sigma.to_bits(), c.a.to_bits(), (c.r + c.g * 2.0 + c.b * 4.0).to_bits()];
        let cached = self.dev().shadows.get(&key).cloned();
        let bmp = match cached {
            Some((b, _)) => b,
            None => {
                let Some(b) = self.shadow_bitmap(r.w, r.h, radius, sigma, pad, c) else { return };
                let dev = self.dev();
                if dev.shadows.len() > 8 {
                    dev.shadows.clear(); // sizes animate; keep only recent ones
                }
                dev.shadows.insert(key, (b.clone(), pad));
                b
            }
        };
        let ctx = self.dev().cur.clone();
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
        let (rt, ctx) = (self.own(), self.dev().ctx.clone());
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
        let ctx = self.dev().cur.clone();
        let b = self.brush(Rgba::TRANSPARENT);
        // SAFETY: drawing inside BeginDraw/EndDraw; the blend mode is restored.
        unsafe {
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
            ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &b);
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
    }

    /// Sets the pixels of the rounded rectangle `r` that lie inside the rounded rectangle `within` to `to` (a hole cut in a box
    /// with round corners: transparent, or a hair of alpha that keeps the pointer on the window).
    pub fn erase_within(&mut self, r: Rect, radius: f32, within: Rect, within_radius: f32, to: Rgba) {
        let rr = |r: Rect, k: f32| D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: k, radiusY: k };
        let f_ = &self.g.factory;
        // SAFETY: plain geometry work on the factory.
        let shape = unsafe {
            (|| -> Result<ID2D1Geometry> {
                let a = f_.CreateRoundedRectangleGeometry(&rr(r, radius))?;
                let b = f_.CreateRoundedRectangleGeometry(&rr(within, within_radius))?;
                let path = f_.CreatePathGeometry()?;
                let sink = path.Open()?;
                a.CombineWithGeometry(&b, D2D1_COMBINE_MODE_INTERSECT, None, 0.1, &sink)?;
                sink.Close()?;
                path.cast()
            })()
            .ok()
        };
        let Some(shape) = shape else { return };
        let ctx = self.dev().cur.clone();
        let b = self.brush(to);
        // SAFETY: drawing inside BeginDraw/EndDraw; the blend mode is restored.
        unsafe {
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
            ctx.FillGeometry(&shape, &b, None);
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
        let ctx = self.dev().cur.clone();
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
        let (rt, ctx, s) = (self.own(), self.dev().ctx.clone(), self.g.scale);
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
            // Light glass over a dark desktop: a light tint at the design's alpha over the blurred picture comes out muddy grey.
            // Like the light acrylic of Windows, the blurred picture is first lifted towards white, so the glass reads frosted
            // and still lets the colours through.
            let lift = if winrust_common::style::light() { 0.5 } else { 0.0 };
            if lift > 0.0 {
                let k = 1.0 - lift;
                let m = [k, 0.0, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, 0.0, 1.0, lift, lift, lift, 0.0];
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
            Some(Picture { bmp: baked.GetBitmap().ok()?, epoch: self.dev().epoch, w: size.width, h: size.height })
        }
    }

    /// A screen grab (`w`×`h` device pixels, opaque) as a picture that [`Canvas::picture`] draws pixel for pixel; no blur.
    pub fn pixels(&mut self, px: &[u32], w: i32, h: i32) -> Option<Picture> {
        if w <= 0 || h <= 0 || px.len() < (w * h) as usize {
            return None;
        }
        let (rt, s) = (self.own(), self.g.scale);
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * s,
            dpiY: 96.0 * s,
        };
        // SAFETY: the pixels are read once by CreateBitmap.
        let bmp = unsafe { rt.CreateBitmap(D2D_SIZE_U { width: w as u32, height: h as u32 }, Some(px.as_ptr().cast()), (w * 4) as u32, &props).ok()? };
        Some(Picture { bmp, epoch: self.dev().epoch, w: w as f32 / s, h: h as f32 / s })
    }

    /// Whether `p` was made on the device in use now (after a lost device it draws nothing and must be made again).
    pub fn is_current(&mut self, p: &Picture) -> bool {
        p.epoch == self.dev().epoch
    }

    /// An image file (JPEG, PNG, ...) scaled to cover `w` × `h` DIPs, as a picture to draw with [`Canvas::picture`]; it is
    /// centred, so what sticks out of the box is cut by the caller (`CSS object-fit: cover`). `None` if it cannot be read.
    pub fn image(&mut self, path: &str, w: f32, h: f32) -> Option<Picture> {
        use windows::Win32::Graphics::Imaging::{WICBitmapInterpolationModeFant, WICDecodeMetadataCacheOnDemand};
        let (rt, s) = (self.own(), self.g.scale);
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
            Some(Picture { bmp, epoch: self.dev().epoch, w: pw as f32 / s, h: ph as f32 / s })
        }
    }

    /// The picture was made on a device that has been lost since: it is not drawn, and must be made again.
    pub fn picture_stale(&mut self, p: &Picture) -> bool {
        p.epoch != self.dev().epoch
    }

    /// Draws a baked picture with its top left corner at `(x, y)`, pixel for pixel.
    pub fn picture(&mut self, p: &Picture, x: f32, y: f32) {
        if p.epoch != self.dev().epoch {
            return;
        }
        let ctx = self.dev().cur.clone();
        let dst = Rect::new(x, y, p.w, p.h);
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { ctx.DrawBitmap(&p.bmp, Some(&dst.d2d()), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None) };
    }

    /// A picture from [`Canvas::image`] (made for a box of `r`'s size) filling the rounded rectangle `r`, centred and cut to it, at
    /// `opacity`: CSS `object-fit: cover` with a `border-radius`. It goes through a brush, so it follows an enclosing transform.
    pub fn picture_cover(&mut self, p: &Picture, r: Rect, radius: f32, opacity: f32) {
        use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BRUSH_PROPERTIES};
        if p.epoch != self.dev().epoch {
            return;
        }
        let rt = self.rt();
        let props = D2D1_BITMAP_BRUSH_PROPERTIES { extendModeX: D2D1_EXTEND_MODE_CLAMP, extendModeY: D2D1_EXTEND_MODE_CLAMP, interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR };
        let brush = D2D1_BRUSH_PROPERTIES { opacity, transform: Matrix3x2::translation(r.cx() - p.w / 2.0, r.cy() - p.h / 2.0) };
        // SAFETY: the brush is made and used inside BeginDraw/EndDraw, on the target the picture was made on.
        unsafe {
            if let Ok(b) = rt.CreateBitmapBrush(&p.bmp, Some(&props), Some(&brush)) {
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &b);
            }
        }
    }

    /// Runs `f` with everything it draws clipped to the rounded rectangle `r` as `xf` moves it (CSS `clip-path: inset(..
    /// round ..)` on an element with a transform). Call it with no transform in force: the shape is in window coordinates.
    pub fn masked(&mut self, r: Rect, radius: f32, xf: Xf, f: impl FnOnce(&mut Self)) {
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
        self.under_mask(mask, f);
    }

    /// Runs `f` with everything it draws kept out of the rounded rectangle `r`: a CSS outer `box-shadow` is not painted under
    /// its own box. [`Canvas::erase`] does that by cutting a hole, which in a window with a see-through backdrop (a panel's
    /// glass) shows the desktop; this leaves whatever is already under the box alone.
    pub fn outside(&mut self, r: Rect, radius: f32, f: impl FnOnce(&mut Self)) {
        let rr = |r: Rect, k: f32| D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: k, radiusY: k };
        let f_ = &self.g.factory;
        // SAFETY: plain geometry work on the factory.
        let mask = unsafe {
            (|| -> Result<ID2D1Geometry> {
                let all = f_.CreateRoundedRectangleGeometry(&rr(Rect::new(-1e5, -1e5, 2e5, 2e5), 0.0))?;
                let hole = f_.CreateRoundedRectangleGeometry(&rr(r, radius))?;
                let path = f_.CreatePathGeometry()?;
                let sink = path.Open()?;
                all.CombineWithGeometry(&hole, D2D1_COMBINE_MODE_EXCLUDE, None, 0.1, &sink)?;
                sink.Close()?;
                path.cast()
            })()
            .ok()
        };
        match mask {
            Some(mask) => self.under_mask(mask, f),
            None => f(self),
        }
    }

    /// Runs `f` with everything it draws clipped to the geometry `mask` (a layer; the shape is in window coordinates).
    fn under_mask(&mut self, mask: ID2D1Geometry, f: impl FnOnce(&mut Self)) {
        let ctx = self.dev().cur.clone();
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

// ---------------------------------------------------------------------------------------------------------------------
// wrapped text and a picture that sticks to one edge (the notification cards)

impl Gfx {
    /// Lays out `s` wrapped to `w` DIPs, `line` DIPs per line, at most `max` lines high (what does not fit ends in an ellipsis);
    /// `max = 0` is no limit.
    fn wrapped(&mut self, s: &str, f: Font, w: f32, line: f32, max: usize) -> Option<windows::Win32::Graphics::DirectWrite::IDWriteTextLayout> {
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_LINE_SPACING_METHOD_UNIFORM, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            DWRITE_WORD_WRAPPING_WRAP,
        };
        let fmt = self.format(f)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        let h = if max == 0 { 1e4 } else { line * max as f32 };
        // SAFETY: plain DirectWrite layout creation and settings on a layout of ours.
        unsafe {
            let lay = self.dwrite.CreateTextLayout(&wide, &fmt, w.max(1.0), h).ok()?;
            let _ = lay.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let _ = lay.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
            // `line-height` of the design: the glyphs sit a little below the middle of their line box
            let _ = lay.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line, line * 0.78);
            if max > 0 {
                let sign = self.dwrite.CreateEllipsisTrimmingSign(&fmt).ok();
                let t = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
                let _ = lay.SetTrimming(&t, sign.as_ref());
            }
            Some(lay)
        }
    }

    /// How many lines `s` takes wrapped to `w` DIPs (at least one).
    pub fn lines(&mut self, s: &str, f: Font, w: f32) -> usize {
        let Some(lay) = self.wrapped(s, f, w, f.size * 1.45, 0) else { return 1 };
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: `lay` is live; `m` is a local out-param.
        unsafe { lay.GetMetrics(&mut m).map_or(1, |()| m.lineCount.max(1) as usize) }
    }
}

impl Canvas<'_> {
    /// How many lines `s` takes wrapped to `w` DIPs (at least one).
    pub fn lines(&mut self, s: &str, f: Font, w: f32) -> usize {
        self.g.lines(s, f, w)
    }

    /// Text wrapped inside `r` from its top, `line` DIPs per line (CSS `line-height`), at most `max` lines; what does not fit
    /// ends in an ellipsis (`-webkit-line-clamp`).
    pub fn text_wrapped(&mut self, s: &str, r: Rect, f: Font, c: Rgba, line: f32, max: usize) {
        if c.a <= 0.0 {
            return;
        }
        let Some(lay) = self.g.wrapped(s, f, r.w, line, max) else { return };
        let (rt, b) = (self.rt(), self.brush(c));
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { rt.DrawTextLayout(Vector2 { X: r.x, Y: r.y }, &lay, &b, windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP) };
    }

    /// Like [`Canvas::picture_cover`], with the picture's own place in the box: `v` is the CSS `object-position` y (0 top, 1 bottom).
    pub fn picture_cover_at(&mut self, p: &Picture, r: Rect, radius: f32, opacity: f32, v: f32) {
        use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BRUSH_PROPERTIES};
        if p.epoch != self.dev().epoch {
            return; // made on a device that was lost since, as in `picture_cover`
        }
        let rt = self.rt();
        let props = D2D1_BITMAP_BRUSH_PROPERTIES { extendModeX: D2D1_EXTEND_MODE_CLAMP, extendModeY: D2D1_EXTEND_MODE_CLAMP, interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR };
        let brush = D2D1_BRUSH_PROPERTIES { opacity, transform: Matrix3x2::translation(r.cx() - p.w / 2.0, r.y - (p.h - r.h) * v) };
        // SAFETY: the brush is made and used inside BeginDraw/EndDraw, on the target the picture was made on.
        unsafe {
            if let Ok(b) = rt.CreateBitmapBrush(&p.bmp, Some(&props), Some(&brush)) {
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

    fn gfx() -> Gfx {
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        Gfx::new(1.0).expect("direct2d")
    }

    fn frame(g: &mut Gfx, s: &mut Surface) {
        g.paint(s, 64, 32, |c| {
            c.fill(Rect::new(4.0, 4.0, 40.0, 20.0), 6.0, Rgba::hex(0x3366cc).alpha(0.8));
            c.text("Ab", Rect::new(6.0, 4.0, 40.0, 20.0), Font::new(13.0, 600), Rgba::pure_white(1.0), Align::Start);
        })
        .expect("a frame");
    }

    #[test]
    fn a_dark_literal_becomes_its_light_token_and_keeps_its_alpha() {
        let near = |a: Rgba, b: Rgba| (a.r - b.r).abs() < 0.002 && (a.g - b.g).abs() < 0.002 && (a.b - b.b).abs() < 0.002 && a.a == b.a;
        assert!(near(Rgba::css(236, 238, 244, 0.5).in_light(), Rgba::css(27, 29, 35, 0.5)), "--t1");
        assert!(near(Rgba::hex(0x2c2d34).in_light(), Rgba::hex(0xe6e7e9)), "--h2c2d34");
        assert!(near(Rgba::css(16, 17, 24, 0.9).in_light(), Rgba::css(243, 244, 246, 0.9)), "--s16_17_24");
        assert!(near(Rgba::white(0.1).in_light(), Rgba::css(20, 22, 30, 0.1)), "the ink");
        assert!(near(Rgba::strong(1.0).in_light(), Rgba::css(27, 29, 35, 1.0)), "white that is --t1");
        // literals stay: shadows, white on accent fills, the accent, text on it, colours of the system
        for c in [Rgba::css(0, 0, 0, 0.5), Rgba::pure_white(1.0), Rgba::hex(0xffffff), Rgba::hex(0x62a0ea), Rgba::css(11, 18, 32, 1.0), Rgba::colorref(0x00_f1_f2_f4)] {
            assert_eq!(c.in_light(), c);
        }
        // a colour that kept its token through `alpha` and `fade`, and loses it when mixed (the blend is of what is drawn)
        assert!(near(Rgba::white(1.0).alpha(0.3).fade(0.5).in_light(), Rgba::css(20, 22, 30, 0.15)));
        // dark mode draws the literal itself
        assert_eq!(Rgba::css(236, 238, 244, 0.5).themed(), Rgba::new(236.0 / 255.0, 238.0 / 255.0, 244.0 / 255.0, 0.5));
    }

    #[test]
    fn an_apps_own_pair_is_the_dark_value_and_its_light_twin_with_the_alpha_kept() {
        let near = |a: Rgba, b: Rgba| (a.r - b.r).abs() < 0.002 && (a.g - b.g).abs() < 0.002 && (a.b - b.b).abs() < 0.002 && a.a == b.a;
        let win = Rgba::pair(0x16171b, 0xf7f7f9);
        assert!(near(win, Rgba::hex(0x16171b)), "dark is what it was written as");
        assert!(near(win.in_light(), Rgba::hex(0xf7f7f9)));
        assert!(near(win.alpha(0.4).in_light(), Rgba::hex(0xf7f7f9).alpha(0.4)), "through alpha and fade it keeps its twin");
        assert!(near(win.fade(0.5).in_light(), Rgba::hex(0xf7f7f9).alpha(0.5)));
        // a pair of black: the light value 0 is a colour, not "none"
        assert!(near(Rgba::pair(0xffffff, 0x000000).in_light(), Rgba::new(0.0, 0.0, 0.0, 1.0)));
        // the bar's literals are untouched by it: no token, no pair
        assert_eq!(Rgba::new(0.5, 0.5, 0.5, 1.0).in_light(), Rgba::new(0.5, 0.5, 0.5, 1.0));
    }

    #[test]
    fn round_corners_cut_the_four_corners_and_nothing_else() {
        let mut g = gfx();
        let mut s = Surface::new();
        g.paint(&mut s, 60, 40, |c| {
            c.fill(Rect::new(0.0, 0.0, 60.0, 40.0), 0.0, Rgba::hex(0x3366cc));
            c.round_corners(Rect::new(0.0, 0.0, 60.0, 40.0), 16.0);
        })
        .expect("a frame");
        let a = |x: usize, y: usize| s.pixels()[y * 60 + x] >> 24;
        assert_eq!((a(0, 0), a(59, 0), a(0, 39), a(59, 39)), (0, 0, 0, 0), "the corners are transparent");
        assert_eq!((a(30, 0), a(0, 20), a(59, 20), a(30, 39), a(30, 20)), (255, 255, 255, 255, 255), "the edges and the middle are not");
        let rim = (0..16).map(|i| a(i, 15 - i)).filter(|&v| v != 0 && v != 255).count();
        assert!(rim > 0, "the rim of the corner is antialiased, not a hard cut");
    }

    #[test]
    fn a_new_scale_draws_the_next_frame_at_the_new_size() {
        let mut g = gfx();
        let mut s = Surface::new();
        let draw = |g: &mut Gfx, s: &mut Surface| g.paint(s, 20, 20, |c| c.fill(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, Rgba::hex(0xff0000))).expect("a frame");
        draw(&mut g, &mut s);
        let filled = |s: &Surface| s.pixels().iter().filter(|&&p| p >> 24 == 255).count();
        assert_eq!(filled(&s), 100, "10 x 10 DIPs at scale 1");
        g.set_scale(2.0);
        draw(&mut g, &mut s);
        assert_eq!(filled(&s), 400, "20 x 20 pixels at scale 2");
        assert_eq!(g.scale(), 2.0);
        // a DIB surface has no visual to show
        assert!(!s.show(HWND(std::ptr::null_mut())));
    }

    #[test]
    fn one_device_per_thread() {
        let (mut a, mut b) = (gfx(), gfx());
        let (mut sa, mut sb) = (Surface::new(), Surface::new());
        frame(&mut a, &mut sa);
        let first = current_dev().expect("a device");
        frame(&mut b, &mut sb);
        assert!(Rc::ptr_eq(&first, &current_dev().expect("still one")), "the second renderer made a device of its own");
        assert_eq!(sa.pixels(), sb.pixels(), "same drawing, same pixels");
    }

    #[test]
    fn paint_survives_device_loss() {
        let mut g = gfx();
        let mut s = Surface::new();
        frame(&mut g, &mut s);
        let before = s.pixels().to_vec();
        assert!(before.iter().any(|&p| p >> 24 != 0), "something was drawn");
        let epoch = current_dev().expect("a device").epoch;
        lose_device();
        frame(&mut g, &mut s);
        assert!(current_dev().expect("a new device").epoch > epoch);
        assert_eq!(before, s.pixels(), "the frame after the loss is the same frame");
    }

    #[test]
    fn layouts_in_use_survive_a_full_cache() {
        let mut g = gfx();
        let f = Font::new(13.0, 400);
        let key = |i: usize| format!("row {i}");
        let hot = g.layout("hot", f, 100.0, 20.0, Align::Start, false).expect("a layout");
        for i in 0..2 * LAYOUTS_PER_GENERATION + 100 {
            g.layout(&key(i), f, 100.0, 20.0, Align::Start, false).expect("a layout");
            if i % 500 == 0 {
                // a list that scrolls draws its visible rows again at every frame: they must not be rebuilt when the cache turns over
                let again = g.layout("hot", f, 100.0, 20.0, Align::Start, false).expect("a layout");
                assert!(again.as_raw() == hot.as_raw(), "the layout in use was dropped at row {i}");
            }
        }
        assert!(g.layouts.len() <= LAYOUTS_PER_GENERATION && g.layouts_old.len() <= LAYOUTS_PER_GENERATION, "two generations at most");
        // what has not been asked for since a whole generation went by is gone: the cache is bounded
        assert!(g.layouts.len() + g.layouts_old.len() <= 2 * LAYOUTS_PER_GENERATION);
    }

    #[test]
    fn a_click_lands_on_a_caret_position_and_the_caret_is_drawn_back_where_it_was() {
        let mut g = gfx();
        let f = Font::new(13.0, 400);
        let s = "Hello 😀 мир";
        // UTF-16: "Hello " is 6 units, the emoji 2, then " мир"
        assert_eq!(g.text_hit(s, f, -5.0), 0, "left of the text");
        assert_eq!(g.text_hit(s, f, 4000.0), s.encode_utf16().count(), "right of the text");
        for idx in [0, 3, 6, 8, 9, 12] {
            let x = g.text_x(s, f, idx);
            assert_eq!(g.text_hit(s, f, x + 0.5), idx, "the caret at {idx} (x = {x}) is where a click on it lands");
        }
        assert!(g.text_x(s, f, 6) > g.text_x(s, f, 3) && g.text_x(s, f, 12) > g.text_x(s, f, 9));
        // a click on the middle of the emoji goes to one of its two edges, never between the surrogates
        let (a, b) = (g.text_x(s, f, 6), g.text_x(s, f, 8));
        let mid = g.text_hit(s, f, (a + b) / 2.0 + 1.0);
        assert!(mid == 6 || mid == 8, "inside the emoji: {mid}");
    }

    #[test]
    fn a_mask_stands_in_the_world_it_was_pushed_in() {
        let mut g = gfx();
        let mut s = Surface::new();
        g.paint(&mut s, 200, 20, |c| {
            c.transformed(0.0, 0.0, 1.0, 50.0, 0.0, |c| {
                c.masked(Rect::new(0.0, 0.0, 100.0, 20.0), 0.0, Xf::NONE, |c| c.fill(Rect::new(-1000.0, -1000.0, 3000.0, 3000.0), 0.0, Rgba::hex(0xff0000)));
            });
        })
        .expect("a frame");
        let row = &s.pixels()[..200];
        let (first, last) = (row.iter().position(|&p| p >> 24 != 0), row.iter().rposition(|&p| p >> 24 != 0));
        // a DirectComposition surface sits at an offset inside its texture and is drawn under that transform: masks follow it
        assert_eq!((first, last), (Some(50), Some(149)), "the mask follows the world transform");
    }
}
