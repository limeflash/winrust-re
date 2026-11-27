//! The notification centre and what its cards and the pop-ups' cards are made of (the `.ncard`, `.nci` and `.pop` of the
//! mockup).
//!
//! The centre is the card above the calendar: a header (the title, how many are unread, Do not disturb, Clear all), a strip
//! that says the pop-ups are silenced, the notes grouped by app (a stack of more than two is closed and shows the newest with
//! the rest peeking from under it) and, when there are none, the "all caught up" picture. [`Centre`] lays itself out from the
//! store ([`crate::notify`]) every frame, draws, answers hit tests and clicks. [`crate::ui::cal_popup`] puts it in the
//! calendar's window; [`crate::ui::notice_pop`] draws the pop-ups with the same atoms.
//!
//! Nothing here owns a window. Animations are the mockup's keyframes: `nciIn`, `nciHl`, `ncrIn`, `ncdIn`, `nceIn`, the
//! collapse of `.nciw.gone`, the hover fades of `.nci`, `.ncx`, `.ncgx`, the chevron's turn.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use winrust_common::style::pal;

use super::anim::{CubicBezier, EASE, SPRING, Transition, lerp, timing};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::panel::{Button, Cx, Frame};
use crate::notify::{self, Act, Ago, App, AppIcon, Do, Note};

// ---------------------------------------------------------------------------------------------------------------------
// colours and type of the mockup

pub const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
pub const DIM: Rgba = Rgba::css(163, 169, 184, 1.0);
pub const CHIP: Rgba = Rgba::css(201, 205, 216, 1.0);
pub const BODY: Rgba = Rgba::css(196, 200, 210, 1.0);
pub const MUTED: Rgba = Rgba::css(154, 160, 176, 1.0);
pub const FAINT: Rgba = Rgba::css(125, 131, 148, 1.0);
/// Text on an accent fill (the design's `onA`): ink on the dark style, white on the light one.
pub const ON_ACCENT: Rgba = Rgba::pair(0x0b1220, 0xffffff);
pub const NUMB: Rgba = Rgba::css(138, 144, 162, 1.0);
const LILAC: Rgba = Rgba::hex(0xc0a5f5);
const ROW_UNREAD: Rgba = Rgba::hex(0x2c2d34);
const ROW_READ: Rgba = Rgba::hex(0x25262c);
const ROW_HOT: Rgba = Rgba::hex(0x313239);
const PEEK: Rgba = Rgba::hex(0x222329);
const RED: Rgba = Rgba::css(229, 72, 77, 1.0);
const RED_INK: Rgba = Rgba::hex(0xff9a9e);
const CARD: Rgba = Rgba::css(27, 28, 34, 1.0);

pub const TITLE: Font = Font::new(13.0, 600);
pub const TEXT: Font = Font::new(12.5, 400);
pub const LINE: f32 = 12.5 * 1.45;
const SMALL: Font = Font::new(11.5, 400);

pub const MOON: &str = "M20 14.5A8 8 0 1 1 9.5 4 6.5 6.5 0 0 0 20 14.5z";
const BELL: &str = "M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0";
pub const CROSS: &str = "M7 7l10 10M17 7L7 17";
pub const SEND: &str = "M4 12l16-8-6 16-2.5-6.5z";
const CHEVRON: &str = "M6 9l6 6 6-6";

pub fn accent() -> Rgba {
    Rgba::colorref(pal().accent)
}

// ---------------------------------------------------------------------------------------------------------------------
// what the drawing code asks of its host

/// Text measures (a frame's canvas, or the renderer before one).
pub trait Measure {
    fn width(&mut self, s: &str, f: Font) -> f32;
    fn lines(&mut self, s: &str, f: Font, w: f32) -> usize;
}

impl Measure for Gfx {
    fn width(&mut self, s: &str, f: Font) -> f32 {
        self.measure(s, f)
    }
    fn lines(&mut self, s: &str, f: Font, w: f32) -> usize {
        Gfx::lines(self, s, f, w)
    }
}

impl Measure for Canvas<'_> {
    fn width(&mut self, s: &str, f: Font) -> f32 {
        self.measure(s, f)
    }
    fn lines(&mut self, s: &str, f: Font, w: f32) -> usize {
        Canvas::lines(self, s, f, w)
    }
}

/// CSS transitions of items: a value that glides to its target and starts there when first seen.
pub trait Tween {
    fn go(&mut self, key: i32, slot: u8, to: f32, dur: u32, ease: CubicBezier) -> f32;
}

impl Tween for Frame<'_> {
    fn go(&mut self, key: i32, slot: u8, to: f32, dur: u32, ease: CubicBezier) -> f32 {
        self.tween(key, slot, to, dur, ease)
    }
}

/// A set of transitions of its own, for a window that has no [`Frame`] (the pop-ups).
pub struct Tw {
    pub now: u64,
    mode: winrust_common::style::Anim,
    t: HashMap<(i32, u8), Transition>,
}

impl Tw {
    #[must_use]
    pub fn new() -> Self {
        Self { now: 0, mode: winrust_common::style::Anim::Full, t: HashMap::new() }
    }

    /// Moves to `now` with the user's motion setting.
    pub fn at(&mut self, now: u64) {
        self.now = now;
        self.mode = winrust_common::style::anim();
    }

    #[must_use]
    pub fn running(&self) -> bool {
        self.t.values().any(|t| t.running(self.now))
    }
}

impl Default for Tw {
    fn default() -> Self {
        Self::new()
    }
}

impl Tween for Tw {
    fn go(&mut self, key: i32, slot: u8, to: f32, dur: u32, ease: CubicBezier) -> f32 {
        let (dur, delay) = timing(self.mode, dur, 0);
        let t = self.t.entry((key, slot)).or_insert_with(|| Transition::new(to));
        t.set(to, self.now, dur, delay, ease);
        t.value(self.now)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// things a pointer can be on

/// A control of a card. The same words name them in the centre and in the pop-ups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Dnd,
    ClearAll,
    DndOff,
    Undo,
    /// A closed stack (a click opens it); the group's index.
    Stack(u32),
    /// The "+N" / "Show less" pill of a group.
    More(u32),
    ClearGroup(u32),
    Item(u64),
    Dismiss(u64),
    Act(u64, u8),
    Send(u64),
    Field(u64),
}

impl Hit {
    /// A key for the item's own transitions that does not change when the list does.
    #[must_use]
    pub fn key(self) -> i32 {
        let id = |i: u64, k: i32| 10_000 + ((i & 0x00ff_ffff) as i32) * 16 + k;
        match self {
            Hit::Dnd => -1,
            Hit::ClearAll => -2,
            Hit::DndOff => -3,
            Hit::Undo => -4,
            Hit::Stack(g) => -100 - g as i32,
            Hit::More(g) => -200 - g as i32,
            Hit::ClearGroup(g) => -300 - g as i32,
            Hit::Item(i) => id(i, 0),
            Hit::Dismiss(i) => id(i, 1),
            Hit::Act(i, a) => id(i, 2 + i32::from(a)),
            Hit::Send(i) => id(i, 14),
            Hit::Field(i) => id(i, 15),
        }
    }

    /// The note it belongs to.
    #[must_use]
    pub const fn note(self) -> Option<u64> {
        match self {
            Hit::Item(i) | Hit::Dismiss(i) | Hit::Act(i, _) | Hit::Send(i) | Hit::Field(i) => Some(i),
            _ => None,
        }
    }
}

fn hot(tw: &mut dyn Tween, on: Option<Hit>, h: Hit, slot: u8, dur: u32) -> f32 {
    tw.go(h.key(), slot, f32::from(on == Some(h)), dur, EASE)
}

// ---------------------------------------------------------------------------------------------------------------------
// words

fn ago_text(a: Ago) -> String {
    match a {
        Ago::Now => t!("сейчас", "now", ja: "今", pt: "agora", zh: "刚刚").to_string(),
        Ago::Min(n) => tf!("{n} мин", "{n} min", ja: "{n}分前", pt: "{n} min", zh: "{n} 分钟前"),
        Ago::Hour(n) => tf!("{n} ч", "{n} h", ja: "{n}時間前", pt: "{n} h", zh: "{n} 小时前"),
        Ago::Day(n) => tf!("{n} дн", "{n} d", ja: "{n}日前", pt: "{n} d", zh: "{n} 天前"),
    }
}

/// The text the pop-up's top line shows in place of the time.
pub fn just_now() -> &'static str {
    t!("сейчас", "now", ja: "今", pt: "agora", zh: "刚刚")
}

pub fn dismiss_tip() -> &'static str {
    t!("Закрыть", "Dismiss", ja: "閉じる", pt: "Dispensar", zh: "关闭")
}

pub fn reply_to(title: &str) -> String {
    tf!("Ответить: {title}", "Reply to {title}", ja: "{title} に返信", pt: "Responder a {title}", zh: "回复 {title}")
}

pub fn send_tip() -> &'static str {
    t!("Отправить (Enter)", "Send (Enter)", ja: "送信 (Enter)", pt: "Enviar (Enter)", zh: "发送 (Enter)")
}

// ---------------------------------------------------------------------------------------------------------------------
// icons

type Loader = fn(&str) -> isize;

thread_local! {
    static LOADER: Cell<Option<Loader>> = const { Cell::new(None) };
    static ICONS: RefCell<HashMap<String, isize>> = RefCell::new(HashMap::new());
}

/// How an exe's icon is made (the shell's, at the size it is drawn). Set once by whoever has the shell at hand.
pub fn set_icon_loader(f: Loader) {
    LOADER.set(Some(f));
}

fn exe_icon(path: &str) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    let h = ICONS.with_borrow_mut(|m| *m.entry(path.to_string()).or_insert_with(|| LOADER.get().map_or(0, |f| f(path))));
    windows::Win32::UI::WindowsAndMessaging::HICON(h as *mut _)
}

/// The face of an app, 16 DIPs (`.ncg img`, a pop-up's top line).
pub fn app_icon(c: &mut Canvas<'_>, icon: &AppIcon, r: Rect, opacity: f32) {
    match icon {
        AppIcon::Winrust => {
            let k = r.w / 24.0;
            c.fill(r, 6.0 * k, Rgba::hex(0x1f2a3d).fade(opacity));
            let blue = Rgba::hex(0x62a0ea);
            c.fill(Rect::new(r.x + 4.0 * k, r.y + 4.0 * k, 7.0 * k, 7.0 * k), 1.6 * k, blue.fade(opacity));
            c.fill(Rect::new(r.x + 4.0 * k, r.y + 13.0 * k, 7.0 * k, 7.0 * k), 1.6 * k, blue.alpha(0.7 * opacity));
            c.icon("M14 5.5h2.6a2.6 2.6 0 0 1 0 5.2H14zM14 10.7l4.5 8M14 5.5v13.2", (r.cx(), r.cy()), r.w, 1.8, INK.fade(opacity), 0.0);
        }
        AppIcon::Plate(d, from, to) => {
            c.gradient_diagonal(r, r.w * 0.25, Rgba::hex(*from).fade(opacity), Rgba::hex(*to).fade(opacity));
            c.icon(d, (r.cx(), r.cy()), r.w * 0.72, 2.0, Rgba::pure_white(opacity), 0.0); // `stroke="#fff"` on a coloured plate
        }
        AppIcon::Exe(path) => {
            let ic = exe_icon(path);
            if ic.is_invalid() {
                c.gradient_diagonal(r, r.w * 0.25, Rgba::hex(0x3a3f4b).fade(opacity), Rgba::hex(0x22252d).fade(opacity));
            } else {
                c.hicon(ic, r, opacity);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the geometry of one card

/// Which of the two looks a card has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    /// A row of the centre (`.nci`): 360 wide, padding 12.
    Centre,
    /// A pop-up (`.pop`): 368 wide, padding 12 14 14, the app's name on top.
    Pop,
}

/// Where the parts of a card are, from the card's own top left corner.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parts {
    pub h: f32,
    pub avatar: Option<Rect>,
    /// The title's line (the unread dot, the title and, in the centre, the time share it).
    pub head: Rect,
    pub time_w: f32,
    pub body: Option<(Rect, usize)>,
    pub image: Option<Rect>,
    /// The reply field and its send button.
    pub reply: Option<(Rect, Rect)>,
    /// The buttons: the index into the note's `acts`, and where.
    pub acts: Vec<(usize, Rect)>,
    pub dismiss: Rect,
}

/// The note's buttons as they show in this flavour: the centre has no "Mark as read".
fn shown_acts(n: &Note, flavour: Flavour) -> Vec<(usize, &Act)> {
    n.acts.iter().enumerate().filter(|(_, a)| flavour == Flavour::Pop || a.does != Do::Read).collect()
}

/// Lays a note's card out for width `w`.
pub fn parts(n: &Note, flavour: Flavour, w: f32, replying: bool, now: u64, m: &mut dyn Measure) -> Parts {
    let mut p = Parts::default();
    let pop = flavour == Flavour::Pop;
    let (pad_x, top, bottom) = if pop { (14.0, 12.0, 14.0) } else { (12.0, 12.0, 12.0) };
    let av = if pop { 40.0 } else { 36.0 };
    let mut y = top;
    if pop {
        y += 20.0 + 10.0; // the line with the app's name
        p.dismiss = Rect::new(w - 8.0 - 24.0, top - 2.0, 24.0, 24.0);
    } else {
        p.dismiss = Rect::new(w - 8.0 - 24.0, 9.0, 24.0, 24.0);
    }
    let text_x = pad_x + if n.avatar.is_some() { av + 12.0 } else { 0.0 };
    let text_w = w - pad_x - text_x;
    if n.avatar.is_some() {
        p.avatar = Some(Rect::new(pad_x, y, av, av));
    }
    // the text column
    let mut col = 0.0;
    let title_h = if pop { 16.0 } else { 20.0 };
    p.head = Rect::new(text_x, y, text_w, title_h);
    col += title_h;
    if !pop {
        p.time_w = m.width(&ago_text(notify::ago(now, n.born)), SMALL);
    }
    if !n.body.is_empty() {
        let clamp = if pop { 3 } else if n.image.is_some() { 1 } else { 2 };
        let lines = m.lines(&n.body, TEXT, text_w).min(clamp);
        col += 3.0;
        p.body = Some((Rect::new(text_x, y + col, text_w, lines as f32 * LINE), lines));
        col += lines as f32 * LINE;
    }
    let row = if n.avatar.is_some() { col.max(av) } else { col };
    // the rest runs under the whole row in a pop-up, under the text in the centre
    let (rx, rw, mut yy) = if pop { (pad_x, w - 2.0 * pad_x, y + row) } else { (text_x, text_w, y + col) };
    let gap = if pop { 0.0 } else { 3.0 };
    if n.image.is_some() {
        let (mt, h) = if pop { (10.0, 150.0) } else { (8.0, 110.0) };
        yy += gap + mt;
        p.image = Some(Rect::new(rx, yy, rw, h));
        yy += h;
    }
    if replying {
        let (mt, h, r) = if pop { (10.0, 38.0, 30.0) } else { (8.0, 36.0, 28.0) };
        yy += gap + mt;
        let field = Rect::new(rx, yy, rw, h);
        p.reply = Some((field, Rect::new(field.x + field.w - 4.0 - r, field.cy() - r / 2.0, r, r)));
        yy += h;
    } else {
        let acts = shown_acts(n, flavour);
        if !acts.is_empty() {
            if pop {
                yy += 12.0;
                let each = (rw - 8.0 * (acts.len() as f32 - 1.0)) / acts.len() as f32;
                for (k, (i, _)) in acts.iter().enumerate() {
                    p.acts.push((*i, Rect::new(rx + k as f32 * (each + 8.0), yy, each, 32.0)));
                }
                yy += 32.0;
            } else {
                yy += gap + 8.0;
                let (mut x, mut y0) = (0.0, yy);
                for (i, a) in &acts {
                    let bw = m.width(&a.label, Font::new(12.0, 600)).ceil() + 24.0;
                    if x > 0.0 && x + bw > rw {
                        x = 0.0;
                        y0 += 28.0 + 6.0;
                    }
                    p.acts.push((*i, Rect::new(rx + x, y0, bw, 28.0)));
                    x += bw + 6.0;
                }
                yy = y0 + 28.0;
            }
        }
    }
    p.h = yy.max(y + row) + bottom;
    p
}

// ---------------------------------------------------------------------------------------------------------------------
// the pieces of a card that both flavours draw

/// The round letter (`.avatar`).
pub fn avatar(c: &mut Canvas<'_>, a: &notify::Avatar, r: Rect, size: f32) {
    c.gradient_diagonal(r, r.w / 2.0, Rgba::hex(a.from), Rgba::hex(a.to));
    c.text(&a.letter.to_string(), r, Font::new(size, 600), Rgba::pure_white(1.0), Align::Center);
}

/// The unread dot and the title, nowrap with an ellipsis.
pub fn title(c: &mut Canvas<'_>, n: &Note, head: Rect, right: f32, dot: bool) {
    let mut x = head.x;
    if dot {
        c.circle(x + 3.5, head.cy(), 3.5, accent());
        x += 13.0;
    }
    c.text_fit(&n.title, Rect::new(x, head.y, (head.x + head.w - right - x).max(0.0), head.h), TITLE, INK, Align::Start);
}

pub fn body(c: &mut Canvas<'_>, n: &Note, b: (Rect, usize)) {
    c.text_wrapped(&n.body, b.0, TEXT, BODY, LINE, b.1);
}

/// The screenshots under the notes, read and scaled once. A picture belongs to the render target it was made on, so each
/// window that draws cards keeps its own set.
#[derive(Default)]
pub struct Pictures(RefCell<HashMap<(String, u32, u32), Option<super::gfx::Picture>>>);

impl Pictures {
    /// The pictures are the device's: a window that is gone from the screen lets them go.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

/// A screenshot under the text: covers its box, sticks to the bottom (`object-fit: cover; object-position: 50% 100%`), with a
/// hairline ring.
pub fn picture(c: &mut Canvas<'_>, pics: &Pictures, path: &str, r: Rect) {
    let key = (path.to_string(), r.w.to_bits(), r.h.to_bits());
    let mut m = pics.0.borrow_mut();
    if m.len() > 12 && !m.contains_key(&key) {
        m.clear();
    }
    match m.entry(key).or_insert_with(|| c.image(path, r.w, r.h)) {
        Some(p) => c.picture_cover_at(p, r, 10.0, 1.0, 1.0),
        None => c.fill(r, 10.0, Rgba::white(0.06)),
    }
    c.ring(r, 10.0, 1.0, Rgba::white(0.08));
}

/// The cover of a video: a play button over the middle of its picture and its length in the lower right corner.
pub fn video_badge(c: &mut Canvas<'_>, n: &Note, r: Rect) {
    let Some(len) = n.video.as_deref() else { return };
    c.fill(Rect::new(r.cx() - 17.0, r.cy() - 17.0, 34.0, 34.0), 17.0, Rgba::css(12, 13, 18, 0.72));
    c.glyph("M8 5v14l11-7z", (r.cx(), r.cy()), 13.0, Rgba::pure_white(1.0));
    let f = Font::new(11.0, 600).tabular();
    let w = c.measure(len, f) + 12.0;
    let chip = Rect::new(r.x + r.w - 6.0 - w, r.y + r.h - 6.0 - 18.0, w, 18.0);
    c.fill(chip, 5.0, Rgba::css(12, 13, 18, 0.78));
    c.text(len, chip, f, Rgba::pure_white(1.0), Align::Center);
}

/// `.ncr`: the field an answer is typed in, with the caret and the send button.
#[allow(clippy::too_many_arguments)]
pub fn reply_field(c: &mut Canvas<'_>, tw: &mut dyn Tween, hov: Option<Hit>, id: u64, n: &Note, text: &str, r: (Rect, Rect), pop: bool, since: u64, now: u64) {
    let (field, send) = r;
    let k = (now.saturating_sub(since) as f32 / 240.0).min(1.0);
    let e = SPRING.ease(k);
    c.faded(e, |c| {
        c.transformed(field.cx(), field.y, 1.0, 0.0, -4.0 * (1.0 - e), |c| {
            let radius = if pop { 11.0 } else { 10.0 };
            c.fill(field, radius, Rgba::new(0.0, 0.0, 0.0, 0.25));
            c.ring(field, radius, 1.5, accent().alpha(0.7));
            let tx = Rect::new(field.x + 12.0, field.y, field.w - 12.0 - send.w - 10.0, field.h);
            if text.is_empty() {
                c.text_fit(&reply_to(&n.title), tx, TEXT, FAINT, Align::Start);
            } else {
                let w = c.measure(text, TEXT);
                let shown = Rect::new(tx.x - (w - tx.w + 2.0).max(0.0), tx.y, w.max(tx.w), tx.h);
                c.clipped(tx, |c| c.text(text, shown, TEXT, INK, Align::Start));
            }
            // the caret blinks
            if (now / 530).is_multiple_of(2) {
                let w = c.measure(text, TEXT).min(tx.w - 2.0);
                c.fill(Rect::new(tx.x + w + 1.0, field.cy() - 8.0, 1.0, 16.0), 0.0, INK);
            }
            let full = !text.trim().is_empty();
            let down = hot(tw, hov, Hit::Send(id), 0, 160);
            let bg = Rgba::white(0.08).mix(accent().accent_fill(), f32::from(full));
            c.fill(send, 8.0, bg.mix(Rgba::pure_white(bg.a.max(0.2)), 0.12 * down * f32::from(full)));
            c.icon(SEND, (send.cx(), send.cy()), 14.0, 2.2, if full { ON_ACCENT } else { NUMB }, 0.0);
        });
    });
}

/// `.ncb`: a button under a note.
#[allow(clippy::too_many_arguments)]
pub fn act_button(c: &mut Canvas<'_>, tw: &mut dyn Tween, hov: Option<Hit>, h: Hit, r: Rect, label: &str, first: bool, danger: bool, pop: bool) {
    let over = hot(tw, hov, h, 0, 140);
    let (bg, ink) = match (first, danger) {
        (true, true) => (RED.alpha(0.2), RED_INK),
        (true, false) => (accent().alpha(0.24), INK),
        _ => (Rgba::white(0.08), CHIP),
    };
    let radius = if pop { 10.0 } else { 8.0 };
    let size = if pop { 12.5 } else { 12.0 };
    c.fill(r, radius, bg);
    c.fill(r, radius, Rgba::white(0.1 * over)); // `filter: brightness(1.2)`
    c.text_fit(label, r.inset(6.0).shift(0.0, 0.0), Font::new(size, 600), ink, Align::Center);
}

/// The x of a note: a small square that comes in on hover.
pub fn dismiss_button(c: &mut Canvas<'_>, tw: &mut dyn Tween, hov: Option<Hit>, id: u64, r: Rect, card_hot: bool, pop: bool) {
    let h = Hit::Dismiss(id);
    let show = tw.go(h.key(), 1, f32::from(card_hot || pop), 140, EASE);
    let grow = tw.go(h.key(), 2, f32::from(card_hot || pop), 180, SPRING);
    let over = hot(tw, hov, h, 0, 140);
    if show <= 0.003 {
        return;
    }
    c.faded(show, |c| {
        c.transformed(r.cx(), r.cy(), lerp(0.85, 1.0, grow), 0.0, 0.0, |c| {
            if pop {
                // `.hv` of a pop-up's x: it brightens
                c.fill(r, 7.0, Rgba::white(0.1 * over));
                c.icon(CROSS, (r.cx(), r.cy()), 10.0, 2.8, DIM.mix(INK, over), 0.0);
            } else {
                // `--ncxBg` and `--ncxSh`: a slate plate on the dark style, a white one with a soft shadow on the light one; the red
                // hover turns the x white
                let light = winrust_common::style::light();
                if light {
                    c.outside(r, 7.0, |c| c.shadow(r, 7.0, 1.0, 3.0, Rgba::white(0.16)));
                }
                c.fill(r, 7.0, if light { Rgba::pure_white(1.0) } else { Rgba::css(52, 54, 64, 0.96) }.mix(RED.alpha(0.9), over));
                if light {
                    c.ring(r, 7.0, 0.5, Rgba::white(0.12));
                }
                c.icon(CROSS, (r.cx(), r.cy()), 10.0, 2.8, INK.mix(Rgba::pure_white(1.0), over), 0.0);
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// the centre

const W: f32 = 380.0;
const HEAD_H: f32 = 52.0;
const PAD: f32 = 10.0;
const ITEM_W: f32 = W - 2.0 * PAD;
const EMPTY_H: f32 = 128.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry {
    rect: Rect,
    hit: Hit,
    /// Index of the group it is in.
    group: Option<usize>,
    /// Under the header: only valid inside the list's viewport.
    scrolls: bool,
}

#[derive(Clone, Debug)]
struct ItemL {
    id: u64,
    /// The card as it is now (collapsing: shorter than `full`).
    full: Rect,
    shown_h: f32,
    parts: Parts,
    /// 1 at rest, 0 collapsed.
    k: f32,
    /// How far the dismissal has gone, 0..1 (for the fade and the slide).
    away: f32,
}

#[derive(Clone, Debug)]
struct GroupL {
    app: App,
    key: String,
    head: Rect,
    items: Vec<ItemL>,
    /// The stack (the cards and the peeking one), for hover.
    stack: Rect,
    open: bool,
    can_expand: bool,
    pill: Rect,
    pill_text: String,
    clear: Rect,
}

#[derive(Clone, Debug, Default)]
struct Layout {
    h: f32,
    /// The part of the card the list shows, in card coordinates.
    view: Rect,
    content: f32,
    groups: Vec<GroupL>,
    hits: Vec<Entry>,
    dnd_strip: Option<Rect>,
    empty: Option<Rect>,
    any: bool,
}

/// What the store says now, copied out so the layout and the paint do not hold it.
struct Snap {
    notes: HashMap<u64, Note>,
    groups: Vec<notify::Group>,
    dnd: bool,
    unread: usize,
    reply: Option<notify::Reply>,
    highlight: Option<(u64, f32)>,
    undo: bool,
    count: usize,
    total: usize,
}

fn snapshot(now: u64) -> Snap {
    notify::with(|s| {
        s.tick(now);
        Snap {
            notes: s.notes().iter().map(|n| (n.id, n.clone())).collect(),
            groups: s.groups(),
            dnd: s.dnd(),
            unread: s.unread(),
            reply: s.reply().cloned(),
            highlight: s.highlight(now),
            undo: s.can_undo(),
            count: s.count(),
            total: s.notes().len(),
        }
    })
}

pub struct Centre {
    /// The most the card may be tall, DIPs.
    room: f32,
    /// The width of a row: 360, or 350 when the list scrolls and its bar takes 10.
    iw: f32,
    lay: Layout,
    scroll: f32,
    /// When the panel opened (the first layout): notes that are fresh come in from then.
    opened: Option<u64>,
    dnd_since: Option<u64>,
    empty_since: Option<u64>,
    reply_since: Option<u64>,
    /// Until when something moves by itself.
    until: u64,
    parts: HashMap<(u64, bool), Parts>,
    pics: Pictures,
    /// The note whose reply field is on screen (its `since` belongs to it).
    replying: Option<u64>,
    snap: Option<Snap>,
}

impl Centre {
    #[must_use]
    pub fn new(room: f32) -> Self {
        Self {
            room: room.max(HEAD_H + EMPTY_H),
            iw: ITEM_W,
            lay: Layout::default(),
            scroll: 0.0,
            opened: None,
            dnd_since: None,
            empty_since: None,
            reply_since: None,
            until: 0,
            parts: HashMap::new(),
            pics: Pictures::default(),
            replying: None,
            snap: None,
        }
    }

    /// The card's width.
    pub const WIDTH: f32 = W;

    #[must_use]
    pub fn typing(&self) -> bool {
        self.replying.is_some()
    }

    /// Something moves by itself (a collapse, a note coming in, the highlight): the panel keeps asking for frames.
    #[must_use]
    pub fn animating(&self, now: u64) -> bool {
        now < self.until
    }

    /// Lays the card out for `now` and says how tall it is. Call before every frame.
    pub fn relayout(&mut self, m: &mut dyn Measure, now: u64) -> f32 {
        let snap = snapshot(now);
        self.opened.get_or_insert(now);
        let mut until = 0u64;
        let mut lay = Layout { any: snap.total > 0, ..Layout::default() };
        let mut y = HEAD_H;
        // the strip that says the pop-ups are silenced (`ncdIn` when it appears)
        if snap.dnd {
            self.dnd_since.get_or_insert(now);
            lay.dnd_strip = Some(Rect::new(PAD, y, ITEM_W, 40.0));
            y += 48.0;
            until = until.max(self.dnd_since.unwrap_or(now) + 260);
        } else {
            self.dnd_since = None;
        }
        // what the reply field belongs to
        let replying = snap.reply.as_ref().map(|r| r.id);
        if replying != self.replying {
            self.replying = replying;
            self.reply_since = replying.map(|_| now);
        }
        if let Some(t) = self.reply_since.filter(|_| replying.is_some()) {
            until = until.max(t + 240);
        }
        let list_top = y;
        let mut content = 0.0;
        let mut groups: Vec<GroupL> = Vec::new();
        if lay.any {
            self.empty_since = None;
            let avail = (self.room - list_top).max(0.0);
            // the list scrolls when it does not fit, and a scrollbar takes 10 px of its width: the rows are laid out again, narrower
            for iw in [ITEM_W, ITEM_W - 10.0] {
                if (self.iw - iw).abs() > 0.01 {
                    self.iw = iw;
                    self.parts.clear();
                }
                (content, groups) = self.build(m, &snap, now, replying, &mut until);
                if content <= avail {
                    break;
                }
            }
            let view_h = content.min(avail);
            self.scroll = self.scroll.clamp(0.0, (content - view_h).max(0.0));
            lay.view = Rect::new(0.0, list_top, W, view_h);
            lay.h = list_top + view_h;
        } else {
            let since = *self.empty_since.get_or_insert(now);
            until = until.max(since + 300);
            lay.empty = Some(Rect::new(0.0, list_top, W, EMPTY_H));
            lay.view = Rect::new(0.0, list_top, W, 0.0);
            lay.h = list_top + EMPTY_H;
            self.scroll = 0.0;
        }
        lay.content = content;

        // move the list into the card (scrolled) and make the table of what can be hit
        let oy = list_top - self.scroll;
        for g in &mut groups {
            g.head = g.head.shift(0.0, oy);
            g.stack = g.stack.shift(0.0, oy);
            g.pill = g.pill.shift(0.0, oy);
            g.clear = g.clear.shift(0.0, oy);
            for it in &mut g.items {
                it.full = it.full.shift(0.0, oy);
            }
        }
        // the header: [title][unread] ... [Do not disturb][Clear all], 8 apart, 10 from the right edge
        let clear_w = if lay.any { m.width(t!("Очистить все", "Clear all", ja: "すべてクリア", pt: "Limpar tudo", zh: "全部清除"), Font::new(12.5, 500)).ceil() + 20.0 } else { 0.0 };
        let dnd_x = if lay.any { W - 10.0 - clear_w - 8.0 - 32.0 } else { W - 10.0 - 32.0 };
        let mut hits = vec![Entry { rect: Rect::new(dnd_x, 10.0, 32.0, 32.0), hit: Hit::Dnd, group: None, scrolls: false }];
        if lay.any {
            hits.push(Entry { rect: Rect::new(W - 10.0 - clear_w, 10.0, clear_w, 32.0), hit: Hit::ClearAll, group: None, scrolls: false });
        }
        if let Some(s) = lay.dnd_strip {
            let bw = m.width(t!("Выключить", "Turn off", ja: "オフにする", pt: "Desativar", zh: "关闭"), Font::new(12.0, 600)).ceil() + 20.0;
            hits.push(Entry { rect: Rect::new(s.x + s.w - 6.0 - bw, s.y + 6.0, bw, 28.0), hit: Hit::DndOff, group: None, scrolls: false });
        }
        if lay.empty.is_some() && snap.undo {
            let bw = m.width(t!("Отменить", "Undo", ja: "元に戻す", pt: "Desfazer", zh: "撤销"), Font::new(12.0, 600)).ceil() + 24.0;
            // 40 + 8 + 16 + 8 + 28 centred in 128
            let top = list_top + (EMPTY_H - (40.0 + 8.0 + 16.0 + 8.0 + 28.0)) / 2.0;
            hits.push(Entry { rect: Rect::new((W - bw) / 2.0, top + 40.0 + 8.0 + 16.0 + 8.0, bw, 28.0), hit: Hit::Undo, group: None, scrolls: false });
        }
        for (gi, g) in groups.iter().enumerate() {
            let gi32 = gi as u32;
            if g.can_expand {
                hits.push(Entry { rect: g.pill, hit: Hit::More(gi32), group: Some(gi), scrolls: true });
            }
            hits.push(Entry { rect: g.clear, hit: Hit::ClearGroup(gi32), group: Some(gi), scrolls: true });
            if !g.open {
                hits.push(Entry { rect: g.stack, hit: Hit::Stack(gi32), group: Some(gi), scrolls: true });
            }
            for it in &g.items {
                if it.k < 0.5 || it.away > 0.0 {
                    continue;
                }
                let card = it.full;
                if g.open {
                    hits.push(Entry { rect: card, hit: Hit::Item(it.id), group: Some(gi), scrolls: true });
                }
                let at = |r: Rect| r.shift(card.x, card.y);
                hits.push(Entry { rect: at(it.parts.dismiss), hit: Hit::Dismiss(it.id), group: Some(gi), scrolls: true });
                if let Some((f, s)) = it.parts.reply {
                    hits.push(Entry { rect: at(f), hit: Hit::Field(it.id), group: Some(gi), scrolls: true });
                    hits.push(Entry { rect: at(s), hit: Hit::Send(it.id), group: Some(gi), scrolls: true });
                }
                for (a, r) in &it.parts.acts {
                    hits.push(Entry { rect: at(*r), hit: Hit::Act(it.id, *a as u8), group: Some(gi), scrolls: true });
                }
            }
        }
        lay.hits = hits;
        lay.groups = groups;
        if let Some((_, p)) = snap.highlight {
            until = until.max(now + ((1.0 - p) * notify::HIGHLIGHT_MS as f32) as u64);
        }
        self.until = until;
        let h = lay.h;
        self.lay = lay;
        self.snap = Some(snap);
        self.parts.retain(|(id, _), _| self.snap.as_ref().is_some_and(|s| s.notes.contains_key(id)));
        h
    }


    /// The groups and their rows at the width `self.iw`, in the list's own coordinates; returns them and the list's height.
    fn build(&mut self, m: &mut dyn Measure, snap: &Snap, now: u64, replying: Option<u64>, until: &mut u64) -> (f32, Vec<GroupL>) {
        let (iw, opened) = (self.iw, self.opened.unwrap_or(now));
        let mut content = 0.0;
        let mut groups: Vec<GroupL> = Vec::new();
        for (gi, g) in snap.groups.iter().enumerate() {
            if gi > 0 {
                content += 12.0;
            }
            let head = Rect::new(PAD, content, iw, 24.0);
            content += 24.0 + 6.0;
            let stack_top = content;
            let shown = g.shown();
            let mut items = Vec::new();
            for (ii, id) in shown.iter().enumerate() {
                let Some(n) = snap.notes.get(id) else { continue };
                let rep = replying == Some(*id);
                let p = self.parts.entry((*id, rep)).or_insert_with(|| parts(n, Flavour::Centre, iw, rep, now, m)).clone();
                // `.nciw.gone`: the row shrinks to nothing in .3 s while it fades and slides away
                let away = n.gone.map_or(0.0, |t| (now.saturating_sub(t) as f32 / 300.0).min(1.0));
                let (dur, _) = timing(winrust_common::style::anim(), 300, 0);
                let away = if dur == 0 && n.gone.is_some_and(|t| now >= t) { 1.0 } else { away };
                if let Some(t) = n.gone {
                    *until = (*until).max(t + 300);
                }
                let k = 1.0 - SPRING.ease(away);
                content += if ii > 0 { 6.0 * k } else { 0.0 };
                items.push(ItemL { id: *id, full: Rect::new(PAD, content, iw, p.h), shown_h: p.h * k, parts: p, k, away });
                content += items.last().map_or(0.0, |i| i.shown_h);
                if n.fresh {
                    *until = (*until).max(opened.max(n.born) + 400);
                }
            }
            if !g.open {
                content += 8.0;
            }
            let stack = Rect::new(PAD, stack_top, iw, content - stack_top);
            let pill_text = if g.open { t!("Свернуть", "Show less", ja: "折りたたむ", pt: "Mostrar menos", zh: "收起").to_string() } else { format!("+{}", g.ids.len() - 1) };
            let pill_w = if g.can_expand() { m.width(&pill_text, Font::new(11.5, 500)).ceil() + 8.0 + 3.0 + 12.0 + 4.0 } else { 0.0 };
            // a long name gives way to the pill and the Clear button (it is drawn trimmed to what is left)
            let room = (head.w - (6.0 + 16.0 + 8.0) - 8.0 - pill_w - 4.0 - (2.0 + 24.0)).max(0.0);
            let name_w = m.width(&g.app.name, Font::new(12.0, 600)).ceil().min(room);
            let pill = Rect::new(head.x + 6.0 + 16.0 + 8.0 + name_w + 8.0, head.y + 1.0, pill_w, 22.0);
            groups.push(GroupL {
                app: g.app.clone(),
                key: g.app.key.clone(),
                head,
                items,
                stack,
                open: g.open,
                can_expand: g.can_expand(),
                pill,
                pill_text,
                clear: Rect::new(head.x + head.w - 2.0 - 24.0, head.y, 24.0, 24.0),
            });
        }
        (content + 14.0, groups)
    }

    // -- input

    #[must_use]
    pub fn hit(&self, x: f32, y: f32) -> Option<i32> {
        let view = self.lay.view;
        self.lay
            .hits
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.rect.contains(x, y) && (!e.scrolls || (view.contains(x, y) && e.rect.h > 0.0)))
            .map(|(i, _)| i as i32)
    }

    /// The tip of a control (the mockup's `title`), and the rectangle it belongs to.
    #[must_use]
    pub fn tip(&self, id: i32) -> Option<(String, Rect)> {
        let e = self.lay.hits.get(usize::try_from(id).ok()?)?;
        let on = self.snap.as_ref().is_some_and(|s| s.dnd);
        match e.hit {
            Hit::Dnd => Some((if on { t!("Режим «Не беспокоить» включён", "Do not disturb is on", ja: "応答不可はオンです", pt: "Não perturbe está ativado", zh: "勿扰已开启") } else { t!("Не беспокоить", "Do not disturb", ja: "応答不可", pt: "Não perturbe", zh: "勿扰") }.to_string(), e.rect)),
            _ => None,
        }
    }

    pub fn wheel(&mut self, up: bool) {
        let max = (self.lay.content - self.lay.view.h).max(0.0);
        self.scroll = (self.scroll + if up { -60.0 } else { 60.0 }).clamp(0.0, max);
    }

    /// A click on control `id` of the last layout.
    pub fn click(&mut self, id: i32, b: Button, now: u64, cx: &mut Cx) {
        if b != Button::Left {
            return;
        }
        self.clicked(id, now, cx);
        notify::bar_changed(); // the badge on the clock follows what was read, cleared or silenced
    }

    fn clicked(&mut self, id: i32, now: u64, cx: &mut Cx) {
        let Some(e) = usize::try_from(id).ok().and_then(|i| self.lay.hits.get(i)).copied() else { return };
        let key = |gi: u32| self.lay.groups.get(gi as usize).map(|g| g.key.clone());
        match e.hit {
            Hit::Dnd => toggle_dnd(),
            Hit::DndOff => toggle_dnd(),
            Hit::ClearAll => notify::with(|s| s.clear_all(now)),
            Hit::Undo => drop(notify::with(notify::Store::undo)),
            Hit::Stack(g) | Hit::More(g) => {
                if let Some(k) = key(g) {
                    let toggle = matches!(e.hit, Hit::More(_));
                    notify::with(|s| if toggle { s.toggle_stack(&k) } else { s.open_stack(&k) });
                }
            }
            Hit::ClearGroup(g) => {
                if let Some(k) = key(g) {
                    notify::with(|s| s.clear_app(&k, now));
                }
            }
            Hit::Dismiss(i) => notify::with(|s| s.dismiss(i, now)),
            Hit::Item(i) => note_clicked(i, now, cx),
            Hit::Act(i, a) => act_clicked(i, usize::from(a), now, cx),
            Hit::Send(_) => send_clicked(now, cx),
            Hit::Field(_) => {}
        }
    }

    /// A typed character goes to the reply field.
    pub fn text(&mut self, ch: char) {
        notify::with(|s| s.reply_char(ch));
    }

    /// Enter in the reply field.
    pub fn enter(&mut self, now: u64, cx: &mut Cx) {
        if self.replying.is_some() {
            send_clicked(now, cx);
        }
    }

    /// Esc: closes the reply field first. True if it did.
    pub fn escape(&mut self) -> bool {
        notify::with(|s| s.reply().is_some() && {
            s.cancel_reply();
            true
        })
    }

    // -- painting

    /// Draws the card at the box's origin (its glass and rings are the panel's).
    pub fn draw(&mut self, c: &mut Canvas<'_>, tw: &mut dyn Tween, hover: Option<i32>, now: u64) {
        let Some(snap) = self.snap.take() else { return };
        let lay = std::mem::take(&mut self.lay);
        let hov_entry = hover.and_then(|i| usize::try_from(i).ok()).and_then(|i| lay.hits.get(i)).copied();
        let on = hov_entry.map(|e| e.hit);
        // a closed stack shows its newest note under the pointer
        let on_note = on.and_then(Hit::note).or_else(|| match on {
            Some(Hit::Stack(g)) => lay.groups.get(g as usize).and_then(|g| g.items.first()).map(|i| i.id),
            _ => None,
        });
        let on_group = hov_entry.and_then(|e| e.group);
        let ac = accent();

        // the header
        c.text(t!("Уведомления", "Notifications", ja: "通知", pt: "Notificações", zh: "通知"), Rect::new(18.0, 0.0, 200.0, HEAD_H), Font::new(14.0, 600), INK, Align::Start);
        if snap.unread > 0 {
            let tw_ = c.measure(&snap.unread.to_string(), Font::new(11.5, 600));
            let chip_w = (tw_.ceil() + 12.0).max(20.0);
            let title_w = c.measure(t!("Уведомления", "Notifications", ja: "通知", pt: "Notificações", zh: "通知"), Font::new(14.0, 600));
            let chip = Rect::new(18.0 + title_w + 8.0, HEAD_H / 2.0 - 10.0, chip_w, 20.0);
            c.fill(chip, 10.0, Rgba::white(0.08));
            c.text(&snap.unread.to_string(), chip, Font::new(11.5, 600), CHIP, Align::Center);
        }
        for e in lay.hits.iter().filter(|e| matches!(e.hit, Hit::Dnd | Hit::ClearAll)) {
            let over = hot(tw, on, e.hit, 0, 250);
            match e.hit {
                Hit::Dnd => {
                    let d = tw.go(Hit::Dnd.key(), 1, f32::from(snap.dnd), 250, EASE);
                    c.fill(e.rect, 9.0, LILAC.alpha(0.22 * d));
                    c.fill(e.rect, 9.0, Rgba::white(0.1 * over));
                    let ink = DIM.mix(LILAC, d);
                    if snap.dnd {
                        c.glyph(MOON, (e.rect.cx(), e.rect.cy()), 16.0, ink);
                    }
                    c.icon(MOON, (e.rect.cx(), e.rect.cy()), 16.0, 1.9, ink, 0.0);
                }
                _ => {
                    c.fill(e.rect, 9.0, Rgba::white(0.1 * over));
                    c.text(t!("Очистить все", "Clear all", ja: "すべてクリア", pt: "Limpar tudo", zh: "全部清除"), e.rect, Font::new(12.5, 500), DIM, Align::Center);
                }
            }
        }
        if let Some(s) = lay.dnd_strip {
            let e = SPRING.ease((now.saturating_sub(self.dnd_since.unwrap_or(now)) as f32 / 260.0).min(1.0));
            c.faded(e, |c| {
                c.transformed(s.cx(), s.y, 1.0, 0.0, -4.0 * (1.0 - e), |c| {
                    c.fill(s, 12.0, Rgba::white(0.06));
                    c.glyph(MOON, (s.x + 12.0 + 7.0, s.cy()), 14.0, LILAC);
                    c.text(t!("Всплывающие уведомления отключены", "Pop-ups are silenced", ja: "ポップアップは表示されません", pt: "Pop-ups silenciados", zh: "已静音弹出通知"), Rect::new(s.x + 36.0, s.y, s.w - 36.0 - 90.0, s.h), TEXT, CHIP, Align::Start);
                    if let Some(b) = lay.hits.iter().find(|e| e.hit == Hit::DndOff) {
                        let over = hot(tw, on, Hit::DndOff, 0, 250);
                        c.fill(b.rect, 8.0, Rgba::white(0.1 * over));
                        c.text(t!("Выключить", "Turn off", ja: "オフにする", pt: "Desativar", zh: "关闭"), b.rect, Font::new(12.0, 600), ac.accent_text(), Align::Center);
                    }
                });
            });
        }

        if let Some(r) = lay.empty {
            self.draw_empty(c, tw, on, (&lay, r), &snap, now);
        } else {
            self.draw_list(c, tw, &lay, &snap, (on, on_note, on_group), now);
        }
        self.lay = lay;
        self.snap = Some(snap);
    }

    fn draw_empty(&self, c: &mut Canvas<'_>, tw: &mut dyn Tween, on: Option<Hit>, (lay, r): (&Layout, Rect), snap: &Snap, now: u64) {
        let e = SPRING.ease((now.saturating_sub(self.empty_since.unwrap_or(now)) as f32 / 300.0).min(1.0));
        let top = r.y + (EMPTY_H - (40.0 + 8.0 + 16.0 + 8.0 + 28.0)) / 2.0 + if snap.undo { 0.0 } else { 6.0 };
        c.faded(e, |c| {
            c.transformed(r.cx(), r.cy(), lerp(0.96, 1.0, e), 0.0, 0.0, |c| {
                c.circle(r.cx(), top + 20.0, 20.0, Rgba::white(0.06));
                c.icon(BELL, (r.cx(), top + 20.0), 18.0, 1.8, MUTED, 0.0);
                let line = if snap.undo { t!("Уведомления очищены", "Notifications cleared", ja: "通知をクリアしました", pt: "Notificações limpas", zh: "通知已清除") } else { t!("Всё прочитано", "You’re all caught up", ja: "すべて確認しました", pt: "Tudo em dia", zh: "没有新通知") };
                c.text(line, Rect::new(r.x, top + 48.0, r.w, 16.0), Font::new(13.0, 500), CHIP, Align::Center);
                if snap.undo {
                    let b = lay.hits.iter().find(|e| e.hit == Hit::Undo).map(|e| e.rect);
                    if let Some(b) = b {
                        let over = hot(tw, on, Hit::Undo, 0, 250);
                        c.fill(b, 8.0, Rgba::white(0.06 + 0.04 * over));
                        c.text(t!("Отменить", "Undo", ja: "元に戻す", pt: "Desfazer", zh: "撤销"), b, Font::new(12.0, 600), accent().accent_text(), Align::Center);
                    }
                } else {
                    c.text(t!("Новые уведомления появятся здесь", "New notifications will show up here", ja: "新しい通知はここに表示されます", pt: "Novas notificações aparecerão aqui", zh: "新通知将显示在这里"), Rect::new(r.x, top + 48.0 + 16.0 + 8.0, r.w, 16.0), Font::new(12.0, 400), MUTED, Align::Center);
                }
            });
        });
    }

    fn draw_list(&self, c: &mut Canvas<'_>, tw: &mut dyn Tween, lay: &Layout, snap: &Snap, on: (Option<Hit>, Option<u64>, Option<usize>), now: u64) {
        let (hov, on_note, on_group) = on;
        let view = lay.view;
        let opened = self.opened.unwrap_or(now);
        c.clipped(view, |c| {
            for (gi, g) in lay.groups.iter().enumerate() {
                let group_hot = on_group == Some(gi);
                // the group's line: its face, its name, the pill, the x
                app_icon(c, &g.app.icon, Rect::new(g.head.x + 6.0, g.head.cy() - 8.0, 16.0, 16.0), 1.0);
                c.text_fit(&g.app.name, Rect::new(g.head.x + 6.0 + 16.0 + 8.0, g.head.y, g.pill.x - 8.0 - (g.head.x + 6.0 + 16.0 + 8.0), g.head.h), Font::new(12.0, 600), CHIP, Align::Start);
                if g.can_expand {
                    let h = Hit::More(gi as u32);
                    let over = hot(tw, hov, h, 0, 250);
                    let turn = tw.go(h.key(), 1, f32::from(g.open) * 180.0, 300, SPRING);
                    c.fill(g.pill, 11.0, Rgba::white(0.06 + 0.04 * over));
                    c.text(&g.pill_text, Rect::new(g.pill.x + 8.0, g.pill.y, g.pill.w - 8.0 - 3.0 - 12.0 - 4.0, g.pill.h), Font::new(11.5, 500), DIM, Align::Start);
                    c.icon(CHEVRON, (g.pill.x + g.pill.w - 4.0 - 6.0, g.pill.cy()), 12.0, 2.4, DIM, turn);
                }
                let h = Hit::ClearGroup(gi as u32);
                let show = tw.go(h.key(), 1, f32::from(group_hot), 140, EASE);
                let over = hot(tw, hov, h, 0, 140);
                c.faded(show, |c| {
                    c.fill(g.clear, 7.0, Rgba::white(0.1 * over));
                    c.icon(CROSS, (g.clear.cx(), g.clear.cy()), 10.0, 2.8, DIM, 0.0);
                });
                // the stack: a card peeking from under the newest one
                if !g.open {
                    let lift = tw.go(Hit::Stack(gi as u32).key(), 0, f32::from(group_hot), 220, SPRING) * 2.0;
                    let bottom = g.stack.y + g.stack.h;
                    let r = Rect::new(g.stack.x + 12.0, bottom - 24.0 + lift, g.stack.w - 24.0, 24.0);
                    c.fill(r.shift(0.0, 1.0), 14.0, Rgba::white(0.04));
                    c.fill(r, 14.0, PEEK);
                }
                for it in &g.items {
                    let Some(n) = snap.notes.get(&it.id) else { continue };
                    self.draw_item(c, tw, g, it, n, snap, (hov, on_note), (now, opened));
                }
            }
        });
        // `mask-image: linear-gradient(#000 calc(100% - 22px), transparent)`: the last 22 px of the list fade into the card
        let fade = Rect::new(PAD, view.y + view.h - 22.0, self.iw, 22.0);
        c.gradient(fade, 12.0, CARD.alpha(0.0), CARD);
        if lay.content > view.h + 0.5 {
            let k = view.h / lay.content;
            let (th, ty) = ((view.h * k).max(24.0), view.y + (view.h - (view.h * k).max(24.0)) * self.scroll / (lay.content - view.h));
            c.fill(Rect::new(W - 7.0, ty, 4.0, th), 2.0, Rgba::white(0.14));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_item(&self, c: &mut Canvas<'_>, tw: &mut dyn Tween, _g: &GroupL, it: &ItemL, n: &Note, snap: &Snap, on: (Option<Hit>, Option<u64>), time: (u64, u64)) {
        let (hov, on_note) = on;
        let (now, opened) = time;
        if it.shown_h <= 0.5 {
            return;
        }
        let card = it.full;
        let p = &it.parts;
        let hot_card = on_note == Some(it.id);
        let over = tw.go(Hit::Item(it.id).key(), 0, f32::from(hot_card), 180, EASE);
        let clip = Rect::new(card.x - 2.0, card.y - 2.0, card.w + 4.0, it.shown_h + 4.0);
        let fade = 1.0 - EASE.ease((it.away * 300.0 / 200.0).min(1.0));
        let slide = 28.0 * SPRING.ease(it.away);
        // `nciIn`: a note that is new comes down from above
        let appear = if n.fresh { SPRING.ease((now.saturating_sub(opened.max(n.born)) as f32 / 400.0).min(1.0)) } else { 1.0 };
        c.clipped(clip, |c| {
            c.faded(fade * appear, |c| {
                c.transformed(card.cx(), card.cy(), lerp(0.98, 1.0, appear), slide, -8.0 * (1.0 - appear), |c| {
                    let base = if n.unread { ROW_UNREAD } else { ROW_READ };
                    c.fill(card, 16.0, base.mix(ROW_HOT, over));
                    if let Some((id, k)) = snap.highlight.filter(|(id, _)| *id == it.id) {
                        let _ = id;
                        let a = if k < 0.2 { k / 0.2 } else if k < 0.6 { 1.0 } else { (1.0 - k) / 0.4 };
                        c.ring(card, 16.0, 1.5, accent().alpha(a));
                    }
                    let at = |r: Rect| r.shift(card.x, card.y);
                    if let (Some(a), Some(r)) = (n.avatar.as_ref(), p.avatar) {
                        avatar(c, a, at(r), 13.0);
                    }
                    // the title line: the dot, the title, the time (which gives way to the x on hover)
                    let time_alpha = tw.go(Hit::Item(it.id).key(), 2, f32::from(!hot_card), 120, EASE);
                    let time = ago_text(notify::ago(now, n.born));
                    let head = at(p.head);
                    title(c, n, head, p.time_w + 6.0, n.unread);
                    c.text(&time, Rect::new(head.x + head.w - p.time_w - 2.0, head.y, p.time_w + 2.0, head.h), SMALL, MUTED.fade(time_alpha), Align::End);
                    if let Some(b) = p.body {
                        body(c, n, (at(b.0), b.1));
                    }
                    if let (Some(path), Some(r)) = (n.image.as_deref(), p.image) {
                        picture(c, &self.pics, path, at(r));
                        video_badge(c, n, at(r));
                    }
                    if let (Some(rep), Some(r)) = (snap.reply.as_ref().filter(|r| r.id == it.id), p.reply) {
                        reply_field(c, tw, hov, it.id, n, &rep.text, (at(r.0), at(r.1)), false, self.reply_since.unwrap_or(now), now);
                    }
                    for (k, (a, r)) in p.acts.iter().enumerate() {
                        let first = shown_acts(n, Flavour::Centre).first().is_some_and(|(i, _)| i == a);
                        let _ = k;
                        act_button(c, tw, hov, Hit::Act(it.id, *a as u8), at(*r), &n.acts[*a].label, first, n.danger, false);
                    }
                    dismiss_button(c, tw, hov, it.id, at(p.dismiss), hot_card, false);
                });
            });
        });
    }
}

fn toggle_dnd() {
    let on = notify::with(|s| {
        let on = !s.dnd();
        s.set_dnd(on);
        on
    });
    notify::save_dnd(on);
}

/// A click on a note: it is read; if it has a source to open, that opens.
pub fn note_clicked(id: u64, now: u64, cx: &mut Cx) {
    let target = notify::with(|s| {
        s.mark_read(id);
        s.note(id).map(|n| n.target.clone())
    });
    if let Some(t) = target.filter(|t| *t != notify::Target::Nothing) {
        // the source has it now, as Windows removes a toast that was clicked
        cx.run(move || notify::open_target(&t));
        notify::with(|s| s.dismiss(id, now));
    }
}

pub fn act_clicked(id: u64, a: usize, now: u64, cx: &mut Cx) {
    let does = notify::with(|s| s.note(id).and_then(|n| n.acts.get(a).map(|a| a.does.clone())));
    let Some(does) = does else { return };
    match does {
        Do::Reply => notify::with(|s| s.start_reply(id)),
        Do::Read => notify::with(|s| s.mark_read(id)),
        d => {
            notify::with(|s| s.dismiss(id, now));
            cx.run(move || notify::perform(&d));
        }
    }
}

pub fn send_clicked(now: u64, cx: &mut Cx) {
    if let Some((id, text)) = notify::with(|s| s.send_reply(now)) {
        cx.run(move || notify::reply_sent(id, &text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 7 DIPs a character, wraps on the box's width.
    struct Fake;
    impl Measure for Fake {
        fn width(&mut self, s: &str, _: Font) -> f32 {
            s.chars().count() as f32 * 7.0
        }
        fn lines(&mut self, s: &str, _: Font, w: f32) -> usize {
            ((s.chars().count() as f32 * 7.0 / w).ceil() as usize).max(1)
        }
    }

    fn app() -> App {
        App { key: "a".into(), name: "App".into(), icon: AppIcon::Winrust }
    }

    #[test]
    fn a_centre_row_is_as_tall_as_the_mockups() {
        // padding 12 + title 20 + gap 3 + two lines of 18.125 + padding 12
        let n = Note::new(app(), "Title", "x".repeat(60));
        let p = parts(&n, Flavour::Centre, 360.0, false, 0, &mut Fake);
        assert!((p.h - (24.0 + 20.0 + 3.0 + 2.0 * LINE)).abs() < 1e-3, "{}", p.h);
        // a long text is cut at two lines
        let long = Note::new(app(), "T", "x".repeat(500));
        assert_eq!(parts(&long, Flavour::Centre, 360.0, false, 0, &mut Fake).body.map(|b| b.1), Some(2));
        // a picture leaves one line and adds 3 + 8 + 110
        let pic = Note::new(app(), "T", "x".repeat(500)).image("a.png");
        let p = parts(&pic, Flavour::Centre, 360.0, false, 0, &mut Fake);
        assert!((p.h - (24.0 + 20.0 + 3.0 + LINE + 3.0 + 8.0 + 110.0)).abs() < 1e-3);
        // buttons: 3 + 8 + 28
        let acts = Note::new(app(), "T", "b").acts(vec![Act { label: "Open".into(), does: Do::Open("x".into()) }, Act { label: "Show".into(), does: Do::Show("x".into()) }]);
        let p = parts(&acts, Flavour::Centre, 360.0, false, 0, &mut Fake);
        assert!((p.h - (24.0 + 20.0 + 3.0 + LINE + 3.0 + 8.0 + 28.0)).abs() < 1e-3);
        assert_eq!(p.acts.len(), 2);
        assert!(p.acts[1].1.x > p.acts[0].1.x + p.acts[0].1.w, "6 apart, side by side");
    }

    #[test]
    fn mark_as_read_is_a_pop_up_button_only_and_a_reply_field_takes_the_buttons_place() {
        let n = Note::new(app(), "T", "b").acts(vec![Act { label: "Reply".into(), does: Do::Reply }, Act { label: "Mark as read".into(), does: Do::Read }]);
        assert_eq!(parts(&n, Flavour::Centre, 360.0, false, 0, &mut Fake).acts.len(), 1);
        assert_eq!(parts(&n, Flavour::Pop, 368.0, false, 0, &mut Fake).acts.len(), 2);
        let p = parts(&n, Flavour::Centre, 360.0, true, 0, &mut Fake);
        assert!(p.acts.is_empty() && p.reply.is_some());
        let (f, s) = p.reply.unwrap();
        assert!(f.contains(s.cx(), s.cy()) && (f.h - 36.0).abs() < 1e-3 && (s.w - 28.0).abs() < 1e-3);
    }

    #[test]
    fn a_pop_up_is_laid_out_like_the_mockups() {
        // 12 + the app's line 20 + 10 + title 16 + 3 + three lines + 14
        let n = Note::new(app(), "Title", "x".repeat(900));
        let p = parts(&n, Flavour::Pop, 368.0, false, 0, &mut Fake);
        assert!((p.h - (12.0 + 20.0 + 10.0 + 16.0 + 3.0 + 3.0 * LINE + 14.0)).abs() < 1e-3, "{}", p.h);
        let with = Note::new(app(), "T", "b").avatar('A', 0, 0).image("a.png").acts(vec![Act { label: "Open".into(), does: Do::Read }]);
        let q = parts(&with, Flavour::Pop, 368.0, false, 0, &mut Fake);
        // the avatar (40) is taller than the text; the picture is 150 high 10 below; the buttons are 32 high 12 below
        assert!((q.h - (12.0 + 20.0 + 10.0 + 40.0 + 10.0 + 150.0 + 12.0 + 32.0 + 14.0)).abs() < 1e-3, "{}", q.h);
        assert_eq!(q.acts[0].1.w, 368.0 - 28.0);
    }

    #[test]
    fn hits_have_keys_that_do_not_collide() {
        let hs = [Hit::Dnd, Hit::ClearAll, Hit::DndOff, Hit::Undo, Hit::Stack(0), Hit::More(0), Hit::ClearGroup(0), Hit::Item(7), Hit::Dismiss(7), Hit::Act(7, 0), Hit::Act(7, 1), Hit::Send(7), Hit::Field(7), Hit::Item(8)];
        let mut keys: Vec<i32> = hs.iter().map(|h| h.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), hs.len());
    }

    #[test]
    fn the_centre_lays_out_groups_and_scrolls_inside_its_room() {
        notify::with(|s| *s = notify::Store::new());
        notify::with(|s| {
            for i in 0..12 {
                s.push(Note::new(app(), format!("n{i}"), "x".repeat(300)), 0);
            }
            s.toggle_stack("a"); // twelve of one app are a closed stack until it is opened
        });
        let mut c = Centre::new(400.0);
        let h = c.relayout(&mut Fake, 1000);
        assert!((h - 400.0).abs() < 1e-3, "twelve notes fill the room: {h}");
        assert!(c.lay.content > 400.0);
        c.wheel(false);
        c.relayout(&mut Fake, 1000);
        assert!((c.scroll - 60.0).abs() < 1e-3);
        for _ in 0..100 {
            c.wheel(false);
        }
        c.relayout(&mut Fake, 1000);
        assert!((c.scroll - (c.lay.content - c.lay.view.h)).abs() < 1e-3, "scrolled to the end and no further");
        // the list's controls answer only inside its viewport
        let first = c.lay.hits.iter().position(|e| e.scrolls).unwrap();
        let r = c.lay.hits[first].rect;
        let above = Rect::new(r.x, c.lay.view.y - 10.0, r.w, 5.0);
        assert!(c.hit(above.cx(), above.cy()).is_none_or(|i| !c.lay.hits[i as usize].scrolls));
        notify::with(|s| *s = notify::Store::new());
        let h = c.relayout(&mut Fake, 2000);
        assert!((h - (52.0 + 128.0)).abs() < 1e-3, "empty: the header and the picture: {h}");
        assert_eq!(c.scroll, 0.0);
    }

    #[test]
    fn a_long_app_name_leaves_room_for_the_pill_and_the_clear_button() {
        notify::with(|s| *s = notify::Store::new());
        let long = App { key: "l".into(), name: "A very long application name that goes on and on".repeat(3), icon: AppIcon::Winrust };
        notify::with(|s| {
            for t in ["one", "two", "three"] {
                s.push(Note::new(long.clone(), t, "b"), 0);
            }
        });
        let mut c = Centre::new(800.0);
        c.relayout(&mut Fake, 0);
        let g = &c.lay.groups[0];
        assert!(g.can_expand && g.pill.x + g.pill.w <= g.clear.x, "pill {:?}, clear {:?}", g.pill, g.clear);
        notify::with(|s| *s = notify::Store::new());
    }

    #[test]
    fn do_not_disturb_adds_its_strip_and_the_buttons_stand_where_the_mockup_has_them() {
        notify::with(|s| *s = notify::Store::new());
        let mut c = Centre::new(800.0);
        let empty = c.relayout(&mut Fake, 0);
        notify::with(|s| s.set_dnd(true));
        let dnd = c.relayout(&mut Fake, 10);
        assert!((dnd - empty - 48.0).abs() < 1e-3);
        let btn = c.lay.hits.iter().find(|e| e.hit == Hit::Dnd).unwrap().rect;
        assert_eq!((btn.x + btn.w, btn.w, btn.h), (W - 10.0, 32.0, 32.0), "32 square, 10 from the right with nothing beside it");
        notify::with(|s| {
            s.set_dnd(false);
            s.push(Note::new(app(), "t", "b"), 0);
        });
        c.relayout(&mut Fake, 20);
        let dnd = c.lay.hits.iter().find(|e| e.hit == Hit::Dnd).unwrap().rect;
        let clear = c.lay.hits.iter().find(|e| e.hit == Hit::ClearAll).unwrap().rect;
        assert!((clear.x - (dnd.x + dnd.w + 8.0)).abs() < 1e-3 && (clear.x + clear.w - (W - 10.0)).abs() < 1e-3, "Clear all is last, Do not disturb 8 before it");
        notify::with(|s| *s = notify::Store::new());
    }
}
