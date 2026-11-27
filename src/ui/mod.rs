//! The installer's window, in the Obsidian design (Installer.html): a title bar with the language, the list of modules with a
//! preview beside it, and a footer with the one big button. It is an [`App`](winrust_ui::core::App) of `winrust-ui`, so it never sees a
//! window; the real work is behind [`model::Backend`].
//!
//! * `catalog`  the modules as the design lists them, with their texts in five languages, and which plugin each is
//! * `text`     every other text of the window, in five languages
//! * `model`    the switches, what they ask for, and the job that does it on a worker thread (no window, tested)
//! * `app`      the window's state and what a click, a key or a worker's message does
//! * `draw`     how it is drawn
//! * `preview`  the embedded pictures, decoded by WIC
pub mod app;
pub mod catalog;
mod draw;
pub mod look;
pub mod model;
pub mod preview;
pub mod text;

pub use app::Installer;
pub use model::{Backend, ModInfo};
