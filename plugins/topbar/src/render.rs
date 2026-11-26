//! `topbar --render <file.png> [classic|floating|dock] [bottom|left|right] [0|1|2] [hover=<element>] [press=<element>]
//! [open=gear|start|qs|clock|tray|lang] [scale=1.25] [op=85] [battery] [tray=2] [apps=8]`
//!
//! Draws one frame of the bar through the same renderer as the real one, with the mockup's fake buttons (8 apps, the focused
//! one lit), onto a plain backdrop, and writes the strip around it to a PNG. It never touches the system taskbar and creates
//! no window, so it is safe to run next to the user's desktop. `<element>` is a name (gear, search, taskview, start, chevron,
//! lang, wifi, volume, battery, restart, power, clock) or the number of an app button (1..).
use super::*;
use ui::bar::Net;
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
