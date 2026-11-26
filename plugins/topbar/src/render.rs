//! `topbar --render <file.png> [classic|floating|dock] [bottom|left|right] [0|1|2] [hover=<element>] [press=<element>]
//! [open=gear|start|qs|clock|tray|lang] [scale=1.25] [op=85] [battery] [tray=2] [apps=8]`
//!
//! Draws one frame of the bar through the same renderer as the real one, with the mockup's fake buttons (8 apps, the focused
//! one lit), onto a plain backdrop, and writes the strip around it to a PNG. It never touches the system taskbar and creates
//! no window, so it is safe to run next to the user's desktop. `<element>` is a name (gear, search, taskview, start, chevron,
//! lang, wifi, volume, battery, restart, power, clock) or the number of an app button (1..).
use super::*;
use ui::bar::Net;
use ui::quick_popup as qp;
use winrust_common::style::{Side, Style};

/// Hidden window and per-frame timings for the same surfaces used by the live UI.
struct Bench {
    hwnd: HWND,
    draw: Vec<f64>,
    end: Vec<f64>,
    present: Vec<f64>,
}

impl Bench {
    unsafe fn new() -> Self {
        let hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW, wide("STATIC").as_ptr(), null(), WS_POPUP,
            -5000, -5000, 1, 1, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        assert!(!hwnd.is_null(), "benchmark window creation failed: {}", GetLastError());
        Self { hwnd, draw: Vec::new(), end: Vec::new(), present: Vec::new() }
    }

    fn record(&mut self, elapsed: std::time::Duration, end: std::time::Duration, present: std::time::Duration) {
        self.draw.push(elapsed.saturating_sub(end).as_secs_f64() * 1000.0);
        self.end.push(end.as_secs_f64() * 1000.0);
        self.present.push(present.as_secs_f64() * 1000.0);
    }

    fn show(&self, surface: &ui::gfx::Surface, alpha: u8) {
        assert!(surface.present(whwnd(self.hwnd), -5000, -5000, alpha), "UpdateLayeredWindow failed in benchmark");
    }

    fn report(&mut self, name: &str) {
        fn stats(v: &mut [f64]) -> (f64, f64, f64) {
            v.sort_by(f64::total_cmp);
            (v.iter().sum::<f64>() / v.len() as f64, v[(v.len() * 95).div_ceil(100) - 1], v[v.len() - 1])
        }
        let (a, b, c) = (stats(&mut self.draw), stats(&mut self.end), stats(&mut self.present));
        println!("{name:10} | {:7.3} {:7.3} {:7.3} | {:7.3} {:7.3} {:7.3} | {:7.3} {:7.3} {:7.3}", a.0, a.1, a.2, b.0, b.1, b.2, c.0, c.1, c.2);
    }
}

impl Drop for Bench {
    fn drop(&mut self) { unsafe { DestroyWindow(self.hwnd); } }
}

pub(crate) unsafe fn bench() {
    // %TEMP%, not the working directory: %ProgramData% is redirected below and drivers write there too (NVIDIA's umdlogs)
    let scratch = std::env::temp_dir().join(format!("topbar-bench-{}", std::process::id()));
    std::fs::create_dir(&scratch).expect("create benchmark scratch directory");
    // The benchmark is a separate process; all settings writes stay in its private scratch directory.
    // SAFETY: single-threaded here, before any other thread exists that could read the environment.
    unsafe { std::env::set_var("ProgramData", &scratch) };
    i18n::style::set_anim(i18n::style::Anim::Full);
    println!("surface    | draw mean     p95     max | EndDraw mean    p95     max | present mean    p95     max (ms)");
    for name in ["bar", "settings", "start", "quick", "search", "taskview", "power-restart"] {
        render_panel(&["unused.png".into(), name.into(), "width=2560".into(), "height=1440".into(), "bench".into()]);
    }
    std::fs::remove_dir_all(scratch).expect("remove benchmark scratch directory");
}

/// What the mockup's bar holds: name, colour of its indicators, windows, an exe whose icon stands in for the brand's.
const APPS: [(&str, u32, usize, &str); 8] = [
    ("Claude", 0xe58a66, 1, r"C:\Windows\System32\mspaint.exe"),
    ("File manager", 0xf6c453, 2, r"C:\Windows\explorer.exe"),
    ("Microsoft Edge", 0x3cc4f2, 3, r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
    ("Spotify", 0x1ed760, 1, r"C:\Windows\System32\notepad.exe"),
    ("Task Manager", 0x4fd38a, 1, r"C:\Windows\System32\Taskmgr.exe"),
    ("Outlook", 0x3cc4f2, 0, r"C:\Windows\System32\calc.exe"),
    ("Discord", 0x8291ff, 1, r"C:\Windows\System32\cmd.exe"),
    ("Telegram", 0x3fb4ea, 1, r"C:\Windows\System32\control.exe"),
];

fn key_of(name: &str) -> i32 {
    match name {
        "gear" => GEAR,
        "search" => SEARCH,
        "taskview" => TASKVIEW,
        "start" => 0,
        "chevron" => TAPP,
        "lang" => LANG,
        "wifi" => WIFI_I,
        "volume" => VOL_I,
        "battery" => BAT_I,
        "restart" => POW_R,
        "power" => POW_S,
        "clock" => CLOCK,
        n => n.parse().unwrap_or(NONE),
    }
}

/// Alpha-blends a premultiplied BGRA layer onto an opaque canvas, `alpha` being the window's constant alpha.
fn blend(dst: &mut [u32], (dw, dh): (i32, i32), src: &[u32], sw: i32, (x, y): (i32, i32), alpha: u8) {
    for sy in 0..src.len() as i32 / sw.max(1) {
        let ty = y + sy;
        if ty < 0 || ty >= dh {
            continue;
        }
        for sx in 0..sw {
            let tx = x + sx;
            if tx < 0 || tx >= dw {
                continue;
            }
            let p = src[(sy * sw + sx) as usize];
            let k = u32::from(alpha);
            let ch = |s: u32| ((p >> s) & 255) * k / 255;
            let sa = ch(24);
            let d = &mut dst[(ty * dw + tx) as usize];
            let mix = |s: u32| (ch(s) + ((*d >> s) & 255) * (255 - sa) / 255).min(255);
            *d = 0xff00_0000 | mix(16) << 16 | mix(8) << 8 | mix(0);
        }
    }
}

pub(crate) unsafe fn render(args: &[String]) {
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let (mut style, mut side, mut size) = (Style::Classic, Side::Bottom, 1u8);
    let (mut hover, mut press, mut open, mut scale, mut op) = (NONE, NONE, "", 1.0f32, 0.85f32);
    let (mut battery, mut tray, mut n_apps) = (false, 0usize, 8usize);
    let (mut unread, mut dnd) = (0usize, false);
    let Some(file) = args.first() else { return };
    for a in &args[1..] {
        match a.split_once('=') {
            Some(("hover", v)) => hover = key_of(v),
            Some(("press", v)) => press = key_of(v),
            Some(("open", v)) => open = Box::leak(v.to_string().into_boxed_str()),
            Some(("scale", v)) => scale = v.parse().unwrap_or(1.0),
            Some(("op", v)) => op = v.parse::<f32>().unwrap_or(85.0) / 100.0,
            Some(("tray", v)) => tray = v.parse().unwrap_or(0),
            Some(("apps", v)) => n_apps = v.parse().unwrap_or(8),
            Some(("unread", v)) => unread = v.parse().unwrap_or(0),
            _ => match a.as_str() {
                "dnd" => dnd = true,
                "battery" => battery = true,
                "0" | "1" | "2" => size = a.parse().unwrap_or(1),
                "bottom" | "left" | "right" => side = Side::parse(a),
                s => style = Style::parse(s),
            },
        }
    }
    SCALE.set(scale);
    let Ok(mut bar) = barui::Bar::new(scale) else { return };
    let icon = |path: &str| {
        let ic = if std::path::Path::new(path).exists() { item_icon(path, bar_icon_px()) } else { null_mut() };
        wicon(ic)
    };
    let apps: Vec<barui::AppView> = (0..n_apps)
        .map(|i| {
            let (_, color, wins, exe) = APPS[i % APPS.len()];
            barui::AppView { icon: icon(exe), wins, active: i == 1, flash: false, color: ui::gfx::Rgba::hex(color) }
        })
        .collect();
    let tray_icons: Vec<_> = (0..tray).map(|_| icon(r"C:\Windows\System32\notepad.exe")).collect();
    let (sw, sh) = ((1440.0 * scale).round() as i32, (900.0 * scale).round() as i32);
    let view = barui::View {
        style,
        side,
        size,
        align_left: args.iter().any(|a| a == "alignl"),
        screen: (1440.0, 900.0),
        op,
        apps,
        tray: tray_icons,
        battery: battery.then_some((98, false)),
        vol: (72, false),
        net: Net::Wifi(85),
        lang: "RUS".into(),
        time: "03:57".into(),
        date: "11/26/2026".into(),
        unread,
        dnd,
    };
    let still = barui::Live { now: 0, hot: NONE, press: NONE, ..Default::default() };
    let shown = barui::Live {
        now: 100_000,
        hot: hover,
        press,
        gear: open == "gear",
        start: open == "start",
        search: open == "search",
        qs: open == "qs",
        cal: open == "clock",
        lang: open == "lang",
        tray: open == "tray",
        task: open == "taskview",
        jump: None,
        hidden: false,
    };
    bar.frame(&view, &still);
    bar.frame(&view, &shown); // the targets are set here...
    let p = bar.frame(&view, &barui::Live { now: 110_000, ..shown }); // ...and reached here

    // where everything is, in screen dip: the numbers to hold against the mockup's DOM
    let (wx, wy) = (p.win.0 as f32 / scale, p.win.1 as f32 / scale);
    let dump: String = bar
        .layout()
        .items
        .iter()
        .map(|i| format!("{:?} {} {:.1},{:.1},{:.1},{:.1}
", i.kind, i.key, wx + i.rect.x, wy + i.rect.y, i.rect.w, i.rect.h))
        .collect();
    let _ = std::fs::write(format!("{file}.txt"), dump);

    // the strip around the bar on a plain backdrop, the shadow window under the bar window like on screen
    let mut canvas: Vec<u32> = (0..sw * sh)
        .map(|i| {
            let t = (i / sw) as f32 / sh as f32;
            let c = |a: f32, b: f32| (a + (b - a) * t) as u32;
            0xff00_0000 | c(10.0, 24.0) << 16 | c(13.0, 32.0) << 8 | c(20.0, 52.0)
        })
        .collect();
    let (bar_surf, shadow_surf) = bar.surfaces();
    if let Some((x, y, w, _)) = p.shadow {
        blend(&mut canvas, (sw, sh), shadow_surf.pixels(), w, (x, y), p.shadow_alpha);
    }
    blend(&mut canvas, (sw, sh), bar_surf.pixels(), p.win.2, (p.win.0, p.win.1), p.alpha);
    let (x0, y0, x1, y1) = match (side, style) {
        (Side::Bottom, _) => (0, sh - (170.0 * scale) as i32, sw, sh),
        (Side::Left, _) => (0, 0, (200.0 * scale) as i32, sh),
        (Side::Right, _) => (sw - (200.0 * scale) as i32, 0, sw, sh),
    };
    let (w, h) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in y0..y1 {
        out.extend_from_slice(&canvas[(y * sw + x0) as usize..(y * sw + x1) as usize]);
    }
    if bar.gfx().save_png(file, w as u32, h as u32, &out).is_err() {
        let _ = std::fs::write(format!("{file}.err.txt"), "could not write the png");
    }
}

// ---------------------------------------------------------------------------------------------------------------------

thread_local! {
    static DEMO: std::cell::RefCell<Vec<ui::tray_popup::Cell>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn demo_cells() -> Vec<ui::tray_popup::Cell> {
    DEMO.with_borrow(|d| d.iter().map(|c| ui::tray_popup::Cell { icon: c.icon, tip: c.tip.clone(), token: c.token }).collect())
}

/// What the quick settings panel shows when it is drawn into a PNG: the mockup's state, not the machine's.
struct Demo {
    on: Vec<usize>,
    vol: Option<(i32, bool)>,
    bright: Option<i32>,
    battery: Option<(i32, bool)>,
    media: bool,
    wifi_on: bool,
}

impl Demo {
    fn from(args: &[String]) -> Self {
        let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
        let num = |k: &str| get(k).and_then(|v| v.parse::<i32>().ok());
        Self {
            on: get("on").map_or(vec![1, 7], |v| v.split(',').filter_map(|n| n.parse().ok()).collect()),
            vol: Some((num("vol").unwrap_or(72), args.iter().any(|a| a == "muted"))),
            bright: num("bright"),
            battery: if get("batt") == Some("none") { None } else { Some((num("batt").unwrap_or(98), args.iter().any(|a| a == "charging"))) },
            media: args.iter().any(|a| a == "media"),
            wifi_on: !args.iter().any(|a| a == "wifioff"),
        }
    }
}

impl qp::Host for Demo {
    fn tick(&mut self, _wifi_page: bool) {}

    fn state(&mut self) -> qp::State {
        let status = ["Not connected", "Off", "Off", "Off", "Off", "Win+Ctrl+L", "Off", "Off", "Win+P"];
        let on_text = ["Home network", "On", "On", "On", "On", "Win+Ctrl+L", "On", "On", "Win+P"];
        let tiles = (0..9).map(|i| qp::Tile { on: self.on.contains(&i), status: (if self.on.contains(&i) { on_text[i] } else { status[i] }).into() }).collect();
        qp::State {
            tiles,
            vol: self.vol,
            bright: self.bright,
            media: self.media.then(|| qp::Media { title: "Midnight City".into(), artist: "M83".into(), playing: true }),
            battery: self.battery,
            audio: std::rc::Rc::new(demo_audio()),
        }
    }

    fn tile(&mut self, _i: usize, _cx: &mut ui::panel::Cx) {}
    fn volume(&mut self, _percent: i32) {}
    fn mute(&mut self) {}
    fn audio_set(&mut self, _input: bool, _id: &str) {}
    fn brightness(&mut self, _percent: i32, _done: bool) {}
    fn media(&mut self, _cmd: u8) {}
    fn settings(&mut self, _uri: &'static str, _cx: &mut ui::panel::Cx) {}
    fn wifi_open(&mut self) {}

    fn wifi(&mut self) -> qp::Wifi {
        let net = |ssid: &str, quality, secure, saved, connected| qp::Network { ssid: ssid.into(), quality, secure, saved, connected };
        let nets = if self.wifi_on {
            vec![net("Home network", 85, true, true, true), net("Office", 70, true, true, false), net("Cafe Guest", 62, false, false, false), net("Neighbour 5G", 45, true, false, false), net("DIRECT-roku-1F", 30, true, false, false)]
        } else {
            Vec::new()
        };
        qp::Wifi {
            on: self.wifi_on,
            link: self.wifi_on.then(|| qp::Link { phy: "Wi\u{2011}Fi 6".into(), down: 866, up: 866, auth: "WPA2\u{2011}Personal".into() }),
            status: (!self.wifi_on).then(|| ("Turned off".to_string(), qp::Tone::Dim)),
            nets,
            joining: None,
            offline: false,
        }
    }

    fn wifi_power(&mut self, _on: bool) {}
    fn wifi_pick(&mut self, _ssid: &str) -> qp::Join {
        qp::Join::Started
    }
    fn wifi_join(&mut self, _ssid: &str, _password: &str) {}
    fn wifi_leave(&mut self) {}
    fn wifi_forget(&mut self, _ssid: &str) {}
}

/// The devices of the mockup's fake machine.
fn demo_audio() -> qp::Audio {
    use qp::Kind::{Headphones, Microphone, Monitor, Speakers};
    let d = |name: &str, sub: &str, kind, current| qp::Device { id: name.into(), name: name.into(), sub: sub.into(), kind, current };
    qp::Audio {
        outputs: vec![d("Speakers", "Realtek Audio \u{b7} built-in", Speakers, true), d("WH-1000XM5", "Bluetooth \u{b7} battery 80%", Headphones, false), d("DELL U2723QE", "Monitor speakers \u{b7} HDMI", Monitor, false)],
        inputs: vec![d("Microphone Array", "Realtek Audio \u{b7} built-in", Microphone, true), d("WH-1000XM5", "Bluetooth headset", Headphones, false)],
    }
}

thread_local! {
    static DEMO_DATE: std::cell::Cell<(i32, i32, i32)> = const { std::cell::Cell::new((2026, 11, 26)) };
}

fn demo_now() -> ui::cal_popup::Now {
    ui::cal_popup::Now { date: DEMO_DATE.get(), time: "03:57".into(), tick: NOTIFY_BASE }
}

/// `y-m-d` or `y-m` as numbers.
fn parse_date(v: &str) -> Option<(i32, i32, i32)> {
    let mut p = v.split('-').map(|n| n.parse::<i32>().ok());
    Some((p.next()??, p.next()??, p.next().flatten().unwrap_or(1)))
}

/// `topbar --render-panel <file.png> tray|lang|tip|quick|wifi|calendar|start|search|settings[:<page>]|toast|jumplist|preview|taskview|
/// switcher|menu|power-restart|power-shutdown|power-busy [classic|floating|dock] [bottom|left|right] [scale=1.25] [t=<ms>] [hover=<n>] [n=<icons>] [real] [at=<n>]
/// [q=<text>] [page=<n>] [pins=<n>] [wins=<n>] [recent] [kind=ctx|clock|winx] [desks=<n>]`
///
/// `quick` shows the list of sound devices after a click on the Sound card's button: `click=23 after=800`. `switcher` takes
/// `permon` (the list is one monitor's), `menu kind=winx` shows the sign-out submenu with `hover=23`.
///
/// `quick` and `wifi` also take the state of the mockup's fake machine: `on=<tiles, 0..8>` `vol=<n>` `muted` `bright=<n>`
/// `batt=<n|none>` `charging` `media` `wifioff`, and for `wifi` `ask=<ssid>` (the password prompt) or `menu=<ssid>` (the context
/// menu); `calendar` takes `today=<y-m-d>`, `view=<y-m>` and `sel=<y-m-d>`.
///
/// `settings` takes a page (main, colors, templates, lang, bar, reset, adv, upd, rollback) and, besides the above, `lang=<en|ru|ja|pt|zh>`,
/// `open=<colour row>`, `upd=busy|done`, `tpl=<n>` sample templates, `tiled`, `del`, `size=<0|1|2>`, `op=<percent>`, `hide`,
/// `alignl`, `accent=<blue|teal|lilac|amber|rose>`, `dir=fwd|bwd` (the page has just arrived: `t` is the time since); `toast` takes `text=<words>`. Both run on a scratch `%ProgramData%`: the
/// settings and the language are theirs, the machine's own are never read or written.
///
/// Draws a popup panel through the same frame as the live one, next to the bar, on a backdrop with something to blur, and
/// writes the area around it to a PNG. `t` is the time since the panel opened (default: at rest), `hover` the hovered item,
/// `n` the number of tray icons, `real` the machine's own keyboard layouts instead of the three of the mockup, and for `tip`
/// the element it hints at is `at` (an app button number, default 3). For `start` and `search`: `q` is what is typed, `page` the
/// page of pinned apps, `pins` how many of the machine's apps stand for the pinned ones (the machine's own pins are whatever they
/// are); the apps, the recent files and the account are the machine's. `jumplist` and `preview` belong to app button `at`
/// (`wins` windows, `recent` files in the jump list), `menu` is the bar's context menu (`kind=ctx`), the clock's or Win+X, and
/// `taskview` shows `wins` windows on `desks` desktops; the live thumbnails are faked here (there is no window to show).
pub(crate) unsafe fn render_panel(args: &[String]) {
    use ui::panel::{Anchor, Popup};
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let Some(file) = args.first() else { return };
    let what = args.get(1).map_or("tray", String::as_str).to_string();
    let benchmarking = args.iter().any(|a| a == "bench");
    let (mut style, mut side, mut scale, mut t, mut hover, mut n_icons, mut real, mut at) = (Style::Classic, Side::Bottom, 1.0f32, 5000u64, None, 6usize, false, 3i32);
    let (mut query, mut page, mut pins) = (String::new(), 0usize, 24usize);
    let (mut screen_w, mut screen_h) = (1440.0f32, 900.0f32);
    let (mut t_given, mut lang_arg, mut open_row, mut upd, mut tpl, mut text) = (false, "en", None, "", 0usize, "Alt+Tab lists windows from this monitor only");
    let (mut tiled, mut del, mut hide, mut alignl, mut bar_size, mut opacity, mut accent) = (false, false, false, false, 1u8, 85u8, "blue");
    let mut dir_arg = "";
    let (mut click, mut after, mut busy, mut per_monitor) = (None::<i32>, 0u64, false, false);
    // the notification centre (`calendar`) and the pop-ups (`popups`): what is in the store
    let (mut notes_arg, mut expand, mut reply_text, mut pops_arg) = ("demo", "", None::<&str>, "msg,shot,cal");
    let (mut dnd_flag, mut undo_flag) = (false, false);
    let (mut n_wins, mut recent, mut kind, mut n_desks) = (3usize, false, "ctx", 2usize);
    for a in args.iter().skip(2) {
        match a.split_once('=') {
            Some(("width", v)) => screen_w = v.parse().unwrap_or(1440.0),
            Some(("height", v)) => screen_h = v.parse().unwrap_or(900.0),
            Some(("q", v)) => query = v.to_string(),
            Some(("page", v)) => page = v.parse().unwrap_or(0),
            Some(("pins", v)) => pins = v.parse().unwrap_or(24),
            Some(("scale", v)) => scale = v.parse().unwrap_or(1.0),
            Some(("t", v)) => {
                t = v.parse().unwrap_or(5000);
                t_given = true;
            }
            Some(("lang", v)) => lang_arg = Box::leak(v.to_string().into_boxed_str()),
            Some(("open", v)) => open_row = v.parse().ok(),
            Some(("upd", v)) => upd = Box::leak(v.to_string().into_boxed_str()),
            Some(("tpl", v)) => tpl = v.parse().unwrap_or(0),
            Some(("text", v)) => text = Box::leak(v.to_string().into_boxed_str()),
            Some(("size", v)) => bar_size = v.parse().unwrap_or(1),
            Some(("op", v)) => opacity = v.parse().unwrap_or(85),
            Some(("accent", v)) => accent = Box::leak(v.to_string().into_boxed_str()),
            Some(("dir", v)) => dir_arg = Box::leak(v.to_string().into_boxed_str()),
            Some(("hover", v)) => hover = v.parse().ok(),
            Some(("click", v)) => click = v.parse().ok(),
            Some(("after", v)) => after = v.parse().unwrap_or(0),
            Some(("n", v)) => n_icons = v.parse().unwrap_or(6),
            Some(("at", v)) => at = v.parse().unwrap_or(3),
            Some(("wins", v)) => n_wins = v.parse().unwrap_or(3),
            Some(("desks", v)) => n_desks = v.parse().unwrap_or(2),
            Some(("kind", v)) => kind = Box::leak(v.to_string().into_boxed_str()),
            Some(("notes", v)) => notes_arg = Box::leak(v.to_string().into_boxed_str()),
            Some(("expand", v)) => expand = Box::leak(v.to_string().into_boxed_str()),
            Some(("reply", v)) => reply_text = Some(Box::leak(v.to_string().into_boxed_str())),
            Some(("pops", v)) => pops_arg = Box::leak(v.to_string().into_boxed_str()),
            _ => match a.as_str() {
                "dnd" => dnd_flag = true,
                "undo" => undo_flag = true,
                "real" => real = true,
                "busy" => busy = true,
                "permon" => per_monitor = true,
                "tiled" => tiled = true,
                "del" => del = true,
                "hide" => hide = true,
                "alignl" => alignl = true,
                "recent" => recent = true,
                "bottom" | "left" | "right" => side = Side::parse(a),
                s => style = Style::parse(s),
            },
        }
    }
    let notifying = matches!(what.as_str(), "calendar" | "popups");
    let base = if notifying { NOTIFY_BASE } else { 0 };
    if notifying {
        // the machine's own notifications are never read: the store holds the mockup's
        ui::notice::set_icon_loader(|p| {
            // SAFETY: the shell makes the icon; the handle lives as long as the process.
            unsafe { item_icon(p, 32) as isize }
        });
        demo_notes(what == "popups", notes_arg, expand, reply_text, (dnd_flag, undo_flag), pops_arg, base);
    }
    let sandbox = what.starts_with("settings") || what == "toast" || notifying;
    if sandbox {
        // SAFETY: single-threaded at this point; the process only ever reads these settings from its own scratch folder.
        if !benchmarking { unsafe { std::env::set_var("ProgramData", std::env::temp_dir().join("topbar-render")) }; }
        i18n::set_lang(i18n::Lang::parse(lang_arg));
        i18n::style::set_style(style);
        i18n::style::set_side(side);
        i18n::style::set_bar_size(bar_size);
        i18n::style::set_bar_opacity(opacity);
        i18n::style::set_autohide(hide);
        i18n::style::set_align_left(alignl);
        i18n::style::set_accent(i18n::style::Accent::parse(accent));
        if !t_given && what == "toast" {
            t = 800;
        }
    }
    SCALE.set(scale);
    let Ok(mut bar) = barui::Bar::new(scale) else { return };
    let icon = |path: &str| {
        let ic = if std::path::Path::new(path).exists() { item_icon(path, bar_icon_px()) } else { null_mut() };
        wicon(ic)
    };
    let apps: Vec<barui::AppView> = (0..8)
        .map(|i| {
            let (_, color, wins, exe) = APPS[i % APPS.len()];
            barui::AppView { icon: icon(exe), wins, active: i == 1, flash: false, color: ui::gfx::Rgba::hex(color) }
        })
        .collect();
    let (sw, sh) = ((screen_w * scale).round() as i32, (screen_h * scale).round() as i32);
    let view = barui::View {
        style,
        side,
        size: 1,
        align_left: false,
        screen: (screen_w, screen_h),
        op: 0.85,
        apps,
        tray: Vec::new(),
        battery: None,
        vol: (72, false),
        net: Net::Wifi(85),
        lang: "RUS".into(),
        time: "03:57".into(),
        date: "11/26/2026".into(),
        unread: 0,
        dnd: false,
    };
    let lit = barui::Live {
        now: 100_000,
        hot: if what == "tip" || what == "preview" { at } else { NONE },
        tray: what == "tray",
        lang: what == "lang",
        start: what == "start",
        search: what == "search",
        gear: what.starts_with("settings"),
        qs: what == "quick" || what == "wifi",
        cal: what == "calendar",
        task: what == "taskview",
        jump: (what == "jumplist").then_some(at),
        ..Default::default()
    };
    if benchmarking && what == "bar" {
        let mut bench = Bench::new();
        for i in 0..120 {
            ui::gfx::bench_start();
            let start = std::time::Instant::now();
            let p = bar.frame(&view, &barui::Live { now: i * 4, ..lit });
            let elapsed = start.elapsed();
            let end = ui::gfx::bench_end();
            let start = std::time::Instant::now();
            bench.show(bar.surfaces().0, p.alpha);
            if p.shadow.is_some() { bench.show(bar.surfaces().1, p.shadow_alpha); }
            bench.record(elapsed, end, start.elapsed());
        }
        bench.report("bar");
        return;
    }
    bar.frame(&view, &barui::Live { now: 0, hot: NONE, press: NONE, ..Default::default() });
    bar.frame(&view, &lit);
    let p = bar.frame(&view, &barui::Live { now: 110_000, ..lit });

    // a backdrop with something to blur: a dark desktop with a few lit windows
    let mut canvas: Vec<u32> = (0..sw * sh)
        .map(|i| {
            let (x, y) = ((i % sw) as f32 / scale, (i / sw) as f32 / scale);
            let mut c = [5.0, 7.0, 12.0];
            let mut lit = |x0: f32, y0: f32, w: f32, h: f32, col: [f32; 3]| {
                if x >= x0 && x < x0 + w && y >= y0 && y < y0 + h {
                    c = col;
                }
            };
            lit(300.0, 120.0, 520.0, 300.0, [34.0, 38.0, 52.0]);
            lit(820.0, 560.0, 420.0, 230.0, [96.0, 120.0, 190.0]);
            lit(900.0, 600.0, 120.0, 40.0, [240.0, 240.0, 245.0]);
            lit(1060.0, 640.0, 140.0, 120.0, [232.0, 112.0, 90.0]);
            lit(700.0, 500.0, 160.0, 160.0, [60.0, 190.0, 120.0]);
            if busy {
                // text-like stripes everywhere: what a panel with a hole in its glass would show through it
                if (y as i32 % 6) < 2 && (x as i32 % 23) < 15 {
                    c = [235.0, 235.0, 240.0];
                }
            }
            0xff00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
        })
        .collect();
    let (bar_surf, shadow_surf) = bar.surfaces();
    if let Some((x, y, w, _)) = p.shadow {
        blend(&mut canvas, (sw, sh), shadow_surf.pixels(), w, (x, y), p.shadow_alpha);
    }
    blend(&mut canvas, (sw, sh), bar_surf.pixels(), p.win.2, (p.win.0, p.win.1), p.alpha);

    let mut gfx = ui::gfx::Gfx::new(scale).expect("the renderer");
    let key = match (what.split(':').next().unwrap_or(""), kind) {
        ("lang", _) => LANG,
        ("tip" | "jumplist" | "preview", _) => at,
        ("taskview" | "switcher", _) => TASKVIEW,
        ("menu", "clock") => CLOCK,
        ("menu", "winx") => 0,
        ("menu", _) => at,
        ("quick" | "wifi", _) => WIFI_I,
        ("calendar" | "popups", _) => CLOCK,
        ("start", _) => 0,
        ("search", _) => SEARCH,
        ("settings" | "toast", _) => GEAR,
        _ => TAPP,
    };
    let r = bar.screen_rect(key).unwrap_or((0, 0, 0, 0));
    let anchor = Anchor { side, button: r, bar: (p.rest.0, p.rest.1, p.rest.0 + p.rest.2, p.rest.1 + p.rest.3), screen: (sw, sh) };
    let around = |b: (i32, i32, i32, i32)| {
        let m = (110.0 * scale) as i32;
        let (x0, y0, x1, y1) = (b.0 - m, b.1 - m, b.0 + b.2 + m, b.1 + b.3 + m);
        (x0.min(anchor.bar.0).max(0), y0.min(anchor.bar.1).max(0), x1.max(anchor.bar.2).min(sw), y1.max(anchor.bar.3).min(sh))
    };
    let whole = what == "search";
    let crop = if what == "toast" {
        let mut toast = ui::toast::Toast::new(scale, (sw, sh));
        toast.show(&mut gfx, text, side, (p.rest.0, p.rest.1, p.rest.0 + p.rest.2, p.rest.1 + p.rest.3), 0);
        let (win, _) = toast.frame(&mut gfx, t);
        blend(&mut canvas, (sw, sh), toast.surface().pixels(), win.2, (win.0, win.1), 255);
        around(win)
    } else if what == "tip" {
        let mut tip = ui::tip::Tip::new(scale, (sw, sh));
        let text = ["Claude", "File manager", "Microsoft Edge", "Spotify", "Task Manager", "Outlook", "Discord", "Telegram"][(at.max(1) as usize - 1) % 8];
        let along = if side == Side::Bottom { (r.0 + r.2) / 2 } else { (r.1 + r.3) / 2 };
        tip.show(&mut gfx, text, ui::tip::Place::button(side, r, along, scale), 0, 250, None);
        let (win, _) = tip.frame(&mut gfx, t);
        blend(&mut canvas, (sw, sh), tip.surface().pixels(), win.2, (win.0, win.1), 255);
        around(win)
    } else {
        let content: Box<dyn ui::panel::Content> = if what == "quick" || what == "wifi" {
            let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
            let panel = qp::QuickPanel::new(Box::new(Demo::from(args)), side, what == "wifi");
            Box::new(match (get("ask"), get("menu")) {
                (Some(ssid), _) => panel.asking(ssid),
                (_, Some(ssid)) => panel.with_menu(ssid),
                _ => panel,
            })
        } else if what == "popups" {
            Box::new(ui::tray_popup::TrayPanel::new(demo_cells, |_, _, _| {})) // not shown: `render_popups` takes over
        } else if what == "calendar" {
            let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
            let today = get("today").and_then(parse_date).unwrap_or((2026, 11, 26));
            DEMO_DATE.set(today);
            let view = get("view").and_then(parse_date).map_or((today.0, today.1), |d| (d.0, d.1));
            // the whole column: the mockup's room is the screen less 12 at the top and the bar's 12 and height at the bottom
            let room = if side == Side::Bottom { (p.rest.1 as f32 / scale) - 24.0 } else { screen_h - 24.0 };
            Box::new(ui::cal_popup::CalPanel::new(demo_now, side, room).pick(view, get("sel").and_then(parse_date).unwrap_or(today)))
        } else if what == "start" || what == "search" {
            apps::load(); // the machine's apps, synchronously, with the icons of what is pinned
            let recent = apps::recent(if what == "start" { 4 } else { 3 });
            if what == "start" {
                let items = if query.is_empty() { apps::all().into_iter().take(pins).collect() } else { apps::search(&query).into_iter().take(12).collect() };
                Box::new(ui::start_panel::StartPanel::with_state(&query, page, items, recent, apps::user()))
            } else {
                let files = if query.chars().count() < 2 {
                    Vec::new()
                } else {
                    winrust_common::search::ask_all(&query).into_iter().take(3).map(|p| (std::fs::metadata(&p).is_ok_and(|m| m.is_dir()), p)).map(|(d, p)| (p, d)).collect()
                };
                Box::new(ui::search_panel::SearchPanel::with_query(900.0, &query, files, recent))
            }
        } else if what.starts_with("settings") {
            use ui::settings_panel::{Info, Page, SettingsPanel, System, Upd};
            let page = what.split_once(':').and_then(|(_, p)| Page::parse(p)).unwrap_or(Page::Main);
            let themes: Vec<_> = (0..tpl)
                .map(|i| i18n::style::Theme {
                    name: format!("Template {}", i + 1),
                    style: i18n::style::ALL[(i + 1) % 3],
                    colors: if i % 2 == 0 { [u32::MAX; 7] } else { [0x2a1a10, u32::MAX, u32::MAX, u32::MAX, 0x48a3ff, u32::MAX, u32::MAX] },
                    opacity: [85, 70, 55][i % 3],
                    accent: i18n::style::Accent::ALL[(i + 1) % 5],
                })
                .collect();
            let info = Info {
                version: "0.5.9".into(),
                auto: || Some(true),
                act: |_| {},
                wallpaper: || r"C:\Windows\Web\Wallpaper\Windows\img19.jpg".into(),
                system: || System { shell: false, tray: None, search_key: true, taskmgr_key: true, clipboard: true, wsearch_off: Some(true), taskbar_hidden: true },
            };
            let mut panel = SettingsPanel::new(info, themes, page);
            let state = match upd {
                "busy" => Upd::Busy(Some(0)),
                "done" => Upd::Done(0),
                _ => Upd::Idle,
            };
            panel.show(open_row, state, tiled, del, match dir_arg { "fwd" => Some(true), "bwd" => Some(false), _ => None });
            Box::new(panel)
        } else if what == "jumplist" {
            demo_jump(side, n_wins, recent, &icon, at)
        } else if what == "preview" {
            demo_preview(n_wins, &icon, at)
        } else if what == "taskview" {
            demo_taskview(scale, (sw, sh), n_wins, n_desks, &icon)
        } else if what == "switcher" {
            demo_switcher(scale, (sw, sh), n_wins, per_monitor, &icon)
        } else if what == "menu" {
            demo_menu(kind, side, (sh as f32 / scale) - 130.0, side == Side::Right)
        } else if let Some(which) = what.strip_prefix("power-") {
            let kind = if which == "shutdown" { ui::power_popup::Kind::Shutdown } else { ui::power_popup::Kind::Restart };
            let panel = ui::power_popup::PowerPanel::new(kind, |_| {});
            Box::new(if which == "busy" { panel.busy_since(0) } else { panel })
        } else if what == "lang" {
            let items = if real {
                layout_rows()
            } else {
                [("RUS", "Русский", "Russian · ЙЦУКЕН", true), ("ENG", "English", "United States · QWERTY", false), ("TUR", "Türkçe", "Turkish Q", false)]
                    .iter()
                    .enumerate()
                    .map(|(i, &(code, name, sub, on))| ui::lang_popup::Layout { id: i as isize, code: code.into(), name: name.into(), sub: sub.into(), on })
                    .collect()
            };
            Box::new(ui::lang_popup::LangPanel::new(items, |_| {}, || {}))
        } else {
            let exes = [
                r"C:\Windows\System32\notepad.exe",
                r"C:\Windows\System32\mspaint.exe",
                r"C:\Windows\explorer.exe",
                r"C:\Windows\System32\calc.exe",
                r"C:\Windows\System32\cmd.exe",
                r"C:\Windows\System32\control.exe",
                r"C:\Windows\System32\Taskmgr.exe",
                r"C:\Windows\System32\charmap.exe",
            ];
            DEMO.with_borrow_mut(|d| {
                *d = (0..n_icons).map(|i| ui::tray_popup::Cell { icon: icon(exes[i % exes.len()]), tip: format!("Icon {}", i + 1), token: i }).collect();
            });
            Box::new(ui::tray_popup::TrayPanel::new(demo_cells, |_, _, _| {}))
        };
        let snap = canvas.clone();
        let mut grab = |x: i32, y: i32, w: i32, h: i32| {
            Some((0..h).flat_map(|row| snap[((y + row) * sw + x) as usize..((y + row) * sw + x + w) as usize].iter().copied()).collect::<Vec<u32>>())
        };
        if what == "popups" {
            render_popups((&mut gfx, canvas.as_mut_slice()), (sw, sh), (file, scale, t), (anchor, hover), (&p, bar_surf));
            return;
        }
        let mut popup = Popup::new(&mut gfx, content, anchor, base, &mut grab);
        popup.set_hover(hover);
        if benchmarking {
            let mut bench = Bench::new();
            for i in 0..120 {
                ui::gfx::bench_start();
                let start = std::time::Instant::now();
                let shown = popup.frame(&mut gfx, i * 4);
                let elapsed = start.elapsed();
                let end = ui::gfx::bench_end();
                let start = std::time::Instant::now();
                if let Some((_, alpha)) = shown.scrim { bench.show(popup.scrim_surface(), alpha); }
                if shown.shadow.2 > 0 && shown.shadow.3 > 0 { bench.show(popup.surfaces().1, shown.shadow_alpha); }
                bench.show(popup.surfaces().0, shown.alpha);
                bench.record(elapsed, end, start.elapsed());
            }
            bench.report(&what);
            return;
        }
        let _ = popup.frame(&mut gfx, base); // the first frame stamps what opens with the panel (the rising blocks of the first page)
        if let Some(id) = click {
            // the panel at rest, a click on item `id`, then the frame `after` ms later (the ones in between run as the live timer does)
            let _ = popup.frame(&mut gfx, base + t);
            let _ = popup.click_id(id); // what it asks of the bar (a repaint, a toast) is not done here
            for ms in (0..after).step_by(16) {
                let _ = popup.frame(&mut gfx, base + t + ms);
            }
        }
        let shown = popup.frame(&mut gfx, base + t + after);
        for (i, h) in popup.holes().iter().enumerate() {
            fake_window(&mut canvas, (sw, sh), h.rect, i);
        }
        let (panel, shade) = popup.surfaces();
        if let Some((r, a)) = shown.scrim {
            blend(&mut canvas, (sw, sh), popup.scrim_surface().pixels(), r.2, (r.0, r.1), a);
        }
        blend(&mut canvas, (sw, sh), shade.pixels(), shown.shadow.2, (shown.shadow.0, shown.shadow.1), shown.shadow_alpha);
        blend(&mut canvas, (sw, sh), panel.pixels(), shown.win.2, (shown.win.0, shown.win.1), shown.alpha);
        blend(&mut canvas, (sw, sh), bar_surf.pixels(), p.win.2, (p.win.0, p.win.1), p.alpha); // the bar stands above every panel
        if let Some((text, along, top)) = popup.tip() {
            let mut tip = ui::tip::Tip::new(scale, (sw, sh));
            let place = ui::tip::Place { side: Side::Bottom, along, edge: top - (8.0 * scale).round() as i32 };
            tip.show(&mut gfx, &text, place, 0, 0, None);
            let (win, _) = tip.frame(&mut gfx, 5000);
            blend(&mut canvas, (sw, sh), tip.surface().pixels(), win.2, (win.0, win.1), 255);
        }
        if whole { (0, 0, sw, sh) } else { around(popup.rect()) }
    };
    let (x0, y0, x1, y1) = crop;
    let (w, h) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in y0..y1 {
        out.extend_from_slice(&canvas[(y * sw + x0) as usize..(y * sw + x1) as usize]);
    }
    if gfx.save_png(file, w as u32, h as u32, &out).is_err() {
        let _ = std::fs::write(format!("{file}.err.txt"), "could not write the png");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the notifications of the mockup, for `--render-panel calendar` and `popups`

/// The clock of the notification renders: the frame at `t` ms stands at `NOTIFY_BASE + t`, so notes can be hours old.
const NOTIFY_BASE: u64 = 36_000_000;

fn demo_app(key: &str) -> notify::App {
    use notify::{App, AppIcon};
    let (name, icon) = match key {
        "telegram" => ("Telegram", AppIcon::Plate("M20.5 4.5L3.5 11l5 2 2 6 3-4 5 3z", 0x37aee2, 0x1e96c8)),
        "discord" => ("Discord", AppIcon::Plate("M6 8c3-2 9-2 12 0 1 3 1 7-1 10-2 1-3 1-5 0-2 1-3 1-5 0-2-3-2-7-1-10zM9.5 12.5h.01M14.5 12.5h.01", 0x7a84f7, 0x4752c4)),
        "tm" => ("Task Manager", AppIcon::Plate("M4 18V9M9 18V5M14 18v-7M19 18V7", 0x3a3f4b, 0x22252d)),
        "winrust" => ("WinRust", AppIcon::Winrust),
        "cal" => ("Calendar", AppIcon::Plate("M5 8h14v11H5zM5 11h14M9 5v3M15 5v3", 0xe5484d, 0xc13a40)),
        _ => ("Screenshot", AppIcon::Plate("M8 5H5v3M16 5h3v3M8 19H5v-3M16 19h3v-3M12 12h.01", 0x7b6cf0, 0x5b4bd8)),
    };
    App { key: key.into(), name: name.into(), icon }
}

/// Fills the store the way the mockup's states do: the seven notes of `NDEF` (`notes=demo`, the default) or none, the pop-ups
/// of `popups` (`pops=msg,shot,cal,sus`), Do not disturb, an open stack, an answer being typed, "Clear all" just pressed.
fn demo_notes(popups: bool, which: &str, expand: &str, reply: Option<&str>, (dnd, undo): (bool, bool), pops: &str, base: u64) {
    use notify::{Act, Do, Note};
    let min = 60_000u64;
    let wall = r"C:\Windows\Web\Wallpaper\Windows\img19.jpg";
    let acts = |l: &[(&str, Do)]| l.iter().map(|(t, d)| Act { label: (*t).into(), does: d.clone() }).collect::<Vec<_>>();
    let chat = || acts(&[("Reply", Do::Reply), ("Mark as read", Do::Read)]);
    let go = |s: &str| Do::Open(s.into());
    let mk = |app: &str, title: &str, body: &str, a: Vec<Act>| Note::new(demo_app(app), title, body).acts(a);
    let asel = || mk("telegram", "Asel", "Скинула новые макеты установщика, глянь когда будет минутка", chat()).avatar('A', 0xf472b6, 0xbe185d);
    notify::with(|s| {
        *s = notify::Store::new();
        if popups {
            for (k, name) in pops.split(',').enumerate() {
                let n = match name {
                    "shot" => mk("snip", "Screenshot saved", "Copied to clipboard \u{b7} Pictures \u{203a} Screenshots", acts(&[("Open", go("x")), ("Show in folder", Do::Show("x".into()))])).image(wall),
                    "cal" => mk("cal", "Design sync in 10 minutes", "19:30 \u{2013} 20:00 \u{b7} Google Meet", acts(&[("Join", go("x")), ("Snooze", Do::Read)])),
                    "long" => mk("telegram", "A very long title that does not fit on one line of a pop-up at all, truly", "The body of this one runs on and on and on, past the three lines a pop-up allows, so that its last line has to end in an ellipsis instead of spilling out of the card over the buttons.", chat()).avatar('A', 0xf472b6, 0xbe185d),
                    "sus" => mk("tm", "winhelper.exe looks suspicious", "Unsigned, runs from %TEMP% and opens outbound connections.", acts(&[("End task", go("x")), ("Show", Do::Show("x".into()))])).danger(),
                    _ => mk("telegram", "Asel", "Ты тут? Созвонимся в 19:30?", chat()).avatar('A', 0xf472b6, 0xbe185d),
                };
                s.push(n, base + [0, 500, 950][k.min(2)]);
            }
        } else {
            s.open_centre(); // the centre is on show: nothing pops up
            if which == "demo" || undo {
                // oldest first, the list stands newest first; the first four are read
                let list = [
                    (180, mk("discord", "#design-review", "Kira: the new Alt+Tab looks great, ship it", vec![]).avatar('#', 0x818cf8, 0x4338ca), false),
                    (120, mk("telegram", "WinRust chat", "Lemxy: new nightly is up, please test the topbar on 2 monitors", chat()).avatar('W', 0x34d399, 0x047857), false),
                    (60, mk("snip", "Screenshot saved", "Pictures \u{203a} Screenshots \u{203a} desktop.png", acts(&[("Open", go("x")), ("Show in folder", Do::Show("x".into()))])).image(wall), false),
                    (60, mk("winrust", "WinRust 0.5.2 is ready", "Faster search indexing and a new Terminal. Restart the bar to update.", acts(&[("Update now", go("x")), ("Later", Do::Read)])), false),
                    (21, mk("tm", "winhelper.exe looks suspicious", "Unsigned, runs from %TEMP% and opens outbound connections.", acts(&[("End task", go("x")), ("Show", Do::Show("x".into()))])).danger(), true),
                    (14, mk("telegram", "Dima", "PR с фото-вьюером посмотрел, пара комментов по анимации", chat()).avatar('D', 0x60a5fa, 0x1d4ed8), true),
                    (2, asel(), true),
                ];
                for (ago, n, unread) in list {
                    let id = s.push(n, base.saturating_sub(ago * min));
                    if !unread {
                        s.mark_read(id);
                    }
                }
            }
            if undo {
                s.clear_all(base);
                s.tick(base + 400);
            }
        }
        s.set_dnd(dnd);
        if !expand.is_empty() {
            s.toggle_stack(expand);
        }
        if let (Some(text), Some(id)) = (reply, s.notes().iter().find(|n| n.acts.iter().any(|a| a.does == Do::Reply)).map(|n| n.id)) {
            s.start_reply(id);
            text.chars().for_each(|c| s.reply_char(c));
        }
    });
}

/// `popups`: the cards of the pop-up stack over the desktop, with the bar.
fn render_popups(
    (gfx, canvas): (&mut ui::gfx::Gfx, &mut [u32]),
    (sw, sh): (i32, i32),
    (file, scale, t): (&str, f32, u64),
    (anchor, hover): (ui::panel::Anchor, Option<i32>),
    (p, bar_surf): (&barui::Placement, &ui::gfx::Surface),
) {
    let mut stack = ui::notice_pop::Stack::new(scale, anchor);
    let snap = canvas.to_vec();
    let mut grab = |x: i32, y: i32, w: i32, h: i32| {
        Some((0..h).flat_map(|row| snap[((y + row) * sw + x) as usize..((y + row) * sw + x + w) as usize].iter().copied()).collect::<Vec<u32>>())
    };
    // the first frame takes the backdrop, then the one asked for
    let _ = stack.frame(gfx, NOTIFY_BASE, &mut grab);
    let _ = stack.frame(gfx, NOTIFY_BASE + t, &mut grab);
    // the pointer comes onto a card, and what it does to the card has time to run
    let after = if hover.is_some() { 400 } else { 0 };
    if let Some(k) = hover {
        stack.point_at(usize::try_from(k).unwrap_or(0));
        let _ = stack.frame(gfx, NOTIFY_BASE + t + 10, &mut grab);
    }
    let shown = stack.frame(gfx, NOTIFY_BASE + t + after, &mut grab);
    let (surf, shade) = stack.surfaces();
    blend(canvas, (sw, sh), shade.pixels(), shown.shade.2, (shown.shade.0, shown.shade.1), 255);
    blend(canvas, (sw, sh), surf.pixels(), shown.win.2, (shown.win.0, shown.win.1), 255);
    blend(canvas, (sw, sh), bar_surf.pixels(), p.win.2, (p.win.0, p.win.1), p.alpha);
    let m = (30.0 * scale) as i32;
    let (x0, y0, x1, y1) = ((shown.win.0 - m).max(0), (shown.win.1 - m).max(0), sw, sh);
    let (w, h) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in y0..y1 {
        out.extend_from_slice(&canvas[(y * sw + x0) as usize..(y * sw + x1) as usize]);
    }
    if gfx.save_png(file, w as u32, h as u32, &out).is_err() {
        let _ = std::fs::write(format!("{file}.err.txt"), "could not write the png");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// what the panels of the mockup hold, for `--render-panel`

type Hi = windows::Win32::UI::WindowsAndMessaging::HICON;

const DEMO_EXES: [&str; 3] = [r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", r"C:\Windows\explorer.exe", r"C:\Windows\System32\notepad.exe"];

fn demo_icon(icon: &dyn Fn(&str) -> Hi, i: usize) -> Hi {
    let ic = icon(DEMO_EXES[i % 3]);
    if ic.0.is_null() { icon(r"C:\Windows\System32\notepad.exe") } else { ic }
}

fn demo_jump(side: Side, n_wins: usize, recent: bool, icon: &dyn Fn(&str) -> Hi, at: i32) -> Box<dyn ui::panel::Content> {
    use ui::jump::{Act, Data, JumpPanel, Recent, Win};
    let titles = ["Lemxy/WinRust \u{b7} GitHub", "heydamir/unslop \u{b7} GitHub", "New Tab"];
    let wins: Vec<Win> = (0..n_wins.min(3)).map(|i| Win { hwnd: i as isize + 1, title: titles[i].into(), active: i == 0 }).collect();
    let running = !wins.is_empty();
    let name = ["Claude", "File manager", "Microsoft Edge", "Spotify", "Task Manager", "Outlook", "Discord", "Telegram"][(at.max(1) as usize - 1) % 8];
    let meta = if running { format!("Pinned \u{b7} {} window{}", wins.len(), if wins.len() == 1 { "" } else { "s" }) } else { "Pinned \u{b7} not running".into() };
    let act = |icon: &'static str, label: &str| Act { icon, label: label.into() };
    let acts = vec![
        act("M4 5h16v14H4zM12 9v6M9 12h6", "New window"),
        act("M12 17v5M9 4h6l-1 5 3 3v2H7v-2l3-3zM3 3l18 18", "Unpin from the bar"),
        act("M15 6l-6 6 6 6", "Move left"),
        act("M9 6l6 6-6 6", "Move right"),
        act("M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM14 16.5h6", "Remove from Start"),
    ];
    let recent = if recent {
        vec![Recent { name: "WinRust".into(), folder: true }, Recent { name: "README.md".into(), folder: false }, Recent { name: "Plan 2027.docx".into(), folder: false }]
    } else {
        Vec::new()
    };
    let d = Data {
        icon: demo_icon(icon, 0),
        name: name.into(),
        meta,
        running,
        wins,
        recent,
        acts,
        close: running.then(|| if n_wins > 1 { "Close all windows".to_string() } else { "Close window".to_string() }),
        kill: running.then(|| "End process".to_string()),
    };
    Box::new(JumpPanel::new(d, side, |_| {}))
}

fn demo_preview(n_wins: usize, icon: &dyn Fn(&str) -> Hi, at: i32) -> Box<dyn ui::panel::Content> {
    use ui::preview::{Card, PreviewPanel};
    let titles = ["Lemxy/WinRust \u{b7} GitHub", "heydamir/unslop \u{b7} GitHub", "New Tab"];
    let cards: Vec<Card> = (0..n_wins.min(3)).map(|i| Card { hwnd: i as isize + 1, title: titles[i].into(), src: (1280, 720 + 80 * i as i32), minimized: false, current: i == 0 }).collect();
    let color = APPS[(at.max(1) as usize - 1) % 8].1;
    Box::new(PreviewPanel::new(cards, demo_icon(icon, 0), ui::gfx::Rgba::hex(color), false, |_| {}))
}

fn demo_taskview(scale: f32, screen: (i32, i32), n_wins: usize, n_desks: usize, icon: &dyn Fn(&str) -> Hi) -> Box<dyn ui::panel::Content> {
    use std::rc::Rc;
    use ui::taskview::{Desk, OwnedIcon, TaskView, WinCard};
    let fr = |l: f32, t: f32, w: f32, h: f32| ((l * scale) as i32, (t * scale) as i32, ((l + w) * scale) as i32, ((t + h) * scale) as i32);
    let frames = [fr(300.0, 120.0, 850.0, 560.0), fr(520.0, 200.0, 760.0, 300.0), fr(80.0, 90.0, 640.0, 420.0), fr(700.0, 60.0, 600.0, 400.0)];
    let titles = ["File manager", "Task Manager", "Lemxy/WinRust \u{b7} GitHub", "Notepad"];
    let cards: Vec<WinCard> = (0..n_wins.min(4))
        .map(|i| WinCard { hwnd: i as isize + 1, title: titles[i].into(), icon: Rc::new(OwnedIcon(demo_icon(icon, i + 1))), frame: frames[i], minimized: i == 2 })
        .collect();
    let on_first = [(0.2, 0.13, 0.59, 0.62), (0.36, 0.22, 0.52, 0.33)];
    let desks: Vec<Desk> = (0..n_desks)
        .map(|i| Desk { name: format!("Desktop {}", i + 1), current: i == 0, wallpaper: r"C:\Windows\Web\Wallpaper\Windows\img19.jpg".into(), wins: if i == 0 { on_first.iter().map(|&r| (0, r)).collect() } else { Vec::new() } })
        .collect();
    Box::new(TaskView::new(scale, screen, desks, n_desks < 4, cards, false, |_| {}))
}

fn demo_switcher(scale: f32, screen: (i32, i32), n_wins: usize, per_monitor: bool, icon: &dyn Fn(&str) -> Hi) -> Box<dyn ui::panel::Content> {
    use std::rc::Rc;
    use ui::taskview::{OwnedIcon, Switch, Switcher, WinCard};
    let fr = |l: f32, t: f32, w: f32, h: f32| ((l * scale) as i32, (t * scale) as i32, ((l + w) * scale) as i32, ((t + h) * scale) as i32);
    let frames = [fr(300.0, 120.0, 850.0, 560.0), fr(520.0, 200.0, 760.0, 300.0), fr(80.0, 90.0, 640.0, 420.0), fr(700.0, 60.0, 600.0, 400.0)];
    let titles = ["File manager", "Task Manager", "Lemxy/WinRust \u{b7} GitHub", "Notepad"];
    let cards: Vec<WinCard> = (0..n_wins)
        .map(|i| WinCard { hwnd: i as isize + 1, title: titles[i % 4].into(), icon: Rc::new(OwnedIcon(demo_icon(icon, i + 1))), frame: frames[i % 4], minimized: i == 3 })
        .collect();
    Box::new(Switcher::new(scale, (0, 0, screen.0, screen.1), cards, per_monitor, Switch { selected: || 1, point: |_| {}, pick: |_| {} }))
}

fn demo_menu(kind: &str, side: Side, max_h: f32, sub_left: bool) -> Box<dyn ui::panel::Content> {
    if kind == "winx" {
        // SAFETY: reads the machine's icons for the tiles, on the renderer's thread.
        return unsafe { menus::winx_content(side, max_h, sub_left) };
    }
    let (items, w) = menus::bar_menu_items(kind);
    Box::new(ui::menu::MenuPanel::new(items, w, max_h, side, |_| {}))
}

/// Something that looks like a window where a live thumbnail would be: a title bar, a tab, a few lines.
fn fake_window(canvas: &mut [u32], (sw, sh): (i32, i32), r: (i32, i32, i32, i32), seed: usize) {
    let (w, h) = (r.2 - r.0, r.3 - r.1);
    let hero = [0xff3a5f8a_u32, 0xff8a4a3a, 0xff3a8a5f][seed % 3];
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (r.0 + x, r.1 + y);
            if px < 0 || py < 0 || px >= sw || py >= sh {
                continue;
            }
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let line = (0.3..0.4).contains(&fy) && (0.58..0.94).contains(&fx) || (0.45..0.52).contains(&fy) && (0.58..0.84).contains(&fx) || (0.7..0.76).contains(&fy) && (0.06..0.9).contains(&fx);
            let c = if fy < 0.1 {
                0xff26272f
            } else if (0.12..0.2).contains(&fy) && (0.05..0.95).contains(&fx) {
                0xff32343c
            } else if (0.3..0.6).contains(&fy) && (0.06..0.5).contains(&fx) {
                hero
            } else if line {
                0xff3a3c46
            } else {
                0xff1d1e24
            };
            canvas[(py * sw + px) as usize] = c;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// `topbar --render-thumbs <file.png>`: does `DwmRegisterThumbnail` draw into the windows the panels are made of? Creates a
/// source window (left half red, right half blue), then tries three ways to show its thumbnail and looks at the screen:
/// A) registered on a layered window painted with `UpdateLayeredWindow` (what a panel is); B) registered on a plain window
/// under that layered one, which has a hole over the thumbnail (what `ui::live` does); C) on a layered window set up with
/// `SetLayeredWindowAttributes` (what the old flyouts were). Prints the colours found to `<file.png>.txt`. The windows are on
/// the screen for about a second and never take the focus.
pub(crate) unsafe fn render_thumbs(args: &[String]) {
    use ui::gfx::{Gfx, Rect, Rgba, Surface};
    use windows_sys::Win32::Graphics::Dwm::{DWM_THUMBNAIL_PROPERTIES, DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE, DwmFlush, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties};
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    SetProcessDPIAware();
    let Some(file) = args.first() else { return };
    let hi = GetModuleHandleW(null());
    unsafe extern "system" fn gray_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_PAINT {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(h, &mut ps);
            let mut r: RECT = zeroed();
            GetClientRect(h, &mut r);
            let b = CreateSolidBrush(0x0040_4040);
            FillRect(dc, &r, b);
            DeleteObject(b);
            EndPaint(h, &ps);
            return 0;
        }
        DefWindowProcW(h, m, w, l)
    }
    unsafe extern "system" fn src_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_PAINT {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(h, &mut ps);
            let mut r: RECT = zeroed();
            GetClientRect(h, &mut r);
            let (red, blue) = (CreateSolidBrush(0x0000_00FF), CreateSolidBrush(0x00FF_0000));
            FillRect(dc, &RECT { left: 0, top: 0, right: r.right / 2, bottom: r.bottom }, red);
            FillRect(dc, &RECT { left: r.right / 2, top: 0, right: r.right, bottom: r.bottom }, blue);
            DeleteObject(red);
            DeleteObject(blue);
            EndPaint(h, &ps);
            return 0;
        }
        DefWindowProcW(h, m, w, l)
    }
    let class = |name: &str, p: WNDPROC| {
        let n = wide(name);
        RegisterClassW(&WNDCLASSW { lpfnWndProc: p, hInstance: hi, lpszClassName: n.as_ptr(), ..zeroed() });
        n
    };
    let (src_c, gray_c) = (class("topbar_probe_src", Some(src_proc)), class("topbar_probe_gray", Some(gray_proc)));
    let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
    let make = |c: &[u16], ex: u32, x: i32, y: i32, w: i32, h: i32| CreateWindowExW(ex, c.as_ptr(), null(), WS_POPUP | WS_VISIBLE, x, y, w, h, null_mut(), null_mut(), hi, null());
    let pump = |ms: u32| {
        let end = GetTickCount64() + u64::from(ms);
        let mut m: MSG = zeroed();
        while GetTickCount64() < end {
            while PeekMessageW(&mut m, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        DwmFlush();
    };
    let src = make(&src_c, ex, 40, 40, 300, 200);
    pump(200);
    let (px, py, pw, ph) = (500, 300, 400, 300);
    let rect = RECT { left: 50, top: 50, right: 350, bottom: 250 };
    let props = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE, rcDestination: rect, rcSource: zeroed(), opacity: 255, fVisible: 1, fSourceClientAreaOnly: 0 };
    let mut gfx = Gfx::new(1.0).expect("the renderer");
    let mut grab = Surface::new();
    let mut report = String::new();
    // what colour the screen shows at three places of the probe area: left of the thumbnail, right of it, in the glass
    let look = |g: &mut Surface| -> (u32, u32, u32) {
        g.grab(px, py, pw, ph);
        let p = g.pixels();
        let at = |x: i32, y: i32| p[(y * pw + x) as usize] & 0x00ff_ffff;
        (at(130, 150), at(270, 150), at(20, 20))
    };
    let tint = |gfx: &mut Gfx, s: &mut Surface, a: f32, hole: bool| {
        let _ = gfx.paint(s, pw, ph, |c| {
            c.fill(Rect::new(0.0, 0.0, pw as f32, ph as f32), 20.0, Rgba::css(16, 17, 24, a));
            if hole {
                c.erase(Rect::new(50.0, 50.0, 300.0, 200.0), 12.0);
                c.fill(Rect::new(50.0, 50.0, 300.0, 200.0), 12.0, Rgba::new(0.0, 0.0, 0.0, 1.0 / 255.0));
            }
        });
    };

    // A: a layered window with per-pixel alpha, the thumbnail on it
    let l = make(&gray_c, ex | WS_EX_LAYERED, px, py, pw, ph);
    let mut surf = Surface::new();
    tint(&mut gfx, &mut surf, 0.3, false);
    surf.present(whwnd(l), px, py, 255);
    let mut id = 0isize;
    let r = DwmRegisterThumbnail(l, src, &mut id);
    DwmUpdateThumbnailProperties(id, &props);
    pump(500);
    let (a, b, c) = look(&mut grab);
    report += &format!("A UpdateLayeredWindow, thumbnail on it: register={r} left={a:06x} right={b:06x} corner={c:06x}\n");
    DwmUnregisterThumbnail(id);
    DestroyWindow(l);

    // B: a plain window under a layered one with a hole
    let h = make(&gray_c, ex, px, py, pw, ph);
    let mut id = 0isize;
    let r = DwmRegisterThumbnail(h, src, &mut id);
    DwmUpdateThumbnailProperties(id, &props);
    let l = make(&gray_c, ex | WS_EX_LAYERED, px, py, pw, ph);
    tint(&mut gfx, &mut surf, 0.82, true);
    surf.present(whwnd(l), px, py, 255);
    SetWindowPos(h, l, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); // under it
    pump(500);
    let (a, b, c) = look(&mut grab);
    let top = WindowFromPoint(POINT { x: px + 130, y: py + 150 });
    let glass = WindowFromPoint(POINT { x: px + 20, y: py + 20 });
    let which = |w: HWND| if w == l { "layered" } else if w == h { "plain" } else { "other" };
    report += &format!("B plain window under a layered one with a hole: register={r} left={a:06x} right={b:06x} corner={c:06x} pointer in the hole: {} in the glass: {}\n", which(top), which(glass));
    let _ = gfx.save_png(file, pw as u32, ph as u32, grab.pixels());
    DwmUnregisterThumbnail(id);
    DestroyWindow(l);
    DestroyWindow(h);

    // C: a layered window with a constant alpha (the old flyouts)
    let l = make(&gray_c, ex | WS_EX_LAYERED, px, py, pw, ph);
    SetLayeredWindowAttributes(l, 0, 255, LWA_ALPHA);
    let mut id = 0isize;
    let r = DwmRegisterThumbnail(l, src, &mut id);
    DwmUpdateThumbnailProperties(id, &props);
    pump(500);
    let (a, b, c) = look(&mut grab);
    report += &format!("C SetLayeredWindowAttributes, thumbnail on it: register={r} left={a:06x} right={b:06x} corner={c:06x}\n");
    DwmUnregisterThumbnail(id);
    DestroyWindow(l);
    DestroyWindow(src);
    let _ = std::fs::write(format!("{file}.txt"), report);
}
