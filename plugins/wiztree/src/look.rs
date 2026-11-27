//! The colours, icons and number formats of the mockup (DiskMap.html), written as its CSS and its script write them.
use crate::model::Category;
use winrust_ui::gfx::Rgba;
use winrust_common::style;

// `[data-app=dk]` and `[data-theme=light]`: dark value, light value
pub const WIN: Rgba = Rgba::pair(0x18191e, 0xfbfbfc);
pub const BAR: Rgba = Rgba::pair(0x1e1f25, 0xf1f2f5);
pub const SIDE: Rgba = Rgba::pair(0x1b1c21, 0xf5f6f8);
pub const MAP: Rgba = Rgba::pair(0x111216, 0xeceef2);
pub const WELL: Rgba = Rgba::pair(0x15161b, 0xe8eaee);
pub const MENU: Rgba = Rgba::pair(0x1e1f26, 0xfcfcfd).alpha(0.98);
pub const RED: Rgba = Rgba::pair(0xff8a8f, 0xc8333a);
pub const GREEN: Rgba = Rgba::pair(0x7fe0ad, 0x18804d);
pub const BLOCK_ORANGE: u32 = 0xffb877;
pub const FOLDER_YELLOW: u32 = 0xf6c453;

/// `--redBg`: the red plate behind a danger button and the dialog's icon.
#[must_use]
pub fn red_bg() -> Rgba {
    Rgba::css(229, 72, 77, if style::light() { 0.12 } else { 0.16 })
}

/// The colour of a category in the legend and on the map (the same in both modes).
#[must_use]
pub fn cat_color(c: Category) -> u32 {
    [0x8b7cf6, 0xec6f97, 0xf08a3c, 0x4f8ff0, 0x22b5c8, 0x2fb67c, 0x7c869c, 0xb59a63][c as usize]
}

#[must_use]
pub fn rgb(hex: u32) -> Rgba {
    Rgba::new(((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, 1.0)
}

/// What a tile's label is drawn in: `--tileT`, and `--tileT2` for the second line and the sub-tiles' names.
#[must_use]
pub fn tile_text() -> Rgba {
    if style::light() { Rgba::new(22.0 / 255.0, 24.0 / 255.0, 29.0 / 255.0, 1.0) } else { Rgba::pure_white(1.0) }
}

#[must_use]
pub fn tile_text2() -> Rgba {
    if style::light() { Rgba::new(22.0 / 255.0, 24.0 / 255.0, 29.0 / 255.0, 0.66) } else { Rgba::pure_white(0.74) }
}

/// What the tile's colour is mixed into: `--tileMix`.
fn tile_mix() -> [f32; 3] {
    if style::light() { [1.0, 1.0, 1.0] } else { [21.0 / 255.0, 22.0 / 255.0, 27.0 / 255.0] }
}

// ---------------------------------------------------------------------------------------------------------------------
// `color-mix(in oklab, ..)`

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        from_linear(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        from_linear(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s),
        from_linear(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

/// `color-mix(in oklab, a pa%, b)` for colours in sRGB.
#[must_use]
pub fn oklab_mix(a: [f32; 3], b: [f32; 3], pa: f32) -> [f32; 3] {
    let (x, y) = (oklab(a), oklab(b));
    from_oklab([x[0] * pa + y[0] * (1.0 - pa), x[1] * pa + y[1] * (1.0 - pa), x[2] * pa + y[2] * (1.0 - pa)])
}

fn parts(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

fn from_parts(c: [f32; 3]) -> Rgba {
    Rgba::new(c[0], c[1], c[2], 1.0)
}

/// A tile of category `c` whose size is `frac` of the biggest's (the mockup's `tileBg`): the category's colour, the more of it the
/// bigger the tile is, mixed into the map's tile colour.
#[must_use]
pub fn tile_bg(c: Category, frac: f32) -> Rgba {
    let k = frac.clamp(0.0, 1.0).sqrt();
    let pc = if style::light() { 34.0 + 34.0 * k } else { 30.0 + 40.0 * k };
    from_parts(oklab_mix(parts(cat_color(c)), tile_mix(), pc / 100.0))
}

/// The tile of the details panel: the category's colour 80 % in black (or the accent's 70 %).
#[must_use]
pub fn dark_of(base: Rgba, pct: f32) -> Rgba {
    let t = base.themed();
    from_parts(oklab_mix([t.r, t.g, t.b], [0.0, 0.0, 0.0], pct))
}

/// `filter: brightness(k)`.
#[must_use]
pub fn brighter(c: Rgba, k: f32) -> Rgba {
    Rgba::new((c.r * k).min(1.0), (c.g * k).min(1.0), (c.b * k).min(1.0), c.a)
}

// ---------------------------------------------------------------------------------------------------------------------
// numbers as the mockup writes them

/// `human` of the mockup: 1.4 KB is 2 KB, 12 MB, 7.3 GB, 421 GB, 1.25 TB.
#[must_use]
pub fn human(b: u64) -> String {
    const KB: f64 = 1024.0;
    let v = b as f64;
    if v >= KB * KB * KB * KB {
        format!("{:.2} TB", v / (KB * KB * KB * KB))
    } else if v >= KB * KB * KB {
        let gb = v / (KB * KB * KB);
        if v >= 100.0 * KB * KB * KB { format!("{gb:.0} GB") } else { format!("{gb:.1} GB") }
    } else if v >= KB * KB {
        format!("{:.0} MB", v / (KB * KB))
    } else {
        format!("{} KB", (v / KB).round().max(1.0))
    }
}

/// `pctOf`: 49%, 5.0%, <1%, 0%.
#[must_use]
pub fn pct(x: u64, total: u64) -> String {
    let v = if total == 0 { 0.0 } else { x as f64 / total as f64 * 100.0 };
    if v >= 10.0 {
        format!("{}%", v.round())
    } else if v >= 1.0 {
        format!("{v:.1}%")
    } else if v > 0.0 {
        "<1%".into()
    } else {
        "0%".into()
    }
}

/// `toLocaleString('en-US')`: 1,207,760.
#[must_use]
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// icons (24 x 24, as the mockup draws them)

pub const ICON_FOLDER: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6z";
pub const ICON_FOLDER_FILL: &str = "M2.5 6.5a2 2 0 0 1 2-2h4.2l2 2h8.8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2z";
pub const ICON_FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4";
pub const ICON_DRIVE: &str = "M4 7h16v10H4zM7.5 12h.01M11 12h6";
pub const ICON_USB: &str = "M8 3h8v6H8zM6 9h12v12H6zM10 5.5h.01M14 5.5h.01";
pub const ICON_RESCAN: &str = "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5";
pub const ICON_UP: &str = "M12 19V5M6 11l6-6 6 6";
pub const ICON_CHEVRON: &str = "M9 6l6 6-6 6";
pub const ICON_MAP: &str = "M4 4h9v10H4zM15 4h5v6h-5zM15 12h5v8h-5zM4 16h9v4H4z";
pub const ICON_LIST: &str = "M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01";
pub const ICON_OPEN: &str = "M9 6l6 6-6 6";
pub const ICON_COPY: &str = "M8 8h11v11H8zM5 15V5h10";
pub const ICON_TRASH: &str = "M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13M10 11v5M14 11v5";
pub const ICON_CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
pub const ICON_SHIELD: &str = "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z";
pub const ICON_SHIELD_BANG: &str = "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM12 9v4M12 16h.01";
pub const ICON_USB_BANG: &str = "M8 3h8v6H8zM6 9h12v12H6zM12 13v3M12 18.5h.01";
pub const ICON_MIN: &str = "M6 12h12";
pub const ICON_MAX: &str = "M6.5 6.5h11v11h-11z";
pub const ICON_RESTORE: &str = "M8.5 8.5h9v9h-9zM6 15V6h9";
pub const ICON_CLOSE: &str = "M7 7l10 10M17 7L7 17";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_as_the_mockup_writes_them() {
        let gb = |x: f64| (x * 1_073_741_824.0).round() as u64;
        assert_eq!(human(gb(98.4)), "98.4 GB");
        assert_eq!(human(gb(618.0)), "618 GB");
        assert_eq!(human(gb(9.8)), "9.8 GB");
        assert_eq!(human(gb(1.25 * 1024.0)), "1.25 TB");
        assert_eq!(human(37 * 1_048_576), "37 MB");
        assert_eq!(human(4000), "4 KB");
        assert_eq!(human(1), "1 KB");
        assert_eq!(pct(49, 100), "49%");
        assert_eq!(pct(5, 100), "5.0%");
        assert_eq!(pct(1, 1000), "<1%");
        assert_eq!(pct(0, 100), "0%");
        assert_eq!(pct(5, 0), "0%");
        assert_eq!(grouped(1_207_760), "1,207,760");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(0), "0");
    }

    #[test]
    fn color_mix_in_oklab_is_the_end_points_and_in_between() {
        let (a, b) = ([0.5, 0.2, 0.8], [0.1, 0.9, 0.3]);
        let near = |x: [f32; 3], y: [f32; 3]| x.iter().zip(y).all(|(p, q)| (p - q).abs() < 2e-3);
        assert!(near(oklab_mix(a, b, 1.0), a) && near(oklab_mix(a, b, 0.0), b));
        // the mockup's Games tile at the top of the dark map: 70 % of #f08a3c in #15161b is a warm brown, not the mean of the sRGB values
        let g = oklab_mix(parts(0xf08a3c), parts(0x15161b), 0.7);
        assert!(g[0] > g[1] && g[1] > g[2], "{g:?}");
    }
}
