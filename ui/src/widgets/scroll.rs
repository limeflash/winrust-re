//! A scrolled area with the mockups' thin bar (`scrollbar-width: thin`): 6 DIPs wide at rest, wider and brighter under the pointer,
//! and it can be dragged. The wheel glides (150 ms) for a notch and follows a touchpad at once.
//!
//! The app owns a [`Scroll`] and forwards what the window tells it:
//! * `App::wheel`: [`wheel`] (it knows where the pointer is);
//! * `App::down(bar_id(id))`: [`bar_down`]; `App::drag(bar_id(id))`: [`bar_drag`]; `App::release(bar_id(id))`: [`bar_up`].

use super::scroll_model::Scroll;
use crate::anim::{EASE, LINEAR};
use crate::frame::{Frame, Id};
use crate::gfx::{Canvas, Rect, Rgba};
use winrust_common::style::{self, Anim};

/// The part of the content that is in view, and where it is drawn.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The viewport on the window (DIPs).
    pub r: Rect,
    /// How far the content is scrolled: the content's `y` at the viewport's top.
    pub y: f32,
}

impl View {
    /// The content's `y` of the viewport's bottom.
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y + self.r.h
    }
    /// Where on the window the content's `y` is drawn.
    #[must_use]
    pub fn at(&self, content_y: f32) -> f32 {
        self.r.y + content_y - self.y
    }
}

/// The item of the bar of a scroll area named `id`.
#[must_use]
pub const fn bar_id(id: Id) -> Id {
    id + 1
}

/// The width of the strip along the right edge that takes the bar's clicks.
const STRIP: f32 = 14.0;

/// A scrolled area: `body` draws what is in `view` (everything it names is cut to the viewport), then the bar. `id` names the
/// viewport (the hover for the wheel), `id + 1` the bar.
pub fn scroll(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, s: &mut Scroll, content_h: f32, body: impl FnOnce(&mut Canvas<'_>, &mut Frame<'_>, &View)) {
    scroll_with(c, f, id, r, s, content_h, false, body);
}

/// [`scroll`]; `tab` makes the viewport a stop of Tab (a list).
#[allow(clippy::too_many_arguments)] // `scroll` and the one flag a list adds
pub(crate) fn scroll_with(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, s: &mut Scroll, content_h: f32, tab: bool, body: impl FnOnce(&mut Canvas<'_>, &mut Frame<'_>, &View)) {
    s.set_extent(content_h, r.h);
    s.area = r;
    if style::anim() == Anim::Off {
        s.settle();
    }
    // a glide is a tween as far as the frame loop goes: frames come while it runs
    f.tween(id, 5, s.serial() as f32, 150, LINEAR);
    if tab {
        f.focusable(id, r);
    } else {
        f.item(id, r);
    }
    let view = View { r, y: s.offset(f.now) };
    f.clipped(c, r, |c, f| body(c, f, &view));
    if !s.scrollable() {
        s.track = (0.0, 0.0);
        return;
    }
    let strip = Rect::new(r.x + r.w - STRIP, r.y + 2.0, STRIP, (r.h - 4.0).max(1.0));
    s.track = (strip.y, strip.h);
    let hot = f.item(bar_id(id), strip);
    let held = f32::from(s.dragging());
    let wide = hot.hover.max(f.tween(bar_id(id), 4, held, 160, EASE));
    let (t0, th) = s.thumb(strip.h, f.now);
    let w = 6.0 + 4.0 * wide;
    c.fill(Rect::new(strip.x + STRIP - 2.0 - w, strip.y + t0, w, th), w / 2.0, Rgba::white(0.14 + 0.14 * wide));
}

/// The wheel, `dy` notches (positive scrolls up, as `App::wheel` has it), over the pointer's place. True when the area took it.
pub fn wheel(s: &mut Scroll, pointer: Option<(f32, f32)>, dy: f32, line_h: f32, now: u64) -> bool {
    let Some((x, y)) = pointer else { return false };
    if !s.area.contains(x, y) || !s.scrollable() || dy == 0.0 {
        return false;
    }
    s.wheel(dy * 120.0, now, line_h);
    true
}

/// The button went down on the bar's strip (`y` on the window).
pub fn bar_down(s: &mut Scroll, y: f32, now: u64) -> bool {
    s.press(y, now)
}

/// The pointer moved with the thumb held.
pub fn bar_drag(s: &mut Scroll, y: f32, now: u64) {
    s.drag_to(y, now);
}

pub fn bar_up(s: &mut Scroll) {
    s.release();
}
