//! The menus of the bar: right click on an empty spot and on the clock (Win+X has its own panel, `winx`). The mockup's menus in one piece: the jump list's
//! glass (`rgba(16,17,24,.82)`, blur 40, radius 20, padding 8) and the desktop menu's rows (`.dmr`: 38 px, a 17 px icon,
//! the label, a hint on the right, `rgba(255,255,255,.08)` on hover, red for what destroys something), the sections divided
//! by a hairline and rising one after the other. It opens like the jump list does: unrolls from the bar.

use std::rc::Rc;

use winrust_common::style::Side;

use super::anim::{SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::panel::{Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Inset, Look, Origin, Place, Pose, Shadow};

pub const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
pub const DIM: Rgba = Rgba::css(163, 169, 184, 1.0);
pub const MUTED: Rgba = Rgba::css(138, 144, 162, 1.0);
pub const DANGER: Rgba = Rgba::pair(0xff8a8f, 0xc8333a); // `--dangerT`: pink on the dark style, deep red on the light one

pub const PAD: f32 = 8.0;
pub const ROW: f32 = 38.0;
/// A rule between sections: 1 px with 6 px above and below.
const RULE: f32 = 13.0;

/// What stands on the right of a row.
#[derive(Clone, Copy)]
pub enum Hint {
    None,
    /// Keys as little caps: `["Alt", "F4"]`.
    Keys(&'static [&'static str]),
}

pub struct Item {
    pub icon: &'static str,
    pub label: String,
    pub hint: Hint,
    pub danger: bool,
    /// A rule is drawn above it (a new section begins).
    pub rule: bool,
}

impl Item {
    #[must_use]
    pub fn new(icon: &'static str, label: impl Into<String>) -> Self {
        Self { icon, label: label.into(), hint: Hint::None, danger: false, rule: false }
    }

    #[must_use]
    pub const fn keys(mut self, k: &'static [&'static str]) -> Self {
        self.hint = Hint::Keys(k);
        self
    }

    #[must_use]
    pub const fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    #[must_use]
    pub const fn rule(mut self) -> Self {
        self.rule = true;
        self
    }
}

/// `.jli:nth-child(n)` delays of the jump list's sections, ms.
pub const DELAYS: [u32; 5] = [0, 30, 60, 80, 100];

/// How the glass of a bar's popup unrolls out of the bar and rolls back (`jlIn` / `jlOut`): from a 40 px stub at the bar
/// to the whole box. The mockup's bar is at the bottom; for a side bar the same motion turns toward the bar.
#[must_use]
pub fn reveal(side: Side, (w, h): (f32, f32)) -> (Entry, Exit) {
    let (dx, dy, clip) = match side {
        Side::Bottom => (0.0, 42.0, Inset::new(h - 40.0, w / 2.0 - 20.0, 0.0, w / 2.0 - 20.0, 10.0)),
        Side::Left => (-42.0, 0.0, Inset::new(h / 2.0 - 20.0, w - 40.0, h / 2.0 - 20.0, 0.0, 10.0)),
        Side::Right => (42.0, 0.0, Inset::new(h / 2.0 - 20.0, 0.0, h / 2.0 - 20.0, w - 40.0, 10.0)),
    };
    let origin = Origin::Frac(0.5, 1.0);
    let entry = Entry { from: Pose { opacity: 0.9, dx, dy, k: 1.0, clip: Some(clip) }, dur: 320, ease: SNAP, origin };
    let exit = Exit { to: Pose { opacity: 0.5, dx, dy, k: 1.0, clip: Some(clip) }, dur: 200, ease: super::anim::EXIT, origin };
    (entry, exit)
}

/// The jump list's glass: radius 20, `0 30px 80px rgba(0,0,0,.6)`.
#[must_use]
pub const fn look() -> Look {
    Look::glass(20.0, 0.82, Shadow { dy: 30.0, blur: 80.0, color: Rgba::css(0, 0, 0, 0.6) })
}

/// The width of the key cap of `text` (`kbd`: 11 px, weight 500, padding 1 x 6).
pub fn cap_width(c: &mut Canvas<'_>, text: &str) -> f32 {
    c.measure(text, Font::new(11.0, 500)).ceil() + 12.0
}

/// One key cap with its left edge at `x`, centred on `cy`: radius 4, `plate` white behind it and a lit lower edge.
pub fn cap(c: &mut Canvas<'_>, x: f32, cy: f32, text: &str, ink: Rgba, plate: f32) -> f32 {
    let w = cap_width(c, text);
    let r = Rect::new(x, cy - 7.5, w, 15.0);
    c.fill(r, 4.0, Rgba::white(plate));
    c.fill(Rect::new(r.x + 1.5, r.y + r.h - 1.0, r.w - 3.0, 1.0), 0.0, Rgba::white(0.08));
    c.text(text, r, Font::new(11.0, 500), ink, Align::Center);
    w
}

/// One key cap of a row's hint; returns where the next one to its left ends.
pub fn kbd(c: &mut Canvas<'_>, right: f32, cy: f32, text: &str) -> f32 {
    let w = cap_width(c, text);
    cap(c, right - w, cy, text, DIM, 0.07);
    right - w - 3.0
}

/// A row of a menu: the hover plate, the icon, the label and the hint. `hot` is the hover 0..1.
pub fn draw_row(c: &mut Canvas<'_>, r: Rect, hot: f32, danger: bool, icon: &'static str, label: &str, hint: Hint) {
    let plate = if danger { Rgba::css(229, 72, 77, 0.14) } else { Rgba::white(0.08) };
    draw_row_ink(c, r, plate.alpha(plate.a * hot), icon, if danger { DANGER } else { DIM }, if danger { DANGER } else { INK }, label, hint);
}

/// [`draw_row`] with its colours given: the plate as it is now, the icon's and the label's.
#[allow(clippy::too_many_arguments)]
pub fn draw_row_ink(c: &mut Canvas<'_>, r: Rect, plate: Rgba, icon: &'static str, icon_ink: Rgba, ink: Rgba, label: &str, hint: Hint) {
    c.fill(r, 10.0, plate);
    c.icon(icon, (r.x + 10.0 + 8.5, r.cy()), 17.0, 1.8, icon_ink, 0.0);
    let mut right = r.x + r.w - 10.0;
    if let Hint::Keys(keys) = hint {
        for k in keys.iter().rev() {
            right = kbd(c, right, r.cy(), k);
        }
        right += 3.0;
    }
    let x = r.x + 10.0 + 17.0 + 12.0;
    c.text_fit(label, Rect::new(x, r.y, (right - 8.0 - x).max(10.0), r.h), Font::new(13.0, 400), ink, Align::Start);
}

struct Placed {
    rect: Rect,
    section: usize,
}

pub struct MenuPanel {
    items: Vec<Item>,
    rows: Vec<Placed>,
    /// The y of each rule.
    rules: Vec<f32>,
    size: (f32, f32),
    side: Side,
    place: Place,
    pick: Rc<dyn Fn(usize)>,
}

impl MenuPanel {
    /// `max_h` is the room there is (DIPs): a menu that is taller shrinks its rows to fit (down to 30).
    #[must_use]
    pub fn new(items: Vec<Item>, width: f32, max_h: f32, side: Side, pick: impl Fn(usize) + 'static) -> Self {
        let rules = items.iter().skip(1).filter(|i| i.rule).count() as f32;
        let n = items.len() as f32;
        let fixed = 2.0 * PAD + rules * RULE + (n - 1.0 - rules).max(0.0) * 1.0;
        let row = ((max_h - fixed) / n.max(1.0)).clamp(30.0, ROW);
        let (mut y, mut section) = (PAD, 0);
        let (mut rows, mut rule_ys) = (Vec::new(), Vec::new());
        for (i, it) in items.iter().enumerate() {
            if it.rule && i > 0 {
                rule_ys.push(y + 6.0);
                y += RULE;
                section += 1;
            } else if i > 0 {
                y += 1.0;
            }
            rows.push(Placed { rect: Rect::new(PAD, y, width - 2.0 * PAD, row), section });
            y += row;
        }
        Self { items, rows, rules: rule_ys, size: (width, y + PAD), side, place: Place { gap: 8.0, ..Place::default() }, pick: Rc::new(pick) }
    }

    #[must_use]
    pub const fn height(&self) -> f32 {
        self.size.1
    }
}

impl Content for MenuPanel {
    fn id(&self) -> Id {
        Id::Menu
    }

    fn look(&self) -> Look {
        look()
    }

    fn entry(&self) -> Entry {
        reveal(self.side, self.size).0
    }

    fn exit(&self) -> Exit {
        reveal(self.side, self.size).1
    }

    fn place(&self) -> Place {
        self.place
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.size
    }

    fn settle(&self) -> u32 {
        500
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let out = f.fade_out(100);
        let sections = self.rows.last().map_or(0, |r| r.section + 1);
        for s in 0..sections {
            let rise = f.rise(DELAYS[s.min(DELAYS.len() - 1)], 300, SNAP, 12.0, 1.0);
            let (lo, hi) = (self.rows.iter().position(|r| r.section == s).unwrap_or(0), self.rows.iter().rposition(|r| r.section == s).unwrap_or(0));
            let top = self.rows[lo].rect;
            c.faded(out, |c| {
                rise.draw(c, (top.cx(), top.y), |c| {
                    for i in lo..=hi {
                        let (it, r) = (&self.items[i], self.rows[i].rect);
                        let hot = f.hover(i as i32, 140, SPRING);
                        draw_row(c, r, hot, it.danger, it.icon, &it.label, it.hint);
                    }
                    if s > 0 {
                        let y = self.rules[s - 1];
                        c.fill(Rect::new(PAD + 8.0, y, self.size.0 - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.07));
                    }
                });
            });
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        self.rows.iter().position(|r| r.rect.contains(x, y)).map(|i| i as i32)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if b == Button::Middle {
            return;
        }
        let pick = Rc::clone(&self.pick);
        let i = id as usize;
        cx.close_then(move || pick(i));
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let n = self.items.len() as i32;
        let i = match (from, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n - 1,
            (Some(i), Dir::Up | Dir::Left) => i - 1,
            (Some(i), Dir::Down | Dir::Right) => i + 1,
        };
        (n > 0).then(|| i.clamp(0, n - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<Item> {
        (0..n).map(|i| if i == n - 4 { Item::new("M0 0", "x").rule() } else { Item::new("M0 0", "x") }).collect()
    }

    #[test]
    fn a_tall_menu_shrinks_its_rows_to_the_room_there_is() {
        let roomy = MenuPanel::new(items(12), 288.0, 900.0, Side::Bottom, |_| {});
        let tight = MenuPanel::new(items(12), 288.0, 450.0, Side::Bottom, |_| {});
        assert_eq!(roomy.rows[0].rect.h, ROW);
        assert!(tight.rows[0].rect.h < ROW && tight.rows[0].rect.h >= 30.0);
        assert!(tight.height() <= 450.5, "{}", tight.height());
    }

    #[test]
    fn rows_answer_to_their_own_rectangle_and_a_rule_opens_a_section() {
        let m = MenuPanel::new(items(8), 288.0, 900.0, Side::Bottom, |_| {});
        let r = m.rows[2].rect;
        assert_eq!(m.hit(r.cx(), r.cy()), Some(2));
        assert_eq!(m.hit(2.0, 2.0), None);
        assert_eq!(m.rows[3].section, 0);
        assert_eq!(m.rows[4].section, 1);
        assert_eq!(m.rules.len(), 1);
    }
}
