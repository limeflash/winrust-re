//! `--bench` and `--selftest` of the file manager: what a window of it costs on this machine, and whether it does what the design says,
//! on real windows put where nobody sees them, on made-up folders that never touch the machine (`demo::Fake`), and for the live parts
//! (listing, renaming, pictures, the queue of what must run outside the window's call) on a temp folder the test makes itself.
//!
//! * `--bench [open|listing|hover|scroll|menu|resize|rest|memory|all]`, the form of the other windows' benchmark. `open` is the generic
//!   one (the time from `main` to the first frame committed); `listing` reads a folder of 10,000 files on disk and shows it; `hover`
//!   sweeps the pointer over the toolbar, the sidebar and the rows, `scroll` runs the wheel over 10,000 rows, `menu` opens the folder's
//!   menu, `resize` drags a corner, `rest` counts the frames in 3 s of nothing, `memory` says what the 10,000 rows hold.
//! * `--selftest [files|window|memory]`: pointer, keys and messages through the window's procedure; nothing is ever opened, deleted or
//!   copied; the temp folder it makes is removed.
use std::{path::PathBuf, sync::Arc, time::Instant};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{GetWindowTextW, IsWindow, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP},
};
use winrust_ui::{
    anim::now_ms,
    frame::{Id, Mods, WIN_CLOSE, WIN_MAX, WIN_MIN},
    gfx,
    probe::{self, Options},
    selftest::memory_now,
    widgets::{list::row_id, overlay::menu_row_id},
};

use super::{
    backend::{Backend, Real},
    demo,
    model::*,
};

pub fn cli(args: &[String]) -> Option<i32> {
    let after = |f: &str| args.iter().position(|a| a == f).map(|i| &args[i + 1..]);
    if after("--bench").is_some_and(|r| matches!(r.first().map(String::as_str), Some("open1"))) {
        return None; // a child of the generic `open`
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
            "listing" | "hover" | "scroll" | "menu" | "resize" | "rest" | "memory" | "all" => Some(bench(what)),
            // `open` is the generic one: it starts the exe again with `open1`
            _ => None,
        };
    }
    after("--selftest").and_then(|rest| match rest.first().map(String::as_str) {
        None | Some("files" | "window" | "memory") => Some(selftest()),
        Some("layout") => Some(layout(rest.get(1).map_or("folder", String::as_str))),
        _ => None,
    })
}

/// `--selftest layout [state]`: where every item of a state is, in DIPs of the window (what the mockup's geometry is compared with).
fn layout(state: &str) -> i32 {
    use winrust_ui::{
        core::Core,
        gfx::{Gfx, Surface},
    };
    let Ok(mut g) = Gfx::new(1.0) else { return 1 };
    let mut core = Core::new(demo::demo(state), (1160.0, 740.0), 1.0, 0);
    let mut surf = Surface::new();
    for t in [1_000_000u64, 1_001_000] {
        if g.paint(&mut surf, 1160, 740, |c| core.draw(c, t)).is_err() {
            return 1;
        }
    }
    for h in core.hits().all() {
        println!("{:6} {:7.1} {:7.1} {:7.1} {:7.1}", h.id, h.r.x, h.r.y, h.r.w, h.r.h);
    }
    0
}

// ---------------------------------------------------------------------------------------------------------------------
// the windows of both

fn opts(class: &'static str, scale: f32) -> Options {
    Options { class, size: (2560.0 / scale, 1440.0 / scale), icon: 0, place_file: None }
}

fn system_scale() -> f32 {
    // SAFETY: a plain query.
    unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f32 / 96.0 }
}

fn make(app: Box<dyn winrust_ui::core::App>, o: &Options) -> Option<isize> {
    let hwnd = probe::make(app, o, gfx::compose_windows(), (-5000, -5000))?;
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

fn size(hwnd: isize) -> (f32, f32) {
    probe::with_core(hwnd, |c| c.size).unwrap_or((1.0, 1.0))
}

fn title(hwnd: isize) -> String {
    let mut b = [0u16; 512];
    // SAFETY: a query of our own window into a buffer of the length given.
    let n = unsafe { GetWindowTextW(hwnd as HWND, b.as_mut_ptr(), 512) };
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}

/// A message of the mouse, through the window's procedure.
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

/// A key with Ctrl, through the core (the keyboard's real state is the user's, not ours to set).
fn ctrl_key(hwnd: isize, vk: u32) {
    probe::with_core(hwnd, |c| c.key(vk, Mods { ctrl: true, shift: false }, false, now_ms()));
    probe::frame(hwnd);
    probe::pump(30);
}

fn plain_key(hwnd: isize, vk: u32) {
    probe::with_core(hwnd, |c| c.key(vk, Mods::default(), false, now_ms()));
    probe::frame(hwnd);
    probe::pump(30);
}

fn typed(hwnd: isize, s: &str) {
    probe::with_core(hwnd, |c| c.text(s, now_ms()));
    probe::frame(hwnd);
}

fn click_at(hwnd: isize, x: f32, y: f32, right: bool) {
    mouse(hwnd, WM_MOUSEMOVE, x, y);
    mouse(hwnd, if right { WM_RBUTTONDOWN } else { WM_LBUTTONDOWN }, x, y);
    mouse(hwnd, if right { WM_RBUTTONUP } else { WM_LBUTTONUP }, x, y);
    probe::pump(30);
}

fn click_id(hwnd: isize, id: Id, right: bool) {
    if let Some((x, y)) = centre(hwnd, id) {
        click_at(hwnd, x, y, right);
    }
}

/// A folder of `n` files on disk, made here (and removed by the caller).
fn temp_folder(name: &str, n: usize) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::create_dir_all(&d);
    for i in 0..n {
        let _ = std::fs::write(d.join(format!("file {i:05}.txt")), b"x");
    }
    d
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
        "{name:17}| {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {ws:4.0} {private:4.0}   ({} frames)",
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

/// Where the rows start in the window, DIPs: right of the sidebar, under the header.
const LIST_AT: (f32, f32) = (216.0, 98.0 + 34.0);

fn bench(what: &str) -> i32 {
    let all = what == "all";
    let composed = gfx::compose_windows();
    println!("{} windows; ms per frame, mean / p95 / max; memory in MB: working set, private", if composed { "composed" } else { "layered" });
    if all {
        // the generic start: from `main` to the first frame committed
        winrust_ui::bench::run(&["open".to_string()], demo::demo);
    }
    if all || what == "listing" {
        listing();
    }
    println!("scenario         | encode  mean    p95     max |    end  mean    p95     max | present mean    p95     max |  total  mean    p95     max |    gpu  mean    p95     max |   ws priv");
    let scale = system_scale();
    let (fm, _fake) = demo::build("many"); // 10,000 rows
    let Some(hwnd) = make(Box::new(fm), &opts("rust_explorer_bench", scale)) else { return 1 };
    let (w, h) = size(hwnd);
    println!("window: {w:.0} x {h:.0} DIPs at scale {scale}, 10,000 rows");
    let (lx, ly) = LIST_AT;
    if all || what == "hover" {
        // the pointer runs over the toolbar's buttons, then down the sidebar, then through the rows
        let bar: Vec<(f32, f32)> = (0..150).map(|i| (10.0 + i as f32 * (w - 20.0) / 150.0, 72.0)).collect();
        step(hwnd, "hover toolbar", bar.len(), |i| {
            probe::with_core(hwnd, |c| c.pointer_move(bar[i].0, bar[i].1, now_ms()));
        });
        let side: Vec<(f32, f32)> = (0..150).map(|i| (110.0, 110.0 + i as f32 * (h - 130.0) / 150.0)).collect();
        step(hwnd, "hover sidebar", side.len(), |i| {
            probe::with_core(hwnd, |c| c.pointer_move(side[i].0, side[i].1, now_ms()));
        });
        step(hwnd, "hover rows", 300, |i| {
            probe::with_core(hwnd, |c| c.pointer_move(lx + 100.0 + (i * 13 % 400) as f32, ly + 10.0 + (i * 7 % 600) as f32, now_ms()));
        });
    }
    if all || what == "scroll" {
        step(hwnd, "scroll 10k rows", 300, |i| {
            probe::with_core(hwnd, |c| {
                c.pointer_move(lx + 300.0, ly + 300.0, now_ms());
                c.wheel(0.0, if (i / 60) % 2 == 0 { -0.35 } else { 0.35 }, Mods::default(), now_ms());
            });
        });
        // a jump to the end and back by the keys: the cursor row is brought into view
        step(hwnd, "End / Home", 40, |i| {
            probe::with_core(hwnd, |c| c.key(if i % 2 == 0 { 0x23 } else { 0x24 }, Mods::default(), false, now_ms()));
        });
        step(hwnd, "type-ahead", 60, |i| {
            probe::with_core(hwnd, |c| c.text(&"file 0".chars().nth(i % 6).map_or_else(String::new, |ch| ch.to_string()), now_ms()));
        });
    }
    if all || what == "menu" {
        step(hwnd, "More menu", 60, |i| {
            if i == 0 {
                click_id(hwnd, ID_MORE, false);
            }
        });
        plain_key(hwnd, 0x1B);
    }
    if all || what == "resize" {
        let (l, t, r, b) = probe::window_rect(hwnd);
        let (w0, h0) = (r - l, b - t);
        step(hwnd, "resize", 300, |i| {
            let k = (i as f32 * 0.21).sin();
            let (w, h) = (w0 - (80.0 + 60.0 * k) as i32, h0 - (60.0 + 40.0 * (i as f32 * 0.13).cos()) as i32);
            // SAFETY: our own window; the call draws the frame inside it, through WM_SIZE.
            unsafe { SetWindowPos(hwnd as HWND, std::ptr::null_mut(), -5000, -5000, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
        });
    }
    if all || what == "rest" {
        probe::pump(800);
        let n = probe::frames(hwnd);
        probe::pump(3000);
        let m = probe::frames(hwnd);
        let (ws, pv) = memory_now();
        println!("rest             | {} frames in 3 s at rest {} | {ws:4.0} {pv:4.0}", m - n, if m == n { "(ok)" } else { "(NOT ZERO)" });
    }
    probe::destroy(hwnd);
    if all || what == "memory" {
        let (ws0, pv0) = memory_now();
        let (fm, _fake) = demo::build("many");
        let (ws1, pv1) = memory_now();
        println!("memory           | {} rows held: {:.1} MB more private bytes ({pv0:.0} -> {pv1:.0}), working set {ws0:.0} -> {ws1:.0}", fm.rows.len(), pv1 - pv0);
    }
    0
}

/// A folder of 10,000 files on disk, read by the real machine and put in a window: what a user waits for when he opens it.
fn listing() {
    let d = temp_folder("bench", 10_000);
    let be: Arc<dyn Backend> = Arc::new(Real);
    let mut cx = winrust_ui::frame::Cx::default();
    let mut best = (f64::MAX, f64::MAX, f64::MAX);
    for _ in 0..5 {
        let mut fm = Fm::new(be.clone(), d.clone());
        let t = Instant::now();
        let rows = be.list(&d, &fm.opts()).map_or(0, |r| r.len());
        let read = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        fm.load(d.clone(), true, false, &mut cx);
        let load = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        fm.sort_by(3, &mut cx);
        let sort = t.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(rows, 10_000);
        best = (best.0.min(read), best.1.min(load), best.2.min(sort));
    }
    println!("listing          | 10,000 files on disk: read and sorted {:.1} ms, put in the window (reads again) {:.1} ms, sorted by size again in memory {:.1} ms (best of 5)", best.0, best.1, best.2);
    // and the first frame with them
    let mut fm = Fm::new(be.clone(), d.clone());
    fm.load(d.clone(), true, false, &mut cx);
    let t = Instant::now();
    let h = probe::make(Box::new(fm), &opts("rust_explorer_bench_listing", system_scale()), gfx::compose_windows(), (-5000, -5000));
    println!("listing          | the window with those rows made and its first frame drawn in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    if let Some(h) = h {
        probe::destroy(h);
    }
    let _ = std::fs::remove_dir_all(&d);
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

fn selftest() -> i32 {
    let mut rep = Report(0);
    let composed = gfx::compose_windows();
    println!("File manager self-test: {} window, off the screen, on made-up folders; nothing is opened, deleted or copied", if composed { "a composed" } else { "a layered" });
    let o = |class| Options { class, size: (1160.0, 740.0), icon: 0, place_file: None };
    let (fm, fake) = demo::build("folder");
    let Some(hwnd) = probe::make(Box::new(fm), &o("rust_explorer_selftest"), composed, (-5000, -5000)) else {
        rep.check(false, "the window can be made");
        return 1;
    };
    probe::show_off_screen(hwnd);
    probe::pump(500);
    rep.check(probe::frames(hwnd) >= 1, "the first frame is drawn");

    // Windows' questions
    let (w, _) = size(hwnd);
    let (mx, my) = centre(hwnd, WIN_MAX).unwrap_or((-1.0, -1.0));
    let (nx, ny) = centre(hwnd, WIN_MIN).unwrap_or((-1.0, -1.0));
    let (cx, cy) = centre(hwnd, WIN_CLOSE).unwrap_or((-1.0, -1.0));
    rep.check(mx > nx && cx > mx && my > 0.0, "the window's three buttons are there, left to right: min, max, close");
    rep.check(probe::nc_hit(hwnd, mx, my) == HT_MAXBUTTON, "over the maximize button: HTMAXBUTTON (Snap Layouts)");
    rep.check(probe::nc_hit(hwnd, nx, ny) == HT_CLIENT && probe::nc_hit(hwnd, cx, cy) == HT_CLIENT, "over minimize and close: the client's");
    rep.check(probe::nc_hit(hwnd, w - 200.0, 30.0) == HT_CAPTION, "the empty part of the caption (next to the tabs): HTCAPTION");
    rep.check(centre(hwnd, ID_TAB).is_some_and(|(x, y)| probe::nc_hit(hwnd, x, y) == HT_CLIENT), "over a tab: the client's, so a tab can be clicked and dragged");
    rep.check(probe::nc_hit(hwnd, 400.0, 2.0) == HT_TOP && probe::nc_hit(hwnd, 400.0, 300.0) == HT_CLIENT, "the top edge is HTTOP, the list the client's");

    // everything of the design is named
    let named = [ID_BACK, ID_FWD, ID_UP, ID_ADDR_PILL, ID_SEARCH, ID_VIEW, ID_VIEW + 1, ID_PV, ID_MORE, ID_NEWTAB, ID_TAB, ID_TAB + 1, ID_PC, ID_DRIVE + 3, ID_EJECT + 2, ID_COLS, ID_COLS + 3, ID_PV_OPEN];
    let missing: Vec<Id> = named.iter().copied().filter(|&i| !has(hwnd, i)).collect();
    rep.check(missing.is_empty(), &format!("the tabs, arrows, address, search, view switch, sidebar, header and preview are named (missing: {missing:?})"));
    rep.check(has(hwnd, row_id(ID_LIST, 0)) && has(hwnd, row_id(ID_LIST, 9)) && !has(hwnd, row_id(ID_LIST, 10)), "the ten rows of the folder are named, and no more");
    rep.check(title(hwnd) == r"C:\", "the window's title is the folder's path");

    // the pointer: a double click opens a folder, Backspace goes up
    if let Some((x, y)) = centre(hwnd, row_id(ID_LIST, 3)) {
        mouse(hwnd, WM_MOUSEMOVE, x, y);
        rep.check(probe::with_core(hwnd, |c| c.hover) == Some(Some(row_id(ID_LIST, 3))), "WM_MOUSEMOVE over a row hovers it");
        click_at(hwnd, x, y, false);
        click_at(hwnd, x, y, false);
        probe::pump(200);
        rep.check(title(hwnd) == r"C:\Projects", &format!("a double click on Projects opens it (title {})", title(hwnd)));
        key(hwnd, 0x08);
        probe::pump(200);
        rep.check(title(hwnd) == r"C:\", "Backspace goes up");
    }
    // tabs
    ctrl_key(hwnd, 0x54);
    rep.check(has(hwnd, ID_TAB + 2), "Ctrl+T: a third tab");
    ctrl_key(hwnd, 0x57);
    rep.check(!has(hwnd, ID_TAB + 2), "Ctrl+W: it is gone again");
    click_id(hwnd, ID_TAB + 1, false);
    probe::pump(200);
    rep.check(title(hwnd).ends_with("Downloads"), "a click on the second tab shows its folder");
    click_id(hwnd, ID_TAB, false);
    probe::pump(200);
    // the address box
    ctrl_key(hwnd, 0x4C);
    rep.check(probe::with_core(hwnd, |c| c.hits().focus) == Some(Some(ID_ADDR)), "Ctrl+L: the address box has the focus");
    typed(hwnd, "D");
    typed(hwnd, ":");
    typed(hwnd, "\\");
    plain_key(hwnd, 0x0D); // Enter
    probe::pump(100);
    rep.check(title(hwnd).starts_with("D:") || title(hwnd) == r"C:\", &format!("Enter in the address box opens what was typed or says it cannot (title {})", title(hwnd)));
    // the menus
    click_id(hwnd, row_id(ID_LIST, 1), true);
    rep.check(probe::with_core(hwnd, |c| c.ov.modal()) == Some(true) && has(hwnd, menu_row_id(0, 0)), "the right button over a row opens the menu (there is no bar here)");
    key(hwnd, 0x1B);
    rep.check(probe::with_core(hwnd, |c| c.ov.modal()) == Some(false), "Esc closes it");
    click_id(hwnd, ID_MORE, false);
    rep.check(probe::with_core(hwnd, |c| c.ov.modal()) == Some(true), "More opens the folder's menu");
    key(hwnd, 0x1B);
    // search
    ctrl_key(hwnd, 0x46);
    typed(hwnd, "read");
    plain_key(hwnd, 0x0D);
    rep.check(title(hwnd).starts_with("search"), &format!("Ctrl+F, a word, Enter: the results (title {})", title(hwnd)));
    key(hwnd, 0x1B);
    probe::pump(100);
    // nothing was touched
    rep.check(!fake.take_calls().iter().any(|c| c.starts_with("Delete") || c.starts_with("Paste") || c.starts_with("DropOn")), "no file was deleted, pasted or dropped");
    // rest
    probe::pump(900);
    let n = probe::frames(hwnd);
    probe::pump(2500);
    let m = probe::frames(hwnd);
    rep.check(m == n, &format!("at rest: {} frames in 2.5 s", m - n));
    probe::destroy(hwnd);
    probe::pump(100);
    // SAFETY: a query of a handle that may be gone.
    rep.check(unsafe { IsWindow(hwnd as HWND) } == 0, "the window is gone");

    // the live parts, on a temp folder of its own
    live_parts(&mut rep, &o("rust_explorer_selftest_live"));
    pick_parts(&mut rep, &o("rust_explorer_selftest_pick"));

    // memory: the folder's menu and an item's, thirty times
    let (fm, _) = demo::build("folder");
    let Some(h3) = make(Box::new(fm), &o("rust_explorer_selftest_memory")) else { return rep.0 };
    let mut seen: Vec<(f64, f64)> = Vec::new();
    for cycle in 1..=30 {
        click_id(h3, ID_MORE, false);
        probe::pump(150);
        key(h3, 0x1B);
        click_id(h3, row_id(ID_LIST, 2), true);
        probe::pump(150);
        key(h3, 0x1B);
        click_id(h3, ID_VIEW + 1, false);
        probe::pump(60);
        click_id(h3, ID_VIEW, false);
        probe::pump(60);
        seen.push(memory_now());
        if [1, 3, 6, 10, 15, 20, 25, 30].contains(&cycle) {
            let (ws, pv) = *seen.last().unwrap_or(&(0.0, 0.0));
            println!("  cycle {cycle:2}: {ws:.1} / {pv:.1} MB");
        }
    }
    let low = |r: &[(f64, f64)]| r.iter().fold((f64::MAX, f64::MAX), |m, v| (m.0.min(v.0), m.1.min(v.1)));
    let (first, last) = (low(&seen[2..8]), low(&seen[seen.len() - 6..]));
    rep.check(last.1 - first.1 <= 2.0, &format!("30 cycles of menus and the view switch: private {:.1} -> {:.1} MB (growth {:+.1})", first.1, last.1, last.1 - first.1));
    probe::destroy(h3);
    println!("{} checks wrong", rep.0);
    rep.0
}

/// The real machine on a folder this test made: what is listed, what a new folder and a rename do on disk, the pictures the worker
/// brings, and the queue of what must run after the window's own call.
fn live_parts(rep: &mut Report, o: &Options) {
    let d = temp_folder("selftest", 3);
    let _ = std::fs::create_dir_all(d.join("sub"));
    let png: [u8; 68] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
        0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xFF, 0xFF, 0x3F, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    ];
    let _ = std::fs::write(d.join("pic.png"), &png[..]);
    let be: Arc<dyn Backend> = Arc::new(Real);
    let mut fm = Fm::new(be.clone(), d.clone());
    fm.flags_from_backend();
    let mut cx = winrust_ui::frame::Cx::default();
    fm.load(d.clone(), true, false, &mut cx);
    rep.check(fm.rows.len() == 5 && fm.rows[0].name == "sub" && fm.rows[0].dir, "the real machine lists the temp folder: its folder first, then its four files");
    let Some(hwnd) = probe::make(Box::new(fm), o, gfx::compose_windows(), (-5000, -5000)) else {
        rep.check(false, "a window on the real machine can be made");
        let _ = std::fs::remove_dir_all(&d);
        return;
    };
    probe::show_off_screen(hwnd);
    probe::pump(800);
    rep.check(title(hwnd) == d.to_string_lossy(), "its title is the temp folder");
    // the picture worker answers (the thumbnail of the PNG)
    probe::pump(1500);
    rep.check(super::live::pictures() >= 1, &format!("the worker brought a picture for the PNG ({} so far)", super::live::pictures()));
    // what must run after the call: queued, and run once the host has handled the message
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let f2 = flag.clone();
    super::live::defer(move || f2.store(true, std::sync::atomic::Ordering::Relaxed));
    probe::pump(200);
    rep.check(flag.load(std::sync::atomic::Ordering::Relaxed), "a deferred job runs after the window's call, with nothing of the window borrowed");
    // a new folder and its name
    ctrl_key(hwnd, 0x4E);
    probe::pump(200);
    rep.check(d.join("New folder").is_dir() || d.join("Новая папка").is_dir(), "Ctrl+N makes the folder on disk");
    typed(hwnd, "Docs");
    plain_key(hwnd, 0x0D);
    probe::pump(200);
    rep.check(d.join("Docs").is_dir() && !d.join("New folder").exists() && !d.join("Новая папка").exists(), "typing a name and Enter renames it on disk");
    // another thread creates, renames and deletes files: the listing follows within about a second (the watcher)
    let count = |h: isize| (0..64).filter(|&i| has(h, row_id(ID_LIST, i))).count();
    let follows = |h: isize, want: usize, limit: u128| {
        let t0 = Instant::now();
        while t0.elapsed().as_millis() < limit {
            probe::pump(50);
            if count(h) == want {
                return Some(t0.elapsed().as_millis());
            }
        }
        None
    };
    let n0 = count(hwnd);
    let dd = d.clone();
    let worker = std::thread::spawn(move || {
        let _ = std::fs::write(dd.join("w1.txt"), b"x");
        std::thread::sleep(std::time::Duration::from_millis(1900));
        let _ = std::fs::rename(dd.join("w1.txt"), dd.join("w2.txt"));
        let _ = std::fs::write(dd.join("w3.txt"), b"x");
        std::thread::sleep(std::time::Duration::from_millis(1900));
        let _ = std::fs::remove_file(dd.join("w2.txt"));
        let _ = std::fs::remove_file(dd.join("w3.txt"));
    });
    let a = follows(hwnd, n0 + 1, 1800);
    rep.check(a.is_some(), &format!("a file made by another thread shows in the listing ({n0} -> {} rows, after {a:?} ms)", count(hwnd)));
    probe::pump(1700);
    let b = follows(hwnd, n0 + 2, 1800);
    rep.check(b.is_some(), &format!("a rename and a new file: two more rows ({} rows, after {b:?} ms)", count(hwnd)));
    let c = follows(hwnd, n0, 2500); // the worker deletes about 3.8 s after it started
    rep.check(c.is_some(), &format!("both deleted: back to {n0} rows ({} rows, after {c:?} ms)", count(hwnd)));
    let _ = worker.join();
    probe::destroy(hwnd);
    probe::pump(100);
    // SAFETY: a query of a handle that may be gone.
    rep.check(unsafe { IsWindow(hwnd as HWND) } == 0, "the live window is gone too");
    crate::shellwin::revoke();
    let _ = std::fs::remove_dir_all(&d);
}

/// Where row `i` stands in the window and whether it is inside the list's viewport (None: it is not drawn at all).
fn row_in_view(hwnd: isize, i: usize) -> Option<bool> {
    probe::with_core(hwnd, |c| {
        let r = c.hits().get(row_id(ID_LIST, i)).map(|h| h.r)?;
        let v = c.hits().get(ID_LIST).map(|h| h.r)?;
        Some(r.y >= v.y - 0.5 && r.y + r.h <= v.y + v.h + 0.5)
    })
    .flatten()
}

/// "Show in folder" from another app: the file is picked and brought into view, in a window that is made on it (`/select,`) and in one
/// that is open on another file of the same folder (the shell's `SelectItem`, which arrives as a message).
fn pick_parts(rep: &mut Report, o: &Options) {
    let d = temp_folder("pick", 400);
    let be: Arc<dyn Backend> = Arc::new(Real);
    let mut fm = Fm::new(be, d.clone());
    fm.flags_from_backend();
    fm.places = fm.be.places();
    let mut cx = winrust_ui::frame::Cx::default();
    fm.load(d.clone(), true, false, &mut cx);
    fm.reveal_names(&["file 00350.txt".to_string()]);
    let Some(hwnd) = probe::make(Box::new(fm), o, gfx::compose_windows(), (-5000, -5000)) else {
        rep.check(false, "a window on a folder of 400 files can be made");
        let _ = std::fs::remove_dir_all(&d);
        return;
    };
    probe::show_off_screen(hwnd);
    probe::pump(1200);
    let at = |h: isize, i: usize| row_in_view(h, i);
    rep.check(at(hwnd, 350) == Some(true), &format!("a window made with /select, shows the picked file (row 350 of 400) inside the list: {:?}", at(hwnd, 350)));
    let picked = |h: isize, i: usize| probe::with_core(h, |c| c.hits().get(row_id(ID_LIST, i)).is_some()).unwrap_or(false);
    rep.check(picked(hwnd, 350), "... and it is drawn");
    // the shell asks an open window to show another file of the folder
    let want = d.join("file 00020.txt");
    probe::send(hwnd, crate::shellwin::WM_SHELL_SELECT, 0, Box::into_raw(Box::new(want)) as isize);
    probe::pump(1200);
    rep.check(at(hwnd, 20) == Some(true), &format!("SelectItem on an open window: file 20 is in view ({:?})", at(hwnd, 20)));
    let want = d.join("file 00399.txt");
    probe::send(hwnd, crate::shellwin::WM_SHELL_SELECT, 0, Box::into_raw(Box::new(want)) as isize);
    probe::pump(1200);
    rep.check(at(hwnd, 399) == Some(true), &format!("... and file 399 at the other end ({:?})", at(hwnd, 399)));
    probe::destroy(hwnd);
    probe::pump(100);
    let _ = std::fs::remove_dir_all(&d);
}
