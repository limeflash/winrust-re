//! The colours the widgets share: the mockups' `--t1`..`--t4`, `--red`, `--menu`, written beside their light values with
//! [`Rgba::pair`]. An application's own palette (`--win`, `--bar`, `--card`) lives next to its screens.
use crate::gfx::{Canvas, Font, Rect, Rgba};
use winrust_common::style;

pub const T1: Rgba = Rgba::pair(0xeceef4, 0x1b1d23);
pub const T2: Rgba = Rgba::pair(0xa3a9b8, 0x4a4f5c);
pub const T3: Rgba = Rgba::pair(0x8a90a2, 0x6a7080);
pub const T4: Rgba = Rgba::pair(0x5f6474, 0x9298a6);
pub const RED: Rgba = Rgba::pair(0xff8a8f, 0xd0343b);
/// Menus, dialogs and toasts (`--menu`, `--toast`).
pub const MENU: Rgba = Rgba::pair(0x1e1f26, 0xfcfcfd).alpha(0.98);
pub const TOAST: Rgba = Rgba::pair(0x1e1f26, 0xfcfcfd).alpha(0.97);
/// The trough of a segmented control (`--well`) and the raised card in it (`--card`).
pub const WELL: Rgba = Rgba::pair(0x16171c, 0xebecf0);
pub const CARD: Rgba = Rgba::pair(0x22232a, 0xffffff);
/// `#e5484d`: a destructive button, in both modes.
pub const DANGER: Rgba = Rgba::new(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);
/// Text on an accent fill.
pub const ON_ACCENT: Rgba = Rgba::new(0x0b as f32 / 255.0, 0x12 as f32 / 255.0, 0x20 as f32 / 255.0, 1.0);

/// 13 px Inter, the mockups' body text.
pub const BODY: Font = Font::new(13.0, 400);

/// The accent now (the user's choice, or a colour they picked by hand).
#[must_use]
pub fn accent() -> Rgba {
    Rgba::colorref(style::pal().accent)
}

/// The trough of the bar's segmented controls (the design's `--segTrack` and `--segTrackSh`): a dark well with a shadow inside
/// it on the dark style, a faint ink tint with a half-pixel edge on the light one.
pub fn seg_track(c: &mut Canvas<'_>, r: Rect, radius: f32) {
    if style::light() {
        c.fill(r, radius, Rgba::white(0.07));
        c.ring(r, radius, 0.5, Rgba::white(0.06));
    } else {
        c.fill(r, radius, Rgba::css(0, 0, 0, 0.28));
        c.fill(Rect::new(r.x + 3.0, r.y, r.w - 6.0, 1.0), 0.0, Rgba::css(0, 0, 0, 0.3));
        c.fill(Rect::new(r.x + 2.0, r.y + 1.0, r.w - 4.0, 1.0), 0.0, Rgba::css(0, 0, 0, 0.12));
    }
}

/// The raised option in it (`--segThumb`, `--segThumbSh`): a lit glass gradient on the dark style, a white card on the light one.
pub fn seg_thumb(c: &mut Canvas<'_>, thumb: Rect, radius: f32) {
    if style::light() {
        c.outside(thumb, radius, |c| c.shadow(thumb, radius, 1.0, 3.0, Rgba::white(0.16)));
        c.fill(thumb, radius, Rgba::pure_white(1.0));
        c.ring(thumb, radius, 0.5, Rgba::white(0.1));
    } else {
        c.outside(thumb, radius, |c| c.shadow(thumb, radius, 1.0, 3.0, Rgba::css(0, 0, 0, 0.45)));
        c.gradient(thumb, radius, Rgba::white(0.16), Rgba::white(0.1));
        c.fill(Rect::new(thumb.x + 3.0, thumb.y, thumb.w - 6.0, 1.0), 0.0, Rgba::white(0.14));
    }
}

/// `color-mix(in oklab, var(--acc) 80%, #fff)`, the focus ring: near enough in sRGB.
#[must_use]
pub fn focus_ring() -> Rgba {
    accent().mix(Rgba::pure_white(1.0), 0.2)
}
