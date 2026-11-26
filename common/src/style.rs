//! The UI style shared by the WinRust programs: %ProgramData%\WinRust\style.txt holds its name (the topbar gear menu writes it).
//! Programs re-read it at most once a second and repaint at once on the broadcast `set_style` sends (`msg()`).
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, PostMessageW, RegisterWindowMessageW};

/// The shape of the bar. Every style shares one palette (`BASE` plus the accent); they differ only in geometry.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Style {
    /// Edge to edge along the screen side, square.
    #[default]
    Classic,
    /// Inset 8 px from the screen edges, rounded, with a shadow.
    Floating,
    /// A centred pill as wide as its buttons.
    Dock,
}

pub const ALL: [Style; 3] = [Style::Classic, Style::Floating, Style::Dock];

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Style::Classic => "classic",
            Style::Floating => "floating",
            Style::Dock => "dock",
        }
    }
    /// Unknown or empty text (no file yet, a hand edit, a style of an older version) is the classic style.
    pub fn parse(s: &str) -> Style {
        ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or_default()
    }
}

/// The accent colour (%ProgramData%\WinRust\accent.txt). A hand-picked accent in colors.txt wins over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Accent {
    #[default]
    Blue,
    Teal,
    Lilac,
    Amber,
    Rose,
}

impl Accent {
    pub const ALL: [Accent; 5] = [Accent::Blue, Accent::Teal, Accent::Lilac, Accent::Amber, Accent::Rose];

    pub fn name(self) -> &'static str {
        match self {
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Lilac => "lilac",
            Accent::Amber => "amber",
            Accent::Rose => "rose",
        }
    }
    /// Unknown or empty text is blue.
    pub fn parse(s: &str) -> Accent {
        Accent::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or_default()
    }
    pub const fn color(self) -> u32 {
        match self {
            Accent::Blue => rgb(0x62, 0xa0, 0xea),
            Accent::Teal => rgb(0x5b, 0xc8, 0xaf),
            Accent::Lilac => rgb(0xc0, 0xa5, 0xf5),
            Accent::Amber => rgb(0xff, 0xa3, 0x48),
            Accent::Rose => rgb(0xf0, 0x8a, 0xa6),
        }
    }
}

/// How much the UI moves (%ProgramData%\WinRust\anim.txt): full, short (80 ms, no delays) or none.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Anim {
    #[default]
    Full,
    Reduced,
    Off,
}

impl Anim {
    pub const ALL: [Anim; 3] = [Anim::Full, Anim::Reduced, Anim::Off];

    pub fn name(self) -> &'static str {
        match self {
            Anim::Full => "full",
            Anim::Reduced => "reduced",
            Anim::Off => "off",
        }
    }
    /// Unknown or empty text is full.
    pub fn parse(s: &str) -> Anim {
        Anim::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or_default()
    }
}

/// One word in %ProgramData%\WinRust\<file>, cached as an index and read again at most once a second (like the style).
struct Choice {
    file: &'static str,
    cur: AtomicU8, // MAX = not read yet
    at: AtomicU64, // when the file was last read, ms since the epoch
}

impl Choice {
    const fn new(file: &'static str) -> Choice {
        Choice { file, cur: AtomicU8::new(u8::MAX), at: AtomicU64::new(0) }
    }
    fn path(&self) -> Option<PathBuf> {
        std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join(self.file))
    }
    /// The index `parse` makes of the file's text.
    fn get(&self, parse: impl Fn(&str) -> usize) -> usize {
        // one load, used as it is: a second load could see the MAX that another thread's `forget` stored in between
        let cur = self.cur.load(Relaxed);
        if cur != u8::MAX && now_ms().saturating_sub(self.at.load(Relaxed)) <= 1000 {
            return cur as usize;
        }
        let text = self.path().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
        let v = parse(&text);
        self.cur.store(v as u8, Relaxed);
        self.at.store(now_ms(), Relaxed);
        v
    }
    /// Switches this process, writes the choice and tells the other WinRust windows to repaint.
    fn set(&self, index: usize, name: &str) {
        self.cur.store(index as u8, Relaxed);
        self.at.store(now_ms(), Relaxed);
        if let Some(f) = self.path() {
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(f, name);
        }
        unsafe { PostMessageW(HWND_BROADCAST, msg(), 0, 0) };
    }
    /// The next `get` reads the file.
    fn forget(&self) {
        self.cur.store(u8::MAX, Relaxed);
    }
}

static ACCENT: Choice = Choice::new("accent.txt");
static ANIM: Choice = Choice::new("anim.txt");

pub fn accent() -> Accent {
    Accent::ALL[ACCENT.get(|s| Accent::parse(s) as usize)]
}

/// Also drops a hand-picked accent colour, so the choice shows.
pub fn set_accent(a: Accent) {
    set_custom_color(4, None);
    ACCENT.set(a as usize, a.name());
}

pub fn anim() -> Anim {
    Anim::ALL[ANIM.get(|s| Anim::parse(s) as usize)]
}

pub fn set_anim(a: Anim) {
    ANIM.set(a as usize, a.name());
}

static CUR: AtomicU8 = AtomicU8::new(u8::MAX); // index into ALL; MAX = not read yet
static READ_AT: AtomicU64 = AtomicU64::new(0); // when the file was last read, ms since the epoch

fn file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("style.txt"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn store(s: Style) {
    CUR.store(ALL.iter().position(|&v| v == s).unwrap_or(0) as u8, Relaxed);
    READ_AT.store(now_ms(), Relaxed);
}

/// Reads the file again now (on the "style changed" broadcast).
pub fn reload() {
    // WINRUST_STYLE overrides the file for this process only (to look at a style without switching every window)
    let name = std::env::var("WINRUST_STYLE").ok().or_else(|| file().and_then(|f| std::fs::read_to_string(f).ok()));
    store(Style::parse(&name.unwrap_or_default()));
    ACCENT.forget();
    ANIM.forget();
}

/// The current style. The file is read again at most once a second, so a program that missed the broadcast still follows.
pub fn style() -> Style {
    if CUR.load(Relaxed) == u8::MAX || now_ms().saturating_sub(READ_AT.load(Relaxed)) > 1000 {
        reload();
    }
    ALL[CUR.load(Relaxed) as usize % ALL.len()]
}

/// Switches this process, writes the choice and tells the other WinRust windows to repaint in it.
pub fn set_style(s: Style) {
    store(s);
    if let Some(f) = file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, s.name());
    }
    unsafe { PostMessageW(HWND_BROADCAST, msg(), 0, 0) };
}

fn autohide_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("autohide.txt"))
}

/// The bar hides when the pointer is away and comes back at the screen edge (off unless the file says 1). Re-read once a second.
pub fn autohide() -> bool {
    if now_ms().saturating_sub(HIDE_AT.load(Relaxed)) > 1000 {
        HIDE.store(autohide_file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|s| s.trim() == "1") as u8, Relaxed);
        HIDE_AT.store(now_ms(), Relaxed);
    }
    HIDE.load(Relaxed) == 1
}

static HIDE: AtomicU8 = AtomicU8::new(0);
static HIDE_AT: AtomicU64 = AtomicU64::new(0);

pub fn set_autohide(on: bool) {
    HIDE.store(on as u8, Relaxed);
    HIDE_AT.store(now_ms(), Relaxed);
    if let Some(f) = autohide_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
}

fn left_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("align-left.txt"))
}

static LEFT: AtomicU8 = AtomicU8::new(0);
static LEFT_AT: AtomicU64 = AtomicU64::new(0);

/// Start and the pinned apps sit at the bar's left end, like Windows 10 (off unless the file says 1). Re-read once a second.
pub fn align_left() -> bool {
    if now_ms().saturating_sub(LEFT_AT.load(Relaxed)) > 1000 {
        LEFT.store(left_file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|s| s.trim() == "1") as u8, Relaxed);
        LEFT_AT.store(now_ms(), Relaxed);
    }
    LEFT.load(Relaxed) == 1
}

pub fn set_align_left(on: bool) {
    LEFT.store(on as u8, Relaxed);
    LEFT_AT.store(now_ms(), Relaxed);
    if let Some(f) = left_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
}

/// Which screen edge the topbar sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Bottom,
    Left,
    Right,
}

impl Side {
    pub fn parse(s: &str) -> Side {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" => Side::Left,
            "right" => Side::Right,
            _ => Side::Bottom,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Side::Bottom => "bottom",
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

fn side_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("side.txt"))
}

static SIDE: AtomicU8 = AtomicU8::new(0);
static SIDE_AT: AtomicU64 = AtomicU64::new(0);

fn side_code(s: Side) -> u8 {
    match s {
        Side::Bottom => 0,
        Side::Left => 1,
        Side::Right => 2,
    }
}

/// The bar's edge (bottom unless %ProgramData%\WinRust\side.txt says left or right). Re-read once a second.
pub fn side() -> Side {
    if now_ms().saturating_sub(SIDE_AT.load(Relaxed)) > 1000 {
        let s = side_file().and_then(|f| std::fs::read_to_string(f).ok()).map_or(Side::Bottom, |s| Side::parse(&s));
        SIDE.store(side_code(s), Relaxed);
        SIDE_AT.store(now_ms(), Relaxed);
    }
    match SIDE.load(Relaxed) {
        1 => Side::Left,
        2 => Side::Right,
        _ => Side::Bottom,
    }
}

pub fn set_side(s: Side) {
    SIDE.store(side_code(s), Relaxed);
    SIDE_AT.store(now_ms(), Relaxed);
    if let Some(f) = side_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, s.name());
    }
}

/// The broadcast message a window gets when the style changed: call `reload()` and repaint.
pub fn msg() -> u32 {
    static M: AtomicU32 = AtomicU32::new(0);
    match M.load(Relaxed) {
        0 => {
            let n: Vec<u16> = "WinRustStyleChanged\0".encode_utf16().collect();
            let m = unsafe { RegisterWindowMessageW(n.as_ptr()) };
            M.store(m, Relaxed);
            m
        }
        m => m,
    }
}

/// Colours of one style, as COLORREF (0x00BBGGRR) like GDI wants them.
#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: u32,
    pub line: u32,
    pub hover: u32,
    pub active: u32,
    pub accent: u32,
    pub text: u32,
    pub dim: u32,
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// The Obsidian palette every style shares: near-black ink, cool white text, a soft blue accent.
pub const BASE: Pal = Pal {
    bg: rgb(0x14, 0x15, 0x1b),
    line: rgb(0x2a, 0x2b, 0x33),
    hover: rgb(0x26, 0x27, 0x2f),
    active: rgb(0x2f, 0x30, 0x38),
    accent: Accent::Blue.color(),
    text: rgb(0xec, 0xee, 0xf4),
    dim: rgb(0xa3, 0xa9, 0xb8),
};

/// `BASE` in each accent, in the order of `Accent::ALL`.
static TINTED: [Pal; 5] = {
    let mut p = [BASE; 5];
    let mut i = 0;
    while i < 5 {
        p[i].accent = Accent::ALL[i].color();
        i += 1;
    }
    p
};

/// Status colours shared by every style (Apple's dark-mode system green, yellow, red).
pub const GOOD: u32 = rgb(48, 209, 88);
pub const WARN: u32 = rgb(255, 214, 10);
pub const BAD: u32 = rgb(255, 69, 58);

pub fn pal() -> &'static Pal {
    let base = &TINTED[accent() as usize];
    let Some(o) = custom_colors() else { return base };
    // one leaked copy per (accent, overrides): a handful at most, and pal() has to hand out a reference
    static CACHE: std::sync::Mutex<Vec<(usize, [u32; 7], &'static Pal)>> = std::sync::Mutex::new(Vec::new());
    let key = base as *const Pal as usize;
    let mut v = CACHE.lock().unwrap();
    if let Some(e) = v.iter().rev().find(|e| e.0 == key && e.1 == o) { // the newest first: a colour being dragged in the editor is the last one
        return e.2;
    }
    let p: &'static Pal = Box::leak(Box::new(custom_pal(base, o)));
    v.push((key, o, p));
    p
}

/// The style's palette with the user's colours on top. Lines, hover and pressed colours that were not set follow the
/// background and text colours, so changing just the background still gives a consistent bar.
fn custom_pal(base: &Pal, o: [u32; 7]) -> Pal {
    let get = |i: usize, d: u32| if o[i] == u32::MAX { d } else { o[i] };
    let bg = get(0, base.bg);
    let text = get(5, base.text);
    let derive = o[0] != u32::MAX || o[5] != u32::MAX;
    Pal {
        bg,
        line: get(1, if derive { mix(bg, text, 0.12) } else { base.line }),
        hover: get(2, if derive { mix(bg, text, 0.07) } else { base.hover }),
        active: get(3, if derive { mix(bg, text, 0.12) } else { base.active }),
        accent: get(4, base.accent),
        text,
        dim: get(6, if derive { mix(bg, text, 0.55) } else { base.dim }),
    }
}

/// Names of the colours the user can set, in the order of `Pal`'s fields (bg, line, hover, active, accent, text, dim).
pub const COLOR_NAMES: [&str; 7] = ["bg", "line", "hover", "active", "accent", "text", "dim"];

fn colors_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("colors.txt"))
}

static COLORS: std::sync::Mutex<[u32; 7]> = std::sync::Mutex::new([u32::MAX; 7]); // COLORREF, u32::MAX = the style's own
static COLORS_AT: AtomicU64 = AtomicU64::new(0);

fn read_colors() -> [u32; 7] {
    parse_colors(&colors_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default())
}

/// `bg=RRGGBB` pairs, one per line or separated by `;` (colors.txt and the templates); unknown names and junk are skipped.
fn parse_colors(text: &str) -> [u32; 7] {
    let mut o = [u32::MAX; 7];
    for l in text.split(['\n', ';']) {
        if let Some((k, v)) = l.split_once('=')
            && let (Some(i), Ok(v)) = (COLOR_NAMES.iter().position(|n| *n == k.trim()), u32::from_str_radix(v.trim(), 16))
            && v <= 0xFFFFFF { o[i] = rgb((v >> 16) as u8, (v >> 8) as u8, v as u8); }
    }
    o
}

fn colors_text(c: &[u32; 7], sep: &str) -> String {
    c.iter().enumerate().filter(|(_, v)| **v != u32::MAX).map(|(i, v)| format!("{}={:02X}{:02X}{:02X}{sep}", COLOR_NAMES[i], v & 255, (v >> 8) & 255, (v >> 16) & 255)).collect()
}

/// The user's colours (one per `COLOR_NAMES`, u32::MAX = not set), None when none is set. Re-read once a second.
pub fn custom_colors() -> Option<[u32; 7]> {
    let mut c = COLORS.lock().unwrap();
    if now_ms().saturating_sub(COLORS_AT.load(Relaxed)) > 1000 {
        *c = read_colors();
        COLORS_AT.store(now_ms(), Relaxed);
    }
    c.iter().any(|v| *v != u32::MAX).then_some(*c)
}

/// Sets colour `i` (COLORREF; None = back to the style's own) and tells the other WinRust windows to repaint.
pub fn set_custom_color(i: usize, c: Option<u32>) {
    let mut cur = read_colors();
    cur[i] = c.unwrap_or(u32::MAX);
    save_colors(cur);
}

/// Back to the style's own colours and opacity.
pub fn reset_custom() {
    save_colors([u32::MAX; 7]);
    set_bar_opacity(85);
}

fn save_colors(c: [u32; 7]) {
    let text = colors_text(&c, "\n");
    if let Some(f) = colors_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = if text.is_empty() { std::fs::remove_file(f) } else { std::fs::write(f, text) };
    }
    *COLORS.lock().unwrap() = c;
    COLORS_AT.store(now_ms(), Relaxed);
    unsafe { PostMessageW(HWND_BROADCAST, msg(), 0, 0) };
}

fn opacity_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("bar-opacity.txt"))
}

static OPACITY: AtomicU8 = AtomicU8::new(85);
static OPACITY_AT: AtomicU64 = AtomicU64::new(0);

fn nearest_opacity(v: u8) -> u8 { [100u8, 85, 70, 55, 40].into_iter().min_by_key(|n| n.abs_diff(v)).unwrap() }

/// How opaque the topbar is, percent. Re-read once a second.
pub fn bar_opacity() -> u8 {
    if now_ms().saturating_sub(OPACITY_AT.load(Relaxed)) > 1000 {
        let v = opacity_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| s.trim().parse::<u8>().ok()).map_or(85, nearest_opacity);
        OPACITY.store(v, Relaxed);
        OPACITY_AT.store(now_ms(), Relaxed);
    }
    OPACITY.load(Relaxed)
}

pub fn set_bar_opacity(v: u8) {
    let v = nearest_opacity(v);
    OPACITY.store(v, Relaxed);
    OPACITY_AT.store(now_ms(), Relaxed);
    if let Some(f) = opacity_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, v.to_string());
    }
}

/// A saved look (a "template"): the style, the user's colours and the bar's opacity. One per line of
/// %ProgramData%\WinRust\themes.txt: `name<TAB>style<TAB>opacity<TAB>bg=RRGGBB;text=RRGGBB;`.
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    pub name: String,
    pub style: Style,
    pub colors: [u32; 7],
    pub opacity: u8,
    pub accent: Accent,
}

impl Theme {
    fn line(&self) -> String {
        format!("{}\t{}\t{}\t{}\t{}", self.name.replace(['\t', '\r', '\n'], " "), self.style.name(), self.opacity, colors_text(&self.colors, ";"), self.accent.name())
    }
    fn parse(l: &str) -> Option<Theme> {
        let mut f = l.trim_end_matches('\r').splitn(5, '\t');
        let name = f.next().filter(|n| !n.trim().is_empty())?.to_string();
        let style = Style::parse(f.next()?);
        let opacity = f.next()?.trim().parse::<u8>().ok().map(nearest_opacity)?;
        Some(Theme { name, style, colors: parse_colors(f.next().unwrap_or("")), opacity, accent: Accent::parse(f.next().unwrap_or("")) })
    }
    /// What is on screen now.
    pub fn current(name: &str) -> Theme {
        Theme { name: name.into(), style: style(), colors: read_colors(), opacity: bar_opacity(), accent: accent() }
    }
    /// Switches every WinRust window to this look.
    pub fn apply(&self) {
        set_bar_opacity(self.opacity);
        set_accent(self.accent);
        save_colors(self.colors);
        set_style(self.style);
    }
}

fn themes_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("themes.txt"))
}

/// The saved templates, oldest first.
pub fn themes() -> Vec<Theme> {
    themes_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default().lines().filter_map(Theme::parse).collect()
}

pub fn save_themes(t: &[Theme]) {
    if let Some(f) = themes_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, t.iter().map(|t| t.line() + "\n").collect::<String>());
    }
}

/// Everything the gear menu sets, back to how a fresh install looks (the saved templates stay).
pub fn reset_all() {
    set_side(Side::Bottom);
    set_bar_size(1);
    set_autohide(false);
    set_align_left(false);
    set_alttab_per_monitor(false);
    Theme { name: String::new(), style: Style::Classic, colors: [u32::MAX; 7], opacity: 85, accent: Accent::Blue }.apply();
}

/// A raised surface (a card, a tile): pure white on the light style, a step up from the background on the dark ones.
pub fn card() -> u32 {
    mix(pal().bg, pal().text, 0.06)
}

/// Text and glyphs drawn on an accent fill.
pub fn on_accent() -> u32 {
    rgb(11, 18, 32)
}

/// `a` moved toward `b` by `t` (0..1), per channel.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        (x + (y - x) * t.clamp(0.0, 1.0)).round() as u32
    };
    ch(0) | ch(8) << 8 | ch(16) << 16
}

fn font_file(name: &str) -> bool {
    std::env::var_os("WINDIR").is_some_and(|w| PathBuf::from(w).join("Fonts").join(name).is_file())
}

/// The proportional UI font: Windows 11's Segoe UI Variable where it is installed, else Segoe UI.
pub fn sans_face() -> &'static str {
    static F: std::sync::OnceLock<&str> = std::sync::OnceLock::new();
    F.get_or_init(|| if font_file("SegUIVar.ttf") { "Segoe UI Variable Text" } else { "Segoe UI" })
}

/// The UI font of the current style.
pub fn ui_face() -> &'static str {
    sans_face()
}

/// The icon font: Segoe Fluent Icons (Windows 11, same code points) or Segoe MDL2 Assets.
pub fn icon_face() -> &'static str {
    static F: std::sync::OnceLock<&str> = std::sync::OnceLock::new();
    F.get_or_init(|| if font_file("SegoeIcons.ttf") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_always_answers_with_a_parsed_index() {
        // `forget` (another thread's reload) leaves the cache at MAX: the answer must come from the parse, never from MAX
        let c = Choice::new("winrust-test-no-such-file.txt");
        assert_eq!(c.get(|_| 2), 2);
        c.forget();
        assert_eq!(c.get(|_| 1), 1);
        assert_eq!(c.get(|_| 0), 1, "a fresh answer is kept for the second");
    }

    #[test]
    fn names_round_trip_and_junk_is_normal() {
        for s in ALL {
            assert_eq!(Style::parse(s.name()), s);
        }
        assert_eq!(Style::parse(" Tiling\r\n"), Style::Classic);
        assert_eq!(Style::parse(""), Style::Classic);
        assert_eq!(Style::parse("neon"), Style::Classic);
    }

    #[test]
    fn mix_blends() {
        assert_eq!(mix(rgb(0, 0, 0), rgb(200, 100, 50), 0.5), rgb(100, 50, 25));
    }

    #[test]
    fn theme_line_round_trips() {
        let mut c = [u32::MAX; 7];
        c[0] = rgb(1, 2, 3);
        c[4] = rgb(250, 128, 0);
        let t = Theme { name: "Ночь\tтест".into(), style: Style::Floating, colors: c, opacity: 70, accent: Accent::Rose };
        let back = Theme::parse(&t.line()).unwrap();
        assert_eq!(back.name, "Ночь тест");
        assert_eq!((back.style, back.colors, back.opacity, back.accent), (Style::Floating, c, 70, Accent::Rose));
        assert_eq!(Theme::parse("a\tnormal\t5\t").unwrap().opacity, 40);
        assert!(Theme::parse("").is_none() && Theme::parse("x\tnormal").is_none());
        let mut one = [u32::MAX; 7];
        one[0] = rgb(1, 2, 3);
        assert_eq!(parse_colors("bg=010203\nzz=1;text=GG"), one);
    }

    #[test]
    fn side_parses_and_defaults_to_bottom() {
        assert_eq!(Side::parse(" Left
"), Side::Left);
        assert_eq!(Side::parse("right"), Side::Right);
        assert_eq!(Side::parse(""), Side::Bottom);
        assert_eq!(Side::parse("top"), Side::Bottom);
    }
}

fn size_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("bar-size.txt"))
}

static SIZE: AtomicU8 = AtomicU8::new(1);
static SIZE_AT: AtomicU64 = AtomicU64::new(0);

/// Height class of the topbar: 0 small, 1 normal (default), 2 large. Re-read once a second.
pub fn bar_size() -> u8 {
    if now_ms().saturating_sub(SIZE_AT.load(Relaxed)) > 1000 {
        let v = size_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| s.trim().parse::<u8>().ok()).filter(|v| *v <= 2).unwrap_or(1);
        SIZE.store(v, Relaxed);
        SIZE_AT.store(now_ms(), Relaxed);
    }
    SIZE.load(Relaxed)
}

pub fn set_bar_size(v: u8) {
    SIZE.store(v.min(2), Relaxed);
    SIZE_AT.store(now_ms(), Relaxed);
    if let Some(f) = size_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, v.min(2).to_string());
    }
}

fn mon_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("alttab-monitor.txt"))
}

static MON: AtomicU8 = AtomicU8::new(0);
static MON_AT: AtomicU64 = AtomicU64::new(0);

/// Alt+Tab lists only the windows of the monitor the active window is on (off unless the file says 1). Re-read once a second.
pub fn alttab_per_monitor() -> bool {
    if now_ms().saturating_sub(MON_AT.load(Relaxed)) > 1000 {
        MON.store(mon_file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|s| s.trim() == "1") as u8, Relaxed);
        MON_AT.store(now_ms(), Relaxed);
    }
    MON.load(Relaxed) == 1
}

pub fn set_alttab_per_monitor(on: bool) {
    MON.store(on as u8, Relaxed);
    MON_AT.store(now_ms(), Relaxed);
    if let Some(f) = mon_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
}
