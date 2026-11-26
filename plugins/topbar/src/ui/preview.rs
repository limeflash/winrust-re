//! The previews over an app button that has windows: one card per window, 208 × 136, a title row (the app's icon, the title,
//! a close cross on hover) over a body that shows the window live. The body is a hole in the glass: DWM draws the window's
//! thumbnail in a window under the panel (see [`super::live`]). The window in front has an accent ring. The panel
//! rises in (`pvIn`, .2 s) and its cards one after the other (`pvcIn`, .24 s, 30 ms apart); it does not take the focus.

use std::rc::Rc;

use windows::Win32::UI::WindowsAndMessaging::HICON;
use winrust_common::style::pal;

use super::anim::{EASE, SNAP};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::menu::{DIM, INK};
use super::panel::{Button, Content, Cx, Entry, Frame, Hole, Id, Look, Origin, Place, Pose, Shadow};

pub const CARD_W: f32 = 208.0;
pub const CARD_H: f32 = 136.0;
const GAP: f32 = 8.0;
const PAD: f32 = 8.0;
const TITLE: f32 = 34.0;
const CLOSE_BASE: i32 = 100;
const CROSS: &str = "M7 7l10 10M17 7L7 17";

/// One window of the app.
pub struct Card {
    pub hwnd: isize,
    pub title: String,
    /// What its thumbnail shows, device pixels (for the aspect).
    pub src: (i32, i32),
    /// A minimized window has no thumbnail: its card shows the app's icon.
    pub minimized: bool,
    /// The window in front.
    pub current: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Window(isize),
    Close(isize),
}

pub struct PreviewPanel {
    cards: Vec<Card>,
    icon: HICON,
    /// The app's colour (`color-mix(.. 10%, #16171c)` is the body's ground).
    color: Rgba,
    pick: Rc<dyn Fn(Pick)>,
    holes: Vec<Hole>,
    /// Appears at once (the panel is laid out again after a window was closed).
    calm: bool,
}

impl PreviewPanel {
    #[must_use]
    pub fn new(cards: Vec<Card>, icon: HICON, color: Rgba, calm: bool, pick: impl Fn(Pick) + 'static) -> Self {
        Self { cards, icon, color, pick: Rc::new(pick), holes: Vec::new(), calm }
    }

    fn card(i: usize) -> Rect {
        Rect::new(PAD + i as f32 * (CARD_W + GAP), PAD, CARD_W, CARD_H)
    }

    fn close(i: usize) -> Rect {
        let r = Self::card(i);
        Rect::new(r.x + CARD_W - 6.0 - 24.0, r.y + 5.0, 24.0, 24.0)
    }

    /// Where the thumbnail of card `i` goes: the largest rectangle of the window's aspect in the body.
    fn thumb(&self, i: usize) -> Rect {
        let r = Self::card(i);
        let area = Rect::new(r.x + 8.0, r.y + TITLE, CARD_W - 16.0, CARD_H - TITLE - 8.0);
        let (sw, sh) = self.cards[i].src;
        let k = (area.w / sw.max(1) as f32).min(area.h / sh.max(1) as f32);
        let (w, h) = ((sw as f32 * k).floor(), (sh as f32 * k).floor());
        Rect::new((area.cx() - w / 2.0).round(), (area.cy() - h / 2.0).round(), w, h)
    }
}

impl Content for PreviewPanel {
    fn id(&self) -> Id {
        Id::Preview
    }

    fn look(&self) -> Look {
        Look::glass(18.0, 0.82, Shadow { dy: 24.0, blur: 60.0, color: Rgba::css(0, 0, 0, 0.55) })
    }

    fn entry(&self) -> Entry {
        let dur = if self.calm { 0 } else { 200 };
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: 8.0, k: 0.98, clip: None }, dur, ease: SNAP, origin: Origin::Frac(0.5, 1.0) }
    }

    fn place(&self) -> Place {
        // `bottom: calc(var(--pb) - 6px)`
        Place { gap: 6.0, ..Place::default() }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        let n = self.cards.len() as f32;
        (n * CARD_W + (n - 1.0).max(0.0) * GAP + 2.0 * PAD, CARD_H + 2.0 * PAD)
    }

    fn settle(&self) -> u32 {
        if self.calm { 0 } else { 240 + 30 * self.cards.len() as u32 }
    }

    fn activates(&self) -> bool {
        false
    }

    fn holes(&self) -> Vec<Hole> {
        self.holes.clone()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let ground = Rgba::css(22, 23, 28, 1.0).mix(self.color, 0.10);
        self.holes.clear();
        for i in 0..self.cards.len() {
            let r = Self::card(i);
            let rise = if self.calm { f.rise(0, 0, SNAP, 0.0, 1.0) } else { f.rise(i as u32 * 30, 240, SNAP, 6.0, 1.0) };
            let (hot_card, hot_x) = (f.hover(i as i32, 160, EASE), f.hover(CLOSE_BASE + i as i32, 140, EASE));
            let hot = hot_card.max(hot_x);
            let card = &self.cards[i];
            let thumb = self.thumb(i);
            let live = !card.minimized;
            let opacity = f.opacity * rise.opacity;
            let ring = if card.current { (2.0, accent) } else { (1.0 + 0.5 * hot, Rgba::white(0.08 + 0.22 * hot)) };
            let (title, icon) = (&card.title, self.icon);
            c.faded(rise.opacity, |c| {
                c.transformed(r.cx(), r.cy(), 1.0, 0.0, rise.dy, |c| {
                    c.fill(r, 12.0, Rgba::css(29, 30, 36, 1.0));
                    c.hicon(icon, Rect::new(r.x + 10.0, r.y + TITLE / 2.0 - 8.0, 16.0, 16.0), 1.0);
                    c.text_fit(title, Rect::new(r.x + 34.0, r.y, CARD_W - 34.0 - 34.0, TITLE), Font::new(12.0, 500), INK, Align::Start);
                    c.clipped(Rect::new(r.x, r.y + TITLE, CARD_W, CARD_H - TITLE), |c| c.fill(r, 12.0, ground));
                    c.clipped(Rect::new(r.x, r.y + TITLE, CARD_W, CARD_H - TITLE), |c| c.fill(r, 12.0, Rgba::white(0.045 * hot)));
                    if live {
                        // the window shows through here; a hair of alpha keeps the pointer on the panel
                        c.erase(thumb, 6.0);
                        c.fill(thumb, 6.0, Rgba::new(0.0, 0.0, 0.0, 1.0 / 255.0));
                    } else {
                        c.hicon(icon, Rect::new(r.cx() - 22.0, r.y + TITLE + (CARD_H - TITLE) / 2.0 - 22.0, 44.0, 44.0), 1.0);
                    }
                    c.ring(r, 12.0, ring.0, ring.1);
                    let x = Self::close(i);
                    c.faded(hot, |c| {
                        c.fill(x, 7.0, Rgba::css(229, 72, 77, hot_x));
                        c.icon(CROSS, (x.cx(), x.cy()), 11.0, 2.6, DIM.mix(Rgba::white(1.0), hot_x), 0.0);
                    });
                });
            });
            if live {
                self.holes.push(Hole { win: card.hwnd, rect: thumb.shift(0.0, rise.dy), opacity });
            }
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        for i in 0..self.cards.len() {
            if Self::close(i).contains(x, y) {
                return Some(CLOSE_BASE + i as i32);
            }
            if Self::card(i).contains(x, y) {
                return Some(i as i32);
            }
        }
        None
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        let pick = Rc::clone(&self.pick);
        if id >= CLOSE_BASE || b == Button::Middle {
            let Some(w) = self.cards.get((id % CLOSE_BASE) as usize).map(|c| c.hwnd) else { return };
            return cx.run(move || pick(Pick::Close(w)));
        }
        let Some(w) = self.cards.get(id as usize).map(|c| c.hwnd) else { return };
        cx.close_then(move || pick(Pick::Window(w)));
    }
}
