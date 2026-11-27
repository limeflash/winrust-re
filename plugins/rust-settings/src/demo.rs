//! The demo source: the mockup's data (two displays, a laptop battery, paired and nearby devices, six Wi-Fi networks, twelve programs,
//! the update states), changes kept in memory, every call that would change the machine written to a log. `--render`, `--bench`,
//! `--selftest` and the tests run on it: nothing here reads or writes anything of the system (the registry, the displays, the audio
//! devices, the radios, the network, the files), and no window is made.
//!
//! `cli` is the command line of those modes; `app(state)` is the window in a state of the design's "Preview states" list.
use std::{cell::RefCell, net::Ipv4Addr, rc::Rc};

use winrust_ui::core::App;

use crate::{
    Fill, Wall, audio, bt,
    display::{Mode, Mon, Topo},
    ext,
    more::{CApp, Cap, Consent, DATE_PATS, Flag, Gauge, Layout, NotifyApp, Opt, RawDesk, Security, When, Who, fmt_date, group_desktop},
    net,
    os::{Battery, Note, Os, Ping, WuExtra, set_os},
    power, wifi, wu,
};
use windows_sys::core::GUID;

/// What `--render-all` goes through: the states of the design's "Preview states" list, the pages, and a few more.
pub const STATES: [&str; 59] = [
    "display", "sound", "power", "network", "bluetooth", "apps", "update", "personal", "about", "battery-charging", "battery-on-battery", "battery-low", "battery-none", "net-wifi", "net-ethernet", "net-no-wifi-adapter",
    "net-offline", "net-airplane", "identify", "bt-paired", "bt-searching", "bt-needs-pin", "bt-off", "wu-up-to-date", "wu-available", "wu-installing", "wu-restart", "dialog-none", "dialog-keep-display", "appearance-dark",
    "appearance-light", "appearance-night-light", "wifi-password", "wifi-connected-open", "adapter-open", "dropdown", "search", "search-page", "search-none", "display-single", "sound-muted", "power-plans-many", "update-checking",
    "bt-pairing", "network-hidden", "proxy-on", "personal-light", "apps-many", "apps-search", "apps-none", "notify", "notify-open", "notify-off", "keyboard", "time", "privacy", "privacy-camera-off", "privacy-scanning",
    "privacy-location",
];

fn guid() -> GUID {
    GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] }
}

fn mode(w: u32, h: u32, hz: u32) -> Mode {
    Mode { w, h, hz }
}

const GB: u64 = 1 << 30;

/// Everything the demo keeps.
struct State {
    mons: Vec<Mon>,
    sel: usize,
    names: Vec<String>,
    modes: Vec<Vec<Mode>>,
    cur: Vec<Mode>,
    scales: Vec<usize>,
    orient: Vec<u32>,
    prev: Option<(Vec<Mode>, Vec<u32>, Topo)>,
    topo: Topo,
    hdr: Vec<Option<bool>>,
    night: u32,
    out: Vec<(String, String)>,
    inp: Vec<(String, String)>,
    out_def: usize,
    in_def: usize,
    vol: [f32; 2],
    mute: [bool; 2],
    apps: Vec<(u32, String, f32)>,
    bt: bt::State,
    battery: Option<Battery>,
    plans: Vec<String>,
    plan: usize,
    screen: usize,
    sleep: usize,
    temp_ready: bool,
    bin: u64,
    adapters: Vec<net::Adapter>,
    radio: Option<(bool, bool)>,
    nets: Vec<wifi::Net>,
    vpns: Vec<ext::Vpn>,
    reg: std::collections::HashMap<(String, String), u32>,
    proxy: String,
    pac: String,
    programs: Vec<ext::App>,
    wu: wu::State,
    wu_extra: WuExtra,
    walls: Vec<(String, Fill)>,
    wall: usize,
    lang: winrust_common::Lang,
    /// The new pages: the entry each list is on, the gauges, the flags that are not one registry value, the apps.
    opts: std::collections::HashMap<String, usize>,
    gauges: [f32; 3],
    flags: std::collections::HashMap<String, bool>,
    notify: Vec<NotifyApp>,
    consent: Vec<(Cap, Vec<CApp>)>,
    security: Security,
    /// Changes that need administrator rights are refused.
    deny_admin: bool,
    /// The WinRust accent (an index of `style::Accent::ALL`).
    accent: usize,
}

/// The demo source. `log` says what a test or a render asked it to change.
pub struct Demo {
    s: RefCell<State>,
    log: RefCell<Vec<String>>,
}

fn ipv4(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
    Ipv4Addr::new(a, b, c, d)
}

fn adapter(name: &str, desc: &str, wifi: bool, up: bool, speed: u64, ip: Option<Ipv4Addr>) -> net::Adapter {
    net::Adapter {
        name: name.into(),
        desc: desc.into(),
        wifi,
        up,
        speed,
        ipv4: ip.map(|i| vec![(i, 24)]).unwrap_or_default(),
        ipv6: Vec::new(),
        gateway: ip.map(|_| vec!["192.168.1.1".into()]).unwrap_or_default(),
        dns: if ip.is_some() { vec!["1.1.1.1".into(), "8.8.8.8".into()] } else { Vec::new() },
        mac: if wifi { "A4:C3:F0:5B:19:7E".into() } else { "00:E0:4C:68:02:11".into() },
        dhcp: true,
        internet: ip.map(|_| true),
    }
}

fn net(ssid: &str, quality: u32, sec: Option<wifi::Sec>, eap: Option<&'static str>, saved: bool, connected: bool) -> wifi::Net {
    wifi::Net { ssid: ssid.into(), raw: ssid.as_bytes().to_vec(), quality, connected, profile: saved.then(|| ssid.to_string()), sec, eap, iface: guid() }
}

/// A moment of the demo's day: Friday 27 November, `ago` days back, at `h:m`.
fn when(ago: u32, h: u8, m: u8) -> When {
    let day = 27 - ago.min(26) as u8;
    When { days_ago: ago, hour: h, min: m, day, month: 11 }
}

/// What the design's Privacy page lists: Store apps and desktop apps for each of camera, microphone and location.
/// The desktop apps of the microphone as the registry has them: Discord and Edge WebView2 keep one key for every version they ever installed
/// (two of Discord's were left open by a crash); they must come out as one row each, and Discord is the one in use now.
fn demo_desktop_mic() -> Vec<CApp> {
    let key = |k: &str, start: u64, stop: u64, ws: When, we: Option<When>| RawDesk { key: k.into(), start, stop, start_when: Some(ws), stop_when: we };
    let raw = vec![
        key("C:#Users#me#AppData#Local#Discord#app-1.0.9248#Discord.exe", 100, 0, when(130, 10, 0), None),
        key("C:#Users#me#AppData#Local#Discord#app-1.0.9251#Discord.exe", 200, 0, when(108, 21, 30), None),
        key("C:#Users#me#AppData#Local#Discord#app-1.0.9260#Discord.exe", 300, 0, when(0, 9, 14), None),
        key("C:#Program Files (x86)#Microsoft#EdgeWebView#Application#131.0.2903#msedgewebview2.exe", 50, 60, when(9, 18, 0), Some(when(9, 18, 2))),
        key("C:#Program Files (x86)#Microsoft#EdgeWebView#Application#132.0.1#MSEDGEWEBVIEW2.EXE", 70, 80, when(7, 18, 0), Some(when(7, 18, 2))),
    ];
    group_desktop(raw, &|exe| exe == "discord.exe", &|path| {
        let exe = path.rsplit('\\').next().unwrap_or("").to_lowercase();
        match exe.as_str() {
            "discord.exe" => Some("Discord".into()),
            "msedgewebview2.exe" => Some("Microsoft Edge WebView2".into()),
            _ => None,
        }
    })
}

fn demo_consent() -> Vec<(Cap, Vec<CApp>)> {
    let app = |id: &str, name: &str, desktop: bool, last: Option<When>, live: bool| CApp { id: id.into(), name: name.into(), desktop, last, live };
    vec![
        (Cap::Cam, vec![app("wcam", "Camera", false, Some(when(0, 8, 31)), false), app("plink", "Phone Link", false, None, false), app("telegram", "Telegram", true, Some(when(1, 21, 40)), false), app("discord", "Discord", true, Some(when(3, 12, 5)), false), app("edge", "Microsoft Edge", true, None, false)]),
        (
            Cap::Mic,
            vec![
                app("vrec", "Voice Recorder", false, Some(when(15, 9, 0)), false),
                app("plink", "Phone Link", false, None, false),
                app("telegram", "Telegram", true, Some(when(1, 21, 40)), false),
                                app("winrust", "WinRust screen recorder", true, Some(when(0, 12, 40)), false),
            ]
            .into_iter()
            .chain(demo_desktop_mic())
            .collect(),
        ),
        (Cap::Loc, vec![app("weather", "Weather", false, Some(when(0, 9, 11)), false), app("maps", "Maps", false, Some(when(9, 14, 20)), false), app("edge", "Microsoft Edge", true, None, false), app("telegram", "Telegram", true, None, false)]),
    ]
}

fn app_entry(name: &str, version: &str, publisher: &str) -> ext::App {
    ext::App { name: name.into(), version: version.into(), publisher: publisher.into(), uninstall: format!("demo-uninstall {name}") }
}

impl Demo {
    /// The mockup's first state: two displays, Wi-Fi connected to Home-5G, a charging laptop, an up-to-date Windows.
    #[must_use]
    pub fn new() -> Self {
        let light = winrust_common::style::light();
        let mut reg = std::collections::HashMap::new();
        for (k, n, v) in [(crate::PERSONALIZE, "AppsUseLightTheme", u32::from(light)), (crate::PERSONALIZE, "SystemUsesLightTheme", 0), (crate::PERSONALIZE, "EnableTransparency", 1), (r"Software\Microsoft\Windows\DWM", "ColorPrevalence", 0), (crate::STORAGE_SENSE, "01", 1), (ext::INET, "ProxyEnable", 0)] {
            reg.insert((k.to_string(), n.to_string()), v);
        }
        let dev = |id: &str, name: &str| (id.to_string(), name.to_string());
        let s = State {
            mons: vec![Mon { x: 0, y: 0, w: 2560, h: 1600, primary: true }, Mon { x: 2560, y: 0, w: 3840, h: 2160, primary: false }],
            sel: 0,
            names: vec!["Built-in display".into(), "DELL U2723QE".into()],
            modes: vec![
                vec![mode(2560, 1600, 60), mode(2560, 1600, 120), mode(2560, 1600, 165), mode(1920, 1200, 60), mode(1920, 1200, 120), mode(1680, 1050, 60), mode(1280, 800, 60)],
                vec![mode(3840, 2160, 30), mode(3840, 2160, 60), mode(2560, 1440, 60), mode(1920, 1080, 60)],
            ],
            cur: vec![mode(2560, 1600, 165), mode(3840, 2160, 60)],
            scales: vec![2, 2],
            orient: vec![0, 0],
            prev: None,
            topo: Topo::Extend,
            hdr: vec![None, Some(true)],
            night: 0,
            out: vec![dev("spk", "Speakers \u{b7} Realtek Audio"), dev("bt", "WH-1000XM5 \u{b7} Bluetooth"), dev("hdmi", "DELL U2723QE \u{b7} HDMI")],
            inp: vec![dev("mic", "Microphone Array \u{b7} Realtek"), dev("btmic", "WH-1000XM5 \u{b7} Bluetooth")],
            out_def: 0,
            in_def: 0,
            vol: [0.62, 0.70],
            mute: [false, false],
            apps: vec![(101, "Spotify".into(), 0.70), (102, "Discord".into(), 0.55), (103, "Microsoft Edge".into(), 0.40), (104, "System sounds".into(), 0.80)],
            bt: bt::State {
                bt: Some(true),
                air: Some(false),
                paired: vec![bt::Dev { id: "xm5".into(), name: "WH-1000XM5".into(), connected: true }, bt::Dev { id: "mx".into(), name: "MX Master 3S".into(), connected: true }, bt::Dev { id: "k3".into(), name: "Keychron K3".into(), connected: false }],
                nearby: Vec::new(),
                scanning: false,
                note: String::new(),
            },
            battery: Some(Battery { pct: 78, ac: true, charging: true, left: None }),
            plans: vec!["Battery saver".into(), "Balanced".into(), "Best performance".into()],
            plan: 1,
            screen: 2,
            sleep: 2,
            temp_ready: true,
            bin: 860 << 20,
            adapters: vec![adapter("Wi-Fi", "Intel Wi-Fi 6E AX211", true, true, 866_000_000, Some(ipv4(192, 168, 1, 24))), adapter("Ethernet", "Realtek USB GbE Family Controller", false, false, 0, None)],
            radio: Some((true, true)),
            nets: vec![
                net("Home-5G", 90, Some(wifi::Sec::Wpa3), None, true, true),
                net("Home-2.4G", 60, Some(wifi::Sec::Wpa2 { tkip: false }), None, true, false),
                net("TurkTelekom_ZT4F2", 35, Some(wifi::Sec::Wpa2 { tkip: false }), None, false, false),
                net("Office-Corp", 35, None, Some("WPA2"), false, false),
                net("Kepez_Free", 35, Some(wifi::Sec::Open), None, false, false),
                net("DIRECT-HP-LaserJet", 10, Some(wifi::Sec::Wpa2 { tkip: false }), None, false, false),
            ],
            vpns: vec![ext::Vpn { name: "Work VPN".into(), pbk: None, on: false }],
            reg,
            proxy: String::new(),
            pac: String::new(),
            programs: vec![
                app_entry("7-Zip", "24.08", "Igor Pavlov"),
                app_entry("Claude", "1.0.1890", "Anthropic"),
                app_entry("Discord", "1.0.9172", "Discord Inc."),
                app_entry("Figma", "124.5.4", "Figma, Inc."),
                app_entry("Git", "2.47.0", "The Git Development Community"),
                app_entry("Microsoft Edge", "131.0.2903", "Microsoft Corporation"),
                app_entry("Microsoft Outlook", "1.2024.1120", "Microsoft Corporation"),
                app_entry("NVIDIA Broadcast", "1.4.0", "NVIDIA Corporation"),
                app_entry("Spotify", "1.2.52", "Spotify AB"),
                app_entry("Steam", "2.10.91", "Valve Corporation"),
                app_entry("Telegram Desktop", "5.8.3", "Telegram FZ-LLC"),
                app_entry("Visual Studio Code", "1.95.3", "Microsoft Corporation"),
            ],
            wu: wu::State { busy: None, found: Some(Vec::new()), note: "Last checked today, 17:02".into() },
            wu_extra: WuExtra { last_check: Some(when(0, 17, 2)), last_install: Some(when(3, 8, 40)), ..WuExtra::default() },
            walls: vec![
                ("Obsidian (current)".into(), Fill::Gradient(0x1a2a46, 0x14203a, 0x0a0f1a, 0.6)),
                ("Dusk".into(), Fill::Gradient(0x1e2a4a, 0x4a2a5e, 0xc2546a, 0.6)),
                ("Sea".into(), Fill::Gradient(0x0b3d4f, 0x1f7a8c, 0x9fd3c7, 0.55)),
                ("Sand".into(), Fill::Gradient(0x3b2f2a, 0x8c6a4f, 0xe7c9a0, 0.55)),
                ("Graphite".into(), Fill::Solid(0x2a2c33)),
            ],
            wall: 0,
            lang: winrust_common::Lang::En,
            opts: [("Wheel", 1), ("Zone", 0)].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            gauges: [0.33, 0.70, 0.50],
            flags: [(Flag::MouseAccel, true), (Flag::AutoTime, true)].into_iter().chain([("vrec", Cap::Mic, true), ("plink", Cap::Mic, false), ("wcam", Cap::Cam, true), ("plink", Cap::Cam, false), ("weather", Cap::Loc, true), ("maps", Cap::Loc, true)].map(|(id, c, on)| (Flag::Access(c, Who::App(id.into())), on))).map(|(f, on)| (format!("{f:?}"), on)).collect(),
            notify: [("telegram", "Telegram", 0), ("discord", "Discord", 0), ("outlook", "Microsoft Outlook", 0), ("taskmgr", "Task Manager", 0), ("update", "Windows Update", 1), ("edge", "Microsoft Edge", 1), ("spotify", "Spotify", 2)].map(|(id, name, mode)| NotifyApp { id: id.into(), name: name.into(), mode }).to_vec(),
            consent: demo_consent(),
            security: Security { av: Some(true), fw: Some(true), sig: "1.421.316.0".into(), sig_when: Some(when(0, 7, 14)), smart: "Warn".into(), scanning: false, scanned: false },
            deny_admin: false,
            accent: 0,
        };
        Self { s: RefCell::new(s), log: RefCell::new(Vec::new()) }
    }

    /// What was asked of the "system" so far (only what would change something).
    #[cfg(test)]
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// From now on the changes that need administrator rights are refused.
    #[cfg(test)]
    pub fn deny_admin(&self) {
        self.with(|s| s.deny_admin = true);
    }

    #[cfg(test)]
    pub fn clear_log(&self) {
        self.log.borrow_mut().clear();
    }

    fn note(&self, s: impl Into<String>) {
        self.log.borrow_mut().push(s.into());
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.s.borrow_mut())
    }

    /// The demo in the state `name` of the design (the data; what the window itself shows is `app`).
    #[must_use]
    pub fn for_state(name: &str) -> Self {
        let d = Self::new();
        d.with(|s| match name {
            "battery-on-battery" => s.battery = Some(Battery { pct: 64, ac: false, charging: false, left: Some(4 * 3600 + 20 * 60) }),
            "battery-low" => s.battery = Some(Battery { pct: 14, ac: false, charging: false, left: Some(38 * 60) }),
            "battery-none" => (s.battery, s.plans[0]) = (None, "Power saver".into()),
            "net-ethernet" => {
                s.adapters = vec![adapter("Ethernet", "Realtek USB GbE Family Controller", false, true, 1_000_000_000, Some(ipv4(192, 168, 1, 31))), adapter("Wi-Fi", "Intel Wi-Fi 6E AX211", true, true, 866_000_000, Some(ipv4(192, 168, 1, 24)))];
            }
            "net-no-wifi-adapter" => {
                s.adapters = vec![adapter("Ethernet", "Realtek USB GbE Family Controller", false, true, 1_000_000_000, Some(ipv4(192, 168, 1, 31)))];
                (s.radio, s.nets) = (None, Vec::new());
            }
            "net-offline" => {
                s.adapters = vec![adapter("Wi-Fi", "Intel Wi-Fi 6E AX211", true, false, 0, None), adapter("Ethernet", "Realtek USB GbE Family Controller", false, false, 0, None)];
                s.radio = Some((false, true));
                s.nets.iter_mut().for_each(|n| n.connected = false);
            }
            "net-airplane" => {
                s.adapters = vec![adapter("Wi-Fi", "Intel Wi-Fi 6E AX211", true, false, 0, None), adapter("Ethernet", "Realtek USB GbE Family Controller", false, false, 0, None)];
                s.radio = Some((false, true));
                s.bt.air = Some(true);
                s.nets.iter_mut().for_each(|n| n.connected = false);
            }
            "bt-searching" | "bt-pairing" => {
                s.bt.scanning = name == "bt-searching";
                s.bt.nearby = vec![bt::Dev { id: "px".into(), name: "Pixel 8 Pro".into(), connected: false }];
            }
            "bt-needs-pin" => s.bt.nearby = vec![bt::Dev { id: "gb".into(), name: "Galaxy Buds2".into(), connected: false }],
            "bt-off" => s.bt.bt = Some(false),
            "wu-available" => s.wu = wu::State { busy: None, found: Some(vec!["2026-11 Cumulative Update for Windows 11 Version 24H2 (KB5046617)".into(), "Security Intelligence Update for Microsoft Defender (KB2267602)".into()]), note: String::new() },
            "wu-installing" => {
                s.wu = wu::State { busy: Some("Downloading and installing updates\u{2026}".into()), found: Some(vec!["2026-11 Cumulative Update for Windows 11 Version 24H2 (KB5046617)".into(), "Security Intelligence Update for Microsoft Defender (KB2267602)".into()]), note: String::new() };
                s.wu_extra = WuExtra { progress: Some(46), restart: false, ..s.wu_extra };
            }
            "wu-checking" | "update-checking" => s.wu = wu::State { busy: Some("Checking for updates\u{2026}".into()), found: None, note: String::new() },
            "wu-restart" => {
                s.wu = wu::State { busy: None, found: Some(vec!["2026-11 Cumulative Update for Windows 11 Version 24H2 (KB5046617)".into(), "Security Intelligence Update for Microsoft Defender (KB2267602)".into()]), note: String::new() };
                s.wu_extra = WuExtra { progress: None, restart: true, ..s.wu_extra };
            }
            "appearance-night-light" => s.night = 4800,
            "display-single" => {
                s.mons.truncate(1);
                s.names.truncate(1);
            }
            "apps-many" => s.programs = (0..400).map(|i| app_entry(&format!("Program {i:03}"), &format!("{}.{}.{}", i % 9 + 1, i % 7, i % 13), "")).collect(),
            "sound-muted" => s.mute = [true, true],
            "power-plans-many" => s.plans = vec!["Power saver".into(), "Balanced".into(), "High performance".into(), "Ultimate Performance".into()],
            "notify-off" => {
                s.reg.insert((crate::more::TOAST.to_string(), "ToastEnabled".to_string()), 0);
            }
            "privacy-camera-off" => {
                s.flags.insert(format!("{:?}", Flag::Access(Cap::Cam, Who::Pc)), false);
            }
            "privacy-scanning" => s.security.scanning = true,
            "proxy-on" => {
                s.reg.insert((ext::INET.to_string(), "ProxyEnable".to_string()), 1);
                s.proxy = "proxy.example.com:8080".into();
            }
            _ => {}
        });
        d
    }
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

impl Os for Demo {
    fn display_current(&self) -> Option<Mode> {
        self.with(|s| s.cur.get(s.sel).copied())
    }
    fn display_modes(&self) -> Vec<Mode> {
        self.with(|s| s.modes.get(s.sel).cloned().unwrap_or_default())
    }
    fn display_orientation(&self) -> Option<u32> {
        self.with(|s| s.orient.get(s.sel).copied())
    }
    fn display_set_orientation(&self, o: u32, save: bool) -> bool {
        self.note(format!("display_set_orientation {o} save={save}"));
        self.with(|s| {
            s.prev.get_or_insert_with(|| (s.cur.clone(), s.orient.clone(), s.topo));
            let i = s.sel;
            s.orient[i] = o;
        });
        true
    }
    fn display_apply(&self, m: Mode, save: bool) -> bool {
        self.note(format!("display_apply {}x{}@{} save={save}", m.w, m.h, m.hz));
        self.with(|s| {
            if !save {
                s.prev.get_or_insert_with(|| (s.cur.clone(), s.orient.clone(), s.topo));
            } else {
                s.prev = None;
            }
            let i = s.sel;
            s.cur[i] = m;
        });
        true
    }
    fn display_revert(&self) {
        self.note("display_revert");
        self.with(|s| {
            if let Some((cur, orient, topo)) = s.prev.take() {
                (s.cur, s.orient, s.topo) = (cur, orient, topo);
            }
        });
    }
    fn display_make_primary(&self) -> bool {
        self.note("display_make_primary");
        self.with(|s| {
            let i = s.sel;
            s.mons.iter_mut().enumerate().for_each(|(j, m)| m.primary = j == i);
        });
        true
    }
    fn display_topology(&self) -> Option<Topo> {
        Some(self.with(|s| s.topo))
    }
    fn display_connected(&self) -> usize {
        self.with(|s| s.mons.len())
    }
    fn display_set_topology(&self, t: Topo) -> bool {
        self.note(format!("display_set_topology {t:?}"));
        self.with(|s| {
            s.prev.get_or_insert_with(|| (s.cur.clone(), s.orient.clone(), s.topo));
            s.topo = t;
        });
        true
    }
    fn display_scales(&self) -> Option<(Vec<u32>, usize)> {
        Some((vec![100, 125, 150, 175, 200], self.with(|s| s.scales[s.sel])))
    }
    fn display_scale(&self) -> Option<u32> {
        self.with(|s| Some([100, 125, 150, 175, 200][s.scales[s.sel]]))
    }
    fn display_set_scale_index(&self, i: usize) -> bool {
        self.note(format!("display_set_scale_index {i}"));
        self.with(|s| {
            let k = s.sel;
            s.scales[k] = i.min(4);
        });
        true
    }
    fn display_hdr(&self) -> Option<bool> {
        self.with(|s| s.hdr.get(s.sel).copied().flatten())
    }
    fn display_set_hdr(&self, on: bool) -> bool {
        self.note(format!("display_set_hdr {on}"));
        self.with(|s| {
            let k = s.sel;
            s.hdr[k] = Some(on);
        });
        true
    }
    fn display_layout(&self) -> Vec<Mon> {
        self.with(|s| s.mons.clone())
    }
    fn display_move_monitor(&self, i: usize, x: i32, y: i32) -> bool {
        self.note(format!("display_move_monitor {i} {x} {y}"));
        self.with(|s| {
            if let Some(m) = s.mons.get_mut(i) {
                (m.x, m.y) = (x, y);
            }
        });
        true
    }
    fn display_monitor_info(&self) -> Option<(usize, usize, String, bool)> {
        self.with(|s| Some((s.sel + 1, s.mons.len(), s.names.get(s.sel).cloned().unwrap_or_default(), s.mons.get(s.sel).is_some_and(|m| m.primary))))
    }
    fn display_monitor_names(&self) -> Vec<String> {
        self.with(|s| s.names.clone())
    }
    fn display_selected(&self) -> usize {
        self.with(|s| s.sel)
    }
    fn display_select(&self, i: usize) {
        self.note(format!("display_select {i}"));
        self.with(|s| s.sel = i.min(s.mons.len().saturating_sub(1)));
    }

    fn audio_devices(&self, mic: bool) -> Vec<audio::Dev> {
        self.with(|s| {
            let (list, def) = if mic { (&s.inp, s.in_def) } else { (&s.out, s.out_def) };
            list.iter().enumerate().map(|(i, (id, name))| audio::Dev { id: id.clone(), name: name.clone(), default: i == def }).collect()
        })
    }
    fn audio_default_name(&self, mic: bool) -> String {
        self.with(|s| if mic { s.inp[s.in_def].1.clone() } else { s.out[s.out_def].1.clone() })
    }
    fn audio_set_default(&self, id: &str) -> bool {
        self.note(format!("audio_set_default {id}"));
        self.with(|s| {
            if let Some(i) = s.out.iter().position(|d| d.0 == id) {
                s.out_def = i;
            } else if let Some(i) = s.inp.iter().position(|d| d.0 == id) {
                s.in_def = i;
            }
        });
        true
    }
    fn audio_apps(&self) -> Vec<(u32, String)> {
        self.with(|s| s.apps.iter().map(|a| (a.0, a.1.clone())).collect())
    }
    fn audio_volume(&self, src: audio::Src) -> Option<f32> {
        self.with(|s| match src {
            audio::Src::Out => Some(s.vol[0]),
            audio::Src::Mic => Some(s.vol[1]),
            audio::Src::App(pid) => s.apps.iter().find(|a| a.0 == pid).map(|a| a.2),
        })
    }
    fn audio_set_volume(&self, src: audio::Src, v: f32) {
        self.note(format!("audio_set_volume {src:?} {v:.2}"));
        self.with(|s| match src {
            audio::Src::Out => (s.vol[0], s.mute[0]) = (v, false),
            audio::Src::Mic => (s.vol[1], s.mute[1]) = (v, false),
            audio::Src::App(pid) => {
                if let Some(a) = s.apps.iter_mut().find(|a| a.0 == pid) {
                    a.2 = v;
                }
            }
        });
    }
    fn audio_muted(&self, mic: bool) -> Option<bool> {
        Some(self.with(|s| s.mute[usize::from(mic)]))
    }
    fn audio_set_mute(&self, mic: bool, m: bool) {
        self.note(format!("audio_set_mute mic={mic} {m}"));
        self.with(|s| s.mute[usize::from(mic)] = m);
    }

    fn bt_state(&self) -> bt::State {
        self.with(|s| s.bt.clone())
    }
    fn bt_refresh(&self, _done: Ping) {}
    fn bt_set_bluetooth(&self, on: bool, _done: Ping) {
        self.note(format!("bt_set_bluetooth {on}"));
        self.with(|s| {
            s.bt.bt = Some(on);
            if !on {
                (s.bt.nearby, s.bt.scanning) = (Vec::new(), false);
            }
        });
    }
    fn bt_set_airplane(&self, on: bool, _done: Ping) {
        self.note(format!("bt_set_airplane {on}"));
        self.with(|s| s.bt.air = Some(on));
    }
    fn bt_scan(&self, _done: Ping) {
        self.note("bt_scan");
        self.with(|s| {
            s.bt.scanning = true;
            s.bt.nearby = vec![bt::Dev { id: "px".into(), name: "Pixel 8 Pro".into(), connected: false }];
        });
    }
    fn bt_pair(&self, id: String, _done: Ping) {
        self.note(format!("bt_pair {id}"));
    }
    fn bt_unpair(&self, id: String, _done: Ping) {
        self.note(format!("bt_unpair {id}"));
        self.with(|s| s.bt.paired.retain(|d| d.id != id));
    }
    fn bt_take_note(&self) -> Option<String> {
        None
    }

    fn power_battery(&self) -> Option<Battery> {
        self.with(|s| s.battery)
    }
    fn power_drives(&self) -> Vec<(char, u64, u64)> {
        vec![('C', 349 * GB, 953 * GB), ('D', 1126 * GB, 1843 * GB)]
    }
    fn power_plan_text(&self) -> String {
        self.with(|s| s.plans[s.plan].clone())
    }
    fn power_plans(&self) -> (Vec<String>, Option<usize>) {
        self.with(|s| (s.plans.clone(), Some(s.plan)))
    }
    fn power_set_plan(&self, i: usize) {
        self.note(format!("power_set_plan {i}"));
        self.with(|s| s.plan = i);
    }
    fn power_timeouts(&self, t: power::Timeout) -> (Vec<String>, Option<usize>) {
        let screen = matches!(t, power::Timeout::Screen);
        let list: Vec<String> = if screen { ["1 minute", "3 minutes", "5 minutes", "10 minutes", "15 minutes", "30 minutes", "Never"].map(String::from).to_vec() } else { ["5 minutes", "10 minutes", "15 minutes", "30 minutes", "1 hour", "Never"].map(String::from).to_vec() };
        let cur = self.with(|s| if screen { s.screen } else { s.sleep });
        (list, Some(cur))
    }
    fn power_timeout_text(&self, t: power::Timeout) -> String {
        let (l, c) = self.power_timeouts(t);
        c.and_then(|c| l.get(c).cloned()).unwrap_or_default()
    }
    fn power_set_timeout(&self, t: power::Timeout, i: usize) {
        let screen = matches!(t, power::Timeout::Screen);
        self.note(format!("power_set_timeout screen={screen} {i}"));
        self.with(|s| if screen { s.screen = i } else { s.sleep = i });
    }
    fn temp_text(&self) -> String {
        "2.4 GB".into()
    }
    fn temp_measured(&self) -> bool {
        self.with(|s| s.temp_ready)
    }
    fn temp_measure(&self, _done: Ping) {}
    fn temp_clean(&self, _done: Note) {
        self.note("temp_clean");
    }
    fn bin_bytes(&self) -> u64 {
        self.with(|s| s.bin)
    }
    fn bin_empty(&self, _owner: isize) -> bool {
        self.note("bin_empty");
        self.with(|s| s.bin = 0);
        true
    }

    fn net_adapters(&self) -> Vec<net::Adapter> {
        self.with(|s| {
            s.adapters
                .iter()
                .map(|a| net::Adapter { name: a.name.clone(), desc: a.desc.clone(), wifi: a.wifi, up: a.up, speed: a.speed, ipv4: a.ipv4.clone(), ipv6: a.ipv6.clone(), gateway: a.gateway.clone(), dns: a.dns.clone(), mac: a.mac.clone(), dhcp: a.dhcp, internet: a.internet })
                .collect()
        })
    }
    fn wifi_radio(&self) -> Option<(bool, bool)> {
        self.with(|s| s.radio)
    }
    fn wifi_list(&self) -> Vec<wifi::Net> {
        self.with(|s| s.nets.clone())
    }
    fn wifi_scan(&self) {
        self.note("wifi_scan");
    }
    fn wifi_set_radio(&self, on: bool) {
        self.note(format!("wifi_set_radio {on}"));
        self.with(|s| s.radio = s.radio.map(|r| (on, r.1)));
    }
    fn wifi_connect(&self, profile: &str, _iface: &GUID) -> bool {
        self.note(format!("wifi_connect {profile}"));
        true
    }
    fn wifi_disconnect(&self) -> bool {
        self.note("wifi_disconnect");
        true
    }
    fn wifi_forget(&self, profile: &str) -> bool {
        self.note(format!("wifi_forget {profile}"));
        true
    }
    fn wifi_add_and_connect(&self, ssid: &str, _raw: &[u8], sec: wifi::Sec, key: &str, hidden: bool, _iface: Option<&GUID>) -> Result<String, String> {
        // the key is not written to the log: it only says how long it was
        self.note(format!("wifi_add_and_connect {ssid} {sec:?} key_len={} hidden={hidden}", key.len()));
        if sec.needs_key() && !(8..=63).contains(&key.len()) {
            return Err("Password: 8 to 63 characters".into());
        }
        Ok(ssid.to_string())
    }
    fn wifi_add_eap_and_connect(&self, ssid: &str, _raw: &[u8], auth: &str, user: &str, pass: &str, hidden: bool, _iface: Option<&GUID>) -> Result<String, String> {
        self.note(format!("wifi_add_eap_and_connect {ssid} {auth} user={user} pass_len={} hidden={hidden}", pass.len()));
        Ok(ssid.to_string())
    }
    fn vpns(&self) -> Vec<ext::Vpn> {
        self.with(|s| s.vpns.clone())
    }
    fn vpn_dial(&self, name: String, _pbk: Option<String>, _done: Note) {
        self.note(format!("vpn_dial {name}"));
        self.with(|s| s.vpns.iter_mut().filter(|v| v.name == name).for_each(|v| v.on = true));
    }
    fn vpn_hang_up(&self, name: String, _done: Note) {
        self.note(format!("vpn_hang_up {name}"));
        self.with(|s| s.vpns.iter_mut().filter(|v| v.name == name).for_each(|v| v.on = false));
    }
    fn proxy_text(&self) -> String {
        self.with(|s| if s.proxy.is_empty() { "not set".into() } else { s.proxy.clone() })
    }
    fn pac_text(&self) -> String {
        self.with(|s| if s.pac.is_empty() { "not set".into() } else { s.pac.clone() })
    }
    fn proxy_set(&self, pac: bool, v: &str) -> bool {
        self.note(format!("proxy_set pac={pac} {v}"));
        self.with(|s| {
            if pac {
                s.pac = v.to_string();
            } else {
                s.proxy = v.to_string();
                if !v.is_empty() {
                    s.reg.insert((ext::INET.to_string(), "ProxyEnable".to_string()), 1);
                }
            }
        });
        true
    }
    fn proxy_changed(&self) {
        self.note("proxy_changed");
    }

    fn programs(&self) -> Vec<ext::App> {
        self.with(|s| s.programs.clone())
    }
    fn uninstall(&self, cmd: &str) -> bool {
        self.note(format!("uninstall {cmd}"));
        true
    }
    fn wu_state(&self) -> wu::State {
        self.with(|s| s.wu.clone())
    }
    fn wu_extra(&self) -> WuExtra {
        self.with(|s| s.wu_extra)
    }
    fn restart_now(&self) -> bool {
        self.note("restart_now");
        true
    }
    fn wu_check(&self, _done: Ping) {
        self.note("wu_check");
    }
    fn wu_install(&self, _done: Ping) {
        self.note("wu_install");
    }

    fn night_kelvin(&self) -> u32 {
        self.with(|s| s.night)
    }
    fn night_text(&self) -> String {
        match self.night_kelvin() {
            0 => "Off".into(),
            k => format!("{k} K"),
        }
    }
    fn night_set(&self, k: u32) {
        self.note(format!("night_set {k}"));
        self.with(|s| s.night = k);
    }
    fn reg_dword(&self, key: &str, name: &str) -> Option<u32> {
        self.with(|s| s.reg.get(&(key.to_string(), name.to_string())).copied())
    }
    fn reg_set_dword(&self, key: &str, name: &str, v: u32) -> bool {
        self.note(format!("reg_set_dword {name} {v}"));
        self.with(|s| s.reg.insert((key.to_string(), name.to_string()), v));
        true
    }
    fn announce_theme(&self) {
        self.note("announce_theme");
    }
    fn wallpaper_name(&self) -> String {
        self.with(|s| s.walls[s.wall].0.clone())
    }
    fn wallpaper_pick(&self, _owner: isize) -> Result<(), ()> {
        self.note("wallpaper_pick");
        Ok(())
    }
    fn wallpapers(&self) -> Vec<Wall> {
        self.with(|s| s.walls.iter().enumerate().map(|(i, (label, fill))| Wall { label: label.clone(), fill: *fill, on: i == s.wall, pic: None }).collect())
    }
    fn windows_value(&self, name: &str) -> String {
        match name {
            "ProductName" => "Windows 10 Pro".into(),
            "DisplayVersion" => "24H2".into(),
            "CurrentBuild" => "26100".into(),
            _ => String::new(),
        }
    }
    fn ram_text(&self) -> String {
        "32 GB (31.3 GB usable)".into()
    }
    fn computer_name(&self) -> String {
        "DANIIAR-PC".into()
    }
    fn user_name(&self) -> String {
        "Daniiar".into()
    }
    fn set_lang(&self, l: winrust_common::Lang) -> bool {
        self.note(format!("set_lang {l:?}"));
        self.with(|s| s.lang = l);
        true
    }
    fn wallpaper_select(&self, k: usize, _done: Note) {
        self.note(format!("wallpaper_select {k}"));
        self.with(|s| s.wall = k.min(s.walls.len() - 1));
    }
    fn wall_prepare(&self, _done: Ping) {}
    fn accent(&self) -> usize {
        self.with(|s| s.accent)
    }
    fn accent_set(&self, i: usize) {
        self.note(format!("accent_set {i}"));
        self.with(|s| s.accent = i.min(winrust_common::style::Accent::ALL.len() - 1));
    }
    fn opt_list(&self, o: Opt) -> (Vec<String>, Option<usize>) {
        let sec = |n: &[u32]| -> Vec<String> { n.iter().map(|&v| if v >= 60 { format!("{} min", v / 60) } else { format!("{v} s") }).collect() };
        let list: Vec<String> = match o {
            Opt::Toast => sec(&[5, 7, 15, 30, 60, 300]),
            Opt::LangKey => ["Alt + Shift", "Ctrl + Shift", "Win + Space only"].map(String::from).to_vec(),
            Opt::Hand => ["Left", "Right"].map(String::from).to_vec(),
            Opt::Wheel => ["1 line", "3 lines", "5 lines", "A whole screen"].map(String::from).to_vec(),
            Opt::Clock => ["24-hour \u{b7} 21:30", "12-hour \u{b7} 9:30 PM"].map(String::from).to_vec(),
            Opt::Date => DATE_PATS.iter().map(|p| fmt_date(p, 2026, 11, 27)).collect(),
            Opt::Week => ["Monday", "Sunday", "Saturday"].map(String::from).to_vec(),
            Opt::Zone => ["(UTC+03:00) Istanbul", "(UTC+03:00) Moscow, St. Petersburg", "(UTC+05:00) Almaty", "(UTC+00:00) London", "(UTC\u{2212}05:00) New York"].map(String::from).to_vec(),
        };
        (list, Some(self.with(|s| s.opts.get(&format!("{o:?}")).copied().unwrap_or(0))))
    }
    fn opt_set(&self, o: Opt, i: usize) -> bool {
        self.note(format!("opt_set {o:?} {i}"));
        self.with(|s| {
            if s.deny_admin && matches!(o, Opt::Zone) {
                return false;
            }
            s.opts.insert(format!("{o:?}"), i);
            true
        })
    }
    fn gauge(&self, g: Gauge) -> Option<f32> {
        Some(self.with(|s| s.gauges[g as usize]))
    }
    fn gauge_set(&self, g: Gauge, v: f32) {
        self.note(format!("gauge_set {g:?} {v:.2}"));
        // the delay has four steps, as the real one
        let v = if g == Gauge::KeyDelay { (v * 3.0).round() / 3.0 } else { v };
        self.with(|s| s.gauges[g as usize] = v);
    }
    fn flag(&self, f: &Flag) -> Option<bool> {
        // Do not disturb starts off; everything else the demo does not know is allowed
        Some(self.with(|s| s.flags.get(&format!("{f:?}")).copied().unwrap_or(*f != Flag::Dnd)))
    }
    fn flag_set(&self, f: &Flag, on: bool) -> bool {
        self.note(format!("flag_set {f:?} {on}"));
        self.with(|s| {
            if s.deny_admin && matches!(f, Flag::AutoTime | Flag::Access(_, Who::Pc)) {
                return false;
            }
            s.flags.insert(format!("{f:?}"), on);
            true
        })
    }
    fn notify_apps(&self) -> Vec<NotifyApp> {
        self.with(|s| s.notify.clone())
    }
    fn notify_set(&self, id: &str, mode: usize) -> bool {
        self.note(format!("notify_set {id} {mode}"));
        self.with(|s| s.notify.iter_mut().filter(|a| a.id == id).for_each(|a| a.mode = mode));
        true
    }
    fn consent(&self, cap: Cap) -> Consent {
        Consent { apps: self.with(|s| s.consent.iter().find(|c| c.0 == cap).map(|c| c.1.clone()).unwrap_or_default()) }
    }
    fn security(&self) -> Security {
        self.with(|s| s.security.clone())
    }
    fn quick_scan(&self, _done: Ping) {
        self.note("quick_scan");
        // the demo's scan is over at once
        self.with(|s| (s.security.scanning, s.security.scanned) = (false, true));
    }
    fn open_security(&self) {
        self.note("open_security");
    }
    fn layouts(&self) -> Vec<Layout> {
        [("\u{420}\u{443}\u{441}\u{441}\u{43a}\u{438}\u{439}", "\u{419}\u{426}\u{423}\u{41a}\u{415}\u{41d}", "RU"), ("English (US)", "QWERTY", "EN")].map(|(name, sub, tag)| Layout { name: name.into(), sub: sub.into(), tag: tag.into() }).to_vec()
    }
    fn clock_text(&self) -> String {
        let (h24, date) = self.with(|s| (s.opts.get("Clock").copied().unwrap_or(0) == 0, s.opts.get("Date").copied().unwrap_or(0)));
        format!("{}  \u{b7}  {}", if h24 { "09:14" } else { "9:14 AM" }, fmt_date(DATE_PATS[date.min(DATE_PATS.len() - 1)], 2026, 11, 27))
    }
    fn region_text(&self) -> String {
        "Russia".into()
    }
    fn numbers_text(&self) -> String {
        "1\u{a0}234,56 \u{b7} 1\u{a0}234,56 \u{20bd}".into()
    }
    fn time_server(&self) -> String {
        "Server: time.windows.com".into()
    }
    fn time_sync(&self, done: Note) {
        self.note("time_sync");
        done("Clock synced".into());
    }
    fn init(&self) {}
    fn start(&self, _hwnd: isize) {}
    fn identify(&self, mons: &[Mon]) {
        self.note(format!("identify {}", mons.len()));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the window in a state of the design

/// The window in the state `name` (`""`: the first state) on the demo source: the page it shows, what is open, what has been clicked.
#[must_use]
pub fn settings(name: &str) -> (crate::app::Settings, Rc<Demo>) {
    let d = Rc::new(Demo::for_state(name));
    set_os(d.clone());
    crate::set_app_query(""); // the Apps page's own search box starts empty in every state
    crate::more::set_pv_tab(Cap::Mic); // and the Privacy page's tab is the design's first one
    let page = match name {
        "sound" | "sound-muted" => 1,
        "power" | "battery-charging" | "battery-on-battery" | "battery-low" | "battery-none" | "power-plans-many" => 2,
        "network" | "net-wifi" | "net-ethernet" | "net-no-wifi-adapter" | "net-offline" | "net-airplane" | "wifi-password" | "wifi-connected-open" | "adapter-open" | "network-hidden" | "proxy-on" => 3,
        "bluetooth" | "bt-paired" | "bt-searching" | "bt-needs-pin" | "bt-off" | "bt-pairing" => 4,
        "apps" | "apps-many" | "apps-search" | "apps-none" => 5,
        "update" | "wu-up-to-date" | "wu-available" | "wu-installing" | "wu-restart" | "update-checking" => crate::WU_PAGE,
        "notify" | "notify-open" | "notify-off" => crate::NOTIFY_PAGE,
        "personal" | "appearance-dark" | "appearance-light" | "personal-light" => crate::PERSONAL_PAGE,
        "privacy" | "privacy-camera-off" | "privacy-scanning" | "privacy-location" => crate::PRIVACY_PAGE,
        "keyboard" => crate::KEYBOARD_PAGE,
        "time" => crate::TIME_PAGE,
        "about" => crate::ABOUT_PAGE,
        _ => 0,
    };
    let mut s = crate::app::Settings::new(page);
    match name {
        "dialog-keep-display" => s.demo_confirm(),
        "wifi-password" => s.demo_open("wifi:TurkTelekom_ZT4F2", Some("Password: 8 to 63 characters")),
        "wifi-connected-open" => s.demo_open("wifi:Home-5G", None),
        "adapter-open" => crate::toggle_adapter("Ethernet"),
        "network-hidden" => s.demo_open("wifi:other", None),
        "identify" => s.demo_identify = true,
        "bt-needs-pin" => s.pin_err = Some("gb".into()),
        "bt-pairing" => s.pairing = Some("px".into()),
        "dropdown" => s.demo_clicks.push(crate::app::ID_FIRST_SELECT), // the Resolution line
        "apps-search" => s.demo_app_search("micro"),
        "apps-none" => s.demo_app_search("photoshop"),
        "notify-open" => s.demo_open("na:telegram", None),
        "privacy-camera-off" => crate::more::set_pv_tab(Cap::Cam),
        "privacy-location" => crate::more::set_pv_tab(Cap::Loc),
        "search" => s.demo_search("net"),
        "search-page" => s.demo_search("blue"),
        "search-none" => s.demo_search("zzzq"),
        _ => {}
    }
    s.reread();
    (s, d)
}

/// The window of `state` as a boxed [`App`] (what `--render`, `--bench` and `--selftest` take).
pub fn app(state: &str) -> Box<dyn App> {
    Box::new(settings(state).0)
}

// ---------------------------------------------------------------------------------------------------------------------
// the command line

/// `--render`, `--render-all`, `--bench` and `--selftest`: on the demo source, with the language file of a folder of our own (so the
/// renders are in English, and nothing is written to the real `%ProgramData%\WinRust`). Some(exit code) when one of them was asked.
pub fn cli(args: &[String]) -> Option<i32> {
    let has = |f: &str| args.iter().any(|a| a == f);
    if !(has("--render") || has("--render-all") || has("--bench") || has("--selftest")) {
        return None;
    }
    let dir = std::env::temp_dir().join("winrust-settings-demo");
    let _ = std::fs::create_dir_all(dir.join("WinRust"));
    let lang = args.iter().find_map(|a| a.strip_prefix("lang=")).unwrap_or("en");
    let _ = std::fs::write(dir.join("WinRust").join("lang.txt"), lang);
    // SAFETY: before any other thread exists and before the language is first read.
    unsafe { std::env::set_var("ProgramData", &dir) };
    set_os(Rc::new(Demo::new()));
    if let Some(i) = args.iter().position(|a| a == "--bench")
        && args.get(i + 1).map(String::as_str) == Some("pages")
    {
        return Some(bench_pages(has("light")));
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest")
        && args.get(i + 1).map(String::as_str) == Some("identify")
    {
        return Some(crate::identify::selftest());
    }
    winrust_ui::render::cli(args, app, &STATES)
}

// ---------------------------------------------------------------------------------------------------------------------
// `--bench pages`

fn stats(v: &mut [f64]) -> (f64, f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    (v.iter().sum::<f64>() / v.len() as f64, v[(v.len() * 95).div_ceil(100) - 1], v[v.len() - 1])
}

/// What a page switch and a hover cost: the window drawn into a DIB, no window, the demo source. CPU only (the draw and the
/// copy out of the GPU); the live window's numbers are `--bench hover` and the others of the shared benchmark.
fn bench_pages(light: bool) -> i32 {
    use std::time::Instant;
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use winrust_ui::{
        core::Core,
        frame::Button,
        gfx::{Gfx, Surface},
    };
    // SAFETY: COM for WIC on this thread, once.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    winrust_common::style::set_light(light);
    let scale = 1.0;
    let (w, h) = (1000.0f32, 720.0f32);
    let Ok(mut g) = Gfx::new(scale) else { return 1 };
    let (app, _d) = settings("");
    let mut core = Core::new(Box::new(app), (w, h), scale, 0);
    let mut surf = Surface::new();
    let mut t = 1_000_000u64;
    // (ok, the draw alone, the draw and the copy out of the GPU)
    let mut frame = |core: &mut Core, g: &mut Gfx, t: u64| {
        let t0 = Instant::now();
        let mut draw = 0.0;
        let ok = g
            .paint(&mut surf, w as i32, h as i32, |c| {
                let t1 = Instant::now();
                core.draw(c, t);
                draw = t1.elapsed().as_secs_f64() * 1000.0;
            })
            .is_ok();
        (ok, draw, t0.elapsed().as_secs_f64() * 1000.0)
    };
    for _ in 0..3 {
        t += 400;
        frame(&mut core, &mut g, t);
    }
    let (mut pages, mut hovers, mut pages_all, mut hovers_all) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..12 {
        for p in (1..crate::PAGES).chain([0]) {
            t += 600;
            let Some((x, y)) = core.hits().get(crate::app::ID_NAV + p as winrust_ui::frame::Id).map(|h| (h.r.cx(), h.r.cy())) else { return 1 };
            core.pointer_move(x, y, t);
            core.down(Button::Left, x, y, t);
            core.up(Button::Left, x, y, t);
            core.take_host();
            let (ok, ms, all) = frame(&mut core, &mut g, t);
            if !ok {
                return 1;
            }
            pages.push(ms);
            pages_all.push(all);
            // the pointer walks down the rows: every step moves a hover
            for k in 0..20 {
                t += 8;
                core.pointer_move(500.0 + k as f32 * 5.0, 130.0 + k as f32 * 25.0, t);
                let (_, ms, all) = frame(&mut core, &mut g, t);
                hovers.push(ms);
                hovers_all.push(all);
            }
        }
    }
    // 400 programs on the Apps page, scrolled by the wheel: only the lines in view are drawn
    let (app, _d) = settings("apps-many");
    let mut core = Core::new(Box::new(app), (w, h), scale, 0);
    let (mut scrolls, mut scrolls_all) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        t += 400;
        frame(&mut core, &mut g, t);
    }
    core.pointer_move(600.0, 400.0, t);
    for k in 0..300 {
        t += 8;
        core.wheel(0.0, if (k / 100) % 2 == 0 { -0.4 } else { 0.4 }, Default::default(), t);
        let (_, ms, all) = frame(&mut core, &mut g, t);
        scrolls.push(ms);
        scrolls_all.push(all);
    }
    let (a, b, c, d) = (stats(&mut pages), stats(&mut hovers), stats(&mut pages_all), stats(&mut hovers_all));
    let (e, f) = (stats(&mut scrolls), stats(&mut scrolls_all));
    println!("ms per frame at scale {scale}, {} theme, no window (the draw alone, then with the copy of the frame out of the GPU into a DIB)", if light { "light" } else { "dark" });
    println!("scenario            |  mean    p95    max");
    println!("page switch, draw   | {:6.3} {:6.3} {:6.3}   ({} frames)", a.0, a.1, a.2, pages.len());
    println!("page switch, + copy | {:6.3} {:6.3} {:6.3}", c.0, c.1, c.2);
    println!("hover, draw         | {:6.3} {:6.3} {:6.3}   ({} frames)", b.0, b.1, b.2, hovers.len());
    println!("hover, + copy       | {:6.3} {:6.3} {:6.3}", d.0, d.1, d.2);
    println!("scroll 400, draw    | {:6.3} {:6.3} {:6.3}   ({} frames)", e.0, e.1, e.2, scrolls.len());
    println!("scroll 400, + copy  | {:6.3} {:6.3} {:6.3}", f.0, f.1, f.2);
    0
}
