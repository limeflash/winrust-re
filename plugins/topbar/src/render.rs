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
            _ => match a.as_str() {
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
        }
    }

    fn tile(&mut self, _i: usize, _cx: &mut ui::panel::Cx) {}
    fn volume(&mut self, _percent: i32) {}
    fn mute(&mut self) {}
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

thread_local! {
    static DEMO_DATE: std::cell::Cell<(i32, i32, i32)> = const { std::cell::Cell::new((2026, 11, 26)) };
}

fn demo_now() -> ui::cal_popup::Now {
    ui::cal_popup::Now { date: DEMO_DATE.get(), time: "03:57".into() }
}

/// `y-m-d` or `y-m` as numbers.
fn parse_date(v: &str) -> Option<(i32, i32, i32)> {
    let mut p = v.split('-').map(|n| n.parse::<i32>().ok());
    Some((p.next()??, p.next()??, p.next().flatten().unwrap_or(1)))
}

/// `topbar --render-panel <file.png> tray|lang|tip|quick|wifi|calendar|start|search|settings[:<page>]|toast [classic|floating|dock]
/// [bottom|left|right] [scale=1.25] [t=<ms>] [hover=<n>] [n=<icons>] [real] [at=<n>] [q=<text>] [page=<n>] [pins=<n>]
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
/// are); the apps, the recent files and the account are the machine's.
pub(crate) unsafe fn render_panel(args: &[String]) {
    use ui::panel::{Anchor, Popup};
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let Some(file) = args.first() else { return };
    let what = args.get(1).map_or("tray", String::as_str).to_string();
    let (mut style, mut side, mut scale, mut t, mut hover, mut n_icons, mut real, mut at) = (Style::Classic, Side::Bottom, 1.0f32, 5000u64, None, 6usize, false, 3i32);
    let (mut query, mut page, mut pins) = (String::new(), 0usize, 24usize);
    let (mut t_given, mut lang_arg, mut open_row, mut upd, mut tpl, mut text) = (false, "en", None, "", 0usize, "Alt+Tab lists windows from this monitor only");
    let (mut tiled, mut del, mut hide, mut alignl, mut bar_size, mut opacity, mut accent) = (false, false, false, false, 1u8, 85u8, "blue");
    let mut dir_arg = "";
    for a in args.iter().skip(2) {
        match a.split_once('=') {
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
            Some(("n", v)) => n_icons = v.parse().unwrap_or(6),
            Some(("at", v)) => at = v.parse().unwrap_or(3),
            _ => match a.as_str() {
                "real" => real = true,
                "tiled" => tiled = true,
                "del" => del = true,
                "hide" => hide = true,
                "alignl" => alignl = true,
                "bottom" | "left" | "right" => side = Side::parse(a),
                s => style = Style::parse(s),
            },
        }
    }
    let sandbox = what.starts_with("settings") || what == "toast";
    if sandbox {
        // SAFETY: single-threaded at this point; the process only ever reads these settings from its own scratch folder.
        unsafe { std::env::set_var("ProgramData", std::env::temp_dir().join("topbar-render")) };
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
    let (sw, sh) = ((1440.0 * scale).round() as i32, (900.0 * scale).round() as i32);
    let view = barui::View {
        style,
        side,
        size: 1,
        align_left: false,
        screen: (1440.0, 900.0),
        op: 0.85,
        apps,
        tray: Vec::new(),
        battery: None,
        vol: (72, false),
        net: Net::Wifi(85),
        lang: "RUS".into(),
        time: "03:57".into(),
        date: "11/26/2026".into(),
    };
    let lit = barui::Live {
        now: 100_000,
        hot: if what == "tip" { at } else { NONE },
        tray: what == "tray",
        lang: what == "lang",
        start: what == "start",
        search: what == "search",
        gear: what.starts_with("settings"),
        qs: what == "quick" || what == "wifi",
        cal: what == "calendar",
        ..Default::default()
    };
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
            0xff00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
        })
        .collect();
    let (bar_surf, shadow_surf) = bar.surfaces();
    if let Some((x, y, w, _)) = p.shadow {
        blend(&mut canvas, (sw, sh), shadow_surf.pixels(), w, (x, y), p.shadow_alpha);
    }
    blend(&mut canvas, (sw, sh), bar_surf.pixels(), p.win.2, (p.win.0, p.win.1), p.alpha);

    let mut gfx = ui::gfx::Gfx::new(scale).expect("the renderer");
    let key = match what.split(':').next().unwrap_or("") {
        "lang" => LANG,
        "tip" => at,
        "quick" | "wifi" => WIFI_I,
        "calendar" => CLOCK,
        "start" => 0,
        "search" => SEARCH,
        "settings" | "toast" => GEAR,
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
        } else if what == "calendar" {
            let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
            let today = get("today").and_then(parse_date).unwrap_or((2026, 11, 26));
            DEMO_DATE.set(today);
            let view = get("view").and_then(parse_date).map_or((today.0, today.1), |d| (d.0, d.1));
            Box::new(ui::cal_popup::CalPanel::new(demo_now, side).pick(view, get("sel").and_then(parse_date).unwrap_or(today)))
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
            Box::new(ui::tray_popup::TrayPanel::new(demo_cells, |_, _| {}))
        };
        let snap = canvas.clone();
        let mut grab = |x: i32, y: i32, w: i32, h: i32| {
            Some((0..h).flat_map(|row| snap[((y + row) * sw + x) as usize..((y + row) * sw + x + w) as usize].iter().copied()).collect::<Vec<u32>>())
        };
        let mut popup = Popup::new(&mut gfx, content, anchor, 0, &mut grab);
        popup.set_hover(hover);
        let _ = popup.frame(&mut gfx, 0); // the first frame stamps what opens with the panel (the rising blocks of the first page)
        let shown = popup.frame(&mut gfx, t);
        let (panel, shade) = popup.surfaces();
        if let Some((r, a)) = shown.scrim {
            blend(&mut canvas, (sw, sh), popup.scrim_surface().pixels(), r.2, (r.0, r.1), a);
        }
        blend(&mut canvas, (sw, sh), shade.pixels(), shown.shadow.2, (shown.shadow.0, shown.shadow.1), shown.shadow_alpha);
        blend(&mut canvas, (sw, sh), panel.pixels(), shown.win.2, (shown.win.0, shown.win.1), 255);
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
