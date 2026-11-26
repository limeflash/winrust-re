//! The notification pop-ups (`.pop` of the mockup): up to three cards stacked over the bottom right corner, each on its own
//! glass (`blur(36px) saturate(1.5)` under `rgba(30,31,38,.9)`), coming in from the right (`popIn`), leaving to the right
//! (`popOut`), with a thin timer under them that shows when the pointer rests on one (the card's clock stops while it does).
//! A card can be swiped away to the right, answered inline (if its source takes an answer) and has the buttons of its note.
//!
//! [`Stack`] is the windowless core (layout, painting into two surfaces, hit tests, the pointer and the keys); the functions at
//! the bottom bind it to two real windows: one for the cards, which takes the mouse and never the focus (but for a reply being
//! typed), and under it one that holds the shadows and lets every click through.

use std::cell::RefCell;
use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, KillTimer, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowPos,
    ShowWindow, WM_ACTIVATE, WM_CHAR, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER, WM_USER, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use winrust_common::style::{self, Side};

use super::anim::{CubicBezier, EASE, POP, SPRING, lerp, timing};
use super::gfx::{Align, Canvas, Font, Gfx, Picture, Rect, Rgba, Surface};
use super::notice::{self as nc, Flavour, Hit, Measure, Parts, Tw, Tween};
use super::panel::{Anchor, Cx};
use crate::notify::{self, Note, Pop};

const W: f32 = 368.0;
const GAP: f32 = 10.0;
/// Room in the window around the cards: for the slide in from the right and out again.
const ROOM: f32 = 16.0;
const ROOM_RIGHT: f32 = 96.0;
/// `0 24px 60px` shadow: how far it reaches.
const SHADOW_PAD: f32 = 60.0 / 2.0 * 3.0 + 24.0 + 4.0;
const TINT: Rgba = Rgba::css(30, 31, 38, 0.9);
const RADIUS: f32 = 18.0;
const BLUR: f32 = 36.0;
const SATURATE: f32 = 1.5;
/// `popOut`'s curve.
const OUT: CubicBezier = CubicBezier::new(0.4, 0.0, 1.0, 1.0);
const VK_ESCAPE: u32 = 0x1B;
const VK_RETURN: u32 = 0x0D;

/// Where the last frame went, in device pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shown {
    pub win: (i32, i32, i32, i32),
    pub shade: (i32, i32, i32, i32),
    /// There are cards: the windows are up.
    pub visible: bool,
    /// Something moves: the next frame comes with the compositor.
    pub busy: bool,
}

/// Pixels of a screen rectangle (left, top, width, height).
type Capture = (Vec<u32>, (i32, i32, i32, i32));

struct Card {
    id: u64,
    pop: Pop,
    note: Note,
    /// At rest, in the window's DIPs.
    rect: Rect,
    parts: Parts,
    progress: f32,
}

#[derive(Clone, Copy)]
struct Drag {
    id: u64,
    x0: f32,
    moved: bool,
}

#[derive(Clone, Copy)]
struct Swipe {
    id: u64,
    dx: f32,
    live: bool,
    /// When a swipe to the right was let go: the card is dropped a moment later.
    gone_at: Option<u64>,
}

pub struct Stack {
    scale: f32,
    anchor: Anchor,
    tw: Tw,
    surf: Surface,
    shade: Surface,
    /// What was behind the cards when they came up, until it is blurred; and the blurred picture with its device origin.
    cap: Option<Capture>,
    glass: Option<(Picture, i32, i32)>,
    win: (i32, i32, i32, i32),
    cards: Vec<Card>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    drag: Option<Drag>,
    swipe: Option<Swipe>,
    /// When a swipe last ended: a click right after it is not a click.
    swiped: u64,
    parts: HashMap<(u64, bool), Parts>,
    pics: nc::Pictures,
    reply_since: u64,
    replying: Option<u64>,
}

impl Stack {
    #[must_use]
    pub fn new(scale: f32, anchor: Anchor) -> Self {
        Self {
            scale,
            anchor,
            tw: Tw::new(),
            surf: Surface::new(),
            shade: Surface::new(),
            cap: None,
            glass: None,
            win: (0, 0, 0, 0),
            cards: Vec::new(),
            hits: Vec::new(),
            hover: None,
            drag: None,
            swipe: None,
            swiped: 0,
            parts: HashMap::new(),
            pics: nc::Pictures::default(),
            reply_since: 0,
            replying: None,
        }
    }

    pub fn set_anchor(&mut self, a: Anchor) {
        self.anchor = a;
    }

    #[must_use]
    pub const fn surfaces(&self) -> (&Surface, &Surface) {
        (&self.surf, &self.shade)
    }

    #[must_use]
    pub const fn typing(&self) -> bool {
        self.replying.is_some()
    }

    /// The column's place: the right edge 12 from the screen's (beside a side bar, 12 from the bar), the bottom 12 above the bar
    /// and 2 more (`bottom: calc(var(--pb) + 2px)`). Left, bottom of the lowest card.
    fn origin(&self) -> (f32, f32) {
        let s = self.scale;
        let (sw, sh) = (self.anchor.screen.0 as f32 / s, self.anchor.screen.1 as f32 / s);
        let b = self.anchor.bar;
        match self.anchor.side {
            Side::Bottom => (sw - 12.0 - W, b.1 as f32 / s - 12.0 - 2.0),
            Side::Left => (b.2 as f32 / s + 12.0, sh - 12.0 - 2.0),
            Side::Right => (b.0 as f32 / s - 12.0 - W, sh - 12.0 - 2.0),
        }
    }

    /// The pop-ups on screen laid out for `now`. `grab` copies a rectangle of the screen (entirely on it): what the glass blurs.
    pub fn frame(&mut self, g: &mut Gfx, now: u64, grab: &mut dyn FnMut(i32, i32, i32, i32) -> Option<Vec<u32>>) -> Shown {
        self.tw.at(now);
        let s = self.scale;
        // the clocks of the cards stand still while the pointer is on one (or an answer is being typed)
        let on = self.hover.and_then(Hit::note);
        let typed = notify::with(|st| {
            st.tick(now);
            for p in st.pops().to_vec() {
                let held = on == Some(p.id) || self.drag.is_some_and(|d| d.id == p.id) || st.reply().is_some_and(|r| r.id == p.id);
                st.pop_hold(p.id, held, now);
            }
            st.reply().map(|r| r.id)
        });
        if typed != self.replying {
            self.replying = typed;
            self.reply_since = now;
        }
        let list: Vec<(Pop, Note, f32)> = notify::with(|st| st.pops().iter().filter_map(|p| st.note(p.id).map(|n| (*p, n.clone(), st.pop_progress(p, now)))).collect());
        // a swipe that has gone far enough is done: the card leaves
        if let Some(sw) = self.swipe {
            if sw.gone_at.is_some_and(|t| now >= t + 260) {
                notify::with(|st| st.pop_drop(sw.id));
                self.swipe = None;
            } else if !list.iter().any(|(p, ..)| p.id == sw.id) {
                self.swipe = None;
            }
        }
        if list.is_empty() {
            self.cards.clear();
            self.hits.clear();
            self.glass = None;
            self.cap = None;
            self.hover = None;
            self.parts.clear();
            self.pics.clear();
            return Shown::default();
        }

        // layout: the column grows upward from its bottom; the oldest card is the top one
        let (x, bottom) = self.origin();
        let mut cards: Vec<Card> = Vec::new();
        for (pop, note, progress) in list {
            let rep = self.replying == Some(pop.id);
            let parts = self.parts.entry((pop.id, rep)).or_insert_with(|| nc::parts(&note, Flavour::Pop, W, rep, now, &mut MeasureGfx(g))).clone();
            cards.push(Card { id: pop.id, pop, note, rect: Rect::new(x, 0.0, W, parts.h), parts, progress });
        }
        let total: f32 = cards.iter().map(|c| c.rect.h).sum::<f32>() + GAP * (cards.len() as f32 - 1.0);
        let mut y = bottom - total;
        let top = y;
        for c in &mut cards {
            c.rect.y = y;
            y += c.rect.h + GAP;
        }
        self.parts.retain(|(id, _), _| cards.iter().any(|c| c.id == *id));

        // the windows: the cards with room around, and under them the shadows
        let (wx, wy) = (((x - ROOM) * s).round() as i32, ((top - ROOM) * s).round() as i32);
        let (ww, wh) = (((W + ROOM + ROOM_RIGHT) * s).round() as i32, (((bottom - top) + 2.0 * ROOM) * s).round() as i32);
        let pad = (SHADOW_PAD * s).ceil() as i32;
        let shade_win = (wx - pad, wy - pad, ww + 2 * pad, wh + 2 * pad);
        let origin = (wx as f32 / s, wy as f32 / s);
        for c in &mut cards {
            c.rect = c.rect.shift(-origin.0, -origin.1);
        }
        // what the glass blurs: the whole column above the bar, taken while no card was up
        if self.glass.is_none() && self.cap.is_none() {
            let reach = (BLUR * 3.0 * s).ceil() as i32;
            let (sw, sh) = self.anchor.screen;
            let tall = (1000.0 * s) as i32;
            let (x0, x1) = ((wx - reach).max(0), sw);
            let (y1, y0) = ((wy + wh + reach).min(sh), (wy + wh - tall).max(0));
            if x1 > x0 && y1 > y0 {
                self.cap = grab(x0, y0, x1 - x0, y1 - y0).map(|px| (px, (x0, y0, x1 - x0, y1 - y0)));
            }
        }
        self.win = (wx, wy, ww, wh);

        // each card's pose now
        let mode = style::anim();
        let mut busy = false;
        let mut posed: Vec<(f32, f32, f32)> = Vec::new(); // opacity, dx, scale
        for c in &cards {
            let age = now.saturating_sub(c.pop.born);
            let swiping = self.swipe.filter(|sw| sw.id == c.id);
            let (op, dx, k) = if let Some(sw) = swiping {
                // `.pop.drag`: no keyframes while it is held; it follows the pointer and springs back or flies off
                let key = Hit::Item(c.id).key();
                let dx = self.tw.go(key, 5, sw.dx, if sw.live { 0 } else { 320 }, SPRING);
                let op = self.tw.go(key, 6, (1.0 - sw.dx.max(0.0) / 260.0).max(0.2), if sw.live { 0 } else { 200 }, EASE);
                (op, dx.max(-20.0), 1.0)
            } else if let Some(t) = c.pop.out {
                let (dur, _) = timing(mode, 240, 0);
                let e = if dur == 0 { 1.0 } else { OUT.ease((now.saturating_sub(t) as f32 / dur as f32).min(1.0)) };
                busy |= dur > 0;
                (1.0 - e, 56.0 * e, lerp(1.0, 0.98, e))
            } else {
                let (dur, _) = timing(mode, 500, 0);
                let e = if dur == 0 { 1.0 } else { POP.ease((age as f32 / dur as f32).min(1.0)) };
                busy |= age < u64::from(dur);
                (e, 36.0 * (1.0 - e), lerp(0.98, 1.0, e))
            };
            posed.push((op, dx, k));
            busy |= c.pop.out.is_none() && self.swipe.is_some_and(|sw| sw.id == c.id);
        }

        // the hit table: what answers a pointer, over the cards at rest
        let mut hits: Vec<(Rect, Hit)> = Vec::new();
        for c in &cards {
            if c.pop.out.is_some() {
                continue;
            }
            let at = |r: Rect| r.shift(c.rect.x, c.rect.y);
            hits.push((c.rect, Hit::Item(c.id)));
            hits.push((at(c.parts.dismiss), Hit::Dismiss(c.id)));
            if let Some((f, sd)) = c.parts.reply {
                hits.push((at(f), Hit::Field(c.id)));
                hits.push((at(sd), Hit::Send(c.id)));
            }
            for (a, r) in &c.parts.acts {
                hits.push((at(*r), Hit::Act(c.id, *a as u8)));
            }
        }
        self.hits = hits;

        // paint: the shadows, then the cards
        let sh_pad = (pad as f32 / s, pad as f32 / s);
        let posed_rect = |c: &Card, p: (f32, f32, f32)| {
            let r = c.rect;
            Rect::new(r.cx() + (r.x - r.cx()) * p.2 + p.1, r.cy() + (r.y - r.cy()) * p.2, r.w * p.2, r.h * p.2)
        };
        let _ = g.paint(&mut self.shade, shade_win.2, shade_win.3, |c| {
            for (card, p) in cards.iter().zip(&posed) {
                let r = posed_rect(card, *p).shift(sh_pad.0, sh_pad.1);
                c.faded(p.0, |c| {
                    c.shadow(r, RADIUS, 24.0, 60.0, Rgba::css(4, 6, 20, 0.6));
                    c.ring(r.inset(-1.0), RADIUS + 1.0, 1.0, Rgba::css(0, 0, 0, 0.55));
                });
            }
            for (card, p) in cards.iter().zip(&posed) {
                c.erase(posed_rect(card, *p).shift(sh_pad.0, sh_pad.1), RADIUS);
            }
        });
        let Self { surf, glass, cap, tw, swipe, hover, reply_since, pics, .. } = self;
        let win = (wx, wy);
        let hover_hit = *hover;
        let swipe_now = *swipe;
        let rs = *reply_since;
        let mut keep_busy = false;
        let _ = g.paint(surf, ww, wh, |c| {
            if let Some((px, (cx, cy, cw, ch))) = cap.take() {
                *glass = c.glass(&px, cw, ch, BLUR, SATURATE).map(|p| (p, cx, cy));
            }
            for (card, p) in cards.iter().zip(&posed) {
                let (op, dx, k) = *p;
                let r = card.rect;
                let xf = super::gfx::Xf { cx: r.cx(), cy: r.cy(), k, dx, dy: 0.0 };
                c.faded(op, |c| {
                    c.masked(r, RADIUS, xf, |c| {
                        if let Some((pic, gx, gy)) = glass.as_ref() {
                            c.picture(pic, (*gx - win.0) as f32 / s, (*gy - win.1) as f32 / s);
                        }
                        c.transformed(r.cx(), r.cy(), k, dx, 0.0, |c| {
                            c.fill(r, RADIUS, if glass.is_some() { TINT } else { TINT.alpha(0.97) });
                            c.ring(r, RADIUS, 1.0, Rgba::white(0.1));
                            c.transformed(0.0, 0.0, 1.0, r.x, r.y, |c| {
                                draw_card(c, (tw, pics), hover_hit, card, now, rs, swipe_now.is_some_and(|sw| sw.id == card.id));
                            });
                        });
                    });
                });
                keep_busy |= card.parts.reply.is_some();
            }
        });
        let busy = busy || keep_busy || self.tw.running();
        self.cards = cards;
        Shown { win: self.win, shade: shade_win, visible: true, busy }
    }

    // -- the pointer and the keys; coordinates are the window's client pixels

    fn at(&self, x: i32, y: i32) -> (f32, f32) {
        (x as f32 / self.scale, y as f32 / self.scale)
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }

    /// The pointer moved; true if what is under it (or a swipe) changed.
    pub fn pointer(&mut self, x: i32, y: i32) -> bool {
        let (px, py) = self.at(x, y);
        if let Some(d) = self.drag.as_mut() {
            let dx = px - d.x0;
            if dx.abs() > 4.0 {
                d.moved = true;
            }
            if d.moved {
                self.swipe = Some(Swipe { id: d.id, dx, live: true, gone_at: None });
                return true;
            }
        }
        let h = self.hit_at(px, py);
        std::mem::replace(&mut self.hover, h) != h
    }

    /// Puts the pointer on the `k`-th card (the renderer's hand).
    pub fn point_at(&mut self, k: usize) {
        self.hover = self.cards.get(k).map(|c| Hit::Item(c.id));
    }

    pub fn leave(&mut self) -> bool {
        self.hover.take().is_some()
    }

    /// The left button went down: a button acts at once, a card may be picked up.
    pub fn down(&mut self, x: i32, y: i32, now: u64, cx: &mut Cx) {
        let (px, py) = self.at(x, y);
        let Some(h) = self.hit_at(px, py) else { return };
        self.hover = Some(h);
        match h {
            Hit::Item(id) => self.drag = Some(Drag { id, x0: px, moved: false }),
            Hit::Dismiss(id) => notify::with(|s| s.pop_off(id, now)),
            Hit::Act(id, a) => nc::act_clicked(id, usize::from(a), now, cx),
            Hit::Send(_) => nc::send_clicked(now, cx),
            _ => {}
        }
        // answering takes the keyboard
        if notify::with(|s| s.reply().is_some()) {
            self.replying = notify::with(|s| s.reply().map(|r| r.id));
        }
    }

    /// The button came up: a card that was swiped goes or comes back; one that was only pressed is clicked.
    pub fn up(&mut self, now: u64, cx: &mut Cx) {
        let Some(d) = self.drag.take() else { return };
        if d.moved {
            self.swiped = now;
            if let Some(sw) = self.swipe.as_mut() {
                if sw.dx > 90.0 {
                    *sw = Swipe { id: d.id, dx: 420.0, live: false, gone_at: Some(now) };
                } else {
                    *sw = Swipe { id: d.id, dx: 0.0, live: false, gone_at: None };
                }
            }
        } else if now.saturating_sub(self.swiped) >= 300 {
            self.picked(d.id, now, cx);
        }
    }

    /// A click on a card: the source it came from opens, or the centre does with the note lit.
    fn picked(&mut self, id: u64, now: u64, cx: &mut Cx) {
        let has_source = notify::with(|s| s.note(id).is_some_and(|n| n.target != notify::Target::Nothing));
        if has_source {
            nc::note_clicked(id, now, cx);
        } else {
            notify::with(|s| {
                s.pop_off(id, now);
                s.mark_read(id);
                s.highlight_note(id, now);
            });
            cx.run(|| {
                // SAFETY: opens the calendar panel on the bar's thread, as a click on the clock does.
                unsafe { crate::cal_open() }
            });
        }
    }

    /// A typed character goes to the answer being written.
    pub fn text(&mut self, ch: char) {
        notify::with(|s| s.reply_char(ch));
    }

    /// Enter sends the answer, Esc takes it back. True if the key was used.
    pub fn key(&mut self, vk: u32, now: u64, cx: &mut Cx) -> bool {
        match vk {
            VK_RETURN if self.typing() => {
                nc::send_clicked(now, cx);
                true
            }
            VK_ESCAPE if self.typing() => {
                notify::with(notify::Store::cancel_reply);
                true
            }
            _ => false,
        }
    }
}

/// `Measure` for a bare `Gfx` behind a mutable reference (the renderer holds one).
struct MeasureGfx<'a>(&'a mut Gfx);

impl Measure for MeasureGfx<'_> {
    fn width(&mut self, s: &str, f: Font) -> f32 {
        self.0.measure(s, f)
    }
    fn lines(&mut self, s: &str, f: Font, w: f32) -> usize {
        self.0.lines(s, f, w)
    }
}

/// Paints one card's insides (its own coordinates, origin at its top left corner).
fn draw_card(c: &mut Canvas<'_>, (tw, pics): (&mut Tw, &nc::Pictures), hov: Option<Hit>, card: &Card, now: u64, since: u64, swiping: bool) {
    let (n, p) = (&card.note, &card.parts);
    // the top line: the app's face, its name, "now", the x
    nc::app_icon(c, &n.app.icon, Rect::new(14.0, 14.0, 16.0, 16.0), 1.0);
    let name_w = c.measure(&n.app.name, Font::new(12.0, 500)).ceil();
    c.text(&n.app.name, Rect::new(14.0 + 16.0 + 8.0, 12.0, name_w + 2.0, 20.0), Font::new(12.0, 500), nc::DIM, Align::Start);
    c.text(nc::just_now(), Rect::new(14.0 + 16.0 + 8.0 + name_w + 8.0, 12.0, 80.0, 20.0), Font::new(12.0, 400), nc::FAINT, Align::Start);
    nc::dismiss_button(c, tw, hov, card.id, p.dismiss, true, true);
    if let (Some(a), Some(r)) = (n.avatar.as_ref(), p.avatar) {
        nc::avatar(c, a, r, 14.0);
    }
    nc::title(c, n, p.head, 0.0, false);
    if let Some(b) = p.body {
        nc::body(c, n, b);
    }
    if let (Some(path), Some(r)) = (n.image.as_deref(), p.image) {
        nc::picture(c, pics, path, r);
        nc::video_badge(c, n, r);
    }
    let reply = notify::with(|s| s.reply().filter(|r| r.id == card.id).map(|r| r.text.clone()));
    if let (Some(text), Some(r)) = (reply.as_deref(), p.reply) {
        nc::reply_field(c, tw, hov, card.id, n, text, r, true, since, now);
    }
    let first = n.acts.iter().position(|a| a.does != notify::Do::Read).map_or(0, |_| 0);
    for (k, (a, r)) in p.acts.iter().enumerate() {
        nc::act_button(c, tw, hov, Hit::Act(card.id, *a as u8), *r, &n.acts[*a].label, k == first, n.danger, true);
    }
    // the timer: a line that runs out from the right, seen only while the pointer rests on the card
    let seen = tw.go(Hit::Item(card.id).key(), 3, f32::from(card.pop.held() || swiping || hov.and_then(Hit::note) == Some(card.id)), 200, EASE);
    if seen > 0.003 {
        let full = Rect::new(18.0, p.h - 2.0, W - 36.0, 2.0);
        c.faded(seen, |c| c.fill(Rect::new(full.x, full.y, full.w * (1.0 - card.progress), full.h), 1.0, Rgba::white(0.22)));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the windows

struct Host {
    stack: Stack,
    gfx: Gfx,
    wnd: HWND,
    shade: HWND,
    grab: Surface,
    up: bool,
    timer: bool,
    last_win: (i32, i32, i32, i32),
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn now() -> u64 {
    // SAFETY: plain query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// Creates the two windows.
pub fn init(hi: HINSTANCE, scale: f32, anchor: Anchor) -> bool {
    let Ok(gfx) = Gfx::new(scale) else { return false };
    let (name, shade_name): (Vec<u16>, Vec<u16>) = ("topbar_notice\0".encode_utf16().collect(), "topbar_notice_shadow\0".encode_utf16().collect());
    super::panel::register_class(hi, &name, Some(proc), true);
    super::panel::register_class(hi, &shade_name, Some(DefWindowProcW), false);
    let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED;
    // SAFETY: window creation with live, NUL-terminated class names.
    let (wnd, shade) = unsafe {
        let make = |ex: u32, class: &[u16]| CreateWindowExW(ex, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), hi, std::ptr::null());
        (make(ex, &name), make(ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, &shade_name))
    };
    if wnd.is_null() || shade.is_null() {
        return false;
    }
    HOST.set(Some(Host { stack: Stack::new(scale, anchor), gfx, wnd, shade, grab: Surface::new(), up: false, timer: false, last_win: (0, 0, 0, 0) }));
    true
}

/// The store changed (a note arrived, one was dismissed, the centre opened): the pop-ups follow. `anchor` is the bar now.
pub fn changed(anchor: Option<Anchor>) {
    with(|h| {
        if let Some(a) = anchor {
            h.stack.set_anchor(a);
        }
        h.frame(now());
    });
}

/// A panel opened over the corner: the cards stay above it.
pub fn restack() {
    with(|h| {
        if h.up {
            h.raise();
        }
    });
}

impl Host {
    fn raise(&self) {
        let f = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        // SAFETY: z-order of our own windows.
        unsafe {
            SetWindowPos(self.shade, HWND_TOPMOST, 0, 0, 0, 0, f);
            SetWindowPos(self.wnd, HWND_TOPMOST, 0, 0, 0, 0, f);
        }
    }

    fn frame(&mut self, t: u64) {
        if !self.up && notify::with(|s| !s.pops().is_empty()) {
            // SAFETY: waits for the compositor to take the cards of a moment ago off the screen, so the grab below does not see them.
            unsafe { windows_sys::Win32::Graphics::Dwm::DwmFlush() };
        }
        let Self { stack, gfx, grab, .. } = self;
        let mut grabber = |x, y, w, h| if grab.grab(x, y, w, h) { Some(grab.pixels().to_vec()) } else { None };
        let shown = stack.frame(gfx, t, &mut grabber);
        if shown.visible {
            let (surf, shade) = self.stack.surfaces();
            let s = super::panel::whwnd(self.shade);
            let _ = shade.present(s, shown.shade.0, shown.shade.1, 255);
            let _ = surf.present(super::panel::whwnd(self.wnd), shown.win.0, shown.win.1, 255);
            if !self.up {
                self.up = true;
                self.raise();
                let f = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
                // SAFETY: showing our own windows without activating them.
                unsafe {
                    SetWindowPos(self.shade, HWND_TOPMOST, 0, 0, 0, 0, f);
                    SetWindowPos(self.wnd, HWND_TOPMOST, 0, 0, 0, 0, f);
                }
            }
            self.last_win = shown.win;
        } else if self.up {
            self.up = false;
            // SAFETY: hiding our own windows.
            unsafe {
                ShowWindow(self.wnd, SW_HIDE);
                ShowWindow(self.shade, SW_HIDE);
            }
        }
        crate::pacing::set(self.wnd as isize, shown.busy);
        // pop-ups that only wait for their time need a look now and then, not a frame per refresh
        let wait = shown.visible && !shown.busy;
        if wait != self.timer {
            self.timer = wait;
            // SAFETY: a timer on our own window.
            unsafe {
                if wait {
                    SetTimer(self.wnd, 1, 100, None);
                } else {
                    KillTimer(self.wnd, 1);
                }
            }
        }
    }
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32)
}

/// Runs an input on the stack, repaints, then does what it asked of the world (with the stack free again).
fn input(f: impl FnOnce(&mut Stack, &mut Cx) -> bool) {
    let cx = with(|h| {
        let mut cx = Cx::default();
        let again = f(&mut h.stack, &mut cx);
        if again {
            h.frame(now());
        }
        cx
    });
    if let Some(cx) = cx {
        cx.finish();
        // what the click did to the store shows now, and on the bar's clock
        with(|h| h.frame(now()));
        crate::notify::bar_changed();
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        0x0014 => return 1, // WM_ERASEBKGND
        0x0021 => return 3, // WM_MOUSEACTIVATE: a click never takes the focus from the app in front
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            // SAFETY: asks for WM_MOUSELEAVE.
            unsafe {
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
            }
            input(|s, _| s.pointer(x, y));
        }
        0x02A3 => input(|s, _| s.leave()),
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lp);
            // SAFETY: the pointer is followed past the window while a card is held.
            unsafe { SetCapture(hwnd) };
            input(|s, cx| {
                s.down(x, y, now(), cx);
                true
            });
            // an answer being typed needs the keyboard: this window takes it
            if with(|h| h.stack.typing()).unwrap_or(false) {
                winrust_common::fg::force(hwnd);
            }
        }
        WM_LBUTTONUP => {
            // SAFETY: releases the capture taken above.
            unsafe { ReleaseCapture() };
            input(|s, cx| {
                s.up(now(), cx);
                true
            });
        }
        WM_CHAR => {
            if let Some(ch) = super::panel::wm_char(wp).filter(|c| *c >= ' ' || matches!(c, '\u{8}')) {
                input(|s, _| {
                    s.text(ch);
                    true
                });
            }
        }
        WM_KEYDOWN => input(|s, cx| s.key(wp as u32, now(), cx)),
        // the focus went elsewhere: an answer that was being typed has nowhere to go
        WM_ACTIVATE if wp & 0xffff == 0 => input(|s, _| {
            notify::with(notify::Store::cancel_reply);
            let _ = s;
            true
        }),
        crate::pacing::FRAME => {
            if crate::pacing::received(hwnd as isize) {
                with(|h| h.frame(now()));
            }
        }
        WM_USER => _ = with(|h| h.frame(now())),
        WM_TIMER => _ = with(|h| h.frame(now())),
        _ => {
            // SAFETY: the default procedure of our own window.
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::{App, AppIcon};

    fn anchor() -> Anchor {
        Anchor { side: Side::Bottom, button: (1300, 852, 1400, 894), bar: (0, 846, 1440, 900), screen: (1440, 900) }
    }

    fn note(t: &str) -> Note {
        Note::new(App { key: "a".into(), name: "App".into(), icon: AppIcon::Winrust }, t, "body")
    }

    #[test]
    fn the_column_stands_12_above_the_bar_and_2_more_with_its_right_edge_12_from_the_screen() {
        let s = Stack::new(1.0, anchor());
        assert_eq!(s.origin(), (1440.0 - 12.0 - W, 846.0 - 14.0));
        let left = Stack::new(1.0, Anchor { side: Side::Left, bar: (0, 0, 54, 900), ..anchor() });
        assert_eq!(left.origin(), (54.0 + 12.0, 900.0 - 14.0));
        let right = Stack::new(1.0, Anchor { side: Side::Right, bar: (1386, 0, 1440, 900), ..anchor() });
        assert_eq!(right.origin(), (1386.0 - 12.0 - W, 900.0 - 14.0));
    }

    #[test]
    fn a_swipe_past_90_sends_the_card_away_and_a_short_one_brings_it_back() {
        notify::with(|s| *s = notify::Store::new());
        let id = notify::with(|s| s.push(note("n"), 0));
        let mut st = Stack::new(1.0, anchor());
        // a pointer pressed on the card (the hit table is made by a frame; stand in for it)
        st.hits = vec![(Rect::new(0.0, 0.0, W, 80.0), Hit::Item(id))];
        let mut cx = Cx::default();
        st.down(100, 40, 1000, &mut cx);
        assert!(st.pointer(150, 40) && st.swipe.is_some_and(|s| s.live && (s.dx - 50.0).abs() < 1e-3));
        st.up(1100, &mut cx);
        assert!(st.swipe.is_some_and(|s| s.dx == 0.0 && !s.live), "50 px is not far enough");
        st.down(100, 40, 2000, &mut cx);
        st.pointer(230, 40);
        st.up(2100, &mut cx);
        assert!(st.swipe.is_some_and(|s| s.dx == 420.0 && s.gone_at == Some(2100)));
        // a click right after a swipe is not a click
        st.hits = vec![(Rect::new(0.0, 0.0, W, 80.0), Hit::Item(id))];
        st.down(100, 40, 2200, &mut cx);
        st.up(2250, &mut cx);
        assert!(notify::with(|s| s.note(id).is_some_and(|n| n.unread)), "it was not clicked");
        notify::with(|s| *s = notify::Store::new());
    }

    #[test]
    fn three_cards_are_painted_stand_10_apart_and_leave_when_dismissed() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let mut g = Gfx::new(1.0).unwrap();
        notify::with(|s| *s = notify::Store::new());
        let ids: Vec<u64> = (0..4).map(|i| notify::with(|s| s.push(note("n"), 1000 + i))).collect();
        let mut st = Stack::new(1.0, anchor());
        let mut grab = |_x: i32, _y: i32, w: i32, h: i32| Some(vec![0xff10_1010u32; (w * h) as usize]);
        let shown = st.frame(&mut g, 1100, &mut grab);
        assert!(shown.visible && shown.busy, "they are coming in");
        assert_eq!(st.cards.iter().map(|c| c.id).collect::<Vec<_>>(), ids[1..].to_vec(), "three at most, the oldest made room");
        let rects: Vec<Rect> = st.cards.iter().map(|c| c.rect).collect();
        assert!((rects[1].y - (rects[0].y + rects[0].h + 10.0)).abs() < 1e-3 && (rects[2].y - (rects[1].y + rects[1].h + 10.0)).abs() < 1e-3);
        // the window stands 14 DIPs plus its room above the bar, and the surfaces hold pixels
        assert_eq!(shown.win.1 + shown.win.3, 846 - 14 + 16);
        assert!(st.surfaces().0.pixels().iter().any(|p| p >> 24 != 0) && st.surfaces().1.pixels().iter().any(|p| p >> 24 != 0));
        let settled = st.frame(&mut g, 4000, &mut grab);
        assert!(settled.visible && !settled.busy, "at rest they only wait for their time");
        // the x of the lowest card sends it off; a moment later it is gone and the others stay
        let last = ids[3];
        notify::with(|s| s.pop_off(last, 4000));
        assert!(st.frame(&mut g, 4100, &mut grab).busy);
        notify::with(|s| {
            s.tick(4000 + notify::POP_EXIT_MS);
        });
        assert_eq!(st.frame(&mut g, 4000 + notify::POP_EXIT_MS, &mut grab).visible, true);
        assert_eq!(st.cards.len(), 2);
        notify::with(|s| ids.iter().for_each(|&i| s.dismiss(i, 5000)));
        assert!(!st.frame(&mut g, 5000, &mut grab).visible, "nothing left to show");
        notify::with(|s| *s = notify::Store::new());
    }

    #[test]
    fn the_x_sends_a_pop_up_off_and_a_hold_stops_its_clock() {
        notify::with(|s| *s = notify::Store::new());
        let id = notify::with(|s| s.push(note("n"), 0));
        let mut st = Stack::new(1.0, anchor());
        st.hits = vec![(Rect::new(0.0, 0.0, W, 80.0), Hit::Item(id)), (Rect::new(330.0, 8.0, 24.0, 24.0), Hit::Dismiss(id))];
        let mut cx = Cx::default();
        st.down(340, 20, 500, &mut cx);
        assert!(notify::with(|s| s.pops()[0].out == Some(500)));
        notify::with(|s| *s = notify::Store::new());
    }
}
