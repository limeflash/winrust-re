//! The colours the widgets share: the mockups' `--t1`..`--t4`, `--red`, `--menu`, written beside their light values with
//! [`Rgba::pair`]. An application's own palette (`--win`, `--bar`, `--card`) lives next to its screens.
use crate::gfx::{Font, Rgba};
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

/// `color-mix(in oklab, var(--acc) 80%, #fff)`, the focus ring: near enough in sRGB.
#[must_use]
pub fn focus_ring() -> Rgba {
    accent().mix(Rgba::pure_white(1.0), 0.2)
}
