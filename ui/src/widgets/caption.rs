//! The window's caption: the bar of 48 DIPs with the app's tile, its title and the three round window buttons (`winBtns` of every
//! mockup). The host answers for the buttons (`WIN_MIN`, `WIN_MAX`, `WIN_CLOSE`), and tells Windows the maximize button is one so
//! that Windows 11 shows its Snap Layouts over it; the empty rest of the bar drags the window.

use super::look::{self, BODY};
use crate::anim::SPRING;
use crate::frame::{Frame, WIN_CLOSE, WIN_MAX, WIN_MIN};
use crate::gfx::{Align, Canvas, Font, Rect, Rgba};

/// What the bar shows.
pub struct Caption<'a> {
    /// `--bar`.
    pub bg: Rgba,
    pub title: &'a str,
    /// The small grey text after the title ("3 of 5"); may be empty.
    pub sub: &'a str,
    /// A 24 × 24 icon path drawn in white on the tile.
    pub glyph: &'static str,
    /// The tile's gradient, `linear-gradient(135deg, a, b 55%, c)`.
    pub tile: [Rgba; 3],
    pub max: bool,
    /// How much room the app keeps for its own tools between the title and the window buttons.
    pub tools_w: f32,
}

const MIN_D: &str = "M6 12h12";
const MAX_D: &str = "M6.5 6.5h11v11h-11z";
const RESTORE_D: &str = "M8.5 8.5h9v9h-9zM6 15V6h9";
const CLOSE_D: &str = "M7 7l10 10M17 7L7 17";

/// Draws the caption into `r` (draw it after the body: its hairline lies on the body's first row) and returns the place for the
/// app's tools, `tools_w` wide, right-aligned before the separator.
pub fn caption(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, cap: &Caption<'_>) -> Rect {
    c.fill(r, 0.0, cap.bg);
    c.fill(Rect::new(r.x, r.y + r.h, r.w, 1.0), 0.0, Rgba::white(0.06));

    // right to left: buttons (26, gap 8, 10 from the edge), the separator (1, margin 6, gap 10), the tools
    let btns_x = r.x + r.w - 10.0 - 94.0;
    let sep_x = btns_x - 10.0 - 6.0 - 1.0;
    c.fill(Rect::new(sep_x, r.cy() - 10.0, 1.0, 20.0), 0.0, Rgba::white(0.08));
    let tools = Rect::new(sep_x - 6.0 - 10.0 - cap.tools_w, r.y, cap.tools_w, r.h);

    // the tile, the title and its small text
    let tile = Rect::new(r.x + 14.0, r.cy() - 12.0, 24.0, 24.0);
    c.gradient_diagonal_stops(tile, 7.0, &[(0.0, cap.tile[0]), (0.55, cap.tile[1]), (1.0, cap.tile[2])]);
    c.icon(cap.glyph, (tile.cx(), tile.cy()), 14.0, 2.2, Rgba::pure_white(1.0), 0.0);
    let x = tile.x + 24.0 + 10.0;
    let room = (if cap.tools_w > 0.0 { tools.x - 10.0 } else { sep_x - 10.0 }) - x;
    let title_f = Font::new(13.0, 600);
    let tw = c.measure(cap.title, title_f).ceil().min(room.max(0.0));
    c.text_fit(cap.title, Rect::new(x, r.y, tw, r.h), title_f, look::T1, Align::Start);
    if !cap.sub.is_empty() {
        let sx = x + tw + 8.0;
        c.text_fit(cap.sub, Rect::new(sx, r.y + 1.0, (x + room - sx).max(0.0), r.h), Font::new(12.0, 400).tabular(), look::T3, Align::Start);
    }

    let max_d = if cap.max { RESTORE_D } else { MAX_D };
    for (i, (id, d)) in [(WIN_MIN, MIN_D), (WIN_MAX, max_d), (WIN_CLOSE, CLOSE_D)].into_iter().enumerate() {
        let b = Rect::new(btns_x + i as f32 * 34.0, r.cy() - 13.0, 26.0, 26.0);
        let hot = f.item(id, b);
        f.hand();
        let glide = f.tween(id, 2, f32::from(f.hover == Some(id)), 280, SPRING); // the icon's own motion, `.wb svg`
        let (bg, ink) = if id == WIN_CLOSE {
            (Rgba::white(0.07).mix(Rgba::css(229, 72, 77, 0.22), hot.hover), look::T2.mix(look::RED, hot.hover))
        } else {
            (Rgba::white(0.07 + 0.09 * hot.hover), look::T2.mix(look::T1, hot.hover))
        };
        let k = 1.0 - 0.14 * hot.press;
        c.transformed(b.cx(), b.cy(), k, 0.0, 0.0, |c| {
            c.fill(b, 13.0, bg);
            let (dy, scale, turn) = match id {
                WIN_MIN => (2.0 * glide, 1.0, 0.0),
                WIN_MAX => (0.0, 1.0 + 0.2 * glide, 0.0),
                _ => (0.0, 1.0, 90.0 * glide),
            };
            c.icon(d, (b.cx(), b.cy() + dy), 12.0 * scale, 2.2, ink, turn);
        });
        if hot.focus {
            c.ring(b.inset(-3.0), 16.0, 2.0, look::focus_ring().fade(0.9));
        }
    }
    let _ = BODY;
    tools
}
