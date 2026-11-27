//! `--bench` and `--selftest` of the Task Manager: what a window of it costs on this machine, and whether it does what the design and
//! the old program did, on real windows put where nobody sees them, on made-up processes that never touch the machine (`demo::Fake`).
//!
//! * `--bench [hover|scroll|tick|rest|memory|all]`, the form of the other windows' benchmark. `open` is the generic one (the time from
//!   `main` to the first frame committed, the median of five runs of the exe). `hover` sweeps the pointer over the rows, the head and
//!   the sidebar, `scroll` goes up and down a table of 500 rows, `tick` is the frame the 1 Hz refresh makes (new numbers, the table
//!   rebuilt, the charts moved), `rest` counts the frames in 3 s (one for each tick, none between them), `memory` what 5000 processes take.
//! * `--selftest tm | window | tray | memory`: pointer, click, keys and messages through the window's procedure: what Windows asks of
//!   the caption, the pages, the end of a process (nothing is ended until the dialog's red button is pressed, and only in the fake), the
//!   tray (the helper window made without an icon and without the hotkey: nothing of the user's desktop is touched), "close to tray",
//!   and no growth over 30 openings of a menu and a dialog.
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP};
use winrust_ui::{
    anim::now_ms,
    frame::{Id, Mods, OVERLAY, WIN_CLOSE, WIN_MAX},
    gfx,
    probe::{self, Options},
    selftest::memory_now,
};

use crate::{
    app::*,
    backend::{MSG_HOTKEY, MSG_TRAY_CMD, MSG_TRAY_SHOW},
    demo,
    tray::{CALLBACK, Tray},
};

pub fn cli(args: &[String]) -> Option<i32> {
    let after = |f: &str| args.iter().position(|a| a == f).map(|i| &args[i + 1..]);
    if after("--bench").is_some_and(|r| matches!(r.first().map(String::as_str), Some("open1" | "open"))) {
        return None; // the generic one: it starts the exe again with `open1`
    }
    if let Some(rest) = after("--render-all") {
        return Some(render_all(rest.first().map_or("renders", String::as_str)));
    }
    if after("--bench").is_some() || after("--selftest").is_some() {
        // the devices want COM, as the renderers' command line makes it; and dark is what the benchmarks measure
        // SAFETY: COM for this thread, once.
        let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
        winrust_common::style::set_light(false);
    }
    if let Some(rest) = after("--bench") {
        let what = rest.first().map_or("all", String::as_str);
        return match what {
            "hover" | "scroll" | "tick" | "rest" | "memory" | "all" => Some(bench(what)),
            _ => None,
        };
    }
    after("--selftest").and_then(|rest| match rest.first().map(String::as_str) {
        None | Some("tm" | "window" | "tray" | "memory") => Some(selftest()),
        _ => None,
    })
}

/// `--render-all <dir>`: every state of the design, dark and light, at scale 1 and 1.5, on the design's stage of 1360 x 860 (the
/// generic one draws the stage the other apps have). The files are `<state>-<dark|light>[-x1.5].png`.
fn render_all(dir: &str) -> i32 {
    // SAFETY: COM for the renderer's WIC, once, on this thread.
    let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
    let _ = std::fs::create_dir_all(dir);
    let mut failed = 0;
    for state in demo::STATES {
        for (theme, light) in [("dark", false), ("light", true)] {
            for scale in [1.0, 1.5] {
                winrust_common::style::set_light(light);
                let name = if scale == 1.0 { format!("{dir}/{state}-{theme}.png") } else { format!("{dir}/{state}-{theme}-x{scale}.png") };
                let o = winrust_ui::render::Opts { w: 1360.0, h: 860.0, scale, ..winrust_ui::render::Opts::default() };
                failed += i32::from(!winrust_ui::render::render_with(demo::demo(state), &o, &name));
            }
        }
    }
    failed
}

// ---------------------------------------------------------------------------------------------------------------------
// the windows of both

fn system_scale() -> f32 {
    // SAFETY: a plain query.
    unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f32 / 96.0 }
}

fn make(tm: Tm, class: &'static str, scale: f32) -> Option<isize> {
    let o = Options { class, size: (2560.0 / scale, 1440.0 / scale), icon: 0, place_file: None };
    let hwnd = probe::make(Box::new(tm), &o, gfx::compose_windows(), (-5000, -5000))?;
    probe::show_off_screen(hwnd);
    probe::pump(400);
    Some(hwnd)
}

fn centre(hwnd: isize, id: Id) -> Option<(f32, f32)> {
    probe::with_core(hwnd, |c| c.hits().get(id).map(|h| (h.r.cx(), h.r.cy()))).flatten()
}

fn has(hwnd: isize, id: Id) -> bool {
    probe::with_core(hwnd, |c| c.hits().get(id).is_some()).unwrap_or(false)
}

/// Item `id` is there within a third of a second (an overlay names its items once its first frame is drawn).
fn soon(hwnd: isize, id: Id) -> bool {
    for _ in 0..8 {
        if has(hwnd, id) {
            return true;
        }
        probe::pump(40);
    }
    false
}

fn size(hwnd: isize) -> (f32, f32) {
    probe::with_core(hwnd, |c| c.size).unwrap_or((1.0, 1.0))
}

fn mouse(hwnd: isize, msg: u32, x: f32, y: f32) {
    let wp = match msg {
        WM_LBUTTONDOWN => 1,
        WM_RBUTTONDOWN => 2,
        _ => 0,
    };
    probe::send(hwnd, msg, wp, probe::mouse_lparam(hwnd, x, y));
}

fn key(hwnd: isize, vk: u32) {
    probe::send(hwnd, WM_KEYDOWN, vk as usize, 0);
    probe::pump(30);
}

fn click_at(hwnd: isize, x: f32, y: f32, right: bool) {
    mouse(hwnd, WM_MOUSEMOVE, x, y);
    mouse(hwnd, if right { WM_RBUTTONDOWN } else { WM_LBUTTONDOWN }, x, y);
    mouse(hwnd, if right { WM_RBUTTONUP } else { WM_LBUTTONUP }, x, y);
    probe::pump(30);
}

/// A click on item `id` (waiting a moment for the frame that names it, if it is not there yet).
fn click_id(hwnd: isize, id: Id, right: bool) {
    for _ in 0..8 {
        if let Some((x, y)) = centre(hwnd, id) {
            return click_at(hwnd, x, y, right);
        }
        probe::pump(40);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// --bench

fn stats(v: &mut [f64]) -> (f64, f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    (v.iter().sum::<f64>() / v.len() as f64, v[(v.len() * 95).div_ceil(100) - 1], v[v.len() - 1])
}

fn report(name: &str, spent: &[(f64, f64, f64)], gpu: &mut [f64]) {
    let col = |f: fn(&(f64, f64, f64)) -> f64| spent.iter().map(f).collect::<Vec<f64>>();
    let (mut a, mut b, mut c) = (col(|s| s.0), col(|s| s.1), col(|s| s.2));
    let mut t: Vec<f64> = spent.iter().map(|s| s.0 + s.1 + s.2).collect();
    let (a, b, c, t, g) = (stats(&mut a), stats(&mut b), stats(&mut c), stats(&mut t), stats(gpu));
    let (ws, private) = memory_now();
    println!(
        "{name:19}| {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {ws:4.0} {private:4.0}   ({} frames)",
        a.0, a.1, a.2, b.0, b.1, b.2, c.0, c.1, c.2, t.0, t.1, t.2, g.0, g.1, g.2, spent.len()
    );
}

/// `n` steps, each one `f` and the frame it makes, timed as the other windows' benchmark times them.
fn step(hwnd: isize, label: &str, n: usize, mut f: impl FnMut(usize)) {
    probe::pump(300); // what the last scenario left moving settles
    probe::timing(hwnd, true);
    let mut gpu = Vec::with_capacity(n);
    for i in 0..n {
        f(i);
        probe::frame(hwnd);
        gpu.push(gfx::gpu_wait().as_secs_f64() * 1000.0);
    }
    let spent = probe::timing(hwnd, false);
    report(label, &spent, &mut gpu);
}

fn refreshes(fake: &demo::Fake) -> usize {
    fake.log_of().iter().filter(|l| *l == "read: refresh").count()
}

fn bench(what: &str) -> i32 {
    let all = what == "all";
    let composed = gfx::compose_windows();
    println!("{} windows; ms per frame, mean / p95 / max; memory in MB: working set, private", if composed { "composed" } else { "layered" });
    println!("scenario           | encode  mean    p95     max |    end  mean    p95     max | present mean    p95     max |  total  mean    p95     max |    gpu  mean    p95     max |   ws priv");
    let scale = system_scale();
    if all || matches!(what, "hover" | "tick" | "rest") {
        let (tm, fake) = demo::build("processes");
        let Some(hwnd) = make(tm, "rtaskmgr_bench", scale) else { return 1 };
        let (_, h) = size(hwnd);
        if all || what == "hover" {
            // the pointer sweeps the head's buttons, the sidebar and then runs down the rows: every step moves a hover
            let pts: Vec<(f32, f32)> = (0..300)
                .map(|i| match i / 100 {
                    0 => (540.0 + i as f32 * 6.0, 32.0),
                    1 => (120.0, 70.0 + (i - 100) as f32 * 5.0),
                    _ => (600.0 + (i % 40) as f32 * 20.0, 190.0 + (i - 200) as f32 * (h - 230.0) / 100.0),
                })
                .collect();
            step(hwnd, "hover", pts.len(), |i| {
                probe::with_core(hwnd, |c| c.pointer_move(pts[i].0, pts[i].1, now_ms()));
            });
        }
        if all || what == "tick" {
            // the 1 Hz refresh: new numbers (the fake's, which drift), the table rebuilt, the charts moved, one full frame
            step(hwnd, "tick (processes)", 120, |_| {
                probe::with_core(hwnd, |c| c.wake(now_ms()));
            });
            for (page, label) in [(3usize, "tick (startup)"), (1, "tick (performance)")] {
                click_id(hwnd, ID_NAV + page as Id, false);
                probe::pump(300);
                step(hwnd, label, 120, |_| {
                    probe::with_core(hwnd, |c| c.wake(now_ms()));
                });
            }
            click_id(hwnd, ID_NAV, false);
            probe::pump(300);
        }
        if all || what == "rest" {
            probe::pump(800);
            let (n, t0) = (probe::frames(hwnd), refreshes(&fake));
            probe::pump(3000);
            let (m, t1) = (probe::frames(hwnd), refreshes(&fake));
            let (ws, pv) = memory_now();
            let frames = m - n;
            println!("rest               | {frames} frames in 3 s for {} ticks (a frame a tick, none between) {} | {ws:4.0} {pv:4.0}", t1 - t0, if frames <= (t1 - t0) as u32 + 1 { "(ok)" } else { "(TOO MANY)" });
        }
        probe::destroy(hwnd);
    }
    if all || what == "scroll" {
        let (tm, _) = demo::big(500);
        let Some(hwnd) = make(tm, "rtaskmgr_bench_big", scale) else { return 1 };
        let (w, h) = size(hwnd);
        let (px, py) = (crate::draw::SIDE_W + (w - crate::draw::SIDE_W) / 2.0, h / 2.0);
        step(hwnd, "scroll 500 rows", 300, |i| {
            probe::with_core(hwnd, |c| {
                c.pointer_move(px, py, now_ms());
                c.wheel(0.0, if (i / 60) % 2 == 0 { -0.35 } else { 0.35 }, Mods::default(), now_ms());
            });
        });
        step(hwnd, "hover 500 rows", 150, |i| {
            probe::with_core(hwnd, |c| c.pointer_move(px, 150.0 + (i * 7 % 600) as f32, now_ms()));
        });
        probe::destroy(hwnd);
    }
    if all || what == "memory" {
        let (ws0, pv0) = memory_now();
        let (tm, _) = demo::big(5000);
        let (ws1, pv1) = memory_now();
        println!("memory             | 5000 made-up processes: {:.0} MB more private bytes ({pv0:.0} -> {pv1:.0}), working set {ws0:.0} -> {ws1:.0}", pv1 - pv0);
        drop(tm);
    }
    0
}

// ---------------------------------------------------------------------------------------------------------------------
// --selftest

struct Report(i32);

impl Report {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{} {what}", if ok { "ok   " } else { "WRONG" });
        self.0 += i32::from(!ok);
    }
}

const HT_CLIENT: isize = 1;
const HT_CAPTION: isize = 2;
const HT_MAXBUTTON: isize = 9;
const HT_TOP: isize = 12;
/// The buttons of a dialog of two: `Cancel`, then the primary one (the red one of a dangerous dialog).
const DLG_CANCEL: Id = OVERLAY - 10;
const DLG_PRIMARY: Id = OVERLAY - 10 - 1;

fn selftest() -> i32 {
    let mut rep = Report(0);
    let composed = gfx::compose_windows();
    println!("Task Manager self-test: {} window, off the screen, on made-up processes; nothing is ended or changed, no icon is put in the tray, no key is registered", if composed { "a composed" } else { "a layered" });
    let (tm, fake) = demo::build("tray-question");
    let o = Options { class: "rtaskmgr_selftest", size: (1280.0, 780.0), icon: 0, place_file: None };
    let Some(hwnd) = probe::make(Box::new(tm), &o, composed, (-5000, -5000)) else {
        rep.check(false, "the window can be made");
        return 1;
    };
    probe::show_off_screen(hwnd);
    probe::pump(500);
    let (w, _) = size(hwnd);

    // what Windows asks of the caption: the empty part drags, the maximize button is one, the top edge sizes
    rep.check(probe::nc_hit(hwnd, w - 420.0, 10.0) == HT_CAPTION, "the empty head drags the window (HTCAPTION)");
    let max = centre(hwnd, WIN_MAX).unwrap_or_default();
    rep.check(probe::nc_hit(hwnd, max.0, max.1) == HT_MAXBUTTON, "the maximize button is HTMAXBUTTON (Snap Layouts)");
    rep.check(probe::nc_hit(hwnd, 600.0, 2.0) == HT_TOP, "the top edge sizes the window");
    rep.check(probe::nc_hit(hwnd, 600.0, 400.0) == HT_CLIENT, "the body is the client");

    // 1 Hz: one frame a tick and none between (the demo's refresh is the fake's)
    probe::pump(2500);
    let ticks = refreshes(&fake);
    rep.check((2..=4).contains(&ticks), &format!("the clock ticks about once a second ({ticks} in 2.5 s)"));
    let n = probe::frames(hwnd);
    probe::pump(3000);
    let m = probe::frames(hwnd);
    let t2 = refreshes(&fake);
    rep.check(m - n <= (t2 - ticks) as u32 + 1, &format!("{} frames in 3 s for {} ticks: nothing is drawn between them", m - n, t2 - ticks));

    // the pages: a click on a sidebar item changes the body
    click_id(hwnd, ID_NAV + 1, false);
    probe::pump(100);
    rep.check(has(hwnd, ID_PERF), "Performance: the list of devices is there");
    click_id(hwnd, ID_NAV + 5, false);
    probe::pump(100);
    rep.check(has(hwnd, ID_COLS) && has(hwnd, ID_TABLE), "Details: the header and the list");
    click_id(hwnd, ID_NAV, false);
    probe::pump(100);
    rep.check(has(hwnd, ID_PHEAD) && has(hwnd, ID_PROC), "Processes: the header and the table");

    // ending processes: a group asks first, the dialog's Enter is Cancel, only the red button ends them, and only in the fake
    let before = fake.changes().len();
    click_id(hwnd, ID_PROW, false); // the first row: a group
    key(hwnd, 0x2E); // Delete
    rep.check(soon(hwnd, DLG_PRIMARY), "Delete on a group asks first (a dialog with a red button)");
    rep.check(fake.changes().len() == before, "nothing is ended while the dialog is open");
    key(hwnd, 0x0D); // Enter: Cancel in a dangerous dialog
    probe::pump(150);
    rep.check(!has(hwnd, DLG_PRIMARY) && fake.changes().len() == before, "Enter in the dialog is Cancel: nothing is ended");
    key(hwnd, 0x2E);
    key(hwnd, 0x1B); // Esc
    probe::pump(150);
    rep.check(fake.changes().len() == before && !has(hwnd, DLG_PRIMARY), "Esc in the dialog ends nothing");
    key(hwnd, 0x2E);
    soon(hwnd, DLG_PRIMARY);
    click_id(hwnd, DLG_PRIMARY, false);
    probe::pump(60);
    let kills = fake.changes().into_iter().skip(before).filter(|l| l.starts_with("kill ")).count();
    rep.check(kills > 1, &format!("the red button ends the group, in the fake only ({kills} kills logged)"));

    // the menu of a row, and Escape closes it
    click_id(hwnd, ID_PROW, true);
    let menu0 = winrust_ui::widgets::overlay::menu_row_id(0, 0);
    rep.check(soon(hwnd, menu0), "a right click on a row opens its menu");
    key(hwnd, 0x1B);
    probe::pump(150);
    rep.check(!has(hwnd, menu0), "Escape closes the menu");

    // the tray: its helper window made without the icon and the hotkey, and the messages it posts to the window
    let helper = Tray::create(hwnd, false);
    rep.check(helper.is_some(), "the tray's helper window can be made (without an icon)");
    if let Some(t) = &helper {
        let shown = fake.shown.load(std::sync::atomic::Ordering::Relaxed);
        // a left click on the icon (the shell would send this); the helper posts "show" to the main window
        probe::send(t.hwnd(), CALLBACK, 0, 0x0202);
        probe::pump(100);
        rep.check(fake.shown.load(std::sync::atomic::Ordering::Relaxed) == shown + 1, "a click on the icon shows the window");
        probe::send(hwnd, MSG_HOTKEY, 0, 0);
        probe::pump(100);
        rep.check(fake.shown.load(std::sync::atomic::Ordering::Relaxed) == shown + 2, "the hotkey shows the window");
        probe::send(hwnd, MSG_TRAY_SHOW, 0, 0);
        probe::pump(50);
    }
    drop(helper);
    rep.check(!fake.tray.load(std::sync::atomic::Ordering::Relaxed), "no tray icon was made by the test");

    // close to the tray: the first close of a program that was never asked asks; Cancel leaves the window as it was
    click_id(hwnd, WIN_CLOSE, false);
    probe::pump(50);
    rep.check(soon(hwnd, DLG_PRIMARY - 1), "the first close asks: keep it in the tray, or quit?");
    click_id(hwnd, DLG_CANCEL, false);
    probe::pump(50);
    rep.check(!fake.log_of().iter().any(|l| l == "window: hide"), "Cancel leaves the window as it was");
    click_id(hwnd, WIN_CLOSE, false);
    probe::pump(50);
    click_id(hwnd, DLG_PRIMARY - 1, false); // "Keep in tray"
    probe::pump(100);
    rep.check(fake.log_of().iter().any(|l| l == "window: hide"), "\"Keep in tray\" hides the window and the program runs on");

    // the tray menu's Exit asks the window to close for good
    probe::send(hwnd, MSG_TRAY_CMD, 6, 0);
    probe::pump(200);
    rep.check(fake.log_of().iter().any(|l| l == "window: tray remove"), "Exit from the tray menu closes the window and takes the icon away");
    rep.check(probe::client_size(hwnd).0 == 0, "the window is gone");

    // 30 openings of a menu and a dialog: the process must not grow
    let (tm, _fake2) = demo::build("processes");
    if let Some(hwnd) = make(tm, "rtaskmgr_selftest_mem", system_scale()) {
        probe::pump(300);
        let cycle = |n: usize| {
            for _ in 0..n {
                click_id(hwnd, ID_PROW, true);
                key(hwnd, 0x1B);
                key(hwnd, 0x2E);
                key(hwnd, 0x1B);
            }
        };
        cycle(5);
        let (ws0, pv0) = memory_now();
        cycle(30);
        probe::pump(300);
        let (ws1, pv1) = memory_now();
        rep.check(pv1 - pv0 <= 2.0, &format!("30 openings of a menu and a dialog: private bytes {pv0:.1} -> {pv1:.1} MB (working set {ws0:.1} -> {ws1:.1})"));
        probe::destroy(hwnd);
    }
    println!("{}", if rep.0 == 0 { "all ok" } else { "THERE WERE FAILURES" });
    rep.0
}
