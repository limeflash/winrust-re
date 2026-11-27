//! The file manager's state, with no window and no drawing: tabs, the folder shown and its rows, the selection, the fields, the sidebar,
//! the preview, and what a click or a key does to them. [`super::draw`] paints it, [`super::app`] feeds it the window's events, and a
//! [`Backend`] does what touches the machine.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use winrust_common::{t, tf};
use winrust_ui::{
    frame::{Cx, Id},
    gfx::{Picture, Rect},
    widgets::{
        field_model::TextField,
        list::Heading,
        list_model::{Col, Columns, ListState},
        scroll_model::Scroll,
    },
};

use super::{
    backend::{Backend, Clock, Do, Facts, Flag, JobView, Opts, Place, Places},
    look::{self, IconKind},
    text,
};
use crate::{Hist, Row};

// ---------------------------------------------------------------------------------------------------------------------
// the items of the window (all of the app's own ids are 0 and up)

pub const ID_BACK: Id = 1;
pub const ID_FWD: Id = 2;
pub const ID_UP: Id = 3;
pub const ID_ADDR_PILL: Id = 4;
pub const ID_ADDR: Id = 5;
pub const ID_SEARCH: Id = 6; // and 7: its clear button
pub const ID_VIEW: Id = 8; // 8 list, 9 icons
pub const ID_PV: Id = 10;
pub const ID_MORE: Id = 11;
pub const ID_NEWTAB: Id = 12;
pub const ID_TAB: Id = 20; // 20..60
pub const ID_TAB_X: Id = 60; // 60..100
pub const ID_CRUMB: Id = 100; // 100..140
pub const ID_SIDE: Id = 200; // the sidebar's scroll area; 201 its bar
pub const ID_PLACE: Id = 210; // 210..300
pub const ID_PC: Id = 300;
pub const ID_DRIVE: Id = 310; // 310..350
pub const ID_EJECT: Id = 350; // 350..390
pub const ID_COLS: Id = 400; // 400..528: the header's titles and grips
pub const ID_JOB_CANCEL: Id = 530;
pub const ID_PV_OPEN: Id = 531;
pub const ID_PV_MORE: Id = 532;
pub const ID_EMPTY_BTN: Id = 533;
pub const ID_REN: Id = 534;
pub const ID_CONTENT: Id = 536;
pub const ID_PV_BODY: Id = 537;
pub const ID_DEMO: Id = 540;
pub const ID_LIST: Id = 1000;

pub const MAX_TABS: usize = 40;

/// The window's menus (`Cx::menu` tags).
pub const TAG_FOLDER: u32 = 1;
pub const TAG_ITEM: u32 = 2;
pub const TAG_PLACE: u32 = 3;

/// "This PC": the folder that lists the drives. It is no path on disk.
#[must_use]
pub fn pc_dir() -> PathBuf {
    PathBuf::from("::pc")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    List,
    Icons,
}

/// A date heading of "Group by date": it stands before row `before`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    pub before: usize,
    pub label: String,
    pub count: usize,
}

/// The rename box of a row.
pub struct Ren {
    pub name: String,
    pub field: TextField,
    /// How many times a bad name was refused (the box shakes).
    pub bad: u32,
}

/// A picture a worker made: pixels (premultiplied BGRA) and, once the window has drawn it, the Direct2D bitmap.
pub struct Thumb {
    /// A program's own icon: drawn as it is, without the frame of a photograph.
    pub icon: bool,
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    pub pic: Option<Picture>,
}

/// What the last frame knew of where things are: what a drop or a key needs.
#[derive(Default)]
pub struct Geo {
    pub tabs: Vec<Rect>,
    pub crumbs: Vec<(Id, Rect, PathBuf)>,
    pub places: Vec<(Rect, PathBuf)>,
    /// The rows in view: their rectangle and row number.
    pub rows: Vec<(Rect, usize)>,
    pub rows_per_page: usize,
    pub lines_per_page: usize,
    pub content: Rect,
    pub list: Rect,
    pub more: Rect,
    pub size: (f32, f32),
}

impl Geo {
    /// Where row `i` was drawn (None when it is out of view).
    #[must_use]
    pub fn row_rect(&self, i: usize) -> Option<Rect> {
        self.rows.iter().find(|(_, k)| *k == i).map(|(r, _)| *r)
    }
}

/// Where a drop would land.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropAt {
    /// A crumb, a place of the sidebar, a drive.
    Place(PathBuf),
    /// A folder row.
    Row(usize, PathBuf),
    /// The folder shown.
    Here,
}

/// What a state of the design needs by hand after the window is up (`--render`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hook {
    #[default]
    None,
    /// The menu of the picked row where the design puts it.
    ItemMenu,
    /// The folder's menu from "More".
    FolderMenu,
}

pub struct Fm {
    pub be: Arc<dyn Backend>,
    pub hwnd: isize,
    // where we are
    pub dir: PathBuf,
    pub tabs: Vec<PathBuf>,
    pub active: usize,
    pub hist: Hist,
    // the listing
    pub rows: Vec<Row>,
    pub heads: Vec<Head>,
    /// "This PC": the drives, one per row of `rows`.
    pub drives: Vec<super::backend::DriveInfo>,
    pub list: ListState,
    pub cols: Columns,
    pub sort: (usize, bool),
    pub group: bool,
    pub folders_first: bool,
    pub hidden: bool,
    pub view: View,
    pub pv: bool,
    /// The results of this query are listed (None: a folder is).
    pub query: Option<String>,
    pub search_info: String,
    pub clock: Clock,
    pub era: u32,
    // fields
    pub addr: TextField,
    pub addr_edit: bool,
    pub addr_bad: bool,
    pub srch: TextField,
    pub ren: Option<Ren>,
    // the sidebar
    pub places: Places,
    pub side: Scroll,
    // pictures
    pub thumbs: HashMap<PathBuf, Thumb>,
    pub asked: HashSet<PathBuf>,
    // the preview
    pub facts: Option<(PathBuf, Facts)>,
    pub job: Option<JobView>,
    pub geo: Geo,
    pub title: String,
    pub title_sent: String,
    /// The pointer went down on this row and has not moved far enough to drag yet: (row, x, y).
    pub press: Option<(usize, f32, f32)>,
    pub dragging: bool,
    pub drop_hl: Option<DropAt>,
    pub fx_ctrl: std::cell::Cell<bool>,
    pub fx_move: std::cell::Cell<bool>,
    /// A tab being dragged by the pointer: its index.
    pub tab_drag: Option<usize>,
    /// When the folder last changed (the rows come in over 260 ms), and whether the first frame of that has been seen.
    pub nav_at: u64,
    pub nav_fresh: bool,
    pub hook: Hook,
    /// The menu that is open names these (a row's menu while the bar is asked; the folder's).
    pub menu_paths: Vec<PathBuf>,
    /// Space was a key of the window (a quick look): the space the keyboard types after it is not a letter to jump to.
    pub skip_space: bool,
    /// How the columns were set up last (0 folder, 1 This PC, 2 search hits): their widths are reset only when that changes.
    pub col_mode: u8,
}

impl Fm {
    #[must_use]
    pub fn new(be: Arc<dyn Backend>, dir: PathBuf) -> Self {
        let mut cols = Columns::new(vec![
            Col { title: String::new(), w: 0.0, min: 120.0, align_right: false },
            Col { title: String::new(), w: 152.0, min: 90.0, align_right: false },
            Col { title: String::new(), w: 128.0, min: 80.0, align_right: false },
            Col { title: String::new(), w: 84.0, min: 60.0, align_right: true },
        ]);
        cols.sort = Some((0, true));
        cols.flex = 0;
        let mut fm = Self {
            be,
            hwnd: 0,
            dir: dir.clone(),
            tabs: vec![dir],
            active: 0,
            hist: Hist::default(),
            rows: Vec::new(),
            heads: Vec::new(),
            drives: Vec::new(),
            list: ListState::default(),
            cols,
            sort: (0, true),
            group: false,
            folders_first: true,
            hidden: false,
            view: View::List,
            pv: true,
            query: None,
            search_info: String::new(),
            clock: Clock { now: 0, off: 0 },
            era: 0,
            addr: TextField::new(),
            addr_edit: false,
            addr_bad: false,
            srch: TextField::new(),
            ren: None,
            places: Places::default(),
            side: Scroll::default(),
            thumbs: HashMap::new(),
            asked: HashSet::new(),
            facts: None,
            job: None,
            geo: Geo::default(),
            title: String::new(),
            title_sent: String::new(),
            press: None,
            dragging: false,
            drop_hl: None,
            fx_ctrl: std::cell::Cell::new(false),
            fx_move: std::cell::Cell::new(false),
            tab_drag: None,
            nav_at: 0,
            nav_fresh: false,
            hook: Hook::None,
            menu_paths: Vec::new(),
            skip_space: false,
            col_mode: 9,
        };
        fm.set_titles();
        fm
    }

    // -----------------------------------------------------------------------------------------------------------------
    // what the window shows now

    #[must_use]
    pub fn at_pc(&self) -> bool {
        self.dir == pc_dir()
    }

    #[must_use]
    pub fn searching(&self) -> bool {
        self.query.is_some()
    }

    /// Inside a `.zip`: read-only here.
    #[must_use]
    pub fn in_zip(&self) -> bool {
        !self.at_pc() && crate::zipx::split_zip(&self.dir).is_some()
    }

    #[must_use]
    pub fn opts(&self) -> Opts {
        Opts { col: self.sort.0, asc: self.sort.1, hidden: self.hidden, group: self.group && !self.searching() && self.view == View::List, folders_first: self.folders_first, icons: self.view == View::Icons }
    }

    #[must_use]
    pub fn row_path(&self, i: usize) -> PathBuf {
        self.rows.get(i).map_or_else(PathBuf::new, |r| self.dir.join(&r.name))
    }

    /// The paths of the selected rows, in order.
    #[must_use]
    pub fn sel_paths(&self) -> Vec<PathBuf> {
        self.list.sel.iter().filter(|&&i| i < self.rows.len()).map(|&i| self.row_path(i)).collect()
    }

    #[must_use]
    pub fn sel_names(&self) -> Vec<String> {
        self.list.sel.iter().filter_map(|&i| self.rows.get(i)).map(|r| r.name.clone()).collect()
    }

    /// The one selected row, if exactly one is.
    #[must_use]
    pub fn one(&self) -> Option<usize> {
        (self.list.sel.len() == 1).then(|| *self.list.sel.iter().next().unwrap_or(&0)).filter(|&i| i < self.rows.len())
    }

    /// The row the keys act on: the cursor's, or the only selected one.
    #[must_use]
    pub fn current(&self) -> Option<usize> {
        self.list.cursor.filter(|&i| i < self.rows.len()).or_else(|| self.one())
    }

    #[must_use]
    pub fn headings(&self) -> Vec<Heading> {
        self.heads.iter().map(|h| Heading { before: h.before }).collect()
    }

    /// What the tab or the window calls the folder.
    #[must_use]
    pub fn label_of(&self, dir: &Path) -> String {
        if *dir == pc_dir() {
            return t!("Этот компьютер", "This PC", ja: "PC", pt: "Este Computador", zh: "此电脑").into();
        }
        if let Some(d) = self.places.drives.iter().find(|d| d.root == dir) {
            return d.label.clone();
        }
        crate::label(&dir.to_path_buf())
    }

    #[must_use]
    pub fn icon_of_dir(&self, dir: &Path) -> IconKind {
        if *dir == pc_dir() {
            IconKind::Pc
        } else if let Some(d) = self.places.drives.iter().find(|d| d.root == dir) {
            IconKind::Drive(d.kind)
        } else if let Some(p) = self.places.quick.iter().find(|p| p.path == dir) {
            p.icon
        } else {
            IconKind::Folder(None)
        }
    }

    /// The name a row shows (a search hit has its whole path as its name: the file name is shown, the folder is the Location column).
    #[must_use]
    pub fn shown_name(&self, i: usize) -> String {
        let Some(r) = self.rows.get(i) else { return String::new() };
        if self.at_pc() {
            return self.drives.get(i).map_or_else(String::new, |d| d.label.clone());
        }
        if self.searching() { Path::new(&r.name).file_name().map_or_else(|| r.name.clone(), |n| n.to_string_lossy().into_owned()) } else { r.name.clone() }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the status line

    /// "7 folders, 3 files · 1 selected (26 KB)".
    #[must_use]
    pub fn status(&self) -> String {
        let n = self.rows.len();
        let mut s = if self.at_pc() {
            text::drives(n)
        } else if self.searching() {
            text::results(n)
        } else {
            let d = self.rows.iter().filter(|r| r.dir).count();
            match (d, n - d) {
                (0, 0) => t!("Пусто", "Empty", ja: "空", pt: "Vazio", zh: "空").into(),
                (d, 0) => text::folders(d),
                (0, f) => text::files(f),
                (d, f) => format!("{}, {}", text::folders(d), text::files(f)),
            }
        };
        let k = self.list.sel.len();
        if k > 0 && !self.at_pc() {
            let bytes: u64 = self.list.sel.iter().filter_map(|&i| self.rows.get(i)).filter(|r| !r.dir).map(|r| r.size).sum();
            s += &format!("  \u{00B7}  {}", tf!("выбрано: {}", "{} selected", ja: "{} 個選択", pt: "{} selecionado(s)", zh: "已选择 {} 项", k));
            if bytes > 0 {
                s += &format!(" ({})", crate::size_text(bytes));
            }
        }
        s
    }

    fn set_titles(&mut self) {
        self.title = if let Some(q) = &self.query {
            tf!("поиск: {}", "search: {}", ja: "検索: {}", pt: "pesquisa: {}", zh: "搜索：{}", q)
        } else if self.at_pc() {
            self.label_of(&self.dir)
        } else {
            self.dir.to_string_lossy().into_owned()
        };
    }

    /// Hands the window its new title when it changed.
    pub fn settle(&mut self, cx: &mut Cx) {
        if self.title != self.title_sent {
            self.title_sent.clone_from(&self.title);
            cx.set_title(self.title.clone());
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // rows

    /// Takes a listing: the date headings are split off into `heads`.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.rows.clear();
        self.heads.clear();
        for r in rows {
            if r.hdr {
                self.heads.push(Head { before: self.rows.len(), label: r.name.trim_start_matches('?').to_string(), count: 0 });
            } else {
                self.rows.push(r);
            }
        }
        let n = self.rows.len();
        for k in 0..self.heads.len() {
            let end = self.heads.get(k + 1).map_or(n, |h| h.before);
            self.heads[k].count = end - self.heads[k].before;
        }
        self.list.set_len(n);
        self.cols_for_mode();
    }

    /// The header's titles for what is listed: folders, drives, search hits.
    pub fn cols_for_mode(&mut self) {
        let pc = self.at_pc();
        let hits = self.searching();
        let titles = [
            t!("Имя", "Name", ja: "名前", pt: "Nome", zh: "名称"),
            if pc { t!("Свободно", "Free space", ja: "空き容量", pt: "Espaço livre", zh: "可用空间") } else { t!("Дата изменения", "Date modified", ja: "更新日時", pt: "Data de modificação", zh: "修改日期") },
            if hits { t!("Расположение", "Location", ja: "場所", pt: "Local", zh: "位置") } else { t!("Тип", "Type", ja: "種類", pt: "Tipo", zh: "类型") },
            if pc { t!("Объём", "Total size", ja: "合計サイズ", pt: "Tamanho total", zh: "总大小") } else { t!("Размер", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小") },
        ];
        for (c, title) in self.cols.cols.iter_mut().zip(titles) {
            c.title = title.into();
        }
        // the widths the user dragged stay while the listing is of the same kind
        let mode = u8::from(pc) + 2 * u8::from(hits);
        if mode != self.col_mode {
            self.col_mode = mode;
            self.cols.cols[1].w = if pc { 220.0 } else { 152.0 };
            self.cols.cols[2].w = if pc { 140.0 } else if hits { 200.0 } else { 128.0 };
        }
        self.cols.sort = Some(self.sort);
    }

    fn names_to_rows(&self, names: &[String]) -> Vec<usize> {
        names.iter().filter_map(|n| self.rows.iter().position(|r| r.name.eq_ignore_ascii_case(n))).collect()
    }

    /// Selects the rows called `names` and puts the cursor on the first.
    pub fn select_names(&mut self, names: &[String]) {
        let idx = self.names_to_rows(names);
        self.list.sel = idx.iter().copied().collect();
        self.list.anchor = idx.first().copied();
        self.list.cursor = idx.first().copied();
        self.list.shown = None; // the first of them is brought into view
    }

    /// Reads `dir` into the window. `keep` keeps the selection (the same folder read again). A folder that cannot be read leaves the
    /// window as it is.
    pub fn load(&mut self, dir: PathBuf, record: bool, keep: bool, cx: &mut Cx) {
        let names = if keep { self.sel_names() } else { Vec::new() };
        let mut rows;
        if dir == pc_dir() {
            self.places = self.be.places();
            self.drives = self.places.drives.clone();
            rows = self
                .drives
                .iter()
                .map(|d| Row { name: d.root.to_string_lossy().into_owned(), dir: true, size: d.total, time: d.free, icon: -1, hdr: false })
                .collect::<Vec<_>>();
            self.dir = dir;
            self.clock = self.be.clock();
            rows.shrink_to_fit();
        } else {
            self.clock = self.be.clock();
            let mut o = self.opts();
            o.group = self.group && self.view == View::List;
            let Some(r) = self.be.list(&dir, &o) else {
                cx.toast(tf!("Не удаётся открыть «{}»", "Can\u{2019}t open \u{201C}{}\u{201D}", ja: "「{}」を開けません", pt: "Não foi possível abrir \u{201C}{}\u{201D}", zh: "无法打开「{}」", self.label_of(&dir)), None);
                return;
            };
            rows = r;
            self.drives.clear();
            self.dir = dir;
        }
        self.query = None;
        self.search_info.clear();
        self.era = self.be.era();
        self.thumbs.clear();
        self.asked.clear();
        self.facts = None;
        self.ren = None;
        self.press = None;
        self.drop_hl = None;
        self.list = ListState::default();
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.clone_from(&self.dir);
        }
        if record {
            self.hist.visit(self.dir.clone());
        }
        self.set_rows(rows);
        if !names.is_empty() {
            self.select_names(&names);
        }
        self.addr_edit = false;
        self.addr.set_text(&self.dir_text());
        self.set_titles();
        // the rows slide in, except for the first listing (the window is not there yet) and for what a render or a test draws
        self.nav_fresh = self.hwnd != 0;
        if !self.nav_fresh {
            self.nav_at = 0;
        }
        if !self.at_pc() {
            self.be.run(self.hwnd, Do::Navigated(self.dir.clone()));
            let names: Vec<String> = self.rows.iter().filter(|r| r.dir).map(|r| r.name.clone()).collect();
            if !names.is_empty() && self.dir.parent().is_some() {
                self.be.run(self.hwnd, Do::Sizes { dir: self.dir.clone(), names });
            }
        }
        cx.invalidate();
        self.settle(cx);
    }

    /// The path as the address box shows it.
    #[must_use]
    pub fn dir_text(&self) -> String {
        if self.at_pc() { self.label_of(&self.dir) } else { self.dir.to_string_lossy().into_owned() }
    }

    pub fn reload(&mut self, cx: &mut Cx) {
        let d = self.dir.clone();
        match self.query.clone() {
            Some(q) => self.run_search(&q, cx),
            None => self.load(d, false, true, cx),
        }
    }

    /// Opens `dir` here (a click on a folder, a crumb, a place; Enter in the address box).
    pub fn go(&mut self, dir: PathBuf, cx: &mut Cx) {
        if dir == self.dir && !self.searching() {
            return;
        }
        self.load(dir, true, false, cx);
    }

    pub fn up(&mut self, cx: &mut Cx) {
        if self.at_pc() {
            return;
        }
        match self.dir.parent().map(Path::to_path_buf) {
            Some(p) if crate::zipx::split_zip(&self.dir).is_some() || !p.as_os_str().is_empty() => self.go(p, cx),
            // a drive's root: up from it is the list of drives
            _ => self.go(pc_dir(), cx),
        }
    }

    pub fn back(&mut self, cx: &mut Cx) {
        if let Some(p) = self.hist.back() {
            self.load(p, false, false, cx);
        }
    }

    pub fn forward(&mut self, cx: &mut Cx) {
        if let Some(p) = self.hist.forward() {
            self.load(p, false, false, cx);
        }
    }

    pub fn can_back(&self) -> bool {
        self.hist.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.hist.pos + 1 < self.hist.v.len()
    }

    pub fn can_up(&self) -> bool {
        !self.at_pc()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // tabs

    pub fn tab_new(&mut self, cx: &mut Cx) {
        if self.tabs.len() >= MAX_TABS {
            return;
        }
        let d = self.dir.clone();
        self.tabs.push(d);
        self.active = self.tabs.len() - 1;
        cx.invalidate();
    }

    /// A folder in a new tab, staying on this one (middle click).
    pub fn tab_open(&mut self, dir: PathBuf, cx: &mut Cx) {
        if self.tabs.len() >= MAX_TABS {
            return;
        }
        self.tabs.push(dir);
        cx.invalidate();
    }

    pub fn tab_select(&mut self, i: usize, cx: &mut Cx) {
        if i >= self.tabs.len() || i == self.active {
            return;
        }
        self.active = i;
        let d = self.tabs[i].clone();
        self.load(d, true, false, cx);
    }

    /// Closes tab `i` (the last one stays). The listing changes only when the tab was the active one.
    pub fn tab_close(&mut self, i: usize, cx: &mut Cx) {
        if self.tabs.len() < 2 || i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        let before = self.active;
        self.active = if i < before { before - 1 } else { before.min(self.tabs.len() - 1) };
        if i == before {
            let d = self.tabs[self.active].clone();
            self.load(d, true, false, cx);
        } else {
            cx.invalidate();
        }
    }

    pub fn tab_next(&mut self, back: bool, cx: &mut Cx) {
        let n = self.tabs.len();
        if n > 1 {
            let i = if back { (self.active + n - 1) % n } else { (self.active + 1) % n };
            self.tab_select(i, cx);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // search

    pub fn run_search(&mut self, q: &str, cx: &mut Cx) {
        let q = q.trim();
        if q.is_empty() {
            return;
        }
        if self.at_pc() {
            cx.toast(t!("Сначала откройте диск", "Open a drive first", ja: "先にドライブを開いてください", pt: "Abra uma unidade primeiro", zh: "请先打开一个驱动器"), None);
            return;
        }
        let found = self.be.search(&self.dir, q, &self.opts());
        self.clock = self.be.clock();
        self.query = Some(q.to_string());
        let n = found.rows.len();
        let place = if found.indexed { String::new() } else { tf!(" в «{}» и вложенных папках", " in {} and its folders", ja: "（{} とその下）", pt: " em {} e subpastas", zh: "（{} 及其子文件夹）", self.label_of(&self.dir)) };
        self.search_info = format!("{}{}{}", tf!("{} для «{}»", "{} for \u{201C}{}\u{201D}", ja: "「{1}」の検索結果: {0}", pt: "{} para \u{201C}{}\u{201D}", zh: "「{1}」的搜索结果：{0}", text::results(n), q), place, found.more);
        self.era = self.be.era();
        self.thumbs.clear();
        self.asked.clear();
        self.facts = None;
        self.ren = None;
        let mut rows = found.rows;
        // the hits are sorted by the window's column like a folder's rows (without date headings)
        let o = Opts { group: false, ..self.opts() };
        self.be.sort(&mut rows, &o);
        self.list = ListState::default();
        self.set_rows(rows);
        self.set_titles();
        self.nav_fresh = self.hwnd != 0;
        cx.invalidate();
        self.settle(cx);
    }

    pub fn clear_search(&mut self, cx: &mut Cx) {
        self.srch.set_text("");
        if self.searching() {
            self.query = None;
            let d = self.dir.clone();
            self.load(d, false, false, cx);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // sorting and the view

    /// Sorts by column `c`: the same column turns the order over.
    pub fn sort_by(&mut self, c: usize, cx: &mut Cx) {
        if c >= 4 || (self.searching() && c == 2) {
            return;
        }
        // the first click is ascending, the next one turns it over (as the old header did)
        let asc = if self.sort.0 == c { !self.sort.1 } else { true };
        self.sort = (c, asc);
        self.resort(cx);
    }

    pub fn set_sort(&mut self, col: usize, asc: bool, cx: &mut Cx) {
        self.sort = (col, asc);
        self.resort(cx);
    }

    /// Orders the rows again (no disk read).
    pub fn resort(&mut self, cx: &mut Cx) {
        let names = self.sel_names();
        let cursor = self.current().and_then(|i| self.rows.get(i)).map(|r| r.name.clone());
        let mut rows = std::mem::take(&mut self.rows);
        if self.at_pc() {
            let drives = std::mem::take(&mut self.drives);
            let mut pairs: Vec<_> = rows.drain(..).zip(drives).collect();
            let (col, asc) = self.sort;
            pairs.sort_by(|a, b| {
                let o = match col {
                    1 => a.1.free.cmp(&b.1.free),
                    3 => a.1.total.cmp(&b.1.total),
                    2 => a.1.label.cmp(&b.1.label),
                    _ => a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()),
                };
                if asc { o } else { o.reverse() }
            });
            let (r, d): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
            rows = r;
            self.drives = d;
            self.heads.clear();
            self.set_rows(rows);
        } else {
            let o = Opts { group: self.group && !self.searching() && self.view == View::List, ..self.opts() };
            self.be.sort(&mut rows, &o);
            self.set_rows(rows);
        }
        self.select_names(&names);
        if let Some(c) = cursor.and_then(|n| self.rows.iter().position(|r| r.name == n)) {
            self.list.cursor = Some(c);
        }
        self.cols.sort = Some(self.sort);
        cx.invalidate();
    }

    pub fn set_view(&mut self, v: View, cx: &mut Cx) {
        if self.view == v {
            return;
        }
        self.view = v;
        self.thumbs.clear();
        self.asked.clear();
        // the icons view has no date headings: the rows are listed again
        self.reload(cx);
    }

    pub fn toggle_view(&mut self, cx: &mut Cx) {
        let v = if self.view == View::List { View::Icons } else { View::List };
        self.set_view(v, cx);
    }

    pub fn set_group(&mut self, on: bool, cx: &mut Cx) {
        self.group = on;
        self.be.set_flag(Flag::Group, on);
        self.reload(cx);
    }

    pub fn set_folders_first(&mut self, on: bool, cx: &mut Cx) {
        self.folders_first = on;
        self.be.set_flag(Flag::FoldersFirst, on);
        self.reload(cx);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // pictures

    /// Asks for the picture of row `i` once: its thumbnail, or its icon for a program.
    pub fn want_picture(&mut self, i: usize, size: i32) {
        let Some(r) = self.rows.get(i) else { return };
        if r.dir || self.at_pc() {
            return;
        }
        let p = self.row_path(i);
        if self.thumbs.contains_key(&p) || self.asked.contains(&p) {
            return;
        }
        let ext = look::ext_of(&r.name);
        let own = matches!(ext, "exe" | "lnk" | "ico" | "url" | "scr");
        let pic = crate::is_pic(&p);
        if !(own || pic || self.view == View::Icons) {
            return;
        }
        self.asked.insert(p.clone());
        self.be.run(self.hwnd, Do::Thumb { path: p, icon: own, size });
    }

    /// A worker's picture arrived.
    pub fn got_picture(&mut self, era: u32, path: PathBuf, icon: bool, (w, h): (i32, i32), px: Vec<u32>) {
        if era != self.era || self.thumbs.len() > 600 {
            return;
        }
        self.thumbs.insert(path, Thumb { icon, w, h, px, pic: None });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the preview

    /// The preview's facts of the one selected row (asked for the first time it is drawn).
    pub fn facts_of(&mut self, i: usize) -> Facts {
        let p = self.row_path(i);
        if let Some((fp, f)) = &self.facts
            && *fp == p
        {
            return f.clone();
        }
        let f = if self.at_pc() { Facts::default() } else { self.be.facts(&p) };
        self.facts = Some((p, f.clone()));
        f
    }

    /// The drive of a This PC row.
    #[must_use]
    pub fn drive(&self, i: usize) -> Option<&super::backend::DriveInfo> {
        if self.at_pc() { self.drives.get(i) } else { None }
    }

    /// A place of the sidebar (its path).
    #[must_use]
    pub fn place(&self, i: usize) -> Option<&Place> {
        self.places.quick.get(i)
    }
}
