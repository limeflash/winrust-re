//! The screenshot picker (Win+Shift+S) and the screen-recording overlay (Win+Shift+R) of the Obsidian mockup, without windows:
//! the selection model, the layout of the chrome and the painting, all in DIPs of one monitor (`Mon::scale` device pixels each).
//! `snip.rs` (the host) puts them into windows; `--render-panel shot|shot-window|shot-opts|shot-count|rec-pick|rec-live` into PNGs.
//!
//! The model is in device pixels of the virtual desktop, the chrome in DIPs: the mockup's numbers (toolbar 550 x 48 at the top
//! middle, handles 10 x 10, the action bar 40 high, ...) are used as they are.

use std::collections::HashMap;

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
    }

    /// The window under `p` as the highlight shows it: its frame, kept to the monitor the pointer is on.
    fn win_rect(&self, i: usize, p: (i32, i32)) -> Option<IRect> {
        self.wins.get(i)?.rect.intersect(self.mon_rect(mon_at(&self.mons, p)))
    }

    /// Where the highlight of Window mode is going, in device pixels.
    fn hover_rect(&self) -> Option<IRect> {
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
            Mode::Window => {
                let h = window_at(&self.wins, p).filter(|&i| self.win_rect(i, p).is_some());
                if h != self.hover || h.is_some() {
                    self.hover = h;
                    if let Some(r) = self.hover_rect() {
                        self.glide(r, now);
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
        if self.opts.mode != Mode::Window || self.hover.is_none() {
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
            Hit::Ctl(c) => return self.click(c, now),
            Hit::Chrome => return Act::None,
            Hit::Canvas => {}
        }
        if self.opts_open {
            self.opts_open = false;
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
            }
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
            Key::Esc if self.opts_open => self.opts_open = false,
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
                let ly = if sd.y + sd.h + 36.0 < h && self.opts.mode != Mode::Full {
                    sd.y + sd.h + if ready && self.opts.mode == Mode::Region { 60.0 } else { 10.0 }
                } else {
                    (sd.y + 8.0).max(8.0)
                };
                lay.label = Some(Rect::new(sd.x, ly, lw, 24.0));
        }
        lay
    }

    fn label_text(&self) -> String {
        let Some(s) = self.sel.or_else(|| self.hover_rect()) else { return String::new() };
        let size = format!("{} × {}", s.w(), s.h());
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
                c.fill(Rect::new(0.0, y, w, 1.0), 0.0, Rgba::white(0.35));
                c.fill(Rect::new(x, 0.0, 1.0, h), 0.0, Rgba::white(0.35));
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
                c.dashed_rect(r.inset(-0.25), 1.5, 3.0, 2.0, Rgba::white(0.9));
            }
            if self.editable() {
                for (_, fx, fy) in HANDLES {
                    let (hx, hy) = (r.x + r.w * fx, r.y + r.h * fy);
                    let hr = Rect::new(hx - 5.0, hy - 5.0, 10.0, 10.0);
                    c.outside(hr, 3.0, |c| c.shadow(hr, 3.0, 2.0, 6.0, Rgba::css(0, 0, 0, 0.4)));
                    c.fill(hr.inset(-1.5), 4.0, acc);
                    c.fill(hr, 3.0, Rgba::white(1.0));
                }
            }
            if let Some(at) = self.flash {
                let t = prog(at, now, 320);
                // `shFlash`: 0 -> .75 -> 0, `ease` for each part
                let a = if t < 0.25 { EASE.ease(t / 0.25) * 0.75 } else { 0.75 * (1.0 - EASE.ease((t - 0.25) / 0.75)) };
                c.fill(r, if window_look { 14.0 } else { 0.0 }, Rgba::white(a));
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
                let text = hint_text(self.opts.kind, self.opts.mode);
                let hw = c.measure(text, F_HINT) + 24.0;
                let r = Rect::new(((w - hw) / 2.0).round(), 74.0, hw, 28.0);
                c.fill(r, 14.0, Rgba::css(12, 13, 18, 0.7));
                c.text(text, r, F_HINT, SOFT_INK, Align::Center);
            }
            let p = SNAP.ease(prog(self.tool_at, now, 300));
            let k = 0.97 + 0.03 * p;
            c.faded(p, |c| c.transformed(tool.cx(), tool.cy(), k, 0.0, -10.0 * (1.0 - p), |c| self.paint_tool(c, lay, tool, now)));
        }
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
            let ink = if self.opts.kind == Kind::Rec { Rgba::white(1.0) } else { Rgba::hex(0x0b1220) };
            let bg = if self.opts.kind == Kind::Rec { RED } else { acc };
            c.fill(r, 9.0, bg);
            if hov > 0.001 {
                c.fill(r, 9.0, Rgba::white(0.10 * hov)); // `filter: brightness(1.1)`
            }
            if self.opts.kind == Kind::Rec {
                c.circle(r.x + 10.0 + 5.0, r.cy(), 5.0, Rgba::white(1.0));
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
            let ink = DIM_INK.mix(Rgba::white(1.0), self.val(Ani::Tint(Ctl::Kind(k)), now));
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
            let ink = DIM_INK.mix(Rgba::white(1.0), self.val(Ani::Tint(Ctl::Mode(md)), now));
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
            c.circle(track.x + 2.0 + 8.0 + 14.0 * k, track.cy(), 8.0, Rgba::white(1.0));
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
            c.text(wait_label(n, self.opts.kind), r, F_SEG, if on { Rgba::white(1.0) } else { DIM_INK }, Align::Center);
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
            c.text(&n.to_string(), r, Font::new(56.0, 600), Rgba::white(1.0), Align::Center);
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
                c.fill(Rect::new(stop.x + 10.0, stop.cy() - 5.0, 10.0, 10.0), 2.5, Rgba::white(1.0));
                c.text(stop_label(), Rect::new(stop.x + 27.0, stop.y, stop.w, stop.h), F_GO, Rgba::white(1.0), Align::Start);
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
        let wins = [Win { rect: IRect::new(100, 100, 500, 400), title: "front".into() }, Win { rect: IRect::new(0, 0, 2560, 1440), title: "back".into() }];
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
        let wins = vec![Win { rect: IRect::new(100, 100, 700, 500), title: "A".into() }, Win { rect: IRect::new(2800, 100, 3400, 600), title: "B".into() }];
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
}
