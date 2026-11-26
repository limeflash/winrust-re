//! The bar's glyphs as SVG path data on a 24x24 grid (stroked with round caps and joins), copied from the Obsidian mockup.
//! The states the mockup does not have (no network, a cable, muted, a battery) are drawn in the same style.

/// Bar settings, 19 px, stroke 1.8.
pub const GEAR: &str = "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z";
/// Search, 19 px, stroke 1.9.
pub const SEARCH: &str = "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4";
/// Task view, 19 px, stroke 1.9.
pub const TASK_VIEW: &str = "M3.5 8.5h11v9h-11zM8 5.5h11v9";
/// Hidden icons chevron, 15 px, stroke 2.2.
pub const CHEVRON: &str = "M6 14l6-6 6 6";
/// Restart, 18 px, stroke 1.9.
pub const RESTART: &str = "M4 12a8 8 0 1 0 2.5-5.8M4 4v5h5";
/// Shut down, 18 px, stroke 1.9.
pub const POWER: &str = "M12 3v9M6.3 6.8a8 8 0 1 0 11.4 0";

/// Wi-Fi arcs of the quick settings button, 18 px, stroke 1.9: the three arcs and the dot (the signal strength lights some).
pub const WIFI_ARC_3: &str = "M3 9.5a13 13 0 0 1 18 0";
pub const WIFI_ARC_2: &str = "M6 13a8.5 8.5 0 0 1 12 0";
pub const WIFI_ARC_1: &str = "M9 16.5a4 4 0 0 1 6 0";
pub const WIFI_DOT: &str = "M12 19.5h.01";
/// Not connected: a slash over the arcs.
pub const WIFI_OFF: &str = "M4 4l16 16";
/// Connected without internet: an exclamation mark at the lower right, over a gap cut in the arcs.
pub const WIFI_WARN: &str = "M19 13v3.5M19 20h.01";
/// Ethernet: a plug with three pins.
pub const CABLE: &str = "M5 4h14v9a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3zM9 8v2M12 8v2M15 8v2M12 16v5";

/// Speaker body of the volume button.
pub const SPEAKER: &str = "M4 9.5h3.5L12 6v12l-4.5-3.5H4z";
pub const VOLUME_1: &str = "M15.5 9a4 4 0 0 1 0 6";
pub const VOLUME_2: &str = "M18 6.5a8 8 0 0 1 0 11";
/// Muted: a cross after the speaker.
pub const VOLUME_MUTED: &str = "M16 9.5l5 5M21 9.5l-5 5";

/// Battery outline with the terminal nub; the charge is filled in separately (see `bar::battery`).
pub const BATTERY: &str = "M4 7.5h12a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2zM20.5 10.5v3";
/// The Start button's four squares (x, y, size) on the 24 grid, corner radius 1.8.
pub const START_SQUARES: [(f32, f32); 4] = [(3.5, 3.5), (13.0, 3.5), (3.5, 13.0), (13.0, 13.0)];
pub const START_SQUARE: f32 = 7.5;
pub const START_RADIUS: f32 = 1.8;

/// Do not disturb: the moon in the bell's place on the clock and in the centre, filled or stroked.
pub const MOON: &str = "M20 14.5A8 8 0 1 1 9.5 4 6.5 6.5 0 0 0 20 14.5z";
