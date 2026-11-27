//! The text field of the mockups' inputs (the search box, the address, a rename): a well with a hairline, a caret that blinks on a
//! one-shot timer, a selection, the pointer through DirectWrite's hit test, the keys, and the place for the IME's windows.
//!
//! The text is a [`TextField`] the app owns. The app forwards what the window tells it:
//! * `App::down(id)`: `tf.mouse_down(x, cx.mods.shift, now)`; `App::drag(id)`: `tf.mouse_drag(x, now)`; `App::release(id)`: `tf.mouse_up()`;
//! * `App::key`: [`key`] when the field has the focus (`cx.focus == Some(id)`); `App::text`: [`text`];
//! * `App::click(id + 1)` is the clear button of a [`FieldStyle::search`] field: `tf.set_text("")`.

use super::field_model::{Clip, Ev, TextField};
use super::look::{self, BODY};
use crate::anim::EASE;
use crate::frame::{Cursor, Cx, Frame, Hot, Id, Mods};
use crate::gfx::{Align, Canvas, Font, Rect, Rgba};

/// What the box looks like.
#[derive(Clone, Copy, Debug)]
pub struct FieldStyle {
    pub font: Font,
    /// A 24 x 24 icon path at the left (the search box's magnifier).
    pub icon: Option<&'static str>,
    /// A round clear button at the right while there is text: item `id + 1`.
    pub clear: bool,
    /// No box of its own: the app drew one (a rename inside a row).
    pub bare: bool,
    pub radius: f32,
    /// Space between the box and the text.
    pub pad: f32,
}

impl FieldStyle {
    /// `.srch` without its icon: 13 px text in a well of radius 10.
    pub const INPUT: FieldStyle = FieldStyle { font: BODY, icon: None, clear: false, bare: false, radius: 10.0, pad: 10.0 };
    /// The Explorer's search box: a magnifier, and a clear button once there is something to clear.
    pub const SEARCH: FieldStyle = FieldStyle { icon: Some("M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4"), clear: true, ..Self::INPUT };
}

/// The caret blinks every 530 ms, from the last time it moved.
pub const BLINK_MS: u64 = 530;

/// Ctrl+C / X / V through the window's clipboard (`Cx`).
pub struct CxClip<'a>(pub &'a mut Cx);

impl Clip for CxClip<'_> {
    fn get(&mut self) -> Option<String> {
        self.0.clipboard_text()
    }
    fn set(&mut self, text: &str) {
        self.0.set_clipboard_text(text);
    }
}

/// A key for the focused field, with the window's clipboard (see [`TextField::key`] for what it is told).
pub fn key(tf: &mut TextField, cx: &mut Cx, vk: u32, m: Mods, now: u64) -> Ev {
    let alt = cx.alt;
    tf.key(vk, m, alt, now, &mut CxClip(cx))
}

/// Typed text (`App::text`) for the focused field.
pub fn text(tf: &mut TextField, s: &str, now: u64) {
    tf.typed(s, now);
}

/// A field with the default look.
pub fn field(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, tf: &mut TextField, placeholder: &str) -> Hot {
    field_with(c, f, id, r, tf, placeholder, &FieldStyle::INPUT)
}

/// Draws the field into `r`, names it (`id`, and `id + 1` for the clear button), and does what the pointer asked of the caret since
/// the last frame.
pub fn field_with(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, tf: &mut TextField, placeholder: &str, st: &FieldStyle) -> Hot {
    let hot = f.focusable(id, r);
    f.cursor(Cursor::IBeam);
    let focused = f.focused(id);
    if focused && !tf.view.focused {
        tf.view.touched = f.now; // the caret starts visible
    }
    tf.view.focused = focused;

    // the box
    let ring = f.tween(id, 3, f32::from(focused), 200, EASE);
    if !st.bare {
        c.fill(r, st.radius, look::WELL);
        c.ring(r, st.radius, 1.0, Rgba::white(0.06));
        c.ring(r, st.radius, 1.5, look::accent().alpha(0.7 * ring));
    }

    // what is inside: [icon] text [clear]
    let mut inner = Rect::new(r.x + st.pad, r.y, r.w - 2.0 * st.pad, r.h);
    if let Some(d) = st.icon {
        c.icon(d, (inner.x + 7.5, r.cy()), 15.0, 2.0, look::T3, 0.0);
        inner.x += 23.0;
        inner.w -= 23.0;
    }
    if st.clear && !tf.text.is_empty() {
        let b = Rect::new(r.x + r.w - st.pad - 20.0, r.cy() - 10.0, 20.0, 20.0);
        let hb = f.item(id + 1, b);
        f.hand();
        c.circle(b.cx(), b.cy(), 10.0, Rgba::white(0.1 + 0.08 * hb.hover));
        c.icon("M6 6l12 12M18 6L6 18", (b.cx(), b.cy()), 8.0, 3.4, look::T2, 0.0);
        inner.w -= 28.0;
    }
    let inner = Rect::new(inner.x, inner.y, inner.w.max(1.0), inner.h);
    tf.view.text_x = inner.x;
    tf.view.inner_w = inner.w;

    // where the text is: what the pointer asked is answered now, by the layout of what is drawn
    let shown = tf.display();
    let font = st.font;
    if let Some((x, _)) = tf.view.pending {
        let idx = c.text_hit(&shown, font, x);
        tf.aim(idx);
    }
    let caret_x = c.text_x(&shown, font, tf.idx16(tf.caret));
    let text_w = c.measure(&shown, font);
    tf.reveal(caret_x, text_w, inner.w);
    let (ox, lh) = (inner.x - tf.scroll_x, (font.size * 1.3).round());
    let line = Rect::new(ox, r.cy() - lh / 2.0, 4096.0, lh);

    c.clipped(inner, |c| {
        if tf.text.is_empty() {
            c.text(placeholder, Rect::new(inner.x, r.y, inner.w, r.h), font, look::T3, Align::Start);
        } else {
            let sel = tf.selection();
            if focused && !sel.is_empty() {
                let (a, b) = (c.text_x(&shown, font, tf.idx16(sel.start)), c.text_x(&shown, font, tf.idx16(sel.end)));
                c.fill(Rect::new(ox + a, line.y, b - a, lh), 0.0, look::accent().alpha(0.45));
            }
            c.text(&shown, Rect::new(ox, r.y, 4096.0, r.h), font, look::T1, Align::Start);
        }
        if focused {
            // the caret: on for half a second, off for half a second, from the last time it moved; a frame is asked for at the next change
            let since = f.now.saturating_sub(tf.view.touched);
            if (since / BLINK_MS).is_multiple_of(2) {
                c.fill(Rect::new(ox + caret_x, line.y, 1.0, lh), 0.0, look::accent());
            }
            f.due(tf.view.touched + (since / BLINK_MS + 1) * BLINK_MS);
        }
    });
    if focused {
        // the IME puts its composition and candidate windows here
        f.mo.hits.caret = Some(Rect::new(ox + caret_x, line.y, 1.0, lh));
    }
    hot
}
