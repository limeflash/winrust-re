#![windows_subsystem = "windows"]
//! photoview [picture or folder]   The photo viewer of WinRust:RE, a window of `winrust-ui` in the Obsidian design.
//!
//! Left / Right, Space, Backspace, PgUp / PgDn: previous / next    Home / End: first / last    wheel: zoom    drag: pan    double click,
//! F: fit <-> 100%    1: 100%    + / -: zoom    R / Shift+R: turn    I: details    Del: to the Recycle Bin (Undo for 4 s)    Ctrl+C: copy
//!
//! photoview --render <png> state=<name> [light] [scale=1.5]   a state of the design as a picture (no window)
//! photoview --render-all <dir>                                every state, dark and light, scale 1 and 1.5
//! photoview --bench [open|hover|menu|scroll|pan|resize|rest]  what the window costs (windows off the screen)
//! photoview --selftest window | stage <png> | memory          the host, on windows off the screen
mod decode;
mod decoder;
mod folder;
mod look;
mod shell;
mod view;
mod viewer;

use std::path::PathBuf;

use winrust_ui::core::place_path;
use winrust_ui::window::{Options, quit, run};

/// The pictures the viewer opens (what the system has a codec for may be more: WebP, HEIC, AVIF).
pub const EXTS: [&str; 11] = ["jpg", "jpeg", "jfif", "png", "bmp", "gif", "tif", "tiff", "webp", "heic", "avif"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = winrust_ui::render::cli(&args, viewer::demo, &viewer::STATES) {
        quit(code as u32);
    }
    // the decoding of the first picture starts now, while the window and the GPU are still being made
    let path = args.first().filter(|a| !a.starts_with("--")).map(PathBuf::from);
    let app = viewer::Viewer::open(path.as_deref(), true);
    let code = run(Box::new(app), Options { class: "WinRustPhoto", size: (1160.0, 740.0), icon: 1, place_file: place_path("photoview") });
    quit(code as u32);
}
