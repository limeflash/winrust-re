//! `installer-ui`: the installer's window with fake install steps, so that it can be looked at, measured and checked from an ordinary
//! shell. The real exe (plugin-launcher.exe) asks for administrator rights and installs for real; this is an example target, which
//! build.rs gives no manifest, and whose steps change nothing (`ui_fake.rs`).
//!
//! ```text
//! cargo run --release --example installer-ui -- --render out.png state=choose [light] [scale=1.5]
//! cargo run --release --example installer-ui -- --render-all <dir>            every state, dark and light, scale 1 and 1.5
//! cargo run --release --example installer-ui -- --bench [open|hover|glide|resize|rest|all]
//! cargo run --release --example installer-ui -- --selftest window | memory
//! ```
//! States: choose, installing, done (each also as -ru, -ja, -pt, -zh), p-explorer, p-term, p-keys, p-tree, p-purge, sel-rec, sel-all,
//! sel-none, installed, confirm, remove-all, failed, newer. The stage is the mockup's 1080 x 720.
#![allow(dead_code, unused_imports)]

#[path = "selection.rs"]
mod selection;
#[path = "ui/mod.rs"]
mod ui;
#[path = "ui_fake.rs"]
mod fake;
#[path = "ui_states.rs"]
mod states;

use std::sync::Arc;

use winrust_ui::core::App;
use winrust_ui::render::{Opts, render_with};
use winrust_common::style;

fn demo(state: &str) -> Box<dyn App> {
    states::demo(Arc::new(fake::Fake::new()), state)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // the mockup's stage is 1080 x 720 (the window 1000 x 640 in it)
    if args.iter().any(|a| a == "--render") {
        for (k, v) in [("w", "1080"), ("h", "720")] {
            if !args.iter().any(|a| a.starts_with(&format!("{k}="))) {
                args.push(format!("{k}={v}"));
            }
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--render-all") {
        let Some(dir) = args.get(i + 1) else { std::process::exit(2) };
        // SAFETY: COM for WIC on this thread, once.
        let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
        let _ = std::fs::create_dir_all(dir);
        let mut failed = 0;
        for state in states::STATES {
            for (theme, light) in [("dark", false), ("light", true)] {
                for scale in [1.0, 1.5] {
                    style::set_light(light);
                    let name = if scale == 1.0 { format!("{dir}/{state}-{theme}.png") } else { format!("{dir}/{state}-{theme}-x{scale}.png") };
                    failed += i32::from(!render_with(demo(state), &Opts { w: 1080.0, h: 720.0, scale, ..Opts::default() }, &name));
                }
            }
        }
        println!("{} pictures, {failed} failed", states::STATES.len() * 4);
        std::process::exit(failed);
    }
    if let Some(code) = winrust_ui::render::cli(&args, demo, &states::STATES) {
        winrust_ui::window::quit(code as u32);
    }
    eprintln!("installer-ui --render <png> state=<name> [light] [scale=1.5] | --render-all <dir> | --bench [..] | --selftest window|memory");
    std::process::exit(2);
}
