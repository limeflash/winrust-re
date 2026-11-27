//! The bar settings panel (the gear on the bar), drawn like the Obsidian mockup's `Bar settings`: a 392 px glass panel with
//! nine pages, each a column of the mockup's own pieces: the title tile, theme cards, accent dots, rows with a value and a
//! chevron, accordions with colour chips, segmented controls with a gliding thumb, switches, template rows, the update hero.
//!
//! The panel keeps to the corner above the bar and changes size with its page (`resized`); the page changes with the mockup's
//! `pgF`/`pgB` slide, the first page's blocks rise in one after another (`bsiIn`), and the panel itself opens and closes with the
//! `bsIn`/`bsOut` reveal out of the gear. Choices go straight to `winrust_common::style` (the bar and every other WinRust
//! window follow); what needs the bar itself (a repaint, tiling, a restart, a toast) goes through [`Act`].
//!
//! Layout is built as a list of [`Item`]s (geometry and content, no drawing), so the same numbers serve the size, the hit test
//! and the painting. Every number is the mockup's own, measured from its DOM.

use winrust_common::style::{self, Accent, Anim, Side, Style, Theme, pal};
use winrust_common::{Lang, lang};

use super::anim::{self, EASE, SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Picture, Rect, Rgba};
use super::icons;
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Inset, Look, Place, Rise, Shadow};

const W: f32 = 392.0;
const PAD: f32 = 8.0;
/// The width of what sits inside the panel's padding.
const CW: f32 = W - 2.0 * PAD;
/// Height of the main page: the panel opens on it (the reveal's clip is measured from it).
const MAIN_H: f32 = 656.0;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const DIM: Rgba = Rgba::css(138, 144, 162, 1.0);
const SOFT: Rgba = Rgba::css(163, 169, 184, 1.0);
const BAD: Rgba = Rgba::css(255, 138, 143, 1.0);
const RED: Rgba = Rgba::css(229, 72, 77, 1.0);
const GREEN: Rgba = Rgba::css(76, 195, 138, 1.0);
const GREEN_INK: Rgba = Rgba::css(127, 224, 173, 1.0);

const BODY: Font = Font::new(13.0, 400);
const MEDIUM: Font = Font::new(13.0, 500);
const SMALL: Font = Font::new(12.0, 400);
const LABEL: Font = Font::new(12.0, 500);

const PALETTE: &str = "M12 21a9 9 0 1 1 9-9c0 2-1.5 3-3 3h-2a2 2 0 0 0-1.5 3.3A2 2 0 0 1 12 21zM7.5 11h.01M10 7.5h.01M14.5 7.5h.01";
const GRID: &str = "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z";
const GLOBE: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3c2.5 2.6 3.7 5.6 3.7 9s-1.2 6.4-3.7 9c-2.5-2.6-3.7-5.6-3.7-9S9.5 5.6 12 3z";
const BAR_ICON: &str = "M3 5h18v14H3zM3 15h18";
const TILE_ICON: &str = "M4 4h7v16H4zM14 4h6v7h-6zM14 14h6v6h-6z";
const SLIDERS: &str = "M4 7h10M18 7h2M4 17h2M10 17h10M14 5v4M6 15v4";
const DOWNLOAD: &str = "M12 4v11M7 10.5l5 5 5-5M5 20h14";
const RESET: &str = "M3 12a9 9 0 1 0 3-6.7M3 4v5h5";
const CHEVRON: &str = "M9 6l6 6-6 6";
const BACK_ARROW: &str = "M15 5l-7 7 7 7";
const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const TRASH: &str = "M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13";
const PLUS: &str = "M12 5v14M5 12h14";
const CROSS: &str = "M7 7l10 10M17 7L7 17";
const FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h5";
const REFRESH: &str = "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5";
const SHIELD: &str = "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6zM9 12l2 2 4-4";
const SPIN_ARC: &str = "M12 3a9 9 0 0 1 9 9";
const NEWS_POWER: &str = "M12 3v9M6.3 6.8a8 8 0 1 0 11.4 0";
const NEWS_SLIDERS: &str = "M4 8h10M18 8h2M4 16h2M10 16h10M14 6v4M6 14v4";
const NEWS_CLIP: &str = "M9 4h6v3H9zM7.5 5.5H5.5V20h13V5.5H17";
const NEWS_FOLDER: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6z";

/// The twelve chips of a colour row.
const CHIPS: [u32; 12] = [0x14151b, 0x0b0d14, 0x1d1e24, 0x2a2b33, 0xeceef4, 0xffffff, 0xa3a9b8, 0x62a0ea, 0x5bc8af, 0xc0a5f5, 0xffa348, 0xf08aa6];
const OPACITIES: [u8; 5] = [100, 85, 70, 55, 40];

// ids of what can be hovered and clicked
const BACK: i32 = 1;
const THEME: i32 = 10;
const ACCENT: i32 = 20;
const GO_COLORS: i32 = 30;
const GO_TEMPLATES: i32 = 31;
const GO_LANG: i32 = 32;
const GO_BAR: i32 = 33;
const TILE: i32 = 34;
const GO_ADV: i32 = 35;
const GO_UPD: i32 = 36;
const GO_RESET: i32 = 37;
const COLOR_ROW: i32 = 100; // + row
const CHIP: i32 = 2000; // + row * 20 + chip (12 is "as in the theme")
const OPACITY: i32 = 300;
const RESET_COLORS: i32 = 320;
const TEMPLATE: i32 = 400; // + n
const SAVE: i32 = 450;
const DELETE: i32 = 451;
const LANG: i32 = 500;
const SIDE: i32 = 600;
const SIZE: i32 = 610;
const BEHAVIOR: i32 = 620;
const SHOW_ALL: i32 = 640;
const APPS_ON: i32 = 650; // + 0 all bars, 1 where the window is
const OTHER_BARS: i32 = 660; // + 0 like Windows 11, 1 the same as the main bar
const CANCEL: i32 = 700;
const YES: i32 = 701;
const ANIM: i32 = 820;
const ALT_SPACE: i32 = 840;
const OPEN_LOG: i32 = 830;
const RESTART: i32 = 831;
const CHECK_NOW: i32 = 900;
const AUTO_UPDATE: i32 = 901;
const REMOVE: i32 = 902;
const UNDO: i32 = 950;
const REMOVE_YES: i32 = 951;
/// Tween keys of things that are not hovered.
const PREVIEW: i32 = 9000;

/// The pages of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Colors,
    Templates,
    Lang,
    Bar,
    Reset,
    Adv,
    Upd,
    Rollback,
}

impl Page {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "main" => Self::Main,
            "colors" => Self::Colors,
            "templates" => Self::Templates,
            "lang" => Self::Lang,
            "bar" => Self::Bar,
            "reset" => Self::Reset,
            "adv" => Self::Adv,
            "upd" => Self::Upd,
            "rollback" => Self::Rollback,
            _ => return None,
        })
    }
}

/// What the panel asks the bar to do (always after the click is over, with the panel still open).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// The bar's look, size or side changed: repaint, move, and bring the panel along.
    Relook,
    /// The language changed.
    Language,
    /// Tile the open windows.
    Tile,
    OpenLog,
    Restart,
    /// Remove every plugin (the installer asks for its UAC prompt).
    Remove,
    /// Ask the updater for a new release.
    CheckUpdate,
    AutoUpdate(bool),
    /// Alt+Space opens the bar's search (true) or is left to the program in front (false).
    AltSpace(bool),
    /// A toast (the text of the mockup for that switch).
    Say(&'static str),
}

/// What the panel needs from the program around it.
pub struct Info {
    /// The installed WinRust version, `0.5.9`.
    pub version: String,
    /// Whether the updater's scheduled task is on; `None` when it is not there.
    pub auto: fn() -> Option<bool>,
    pub act: fn(Act),
    /// Whether the machine runs WinRust as its shell, has `rs-launcher` and the task manager installed (the advanced page
    /// shows them; the installer changes them).
    pub system: fn() -> System,
    /// The file of the desktop wallpaper (empty when there is none): it goes under the previews.
    pub wallpaper: fn() -> String,
}

/// What the advanced page can tell about the machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct System {
    pub shell: bool,
    pub tray: Option<bool>,
    /// Whether Alt+Space opens the bar's search (the switch on the advanced page; the bar keeps it in a file).
    pub search_key: bool,
    pub taskmgr_key: bool,
    pub clipboard: bool,
    pub wsearch_off: Option<bool>,
    pub taskbar_hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upd {
    Idle,
    /// Checking since (`None` until the next frame stamps it).
    Busy(Option<u64>),
    Done(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tone {
    Ink,
    Dim,
    Soft,
    Bad,
    Accent,
}

impl Tone {
    fn rgba(self, accent: Rgba) -> Rgba {
        match self {
            Self::Ink => INK,
            Self::Dim => DIM,
            Self::Soft => SOFT,
            Self::Bad => BAD,
            Self::Accent => accent,
        }
    }
}

#[derive(Clone, PartialEq)]
enum Val {
    None,
    Text(String),
    /// The three colours of the bar: background, accent, text.
    Palette,
    /// The green dot and the version.
    Version(String),
}

#[derive(Clone)]
struct Row {
    icon: &'static str,
    label: String,
    val: Val,
    chevron: bool,
    danger: bool,
    /// The tick of "Tile the windows".
    check: bool,
}

impl Row {
    fn new(icon: &'static str, label: &str) -> Self {
        Self { icon, label: label.to_string(), val: Val::None, chevron: false, danger: false, check: false }
    }

    fn val(self, val: Val) -> Self {
        Self { val, chevron: true, ..self }
    }

    fn chevron(self) -> Self {
        Self { chevron: true, ..self }
    }

    fn danger(self) -> Self {
        Self { danger: true, ..self }
    }
}

#[derive(Clone)]
enum K {
    Header,
    Text { s: String, font: Font, ink: Tone, align: Align },
    Rule(Rgba),
    Theme(Style),
    Dot(Accent),
    Row(Row),
    Back,
    ColorRow { i: usize, open: bool },
    Chip { i: usize, hex: u32 },
    ChipReset(&'static str),
    Seg { labels: Vec<String>, cur: usize, base: i32 },
    /// An option of a segmented control (hit area only).
    Hit,
    Switch { label: String, sub: Vec<String>, on: bool, off: bool },
    Preview,
    Template { name: String, sub: String, th: usize, del: bool },
    Note(String),
    Action { icon: &'static str, label: String, ink: Tone, h: f32 },
    LangRow { code: &'static str, name: &'static str, on: bool },
    Badge(&'static str),
    Button { label: String, red: bool },
    Hero { when: String },
    Card,
    UpdToggle { label: String, sub: Vec<String>, on: Option<bool> },
    Signed { label: String, sub: Vec<String> },
    NewsHead,
    News { icon: &'static str, title: String, sub: Vec<String>, first: bool },
    Remove(String),
    /// A group title of the advanced page.
    Group(String),
    LogRow { label: String, path: &'static str },
}

#[derive(Clone)]
struct Item {
    id: i32,
    r: Rect,
    /// The `.bsi` child of the main page this belongs to; the others do not rise.
    stage: u8,
    k: K,
}

struct B {
    items: Vec<Item>,
    stage: u8,
}

const NO_STAGE: u8 = 255;

impl B {
    fn put(&mut self, id: i32, r: Rect, k: K) {
        self.items.push(Item { id, r, stage: self.stage, k });
    }

    fn text(&mut self, r: Rect, s: impl Into<String>, font: Font, ink: Tone) {
        self.put(0, r, K::Text { s: s.into(), font, ink, align: Align::Start });
    }
}

type Measure<'a> = &'a mut dyn FnMut(&str, Font) -> f32;

/// Breaks `s` into lines no wider than `w`: at spaces, or between letters when a word is too long (Chinese, Japanese).
fn wrap(m: Measure<'_>, s: &str, f: Font, w: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_inclusive(' ') {
        if m(&(cur.clone() + word), f) <= w + 0.01 || cur.is_empty() && m(word, f) <= w {
            cur.push_str(word);
            continue;
        }
        if !cur.is_empty() {
            lines.push(cur.trim_end().to_string());
            cur.clear();
        }
        for ch in word.chars() {
            let mut next = cur.clone();
            next.push(ch);
            if m(next.trim_end(), f) > w && !cur.is_empty() {
                lines.push(cur.trim_end().to_string());
                cur = ch.to_string();
            } else {
                cur = next;
            }
        }
    }
    if !cur.is_empty() {
        lines.push(cur.trim_end().to_string());
    }
    lines
}

/// The panel's own sub-page animation and rise delays.
const RISE_DELAYS: [u32; 12] = [0, 30, 60, 85, 105, 0, 0, 0, 0, 0, 0, 0];

/// The wallpaper under the previews, made once for each of the three boxes it fills (`object-fit: cover`).
struct Walls {
    path: String,
    card: Option<Picture>,
    thumb: Option<Picture>,
    preview: Option<Picture>,
}

/// The boxes of the previews: a theme card, a template's thumbnail, the bar page's preview (width, height).
const CARD: (f32, f32) = ((CW - 8.0 - 16.0) / 3.0, 64.0);
const THUMB: (f32, f32) = (44.0, 28.0);
const PREVIEW_BOX: (f32, f32) = (364.0, 112.0);

pub struct SettingsPanel {
    info: Info,
    walls: Option<Walls>,
    page: Page,
    /// How the page arrived: `Some(true)` forward (`pgF`), `Some(false)` back (`pgB`), `None` it was the first one.
    dir: Option<bool>,
    /// When the page was mounted (`None` until the next frame stamps it).
    page_at: Option<u64>,
    open: Option<usize>,
    open_at: Option<u64>,
    del: bool,
    themes: Vec<Theme>,
    tiled: bool,
    upd: Upd,
    items: Vec<Item>,
    size: (f32, f32),
    sys: System,
    /// What the layout was made for: the language (texts), so a new language relays out.
    built: Option<(Page, Lang, bool)>,
}

impl SettingsPanel {
    #[must_use]
    pub fn new(info: Info, themes: Vec<Theme>, page: Page) -> Self {
        let sys = (info.system)();
        Self {
            info,
            walls: None,
            page,
            dir: None,
            page_at: None,
            open: None,
            open_at: None,
            del: false,
            themes,
            tiled: false,
            upd: Upd::Idle,
            items: Vec::new(),
            size: (W, MAIN_H),
            sys,
            built: None,
        }
    }

    /// For the renderer: the state a page shows in the mockup's screenshots.
    pub fn show(&mut self, open: Option<usize>, upd: Upd, tiled: bool, del: bool, dir: Option<bool>) {
        self.dir = dir;
        self.open = open;
        self.upd = upd;
        self.tiled = tiled;
        self.del = del;
    }

    fn go(&mut self, page: Page, forward: bool) {
        self.page = page;
        self.dir = Some(forward);
        self.page_at = None;
        self.open = None;
        self.del = false;
    }

    /// Loads the wallpaper for the three previews, unless the cached one is the file that is the wallpaper now.
    fn load_walls(&mut self, c: &mut Canvas<'_>) {
        let path = (self.info.wallpaper)();
        if self.walls.as_ref().is_some_and(|w| w.path == path) {
            return;
        }
        let mut load = |(w, h): (f32, f32)| if path.is_empty() { None } else { c.image(&path, w, h) };
        self.walls = Some(Walls { card: load(CARD), thumb: load(THUMB), preview: load(PREVIEW_BOX), path: path.clone() });
    }

    fn say(&self, cx: &mut Cx, text: &'static str) {
        let act = self.info.act;
        cx.run(move || {
            act(Act::Relook);
            act(Act::Say(text));
        });
    }

    fn relook(&self, cx: &mut Cx) {
        let act = self.info.act;
        cx.run(move || act(Act::Relook));
    }

    // ----------------------------------------------------------------------------------------------------------------
    // layout

    fn build(&self, m: Measure<'_>) -> (Vec<Item>, f32) {
        let mut b = B { items: Vec::new(), stage: NO_STAGE };
        let h = match self.page {
            Page::Main => self.main_page(&mut b),
            Page::Colors => self.colors_page(&mut b, m),
            Page::Templates => self.templates_page(&mut b, m),
            Page::Lang => self.lang_page(&mut b, m),
            Page::Bar => self.bar_page(&mut b, m),
            Page::Reset => self.confirm_page(&mut b, m, RESET, true),
            Page::Rollback => self.confirm_page(&mut b, m, TRASH, false),
            Page::Adv => self.adv_page(&mut b, m),
            Page::Upd => self.upd_page(&mut b, m),
        };
        (b.items, h)
    }

    /// The back button and the page title; returns the y below them.
    fn head(&self, b: &mut B, m: Measure<'_>, title: &str, sub: &str) -> f32 {
        let back = t!("Назад", "Back", ja: "戻る", pt: "Voltar", zh: "返回");
        let w = 6.0 + 16.0 + 8.0 + m(back, BODY).ceil() + 10.0;
        b.put(BACK, Rect::new(PAD, PAD, w, 36.0), K::Back);
        b.text(Rect::new(18.0, 48.0, 356.0, 19.0), title, Font::new(15.0, 600), Tone::Ink);
        b.text(Rect::new(18.0, 69.0, 356.0, 15.0), sub, SMALL, Tone::Dim);
        94.0
    }

    /// A 40 px row at `y`; returns the y under it.
    fn row(b: &mut B, id: i32, y: f32, row: Row) -> f32 {
        b.put(id, Rect::new(PAD, y, CW, 40.0), K::Row(row));
        y + 40.0
    }

    fn main_page(&self, b: &mut B) -> f32 {
        b.stage = 0;
        b.put(0, Rect::new(PAD, PAD, CW, 60.0), K::Header);
        b.stage = 1;
        let (look, theme) = (t!("Внешний вид", "Look", ja: "外観", pt: "Aparência", zh: "外观"), t!("Тема", "Theme", ja: "テーマ", pt: "Tema", zh: "主题"));
        b.text(Rect::new(18.0, 68.0, 340.0, 15.0), format!("{look} · {theme}"), LABEL, Tone::Dim);
        b.stage = 2;
        let cw = CARD.0;
        for (i, _) in style::ALL.iter().enumerate() {
            b.put(THEME + i as i32, Rect::new(12.0 + i as f32 * (cw + 8.0), 91.0, cw, 85.0), K::Theme(style::ALL[i]));
        }
        b.stage = 3;
        b.text(Rect::new(18.0, 195.0, 120.0, 16.0), t!("Акцент", "Accent", ja: "アクセント", pt: "Destaque", zh: "强调色"), MEDIUM, Tone::Ink);
        for (i, a) in Accent::ALL.iter().enumerate() {
            b.put(ACCENT + i as i32, Rect::new(220.0 + i as f32 * 32.0, 188.0, 30.0, 30.0), K::Dot(*a));
        }
        b.stage = 4;
        b.put(0, Rect::new(18.0, 230.0, 356.0, 1.0), K::Rule(Rgba::white(0.07)));
        b.stage = 5;
        let n = self.themes.len();
        let mut y = 237.0;
        y = Self::row(b, GO_COLORS, y, Row::new(PALETTE, t!("Цвета и прозрачность", "Colors and transparency", ja: "色と透明度", pt: "Cores e transparência", zh: "颜色与透明度")).val(Val::Palette)) + 1.0;
        y = Self::row(b, GO_TEMPLATES, y, Row::new(GRID, t!("Шаблоны", "Templates", ja: "テンプレート", pt: "Modelos", zh: "模板")).val(Val::Text(if n > 0 { n.to_string() } else { String::new() }))) + 1.0;
        y = Self::row(b, GO_LANG, y, Row::new(GLOBE, t!("Язык", "Language", ja: "言語", pt: "Idioma", zh: "语言")).val(Val::Text(lang().name().to_string())));
        b.stage = 6;
        b.text(Rect::new(18.0, y + 14.0, 340.0, 15.0), t!("Панель", "Bar", ja: "バー", pt: "Barra", zh: "任务栏"), LABEL, Tone::Dim);
        y += 35.0;
        b.stage = 7;
        let hint = format!("{} · {}", side_name(style::side()), size_name(style::bar_size()));
        y = Self::row(b, GO_BAR, y, Row::new(BAR_ICON, t!("Положение, размер, поведение", "Position, size, behavior", ja: "位置、サイズ、動作", pt: "Posição, tamanho, comportamento", zh: "位置、大小、行为")).val(Val::Text(hint))) + 1.0;
        let tile = Row { check: self.tiled, ..Row::new(TILE_ICON, t!("Разложить окна плиткой", "Tile the windows", ja: "ウィンドウを並べて表示", pt: "Lado a lado", zh: "平铺窗口")) };
        y = Self::row(b, TILE, y, tile);
        b.stage = 8;
        b.text(Rect::new(18.0, y + 14.0, 340.0, 15.0), "WinRust:RE", LABEL, Tone::Dim);
        y += 35.0;
        b.stage = 9;
        y = Self::row(b, GO_ADV, y, Row::new(SLIDERS, t!("Расширенные настройки", "Advanced settings")).chevron()) + 1.0;
        y = Self::row(b, GO_UPD, y, Row::new(DOWNLOAD, t!("Обновления", "Updates")).val(Val::Version(format!("v{}", self.info.version))));
        b.stage = 10;
        b.put(0, Rect::new(18.0, y + 8.0, 356.0, 1.0), K::Rule(Rgba::white(0.07)));
        y += 17.0;
        b.stage = 11;
        y = Self::row(b, GO_RESET, y, Row::new(RESET, t!("Сбросить всё по умолчанию…", "Reset everything to defaults…", ja: "すべて初期設定に戻す…", pt: "Restaurar tudo ao padrão…", zh: "将所有设置恢复默认…")).danger());
        y + PAD
    }

    fn colors_page(&self, b: &mut B, m: Measure<'_>) -> f32 {
        let mut y = self.head(b, m, t!("Цвета и прозрачность", "Colors and transparency", ja: "色と透明度", pt: "Cores e transparência", zh: "颜色与透明度"), t!("Поверх выбранной темы", "On top of the chosen theme", ja: "選択したテーマの上に適用", pt: "Sobre o tema escolhido", zh: "在所选主题之上"));
        b.text(Rect::new(18.0, y, 356.0, 15.0), t!("Цвета (точка справа: изменён вами)", "Colors (dot on the right: changed by you)", ja: "色 (右の点: 変更済み)", pt: "Cores (ponto à direita: alterada por você)", zh: "颜色（右侧圆点：已由你更改）"), LABEL, Tone::Dim);
        y += 21.0;
        for i in 0..7 {
            let open = self.open == Some(i);
            let h = if open { 108.0 } else { 40.0 };
            b.put(COLOR_ROW + i as i32, Rect::new(PAD, y, CW, 40.0), K::ColorRow { i, open });
            if open {
                for (k, &hex) in CHIPS.iter().take(11).enumerate() {
                    b.put(CHIP + (i * 20 + k) as i32, Rect::new(50.0 + k as f32 * 30.0, y + 42.0, 24.0, 24.0), K::Chip { i, hex: chip(hex) });
                }
                b.put(CHIP + (i * 20 + 11) as i32, Rect::new(50.0, y + 72.0, 24.0, 24.0), K::Chip { i, hex: chip(CHIPS[11]) });
                let theme = t!("Как в теме", "Theme", ja: "テーマ", pt: "Tema", zh: "主题");
                let w = m(theme, Font::new(11.0, 400)).ceil() + 16.0;
                b.put(CHIP + (i * 20 + 12) as i32, Rect::new(80.0, y + 72.0, w, 24.0), K::ChipReset(theme));
            }
            y += h + 1.0;
        }
        y -= 1.0;
        b.text(Rect::new(18.0, y + 14.0, 340.0, 15.0), t!("Непрозрачность панели", "Bar opacity", ja: "バーの不透明度", pt: "Opacidade da barra", zh: "任务栏不透明度"), LABEL, Tone::Dim);
        y += 37.0;
        let labels: Vec<String> = OPACITIES.iter().map(|v| format!("{v}%")).collect();
        let cur = OPACITIES.iter().position(|&v| v == style::bar_opacity()).unwrap_or(1);
        Self::seg(b, OPACITY, y, labels, cur);
        y += 42.0 + 4.0;
        y = Self::row(b, RESET_COLORS, y, Row::new(RESET, t!("Вернуть цвета темы", "Back to the theme's colors", ja: "テーマの色に戻す", pt: "Voltar às cores do tema", zh: "恢复主题颜色")).danger());
        y + PAD
    }

    /// A segmented control at `y` (364 wide, 34 high) and the hit areas of its options.
    fn seg(b: &mut B, base: i32, y: f32, labels: Vec<String>, cur: usize) {
        let n = labels.len();
        let r = Rect::new(14.0, y, 364.0, 34.0);
        let w = (r.w - 6.0) / n as f32;
        b.put(0, r, K::Seg { labels, cur, base });
        for k in 0..n {
            b.put(base + k as i32, Rect::new(r.x + 3.0 + k as f32 * w, y + 3.0, w, 28.0), K::Hit);
        }
    }

    fn templates_page(&self, b: &mut B, m: Measure<'_>) -> f32 {
        let (title, sub) = if self.del {
            (t!("Удалить шаблон", "Delete a template", ja: "テンプレートを削除", pt: "Excluir um modelo", zh: "删除模板"), t!("Нажмите на шаблон, чтобы удалить", "Click a template to delete it", ja: "削除するテンプレートをクリック", pt: "Clique em um modelo para excluí-lo", zh: "点击模板即可删除"))
        } else {
            (t!("Шаблоны", "Templates", ja: "テンプレート", pt: "Modelos", zh: "模板"), t!("Свои наборы на любое настроение", "Your own looks for any mood", ja: "気分に合わせたオリジナルの外観", pt: "Suas próprias aparências para qualquer humor", zh: "随心保存自己的外观"))
        };
        let mut y = self.head(b, m, title, sub);
        b.text(Rect::new(18.0, y, 356.0, 15.0), t!("Мои шаблоны", "My templates", ja: "マイ テンプレート", pt: "Meus modelos", zh: "我的模板"), LABEL, Tone::Dim);
        y += 21.0;
        if self.themes.is_empty() {
            b.put(0, Rect::new(14.0, y, 364.0, 56.0), K::Note(t!("Пока пусто: настройте вид и сохраните", "None yet: set up a look and save it", ja: "まだありません: 外観を設定して保存してください", pt: "Nenhum ainda: monte uma aparência e salve", zh: "暂无：设置好外观后保存").to_string()));
            y += 56.0 + 8.0;
        }
        for (i, th) in self.themes.iter().enumerate() {
            let sub = format!("{} · {}%", style_name(th.style), th.opacity);
            b.put(TEMPLATE + i as i32, Rect::new(PAD, y, CW, 48.0), K::Template { name: th.name.clone(), sub, th: i, del: self.del });
            y += 49.0;
        }
        if !self.themes.is_empty() {
            y -= 1.0;
        }
        y += 6.0;
        b.put(0, Rect::new(PAD, y, CW, 1.0), K::Rule(Rgba::white(0.07)));
        y += 7.0;
        let save = t!("Сохранить текущий вид как шаблон", "Save the current look as a template", ja: "現在の外観をテンプレートとして保存", pt: "Salvar a aparência atual como modelo", zh: "将当前外观保存为模板");
        b.put(SAVE, Rect::new(PAD, y, CW, 40.0), K::Action { icon: PLUS, label: save.to_string(), ink: Tone::Accent, h: 40.0 });
        y += 40.0;
        if !self.themes.is_empty() {
            let label = if self.del { t!("Назад", "Back", ja: "戻る", pt: "Voltar", zh: "返回") } else { t!("Удалить шаблон…", "Delete a template…", ja: "テンプレートを削除…", pt: "Excluir um modelo…", zh: "删除模板…") };
            b.put(DELETE, Rect::new(PAD, y + 1.0, CW, 40.0), K::Action { icon: TRASH, label: label.to_string(), ink: Tone::Bad, h: 40.0 });
            y += 41.0;
        }
        y + PAD
    }

    fn lang_page(&self, b: &mut B, m: Measure<'_>) -> f32 {
        let mut y = self.head(b, m, t!("Язык", "Language", ja: "言語", pt: "Idioma", zh: "语言"), t!("Язык всех программ WinRust:RE", "The language of every WinRust:RE program", ja: "すべての WinRust:RE プログラムの言語", pt: "O idioma de todos os programas da WinRust:RE", zh: "所有 WinRust:RE 程序的语言"));
        let cur = lang();
        for (i, (l, code)) in [(Lang::En, "EN"), (Lang::Ru, "RU"), (Lang::Ja, "JA"), (Lang::Pt, "PT"), (Lang::Zh, "ZH")].into_iter().enumerate() {
            b.put(LANG + i as i32, Rect::new(PAD, y, CW, 44.0), K::LangRow { code, name: l.name(), on: l == cur });
            y += 45.0;
        }
        y - 1.0 + PAD
    }

    fn bar_page(&self, b: &mut B, m: Measure<'_>) -> f32 {
        let mut y = self.head(b, m, t!("Панель", "Bar", ja: "バー", pt: "Barra", zh: "任务栏"), t!("Где она, какого размера и как себя ведёт", "Where it sits, its size and behavior", ja: "配置場所、サイズ、動作", pt: "Onde fica, seu tamanho e comportamento", zh: "其位置、大小和行为"));
        b.put(0, Rect::new(14.0, y, 364.0, 112.0), K::Preview);
        y += 112.0 + 12.0;
        let side = style::side();
        let size = style::bar_size();
        b.text(Rect::new(18.0, y, 340.0, 15.0), t!("Положение", "Position", ja: "位置", pt: "Posição", zh: "位置"), LABEL, Tone::Dim);
        Self::seg(b, SIDE, y + 21.0, [Side::Bottom, Side::Left, Side::Right].iter().map(|&s| side_name(s).to_string()).collect(), [Side::Bottom, Side::Left, Side::Right].iter().position(|&s| s == side).unwrap_or(0));
        y += 21.0 + 34.0 + 12.0;
        b.text(Rect::new(18.0, y, 340.0, 15.0), t!("Размер", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小"), LABEL, Tone::Dim);
        Self::seg(b, SIZE, y + 21.0, (0..3).map(|s| size_name(s).to_string()).collect(), usize::from(size.min(2)));
        y += 21.0 + 34.0 + 10.0;
        // the monitors: whether every one has a bar, where the app buttons are and what the other bars hold
        b.text(Rect::new(18.0, y + 4.0, 340.0, 15.0), t!("Дисплеи", "Displays", ja: "ディスプレイ", pt: "Monitores", zh: "显示器"), LABEL, Tone::Dim);
        y += 23.0;
        let all = style::bar_on_all_displays();
        b.put(SHOW_ALL, Rect::new(PAD, y, CW, 40.0), K::Switch { label: t!("Панель на всех дисплеях", "Show the bar on all displays", ja: "すべてのディスプレイにバーを表示", pt: "Mostrar a barra em todos os monitores", zh: "在所有显示器上显示任务栏").to_string(), sub: vec![], on: all, off: false });
        y += 40.0 + 10.0;
        b.text(Rect::new(18.0, y, 340.0, 15.0), t!("Кнопки приложений", "Apps on", ja: "アプリのボタン", pt: "Aplicativos em", zh: "应用按钮显示于"), LABEL, Tone::Dim);
        let apps = [t!("На всех панелях", "All bars", ja: "すべてのバー", pt: "Todas as barras", zh: "所有任务栏"), t!("Где окно", "Where the window is", ja: "ウィンドウのある画面", pt: "Onde a janela está", zh: "窗口所在的显示器")];
        Self::seg(b, APPS_ON, y + 21.0, apps.iter().map(ToString::to_string).collect(), style::apps_on() as usize);
        y += 21.0 + 34.0 + 12.0;
        b.text(Rect::new(18.0, y, 340.0, 15.0), t!("Остальные панели", "Other bars", ja: "ほかのバー", pt: "Outras barras", zh: "其他任务栏"), LABEL, Tone::Dim);
        let other = [t!("Как в Windows 11", "Like Windows 11", ja: "Windows 11 と同じ", pt: "Como no Windows 11", zh: "如 Windows 11"), t!("Как главная", "Same as main", ja: "メインと同じ", pt: "Como a principal", zh: "与主任务栏相同")];
        Self::seg(b, OTHER_BARS, y + 21.0, other.iter().map(ToString::to_string).collect(), style::other_bars() as usize);
        y += 21.0 + 34.0 + 10.0;
        b.text(Rect::new(18.0, y + 4.0, 340.0, 15.0), t!("Поведение", "Behavior", ja: "動作", pt: "Comportamento", zh: "行为"), LABEL, Tone::Dim);
        y += 23.0;
        let dock = style::style() == Style::Dock;
        let left_off = dock || side != Side::Bottom;
        let left_sub = if !left_off {
            String::new()
        } else if side != Side::Bottom {
            t!("Только когда панель снизу", "Only when the bar is at the bottom", ja: "バーが下にあるときのみ", pt: "Só com a barra embaixo", zh: "仅当任务栏在底部时").to_string()
        } else {
            t!("В доке кнопки всегда по центру", "The dock keeps buttons centered", ja: "ドックではボタンは常に中央", pt: "No dock os botões ficam no centro", zh: "停靠模式下按钮始终居中").to_string()
        };
        let rows = [
            (t!("Скрывать автоматически", "Hide automatically", ja: "自動的に隠す", pt: "Ocultar automaticamente", zh: "自动隐藏"), String::new(), style::autohide(), false),
            (t!("Кнопки слева, как в Windows 10", "Buttons on the left, like Windows 10", ja: "ボタンを左に配置 (Windows 10 風)", pt: "Botões à esquerda, como no Windows 10", zh: "按钮靠左，如同 Windows 10"), left_sub, style::align_left() && !left_off, left_off),
            (t!("Alt+Tab: только этот монитор", "Alt+Tab: this monitor only", ja: "Alt+Tab: このモニターのみ", pt: "Alt+Tab: somente este monitor", zh: "Alt+Tab：仅限此显示器"), String::new(), style::alttab_per_monitor(), false),
            (t!("Окна прилипают к краям и друг к другу", "Windows stick to edges and each other", ja: "ウィンドウを端やほかのウィンドウに吸着", pt: "Janelas grudam nas bordas e umas nas outras", zh: "窗口吸附到屏幕边缘和其他窗口"), String::new(), crate::stick(), false),
        ];
        for (i, (label, sub, on, off)) in rows.into_iter().enumerate() {
            let sub: Vec<String> = if sub.is_empty() { vec![] } else { vec![sub] };
            let h: f32 = 12.0 + 17.5 + if sub.is_empty() { 0.0 } else { 2.0 + 16.2 };
            let h = h.max(40.0);
            b.put(if off { 0 } else { BEHAVIOR + i as i32 }, Rect::new(PAD, y, CW, h), K::Switch { label: label.to_string(), sub, on, off });
            y += h + 1.0;
        }
        y - 1.0 + PAD
    }

    fn confirm_page(&self, b: &mut B, m: Measure<'_>, icon: &'static str, reset: bool) -> f32 {
        let back = t!("Назад", "Back", ja: "戻る", pt: "Voltar", zh: "返回");
        b.put(BACK, Rect::new(PAD, PAD, 6.0 + 16.0 + 8.0 + m(back, BODY).ceil() + 10.0, 36.0), K::Back);
        b.put(0, Rect::new(172.0, 56.0, 48.0, 48.0), K::Badge(icon));
        let (q, sub) = if reset {
            (t!("Сбросить всё?", "Reset everything?", ja: "すべてをリセットしますか?", pt: "Restaurar tudo?", zh: "全部重置？"), t!("Тема, цвета, панель. Шаблоны останутся", "Theme, colors, bar. Templates stay", ja: "テーマ、色、バー。テンプレートは残ります", pt: "Tema, cores, barra. Os modelos permanecem", zh: "主题、颜色、任务栏。模板保留"))
        } else {
            (t!("Удалить все плагины?", "Remove every plugin?"), t!("Исходные настройки Windows вернутся на место", "The original Windows settings are put back"))
        };
        b.put(0, Rect::new(PAD, 116.0, CW, 20.0), K::Text { s: q.to_string(), font: Font::new(16.0, 600), ink: Tone::Ink, align: Align::Center });
        b.put(0, Rect::new(PAD, 142.0, CW, 16.0), K::Text { s: sub.to_string(), font: BODY, ink: Tone::Soft, align: Align::Center });
        let cancel = t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消");
        let yes = if reset { t!("Да, сбросить", "Yes, reset", ja: "はい、リセットします", pt: "Sim, restaurar", zh: "是，重置") } else { t!("Да, удалить", "Yes, remove") };
        b.put(if reset { CANCEL } else { UNDO }, Rect::new(14.0, 176.0, 178.0, 40.0), K::Button { label: cancel.to_string(), red: false });
        b.put(if reset { YES } else { REMOVE_YES }, Rect::new(200.0, 176.0, 178.0, 40.0), K::Button { label: yes.to_string(), red: true });
        176.0 + 40.0 + 6.0 + PAD
    }

    fn adv_page(&self, b: &mut B, m: Measure<'_>) -> f32 {
        let mut y = self.head(b, m, t!("Расширенные настройки", "Advanced settings"), t!("Оболочка, клавиши, Windows и анимации", "Shell, keys, Windows and animations"));
        let s = self.sys;
        let group = |b: &mut B, y: f32, name: &str| b.put(0, Rect::new(PAD, y, CW, 31.0), K::Group(name.to_string()));
        let sw = |b: &mut B, m: Measure<'_>, y: f32, id: i32, label: &str, sub: &str, on: Option<bool>| -> f32 {
            let subs = if sub.is_empty() { vec![] } else { wrap(m, sub, SMALL, 308.0) };
            let lines = wrap(m, label, BODY, 308.0).len() as f32;
            let h = (12.0 + 17.5 * lines + if subs.is_empty() { 0.0 } else { 2.0 + 16.2 * subs.len() as f32 }).max(40.0);
            // `id` 0: not wired to a setting the bar can change, shown as the machine has it and greyed out (the installer changes it)
            b.put(id, Rect::new(PAD, y, CW, h), K::Switch { label: label.to_string(), sub: subs, on: on.unwrap_or(false), off: id == 0 });
            y + h
        };
        group(b, y, t!("Оболочка", "Shell"));
        y += 31.0;
        y = sw(b, m, y, 0, t!("WinRust:RE вместо Проводника как оболочка", "WinRust:RE replaces Explorer as the shell"), t!("Применится после повторного входа", "Applies after you sign in again"), Some(s.shell));
        y = sw(b, m, y, 0, t!("Диспетчер задач остаётся в трее при закрытии", "Task Manager stays in the tray when closed"), "", s.tray);
        group(b, y, t!("Клавиши", "Keys"));
        y += 31.0;
        y = sw(b, m, y, ALT_SPACE, t!("Alt+Space — поиск WinRust:RE", "Alt+Space opens WinRust:RE search"), "", Some(s.search_key));
        y = sw(b, m, y, 0, t!("Ctrl+Shift+Esc — Диспетчер задач WinRust:RE", "Ctrl+Shift+Esc opens WinRust:RE Task Manager"), "", Some(s.taskmgr_key));
        y = sw(b, m, y, 0, t!("Win+V — история буфера обмена", "Win+V clipboard history"), t!("Текст, хранится в памяти. Когда Проводник не оболочка", "Text, kept in memory. When Explorer is not the shell"), Some(s.clipboard));
        group(b, y, "Windows");
        y += 31.0;
        y = sw(b, m, y, 0, t!("Отключить поиск Windows (WSearch)", "Turn off Windows Search (WSearch)"), t!("При удалении всё вернётся как было", "Put back when WinRust is removed"), s.wsearch_off);
        y = sw(b, m, y, 0, t!("Скрыть панель задач Windows", "Hide the Windows taskbar"), "", Some(s.taskbar_hidden));
        group(b, y, t!("Анимации", "Animations"));
        y += 31.0;
        let labels = vec![t!("Полные", "Full").to_string(), t!("Короткие", "Reduced").to_string(), t!("Выкл.", "Off").to_string()];
        Self::seg(b, ANIM, y + 2.0, labels, style::anim() as usize);
        y += 40.0;
        group(b, y, t!("Диагностика", "Diagnostics"));
        y += 31.0;
        b.put(OPEN_LOG, Rect::new(PAD, y, CW, 44.0), K::LogRow { label: t!("Открыть журнал установки", "Open the install log").to_string(), path: "%ProgramData%\\plugin-launcher\\install.log" });
        y += 45.0;
        y = Self::row(b, RESTART, y, Row::new(REFRESH, t!("Перезапустить панель", "Restart the bar")));
        y + 4.0 + PAD
    }

    fn upd_page(&self, b: &mut B, m: Measure<'_>) -> f32 {
        let back = t!("Назад", "Back", ja: "戻る", pt: "Voltar", zh: "返回");
        b.put(BACK, Rect::new(PAD, PAD, 6.0 + 16.0 + 8.0 + m(back, BODY).ceil() + 10.0, 36.0), K::Back);
        // the hero: padding 20 18 18, the logo row 56, the check row 34 after 16
        b.put(0, Rect::new(10.0, 48.0, 372.0, 144.0), K::Hero { when: self.when_text() });
        b.put(CHECK_NOW, Rect::new(10.0 + 372.0 - 18.0 - 118.5, 140.0, 118.5, 34.0), K::Hit);
        let mut y = 200.0;
        // the card of switches: the updater and the signatures
        let auto = (self.info.auto)();
        let t1 = t!("Обновлять автоматически", "Install updates automatically");
        let s1 = wrap(m, t!("Раз в день, файлы заменяются на месте", "Once a day, files are swapped in place"), SMALL, 254.0);
        let h1 = 12.0 + 17.5 + 2.0 + 16.2 * s1.len() as f32 + 12.0;
        let t2 = t!("Обновления подписаны", "Updates are signed");
        let s2 = wrap(m, t!("Перед установкой релиз сверяется с ключом WinRust", "Each release is checked against the WinRust key before it is installed"), SMALL, 302.0);
        let h2 = 12.0 + 17.5 + 2.0 + 16.2 * s2.len() as f32 + 12.0;
        b.put(0, Rect::new(10.0, y, 372.0, h1 + 1.0 + h2), K::Card);
        b.put(if auto.is_some() { AUTO_UPDATE } else { 0 }, Rect::new(10.0, y, 372.0, h1), K::UpdToggle { label: t1.to_string(), sub: s1, on: auto });
        b.put(0, Rect::new(66.0, y + h1, 302.0, 1.0), K::Rule(Rgba::white(0.06)));
        b.put(0, Rect::new(10.0, y + h1 + 1.0, 372.0, h2), K::Signed { label: t2.to_string(), sub: s2 });
        y += h1 + 1.0 + h2 + 8.0;
        // what's new
        let news = [
            (NEWS_POWER, t!("Перезагрузка и выключение", "Restart and Shut down"), t!("Кнопки у часов, с подтверждением", "Next to the clock, with a confirmation")),
            (NEWS_SLIDERS, t!("Новые быстрые настройки", "More quick settings"), t!("Точка доступа, ночной свет, проецирование, плеер", "Mobile hotspot, Night light, Project and a media card")),
            (NEWS_CLIP, t!("История буфера Win+V", "Win+V clipboard history"), t!("И магнит окон к краям экрана", "Plus an optional window magnet")),
            (NEWS_FOLDER, "rust-explorer", t!("«Сначала папки» отключается, размеры папок в фоне", "Folders first can be turned off, sizes fill in the background")),
        ];
        let rows: Vec<(&str, &str, Vec<String>)> = news.iter().map(|&(i, t, s)| (i, t, wrap(m, s, SMALL, 308.0))).collect();
        let total: f32 = 14.0 + 20.0 + 6.0 + rows.iter().enumerate().map(|(i, r)| 16.0 + 18.2 + 1.0 + 16.8 * r.2.len() as f32 + if i > 0 { 1.0 } else { 0.0 }).sum::<f32>() + 6.0;
        b.put(0, Rect::new(10.0, y, 372.0, total), K::Card);
        b.put(0, Rect::new(24.0, y + 14.0, 340.0, 20.0), K::NewsHead);
        let mut iy = y + 14.0 + 20.0 + 6.0;
        for (i, (icon, title, sub)) in rows.into_iter().enumerate() {
            let h = 16.0 + 18.2 + 1.0 + 16.8 * sub.len() as f32 + if i > 0 { 1.0 } else { 0.0 };
            b.put(0, Rect::new(24.0, iy, 344.0, h), K::News { icon, title: title.to_string(), sub, first: i == 0 });
            iy += h;
        }
        y += total + 6.0;
        b.put(REMOVE, Rect::new(PAD, y, CW, 38.0), K::Remove(t!("Удалить все плагины…", "Remove every plugin…").to_string()));
        y += 38.0;
        y + PAD
    }

    fn when_text(&self) -> String {
        match self.upd {
            Upd::Busy(_) => t!("Связываюсь с GitHub…", "Contacting GitHub…").to_string(),
            Upd::Done(_) => t!("Проверка запущена", "The check was started").to_string(),
            Upd::Idle => t!("Релизы на GitHub", "Releases on GitHub").to_string(),
        }
    }
}

fn side_name(s: Side) -> &'static str {
    match s {
        Side::Bottom => t!("Снизу", "Bottom", ja: "下", pt: "Embaixo", zh: "底部"),
        Side::Left => t!("Слева", "Left", ja: "左", pt: "Esquerda", zh: "左侧"),
        Side::Right => t!("Справа", "Right", ja: "右", pt: "Direita", zh: "右侧"),
    }
}

fn size_name(n: u8) -> &'static str {
    match n {
        0 => t!("Маленькая", "Small", ja: "小", pt: "Pequeno", zh: "小"),
        1 => t!("Обычная", "Normal", ja: "通常", pt: "Normal", zh: "正常"),
        _ => t!("Большая", "Large", ja: "大", pt: "Grande", zh: "大"),
    }
}

fn style_name(s: Style) -> &'static str {
    match s {
        Style::Classic => t!("Классика", "Classic", ja: "クラシック", pt: "Clássico", zh: "经典"),
        Style::Floating => t!("Парящая", "Floating", ja: "フローティング", pt: "Flutuante", zh: "悬浮"),
        Style::Dock => t!("Док", "Dock", ja: "ドック", pt: "Dock", zh: "停靠"),
    }
}

/// `#rrggbb` as the GDI colour the style module stores.
fn colorref(hex: u32) -> u32 {
    ((hex >> 16) & 255) | (hex & 0xff00) | ((hex & 255) << 16)
}

/// A swatch as it is in the current mode: the design's dark literals are colour tokens, so in light mode the first swatches
/// are the light surfaces and the text shades (and what a click stores is what is shown).
fn chip(hex: u32) -> u32 {
    match style::token((hex >> 16) as u8, (hex >> 8) as u8, hex as u8) {
        0 => hex,
        t => style::token_rgb(t),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// painting

/// What painting needs besides the items.
struct Dc<'a> {
    walls: Option<&'a Walls>,
    accent: Rgba,
    now: u64,
    open_at: u64,
    upd: Upd,
    version: String,
    themes: Vec<Theme>,
}

/// What the mockup puts under its previews: `#0b0d14` with the user's wallpaper over it at `opacity` (.6 on the cards and
/// thumbnails, .55 on the bar preview), cut to the rounded box. Without a wallpaper file a dark gradient stands in.
fn wallpaper(c: &mut Canvas<'_>, r: Rect, radius: f32, pic: Option<&Picture>, opacity: f32) {
    c.fill(r, radius, Rgba::css(11, 13, 20, 1.0));
    match pic {
        Some(p) => c.picture_cover(p, r, radius, opacity),
        None => c.gradient(r, radius, Rgba::css(36, 48, 92, 0.6), Rgba::css(10, 12, 22, 0.6)),
    }
}

/// A bar in miniature: the fill, the half-pixel ring, and optionally the three squares.
fn mini_bar(c: &mut Canvas<'_>, r: Rect, radius: f32, accent: Rgba, o: f32, dots: bool) {
    let p = pal();
    c.faded(o, |c| {
        c.fill(r, radius, Rgba::colorref(p.bg));
        c.ring(r, radius, 0.5, Rgba::colorref(p.line));
        if dots {
            let fg = Rgba::colorref(p.text).fade(0.55);
            let x0 = r.cx() - 8.0;
            let y = r.cy() - 2.0;
            for (k, col) in [accent, fg, fg].into_iter().enumerate() {
                c.fill(Rect::new(x0 + k as f32 * 6.0, y, 4.0, 4.0), 1.0, col);
            }
        }
    });
}

fn draw_item(c: &mut Canvas<'_>, f: &mut Frame<'_>, it: &Item, d: &Dc<'_>) {
    let r = it.r;
    match &it.k {
        K::Header => {
            let tile = Rect::new(r.x + 8.0, r.y + 8.0, 40.0, 40.0);
            c.fill(tile, 12.0, d.accent.fade(0.2));
            c.icon(icons::GEAR, (tile.cx(), tile.cy()), 20.0, 1.8, d.accent, 0.0);
            c.text(t!("Настройки панели", "Bar settings", ja: "バーの設定", pt: "Configurações da barra", zh: "任务栏设置"), Rect::new(r.x + 60.0, r.y + 10.0, 300.0, 19.0), Font::new(15.0, 600), INK, Align::Start);
            c.text(t!("Вид панели и программ WinRust:RE", "Look of the bar and the WinRust:RE programs", ja: "バーと WinRust:RE プログラムの外観", pt: "Aparência da barra e dos programas da WinRust:RE", zh: "任务栏与 WinRust:RE 程序的外观"), Rect::new(r.x + 60.0, r.y + 31.0, 300.0, 15.0), SMALL, DIM, Align::Start);
        }
        K::Text { s, font, ink, align } => c.text(s, r, *font, ink.rgba(d.accent), *align),
        K::Rule(col) => c.fill(r, 0.0, *col),
        K::Theme(st) => {
            let on = *st == style::style();
            let hot = f.hover(it.id, 160, EASE);
            let prev = Rect::new(r.x, r.y, r.w, 64.0);
            wallpaper(c, prev, 10.0, d.walls.and_then(|w| w.card.as_ref()), 0.6);
            let (l, bottom, rad) = match st {
                Style::Classic => (0.0, 0.0, 0.0),
                Style::Floating => (5.0, 5.0, 4.0),
                Style::Dock => (prev.w * 0.26, 5.0, 4.0),
            };
            let bar = Rect::new(prev.x + l, prev.y + prev.h - bottom - 10.0, prev.w - 2.0 * l, 10.0);
            if rad == 0.0 {
                // flush with the card's rounded corners
                c.clipped(bar, |c| mini_bar(c, Rect::new(bar.x, bar.y - 10.0, bar.w, 20.0), 10.0, d.accent, 1.0, false));
                let fg = Rgba::colorref(pal().text).fade(0.55);
                for (k, col) in [d.accent, fg, fg].into_iter().enumerate() {
                    c.fill(Rect::new(bar.cx() - 8.0 + k as f32 * 6.0, bar.cy() - 2.0, 4.0, 4.0), 1.0, col);
                }
            } else {
                mini_bar(c, bar, rad, d.accent, 1.0, true);
            }
            let ring = Rgba::white(0.1).mix(Rgba::white(0.3), hot);
            if on {
                c.ring(prev, 10.0, 2.0, d.accent);
            } else {
                c.ring(prev, 10.0, 1.0 + 0.5 * hot, ring);
            }
            c.text(style_name(*st), Rect::new(r.x, r.y + 70.0, r.w, 15.0), LABEL, if on { INK } else { SOFT }, Align::Center);
        }
        K::Dot(a) => {
            let on = *a == style::accent() && style::custom_colors().is_none_or(|c| c[4] == u32::MAX);
            let k = 1.0 + 0.1 * f.hover(it.id, 200, SPRING);
            let col = Rgba::colorref(a.color());
            let (cx, cy) = (r.cx(), r.cy());
            c.transformed(cx, cy, k, 0.0, 0.0, |c| {
                if on {
                    c.ring(Rect::new(cx - 11.0, cy - 11.0, 22.0, 22.0), 11.0, 1.6, col);
                }
                c.circle(cx, cy, if on { 7.0 } else { 9.0 }, col);
            });
        }
        K::Row(row) => draw_row(c, f, it, row, d),
        K::Back => {
            let hot = f.hover(it.id, 140, SPRING);
            c.fill(r, 10.0, Rgba::white(0.09 * hot));
            c.icon(BACK_ARROW, (r.x + 14.0, r.cy()), 16.0, 2.2, SOFT, 0.0);
            c.text(t!("Назад", "Back", ja: "戻る", pt: "Voltar", zh: "返回"), Rect::new(r.x + 30.0, r.y, r.w, r.h), BODY, SOFT, Align::Start);
        }
        K::ColorRow { i, open } => {
            let hot = f.hover(it.id, 140, SPRING);
            let p = pal();
            let cols = style::custom_colors();
            let eff = [p.bg, p.line, p.hover, p.active, p.accent, p.text, p.dim][*i];
            let set = cols.is_some_and(|c| c[*i] != u32::MAX);
            if *open {
                c.fill(Rect::new(r.x, r.y, r.w, 108.0), 12.0, Rgba::white(0.05));
            }
            c.fill(r, 10.0, Rgba::white(0.09 * hot));
            let (cx, cy) = (r.x + 10.0 + 10.0, r.cy());
            c.circle(cx, cy, 9.0, Rgba::colorref(eff));
            c.ring(Rect::new(cx - 9.5, cy - 9.5, 19.0, 19.0), 9.5, 1.0, Rgba::white(0.22));
            c.text(color_name(*i), Rect::new(r.x + 42.0, r.y, 280.0, r.h), BODY, INK, Align::Start);
            c.circle(r.x + r.w - 10.0 - 4.0, cy, 4.0, d.accent.fade(f32::from(set)));
        }
        K::Chip { i, hex } => {
            let sel = style::custom_colors().is_some_and(|c| c[*i] == colorref(*hex));
            let k = 1.0 + 0.12 * f.hover(it.id, 160, SPRING);
            let (cx, cy) = (r.cx(), r.cy());
            let e = chips_in(d);
            c.faded(e.0, |c| {
                c.transformed(cx, cy, k, 0.0, 0.0, |c| {
                    if sel {
                        c.ring(Rect::new(cx - 11.0, cy - 11.0, 22.0, 22.0), 11.0, 1.5, Rgba::pure_white(1.0));
                    }
                    c.circle(cx, cy, 8.0, Rgba::colorref(colorref(*hex))); // already the mode's value (see `chip`)
                    c.ring(Rect::new(cx - 8.0, cy - 8.0, 16.0, 16.0), 8.0, 0.75, Rgba::white(0.2));
                });
            });
        }
        K::ChipReset(s) => {
            let hot = f.hover(it.id, 140, SPRING);
            let e = chips_in(d);
            c.faded(e.0, |c| {
                c.fill(r, 7.0, Rgba::white(0.06).mix(Rgba::white(0.09), hot));
                c.text(s, r, Font::new(11.0, 400), SOFT, Align::Center);
            });
        }
        K::Seg { labels, cur, base } => draw_seg(c, f, r, labels, *cur, *base),
        K::Hit => {}
        K::Switch { label, sub, on, off } => {
            let hot = if *off { 0.0 } else { f.hover(it.id, 140, SPRING) };
            c.faded(if *off { 0.45 } else { 1.0 }, |c| {
                c.fill(r, 10.0, Rgba::white(0.09 * hot));
                let mut y = r.y + 6.0;
                c.text(label, Rect::new(r.x + 10.0, y, 308.0, 17.5), BODY, INK, Align::Start);
                y += 17.5 + 2.0;
                for l in sub {
                    c.text(l, Rect::new(r.x + 10.0, y, 308.0, 16.2), SMALL, DIM, Align::Start);
                    y += 16.2;
                }
                switch(c, f, it.id, (r.x + r.w - 10.0 - 36.0, r.cy() - 10.0), *on, d.accent);
            });
        }
        K::Preview => draw_preview(c, f, r, d),
        K::Template { name, sub, th, del } => {
            let hot = f.hover(it.id, 140, SPRING);
            let hov = if *del { Rgba::css(229, 72, 77, 0.14) } else { Rgba::white(0.09) };
            c.fill(r, 10.0, hov.fade(hot));
            let thm = &d.themes[*th];
            let thumb = Rect::new(r.x + 10.0, r.cy() - 14.0, 44.0, 28.0);
            let bg = if thm.colors[0] != u32::MAX { thm.colors[0] } else { style::BASE.bg };
            let acc = if thm.colors[4] != u32::MAX { thm.colors[4] } else { thm.accent.color() };
            wallpaper(c, thumb, 6.0, d.walls.and_then(|w| w.thumb.as_ref()), 0.6);
            c.clipped(Rect::new(thumb.x, thumb.y + 21.0, 44.0, 7.0), |c| c.fill(Rect::new(thumb.x, thumb.y + 8.0, 44.0, 20.0), 6.0, Rgba::colorref(bg)));
            c.fill(Rect::new(thumb.x + 19.0, thumb.y + 28.0 - 2.0 - 3.0, 6.0, 3.0), 1.0, Rgba::colorref(acc));
            c.text(name, Rect::new(r.x + 66.0, r.y + 8.0, 250.0, 16.0), MEDIUM, INK, Align::Start);
            c.text(sub, Rect::new(r.x + 66.0, r.y + 25.0, 250.0, 15.0), SMALL, DIM, Align::Start);
            let (ic, col) = if *del { (CROSS, BAD) } else { (CHEVRON, DIM) };
            c.icon(ic, (r.x + r.w - 10.0 - 8.0, r.cy()), 16.0, 2.0, col, 0.0);
        }
        K::Note(s) => {
            c.fill(r, 12.0, Rgba::white(0.04));
            c.text(s, r, BODY, DIM, Align::Center);
        }
        K::Action { icon, label, ink, h } => {
            let hot = f.hover(it.id, 140, SPRING);
            let hov = if *ink == Tone::Bad { Rgba::css(229, 72, 77, 0.14) } else { Rgba::white(0.09) };
            c.fill(r, 10.0, hov.fade(hot));
            c.icon(icon, (r.x + 19.0, r.y + h / 2.0), 18.0, 1.8, ink.rgba(d.accent), 0.0);
            c.text(label, Rect::new(r.x + 40.0, r.y, r.w - 50.0, *h), if *ink == Tone::Accent { MEDIUM } else { BODY }, ink.rgba(d.accent), Align::Start);
        }
        K::LangRow { code, name, on } => {
            let hot = f.hover(it.id, 140, SPRING);
            c.fill(r, 10.0, Rgba::white(if *on { 0.05 } else { 0.0 }).mix(Rgba::white(0.09), hot));
            let chip = Rect::new(r.x + 12.0, r.cy() - 10.0, 28.0, 20.0);
            c.fill(chip, 5.0, Rgba::white(0.08));
            c.text(code, chip, Font::new(10.0, 600).tracking(0.02), SOFT, Align::Center);
            c.text(name, Rect::new(r.x + 52.0, r.y, 240.0, r.h), MEDIUM, INK, Align::Start);
            if *on {
                c.icon(CHECK, (r.x + r.w - 12.0 - 8.0, r.cy()), 16.0, 2.6, d.accent, 0.0);
            }
        }
        K::Badge(icon) => {
            c.circle(r.cx(), r.cy(), 24.0, Rgba::css(229, 72, 77, 0.14));
            c.icon(icon, (r.cx(), r.cy()), 22.0, 2.0, BAD, 0.0);
        }
        K::Button { label, red } => {
            let hot = f.hover(it.id, 200, EASE);
            let press = f.tween(it.id, 3, if f.press == Some(it.id) { 0.96 } else { 1.0 }, 200, SPRING);
            c.transformed(r.cx(), r.cy(), if *red { press } else { 1.0 }, 0.0, 0.0, |c| {
                if *red {
                    let k = 1.0 + 0.15 * hot;
                    c.fill(r, 12.0, Rgba::new((RED.r * k).min(1.0), (RED.g * k).min(1.0), (RED.b * k).min(1.0), 1.0));
                    c.text(label, r, Font::new(13.0, 600), Rgba::pure_white(1.0), Align::Center);
                } else {
                    c.fill(r, 12.0, Rgba::white(0.07).mix(Rgba::white(0.09), hot));
                    c.text(label, r, MEDIUM, INK, Align::Center);
                }
            });
        }
        K::Hero { when } => draw_hero(c, f, r, when, d),
        K::Card => {
            c.fill(r, 16.0, Rgba::white(0.04));
            c.ring(r, 16.0, 1.0, Rgba::white(0.06));
        }
        K::UpdToggle { label, sub, on } => {
            let hot = if on.is_some() { f.hover(it.id, 140, SPRING) } else { 0.0 };
            c.clipped(r, |c| c.fill(Rect::new(r.x, r.y, r.w, r.h + 200.0), 16.0, Rgba::white(0.09 * hot)));
            let tile = Rect::new(r.x + 12.0, r.y + 12.0, 32.0, 32.0);
            c.fill(tile, 9.0, Rgba::white(0.06));
            c.icon(REFRESH, (tile.cx(), tile.cy()), 17.0, 1.8, SOFT, 0.0);
            c.text(label, Rect::new(r.x + 56.0, r.y + 12.0, 254.0, 17.5), MEDIUM, INK, Align::Start);
            for (k, l) in sub.iter().enumerate() {
                c.text(l, Rect::new(r.x + 56.0, r.y + 12.0 + 19.5 + k as f32 * 16.2, 254.0, 16.2), SMALL, DIM, Align::Start);
            }
            c.faded(if on.is_some() { 1.0 } else { 0.45 }, |c| switch(c, f, it.id, (r.x + r.w - 14.0 - 36.0, r.cy() - 10.0), on.unwrap_or(false), d.accent));
        }
        K::Signed { label, sub } => {
            let tile = Rect::new(r.x + 12.0, r.y + 12.0, 32.0, 32.0);
            c.fill(tile, 9.0, GREEN.alpha(0.12));
            c.icon(SHIELD, (tile.cx(), tile.cy()), 17.0, 1.8, GREEN, 0.0);
            c.text(label, Rect::new(r.x + 56.0, r.y + 12.0, 302.0, 17.5), MEDIUM, INK, Align::Start);
            for (k, l) in sub.iter().enumerate() {
                c.text(l, Rect::new(r.x + 56.0, r.y + 12.0 + 19.5 + k as f32 * 16.2, 302.0, 16.2), SMALL, DIM, Align::Start);
            }
        }
        K::NewsHead => {
            c.text(t!("Что нового", "What's new"), Rect::new(r.x, r.y, 120.0, 20.0), Font::new(13.0, 600), INK, Align::Start);
            let tw = c.measure(t!("Что нового", "What's new"), Font::new(13.0, 600)).ceil();
            let badge = Rect::new(r.x + tw + 8.0, r.y, c.measure(&d.version, Font::new(11.0, 600)).ceil() + 14.0, 20.0);
            c.fill(badge, 6.0, d.accent.fade(0.2));
            c.text(&d.version, badge, Font::new(11.0, 600), d.accent, Align::Center);
        }
        K::News { icon, title, sub, first } => {
            if !first {
                c.fill(Rect::new(r.x, r.y, r.w, 1.0), 0.0, Rgba::white(0.06));
            }
            let top = r.y + if *first { 8.0 } else { 9.0 };
            let tile = Rect::new(r.x, top, 24.0, 24.0);
            c.fill(tile, 7.0, Rgba::white(0.06));
            c.icon(icon, (tile.cx(), tile.cy()), 14.0, 2.0, SOFT, 0.0);
            c.text(title, Rect::new(r.x + 36.0, top, 308.0, 18.2), MEDIUM, INK, Align::Start);
            for (k, l) in sub.iter().enumerate() {
                c.text(l, Rect::new(r.x + 36.0, top + 19.2 + k as f32 * 16.8, 308.0, 16.8), SMALL, DIM, Align::Start);
            }
        }
        K::Remove(label) => {
            let hot = f.hover(it.id, 140, SPRING);
            c.fill(r, 10.0, Rgba::css(229, 72, 77, 0.14).fade(hot));
            let tw = c.measure(label, SMALL).ceil();
            let x = r.cx() - (14.0 + 8.0 + tw) / 2.0;
            c.icon(TRASH, (x + 7.0, r.cy()), 14.0, 2.0, BAD, 0.0);
            c.text(label, Rect::new(x + 22.0, r.y, tw + 4.0, r.h), SMALL, BAD, Align::Start);
        }
        K::Group(s) => c.text(s, Rect::new(r.x + 10.0, r.y + 12.0, 340.0, 15.0), LABEL, DIM, Align::Start),
        K::LogRow { label, path } => {
            let hot = f.hover(it.id, 140, SPRING);
            c.fill(r, 10.0, Rgba::white(0.09 * hot));
            c.icon(FILE, (r.x + 19.0, r.cy()), 18.0, 1.8, SOFT, 0.0);
            c.text(label, Rect::new(r.x + 40.0, r.y + 6.0, 280.0, 16.0), BODY, INK, Align::Start);
            c.text(path, Rect::new(r.x + 40.0, r.y + 24.0, 290.0, 14.0), Font::new(11.0, 400), DIM, Align::Start);
            c.icon(CHEVRON, (r.x + r.w - 17.0, r.cy()), 14.0, 2.4, DIM, 0.0);
        }
    }
}

fn color_name(i: usize) -> &'static str {
    [
        t!("Фон", "Background", ja: "背景", pt: "Fundo", zh: "背景"),
        t!("Линии и рамки", "Lines and borders", ja: "線と枠", pt: "Linhas e bordas", zh: "线条与边框"),
        t!("Наведение", "Hover", ja: "ホバー", pt: "Passar o mouse", zh: "悬停"),
        t!("Нажатие и выбранное", "Pressed and selected", ja: "押下中と選択中", pt: "Pressionado e selecionado", zh: "按下和选中"),
        t!("Акцент", "Accent", ja: "アクセント", pt: "Destaque", zh: "强调色"),
        t!("Текст", "Text", ja: "テキスト", pt: "Texto", zh: "文本"),
        t!("Второстепенный текст", "Secondary text", ja: "補助テキスト", pt: "Texto secundário", zh: "次要文本"),
    ][i]
}

/// `chipsIn`: the chips of a row that has just opened fade in and settle 4 px (opacity, dy).
fn chips_in(d: &Dc<'_>) -> (f32, f32) {
    let (dur, _) = anim::timing(style::anim(), 220, 0);
    if dur == 0 {
        return (1.0, 0.0);
    }
    let e = SNAP.ease((d.now.saturating_sub(d.open_at) as f32 / dur as f32).clamp(0.0, 1.0));
    (e, anim::lerp(-4.0, 0.0, e))
}

fn draw_row(c: &mut Canvas<'_>, f: &mut Frame<'_>, it: &Item, row: &Row, d: &Dc<'_>) {
    let r = it.r;
    let hot = f.hover(it.id, 140, SPRING);
    let hov = if row.danger { Rgba::css(229, 72, 77, 0.14) } else { Rgba::white(0.09) };
    c.fill(r, 10.0, hov.fade(hot));
    let ink = if row.danger { BAD } else { INK };
    c.icon(row.icon, (r.x + 19.0, r.cy()), 18.0, 1.8, if row.danger { BAD } else { SOFT }, 0.0);
    c.text(&row.label, Rect::new(r.x + 40.0, r.y, r.w - 50.0, r.h), BODY, ink, Align::Start);
    let mut right = r.x + r.w - 10.0;
    if row.chevron {
        c.icon(CHEVRON, (right - 7.0, r.cy()), 14.0, 2.4, DIM, 0.0);
        right -= 14.0 + 12.0;
    }
    if row.check {
        c.icon(CHECK, (r.x + r.w - 10.0 - 8.0, r.cy()), 16.0, 2.6, d.accent, 0.0);
    }
    match &row.val {
        Val::None => {}
        Val::Text(s) => c.text(s, Rect::new(right - 200.0, r.cy() - 7.5, 200.0, 15.0), SMALL, DIM, Align::End),
        Val::Palette => {
            let p = pal();
            for (k, col) in [p.bg, p.accent, p.text].into_iter().enumerate() {
                let cx = right - 36.0 + 5.0 + k as f32 * 13.0;
                c.circle(cx, r.cy(), 4.5, Rgba::colorref(col));
                c.ring(Rect::new(cx - 4.75, r.cy() - 4.75, 9.5, 9.5), 4.75, 0.5, Rgba::white(0.2));
            }
        }
        Val::Version(v) => {
            let tw = c.measure(v, SMALL).ceil();
            c.text(v, Rect::new(right - tw - 4.0, r.cy() - 7.5, tw + 4.0, 15.0), SMALL, DIM, Align::End);
            c.circle(right - tw - 6.0 - 3.5, r.cy(), 3.5, GREEN);
        }
    }
}

fn draw_seg(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, labels: &[String], cur: usize, base: i32) {
    c.fill(r, 10.0, Rgba::css(0, 0, 0, 0.28));
    c.fill(Rect::new(r.x + 3.0, r.y, r.w - 6.0, 1.0), 0.0, Rgba::css(0, 0, 0, 0.3));
    c.fill(Rect::new(r.x + 2.0, r.y + 1.0, r.w - 4.0, 1.0), 0.0, Rgba::css(0, 0, 0, 0.12));
    let n = labels.len();
    let w = (r.w - 6.0) / n as f32;
    let pos = f.tween(base, 7, cur as f32, 320, SNAP);
    let thumb = Rect::new(r.x + 3.0 + pos * w, r.y + 3.0, w, 28.0);
    c.outside(thumb, 7.0, |c| c.shadow(thumb, 7.0, 1.0, 3.0, Rgba::css(0, 0, 0, 0.45)));
    c.gradient(thumb, 7.0, Rgba::white(0.16), Rgba::white(0.1));
    c.fill(Rect::new(thumb.x + 3.0, thumb.y, thumb.w - 6.0, 1.0), 0.0, Rgba::white(0.14));
    for (k, l) in labels.iter().enumerate() {
        let id = base + k as i32;
        let sel = f.tween(id, 8, f32::from(k == cur), 200, EASE);
        let hot = f.hover(id, 200, EASE);
        let col = DIM.mix(INK, sel.max(hot));
        c.text(l, Rect::new(r.x + 3.0 + k as f32 * w, r.y + 3.0, w, 28.0), LABEL, col, Align::Center);
    }
}

/// A switch: the 36×20 track and the 16 px knob (`.16s` colour, `.18s` slide).
fn switch(c: &mut Canvas<'_>, f: &mut Frame<'_>, key: i32, (x, y): (f32, f32), on: bool, accent: Rgba) {
    let k = f.tween(key, 1, f32::from(on), 160, EASE);
    let slide = f.tween(key, 2, f32::from(on), 180, SPRING);
    c.fill(Rect::new(x, y, 36.0, 20.0), 10.0, Rgba::white(0.16).mix(accent, k));
    c.circle(x + 2.0 + 8.0 + 16.0 * slide, y + 10.0, 8.0, Rgba::pure_white(1.0));
}

fn draw_preview(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, d: &Dc<'_>) {
    let (side, st, size) = (style::side(), style::style(), style::bar_size().min(2));
    let classic = st == Style::Classic;
    let pill = st == Style::Dock;
    let thick = [9.0, 11.0, 13.0][usize::from(size)];
    let (w, h) = (r.w, r.h);
    let edge = if classic { 0.0 } else { 6.0 };
    // the box of the bar inside the preview, as the mockup's left/right/top/bottom
    let (x0, y0, x1, y1) = match side {
        Side::Bottom => {
            let l = if classic { 0.0 } else if pill { w * 0.28 } else { 6.0 };
            (l, h - edge - thick, w - l, h - edge)
        }
        Side::Left => (edge, edge, edge + thick, h - edge),
        Side::Right => (w - edge - thick, edge, w - edge, h - edge),
    };
    let rad = if classic { 0.0 } else { 6.0 };
    let mut g = |slot: u8, to: f32| f.tween(PREVIEW, slot, to, 350, SNAP);
    let (x0, y0, x1, y1) = (g(0, x0), g(1, y0), g(2, x1), g(3, y1));
    let rad = f.tween(PREVIEW, 4, rad, 350, EASE);
    let o = f.tween(PREVIEW, 5, if style::autohide() { 0.35 } else { 1.0 }, 300, EASE);
    wallpaper(c, r, 12.0, d.walls.and_then(|w| w.preview.as_ref()), 0.55);
    let bar = Rect::new(r.x + x0, r.y + y0, x1 - x0, y1 - y0);
    let bg = Rgba::colorref(pal().bg).alpha(f32::from(style::bar_opacity()) / 100.0);
    c.clipped(r, |c| {
        c.faded(o, |c| {
            c.fill(bar, rad, bg);
            c.ring(bar, rad, 0.5, Rgba::white(0.2));
        });
    });
    c.ring(r, 12.0, 1.0, Rgba::white(0.08));
}

fn draw_hero(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, when: &str, d: &Dc<'_>) {
    c.fill(r, 18.0, Rgba::white(0.04));
    c.radial(r, 18.0, (r.x, r.y), (r.w * 1.2, r.h * 0.9), d.accent.fade(0.2), 0.6);
    c.ring(r, 18.0, 1.0, Rgba::white(0.07));
    // the logo tile
    let logo = Rect::new(r.x + 18.0, r.y + 20.0, 56.0, 56.0);
    c.outside(logo, 16.0, |c| c.shadow(logo, 16.0, 8.0, 20.0, Rgba::css(0, 0, 0, 0.35)));
    c.gradient(logo, 16.0, Rgba::css(35, 37, 46, 1.0), Rgba::css(23, 24, 30, 1.0));
    c.ring(logo, 16.0, 1.0, Rgba::white(0.08));
    let (lx, ly) = (logo.cx() - 12.0 * 32.0 / 24.0, logo.cy() - 12.0 * 32.0 / 24.0);
    let k = 32.0 / 24.0;
    for (i, (sx, sy)) in [(2.0, 2.0), (2.0, 13.0)].into_iter().enumerate() {
        c.fill(Rect::new(lx + sx * k, ly + sy * k, 9.0 * k, 9.0 * k), 2.2 * k, d.accent.fade(if i == 0 { 1.0 } else { 0.7 }));
    }
    c.icon("M14.5 4.5h3.2a3.2 3.2 0 0 1 0 6.4h-3.2zM14.5 10.9l5.4 9.6M14.5 4.5v16", (logo.cx(), logo.cy()), 32.0, 2.2, INK, 0.0);
    c.text("WinRust:RE", Rect::new(r.x + 88.0, r.y + 26.5, 150.0, 15.0), SMALL, SOFT, Align::Start);
    c.text(&d.version, Rect::new(r.x + 88.0, r.y + 45.5, 150.0, 24.0), Font::new(24.0, 600).tracking(-0.02).tabular(), INK, Align::Start);
    // the status chip
    let busy = matches!(d.upd, Upd::Busy(_));
    let chip_text = match d.upd {
        Upd::Busy(_) => t!("Проверяю", "Checking"),
        Upd::Done(_) => t!("Проверка запущена", "Check started"),
        Upd::Idle => t!("Установлена", "Installed"),
    };
    let tw = c.measure(chip_text, MEDIUM.with_size(12.0)).ceil();
    let cw = 10.0 + 12.0 + 6.0 + tw + 10.0;
    let chip = Rect::new(r.x + r.w - 18.0 - cw, r.y + 35.0, cw, 26.0);
    let pulse = if let Upd::Done(at) = d.upd { SNAP.ease((d.now.saturating_sub(at) as f32 / 350.0).clamp(0.0, 1.0)) } else { 1.0 };
    let (bg, fg) = if busy { (Rgba::white(0.08), SOFT) } else { (GREEN.alpha(0.14), GREEN_INK) };
    c.transformed(chip.cx(), chip.cy(), anim::lerp(0.9, 1.0, pulse), 0.0, 0.0, |c| {
        c.faded(anim::lerp(0.4, 1.0, pulse), |c| {
            c.fill(chip, 13.0, bg);
            let ic = (chip.x + 10.0 + 6.0, chip.cy());
            if busy {
                c.ring(Rect::new(ic.0 - 4.5, ic.1 - 4.5, 9.0, 9.0), 4.5, 1.5, Rgba::white(0.18));
                let turn = (d.now % 900) as f32 / 900.0 * 360.0;
                c.icon(SPIN_ARC, ic, 12.0, 3.0, fg, turn);
            } else {
                c.icon(CHECK, ic, 12.0, 3.0, fg, 0.0);
            }
            c.text(chip_text, Rect::new(chip.x + 10.0 + 12.0 + 6.0, chip.y, tw + 4.0, chip.h), MEDIUM.with_size(12.0), fg, Align::Start);
        });
    });
    // the check row
    c.text(when, Rect::new(r.x + 18.0, r.y + 101.5, 200.0, 15.0), SMALL, DIM, Align::Start);
    let btn = Rect::new(r.x + r.w - 18.0 - 118.5, r.y + 92.0, 118.5, 34.0);
    let hot = f.hover(CHECK_NOW, 160, EASE);
    c.fill(btn, 10.0, Rgba::white(0.1).mix(Rgba::white(0.16), hot));
    c.ring(btn, 10.0, 1.0, Rgba::white(0.08));
    let turn = if busy { (d.now % 900) as f32 / 900.0 * 360.0 } else { 0.0 };
    c.icon(REFRESH, (btn.x + 12.0 + 7.5, btn.cy()), 15.0, 2.2, INK, turn);
    let label = if busy { t!("Проверяю…", "Checking…") } else { t!("Проверить", "Check now") };
    c.text(label, Rect::new(btn.x + 12.0 + 15.0 + 8.0, btn.y, 90.0, btn.h), MEDIUM, INK, Align::Start);
}

trait WithSize {
    fn with_size(self, s: f32) -> Self;
}
impl WithSize for Font {
    fn with_size(self, s: f32) -> Self {
        Self { size: s, ..self }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the panel's contract with the frame

impl Content for SettingsPanel {
    fn id(&self) -> Id {
        Id::Bar
    }

    fn look(&self) -> Look {
        Look::glass(22.0, 0.86, Shadow { dy: 30.0, blur: 80.0, color: Rgba::css(0, 0, 0, 0.6) })
    }

    fn place(&self) -> Place {
        Place { along: Along::Corner, gap: 12.0 }
    }

    /// `bsIn`: from a button-sized rounded square above the gear, 40 px lower, to the whole panel.
    fn entry(&self) -> Entry {
        Entry::reveal(40.0, Inset::new(MAIN_H - 40.0, W - 44.0, 0.0, 4.0, 10.0), 0.9, 360)
    }

    /// `bsOut`: back into that square, 220 ms.
    fn exit(&self) -> Exit {
        Exit::reveal(40.0, Inset::new(self.size.1 - 40.0, W - 44.0, 0.0, 4.0, 10.0), 0.5, 220)
    }

    fn size(&mut self, g: &mut Gfx) -> (f32, f32) {
        let (items, h) = self.build(&mut |s, f| g.measure(s, f));
        self.items = items;
        self.size = (W, h);
        self.built = Some((self.page, lang(), self.tiled));
        self.size
    }

    fn max_size(&mut self, _g: &mut Gfx) -> Option<(f32, f32)> {
        Some((W, 760.0))
    }

    fn resized(&mut self, g: &mut Gfx, _now: u64) -> Option<(f32, f32)> {
        let (items, h) = self.build(&mut |s, f| g.measure(s, f));
        self.items = items;
        let new = (W, h);
        (new != std::mem::replace(&mut self.size, new)).then_some(new)
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let now = f.now;
        if self.walls.is_none() || self.page_at.is_none() {
            self.load_walls(c); // when the panel opens and when a page arrives: the wallpaper may have changed
        }
        let page_at = *self.page_at.get_or_insert(now);
        let open_at = *self.open_at.get_or_insert(now);
        match self.upd {
            Upd::Busy(None) => self.upd = Upd::Busy(Some(now)),
            Upd::Busy(Some(at)) if now.saturating_sub(at) >= 1600 => self.upd = Upd::Done(now),
            _ => {}
        }
        let d = Dc { walls: self.walls.as_ref(), accent: Rgba::colorref(pal().accent), now, open_at, upd: self.upd, version: self.info.version.clone(), themes: self.themes.clone() };
        let leave = f.fade_out(100);
        // the page arrives: pgF / pgB, .3 s
        let (po, pdx) = match self.dir {
            Some(fwd) => {
                let (dur, _) = anim::timing(style::anim(), 300, 0);
                let e = if dur == 0 { 1.0 } else { SNAP.ease((now.saturating_sub(page_at) as f32 / dur as f32).clamp(0.0, 1.0)) };
                (e, if fwd { 28.0 } else { -28.0 } * (1.0 - e))
            }
            None => (1.0, 0.0),
        };
        let items = &self.items;
        c.faded(po, |c| {
            c.transformed(0.0, 0.0, 1.0, pdx, 0.0, |c| {
                let mut i = 0;
                while i < items.len() {
                    // one run of items of the same stage rises together
                    let stage = items[i].stage;
                    let j = i + items[i..].iter().take_while(|it| it.stage == stage).count();
                    let rise = if stage == NO_STAGE || self.page != Page::Main {
                        None
                    } else {
                        let (dur, delay) = anim::timing(style::anim(), 300, RISE_DELAYS[usize::from(stage).min(11)]);
                        let age = now.saturating_sub(page_at) as f32;
                        let e = if dur == 0 { 1.0 } else { SNAP.ease(((age - delay as f32) / dur as f32).clamp(0.0, 1.0)) };
                        Some(Rise { opacity: e * leave, dy: anim::lerp(14.0, 0.0, e), k: 1.0 })
                    };
                    let run = &items[i..j];
                    let mut paint = |c: &mut Canvas<'_>| {
                        for it in run {
                            draw_item(c, f, it, &d);
                        }
                    };
                    match rise {
                        Some(r) => r.draw(c, (0.0, 0.0), paint),
                        None => paint(c),
                    }
                    i = j;
                }
            });
        });
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        self.items.iter().rev().find(|it| it.id != 0 && it.r.contains(x, y)).map(|it| it.id)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if b != Button::Left {
            return;
        }
        let act = self.info.act;
        match id {
            BACK => self.go(Page::Main, false),
            i if (THEME..THEME + 3).contains(&i) => {
                style::set_style(style::ALL[(i - THEME) as usize]);
                self.relook(cx);
            }
            i if (ACCENT..ACCENT + 5).contains(&i) => {
                style::set_accent(Accent::ALL[(i - ACCENT) as usize]);
                self.relook(cx);
            }
            GO_COLORS => self.go(Page::Colors, true),
            GO_TEMPLATES => self.go(Page::Templates, true),
            GO_LANG => self.go(Page::Lang, true),
            GO_BAR => self.go(Page::Bar, true),
            GO_ADV => self.go(Page::Adv, true),
            GO_UPD => self.go(Page::Upd, true),
            GO_RESET => self.go(Page::Reset, true),
            TILE => {
                self.tiled = true;
                cx.run(move || act(Act::Tile));
            }
            i if (COLOR_ROW..COLOR_ROW + 7).contains(&i) => {
                let row = (i - COLOR_ROW) as usize;
                self.open = if self.open == Some(row) { None } else { Some(row) };
                self.open_at = None;
            }
            i if (CHIP..CHIP + 140).contains(&i) => {
                let (row, k) = (((i - CHIP) / 20) as usize, ((i - CHIP) % 20) as usize);
                style::set_custom_color(row, CHIPS.get(k).map(|&h| colorref(chip(h))));
                self.relook(cx);
            }
            i if (OPACITY..OPACITY + 5).contains(&i) => {
                style::set_bar_opacity(OPACITIES[(i - OPACITY) as usize]);
                self.relook(cx);
            }
            RESET_COLORS => {
                style::reset_custom();
                self.open = None;
                self.relook(cx);
            }
            i if (TEMPLATE..TEMPLATE + 50).contains(&i) => {
                let n = (i - TEMPLATE) as usize;
                if n >= self.themes.len() {
                    return;
                }
                if self.del {
                    self.themes.remove(n);
                    style::save_themes(&self.themes);
                    self.del = !self.themes.is_empty();
                } else {
                    self.themes[n].apply();
                    self.relook(cx);
                }
            }
            SAVE => {
                let name = |n: usize| format!("{} {n}", t!("Шаблон", "Template", ja: "テンプレート", pt: "Modelo", zh: "模板"));
                let n = (1..).find(|&n| !self.themes.iter().any(|t| t.name == name(n))).unwrap_or(1);
                self.themes.push(Theme::current(&name(n)));
                style::save_themes(&self.themes);
                self.del = false;
            }
            DELETE => self.del = !self.del,
            i if (LANG..LANG + 5).contains(&i) => {
                let l = [Lang::En, Lang::Ru, Lang::Ja, Lang::Pt, Lang::Zh][(i - LANG) as usize];
                winrust_common::set_lang(l);
                cx.run(move || act(Act::Language));
            }
            i if (SIDE..SIDE + 3).contains(&i) => {
                style::set_side([Side::Bottom, Side::Left, Side::Right][(i - SIDE) as usize]);
                self.relook(cx);
            }
            i if (SIZE..SIZE + 3).contains(&i) => {
                style::set_bar_size((i - SIZE) as u8);
                self.relook(cx);
            }
            i if (BEHAVIOR..BEHAVIOR + 4).contains(&i) => self.behavior((i - BEHAVIOR) as usize, cx),
            SHOW_ALL => {
                let on = !style::bar_on_all_displays();
                style::set_bar_on_all_displays(on);
                self.say(cx, if on {
                    t!("Панель на каждом дисплее", "A bar on every display", ja: "すべてのディスプレイにバーを表示します", pt: "Uma barra em cada monitor", zh: "每个显示器上都有任务栏")
                } else {
                    t!("Панель только на главном дисплее", "The bar stays on the main display", ja: "メイン ディスプレイのみにバーを表示します", pt: "A barra fica só no monitor principal", zh: "任务栏仅在主显示器上")
                });
            }
            i if (APPS_ON..APPS_ON + 2).contains(&i) => {
                style::set_apps_on(style::AppsOn::ALL[(i - APPS_ON) as usize]);
                self.relook(cx);
            }
            i if (OTHER_BARS..OTHER_BARS + 2).contains(&i) => {
                style::set_other_bars(style::OtherBars::ALL[(i - OTHER_BARS) as usize]);
                self.relook(cx);
            }
            CANCEL => self.go(Page::Main, false),
            YES => {
                style::reset_all();
                self.tiled = false;
                self.go(Page::Main, false);
                self.relook(cx);
            }
            i if (ANIM..ANIM + 3).contains(&i) => style::set_anim(Anim::ALL[(i - ANIM) as usize]),
            ALT_SPACE => {
                self.sys.search_key = !self.sys.search_key;
                let on = self.sys.search_key;
                cx.run(move || act(Act::AltSpace(on)));
            }
            OPEN_LOG => cx.run(move || act(Act::OpenLog)),
            RESTART => cx.close_then(move || act(Act::Restart)),
            CHECK_NOW => {
                if !matches!(self.upd, Upd::Busy(_)) {
                    self.upd = Upd::Busy(None);
                    cx.run(move || act(Act::CheckUpdate));
                }
            }
            AUTO_UPDATE => {
                if let Some(on) = (self.info.auto)() {
                    cx.run(move || act(Act::AutoUpdate(!on)));
                }
            }
            REMOVE => self.go(Page::Rollback, true),
            UNDO => self.go(Page::Upd, false),
            REMOVE_YES => {
                self.go(Page::Upd, false);
                cx.run(move || act(Act::Remove));
            }
            _ => {}
        }
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let order: Vec<i32> = self.items.iter().filter(|it| it.id != 0).map(|it| it.id).collect();
        let n = order.len();
        let at = from.and_then(|f| order.iter().position(|&o| o == f));
        let i = match (at, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n.checked_sub(1)?,
            (Some(i), Dir::Up | Dir::Left) => i.saturating_sub(1),
            (Some(i), Dir::Down | Dir::Right) => (i + 1).min(n - 1),
        };
        order.get(i).copied()
    }

    fn settle(&self) -> u32 {
        420
    }

    fn animating(&self, now: u64) -> bool {
        let page = self.page_at.is_none_or(|t| now < t + 420) || self.open_at.is_none_or(|t| now < t + 240);
        page || matches!(self.upd, Upd::Busy(_)) || matches!(self.upd, Upd::Done(t) if now < t + 400)
    }
}

impl SettingsPanel {
    /// One of the four switches of the bar page.
    fn behavior(&mut self, n: usize, cx: &mut Cx) {
        let text: &'static str = match n {
            0 => {
                let on = !style::autohide();
                style::set_autohide(on);
                if on {
                    t!("Панель прячется — наведите на край экрана", "The bar hides — point at the screen edge", ja: "バーが隠れます — 画面の端にポインターを", pt: "A barra se oculta — aponte para a borda", zh: "任务栏已隐藏 — 指向屏幕边缘")
                } else {
                    t!("Панель всегда на виду", "The bar stays visible", ja: "バーを常に表示", pt: "A barra fica visível", zh: "任务栏始终显示")
                }
            }
            1 => {
                let on = !style::align_left();
                style::set_align_left(on);
                if on {
                    t!("Кнопки прижаты влево", "Buttons are on the left", ja: "ボタンを左に配置しました", pt: "Botões à esquerda", zh: "按钮已靠左")
                } else {
                    t!("Кнопки по центру", "Buttons are centered", ja: "ボタンを中央に配置しました", pt: "Botões centralizados", zh: "按钮已居中")
                }
            }
            2 => {
                let on = !style::alttab_per_monitor();
                style::set_alttab_per_monitor(on);
                if on {
                    t!("Alt+Tab покажет окна только этого монитора", "Alt+Tab lists windows from this monitor only", ja: "Alt+Tab はこのモニターのウィンドウのみ", pt: "Alt+Tab mostra só as janelas deste monitor", zh: "Alt+Tab 仅显示此显示器的窗口")
                } else {
                    t!("Alt+Tab покажет окна всех мониторов", "Alt+Tab lists windows from every monitor", ja: "Alt+Tab はすべてのモニターのウィンドウ", pt: "Alt+Tab mostra as janelas de todos os monitores", zh: "Alt+Tab 显示所有显示器的窗口")
                }
            }
            _ => {
                let on = !crate::stick();
                crate::set_stick(on);
                if on {
                    t!("Окна прилипают к краям и друг к другу", "Windows snap to edges and each other", ja: "ウィンドウが端やほかのウィンドウに吸着します", pt: "As janelas grudam nas bordas", zh: "窗口会吸附到边缘和其他窗口")
                } else {
                    t!("Окна двигаются свободно", "Windows move freely", ja: "ウィンドウは自由に移動します", pt: "As janelas se movem livremente", zh: "窗口可自由移动")
                }
            }
        };
        self.say(cx, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(page: Page, themes: usize) -> SettingsPanel {
        let info = Info { version: "0.5.9".into(), auto: || None, act: |_| {}, system: System::default, wallpaper: String::new };
        let themes = (0..themes).map(|i| Theme { name: format!("T{i}"), style: Style::Classic, colors: [u32::MAX; 7], opacity: 85, accent: Accent::Blue }).collect();
        SettingsPanel::new(info, themes, page)
    }

    fn gfx() -> Gfx {
        // SAFETY: COM for WIC on this test thread.
        unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED).ok().ok() };
        Gfx::new(1.0).unwrap()
    }

    #[test]
    fn pages_are_as_tall_as_the_mockups() {
        let mut g = gfx();
        // measured from the mockup's DOM: the pages whose height does not depend on a setting or on how a text wraps
        for (page, themes, h) in [(Page::Main, 0, 656.0), (Page::Colors, 0, 532.0), (Page::Templates, 0, 240.0), (Page::Lang, 0, 326.0), (Page::Reset, 0, 230.0), (Page::Rollback, 0, 230.0)] {
            assert_eq!(panel(page, themes).size(&mut g), (392.0, h), "{page:?}");
        }
        // two templates: rows of 48 with a gap replace the empty note (56 + 8), and "delete a template" joins the save row (41)
        assert_eq!(panel(Page::Templates, 2).size(&mut g).1, 240.0 - 64.0 + 97.0 + 41.0);
    }

    #[test]
    fn the_main_page_answers_hits_and_keys() {
        let mut g = gfx();
        let mut p = panel(Page::Main, 0);
        p.size(&mut g);
        assert_eq!(p.hit(100.0, 257.0), Some(GO_COLORS));
        assert_eq!(p.hit(366.0, 628.0), Some(GO_RESET));
        assert_eq!(p.hit(10.0, 300.0), Some(GO_TEMPLATES)); // the row reaches the panel's padding
        assert_eq!(p.hit(100.0, 100.0), Some(THEME)); // the first theme card
        assert_eq!(p.hit(200.0, 230.5), None); // the rule
        assert_eq!(p.nav(None, Dir::Down), Some(THEME));
        assert_eq!(p.nav(Some(GO_RESET), Dir::Down), Some(GO_RESET));
    }

    #[test]
    fn a_page_change_resizes_the_panel_and_keeps_its_bottom_against_the_bar() {
        use crate::ui::panel::{Anchor, Popup};
        let mut g = gfx();
        let anchor = Anchor { side: Side::Bottom, button: (10, 846, 50, 886), bar: (0, 846, 1440, 900), screen: (1440, 900), origin: (0, 0) };
        let mut grab = |_x, _y, w: i32, h: i32| Some(vec![0xff10_1010u32; (w * h) as usize]);
        let mut p = Popup::new(&mut g, Box::new(panel(Page::Main, 0)), anchor, 0, &mut grab);
        let shown = p.frame(&mut g, 0);
        let (x, y, w, h) = p.rect();
        assert_eq!((w, h, y + h), (392, 656, 846 - 12)); // 12 above the bar, as the mockup's `bottom: var(--pb)`
        assert_eq!(x, 12);
        // a click on "Colors and transparency" (client coordinates: the room m around the box is in the window)
        p.button(x - shown.win.0 + 100, y - shown.win.1 + 257, Button::Left);
        p.frame(&mut g, 16);
        let (x2, y2, w2, h2) = p.rect();
        assert_eq!((x2, w2, h2), (12, 392, 532));
        assert_eq!(y2 + h2, 846 - 12);
        p.frame(&mut g, 400);
    }

    #[test]
    fn every_page_has_its_own_ids() {
        let mut g = gfx();
        for page in [Page::Main, Page::Colors, Page::Templates, Page::Lang, Page::Bar, Page::Reset, Page::Adv, Page::Upd, Page::Rollback] {
            let mut p = panel(page, 2);
            p.open = Some(0);
            p.size(&mut g);
            let mut ids: Vec<i32> = p.items.iter().map(|it| it.id).filter(|&i| i != 0).collect();
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), n, "{page:?} reuses an id");
            // the colour chips own a range of their own: the opacity segments and the reset row must not fall into it
            for id in ids.iter().filter(|&&i| (CHIP..CHIP + 140).contains(&i)) {
                assert!(matches!(p.items.iter().find(|it| it.id == *id).map(|it| &it.k), Some(K::Chip { .. } | K::ChipReset(_))), "{page:?}: {id}");
            }
        }
    }

    #[test]
    fn words_wrap_at_the_width() {
        let mut g = gfx();
        let mut m = |s: &str, f: Font| g.measure(s, f);
        let lines = wrap(&mut m, "Each release is checked against the WinRust key before it is installed", SMALL, 302.0);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| m(l, SMALL) <= 302.5));
        assert_eq!(wrap(&mut m, "short", SMALL, 302.0), ["short"]);
        // a text with no spaces breaks between letters
        let cjk = "每个版本在安装之前都会与密钥核对每个版本在安装之前都会与密钥核对每个版本在安装之前都会与密钥核对";
        assert!(wrap(&mut m, cjk, SMALL, 200.0).len() >= 2);
    }

    #[test]
    fn pages_parse_by_name() {
        assert_eq!(Page::parse("rollback"), Some(Page::Rollback));
        assert_eq!(Page::parse("nope"), None);
        assert_eq!(colorref(0x62a0ea), 0xea_a0_62);
    }
}
