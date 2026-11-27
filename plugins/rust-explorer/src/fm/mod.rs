//! The file manager window in the Obsidian design, on `winrust-ui`: tabs in the caption, the toolbar with the address and search boxes,
//! the sidebar, the table (or tiles) of the folder, the status line and the preview pane. Only this window: `--desktop` (the user's
//! desktop, GDI) is `desktop.rs` and is left as it is.
//!
//! * `model`: the state and what a click or a key does to it, with no window; `ops`: the commands and menus; `app`: the window's events;
//! * `draw`: painting; `look`: palette and icons; `text`: dates, types, counts;
//! * `backend`: what touches the machine (`Real`), `live`: how the live window does it; `demo`: the made-up machine of the design
//!   and the states `--render` shows; `check`: the benchmark and self-test.
pub mod app;
pub mod backend;
pub mod check;
pub mod demo;
pub mod draw;
pub mod live;
pub mod look;
pub mod model;
pub mod ops;
pub mod text;
#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc};

use winrust_ui::{
    core::place_path,
    window::{Options, run},
};

use backend::Real;
use model::Fm;

/// Where the window was when it last closed was `%APPDATA%\rust-explorer\window.txt`; the window host keeps its place in its own file.
/// The old one is read once, so the first start with the new window opens where the last one was.
fn migrate_place(to: &std::path::Path) {
    if to.exists() {
        return;
    }
    let Some((max, r)) = crate::saved_place() else { return };
    if let Some(d) = to.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(to, format!("{} {} {} {} {}", r.left, r.top, r.right, r.bottom, i32::from(max)));
}

/// The window: opens `dir` (and picks `pick` in it) and runs until it is closed. The device is made on its own thread while the folder is read.
pub fn open(dir: PathBuf, pick: Option<String>) -> i32 {
    winrust_ui::gfx::fast_start();
    let be: Arc<dyn backend::Backend> = Arc::new(Real);
    // the drives are asked (a card reader may take its time) while the folder is read
    let places = {
        let be = be.clone();
        std::thread::spawn(move || be.places())
    };
    let mut fm = Fm::new(be, dir.clone());
    fm.flags_from_backend();
    fm.tabs = vec![dir.clone()];
    let mut cx = winrust_ui::frame::Cx::default();
    fm.load(dir.clone(), true, false, &mut cx);
    fm.places = places.join().unwrap_or_default();
    if let Some(name) = pick {
        fm.select_names(&[name]);
    }
    let place = place_path("rust-explorer");
    if let Some(p) = &place {
        migrate_place(p);
    }
    crate::shellwin::navigated(&fm.dir);
    let code = run(Box::new(fm), Options { class: "RustExplorer", size: (1160.0, 740.0), icon: 1, place_file: place });
    crate::shellwin::revoke();
    code
}
