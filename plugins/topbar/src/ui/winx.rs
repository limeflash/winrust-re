//! The Start button's menu: right click on Start and Win+X. A row of four tiles, the tools as rows with an underlined key
//! (a letter chooses the row) and the keys of what has them, and a submenu for shutting down and signing out that stands beside
//! it on a glass of its own (the mockup's `.smenu` and `.smsub`).
//!
//! The submenu is drawn by [`Content::overlay`] outside the box: it opens when the pointer is on its row (or the keyboard is) and
//! stays while the pointer crosses the 6 px between the two.

use std::cell::Cell;
use std::rc::Rc;

use winrust_common::style::Side;

use super::anim::{EASE, SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::menu::{DANGER, DIM, INK, MUTED, cap, cap_width};
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Look, Mods, Origin, Place, Pose, Shadow};
use super::taskview::OwnedIcon;

const W: f32 = 296.0;
const PAD: f32 = 8.0;
const TILE_H: f32 = 66.0;
const ROW: f32 = 32.0;
const SUB_W: f32 = 210.0;
const SUB_GAP: f32 = 6.0;
/// What the submenu's shadow is given beyond its edge.
const SUB_SHADOW: f32 = 40.0;
const CHEVRON: &str = "M9 6l6 6-6 6";
const SUB_INK: Rgba = Rgba::css(255, 179, 181, 1.0);

// the ids of what can be hit
const TILE: i32 = 0;
const ROWS: i32 = 10;
const SUBS: i32 = 100;
/// The submenu's own tweens.
const SUB_T: i32 = 200;

/// What a tile shows: the program's own icon, or a drawing on a plate.
pub enum Glyph {
    Icon(Rc<OwnedIcon>),
    /// A path of the 24-unit icon grid on a diagonal gradient (`linear-gradient(135deg, from, to)`).
    Plate(&'static str, Rgba, Rgba),
}

pub struct Tile {
    pub label: String,
    pub glyph: Glyph,
}

/// A label with the letter that chooses it set apart: `pre` `mn` `post`.
struct Label {
    pre: String,
    mn: String,
    post: String,
}

impl Label {
    /// Splits at the first `mn` in the label (as the mockup's `mnSplit`). A label that has none (the key is a Latin letter and
    /// the label is not Latin) names it in brackets at the end, like Windows does.
    fn new(label: &str, mn: char) -> Self {
        match label.char_indices().find(|(_, c)| c.eq_ignore_ascii_case(&mn)) {
            Some((i, c)) => Self { pre: label[..i].into(), mn: c.to_string(), post: label[i + c.len_utf8()..].into() },
            None if label.is_ascii() => Self { pre: label.into(), mn: String::new(), post: String::new() },
            None => Self { pre: format!("{label} ("), mn: mn.to_string(), post: ")".into() },
        }
    }
}

pub struct Item {
    icon: &'static str,
    label: Label,
    mn: char,
    keys: &'static str,
    sub: bool,
    danger: bool,
}

impl Item {
    #[must_use]
    pub fn new(icon: &'static str, label: &str, mn: char) -> Self {
        Self { icon, label: Label::new(label, mn), mn, keys: "", sub: false, danger: false }
    }

    /// The shortcut named on its right (one cap: "Win+S").
    #[must_use]
    pub const fn keys(mut self, keys: &'static str) -> Self {
        self.keys = keys;
        self
    }

    /// Choosing it opens the submenu.
    #[must_use]
    pub const fn submenu(mut self) -> Self {
        self.sub = true;
        self
    }

    /// Red: shutting down.
    #[must_use]
    pub const fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

pub enum Row {
    Item(Item),
    /// A hairline with 4 px above and below.
    Rule,
}

/// What was chosen; the indexes are those of the lists the panel was built with (a rule counts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Tile(usize),
    Row(usize),
    Sub(usize),
}

pub struct WinxPanel {
    tiles: Vec<Tile>,
    rows: Vec<Row>,
    sub: Vec<Row>,
    tile_at: Vec<Rect>,
    row_at: Vec<Rect>,
    /// The y of each rule of the menu, and of the submenu.
    rules: Vec<f32>,
    sub_rules: Vec<f32>,
    sub_at: Vec<Rect>,
    card: Rect,
    size: (f32, f32),
    side: Side,
    /// The row that opens the submenu.
    shut: Option<usize>,
    sub_open: Cell<bool>,
    pick: Rc<dyn Fn(Pick)>,
}

impl WinxPanel {
    /// `max_h` is the room there is (DIPs): a taller menu shrinks its rows to fit (down to 26). `sub_left`: the submenu goes to
    /// the left of the menu (no room on the right, or the bar is on the right).
    #[must_use]
    pub fn new(tiles: Vec<Tile>, rows: Vec<Row>, sub: Vec<Row>, max_h: f32, side: Side, sub_left: bool, pick: impl Fn(Pick) + 'static) -> Self {
        let items = rows.iter().filter(|r| matches!(r, Row::Item(_))).count() as f32;
        let rules = (rows.len() as f32) - items;
        let fixed = 2.0 * PAD + TILE_H + 8.0 + (items - 1.0).max(0.0) * 2.0 + rules * 11.0;
        let rh = ((max_h - fixed) / items.max(1.0)).clamp(26.0, ROW);

        let tw = (W - 2.0 * PAD - 3.0 * 4.0) / 4.0;
        let tile_at = (0..tiles.len()).map(|i| Rect::new(PAD + i as f32 * (tw + 4.0), PAD, tw, TILE_H)).collect();
        // the rows: 2 px apart, a rule takes 11 more (2 + 4 + 1 + 4, and the 2 after it is the usual gap)
        let (mut y, mut gap, mut first) = (PAD + TILE_H + 6.0 + 2.0, 0.0, true);
        let (mut row_at, mut rule_ys) = (Vec::new(), Vec::new());
        for r in &rows {
            match r {
                Row::Rule => {
                    rule_ys.push(y + gap + 6.0);
                    gap += 11.0;
                    row_at.push(Rect::default());
                }
                Row::Item(_) => {
                    if !first {
                        y += gap + 2.0;
                    }
                    first = false;
                    gap = 0.0;
                    row_at.push(Rect::new(PAD, y, W - 2.0 * PAD, rh));
                    y += rh;
                }
            }
        }
        // `y` is the bottom of the last row; a rule's y was reckoned from the row above it
        let height = y + PAD;
        let shut = rows.iter().position(|r| matches!(r, Row::Item(i) if i.sub));

        // the submenu: 210 wide, padding 6, rows of 32 (2 apart), its bottom 8 below its row's
        let sub_items = sub.iter().filter(|r| matches!(r, Row::Item(_))).count() as f32;
        let sub_rules = sub.len() as f32 - sub_items;
        let sub_h = 12.0 + sub_items * ROW + sub_rules * 9.0 + (sub.len() as f32 - 1.0).max(0.0) * 2.0;
        let bottom = shut.map_or(height - PAD, |i| row_at[i].y + row_at[i].h + 8.0);
        let x = if sub_left { PAD - SUB_GAP - SUB_W } else { W - PAD + SUB_GAP };
        let card = Rect::new(x, bottom - sub_h, SUB_W, sub_h);
        let (mut sy, mut sub_at, mut sub_rule_ys) = (card.y + 6.0, Vec::new(), Vec::new());
        for r in &sub {
            match r {
                Row::Item(_) => {
                    sub_at.push(Rect::new(card.x + 6.0, sy, SUB_W - 12.0, ROW));
                    sy += ROW + 2.0;
                }
                Row::Rule => {
                    sub_at.push(Rect::default());
                    sub_rule_ys.push(sy + 4.0);
                    sy += 9.0 + 2.0;
                }
            }
        }
        Self { tiles, rows, sub, tile_at, row_at, rules: rule_ys, sub_rules: sub_rule_ys, sub_at, card, size: (W, height), side, shut, sub_open: Cell::new(false), pick: Rc::new(pick) }
    }

    /// The id of the row that opens the submenu.
    #[must_use]
    pub fn shut_id(&self) -> Option<i32> {
        self.shut.map(|i| ROWS + i as i32)
    }

    fn item_ids(&self) -> Vec<i32> {
        self.rows.iter().enumerate().filter(|(_, r)| matches!(r, Row::Item(_))).map(|(i, _)| ROWS + i as i32).collect()
    }

    fn sub_ids(&self) -> Vec<i32> {
        self.sub.iter().enumerate().filter(|(_, r)| matches!(r, Row::Item(_))).map(|(i, _)| SUBS + i as i32).collect()
    }

    /// Choosing item `id`.
    fn choose(&self, id: i32, cx: &mut Cx) {
        let pick = Rc::clone(&self.pick);
        let what = match id {
            i if (TILE..ROWS).contains(&i) => Pick::Tile((i - TILE) as usize),
            i if (ROWS..SUBS).contains(&i) => {
                if self.shut_id() == Some(i) {
                    return self.sub_open.set(true);
                }
                Pick::Row((i - ROWS) as usize)
            }
            i => Pick::Sub((i - SUBS) as usize),
        };
        cx.close_then(move || pick(what));
    }

    /// One row of the menu or the submenu: the plate, the icon, the label with its key underlined, the cap and the chevron.
    fn draw_row(c: &mut Canvas<'_>, r: Rect, hot: f32, item: &Item) {
        c.fill(r, 9.0, Rgba::white(0.09 * hot));
        let (ink, icon) = if item.danger { (SUB_INK, DANGER) } else { (INK, DIM.mix(INK, hot)) };
        c.icon(item.icon, (r.x + 10.0 + 8.0, r.cy()), 16.0, 1.8, icon, 0.0);
        let mut right = r.x + r.w - 10.0;
        if item.sub {
            c.icon(CHEVRON, (right - 6.0, r.cy()), 12.0, 2.4, MUTED, 0.0);
        } else if !item.keys.is_empty() {
            let w = cap_width(c, item.keys);
            cap(c, right - w, r.cy(), item.keys, MUTED, 0.07);
            right -= w;
        }
        let font = Font::new(13.0, 400);
        let mut x = r.x + 10.0 + 16.0 + 11.0;
        let l = &item.label;
        if l.mn.is_empty() {
            return c.text_fit(&l.pre, Rect::new(x, r.y, (right - 8.0 - x).max(10.0), r.h), font, ink, Align::Start);
        }
        for (k, part) in [&l.pre, &l.mn, &l.post].into_iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            let w = c.measure(part, font);
            c.text(part, Rect::new(x, r.y, w + 2.0, r.h), font, ink, Align::Start);
            if k == 1 {
                // `text-decoration-color: rgba(255,255,255,.4); text-underline-offset: 2px`
                c.fill(Rect::new(x, r.cy() + 6.5, w, 1.0), 0.0, Rgba::white(0.4));
            }
            x += w;
        }
    }
}

impl Content for WinxPanel {
    fn id(&self) -> Id {
        Id::Menu
    }

    /// `.smenu`: `rgba(16,17,24,.86)`, blur 40, radius 18, `0 30px 80px rgba(0,0,0,.6)`.
    fn look(&self) -> Look {
        Look::glass(18.0, 0.86, Shadow { dy: 30.0, blur: 80.0, color: Rgba::css(0, 0, 0, 0.6) })
    }

    /// `smIn`: from `opacity 0, translateY(8px) scale(.96)` (`smInV`: from the side) in 260 ms, around `24px 100%`.
    fn entry(&self) -> Entry {
        let (dx, dy, origin) = self.motion();
        Entry { from: Pose { opacity: 0.0, dx, dy, k: 0.96, clip: None }, dur: 260, ease: SNAP, origin }
    }

    /// `smOut`: to `opacity 0, translateY(4px) scale(.98)` in 140 ms.
    fn exit(&self) -> Exit {
        Exit { to: Pose { opacity: 0.0, dx: 0.0, dy: 4.0, k: 0.98, clip: None }, dur: 140, ease: EASE, origin: self.motion().2 }
    }

    fn place(&self) -> Place {
        Place { along: Along::Start, gap: if self.side == Side::Bottom { 8.0 } else { 12.0 } }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.size
    }

    fn reach(&self) -> f32 {
        SUB_GAP + SUB_W + SUB_SHADOW
    }

    fn settle(&self) -> u32 {
        500
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        // the pointer (or the keys) on the shutdown row opens the submenu; on another row of the menu it closes it
        match f.hover {
            Some(h) if self.shut_id() == Some(h) => self.sub_open.set(true),
            Some(h) if h < SUBS => self.sub_open.set(false),
            _ => {}
        }
        // the tiles rise in as one (`smiIn`: 5 px, 300 ms)
        let rise = f.rise(0, 300, SNAP, 5.0, 1.0);
        rise.draw(c, (W / 2.0, PAD + TILE_H / 2.0), |c| {
            for (i, (t, r)) in self.tiles.iter().zip(&self.tile_at).enumerate() {
                let id = TILE + i as i32;
                let hot = f.hover(id, 140, SPRING);
                let press = f.tween(id, 7, f32::from(f.press == Some(id)), 140, SPRING);
                c.transformed(r.cx(), r.cy(), 1.0 - 0.05 * press, 0.0, 0.0, |c| {
                    c.fill(*r, 12.0, Rgba::white(0.05 + 0.04 * hot));
                    let at = Rect::new(r.cx() - 13.0, r.y + 10.0, 26.0, 26.0);
                    match &t.glyph {
                        Glyph::Icon(icon) => c.hicon(icon.0, at, 1.0),
                        Glyph::Plate(d, from, to) => {
                            c.gradient_diagonal(at, 7.0, *from, *to);
                            c.icon(d, (at.cx(), at.cy()), 15.0, 2.0, Rgba::white(1.0), 0.0);
                        }
                    }
                    c.text_fit(&t.label, Rect::new(r.x + 2.0, at.y + 26.0 + 6.0, r.w - 4.0, 14.0), Font::new(11.0, 500), Rgba::css(201, 205, 216, 1.0), Align::Center);
                });
            }
        });
        for (i, (row, r)) in self.rows.iter().zip(&self.row_at).enumerate() {
            if let Row::Item(item) = row {
                let hot = f.hover(ROWS + i as i32, 140, SPRING);
                Self::draw_row(c, *r, hot, item);
            }
        }
        for y in &self.rules {
            c.fill(Rect::new(PAD + 8.0, *y, W - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.08));
        }
    }

    /// The submenu: opacity .14 s, slides 4 px in .18 s (`.smsub`), on `rgba(20,21,28,.94)` with its own rings and shadow.
    fn overlay(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let open = f32::from(self.sub_open.get());
        let shown = f.tween(SUB_T, 1, open, 140, EASE);
        let slide = f.tween(SUB_T, 2, open, 180, SPRING);
        if shown <= 0.002 {
            return;
        }
        let card = self.card;
        c.faded(shown, |c| {
            c.transformed(0.0, 0.0, 1.0, -4.0 * (1.0 - slide), 0.0, |c| {
                c.shadow(card, 14.0, 20.0, 50.0, Rgba::css(0, 0, 0, 0.55));
                c.fill(card, 14.0, Rgba::css(20, 21, 28, 0.94));
                c.ring(card, 14.0, 1.0, Rgba::white(0.1));
                c.ring(card.inset(-1.0), 15.0, 1.0, Rgba::css(0, 0, 0, 0.5));
                for (j, (row, r)) in self.sub.iter().zip(&self.sub_at).enumerate() {
                    if let Row::Item(item) = row {
                        let hot = f.hover(SUBS + j as i32, 140, SPRING);
                        Self::draw_row(c, *r, hot, item);
                    }
                }
                for y in &self.sub_rules {
                    c.fill(Rect::new(card.x + 6.0 + 8.0, *y, SUB_W - 12.0 - 16.0, 1.0), 0.0, Rgba::white(0.08));
                }
            });
        });
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        if self.sub_open.get()
            && let Some(j) = self.sub_at.iter().position(|r| r.contains(x, y))
        {
            return Some(SUBS + j as i32);
        }
        if let Some(i) = self.tile_at.iter().position(|r| r.contains(x, y)) {
            return Some(TILE + i as i32);
        }
        self.row_at.iter().position(|r| r.w > 0.0 && r.contains(x, y)).map(|i| ROWS + i as i32)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if b == Button::Left {
            self.choose(id, cx);
        }
    }

    fn enter(&mut self, hover: Option<i32>, _mods: Mods, cx: &mut Cx) {
        if let Some(id) = hover {
            self.choose(id, cx);
        }
    }

    /// Esc closes the submenu first.
    fn escape(&mut self) -> bool {
        self.sub_open.replace(false)
    }

    /// A letter chooses the row it is the key of (in the submenu while it is open).
    fn key(&mut self, vk: u32, cx: &mut Cx) -> bool {
        if !(0x41..=0x5A).contains(&vk) {
            return false;
        }
        let ch = vk as u8 as char;
        let (list, base) = if self.sub_open.get() { (&self.sub, SUBS) } else { (&self.rows, ROWS) };
        if let Some(i) = list.iter().position(|r| matches!(r, Row::Item(it) if it.mn == ch)) {
            self.choose(base + i as i32, cx);
        }
        true
    }

    /// Up and Down walk the rows (round, the tiles are not in the walk); Right on the shutdown row goes into the submenu, Left
    /// comes back.
    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let step = |ids: &[i32], from: Option<i32>, dir: Dir| -> Option<i32> {
            let n = ids.len() as i32;
            let at = from.and_then(|f| ids.iter().position(|&i| i == f)).map(|p| p as i32);
            let to = match (at, dir) {
                (_, Dir::First) | (None, Dir::Down) => 0,
                (_, Dir::Last) | (None, Dir::Up) => n - 1,
                (Some(p), Dir::Down) => (p + 1) % n,
                (Some(p), Dir::Up) => (p + n - 1) % n,
                _ => return None,
            };
            (n > 0).then(|| ids[to as usize])
        };
        if from.is_some_and(|f| f >= SUBS) {
            if dir == Dir::Left {
                self.sub_open.set(false);
                return self.shut_id();
            }
            return step(&self.sub_ids(), from, dir);
        }
        match dir {
            Dir::Right if from.is_some() && from == self.shut_id() => {
                self.sub_open.set(true);
                self.sub_ids().first().copied()
            }
            Dir::Left | Dir::Right => None,
            _ => step(&self.item_ids(), from, dir),
        }
    }
}

impl WinxPanel {
    /// How the box comes in (`smIn` / `smInV`): the offset and the point it grows from.
    fn motion(&self) -> (f32, f32, Origin) {
        match self.side {
            Side::Bottom => (0.0, 8.0, Origin::Px(24.0, self.size.1)),
            Side::Left => (-8.0, 0.0, Origin::Px(0.0, self.size.1 * 0.3)),
            Side::Right => (-8.0, 0.0, Origin::Px(self.size.0, self.size.1 * 0.3)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(max_h: f32) -> WinxPanel {
        let tiles = (0..4).map(|i| Tile { label: format!("T{i}"), glyph: Glyph::Plate("M0 0", Rgba::white(1.0), Rgba::white(1.0)) }).collect();
        let item = |m: char| Row::Item(Item::new("M0 0", "Installed apps", m));
        let rows = vec![item('F'), item('O'), Row::Rule, item('A'), Row::Rule, Row::Item(Item::new("M0 0", "Shut down or sign out", 'U').submenu()), item('D')];
        let sub = vec![item('L'), Row::Rule, item('S')];
        WinxPanel::new(tiles, rows, sub, max_h, Side::Bottom, false, |_| {})
    }

    #[test]
    fn the_menu_is_laid_out_as_the_mockups_is() {
        // 8 + tiles 66 + 6 + 2 = 82; rows of 32 two apart; a rule is 11 more; 8 at the bottom
        let m = menu(900.0);
        assert_eq!(m.row_at[0].y, 82.0);
        assert_eq!(m.row_at[1].y, 116.0);
        assert_eq!(m.row_at[3].y, 116.0 + 32.0 + 13.0);
        assert_eq!(m.size.1, 280.0);
        let sub = menu(900.0).card;
        assert_eq!((sub.x, sub.w), (W - PAD + SUB_GAP, SUB_W));
    }

    #[test]
    fn a_pointer_finds_the_row_and_the_submenu_only_when_it_is_open() {
        let m = menu(900.0);
        let r = m.row_at[3];
        assert_eq!(m.hit(r.cx(), r.cy()), Some(ROWS + 3));
        let s = m.sub_at[0];
        assert_eq!(m.hit(s.cx(), s.cy()), None);
        m.sub_open.set(true);
        assert_eq!(m.hit(s.cx(), s.cy()), Some(SUBS));
    }

    #[test]
    fn a_label_without_its_key_names_it_in_brackets_unless_it_is_latin() {
        let l = Label::new("Installed apps", 'F');
        assert_eq!((l.pre.as_str(), l.mn.as_str()), ("Installed apps", ""));
        let l = Label::new("Диспетчер задач", 'K');
        assert_eq!((l.pre.as_str(), l.mn.as_str(), l.post.as_str()), ("Диспетчер задач (", "K", ")"));
        let l = Label::new("Device Manager", 'M');
        assert_eq!((l.pre.as_str(), l.mn.as_str(), l.post.as_str()), ("Device ", "M", "anager"));
    }

    #[test]
    fn escape_closes_the_submenu_first_and_a_letter_chooses_in_the_open_one() {
        use std::cell::RefCell;
        let chosen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&chosen);
        let mut m = menu(900.0);
        m.pick = Rc::new(move |p| log.borrow_mut().push(p));
        assert!(!m.escape());
        m.sub_open.set(true);
        let mut cx = Cx::default();
        assert!(m.key(u32::from(b'S'), &mut cx)); // Sleep's key in the submenu, not Search's
        cx.run_for_test();
        assert_eq!(*chosen.borrow(), vec![Pick::Sub(2)]);
        assert!(m.escape() && !m.sub_open.get());
        assert!(!m.escape());
    }

    #[test]
    fn arrows_go_round_the_rows_and_into_the_submenu() {
        let m = menu(900.0);
        let ids = m.item_ids();
        assert_eq!(m.nav(None, Dir::Down), Some(ids[0]));
        assert_eq!(m.nav(Some(*ids.last().unwrap()), Dir::Down), Some(ids[0]));
        assert_eq!(m.nav(Some(ids[0]), Dir::Up), Some(*ids.last().unwrap()));
        let shut = m.shut_id().unwrap();
        assert_eq!(m.nav(Some(shut), Dir::Right), Some(SUBS));
        assert!(m.sub_open.get());
        assert_eq!(m.nav(Some(SUBS), Dir::Left), Some(shut));
        assert!(!m.sub_open.get());
    }
}
