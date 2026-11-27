//! The colours and the icons of the design (Explorer.v2.html): the window's palette written beside its light values, and the icons of
//! files, folders, drives and places as the mockup draws them (a back and a front plate, and a glyph on the front one).
use winrust_ui::gfx::{Align, Canvas, Font, Rect, Rgba};

// `[data-app=ex]` and its `[data-theme=light]`
pub const WIN: Rgba = Rgba::pair(0x1b1c22, 0xfbfbfc);
pub const BAR: Rgba = Rgba::pair(0x202128, 0xf0f1f4);
pub const SIDE: Rgba = Rgba::pair(0x1d1e24, 0xf5f6f8);
pub const WELL: Rgba = Rgba::pair(0x16171c, 0xe8eaee);
pub const AMBER: Rgba = Rgba::pair(0xffb877, 0xa95b06);
/// The orange of the bar of a drive that is nearly full (the same in both modes).
pub const FULL_FILL: Rgba = rgb(0xffa348);
pub const T1: Rgba = winrust_ui::widgets::look::T1;
pub const T2: Rgba = winrust_ui::widgets::look::T2;
pub const T3: Rgba = winrust_ui::widgets::look::T3;
pub const T4: Rgba = winrust_ui::widgets::look::T4;

#[must_use]
pub fn accent() -> Rgba {
    winrust_ui::widgets::look::accent()
}

/// The ink at `a` (`rgba(var(--ink), a)`).
#[must_use]
pub const fn ink(a: f32) -> Rgba {
    Rgba::white(a)
}

pub const BODY: Font = Font::new(13.0, 400);
pub const BODY_M: Font = Font::new(13.0, 500);
pub const SMALL: Font = Font::new(12.0, 400);

const fn rgb(v: u32) -> Rgba {
    Rgba::new(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, 1.0)
}

/// The bar of a drive: the accent, and orange from 90 % full.
#[must_use]
pub fn bar_colour(used_pct: u32) -> Rgba {
    if used_pct >= 90 { FULL_FILL } else { accent() }
}

/// What the free space is written in: amber from 90 % full, `normal` otherwise.
#[must_use]
pub fn free_colour(used_pct: u32, normal: Rgba) -> Rgba {
    if used_pct >= 90 { AMBER } else { normal }
}

// ---------------------------------------------------------------------------------------------------------------------
// icons: back plate, front plate, glyph on the front one (24 x 24, as the mockup's `iconOf` writes them)

/// The folders of the user's profile that have a glyph on their plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Ssd,
    Hdd,
    Usb,
    Net,
}

/// What an icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Folder(Option<Special>),
    /// A file by its extension as [`ext_of`] reads it ("" for one the design has no colours for).
    File(&'static str),
    Drive(DriveKind),
    Pc,
    Home,
}

pub struct Plates {
    pub back: &'static str,
    pub front: &'static str,
    pub c1: Rgba,
    pub c2: Rgba,
    pub glyph: &'static str,
    pub gc: Rgba,
    /// The extension's badge on the front plate of a file ("" for none).
    pub ext: &'static str,
}

const FOLDER_BACK: &str = "M2.5 6.5a2 2 0 0 1 2-2h4.2l2 2h8.8a2 2 0 0 1 2 2V10h-19z";
const FOLDER_FRONT: &str = "M2.5 9.5h19v8a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2z";
const FILE_BACK: &str = "M14 2.5v5h5z";
const FILE_FRONT: &str = "M5.5 2.5H14l5 5v14H5.5z";
const DRIVE_BACK: &str = "M3 13.5h18v5.5H3z";
const DRIVE_FRONT: &str = "M3 13.5l2.6-8h12.8l2.6 8zM16.5 16.2h1.5";

fn special_glyph(s: Special) -> &'static str {
    match s {
        Special::Desktop => "M8 10h8v5H8zM10 17h4",
        Special::Documents => "M9.5 11.5h5M9.5 14h5M9.5 16.5h3",
        Special::Downloads => "M12 10.5v6M9.5 14l2.5 2.5 2.5-2.5",
        Special::Music => "M11 16.5V11l4-1v5M11 16.5a1.2 1.2 0 1 1-2.4 0 1.2 1.2 0 0 1 2.4 0zM15 15a1.2 1.2 0 1 1-2.4 0 1.2 1.2 0 0 1 2.4 0z",
        Special::Pictures => "M8.5 11h7v5.5h-7zM8.5 15l2-2 1.5 1.5 1-1 2.5 2.5",
        Special::Videos => "M8.5 11h5v5h-5zM13.5 12.5l2-1v4l-2-1",
    }
}

fn file_glyph(ext: &str) -> &'static str {
    match ext {
        "exe" => "M8 8.8h8v6.2H8zM8 10.6h8",
        "lnk" | "url" => "M9.2 14.6l5.2-5.2M10.8 9.4h3.6V13",
        "dll" => "M8.5 9h2.7v1a1.1 1.1 0 1 0 2.2 0V9h2.1v6H8.5z",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "bmp" | "ico" => "M8 15l2.8-3.5 2 2.3 1.4-1.6L16.5 15zM10 9.6a1.1 1.1 0 1 0 0 .01",
        "pdf" => "M8 15c1.5-1.8 2.8-4 3.2-6 .3-1.4-1-1.4-1 0 .2 2.4 2.6 4.8 5 5.1 1.2.2 1.2-.9 0-.9-2.1 0-5.1 1-7.2 1.8z",
        "zip" | "7z" | "rar" => "M12 3.5v1.3M12 6v1.3M12 8.5v1.3M10.7 11h2.6v3.6h-2.6z",
        "xlsx" | "csv" => "M8 9h8v6H8zM8 12h8M12 9v6",
        "fig" => "M10 8.8h2v2.2h-2a1.1 1.1 0 0 1 0-2.2zM12 8.8h2a1.1 1.1 0 0 1 0 2.2h-2zM10 11h2v2.2h-2a1.1 1.1 0 0 1 0-2.2zM14 12.1a1.1 1.1 0 1 1 0 .01M10 13.2h2v1.1a1.1 1.1 0 1 1-2 0z",
        "md" => "M8 14.6V9.2l2 2.4 2-2.4v5.4M14.5 9.2v5.2M13.2 13.2l1.3 1.4 1.3-1.4",
        "toml" | "ini" | "cfg" | "conf" => "M12 9.8a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM12 8.2v.9M12 14.6v.9M8.9 11.8h.9M14.2 11.8h.9",
        "json" | "js" | "ts" => "M10.4 8.8c-1 0-1.2.6-1.2 1.5s-.5 1.5-1.2 1.5c.7 0 1.2.6 1.2 1.5s.2 1.5 1.2 1.5M13.6 8.8c1 0 1.2.6 1.2 1.5s.5 1.5 1.2 1.5c-.7 0-1.2.6-1.2 1.5s-.2 1.5-1.2 1.5",
        "html" | "rs" | "c" | "cpp" | "h" | "py" | "go" | "css" => "M10 9l-2 2.8 2 2.8M14 9l2 2.8-2 2.8",
        "tmp" => "M12 8.6a3.3 3.3 0 1 0 0 6.6 3.3 3.3 0 0 0 0-6.6zM12 10.2v1.8l1.2.8",
        "iso" => "M12 8.6a3.3 3.3 0 1 0 0 6.6 3.3 3.3 0 0 0 0-6.6zM12 11.6v.4",
        "vhdx" | "vhd" => "M8 9.5h8v5H8zM14 12h.01",
        "ps1" | "bat" | "cmd" | "sh" => "M8.5 9.5l2.5 2.3-2.5 2.3M12.5 14.6h3",
        _ => "M8.5 9.5h7M8.5 12h7M8.5 14.5h4.5",
    }
}

/// The two colours of a file's icon: (back and glyph, front).
fn file_colours(ext: &str) -> (Rgba, Rgba) {
    let (a, b) = match ext {
        "exe" | "lnk" | "url" => (0x3d6fd8, 0x8fb6ff),
        "dll" => (0x6b7187, 0xc9cdd8),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "bmp" | "ico" => (0x2f9a6c, 0x8fe0bd),
        "pdf" => (0xc43f45, 0xff9b9b),
        "zip" | "7z" | "rar" => (0xa07a2a, 0xecd08a),
        "docx" | "doc" => (0x2f5fc8, 0xa8c2f7),
        "xlsx" | "csv" => (0x1f8a4c, 0x8fdcae),
        "fig" => (0xa259ff, 0xd6b8ff),
        "toml" | "json" => (0xb9662a, 0xf2c29c),
        "js" | "ts" => (0xb39a12, 0xf3e28a),
        "html" | "rs" | "c" | "cpp" | "h" | "py" | "go" | "css" => (0xc8622a, 0xf4b48a),
        "tmp" => (0x6b7187, 0xc9cdd8),
        "iso" => (0x7a5bd6, 0xc9b8ff),
        "vhdx" | "vhd" => (0x4b5163, 0xb9bfcc),
        "ps1" | "bat" | "cmd" | "sh" => (0x2b5fc9, 0xa8c4ff),
        _ => (0x6b7187, 0xe6e8f0),
    };
    (rgb(a), rgb(b))
}

/// The plates of an icon.
#[must_use]
pub fn plates(kind: IconKind) -> Plates {
    match kind {
        IconKind::Folder(special) => Plates { back: FOLDER_BACK, front: FOLDER_FRONT, c1: rgb(0xd49a2a), c2: rgb(0xf6c453), glyph: special.map_or("", special_glyph), gc: rgb(0x9a6a12), ext: "" },
        IconKind::File(ext) => {
            let (c1, c2) = file_colours(ext);
            Plates { back: FILE_BACK, front: FILE_FRONT, c1, c2, glyph: file_glyph(ext), gc: c1, ext: badge(ext) }
        }
        IconKind::Drive(DriveKind::Ssd) => Plates { back: DRIVE_BACK, front: DRIVE_FRONT, c1: rgb(0x5a6074), c2: rgb(0x8f96aa), glyph: "M16.5 16.2h1.5", gc: rgb(0x7fe0ad), ext: "" },
        IconKind::Drive(DriveKind::Hdd) => Plates { back: DRIVE_BACK, front: DRIVE_FRONT, c1: rgb(0x4b5163), c2: rgb(0x7d8498), glyph: "M16.5 16.2h1.5", gc: rgb(0xffb877), ext: "" },
        IconKind::Drive(DriveKind::Usb) => Plates { back: "M9 2.5h6v5H9z", front: "M7 7h10v12.5a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2z", c1: rgb(0x8f96aa), c2: rgb(0x5a6074), glyph: "M10.5 4.5h.01M13.5 4.5h.01", gc: rgb(0x2a2d38), ext: "" },
        IconKind::Drive(DriveKind::Net) => Plates { back: "M3.5 3.5h17v10h-17z", front: "M3.5 15.5h17v4h-17zM11 13.5h2v2h-2z", c1: rgb(0x4a7fe0), c2: rgb(0x5a6074), glyph: "M6.5 17.5h.01", gc: rgb(0x7fe0ad), ext: "" },
        IconKind::Pc => Plates { back: "M3 4h18v12H3z", front: "M9 17.5h6v2H9zM6.5 19.5h11v1.5h-11z", c1: rgb(0x4a7fe0), c2: rgb(0x8f96aa), glyph: "", gc: Rgba::TRANSPARENT, ext: "" },
        IconKind::Home => Plates { back: "M4 11l8-7 8 7z", front: "M5.5 10h13v10h-13z", c1: rgb(0x4a7fe0), c2: rgb(0x8fb6ff), glyph: "M10 20v-5h4v5", gc: rgb(0x2f5fc8), ext: "" },
    }
}

/// The extension in capitals for the badge of a big icon (a static string per known extension keeps the icons free of allocation).
fn badge(ext: &str) -> &'static str {
    match ext {
        "exe" => "EXE",
        "lnk" => "LNK",
        "url" => "URL",
        "dll" => "DLL",
        "jpg" => "JPG",
        "jpeg" => "JPEG",
        "png" => "PNG",
        "gif" => "GIF",
        "webp" => "WEBP",
        "heic" => "HEIC",
        "bmp" => "BMP",
        "ico" => "ICO",
        "pdf" => "PDF",
        "zip" => "ZIP",
        "7z" => "7Z",
        "rar" => "RAR",
        "docx" => "DOCX",
        "doc" => "DOC",
        "xlsx" => "XLSX",
        "csv" => "CSV",
        "fig" => "FIG",
        "md" => "MD",
        "toml" => "TOML",
        "json" => "JSON",
        "js" => "JS",
        "ts" => "TS",
        "html" => "HTML",
        "rs" => "RS",
        "c" => "C",
        "cpp" => "CPP",
        "h" => "H",
        "py" => "PY",
        "go" => "GO",
        "css" => "CSS",
        "txt" => "TXT",
        "log" => "LOG",
        "tmp" => "TMP",
        "iso" => "ISO",
        "vhdx" => "VHDX",
        "vhd" => "VHD",
        "ps1" => "PS1",
        "bat" => "BAT",
        "cmd" => "CMD",
        "sh" => "SH",
        "ini" => "INI",
        "cfg" => "CFG",
        "conf" => "CONF",
        _ => "",
    }
}

const KNOWN: [&str; 49] = [
    "exe", "lnk", "url", "dll", "jpg", "jpeg", "png", "gif", "webp", "heic", "bmp", "ico", "pdf", "zip", "7z", "rar", "docx", "doc", "xlsx", "csv", "fig", "md", "toml", "json", "js", "ts", "html", "rs", "c", "cpp", "h", "py", "go",
    "css", "txt", "log", "tmp", "iso", "vhdx", "vhd", "ps1", "bat", "cmd", "sh", "ini", "cfg", "conf", "scr", "msi",
];

/// The extension of a file name as `IconKind::File` wants it: lower case, one the design knows ("" for none, and for the rest, which
/// share one plain document icon).
#[must_use]
pub fn ext_of(name: &str) -> &'static str {
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let Some((stem, e)) = file.rsplit_once('.') else { return "" };
    if stem.is_empty() {
        return "";
    }
    KNOWN.iter().find(|k| k.eq_ignore_ascii_case(e)).copied().unwrap_or("")
}

/// Draws an icon of `size` DIPs centred at `(cx, cy)`: the plates, then the glyph (`stroke` is the glyph's stroke in icon units: 1.6 at
/// 22 px, 1.25 at 56, 1.1 at 84). `badge` puts the extension on a file's front plate (the big icons).
pub fn icon(c: &mut Canvas<'_>, kind: IconKind, (cx, cy): (f32, f32), size: f32, stroke: f32, with_badge: bool) {
    let p = plates(kind);
    c.glyph(p.back, (cx, cy), size, p.c1);
    c.glyph(p.front, (cx, cy), size, p.c2);
    if !p.glyph.is_empty() {
        c.icon(p.glyph, (cx, cy), size, stroke, p.gc, 0.0);
    }
    if with_badge && !p.ext.is_empty() {
        let k = size / 56.0;
        let f = Font::new(7.0 * k, 700).tracking(0.04);
        let w = c.measure(p.ext, f) + 6.0 * k;
        let b = Rect::new(cx - w / 2.0, cy + size / 2.0 - 9.0 * k - 10.0 * k, w, 10.0 * k);
        c.fill(b, 2.0 * k, p.c1);
        c.text(p.ext, b, f, Rgba::pure_white(1.0), Align::Center);
    }
}
