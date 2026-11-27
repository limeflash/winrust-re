//! The screenshot picker (Win+Shift+S) and the screen-recording overlay (Win+Shift+R) of the Obsidian mockup, without windows:
//! the selection model, the layout of the chrome and the painting, all in DIPs of one monitor (`Mon::scale` device pixels each).
//! `snip.rs` (the host) puts them into windows; `--render-panel shot|shot-window|shot-opts|shot-count|rec-pick|rec-live` into PNGs.
//!
//! The model is in device pixels of the virtual desktop, the chrome in DIPs: the mockup's numbers (toolbar 550 x 48 at the top
//! middle, handles 10 x 10, the action bar 40 high, ...) are used as they are.

use std::{cell::RefCell, collections::HashMap};

use winrust_common::style::{self, pal};

use super::anim::{self, EASE, SNAP, SPRING, Transition};
use super::gfx::{Align, Canvas, Font, Picture, Rect, Rgba};

// ---------------------------------------------------------------------------------------------------------------------
// geometry

/// A rectangle in device pixels of the virtual desktop: `l`, `t` inside, `r`, `b` outside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IRect {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl IRect {
    #[must_use]
    pub const fn new(l: i32, t: i32, r: i32, b: i32) -> Self {
        Self { l, t, r, b }
    }

    #[must_use]
    pub const fn w(self) -> i32 {
        self.r - self.l
    }

    #[must_use]
    pub const fn h(self) -> i32 {
        self.b - self.t
    }

    #[must_use]
    pub const fn contains(self, p: (i32, i32)) -> bool {
        p.0 >= self.l && p.0 < self.r && p.1 >= self.t && p.1 < self.b
    }

    #[must_use]
    pub fn intersect(self, o: Self) -> Option<Self> {
        let r = Self::new(self.l.max(o.l), self.t.max(o.t), self.r.min(o.r), self.b.min(o.b));
        (r.w() > 0 && r.h() > 0).then_some(r)
    }

    /// The rectangle between two corners, whichever way it was dragged.
    #[must_use]
    pub fn between(a: (i32, i32), b: (i32, i32)) -> Self {
        Self::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
    }

    /// Moved (not resized) to lie inside `m`, as far as it fits.
    #[must_use]
    pub fn moved_into(self, m: Self) -> Self {
        let dx = if self.w() >= m.w() { m.l - self.l } else { (m.l - self.l).max(0) + (m.r - self.r).min(0) };
        let dy = if self.h() >= m.h() { m.t - self.t } else { (m.t - self.t).max(0) + (m.b - self.b).min(0) };
        Self::new(self.l + dx, self.t + dy, self.r + dx, self.b + dy)
    }

    /// The size a recording takes: the video encoder wants even sides, so an odd side loses its last pixel.
    #[must_use]
    pub const fn even(self) -> Self {
        Self::new(self.l, self.t, self.r - (self.w() & 1), self.b - (self.h() & 1))
    }

    /// In DIPs of the monitor `m` (scale `k`), window-local.
    #[must_use]
    pub fn dip(self, m: IRect, k: f32) -> Rect {
        Rect::new((self.l - m.l) as f32 / k, (self.t - m.t) as f32 / k, self.w() as f32 / k, self.h() as f32 / k)
    }
}

/// A monitor: where it is on the virtual desktop (device pixels) and how many device pixels a DIP is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mon {
    pub rect: IRect,
    pub scale: f32,
}

/// The monitor a point is on (the nearest one for a point outside all of them).
#[must_use]
pub fn mon_at(mons: &[Mon], p: (i32, i32)) -> usize {
    let dist = |m: &Mon| {
        let dx = (m.rect.l - p.0).max(0).max(p.0 - (m.rect.r - 1));
        let dy = (m.rect.t - p.1).max(0).max(p.1 - (m.rect.b - 1));
        i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
    };
    (0..mons.len()).min_by_key(|&i| dist(&mons[i])).unwrap_or(0)
}

/// A handle of the selection, or the selection itself (moving it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Grip {
    Move,
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

/// The eight handles with their place on the selection (0..1 across, 0..1 down), in the mockup's order.
pub const HANDLES: [(Grip, f32, f32); 8] =
    [(Grip::Nw, 0.0, 0.0), (Grip::N, 0.5, 0.0), (Grip::Ne, 1.0, 0.0), (Grip::E, 1.0, 0.5), (Grip::Se, 1.0, 1.0), (Grip::S, 0.5, 1.0), (Grip::Sw, 0.0, 1.0), (Grip::W, 0.0, 0.5)];

/// A new selection dragged from `from` to `to`: both ends kept inside the monitor.
#[must_use]
pub fn drag_new(from: (i32, i32), to: (i32, i32), mon: IRect) -> IRect {
    let clamp = |p: (i32, i32)| (p.0.clamp(mon.l, mon.r), p.1.clamp(mon.t, mon.b));
    IRect::between(clamp(from), clamp(to))
}

/// `orig` dragged by `(dx, dy)` at `grip`: moved (kept inside the monitor) or resized (the edges stop at the monitor and may
/// cross each other, which flips the rectangle, like the mockup).
#[must_use]
pub fn drag_grip(orig: IRect, grip: Grip, (dx, dy): (i32, i32), mon: IRect) -> IRect {
    if grip == Grip::Move {
        return IRect::new(orig.l + dx, orig.t + dy, orig.r + dx, orig.b + dy).moved_into(mon);
    }
    let name = format!("{grip:?}").to_lowercase();
    let (mut l, mut t, mut r, mut b) = (orig.l, orig.t, orig.r, orig.b);
    if name.contains('w') {
        l += dx;
    }
    if name.contains('e') {
        r += dx;
    }
    if name.contains('n') {
        t += dy;
    }
    if name.contains('s') {
        b += dy;
    }
    drag_new((l, t), (r, b), mon)
}

/// The topmost window that holds the point.
#[must_use]
pub fn window_at(wins: &[Win], p: (i32, i32)) -> Option<usize> {
    wins.iter().position(|w| w.rect.contains(p))
}

/// A window the picker can take (top of the stack first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Win {
    pub rect: IRect,
    pub title: String,
    /// The window's handle (0 for the demo's); the strip's card of the same window lights up with it.
    pub hwnd: isize,
}

// ---------------------------------------------------------------------------------------------------------------------
// the strip of windows (`shpick`): Window mode's "Or pick a window"

/// Where a window of the strip is; the tag on its card says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    /// On this screen with (almost) nothing over it: no tag.
    Visible,
    /// On this screen, mostly hidden behind other windows.
    Covered,
    Minimized,
    /// On another monitor (its index).
    Monitor(usize),
}

impl Where {
    /// The order of the strip: what is in sight, then what is covered, then what has to be brought here first.
    const fn rank(self) -> (u8, usize) {
        match self {
            Self::Visible => (0, 0),
            Self::Covered => (1, 0),
            Self::Minimized => (2, 0),
            Self::Monitor(i) => (3, i),
        }
    }

    /// The tag of the card (`None`: it has none).
    #[must_use]
    pub fn tag(self) -> Option<String> {
        match self {
            Self::Visible => None,
            Self::Covered => Some(t!("Закрыто", "Covered", ja: "隠れています", pt: "Coberta", zh: "被遮挡").into()),
            Self::Minimized => Some(t!("Свёрнуто", "Minimized", ja: "最小化", pt: "Minimizada", zh: "已最小化").into()),
            Self::Monitor(i) => Some(winrust_common::tf!("Монитор {}", "Monitor {}", ja: "モニター {}", pt: "Monitor {}", zh: "显示器 {}", i + 1)),
        }
    }
}

/// A window as the enumeration finds it; the list is topmost first.
#[derive(Clone, Debug)]
pub struct RawWin {
    pub hwnd: isize,
    pub title: String,
    /// Its frame on the virtual desktop; for a minimized window where it would come back to.
    pub rect: IRect,
    pub minimized: bool,
}

/// A card of the strip.
#[derive(Clone, Debug)]
pub struct PickWin {
    pub hwnd: isize,
    pub title: String,
    pub rect: IRect,
    pub at: Where,
    /// The app's icon (an `HICON`, 0 until it is loaded) and the colour the card's gradient comes from (`None`: the accent).
    pub icon: isize,
    pub tint: Option<Rgba>,
}

/// A window with less than this much of it uncovered counts as covered.
const COVERED_BELOW: f32 = 0.85;

/// What is left of `r` when `cut` is taken out of it, as up to four rectangles.
fn minus(r: IRect, cut: IRect) -> Vec<IRect> {
    let Some(c) = r.intersect(cut) else { return vec![r] };
    let mut v = Vec::with_capacity(4);
    if c.t > r.t {
        v.push(IRect::new(r.l, r.t, r.r, c.t));
    }
    if c.b < r.b {
        v.push(IRect::new(r.l, c.b, r.r, r.b));
    }
    if c.l > r.l {
        v.push(IRect::new(r.l, c.t, c.l, c.b));
    }
    if c.r < r.r {
        v.push(IRect::new(c.r, c.t, r.r, c.b));
    }
    v
}

/// The share (0..1) of `r` that none of `above` covers.
fn uncovered(r: IRect, above: &[IRect]) -> f32 {
    let mut left = vec![r];
    for &a in above {
        left = left.iter().flat_map(|&p| minus(p, a)).collect();
    }
    let area = |r: &IRect| i64::from(r.w()) * i64::from(r.h());
    left.iter().map(area).sum::<i64>() as f32 / area(&r).max(1) as f32
}

/// The strip's cards from the windows of the desktop (topmost first), seen from monitor `active`: in sight first, then covered,
/// then minimized, then those of other monitors (each group in z-order, the other monitors one after the other).
#[must_use]
pub fn strip_of(raw: &[RawWin], mons: &[Mon], active: usize) -> Vec<PickWin> {
    let mut above: Vec<IRect> = Vec::new();
    let mut out: Vec<PickWin> = Vec::new();
    for w in raw {
        let mon = mon_at(mons, ((w.rect.l + w.rect.r) / 2, (w.rect.t + w.rect.b) / 2));
        let at = if w.minimized {
            Where::Minimized
        } else if mon != active {
            Where::Monitor(mon)
        } else {
            match mons.get(mon).and_then(|m| w.rect.intersect(m.rect)) {
                Some(on) if uncovered(on, &above) >= COVERED_BELOW => Where::Visible,
                _ => Where::Covered,
            }
        };
        if !w.minimized {
            above.push(w.rect);
        }
        out.push(PickWin { hwnd: w.hwnd, title: w.title.clone(), rect: w.rect, at, icon: 0, tint: None });
    }
    out.sort_by_key(|w| w.at.rank());
    out
}

/// A window's picture as its card shows it.
#[derive(Clone, Debug)]
pub struct Thumb {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

/// The card's picture box, DIPs.
const THUMB_W: f32 = 140.0;
const THUMB_H: f32 = 84.0;

/// A picture of a window (`sw` x `sh` opaque pixels) cut for a card: scaled to cover the 140 x 84 box, kept to its top left corner
/// (`object-fit: cover; object-position: top left`), at `scale` device pixels per DIP. Averaged: the window is several times the card.
#[must_use]
pub fn thumb_of(src: &[u32], sw: i32, sh: i32, scale: f32) -> Option<Thumb> {
    if sw <= 0 || sh <= 0 || src.len() < (sw as usize) * (sh as usize) {
        return None;
    }
    let (dw, dh) = (((THUMB_W * scale).round() as i32).max(1), ((THUMB_H * scale).round() as i32).max(1));
    let k = (dw as f32 / sw as f32).max(dh as f32 / sh as f32); // destination pixels per source pixel
    let mut px = Vec::with_capacity((dw * dh) as usize);
    for y in 0..dh {
        let y0 = ((y as f32 / k) as i32).min(sh - 1);
        let y1 = (((y + 1) as f32 / k).ceil() as i32).clamp(y0 + 1, sh);
        for x in 0..dw {
            let x0 = ((x as f32 / k) as i32).min(sw - 1);
            let x1 = (((x + 1) as f32 / k).ceil() as i32).clamp(x0 + 1, sw);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1 {
                for &p in &src[(sy * sw + x0) as usize..(sy * sw + x1) as usize] {
                    r += (p >> 16) & 255;
                    g += (p >> 8) & 255;
                    b += p & 255;
                    n += 1;
                }
            }
            px.push(0xff00_0000 | ((r / n) << 16) | ((g / n) << 8) | (b / n));
        }
    }
    Some(Thumb { w: dw, h: dh, px })
}

// ---------------------------------------------------------------------------------------------------------------------
// what is chosen and remembered

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Shot,
    Rec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Region,
    Window,
    Full,
}

/// The choices of the popover and the last mode and kind; they live in `%APPDATA%\topbar-snip.txt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    pub kind: Kind,
    pub mode: Mode,
    pub clip: bool,
    pub save: bool,
    pub notify: bool,
    /// Seconds before a screenshot is taken: 0, 3 or 5.
    pub delay: u8,
    pub mic: bool,
    /// Seconds before a recording starts: 0, 3 or 5.
    pub countdown: u8,
}

pub const DELAYS: [u8; 3] = [0, 3, 5];

impl Default for Opts {
    fn default() -> Self {
        Self { kind: Kind::Shot, mode: Mode::Region, clip: true, save: true, notify: true, delay: 0, mic: true, countdown: 3 }
    }
}

impl Opts {
    #[must_use]
    pub fn to_text(self) -> String {
        let b = |v: bool| u8::from(v);
        format!(
            "kind={}\nmode={}\nclip={}\nsave={}\nnotify={}\ndelay={}\nmic={}\ncountdown={}\n",
            if self.kind == Kind::Rec { "rec" } else { "shot" },
            match self.mode {
                Mode::Region => "region",
                Mode::Window => "window",
                Mode::Full => "full",
            },
            b(self.clip),
            b(self.save),
            b(self.notify),
            self.delay,
            b(self.mic),
            self.countdown
        )
    }

    /// What `to_text` wrote; a line that is missing or makes no sense leaves the default.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.trim().split_once('=') else { continue };
            let flag = |d: bool| match v {
                "1" => true,
                "0" => false,
                _ => d,
            };
            let secs = |d: u8| v.parse::<u8>().ok().filter(|s| DELAYS.contains(s)).unwrap_or(d);
            match k {
                "kind" => o.kind = if v == "rec" { Kind::Rec } else { Kind::Shot },
                "mode" => o.mode = match v { "window" => Mode::Window, "full" => Mode::Full, _ => Mode::Region },
                "clip" => o.clip = flag(o.clip),
                "save" => o.save = flag(o.save),
                "notify" => o.notify = flag(o.notify),
                "delay" => o.delay = secs(o.delay),
                "mic" => o.mic = flag(o.mic),
                "countdown" => o.countdown = secs(o.countdown),
                _ => {}
            }
        }
        o
    }

    /// The seconds the current kind waits before it acts.
    #[must_use]
    pub const fn wait(&self) -> u8 {
        if matches!(self.kind, Kind::Rec) { self.countdown } else { self.delay }
    }

    /// The switches of the popover for the current kind: label, sub label, state.
    #[must_use]
    pub fn switches(&self) -> Vec<(String, String, bool)> {
        if self.kind == Kind::Rec {
            vec![(
                t!("Микрофон", "Microphone", ja: "マイク", pt: "Microfone", zh: "麦克风").into(),
                t!("Записывает ваш голос", "Records your voice", ja: "音声を録音します", pt: "Grava sua voz", zh: "录制你的声音").into(),
                self.mic,
            )]
        } else {
            vec![
                (
                    t!("Копировать в буфер", "Copy to clipboard", ja: "クリップボードにコピー", pt: "Copiar para a área de transferência", zh: "复制到剪贴板").into(),
                    t!("Сразу вставляйте куда угодно", "Paste it anywhere right away", ja: "すぐにどこにでも貼り付けられます", pt: "Cole em qualquer lugar na hora", zh: "可立即粘贴到任何地方").into(),
                    self.clip,
                ),
                (
                    t!("Сохранять в «Снимки экрана»", "Save to Screenshots", ja: "スクリーンショットに保存", pt: "Salvar em Capturas de tela", zh: "保存到截图文件夹").into(),
                    t!("Изображения › Снимки экрана, PNG", "Pictures › Screenshots, as PNG", ja: "ピクチャ › スクリーンショット、PNG 形式", pt: "Imagens › Capturas de tela, em PNG", zh: "图片 › 屏幕截图，PNG 格式").into(),
                    self.save,
                ),
                (
                    t!("Показывать уведомление", "Show a notification", ja: "通知を表示", pt: "Mostrar uma notificação", zh: "显示通知").into(),
                    t!("С превью и быстрыми действиями", "With a preview and quick actions", ja: "プレビューとクイック操作つき", pt: "Com pré-visualização e ações rápidas", zh: "带预览和快捷操作").into(),
                    self.notify,
                ),
            ]
        }
    }

    pub fn toggle(&mut self, i: usize) {
        match (self.kind, i) {
            (Kind::Rec, 0) => self.mic = !self.mic,
            (Kind::Shot, 0) => self.clip = !self.clip,
            (Kind::Shot, 1) => self.save = !self.save,
            (Kind::Shot, 2) => self.notify = !self.notify,
            _ => {}
        }
    }

    pub fn set_wait(&mut self, secs: u8) {
        if self.kind == Kind::Rec { self.countdown = secs } else { self.delay = secs }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the controls

/// What on the chrome can be pointed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctl {
    Kind(Kind),
    Mode(Mode),
    Gear,
    Close,
    /// The `x` of the action bar.
    Cancel,
    Copy,
    /// Done (a screenshot) or Record.
    Go,
    Switch(usize),
    Wait(usize),
    /// The strip of windows as a whole (it takes the press and does nothing), one of its cards (an index into `Snip::strip`),
    /// and the "Record the full screen" button of its empty state.
    Strip,
    Pick(usize),
    ToFull,
}

/// What a pointer press landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Ctl(Ctl),
    /// A part of the chrome that is no button: it takes the press and does nothing.
    Chrome,
    /// Open screen: the picker's own business.
    Canvas,
}

/// What the host has to do after an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    None,
    Cancel,
    Capture { copy_only: bool },
    /// A card of the strip was picked (an index into `Snip::strip`): the host brings the window and takes it.
    Window(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Esc,
    Enter,
    Char(char),
}

/// Where the chrome of one monitor stands, DIPs of that monitor.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub size: (f32, f32),
    pub tool: Option<Rect>,
    pub pop: Option<Rect>,
    pub bar: Option<Rect>,
    pub items: Vec<(Ctl, Rect)>,
    /// The size label and, in the action bar's place, where the coordinates tip goes.
    pub label: Option<Rect>,
    /// The strip of windows, the part of it the cards scroll in, each card (its index into `Snip::strip`) where it stands now, and
    /// how far they can scroll.
    pub strip: Option<Rect>,
    pub view: Option<Rect>,
    pub cards: Vec<(usize, Rect)>,
    pub scroll_max: f32,
}

impl Layout {
    #[must_use]
    pub fn rect_of(&self, c: Ctl) -> Option<Rect> {
        self.items.iter().find(|(k, _)| *k == c).map(|(_, r)| *r)
    }

    #[must_use]
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        if let Some((c, _)) = self.items.iter().rev().find(|(_, r)| r.contains(x, y)) {
            return Hit::Ctl(*c);
        }
        if [self.pop, self.tool, self.bar].into_iter().flatten().any(|r| r.contains(x, y)) {
            return Hit::Chrome;
        }
        Hit::Canvas
    }
}

pub const TOOL_W: f32 = 550.0;
pub const TOOL_H: f32 = 48.0;
pub const TOOL_TOP: f32 = 16.0;
const POP_W: f32 = 300.0;
const KINDS: [Kind; 2] = [Kind::Shot, Kind::Rec];
const MODES: [Mode; 3] = [Mode::Region, Mode::Window, Mode::Full];

const F_MODE: Font = Font::new(12.5, 500);
const F_GO: Font = Font::new(12.5, 600);
const F_KBD: Font = Font::new(10.5, 600);
const F_LABEL: Font = Font::new(12.0, 600).tabular();
const F_COORD: Font = Font::new(11.5, 600).tabular();
const F_HINT: Font = Font::new(12.0, 400);
const F_TITLE: Font = Font::new(11.0, 600).tracking(0.05);
const F_SUB: Font = Font::new(11.5, 400);
const F_SEG: Font = Font::new(12.0, 500);
const F_ATK: Font = Font::new(11.0, 500);
const F_FOOT: Font = Font::new(12.0, 400);
const F_HEAD: Font = Font::new(12.0, 600);
const F_CARD: Font = Font::new(12.0, 500);
const F_TAG: Font = Font::new(11.0, 500);
const F_BODY: Font = Font::new(13.0, 400);
const F_BTN: Font = Font::new(13.0, 500);
/// The strip (`.shpick`): 20 from the bottom, 10 / 12 / 12 of padding, a 32 high head, cards of 150 x 117 with 8 between, 1160 at most.
const STRIP_BOTTOM: f32 = 20.0;
const STRIP_MAX: f32 = 1160.0;
const STRIP_HEAD: f32 = 32.0;
const CARD_W: f32 = 150.0;
const CARD_H: f32 = 117.0;
const CARD_GAP: f32 = 8.0;
/// The height the scrollbar takes under the cards when they do not all fit.
const SCROLL_H: f32 = 10.0;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const DIM_INK: Rgba = Rgba::css(163, 169, 184, 1.0);
const SOFT_INK: Rgba = Rgba::css(201, 205, 216, 1.0);
const MUTED: Rgba = Rgba::css(138, 144, 162, 1.0);
const TITLE_INK: Rgba = Rgba::css(111, 116, 134, 1.0);
const SCRIM: Rgba = Rgba::css(5, 7, 12, 0.5);
const PLATE: Rgba = Rgba::css(12, 13, 18, 0.9);
const RED: Rgba = Rgba::hex(0xe5484d);
const AMBER: Rgba = Rgba::hex(0xffb877);

// icons on the 24 grid, from the mockup
const I_SHOT: &str = "M4 8h3l2-3h6l2 3h3v11H4zM12 16.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z";
const I_REC: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z";
const I_REGION: &str = "M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4";
const I_WINDOW: &str = "M4 5h16v14H4zM4 9h16";
const I_FULL: &str = "M4 5h16v12H4zM9 21h6M12 17v4";
const I_X: &str = "M7 7l10 10M17 7L7 17";
const I_COPY: &str = "M8 8h11v11H8zM5 15V5h10";
const I_CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const I_PLAY: &str = "M8 5v14l11-7z";

pub(crate) fn accent() -> Rgba {
    Rgba::colorref(pal().accent)
}

fn mode_label(m: Mode) -> &'static str {
    match m {
        Mode::Region => t!("Область", "Region", ja: "範囲", pt: "Região", zh: "区域"),
        Mode::Window => t!("Окно", "Window", ja: "ウィンドウ", pt: "Janela", zh: "窗口"),
        Mode::Full => t!("Весь экран", "Full screen", ja: "全画面", pt: "Tela cheia", zh: "全屏"),
    }
}

fn hint_text(kind: Kind, mode: Mode) -> &'static str {
    match (kind, mode) {
        (Kind::Shot, Mode::Region) => t!("Выделите область мышью · Esc — отмена", "Drag to select an area · Esc to cancel", ja: "ドラッグして範囲を選択 · Esc でキャンセル", pt: "Arraste para selecionar uma área · Esc para cancelar", zh: "拖动选择区域 · Esc 取消"),
        (Kind::Shot, Mode::Window) => t!("Щёлкните окно, чтобы снять его", "Click a window to capture it", ja: "ウィンドウをクリックして撮影", pt: "Clique em uma janela para capturá-la", zh: "点击窗口进行截图"),
        (Kind::Shot, Mode::Full) => t!("Щёлкните или нажмите Enter, чтобы снять экран", "Click or press Enter to capture the screen", ja: "クリックまたは Enter で画面を撮影", pt: "Clique ou pressione Enter para capturar a tela", zh: "点击或按 Enter 截取屏幕"),
        (Kind::Rec, Mode::Region) => t!("Выделите область для записи · Esc — отмена", "Drag to select an area to record · Esc to cancel", ja: "ドラッグして録画する範囲を選択 · Esc でキャンセル", pt: "Arraste para selecionar a área a gravar · Esc para cancelar", zh: "拖动选择录制区域 · Esc 取消"),
        (Kind::Rec, Mode::Window) => t!("Щёлкните окно, чтобы записать его", "Click a window to record it", ja: "ウィンドウをクリックして録画", pt: "Clique em uma janela para gravá-la", zh: "点击窗口开始录制"),
        (Kind::Rec, Mode::Full) => t!("Щёлкните или нажмите Enter, чтобы записать весь экран", "Click or press Enter to record the whole screen", ja: "クリックまたは Enter で画面全体を録画", pt: "Clique ou pressione Enter para gravar a tela inteira", zh: "点击或按 Enter 录制整个屏幕"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the picker

/// A value that follows a target with a CSS transition, by what it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Ani {
    Hover(Ctl),
    /// 1 when a toolbar button is the chosen one (its ink is white).
    Tint(Ctl),
    Switch(usize),
    KindX,
    ModeX,
    /// The highlight of Window mode gliding between windows (left, top, width, height in device pixels), and its radius.
    WinL,
    WinT,
    WinW,
    WinH,
    WinR,
    /// 1 while the window of a strip card is the one pointed at (on the card or on the screen), and while the card is pressed.
    Lit(usize),
    Press(usize),
}

struct Drag {
    /// `None`: a new selection.
    grip: Option<Grip>,
    from: (i32, i32),
    orig: IRect,
}

/// The state of one picker, shared by all of its windows.
pub struct Snip {
    pub mons: Vec<Mon>,
    pub wins: Vec<Win>,
    pub opts: Opts,
    pub sel: Option<IRect>,
    pub cur: Option<(i32, i32)>,
    pub opts_open: bool,
    /// The monitor the toolbar is on.
    pub active: usize,
    hover: Option<usize>,
    hot: Option<Ctl>,
    drag: Option<Drag>,
    ani: HashMap<Ani, Transition>,
    opened: u64,
    tool_at: u64,
    bar_at: Option<u64>,
    pop_at: u64,
    flash: Option<u64>,
    closing: Option<u64>,
    /// A countdown is running over the picker (a recording about to start): the action bar goes away, as in the mockup.
    pub counting: bool,
    pub dirty: bool,
    /// The strip of windows (Window mode), the pictures the host made of them, and those pictures on this device.
    pub strip: Vec<PickWin>,
    thumbs: HashMap<isize, Thumb>,
    pics: RefCell<HashMap<isize, Picture>>,
    /// The card the pointer is on, how far the cards are scrolled (DIPs), when the strip came up, whether it is up, and whether the
    /// pointer is on it (the screen under it is then not pointed at).
    strip_hover: Option<usize>,
    scroll: f32,
    strip_at: u64,
    strip_up: bool,
    on_strip: bool,
}

fn dur(ms: u32) -> f32 {
    anim::timing(style::anim(), ms, 0).0 as f32
}

/// CSS `animation` progress at `now` for one that began at `at` and lasts `ms` (0..1; an animation mode of Off finishes at once).
fn prog(at: u64, now: u64, ms: u32) -> f32 {
    let d = dur(ms);
    if d <= 0.0 { 1.0 } else { (now.saturating_sub(at) as f32 / d).clamp(0.0, 1.0) }
}

impl Snip {
    #[must_use]
    pub fn new(mons: Vec<Mon>, wins: Vec<Win>, opts: Opts, active: usize, now: u64) -> Self {
        let mut s = Self {
            mons, wins, opts, sel: None, cur: None, opts_open: false, active, hover: None, hot: None, drag: None, ani: HashMap::new(),
            opened: now, tool_at: now, bar_at: None, pop_at: now, flash: None, closing: None, counting: false, dirty: true,
            strip: Vec::new(), thumbs: HashMap::new(), pics: RefCell::new(HashMap::new()), strip_hover: None, scroll: 0.0, strip_at: now, strip_up: false, on_strip: false,
        };
        if opts.mode == Mode::Full {
            s.sel = s.mons.get(active).map(|m| m.rect);
            s.bar_at = Some(now);
        }
        s.snap_toolbar();
        s
    }

    fn snap_toolbar(&mut self) {
        let (k, m) = (self.opts.kind, self.opts.mode);
        self.ani.insert(Ani::KindX, Transition::new(if k == Kind::Rec { 40.0 } else { 0.0 }));
        self.ani.insert(Ani::ModeX, Transition::new(MODES.iter().position(|&x| x == m).unwrap_or(0) as f32 * 112.0));
        for kk in KINDS {
            self.ani.insert(Ani::Tint(Ctl::Kind(kk)), Transition::new(f32::from(kk == k)));
        }
        for mm in MODES {
            self.ani.insert(Ani::Tint(Ctl::Mode(mm)), Transition::new(f32::from(mm == m)));
        }
    }

    fn go(&mut self, key: Ani, to: f32, now: u64, ms: u32, ease: anim::CubicBezier) {
        let (d, _) = anim::timing(style::anim(), ms, 0);
        self.ani.entry(key).or_insert_with(|| Transition::new(to)).set(to, now, d, 0, ease);
        self.dirty = true;
    }

    fn val(&self, key: Ani, now: u64) -> f32 {
        self.ani.get(&key).map_or(0.0, |t| t.value(now))
    }

    #[must_use]
    pub fn ready(&self) -> bool {
        self.sel.is_some() && self.drag.is_none()
    }

    /// The region that would be taken now.
    #[must_use]
    pub fn target(&self) -> Option<IRect> {
        self.sel
    }

    #[must_use]
    pub fn mon_rect(&self, i: usize) -> IRect {
        self.mons.get(i).map_or_else(IRect::default, |m| m.rect)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the strip of windows

    /// The strip's cards (set once the windows are listed; the host fills in their icons and pictures as it gets them).
    pub fn set_strip(&mut self, strip: Vec<PickWin>, now: u64) {
        self.strip = strip;
        self.thumbs.clear();
        self.pics.borrow_mut().clear();
        self.scroll = 0.0;
        self.sync_strip(now);
    }

    /// A card's picture arrived.
    pub fn set_thumb(&mut self, hwnd: isize, t: Thumb) {
        self.pics.borrow_mut().remove(&hwnd);
        self.thumbs.insert(hwnd, t);
        self.dirty = true;
    }

    /// A card's icon (an `HICON`) and gradient colour arrived.
    pub fn set_icon(&mut self, hwnd: isize, icon: isize, tint: Option<Rgba>) {
        if let Some(w) = self.strip.iter_mut().find(|w| w.hwnd == hwnd) {
            (w.icon, w.tint) = (icon, tint);
            self.dirty = true;
        }
    }

    /// The cards the strip shows: a recording only takes what is on this screen.
    fn shown(&self) -> Vec<usize> {
        (0..self.strip.len()).filter(|&i| self.opts.kind == Kind::Shot || !matches!(self.strip[i].at, Where::Monitor(_))).collect()
    }

    /// The strip is up: Window mode with nothing chosen yet, the popover and the countdown out of the way. A recording that has no
    /// window to take still shows it (its empty state offers the whole screen); a screenshot with none has nothing to offer.
    #[must_use]
    pub fn strip_on(&self) -> bool {
        self.opts.mode == Mode::Window && self.sel.is_none() && !self.counting && !self.opts_open && (self.opts.kind == Kind::Rec || !self.strip.is_empty())
    }

    /// The strip comes up with its slide each time it appears (`shpickIn`).
    fn sync_strip(&mut self, now: u64) {
        let up = self.strip_on();
        if up && !self.strip_up {
            self.strip_at = now;
        }
        self.strip_up = up;
        if !up {
            self.strip_hover = None;
            self.on_strip = false;
        }
        self.sync_lit(now);
    }

    /// The cards of the windows that are pointed at (on their card or on the screen) light up (`.14s`).
    fn sync_lit(&mut self, now: u64) {
        let hw = self.hover.and_then(|h| self.wins.get(h)).map_or(0, |w| w.hwnd);
        let (d, _) = anim::timing(style::anim(), 140, 0);
        for i in 0..self.strip.len() {
            let on = self.strip_hover == Some(i) || (hw != 0 && self.strip[i].hwnd == hw);
            self.ani.entry(Ani::Lit(i)).or_insert_with(|| Transition::new(0.0)).set(f32::from(on), now, d, 0, EASE);
        }
        self.dirty = true;
    }

    /// The strip's size: the box, all the cards in a row, and the part of it the cards show in.
    fn strip_dims(n: usize, screen_w: f32) -> (f32, f32, f32) {
        let content = n as f32 * CARD_W + n.saturating_sub(1) as f32 * CARD_GAP;
        let w = (24.0 + content).min(STRIP_MAX).min(screen_w - 24.0).max(24.0);
        (w, content, w - 24.0)
    }

    /// The pointer is on the strip (the wheel moves its cards).
    #[must_use]
    pub const fn over_strip(&self) -> bool {
        self.on_strip
    }

    /// A card was picked and its window is the selection, as if it had been clicked on the screen.
    pub fn choose(&mut self, r: IRect, now: u64) {
        self.sel = Some(r);
        self.hover = None;
        self.sync_strip(now);
    }

    /// The wheel over the strip moves the cards sideways.
    pub fn scroll_by(&mut self, dx: f32) {
        let Some(m) = self.mons.get(self.active) else { return };
        let (_, content, view) = Self::strip_dims(self.shown().len(), m.rect.w() as f32 / m.scale);
        self.scroll = (self.scroll + dx).clamp(0.0, (content - view).max(0.0));
        self.dirty = true;
    }

    /// Whether the pointer is over a selection that can be dragged about (a region).
    #[must_use]
    pub fn editable(&self) -> bool {
        self.sel.is_some() && self.opts.mode == Mode::Region && self.drag.is_none()
    }

    pub fn set_kind(&mut self, k: Kind, now: u64) {
        if self.opts.kind == k {
            return;
        }
        self.opts.kind = k;
        self.go(Ani::KindX, if k == Kind::Rec { 40.0 } else { 0.0 }, now, 320, SPRING);
        for kk in KINDS {
            self.go(Ani::Tint(Ctl::Kind(kk)), f32::from(kk == k), now, 160, EASE);
        }
        if self.opts_open {
            self.pop_at = now; // the popover shows the other kind's switches
        }
        if self.ready() {
            self.bar_at = Some(now);
        }
        self.scroll = 0.0;
        self.sync_strip(now);
    }

    pub fn set_mode(&mut self, m: Mode, now: u64) {
        self.opts.mode = m;
        self.sel = if m == Mode::Full { Some(self.mon_rect(self.active)) } else { None };
        self.hover = None;
        self.drag = None;
        self.bar_at = self.sel.map(|_| now);
        self.go(Ani::ModeX, MODES.iter().position(|&x| x == m).unwrap_or(0) as f32 * 112.0, now, 320, SPRING);
        for mm in MODES {
            self.go(Ani::Tint(Ctl::Mode(mm)), f32::from(mm == m), now, 160, EASE);
        }
        self.sync_strip(now);
    }

    /// The window under `p` as the highlight shows it: its frame, kept to the monitor the pointer is on.
    fn win_rect(&self, i: usize, p: (i32, i32)) -> Option<IRect> {
        self.wins.get(i)?.rect.intersect(self.mon_rect(mon_at(&self.mons, p)))
    }

    /// Where the highlight of Window mode is going, in device pixels.
    fn hover_rect(&self) -> Option<IRect> {
        if let Some(w) = self.strip_hover.and_then(|i| self.strip.get(i)) {
            // a card is pointed at: its window is outlined where it is (on its own monitor), whatever covers it
            return w.rect.intersect(self.mon_rect(mon_at(&self.mons, ((w.rect.l + w.rect.r) / 2, (w.rect.t + w.rect.b) / 2))));
        }
        let (i, p) = (self.hover?, self.cur?);
        self.win_rect(i, p)
    }

    pub fn move_to(&mut self, p: (i32, i32), now: u64) {
        if self.cur == Some(p) {
            return;
        }
        self.cur = Some(p);
        self.dirty = true;
        if let Some(d) = &self.drag {
            let mon = self.mon_rect(mon_at(&self.mons, d.from));
            self.sel = Some(match d.grip {
                None => drag_new(d.from, p, mon),
                Some(g) => drag_grip(d.orig, g, (p.0 - d.from.0, p.1 - d.from.1), mon),
            });
            return;
        }
        match self.opts.mode {
            Mode::Window if self.on_strip => {}
            Mode::Window => {
                let h = window_at(&self.wins, p).filter(|&i| self.win_rect(i, p).is_some());
                if h != self.hover || h.is_some() {
                    let changed = h != self.hover;
                    self.hover = h;
                    if let Some(r) = self.hover_rect() {
                        self.glide(r, now);
                    }
                    if changed {
                        self.sync_lit(now);
                    }
                }
            }
            Mode::Full => self.sel = Some(self.mon_rect(mon_at(&self.mons, p))),
            Mode::Region => {}
        }
    }

    /// The highlight moves to `r` (`.shsel.anim`: left, top, width, height over .32 s, the first sight does not glide).
    fn glide(&mut self, r: IRect, now: u64) {
        let had = self.ani.contains_key(&Ani::WinL);
        let pairs = [(Ani::WinL, r.l), (Ani::WinT, r.t), (Ani::WinW, r.w()), (Ani::WinH, r.h())];
        for (k, v) in pairs {
            if had {
                self.go(k, v as f32, now, 320, SPRING);
            } else {
                self.ani.insert(k, Transition::new(v as f32));
            }
        }
        self.dirty = true;
    }

    /// The window highlight that is showing: `None` when no window is under the pointer.
    fn showing(&self, now: u64) -> Option<IRect> {
        if let Some(s) = self.sel {
            return Some(s);
        }
        if self.opts.mode != Mode::Window || (self.hover.is_none() && self.strip_hover.is_none()) {
            return None;
        }
        let v = |k| self.val(k, now);
        let (l, t, w, h) = (v(Ani::WinL), v(Ani::WinT), v(Ani::WinW), v(Ani::WinH));
        Some(IRect::new(l.round() as i32, t.round() as i32, (l + w).round() as i32, (t + h).round() as i32))
    }

    pub fn press(&mut self, p: (i32, i32), hit: Hit, now: u64) -> Act {
        self.cur = Some(p);
        self.dirty = true;
        match hit {
            Hit::Ctl(c) => {
                if let Ctl::Pick(i) = c {
                    self.go(Ani::Press(i), 1.0, now, 140, SPRING);
                }
                return self.click(c, now);
            }
            Hit::Chrome => return Act::None,
            Hit::Canvas => {}
        }
        if self.opts_open {
            self.opts_open = false;
            self.sync_strip(now);
            return Act::None;
        }
        let mon = mon_at(&self.mons, p);
        match self.opts.mode {
            Mode::Full => {
                self.sel = Some(self.mon_rect(mon));
                return Act::Capture { copy_only: false };
            }
            Mode::Window => {
                let Some(i) = window_at(&self.wins, p).filter(|&i| self.win_rect(i, p).is_some()) else { return Act::None };
                self.sel = self.win_rect(i, p);
                return Act::Capture { copy_only: false };
            }
            Mode::Region => {}
        }
        if mon != self.active {
            self.active = mon;
            self.tool_at = now;
        }
        self.bar_at = None;
        self.drag = Some(match self.grip_at(p) {
            Some(g) => Drag { grip: Some(g), from: p, orig: self.sel.unwrap_or_default() },
            None => {
                self.sel = None;
                Drag { grip: None, from: p, orig: IRect::default() }
            }
        });
        Act::None
    }

    /// The handle or the body of the selection under a point (device pixels).
    #[must_use]
    pub fn grip_at(&self, p: (i32, i32)) -> Option<Grip> {
        let s = self.sel.filter(|_| self.opts.mode == Mode::Region)?;
        let k = self.mons[mon_at(&self.mons, (s.l, s.t))].scale;
        let reach = (5.0 + 3.0) * k;
        for (g, fx, fy) in HANDLES {
            let (hx, hy) = (s.l as f32 + s.w() as f32 * fx, s.t as f32 + s.h() as f32 * fy);
            if (p.0 as f32 - hx).abs() <= reach && (p.1 as f32 - hy).abs() <= reach {
                return Some(g);
            }
        }
        s.contains(p).then_some(Grip::Move)
    }

    pub fn release(&mut self, now: u64) {
        for i in 0..self.strip.len() {
            if self.ani.get(&Ani::Press(i)).is_some_and(|t| t.target() > 0.5) {
                self.go(Ani::Press(i), 0.0, now, 140, SPRING);
            }
        }
        let Some(d) = self.drag.take() else { return };
        self.dirty = true;
        let k = self.mons[mon_at(&self.mons, d.from)].scale;
        // a click that dragged nothing (under 6 DIPs each way) is no selection
        if d.grip.is_none() && self.sel.is_none_or(|s| (s.w() as f32) < 6.0 * k || (s.h() as f32) < 6.0 * k) {
            self.sel = None;
            return;
        }
        self.bar_at = Some(now);
    }

    fn click(&mut self, c: Ctl, now: u64) -> Act {
        self.dirty = true;
        match c {
            Ctl::Kind(k) => self.set_kind(k, now),
            Ctl::Mode(m) => self.set_mode(m, now),
            Ctl::Gear => {
                self.opts_open = !self.opts_open;
                self.pop_at = now;
                self.sync_strip(now);
            }
            Ctl::Strip => {}
            Ctl::Pick(i) => return Act::Window(i),
            Ctl::ToFull => self.set_mode(Mode::Full, now),
            Ctl::Close | Ctl::Cancel => return Act::Cancel,
            Ctl::Copy => return Act::Capture { copy_only: true },
            Ctl::Go => return Act::Capture { copy_only: false },
            Ctl::Switch(i) => {
                self.opts.toggle(i);
                let on = self.opts.switches().get(i).is_some_and(|s| s.2);
                self.go(Ani::Switch(i), f32::from(on), now, 200, SPRING);
            }
            Ctl::Wait(i) => self.opts.set_wait(DELAYS[i.min(2)]),
        }
        Act::None
    }

    pub fn key(&mut self, k: Key, now: u64) -> Act {
        self.dirty = true;
        match k {
            Key::Esc if self.opts_open => {
                self.opts_open = false;
                self.sync_strip(now);
            }
            Key::Esc => return Act::Cancel,
            Key::Enter if self.sel.is_some() => return Act::Capture { copy_only: false },
            Key::Enter => {}
            Key::Char(c) => match c.to_ascii_lowercase() {
                'v' => self.set_kind(Kind::Rec, now),
                's' => self.set_kind(Kind::Shot, now),
                'r' => self.set_mode(Mode::Region, now),
                'w' => self.set_mode(Mode::Window, now),
                'f' => self.set_mode(Mode::Full, now),
                _ => {}
            },
        }
        Act::None
    }

    /// The pointer is over `c` (or over none).
    pub fn point_at(&mut self, c: Option<Ctl>, now: u64) {
        let on = matches!(c, Some(Ctl::Strip | Ctl::Pick(_) | Ctl::ToFull));
        let card = if let Some(Ctl::Pick(i)) = c { Some(i) } else { None };
        if on != self.on_strip || card != self.strip_hover {
            self.on_strip = on;
            if on {
                self.hover = None; // the screen under the strip is not pointed at
            }
            self.strip_hover = card;
            if let Some(r) = self.hover_rect().filter(|_| card.is_some()) {
                self.glide(r, now);
            }
            self.sync_lit(now);
        }
        if self.hot == c {
            return;
        }
        if let Some(old) = self.hot {
            self.go(Ani::Hover(old), 0.0, now, 140, SPRING);
        }
        if let Some(new) = c {
            self.go(Ani::Hover(new), 1.0, now, 140, SPRING);
        }
        self.hot = c;
    }

    pub fn flash(&mut self, now: u64) {
        self.flash = Some(now);
        self.opts_open = false;
        self.dirty = true;
    }

    pub fn close(&mut self, now: u64) {
        self.closing = Some(now);
        self.dirty = true;
    }

    /// How opaque the whole overlay is: it fades in over .18 s (`shIn`) and out over .16 s (`dmOut`).
    #[must_use]
    pub fn opacity(&self, now: u64) -> f32 {
        match self.closing {
            Some(at) => 1.0 - EASE.ease(prog(at, now, 160)),
            None => EASE.ease(prog(self.opened, now, 180)),
        }
    }

    #[must_use]
    pub fn closed(&self, now: u64) -> bool {
        self.closing.is_some_and(|at| prog(at, now, 160) >= 1.0)
    }

    /// Something moves: frames on every tick of the screen.
    #[must_use]
    pub fn busy(&self, now: u64) -> bool {
        let early = |at: u64, ms: u32| prog(at, now, ms) < 1.0;
        self.ani.values().any(|t| t.running(now))
            || early(self.opened, 180)
            || early(self.tool_at, 300)
            || self.bar_at.is_some_and(|a| early(a, 220))
            || (self.strip_up && early(self.strip_at, 280))
            || (self.opts_open && early(self.pop_at, 200))
            || self.flash.is_some_and(|a| early(a, 320))
            || self.closing.is_some()
    }

    #[must_use]
    pub fn flashed(&self, now: u64) -> bool {
        self.flash.is_some_and(|a| prog(a, now, 320) >= 1.0)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // layout

    /// The chrome of monitor `i`; `measure` is the text width in DIPs.
    pub fn layout(&self, i: usize, measure: &mut dyn FnMut(&str, Font) -> f32) -> Layout {
        let Some(m) = self.mons.get(i).copied() else { return Layout::default() };
        let (w, h) = (m.rect.w() as f32 / m.scale, m.rect.h() as f32 / m.scale);
        let mut lay = Layout { size: (w, h), ..Layout::default() };
        if i == self.active {
            // centred; on a monitor narrower than the toolbar (high scale) it keeps its right end, so Close stays on the screen
            let tool = Rect::new(((w - TOOL_W) / 2.0).round().min(w - TOOL_W), TOOL_TOP, TOOL_W, TOOL_H);
            let (kx, mx, gx) = (tool.x + 6.0, tool.x + 109.0, tool.x + 468.0);
            for (n, k) in KINDS.into_iter().enumerate() {
                lay.items.push((Ctl::Kind(k), Rect::new(kx + 3.0 + 40.0 * n as f32, tool.y + 7.0, 40.0, 34.0)));
            }
            for (n, md) in MODES.into_iter().enumerate() {
                lay.items.push((Ctl::Mode(md), Rect::new(mx + 3.0 + 112.0 * n as f32, tool.y + 7.0, 112.0, 34.0)));
            }
            lay.items.push((Ctl::Gear, Rect::new(gx, tool.y + 6.0, 36.0, 36.0)));
            lay.items.push((Ctl::Close, Rect::new(gx + 40.0, tool.y + 6.0, 36.0, 36.0)));
            lay.tool = Some(tool);
            if self.opts_open {
                let sw = self.opts.switches();
                let rows = sw.len() as f32;
                let pop = Rect::new(tool.x + TOOL_W - POP_W, tool.y + 56.0, POP_W, 8.0 + 24.0 + 44.0 * rows + 44.0 + 4.0 + 31.0 + 8.0);
                for n in 0..sw.len() {
                    lay.items.push((Ctl::Switch(n), Rect::new(pop.x + 8.0, pop.y + 32.0 + 44.0 * n as f32, POP_W - 16.0, 44.0)));
                }
                let y = pop.y + 32.0 + 44.0 * rows;
                let mut x = pop.x + POP_W - 8.0 - 10.0 - 2.0;
                for n in (0..3).rev() {
                    let bw = measure(wait_label(n, self.opts.kind), F_SEG) + 18.0;
                    x -= bw;
                    lay.items.push((Ctl::Wait(n), Rect::new(x, y + 9.0, bw, 26.0)));
                }
                lay.pop = Some(pop);
            }
        }
        if i == self.active && self.strip_on() {
            self.strip_layout(&mut lay, measure);
        }
        // the strip takes the bottom of the monitor: the label of a window above it does not go under it
        let free = lay.strip.map_or(h, |r| r.y - 8.0);
        if let Some(s) = self.sel.or_else(|| self.hover_rect())
            && let Some(vis) = s.intersect(m.rect)
        {
                let sd = vis.dip(m.rect, m.scale);
                let ready = self.ready();
                if ready && self.flash.is_none() && !self.counting {
                    let enter = measure("Enter", F_KBD) + 10.0;
                    let cancel = 32.0;
                    let bw = if self.opts.kind == Kind::Rec {
                        let rec = 10.0 + 10.0 + 8.0 + measure(rec_label(), F_GO) + 8.0 + enter + 12.0;
                        4.0 + cancel + 4.0 + rec + 4.0
                    } else {
                        let copy = 11.0 + 14.0 + 7.0 + measure(copy_label(), F_MODE) + 11.0;
                        let done = 12.0 + 14.0 + 7.0 + measure(done_label(), F_GO) + 7.0 + enter + 12.0;
                        4.0 + cancel + 4.0 + copy + 4.0 + done + 4.0
                    };
                    let by = if sd.y + sd.h + 56.0 < h { sd.y + sd.h + 12.0 } else { (sd.y - 52.0).max(8.0) };
                    let bx = (sd.x + sd.w - 222.0).min(w - 230.0).max(8.0);
                    let bar = Rect::new(bx, by, bw, 40.0);
                    let mut x = bar.x + 4.0;
                    lay.items.push((Ctl::Cancel, Rect::new(x, bar.y + 4.0, 32.0, 32.0)));
                    x += 36.0;
                    if self.opts.kind == Kind::Shot {
                        let copy = 11.0 + 14.0 + 7.0 + measure(copy_label(), F_MODE) + 11.0;
                        lay.items.push((Ctl::Copy, Rect::new(x, bar.y + 4.0, copy, 32.0)));
                        x += copy + 4.0;
                    }
                    lay.items.push((Ctl::Go, Rect::new(x, bar.y + 4.0, bar.x + bar.w - 4.0 - x, 32.0)));
                    lay.bar = Some(bar);
                }
                let lw = measure(&self.label_text(), F_LABEL) + 18.0;
                let ly = if sd.y + sd.h + 36.0 < free && self.opts.mode != Mode::Full {
                    sd.y + sd.h + if ready && self.opts.mode == Mode::Region { 60.0 } else { 10.0 }
                } else {
                    (sd.y + 8.0).max(8.0)
                };
                lay.label = Some(Rect::new(sd.x, ly, lw, 24.0));
        }
        lay
    }

    /// The strip of windows at the bottom middle of the active monitor, and where each card of it stands now.
    fn strip_layout(&self, lay: &mut Layout, measure: &mut dyn FnMut(&str, Font) -> f32) {
        let (w, h) = lay.size;
        let cards = self.shown();
        if cards.is_empty() {
            // a recording with no window to take: the sentence and the button that records the whole screen
            let (tw, bw) = (measure(empty_text(), F_BODY), measure(full_label(), F_BTN) + 24.0);
            let sw = 12.0 + 4.0 + tw + 12.0 + bw + 4.0 + 12.0;
            let r = Rect::new(((w - sw) / 2.0).round(), h - STRIP_BOTTOM - 56.0, sw, 56.0);
            lay.items.push((Ctl::Strip, r));
            lay.items.push((Ctl::ToFull, Rect::new(r.x + 12.0 + 4.0 + tw + 12.0, r.y + 12.0, bw, 30.0)));
            lay.strip = Some(r);
            return;
        }
        let (sw, content, view_w) = Self::strip_dims(cards.len(), w);
        let over = content > view_w + 0.5;
        let row = CARD_H + if over { SCROLL_H } else { 0.0 };
        let sh = 10.0 + STRIP_HEAD + row + 12.0;
        let r = Rect::new(((w - sw) / 2.0).round(), h - STRIP_BOTTOM - sh, sw, sh);
        let view = Rect::new(r.x + 12.0, r.y + 10.0 + STRIP_HEAD, view_w, row);
        let max = (content - view_w).max(0.0);
        let at = self.scroll.clamp(0.0, max);
        lay.items.push((Ctl::Strip, r));
        for (k, &idx) in cards.iter().enumerate() {
            let c = Rect::new(view.x + k as f32 * (CARD_W + CARD_GAP) - at, view.y, CARD_W, CARD_H);
            lay.cards.push((idx, c));
            // the part of a card that is in view takes the pointer
            let (l, rr) = (c.x.max(view.x), (c.x + c.w).min(view.x + view.w));
            if rr > l {
                lay.items.push((Ctl::Pick(idx), Rect::new(l, c.y, rr - l, c.h)));
            }
        }
        (lay.strip, lay.view, lay.scroll_max) = (Some(r), Some(view), max);
    }

    /// What the hint says: Window mode adds the strip when there is one.
    fn hint(&self) -> &'static str {
        let cards = !self.shown().is_empty();
        match (self.opts.kind, self.opts.mode) {
            (Kind::Shot, Mode::Window) if cards => t!("Щёлкните окно, чтобы снять его, или выберите ниже", "Click a window to capture it, or pick one below", ja: "ウィンドウをクリックして撮影、または下から選択", pt: "Clique em uma janela para capturá-la ou escolha abaixo", zh: "点击窗口进行截图，或在下方选择"),
            (Kind::Rec, Mode::Window) if cards => t!("Щёлкните окно, чтобы записать его, или выберите ниже", "Click a window to record it, or pick one below", ja: "ウィンドウをクリックして録画、または下から選択", pt: "Clique em uma janela para gravá-la ou escolha abaixo", zh: "点击窗口开始录制，或在下方选择"),
            (Kind::Rec, Mode::Window) => t!("Здесь нет окон для записи", "No windows here to record", ja: "ここには録画できるウィンドウがありません", pt: "Nenhuma janela aqui para gravar", zh: "此处没有可录制的窗口"),
            (k, m) => hint_text(k, m),
        }
    }

    fn label_text(&self) -> String {
        let Some(s) = self.sel.or_else(|| self.hover_rect()) else { return String::new() };
        let size = format!("{} × {}", s.w(), s.h());
        if let (None, Some(w)) = (self.sel, self.strip_hover.and_then(|i| self.strip.get(i)))
            && self.opts.mode == Mode::Window
        {
            return format!("{} · {size}", trunc(&w.title, 36));
        }
        match (self.sel, self.hover) {
            (None, Some(i)) if self.opts.mode == Mode::Window => format!("{} · {size}", trunc(&self.wins[i].title, 36)),
            _ => size,
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // painting

    /// One frame of monitor `i`: the frozen picture, the dimming, the selection and the chrome.
    pub fn paint(&self, c: &mut Canvas<'_>, i: usize, frozen: Option<&Picture>, now: u64) {
        let Some(m) = self.mons.get(i).copied() else { return };
        let lay = self.layout(i, &mut |s, f| c.measure(s, f));
        let (w, h) = lay.size;
        let full = Rect::new(0.0, 0.0, w, h);
        if let Some(p) = frozen {
            c.picture(p, 0.0, 0.0);
        }
        let shown = self.showing(now).and_then(|s| s.intersect(m.rect));
        let window_look = self.opts.mode == Mode::Window;
        let rad = if window_look { 14.0 } else { 0.0 };
        match shown {
            Some(s) => {
                let hole = s.dip(m.rect, m.scale);
                c.outside(hole, rad, |c| c.fill(full, 0.0, SCRIM));
            }
            None => c.fill(full, 0.0, SCRIM),
        }
        let k = match self.closing {
            Some(at) => 1.0 - 0.04 * EASE.ease(prog(at, now, 160)),
            None => 1.0,
        };
        c.transformed(w / 2.0, h / 2.0, k, 0.0, 0.0, |c| self.paint_chrome(c, i, &lay, shown, now));
    }

    fn paint_chrome(&self, c: &mut Canvas<'_>, i: usize, lay: &Layout, shown: Option<IRect>, now: u64) {
        let m = self.mons[i];
        let acc = accent();
        let (w, h) = lay.size;
        // the guides of an empty region picker
        if let (Mode::Region, None, Some(p), None) = (self.opts.mode, self.sel, self.cur, &self.drag)
            && m.rect.contains(p)
        {
                let (x, y) = ((p.0 - m.rect.l) as f32 / m.scale, (p.1 - m.rect.t) as f32 / m.scale);
                c.fill(Rect::new(0.0, y, w, 1.0), 0.0, Rgba::pure_white(0.35));
                c.fill(Rect::new(x, 0.0, 1.0, h), 0.0, Rgba::pure_white(0.35));
                let text = format!("{} , {}", p.0 - m.rect.l, p.1 - m.rect.t);
                let tw = c.measure(&text, F_COORD) + 16.0;
                let r = Rect::new((x + 14.0).min(w - 110.0), (y + 14.0).min(h - 30.0), tw, 22.0);
                c.fill(r, 6.0, Rgba::css(12, 13, 18, 0.86));
                c.text(&text, Rect::new(r.x + 8.0, r.y, tw - 16.0, r.h), F_COORD, INK, Align::Start);
        }
        if let Some(s) = shown {
            let r = s.dip(m.rect, m.scale);
            let window_look = self.opts.mode == Mode::Window;
            if window_look {
                // `inset:-3px; border: 3px solid accent; border-radius: 17px`
                c.ring(r.inset(-3.0), 17.0, 3.0, acc);
            } else {
                c.dashed_rect(r.inset(-0.25), 1.5, 3.0, 2.0, Rgba::pure_white(0.9));
            }
            if self.editable() {
                for (_, fx, fy) in HANDLES {
                    let (hx, hy) = (r.x + r.w * fx, r.y + r.h * fy);
                    let hr = Rect::new(hx - 5.0, hy - 5.0, 10.0, 10.0);
                    c.outside(hr, 3.0, |c| c.shadow(hr, 3.0, 2.0, 6.0, Rgba::css(0, 0, 0, 0.4)));
                    c.fill(hr.inset(-1.5), 4.0, acc);
                    c.fill(hr, 3.0, Rgba::pure_white(1.0));
                }
            }
            if let Some(at) = self.flash {
                let t = prog(at, now, 320);
                // `shFlash`: 0 -> .75 -> 0, `ease` for each part
                let a = if t < 0.25 { EASE.ease(t / 0.25) * 0.75 } else { 0.75 * (1.0 - EASE.ease((t - 0.25) / 0.75)) };
                c.fill(r, if window_look { 14.0 } else { 0.0 }, Rgba::pure_white(a)); // `.shflash`: #fff
            }
            if let Some(l) = lay.label {
                c.fill(l, 7.0, PLATE);
                c.text(&self.label_text(), Rect::new(l.x + 9.0, l.y, l.w - 18.0, l.h), F_LABEL, INK, Align::Start);
            }
        }
        if let (Some(bar), Some(at)) = (lay.bar, self.bar_at) {
            let p = SNAP.ease(prog(at, now, 220));
            let k = 0.97 + 0.03 * p;
            c.faded(p, |c| c.transformed(bar.cx(), bar.cy(), k, 0.0, -4.0 * (1.0 - p), |c| self.paint_bar(c, lay, bar, now)));
        } else if let Some(bar) = lay.bar {
            self.paint_bar(c, lay, bar, now);
        }
        if let Some(tool) = lay.tool {
            if shown.is_none() && !self.opts_open && self.drag.is_none() {
                let text = self.hint();
                let hw = c.measure(text, F_HINT) + 24.0;
                let r = Rect::new(((w - hw) / 2.0).round(), 74.0, hw, 28.0);
                c.fill(r, 14.0, Rgba::css(12, 13, 18, 0.7));
                c.text(text, r, F_HINT, SOFT_INK, Align::Center);
            }
            let p = SNAP.ease(prog(self.tool_at, now, 300));
            let k = 0.97 + 0.03 * p;
            c.faded(p, |c| c.transformed(tool.cx(), tool.cy(), k, 0.0, -10.0 * (1.0 - p), |c| self.paint_tool(c, lay, tool, now)));
        }
        if let Some(strip) = lay.strip {
            // `shpickIn`: from `opacity: 0; translate(0, 12px)`, .28 s, the springy ease
            let p = SPRING.ease(prog(self.strip_at, now, 280));
            c.faded(p, |c| c.transformed(strip.cx(), strip.cy(), 1.0, 0.0, 12.0 * (1.0 - p), |c| self.paint_strip(c, lay, strip, now)));
        }
    }

    fn paint_strip(&self, c: &mut Canvas<'_>, lay: &Layout, r: Rect, now: u64) {
        c.outside(r, 20.0, |c| {
            c.shadow(r, 20.0, 24.0, 60.0, Rgba::css(0, 0, 0, 0.5));
            c.ring(r.inset(-1.0), 21.0, 1.0, Rgba::css(0, 0, 0, 0.45));
        });
        c.fill(r, 20.0, Rgba::css(16, 17, 24, 0.84));
        c.ring(r, 20.0, 1.0, Rgba::white(0.1));
        let Some(view) = lay.view else {
            // the empty state: the sentence and the button
            let tw = c.measure(empty_text(), F_BODY);
            c.text(empty_text(), Rect::new(r.x + 16.0, r.y + 12.0, tw, 30.0), F_BODY, DIM_INK, Align::Start);
            if let Some(b) = lay.rect_of(Ctl::ToFull) {
                c.fill(b, 9.0, Rgba::white(0.1));
                self.hover_fill(c, Ctl::ToFull, b, 9.0, now);
                c.text(full_label(), b, F_BTN, INK, Align::Center);
            }
            return;
        };
        let head = t!("Или выберите окно", "Or pick a window", ja: "ウィンドウを選択", pt: "Ou escolha uma janela", zh: "或选择窗口");
        let note = if self.opts.kind == Kind::Rec {
            t!("окна с других экранов отсюда записать нельзя", "windows from other screens can’t be recorded from here", ja: "他の画面のウィンドウはここから録画できません", pt: "janelas de outras telas não podem ser gravadas daqui", zh: "无法在此录制其他屏幕上的窗口")
        } else {
            t!("свёрнутые, перекрытые и на других экранах тоже", "minimized, covered and on other screens too", ja: "最小化・隠れている・他の画面のウィンドウも", pt: "minimizadas, cobertas e em outras telas também", zh: "最小化、被遮挡及其他屏幕上的窗口也可选")
        };
        let hw = c.measure(head, F_HEAD);
        let (hx, hy) = (r.x + 12.0 + 4.0, r.y + 10.0);
        c.text(head, Rect::new(hx, hy, hw, 24.0), F_HEAD, INK, Align::Start);
        c.text(&format!("· {note}"), Rect::new(hx + hw + 8.0, hy, r.w, 24.0), F_FOOT, DIM_INK, Align::Start);
        c.clipped(Rect::new(view.x, view.y, view.w, view.h), |c| {
            for &(idx, card) in &lay.cards {
                if card.x < view.x + view.w && card.x + card.w > view.x {
                    self.paint_card(c, idx, card, now);
                }
            }
        });
        if lay.scroll_max > 0.0 {
            // the thin scrollbar under the cards
            let content = view.w + lay.scroll_max;
            let tw = (view.w * view.w / content).max(24.0);
            let at = self.scroll.clamp(0.0, lay.scroll_max) / lay.scroll_max;
            c.fill(Rect::new(view.x + (view.w - tw) * at, view.y + CARD_H + 3.0, tw, 4.0), 2.0, Rgba::white(0.28));
        }
    }

    /// One card: the window's picture (or its icon on a gradient of the app's colour), its tag, its icon and its title.
    fn paint_card(&self, c: &mut Canvas<'_>, idx: usize, r: Rect, now: u64) {
        let Some(w) = self.strip.get(idx) else { return };
        let (lit, hover, press) = (self.val(Ani::Lit(idx), now), self.val(Ani::Hover(Ctl::Pick(idx)), now), self.val(Ani::Press(idx), now));
        c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * press, 0.0, 0.0, |c| {
            // `.pwc:hover` is .08 of the ink; a card lit from the screen has .1 and the accent ring (`inset 0 0 0 2px`)
            let a = 0.08 * hover + 0.10 * lit * (1.0 - hover);
            if a > 0.001 {
                c.fill(r, 14.0, Rgba::white(a));
            }
            if lit > 0.001 {
                c.ring(r, 14.0, 2.0, accent().fade(lit));
            }
            let t = Rect::new(r.x + 5.0, r.y + 5.0, THUMB_W, THUMB_H);
            let shot = {
                let mut pics = self.pics.borrow_mut();
                if let Some(th) = self.thumbs.get(&w.hwnd)
                    && pics.get(&w.hwnd).is_none_or(|p| !c.is_current(p))
                    && let Some(p) = c.pixels(&th.px, th.w, th.h)
                {
                    pics.insert(w.hwnd, p);
                }
                match pics.get(&w.hwnd) {
                    Some(p) => {
                        c.fill(t, 9.0, Rgba::hex(0x0c0d10));
                        c.picture_cover(p, t, 9.0, 1.0);
                        true
                    }
                    None => false,
                }
            };
            let icon = windows::Win32::UI::WindowsAndMessaging::HICON(w.icon as *mut std::ffi::c_void);
            if !shot {
                // `linear-gradient(140deg, ..)` of the app's colour, and its icon big in the middle
                let base = w.tint.unwrap_or_else(accent);
                let ink = Rgba::new(12.0 / 255.0, 13.0 / 255.0, 16.0 / 255.0, 1.0);
                c.gradient_angle(t, 9.0, 140.0, &[(0.0, base.mix(ink, 0.55)), (1.0, base.mix(ink, 0.78))]);
                c.hicon(icon, Rect::new(t.cx() - 18.0, t.cy() - 18.0, 36.0, 36.0), 1.0);
            }
            if let Some(tag) = w.at.tag() {
                let tw = c.measure(&tag, F_TAG) + 14.0;
                let chip = Rect::new(t.x + 6.0, t.y + t.h - 6.0 - 20.0, tw, 20.0);
                c.fill(chip, 10.0, Rgba::new(10.0 / 255.0, 11.0 / 255.0, 15.0 / 255.0, 0.72));
                c.text(&tag, Rect::new(chip.x + 7.0, chip.y, tw - 14.0, 20.0), F_TAG, Rgba::pure_white(1.0), Align::Start);
            }
            let y = t.y + t.h + 7.0;
            let mut x = r.x + 5.0 + 2.0;
            if w.icon != 0 {
                c.hicon(icon, Rect::new(x, y, 16.0, 16.0), 1.0);
                x += 16.0 + 7.0;
            }
            c.text_fit(&w.title, Rect::new(x, y, (t.x + t.w - 2.0 - x).max(0.0), 16.0), F_CARD, INK, Align::Start);
        });
    }

    fn glass(c: &mut Canvas<'_>, r: Rect, radius: f32, bg: Rgba, lift: (f32, f32, f32)) {
        c.outside(r, radius, |c| {
            c.shadow(r, radius, lift.0, lift.1, Rgba::css(0, 0, 0, lift.2));
            c.ring(r.inset(-1.0), radius + 1.0, 1.0, Rgba::css(0, 0, 0, 0.5));
        });
        c.fill(r, radius, bg);
        c.ring(r, radius, 1.0, Rgba::white(0.1));
    }

    fn hover_fill(&self, c: &mut Canvas<'_>, ctl: Ctl, r: Rect, radius: f32, now: u64) {
        let a = self.val(Ani::Hover(ctl), now);
        if a > 0.001 {
            c.fill(r, radius, Rgba::white(0.09 * a));
        }
    }

    fn paint_bar(&self, c: &mut Canvas<'_>, lay: &Layout, bar: Rect, now: u64) {
        Self::glass(c, bar, 12.0, Rgba::css(20, 21, 28, 0.92), (14.0, 36.0, 0.5));
        let acc = accent();
        let r = lay.rect_of(Ctl::Cancel).unwrap_or_default();
        self.hover_fill(c, Ctl::Cancel, r, 9.0, now);
        c.icon(I_X, (r.cx(), r.cy()), 13.0, 2.4, SOFT_INK, 0.0);
        let enter = |c: &mut Canvas<'_>, r: Rect, bg: Rgba, ink: Rgba| {
            let kw = c.measure("Enter", F_KBD) + 10.0;
            let k = Rect::new(r.x + r.w - 12.0 - kw, r.cy() - 6.5, kw, 13.0);
            c.fill(k, 4.0, bg);
            c.text("Enter", k, F_KBD, ink, Align::Center);
        };
        if self.opts.kind == Kind::Shot
            && let Some(r) = lay.rect_of(Ctl::Copy)
        {
                self.hover_fill(c, Ctl::Copy, r, 9.0, now);
                c.icon(I_COPY, (r.x + 11.0 + 7.0, r.cy()), 14.0, 2.0, INK, 0.0);
                c.text(copy_label(), Rect::new(r.x + 11.0 + 14.0 + 7.0, r.y, r.w, r.h), F_MODE, INK, Align::Start);
        }
        if let Some(r) = lay.rect_of(Ctl::Go) {
            let hov = self.val(Ani::Hover(Ctl::Go), now);
            let ink = if self.opts.kind == Kind::Rec { Rgba::pure_white(1.0) } else { Rgba::hex(0x0b1220) };
            let bg = if self.opts.kind == Kind::Rec { RED } else { acc };
            c.fill(r, 9.0, bg);
            if hov > 0.001 {
                c.fill(r, 9.0, Rgba::white(0.10 * hov)); // `filter: brightness(1.1)`
            }
            if self.opts.kind == Kind::Rec {
                c.circle(r.x + 10.0 + 5.0, r.cy(), 5.0, Rgba::pure_white(1.0));
                c.text(rec_label(), Rect::new(r.x + 10.0 + 10.0 + 8.0, r.y, r.w, r.h), F_GO, ink, Align::Start);
                enter(c, r, Rgba::css(0, 0, 0, 0.2), ink);
            } else {
                c.icon(I_CHECK, (r.x + 12.0 + 7.0, r.cy()), 14.0, 2.4, ink, 0.0);
                c.text(done_label(), Rect::new(r.x + 12.0 + 14.0 + 7.0, r.y, r.w, r.h), F_GO, ink, Align::Start);
                enter(c, r, Rgba::css(11, 18, 32, 0.18), ink);
            }
        }
    }

    fn paint_tool(&self, c: &mut Canvas<'_>, lay: &Layout, tool: Rect, now: u64) {
        let acc = accent();
        Self::glass(c, tool, 16.0, Rgba::css(20, 21, 28, 0.9), (18.0, 44.0, 0.55));
        let well = |c: &mut Canvas<'_>, r: Rect| c.fill(r, 12.0, Rgba::css(0, 0, 0, 0.28));
        // the two radio groups, each with its thumb sliding under the chosen button
        let kinds = Rect::new(tool.x + 6.0, tool.y + 4.0, 86.0, 40.0);
        well(c, kinds);
        let kt = Rect::new(kinds.x + 3.0 + self.val(Ani::KindX, now), kinds.y + 3.0, 40.0, 34.0);
        c.fill(kt, 9.0, if self.opts.kind == Kind::Rec { RED } else { Rgba::white(0.12) });
        for k in KINDS {
            let Some(r) = lay.rect_of(Ctl::Kind(k)) else { continue };
            let ink = DIM_INK.mix(if k == Kind::Rec { Rgba::pure_white(1.0) } else { Rgba::strong(1.0) }, self.val(Ani::Tint(Ctl::Kind(k)), now));
            if k == Kind::Shot {
                c.icon(I_SHOT, (r.cx(), r.cy()), 17.0, 1.9, ink, 0.0);
            } else {
                c.icon(I_REC, (r.cx(), r.cy()), 17.0, 1.9, ink, 0.0);
                c.circle(r.cx(), r.cy(), 3.2 * 17.0 / 24.0, ink);
            }
        }
        for x in [tool.x + 100.0, tool.x + 459.0] {
            c.fill(Rect::new(x, tool.y + 13.0, 1.0, 22.0), 0.0, Rgba::white(0.1));
        }
        let modes = Rect::new(tool.x + 109.0, tool.y + 4.0, 342.0, 40.0);
        well(c, modes);
        let mt = Rect::new(modes.x + 3.0 + self.val(Ani::ModeX, now), modes.y + 3.0, 112.0, 34.0);
        c.fill(mt, 9.0, Rgba::white(0.12));
        c.inset_edge(mt, 9.0, 1.0, Rgba::white(0.1));
        for md in MODES {
            let Some(r) = lay.rect_of(Ctl::Mode(md)) else { continue };
            let ink = DIM_INK.mix(Rgba::strong(1.0), self.val(Ani::Tint(Ctl::Mode(md)), now));
            let label = mode_label(md);
            let tw = c.measure(label, F_MODE);
            let x = r.cx() - (15.0 + 7.0 + tw) / 2.0;
            let d = match md {
                Mode::Region => I_REGION,
                Mode::Window => I_WINDOW,
                Mode::Full => I_FULL,
            };
            c.icon(d, (x + 7.5, r.cy()), 15.0, 1.9, ink, 0.0);
            c.text(label, Rect::new(x + 22.0, r.y, tw + 4.0, r.h), F_MODE, ink, Align::Start);
        }
        if let Some(r) = lay.rect_of(Ctl::Gear) {
            if self.opts_open {
                c.fill(r, 10.0, Rgba::white(0.12));
            } else {
                self.hover_fill(c, Ctl::Gear, r, 10.0, now);
            }
            c.icon(super::icons::GEAR, (r.cx(), r.cy()), 16.0, 1.8, SOFT_INK, 0.0);
        }
        if let Some(r) = lay.rect_of(Ctl::Close) {
            self.hover_fill(c, Ctl::Close, r, 10.0, now);
            c.icon(I_X, (r.cx(), r.cy()), 13.0, 2.4, SOFT_INK, 0.0);
        }
        if let Some(pop) = lay.pop {
            let p = SNAP.ease(prog(self.pop_at, now, 200));
            // `shOpts`: from `translateY(-6px) scale(.97)`, origin at the top right
            c.faded(p, |c| c.transformed(pop.x + pop.w, pop.y, 0.97 + 0.03 * p, 0.0, -6.0 * (1.0 - p), |c| self.paint_pop(c, lay, pop, acc, now)));
        }
    }

    fn paint_pop(&self, c: &mut Canvas<'_>, lay: &Layout, pop: Rect, acc: Rgba, now: u64) {
        Self::glass(c, pop, 16.0, Rgba::css(20, 21, 28, 0.96), (20.0, 50.0, 0.55));
        let title = if self.opts.kind == Kind::Rec { t!("Запись", "Recording", ja: "録画", pt: "Gravação", zh: "录制") } else { t!("Скриншот", "Screenshot", ja: "スクリーンショット", pt: "Captura de tela", zh: "截图") };
        c.text(&title.to_uppercase(), Rect::new(pop.x + 18.0, pop.y + 12.0, pop.w - 36.0, 14.0), F_TITLE, TITLE_INK, Align::Start);
        for (n, (label, sub, on)) in self.opts.switches().into_iter().enumerate() {
            let Some(r) = lay.rect_of(Ctl::Switch(n)) else { continue };
            self.hover_fill(c, Ctl::Switch(n), r, 10.0, now);
            c.text(&label, Rect::new(r.x + 10.0, r.cy() - 15.5, r.w - 60.0, 15.0), F_MODE, INK, Align::Start);
            c.text(&sub, Rect::new(r.x + 10.0, r.cy() + 1.5, r.w - 60.0, 14.0), F_SUB, MUTED, Align::Start);
            let k = self.ani.get(&Ani::Switch(n)).map_or(f32::from(on), |t| t.value(now));
            let track = Rect::new(r.x + r.w - 10.0 - 34.0, r.cy() - 10.0, 34.0, 20.0);
            c.fill(track, 10.0, Rgba::white(0.16).mix(acc, k));
            c.circle(track.x + 2.0 + 8.0 + 14.0 * k, track.cy(), 8.0, Rgba::pure_white(1.0));
        }
        let rows = self.opts.switches().len() as f32;
        let dy = pop.y + 32.0 + 44.0 * rows;
        let label = if self.opts.kind == Kind::Rec { t!("Обратный отсчёт", "Countdown", ja: "カウントダウン", pt: "Contagem regressiva", zh: "倒计时") } else { t!("Задержка", "Delay", ja: "遅延", pt: "Atraso", zh: "延迟") };
        c.text(label, Rect::new(pop.x + 18.0, dy, 160.0, 44.0), F_MODE, INK, Align::Start);
        let first = lay.rect_of(Ctl::Wait(0)).unwrap_or_default();
        let last = lay.rect_of(Ctl::Wait(2)).unwrap_or_default();
        c.fill(Rect::new(first.x - 2.0, first.y - 2.0, last.x + last.w - first.x + 4.0, 30.0), 9.0, Rgba::css(0, 0, 0, 0.28));
        for (n, &secs) in DELAYS.iter().enumerate() {
            let Some(r) = lay.rect_of(Ctl::Wait(n)) else { continue };
            let on = secs == self.opts.wait();
            if on {
                c.fill(r, 7.0, Rgba::white(0.14));
            }
            c.text(wait_label(n, self.opts.kind), r, F_SEG, if on { Rgba::strong(1.0) } else { DIM_INK }, Align::Center);
        }
        // the shortcut footer
        let fy = dy + 44.0 + 4.0;
        c.fill(Rect::new(pop.x + 10.0, fy, pop.w - 20.0, 1.0), 0.0, Rgba::white(0.07));
        c.text(t!("Сочетание", "Shortcut", ja: "ショートカット", pt: "Atalho", zh: "快捷键"), Rect::new(pop.x + 18.0, fy + 10.0, 120.0, 16.0), F_FOOT, MUTED, Align::Start);
        let keys = ["Win", "Shift", if self.opts.kind == Kind::Rec { "R" } else { "S" }];
        let mut x = pop.x + pop.w - 8.0 - 2.0 - 8.0 - 3.0;
        for k in keys.into_iter().rev() {
            let kw = c.measure(k, F_ATK) + 12.0;
            x -= kw;
            let r = Rect::new(x, fy + 10.0, kw, 16.0);
            c.fill(r, 4.0, Rgba::white(0.08));
            c.inset_edge(r, 4.0, -1.0, Rgba::white(0.08));
            c.text(k, r, F_ATK, SOFT_INK, Align::Center);
            x -= 7.0;
        }
    }
}


fn empty_text() -> &'static str {
    t!("На этом экране нет окон для записи.", "There are no windows on this screen to record.", ja: "この画面に録画できるウィンドウはありません。", pt: "Não há janelas nesta tela para gravar.", zh: "此屏幕上没有可录制的窗口。")
}

fn full_label() -> &'static str {
    t!("Записать весь экран", "Record the full screen", ja: "画面全体を録画", pt: "Gravar a tela inteira", zh: "录制整个屏幕")
}

fn wait_label(n: usize, kind: Kind) -> &'static str {
    match (n, kind) {
        (0, Kind::Shot) => t!("Нет", "None", ja: "なし", pt: "Nenhum", zh: "无"),
        (0, Kind::Rec) => t!("Выкл.", "Off", ja: "オフ", pt: "Desl.", zh: "关"),
        (1, _) => t!("3 с", "3 s", ja: "3 秒", pt: "3 s", zh: "3 秒"),
        _ => t!("5 с", "5 s", ja: "5 秒", pt: "5 s", zh: "5 秒"),
    }
}

fn copy_label() -> &'static str {
    t!("Копировать", "Copy", ja: "コピー", pt: "Copiar", zh: "复制")
}

fn done_label() -> &'static str {
    t!("Готово", "Done", ja: "完了", pt: "Concluído", zh: "完成")
}

fn rec_label() -> &'static str {
    t!("Запись", "Record", ja: "録画", pt: "Gravar", zh: "录制")
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>().trim_end()) }
}

// ---------------------------------------------------------------------------------------------------------------------
// the countdown and the recording

/// A delay or countdown: the number the disc shows, down by one each second until it is gone, and then the action happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Countdown(u32);

impl Countdown {
    #[must_use]
    pub const fn new(secs: u8) -> Self {
        Self(secs as u32)
    }

    /// The number on the disc.
    #[must_use]
    pub const fn shown(self) -> u32 {
        self.0
    }

    /// A second has passed; true once the count has run out.
    pub fn tick(&mut self) -> bool {
        self.0 = self.0.saturating_sub(1);
        self.0 == 0
    }
}

/// The countdown's disc (`shcount`): 120 DIPs, the number in 56 px; it pops in over .4 s from scale .8.
pub fn paint_count(c: &mut Canvas<'_>, size: f32, n: u32, since: u64, now: u64) {
    let t = prog(since, now, 400);
    let e = SPRING.ease(t);
    let k = 0.8 + 0.2 * e;
    let a = (t / 0.2).min(1.0);
    let r = Rect::new((size - 120.0) / 2.0, (size - 120.0) / 2.0, 120.0, 120.0);
    c.faded(a, |c| {
        c.transformed(r.cx(), r.cy(), k, 0.0, 0.0, |c| {
            c.fill(r, 60.0, Rgba::css(12, 13, 18, 0.7));
            c.text(&n.to_string(), r, Font::new(56.0, 600), Rgba::pure_white(1.0), Align::Center);
        });
    });
}

/// How the recording shows: the frame round the area, the floating control.
#[derive(Clone, Debug)]
pub struct RecView {
    pub time: String,
    /// Four bars of the microphone meter, 0..1; `None` without a microphone.
    pub meter: Option<[f32; 4]>,
    /// The Stop button is under the pointer (0..1) and pressed.
    pub hot: f32,
    pub down: bool,
    /// Milliseconds since the frame and the control came up, and (once stopping) since they started to go.
    pub since: u64,
    pub leaving: Option<u64>,
    pub window: bool,
    pub full: bool,
}

/// The frame of a recording (`recframe`): a ring just outside the area and four corner brackets. `area` is the recorded
/// rectangle in DIPs of the frame's canvas.
pub fn paint_rec_frame(c: &mut Canvas<'_>, area: Rect, v: &RecView, now: u64) {
    let (p, out) = (prog(v.since, now, 300), v.leaving.map_or(0.0, |at| EASE.ease(prog(at, now, 160))));
    let k = 1.02 - 0.02 * SPRING.ease(p);
    let op = SPRING.ease(p).min(1.0 - out);
    c.faded(op, |c| {
        c.transformed(area.cx(), area.cy(), k, 0.0, 0.0, |c| {
            let rad = if v.window { 16.0 } else { 2.0 };
            c.ring(area.inset(-2.0), rad, 2.0, RED);
            if !v.full {
                // corner brackets: 18 x 18 boxes at -4,-4 with 3 px borders on two sides
                let (x0, y0, x1, y1) = (area.x - 4.0, area.y - 4.0, area.x + area.w + 4.0, area.y + area.h + 4.0);
                for (cx, cy, sx, sy) in [(x0, y0, 1.0, 1.0), (x1, y0, -1.0, 1.0), (x1, y1, -1.0, -1.0), (x0, y1, 1.0, -1.0)] {
                    c.fill(Rect::new(cx.min(cx + 18.0 * sx), cy.min(cy + 3.0 * sy), 18.0, 3.0), 0.0, RED);
                    c.fill(Rect::new(cx.min(cx + 3.0 * sx), cy.min(cy + 18.0 * sy), 3.0, 18.0), 0.0, RED);
                }
            }
        });
    });
}

/// Where the parts of the recording control are, relative to its box (`ctl`, 44 high): the dot, the time, the meter, Stop.
#[must_use]
pub fn pill_layout(time_w: f32, stop_w: f32, mic: bool) -> (f32, Rect, Option<Rect>, Rect) {
    let dot = 14.0;
    let time = Rect::new(dot + 10.0 + 4.0 + 6.0, 0.0, time_w.max(44.0), 44.0);
    let mut x = time.x + time.w + 4.0;
    let meter = mic.then(|| {
        let r = Rect::new(x + 6.0, 14.0, 18.0, 16.0);
        x = r.x + r.w + 6.0 + 4.0;
        r
    });
    let sep = x + 4.0;
    let stop = Rect::new(sep + 1.0 + 4.0 + 4.0, 6.0, stop_w, 32.0);
    (stop.x + stop.w + 6.0, time, meter, stop)
}

const F_TIME: Font = Font::new(13.0, 600).tabular();

fn stop_label() -> &'static str {
    t!("Стоп", "Stop", ja: "停止", pt: "Parar", zh: "停止")
}

/// The widest the clock of the control can be while it reads `time` ("0:00" on the first minutes, "10:00" and "1:00:00" later): its digits, as zeros.
fn time_shape(time: &str) -> String {
    if time.len() < 4 { "0:00".into() } else { time.chars().map(|c| if c.is_ascii_digit() { '0' } else { c }).collect() }
}

/// The width of the control box (it is 44 high), whatever the language and whether the meter is there; `time` is the clock it shows.
pub fn pill_width(measure: &mut dyn FnMut(&str, Font) -> f32, mic: bool, time: &str) -> f32 {
    let (tw, sw) = (measure(&time_shape(time), F_TIME), 10.0 + 10.0 + 7.0 + measure(stop_label(), F_GO) + 12.0);
    pill_layout(tw, sw, mic).0
}

/// The floating control (`recpill`) with its top left corner at `origin` on the canvas.
pub fn paint_pill(c: &mut Canvas<'_>, origin: (f32, f32), v: &RecView, now: u64) {
    c.transformed(0.0, 0.0, 1.0, origin.0, origin.1, |c| pill(c, v, now));
}

/// The Stop button of a control whose top left corner is the origin.
pub fn stop_rect(measure: &mut dyn FnMut(&str, Font) -> f32, mic: bool, time: &str) -> Rect {
    let (tw, sw) = (measure(&time_shape(time), F_TIME), 10.0 + 10.0 + 7.0 + measure(stop_label(), F_GO) + 12.0);
    pill_layout(tw, sw, mic).3
}

fn pill(c: &mut Canvas<'_>, v: &RecView, now: u64) {
    let (p, out) = (SNAP.ease(prog(v.since, now, 320)), v.leaving.map_or(0.0, |at| EASE.ease(prog(at, now, 160))));
    let (tw, sw) = (c.measure(&time_shape(&v.time), F_TIME), 10.0 + 10.0 + 7.0 + c.measure(stop_label(), F_GO) + 12.0);
    let (width, time, meter, stop) = pill_layout(tw, sw, v.meter.is_some());
    let r = Rect::new(0.0, 0.0, width, 44.0);
    let k = (0.96 + 0.04 * p) * (1.0 - 0.04 * out);
    c.faded(p.min(1.0 - out), |c| {
        c.transformed(r.cx(), r.cy(), k, 0.0, -10.0 * (1.0 - p), |c| {
            Snip::glass(c, r, 22.0, Rgba::css(20, 21, 28, 0.92), (18.0, 44.0, 0.5));
            // `recPulse`: 1.3 s, opacity 1 -> .45 -> 1 and scale 1 -> .82 -> 1, ease-in-out
            let ph = (now.saturating_sub(v.since) % 1300) as f32 / 1300.0;
            let e = anim::CubicBezier::new(0.42, 0.0, 0.58, 1.0).ease(1.0 - (2.0 * ph - 1.0).abs());
            let d = 10.0 * (1.0 - 0.18 * e);
            c.fill(Rect::new(19.0 - d / 2.0, 22.0 - d / 2.0, d, d), d / 2.0, RED.fade(1.0 - 0.55 * e));
            c.text(&v.time, time, F_TIME, INK, Align::Start);
            if let (Some(m), Some(l)) = (meter, v.meter) {
                for (n, lv) in l.iter().enumerate() {
                    let bh = 4.0 + 10.0 * lv.clamp(0.0, 1.0);
                    c.fill(Rect::new(m.x + 5.0 * n as f32, m.cy() - bh / 2.0, 3.0, bh), 2.0, Rgba::hex(0x7fe0ad));
                }
            }
            c.fill(Rect::new(stop.x - 9.0, 12.0, 1.0, 20.0), 0.0, Rgba::white(0.1));
            let s = if v.down { 0.96 } else { 1.0 };
            c.transformed(stop.cx(), stop.cy(), s, 0.0, 0.0, |c| {
                c.fill(stop, 16.0, RED);
                if v.hot > 0.001 {
                    c.fill(stop, 16.0, Rgba::white(0.10 * v.hot));
                }
                c.fill(Rect::new(stop.x + 10.0, stop.cy() - 5.0, 10.0, 10.0), 2.5, Rgba::pure_white(1.0));
                c.text(stop_label(), Rect::new(stop.x + 27.0, stop.y, stop.w, stop.h), F_GO, Rgba::pure_white(1.0), Align::Start);
            });
        });
    });
}

/// The time of a recording as the control writes it: `0:24`, and `1:02:03` after an hour.
#[must_use]
pub fn clock_text(secs: u64) -> String {
    if secs >= 3600 { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) } else { format!("{}:{:02}", secs / 60, secs % 60) }
}

/// The bars of the microphone meter for a peak level: each follows the level with its own weight, so they do not move as one.
#[must_use]
pub fn meter_bars(level: f32, phase: f32) -> [f32; 4] {
    let l = level.clamp(0.0, 1.0).powf(0.6);
    let w = [0.8, 1.0, 0.9, 0.7];
    std::array::from_fn(|i| (l * w[i] * (0.75 + 0.25 * (phase * 6.0 + i as f32 * 1.7).sin())).clamp(0.0, 1.0))
}

/// Where the recording control stands: the top middle of the monitor, or under the recorded area when that touches the top
/// (`pillY` of the mockup). DIPs of the monitor; `h` is the monitor's height.
#[must_use]
pub fn pill_top(area: Rect, h: f32, full: bool) -> f32 {
    if area.y > 70.0 {
        16.0
    } else if !full && area.y + area.h < h - 120.0 {
        area.y + area.h + 14.0
    } else {
        16.0
    }
}

// ---------------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const M: IRect = IRect::new(0, 0, 2560, 1440);
    const M2: IRect = IRect::new(2560, 0, 4480, 1080);

    #[test]
    fn a_new_selection_is_kept_to_the_monitor_whichever_way_it_is_dragged() {
        assert_eq!(drag_new((100, 200), (50, 30), M), IRect::new(50, 30, 100, 200));
        assert_eq!(drag_new((100, 200), (-40, 5000), M), IRect::new(0, 200, 100, 1440));
        // dragging onto the next monitor stops at the edge of the first
        assert_eq!(drag_new((2500, 100), (3000, 300), M), IRect::new(2500, 100, 2560, 300));
        assert_eq!(drag_new((3000, 100), (2000, 300), M2), IRect::new(2560, 100, 3000, 300));
    }

    #[test]
    fn handles_resize_and_flip_and_the_body_moves_within_the_monitor() {
        let s = IRect::new(100, 100, 300, 200);
        assert_eq!(drag_grip(s, Grip::E, (50, 999), M), IRect::new(100, 100, 350, 200));
        assert_eq!(drag_grip(s, Grip::Nw, (-30, -20), M), IRect::new(70, 80, 300, 200));
        // dragging the right edge past the left one flips the rectangle
        assert_eq!(drag_grip(s, Grip::E, (-250, 0), M), IRect::new(50, 100, 100, 200));
        // the bottom edge stops at the monitor
        assert_eq!(drag_grip(s, Grip::S, (0, 5000), M), IRect::new(100, 100, 300, 1440));
        assert_eq!(drag_grip(s, Grip::Move, (-500, 3000), M), IRect::new(0, 1340, 200, 1440));
        // a selection bigger than the monitor stays pinned to its corner
        assert_eq!(IRect::new(-10, -10, 3000, 1500).moved_into(M), IRect::new(0, 0, 3010, 1510));
    }

    #[test]
    fn a_recording_area_has_even_sides() {
        assert_eq!(IRect::new(10, 20, 651, 381).even(), IRect::new(10, 20, 650, 380));
        assert_eq!(IRect::new(0, 0, 640, 360).even(), IRect::new(0, 0, 640, 360));
    }

    #[test]
    fn the_monitor_of_a_point_and_the_window_under_it() {
        let mons = [Mon { rect: M, scale: 1.0 }, Mon { rect: M2, scale: 1.0 }];
        assert_eq!(mon_at(&mons, (10, 10)), 0);
        assert_eq!(mon_at(&mons, (3000, 500)), 1);
        assert_eq!(mon_at(&mons, (3000, 1300)), 1, "below the shorter monitor: the nearest one");
        let wins = [Win { rect: IRect::new(100, 100, 500, 400), title: "front".into(), hwnd: 0 }, Win { rect: IRect::new(0, 0, 2560, 1440), title: "back".into(), hwnd: 0 }];
        assert_eq!(window_at(&wins, (200, 200)), Some(0));
        assert_eq!(window_at(&wins, (600, 600)), Some(1));
        assert_eq!(window_at(&wins, (3000, 600)), None);
    }

    #[test]
    fn the_choices_survive_a_round_trip_and_bad_lines_keep_the_defaults() {
        let o = Opts { kind: Kind::Rec, mode: Mode::Window, clip: false, save: true, notify: false, delay: 5, mic: false, countdown: 0 };
        assert_eq!(Opts::parse(&o.to_text()), o);
        let odd = Opts::parse("kind=rec\ndelay=7\nclip=maybe\nnonsense\nmode=full\ncountdown=5");
        assert_eq!((odd.kind, odd.mode, odd.delay, odd.clip, odd.countdown), (Kind::Rec, Mode::Full, 0, true, 5));
        assert_eq!(Opts::parse(""), Opts::default());
    }

    #[test]
    fn the_popover_works_on_the_switches_and_the_wait_of_the_current_kind() {
        let mut o = Opts::default();
        assert_eq!(o.switches().len(), 3);
        o.toggle(1);
        o.set_wait(5);
        assert!(!o.save && o.delay == 5 && o.wait() == 5);
        o.kind = Kind::Rec;
        assert_eq!(o.switches().len(), 1);
        o.toggle(0);
        o.set_wait(0);
        assert!(!o.mic && o.countdown == 0 && o.delay == 5, "the other kind's choices stay");
    }

    fn picker(mode: Mode) -> Snip {
        let mons = vec![Mon { rect: M, scale: 1.0 }, Mon { rect: M2, scale: 1.25 }];
        let wins = vec![Win { rect: IRect::new(100, 100, 700, 500), title: "A".into(), hwnd: 0 }, Win { rect: IRect::new(2800, 100, 3400, 600), title: "B".into(), hwnd: 0 }];
        Snip::new(mons, wins, Opts { mode, ..Opts::default() }, 0, 0)
    }

    fn text_w(s: &str, f: Font) -> f32 {
        s.chars().count() as f32 * f.size * 0.55
    }

    #[test]
    fn dragging_a_region_selects_it_then_the_handles_and_the_body_edit_it() {
        let mut s = picker(Mode::Region);
        assert_eq!(s.press((100, 100), Hit::Canvas, 0), Act::None);
        s.move_to((400, 300), 1);
        assert_eq!(s.target(), Some(IRect::new(100, 100, 400, 300)));
        assert!(!s.ready(), "still dragging: no action bar");
        s.release(2);
        assert!(s.ready());
        // the east handle
        assert_eq!(s.grip_at((400, 200)), Some(Grip::E));
        s.press((400, 200), Hit::Canvas, 3);
        s.move_to((500, 260), 4);
        s.release(5);
        assert_eq!(s.target(), Some(IRect::new(100, 100, 500, 300)));
        // inside it moves
        assert_eq!(s.grip_at((300, 200)), Some(Grip::Move));
        s.press((300, 200), Hit::Canvas, 6);
        s.move_to((310, 210), 7);
        s.release(8);
        assert_eq!(s.target(), Some(IRect::new(110, 110, 510, 310)));
        // a click outside starts afresh; one that drags nothing selects nothing
        s.press((900, 900), Hit::Canvas, 9);
        s.release(10);
        assert_eq!(s.target(), None);
    }

    #[test]
    fn a_selection_starts_on_the_monitor_where_it_was_pressed_and_the_toolbar_follows() {
        let mut s = picker(Mode::Region);
        s.press((3000, 200), Hit::Canvas, 0);
        s.move_to((200, 900), 1);
        s.release(2);
        assert_eq!(s.active, 1);
        assert_eq!(s.target(), Some(IRect::new(2560, 200, 3000, 900)));
    }

    #[test]
    fn window_mode_takes_the_window_under_the_pointer_and_full_screen_the_monitor() {
        let mut s = picker(Mode::Window);
        s.move_to((200, 200), 0);
        assert_eq!(s.showing(0), Some(IRect::new(100, 100, 700, 500)));
        assert_eq!(s.label_text(), "A · 600 × 400");
        s.move_to((1500, 900), 1);
        assert_eq!(s.showing(1), None);
        s.move_to((3000, 300), 2);
        assert_eq!(s.press((3000, 300), Hit::Canvas, 3), Act::Capture { copy_only: false });
        assert_eq!(s.target(), Some(IRect::new(2800, 100, 3400, 600)));
        let mut f = picker(Mode::Full);
        f.move_to((3000, 300), 0);
        assert_eq!(f.press((3000, 300), Hit::Canvas, 1), Act::Capture { copy_only: false });
        assert_eq!(f.target(), Some(M2));
    }

    #[test]
    fn the_keys_and_the_controls_do_what_the_mockup_does() {
        let mut s = picker(Mode::Region);
        assert_eq!(s.key(Key::Char('v'), 0), Act::None);
        assert_eq!(s.opts.kind, Kind::Rec);
        s.key(Key::Char('F'), 1);
        assert_eq!((s.opts.mode, s.target()), (Mode::Full, Some(M)));
        s.key(Key::Char('w'), 2);
        assert_eq!(s.target(), None);
        s.press((0, 0), Hit::Ctl(Ctl::Gear), 3);
        assert!(s.opts_open);
        assert_eq!(s.key(Key::Esc, 4), Act::None, "Esc closes the popover first");
        assert!(!s.opts_open);
        assert_eq!(s.key(Key::Esc, 5), Act::Cancel);
        assert_eq!(s.press((0, 0), Hit::Ctl(Ctl::Close), 6), Act::Cancel);
        s.key(Key::Char('f'), 7);
        assert_eq!(s.key(Key::Enter, 8), Act::Capture { copy_only: false });
        assert_eq!(s.press((0, 0), Hit::Ctl(Ctl::Copy), 9), Act::Capture { copy_only: true });
    }

    #[test]
    fn the_chrome_stands_where_the_mockup_puts_it() {
        let mut s = picker(Mode::Region);
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        // the toolbar: 550 x 48, 16 down, in the middle of a 2560 wide monitor
        assert_eq!(lay.tool, Some(Rect::new(1005.0, 16.0, 550.0, 48.0)));
        assert_eq!(lay.rect_of(Ctl::Kind(Kind::Shot)), Some(Rect::new(1014.0, 23.0, 40.0, 34.0)));
        assert_eq!(lay.rect_of(Ctl::Mode(Mode::Window)), Some(Rect::new(1005.0 + 109.0 + 3.0 + 112.0, 23.0, 112.0, 34.0)));
        assert_eq!(lay.rect_of(Ctl::Gear), Some(Rect::new(1005.0 + 468.0, 22.0, 36.0, 36.0)));
        assert_eq!(lay.hit(1070.0, 30.0), Hit::Ctl(Ctl::Kind(Kind::Rec)));
        assert_eq!(lay.hit(10.0, 500.0), Hit::Canvas);
        assert_eq!(s.layout(1, &mut |t, f| text_w(t, f)).tool, None, "the other monitor has no toolbar");
        // a selection: the action bar below it, the size label under that
        s.press((100, 100), Hit::Canvas, 0);
        s.move_to((400, 300), 1);
        s.release(2);
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        let bar = lay.bar.expect("the action bar");
        assert_eq!((bar.y, bar.h), (312.0, 40.0));
        assert_eq!(lay.label.map(|l| l.y), Some(360.0));
        assert!(lay.rect_of(Ctl::Copy).is_some() && lay.rect_of(Ctl::Go).is_some() && lay.rect_of(Ctl::Cancel).is_some());
        // a selection at the bottom puts the bar above it
        s.press((100, 1300), Hit::Canvas, 3);
        s.move_to((400, 1400), 4);
        s.release(5);
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        assert_eq!(lay.bar.map(|b| b.y), Some(1248.0));
        // the popover opens under the toolbar, right-aligned to it, 251 high with three switches
        s.opts_open = true;
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        let pop = lay.pop.expect("the popover");
        assert_eq!((pop.x + pop.w, pop.y, pop.h), (1555.0, 72.0, 251.0));
    }

    #[test]
    fn the_control_makes_room_for_the_hours() {
        let mut w = |t: &str| pill_width(&mut |s, f| text_w(s, f), true, t);
        assert_eq!((w("0:07"), w("9:59")), (w("0:00"), w("0:00")));
        assert_eq!(w("12:34"), w("0:00"), "the box has a minimum that holds minutes");
        assert!(w("1:02:03") > w("0:00"), "hours do not fit it");
        assert_eq!(w("1:02:03"), w("9:59:59"));
    }

    #[test]
    fn the_toolbar_on_a_narrow_monitor_keeps_its_close_button_on_screen() {
        let mons = vec![Mon { rect: IRect::new(0, 0, 1000, 800), scale: 2.0 }]; // 500 DIPs wide: narrower than the 550 toolbar
        let s = Snip::new(mons, vec![], Opts::default(), 0, 0);
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        let close = lay.rect_of(Ctl::Close).expect("close");
        assert!(close.x >= 0.0 && close.x + close.w <= 500.0, "{close:?}");
    }

    #[test]
    fn the_countdown_shows_each_second_and_fires_when_the_last_one_is_gone() {
        let mut c = Countdown::new(3);
        assert_eq!(c.shown(), 3);
        assert!(!c.tick());
        assert_eq!(c.shown(), 2);
        assert!(!c.tick());
        assert!(c.tick(), "the third second ends it");
        assert!(Countdown::new(0).tick(), "no wait fires at the first tick");
        // the choices of the popover are what start it: 0, 3 or 5 seconds, each kind with its own
        let mut o = Opts::default();
        o.set_wait(5);
        assert_eq!(Countdown::new(o.wait()).shown(), 5);
        o.kind = Kind::Rec;
        assert_eq!(Countdown::new(o.wait()).shown(), 3, "a recording counts down 3 s by default");
    }

    #[test]
    fn the_clock_text_is_short_and_the_meter_follows_the_level() {
        assert_eq!(clock_text(24), "0:24");
        assert_eq!(clock_text(75), "1:15");
        assert_eq!(clock_text(3723), "1:02:03");
        let silent = meter_bars(0.0, 1.0);
        assert!(silent.iter().all(|&b| b == 0.0));
        assert!(meter_bars(1.0, 0.3).iter().all(|&b| (0.0..=1.0).contains(&b) && b > 0.4));
    }

    #[test]
    fn the_recording_control_goes_under_the_area_only_when_the_area_touches_the_top() {
        assert_eq!(pill_top(Rect::new(300.0, 120.0, 840.0, 560.0), 900.0, false), 16.0);
        assert_eq!(pill_top(Rect::new(300.0, 0.0, 840.0, 400.0), 900.0, false), 414.0);
        assert_eq!(pill_top(Rect::new(0.0, 0.0, 1440.0, 900.0), 900.0, true), 16.0);
    }

    // ---- the strip of windows

    fn raw(hwnd: isize, title: &str, r: (i32, i32, i32, i32), minimized: bool) -> RawWin {
        RawWin { hwnd, title: title.into(), rect: IRect::new(r.0, r.1, r.2, r.3), minimized }
    }

    /// Two monitors (2560 x 1440 and 1920 x 1080) and these windows, topmost first.
    fn desktop() -> Vec<RawWin> {
        vec![
            raw(1, "P", (0, 600, 100, 1000), false),       // a small window in front of G: 10 % of it
            raw(2, "A", (100, 100, 700, 500), false),      // in sight
            raw(3, "B", (200, 150, 600, 450), false),      // behind A, wholly
            raw(4, "C", (300, 300, 900, 700), true),       // minimized
            raw(5, "D", (2800, 100, 3400, 600), false),    // on the other monitor
            raw(6, "E", (1000, 100, 1800, 800), false),    // in sight
            raw(7, "F", (1400, 100, 2200, 800), false),    // half of it behind E
            raw(8, "G", (0, 600, 1000, 1000), false),      // 90 % in sight
        ]
    }

    fn titles(strip: &[PickWin]) -> Vec<&str> {
        strip.iter().map(|w| w.title.as_str()).collect()
    }

    #[test]
    fn the_strip_lists_what_is_in_sight_then_covered_then_minimized_then_other_monitors() {
        let mons = [Mon { rect: M, scale: 1.0 }, Mon { rect: M2, scale: 1.0 }];
        let strip = strip_of(&desktop(), &mons, 0);
        assert_eq!(titles(&strip), ["P", "A", "E", "G", "B", "F", "C", "D"], "z-order inside each group");
        let at: Vec<Where> = strip.iter().map(|w| w.at).collect();
        assert_eq!(at[..4], [Where::Visible; 4]);
        assert_eq!(at[4..], [Where::Covered, Where::Covered, Where::Minimized, Where::Monitor(1)]);
        assert_eq!(Where::Visible.tag(), None);
        assert!(Where::Monitor(1).tag().is_some_and(|t| t.ends_with('2')), "the tag counts monitors from 1");
        // seen from the other monitor: its own window is in sight, the rest is "Monitor 1" (a minimized one stays minimized)
        let from_2 = strip_of(&desktop(), &mons, 1);
        assert_eq!(titles(&from_2), ["D", "C", "P", "A", "B", "E", "F", "G"]);
        assert_eq!((from_2[0].at, from_2[1].at, from_2[2].at), (Where::Visible, Where::Minimized, Where::Monitor(0)));
    }

    #[test]
    fn a_window_mostly_off_the_monitor_counts_by_what_is_on_it_and_one_wholly_off_is_covered() {
        let mons = [Mon { rect: M, scale: 1.0 }];
        let wins = [raw(1, "half", (2300, 100, 2820, 500), false), raw(2, "gone", (-900, 100, -300, 500), false)];
        let at: Vec<Where> = strip_of(&wins, &mons, 0).iter().map(|w| w.at).collect();
        assert_eq!(at, [Where::Visible, Where::Covered]);
    }

    #[test]
    fn a_picture_is_cut_for_the_card_from_its_top_left_and_averaged() {
        // a wide window: scaled to the card's height, its right part is cut off (the card shows its first 280 columns)
        let (w, h) = (560, 168);
        let px: Vec<u32> = (0..w * h).map(|i| if (i % w) < 140 { 0xff00_00ff } else { 0xffff_0000 }).collect();
        let t = thumb_of(&px, w, h, 1.0).expect("a thumb");
        assert_eq!((t.w, t.h, t.px.len()), (140, 84, 140 * 84));
        assert_eq!(t.px[0], 0xff00_00ff, "the top left corner of the window is the card's");
        // two window columns to a card column: the colour changes at the card's column 70
        assert_eq!((t.px[69], t.px[70]), (0xff00_00ff, 0xffff_0000));
        // averaged: stripes of black and white are grey, not one of them
        let stripes: Vec<u32> = (0..280 * 168).map(|i| if (i % 280) % 2 == 0 { 0xff00_0000 } else { 0xffff_ffff }).collect();
        let g = thumb_of(&stripes, 280, 168, 1.0).expect("a thumb");
        let c = g.px[10] & 255;
        assert!((120..=135).contains(&c), "{c}");
        // a small window is blown up to cover the card, and a scale of 1.5 makes a picture of 210 x 126 pixels
        let tiny = thumb_of(&[0xff12_3456; 4], 2, 2, 1.5).expect("a thumb");
        assert_eq!((tiny.w, tiny.h, tiny.px[0]), (210, 126, 0xff12_3456));
        assert!(thumb_of(&[], 0, 0, 1.0).is_none() && thumb_of(&[0; 3], 2, 2, 1.0).is_none());
    }

    /// A picker on a 1440 x 900 monitor with `n` windows in sight in its strip (more than 7 do not fit).
    fn strip_picker(n: usize, kind: Kind) -> Snip {
        let mons = vec![Mon { rect: IRect::new(0, 0, 1440, 900), scale: 1.0 }];
        let raws: Vec<RawWin> = (0..n).map(|i| raw(i as isize + 1, &format!("w{i}"), (10 + i as i32 * 60, 100, 50 + i as i32 * 60, 140), false)).collect();
        let wins: Vec<Win> = raws.iter().map(|r| Win { rect: r.rect, title: r.title.clone(), hwnd: r.hwnd }).collect();
        let mut s = Snip::new(mons.clone(), wins, Opts { mode: Mode::Window, kind, ..Opts::default() }, 0, 0);
        s.set_strip(strip_of(&raws, &mons, 0), 0);
        s
    }

    #[test]
    fn the_strip_stands_where_the_mockup_puts_it() {
        let lay = strip_picker(7, Kind::Shot).layout(0, &mut |t, f| text_w(t, f));
        // seven cards: 24 + 7 * 150 + 6 * 8 wide, a 32 high head, 117 high cards, 20 above the bottom edge
        assert_eq!(lay.strip, Some(Rect::new(159.0, 709.0, 1122.0, 171.0)));
        assert_eq!(lay.cards[0], (0, Rect::new(171.0, 751.0, 150.0, 117.0)));
        assert_eq!(lay.cards[1].1.x, 171.0 + 158.0);
        assert_eq!(lay.scroll_max, 0.0);
        assert_eq!(lay.hit(200.0, 800.0), Hit::Ctl(Ctl::Pick(0)));
        assert_eq!(lay.hit(163.0, 800.0), Hit::Ctl(Ctl::Strip), "the strip's padding takes the press and does nothing");
        assert_eq!(lay.hit(700.0, 300.0), Hit::Canvas);
        // eleven do not fit: the strip stops at 1160, grows a scrollbar, and the wheel moves the cards
        let mut s = strip_picker(11, Kind::Shot);
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        assert_eq!(lay.strip, Some(Rect::new(140.0, 699.0, 1160.0, 181.0)));
        assert_eq!(lay.scroll_max, 11.0 * 150.0 + 10.0 * 8.0 - 1136.0);
        s.scroll_by(100.0);
        assert_eq!(s.layout(0, &mut |t, f| text_w(t, f)).cards[0].1.x, 152.0 - 100.0);
        s.scroll_by(10_000.0);
        let lay = s.layout(0, &mut |t, f| text_w(t, f));
        assert_eq!(lay.cards[10].1.x + 150.0, 152.0 + 1136.0, "the last card ends at the end of the view");
        s.scroll_by(-10_000.0);
        assert_eq!(s.layout(0, &mut |t, f| text_w(t, f)).cards[0].1.x, 152.0);
    }

    #[test]
    fn a_card_under_the_pointer_outlines_its_window_and_lights_up_and_a_click_picks_it() {
        let mut s = strip_picker(5, Kind::Shot);
        assert!(s.strip_on());
        s.move_to((700, 400), 1); // the screen under the strip: nothing there
        s.point_at(Some(Ctl::Pick(2)), 2);
        assert!(s.over_strip());
        assert_eq!(s.showing(2), Some(IRect::new(130, 100, 170, 140)), "w2's frame");
        assert_eq!(s.label_text(), "w2 · 40 × 40");
        assert!((s.val(Ani::Lit(2), 1000) - 1.0).abs() < 1e-3 && s.val(Ani::Lit(1), 1000) < 1e-3);
        // the screen under the strip is not pointed at while the pointer is on it
        s.move_to((70, 120), 3);
        assert_eq!(s.hover, None);
        assert_eq!(s.press((200, 800), Hit::Ctl(Ctl::Pick(3)), 4), Act::Window(3));
        s.release(5);
        // pointing at a window on the screen lights its card; the card is the one of that window (the strip is in z-order here)
        s.point_at(None, 6);
        s.move_to((190, 120), 7);
        assert_eq!(s.hover, Some(3));
        assert!((s.val(Ani::Lit(3), 1000) - 1.0).abs() < 1e-3 && s.val(Ani::Lit(2), 1000) < 1e-3);
        // a card chosen is the selection and the strip goes away with it
        s.choose(IRect::new(130, 100, 170, 140), 8);
        assert!(!s.strip_on() && s.ready());
        assert_eq!(s.target(), Some(IRect::new(130, 100, 170, 140)));
    }

    #[test]
    fn the_strip_comes_up_in_window_mode_only_and_goes_with_the_popover_and_the_countdown() {
        let mut s = strip_picker(3, Kind::Shot);
        assert!(s.strip_on());
        s.set_mode(Mode::Region, 1);
        assert!(!s.strip_on());
        s.set_mode(Mode::Window, 2);
        assert!(s.strip_on() && s.busy(100), "it slides in over .28 s");
        assert!(!s.busy(10_000));
        s.press((0, 0), Hit::Ctl(Ctl::Gear), 3);
        assert!(!s.strip_on());
        assert_eq!(s.key(Key::Esc, 4), Act::None);
        assert!(s.strip_on());
        s.counting = true;
        assert!(!s.strip_on());
        s.counting = false;
        // no windows: nothing to pick for a screenshot
        let mut none = strip_picker(0, Kind::Shot);
        assert!(!none.strip_on());
        assert_eq!(none.layout(0, &mut |t, f| text_w(t, f)).strip, None);
        // a recording offers the whole screen instead
        none.set_kind(Kind::Rec, 5);
        assert!(none.strip_on());
        let lay = none.layout(0, &mut |t, f| text_w(t, f));
        let to_full = lay.rect_of(Ctl::ToFull).expect("the button");
        assert_eq!((lay.strip.map(|r| r.h), to_full.h), (Some(56.0), 30.0));
        assert_eq!(none.press((0, 0), Hit::Ctl(Ctl::ToFull), 6), Act::None);
        assert_eq!((none.opts.mode, none.target()), (Mode::Full, Some(IRect::new(0, 0, 1440, 900))));
    }

    #[test]
    fn a_recording_takes_what_is_on_this_screen_only() {
        let mons = vec![Mon { rect: M, scale: 1.0 }, Mon { rect: M2, scale: 1.0 }];
        let strip = strip_of(&desktop(), &mons, 0);
        let mut s = Snip::new(mons, Vec::new(), Opts { mode: Mode::Window, ..Opts::default() }, 0, 0);
        s.set_strip(strip, 0);
        let n = |s: &Snip| s.layout(0, &mut |t, f| text_w(t, f)).cards.len();
        assert_eq!(n(&s), 8);
        s.set_kind(Kind::Rec, 1);
        assert_eq!(n(&s), 7, "the window of the other monitor is not offered");
        s.set_kind(Kind::Shot, 2);
        assert_eq!(n(&s), 8);
    }
}
