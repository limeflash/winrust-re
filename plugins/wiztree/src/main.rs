#![windows_subsystem = "windows"]
//! wiztree [X:]   WinRust Disk: where the space on a drive went. A window of `winrust-ui` in the Obsidian design.
//!
//! Click a folder: go in    Right click: its menu    Backspace: up    Tab: the next drive    F5: scan again    Del: delete (a dialog asks how)
//! It reads the NTFS file table, which Windows allows only to administrators: it asks for them (UAC) and starts again.
//!
//! wiztree --render <png> state=<name> [light] [scale=1.5] [max]   a state of the design as a picture (no window, no scan)
//! wiztree --render-all <dir>                                      every state, dark and light, scale 1 and 1.5
//! wiztree --bench [open|hover|zoom|scroll|rest|memory|all]        what it costs (windows off the screen, made-up drives)
//! wiztree --selftest disk | window | stage <png>                  the window on made-up drives, off the screen
mod app;
mod backend;
mod check;
mod delete;
mod demo;
mod draw;
mod layout;
mod look;
mod model;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_review;

use std::sync::Arc;

use backend::{Backend, Real};
use winrust_ui::core::place_path;
use winrust_ui::window::{quit, run, Options};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // renders, benchmarks and self-tests never ask for rights and never read a drive: they run on made-up ones
    if let Some(code) = check::cli(&args).or_else(|| winrust_ui::render::cli(&args, demo::demo, &demo::STATES)) {
        quit(code as u32);
    }
    let letter = args.first().and_then(|a| a.chars().next()).filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase());
    let be: Arc<dyn Backend> = Arc::new(Real);
    // the file table needs administrator rights: ask once through UAC and let the elevated copy do the work; if the user says no,
    // the window opens and says what it needs
    if !be.is_admin() && be.restart_elevated(letter.unwrap_or('C')) {
        quit(0);
    }
    let app = app::Disk::new(be, letter);
    let code = run(Box::new(app), Options { class: "WinRustDisk", size: (1160.0, 740.0), icon: 1, place_file: place_path("wiztree") });
    quit(code as u32);
}
