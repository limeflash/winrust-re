//! The Settings window's own colours and icons, as the mockup's CSS writes them (`[data-theme]` and `[data-theme=light]` of
//! Settings.html): dark value, light value. What the widgets share (`--t1`..`--t3`, `--card`, `--well`, `--menu`) is in
//! `winrust_ui::widgets::look`.
use winrust_ui::gfx::Rgba;

pub use winrust_ui::widgets::look::{self as ui, CARD, RED, T1, T2, T3};

/// The window: `--win`.
pub const WIN: Rgba = Rgba::pair(0x1b1c22, 0xf4f4f7);
/// The sidebar: `--side`.
pub const SIDE: Rgba = Rgba::pair(0x18191e, 0xebebf0);
/// The dimmest text and the chevrons: `--t4` (its light value is not the widgets' one).
pub const T4: Rgba = Rgba::pair(0x5f6474, 0xa3a7b3);
pub const GREEN: Rgba = Rgba::pair(0x7fe0ad, 0x18804d);
pub const AMBER: Rgba = Rgba::pair(0xffb877, 0xa95b06);
/// `#e5484d`, the badge of the Windows Update entry.
pub const BADGE: Rgba = Rgba::new(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);

/// The ink at `a` (the mockup's `rgba(var(--ink),a)`).
#[must_use]
pub const fn ink(a: f32) -> Rgba {
    Rgba::white(a)
}

/// The accent now.
#[must_use]
pub fn acc() -> Rgba {
    ui::accent()
}

/// `color-mix(in oklab, var(--acc) p%, transparent)`: the accent at `p`.
#[must_use]
pub fn acc_at(p: f32) -> Rgba {
    acc().alpha(p)
}

/// `0xRRGGBB` as a colour.
#[must_use]
pub const fn rgb(c: u32) -> Rgba {
    Rgba::new(((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0)
}

/// Icons of the pages, in the order of `pages()`: tile colour and the 24 x 24 glyph (the mockup's `NAV`).
pub const NAV: [(u32, &str); 14] = [
    (0x3b82f6, "M3 4h18v12H3zM8 20h8M12 16v4"),
    (0xe2477a, "M11 5L6 9H3v6h3l5 4zM15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"),
    (0x2f9e5b, "M6 7h11a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2zM21 10v4M8 10v4M11 10v4"),
    (0x2f7bf6, WIFI),
    (0x2563eb, BLUETOOTH),
    (0x8b5cf6, "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z"),
    (0x0891b2, "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM14.5 16.5l2 2 4-4"),
    (0xc026d3, BELL),
    (0xf08c1a, "M18 3l3 3-9 9-4 1 1-4zM4 21c0-2 1.5-4 4-4"),
    (0x15803d, "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM9 12l2 2 4-4"),
    (0x64748b, KEYBOARD),
    (0x4f46e5, CLOCK),
    (0x0ea5a4, "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5"),
    (0x6b7187, "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v6M12 7.5h.01"),
];

pub const BELL: &str = "M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0";
pub const KEYBOARD: &str = "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8";
pub const CLOCK: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2";
pub const CAMERA: &str = "M3 7h4l2-2.5h6L17 7h4v12H3zM12 16.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z";
pub const LOCATION: &str = "M12 21s7-6.2 7-11.5a7 7 0 1 0-14 0C5 14.8 12 21 12 21zM12 12a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z";
pub const DESKTOP: &str = "M3 5h18v12H3zM8 21h8M12 17v4";
pub const SHIELD_CHECK: &str = "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM8.5 12l2.5 2.5 4.5-5";
pub const SHIELD_INFO: &str = "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM12 8v5M12 16h.01";
pub const FIREWALL: &str = "M4 5h16v14H4zM4 10h16M4 14.5h16M9 5v5M15 10v4.5M9 14.5V19";

pub const WIFI: &str = "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01";
pub const WIFI_OFF: &str = "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01M3 3l18 18";
pub const BLUETOOTH: &str = "M7 7l10 10-5 5V2l5 5L7 17";
pub const ETHERNET: &str = "M5 9h14v8H5zM8 17v3M12 17v3M16 17v3M9 6h6v3H9z";
pub const SHIELD: &str = "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z";
pub const PLUS: &str = "M12 5v14M5 12h14";
pub const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";
pub const CHEVRON_LEFT: &str = "M15 5l-7 7 7 7";
pub const SEARCH: &str = "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4";
pub const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
pub const MOON: &str = "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z";
pub const PLANE: &str = "M10.5 21l1.5-1.5V15l7 2v-2l-7-4.5V5a1.5 1.5 0 0 0-3 0v5.5L2 15v2l7-2v4.5L10.5 21";
pub const DOWNLOAD: &str = "M12 4v11M7 10.5l5 5 5-5M5 20h14";
pub const WARN: &str = "M12 8v5M12 16.5h.01M12 3l9 16H3z";
pub const REFRESH: &str = "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5";
pub const SPEAKER: &str = "M11 5L6 9H3v6h3l5 4zM15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13";
pub const SPEAKER_OFF: &str = "M11 5L6 9H3v6h3l5 4zM22 9l-6 6M16 9l6 6";
pub const MIC: &str = "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3";
pub const MIC_OFF: &str = "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3M3 3l18 18";
pub const APP_GRID: &str = "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z";
pub const DRIVE: &str = "M3 14h18v5H3zM3 14l2.5-8h13L21 14M17 16.5h.01";
pub const PLUG: &str = "M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4";
