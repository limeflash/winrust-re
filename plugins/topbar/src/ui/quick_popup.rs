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

use super::anim::{self, CubicBezier, EASE, SNAP, SPRING};
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

/// The rectangles of the first page.
struct Geo {
    tiles: [Rect; 9],
    sound: Rect,
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

fn geo(has_bright: bool, device_w: f32) -> Geo {
    let tw = (INNER - 2.0 * GAP) / 3.0;
    let tiles = std::array::from_fn(|i| Rect::new(PAD + (i % 3) as f32 * (tw + GAP), PAD + (i / 3) as f32 * (TILE_H + GAP), tw, TILE_H));
    let y = PAD + 3.0 * TILE_H + 2.0 * GAP + GAP;
    let h = 10.0 + 26.0 + GAP + 32.0 + if has_bright { GAP + 32.0 } else { 0.0 } + 12.0;
    let sound = Rect::new(PAD, y, INNER, h);
    let (x0, x1) = (sound.x + 12.0, sound.x + sound.w - 12.0);
    let device = Rect::new(x1 + 6.0 - device_w, y + 10.0, device_w, 26.0);
    // a row is 32 high (the mute button); the 28 px track and the value are centred in it
    let row = |top: f32| {
        let track = Rect::new(x0 - 4.0 + 32.0 + 10.0, top + 2.0, (x1 - 40.0 - 10.0) - (x0 - 4.0 + 32.0 + 10.0), 28.0);
        (Rect::new(x0 - 4.0, top, 32.0, 32.0), track, Rect::new(x1 - 40.0, top, 40.0, 32.0))
    };
    let (mute, vol, vol_num) = row(y + 10.0 + 26.0 + GAP);
    let (bright_icon, bright, bright_num) = row(y + 10.0 + 26.0 + GAP + 32.0 + GAP);
    let media = Rect::new(PAD, y + h + GAP, INNER, 58.0);
    let buttons = std::array::from_fn(|i| Rect::new(media.x + media.w - 8.0 - 40.0 * (3 - i) as f32, media.cy() - 20.0, 40.0, 40.0));
    let foot = Rect::new(PAD, media.y + media.h + GAP, INNER, 40.0);
    let gear = Rect::new(foot.x + foot.w - 40.0, foot.y, 40.0, 40.0);
    Geo { tiles, sound, device, mute, vol, vol_num, bright_icon, bright, bright_num, media, buttons, foot, gear, height: foot.y + foot.h + PAD }
}

/// The value (0..1) a pointer at `x` means on a 28 px track with a 24 px knob.
fn slider_value(track: Rect, x: f32) -> f32 {
    ((x - track.x - 14.0) / (track.w - 28.0)).clamp(0.0, 1.0)
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
        Self {
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
        }
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
        self.flip = Some((0, dir));
        self.pass = None;
        self.ctx = None;
        ON_WIFI.set(to == Page::Wifi);
    }

    fn geo(&self) -> Geo {
        geo(self.bright, self.device_w)
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
        let room = (self.footer().y - GAP - y + 1.0) / 45.0;
        let rows = others.iter().take(room.max(0.0) as usize).enumerate().map(|(k, &i)| (i, Rect::new(PAD, y + k as f32 * 45.0, INNER, 44.0))).collect();
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

    fn draw_main(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let g = self.geo();

        for (i, r) in g.tiles.iter().enumerate() {
            let Some(tile) = self.st.tiles.get(i) else { continue };
            let id = i as i32;
            let on = f32::from(tile.on);
            let (fg_t, chip_t, op_t) = (f.tween(id, 1, on, 300, EASE), f.tween(id, 2, on, 350, EASE), f.tween(id, 3, on, 350, EASE));
            let (reveal, turn) = (f.tween(id, 4, on, 350, SPRING), f.tween(id, 5, on, 300, SPRING));
            let icon_t = f.tween(id, 6, on, 300, EASE);
            let hot = f.hover(id, 250, EASE);
            let press = f.tween(id, 7, f32::from(f.press == Some(id)), 250, SPRING);
            let fg = INK.mix(ON_ACCENT, fg_t);
            c.transformed(r.cx(), r.cy(), 1.0 - 0.05 * press, 0.0, 0.0, |c| {
                c.fill(*r, 16.0, Rgba::white(0.07 + 0.025 * hot));
                // `.fill`: a circle of the accent that grows from the chip, `clip-path` .35s spring, opacity .4 to 1
                let origin = (r.x + 27.0, r.y + 26.0);
                c.fill_disc_in(*r, 16.0, origin, 140.0 * reveal, brighter(accent, 1.0 + 0.12 * hot).alpha(0.4 + 0.6 * op_t));
                c.circle(origin.0, origin.1, 16.0, Rgba::white(0.09).mix(Rgba::css(11, 18, 32, 0.14), chip_t));
                // two icons on the chip: the off one turns away and shrinks, the on one turns in
                let ic = &ICONS[i];
                c.faded(1.0 - icon_t, |c| c.icon(ic.off, origin, 17.0 * (1.0 - 0.7 * turn), 2.0, fg, 90.0 * turn));
                c.faded(icon_t, |c| {
                    let size = 17.0 * (0.3 + 0.7 * turn);
                    if ic.filled {
                        c.glyph(ic.on, origin, size, fg);
                    }
                    c.icon(ic.on, origin, size, if ic.filled { 1.2 } else { 2.0 }, fg, -90.0 * (1.0 - turn));
                });
                if i == 0 {
                    let h = f.hover(MORE, 160, EASE);
                    let at = (r.x + r.w - 11.0 - 7.0, origin.1);
                    c.circle(at.0, at.1, 12.0, Rgba::css(11, 18, 32, 0.12).mix(Rgba::white(0.12), 1.0 - fg_t).fade(h));
                    c.icon(CHEVRON_RIGHT, at, 14.0, 2.4, fg, 0.0);
                }
                let text_w = r.w - 22.0;
                let (lf, sf) = (Font::new(13.0, 600), Font::new(12.0, 400));
                let name = fit(c, label(i), lf, text_w);
                c.text(&name, Rect::new(r.x + 11.0, r.y + TILE_H - 10.0 - 31.0, text_w, 15.6), lf, fg, Align::Start);
                let status = fit(c, &tile.status, sf, text_w);
                c.text(&status, Rect::new(r.x + 11.0, r.y + TILE_H - 10.0 - 14.4, text_w, 14.4), sf, fg.fade(0.82), Align::Start);
            });
        }

        // the sound card
        c.fill(g.sound, 16.0, Rgba::white(0.07));
        c.text(t!("Звук", "Sound", ja: "サウンド", pt: "Som", zh: "声音"), Rect::new(g.sound.x + 12.0, g.sound.y + 10.0, 120.0, 26.0), Font::new(13.0, 600), INK, Align::Start);
        let hot = f.hover(DEVICE, 250, EASE);
        c.fill(g.device, 8.0, Rgba::white(0.1 * hot));
        let name = t!("Динамики", "Speakers", ja: "スピーカー", pt: "Alto-falantes", zh: "扬声器");
        let tw = c.measure(name, Font::new(12.0, 400)).ceil();
        self.device_w = 8.0 + tw + 6.0 + 11.0 + 8.0;
        c.text(name, Rect::new(g.device.x + 8.0, g.device.y, tw + 2.0, 26.0), Font::new(12.0, 400), DIM, Align::Start);
        c.icon(CHEVRON_DOWN, (g.device.x + g.device.w - 8.0 - 5.5, g.device.cy()), 11.0, 2.6, DIM, 0.0);

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

    fn pulse(&self) -> u32 {
        1000
    }

    fn animating(&self, _now: u64) -> bool {
        self.pass.is_some() || self.flip.is_some() || self.wifi.joining.is_some()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
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
                    return Some(if i == 0 && Rect::new(g.tiles[0].x + g.tiles[0].w - 11.0 - 14.0 - 10.0, g.tiles[0].y + 10.0, 34.0, 32.0).contains(x, y) { MORE } else { i as i32 });
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
            DEVICE => self.host.settings("ms-settings:sound", cx),
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

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let order: Vec<i32> = match self.page {
            Page::Main => (0..9).chain([MORE, DEVICE, MUTE, VOL]).chain(self.bright.then_some(BRIGHT)).chain([MEDIA, MEDIA + 1, MEDIA + 2, GEAR]).collect(),
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

    #[test]
    fn the_first_page_is_the_mockups() {
        let g = geo(false, 90.0);
        assert_eq!(g.height, 508.0); // 12 + 3 tiles of 86 and 2 gaps + 8 + sound 88 + 8 + player 58 + 8 + footer 40 + 12
        assert_eq!((g.tiles[0].x, g.tiles[0].y, g.tiles[0].h), (12.0, 12.0, 86.0));
        assert!((g.tiles[1].x - (12.0 + 113.333_33 + 8.0)).abs() < 1e-3);
        assert_eq!((g.mute.x, g.mute.y, g.mute.w, g.vol.x, g.vol.y, g.vol.w, g.vol.h), (20.0, 338.0, 32.0, 62.0, 340.0, 244.0, 28.0));
        assert_eq!(g.vol_num.x + g.vol_num.w, 356.0);
        assert_eq!(g.gear.x + g.gear.w, 368.0);
        assert_eq!(geo(true, 90.0).height, 548.0); // a laptop's brightness row: 32 and a gap more
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
