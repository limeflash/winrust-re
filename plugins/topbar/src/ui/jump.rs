//! The jump list: the right click on an app button of the bar. The app (its icon in a tile, its name, "Pinned · 2 windows" with
//! a status dot), its windows (a row each, a close cross on hover), the app's recent files when Windows keeps a list for it,
//! the actions (new window, unpin, move, Start), and below a rule: close the windows, end the process in red.
//! Glass `rgba(16,17,24,.82)`, blur 40, radius 20, 288 wide; it unrolls from the button (`jlIn`) and its parts rise one after
//! the other (`jliIn`, 30 ms apart).

use std::rc::Rc;

use winrust_common::style::{Side, pal};
use windows::Win32::UI::WindowsAndMessaging::HICON;

use super::anim::{SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::menu::{self, DELAYS, Hint, INK, MUTED, PAD, ROW};
use super::panel::{Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Look, Place};

const W: f32 = 288.0;
const WIN_ROW: f32 = 40.0;

const WIN_BASE: i32 = 0;
// a stride of 1000 per kind of row, so a long list of windows never meets the next kind
const CLOSE_BASE: i32 = 1000;
const RECENT_BASE: i32 = 2000;
const ACT_BASE: i32 = 3000;
const CLOSE_ALL: i32 = 4000;
const KILL: i32 = 4001;

const CROSS: &str = "M7 7l10 10M17 7L7 17";
const CLOSE_WINDOW: &str = "M4 5h16v14H4zM9.5 9.5l5 5M14.5 9.5l-5 5";
const END_PROCESS: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM5.6 5.6l12.8 12.8";
const FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h5";
const FOLDER: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6z";

/// What was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Bring this window to the front.
    Window(isize),
    CloseWindow(isize),
    /// The n-th recent file.
    Recent(usize),
    /// The n-th action, in the order the owner gave them.
    Act(usize),
    CloseAll,
    Kill,
}

pub struct Win {
    pub hwnd: isize,
    pub title: String,
    /// The one in front: an accent bar and a plate.
    pub active: bool,
}

pub struct Recent {
    pub name: String,
    pub folder: bool,
}

pub struct Act {
    pub icon: &'static str,
    pub label: String,
}

pub struct Data {
    pub icon: HICON,
    pub name: String,
    pub meta: String,
    pub running: bool,
    pub wins: Vec<Win>,
    pub recent: Vec<Recent>,
    pub acts: Vec<Act>,
    /// "Close window" / "Close all windows", when the app is running.
    pub close: Option<String>,
    pub kill: Option<String>,
}

pub struct JumpPanel {
    d: Data,
    pick: Rc<dyn Fn(Pick)>,
    side: Side,
    size: (f32, f32),
    /// Tops of the parts (header, windows, recent, acts, rule, close group) in DIPs.
    y: [f32; 6],
    caption: String,
}

fn kbd_alt_f4() -> &'static [&'static str] {
    &["Alt", "F4"]
}

impl JumpPanel {
    #[must_use]
    pub fn new(d: Data, side: Side, pick: impl Fn(Pick) + 'static) -> Self {
        let mut y = [0.0f32; 6];
        let mut at = PAD;
        y[0] = at;
        at += 8.0 + 44.0 + 12.0;
        y[1] = at;
        if !d.wins.is_empty() {
            let n = d.wins.len() as f32;
            at += 8.0 + n * WIN_ROW + (n - 1.0) * 2.0 + 6.0;
        }
        y[2] = at;
        if !d.recent.is_empty() {
            let n = d.recent.len() as f32;
            at += 26.0 + n * ROW + (n - 1.0) + 13.0;
        }
        y[3] = at;
        if !d.acts.is_empty() {
            let n = d.acts.len() as f32;
            at += n * ROW + (n - 1.0);
        }
        y[4] = at;
        let tail = usize::from(d.close.is_some()) + usize::from(d.kill.is_some());
        y[5] = at;
        if tail > 0 {
            at += 13.0;
            y[5] = at;
            at += tail as f32 * ROW + (tail as f32 - 1.0);
        }
        let caption = winrust_common::t!("Недавние", "Recent", ja: "最近使った項目", pt: "Recentes", zh: "最近").to_string();
        Self { d, pick: Rc::new(pick), side, size: (W, at + PAD), y, caption }
    }

    fn win_row(&self, i: usize) -> Rect {
        Rect::new(PAD + 4.0, self.y[1] + 4.0 + i as f32 * (WIN_ROW + 2.0), W - 2.0 * PAD - 8.0, WIN_ROW)
    }

    fn win_close(&self, i: usize) -> Rect {
        let r = self.win_row(i);
        Rect::new(r.x + r.w - 10.0 - 24.0, r.cy() - 12.0, 24.0, 24.0)
    }

    fn recent_row(&self, i: usize) -> Rect {
        Rect::new(PAD, self.y[2] + 26.0 + i as f32 * (ROW + 1.0), W - 2.0 * PAD, ROW)
    }

    fn act_row(&self, i: usize) -> Rect {
        Rect::new(PAD, self.y[3] + i as f32 * (ROW + 1.0), W - 2.0 * PAD, ROW)
    }

    fn tail_row(&self, i: usize) -> Rect {
        Rect::new(PAD, self.y[5] + i as f32 * (ROW + 1.0), W - 2.0 * PAD, ROW)
    }

    fn header(&self, c: &mut Canvas<'_>) {
        let tile = Rect::new(PAD + 8.0, PAD + 8.0, 44.0, 44.0);
        c.fill(tile, 12.0, Rgba::white(0.07));
        c.ring(tile, 12.0, 1.0, Rgba::white(0.06));
        c.hicon(self.d.icon, Rect::new(tile.cx() - 17.0, tile.cy() - 17.0, 34.0, 34.0), 1.0);
        let x = tile.x + 44.0 + 12.0;
        let top = tile.cy() - 17.5;
        c.text_fit(&self.d.name, Rect::new(x, top, W - PAD - 8.0 - x, 18.0), Font::new(15.0, 600), INK, Align::Start);
        let dot = if self.d.running { Rgba::css(76, 195, 138, 1.0) } else { Rgba::white(0.25) };
        c.circle(x + 3.0, top + 20.0 + 7.5, 3.0, dot);
        c.text_fit(&self.d.meta, Rect::new(x + 12.0, top + 20.0, W - PAD - 8.0 - x - 12.0, 15.0), Font::new(12.0, 400), MUTED, Align::Start);
    }
}

impl Content for JumpPanel {
    fn id(&self) -> Id {
        Id::Jump
    }

    fn look(&self) -> Look {
        menu::look()
    }

    fn entry(&self) -> Entry {
        menu::reveal(self.side, self.size).0
    }

    fn exit(&self) -> Exit {
        menu::reveal(self.side, self.size).1
    }

    fn place(&self) -> Place {
        // `bottom: calc(var(--pb) - 4px)`: 4 DIPs nearer the bar than the quick settings
        Place { gap: 8.0, ..Place::default() }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.size
    }

    fn settle(&self) -> u32 {
        500
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let out = f.fade_out(100);
        let mut part = 0usize;
        let mut next = |f: &Frame<'_>| {
            let d = DELAYS[part.min(DELAYS.len() - 1)];
            part += 1;
            f.rise(d, 300, SNAP, 12.0, 1.0)
        };
        let mid = W / 2.0;

        let rise = next(f);
        c.faded(out, |c| rise.draw(c, (mid, 0.0), |c| self.header(c)));

        if !self.d.wins.is_empty() {
            let rise = next(f);
            let n = self.d.wins.len() as f32;
            let list = Rect::new(PAD, self.y[1], W - 2.0 * PAD, 8.0 + n * WIN_ROW + (n - 1.0) * 2.0);
            c.faded(out, |c| {
                rise.draw(c, (mid, list.y), |c| {
                    c.fill(list, 14.0, Rgba::white(0.04));
                    for (i, w) in self.d.wins.iter().enumerate() {
                        let (r, id) = (self.win_row(i), WIN_BASE + i as i32);
                        let (hot, xhot) = (f.hover(id, 140, SPRING), f.hover(CLOSE_BASE + i as i32, 140, SPRING));
                        let plate = if w.active { 0.06 } else { 0.0 };
                        c.fill(r, 10.0, Rgba::white(plate + (0.08 - plate) * hot));
                        c.fill(Rect::new(r.x + 10.0, r.cy() - 8.0, 4.0, 16.0), 2.0, if w.active { accent } else { Rgba::white(0.18) });
                        c.text_fit(&w.title, Rect::new(r.x + 10.0 + 4.0 + 10.0, r.y, r.w - 24.0 - 10.0 - 10.0 - 24.0 - 4.0, r.h), Font::new(13.0, 400), INK, Align::Start);
                        let show = hot.max(xhot);
                        let x = self.win_close(i);
                        c.faded(show, |c| {
                            c.fill(x, 6.0, Rgba::white(0.1 * xhot));
                            c.icon(CROSS, (x.cx(), x.cy()), 12.0, 2.4, if xhot > 0.0 { MUTED.mix(INK, xhot) } else { MUTED }, 0.0);
                        });
                    }
                });
            });
        }

        if !self.d.recent.is_empty() {
            let rise = next(f);
            c.faded(out, |c| {
                rise.draw(c, (mid, self.y[2]), |c| {
                    c.text_fit(&self.caption, Rect::new(PAD + 10.0, self.y[2] + 4.0, W - 2.0 * PAD - 20.0, 18.0), Font::new(12.0, 500), MUTED, Align::Start);
                    for (i, r) in self.d.recent.iter().enumerate() {
                        let hot = f.hover(RECENT_BASE + i as i32, 140, SPRING);
                        menu::draw_row(c, self.recent_row(i), hot, false, if r.folder { FOLDER } else { FILE }, &r.name, Hint::None);
                    }
                    let y = self.y[3] - 7.0;
                    c.fill(Rect::new(PAD + 8.0, y, W - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.07));
                });
            });
        }

        if !self.d.acts.is_empty() {
            let rise = next(f);
            c.faded(out, |c| {
                rise.draw(c, (mid, self.y[3]), |c| {
                    for (i, a) in self.d.acts.iter().enumerate() {
                        let hot = f.hover(ACT_BASE + i as i32, 140, SPRING);
                        menu::draw_row(c, self.act_row(i), hot, false, a.icon, &a.label, Hint::None);
                    }
                });
            });
        }

        if self.d.close.is_some() || self.d.kill.is_some() {
            let (rule, group) = (next(f), next(f));
            c.faded(out, |c| {
                rule.draw(c, (mid, self.y[4]), |c| c.fill(Rect::new(PAD + 8.0, self.y[4] + 6.0, W - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.07)));
                group.draw(c, (mid, self.y[5]), |c| {
                    let mut slot = 0;
                    if let Some(l) = &self.d.close {
                        let hot = f.hover(CLOSE_ALL, 140, SPRING);
                        menu::draw_row(c, self.tail_row(slot), hot, false, CLOSE_WINDOW, l, Hint::Keys(kbd_alt_f4()));
                        slot += 1;
                    }
                    if let Some(l) = &self.d.kill {
                        let hot = f.hover(KILL, 140, SPRING);
                        menu::draw_row(c, self.tail_row(slot), hot, true, END_PROCESS, l, Hint::None);
                    }
                });
            });
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        for i in 0..self.d.wins.len() {
            if self.win_close(i).contains(x, y) {
                return Some(CLOSE_BASE + i as i32);
            }
            if self.win_row(i).contains(x, y) {
                return Some(WIN_BASE + i as i32);
            }
        }
        if let Some(i) = (0..self.d.recent.len()).find(|&i| self.recent_row(i).contains(x, y)) {
            return Some(RECENT_BASE + i as i32);
        }
        if let Some(i) = (0..self.d.acts.len()).find(|&i| self.act_row(i).contains(x, y)) {
            return Some(ACT_BASE + i as i32);
        }
        let mut slot = 0;
        if self.d.close.is_some() {
            if self.tail_row(slot).contains(x, y) {
                return Some(CLOSE_ALL);
            }
            slot += 1;
        }
        (self.d.kill.is_some() && self.tail_row(slot).contains(x, y)).then_some(KILL)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        let pick = Rc::clone(&self.pick);
        let what = match id {
            WIN_BASE..CLOSE_BASE => {
                let Some(w) = self.d.wins.get((id - WIN_BASE) as usize) else { return };
                if b == Button::Middle { Pick::CloseWindow(w.hwnd) } else { Pick::Window(w.hwnd) }
            }
            CLOSE_BASE..RECENT_BASE => {
                let Some(w) = self.d.wins.get((id - CLOSE_BASE) as usize) else { return };
                Pick::CloseWindow(w.hwnd)
            }
            RECENT_BASE..ACT_BASE => Pick::Recent((id - RECENT_BASE) as usize),
            ACT_BASE..CLOSE_ALL => Pick::Act((id - ACT_BASE) as usize),
            CLOSE_ALL => Pick::CloseAll,
            KILL => Pick::Kill,
            _ => return,
        };
        cx.close_then(move || pick(what));
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let mut order: Vec<i32> = (0..self.d.wins.len() as i32).collect();
        order.extend((0..self.d.recent.len() as i32).map(|i| RECENT_BASE + i));
        order.extend((0..self.d.acts.len() as i32).map(|i| ACT_BASE + i));
        if self.d.close.is_some() {
            order.push(CLOSE_ALL);
        }
        if self.d.kill.is_some() {
            order.push(KILL);
        }
        let n = order.len();
        let at = from.and_then(|f| order.iter().position(|&o| o == f || ((CLOSE_BASE..RECENT_BASE).contains(&f) && o == f - CLOSE_BASE)));
        let i = match (at, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n.checked_sub(1)?,
            (Some(i), Dir::Up | Dir::Left) => i.saturating_sub(1),
            (Some(i), Dir::Down | Dir::Right) => (i + 1).min(n - 1),
        };
        order.get(i).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(wins: usize, recent: usize) -> Data {
        Data {
            icon: HICON(std::ptr::null_mut()),
            name: "App".into(),
            meta: "Pinned".into(),
            running: wins > 0,
            wins: (0..wins).map(|i| Win { hwnd: i as isize + 10, title: format!("w{i}"), active: i == 0 }).collect(),
            recent: (0..recent).map(|i| Recent { name: format!("f{i}"), folder: false }).collect(),
            acts: vec![Act { icon: "M0 0", label: "a".into() }, Act { icon: "M0 0", label: "b".into() }],
            close: (wins > 0).then(|| "Close".into()),
            kill: (wins > 0).then(|| "Kill".into()),
        }
    }

    #[test]
    fn the_box_grows_with_what_it_lists_and_its_rows_are_found() {
        let (a, b) = (JumpPanel::new(data(0, 0), Side::Bottom, |_| {}), JumpPanel::new(data(2, 3), Side::Bottom, |_| {}));
        assert!(b.size.1 > a.size.1 + 2.0 * WIN_ROW);
        // a window row, its cross, an action, the tail
        let (r, x) = (b.win_row(1), b.win_close(1));
        assert_eq!(b.hit(r.x + 20.0, r.cy()), Some(WIN_BASE + 1));
        assert_eq!(b.hit(x.cx(), x.cy()), Some(CLOSE_BASE + 1));
        let act = b.act_row(1);
        assert_eq!(b.hit(act.cx(), act.cy()), Some(ACT_BASE + 1));
        let (all, kill) = (b.tail_row(0), b.tail_row(1));
        assert_eq!((b.hit(all.cx(), all.cy()), b.hit(kill.cx(), kill.cy())), (Some(CLOSE_ALL), Some(KILL)));
        // a pinned app that is not running has no window rows and no tail
        assert_eq!(a.hit(a.act_row(0).cx(), a.act_row(0).cy()), Some(ACT_BASE));
        assert_eq!(a.hit(W / 2.0, a.size.1 - 4.0), None);
    }

    #[test]
    fn a_hundred_windows_do_not_run_into_the_next_kind_of_row() {
        let got = Rc::new(std::cell::Cell::new(None));
        let g = Rc::clone(&got);
        let mut p = JumpPanel::new(data(130, 1), Side::Bottom, move |k| g.set(Some(k)));
        let x = p.win_close(120);
        assert_eq!(p.hit(x.cx(), x.cy()), Some(CLOSE_BASE + 120));
        let mut cx = Cx::default();
        p.click(CLOSE_BASE + 120, Button::Left, &mut cx);
        cx.run_for_test();
        assert_eq!(got.get(), Some(Pick::CloseWindow(130)), "the 121st window, not a recent file");
    }

    #[test]
    fn a_click_picks_what_its_row_stands_for() {
        let got = Rc::new(std::cell::Cell::new(None));
        let g = Rc::clone(&got);
        let mut p = JumpPanel::new(data(2, 0), Side::Bottom, move |k| g.set(Some(k)));
        let mut cx = Cx::default();
        p.click(CLOSE_BASE + 1, Button::Left, &mut cx);
        cx.run_for_test();
        assert_eq!(got.get(), Some(Pick::CloseWindow(11)));
        let mut cx = Cx::default();
        p.click(WIN_BASE, Button::Middle, &mut cx);
        cx.run_for_test();
        assert_eq!(got.get(), Some(Pick::CloseWindow(10)));
    }
}
