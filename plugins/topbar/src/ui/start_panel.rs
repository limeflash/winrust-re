//! The Start menu, as in the Obsidian mockup: a search field, the pinned apps in a grid of six columns on pages (what the search
//! finds takes their place while something is typed), the files opened last, and a strip with the account and the settings and
//! power buttons. Glass at .72, radius 24, a lit top edge; it unrolls from the Start button (`stIn`) and its blocks rise in one
//! after another (`stItem`).
//!
//! The data is the machine's own (`crate::apps`): pinned apps, the Start menu and Store apps, the shell's Recent folder, the
//! Windows account. A file dropped on the panel is pinned.

use winrust_common::style::{Side, pal};

use super::anim::{EASE, SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Picture, Rect, Rgba};
use super::icons;
use super::menu::Item;
use super::panel::{Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Inset, Look, Mods, Origin, Place, Pose, Shadow, Along};
use crate::apps::{self, App, Recent};

pub const W: f32 = 648.0;
pub const H: f32 = 569.0;
const SEARCH: Rect = Rect::new(16.0, 16.0, 616.0, 48.0);
const TITLE_Y: f32 = 80.0;
const GRID: Rect = Rect::new(16.0, 116.0, 616.0, 216.0);
const RECENT_Y: f32 = 344.0;
const RECENT_GRID_Y: f32 = 380.0;
const BOTTOM_Y: f32 = 508.0;
/// Pins on a page: six columns, two rows.
const PER_PAGE: usize = 12;
const COLS: usize = 6;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const MUTED: Rgba = Rgba::css(163, 169, 184, 1.0);
const DIM: Rgba = Rgba::css(138, 144, 162, 1.0);
const PLACEHOLDER: Rgba = Rgba::css(117, 117, 117, 1.0);

// item ids
const FIELD: i32 = 1000;
const DOT: i32 = 1010; // + page
const SHOW_ALL: i32 = 1100;
const RECENT: i32 = 1200; // + n
const USER: i32 = 1300;
const SETTINGS: i32 = 1301;
const POWER: i32 = 1302;
// tween keys that are not item ids
const PAGE_KEY: i32 = 2000;

pub struct StartPanel {
    query: String,
    /// The field has been used: it shows its focus ring and caret.
    focus: bool,
    /// Ctrl+A: the next character replaces the text.
    all: bool,
    page: usize,
    /// What the grid shows: the pinned apps, every app when nothing is pinned, or what the query found.
    items: Vec<App>,
    /// Nothing is pinned: the grid lists every app.
    all_apps: bool,
    /// The first result (or the one the keys moved to) while typing.
    sel: usize,
    recent: Vec<Recent>,
    recent_view: bool,
    recent_filter: usize,
    recent_scroll: usize,
    filter_rects: [Rect; 4],
    pictures: std::collections::HashMap<String, Option<Picture>>,
    user: (String, String),
    gen_seen: u32,
    button: Rect,
    side: Side,
    last_hover: Option<i32>,
    /// Widths measured when drawn, for the hit tests.
    user_w: f32,
    show_w: f32,
}

impl StartPanel {
    #[must_use]
    pub fn new() -> Self {
        let mut p = Self::blank(apps::recent(60), apps::user());
        p.update();
        p
    }

    fn blank(recent: Vec<Recent>, user: (String, String)) -> Self {
        Self {
            query: String::new(),
            focus: false,
            all: false,
            page: 0,
            items: Vec::new(),
            all_apps: false,
            sel: 0,
            recent,
            recent_view: false,
            recent_filter: 0,
            recent_scroll: 0,
            filter_rects: [Rect::new(0.0, 0.0, 0.0, 0.0); 4],
            pictures: std::collections::HashMap::new(),
            user,
            gen_seen: 0,
            button: Rect::new(W / 2.0 - 21.0, H + 12.0, 42.0, 42.0),
            side: Side::Bottom,
            last_hover: None,
            user_w: 190.0,
            show_w: 61.0,
        }
    }

    /// The same panel with its data made up (the preview renderer's: the machine's own pins are whatever they are).
    #[must_use]
    pub fn with_state(query: &str, page: usize, items: Vec<App>, recent: Vec<Recent>, user: (String, String)) -> Self {
        let mut p = Self::blank(recent, user);
        (p.query, p.focus, p.page, p.items) = (query.to_string(), !query.is_empty(), page, items);
        p.gen_seen = apps::GEN.load(std::sync::atomic::Ordering::Relaxed);
        p
    }

    pub fn with_recent_view(mut self, filter: usize) -> Self {
        self.recent_view = true;
        self.recent_filter = filter.min(3);
        self
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Takes the apps the panel shows from the machine: what the query finds, else what is pinned (else everything).
    fn update(&mut self) {
        self.gen_seen = apps::GEN.load(std::sync::atomic::Ordering::Relaxed);
        self.items = if self.searching() {
            let mut v = apps::search(&self.query);
            v.truncate(PER_PAGE);
            v
        } else {
            let p = apps::pinned();
            self.all_apps = p.is_empty();
            if p.is_empty() { apps::all() } else { p }
        };
        self.page = self.page.min(self.pages() - 1);
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        // every page's icons start loading now, this page first: paging must never wait for the shell
        let from = self.page * PER_PAGE;
        apps::warm_icons(self.items[from.min(self.items.len())..].iter().chain(&self.items[..from.min(self.items.len())]).map(|a| a.path.clone()));
    }

    fn pages(&self) -> usize {
        self.items.len().div_ceil(PER_PAGE).max(1)
    }

    fn set_query(&mut self, q: String) {
        self.query = q;
        self.page = 0;
        self.sel = 0;
        apps::find_loose_exes(&self.query);
        self.update();
    }

    // ---- geometry ----

    /// A pin's box on its page when the pages stand at `page` (a fraction while they slide).
    fn pin_rect(i: usize, page: f32) -> Rect {
        let (p, k) = (i / PER_PAGE, i % PER_PAGE);
        let (col, row) = ((k % COLS) as f32, (k / COLS) as f32);
        Rect::new(GRID.x + 4.0 + col * 102.0 + (p as f32 - page) * GRID.w, GRID.y + row * 112.0, 98.0, 104.0)
    }

    fn dots(&self) -> Vec<Rect> {
        let n = self.pages();
        let x0 = 624.0 - (n as f32 * 28.0 + (n as f32 - 1.0) * 4.0);
        (0..n).map(|i| Rect::new(x0 + i as f32 * 32.0, TITLE_Y + 2.0, 28.0, 28.0)).collect()
    }

    fn show_all_rect(&self) -> Rect {
        Rect::new(624.0 - self.show_w, RECENT_Y + 2.0, self.show_w, 28.0)
    }

    fn recent_rect(n: usize) -> Rect {
        Rect::new(16.0 + (n % 2) as f32 * 310.0, RECENT_GRID_Y + (n / 2) as f32 * 60.0, 306.0, 56.0)
    }

    fn user_rect(&self) -> Rect {
        Rect::new(16.0, BOTTOM_Y + 9.0, self.user_w, 44.0)
    }

    fn settings_rect() -> Rect {
        Rect::new(W - 16.0 - 84.0, BOTTOM_Y + 11.0, 40.0, 40.0)
    }

    fn power_rect() -> Rect {
        Rect::new(W - 16.0 - 40.0, BOTTOM_Y + 11.0, 40.0, 40.0)
    }

    // ---- the blocks ----

    fn draw_field(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, accent: Rgba) {
        let on = f.tween(FIELD, 1, f32::from(self.focus), 160, EASE);
        c.fill(SEARCH, 14.0, Rgba::white(0.06 + 0.03 * on));
        c.ring(SEARCH, 14.0, 1.5, accent.fade(on));
        c.icon(icons::SEARCH, (SEARCH.x + 16.0 + 9.0, SEARCH.cy()), 18.0, 2.0, MUTED, 0.0);

        // the two keys at the right end: Alt Space (what opens WinRust's own search)
        let kf = Font::new(11.0, 500);
        let mut x = SEARCH.x + SEARCH.w - 8.0 - 4.0;
        for key in ["Space", "Alt"] {
            let w = c.measure(key, kf) + 12.0;
            x -= w;
            kbd(c, Rect::new(x, SEARCH.cy() - 8.65, w, 17.3), key);
            x -= 4.0;
        }
        let tx = SEARCH.x + 46.0;
        let text = Rect::new(tx, SEARCH.cy() - 20.0, x - 8.0 - tx, 40.0);
        let font = Font::new(15.0, 400);
        if self.query.is_empty() {
            c.text_fit(t!("Поиск приложений, файлов, параметров", "Search apps, files, settings", ja: "アプリ、ファイル、設定を検索", pt: "Pesquisar apps, arquivos, configurações", zh: "搜索应用、文件、设置"), text, font, PLACEHOLDER, Align::Start);
        } else {
            let w = c.measure(&self.query, font);
            if self.all {
                c.fill(Rect::new(tx, SEARCH.cy() - 10.0, w.min(text.w), 20.0), 2.0, accent.alpha(0.45));
            }
            c.text_fit(&self.query, text, font, INK, Align::Start);
        }
        // the caret blinks while the field has the keys
        if self.focus && (f.now / 530).is_multiple_of(2) {
            let w = c.measure(&self.query, font).min(text.w);
            c.fill(Rect::new(tx + w, SEARCH.cy() - 9.0, 1.0, 18.0), 0.0, accent);
        }
    }

    fn draw_title(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, accent: Rgba) {
        let title = if self.searching() {
            t!("Лучшие совпадения", "Best matches", ja: "最も一致する検索結果", pt: "Melhores correspondências", zh: "最佳匹配")
        } else if self.all_apps {
            t!("Все приложения", "All apps", ja: "すべてのアプリ", pt: "Todos os apps", zh: "所有应用")
        } else {
            t!("Закреплено", "Pinned", ja: "固定済み", pt: "Fixados", zh: "已固定")
        };
        c.text(title, Rect::new(24.0, TITLE_Y, 300.0, 32.0), Font::new(13.0, 600), INK, Align::Start);
        for (i, r) in self.dots().into_iter().enumerate() {
            let on = f32::from(i == self.page);
            let w = f.tween(DOT + i as i32, 1, 6.0 + 12.0 * on, 300, SPRING);
            let k = f.tween(DOT + i as i32, 2, on, 200, EASE);
            c.fill(Rect::new(r.cx() - w / 2.0, r.cy() - 3.0, w, 6.0), 3.0, Rgba::white(0.25).mix(accent, k));
        }
    }

    fn draw_grid(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, accent: Rgba) {
        if self.items.is_empty() && self.searching() {
            let msg = tf!("Ничего не найдено по запросу «{}»", "Nothing found for “{}”", ja: "「{}」は見つかりませんでした", pt: "Nada encontrado para “{}”", zh: "未找到“{}”", self.query);
            c.text_fit(&msg, GRID.inset(8.0), Font::new(13.0, 400), DIM, Align::Center);
            return;
        }
        let page = f.tween(PAGE_KEY, 0, self.page as f32, 450, SPRING);
        let just = apps::just_pinned();
        let typing_sel = self.searching();
        c.clipped(GRID, |c| {
            for (i, app) in self.items.iter().enumerate() {
                let r = Self::pin_rect(i, page);
                if r.x + r.w < GRID.x || r.x > GRID.x + GRID.w {
                    continue;
                }
                let id = i as i32;
                let hot = f.hover(id, 160, SPRING).max(f32::from(typing_sel && i == self.sel));
                c.fill(r, 14.0, Rgba::white(0.07 * hot));
                if let Some((path, age)) = &just
                    && *path == app.path
                {
                    let k = 1.0 - *age as f32 / 1200.0;
                    c.fill(r, 14.0, accent.fade(0.45 * k * k));
                }
                // the picture lifts 3 px under the pointer and sinks to .94 under a press (`.pin2 .pic`)
                let lift = f.tween(id, 1, -3.0 * f32::from(f.hover == Some(id)), 250, SPRING);
                let k = f.tween(id, 2, if f.press == Some(id) { 0.94 } else { 1.0 }, 250, SPRING);
                let (cx, top) = (r.cx(), r.y + 15.8);
                // an icon still loading is left out for a moment (warm_icons was asked for it in `update`)
                if let Some(ic) = apps::icon_ready(&app.path) {
                    c.transformed(cx, top + 24.0, k, 0.0, lift, |c| c.hicon(ic, Rect::new(cx - 22.0, top + 2.0, 44.0, 44.0), 1.0));
                }
                c.text_fit(&app.name, Rect::new(r.x + 4.0, top + 58.0, r.w - 8.0, 14.4), Font::new(12.0, 400), INK, Align::Center);
            }
        });
    }

    fn draw_recent_head(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        c.text(t!("Недавние", "Recent", ja: "最近", pt: "Recentes", zh: "最近"), Rect::new(24.0, RECENT_Y, 300.0, 32.0), Font::new(13.0, 600), INK, Align::Start);
        let label = t!("Показать все", "Show all", ja: "すべて表示", pt: "Mostrar tudo", zh: "显示全部");
        let font = Font::new(12.0, 400);
        self.show_w = c.measure(label, font) + 16.0;
        let r = self.show_all_rect();
        c.fill(r, 8.0, Rgba::white(0.1 * f.hover(SHOW_ALL, 250, EASE)));
        c.text(label, r, font, MUTED, Align::Center);
    }

    fn draw_recent(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        if self.recent.is_empty() {
            let r = Rect::new(16.0, RECENT_GRID_Y, 616.0, 116.0);
            c.text(t!("Здесь появятся недавно открытые файлы", "Files you open will show up here", ja: "最近開いたファイルがここに表示されます", pt: "Os arquivos abertos recentemente aparecem aqui", zh: "最近打开的文件会显示在这里"), r, Font::new(13.0, 400), DIM, Align::Center);
            return;
        }
        for (n, it) in self.recent.iter().take(4).enumerate() {
            let r = Self::recent_rect(n);
            let id = RECENT + n as i32;
            c.fill(r, 12.0, Rgba::white(0.07 * f.hover(id, 160, SPRING)));
            let lift = f.tween(id, 1, -3.0 * f32::from(f.hover == Some(id)), 250, SPRING);
            let k = f.tween(id, 2, if f.press == Some(id) { 0.94 } else { 1.0 }, 250, SPRING);
            let tile = Rect::new(r.x + 8.0, r.cy() - 20.0, 40.0, 40.0);
            if Self::recent_kind(it) == 2 {
                self.pictures.entry(it.path.clone()).or_insert_with(|| c.image(&it.path, 40.0, 40.0));
            }
            let picture = self.pictures.get(&it.path).and_then(Option::as_ref);
            c.transformed(tile.cx(), tile.cy(), k, 0.0, lift, |c| {
                c.fill(tile, 10.0, Rgba::white(0.06));
                if let Some(p) = picture {
                    c.picture_cover(p, tile, 10.0, 1.0);
                } else if let Some(img) = apps::image(&it.path) {
                    let s = c.scale();
                    let (iw, ih) = (img.w as f32 / s, img.h as f32 / s);
                    if img.thumb {
                        // a thumbnail fills the tile (`object-fit: cover`)
                        let k = (40.0 / iw).max(40.0 / ih);
                        let dst = Rect::new(tile.cx() - iw * k / 2.0, tile.cy() - ih * k / 2.0, iw * k, ih * k);
                        c.clipped(tile, |c| c.hicon(img.icon, dst, 1.0));
                    } else {
                        c.hicon(img.icon, Rect::new(tile.cx() - 12.0, tile.cy() - 12.0, 24.0, 24.0), 1.0);
                    }
                }
            });
            let tx = r.x + 8.0 + 40.0 + 12.0;
            let tw = r.x + r.w - 8.0 - tx;
            c.text_fit(&it.name, Rect::new(tx, r.cy() - 15.6, tw, 15.7), Font::new(13.0, 500), INK, Align::Start);
            c.text_fit(&it.when, Rect::new(tx, r.cy() + 1.0, tw, 14.4), Font::new(12.0, 400), DIM, Align::Start);
        }
    }

    fn draw_bottom(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, accent: Rgba) {
        c.fill(Rect::new(0.0, BOTTOM_Y, W, H - BOTTOM_Y), 0.0, Rgba::css(0, 0, 0, 0.2));
        c.fill(Rect::new(0.0, BOTTOM_Y, W, 1.0), 0.0, Rgba::white(0.06));
        let cy = BOTTOM_Y + 1.0 + 30.0;

        // the account: a round badge in the accent and two lines
        let (name, sub) = (&self.user.0, &self.user.1);
        let (nf, sf) = (Font::new(13.0, 500), Font::new(12.0, 400));
        let tw = c.measure(name, nf).max(c.measure(sub, sf)).ceil().min(230.0);
        let r = Rect::new(16.0, cy - 22.0, 6.0 + 32.0 + 12.0 + tw + 12.0, 44.0);
        self.user_w = r.w;
        c.fill(r, 12.0, Rgba::white(0.1 * f.hover(USER, 250, EASE)));
        let (bx, by) = (r.x + 6.0 + 16.0, cy);
        let face = Rect::new(bx - 16.0, by - 16.0, 32.0, 32.0);
        let picture = apps::avatar();
        if picture.is_invalid() {
            c.circle(bx, by, 16.0, accent.alpha(0.2));
            let initial: String = name.chars().next().into_iter().flat_map(char::to_uppercase).collect();
            c.text(&initial, face, Font::new(13.0, 600), accent, Align::Center);
        } else {
            c.hicon(picture, face, 1.0);
        }
        c.ring(face, 16.0, 1.0, accent.alpha(0.4));
        let tx = r.x + 6.0 + 32.0 + 12.0;
        c.text_fit(name, Rect::new(tx, cy - 15.6, tw, 15.7), nf, INK, Align::Start);
        // the footer's own line (a file was pinned) takes the second line for a moment
        match apps::current_note() {
            Some(note) => c.text_fit(&note, Rect::new(tx, cy + 1.0, (W - 16.0 - 84.0 - 8.0 - tx).max(tw), 14.4), sf, accent, Align::Start),
            None => c.text_fit(sub, Rect::new(tx, cy + 1.0, tw, 14.4), sf, DIM, Align::Start),
        }

        for (id, r, glyph) in [(SETTINGS, Self::settings_rect(), icons::GEAR), (POWER, Self::power_rect(), icons::POWER)] {
            c.fill(r, 10.0, Rgba::white(0.1 * f.hover(id, 250, EASE)));
            c.icon(glyph, (r.cx(), r.cy()), 18.0, 1.8, MUTED, 0.0);
        }
    }

    fn recent_kind(it: &Recent) -> usize {
        if it.dir { return 3; }
        let ext = std::path::Path::new(&it.path).extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        if ["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff"].contains(&ext.as_str()) { 2 } else { 1 }
    }

    fn recent_indices(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.recent.iter().enumerate().filter(|(_, r)| (self.recent_filter == 0 || Self::recent_kind(r) == self.recent_filter) && r.name.to_lowercase().contains(&q)).map(|(i, _)| i).collect()
    }

    fn draw_recent_page(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, progress: f32) {
        let back = Rect::new(16.0, 76.0, 100.0, 32.0);
        c.fill(back, 9.0, Rgba::white(0.07 * f.hover(3000, 140, EASE)));
        c.icon("M15 6l-6 6 6 6", (28.0, 92.0), 16.0, 2.2, INK, 0.0);
        c.text(t!("Недавние", "Recent", ja: "最近", pt: "Recentes", zh: "最近"), Rect::new(40.0, 76.0, 76.0, 32.0), Font::new(13.0, 600), INK, Align::Start);
        let labels = [t!("Все", "All", ja: "すべて", pt: "Todos", zh: "全部"), t!("Документы", "Documents", ja: "文書", pt: "Documentos", zh: "文档"), t!("Изображения", "Images", ja: "画像", pt: "Imagens", zh: "图片"), t!("Папки", "Folders", ja: "フォルダー", pt: "Pastas", zh: "文件夹")];
        let mut x = 624.0;
        for k in (0..4).rev() {
            let count = self.recent.iter().filter(|r| k == 0 || Self::recent_kind(r) == k).count();
            let text = format!("{}  {}", labels[k], count);
            let w = c.measure(&text, Font::new(12.0, 500)) + 22.0;
            x -= w;
            let r = Rect::new(x, 78.0, w, 28.0);
            self.filter_rects[k] = r;
            c.fill(r, 14.0, Rgba::white(if self.recent_filter == k { 0.14 } else { 0.05 }));
            c.text(&text, r, Font::new(12.0, 500), MUTED, Align::Center);
            x -= 4.0;
        }
        let indices = self.recent_indices();
        let area = Rect::new(16.0, 116.0, 616.0, 384.0);
        for &i in indices.iter().skip(self.recent_scroll).take(6) {
            let item = &self.recent[i];
            if Self::recent_kind(item) == 2 { self.pictures.entry(item.path.clone()).or_insert_with(|| c.image(&item.path, 40.0, 40.0)); }
        }
        c.clipped(area, |c| {
            let mut y = 116.0;
            let mut last_day = None;
            for (row, &i) in indices.iter().skip(self.recent_scroll).take(6).enumerate() {
                let it = &self.recent[i];
                if last_day != Some(it.day) {
                    let title = match it.day { 0 => t!("Сегодня", "Today", ja: "今日", pt: "Hoje", zh: "今天"), 1 => t!("Вчера", "Yesterday", ja: "昨日", pt: "Ontem", zh: "昨天"), _ => t!("Ранее", "Earlier", ja: "以前", pt: "Anteriormente", zh: "更早") };
                    c.text(title, Rect::new(26.0, y, 300.0, 28.0), Font::new(11.5, 600), DIM, Align::Start);
                    y += 28.0;
                    last_day = Some(it.day);
                }
                let r = Rect::new(16.0, y, 616.0, 52.0);
                y += 52.0;
                let hot = f.hover(10000 + i as i32, 140, EASE).max(f32::from(f.hover == Some(1000000 + i as i32) || f.hover == Some(2000000 + i as i32)));
                let delay = row as f32 * 0.035;
                let appear = ((progress - delay) / (1.0 - delay)).clamp(0.0, 1.0);
                c.faded(appear, |c| c.transformed(0.0, 0.0, 1.0, 0.0, 6.0 * (1.0 - appear), |c| {
                c.fill(r, 12.0, Rgba::white(0.07 * hot));
                let tile = Rect::new(r.x + 8.0, r.y + 8.0, 36.0, 36.0);
                c.fill(tile, 9.0, Rgba::white(0.06));
                if let Some(Some(picture)) = self.pictures.get(&it.path) {
                    c.picture_cover(picture, tile, 9.0, 1.0);
                } else if let Some(img) = apps::image(&it.path) {
                    if img.thumb {
                        let k = (36.0 / img.w as f32).max(36.0 / img.h as f32);
                        let dst = Rect::new(tile.cx() - img.w as f32 * k / 2.0, tile.cy() - img.h as f32 * k / 2.0, img.w as f32 * k, img.h as f32 * k);
                        c.clipped(tile, |c| c.hicon(img.icon, dst, 1.0));
                    } else { c.hicon(img.icon, Rect::new(tile.x + 7.0, tile.y + 7.0, 22.0, 22.0), 1.0); }
                } else {
                    c.icon(if Self::recent_kind(it) == 3 { FOLDER } else { "M6 3h8l4 4v14H6zM14 3v5h4M9 12h6M9 16h6" }, (tile.cx(), tile.cy()), 22.0, 1.7, MUTED, 0.0);
                }
                c.text_fit(&it.name, Rect::new(72.0, r.y + 8.0, 400.0, 18.0), Font::new(13.0, 500), INK, Align::Start);
                let parent = std::path::Path::new(&it.path).parent().map_or_else(String::new, |p| p.to_string_lossy().into_owned());
                c.text_fit(&parent, Rect::new(72.0, r.y + 27.0, 400.0, 17.0), Font::new(12.0, 400), DIM, Align::Start);
                if hot > 0.01 {
                    c.icon(FOLDER, (581.0, r.cy()), 15.0, 1.9, MUTED, 0.0);
                    c.icon("M7 7l10 10M17 7L7 17", (611.0, r.cy()), 11.0, 2.6, MUTED, 0.0);
                } else { c.text_fit(it.when.rsplit_once(", ").map_or(it.when.as_str(), |(_, time)| time), Rect::new(475.0, r.y, 140.0, 52.0), Font::new(12.0, 400), DIM, Align::End); }
                }));
            }
            if indices.is_empty() { c.text(t!("Здесь пока ничего нет", "Nothing here yet", ja: "ここにはまだ何もありません", pt: "Nada por aqui ainda", zh: "这里还没有内容"), Rect::new(16.0, 200.0, 616.0, 100.0), Font::new(13.0, 500), DIM, Align::Center); }
        });
        if indices.len() > 6 {
            let h = (area.h * 6.0 / indices.len() as f32).max(20.0);
            let y = area.y + (area.h - h) * self.recent_scroll as f32 / (indices.len() - 6) as f32;
            c.fill(Rect::new(628.0, y, 3.0, h), 1.5, Rgba::white(0.14));
        }
    }

    /// The clip the reveal starts from: a 44 px square at the button, `dx`/`dy` to slide in from.
    fn reveal(&self) -> (Inset, f32, f32) {
        let (b, h) = (self.button, H);
        match self.side {
            Side::Bottom => {
                let cx = b.cx().clamp(22.0, W - 22.0);
                (Inset::new(h - 44.0, W - (cx + 22.0), 0.0, cx - 22.0, 12.0), 0.0, 44.0)
            }
            Side::Left => {
                let cy = b.cy().clamp(22.0, h - 22.0);
                (Inset::new(cy - 22.0, W - 44.0, h - (cy + 22.0), 0.0, 12.0), -44.0, 0.0)
            }
            Side::Right => {
                let cy = b.cy().clamp(22.0, h - 22.0);
                (Inset::new(cy - 22.0, 0.0, h - (cy + 22.0), W - 44.0, 12.0), 44.0, 0.0)
            }
        }
    }
}

impl Default for StartPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// `<kbd>` of the mockup: 11 px, a faint plate with a lit lower edge.
fn kbd(c: &mut Canvas<'_>, r: Rect, key: &str) {
    c.fill(r, 4.0, Rgba::white(0.08));
    c.inset_edge(r, 4.0, -1.0, Rgba::white(0.1));
    c.text(key, r, Font::new(11.0, 500), MUTED, Align::Center);
}

impl Content for StartPanel {
    fn id(&self) -> Id {
        Id::Start
    }

    fn look(&self) -> Look {
        Look::glass(24.0, 0.72, Shadow { dy: 40.0, blur: 100.0, color: Rgba::css(0, 0, 0, 0.6) }).lit_top(0.08)
    }

    fn place(&self) -> Place {
        Place { along: Along::Screen, gap: 12.0 }
    }

    fn placed(&mut self, button: Rect, side: Side) {
        self.button = button;
        self.side = side;
    }

    fn entry(&self) -> Entry {
        let (clip, dx, dy) = self.reveal();
        Entry { from: Pose { opacity: 0.9, dx, dy, k: 1.0, clip: Some(clip) }, dur: 380, ease: SNAP, origin: Origin::Frac(0.5, 1.0) }
    }

    fn exit(&self) -> Exit {
        let (clip, dx, dy) = self.reveal();
        Exit { to: Pose { opacity: 0.6, dx, dy, k: 1.0, clip: Some(clip) }, dur: 240, ease: super::anim::EXIT, origin: Origin::Frac(0.5, 1.0) }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        (W, H)
    }

    fn settle(&self) -> u32 {
        500 // the last block rises in at 150 ms + 340 ms
    }

    /// The field's caret blinks: a frame every 106 ms (a fifth of the blink) shows it; a frame on every tick of the screen would
    /// repaint the panel 360 times a second for it.
    fn pulse(&self) -> u32 {
        if self.focus { 106 } else { 0 }
    }

    fn animating(&self, _now: u64) -> bool {
        apps::just_pinned().is_some() || apps::current_note().is_some() || apps::GEN.load(std::sync::atomic::Ordering::Relaxed) != self.gen_seen
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        if apps::GEN.load(std::sync::atomic::Ordering::Relaxed) != self.gen_seen {
            self.update();
        }
        // the keys moved the hover to a pin on another page: the pages follow
        // (and what Enter opens follows the pointer or the keys when they move to another pin, not while they rest)
        if f.hover != self.last_hover {
            self.last_hover = f.hover;
            if let Some(h) = f.hover.filter(|&h| (h as usize) < self.items.len() && h < FIELD) {
                self.sel = h as usize;
                self.page = h as usize / PER_PAGE;
            }
        }
        if self.recent_view && let Some(id) = f.hover.filter(|h| (10000..1000000).contains(h)) && let Some(pos) = self.recent_indices().iter().position(|&i| 10000 + i as i32 == id) {
            if pos < self.recent_scroll { self.recent_scroll = pos; }
            if pos >= self.recent_scroll + 6 { self.recent_scroll = pos.saturating_sub(5); }
        }
        let accent = Rgba::colorref(pal().accent);
        let origin = |bottom: f32| (SEARCH.x + 0.28 * SEARCH.w, bottom);
        // `.stOut .sti{ opacity: 0 }` over .12 s: the blocks go before the glass does
        let fade = f.fade_out(120);
        c.faded(fade, |c| {
            // each block rises in after its own delay (`animation-delay` of the mockup)
            let rise = |f: &Frame<'_>, delay| f.rise(delay, 340, SNAP, 18.0, 0.96);
            let r = rise(f, 0);
            r.draw(c, origin(SEARCH.y + SEARCH.h), |c| self.draw_field(c, f, accent));
            let view = f.tween(7000, 0, f32::from(self.recent_view), 340, SNAP);
            if view > 0.0 { c.faded(view, |c| c.transformed(0.0, 0.0, 1.0, 22.0 * (1.0 - view), 0.0, |c| self.draw_recent_page(c, f, view))); }
            if view < 1.0 { c.faded(1.0 - view, |c| c.transformed(0.0, 0.0, 1.0, -22.0 * view, 0.0, |c| {
            let r = rise(f, 70);
            r.draw(c, origin(TITLE_Y + 32.0), |c| self.draw_title(c, f, accent));
            let r = rise(f, 90);
            r.draw(c, origin(GRID.y + GRID.h), |c| self.draw_grid(c, f, accent));
            let r = rise(f, 110);
            r.draw(c, origin(RECENT_Y + 32.0), |c| self.draw_recent_head(c, f));
            let r = rise(f, 130);
            r.draw(c, origin(RECENT_GRID_Y + 116.0), |c| self.draw_recent(c, f));
            })); }
            let r = rise(f, 150);
            r.draw(c, origin(H), |c| self.draw_bottom(c, f, accent));
        });
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        if SEARCH.contains(x, y) {
            return Some(FIELD);
        }
        if self.recent_view {
            if Rect::new(16.0, 76.0, 100.0, 32.0).contains(x, y) { return Some(3000); }
            if let Some(k) = self.filter_rects.iter().position(|r| r.contains(x, y)) { return Some(3100 + k as i32); }
            if (116.0..500.0).contains(&y) && (16.0..632.0).contains(&x) {
                let mut top = 116.0;
                let mut day = None;
                for i in self.recent_indices().into_iter().skip(self.recent_scroll).take(6) {
                    if day != Some(self.recent[i].day) { top += 28.0; day = Some(self.recent[i].day); }
                    if y >= top && y < top + 52.0 { return Some(if x >= 596.0 { 2000000 + i as i32 } else if x >= 566.0 { 1000000 + i as i32 } else { 10000 + i as i32 }); }
                    top += 52.0;
                }
            }
            return [(USER, self.user_rect()), (SETTINGS, Self::settings_rect()), (POWER, Self::power_rect())].into_iter().find(|(_, r)| r.contains(x, y)).map(|(id, _)| id);
        }
        if GRID.contains(x, y) {
            let i = (0..self.items.len()).filter(|i| i / PER_PAGE == self.page).find(|&i| Self::pin_rect(i, self.page as f32).contains(x, y))?;
            return Some(i as i32);
        }
        if let Some(k) = self.dots().iter().position(|r| r.contains(x, y)) {
            return Some(DOT + k as i32);
        }
        if self.show_all_rect().contains(x, y) {
            return Some(SHOW_ALL);
        }
        if let Some(n) = (0..self.recent.len().min(4)).find(|&n| Self::recent_rect(n).contains(x, y)) {
            return Some(RECENT + n as i32);
        }
        [(USER, self.user_rect()), (SETTINGS, Self::settings_rect()), (POWER, Self::power_rect())].into_iter().find(|(_, r)| r.contains(x, y)).map(|(id, _)| id)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        match id {
            FIELD => self.focus = true,
            3000 => { self.recent_view = false; self.set_query(String::new()); },
            n if (3100..3104).contains(&n) => { self.recent_filter = (n - 3100) as usize; self.recent_scroll = 0; },
            n if (10000..1000000).contains(&n) => { if let Some(path) = self.recent.get((n - 10000) as usize).map(|r| r.path.clone()) { cx.close_then(move || open_str(&path)); } },
            n if (1000000..2000000).contains(&n) => { if let Some(path) = self.recent.get((n - 1000000) as usize).map(|r| r.path.clone()) { cx.close_then(move || winrust_common::reveal(&path)); } },
            n if (2000000..3000000).contains(&n) => { let i = (n - 2000000) as usize; if let Some(item) = self.recent.get(i) && apps::remove_recent(&item.path) { self.recent.remove(i); self.recent_scroll = self.recent_scroll.min(self.recent_indices().len().saturating_sub(6)); } },
            SHOW_ALL => { self.recent_view = true; self.recent_filter = 0; self.recent_scroll = 0; },
            USER => cx.close_then(|| open_str("ms-settings:yourinfo")),
            SETTINGS => cx.close_then(|| open_str("ms-settings:")),
            POWER => cx.close_then(power_menu),
            n if (DOT..DOT + 20).contains(&n) => self.page = ((n - DOT) as usize).min(self.pages() - 1),
            n if (RECENT..RECENT + 4).contains(&n) => {
                if let Some(path) = self.recent.get((n - RECENT) as usize).map(|r| r.path.clone()) {
                    cx.close_then(move || open_str(&path));
                }
            }
            n => {
                let Some(app) = self.items.get(n as usize).cloned() else { return };
                match b {
                    Button::Left => cx.close_then(move || apps::launch(&app)),
                    Button::Right => cx.run(move || app_menu(app)),
                    Button::Middle => {}
                }
            }
        }
    }

    fn enter(&mut self, hover: Option<i32>, _mods: Mods, cx: &mut Cx) {
        if self.recent_view { if let Some(id) = hover.filter(|h| (10000..1000000).contains(h)) { self.click(id, Button::Left, cx); } return; }
        let i = if self.searching() { Some(self.sel) } else { hover.filter(|&h| h < FIELD).map(|h| h as usize) };
        if let Some(i) = i {
            self.click(i as i32, Button::Left, cx);
        }
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        if self.recent_view {
            let indices = self.recent_indices();
            if indices.is_empty() { return None; }
            let at = from.and_then(|h| indices.iter().position(|i| 10000 + *i as i32 == h));
            let next = match dir { Dir::First => 0, Dir::Last => indices.len() - 1, Dir::Up | Dir::Left => at.unwrap_or(0).saturating_sub(1), Dir::Down | Dir::Right => at.map_or(0, |n| (n + 1).min(indices.len() - 1)) };
            return Some(10000 + indices[next] as i32);
        }
        let n = self.items.len() as i32;
        if n == 0 {
            return None;
        }
        let at = from.filter(|&f| f < FIELD).unwrap_or(if self.searching() { self.sel as i32 } else { -1 });
        let i = match (at, dir) {
            (_, Dir::First) | (-1, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (-1, Dir::Up | Dir::Left) => n - 1,
            (i, Dir::Left) => i - 1,
            (i, Dir::Right) => i + 1,
            (i, Dir::Up) => i - COLS as i32,
            (i, Dir::Down) => i + COLS as i32,
        };
        Some(i.clamp(0, n - 1))
    }

    fn typing(&self) -> bool {
        true
    }

    fn text(&mut self, ch: char, _cx: &mut Cx) {
        let all = std::mem::replace(&mut self.all, false);
        self.focus = true;
        let mut q = std::mem::take(&mut self.query);
        match ch {
            '\u{1}' => {
                self.all = !q.is_empty(); // Ctrl+A
            }
            '\u{7f}' => {
                // Ctrl+Backspace: the last word
                if all {
                    q.clear();
                } else {
                    let end = q.trim_end().len();
                    q.truncate(q[..end].rfind(' ').map_or(0, |i| i + 1));
                }
            }
            '\u{8}' => {
                if all {
                    q.clear();
                } else {
                    q.pop();
                }
            }
            ch => {
                if all {
                    q.clear();
                }
                q.push(ch);
            }
        }
        self.set_query(q);
    }

    fn escape(&mut self) -> bool {
        if self.recent_view { self.recent_view = false; self.set_query(String::new()); return true; }
        if self.query.is_empty() {
            return false;
        }
        self.all = false;
        self.set_query(String::new()); // Esc clears the search first, the next one closes
        true
    }

    fn wheel(&mut self, _x: f32, y: f32, up: bool) {
        if self.recent_view { self.recent_scroll = if up { self.recent_scroll.saturating_sub(1) } else { (self.recent_scroll + 1).min(self.recent_indices().len().saturating_sub(6)) }; return; }
        if self.searching() || !(GRID.y..GRID.y + GRID.h).contains(&y) {
            return;
        }
        self.page = if up { self.page.saturating_sub(1) } else { (self.page + 1).min(self.pages() - 1) };
    }

    fn files(&mut self, paths: Vec<String>, _cx: &mut Cx) {
        apps::pin_dropped(&paths);
        self.query.clear();
        self.update();
        if let Some((path, _)) = apps::just_pinned()
            && let Some(i) = self.items.iter().position(|a| a.path == path)
        {
            self.page = i / PER_PAGE;
        }
    }

    fn keeps_open_on_drag(&self) -> bool {
        true
    }
}

fn open_str(what: &str) {
    // SAFETY: `open` hands the path to the shell.
    unsafe { crate::open(&crate::wide(what)) };
}

const OPEN: &str = "M5 4l14 8-14 8z";
const SHIELD: &str = "M12 3l7 3v5c0 5-3 8.5-7 10-4-1.5-7-5-7-10V6z";
const FOLDER: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6z";
const PIN: &str = "M9 4h6l-1 6 3 3H7l3-3zM12 13v7";

/// Opens a menu next to the pointer along the bar, like a right click on the bar does.
fn menu_at_pointer(items: Vec<Item>, width: f32, pick: impl Fn(usize) + 'static) {
    // SAFETY: the cursor position and the bar's own anchor.
    unsafe {
        let at = crate::cursor();
        if let Some(a) = crate::menus::anchor_at(if crate::vert() { at.1 } else { at.0 }) {
            crate::menus::menu_at(items, width, a, pick);
        }
    }
}

/// The right-click menu of an app: open, open as administrator, show its folder, pin or unpin.
fn app_menu(app: App) {
    let pinned = apps::start_pinned(&crate::wstr_of(&app.path));
    let file = apps::is_file(&app);
    let mut items = vec![Item::new(OPEN, t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开"))];
    let mut acts = vec![0];
    if file {
        items.push(Item::new(SHIELD, t!("Запуск от имени администратора", "Run as administrator", ja: "管理者として実行", pt: "Executar como administrador", zh: "以管理员身份运行")));
        acts.push(1);
        items.push(Item::new(FOLDER, t!("Расположение файла", "Open file location", ja: "ファイルの場所を開く", pt: "Abrir local do arquivo", zh: "打开文件所在位置")));
        acts.push(2);
    }
    items.push(Item::new(PIN, if pinned {
        t!("Убрать из Пуска", "Remove from Start", ja: "スタートから削除", pt: "Remover do Iniciar", zh: "从开始菜单移除")
    } else {
        t!("Добавить в Пуск", "Add to Start", ja: "スタートに追加", pt: "Adicionar ao Iniciar", zh: "添加到开始菜单")
    }));
    acts.push(3);
    menu_at_pointer(items, 288.0, move |i| match acts[i] {
        0 => apps::launch(&app),
        1 => apps::launch_admin(&app),
        2 => apps::reveal(&app),
        _ => apps::start_toggle(&crate::wstr_of(&app.path)),
    });
}

/// The power button: lock, sleep, restart, shut down (the last two ask first).
fn power_menu() {
    let items = vec![
        Item::new(crate::menus::LOCK, t!("Блокировка", "Lock", ja: "ロック", pt: "Bloquear", zh: "锁定")),
        Item::new(crate::menus::MOON, t!("Сон", "Sleep", ja: "スリープ", pt: "Suspender", zh: "睡眠")),
        Item::new(icons::RESTART, t!("Перезагрузка", "Restart", ja: "再起動", pt: "Reiniciar", zh: "重启")),
        Item::new(icons::POWER, t!("Завершение работы", "Shut down", ja: "シャットダウン", pt: "Desligar", zh: "关机")),
    ];
    menu_at_pointer(items, 288.0, |i| match i {
        0 => apps::lock(),
        1 => apps::sleep(),
        // SAFETY: the bar's own confirmation box.
        2 => unsafe { crate::power_click(crate::POW_R) },
        _ => unsafe { crate::power_click(crate::POW_S) },
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pins_fill_six_columns_and_two_rows_a_page() {
        let (a, b, c) = (StartPanel::pin_rect(0, 0.0), StartPanel::pin_rect(1, 0.0), StartPanel::pin_rect(6, 0.0));
        assert_eq!((a.x, a.y, a.w, a.h), (20.0, 116.0, 98.0, 104.0));
        assert_eq!(b.x - a.x, 102.0); // 98 and a gap of 4
        assert_eq!(c.y - a.y, 112.0); // 104 and a gap of 8
        assert_eq!(StartPanel::pin_rect(12, 0.0).x - a.x, 616.0); // the next page is one box to the right
        assert_eq!(StartPanel::pin_rect(12, 1.0).x, a.x);
    }

    #[test]
    fn the_blocks_stack_to_the_mockups_height() {
        assert_eq!(SEARCH.y + SEARCH.h + 16.0, TITLE_Y); // the title row's margin
        assert_eq!(TITLE_Y + 32.0 + 4.0, GRID.y);
        assert_eq!(GRID.y + GRID.h + 12.0, RECENT_Y);
        assert_eq!(RECENT_Y + 32.0 + 4.0, RECENT_GRID_Y);
        assert_eq!(RECENT_GRID_Y + 116.0 + 12.0, BOTTOM_Y);
        assert_eq!(BOTTOM_Y + 61.0, H);
    }

    #[test]
    fn typing_edits_the_query_and_escape_clears_it_before_it_closes() {
        let mut p = StartPanel::with_state("", 0, Vec::new(), Vec::new(), (String::new(), String::new()));
        let mut cx = Cx::default();
        for ch in "calc".chars() {
            p.text(ch, &mut cx);
        }
        assert_eq!(p.query, "calc");
        p.text('\u{8}', &mut cx);
        assert_eq!(p.query, "cal");
        p.text('\u{1}', &mut cx); // Ctrl+A, then a letter replaces everything
        p.text('x', &mut cx);
        assert_eq!(p.query, "x");
        p.text(' ', &mut cx);
        p.text('y', &mut cx);
        p.text('\u{7f}', &mut cx); // Ctrl+Backspace drops the last word
        assert_eq!(p.query, "x ");
        assert!(p.escape(), "the first Esc clears the field");
        assert!(p.query.is_empty());
        assert!(!p.escape(), "the next one closes the panel");
    }
}
#[cfg(test)]
mod recent_page_tests {
    use super::*;
    #[test]
    fn category_filters_and_counts() {
        let make = |name: &str, path: &str| Recent { name: name.into(), path: path.into(), when: String::new(), day: 0, dir: std::path::Path::new(path).is_dir() };
        let mut panel = StartPanel::blank(vec![make("document", "x.docx"), make("photo", "x.JPG"), make("folder", ".")], (String::new(), String::new()));
        assert_eq!(panel.recent_indices().len(), 3);
        for filter in 1..=3 { panel.recent_filter = filter; assert_eq!(panel.recent_indices().len(), 1); }
        panel.recent_filter = 0;
        panel.query = "PHOTO".into();
        assert_eq!(panel.recent_indices(), vec![1]);
    }
}
