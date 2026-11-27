//! `--bench` and `--selftest` of WinRust Disk: what a window of it costs on this machine, and whether it does what the design says,
//! on real windows put where nobody sees them, on made-up drives that never touch the machine (`demo::Fake`).
//!
//! * `--bench [open|hover|zoom|list|rest|memory|all]`, the form of the other windows' benchmark. `open` is the generic one (the time
//!   from `main` to the first frame committed, the window as a user gets it: scanning, and then with a made-up drive in it); `hover`
//!   sweeps the pointer over the map (61 tiles as the design has them, and 300), `zoom` goes into a folder and out of it, `list`
//!   scrolls the 300 items of a folder, `rest` counts the frames in 3 s of nothing, `memory` makes a drive of a million records and
//!   says what it takes.
//! * `--selftest disk`: pointer, click, keys and messages through the window's procedure: the three window buttons, the tiles, the
//!   menu, the dialog (nothing is deleted unless it is confirmed), the toast and its Undo, the card of "Needs administrator" (and
//!   that nothing asks Windows for them), the drives, 0 frames at rest, and no growth over 30 openings of a menu and a dialog.
use std::{sync::Arc, time::Instant};

use windows_sys::Win32::UI::WindowsAndMessaging::{WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP};
use winrust_ui::{
    anim::now_ms,
    frame::{Id, Mods, WIN_CLOSE, WIN_MAX, WIN_MIN},
    gfx,
    probe::{self, Options},
    selftest::memory_now,
};

use crate::{
    app::*,
    delete::How,
    demo::{self, scanned, Fake},
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
            "hover" | "zoom" | "list" | "rest" | "memory" | "all" => Some(bench(what)),
            // `open` and the others are the generic ones: they start the exe again with `open1`
            _ => None,
        };
    }
    after("--selftest").and_then(|rest| match rest.first().map(String::as_str) {
        // the generic checks of the host click the items of the demo window by number; these are Disk's own
        None | Some("disk" | "window" | "memory") => Some(selftest()),
        _ => None,
    })
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

fn make(app: Disk, class: &'static str, scale: f32) -> Option<isize> {
    let hwnd = probe::make(Box::new(app), &opts(class, scale), gfx::compose_windows(), (-5000, -5000))?;
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

/// Points over the map that jump from tile to tile.
fn sweep(map: (f32, f32, f32, f32)) -> Vec<(f32, f32)> {
    (0..300).map(|i| (map.0 + 12.0 + (i * 37 % 1000) as f32 * (map.2 - 24.0) / 1000.0, map.1 + 12.0 + (i * 53 % 1000) as f32 * (map.3 - 24.0) / 1000.0)).collect()
}

fn bench(what: &str) -> i32 {
    let all = what == "all";
    let composed = gfx::compose_windows();
    println!("{} windows; ms per frame, mean / p95 / max; memory in MB: working set, private", if composed { "composed" } else { "layered" });
    if all {
        // the generic start: from `main` to the first frame committed, as a user gets it (a scan under way), and with a drive in it
        // SAFETY: set before the child processes the benchmark starts are made, which read it; nothing else runs yet.
        unsafe { std::env::set_var("WIZTREE_DEMO", "scan") };
        println!("start while the scan runs:");
        winrust_ui::bench::run(&["open".to_string()], demo::demo);
        // SAFETY: as above.
        unsafe { std::env::set_var("WIZTREE_DEMO", "drive-map") };
        println!("start with a scanned drive:");
        winrust_ui::bench::run(&["open".to_string()], demo::demo);
    }
    println!("scenario         | encode  mean    p95     max |    end  mean    p95     max | present mean    p95     max |  total  mean    p95     max |    gpu  mean    p95     max |   ws priv");
    let scale = system_scale();
    if all || what == "hover" || what == "zoom" || what == "rest" {
        let Some(hwnd) = make(demo::state_of("drive-map"), "wiztree_bench", scale) else { return 1 };
        let (w, h) = size(hwnd);
        let map = (16.0, 104.0, w - 340.0, h - 148.0);
        if all || what == "hover" {
            // the pointer jumps from tile to tile over the 61 tiles of a level, then runs over the legend and the panel
            let pts = sweep(map);
            step(hwnd, "hover map (61)", pts.len(), |i| {
                probe::with_core(hwnd, |c| c.pointer_move(pts[i].0, pts[i].1, now_ms()));
            });
            let side: Vec<(f32, f32)> = (0..150).map(|i| (w - 300.0 + (i * 7 % 260) as f32, 140.0 + (i * 11 % 560) as f32)).collect();
            step(hwnd, "hover panel", side.len(), |i| {
                probe::with_core(hwnd, |c| c.pointer_move(side[i].0, side[i].1, now_ms()));
            });
        }
        if all || what == "zoom" {
            // a click on the biggest folder: the new level grows out of its tile; then Backspace: it shrinks back into it
            step(hwnd, "zoom in (61)", 60, |i| {
                if i == 0 {
                    click_id(hwnd, ID_TILE, false);
                }
            });
            probe::pump(600);
            step(hwnd, "zoom out (61)", 60, |i| {
                if i == 0 {
                    probe::with_core(hwnd, |c| c.key(0x08, Mods::default(), false, now_ms()));
                }
            });
            probe::pump(600);
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
    }
    if all || what == "hover" || what == "zoom" || what == "list" {
        // a folder of 400 items: 300 tiles on the map, 300 rows in the list
        let Some(hwnd) = make(demo::dense(400), "wiztree_bench_dense", scale) else { return 1 };
        let (w, h) = size(hwnd);
        let map = (16.0, 104.0, w - 340.0, h - 148.0);
        if all || what == "hover" {
            let pts = sweep(map);
            step(hwnd, "hover map (301)", pts.len(), |i| {
                probe::with_core(hwnd, |c| c.pointer_move(pts[i].0, pts[i].1, now_ms()));
            });
        }
        if all || what == "zoom" {
            step(hwnd, "zoom in (301)", 60, |i| {
                if i == 0 {
                    click_id(hwnd, ID_TILE, false);
                }
            });
            probe::pump(600);
            probe::with_core(hwnd, |c| c.key(0x08, Mods::default(), false, now_ms()));
            probe::pump(700);
        }
        if all || what == "list" {
            click_id(hwnd, ID_VIEW + 1, false);
            probe::pump(300);
            let (px, py) = (map.0 + map.2 / 2.0, map.1 + map.3 / 2.0);
            step(hwnd, "scroll list (300)", 300, |i| {
                probe::with_core(hwnd, |c| {
                    c.pointer_move(px, py, now_ms());
                    c.wheel(0.0, if (i / 60) % 2 == 0 { -0.35 } else { 0.35 }, Mods::default(), now_ms());
                });
            });
            step(hwnd, "hover list rows", 150, |i| {
                probe::with_core(hwnd, |c| c.pointer_move(px, map.1 + 50.0 + (i * 7 % 500) as f32, now_ms()));
            });
        }
        probe::destroy(hwnd);
    }
    if all || what == "memory" {
        memory_of_a_million();
    }
    0
}

/// A drive of a million records, as a scan of a big drive would give: what it takes to hold, to classify and to lay out.
fn memory_of_a_million() {
    let (ws0, pv0) = memory_now();
    let t = Instant::now();
    let data = demo::synthetic(1_000_000);
    let (ws1, pv1) = memory_now();
    println!(
        "memory           | {} records read into the tree and classified in {:.2} s: {:.0} MB more private bytes ({pv0:.0} -> {pv1:.0}), working set {ws0:.0} -> {ws1:.0}",
        data.tree.recs.len(),
        t.elapsed().as_secs_f64(),
        pv1 - pv0
    );
    let t = Instant::now();
    let l = crate::layout::Layout::build(&data, 1, winrust_common::mft::ROOT, (820.0, 592.0), crate::layout::MAX_TILES);
    let root = t.elapsed().as_secs_f64() * 1000.0;
    // the biggest folder of the biggest folder: a level a click away
    let t = Instant::now();
    let inner = l.tiles.iter().find(|x| x.dir).map_or(0, |x| x.id);
    let l2 = crate::layout::Layout::build(&data, 1, inner, (820.0, 592.0), crate::layout::MAX_TILES);
    println!(
        "memory           | the first level ({} tiles, {} sub-tiles) is laid out in {root:.1} ms, the next ({} tiles) in {:.1} ms",
        l.tiles.len(),
        l.tiles.iter().map(|t| t.subs.len()).sum::<usize>(),
        l2.tiles.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let mut data = data;
    let t = Instant::now();
    let gone = data.remove(inner);
    println!("memory           | the biggest folder ({}) taken off the totals in {:.1} ms", gone.map_or_else(|| "-".to_string(), |g| crate::look::human(g.size())), t.elapsed().as_secs_f64() * 1000.0);
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

/// The first folder of the level asked about in the menu of its tile and chosen: the Down keys walk Open, Show, Copy path, Delete…
/// (the header and the separator are skipped), Enter chooses the last. Nothing else is chosen on the way, so nothing is copied.
fn delete_through_menu(hwnd: isize) {
    click_id(hwnd, ID_TILE, true);
    for _ in 0..4 {
        key(hwnd, 0x28);
    }
    key(hwnd, 0x0D);
}

fn tiles(hwnd: isize) -> usize {
    (0..12).filter(|k| has(hwnd, ID_TILE + k)).count()
}

fn selftest() -> i32 {
    let mut rep = Report(0);
    let composed = gfx::compose_windows();
    println!("WinRust Disk self-test: {} window, off the screen, on made-up drives; no drive is read, nothing is deleted, nothing asks for rights", if composed { "a composed" } else { "a layered" });
    let fake = Arc::new(Fake::new(true));
    let o = |class| Options { class, size: (1160.0, 740.0), icon: 0, place_file: None };
    let Some(hwnd) = probe::make(Box::new(scanned(fake.clone(), 'C')), &o("wiztree_selftest"), composed, (-5000, -5000)) else {
        rep.check(false, "the window can be made");
        return 1;
    };
    probe::show_off_screen(hwnd);
    probe::pump(500);
    rep.check(probe::frames(hwnd) >= 1, "the first frame is drawn");

    // the caption: Windows' questions
    let (w, _) = size(hwnd);
    let (mx, my) = centre(hwnd, WIN_MAX).unwrap_or((-1.0, -1.0));
    let (nx, ny) = centre(hwnd, WIN_MIN).unwrap_or((-1.0, -1.0));
    let (cx, cy) = centre(hwnd, WIN_CLOSE).unwrap_or((-1.0, -1.0));
    rep.check(mx > nx && cx > mx && my > 0.0, "the window's three buttons are there, left to right: min, max, close");
    rep.check(probe::nc_hit(hwnd, mx, my) == HT_MAXBUTTON, "over the maximize button: HTMAXBUTTON (Snap Layouts)");
    rep.check(probe::nc_hit(hwnd, nx, ny) == HT_CLIENT && probe::nc_hit(hwnd, cx, cy) == HT_CLIENT, "over minimize and close: the client's");
    rep.check(probe::nc_hit(hwnd, w - 310.0, 28.0) == HT_CAPTION, "the empty part of the 56 DIP title bar: HTCAPTION");
    rep.check(centre(hwnd, ID_DRIVE).is_some_and(|(x, y)| probe::nc_hit(hwnd, x, y) == HT_CLIENT), "over a drive: the client's, not the caption");
    rep.check(probe::nc_hit(hwnd, 400.0, 2.0) == HT_TOP && probe::nc_hit(hwnd, 400.0, 300.0) == HT_CLIENT, "the top edge is HTTOP, the map the client's");

    // the map: hover, a folder, the way back
    rep.check(has(hwnd, ID_TILE) && has(hwnd, ID_DRIVE + 2) && has(hwnd, ID_LEGEND) && has(hwnd, ID_TOP), "the tiles, the drives, the legend and Biggest here are named");
    if let Some((x, y)) = centre(hwnd, ID_TILE) {
        mouse(hwnd, WM_MOUSEMOVE, x, y);
        rep.check(probe::with_core(hwnd, |c| c.hover) == Some(Some(ID_TILE)), "WM_MOUSEMOVE over a tile hovers it");
    }
    click_id(hwnd, ID_TILE, false);
    probe::pump(600);
    rep.check(has(hwnd, ID_CRUMB + 1) && !has(hwnd, ID_CRUMB + 2), "a click on the biggest folder goes into it (the path has a second crumb)");
    key(hwnd, 0x08);
    probe::pump(600);
    rep.check(!has(hwnd, ID_CRUMB + 1), "Backspace comes out of it");
    click_id(hwnd, ID_VIEW + 1, false);
    rep.check(has(hwnd, ID_COLS) && has(hwnd, ID_LIST + 2), "List: the header and the rows are named");
    click_id(hwnd, ID_VIEW, false);
    probe::pump(100);
    rep.check(!has(hwnd, ID_COLS) && has(hwnd, ID_TILE), "Map: the tiles are back");

    // the menu
    click_id(hwnd, ID_TILE + 1, true);
    rep.check(probe::with_core(hwnd, |c| c.ov.modal()) == Some(true), "the right button over a tile opens the menu");
    key(hwnd, 0x1B);
    rep.check(probe::with_core(hwnd, |c| c.ov.modal()) == Some(false), "Esc closes it");

    // delete: the dialog, and what only its confirmation does
    click_id(hwnd, ID_TILE + 1, false); // Games
    probe::pump(600);
    let in_games = tiles(hwnd);
    rep.check(in_games == 3, &format!("Games has its three tiles ({in_games})"));
    let no_deletes = || fake.log.lock().is_ok_and(|l| l.deletes.is_empty());
    delete_through_menu(hwnd);
    rep.check(has(hwnd, ID_DLG_OK) && has(hwnd, ID_DLG_CANCEL), "Delete… in the menu opens the dialog, with its buttons");
    rep.check(no_deletes(), "... and has deleted nothing");
    key(hwnd, 0x1B);
    rep.check(!has(hwnd, ID_DLG_OK), "Esc is Cancel");
    delete_through_menu(hwnd);
    click_id(hwnd, ID_DLG_SCRIM + 1, false);
    rep.check(has(hwnd, ID_DLG_OK), "a click on the card of the dialog is not a click on the scrim");
    click_id(hwnd, ID_DLG_CANCEL, false);
    rep.check(!has(hwnd, ID_DLG_OK) && no_deletes(), "Cancel: nothing deleted");
    delete_through_menu(hwnd);
    click_id(hwnd, ID_DLG_OPT, false);
    click_id(hwnd, ID_DLG_OK, false);
    probe::pump(300);
    {
        let log = fake.log.lock().unwrap();
        rep.check(
            log.deletes.len() == 1 && log.deletes[0].1 == How::Permanently && log.deletes[0].2 && log.deletes[0].0[0].ends_with("Call of Duty"),
            "\"Delete permanently\" + Delete: one call, confirmed, for the folder asked about",
        );
    }
    rep.check(tiles(hwnd) == in_games - 1, "... and the tile is gone from the map");
    probe::pump(700);

    // to the Recycle Bin, with Undo
    delete_through_menu(hwnd);
    click_id(hwnd, ID_DLG_OK, false);
    probe::pump(700);
    rep.check(has(hwnd, ID_TOAST_UNDO), "to the Recycle Bin: the toast has Undo");
    rep.check(fake.log.lock().is_ok_and(|l| l.deletes.len() == 1), "... and nothing was sent yet");
    click_id(hwnd, ID_TOAST_UNDO, false);
    probe::pump(300);
    rep.check(tiles(hwnd) == in_games - 1 && !has(hwnd, ID_TOAST_UNDO), "Undo: it is on the map again and the toast is gone");
    // and without it: the bin gets it when the toast has run
    delete_through_menu(hwnd);
    click_id(hwnd, ID_DLG_OK, false);
    probe::pump(5800);
    rep.check(fake.log.lock().is_ok_and(|l| l.deletes.len() == 2 && l.deletes[1].1 == How::RecycleBin && l.deletes[1].2), "after the toast (5 s) the Recycle Bin is sent it, confirmed");

    // the drives
    key(hwnd, 0x09);
    probe::pump(300);
    rep.check(fake.log.lock().is_ok_and(|l| l.scans == vec![('D', false)]), "Tab: the next drive is read, the fast way (NTFS)");
    key(hwnd, 0x09);
    probe::pump(300);
    rep.check(fake.log.lock().is_ok_and(|l| l.scans.len() == 1) && has(hwnd, ID_BLOCK_RUN), "Tab again: E: is exFAT: a card, and no scan");

    // rest
    probe::pump(900);
    let n = probe::frames(hwnd);
    probe::pump(2500);
    let m = probe::frames(hwnd);
    rep.check(m == n, &format!("at rest: {} frames in 2.5 s", m - n));
    probe::destroy(hwnd);

    // no rights: the card, and what the buttons ask of the machine
    let poor = Arc::new(Fake::new(false));
    if let Some(h2) = probe::make(Box::new(scanned(poor.clone(), 'C')), &o("wiztree_selftest_user"), composed, (-5000, -5000)) {
        rep.check(has(h2, ID_BLOCK_RUN) && has(h2, ID_BLOCK_ALT) && !has(h2, ID_TILE), "without administrator rights: the card, and no map under it");
        click_id(h2, ID_BLOCK_RUN, false);
        rep.check(poor.log.lock().is_ok_and(|l| l.elevations == vec!['C']), "\"Restart as administrator\" asks the machine (here a fake: no prompt) once");
        rep.check(poor.log.lock().is_ok_and(|l| l.scans.is_empty()), "... and nothing was read");
        click_id(h2, ID_BLOCK_ALT, false);
        probe::pump(300);
        rep.check(poor.log.lock().is_ok_and(|l| l.scans == vec![('C', true)]) && has(h2, ID_TILE), "\"Scan slowly instead\": a slow scan, and the map");
        probe::destroy(h2);
    }

    // memory: a menu, a dialog, a toast with Undo, thirty times
    let Some(h3) = probe::make(Box::new(scanned(Arc::new(Fake::new(true)), 'C')), &o("wiztree_selftest_memory"), composed, (-5000, -5000)) else { return rep.0 };
    probe::show_off_screen(h3);
    probe::pump(400);
    let video = || gfx::gpu_memory().map_or(0.0, |g| g.0);
    let mut seen: Vec<(f64, f64, f64)> = Vec::new();
    for cycle in 1..=30 {
        click_id(h3, ID_TILE + 1, true);
        probe::pump(200);
        key(h3, 0x1B);
        delete_through_menu(h3);
        probe::pump(200);
        key(h3, 0x1B);
        delete_through_menu(h3);
        click_id(h3, ID_DLG_OK, false);
        probe::pump(150);
        click_id(h3, ID_TOAST_UNDO, false);
        probe::pump(60);
        let (ws, pv) = memory_now();
        seen.push((ws, pv, video()));
        if [1, 3, 6, 10, 15, 20, 25, 30].contains(&cycle) {
            println!("  cycle {cycle:2}: {ws:.1} / {pv:.1} MB, video {:.1}", video());
        }
    }
    let low = |r: &[(f64, f64, f64)]| r.iter().fold((f64::MAX, f64::MAX, f64::MAX), |m, v| (m.0.min(v.0), m.1.min(v.1), m.2.min(v.2)));
    let (first, last) = (low(&seen[2..8]), low(&seen[seen.len() - 6..]));
    rep.check(
        last.1 - first.1 <= 2.0 && last.2 - first.2 <= 4.0,
        &format!("30 cycles of menu, dialog, toast with Undo: private {:.1} -> {:.1} MB, video {:.1} -> {:.1} (growth {:+.1} / {:+.1})", first.1, last.1, first.2, last.2, last.1 - first.1, last.2 - first.2),
    );
    probe::destroy(h3);
    println!("{} checks wrong", rep.0);
    rep.0
}
