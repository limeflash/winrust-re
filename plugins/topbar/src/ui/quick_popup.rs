//! The quick settings panel (the network, volume and battery buttons of the bar), and the Wi-Fi page that opens inside it.
//!
//! The first page is the mockup's: a 3x3 grid of switches, the sound card (volume, and brightness on a laptop), the player and
//! the footer with the battery and the gear. A tile fills with the accent from its chip outwards when it goes on; the Wi-Fi
//! tile has a chevron that leads to the second page.
//!
//! The Wi-Fi page is not in the mockup. It is a page of the same panel, in the manner of the bar settings' pages: a back
//! button, a title with a switch, the connected network on its own card, the other networks as rows (an open network joins at
//! once, a saved one connects, a new one asks for its password right on the page), a way into Windows' network settings, and
//! a context menu on a right click (connect or disconnect, forget). The page slides in like `pgF` and back like `pgB`.
//!
//! The panel owns the look and the input; what is on and what a click does belongs to a [`Host`].

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use super::anim::{self, CubicBezier, EASE, SNAP, SPRING, Transition};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::icons;
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Frame, Id, Look, Origin, Place, Shadow};
use winrust_common::style::{self, Side, pal};

const W: f32 = 380.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const TILE_H: f32 = 86.0;
const INNER: f32 = W - 2.0 * PAD;

// the ids of what can be hit
const MORE: i32 = 9;
const MUTE: i32 = 20;
const VOL: i32 = 21;
const BRIGHT: i32 = 22;
const DEVICE: i32 = 23;
const MEDIA: i32 = 24; // 24 previous, 25 play / pause, 26 next
const GEAR: i32 = 27;
const DEV: i32 = 300; // 300.. the outputs, 340.. the inputs
const SOUND_MORE: i32 = 380;
const BACK: i32 = 40;
const SWITCH: i32 = 41;
const NETWORKS: i32 = 42;
const DISCONNECT: i32 = 43;
const DISMISS: i32 = 44;
const NET: i32 = 60; // 60.. the networks in the order they are listed
const MENU: i32 = 90; // 90.. the items of the context menu

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const DIM: Rgba = Rgba::css(163, 169, 184, 1.0);
const DIM2: Rgba = Rgba::css(138, 144, 162, 1.0);
const ON_ACCENT: Rgba = Rgba::css(11, 18, 32, 1.0);
const MUTED: Rgba = Rgba::css(255, 154, 154, 1.0);
const RED: Rgba = Rgba::css(255, 69, 58, 1.0);
const GREEN: Rgba = Rgba::css(48, 209, 88, 1.0);
const AMBER: Rgba = Rgba::css(255, 214, 10, 1.0);

/// The mockup's `cubic-bezier(.32,.72,0,1)`: the strip and the list of devices changing places.
const SHEET: CubicBezier = CubicBezier::new(0.32, 0.72, 0.0, 1.0);

/// `ease-out`, what the slider's `transition: width .12s ease-out` uses.
const EASE_OUT: CubicBezier = CubicBezier::new(0.0, 0.0, 0.58, 1.0);

const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";
const CHEVRON_LEFT: &str = "M15 5l-7 7 7 7";
const CHEVRON_DOWN: &str = "M6 9l6 6 6-6";
const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const LOCK: &str = "M7 11V8a5 5 0 0 1 10 0v3M5 11h14v9H5z";
const GLOBE: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3c2.5 2.6 3.7 5.6 3.7 9s-1.2 6.4-3.7 9c-2.5-2.6-3.7-5.6-3.7-9S9.5 5.6 12 3z";
const SUN: &str = "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M18.7 5.3l-1.4 1.4M6.7 17.3l-1.4 1.4";
const SPEAKER_0: &str = "M4 9.5h3.5L12 6v12l-4.5-3.5H4zM16 9.5l5 5M21 9.5l-5 5";
const SPEAKER_1: &str = "M4 9.5h3.5L12 6v12l-4.5-3.5H4zM15.5 9a4 4 0 0 1 0 6";
const SPEAKER_2: &str = "M4 9.5h3.5L12 6v12l-4.5-3.5H4zM15.5 9a4 4 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11";
const PREVIOUS: &str = "M19 5L9 12l10 7zM5 5v14";
const PLAY: &str = "M7 4.5v15l12-7.5z";
const PAUSE: &str = "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z";
const NEXT: &str = "M5 5l10 7-10 7zM19 5v14";
const HEADPHONES: &str = "M4 14v-2a8 8 0 0 1 16 0v2M4 14h3v6H5a1 1 0 0 1-1-1zM20 14h-3v6h2a1 1 0 0 0 1-1z";
const MONITOR: &str = "M3 4h18v12H3zM8 20h8M12 16v4";
const MICROPHONE: &str = "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3";

/// One tile's two icons (the off one is the same drawing but for Night light) and whether the on one is filled.
struct Icon {
    on: &'static str,
    off: &'static str,
    filled: bool,
}

const WIFI: &str = "M3 9.5a13 13 0 0 1 18 0M6 13a8.5 8.5 0 0 1 12 0M9 16.5a4 4 0 0 1 6 0M12 19.5h.01";
const BLUETOOTH: &str = "M7 7l10 10-5 4V3l5 4L7 17";
const AIRPLANE: &str = "M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z";
const ACCESS: &str = "M12 5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM5 8l7 1.5L19 8M12 9.5V15M9 21l3-6 3 6";
const SAVER: &str = "M13 2L5 13h6l-1 9 8-12h-6z";
const CAPTIONS: &str = "M3 6h18v12H3zM9.5 10.5a2 2 0 1 0 0 3M16 10.5a2 2 0 1 0 0 3";
const HOTSPOT: &str = "M12 13a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM8.5 16a5 5 0 0 1 0-8M15.5 8a5 5 0 0 1 0 8M5.5 18.5a9 9 0 0 1 0-13M18.5 5.5a9 9 0 0 1 0 13";
const MOON: &str = "M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7zM16.5 3.2l.55 1.25 1.25.55-1.25.55-.55 1.25-.55-1.25L14.7 5l1.25-.55zM20 8.2l.4.9.9.4-.9.4-.4.9-.4-.9-.9-.4.9-.4z";
const PROJECT: &str = "M3 5h18v11H3zM8 20h8M12 16v4";

const ICONS: [Icon; 9] = [
    Icon { on: WIFI, off: WIFI, filled: false },
    Icon { on: BLUETOOTH, off: BLUETOOTH, filled: false },
    Icon { on: AIRPLANE, off: AIRPLANE, filled: false },
    Icon { on: ACCESS, off: ACCESS, filled: false },
    Icon { on: SAVER, off: SAVER, filled: false },
    Icon { on: CAPTIONS, off: CAPTIONS, filled: false },
    Icon { on: HOTSPOT, off: HOTSPOT, filled: false },
    Icon { on: MOON, off: SUN, filled: true },
    Icon { on: PROJECT, off: PROJECT, filled: false },
];

fn label(i: usize) -> &'static str {
    [
        "Wi\u{2011}Fi",
        t!("Bluetooth", "Bluetooth", ja: "Bluetooth", pt: "Bluetooth", zh: "蓝牙"),
        t!("Авиарежим", "Airplane mode", ja: "機内モード", pt: "Modo avião", zh: "飞行模式"),
        t!("Спец. возможности", "Accessibility", ja: "アクセシビリティ", pt: "Acessibilidade", zh: "辅助功能"),
        t!("Экономия энергии", "Energy saver", ja: "省エネ", pt: "Economia de energia", zh: "节能模式"),
        t!("Субтитры", "Live captions", ja: "ライブ キャプション", pt: "Legendas ao vivo", zh: "实时字幕"),
        t!("Точка доступа", "Mobile hotspot", ja: "モバイル ホットスポット", pt: "Hotspot móvel", zh: "移动热点"),
        t!("Ночной свет", "Night light", ja: "夜間モード", pt: "Luz noturna", zh: "夜间模式"),
        t!("Проецировать", "Project", ja: "表示", pt: "Projetar", zh: "投影"),
    ][i]
}

// ---------------------------------------------------------------------------------------------------------------------
// what the panel shows and asks for

/// A tile's second line and whether it is on.
pub struct Tile {
    pub status: String,
    pub on: bool,
}

/// What plays now.
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

/// What a sound device is, for its icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Speakers,
    Headphones,
    Monitor,
    Microphone,
}

impl Kind {
    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Speakers => SPEAKER_2,
            Self::Headphones => HEADPHONES,
            Self::Monitor => MONITOR,
            Self::Microphone => MICROPHONE,
        }
    }
}

/// A sound device of the machine.
#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// The line under the name: the adapter and how it is connected.
    pub sub: String,
    pub kind: Kind,
    /// Windows uses it now.
    pub current: bool,
}

/// The active playback and recording devices.
#[derive(Default, Debug)]
pub struct Audio {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
}

/// Everything the first page shows.
pub struct State {
    pub tiles: Vec<Tile>,
    /// Volume in percent and whether it is muted; `None` when there is no sound device.
    pub vol: Option<(i32, bool)>,
    /// Brightness in percent; `None` where the screen has no brightness to set (a desktop PC).
    pub bright: Option<i32>,
    pub media: Option<Media>,
    /// Charge in percent and whether the charger is plugged in; `None` without a battery.
    pub battery: Option<(i32, bool)>,
    pub audio: Rc<Audio>,
}

/// A network in the list.
pub struct Network {
    pub ssid: String,
    /// Signal, 0..100.
    pub quality: i32,
    pub secure: bool,
    /// Windows has a profile for it.
    pub saved: bool,
    pub connected: bool,
}

/// The link of the connected network.
pub struct Link {
    /// "Wi-Fi 6".
    pub phy: String,
    pub down: u32,
    pub up: u32,
    /// "WPA2-Personal".
    pub auth: String,
}

/// How a status line is tinted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Dim,
    Warn,
}

/// Everything the second page shows.
pub struct Wifi {
    /// The radio is on (or networks are visible).
    pub on: bool,
    pub nets: Vec<Network>,
    pub link: Option<Link>,
    /// A line under the title: a note about the last action, or the state of the connection.
    pub status: Option<(String, Tone)>,
    /// The network being joined now.
    pub joining: Option<String>,
    /// Windows says there is no way out through the connected network.
    pub offline: bool,
}

/// What a click on a network did.
pub enum Join {
    Started,
    /// A new secured network: the page asks for the password.
    NeedPassword,
    /// An enterprise network the panel cannot join: Windows' own page was opened.
    Settings,
}

/// The data and the actions behind the panel (the machine's, or a demo's for the renderer).
pub trait Host {
    /// Called every frame; reads what changes by itself (at most once a second).
    fn tick(&mut self, wifi_page: bool);
    fn state(&mut self) -> State;
    /// A click on tile `i`.
    fn tile(&mut self, i: usize, cx: &mut Cx);
    fn volume(&mut self, percent: i32);
    fn mute(&mut self);
    /// Makes device `id` the one Windows plays through (`input` false) or records with.
    fn audio_set(&mut self, input: bool, id: &str);
    /// Brightness follows the knob; `done` once it is let go (the screen is set then).
    fn brightness(&mut self, percent: i32, done: bool);
    /// 0 previous, 1 play / pause, 2 next.
    fn media(&mut self, cmd: u8);
    /// A Windows settings page; the panel closes.
    fn settings(&mut self, uri: &'static str, cx: &mut Cx);
    /// The Wi-Fi page is opened: look around.
    fn wifi_open(&mut self);
    fn wifi(&mut self) -> Wifi;
    fn wifi_power(&mut self, on: bool);
    fn wifi_pick(&mut self, ssid: &str) -> Join;
    fn wifi_join(&mut self, ssid: &str, password: &str);
    fn wifi_leave(&mut self);
    fn wifi_forget(&mut self, ssid: &str);
}

// ---------------------------------------------------------------------------------------------------------------------
// geometry

/// How far the panel's two states have got into each other: `grid` is the strip the tiles stand in (0 the 3x3 grid, 1 the row
/// of icons), `tiles` each tile on its own clock, `list` the sound card's list of devices (0 hidden, 1 shown).
#[derive(Clone, Copy)]
struct Sheet {
    grid: f32,
    tiles: [f32; 9],
    list: f32,
}

impl Sheet {
    const CLOSED: Self = Self { grid: 0.0, tiles: [0.0; 9], list: 0.0 };
    const OPEN: Self = Self { grid: 1.0, tiles: [1.0; 9], list: 1.0 };
}

/// The rectangles of the first page.
struct Geo {
    tiles: [Rect; 9],
    /// The corner radius and the top padding of each tile (they follow the sheet: 16 and 10 in the grid, 11 and 2 in the row).
    radius: [f32; 9],
    pad_top: [f32; 9],
    sound: Rect,
    /// The part of the list that shows (its box grows from nothing, `grid-template-rows: 0fr` to `1fr`).
    wrap: Rect,
    device: Rect,
    mute: Rect,
    vol: Rect,
    vol_num: Rect,
    bright_icon: Rect,
    bright: Rect,
    bright_num: Rect,
    media: Rect,
    buttons: [Rect; 3],
    foot: Rect,
    gear: Rect,
    height: f32,
}

/// `list_h` is the height of the list when it is all shown (`Lines::height`).
fn geo(sh: Sheet, has_bright: bool, device_w: f32, list_h: f32) -> Geo {
    let tw = (INNER - 2.0 * GAP) / 3.0;
    let tiles = std::array::from_fn(|i| {
        let (grid, row) = (Rect::new(PAD + (i % 3) as f32 * (tw + GAP), PAD + (i / 3) as f32 * (TILE_H + GAP), tw, TILE_H), Rect::new(PAD + i as f32 * 40.0, PAD, 36.0, 36.0));
        let t = sh.tiles[i];
        Rect::new(anim::lerp(grid.x, row.x, t), anim::lerp(grid.y, row.y, t), anim::lerp(grid.w, row.w, t), anim::lerp(grid.h, row.h, t))
    });
    let radius = std::array::from_fn(|i| anim::lerp(16.0, 11.0, sh.tiles[i]));
    let pad_top = std::array::from_fn(|i| anim::lerp(10.0, 2.0, sh.tiles[i]));
    // the strip is 274 high as a grid and 36 as a row; the card follows it, 8 below
    let y = PAD + anim::lerp(3.0 * TILE_H + 2.0 * GAP, 36.0, sh.grid) + GAP;
    // the card: padding 10 12 12 and 8 between; the list's box takes its height and cancels its 8 of margin while it is empty
    let shown = (list_h + GAP) * sh.list;
    let sound_y = y + 10.0 + shown;
    let h = 10.0 + shown + 26.0 + GAP + 32.0 + if has_bright { GAP + 32.0 } else { 0.0 } + 12.0;
    let sound = Rect::new(PAD, y, INNER, h);
    let (x0, x1) = (sound.x + 12.0, sound.x + sound.w - 12.0);
    let wrap = Rect::new(x0, y + 10.0, x1 - x0, list_h * sh.list);
    let device = Rect::new(x1 + 6.0 - device_w, sound_y - 1.0, device_w, 28.0);
    // a row is 32 high (the mute button); the 28 px track and the value are centred in it
    let row = |top: f32| {
        let track = Rect::new(x0 - 4.0 + 32.0 + 10.0, top + 2.0, (x1 - 40.0 - 10.0) - (x0 - 4.0 + 32.0 + 10.0), 28.0);
        (Rect::new(x0 - 4.0, top, 32.0, 32.0), track, Rect::new(x1 - 40.0, top, 40.0, 32.0))
    };
    let (mute, vol, vol_num) = row(sound_y + 26.0 + GAP);
    let (bright_icon, bright, bright_num) = row(sound_y + 26.0 + GAP + 32.0 + GAP);
    let media = Rect::new(PAD, y + h + GAP, INNER, 58.0);
    let buttons = std::array::from_fn(|i| Rect::new(media.x + media.w - 8.0 - 40.0 * (3 - i) as f32, media.cy() - 20.0, 40.0, 40.0));
    let foot = Rect::new(PAD, media.y + media.h + GAP, INNER, 40.0);
    let gear = Rect::new(foot.x + foot.w - 40.0, foot.y, 40.0, 40.0);
    Geo { tiles, radius, pad_top, sound, wrap, device, mute, vol, vol_num, bright_icon, bright, bright_num, media, buttons, foot, gear, height: foot.y + foot.h + PAD }
}

/// One line of the list in the sound card.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Line {
    /// "Output" (true) or "Input".
    Title(bool),
    /// Device `n` of the outputs (true) or the inputs.
    Device(bool, usize),
    /// "Sound settings".
    More,
}

impl Line {
    fn id(self) -> i32 {
        match self {
            Self::Title(_) => -1,
            Self::Device(out, n) => DEV + if out { 0 } else { 40 } + n as i32,
            Self::More => SOUND_MORE,
        }
    }
}

/// The lines of the list from its top (`.audl`: 2 apart, a title 24, a device 40, the settings link 32 with 2 more above it,
/// 6 of padding and a hairline under it) and its height.
struct Lines {
    lines: Vec<(Line, f32, f32)>,
    /// The whole box, as `.audl` is.
    height: f32,
}

impl Lines {
    fn of(a: &Audio) -> Self {
        let (mut y, mut lines) = (0.0, Vec::new());
        let mut push = |l: Line, h: f32, y: &mut f32| {
            lines.push((l, *y, h));
            *y += h + 2.0;
        };
        for (out, list) in [(true, &a.outputs), (false, &a.inputs)] {
            if list.is_empty() {
                continue;
            }
            push(Line::Title(out), 24.0, &mut y);
            for n in 0..list.len() {
                push(Line::Device(out, n), 40.0, &mut y);
            }
        }
        y += 2.0;
        push(Line::More, 32.0, &mut y);
        Self { lines, height: y - 2.0 + 6.0 + 1.0 }
    }

    /// What the box of the list takes in the card: it starts 4 px above its wrapper (a margin of -4).
    fn shown(&self) -> f32 {
        self.height - 4.0
    }
}

/// The value (0..1) a pointer at `x` means on a 28 px track with a 24 px knob.
fn slider_value(track: Rect, x: f32) -> f32 {
    ((x - track.x - 14.0) / (track.w - 28.0)).clamp(0.0, 1.0)
}

/// The strip and the list changing places (`.qg`, `.audw`): when each part of it moves and how, as the mockup's CSS has it.
struct Slide {
    open: bool,
    grid: Transition,
    tiles: [Transition; 9],
    labels: [Transition; 9],
    list: Transition,
    fade: Transition,
    items: Transition,
    shift: Transition,
}

impl Slide {
    fn new() -> Self {
        Self {
            open: false,
            grid: Transition::new(0.0),
            tiles: [Transition::new(0.0); 9],
            labels: [Transition::new(1.0); 9],
            list: Transition::new(0.0),
            fade: Transition::new(0.0),
            items: Transition::new(0.0),
            shift: Transition::new(0.0),
        }
    }

    /// Opens or closes the list; `snap` does it at once.
    fn set(&mut self, open: bool, now: u64, snap: bool) {
        self.open = open;
        let to = f32::from(open);
        let go = |t: &mut Transition, to: f32, dur: u32, delay: u32, ease: CubicBezier| {
            if snap {
                t.snap(to);
            } else {
                let (dur, delay) = anim::timing(style::anim(), dur, delay);
                t.set(to, now, dur, delay, ease);
            }
        };
        go(&mut self.grid, to, 500, 0, SHEET);
        for i in 0..9 {
            // `--dl`: the row's icons set off one after another from the left, the grid's tiles from the bottom right
            let dl = if open { 16 * i as u32 } else { 12 * (8 - i as u32) };
            go(&mut self.tiles[i], to, 500, dl, SHEET);
            // the labels leave at once and come back last
            if open { go(&mut self.labels[i], 0.0, 120, 0, EASE) } else { go(&mut self.labels[i], 1.0, 280, dl + 200, EASE) }
        }
        if open {
            go(&mut self.list, 1.0, 500, 40, SHEET);
            go(&mut self.fade, 1.0, 300, 100, EASE);
        } else {
            go(&mut self.list, 0.0, 450, 0, SHEET);
            go(&mut self.fade, 0.0, 180, 0, EASE);
        }
        go(&mut self.items, to, 300, 0, EASE);
        go(&mut self.shift, to, 450, 0, SHEET);
    }

    fn sheet(&self, now: u64) -> Sheet {
        Sheet { grid: self.grid.value(now), tiles: std::array::from_fn(|i| self.tiles[i].value(now)), list: self.list.value(now) }
    }

    fn running(&self, now: u64) -> bool {
        [&self.grid, &self.list, &self.fade, &self.items, &self.shift].iter().any(|t| t.running(now)) || self.tiles.iter().chain(&self.labels).any(|t| t.running(now))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Main,
    Wifi,
}

thread_local! {
    static ON_WIFI: Cell<bool> = const { Cell::new(false) };
}

/// The panel in front shows its Wi-Fi page (a right click on the network icon asks).
#[must_use]
pub fn on_wifi_page() -> bool {
    ON_WIFI.get()
}

const MENU_W: f32 = 196.0;
const MENU_ROW: f32 = 34.0;

pub struct QuickPanel {
    host: Box<dyn Host>,
    side: Side,
    page: Page,
    /// The page that is sliding in: when it started (0 = not stamped yet) and from which side (+1 from the right).
    flip: Option<(u64, f32)>,
    bright: bool,
    st: State,
    wifi: Wifi,
    /// The network a password is typed for, and the password.
    pass: Option<(String, String)>,
    /// The network whose context menu is open.
    ctx: Option<String>,
    device_w: f32,
    back_w: f32,
    bright_v: i32,
    size: (f32, f32),
    slide: Slide,
    /// The button of the Sound card was pressed: the list opens or closes at the next frame (which knows the time).
    toggle: bool,
    /// The time of the frame being drawn, for the geometry that moves.
    now: u64,
    /// The Wi-Fi page: how many of the other networks the wheel has scrolled past, and the most it can (set by `layout`).
    scroll: Cell<usize>,
    max_scroll: Cell<usize>,
}

impl QuickPanel {
    /// `wifi`: open on the Wi-Fi page.
    #[must_use]
    pub fn new(mut host: Box<dyn Host>, side: Side, wifi: bool) -> Self {
        let st = host.state();
        if wifi {
            host.wifi_open();
        }
        let wf = host.wifi();
        ON_WIFI.set(wifi);
        let mut panel = Self {
            bright: st.bright.is_some(),
            bright_v: st.bright.unwrap_or(0),
            host,
            side,
            page: if wifi { Page::Wifi } else { Page::Main },
            flip: None,
            st,
            wifi: wf,
            pass: None,
            ctx: None,
            device_w: 90.0,
            back_w: 70.0,
            size: (W, 0.0),
            slide: Slide::new(),
            toggle: false,
            now: 0,
            scroll: Cell::new(0),
            max_scroll: Cell::new(0),
        };
        panel.size.1 = panel.geo().height;
        panel
    }

    /// The password prompt for `ssid` already open (the renderer shows it).
    #[must_use]
    pub fn asking(mut self, ssid: &str) -> Self {
        self.pass = Some((ssid.to_string(), "hunter2".into()));
        self
    }

    /// The context menu of `ssid` already open.
    #[must_use]
    pub fn with_menu(mut self, ssid: &str) -> Self {
        self.ctx = Some(ssid.to_string());
        self
    }

    fn go(&mut self, to: Page) {
        let dir = if to == Page::Wifi { 1.0 } else { -1.0 };
        if to == Page::Wifi {
            self.host.wifi_open();
        }
        self.page = to;
        self.slide.set(false, self.now, true); // the Wi-Fi page has the panel at its first size
        self.flip = Some((0, dir));
        self.pass = None;
        self.ctx = None;
        self.scroll.set(0);
        ON_WIFI.set(to == Page::Wifi);
    }

    // ---- the Wi-Fi page's rectangles ----

    fn footer(&self) -> Rect {
        Rect::new(PAD, self.geo().height - PAD - 40.0, INNER, 40.0)
    }

    fn back_rect(&self) -> Rect {
        Rect::new(PAD, PAD, self.back_w, 36.0)
    }

    fn switch_rect() -> Rect {
        Rect::new(PAD + INNER - 10.0 - 36.0, 59.0, 36.0, 20.0)
    }

    /// The connected card, the label of the others and their rows.
    fn layout(&self) -> (Option<Rect>, f32, Vec<(usize, Rect)>) {
        let mut y = PAD + 36.0 + 48.0;
        let nets = &self.wifi.nets;
        let card = nets.iter().position(|n| n.connected).map(|_| {
            let r = Rect::new(PAD, y, INNER, 60.0);
            y += 60.0 + GAP;
            r
        });
        let others: Vec<usize> = (0..nets.len()).filter(|&i| !nets[i].connected).collect();
        let label_y = y + 14.0;
        if !others.is_empty() {
            y = label_y + 15.0 + 6.0;
        }
        let room = ((self.footer().y - GAP - y + 1.0) / 45.0).max(0.0) as usize;
        // more networks than rows: the wheel moves the window over them (the rest could not be joined otherwise)
        self.max_scroll.set(others.len().saturating_sub(room));
        let skip = self.scroll.get().min(self.max_scroll.get());
        let rows = others.iter().skip(skip).take(room).enumerate().map(|(k, &i)| (i, Rect::new(PAD, y + k as f32 * 45.0, INNER, 44.0))).collect();
        (card, label_y, rows)
    }

    fn pass_rect() -> Rect {
        Rect::new(PAD, PAD + 36.0 + 48.0 + 30.0, INNER, 38.0)
    }

    /// The context menu: its box and the rectangle of each item.
    fn menu_rects(&self) -> Option<(Rect, Vec<Rect>)> {
        let ssid = self.ctx.as_ref()?;
        let n = self.menu_items(ssid).len() as f32;
        let (card, _, rows) = self.layout();
        let at = if self.wifi.nets.iter().any(|x| x.connected && &x.ssid == ssid) {
            card?
        } else {
            let (i, _) = self.wifi.nets.iter().enumerate().find(|(_, x)| &x.ssid == ssid)?;
            rows.iter().find(|(k, _)| *k == i)?.1
        };
        let h = 12.0 + MENU_ROW * n;
        let (x, below) = (at.x + at.w - MENU_W - 8.0, at.y + at.h - 6.0);
        let y = if below + h > self.geo().height - PAD { (at.y + 6.0 - h).max(PAD) } else { below };
        let r = Rect::new(x, y, MENU_W, h);
        Some((r, (0..n as usize).map(|j| Rect::new(x + 6.0, y + 6.0 + MENU_ROW * j as f32, MENU_W - 12.0, MENU_ROW)).collect()))
    }

    /// The context menu's items: (what it says, what it does: 0 connect, 1 disconnect, 2 forget, 3 known networks).
    fn menu_items(&self, ssid: &str) -> Vec<(&'static str, u8)> {
        let Some(n) = self.wifi.nets.iter().find(|n| n.ssid == ssid) else { return Vec::new() };
        let mut v = vec![if n.connected { (t!("Отключиться", "Disconnect", ja: "切断", pt: "Desconectar", zh: "断开连接"), 1) } else { (t!("Подключиться", "Connect", ja: "接続", pt: "Conectar", zh: "连接"), 0) }];
        if n.saved {
            v.push((t!("Забыть сеть", "Forget network", ja: "ネットワークを削除", pt: "Esquecer rede", zh: "忘记网络"), 2));
        }
        v.push((t!("Известные сети…", "Known networks…", ja: "既知のネットワーク…", pt: "Redes conhecidas…", zh: "已知网络…"), 3));
        v
    }

    fn pick(&mut self, ssid: &str, cx: &mut Cx) {
        match self.host.wifi_pick(ssid) {
            Join::Started => {}
            Join::NeedPassword => self.pass = Some((ssid.to_string(), String::new())),
            Join::Settings => cx.close(),
        }
    }
}

/// `s` as it fits in `w` DIPs, with an ellipsis if it does not.
fn fit<'a>(c: &mut Canvas<'_>, s: &'a str, font: Font, w: f32) -> Cow<'a, str> {
    if c.measure(s, font) <= w {
        return Cow::Borrowed(s);
    }
    let mut t = s.to_string();
    while t.pop().is_some() {
        let cand = format!("{}…", t.trim_end());
        if c.measure(&cand, font) <= w {
            return Cow::Owned(cand);
        }
    }
    Cow::Borrowed("")
}

/// CSS `filter: brightness(k)`.
fn brighter(c: Rgba, k: f32) -> Rgba {
    Rgba::new((c.r * k).min(1.0), (c.g * k).min(1.0), (c.b * k).min(1.0), c.a)
}

impl QuickPanel {
    // ---- the first page ----

    /// The sheet as it is at the time of the frame being drawn (the Wi-Fi page keeps to the closed one).
    fn sheet(&self) -> Sheet {
        if self.page == Page::Wifi { Sheet::CLOSED } else { self.slide.sheet(self.now) }
    }

    fn lines(&self) -> Lines {
        Lines::of(&self.st.audio)
    }

    fn geo(&self) -> Geo {
        geo(self.sheet(), self.bright, self.device_w, self.lines().shown())
    }

    /// The output Windows plays through now: the name on the Sound card's button.
    fn current_output(&self) -> Option<&Device> {
        let a = &self.st.audio;
        a.outputs.iter().find(|d| d.current).or(a.outputs.first())
    }

    fn draw_main(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let g = self.geo();
        let sheet = self.sheet();

        for (i, r) in g.tiles.iter().enumerate() {
            let Some(tile) = self.st.tiles.get(i) else { continue };
            let id = i as i32;
            let on = f32::from(tile.on);
            let t = sheet.tiles[i];
            let compact = self.slide.open && self.page == Page::Main;
            let (fg_t, chip_t, op_t) = (f.tween(id, 1, on, 300, EASE), f.tween(id, 2, on, 350, EASE), f.tween(id, 3, on, 350, EASE));
            let (reveal, turn) = (f.tween(id, 4, on, 350, SPRING), f.tween(id, 5, on, 300, SPRING));
            let icon_t = f.tween(id, 6, on, 300, EASE);
            let hot = f.hover(id, 250, EASE);
            let press = f.tween(id, 7, f32::from(f.press == Some(id)), 250, SPRING);
            let fg = INK.mix(ON_ACCENT, fg_t);
            let radius = g.radius[i];
            c.transformed(r.cx(), r.cy(), 1.0 - 0.05 * press, 0.0, 0.0, |c| {
                c.fill(*r, radius, Rgba::white(0.07 + 0.025 * hot));
                // `.fill`: a circle of the accent that grows from the chip, `clip-path` .35s spring, opacity .4 to 1; in the row
                // it is `circle(30px at 18px 18px)`, enough to fill the whole 36 px square
                let origin = (r.x + anim::lerp(27.0, 18.0, t), r.y + anim::lerp(26.0, 18.0, t));
                c.fill_disc_in(*r, radius, origin, anim::lerp(140.0, 30.0, t) * reveal, brighter(accent, 1.0 + 0.12 * hot).alpha(0.4 + 0.6 * op_t));
                // the chip: in the grid at the left, in the row the icon alone in the middle (its plate goes at once)
                let chip = (if compact { r.cx() } else { r.x + 11.0 + 16.0 }, r.y + g.pad_top[i] + 16.0);
                if !compact {
                    c.circle(chip.0, chip.1, 16.0, Rgba::white(0.09).mix(Rgba::css(11, 18, 32, 0.14), chip_t));
                }
                // two icons on the chip: the off one turns away and shrinks, the on one turns in
                let ic = &ICONS[i];
                c.faded(1.0 - icon_t, |c| c.icon(ic.off, chip, 17.0 * (1.0 - 0.7 * turn), 2.0, fg, 90.0 * turn));
                c.faded(icon_t, |c| {
                    let size = 17.0 * (0.3 + 0.7 * turn);
                    if ic.filled {
                        c.glyph(ic.on, chip, size, fg);
                    }
                    c.icon(ic.on, chip, size, if ic.filled { 1.2 } else { 2.0 }, fg, -90.0 * (1.0 - turn));
                });
                // the label and the chevron come back last and leave first
                let shown = self.slide.labels[i].value(self.now);
                if shown > 0.01 {
                    c.faded(shown, |c| {
                        c.transformed(0.0, 0.0, 1.0, 0.0, 6.0 * (1.0 - shown), |c| {
                            if i == 0 {
                                let h = f.hover(MORE, 160, EASE);
                                let at = (r.x + r.w - 11.0 - 7.0, chip.1);
                                c.circle(at.0, at.1, 12.0, Rgba::css(11, 18, 32, 0.12).mix(Rgba::white(0.12), 1.0 - fg_t).fade(h));
                                c.icon(CHEVRON_RIGHT, at, 14.0, 2.4, fg, 0.0);
                            }
                            let text_w = (INNER - 2.0 * GAP) / 3.0 - 22.0;
                            let (lf, sf) = (Font::new(13.0, 600), Font::new(12.0, 400));
                            let name = fit(c, label(i), lf, text_w);
                            c.text(&name, Rect::new(r.x + 11.0, r.y + r.h - 10.0 - 31.0, text_w, 15.6), lf, fg, Align::Start);
                            let status = fit(c, &tile.status, sf, text_w);
                            c.text(&status, Rect::new(r.x + 11.0, r.y + r.h - 10.0 - 14.4, text_w, 14.4), sf, fg.fade(0.82), Align::Start);
                        });
                    });
                }
            });
        }

        // the sound card
        c.fill(g.sound, 16.0, Rgba::white(0.07));
        let list = self.lines();
        self.draw_list(c, f, &g, &list, accent);
        let title_y = g.device.cy() - 13.0;
        c.text(t!("Звук", "Sound", ja: "サウンド", pt: "Som", zh: "声音"), Rect::new(g.sound.x + 12.0, title_y, 120.0, 26.0), Font::new(13.0, 600), INK, Align::Start);
        // the button: the icon of the output in use, its name, a chevron that turns over when the list is shown
        let open_t = f.tween(DEVICE, 1, f32::from(self.slide.open), 250, EASE);
        let hot = f.hover(DEVICE, 250, EASE);
        c.fill(g.device, 8.0, Rgba::white((0.1 * hot).max(0.1 * open_t)));
        let (icon, name) = self.current_output().map_or((Kind::Speakers.icon(), "\u{2014}"), |d| (d.kind.icon(), d.name.as_str()));
        let font = Font::new(12.0, 500);
        let name = fit(c, name, font, g.device.w - 8.0 - 14.0 - 7.0 - 7.0 - 11.0 - 8.0);
        c.icon(icon, (g.device.x + 8.0 + 7.0, g.device.cy()), 14.0, 2.0, accent, 0.0);
        c.text(&name, Rect::new(g.device.x + 8.0 + 14.0 + 7.0, g.device.y, g.device.w, g.device.h), font, INK, Align::Start);
        let turn = f.tween(DEVICE, 3, f32::from(self.slide.open), 250, SPRING);
        c.icon(CHEVRON_DOWN, (g.device.x + g.device.w - 8.0 - 5.5, g.device.cy()), 11.0, 2.6, DIM, 180.0 * (1.0 - turn));

        let (vol, muted) = self.st.vol.unwrap_or((0, false));
        let mute_t = f.tween(MUTE, 1, f32::from(muted), 300, EASE);
        let shown = if muted { 0 } else { vol };
        let hot = f.hover(MUTE, 300, EASE);
        c.fill(g.mute, 8.0, Rgba::white(0.08 * hot));
        let icon = if shown == 0 { SPEAKER_0 } else if shown < 40 { SPEAKER_1 } else { SPEAKER_2 };
        c.icon(icon, (g.mute.cx(), g.mute.cy()), 19.0, 2.0, INK.mix(MUTED, mute_t), 0.0);
        let fill = accent.mix(Rgba::white(0.18), mute_t);
        slider(c, f, VOL, g.vol, shown as f32 / 100.0, fill);
        let num = if muted { t!("Выкл.", "Muted", ja: "ミュート", pt: "Mudo", zh: "静音").to_string() } else { format!("{vol}%") };
        c.text(&num, g.vol_num, Font::new(13.0, 600).tabular(), DIM.mix(MUTED, mute_t), Align::End);
        if let Some(b) = self.st.bright {
            c.icon(SUN, (g.bright_icon.cx(), g.bright_icon.cy()), 19.0, 2.0, INK, 0.0);
            slider(c, f, BRIGHT, g.bright, b as f32 / 100.0, accent);
            c.text(&format!("{b}%"), g.bright_num, Font::new(13.0, 600).tabular(), DIM, Align::End);
        }

        // the player
        c.fill(g.media, 16.0, Rgba::white(0.07));
        let text_w = g.buttons[0].x - g.media.x - 16.0 - 8.0;
        match &self.st.media {
            Some(m) => {
                let title = if m.title.is_empty() { t!("Без названия", "Untitled", ja: "タイトルなし", pt: "Sem título", zh: "无标题") } else { m.title.as_str() };
                let (tf, af) = (Font::new(13.0, 600), Font::new(12.0, 400));
                let title = fit(c, title, tf, text_w);
                let artist = fit(c, &m.artist, af, text_w);
                let two = !m.artist.is_empty();
                let top = g.media.cy() - if two { 16.0 } else { 8.0 };
                c.text(&title, Rect::new(g.media.x + 16.0, top, text_w, 16.0), tf, INK, Align::Start);
                if two {
                    c.text(&artist, Rect::new(g.media.x + 16.0, top + 17.0, text_w, 15.0), af, DIM, Align::Start);
                }
            }
            None => c.text(t!("Ничего не воспроизводится", "Nothing playing", ja: "再生中のメディアはありません", pt: "Nada em reprodução", zh: "没有正在播放的内容"), Rect::new(g.media.x + 16.0, g.media.y, text_w, g.media.h), Font::new(13.0, 600), DIM, Align::Start),
        }
        let playing = self.st.media.as_ref().is_some_and(|m| m.playing);
        for (j, (r, d)) in g.buttons.iter().zip([PREVIOUS, if playing { PAUSE } else { PLAY }, NEXT]).enumerate() {
            let live = self.st.media.is_some();
            let hot = if live { f.hover(MEDIA + j as i32, 250, EASE) } else { 0.0 };
            c.fill(Rect::new(r.x, r.y, r.w, r.h), 20.0, Rgba::white(0.1 * hot));
            let ink = if live { INK } else { DIM };
            c.glyph(d, (r.cx(), r.cy()), 17.0, ink);
            c.icon(d, (r.cx(), r.cy()), 17.0, 2.0, ink, 0.0);
        }

        // the footer: the battery, the gear
        if let Some((pct, ac)) = self.st.battery {
            let ink = if ac { GREEN } else if pct <= 15 { RED } else { DIM };
            battery(c, Rect::new(g.foot.x + 6.0, g.foot.cy() - 6.5, 24.0, 13.0), pct, ink);
            c.text(&format!("{pct}%"), Rect::new(g.foot.x + 6.0 + 24.0 + 8.0, g.foot.y, 60.0, g.foot.h), Font::new(13.0, 400), ink, Align::Start);
        }
        let hot = f.hover(GEAR, 250, EASE);
        c.circle(g.gear.cx(), g.gear.cy(), 20.0, Rgba::white(0.07 + 0.03 * hot));
        c.icon(icons::GEAR, (g.gear.cx(), g.gear.cy()), 17.0, 2.0, INK, 0.0);
    }

    /// The list of devices in the sound card (`.audw`): its box opens like a drawer, the lines fade in and sink into place.
    fn draw_list(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, g: &Geo, list: &Lines, accent: Rgba) {
        let fade = self.slide.fade.value(self.now);
        if g.wrap.h < 0.5 || fade < 0.01 {
            return;
        }
        let (items, shift) = (self.slide.items.value(self.now), self.slide.shift.value(self.now));
        // `.audl`: 6 px wider on each side and 4 px higher than its box, which clips it
        let (ax, ay, aw) = (g.wrap.x - 6.0, g.wrap.y - 4.0, g.wrap.w + 12.0);
        let (out, inp) = (&self.st.audio.outputs, &self.st.audio.inputs);
        c.faded(fade, |c| {
            c.clipped(g.wrap, |c| {
                for &(line, y, h) in &list.lines {
                    let r = Rect::new(ax, ay + y, aw, h);
                    if let Line::Title(is_out) = line {
                        let word = if is_out { t!("Вывод", "Output", ja: "出力", pt: "Saída", zh: "输出") } else { t!("Ввод", "Input", ja: "入力", pt: "Entrada", zh: "输入") };
                        c.text(word, Rect::new(r.x + 8.0, r.y + 6.0, r.w - 16.0, 14.0), Font::new(11.5, 500), DIM2, Align::Start);
                        continue;
                    }
                    let id = line.id();
                    c.faded(items, |c| {
                        c.transformed(0.0, 0.0, 1.0, 0.0, -6.0 * (1.0 - shift), |c| {
                            let hot = f.hover(id, 140, EASE);
                            match line {
                                Line::Device(is_out, n) => {
                                    let Some(d) = (if is_out { out } else { inp }).get(n) else { return };
                                    c.fill(r, 10.0, Rgba::white(0.07 * hot));
                                    let on = f.tween(id, 1, f32::from(d.current), 250, EASE);
                                    let plate = Rect::new(r.x + 6.0, r.y + 6.0, 28.0, 28.0);
                                    c.fill(plate, 8.0, Rgba::white(0.08).mix(accent, on));
                                    c.icon(d.kind.icon(), (plate.cx(), plate.cy()), 15.0, 2.0, DIM.mix(ON_ACCENT, on), 0.0);
                                    let w = r.w - 6.0 - 28.0 - 10.0 - 10.0 - 14.0 - 6.0;
                                    let (nf, sf) = (Font::new(13.0, 500), Font::new(11.5, 400));
                                    c.text_fit(&d.name, Rect::new(r.x + 44.0, r.y + 4.0, w, 16.0), nf, INK, Align::Start);
                                    c.text_fit(&d.sub, Rect::new(r.x + 44.0, r.y + 21.0, w, 14.0), sf, DIM2, Align::Start);
                                    if d.current {
                                        c.icon(CHECK, (r.x + r.w - 6.0 - 7.0, r.cy()), 14.0, 2.8, accent, 0.0);
                                    }
                                }
                                _ => {
                                    c.fill(r, 8.0, Rgba::white(0.07 * hot));
                                    c.text(t!("Параметры звука", "Sound settings", ja: "サウンド設定", pt: "Configurações de som", zh: "声音设置"), Rect::new(r.x + 8.0, r.y, r.w - 30.0, r.h), Font::new(12.0, 400), DIM, Align::Start);
                                    c.icon(CHEVRON_RIGHT, (r.x + r.w - 8.0 - 6.0, r.cy()), 12.0, 2.4, DIM, 0.0);
                                }
                            }
                        });
                    });
                }
                c.fill(Rect::new(ax, ay + list.height - 1.0, aw, 1.0), 0.0, Rgba::white(0.07));
            });
        });
    }

    // ---- the second page ----

    fn draw_wifi(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let (card, label_y, rows) = self.layout();
        let wf = &self.wifi;

        // the back button, the title with its switch
        let word = t!("Назад", "Back", ja: "戻る", pt: "Voltar", zh: "返回");
        self.back_w = 6.0 + 16.0 + 8.0 + c.measure(word, Font::new(13.0, 400)).ceil() + 10.0;
        let back = self.back_rect();
        c.fill(back, 10.0, Rgba::white(0.09 * f.hover(BACK, 140, SPRING)));
        c.icon(CHEVRON_LEFT, (back.x + 6.0 + 8.0, back.cy()), 16.0, 2.2, DIM, 0.0);
        c.text(word, Rect::new(back.x + 6.0 + 16.0 + 8.0, back.y, back.w, back.h), Font::new(13.0, 400), DIM, Align::Start);
        let title_y = back.y + back.h + 4.0;
        c.text("Wi\u{2011}Fi", Rect::new(PAD + 10.0, title_y, 200.0, 18.0), Font::new(15.0, 600), INK, Align::Start);
        if let Some((s, tone)) = &wf.status {
            let s = fit(c, s, Font::new(12.0, 400), INNER - 20.0 - 60.0);
            c.text(&s, Rect::new(PAD + 10.0, title_y + 20.0, INNER - 20.0 - 60.0, 15.0), Font::new(12.0, 400), if *tone == Tone::Warn { AMBER } else { DIM2 }, Align::Start);
        }
        let sw = Self::switch_rect();
        let on = f.tween(SWITCH, 1, f32::from(wf.on), 160, EASE);
        let knob = f.tween(SWITCH, 2, f32::from(wf.on), 180, SPRING);
        c.fill(sw, 10.0, Rgba::white(0.16).mix(accent, on));
        c.circle(sw.x + 2.0 + 8.0 + 16.0 * knob, sw.cy(), 8.0, Rgba::white(1.0));

        if let Some((ssid, text)) = &self.pass {
            // the password of a new network
            c.text(&tf!("Пароль для «{ssid}»", "Password for “{ssid}”", ja: "「{ssid}」のパスワード", pt: "Senha de “{ssid}”", zh: "“{ssid}”的密码"), Rect::new(PAD + 10.0, Self::pass_rect().y - 30.0, INNER - 20.0, 22.0), Font::new(13.0, 400), INK, Align::Start);
            let r = Self::pass_rect();
            c.fill(r, 10.0, Rgba::white(0.09));
            c.ring(r, 10.0, 1.5, accent);
            let dots = "•".repeat(text.chars().count());
            c.text(&dots, Rect::new(r.x + 14.0, r.y, r.w - 28.0, r.h), Font::new(14.0, 400), INK, Align::Start);
            let caret = c.measure(&dots, Font::new(14.0, 400)) + r.x + 14.0 + 1.0;
            if (f.now / 530).is_multiple_of(2) {
                c.fill(Rect::new(caret, r.cy() - 9.0, 2.0, 18.0), 1.0, accent);
            }
            c.text(t!("Enter — подключиться · Esc — отмена", "Enter — connect · Esc — cancel", ja: "Enter — 接続 · Esc — キャンセル", pt: "Enter — conectar · Esc — cancelar", zh: "Enter — 连接 · Esc — 取消"), Rect::new(PAD + 10.0, r.y + r.h + 8.0, INNER - 20.0, 16.0), Font::new(12.0, 400), DIM2, Align::Start);
        } else {
            if wf.nets.is_empty() {
                let s = if wf.on { t!("Рядом нет сетей", "No networks nearby", ja: "近くにネットワークがありません", pt: "Nenhuma rede por perto", zh: "附近没有网络") } else { t!("Включите Wi‑Fi, чтобы увидеть сети", "Turn Wi‑Fi on to see networks", ja: "ネットワークを表示するには Wi‑Fi をオンにしてください", pt: "Ative o Wi‑Fi para ver as redes", zh: "开启 Wi‑Fi 以查看网络") };
                c.text(s, Rect::new(PAD + 10.0, PAD + 36.0 + 48.0 + 8.0, INNER - 20.0, 22.0), Font::new(13.0, 400), DIM2, Align::Start);
            }
            // the connected network on its own card
            if let (Some(r), Some(i)) = (card, wf.nets.iter().position(|n| n.connected)) {
                let n = &wf.nets[i];
                let hot = f.hover(NET + i as i32, 250, EASE).max(f.hover(DISCONNECT, 250, EASE));
                c.fill(r, 16.0, Rgba::white(0.07 + 0.02 * hot));
                signal(c, (r.x + 16.0 + 10.0, r.cy()), n.quality, accent, wf.offline, f.now, wf.joining.as_deref() == Some(n.ssid.as_str()));
                let lines = match (&wf.link, wf.offline) {
                    (_, true) => (t!("Подключено, без интернета", "Connected, no internet", ja: "接続済み、インターネットなし", pt: "Conectado, sem internet", zh: "已连接，无 Internet").to_string(), AMBER),
                    (Some(l), _) if hot > 0.5 => (format!("↓{} ↑{} {} · {}", l.down, l.up, t!("Мбит/с", "Mbps", ja: "Mbps", pt: "Mbps", zh: "Mbps"), l.auth), DIM2),
                    (Some(l), _) => (format!("{} · {} · {}%", t!("Подключено", "Connected", ja: "接続済み", pt: "Conectado", zh: "已连接"), l.phy, n.quality), DIM2),
                    (None, _) => (format!("{} · {}%", t!("Подключено", "Connected", ja: "接続済み", pt: "Conectado", zh: "已连接"), n.quality), DIM2),
                };
                let tw = r.w - 16.0 - 20.0 - 14.0 - 110.0;
                let name = fit(c, &n.ssid, Font::new(14.0, 600), tw);
                c.text(&name, Rect::new(r.x + 52.0, r.cy() - 19.0, tw, 19.0), Font::new(14.0, 600), INK, Align::Start);
                let sub = fit(c, &lines.0, Font::new(12.0, 400), tw);
                c.text(&sub, Rect::new(r.x + 52.0, r.cy() + 1.0, tw, 16.0), Font::new(12.0, 400), lines.1, Align::Start);
                let pill = Rect::new(r.x + r.w - 12.0 - 100.0, r.cy() - 14.0, 100.0, 28.0);
                let ph = f.hover(DISCONNECT, 160, EASE);
                c.faded(hot, |c| {
                    c.fill(pill, 14.0, Rgba::white(0.09).mix(RED.alpha(0.18), ph));
                    c.text(t!("Отключиться", "Disconnect", ja: "切断", pt: "Desconectar", zh: "断开连接"), pill, Font::new(12.0, 600), DIM.mix(RED, ph), Align::Center);
                });
                c.faded(1.0 - hot, |c| c.icon(CHECK, (r.x + r.w - 16.0 - 8.0, r.cy()), 16.0, 2.6, accent, 0.0));
            }
            // the other networks
            if !rows.is_empty() {
                c.text(t!("Другие сети", "Other networks", ja: "その他のネットワーク", pt: "Outras redes", zh: "其他网络"), Rect::new(PAD + 10.0, label_y, INNER - 20.0, 15.0), Font::new(12.0, 500), DIM2, Align::Start);
            }
            for (k, (i, r)) in rows.iter().enumerate() {
                let n = &wf.nets[*i];
                let id = NET + *i as i32;
                // the rows rise in one after another when the page opens
                let rise = f.rise(60 + 30 * k as u32, 260, SNAP, 8.0, 1.0);
                let hot = f.hover(id, 140, SPRING);
                let busy = wf.joining.as_deref() == Some(n.ssid.as_str());
                rise.draw(c, (r.cx(), r.cy()), |c| {
                    c.fill(*r, 10.0, Rgba::white(0.09 * hot));
                    signal(c, (r.x + 10.0 + 10.0, r.cy()), n.quality, DIM, false, f.now, busy);
                    let tw = r.w - 10.0 - 20.0 - 12.0 - 100.0;
                    let sub = if busy {
                        Some((format!("{}{}", t!("Подключение", "Connecting", ja: "接続中", pt: "Conectando", zh: "正在连接"), &"...."[..(f.now / 400 % 4) as usize]), accent))
                    } else if !n.secure {
                        Some((t!("Открытая сеть", "Open network", ja: "オープン ネットワーク", pt: "Rede aberta", zh: "开放网络").to_string(), AMBER))
                    } else if n.saved {
                        Some((t!("Сохранена", "Saved", ja: "保存済み", pt: "Salva", zh: "已保存").to_string(), DIM2))
                    } else {
                        None
                    };
                    let name = fit(c, &n.ssid, Font::new(13.0, 500), tw);
                    match sub {
                        Some((s, col)) => {
                            c.text(&name, Rect::new(r.x + 42.0, r.cy() - 17.0, tw, 17.0), Font::new(13.0, 500), INK, Align::Start);
                            c.text(&s, Rect::new(r.x + 42.0, r.cy() + 1.0, tw, 15.0), Font::new(12.0, 400), col, Align::Start);
                        }
                        None => c.text(&name, Rect::new(r.x + 42.0, r.y, tw, r.h), Font::new(13.0, 500), INK, Align::Start),
                    }
                    if busy {
                        let (l, rr) = (r.x + 42.0, r.x + r.w - 14.0);
                        let p = (f.now % 1300) as f32 / 1300.0;
                        let a = l + (rr - l) * (EASE.ease(p) * 1.3 - 0.3);
                        c.fill(Rect::new(l, r.y + r.h - 5.0, rr - l, 2.0), 1.0, accent.alpha(0.18));
                        let (a0, a1) = (a.max(l), (a + (rr - l) * 0.3).min(rr));
                        c.fill(Rect::new(a0, r.y + r.h - 5.0, (a1 - a0).max(0.0), 2.0), 1.0, accent);
                    } else {
                        let action = if n.saved || !n.secure { t!("Подключить", "Connect", ja: "接続", pt: "Conectar", zh: "连接") } else { t!("Ввести пароль", "Enter password", ja: "パスワードを入力", pt: "Digite a senha", zh: "输入密码") };
                        let right = Rect::new(r.x + r.w - 12.0 - 100.0, r.y, 100.0, r.h);
                        c.faded(hot, |c| c.text(action, right, Font::new(12.0, 600), accent, Align::End));
                        if n.secure {
                            c.faded(1.0 - hot, |c| c.icon(LOCK, (r.x + r.w - 12.0 - 7.0, r.cy()), 14.0, 2.0, DIM2, 0.0));
                        }
                    }
                });
            }
        }

        // the way into Windows' settings
        let r = self.footer();
        c.fill(r, 10.0, Rgba::white(0.09 * f.hover(NETWORKS, 140, SPRING)));
        c.icon(GLOBE, (r.x + 10.0 + 9.0, r.cy()), 18.0, 1.8, DIM, 0.0);
        c.text(t!("Параметры сети", "Network settings", ja: "ネットワーク設定", pt: "Configurações de rede", zh: "网络设置"), Rect::new(r.x + 10.0 + 18.0 + 12.0, r.y, r.w - 80.0, r.h), Font::new(13.0, 400), INK, Align::Start);
        c.icon(CHEVRON_RIGHT, (r.x + r.w - 10.0 - 7.0, r.cy()), 14.0, 2.4, DIM2, 0.0);

        // the context menu, over everything
        if let (Some((mb, items)), Some(ssid)) = (self.menu_rects(), self.ctx.clone()) {
            let what = self.menu_items(&ssid);
            c.shadow(mb, 12.0, 8.0, 24.0, Rgba::css(0, 0, 0, 0.5));
            c.fill(mb, 12.0, Rgba::css(44, 45, 54, 0.98));
            c.ring(mb, 12.0, 1.0, Rgba::white(0.1));
            for (j, (r, (text, act))) in items.iter().zip(&what).enumerate() {
                let hot = f.hover(MENU + j as i32, 140, SPRING);
                let danger = *act == 2;
                c.fill(*r, 8.0, if danger { RED.alpha(0.14 * hot) } else { Rgba::white(0.09 * hot) });
                c.text(text, Rect::new(r.x + 12.0, r.y, r.w - 24.0, r.h), Font::new(13.0, 400), if danger { Rgba::css(255, 138, 143, 1.0) } else { INK }, Align::Start);
            }
        }
    }
}

/// A slider of the mockup: a 28 px track, the fill (at least a full circle) and a 24 px knob that grows under the pointer.
fn slider(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: i32, track: Rect, v: f32, fill: Rgba) {
    let v = f.tween(id, 1, v, 120, EASE_OUT);
    let (hot, held) = (f.hover(id, 250, SPRING), f32::from(f.press == Some(id)));
    let grow = f.tween(id, 2, 1.0 + 0.08 * hot - 0.14 * held, 250, SPRING);
    c.fill(track, 14.0, Rgba::white(0.1));
    c.fill(Rect::new(track.x, track.y, 28.0 + (track.w - 28.0) * v, track.h), 14.0, fill);
    let knob = Rect::new(track.x + 2.0 + (track.w - 28.0) * v, track.y + 2.0, 24.0, 24.0);
    c.transformed(knob.cx(), knob.cy(), grow, 0.0, 0.0, |c| {
        c.shadow(knob, 12.0, 1.0, 3.0, Rgba::css(0, 0, 0, 0.35));
        c.circle(knob.cx(), knob.cy(), 12.0, Rgba::white(1.0));
        c.ring(knob, 12.0, 0.5, Rgba::css(0, 0, 0, 0.2));
    });
}

/// The battery of the footer: `r` is the 24x13 box of its drawing (a 26x14 grid), the charge fills the inside.
fn battery(c: &mut Canvas<'_>, r: Rect, pct: i32, ink: Rgba) {
    let k = r.w / 26.0;
    c.ring(Rect::new(r.x, r.y, 22.5 * k, 13.5 * k), 4.25 * k, 1.5 * k, ink);
    let charge = (17.0 * pct.clamp(0, 100) as f32 / 100.0).max(1.5);
    c.fill(Rect::new(r.x + 3.0 * k, r.y + 3.0 * k, charge * k, 8.0 * k), 1.8 * k, ink);
    c.line(r.x + 24.0 * k, r.y + 5.0 * k, r.x + 24.0 * k, r.y + 9.0 * k, 1.8 * k, ink);
}

/// The Wi-Fi glyph of a network: as many arcs as its signal lights, the rest dim; while it is being joined they sweep up.
fn signal(c: &mut Canvas<'_>, at: (f32, f32), q: i32, ink: Rgba, offline: bool, now: u64, busy: bool) {
    let lit = if busy { (now / 230 % 4) as i32 } else if q >= 67 { 3 } else if q >= 34 { 2 } else { 1 };
    let dim = ink.fade(0.3);
    for (i, d) in [icons::WIFI_ARC_1, icons::WIFI_ARC_2, icons::WIFI_ARC_3].into_iter().enumerate() {
        c.icon(d, at, 20.0, 2.0, if (i as i32) < lit { ink } else { dim }, 0.0);
    }
    c.icon(icons::WIFI_DOT, at, 20.0, 2.4, if lit > 0 { ink } else { dim }, 0.0);
    if offline {
        c.icon(icons::WIFI_WARN, at, 20.0, 2.0, AMBER, 0.0);
    }
}

impl Content for QuickPanel {
    fn id(&self) -> Id {
        Id::Qs
    }

    fn look(&self) -> Look {
        Look::dense(22.0, Shadow { dy: 30.0, blur: 80.0, color: Rgba::css(4, 6, 20, 0.65) })
    }

    fn entry(&self) -> Entry {
        Entry { origin: if self.side == Side::Bottom { Origin::Frac(1.0, 1.0) } else { Origin::Button }, ..Entry::pop() }
    }

    fn place(&self) -> Place {
        Place { along: Along::ScreenEnd, ..Place::default() }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        (W, self.geo().height)
    }

    /// The box with the list shown and the strip in its row, the largest the first page takes: the glass under it is grabbed
    /// when the panel opens.
    fn max_size(&mut self, _g: &mut Gfx) -> Option<(f32, f32)> {
        let lines = self.lines();
        Some((W, geo(Sheet::OPEN, self.bright, self.device_w, lines.shown()).height.max(self.geo().height)))
    }

    /// The panel takes the height the strip and the list have got to: the list opens and closes here, at the time of the frame.
    fn resized(&mut self, g: &mut Gfx, now: u64) -> Option<(f32, f32)> {
        self.now = now;
        if std::mem::take(&mut self.toggle) && self.page == Page::Main {
            self.slide.set(!self.slide.open, now, false);
        }
        // the Sound card's button is as wide as the name of the output on it
        let name = self.current_output().map_or("\u{2014}", |d| d.name.as_str());
        let w = 8.0 + 14.0 + 7.0 + g.measure(name, Font::new(12.0, 500)).ceil() + 7.0 + 11.0 + 8.0;
        self.device_w = w.min(220.0);
        let new = (W, self.geo().height);
        (new != std::mem::replace(&mut self.size, new)).then_some(new)
    }

    fn pulse(&self) -> u32 {
        1000
    }

    fn animating(&self, now: u64) -> bool {
        self.pass.is_some() || self.flip.is_some() || self.wifi.joining.is_some() || self.toggle || self.slide.running(now)
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        self.now = f.now;
        self.host.tick(self.page == Page::Wifi);
        self.st = self.host.state();
        self.wifi = self.host.wifi();
        // a page that has just been chosen starts sliding now
        let (dur, _) = anim::timing(style::anim(), 300, 0);
        let (mut op, mut dx) = (1.0, 0.0);
        if let Some((t0, dir)) = self.flip {
            // the stamp of a flip that has just begun is the first frame's time
            let t0 = if t0 == 0 { f.now } else { t0 };
            if dur > 0 && f.now.saturating_sub(t0) < u64::from(dur) {
                self.flip = Some((t0, dir));
                op = SNAP.ease(f.now.saturating_sub(t0) as f32 / dur as f32);
                dx = anim::lerp(28.0 * dir, 0.0, op);
            } else {
                self.flip = None;
            }
        }
        c.faded(op, |c| {
            c.transformed(0.0, 0.0, 1.0, dx, 0.0, |c| match self.page {
                Page::Main => self.draw_main(c, f),
                Page::Wifi => self.draw_wifi(c, f),
            });
        });
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        match self.page {
            Page::Main => {
                let g = self.geo();
                if let Some(i) = g.tiles.iter().position(|r| r.contains(x, y)) {
                    // the chevron of the Wi-Fi tile is only there while the tile is a tile
                    let more = Rect::new(g.tiles[0].x + g.tiles[0].w - 11.0 - 14.0 - 10.0, g.tiles[0].y + 10.0, 34.0, 32.0);
                    return Some(if i == 0 && self.sheet().tiles[0] < 0.5 && more.contains(x, y) { MORE } else { i as i32 });
                }
                // the devices (a line that is not yet, or no longer, in the box of the list is not there to be pointed at)
                if g.wrap.contains(x, y) {
                    let (ax, ay, aw) = (g.wrap.x - 6.0, g.wrap.y - 4.0, g.wrap.w + 12.0);
                    if let Some(&(l, ..)) = self.lines().lines.iter().find(|&&(l, ly, h)| l.id() >= 0 && Rect::new(ax, ay + ly, aw, h).contains(x, y)) {
                        return Some(l.id());
                    }
                }
                let slider = |r: Rect| Rect::new(r.x - 8.0, r.y - 6.0, r.w + 16.0, r.h + 12.0);
                let list = [
                    (g.device, DEVICE),
                    (g.mute, MUTE),
                    (slider(g.vol), VOL),
                    (g.buttons[0], MEDIA),
                    (g.buttons[1], MEDIA + 1),
                    (g.buttons[2], MEDIA + 2),
                    (g.gear, GEAR),
                ];
                if self.bright && slider(g.bright).contains(x, y) {
                    return Some(BRIGHT);
                }
                list.iter().find(|(r, _)| r.contains(x, y)).map(|&(_, id)| id)
            }
            Page::Wifi => {
                if let Some((mb, items)) = self.menu_rects() {
                    return Some(items.iter().position(|r| r.contains(x, y)).map_or(if mb.contains(x, y) { -1 } else { DISMISS }, |j| MENU + j as i32)).filter(|&id| id >= 0);
                }
                if self.back_rect().contains(x, y) {
                    return Some(BACK);
                }
                if Rect::new(Self::switch_rect().x - 10.0, PAD + 36.0 + 4.0, 56.0, 34.0).contains(x, y) {
                    return Some(SWITCH);
                }
                if self.footer().contains(x, y) {
                    return Some(NETWORKS);
                }
                if self.pass.is_some() {
                    return None;
                }
                let (card, _, rows) = self.layout();
                if let Some((i, r)) = card.and_then(|r| self.wifi.nets.iter().position(|n| n.connected).map(|i| (i, r))).filter(|(_, r)| r.contains(x, y)) {
                    let pill = Rect::new(r.x + r.w - 12.0 - 100.0, r.cy() - 14.0, 100.0, 28.0);
                    return Some(if pill.contains(x, y) { DISCONNECT } else { NET + i as i32 });
                }
                rows.iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| NET + *i as i32)
            }
        }
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if let Some(ssid) = self.ctx.take() {
            // a click while the context menu is open: an item acts, anywhere else only closes it
            if id >= MENU {
                match self.menu_items(&ssid).get((id - MENU) as usize).map(|m| m.1) {
                    Some(0) => self.pick(&ssid, cx),
                    Some(1) => self.host.wifi_leave(),
                    Some(2) => self.host.wifi_forget(&ssid),
                    Some(_) => self.host.settings("ms-settings:network-wifisettings", cx),
                    None => {}
                }
            }
            return;
        }
        if b == Button::Right {
            // the context menu of a network
            if (NET..NET + 40).contains(&id) {
                self.ctx = self.wifi.nets.get((id - NET) as usize).map(|n| n.ssid.clone());
            } else if id == DISCONNECT {
                self.ctx = self.wifi.nets.iter().find(|n| n.connected).map(|n| n.ssid.clone());
            }
            return;
        }
        if b != Button::Left {
            return;
        }
        match id {
            0..=8 => self.host.tile(id as usize, cx),
            MORE => self.go(Page::Wifi),
            MUTE => self.host.mute(),
            DEVICE => self.toggle = true,
            SOUND_MORE => self.host.settings("ms-settings:sound", cx),
            d if (DEV..SOUND_MORE).contains(&d) => {
                let (input, n) = if d >= DEV + 40 { (true, (d - DEV - 40) as usize) } else { (false, (d - DEV) as usize) };
                let audio = Rc::clone(&self.st.audio);
                if let Some(dev) = (if input { &audio.inputs } else { &audio.outputs }).get(n) {
                    self.host.audio_set(input, &dev.id);
                }
            }
            GEAR => self.host.settings("ms-settings:", cx),
            m if (MEDIA..MEDIA + 3).contains(&m) => {
                if self.st.media.is_some() {
                    self.host.media((m - MEDIA) as u8);
                }
            }
            BACK => self.go(Page::Main),
            SWITCH => self.host.wifi_power(!self.wifi.on),
            NETWORKS => self.host.settings("ms-settings:network-wifi", cx),
            DISCONNECT => self.host.wifi_leave(),
            n if (NET..NET + 40).contains(&n) => {
                let Some(net) = self.wifi.nets.get((n - NET) as usize) else { return };
                if !net.connected {
                    let ssid = net.ssid.clone();
                    self.pick(&ssid, cx);
                }
            }
            _ => {}
        }
    }

    fn drag(&mut self, id: i32, x: f32, _y: f32) {
        let g = self.geo();
        match id {
            VOL => self.host.volume((slider_value(g.vol, x) * 100.0).round() as i32),
            BRIGHT => {
                self.bright_v = ((slider_value(g.bright, x) * 100.0).round() as i32).max(1);
                self.host.brightness(self.bright_v, false);
            }
            _ => {}
        }
    }

    fn release(&mut self, id: i32) {
        if id == BRIGHT {
            self.host.brightness(self.bright_v, true);
        }
    }

    fn wheel(&mut self, _x: f32, _y: f32, up: bool) {
        if self.page == Page::Wifi {
            let at = self.scroll.get().min(self.max_scroll.get());
            let to = if up { at.saturating_sub(1) } else { (at + 1).min(self.max_scroll.get()) };
            if to != at {
                self.scroll.set(to);
                self.ctx = None;
            }
            return;
        }
        if self.page == Page::Main
            && let Some((v, _)) = self.st.vol
        {
            self.host.volume((v + if up { 2 } else { -2 }).clamp(0, 100));
        }
    }

    fn key(&mut self, vk: u32, _cx: &mut Cx) -> bool {
        const BACKSPACE: u32 = 0x08;
        const RETURN: u32 = 0x0D;
        const ESCAPE: u32 = 0x1B;
        if self.ctx.is_some() && vk == ESCAPE {
            self.ctx = None;
            return true;
        }
        if self.pass.is_none() {
            return false;
        }
        match vk {
            ESCAPE => self.pass = None,
            RETURN => {
                if let Some((ssid, text)) = self.pass.take().filter(|(_, t)| !t.is_empty()) {
                    self.host.wifi_join(&ssid, &text);
                }
            }
            BACKSPACE => {
                if let Some((_, text)) = self.pass.as_mut() {
                    text.pop();
                }
            }
            _ => {}
        }
        true // the password field takes every key: Space and the arrows are not commands now
    }

    fn text(&mut self, c: char, _cx: &mut Cx) {
        // Backspace, Enter and Esc come through `key`; the editing characters of the shared text hook mean nothing to a password
        if !c.is_control()
            && let Some((_, text)) = self.pass.as_mut()
        {
            text.push(c);
        }
    }

    /// The tiles in their row have no labels: the tooltip says which is which (`title` in the mockup).
    fn tip(&self, id: i32) -> Option<(String, Rect)> {
        let i = usize::try_from(id).ok().filter(|&i| i < 9 && self.page == Page::Main && self.sheet().tiles[i] > 0.9)?;
        Some((label(i).to_string(), self.geo().tiles[i]))
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let order: Vec<i32> = match self.page {
            Page::Main => {
                let devices = self.lines().lines.into_iter().map(|(l, ..)| l.id()).filter(|&id| id >= 0 && self.slide.open);
                (0..9).chain([MORE, DEVICE]).chain(devices).chain([MUTE, VOL]).chain(self.bright.then_some(BRIGHT)).chain([MEDIA, MEDIA + 1, MEDIA + 2, GEAR]).collect()
            }
            Page::Wifi => {
                let (card, _, rows) = self.layout();
                let mut v = vec![BACK, SWITCH];
                if let (Some(_), Some(i)) = (card, self.wifi.nets.iter().position(|n| n.connected)) {
                    v.push(NET + i as i32);
                }
                v.extend(rows.iter().map(|(i, _)| NET + *i as i32));
                v.push(NETWORKS);
                v
            }
        };
        let at = from.and_then(|f| order.iter().position(|&o| o == f));
        let n = order.len();
        let i = match (at, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n - 1,
            (Some(i), Dir::Up | Dir::Left) => i.saturating_sub(1),
            (Some(i), Dir::Down | Dir::Right) => (i + 1).min(n - 1),
        };
        order.get(i).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio() -> Audio {
        let d = |name: &str, current: bool| Device { id: name.into(), name: name.into(), sub: String::new(), kind: Kind::Speakers, current };
        Audio { outputs: vec![d("Speakers", true), d("WH-1000XM5", false), d("DELL U2723QE", false)], inputs: vec![d("Microphone Array", true), d("WH-1000XM5", false)] }
    }

    #[test]
    fn the_first_page_is_the_mockups() {
        let g = geo(Sheet::CLOSED, false, 90.0, Lines::of(&audio()).shown());
        assert_eq!(g.height, 508.0); // 12 + 3 tiles of 86 and 2 gaps + 8 + sound 88 + 8 + player 58 + 8 + footer 40 + 12
        assert_eq!((g.tiles[0].x, g.tiles[0].y, g.tiles[0].h), (12.0, 12.0, 86.0));
        assert!((g.tiles[1].x - (12.0 + 113.333_33 + 8.0)).abs() < 1e-3);
        assert_eq!((g.mute.x, g.mute.y, g.mute.w, g.vol.x, g.vol.y, g.vol.w, g.vol.h), (20.0, 338.0, 32.0, 62.0, 340.0, 244.0, 28.0));
        assert_eq!(g.vol_num.x + g.vol_num.w, 356.0);
        assert_eq!(g.gear.x + g.gear.w, 368.0);
        assert_eq!(geo(Sheet::CLOSED, true, 90.0, 0.0).height, 548.0); // a laptop's brightness row: 32 and a gap more
    }

    #[test]
    fn the_list_of_devices_makes_the_panel_as_tall_as_the_mockups() {
        // the strip is 36 high, the card holds a list of 299 (3 outputs, 2 inputs) and its 8 of gap, the rest is as before
        let lines = Lines::of(&audio());
        assert_eq!((lines.height, lines.shown()), (303.0, 299.0));
        let g = geo(Sheet::OPEN, false, 90.0, lines.shown());
        assert_eq!(g.height, 577.0);
        assert_eq!((g.tiles[8].x, g.tiles[8].y, g.tiles[8].w, g.tiles[8].h), (332.0, 12.0, 36.0, 36.0));
        assert_eq!((g.wrap.y, g.wrap.h), (66.0, 299.0));
        assert_eq!(g.device.y, 56.0 + 10.0 + 299.0 + 8.0 - 1.0);
    }

    #[test]
    fn the_lines_of_the_list_stand_as_the_mockups_do() {
        let l = Lines::of(&audio());
        let at = |line| l.lines.iter().find(|x| x.0 == line).map(|x| (x.1, x.2));
        assert_eq!(at(Line::Title(true)), Some((0.0, 24.0)));
        assert_eq!(at(Line::Device(true, 0)), Some((26.0, 40.0)));
        assert_eq!(at(Line::Title(false)), Some((152.0, 24.0)));
        assert_eq!(at(Line::More), Some((264.0, 32.0)));
        // no input device: its title is not there either
        let only_out = Lines::of(&Audio { outputs: audio().outputs, inputs: Vec::new() });
        assert!(only_out.lines.iter().all(|x| x.0 != Line::Title(false)));
    }

    #[test]
    fn the_strip_goes_to_its_row_tile_by_tile_and_back() {
        let mut slide = Slide::new();
        assert_eq!(slide.sheet(0).grid, 0.0);
        slide.set(true, 1000, false);
        assert!(slide.running(1100));
        // each tile starts 16 ms after the one before it, and all have arrived 500 ms after the last
        let mid = slide.sheet(1100);
        assert!(mid.tiles[0] > mid.tiles[8] && mid.grid > 0.0 && mid.grid < 1.0);
        let end = slide.sheet(1000 + 16 * 8 + 500);
        assert_eq!((end.grid, end.list, end.tiles), (1.0, 1.0, [1.0; 9]));
        assert!(!slide.running(3000));
        slide.set(false, 3000, true); // snapped, as the Wi-Fi page does
        assert_eq!(slide.sheet(3000).grid, 0.0);
    }

    #[test]
    fn a_pointer_under_the_knob_means_its_value() {
        let t = Rect::new(62.0, 300.0, 244.0, 28.0);
        assert_eq!(slider_value(t, 62.0), 0.0);
        assert_eq!(slider_value(t, 62.0 + 14.0), 0.0); // the knob's centre at the left end
        assert_eq!(slider_value(t, 62.0 + 244.0 - 14.0), 1.0);
        assert!((slider_value(t, 62.0 + 14.0 + 108.0) - 0.5).abs() < 1e-6);
        assert_eq!(slider_value(t, 1000.0), 1.0);
    }
}
