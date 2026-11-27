//! The Task Manager's own colours, icons and the little measures of its screens, as TaskManager.html writes them
//! (`[data-app=tm]` and `[data-theme=light]`): the dark value and the light one beside it.
use winrust_ui::gfx::Rgba;
pub use winrust_ui::widgets::look::{CARD, T1, T2, T3, WELL, accent, focus_ring};

/// The window: `--win`.
pub const WIN: Rgba = Rgba::pair(0x1b1c22, 0xf7f7f9);
/// The sidebar: `--side`.
pub const SIDE: Rgba = Rgba::pair(0x17181d, 0xefeff3);
/// The text of a button that is `var(--t1)` on `var(--t1)` (a primary button, the dark text on it): `--inv`.
pub const INV: Rgba = Rgba::pair(0x14151a, 0xffffff);
pub const T4: Rgba = Rgba::pair(0x5f6474, 0xa3a7b3);
pub const RED: Rgba = Rgba::pair(0xff8a8f, 0xc8333a);
pub const GREEN: Rgba = Rgba::pair(0x7fe0ad, 0x18804d);
pub const AMBER: Rgba = Rgba::pair(0xffb877, 0xa95b06);

/// `--accT`: the accent as text, darkened in light mode (`color-mix(in oklab, var(--acc) 72%, #000)`).
#[must_use]
pub fn acc_text() -> Rgba {
    if winrust_common::style::light() { accent().mix(Rgba::new(0.0, 0.0, 0.0, 1.0), 0.28) } else { accent() }
}

/// `rgba(76,195,138,.12)`, the pill of what is on.
#[must_use]
pub fn green_soft() -> Rgba {
    Rgba::new(76.0 / 255.0, 195.0 / 255.0, 138.0 / 255.0, 0.12)
}
/// `rgba(255,163,72,.12)`, the pill of what needs care.
#[must_use]
pub fn amber_soft(a: f32) -> Rgba {
    Rgba::new(1.0, 163.0 / 255.0, 72.0 / 255.0, a)
}
/// `rgba(229,72,77,.16)`, the pill of what is wrong.
#[must_use]
pub fn red_soft(a: f32) -> Rgba {
    Rgba::new(229.0 / 255.0, 72.0 / 255.0, 77.0 / 255.0, a)
}
/// A fixed colour that is the same in both modes (the dots of a toast, a chart's line).
#[must_use]
pub const fn rgb(c: u32) -> Rgba {
    Rgba::new(((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0)
}

/// The colours of the five charts: CPU takes the accent, the others are the design's.
pub const MEM_C: Rgba = rgb(0xc0a5f5);
pub const DISK_C: Rgba = rgb(0x5bc8af);
pub const NET_C: Rgba = rgb(0xffa348);
pub const GPU_C: Rgba = rgb(0xf08aa6);
/// The toast dots: done, wrong, take care.
pub const DOT_OK: Rgba = rgb(0x4cc38a);
pub const DOT_BAD: Rgba = rgb(0xe5484d);
pub const DOT_WARN: Rgba = rgb(0xffa348);

/// The cell tint of a heat column: `color-mix(in oklab, var(--acc) p%, transparent)` for a value `v` of at most `max`, p growing
/// from 4 to `cap` with the share, and nothing under a tenth.
#[must_use]
pub fn heat(v: f64, max: f64, cap: f64) -> Option<Rgba> {
    if v <= 0.0 || max <= 0.0 || v / max < 0.1 {
        return None;
    }
    let p = (4.0 + (v / max).min(1.0) * cap).min(cap).round();
    Some(accent().alpha((p / 100.0) as f32))
}

/// A tile for an app that has no icon of its own: the design's `T(c1, c2)` gradient and a glyph in white.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub from: u32,
    pub to: u32,
    pub glyph: &'static str,
}

pub const fn tile(from: u32, to: u32, glyph: &'static str) -> Tile {
    Tile { from, to, glyph }
}

pub const T_SYS: Tile = tile(0x5a6074, 0x2b2e3a, "M4 5h16v14H4zM4 9h16");
pub const T_RUST: Tile = tile(0xe08a5a, 0xa4522c, "M8 7l-5 5 5 5M16 7l5 5-5 5");
pub const T_PS: Tile = tile(0x3d7be8, 0x1f3f9e, "M4 5h16v14H4zM7.5 10l3 2.5-3 2.5M13 15h4");
pub const T_NV: Tile = tile(0x6bc24a, 0x3b7a24, "M3 12c3-5 15-5 18 0-3 5-15 5-18 0zM12 14.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z");
pub const T_SHIELD: Tile = tile(0x3cc4f2, 0x1c74c4, "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z");
pub const T_BOX: Tile = tile(0xe08a5a, 0xa4522c, "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8");
pub const T_CLOUD: Tile = tile(0x3cc4f2, 0x1c74c4, "M7 18a4 4 0 0 1-.5-8 5.5 5.5 0 0 1 10.6-1.3A4.5 4.5 0 0 1 17 18z");
pub const T_GAME: Tile = tile(0x3a4a6a, 0x1a2236, "M7 8h10a4 4 0 0 1 4 4v3a2 2 0 0 1-3.6 1.2L16 14H8l-1.4 2.2A2 2 0 0 1 3 15v-3a4 4 0 0 1 4-4z");
pub const T_WIN: Tile = tile(0x62a0ea, 0x2f5fc8, "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z");
pub const T_UNK: Tile = tile(0x4a4f5e, 0x2a2d38, "M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.7M12 17h.01");
/// What stands in for an app's own icon where there is none to read (a made-up process).
pub const T_APP: Tile = tile(0x6a7cc8, 0x3a4690, "M5 5h14v14H5zM5 9h14");

// icons of the sidebar, the buttons and the menus (24 x 24, stroked), as the mockup draws them
pub const NAV_PROC: &str = "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01";
pub const NAV_PERF: &str = "M3 12h4l2.5-6 4 12 2.5-6H21";
pub const NAV_HIST: &str = "M12 7v5l3 2M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4";
pub const NAV_START: &str = "M13 2L5 13h6l-1 9 8-12h-6z";
pub const NAV_USERS: &str = "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0";
pub const NAV_DET: &str = "M4 6h16M4 10h16M4 14h10M4 18h10";
pub const NAV_SVC: &str = "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M18.4 5.6l-1.8 1.8M7.4 16.6l-1.8 1.8";
pub const NAV_BOOST: &str = "M13 2L4 14h7l-1 8 9-12h-7z";
pub const NAV_SLOW: &str = "M12 14l3.5-3.5M4.2 17a8.5 8.5 0 1 1 15.6 0M12 14h.01";
pub const NAV_RULES: &str = "M4 6h10M4 12h7M4 18h10M17 4v6M14 7h6M15 15l5 5M20 15l-5 5";
pub const NAV_SET: &str = "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";

pub const I_SEARCH: &str = "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4";
pub const I_PLUS: &str = "M12 5v14M5 12h14";
pub const I_END: &str = "M6 6l12 12M18 6L6 18";
pub const I_LEAF: &str = "M5 19c0-8 5-13 14-14-1 9-6 14-14 14zM5 19l7-7";
pub const I_MORE: &str = "M6 12h.01M12 12h.01M18 12h.01";
pub const I_START: &str = "M7 4.5v15l12-7.5z";
pub const I_STOP: &str = "M6 6h12v12H6z";
pub const I_TRASH: &str = "M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13";
pub const I_TREE: &str = "M6 3v12M6 9h8a4 4 0 0 1 4 4v2M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6z";
pub const I_PAUSE: &str = "M7 5v14M17 5v14";
pub const I_PRIO: &str = "M12 20V4M6 10l6-6 6 6";
pub const I_CORE: &str = "M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4";
pub const I_FOLDER: &str = "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z";
pub const I_COPY: &str = "M9 9h11v11H9zM5 15V4h11";
pub const I_RULE: &str = "M4 6h10M4 12h7M4 18h10M17 4v6M14 7h6";
pub const I_EXPAND: &str = "M9 6l6 6-6 6";
pub const I_CSV: &str = "M14 3v5h5M6 3h8l5 5v13H6zM9 13h6M9 17h6";
pub const I_THEME: &str = "M12 3a9 9 0 1 0 9 9 7 7 0 0 1-9-9z";
pub const I_SHIELD: &str = "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z";
pub const I_CHEV: &str = "M9 6l6 6-6 6";
pub const I_CHEV_DOWN: &str = "M6 9l6 6 6-6";
pub const I_DISMISS: &str = "M7 7l10 10M17 7L7 17";
pub const I_ALERT_HIGH: &str = "M8.5 3h7L21 8.5v7L15.5 21h-7L3 15.5v-7zM12 8v5M12 16.5h.01";
pub const I_ALERT_MED: &str = "M12 4L2.8 20h18.4zM12 10v4M12 17h.01";
pub const I_ARROW_DOWN: &str = "M12 5v14M6 13l6 6 6-6";

pub const I_POWER: &str = "M12 3v9M6.3 6.8a8 8 0 1 0 11.4 0";
pub const I_GAME_P: &str = "M7 8h10a4 4 0 0 1 4 4v3a2 2 0 0 1-3.6 1.2L16 14H8l-1.4 2.2A2 2 0 0 1 3 15v-3a4 4 0 0 1 4-4zM8 11v3M6.5 12.5h3M15.5 12h.01M17.5 13.5h.01";
pub const I_WORK_P: &str = "M4 8h16v11H4zM9 8V5h6v3M4 13h16";
pub const I_TW_SHELL: &str = "M4 5h16v14H4zM4 9h16M9 13l6 4M15 13l-6 4";
pub const I_TW_UPD: &str = "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5";
pub const I_TW_DEF: &str = "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6zM9 12l2 2 4-4";
pub const I_TW_NTFS: &str = "M4 6h16v12H4zM8 10h8M8 14h5";
pub const I_TW_ADS: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM5.6 5.6l12.8 12.8";
pub const I_TRAY: &str = "M4 17h16M7 13l5-5 5 5";
pub const I_NET: &str = "M2 12h20M12 2a15 15 0 0 1 0 20M12 2a15 15 0 0 0 0 20M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z";
pub const I_THERMO: &str = "M14 14.8V4a2 2 0 1 0-4 0v10.8a4 4 0 1 0 4 0z";
pub const I_CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heat_is_nothing_under_a_tenth_and_grows_to_its_cap() {
        assert!(heat(0.0, 15.0, 36.0).is_none());
        assert!(heat(1.0, 15.0, 36.0).is_none(), "under a tenth of the scale");
        let (a, b) = (heat(3.0, 15.0, 36.0).expect("a tint"), heat(12.0, 15.0, 36.0).expect("a tint"));
        assert!(b.a > a.a);
        assert!((heat(500.0, 15.0, 36.0).expect("a tint").a - 0.36).abs() < 1e-6, "the cap");
        // the mockup's own number: 12.2 of 15, cap 36 -> 4 + .813 * 36 = 33
        assert!((heat(12.2, 15.0, 36.0).expect("a tint").a - 0.33).abs() < 1e-6);
    }
}
