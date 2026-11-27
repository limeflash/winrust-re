//! `--render`: an application window into a PNG, with no window. The same [`Core`] the live window uses draws it (so does its
//! shadow), and the pointer is driven by hand: hover, clicks and focus go through the real event path, so a menu, a toast or a
//! dialog appears exactly as it does for a user. It creates no window and touches no input, so it is safe next to the user's
//! desktop.
//!
//! ```text
//! <app> --render <file.png> [light] [scale=1.5] [w=1240 h=820] [state=<name>] [hover=<id>] [focus=<id>] [click=<id>,<id>] [max]
//! <app> --render-all <dir>
//! ```
//! The picture is the mockups' stage: `w` × `h` DIPs of `#05070c`, the window 40 DIPs in from every edge (`max` fills it), its
//! shadow round it.

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

use crate::core::{App, Core};
use crate::frame::{Button, Id};
use crate::gfx::{Gfx, Surface};
use crate::window::{SHADOW_PAD, paint_shadow, shadow_extent};
use winrust_common::style;

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// When the command line was first looked at (the benchmark's "from main").
pub(crate) fn started() -> std::time::Instant {
    *START.get_or_init(std::time::Instant::now)
}

/// What a render shows.
#[derive(Clone, Debug)]
pub struct Opts {
    /// The stage, DIPs.
    pub w: f32,
    pub h: f32,
    pub scale: f32,
    pub hover: Option<Id>,
    pub focus: Option<Id>,
    /// Items clicked, one after the other, before the picture is taken.
    pub click: Vec<Id>,
    pub max: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self { w: 1240.0, h: 820.0, scale: 1.0, hover: None, focus: None, click: Vec::new(), max: false }
    }
}

/// The margin between the stage's edge and the window (the mockups' 40 DIPs).
const STAGE_INSET: f32 = 40.0;
/// The stage's colour (`rootBg` of the mockups): the same in both modes.
const STAGE: u32 = 0xff05_070c;

/// A frame of `core` into a DIB surface, premultiplied pixels.
fn frame(g: &mut Gfx, core: &mut Core, surf: &mut Surface, now: u64) -> bool {
    let (w, h) = ((core.size.0 * core.scale).round() as i32, (core.size.1 * core.scale).round() as i32);
    g.paint(surf, w, h, |c| core.draw(c, now)).is_ok()
}

/// Draws `app` the way a user has brought it to (`o`) into the window of a stage and returns the stage's pixels, and its size in
/// device pixels. The window's pointer events are the ones a mouse would make, one at a time, each with a frame between.
pub fn render_pixels(app: Box<dyn App>, o: &Opts) -> Option<(Vec<u32>, i32, i32)> {
    let s = o.scale;
    let px = |d: f32| (d * s).round() as i32;
    let (win_w, win_h) = if o.max { (o.w, o.h) } else { (o.w - 2.0 * STAGE_INSET, o.h - 2.0 * STAGE_INSET) };
    let mut g = Gfx::new(s).ok()?;
    let mut core = Core::new(app, (win_w, win_h), s, 0);
    core.maximized = o.max;
    let mut surf = Surface::new();
    let mut t = 1_000_000u64;
    frame(&mut g, &mut core, &mut surf, t);
    let centre = |core: &Core, id: Id| core.hits().get(id).map(|h| (h.r.cx(), h.r.cy()));
    for &id in &o.click {
        t += 400;
        let Some((x, y)) = centre(&core, id) else { continue };
        core.pointer_move(x, y, t);
        core.down(Button::Left, x, y, t);
        core.up(Button::Left, x, y, t);
        core.take_host();
        frame(&mut g, &mut core, &mut surf, t);
    }
    // what the clicks opened has come in (an overlay names its items only once it is visible)
    t += 400;
    frame(&mut g, &mut core, &mut surf, t);
    if let Some(id) = o.hover
        && let Some((x, y)) = centre(&core, id)
    {
        t += 400;
        core.pointer_move(x, y, t);
        frame(&mut g, &mut core, &mut surf, t);
    }
    if let Some(id) = o.focus {
        core.mo.hits.focus = Some(id);
        core.mo.hits.focus_visible = true;
    }
    // everything that moves has arrived: hover, menus, the toast (it stands still from 8 % to 90 % of its time), the hint's delay
    t += 1000;
    if !frame(&mut g, &mut core, &mut surf, t) {
        return None;
    }

    let (sw, sh) = (px(o.w), px(o.h));
    let mut stage = vec![STAGE; (sw * sh) as usize];
    let origin = if o.max { (0, 0) } else { (px(STAGE_INSET), px(STAGE_INSET)) };
    if !o.max {
        let (dw, dh) = shadow_extent(win_w, win_h);
        let mut shadow = Surface::new();
        let (shw, shh) = (px(dw), px(dh));
        g.paint(&mut shadow, shw, shh, |c| paint_shadow(c, win_w, win_h)).ok()?;
        let pad = px(SHADOW_PAD);
        blend(&mut stage, (sw, sh), shadow.pixels(), shw, (origin.0 - pad, origin.1 - pad));
    }
    blend(&mut stage, (sw, sh), surf.pixels(), px(win_w), origin);
    Some((stage, sw, sh))
}

/// Premultiplied BGRA `src` (`sw` pixels wide) over the opaque `dst` at `(x, y)`, clipped.
fn blend(dst: &mut [u32], (dw, dh): (i32, i32), src: &[u32], sw: i32, (x, y): (i32, i32)) {
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
            let sa = p >> 24;
            if sa == 0 {
                continue;
            }
            let d = &mut dst[(ty * dw + tx) as usize];
            let mix = |s: u32| (((p >> s) & 255) + ((*d >> s) & 255) * (255 - sa) / 255).min(255);
            *d = 0xff00_0000 | mix(16) << 16 | mix(8) << 8 | mix(0);
        }
    }
}

/// [`render_pixels`] into a PNG file.
pub fn render_with(app: Box<dyn App>, o: &Opts, png: &str) -> bool {
    let Some((px, w, h)) = render_pixels(app, o) else { return false };
    let Ok(g) = Gfx::new(o.scale) else { return false };
    g.save_png(png, w as u32, h as u32, &px).is_ok()
}

/// `render(app, w, h, scale, png)`: the app's window on its stage at `scale`.
pub fn render(app: Box<dyn App>, w: f32, h: f32, scale: f32, png: &str) -> bool {
    render_with(app, &Opts { w, h, scale, ..Opts::default() }, png)
}

fn parse(args: &[String]) -> (Opts, String) {
    let mut o = Opts::default();
    let mut state = String::new();
    for a in args {
        match a.split_once('=') {
            Some(("w", v)) => o.w = v.parse().unwrap_or(o.w),
            Some(("h", v)) => o.h = v.parse().unwrap_or(o.h),
            Some(("scale", v)) => o.scale = v.parse().unwrap_or(1.0),
            Some(("state", v)) => state = v.to_string(),
            Some(("hover", v)) => o.hover = v.parse().ok(),
            Some(("focus", v)) => o.focus = v.parse().ok(),
            Some(("click", v)) => o.click = v.split(',').filter_map(|n| n.parse().ok()).collect(),
            _ => o.max |= a == "max",
        }
    }
    (o, state)
}

/// The command line of an app that has renders, a benchmark and self-tests. `Some(exit code)` when the arguments asked for one of
/// them (the app then quits with it), `None` for a normal start. `states` are what `--render-all` goes through.
pub fn cli(args: &[String], demo: fn(&str) -> Box<dyn App>, states: &[&str]) -> Option<i32> {
    started();
    let has = |f: &str| args.iter().any(|a| a == f);
    let after = |f: &str| args.iter().position(|a| a == f).map(|i| &args[i + 1..]);
    if !(has("--render") || has("--render-all") || has("--bench") || has("--selftest")) {
        return None;
    }
    // the renderers draw dark unless `light` is among the arguments: the registry's choice is the live window's, not theirs
    style::set_light(has("light"));
    // SAFETY: COM for WIC on this thread, once.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if let Some(rest) = after("--render") {
        let Some(file) = rest.first() else { return Some(2) };
        let (o, state) = parse(&rest[1..]);
        return Some(if render_with(demo(&state), &o, file) { 0 } else { 1 });
    }
    if let Some(rest) = after("--render-all") {
        let Some(dir) = rest.first() else { return Some(2) };
        let _ = std::fs::create_dir_all(dir);
        let mut failed = 0;
        for state in states {
            for (theme, light) in [("dark", false), ("light", true)] {
                for scale in [1.0, 1.5] {
                    style::set_light(light);
                    let name = if scale == 1.0 { format!("{dir}/{state}-{theme}.png") } else { format!("{dir}/{state}-{theme}-x{scale}.png") };
                    failed += i32::from(!render_with(demo(state), &Opts { scale, ..Opts::default() }, &name));
                }
            }
        }
        return Some(failed);
    }
    if let Some(rest) = after("--bench") {
        return Some(crate::bench::run(rest, demo));
    }
    after("--selftest").map(|rest| crate::selftest::run(rest, demo))
}
