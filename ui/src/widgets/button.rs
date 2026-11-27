//! Buttons: `.btn`, `.tb`, the switch, the segmented control and the chip of the mockups. Each draws, names its item (so the
//! host hit-tests it, and Tab stops at it) and returns how hot it is.

use super::look::{self, BODY};
use crate::anim::EASE;
use crate::frame::{Frame, Hot, Id};
use crate::gfx::{Align, Canvas, Font, Rect, Rgba};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The accent fill, dark text.
    Accent,
    /// A quiet plate: `rgba(ink,.08)`.
    Quiet,
    /// Only a ring.
    Outline,
    /// `#e5484d` with white text: what cannot be undone.
    Danger,
}

/// The focus ring of `:focus-visible`: 2 px, 1 px away from the box.
fn ring(c: &mut Canvas<'_>, r: Rect, radius: f32, hot: Hot) {
    if hot.focus {
        c.ring(r.inset(-3.0), radius + 3.0, 2.0, look::focus_ring().fade(0.9));
    }
}

/// `.btn`: 13 px, radius 10, hover brightens (`filter: brightness(1.12)`), press scales to .97.
pub fn button(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, label: &str, kind: Kind) -> Hot {
    let hot = f.focusable(id, r);
    f.hand();
    let (k, lift) = (1.0 - 0.03 * hot.press, hot.hover);
    c.transformed(r.cx(), r.cy(), k, 0.0, 0.0, |c| {
        let (font, ink) = match kind {
            Kind::Accent => {
                c.fill(r, 10.0, look::accent());
                (Font::new(13.0, 600), look::ON_ACCENT)
            }
            Kind::Quiet => {
                c.fill(r, 10.0, Rgba::white(0.08 + 0.05 * lift));
                (Font::new(13.0, 500), look::T1)
            }
            Kind::Outline => {
                c.fill(r, 10.0, Rgba::white(0.04 * lift));
                c.ring(r, 10.0, 1.0, Rgba::white(0.1 + 0.06 * lift));
                (Font::new(13.0, 500), look::T2)
            }
            Kind::Danger => {
                c.fill(r, 10.0, look::DANGER);
                (Font::new(13.0, 500), Rgba::pure_white(1.0))
            }
        };
        if matches!(kind, Kind::Accent | Kind::Danger) && lift > 0.0 {
            c.fill(r, 10.0, Rgba::pure_white(0.12 * lift));
        }
        c.text(label, r, font, ink, Align::Center);
    });
    ring(c, r, 10.0, hot);
    hot
}

/// `.tb`: a square button with a 16 px icon, radius 9; `on` is `aria-pressed`. Hover lifts its plate and its colour.
pub fn icon_button(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, icon: &'static str, tip: &'static str, on: bool) -> Hot {
    let hot = f.focusable(id, r);
    f.hand();
    f.tip(tip);
    let plate = if on { 0.12_f32.max(0.09 * hot.hover) } else { 0.09 * hot.hover };
    c.transformed(r.cx(), r.cy(), 1.0 - 0.08 * hot.press, 0.0, 0.0, |c| {
        c.fill(r, 9.0, Rgba::white(plate));
        let ink = if on { look::T1 } else { look::T2.mix(look::T1, hot.hover) };
        c.icon(icon, (r.cx(), r.cy()), 16.0, 1.9, ink, 0.0);
    });
    ring(c, r, 9.0, hot);
    hot
}

/// The switch, 38 × 22: a knob of 18 on a track that turns accent. `r` is the track.
pub fn toggle(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, on: bool) -> Hot {
    let hot = f.focusable(id, r);
    f.hand();
    let t = f.tween(id, 2, f32::from(on), 200, EASE);
    c.fill(r, r.h / 2.0, Rgba::white(0.18).mix(look::accent(), t));
    let d = r.h - 4.0;
    c.circle(r.x + 2.0 + d / 2.0 + (r.w - r.h) * t, r.cy(), d / 2.0, Rgba::pure_white(1.0));
    ring(c, r, r.h / 2.0, hot);
    hot
}

/// A segmented control (`.seg`): the labels in a trough, the chosen one raised on a card. Item `i` is `id + i`. `r` is the
/// trough; every segment is 22 px high plus the trough's 2 px padding, as wide as its label plus 22.
pub fn segmented(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, labels: &[&str], sel: usize) {
    c.fill(r, 8.0, look::WELL);
    let font = Font::new(12.0, 500);
    let mut x = r.x + 2.0;
    for (i, l) in labels.iter().enumerate() {
        let w = c.measure(l, font).ceil() + 22.0;
        let b = Rect::new(x, r.y + 2.0, w, r.h - 4.0);
        let hot = f.focusable(id + i as Id, b);
        f.hand();
        let on = f.tween(id + i as Id, 2, f32::from(i == sel), 200, EASE);
        if on > 0.0 {
            c.faded(on, |c| {
                c.shadow(b, 6.0, 1.0, 2.0, Rgba::css(0, 0, 0, 0.25));
                c.fill(b, 6.0, look::CARD);
                c.ring(b, 6.0, 1.0, Rgba::white(0.08));
            });
        }
        c.text(l, b, font, look::T3.mix(look::T1, on.max(0.5 * hot.hover)), Align::Center);
        ring(c, b, 6.0, hot);
        x += w;
    }
}

/// The width a [`segmented`] control needs.
pub fn segmented_width(c: &mut Canvas<'_>, labels: &[&str]) -> f32 {
    4.0 + labels.iter().map(|l| c.measure(l, Font::new(12.0, 500)).ceil() + 22.0).sum::<f32>()
}

/// A pill that filters or tags: `on` fills it with the accent's tint.
pub fn chip(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, label: &str, on: bool) -> Hot {
    let hot = f.focusable(id, r);
    f.hand();
    let t = f.tween(id, 2, f32::from(on), 160, EASE);
    c.fill(r, r.h / 2.0, Rgba::white(0.07 + 0.04 * hot.hover).mix(look::accent().alpha(0.22), t));
    c.ring(r, r.h / 2.0, 1.0, look::accent().alpha(0.5 * t));
    c.text(label, r, Font::new(12.0, 500), look::T2.mix(look::T1, t.max(hot.hover)), Align::Center);
    ring(c, r, r.h / 2.0, hot);
    hot
}

/// What a `chip` of this label needs.
pub fn chip_width(c: &mut Canvas<'_>, label: &str) -> f32 {
    c.measure(label, Font::new(12.0, 500)).ceil() + 24.0
}

/// A line of body text.
pub fn label(c: &mut Canvas<'_>, r: Rect, s: &str, ink: Rgba) {
    c.text(s, r, BODY, ink, Align::Start);
}
