#![windows_subsystem = "windows"]
// rust-settings [page]   A small Settings window that does not need Explorer or the Store Settings app, in the Obsidian design.
// Pages: Display (resolution, refresh rate, scale, orientation, HDR, night light, arrangement of several displays by dragging), Sound (output/input
// device, volume, mute, volume per app), Power & storage (battery, power plan, drives, disk cleanup), Network (adapters, Wi-Fi on every adapter:
// radio, connect with a password or 802.1X user name/password, hidden networks, disconnect/forget; VPN; proxy), Bluetooth (radio,
// airplane mode, paired and nearby devices), Apps (installed programs, uninstall), Notifications (the master switch, Do not disturb, how long a
// pop-up stays, a sound, how each app notifies), Windows Update (check, install), Personalization, Privacy & security (who uses the camera,
// microphone and location, what Windows Security says, a quick scan, what each app may use), Keyboard & mouse (layouts, the key that switches
// them, key repeat, pointer speed and precision, the main button, the wheel), Date & time (12/24 hours, date format, first day of the week,
// time zone, the time service), About. Not here, because Windows has no way to set them from a program: the design's Default apps page
// (Windows protects the choice of each file type), its Shortcuts page (WinRust's hotkeys are fixed in the bar), the automatic Do-not-disturb
// rules, adding and removing keyboard layouts, the touchpad, the clock's seconds and date on the bar, the screen-recording switches.
// `page` is a name or an ms-settings: address; anything unknown opens the first page. Ctrl+Up/Down change the page, Ctrl+F searches.
// A new resolution or refresh rate is temporary until confirmed: Enter keeps it, Esc or 15 s without a key goes back.
// Safety: only documented Win32/WinRT calls (the one undocumented piece is the default-device switch of the sound panel, see audio.rs);
// written are the HKCU values in `pages()`, the proxy values, the display mode and layout, the audio volume/mute/default device,
// Wi-Fi profiles through the WLAN API (a password typed here goes only into the profile Windows stores), the night-light setting; for the
// new pages (sysx.rs): the notification and consent-store values of `HKCU`, the keyboard, mouse and international values, the bar's own
// dnd.txt, and with administrator rights the time service's type and the PC-wide permission; `tzutil /s`, `w32tm /resync` and Defender's
// `MpCmdRun -Scan -ScanType 1` are started with fixed arguments;
// started: a program's own uninstaller after a second click, and this exe again for the night-light filter. No shell, no strings run as commands.
// Display: resolution, refresh rate, scale (undocumented DisplayConfig type -4, the one Windows' Settings uses), orientation, HDR.
// Several displays: pick the display the rows act on, extend/duplicate/one screen only (confirmed like a resolution), make main.
// shortcut: night light is a colour filter of our own (a gamma ramp kept by a helper process), not Windows' schedule-driven one (an undocumented per-build blob).
//
// rust-settings --render <png> state=<name> [light] [scale=1.5] [w=1080 h=800]   a state of the design as a picture (no window, demo data)
// rust-settings --render-all <dir>                                                  every state, dark and light, scale 1 and 1.5
// rust-settings --bench [open|hover|menu|type|scroll|resize|rest]                   what the window costs (off the screen, demo data)
// rust-settings --selftest window | widgets | memory | identify                     the host and the Identify windows (off the screen, demo data)
// None of those touches a setting of the machine: they run on `demo::Demo` (see os.rs).
#[macro_use]
extern crate winrust_common;
mod app;
mod audio;
mod bt;
mod demo;
mod display;
mod draw;
mod ext;
mod identify;
mod look;
mod more;
mod net;
mod os;
mod power;
mod search;
mod sys;
mod sysx;
mod wall;
mod wifi;
mod wu;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

use os::os;

pub const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
pub const STORAGE_SENSE: &str = r"Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy";
pub const NT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// What a line of a page is, and so what a click on it does.
pub enum Row {
    /// On when the DWORD equals `on_when`; a missing value counts as `default`.
    Toggle { label: String, key: &'static str, name: &'static str, on_when: u32, default: u32 },
    Info(String, String),
    /// A click does `Op`; the value shown was read when the page was built.
    Cycle(String, fn() -> String, Op),
    /// The volume of the speakers, the microphone or one app.
    Slider(String, audio::Src),
    /// A row that does `Cmd` on a click; the value is shown dim on the right.
    Item(String, String, Cmd),
    /// A network adapter; a click shows or hides its details when `bool` is set.
    Adapter(net::Adapter, bool),
    /// A switch whose state is read when the page is built (None: no such device, drawn disabled); a click does `Op`.
    Switch(String, fn() -> Option<bool>, Op),
    /// A visible Wi-Fi network and its status text: a click opens it (the password goes there), Connect connects.
    Wifi(wifi::Net, String),
    /// A slider of a number the system keeps as a step of a scale (key repeat, pointer speed); the line's `vol` is 0..=1.
    Gauge(String, more::Gauge),
    /// A switch of a yes/no that is not one DWORD of `HKCU` (Do not disturb, a permission, pointer precision); a click flips it.
    Flag(String, more::Flag),
}

impl Row {
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Row::Toggle { label, .. } | Row::Info(label, _) | Row::Cycle(label, ..) | Row::Slider(label, _) | Row::Item(label, ..) | Row::Switch(label, ..) | Row::Gauge(label, _) | Row::Flag(label, _) => label,
            Row::Adapter(a, _) => &a.name,
            Row::Wifi(n, _) => &n.ssid,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Res,
    Hz,
    Mute,
    MicMute,
    Orient,
    Scale,
    Hdr,
    Monitor,  // which display the rows below act on
    Topology, // extend / duplicate / one screen only
    Primary,  // make the selected display the main one
    Screen,
    Sleep,
    Plan,
    Radio,  // Wi-Fi on/off
    Hidden, // the hidden network's name and password
    OutDev, // playback device
    InDev,  // recording device
    Night,  // night-light strength
    Proxy,  // the proxy address
    Pac,    // the proxy script address
    Bt,     // Bluetooth radio
    Air,    // airplane mode
    UiLang, // the language of every WinRust program
    /// A value of the new pages picked from a list (more.rs `Opt`).
    Opt(more::Opt),
    /// The tab of the Privacy page's App access list.
    PvTab,
    /// The accent colour of every WinRust program (the swatches of Personalization).
    Accent,
    /// The Keyboard page's "Try it" field: nothing to do, the field is only there to hold keys down in.
    Try,
}

/// The languages the drop-down offers, in the order it shows them.
pub const LANGS: [winrust_common::Lang; 5] = [winrust_common::Lang::En, winrust_common::Lang::Ru, winrust_common::Lang::Ja, winrust_common::Lang::Pt, winrust_common::Lang::Zh];

fn lang_text() -> String {
    lang!().name().to_string()
}

#[derive(Clone)]
pub enum Cmd {
    Vpn(ext::Vpn),
    CleanTemp,
    EmptyBin,
    Uninstall(String),
    BtPair(String),
    BtUnpair(String),
    BtScan,
    Check,
    Install,
    Wallpaper,
    /// A notifying app of the Notifications page: a click unfolds its three ways.
    NotifyApp(String),
    /// App id, way (0 banner, 1 only in the centre, 2 off).
    NotifyMode(String, usize),
    /// A line of the Privacy page's "Right now": opens its tab of App access.
    PrivGo(more::Cap),
    QuickScan,
    OpenSecurity,
    SyncTime,
    /// The About page's Computer name: a click unfolds the name field.
    Rename,
}

pub const CONFIRM_SECS: u32 = 15;

/// The colour of a line's second row, a badge, a hero's icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tone {
    #[default]
    Dim,
    Good,
    Warn,
    Bad,
    Accent,
    /// Dimmer than `Dim` (an app that is off).
    Faint,
}

/// The tile in front of a line.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Tile {
    #[default]
    None,
    /// A quiet plate and a grey glyph.
    Quiet(&'static str),
    /// A plate and a glyph in the accent.
    Lit(&'static str),
    /// A coloured plate and a white glyph.
    Solid(&'static str, u32),
    /// A plate and a glyph in the colour of a tone.
    Soft(&'static str, Tone),
}

/// A line of a page: what it is (`row`, with the values read when the page was built) and how the design shows it.
pub struct Line {
    pub row: Row,
    /// Names the line over a refresh (what is open, what waits for a second click, what the search flashes).
    pub key: String,
    pub sub: String,
    pub tone: Tone,
    pub tile: Tile,
    pub lock: bool,
    /// The four signal bars lit (Wi-Fi), 1..=4.
    pub bars: Option<u32>,
    pub badge: Option<(String, Tone)>,
    pub weight: u16,
    /// A spinner: something is going on.
    pub busy: bool,
    /// The value of a `Cycle`, read when the page was built.
    pub val: String,
    /// The state of a `Switch` or `Toggle`; None: no such device (drawn disabled).
    pub on: Option<bool>,
    /// The volume of a `Slider`; None: nothing to set.
    pub vol: Option<f32>,
    /// The mute button beside a slider: what it does and whether it is muted now.
    pub mute: Option<(Op, bool)>,
    /// The values of a segmented row (`Topology`, `Plan`) and the current one.
    pub opts: Vec<String>,
    pub cur: Option<usize>,
    /// A taller row (the Windows Update status).
    pub big: bool,
    /// The buttons of the Windows Update "restart" state (the real one never has them).
    pub restart: bool,
    /// Drawn dim and not clickable (a switch whose master switch is off).
    pub dis: bool,
    /// A height of its own, DIPs.
    pub h: Option<f32>,
}

impl Line {
    /// A line for `row`; the live values it shows are read now.
    #[must_use]
    pub fn new(row: Row) -> Self {
        let mut l = Self {
            key: row.label().to_string(),
            row,
            sub: String::new(),
            tone: Tone::Dim,
            tile: Tile::None,
            lock: false,
            bars: None,
            badge: None,
            weight: 500,
            busy: false,
            val: String::new(),
            on: None,
            vol: None,
            mute: None,
            opts: Vec::new(),
            cur: None,
            big: false,
            restart: false,
            dis: false,
            h: None,
        };
        match &l.row {
            Row::Cycle(_, value, op) => {
                l.val = value();
                if is_seg(*op) {
                    (l.opts, l.cur) = choices(*op).unwrap_or_default();
                } else if matches!(op, Op::Opt(_)) {
                    l.val = choices(*op).and_then(|(v, c)| c.and_then(|c| v.get(c).cloned())).unwrap_or_else(|| "?".into());
                }
            }
            Row::Gauge(_, g) => {
                l.vol = os().gauge(*g);
                l.sub = more::gauge_text(*g, l.vol);
            }
            Row::Flag(_, f) => l.on = os().flag(f),
            Row::Switch(_, value, _) => l.on = value(),
            Row::Toggle { key, name, on_when, default, .. } => l.on = Some(is_on(os().reg_dword(key, name), *on_when, *default)),
            Row::Slider(_, src) => l.vol = os().audio_volume(*src),
            Row::Item(_, value, _) | Row::Info(_, value) => l.val = value.clone(),
            Row::Adapter(..) | Row::Wifi(..) => {}
        }
        l
    }

    #[must_use]
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = k.into();
        self
    }

    #[must_use]
    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.sub = s.into();
        self
    }

    #[must_use]
    pub fn tone(mut self, t: Tone) -> Self {
        self.tone = t;
        self
    }

    #[must_use]
    pub fn tile(mut self, t: Tile) -> Self {
        self.tile = t;
        self
    }

    #[must_use]
    pub fn weight(mut self, w: u16) -> Self {
        self.weight = w;
        self
    }

    #[must_use]
    pub fn busy(mut self, b: bool) -> Self {
        self.busy = b;
        self
    }

    #[must_use]
    pub fn mute(mut self, op: Op, muted: bool) -> Self {
        self.mute = Some((op, muted));
        self
    }
}

/// What a group shows besides its rows (and before them).
pub enum Extra {
    None,
    /// The picture of the displays and how they sit on the desk.
    Arrange,
    Hero(Hero),
    /// The battery card; None: this PC has no battery.
    Battery(Option<os::Battery>),
    Drives(Vec<Drive>),
    /// The desktop backgrounds the window offers (the demo's; the real one only has "Choose…").
    Walls(Vec<Wall>),
    /// The search box of a page's own list (the Apps page): what it says of the result ("3 of 12"), empty while nothing is typed.
    Search(String),
}

/// A big status card (the Network page).
pub struct Hero {
    pub d: &'static str,
    pub tone: Tone,
    pub title: String,
    pub sub: String,
    /// A button and what it does.
    pub act: Option<(String, Op)>,
}

/// The colour of a part of a drive's bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hue {
    Accent,
    /// The accent at 45 %.
    Soft,
    Yellow,
}

/// A drive: its name, the text at the right, and the parts of its bar: (share 0..1, colour, legend).
pub struct Drive {
    pub name: String,
    pub txt: String,
    pub parts: Vec<(f32, Hue, String)>,
}

pub struct Wall {
    pub label: String,
    pub fill: Fill,
    pub on: bool,
    /// The picture itself (the real wallpaper's thumbnail), when it has been decoded; else `fill` is drawn.
    pub pic: Option<std::sync::Arc<wall::Thumb>>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fill {
    Solid(u32),
    /// 160 degrees: top, middle (at the given position, 0..1), bottom.
    Gradient(u32, u32, u32, f32),
}

/// The button at the right of a group's title.
pub struct GroupAct {
    pub label: String,
    pub busy: bool,
    pub what: GAct,
}

#[derive(Clone)]
pub enum GAct {
    /// Looks for Wi-Fi networks again.
    WifiRefresh,
    Cmd(Cmd),
    /// Puts the device details on the clipboard.
    Copy,
}

pub struct Group {
    pub title: String,
    pub note: String,
    pub act: Option<GroupAct>,
    pub extra: Extra,
    pub rows: Vec<Line>,
    /// The Bluetooth radar under the rows (it moves while a search runs).
    pub radar: bool,
    pub empty: String,
    pub foot: String,
}

impl Group {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), note: String::new(), act: None, extra: Extra::None, rows: Vec::new(), radar: false, empty: String::new(), foot: String::new() }
    }

    #[must_use]
    pub fn rows(mut self, rows: Vec<Line>) -> Self {
        self.rows = rows;
        self
    }

    #[must_use]
    pub fn note(mut self, n: impl Into<String>) -> Self {
        self.note = n.into();
        self
    }

    #[must_use]
    pub fn extra(mut self, e: Extra) -> Self {
        self.extra = e;
        self
    }

    #[must_use]
    pub fn empty(mut self, e: impl Into<String>) -> Self {
        self.empty = e.into();
        self
    }

    #[must_use]
    pub fn foot(mut self, f: impl Into<String>) -> Self {
        self.foot = f.into();
        self
    }

    #[must_use]
    pub fn act(mut self, label: impl Into<String>, busy: bool, what: GAct) -> Self {
        self.act = Some(GroupAct { label: label.into(), busy, what });
        self
    }
}

pub struct Page {
    pub title: String,
    pub groups: Vec<Group>,
}

pub fn is_on(val: Option<u32>, on_when: u32, default: u32) -> bool {
    val.unwrap_or(default) == on_when
}

fn os_name(product: &str, build: u32) -> String {
    // the registry still says "Windows 10" on Windows 11
    if build >= 22000 { product.replace("Windows 10", "Windows 11") } else { product.to_string() }
}

pub fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// Words that select each page, in the order of `pages()`.
const PAGE_KEYS: [&[&str]; 13] = [
    &["display"],
    &["sound"],
    &["power", "battery", "storage"],
    &["network", "wifi", "ethernet"],
    &["bluetooth", "airplane"],
    &["apps"],
    &["notification"],
    &["personal", "color", "theme"],
    &["privacy", "webcam", "microphone", "location", "windowsdefender"],
    &["keyboard", "mouse", "typing"],
    &["dateandtime", "time", "region"],
    &["update"],
    &["about"],
];

/// The page a name or an ms-settings: address points at; the first page when nothing matches.
pub fn page_for(arg: &str) -> usize {
    let a = arg.to_ascii_lowercase();
    PAGE_KEYS.iter().position(|ks| ks.iter().any(|k| a.contains(k))).unwrap_or(0)
}

fn res_text() -> String {
    os().display_current().map_or("?".into(), |m| format!("{} x {}", m.w, m.h))
}

fn hz_text() -> String {
    os().display_current().map_or("?".into(), |m| format!("{} Hz", m.hz))
}

fn topo_text() -> String {
    if os().display_connected() < 2 {
        return t!("Один экран", "Single display", ja: "単一ディスプレイ", pt: "Tela única", zh: "单显示器").into();
    }
    os().display_topology().map_or("?".into(), |t| topo_name(t).into())
}

fn topo_name(t: display::Topo) -> &'static str {
    use display::Topo::*;
    match t {
        Extend => t!("Расширить", "Extend", ja: "拡張", pt: "Estender", zh: "扩展"),
        Duplicate => t!("Повторять", "Duplicate", ja: "複製", pt: "Duplicar", zh: "复制"),
        PcOnly => t!("Только этот ПК", "PC screen only", ja: "PC 画面のみ", pt: "Somente a tela do PC", zh: "仅电脑屏幕"),
        SecondOnly => t!("Только второй экран", "Second screen only", ja: "2 番目の画面のみ", pt: "Somente a segunda tela", zh: "仅第二屏幕"),
    }
}

/// The short names of the segmented control (the design's "Only 1", "Only 2").
pub fn topo_short(t: display::Topo) -> &'static str {
    use display::Topo::*;
    match t {
        Extend => t!("Расширить", "Extend"),
        Duplicate => t!("Повторять", "Duplicate"),
        PcOnly => t!("Только 1", "Only 1"),
        SecondOnly => t!("Только 2", "Only 2"),
    }
}

fn primary_text() -> String {
    match os().display_monitor_info() {
        Some((_, _, _, true)) => t!("Этот дисплей основной", "This is the main display", ja: "これがメイン ディスプレイです", pt: "Esta é a tela principal", zh: "这是主显示器").into(),
        Some(_) => t!("Сделать основным", "Make main", ja: "メインにする", pt: "Tornar principal", zh: "设为主显示器").into(),
        None => "?".into(),
    }
}

fn scale_text() -> String {
    os().display_scale().map_or("?".into(), |p| format!("{p}%"))
}

/// The HDR switch: None (drawn disabled) when the display cannot do HDR.
fn hdr_on() -> Option<bool> {
    os().display_hdr()
}

fn orient_text() -> String {
    os().display_orientation().and_then(|o| orient_names().get(o as usize).copied()).unwrap_or("?").into()
}

fn orient_names() -> [&'static str; 4] {
    [t!("Альбомная", "Landscape", ja: "横", pt: "Paisagem", zh: "横向"), t!("Книжная", "Portrait", ja: "縦", pt: "Retrato", zh: "纵向"), t!("Альбомная (перевёрнутая)", "Landscape (flipped)", ja: "横 (反転)", pt: "Paisagem (invertida)", zh: "横向（翻转）"), t!("Книжная (перевёрнутая)", "Portrait (flipped)", ja: "縦 (反転)", pt: "Retrato (invertido)", zh: "纵向（翻转）")]
}

/// Rows whose value is picked from a list (the others act on a click). Screen modes need a second screen.
pub fn listed(op: Op) -> bool {
    matches!(op, Op::Res | Op::Hz | Op::Orient | Op::Scale | Op::Monitor | Op::Plan | Op::Screen | Op::Sleep | Op::OutDev | Op::InDev | Op::Night | Op::UiLang | Op::Opt(_) | Op::PvTab)
        || (matches!(op, Op::Topology) && os().display_connected() > 1)
}

/// Rows drawn as a segmented control whenever they have a list (a click on a segment picks it).
pub fn is_seg(op: Op) -> bool {
    matches!(op, Op::Topology | Op::Plan | Op::PvTab | Op::Accent) || matches!(op, Op::Opt(o) if o.segmented())
}

/// The values a list offers and the index of the current one. None when there is nothing to choose.
pub fn choices(op: Op) -> Option<(Vec<String>, Option<usize>)> {
    let o = os();
    let list = match op {
        Op::UiLang => (LANGS.iter().map(|l| l.name().to_string()).collect(), LANGS.iter().position(|l| *l == lang!())),
        Op::Res => {
            let (modes, c) = (o.display_modes(), o.display_current()?);
            let r = display::resolutions(&modes);
            (r.iter().map(|(w, h)| format!("{w} x {h}")).collect(), r.iter().position(|&x| x == (c.w, c.h)))
        }
        Op::Hz => {
            let (modes, c) = (o.display_modes(), o.display_current()?);
            let r = display::rates_at(&modes, c);
            (r.iter().map(|h| format!("{h} Hz")).collect(), r.iter().position(|&h| h == c.hz))
        }
        Op::Orient => (orient_names().iter().map(|s| s.to_string()).collect(), o.display_orientation().map(|x| x as usize)),
        Op::Scale => {
            let (v, i) = o.display_scales()?;
            (v.iter().map(|p| format!("{p}%")).collect(), Some(i))
        }
        Op::Monitor => (o.display_monitor_names(), Some(o.display_selected())),
        Op::Topology if o.display_connected() > 1 => {
            let cur = o.display_topology();
            (display::Topo::ALL.iter().map(|&t| topo_short(t).to_string()).collect(), display::Topo::ALL.iter().position(|&t| Some(t) == cur))
        }
        Op::OutDev | Op::InDev => {
            let d = o.audio_devices(matches!(op, Op::InDev));
            (d.iter().map(|d| d.name.clone()).collect(), d.iter().position(|d| d.default))
        }
        Op::Night => (night_labels(), ext::NIGHT_STEPS.iter().position(|&k| k == o.night_kelvin())),
        Op::Opt(x) => o.opt_list(x),
        Op::Accent => (winrust_common::style::Accent::ALL.iter().map(|a| a.name().to_string()).collect(), Some(o.accent())),
        Op::PvTab => (more::Cap::ALL.iter().map(|c| c.name().to_string()).collect(), more::Cap::ALL.iter().position(|&c| c == more::pv_tab())),
        Op::Plan => o.power_plans(),
        Op::Screen => o.power_timeouts(power::Timeout::Screen),
        Op::Sleep => o.power_timeouts(power::Timeout::Sleep),
        _ => return None,
    };
    (!list.0.is_empty()).then_some(list)
}

/// What `choose` leaves for the window to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Done {
    Nothing,
    /// A display mode was changed: the window asks whether to keep it (15 s, then it goes back).
    Confirm,
    /// The interface language changed; the flag says whether the other programs will see it (lang.txt was written).
    Lang(bool),
    /// Windows did not take the change (usually: it needs administrator rights).
    Denied,
}

/// Applies entry `i` of `choices(op)`. Display modes are temporary until confirmed (the 15 s countdown, `Done::Confirm`).
pub fn choose(op: Op, i: usize) -> Done {
    let o = os();
    let (modes, cur) = (o.display_modes(), o.display_current());
    let mut done = Done::Nothing;
    match (op, cur) {
        (Op::Res, Some(c)) => {
            if let Some(&(w, h)) = display::resolutions(&modes).get(i) {
                let m = display::with_res(&modes, c, w, h);
                if m != c && o.display_apply(m, false) {
                    done = Done::Confirm;
                }
            }
        }
        (Op::Hz, Some(c)) => {
            if let Some(&hz) = display::rates_at(&modes, c).get(i)
                && hz != c.hz
                && o.display_apply(display::Mode { hz, ..c }, false)
            {
                done = Done::Confirm;
            }
        }
        (Op::Orient, _) if o.display_orientation() != Some(i as u32) && o.display_set_orientation(i as u32, false) => done = Done::Confirm,
        (Op::Scale, _) => drop(o.display_set_scale_index(i)),
        (Op::Monitor, _) => o.display_select(i),
        (Op::UiLang, _) => {
            if let Some(&l) = LANGS.get(i)
                && l != lang!()
            {
                done = Done::Lang(o.set_lang(l));
            }
        }
        (Op::Topology, _) => {
            if display::Topo::ALL.get(i).is_some_and(|&t| o.display_set_topology(t)) {
                done = Done::Confirm;
            }
        }
        (Op::OutDev | Op::InDev, _) => {
            if let Some(d) = o.audio_devices(matches!(op, Op::InDev)).get(i) {
                o.audio_set_default(&d.id);
            }
        }
        (Op::Night, _) => o.night_set(ext::NIGHT_STEPS.get(i).copied().unwrap_or(0)),
        (Op::Opt(x), _) => {
            if !o.opt_set(x, i) {
                done = Done::Denied;
            }
        }
        (Op::Accent, _) => o.accent_set(i),
        (Op::PvTab, _) => {
            if let Some(&c) = more::Cap::ALL.get(i) {
                more::set_pv_tab(c);
            }
        }
        (Op::Plan, _) => o.power_set_plan(i),
        (Op::Screen, _) => o.power_set_timeout(power::Timeout::Screen, i),
        (Op::Sleep, _) => o.power_set_timeout(power::Timeout::Sleep, i),
        _ => {}
    }
    done
}

/// The note the window shows after the language changed.
#[must_use]
pub fn lang_note(written: bool) -> String {
    if written {
        t!(
            "Язык изменён. Перезапустите программы WinRust (или выйдите из системы), чтобы он применился везде.",
            "Language changed. Restart the WinRust programs (or sign out) to apply it everywhere.",
            ja: "言語を変更しました。すべてに反映するには WinRust の各プログラムを再起動 (またはサインアウト) してください。",
            pt: "Idioma alterado. Reinicie os programas da WinRust (ou saia da sessão) para aplicá-lo em todos.",
            zh: "语言已切换。请重启 WinRust 程序（或注销后重新登录），让所有程序都生效。"
        )
    } else {
        t!(
            "Язык переключён только в этом окне: %ProgramData%\\WinRust\\lang.txt защищён от записи. Запустите установщик от имени администратора, чтобы он применился везде.",
            "Only this window switched: %ProgramData%\\WinRust\\lang.txt could not be written. Run the installer as administrator to apply the language everywhere.",
            ja: "このウィンドウだけが切り替わりました: %ProgramData%\\WinRust\\lang.txt に書き込めません。すべてに反映するにはインストーラーを管理者として実行してください。",
            pt: "Somente esta janela mudou: não foi possível gravar %ProgramData%\\WinRust\\lang.txt. Execute o instalador como administrador para aplicar o idioma em todos.",
            zh: "只有这个窗口切换了：无法写入 %ProgramData%\\WinRust\\lang.txt。请以管理员身份运行安装程序，才能让所有程序都生效。"
        )
    }
    .to_string()
}

pub fn night_labels() -> Vec<String> {
    ext::NIGHT_STEPS
        .iter()
        .map(|&k| match k {
            0 => t!("Выкл.", "Off", ja: "オフ", pt: "Desligado", zh: "关").to_string(),
            5500 => tf!("Слабый · {} K", "Low · {} K", ja: "弱 · {} K", pt: "Fraco · {} K", zh: "弱 · {} K", k),
            4800 => tf!("Средний · {} K", "Medium · {} K", ja: "中 · {} K", pt: "Médio · {} K", zh: "中 · {} K", k),
            4200 => tf!("Сильный · {} K", "Strong · {} K", ja: "強 · {} K", pt: "Forte · {} K", zh: "强 · {} K", k),
            _ => tf!("Максимальный · {} K", "Maximum · {} K", ja: "最大 · {} K", pt: "Máximo · {} K", zh: "最高 · {} K", k),
        })
        .collect()
}

fn night_text() -> String {
    os().night_text()
}

fn night_on() -> Option<bool> {
    Some(os().night_kelvin() != 0)
}

fn out_dev_text() -> String {
    os().audio_default_name(false)
}

fn in_dev_text() -> String {
    os().audio_default_name(true)
}

fn bt_on() -> Option<bool> {
    os().bt_state().bt
}

fn air_on() -> Option<bool> {
    os().bt_state().air
}

fn screen_text() -> String {
    os().power_timeout_text(power::Timeout::Screen)
}

fn sleep_text() -> String {
    os().power_timeout_text(power::Timeout::Sleep)
}

fn plan_text() -> String {
    os().power_plan_text()
}

fn proxy_text() -> String {
    os().proxy_text()
}

fn pac_text() -> String {
    os().pac_text()
}

pub const NET_PAGE: usize = 3;
pub const BT_PAGE: usize = 4;
pub const APPS_PAGE: usize = 5;
pub const NOTIFY_PAGE: usize = 6;
pub const PERSONAL_PAGE: usize = 7;
pub const PRIVACY_PAGE: usize = 8;
pub const KEYBOARD_PAGE: usize = 9;
pub const TIME_PAGE: usize = 10;
pub const WU_PAGE: usize = 11;
pub const ABOUT_PAGE: usize = 12;
pub const POWER_PAGE: usize = 2;

thread_local! { static EXPANDED: RefCell<Option<String>> = const { RefCell::new(None) } } // the adapter whose details show
thread_local! { static RADIO: std::cell::Cell<Option<(bool, bool)>> = const { std::cell::Cell::new(None) } }
thread_local! { static PROGRAMS: RefCell<Option<Vec<ext::App>>> = const { RefCell::new(None) } } // read when the Apps page opens
thread_local! { static APP_QUERY: RefCell<String> = const { RefCell::new(String::new()) } } // what the Apps page's own search box holds

/// The Apps page's own search box now holds `q` (the page shows the programs whose name or publisher has it).
pub fn set_app_query(q: &str) {
    APP_QUERY.with_borrow_mut(|a| *a = q.to_string());
}

/// Shows or hides the details of the adapter named `name`.
pub fn toggle_adapter(name: &str) {
    EXPANDED.with_borrow_mut(|e| *e = if e.as_deref() == Some(name) { None } else { Some(name.to_string()) });
}

/// The Apps page reads the installed programs again the next time it is built.
pub fn forget_programs() {
    PROGRAMS.with_borrow_mut(|p| *p = None);
}

/// The Wi-Fi switch: None (drawn disabled) without an adapter or with the hardware switch off.
fn radio_on() -> Option<bool> {
    RADIO.get().filter(|r| r.1).map(|r| r.0)
}

fn sound_muted() -> Option<bool> {
    os().audio_muted(false)
}

fn mic_muted() -> Option<bool> {
    os().audio_muted(true)
}

/// Lit bars (1..=4) for a WLAN signal quality (0..=100).
pub fn bars(q: u32) -> u32 {
    match q {
        0..=19 => 1,
        20..=44 => 2,
        45..=69 => 3,
        _ => 4,
    }
}

fn wifi_note(n: &wifi::Net) -> String {
    use wifi::Sec::*;
    match (n.connected, &n.profile, n.sec) {
        (true, ..) => t!("Подключено  ·  интернет", "Connected  ·  Internet", ja: "接続済み  ·  インターネット", pt: "Conectado  ·  Internet", zh: "已连接  ·  Internet").into(),
        (_, Some(_), _) => t!("Сохранена", "Saved", ja: "保存済み", pt: "Salva", zh: "已保存").into(),
        (_, _, None) if n.eap.is_some() => t!("Корпоративная (802.1X)  ·  имя и пароль", "Enterprise (802.1X)  ·  user name and password", ja: "エンタープライズ (802.1X)  ·  ユーザー名とパスワード", pt: "Empresarial (802.1X)  ·  nome de usuário e senha", zh: "企业级 (802.1X)  ·  用户名和密码").into(),
        (_, _, None) => t!("Корпоративная, сертификатом: не поддерживается", "Enterprise with certificates: not supported", ja: "証明書を使うエンタープライズ: 非対応", pt: "Empresarial com certificados: sem suporte", zh: "证书认证的企业级网络：不支持").into(),
        (_, _, Some(Open | Owe)) => t!("Открытая сеть", "Open network", ja: "オープン ネットワーク", pt: "Rede aberta", zh: "开放网络").into(),
        (_, _, Some(s)) => {
            let kind = match s {
                Wep { .. } => "WEP",
                Wpa { .. } => "WPA",
                Wpa3 => "WPA3",
                _ => "WPA2",
            };
            tf!("Защищена  ·  {}", "Secured  ·  {}", ja: "セキュリティ保護あり  ·  {}", pt: "Protegida  ·  {}", zh: "已加密  ·  {}", kind)
        }
    }
}

/// The status card of the Network page: how this PC is connected, or what to do about it.
#[must_use]
pub fn network_hero(adapters: &[net::Adapter], radio: Option<(bool, bool)>, connected: Option<&wifi::Net>, air: Option<bool>) -> Hero {
    let online = |a: &&net::Adapter| a.up && a.internet != Some(false);
    let line = |a: &net::Adapter, first: &str| {
        let ip = a.ipv4.first().map(|(ip, _)| ip.to_string());
        [Some(first.to_string()), net::speed_text(a.speed), ip, Some(t!("Интернет", "Internet").to_string())].into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} ")
    };
    if let Some(a) = adapters.iter().filter(|a| !a.wifi).find(online) {
        return Hero { d: look::ETHERNET, tone: Tone::Good, title: t!("Подключено по Ethernet", "Connected via Ethernet").into(), sub: line(a, &a.desc), act: None };
    }
    if let Some(a) = adapters.iter().filter(|a| a.wifi).find(online) {
        let first = connected.map_or_else(|| a.name.clone(), |n| n.ssid.clone());
        return Hero { d: look::WIFI, tone: Tone::Good, title: t!("Подключено по Wi-Fi", "Connected via Wi-Fi").into(), sub: line(a, &first), act: None };
    }
    let (sub, act) = if air == Some(true) {
        (t!("Включён режим «в самолёте»", "Airplane mode is on").to_string(), Some((t!("Выключить режим «в самолёте»", "Turn off airplane mode").to_string(), Op::Air)))
    } else if radio.is_none() {
        (t!("Подключите сетевой кабель", "Plug in a network cable").to_string(), None)
    } else if radio.is_some_and(|r| r.0 && r.1) {
        (t!("Выберите сеть Wi-Fi ниже или подключите кабель", "Pick a Wi-Fi network below or plug in a cable").to_string(), None)
    } else {
        (t!("Включите Wi-Fi или подключите сетевой кабель", "Turn on Wi-Fi or plug in a network cable").to_string(), radio.is_some_and(|r| r.1).then(|| (t!("Включить Wi-Fi", "Turn on Wi-Fi").to_string(), Op::Radio)))
    };
    Hero { d: look::WIFI_OFF, tone: Tone::Bad, title: t!("Нет подключения", "Not connected").into(), sub, act }
}

fn network_rows() -> Vec<Group> {
    let o = os();
    let (adapters, radio) = (o.net_adapters(), o.wifi_radio());
    RADIO.set(radio);
    let nets = if radio.is_some() { o.wifi_list() } else { vec![] };
    let air = o.bt_state().air;
    let mut gs = vec![Group::new(t!("Состояние", "Status")).extra(Extra::Hero(network_hero(&adapters, radio, nets.iter().find(|n| n.connected), air)))];
    if !adapters.is_empty() {
        let rows = adapters
            .into_iter()
            .map(|a| {
                let open = EXPANDED.with_borrow(|e| e.as_deref() == Some(a.name.as_str()));
                let (tone, tile) = if a.up && a.internet != Some(false) { (Tone::Good, Tile::Lit(if a.wifi { look::WIFI } else { look::ETHERNET })) } else { (Tone::Dim, Tile::Quiet(if a.wifi { look::WIFI } else { look::ETHERNET })) };
                let sub = format!("{}  ·  {}", a.status(), a.summary());
                Line::new(Row::Adapter(a, open)).subtitle(sub).tone(tone).tile(tile)
            })
            .collect();
        gs.push(Group::new(t!("Подключения", "Connections", ja: "接続", pt: "Conexões", zh: "连接")).rows(rows));
    }
    if let Some((_, hw)) = radio {
        let label: String = if hw { "Wi-Fi".into() } else { t!("Wi-Fi выключен кнопкой на устройстве", "Wi-Fi is off by the hardware switch", ja: "Wi-Fi がハードウェア スイッチでオフになっています", pt: "O Wi-Fi está desligado pela chave de hardware", zh: "Wi-Fi 已被硬件开关关闭").into() };
        let on = radio_on().unwrap_or(false) && air != Some(true);
        let sub = if air == Some(true) { t!("Включён режим «в самолёте»", "Airplane mode is on").to_string() } else if on { nets.iter().find(|n| n.connected).map_or_else(|| t!("Нет подключения", "Not connected").to_string(), |n| tf!("Подключено к {}", "Connected to {}", n.ssid)) } else { t!("Выключен", "Off").to_string() };
        let mut g = Group::new("Wi-Fi").rows(vec![Line::new(Row::Switch(label, radio_on, Op::Radio)).key("wifi-radio").subtitle(sub).tile(Tile::Solid(look::WIFI, 0x2f7bf6))]);
        if air == Some(true) {
            g = g.note(t!("Выключен в режиме «в самолёте»", "Off in airplane mode"));
        }
        gs.push(g);
        if on {
            let mut rows: Vec<Line> = nets
                .into_iter()
                .take(12)
                .map(|n| {
                    let note = wifi_note(&n);
                    let (tile, tone) = if n.connected { (Tile::Lit(look::WIFI), Tone::Good) } else { (Tile::Quiet(look::WIFI), Tone::Dim) };
                    let (q, lock, w, key) = (bars(n.quality), n.sec.is_none_or(|s| s.needs_key()), if n.connected { 600 } else { 500 }, format!("wifi:{}", n.ssid));
                    let mut l = Line::new(Row::Wifi(n, note.clone())).key(key).subtitle(note).tone(tone).tile(tile).weight(w);
                    (l.bars, l.lock) = (Some(q), lock);
                    l
                })
                .collect();
            rows.sort_by_key(|l| !matches!(&l.row, Row::Wifi(n, _) if n.connected));
            if radio.is_some_and(|r| r.0 && r.1) {
                rows.push(Line::new(Row::Cycle(t!("Другая сеть…", "Other network…", ja: "その他のネットワーク…", pt: "Outra rede…", zh: "其他网络…").into(), String::new, Op::Hidden)).key("wifi:other").subtitle(t!("Скрытая сеть: введите её имя", "Hidden network: type its name")).tile(Tile::Quiet(look::PLUS)));
            }
            gs.push(Group::new(t!("Сети", "Networks")).note(t!("Правый клик по сохранённой сети: забыть", "Right click a saved network to forget it")).act(t!("Обновить", "Refresh"), false, GAct::WifiRefresh).rows(rows));
        }
    } else {
        gs.push(Group::new("Wi-Fi").empty(t!("В этом ПК нет адаптера Wi-Fi · USB-адаптер добавит его", "No Wi-Fi adapter in this PC \u{b7} a USB Wi-Fi adapter adds one")));
    }
    let vpns = o.vpns();
    let mut g = Group::new(t!("VPN-подключения", "VPN connections", ja: "VPN 接続", pt: "Conexões VPN", zh: "VPN 连接"));
    if vpns.is_empty() {
        g = g.empty(t!("не настроены", "none set up", ja: "未設定", pt: "nenhuma configurada", zh: "尚未设置"));
    }
    let rows = vpns
        .into_iter()
        .map(|v| {
            let state = if v.on { t!("Подключено", "Connected", ja: "接続済み", pt: "Conectado", zh: "已连接") } else { t!("Отключено", "Disconnected", ja: "切断", pt: "Desconectado", zh: "已断开") };
            let (tone, tile) = if v.on { (Tone::Good, Tile::Lit(look::SHIELD)) } else { (Tone::Dim, Tile::Quiet(look::SHIELD)) };
            Line::new(Row::Item(v.name.clone(), state.into(), Cmd::Vpn(v))).subtitle(state).tone(tone).tile(tile)
        })
        .collect();
    gs.push(g.rows(rows));
    let proxy_row = toggle_row(t!("Использовать прокси-сервер", "Use a proxy server", ja: "プロキシ サーバーを使う", pt: "Usar um servidor proxy", zh: "使用代理服务器"), ext::INET, "ProxyEnable", 1, 0);
    let proxy_on = is_on(o.reg_dword(ext::INET, "ProxyEnable"), 1, 0);
    let mut rows = vec![Line::new(proxy_row).key("proxy").subtitle(if proxy_on { o.proxy_text() } else { t!("Выключен", "Off").to_string() })];
    if proxy_on {
        rows.push(Line::new(Row::Cycle(t!("Адрес прокси-сервера", "Proxy address", ja: "プロキシ アドレス", pt: "Endereço do proxy", zh: "代理地址").into(), proxy_text, Op::Proxy)).key("proxy-addr"));
        rows.push(Line::new(Row::Cycle(t!("Скрипт настройки (PAC)", "Setup script (PAC)", ja: "セットアップ スクリプト (PAC)", pt: "Script de configuração (PAC)", zh: "设置脚本 (PAC)").into(), pac_text, Op::Pac)).key("pac-addr"));
    }
    gs.push(Group::new(t!("Прокси-сервер", "Proxy", ja: "プロキシ", pt: "Proxy", zh: "代理")).rows(rows));
    gs
}

fn toggle_row(label: &str, key: &'static str, name: &'static str, on_when: u32, default: u32) -> Row {
    Row::Toggle { label: label.into(), key, name, on_when, default }
}

fn display_rows() -> Vec<Group> {
    let o = os();
    let layout = o.display_layout();
    let mut gs = vec![];
    if layout.len() > 1 {
        gs.push(Group::new(t!("Дисплеи", "Displays", ja: "ディスプレイ", pt: "Telas", zh: "显示器")).note(tf!("подключено: {}", "{} connected", layout.len())).extra(Extra::Arrange));
    }
    if o.display_connected() > 1 {
        gs.push(Group::new(t!("Несколько экранов", "Multiple displays", ja: "複数のディスプレイ", pt: "Várias telas", zh: "多显示器")).rows(vec![Line::new(Row::Cycle(t!("Когда подключено два экрана", "When two displays are connected").into(), topo_text, Op::Topology)).key("topology")]));
    }
    let (n, name) = o.display_monitor_info().map_or((1, String::new()), |(n, _, name, _)| (n, name));
    let mut rows = vec![
        Line::new(Row::Cycle(t!("Разрешение", "Resolution", ja: "解像度", pt: "Resolução", zh: "分辨率").into(), res_text, Op::Res)),
        Line::new(Row::Cycle(t!("Частота обновления", "Refresh rate", ja: "リフレッシュ レート", pt: "Taxa de atualização", zh: "刷新率").into(), hz_text, Op::Hz)),
        Line::new(Row::Cycle(t!("Масштаб", "Scale", ja: "拡大縮小", pt: "Escala", zh: "缩放").into(), scale_text, Op::Scale)),
        Line::new(Row::Cycle(t!("Ориентация", "Orientation", ja: "向き", pt: "Orientação", zh: "方向").into(), orient_text, Op::Orient)),
    ];
    if o.display_hdr().is_some() {
        rows.push(Line::new(Row::Switch(t!("HDR", "HDR", ja: "HDR", pt: "HDR", zh: "HDR").into(), hdr_on, Op::Hdr)).subtitle(t!("Ярче блики в играх и видео, которые это поддерживают", "Brighter highlights in games and videos that support it")));
    }
    let mut main = Line::new(Row::Cycle(t!("Основной дисплей", "Main display", ja: "メイン ディスプレイ", pt: "Tela principal", zh: "主显示器").into(), primary_text, Op::Primary));
    if matches!(o.display_monitor_info(), Some((_, _, _, true))) {
        main.badge = Some((main.val.clone(), Tone::Accent));
    }
    rows.push(main);
    gs.push(Group::new(tf!("Дисплей {}", "Display {}", ja: "ディスプレイ {}", pt: "Tela {}", zh: "显示器 {}", n)).note(name).rows(rows));
    let mut colour = vec![Line::new(Row::Switch(t!("Ночной свет", "Night light", ja: "夜間モード", pt: "Luz noturna", zh: "夜间模式").into(), night_on, Op::Night)).subtitle(t!("Тёплые цвета меньше утомляют глаза вечером", "Warmer colours that are easier on the eyes at night")).tile(Tile::Soft(look::MOON, Tone::Warn))];
    if o.night_kelvin() != 0 {
        colour.push(Line::new(Row::Cycle(t!("Сила", "Strength").into(), night_text, Op::Night)).key("night-strength"));
    }
    gs.push(Group::new(t!("Цвет", "Colour", ja: "色", pt: "Cor", zh: "颜色")).rows(colour));
    gs
}

fn sound_rows() -> Vec<Group> {
    let o = os();
    let mut gs = vec![
        Group::new(t!("Вывод", "Output", ja: "出力", pt: "Saída", zh: "输出")).rows(vec![
            Line::new(Row::Cycle(t!("Устройство вывода", "Output device", ja: "出力デバイス", pt: "Dispositivo de saída", zh: "输出设备").into(), out_dev_text, Op::OutDev)),
            Line::new(Row::Slider(t!("Громкость", "Volume", ja: "音量", pt: "Volume", zh: "音量").into(), audio::Src::Out)).mute(Op::Mute, sound_muted().unwrap_or(false)),
        ]),
        Group::new(t!("Ввод", "Input", ja: "入力", pt: "Entrada", zh: "输入")).rows(vec![
            Line::new(Row::Cycle(t!("Устройство ввода", "Input device", ja: "入力デバイス", pt: "Dispositivo de entrada", zh: "输入设备").into(), in_dev_text, Op::InDev)),
            {
                let muted = mic_muted().unwrap_or(false);
                let l = Line::new(Row::Slider(t!("Микрофон", "Microphone", ja: "マイク", pt: "Microfone", zh: "麦克风").into(), audio::Src::Mic)).mute(Op::MicMute, muted);
                if muted { l.subtitle(t!("Выключен · приложения слышат тишину", "Muted \u{b7} apps hear silence")) } else { l }
            },
        ]),
    ];
    let apps = o.audio_apps();
    let mut g = Group::new(t!("Громкость приложений", "Volume per app", ja: "アプリごとの音量", pt: "Volume por app", zh: "应用音量")).note(t!("Приложения со звуком сейчас", "Apps with sound right now"));
    if apps.is_empty() {
        g = g.empty(t!("Приложений со звуком сейчас нет", "No apps are playing sound right now"));
    }
    g = g.rows(apps.into_iter().map(|(pid, name)| Line::new(Row::Slider(name, audio::Src::App(pid))).key(format!("app:{pid}")).tile(Tile::Quiet(look::SPEAKER))).collect());
    gs.push(g);
    gs
}

fn power_rows() -> Vec<Group> {
    let o = os();
    let bat = o.power_battery();
    let plan = Line::new(Row::Cycle(t!("Схема питания", "Power plan", ja: "電源プラン", pt: "Plano de energia", zh: "电源计划").into(), plan_text, Op::Plan));
    let drives = o
        .power_drives()
        .into_iter()
        .map(|(l, free, total)| {
            let used = total.saturating_sub(free);
            let share = if total == 0 { 0.0 } else { used as f32 / total as f32 };
            Drive {
                name: tf!("Диск {}:", "Drive {}:", ja: "ドライブ {}:", pt: "Unidade {}:", zh: "驱动器 {}:", l),
                txt: tf!("{} свободно из {}", "{} free of {}", ja: "{} 空き / {}", pt: "{} livres de {}", zh: "{} 可用，共 {}", power::size(free), power::size(total)),
                parts: vec![(share, Hue::Accent, tf!("Занято {}", "Used {}", power::size(used)))],
            }
        })
        .collect();
    let bin = o.bin_bytes();
    let temp_ready = o.temp_measured();
    let mut temp = Line::new(Row::Item(t!("Временные файлы (старше суток)", "Temporary files (older than a day)", ja: "一時ファイル (1 日以上前)", pt: "Arquivos temporários (com mais de um dia)", zh: "临时文件（超过一天）").into(), o.temp_text(), Cmd::CleanTemp)).subtitle(t!("Старше суток", "Older than a day"));
    temp.busy = !temp_ready;
    let recycle = Line::new(Row::Item(t!("Корзина", "Recycle Bin", ja: "ごみ箱", pt: "Lixeira", zh: "回收站").into(), if bin == 0 { t!("пуста", "empty", ja: "空", pt: "vazia", zh: "空").into() } else { power::size(bin) }, Cmd::EmptyBin)).subtitle(if bin == 0 { String::new() } else { power::size(bin) });
    vec![
        Group::new(if bat.is_some() { t!("Батарея", "Battery", ja: "バッテリー", pt: "Bateria", zh: "电池") } else { t!("Источник питания", "Power source") }).extra(Extra::Battery(bat)),
        Group::new(t!("Питание", "Power", ja: "電源", pt: "Energia", zh: "电源")).rows(vec![
            plan,
            Line::new(Row::Cycle(t!("Отключать экран через", "Turn screen off after", ja: "ディスプレイの電源を切るまでの時間", pt: "Desligar a tela após", zh: "在此时间后关闭屏幕").into(), screen_text, Op::Screen)),
            Line::new(Row::Cycle(t!("Переходить в сон через", "Sleep after", ja: "スリープするまでの時間", pt: "Suspender após", zh: "在此时间后进入睡眠").into(), sleep_text, Op::Sleep)),
        ]),
        Group::new(t!("Хранилище", "Storage", ja: "ストレージ", pt: "Armazenamento", zh: "存储")).extra(Extra::Drives(drives)),
        Group::new(t!("Очистка", "Clean up", ja: "ディスク クリーンアップ", pt: "Limpeza de disco", zh: "磁盘清理")).rows(vec![
            Line::new(toggle_row(t!("Контроль памяти (автоочистка)", "Storage Sense (auto cleanup)", ja: "ストレージ センサー (自動クリーンアップ)", pt: "Sensor de Armazenamento (limpeza automática)", zh: "存储感知（自动清理）"), STORAGE_SENSE, "01", 1, 0)).key("storage-sense").subtitle(t!("Сам удаляет временные файлы и старое из корзины", "Removes temporary files and old Recycle Bin items on its own")),
            temp,
            recycle,
        ]),
    ]
}

/// A glyph for a Bluetooth device, guessed from its name (the radio API gives no kind).
#[must_use]
pub fn device_icon(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| n.contains(k));
    if has(&["mouse", "master", "mx "]) {
        "M12 3a6 6 0 0 1 6 6v6a6 6 0 0 1-12 0V9a6 6 0 0 1 6-6zM12 7v3"
    } else if has(&["keyboard", "keychron", "k3"]) {
        "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8"
    } else if has(&["buds", "airpods", "pods"]) {
        "M8 6a3 3 0 0 1 3 3v6H8a3 3 0 0 1 0-6zM16 6a3 3 0 0 0-3 3v6h3a3 3 0 0 0 0-6z"
    } else if has(&["wh-", "head", "wf-", "bose"]) {
        "M4 14v-2a8 8 0 0 1 16 0v2M4 14h3v6H5a1 1 0 0 1-1-1zM20 14h-3v6h2a1 1 0 0 0 1-1z"
    } else if has(&["speaker", "flip", "jbl", "boom"]) {
        "M7 3h10v18H7zM12 14a2.5 2.5 0 1 0 0 .01M12 7h.01"
    } else if has(&["pixel", "iphone", "galaxy s", "phone"]) {
        "M8 3h8v18H8zM11.5 18h1"
    } else {
        look::BLUETOOTH
    }
}

fn bt_rows() -> Vec<Group> {
    let o = os();
    let s = o.bt_state();
    let on = s.bt == Some(true) && s.air != Some(true);
    let label: String = if s.bt.is_some() { "Bluetooth".into() } else { t!("Bluetooth: адаптера нет", "Bluetooth: no adapter", ja: "Bluetooth: アダプターなし", pt: "Bluetooth: sem adaptador", zh: "蓝牙：没有适配器").into() };
    let pc = o.computer_name();
    let sub = if s.air == Some(true) { t!("Выключен в режиме «в самолёте»", "Off in airplane mode").to_string() } else if on { tf!("Виден устройствам рядом как {}", "Visible to nearby devices as {}", pc) } else { t!("Выключен", "Off").to_string() };
    let mut gs = vec![Group::new("").rows(vec![
        Line::new(Row::Switch(label, bt_on, Op::Bt)).key("bt-radio").subtitle(sub).tile(Tile::Solid(look::BLUETOOTH, 0x2563eb)),
        Line::new(Row::Switch(t!("Режим «в самолёте»", "Airplane mode", ja: "機内モード", pt: "Modo avião", zh: "飞行模式").into(), air_on, Op::Air)).subtitle(t!("Выключает Wi-Fi и Bluetooth", "Turns off Wi-Fi and Bluetooth")).tile(Tile::Solid(look::PLANE, 0xf08c1a)),
    ])];
    let paired: Vec<Line> = s
        .paired
        .iter()
        .map(|d| {
            let st = if !on {
                t!("Bluetooth выключен", "Bluetooth is off")
            } else if d.connected {
                t!("Подключено", "Connected", ja: "接続済み", pt: "Conectado", zh: "已连接")
            } else {
                t!("Сопряжено · не подключено", "Paired \u{b7} not connected")
            };
            let live = on && d.connected;
            Line::new(Row::Item(d.name.clone(), st.into(), Cmd::BtUnpair(d.id.clone()))).key(format!("bt:{}", d.id)).subtitle(st).tone(if live { Tone::Good } else { Tone::Dim }).tile(if live { Tile::Lit(device_icon(&d.name)) } else { Tile::Quiet(device_icon(&d.name)) })
        })
        .collect();
    let none = paired.is_empty();
    gs.push(Group::new(t!("Сопряжённые устройства", "Paired devices", ja: "ペアリング済みデバイス", pt: "Dispositivos pareados", zh: "已配对设备")).rows(paired).empty(if none { t!("Сопряжённых устройств пока нет", "No paired devices yet") } else { "" }));
    if on {
        let near: Vec<Line> = s
            .nearby
            .iter()
            .filter(|d| !s.paired.iter().any(|p| p.id == d.id))
            .map(|d| Line::new(Row::Item(d.name.clone(), t!("Сопрячь", "Pair", ja: "ペアリング", pt: "Parear", zh: "配对").into(), Cmd::BtPair(d.id.clone()))).key(format!("near:{}", d.id)).subtitle(t!("Готово к сопряжению", "Ready to pair")).tile(Tile::Quiet(device_icon(&d.name))))
            .collect();
        let mut g = Group::new(t!("Устройства поблизости", "Nearby devices", ja: "近くのデバイス", pt: "Dispositivos próximos", zh: "附近的设备")).act(if s.scanning { t!("Поиск", "Searching") } else { t!("Найти устройства", "Find devices", ja: "デバイスを検索", pt: "Procurar dispositivos", zh: "查找设备") }, s.scanning, GAct::Cmd(Cmd::BtScan));
        g.radar = s.scanning;
        g.empty = if !s.scanning && near.is_empty() { t!("Нажмите «Найти устройства» и переведите своё в режим сопряжения", "Press Find devices and put yours in pairing mode").into() } else { String::new() };
        gs.push(g.rows(near));
    }
    gs
}

fn apps_rows(q: &str) -> Vec<Group> {
    let list = PROGRAMS.with_borrow_mut(|p| p.get_or_insert_with(|| os().programs()).clone());
    let q = q.trim().to_lowercase();
    let shown: Vec<&ext::App> = list.iter().filter(|a| q.is_empty() || format!("{} {}", a.name, a.publisher).to_lowercase().contains(&q)).collect();
    let rows = shown
        .iter()
        .take(400)
        .map(|a| {
            let sub = if a.publisher.is_empty() { a.version.clone() } else { format!("{} · {}", a.publisher, a.version) };
            Line::new(Row::Item(a.name.clone(), a.version.clone(), Cmd::Uninstall(a.uninstall.clone()))).subtitle(sub).tile(Tile::Solid(look::APP_GRID, 0x6b7187))
        })
        .collect();
    let count = if q.is_empty() { String::new() } else { tf!("{} из {}", "{} of {}", ja: "{} / {}", pt: "{} de {}", zh: "{} / {}", shown.len(), list.len()) };
    let head = Group::new(tf!("Приложений: {}", "{} apps", list.len())).extra(Extra::Search(count));
    let mut g = Group::new("").rows(rows);
    if shown.is_empty() && !list.is_empty() {
        g = g.empty(tf!("Нет приложений по запросу «{}». Попробуйте часть названия или издателя.", "No apps match “{}”. Try part of the name or the publisher.", ja: "「{}」に一致するアプリはありません。名前または発行元の一部をお試しください。", pt: "Nenhum app corresponde a “{}”. Tente parte do nome ou do editor.", zh: "没有与“{}”匹配的应用。请尝试名称或发布者的一部分。", q));
    }
    vec![head, g]
}

fn wu_rows() -> Vec<Group> {
    let o = os();
    let (s, extra) = (o.wu_state(), o.wu_extra());
    let n = s.found.as_ref().map_or(0, Vec::len);
    let installed = extra.last_install.map(|w| tf!("установлено: {}", "installed {}", more::when_sentence(w)));
    let (title, sub, tile) = match (&s.busy, &s.found) {
        (Some(b), _) => (b.clone(), extra.progress.map_or_else(|| t!("Это займёт некоторое время · можно работать", "This takes a while \u{b7} you can keep working").to_string(), |p| tf!("{}% · можно работать", "{}% \u{b7} you can keep working", p)), Tile::Lit(look::REFRESH)),
        (_, _) if extra.restart => (t!("Установлено. Перезагрузите, чтобы завершить", "Installed. Restart to finish").to_string(), t!("Windows завершит обновление при перезагрузке", "Windows finishes the updates while it restarts").to_string(), Tile::Soft(look::WARN, Tone::Warn)),
        // not looked in this window yet: what Windows itself last did, when it says
        (_, None) => match extra.last_check {
            Some(w) => (tf!("Windows проверяла: {}", "Windows last checked {}", more::when_sentence(w)), installed.clone().unwrap_or_else(|| t!("Нажмите «Проверить», чтобы увидеть, что ждёт установки", "Check for updates to see what is waiting").to_string()), Tile::Quiet(look::REFRESH)),
            None => (t!("Ещё не проверялось", "Not checked yet", ja: "まだ確認していません", pt: "Ainda não verificado", zh: "尚未检查").to_string(), s.note.clone(), Tile::Quiet(look::REFRESH)),
        },
        // the verdict is the title; the line under it says when Windows last looked (and what it last installed)
        (_, Some(f)) if f.is_empty() => {
            let looked = extra.last_check.map(|w| tf!("Последняя проверка: {}", "Last checked {}", more::when_sentence(w)));
            let sub = [looked, installed.clone()].into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} ");
            (t!("Всё установлено", "You\u{2019}re up to date", ja: "最新の状態です", pt: "Atualizado", zh: "已是最新").to_string(), sub, Tile::Soft(look::CHECK, Tone::Good))
        }
        (_, Some(_)) => (tf!("Доступно обновлений: {}", "{} updates are available", ja: "利用可能な更新プログラム: {}", pt: "Atualizações disponíveis: {}", zh: "可用更新：{}", n), t!("Для установки нужны права администратора", "Installing needs administrator rights").to_string(), Tile::Soft(look::WARN, Tone::Warn)),
    };
    let cmd = if s.found.as_ref().is_some_and(|f| !f.is_empty()) && s.busy.is_none() { Cmd::Install } else { Cmd::Check };
    let mut status = Line::new(Row::Item(title, String::new(), cmd)).key("wu-status").subtitle(sub).tile(tile).weight(600).busy(s.busy.is_some());
    (status.big, status.restart) = (true, extra.restart && s.busy.is_none());
    // what the disabled button says while something runs: installing what was found, or looking
    status.val = if s.found.as_ref().is_some_and(|f| !f.is_empty()) { t!("Установка", "Installing") } else { t!("Проверка", "Checking") }.into();
    let mut gs = vec![Group::new("").rows(vec![status])];
    if s.busy.is_some() || n > 0 {
        let rows = s
            .found
            .iter()
            .flatten()
            .take(30)
            .enumerate()
            .map(|(i, t)| {
                // what each update is doing: the first takes the progress of the run, the next waits for it (the mockup's reading)
                let state = if extra.restart {
                    t!("Установлено · ждёт перезагрузки", "Installed \u{b7} waiting for restart").to_string()
                } else {
                    match (s.busy.is_some(), extra.progress) {
                        (true, Some(p)) if i == 0 || p > 85 => tf!("Установка · {}%", "Installing \u{b7} {}%", if i == 0 { (u32::from(p) * 115 / 100).min(100) } else { ((u32::from(p) - 85) * 66 / 10).min(100) }),
                        (true, Some(_)) => t!("Ждёт", "Waiting").to_string(),
                        (true, None) => String::new(),
                        (false, _) => t!("Готово к загрузке", "Ready to download").to_string(),
                    }
                };
                Line::new(Row::Info(t.clone(), String::new())).subtitle(state).tile(Tile::Quiet(look::DOWNLOAD))
            })
            .collect();
        gs.push(Group::new(t!("Обновления", "Updates")).rows(rows));
    }
    gs.push(Group::new("").foot(t!("Сам WinRust обновляется отдельно: настройки панели → Обновления.", "WinRust itself updates separately: Bar settings \u{2192} Updates.")));
    gs
}

/// The walls the window offers: only the demo has any.
fn wall_tiles() -> Vec<Wall> {
    os().wallpapers()
}

fn personal_rows() -> Vec<Group> {
    let o = os();
    vec![
        Group::new(t!("Фон рабочего стола", "Desktop background", ja: "デスクトップの背景", pt: "Plano de fundo", zh: "桌面背景")).note(o.wallpaper_name()).extra(Extra::Walls(wall_tiles())),
        Group::new(t!("Оформление", "Appearance", ja: "外観", pt: "Aparência", zh: "外观")).rows(vec![
            Line::new(toggle_row(t!("Тёмная тема для приложений", "Dark theme for apps", ja: "アプリのダーク テーマ", pt: "Tema escuro para apps", zh: "应用深色主题"), PERSONALIZE, "AppsUseLightTheme", 0, 1)).subtitle(t!("Это окно, файловый менеджер, диспетчер задач", "This window, the file manager, Task Manager")),
            Line::new(toggle_row(t!("Тёмная тема для системы", "Dark theme for the system", ja: "システムのダーク テーマ", pt: "Tema escuro para o sistema", zh: "系统深色主题"), PERSONALIZE, "SystemUsesLightTheme", 0, 1)).subtitle(t!("Панель, Пуск и уведомления", "The bar, Start and notifications")),
            Line::new(toggle_row(t!("Эффекты прозрачности", "Transparency effects", ja: "透明効果", pt: "Efeitos de transparência", zh: "透明效果"), PERSONALIZE, "EnableTransparency", 1, 1)),
            Line::new(toggle_row(t!("Акцентный цвет на заголовках окон", "Accent colour on title bars", ja: "タイトル バーにアクセント カラーを表示", pt: "Cor de destaque nas barras de título", zh: "标题栏上的强调色"), r"Software\Microsoft\Windows\DWM", "ColorPrevalence", 1, 0)),
            {
                let mut l = Line::new(Row::Cycle(t!("Акцентный цвет", "Accent colour", ja: "アクセント カラー", pt: "Cor de destaque", zh: "强调色").into(), String::new, Op::Accent)).key("accent");
                l.h = Some(48.0);
                l
            },
        ]),
        Group::new(t!("Язык", "Language", ja: "言語", pt: "Idioma", zh: "语言")).rows(vec![Line::new(Row::Cycle(t!("Язык интерфейса", "Interface language", ja: "表示言語", pt: "Idioma da interface", zh: "界面语言").into(), lang_text, Op::UiLang))]),
    ]
}

fn about_rows() -> Vec<Group> {
    let o = os();
    let reg = |n: &str| o.windows_value(n);
    let build: u32 = reg("CurrentBuild").parse().unwrap_or(0);
    let a = o.about_info();
    let info = |l: &str, v: String| Line::new(Row::Info(l.into(), v));
    let name = o.computer_name();
    let mut pc = Line::new(Row::Item(t!("Имя компьютера", "Computer name", ja: "コンピューター名", pt: "Nome do computador", zh: "计算机名").into(), t!("Переименовать", "Rename").into(), Cmd::Rename)).key("pcname");
    pc.sub = match &a.pc_pending {
        Some(n) => tf!("{} · после перезагрузки: {}", "{} \u{b7} after a restart: {}", name, n),
        None => name,
    };
    if a.pc_pending.is_some() {
        pc.tone = Tone::Warn;
    }
    let mut device = vec![pc];
    let mut push = |label: &str, v: String| {
        if !v.is_empty() {
            device.push(info(label, v));
        }
    };
    push(t!("Процессор", "Processor", ja: "プロセッサ", pt: "Processador", zh: "处理器"), a.cpu.clone());
    push(t!("Установленная память", "Installed memory", ja: "実装メモリ", pt: "Memória instalada", zh: "已安装的内存"), if a.ram.is_empty() { o.ram_text() } else { a.ram.clone() });
    for (i, g) in a.gpus.iter().enumerate() {
        let label = t!("Графика", "Graphics", ja: "グラフィックス", pt: "Gráficos", zh: "显卡");
        push(&if i == 0 { label.to_string() } else { format!("{label} {}", i + 1) }, g.clone());
    }
    push(t!("Тип системы", "System type", ja: "システムの種類", pt: "Tipo de sistema", zh: "系统类型"), a.sys_type.clone());
    let mut windows = vec![
        info(t!("Выпуск", "Edition", ja: "エディション", pt: "Edição", zh: "发行版"), os_name(&reg("ProductName"), build)),
        info(t!("Версия", "Version", ja: "バージョン", pt: "Versão", zh: "版本"), reg("DisplayVersion")),
        info(t!("Сборка", "Build", ja: "ビルド", pt: "Compilação", zh: "内部版本"), if a.build.is_empty() { reg("CurrentBuild") } else { a.build.clone() }),
    ];
    if !a.winrust.is_empty() {
        windows.push(info("WinRust", a.winrust.clone()));
    }
    vec![Group::new(t!("Устройство", "Device", ja: "デバイス", pt: "Dispositivo", zh: "设备")).act(t!("Копировать", "Copy"), false, GAct::Copy).rows(device), Group::new("Windows").rows(windows)]
}

/// The titles of the pages, in the order of the sidebar.
#[must_use]
pub fn page_title(i: usize) -> String {
    match i {
        0 => t!("Экран", "Display", ja: "ディスプレイ", pt: "Tela", zh: "显示器"),
        1 => t!("Звук", "Sound", ja: "サウンド", pt: "Som", zh: "声音"),
        2 => t!("Питание и диски", "Power & storage", ja: "電源とストレージ", pt: "Energia e armazenamento", zh: "电源和存储"),
        3 => t!("Сеть", "Network", ja: "ネットワーク", pt: "Rede", zh: "网络"),
        4 => "Bluetooth",
        5 => t!("Приложения", "Apps", ja: "アプリ", pt: "Apps", zh: "应用"),
        NOTIFY_PAGE => t!("Уведомления", "Notifications", ja: "通知", pt: "Notificações", zh: "通知"),
        PERSONAL_PAGE => t!("Персонализация", "Personalization", ja: "個人用設定", pt: "Personalização", zh: "个性化"),
        PRIVACY_PAGE => t!("Конфиденциальность и защита", "Privacy & security", ja: "プライバシーとセキュリティ", pt: "Privacidade e segurança", zh: "隐私和安全"),
        KEYBOARD_PAGE => t!("Клавиатура и мышь", "Keyboard & mouse", ja: "キーボードとマウス", pt: "Teclado e mouse", zh: "键盘和鼠标"),
        TIME_PAGE => t!("Дата и время", "Date & time", ja: "日付と時刻", pt: "Data e hora", zh: "日期和时间"),
        WU_PAGE => t!("Центр обновления", "Windows Update", ja: "Windows Update", pt: "Windows Update", zh: "Windows 更新"),
        _ => t!("О системе", "About", ja: "バージョン情報", pt: "Sobre", zh: "关于"),
    }
    .into()
}

pub const PAGES: usize = PAGE_KEYS.len();

/// Page `i`, built from what the system says now (the Apps page keeps the list of programs it read when it was opened).
#[must_use]
pub fn page(i: usize) -> Page {
    page_with(i, &APP_QUERY.with_borrow(Clone::clone))
}

/// Page `i` with `apps` in the Apps page's own search box.
fn page_with(i: usize, apps: &str) -> Page {
    let groups = match i {
        0 => display_rows(),
        1 => sound_rows(),
        2 => power_rows(),
        NET_PAGE => network_rows(),
        BT_PAGE => bt_rows(),
        APPS_PAGE => apps_rows(apps),
        NOTIFY_PAGE => more::notify_rows(),
        PERSONAL_PAGE => personal_rows(),
        PRIVACY_PAGE => more::privacy_rows(),
        KEYBOARD_PAGE => more::keyboard_rows(),
        TIME_PAGE => more::time_rows(),
        WU_PAGE => wu_rows(),
        _ => about_rows(),
    };
    Page { title: page_title(i), groups }
}

/// Every page (what the search looks through).
#[must_use]
pub fn pages() -> Vec<Page> {
    (0..PAGES).map(|i| page_with(i, "")).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// the arrangement picture

/// Desktop pixels <-> the picture's pixels.
pub struct Map {
    s: f32,
    cx: f32,
    cy: f32,
    ox: f32,
    oy: f32,
}

/// A rectangle in whole pixels (what `windows_sys` calls a `RECT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Box4 {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Map {
    pub fn dev(&self, x: i32, y: i32) -> (i32, i32) {
        ((self.ox + (x as f32 - self.cx) * self.s) as i32, (self.oy + (y as f32 - self.cy) * self.s) as i32)
    }
    pub fn desk(&self, x: i32, y: i32) -> (i32, i32) {
        (((x as f32 - self.ox) / self.s + self.cx) as i32, ((y as f32 - self.oy) / self.s + self.cy) as i32)
    }
}

/// Fits the displays into `bx`, with room around them to drag a display to the other side.
pub fn arr_map(mons: &[display::Mon], bx: Box4) -> Map {
    let l = mons.iter().map(|m| m.x).min().unwrap_or(0);
    let t = mons.iter().map(|m| m.y).min().unwrap_or(0);
    let r = mons.iter().map(|m| m.x + m.w).max().unwrap_or(1);
    let b = mons.iter().map(|m| m.y + m.h).max().unwrap_or(1);
    let (w, h) = (((r - l) as f32 * 1.8).max(1.0), ((b - t) as f32 * 1.8).max(1.0));
    let s = ((bx.right - bx.left) as f32 / w).min((bx.bottom - bx.top) as f32 / h).max(0.0001);
    Map { s, cx: (l + r) as f32 / 2.0, cy: (t + b) as f32 / 2.0, ox: (bx.left + bx.right) as f32 / 2.0, oy: (bx.top + bx.bottom) as f32 / 2.0 }
}

/// The volume (0..=1) a click or drag at `x` means on a slider whose bar runs from `l` to `r`.
pub fn slider_value(x: i32, l: i32, r: i32) -> f32 {
    (((x - l) as f32 / (r - l).max(1) as f32).clamp(0.0, 1.0) * 100.0).round() / 100.0
}

/// Ends the confirmation of a display change: `keep` saves the mode, otherwise the saved one comes back.
pub fn finish_confirm(keep: bool) {
    let o = os();
    match (keep, o.display_current()) {
        (true, Some(m)) => {
            o.display_apply(m, true);
        }
        _ => o.display_revert(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--nightlight") {
        return ext::night_helper(); // the colour filter's background process, see ext.rs
    }
    if let Some(code) = demo::cli(&args) {
        winrust_ui::window::quit(code as u32);
    }
    app::run(args.first().map_or(0, |a| page_for(a)));
}
