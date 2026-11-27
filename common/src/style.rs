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

/// Which windows get an app button on a bar, when there are several (%ProgramData%\WinRustpps-on.txt: `all` / `here`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AppsOn {
    /// Every window on every bar (Windows 11's default).
    #[default]
    All,
    /// A window only on the bar of the monitor it is on.
    Here,
}

impl AppsOn {
    pub const ALL: [AppsOn; 2] = [AppsOn::All, AppsOn::Here];

    pub fn name(self) -> &'static str {
        match self {
            AppsOn::All => "all",
            AppsOn::Here => "here",
        }
    }
    /// Unknown or empty text is `All`.
    pub fn parse(s: &str) -> AppsOn {
        AppsOn::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or_default()
    }
}

/// What the bars on the other monitors hold (%ProgramData%\WinRust\other-bars.txt: `win11` / `same`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OtherBars {
    /// Like Windows 11: Start, search, Task view, the app buttons and the clock; the tray, language, quick settings and power
    /// buttons stay on the main bar.
    #[default]
    Win11,
    /// The same as the main bar.
    Same,
}

impl OtherBars {
    pub const ALL: [OtherBars; 2] = [OtherBars::Win11, OtherBars::Same];

    pub fn name(self) -> &'static str {
        match self {
            OtherBars::Win11 => "win11",
            OtherBars::Same => "same",
        }
    }
    /// Unknown or empty text is `Win11`.
    pub fn parse(s: &str) -> OtherBars {
        OtherBars::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or_default()
    }
}

static ACCENT: Choice = Choice::new("accent.txt");
static ALL_BARS: Choice = Choice::new("all-bars.txt");
static APPS_ON: Choice = Choice::new("apps-on.txt");
static OTHER_BARS: Choice = Choice::new("other-bars.txt");

/// A bar on every monitor, not just the primary one (on unless the file says `off`).
pub fn bar_on_all_displays() -> bool {
    ALL_BARS.get(|s| usize::from(!s.trim().eq_ignore_ascii_case("off"))) == 1
}

pub fn set_bar_on_all_displays(on: bool) {
    ALL_BARS.set(usize::from(on), if on { "on" } else { "off" });
}

pub fn apps_on() -> AppsOn {
    AppsOn::ALL[APPS_ON.get(|s| AppsOn::parse(s) as usize)]
}

pub fn set_apps_on(a: AppsOn) {
    APPS_ON.set(a as usize, a.name());
}

pub fn other_bars() -> OtherBars {
    OtherBars::ALL[OTHER_BARS.get(|s| OtherBars::parse(s) as usize)]
}

pub fn set_other_bars(o: OtherBars) {
    OTHER_BARS.set(o as usize, o.name());
}

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
    MODE.forget();
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

/// Light or dark: one flag per process, dark until a program sets it. The topbar follows Windows or the user's pick
/// (`follow_system` at start and on the "ImmersiveColorSet" `WM_SETTINGCHANGE`); `pal()` and the colour tokens below read it,
/// so one switch repaints everything.
static LIGHT: AtomicU8 = AtomicU8::new(0);

pub fn light() -> bool {
    LIGHT.load(Relaxed) == 1
}

/// Sets the mode of this process; true when it changed.
pub fn set_light(on: bool) -> bool {
    LIGHT.swap(on as u8, Relaxed) != on as u8
}

pub const PERSONALIZE: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// What `SystemUsesLightTheme` says: light only when it is 1 (a missing value keeps the dark bar the shell always had).
pub fn light_from(dword: Option<u32>) -> bool {
    dword == Some(1)
}

/// The bar's Appearance row (%ProgramData%\WinRust\mode.txt): `auto` follows Windows (the default, as the bar always did),
/// `light` and `dark` keep the bar in that mode whatever Windows says, and leave Windows' own setting alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ModePref {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ModePref {
    pub const ALL: [ModePref; 3] = [ModePref::Light, ModePref::Dark, ModePref::Auto];

    pub fn name(self) -> &'static str {
        match self {
            ModePref::Auto => "auto",
            ModePref::Light => "light",
            ModePref::Dark => "dark",
        }
    }
    /// Unknown or empty text (no file yet, a hand edit) is `Auto`.
    pub fn parse(s: &str) -> ModePref {
        ModePref::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or_default()
    }
}

static MODE: Choice = Choice::new("mode.txt");

pub fn mode_pref() -> ModePref {
    match MODE.get(|s| ModePref::parse(s) as usize) {
        1 => ModePref::Light,
        2 => ModePref::Dark,
        _ => ModePref::Auto,
    }
}

/// Whether the bar is light, given what the user picked and what Windows says (`SystemUsesLightTheme`).
pub fn light_for(pref: ModePref, system: Option<u32>) -> bool {
    match pref {
        ModePref::Auto => light_from(system),
        ModePref::Light => true,
        ModePref::Dark => false,
    }
}

/// Reads the pick and Windows' choice into this process (once at start, and again on the "ImmersiveColorSet" broadcast and
/// when the pick changes); true when the mode changed.
pub fn follow_system() -> bool {
    set_light(light_for(mode_pref(), crate::reg::get_dword(PERSONALIZE, "SystemUsesLightTheme")))
}

/// The Appearance row: writes the pick, tells the other WinRust windows, and applies it here; true when the mode changed.
pub fn set_mode_pref(p: ModePref) -> bool {
    MODE.set(p as usize, p.name());
    follow_system()
}

/// Switches Windows after this process: writes `SystemUsesLightTheme` and `AppsUseLightTheme` (apps follow, like the system
/// switch) and broadcasts "ImmersiveColorSet" from a thread of its own (the broadcast waits for every window). The bar itself
/// follows only while its pick is `auto`.
pub fn set_system_light(on: bool) {
    crate::reg::set_dword(PERSONALIZE, "SystemUsesLightTheme", on as u32);
    crate::reg::set_dword(PERSONALIZE, "AppsUseLightTheme", on as u32);
    follow_system();
    std::thread::spawn(|| {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
        let what: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
        // SAFETY: `what` outlives the call, which waits (at most 200 ms per window) for the receivers.
        unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 200, std::ptr::null_mut()) };
    });
}

/// The "Dark style" tile, consistent with the Appearance row: while the bar follows Windows it is Windows' own switch (system
/// and apps, as before); once the user picked Light or Dark it flips that pick (the bar only, Windows is left alone).
pub fn toggle_dark_style() {
    match mode_pref() {
        ModePref::Auto => set_system_light(!light()),
        _ => _ = set_mode_pref(if light() { ModePref::Dark } else { ModePref::Light }),
    }
}

/// True when the `WM_SETTINGCHANGE` a window got says "ImmersiveColorSet" (`lparam` is a wide string, or null).
///
/// # Safety
/// `lparam` is the message's own: null, or a NUL-terminated UTF-16 string valid for the call.
pub unsafe fn is_color_change(lparam: isize) -> bool {
    let p = lparam as *const u16;
    let want = "ImmersiveColorSet";
    // SAFETY: the string ends with NUL, and the loop stops at the first difference, so nothing past that NUL is read.
    !p.is_null() && unsafe { want.encode_utf16().enumerate().all(|(i, w)| *p.add(i) == w) && *p.add(want.len()) == 0 }
}

/// The colour tokens of the mockup (`--ink`, `--t1`.., `--s16_17_24`.., `--h0b0d14`..): every literal dark colour of the design
/// became a variable named after it, and light mode redefines the variable. Here a colour built from a dark literal
/// (`Rgba::css`, `Rgba::hex`) remembers its token, and the renderer swaps in the light value while `light()` is on.
/// (dark RRGGBB, light RRGGBB); entry 0 is `--ink`, the white of hover fills and rings (only `Rgba::white` is it), entry 1 the
/// white that the design turned into `--t1` (selected labels: only `Rgba::strong` is it).
pub const TOKENS: &[(u32, u32)] = &[
    (0xffffff, 0x14161e),
    (0xffffff, 0x1b1d23),
    // text: --t1, --t1b, --t2, --t3, --t4, --t1c; then the near twins the mockup folded into --t3, --t1b and --t4
    (0xeceef4, 0x1b1d23), (0xc9cdd8, 0x343844), (0xa3a9b8, 0x4a4f5c), (0x8a90a2, 0x6a7080), (0x6f7486, 0x8d93a1), (0xe6e8f0, 0xd8dbe3),
    (0x9aa0b0, 0x6a7080), (0xc4c8d2, 0x343844), (0x7d8394, 0x8d93a1),
    // surfaces the design wrote as rgba(r,g,b,a): --s12_13_18, --s16_17_24, --s27_28_34, --s20_21_28, --s30_31_38, --s12_13_17,
    // --s24_25_31, --s20_21_27, --s18_19_26, --s10_11_16, --s5_7_12
    (0x0c0d12, 0xf5f6f8), (0x101118, 0xf3f4f6), (0x1b1c22, 0xeeeff1), (0x14151c, 0xf1f2f4), (0x1e1f26, 0xedeef0), (0x0c0d11, 0xf5f6f8),
    (0x18191f, 0xeff0f2), (0x14151b, 0xf1f2f4), (0x12131a, 0xf2f3f5), (0x0a0b10, 0xf6f7f9), (0x05070c, 0xeceef4),
    // surfaces the design wrote as #rrggbb: --h0b0d14, --h1d1e24, --h26272f, --h2a2b33, ...
    (0x0b0d14, 0xf5f6f8), (0x1d1e24, 0xedeef0), (0x26272f, 0xe9eaec), (0x2a2b33, 0xe7e8ea), (0x313239, 0xe4e5e7), (0x222329, 0xebecee),
    (0x23252e, 0xeaebed), (0x17181e, 0xf0f1f3), (0x212229, 0xebecee), (0x2f3038, 0xe5e6e8), (0x16171c, 0xf1f2f4), (0x2c2d34, 0xe6e7e9),
    (0x25262c, 0xeaebed), (0x22252d, 0xeaebed), (0x0c0d10, 0xf5f6f8), (0x15161b, 0xf1f2f4),
];

/// The token (1-based; 0 = none) of the dark literal `r,g,b`. `const`: colours are built in constants. White is never found
/// here: it is the ink or the strong text only when `Rgba::white` or `Rgba::strong` says so.
pub const fn token(r: u8, g: u8, b: u8) -> u8 {
    let v = (r as u32) << 16 | (g as u32) << 8 | b as u32;
    let mut i = 2;
    while i < TOKENS.len() {
        if TOKENS[i].0 == v {
            return i as u8 + 1;
        }
        i += 1;
    }
    0
}

/// The token `--ink`, and the white that is `--t1` in light mode.
pub const INK_TOKEN: u8 = 1;
pub const STRONG_TOKEN: u8 = 2;

/// A token's value in the current mode, as 0xRRGGBB.
pub fn token_rgb(tok: u8) -> u32 {
    let t = TOKENS[tok as usize - 1];
    if light() { t.1 } else { t.0 }
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

/// The light palette (the mockup's `BASE` in light mode): the same accent on paper.
pub const BASE_LIGHT: Pal = Pal {
    bg: rgb(0xf3, 0xf4, 0xf7),
    line: rgb(0xd9, 0xdc, 0xe3),
    hover: rgb(0xe4, 0xe6, 0xec),
    active: rgb(0xd8, 0xdb, 0xe2),
    accent: Accent::Blue.color(),
    text: rgb(0x1b, 0x1d, 0x23),
    dim: rgb(0x4a, 0x4f, 0x5c),
};

/// A palette in each accent, in the order of `Accent::ALL`.
const fn tinted(base: Pal) -> [Pal; 5] {
    let mut p = [base; 5];
    let mut i = 0;
    while i < 5 {
        p[i].accent = Accent::ALL[i].color();
        i += 1;
    }
    p
}

static TINTED: [Pal; 5] = tinted(BASE);
static TINTED_LIGHT: [Pal; 5] = tinted(BASE_LIGHT);

/// Status colours shared by every style (Apple's dark-mode system green, yellow, red).
pub const GOOD: u32 = rgb(48, 209, 88);
pub const WARN: u32 = rgb(255, 214, 10);
pub const BAD: u32 = rgb(255, 69, 58);

pub fn pal() -> &'static Pal {
    let base = &(if light() { &TINTED_LIGHT } else { &TINTED })[accent() as usize];
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
    set_bar_on_all_displays(true);
    set_apps_on(AppsOn::All);
    set_other_bars(OtherBars::Win11);
    Theme { name: String::new(), style: Style::Classic, colors: [u32::MAX; 7], opacity: 85, accent: Accent::Blue }.apply();
}

/// A raised surface (a card, a tile): pure white on the light style, a step up from the background on the dark ones.
pub fn card() -> u32 {
    mix(pal().bg, pal().text, 0.06)
}

/// Text and glyphs drawn on an accent fill: ink on the dark style, white on the light one (the design's `onA`).
pub fn on_accent() -> u32 {
    if light() { rgb(255, 255, 255) } else { rgb(11, 18, 32) }
}

/// sRGB (components 0..1) as Oklab `[L, a, b]`.
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

/// `color-mix(in oklab, a, b t)` of two sRGB colours (components 0..1): `t` of the way from `a` to `b`, clipped back into sRGB.
pub fn mix_oklab(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let enc = |v: f32| (if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }).clamp(0.0, 1.0);
    let (x, y) = (oklab(a), oklab(b));
    let [l0, a0, b0] = [x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t];
    let l = (l0 + 0.396_337_78 * a0 + 0.215_803_76 * b0).powi(3);
    let m = (l0 - 0.105_561_35 * a0 - 0.063_854_17 * b0).powi(3);
    let s = (l0 - 0.089_484_18 * a0 - 1.291_485_5 * b0).powi(3);
    [
        enc(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        enc(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s),
        enc(-0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

/// What the light style mixes into the accent to make it read on paper (`#0c1a3c`): 22 % for a fill (the design's `aF`), 38 % for
/// text and icons (`aT`); the dark style uses the accent itself. `rgb` is the accent, components 0..1.
pub fn accent_light(rgb: [f32; 3], text: bool) -> [f32; 3] {
    mix_oklab(rgb, [12.0 / 255.0, 26.0 / 255.0, 60.0 / 255.0], if text { 0.38 } else { 0.22 })
}

/// The surface tokens (`--s16_17_24`.. in `TOKENS`, 1-based 12..=21; the scrim `--s5_7_12` after them is not one) get
/// `--pAdd` = .1 more opacity in light mode: the glass of the light style is denser than the dark one.
pub const fn is_surface_token(tok: u8) -> bool {
    tok >= 12 && tok <= 21
}

/// A surface's alpha in light mode: `a + .1`, faded in below .3 so that a surface on its way out (alpha near 0) leaves no ghost.
pub fn lifted_alpha(a: f32) -> f32 {
    (a + 0.1 * (a / 0.3).min(1.0)).min(1.0)
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
    fn tokens_are_distinct_and_light_differs_from_dark() {
        for (i, a) in TOKENS.iter().enumerate() {
            assert!(a.0 != a.1 && a.0 != 0, "token {i} must change between modes");
            assert!(i < 2 || TOKENS[2..i].iter().all(|b| b.0 != a.0), "dark value {:06x} listed twice", a.0);
            let lum = |v: u32| (v >> 16 & 255) * 3 + (v >> 8 & 255) * 6 + (v & 255);
            assert!(i < 11 || lum(a.0) < lum(a.1), "token {i}: a dark surface becomes a light one");
        }
        assert_eq!(token(236, 238, 244), 3); // --t1
        assert_eq!(token(16, 17, 24) as usize, 12 + 1); // --s16_17_24
        assert_eq!(token(255, 255, 255), 0, "white is the ink only by `Rgba::white`");
        assert_eq!(token(0, 0, 0), 0, "shadows stay black");
        assert_eq!(token(11, 18, 32), 0, "text on an accent fill stays as it is");
    }

    #[test]
    fn mode_follows_the_system_value() {
        assert!(light_from(Some(1)) && !light_from(Some(0)) && !light_from(None) && !light_from(Some(7)));
        let was = light();
        assert_eq!(token_rgb(INK_TOKEN), if was { 0x14161e } else { 0xffffff });
        set_light(true);
        assert_eq!(token_rgb(STRONG_TOKEN), 0x1b1d23);
        assert_eq!(pal().text, BASE_LIGHT.text);
        assert_eq!(pal().bg, rgb(0xf3, 0xf4, 0xf7));
        set_light(false);
        assert_eq!(token_rgb(STRONG_TOKEN), 0xffffff);
        assert_eq!(pal().text, BASE.text);
        set_light(was);
    }

    #[test]
    fn the_appearance_pick_wins_over_windows_only_when_it_is_not_auto() {
        assert_eq!([ModePref::parse("light"), ModePref::parse(" DARK
"), ModePref::parse("auto"), ModePref::parse(""), ModePref::parse("sepia")], [ModePref::Light, ModePref::Dark, ModePref::Auto, ModePref::Auto, ModePref::Auto]);
        for (pref, system, want) in [
            (ModePref::Auto, Some(1), true),
            (ModePref::Auto, Some(0), false),
            (ModePref::Auto, None, false),
            (ModePref::Light, Some(0), true),
            (ModePref::Light, None, true),
            (ModePref::Dark, Some(1), false),
        ] {
            assert_eq!(light_for(pref, system), want, "{pref:?} with Windows saying {system:?}");
        }
        assert_eq!(ModePref::ALL.map(ModePref::name), ["light", "dark", "auto"]);
    }

    #[test]
    fn the_accent_on_paper_is_the_designs_colour_mix() {
        // read from the mockup's `color-mix(in oklab, #62a0ea 78%, #0c1a3c)` and 62% (computed style), Oklab L, a, b
        let blue = [0x62 as f32 / 255.0, 0xa0 as f32 / 255.0, 0xea as f32 / 255.0];
        for (text, want) in [(false, [0.592_699, -0.029_006_7, -0.110_193]), (true, [0.517_904, -0.024_348_6, -0.101_541])] {
            let got = oklab(accent_light(blue, text));
            // the result is rounded to 8 bits per channel by whoever draws it: a few thousandths of Oklab are that
            assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.004), "{got:?} for {want:?}");
        }
        assert_eq!(mix_oklab(blue, [0.0; 3], 0.0).map(|v| (v * 255.0).round()), [98.0, 160.0, 234.0]);
    }

    #[test]
    fn the_surface_tokens_are_the_glass_ones_and_light_makes_them_denser() {
        assert_eq!(TOKENS[11].0, 0x0c0d12, "--s12_13_18 is the first surface");
        assert_eq!(TOKENS[20].0, 0x0a0b10, "--s10_11_16 is the last one the lift covers");
        assert_eq!(TOKENS[21].0, 0x05070c, "--s5_7_12, the scrim, is not covered");
        assert!(is_surface_token(token(27, 28, 34)) && is_surface_token(token(10, 11, 16)));
        assert!(!is_surface_token(token(5, 7, 12)) && !is_surface_token(token(0x0b, 0x0d, 0x14)) && !is_surface_token(0));
        assert!((lifted_alpha(0.72) - 0.82).abs() < 1e-6 && (lifted_alpha(0.86) - 0.96).abs() < 1e-6);
        assert_eq!(lifted_alpha(0.94), 1.0);
        assert_eq!(lifted_alpha(0.0), 0.0, "a surface faded out stays gone");
        assert!(lifted_alpha(0.15) < 0.21 && lifted_alpha(0.15) > 0.15, "and fades in smoothly");
    }

    #[test]
    fn the_theme_value_round_trips_through_a_temp_key() {
        let key = format!(r"HKCU\Software\WinRustTest\light-{}", std::process::id());
        assert_eq!(crate::reg::get_dword(&key, "SystemUsesLightTheme"), None);
        assert!(crate::reg::set_dword(&key, "SystemUsesLightTheme", 1));
        assert!(light_from(crate::reg::get_dword(&key, "SystemUsesLightTheme")));
        assert!(crate::reg::set_dword(&key, "SystemUsesLightTheme", 0));
        assert!(!light_from(crate::reg::get_dword(&key, "SystemUsesLightTheme")));
        crate::reg::delete_key(&key);
        assert_eq!(crate::reg::get_dword(&key, "SystemUsesLightTheme"), None);
    }

    #[test]
    fn the_broadcast_text_is_recognised() {
        let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        // SAFETY: NUL-terminated buffers that live for the call
        unsafe {
            assert!(is_color_change(w("ImmersiveColorSet").as_ptr() as isize));
            assert!(!is_color_change(w("ImmersiveColorSetX").as_ptr() as isize) && !is_color_change(w("Environment").as_ptr() as isize));
            assert!(!is_color_change(w("Immersive").as_ptr() as isize) && !is_color_change(0));
        }
    }

    #[test]
    fn the_multi_monitor_choices_parse_and_default_to_windows_11() {
        assert_eq!(AppsOn::parse(" Here
"), AppsOn::Here);
        assert_eq!((AppsOn::parse("all"), AppsOn::parse(""), AppsOn::parse("nope")), (AppsOn::All, AppsOn::All, AppsOn::All));
        assert_eq!(OtherBars::parse("same"), OtherBars::Same);
        assert_eq!((OtherBars::parse("win11"), OtherBars::parse(""), OtherBars::parse("?")), (OtherBars::Win11, OtherBars::Win11, OtherBars::Win11));
        for a in AppsOn::ALL {
            assert_eq!(AppsOn::parse(a.name()), a);
        }
        for o in OtherBars::ALL {
            assert_eq!(OtherBars::parse(o.name()), o);
        }
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
