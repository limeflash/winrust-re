//! The widgets every application window is made of. Immediate mode like the bar's panels: a function draws and names its items
//! with `Frame::item`; the click comes back to `App::click(id)`.
pub mod button;
pub mod caption;
pub mod look;
pub mod overlay;
pub mod field;
pub mod field_model;
pub mod list;
pub mod scroll;
pub mod scroll_model;
pub mod list_model;
#[cfg(test)]
#[path = "widget_tests.rs"]
mod widget_tests;
