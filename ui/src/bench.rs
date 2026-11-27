//! `--bench [open|hover|menu|scroll|resize|rest|all]`: what an application window costs on this machine, in the form of the bar's
//! benchmark. The window is a real composed one, put at (-5000, -5000) where nobody sees it, 2560 × 1440 device pixels big (the
//! user's screen); the pointer, the wheel and the size are driven by calls, one frame each, and the GPU is waited for after every
//! frame (`gfx::gpu_wait`) so that what is left to it is counted too.
//!
//! The columns, milliseconds per frame, mean / p95 / max: `encode` (CPU drawing, without the end of the frame), `end` (`EndDraw`:
//! D2D hands its commands to the GPU), `present` (showing the surface, moving the shadow window, `Commit`), `total` (the three) and
//! `gpu` (what the GPU still needed after the CPU was done). `open` is the time from the start of `main` to the first frame
//! committed, the median of five runs of the exe; memory is the working set and the private bytes in MB.

use std::time::{Duration, Instant};

use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::core::App;
use crate::gfx;
use crate::selftest::{memory_now, pump};
use crate::window::{Options, create, with};

pub fn run(args: &[String], demo: fn(&str) -> Box<dyn App>) -> i32 {
    let what = args.first().map_or("all", String::as_str);
    if what == "open1" {
        return open_once(demo);
    }
    let composed = gfx::compose_windows();
    println!("{} windows; ms per frame, mean / p95 / max; memory in MB: working set, private", if composed { "composed" } else { "layered" });
    if matches!(what, "open" | "all") {
        open();
    }
    let all = what == "all";
    if !matches!(what, "hover" | "menu" | "scroll" | "resize" | "rest") && !all {
        return i32::from(!matches!(what, "open"));
    }
    println!("scenario      | encode  mean    p95     max |    end  mean    p95     max | present mean    p95     max |  total  mean    p95     max |    gpu  mean    p95     max |   ws priv");
    // 2560 x 1440 device pixels whatever the system's DPI is
    let scale = {
        // SAFETY: a plain query.
        unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() as f32 / 96.0 }
    };
    let o = Options { class: "winrust_ui_bench", size: (2560.0 / scale, 1440.0 / scale), icon: 0, place_file: None };
    let Some(hwnd) = create(demo(""), &o, composed, Some((-5000, -5000))) else {
        println!("the window could not be made");
        return 1;
    };
    // SAFETY: a window of ours, shown off the screen without activation: its surfaces are really composed.
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    pump(400);
    let (cw, ch) = with(hwnd, |h| h.core.size).unwrap_or((1.0, 1.0));
    println!("window: {:.0} x {:.0} DIPs at scale {scale} ({:.0} x {:.0} px)", cw, ch, cw * scale, ch * scale);
    let step = |hwnd, label: &str, n: usize, mut f: Box<dyn FnMut(usize)>| {
        pump(300); // what the last scenario left moving settles
        with(hwnd, |h| {
            h.spent.clear();
            h.timing = true;
        });
        let mut gpu = Vec::with_capacity(n);
        let mut total = Vec::with_capacity(n);
        for i in 0..n {
            let t = Instant::now();
            f(i);
            total.push(t.elapsed().as_secs_f64() * 1000.0);
            gpu.push(gfx::gpu_wait().as_secs_f64() * 1000.0);
        }
        let spent = with(hwnd, |h| {
            h.timing = false;
            std::mem::take(&mut h.spent)
        })
        .unwrap_or_default();
        report(label, &spent, &mut gpu);
        let _ = total;
    };
    if all || what == "hover" {
        // the pointer sweeps the row of buttons, then the list: every step moves a hover
        let pts: Vec<(f32, f32)> = (0..300).map(|i| if i < 150 { (30.0 + i as f32 * 4.0, 89.0) } else { (300.0 + (i - 150) as f32 * 3.0, 240.0 + (i - 150) as f32 * 8.0) }).collect();
        step(hwnd, "hover", pts.len(), Box::new(move |i| drive(hwnd, |h| h.core.pointer_move(pts[i].0, pts[i].1, crate::anim::now_ms()))));
    }
    if all || what == "menu" {
        // an overlay opens (a click on the item that asks for it) and comes in: 60 frames of its arrival, then it goes
        for (label, id, close) in [("menu opening", 1, true), ("toast", 51, false), ("dialog", 52, true)] {
            step(hwnd, label, 60, Box::new(move |i| {
                if i == 0 {
                    click(hwnd, id);
                } else {
                    drive(hwnd, |_| true);
                }
            }));
            if close {
                drive(hwnd, |h| h.core.key(0x1B, Default::default(), false, crate::anim::now_ms()));
            } else {
                click(hwnd, crate::widgets::overlay::TOAST_UNDO); // the toast would still be leaving when `rest` counts
            }
        }
    }
    if all || what == "scroll" {
        step(hwnd, "scroll", 300, Box::new(move |i| drive(hwnd, |h| {
            h.core.pointer_move(400.0, 400.0, crate::anim::now_ms());
            h.core.wheel(0.0, if (i / 60) % 2 == 0 { -0.35 } else { 0.35 }, Default::default(), crate::anim::now_ms());
            true
        })));
    }
    if all || what == "resize" {
        // the size changes at every step, as when a corner is dragged: WM_SIZE draws the frame, then the shadow follows
        let r = {
            let mut r = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
            // SAFETY: a query into a local.
            unsafe { GetWindowRect(hwnd, &mut r) };
            r
        };
        let (w0, h0) = (r.right - r.left, r.bottom - r.top);
        step(hwnd, "resize", 300, Box::new(move |i| {
            let k = (i as f32 * 0.21).sin();
            let (w, h) = (w0 - (80.0 + 60.0 * k) as i32, h0 - (60.0 + 40.0 * (i as f32 * 0.13).cos()) as i32);
            // SAFETY: our own window; the call draws the frame inside it, through WM_SIZE.
            unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), -5000, -5000, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
        }));
    }
    if all || what == "rest" {
        pump(800);
        let n = with(hwnd, |h| h.frames).unwrap_or(0);
        pump(3000);
        let m = with(hwnd, |h| h.frames).unwrap_or(0);
        let (ws, pv) = memory_now();
        println!("rest          | {} frames in 3 s at rest {} | {ws:4.0} {pv:4.0}", m - n, if m == n { "(ok)" } else { "(NOT ZERO)" });
    }
    // SAFETY: our own window.
    unsafe { DestroyWindow(hwnd) };
    0
}

/// One frame after `f` (which says whether anything changed) on the host of `hwnd`.
fn drive(hwnd: windows_sys::Win32::Foundation::HWND, f: impl FnOnce(&mut crate::window::Host) -> bool) {
    with(hwnd, |h| {
        f(h);
        h.core.take_redraw();
        h.frame();
    });
}

/// A click on item `id` through the core's real path, and the frame it makes.
fn click(hwnd: windows_sys::Win32::Foundation::HWND, id: i32) {
    with(hwnd, |h| {
        let now = crate::anim::now_ms();
        let Some((x, y)) = h.core.hits().get(id).map(|i| (i.r.cx(), i.r.cy())) else { return };
        h.core.pointer_move(x, y, now);
        h.core.down(crate::frame::Button::Left, x, y, now);
        h.core.up(crate::frame::Button::Left, x, y, now);
        h.frame();
    });
}

fn stats(v: &mut [f64]) -> (f64, f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    (v.iter().sum::<f64>() / v.len() as f64, v[(v.len() * 95).div_ceil(100) - 1], v[v.len() - 1])
}

fn report(name: &str, spent: &[(f64, f64, f64)], gpu: &mut [f64]) {
    let (mut a, mut b, mut c): (Vec<f64>, Vec<f64>, Vec<f64>) = (spent.iter().map(|s| s.0).collect(), spent.iter().map(|s| s.1).collect(), spent.iter().map(|s| s.2).collect());
    let mut t: Vec<f64> = spent.iter().map(|s| s.0 + s.1 + s.2).collect();
    let (a, b, c, t, g) = (stats(&mut a), stats(&mut b), stats(&mut c), stats(&mut t), stats(gpu));
    let (ws, private) = memory_now();
    println!(
        "{name:13} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {:6.3} {:6.3} {:7.3} | {ws:4.0} {private:4.0}   ({} frames)",
        a.0, a.1, a.2, b.0, b.1, b.2, c.0, c.1, c.2, t.0, t.1, t.2, g.0, g.1, g.2, spent.len()
    );
}

/// Starts the exe five times to the first committed frame; the median.
fn open() {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut runs: Vec<Vec<f64>> = Vec::new();
    for _ in 0..5 {
        let Ok(out) = std::process::Command::new(&exe).args(["--bench", "open1"]).output() else { continue };
        let text = String::from_utf8_lossy(&out.stdout);
        let v: Vec<f64> = text.lines().find_map(|l| l.strip_prefix("open1 ")).map(|l| l.split_whitespace().filter_map(|n| n.parse().ok()).collect()).unwrap_or_default();
        if v.len() >= 6 {
            runs.push(v);
        }
    }
    if runs.is_empty() {
        println!("open: the child runs reported nothing");
        return;
    }
    let med = |i: usize| {
        let mut v: Vec<f64> = runs.iter().map(|r| r[i]).collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let all: Vec<String> = runs.iter().map(|r| format!("{:.0}", r[0])).collect();
    println!(
        "open          | first frame committed {:.0} ms from main (median of {}: {} ms): devices {:.0}, window with its first frame {:.0}, show {:.0}; memory {:.0} / {:.0} MB",
        med(0),
        runs.len(),
        all.join(", "),
        med(3),
        med(4),
        med(5),
        med(1),
        med(2)
    );
}

/// One start: the time from the first thing `main` does to the first frame of the window committed to the compositor. Prints the
/// total and where it went: the devices (adapter, Direct3D, Direct2D, DirectComposition), the window with its first frame (the
/// fonts, the first text layouts, the shadow's blur), showing it.
fn open_once(demo: fn(&str) -> Box<dyn App>) -> i32 {
    let started = crate::render::started();
    let composed = gfx::compose_windows();
    let devices = started.elapsed().as_secs_f64() * 1000.0;
    let o = Options { class: "winrust_ui_open", size: (1160.0, 740.0), icon: 0, place_file: None };
    let Some(hwnd) = create(demo(""), &o, composed, Some((-5000, -5000))) else { return 1 };
    let made = started.elapsed().as_secs_f64() * 1000.0;
    // SAFETY: shown off the screen without activation: the frame is really composed.
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    pump(Duration::from_millis(100).as_millis() as u64);
    let (ws, pv) = memory_now();
    println!("open1 {ms:.1} {ws:.1} {pv:.1} devices {devices:.0} window+first frame {:.0} show {:.0}", made - devices, ms - made);
    0
}
