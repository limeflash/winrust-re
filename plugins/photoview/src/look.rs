//! The viewer's own colours, as the mockup's CSS writes them (`[data-app=pv]` and `[data-theme=light]`): dark value, light value.
use winrust_ui::gfx::Rgba;

/// The window: `--win`.
pub const WIN: Rgba = Rgba::pair(0x16171b, 0xf7f7f9);
/// The caption, the filmstrip and the details panel: `--bar`.
pub const BAR: Rgba = Rgba::pair(0x1c1d22, 0xeef0f3);
/// Behind the picture: `--stage`.
pub const STAGE: Rgba = Rgba::pair(0x0c0d10, 0xe6e8ec);
/// A thumbnail while its picture is not there, the same in both modes.
pub const THUMB_BG: Rgba = Rgba::pair(0x24252c, 0x24252c);
/// The text of the zoom hint and the round arrows: always on a dark plate.
pub const ON_DARK: Rgba = Rgba::pair(0xeceef4, 0xeceef4);

/// The glyph of the window's tile and of the empty card's pictures.
pub const GLYPH: &str = "M4 18l5-6 4 4 3-3 4 5M15.5 8.5h.01";

/// Icons of the toolbar, as the mockup draws them (24 × 24, stroked).
pub const ICON_MINUS: &str = "M6 12h12";
pub const ICON_PLUS: &str = "M6 12h12M12 6v12";
pub const ICON_ROTATE: &str = "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7";
pub const ICON_INFO: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01";
pub const ICON_TRASH: &str = "M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13M10 11v5M14 11v5";
pub const ICON_WALL: &str = "M3 5h18v12H3zM8 21h8M12 17v4M3 14l5-4 4 3 3-2 6 4";
pub const ICON_COPY: &str = "M8 8h11v11H8zM5 15V5h10";
pub const ICON_LINK: &str = "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1";
pub const ICON_FOLDER: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6z";
pub const ICON_ROTATE_LEFT: &str = "M4 11a8 8 0 1 1 2.3 5.7M4 4v7h7";
pub const ICON_PREV: &str = "M15 6l-6 6 6 6";
pub const ICON_NEXT: &str = "M9 6l6 6-6 6";
pub const ICON_BAD: &str = "M4 5h16v14H4zM4 16l5-5 3 3M14 12l2-2 4 4M3 3l18 18";
pub const ICON_BAD_SMALL: &str = "M4 5h16v14H4zM3 3l18 18";
pub const ICON_DROP: &str = "M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19h14";
