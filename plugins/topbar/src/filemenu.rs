//! The model of the menu for files, apps and folders (`ui::file_menu` draws it, `menus::file_menu` builds it for a request from
//! rust-explorer; the protocol is `winrust_common::filemenu`). Here the shell's items become the design's rows:
//!
//! * the header says what the target is (a name and a second line, "Text document · 18 KB");
//! * a row of tiles holds what is done to the target itself: Cut, Copy, Rename, Share, Delete;
//! * the main rows are the design's own for the kind of target (a file, an image, an app, a folder, several items...), each
//!   taking its action from the shell's item of the same verb when there is one, so nothing is lost and nothing is invented that
//!   the machine cannot do: a row whose verb the shell does not offer is left out;
//! * "Show more options" holds the whole list the shell gave, in the same style, with the programs' own 16 px bitmaps.
//!
//! Windows names a submenu (Open with, Send to) only by its text, never by a verb, so those two are recognised by the words
//! Windows uses in the five languages the bar speaks.
use std::rc::Rc;

use winrust_common::filemenu::{Act, Icon, ShellItem, Sub, fl, rq};

use crate::ui::taskview::OwnedIcon;

// the design's icons (24 px grid, `I` in the mockup)
pub const CUT: &str = "M6 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM20 4L8.1 15.9M14.5 14.5L20 20M8.1 8.1L12 12";
pub const COPY: &str = "M9 9h11v11H9zM5 15H4V4h11v1";
pub const RENAME: &str = "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4";
pub const SHARE: &str = "M4 12v7h16v-7M12 3v12M7.5 7.5L12 3l4.5 4.5";
pub const DEL: &str = "M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13M10 11v5M14 11v5";
pub const OPEN: &str = "M14 4h6v6M20 4l-9 9M18 14v6H4V6h6";
pub const RUN: &str = "M7 4.5v15l12-7.5z";
pub const EDIT: &str = "M4 20h4L19 9l-4-4L4 16z";
pub const OPEN_WITH: &str = "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM16.5 13v7M13 16.5h7";
pub const PATH: &str = "M8 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2M10 14L20 4M14 4h6v6";
pub const ZIP: &str = "M5 3h14v18H5zM12 3v2M12 7v2M12 11v2M10.5 15h3v3h-3z";
pub const SEND: &str = "M4 12l16-8-6 16-3-7z";
pub const MORE: &str = "M4 6h16M4 12h16M4 18h10";
pub const PROPS: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v6M12 7.5h.01";
pub const WALL: &str = "M3 5h18v14H3zM3 15l5-5 5 5 3-3 5 5";
pub const ROT_R: &str = "M20 11a8 8 0 1 0-2.4 5.7M20 4v7h-7";
pub const ROT_L: &str = "M4 11a8 8 0 1 1 2.4 5.7M4 4v7h7";
pub const EXTRACT: &str = "M12 4v11M7.5 10.5L12 15l4.5-4.5M4 20h16";
pub const SHIELD: &str = "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z";
pub const FOLDER_OPEN: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6z";
pub const PIN: &str = "M9 4h6l-1 6 4 3v2h-5v5l-1 1-1-1v-5H6v-2l4-3z";
pub const UNPIN: &str = "M9 4h6l-1 6 4 3v2h-5v5l-1 1-1-1v-5H6v-2l4-3zM3 3l18 18";
pub const TAB: &str = "M3 7h7l2-3h9v16H3z";
pub const TERM: &str = "M4 5h16v14H4zM7.5 10l3 2.5-3 2.5M13 15h4";
pub const STAR: &str = "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z";
pub const BIN: &str = "M4 7h16M6.5 7l1 13h9l1-13M9 7V4.5h6V7";
pub const BACK: &str = "M15 6l-6 6 6 6";
pub const CHOOSE: &str = "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4";
pub const FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4";
pub const DOC: &str = "M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h5";
pub const IMG: &str = "M4 5h16v14H4zM4 15l4.5-4.5L13 15l3-3 4 4M15.5 9h.01";
pub const DRIVE: &str = "M4 7h16v10H4zM7.5 12h.01M11 12h6";
pub const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";

/// The colour of a drawn icon: the design's `--t2`, the blue of the shield, the red of what destroys something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Dim,
    Blue,
    Red,
}

#[derive(Clone)]
pub enum Pic {
    /// An empty slot: the label keeps its place with the rows around it.
    None,
    Line(&'static str, Tint),
    /// What a program put on its own item.
    Bitmap(Rc<Icon>),
}

/// What a row does when it is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Do {
    /// Opens a submenu, or does nothing (a caption).
    Nothing,
    /// Item `n` of the shell's menu.
    Shell(u32),
    /// Something rust-explorer does itself.
    Ours(Act),
    /// The bar's own: Start's pins and the bar's.
    PinStart,
    PinBar,
    /// Opens the whole list; and comes back.
    More,
    Less,
}

#[derive(Clone)]
pub enum Under {
    Items(Vec<Row>),
    /// Windows fills it when it is opened: ask for this handle.
    Lazy(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sep,
    Caption,
    Item,
}

#[derive(Clone)]
pub struct Row {
    pub kind: Kind,
    pub label: String,
    pub pic: Pic,
    pub keys: Vec<String>,
    pub badge: String,
    pub bold: bool,
    pub danger: bool,
    pub off: bool,
    pub checked: bool,
    pub sub: Option<Under>,
    pub act: Do,
}

impl Row {
    #[must_use]
    pub fn item(label: impl Into<String>, pic: Pic, act: Do) -> Row {
        Row { kind: Kind::Item, label: label.into(), pic, keys: Vec::new(), badge: String::new(), bold: false, danger: false, off: false, checked: false, sub: None, act }
    }

    #[must_use]
    pub fn line(label: impl Into<String>, d: &'static str, act: Do) -> Row {
        Row::item(label, Pic::Line(d, Tint::Dim), act)
    }

    #[must_use]
    pub fn sep() -> Row {
        Row { kind: Kind::Sep, ..Row::item("", Pic::None, Do::Nothing) }
    }

    #[must_use]
    pub fn caption(label: impl Into<String>) -> Row {
        Row { kind: Kind::Caption, ..Row::item(label, Pic::None, Do::Nothing) }
    }

    #[must_use]
    pub fn keys(mut self, k: &[&str]) -> Row {
        self.keys = k.iter().map(|s| (*s).into()).collect();
        self
    }

    #[must_use]
    pub fn bold(mut self) -> Row {
        self.bold = true;
        self
    }

    #[must_use]
    pub fn badge(mut self, b: impl Into<String>) -> Row {
        self.badge = b.into();
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Row {
        self.danger = true;
        self.pic = match self.pic {
            Pic::Line(d, _) => Pic::Line(d, Tint::Red),
            p => p,
        };
        self
    }

    #[must_use]
    pub fn tinted(mut self, t: Tint) -> Row {
        if let Pic::Line(d, _) = self.pic {
            self.pic = Pic::Line(d, t);
        }
        self
    }

    #[must_use]
    pub fn off(mut self) -> Row {
        self.off = true;
        self
    }

    #[must_use]
    pub fn under(mut self, u: Under) -> Row {
        self.sub = Some(u);
        self
    }

    #[must_use]
    pub fn is_item(&self) -> bool {
        self.kind == Kind::Item
    }
}

/// A tile of the row under the header: what is done to the target itself.
#[derive(Clone)]
pub struct Quick {
    pub d: &'static str,
    /// The hint shown in the header while the pointer is on it: "Cut · Ctrl+X".
    pub tip: String,
    pub danger: bool,
    pub act: Do,
}

pub enum HeadIcon {
    /// The shell's own icon of the target.
    Shell(Rc<OwnedIcon>),
    /// The picture of an image, drawn covering the box with rounded corners.
    Thumb(Rc<OwnedIcon>),
    /// A drawing on a plate: `from` to `to` diagonally, the drawing in `ink`.
    Plate(&'static str, u32, u32, u32),
}

pub struct Head {
    pub name: String,
    pub meta: String,
    pub icon: HeadIcon,
}

pub struct Menu {
    pub head: Head,
    pub quick: Vec<Quick>,
    /// The main rows, and the whole list behind "Show more options" (starts with the way back and its caption).
    pub rows: Vec<Row>,
    pub full: Vec<Row>,
}

/// What kind of target the menu is for: the design's sets of rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    File,
    Text,
    Image,
    Archive,
    /// A program or a shortcut to one.
    App,
    Folder,
    Several,
    Drive,
    Bin,
}

/// The facts about the target the rows depend on (the host reads them from the disk, a render makes them up).
pub struct Facts {
    pub target: Target,
    /// The name or, for several items, how many ("3 items").
    pub name: String,
    pub meta: String,
    /// The first target's file name without its extension.
    pub stem: String,
    pub start_pinned: bool,
    pub bar_pinned: bool,
    /// Recycle Bin: how many items it holds.
    pub bin_items: u32,
    /// An archive this machine's file manager can unpack (zip, 7z): "Extract to".
    pub extract: bool,
    /// The request's flags ([`rq`]).
    pub flags: u32,
}

/// The kind of a path by what it is called (a folder is told by the caller).
#[must_use]
pub fn classify(path: &str, dir: bool) -> Target {
    if path.trim_end_matches('\\').to_ascii_lowercase().ends_with("::{645ff040-5081-101b-9f08-00aa002f954e}") {
        return Target::Bin;
    }
    if dir {
        return if path.trim_end_matches('\\').len() <= 2 { Target::Drive } else { Target::Folder };
    }
    let ext = path.rsplit_once('.').map_or("", |e| e.1).to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "log" | "toml" | "json" | "rs" | "ini" | "cfg" | "yml" | "yaml" | "xml" | "csv" | "ps1" | "bat" | "cmd" | "py" | "js" | "ts" | "html" | "css" | "c" | "h" | "cpp" | "go" | "java" | "sql" => Target::Text,
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "heic" | "tif" | "tiff" | "jfif" | "avif" => Target::Image,
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "cab" | "iso" => Target::Archive,
        "exe" | "lnk" | "msi" | "appref-ms" | "url" => Target::App,
        _ => Target::File,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the shell's items

fn has(label: &str, words: &[&str]) -> bool {
    let l = label.trim().trim_end_matches(['…', '.', ' ']).to_lowercase();
    words.iter().any(|w| l == *w)
}

/// "Open with" and "Send to" as Windows words them in the bar's languages (the shell gives a submenu no verb).
fn is_open_with(i: &ShellItem) -> bool {
    i.has_sub() && has(&i.label, &["open with", "открыть с помощью", "プログラムから開く", "abrir com", "打开方式"])
}

fn is_send_to(i: &ShellItem) -> bool {
    i.has_sub() && (i.verb.eq_ignore_ascii_case("sendto") || has(&i.label, &["send to", "отправить", "送る", "enviar para", "发送到"]))
}

fn by_verb<'a>(items: &'a [ShellItem], verb: &str) -> Option<&'a ShellItem> {
    items.iter().find(|i| !i.is_sep() && i.id != 0 && i.verb.eq_ignore_ascii_case(verb))
}

/// The item Enter runs: the one Windows made the default (its text may be "Preview" or "Play"), else the `open` verb.
fn default_item(items: &[ShellItem]) -> Option<&ShellItem> {
    items.iter().find(|i| i.is_default() && i.id != 0).or_else(|| by_verb(items, "open"))
}

fn sub_of(i: &ShellItem) -> Under {
    match &i.sub {
        Sub::Items(v) => Under::Items(sub_rows(v)),
        Sub::Lazy(h) => Under::Lazy(*h),
        Sub::None => Under::Items(Vec::new()),
    }
}

/// `Ctrl+Shift+C` -> `["Ctrl", "Shift", "C"]`.
#[must_use]
pub fn keys_of(hint: &str) -> Vec<String> {
    hint.split('+').map(str::trim).filter(|k| !k.is_empty()).map(str::to_string).collect()
}

/// The icon of a row of the full list: the design's drawing for the few commands it draws one for, else what the program gave.
fn pic_of(i: &ShellItem) -> Pic {
    let verb = i.verb.to_ascii_lowercase();
    let known = match verb.as_str() {
        "open" => Some((OPEN, Tint::Dim)),
        "runas" => Some((SHIELD, Tint::Blue)),
        "windows.modernshare" => Some((SHARE, Tint::Dim)),
        "copyaspath" => Some((PATH, Tint::Dim)),
        "properties" => Some((PROPS, Tint::Dim)),
        "openas" => Some((CHOOSE, Tint::Dim)),
        _ => None,
    };
    if let Some((d, t)) = known {
        return Pic::Line(d, t);
    }
    if is_open_with(i) {
        return Pic::Line(OPEN_WITH, Tint::Dim);
    }
    if is_send_to(i) {
        return Pic::Line(SEND, Tint::Dim);
    }
    i.icon.clone().map_or(Pic::None, |b| Pic::Bitmap(Rc::new(b)))
}

/// The rows of a submenu the shell filled: no further level (the design stops at two), separators kept in between.
#[must_use]
pub fn sub_rows(items: &[ShellItem]) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for i in items {
        if i.is_sep() {
            if out.last().is_some_and(Row::is_item) {
                out.push(Row::sep());
            }
            continue;
        }
        // an item with nothing behind it and no way to run it, or one with a submenu of its own (a third level): not shown
        if i.id == 0 || i.has_sub() {
            continue;
        }
        let mut r = Row::item(i.label.clone(), pic_of(i), Do::Shell(i.id));
        r.off = i.disabled();
        r.checked = i.flags & (fl::CHECKED | fl::RADIO) != 0 && i.flags & fl::CHECKED != 0;
        r.bold = i.is_default();
        r.keys = keys_of(&i.hint);
        out.push(r);
    }
    while out.last().is_some_and(|r| !r.is_item()) {
        out.pop();
    }
    out
}

/// The rows of the whole list: the way back, its caption, and everything the shell gave (plus our Rename after Delete).
fn full_rows(items: &[ShellItem], rename: bool) -> Vec<Row> {
    let mut out = vec![Row::line(t!("Меньше параметров", "Fewer options", ja: "オプションを減らす", pt: "Menos opções", zh: "更少选项"), BACK, Do::Less), Row::caption(t!("Все параметры Windows и других программ", "All options from Windows and other apps", ja: "Windows とその他のアプリのすべてのオプション", pt: "Todas as opções do Windows e de outros aplicativos", zh: "来自 Windows 和其他应用的所有选项"))];
    for i in items {
        if i.is_sep() {
            if out.last().is_some_and(Row::is_item) {
                out.push(Row::sep());
            }
            continue;
        }
        if i.id == 0 && !i.has_sub() {
            continue;
        }
        let mut r = Row::item(i.label.clone(), pic_of(i), if i.has_sub() { Do::Nothing } else { Do::Shell(i.id) });
        r.off = i.disabled();
        r.checked = i.flags & fl::CHECKED != 0;
        r.bold = i.is_default();
        r.keys = match i.verb.to_ascii_lowercase().as_str() {
            "copyaspath" => vec!["Ctrl".into(), "Shift".into(), "C".into()],
            "properties" => vec!["Alt".into(), "Enter".into()],
            _ => keys_of(&i.hint),
        };
        if i.has_sub() {
            r.sub = Some(sub_of(i));
        }
        let is_delete = i.verb.eq_ignore_ascii_case("delete");
        out.push(r);
        if is_delete && rename {
            out.push(Row::line(t!("Переименовать", "Rename", ja: "名前の変更", pt: "Renomear", zh: "重命名"), RENAME, Do::Ours(Act::Rename)).keys(&["F2"]));
        }
    }
    while out.last().is_some_and(|r| !r.is_item()) {
        out.pop();
    }
    out
}

pub fn size_text(bytes: u64) -> String {
    let dot = |s: String| if dot!() { s } else { s.replace('.', ",") };
    let unit = |ru: &'static str, en: &'static str| t!(ru, en);
    match bytes {
        0..=1_048_575 => format!("{} {}", bytes.div_ceil(1024).max(1), unit("КБ", "KB")),
        1_048_576..=1_073_741_823 => format!("{} {}", dot(format!("{:.1}", bytes as f64 / 1_048_576.0)), unit("МБ", "MB")),
        _ => format!("{} {}", dot(format!("{:.2}", bytes as f64 / 1_073_741_824.0)), unit("ГБ", "GB")),
    }
}

/// "Text document · 18 KB": the type Windows names it, and its size.
#[must_use]
pub fn meta_of(type_name: &str, bytes: Option<u64>) -> String {
    match bytes {
        Some(b) if !type_name.is_empty() => format!("{type_name} · {}", size_text(b)),
        Some(b) => size_text(b),
        None => type_name.to_string(),
    }
}

/// "3 items".
#[must_use]
pub fn items_text(n: usize) -> String {
    tf!("Элементов: {}", "{} items", ja: "{} 個の項目", pt: "{} itens", zh: "{} 项", n)
}

// ---------------------------------------------------------------------------------------------------------------------
// the menu

fn label(ru: &'static str, en: &'static str, ja: &'static str, pt: &'static str, zh: &'static str) -> &'static str {
    t!(ru, en, ja: ja, pt: pt, zh: zh)
}

/// Builds the menu for a target of `f`'s kind out of the shell's `items`.
#[must_use]
pub fn build(f: &Facts, items: &[ShellItem], head: HeadIcon) -> Menu {
    let several = f.target == Target::Several;
    let one = !several;
    let rename_ok = f.flags & rq::CAN_RENAME != 0 && one;

    // the tiles
    let tile = |d: &'static str, name: &str, keys: &str, danger: bool, act: Do| Quick { d, tip: if keys.is_empty() { name.to_string() } else { format!("{name} · {keys}") }, danger, act };
    let mut quick = Vec::new();
    if !matches!(f.target, Target::Drive | Target::Bin) {
        let shell = |verb: &str| by_verb(items, verb).map(|i| Do::Shell(i.id));
        if let Some(a) = shell("cut") {
            quick.push(tile(CUT, label("Вырезать", "Cut", "切り取り", "Recortar", "剪切"), "Ctrl+X", false, a));
        }
        if let Some(a) = shell("copy") {
            quick.push(tile(COPY, label("Копировать", "Copy", "コピー", "Copiar", "复制"), "Ctrl+C", false, a));
        }
        if rename_ok {
            quick.push(tile(RENAME, label("Переименовать", "Rename", "名前の変更", "Renomear", "重命名"), "F2", false, Do::Ours(Act::Rename)));
        }
        if f.target != Target::Folder && let Some(a) = shell("Windows.ModernShare") {
            quick.push(tile(SHARE, label("Поделиться", "Share", "共有", "Compartilhar", "共享"), "", false, a));
        }
        if let Some(a) = shell("delete") {
            quick.push(tile(DEL, label("Удалить", "Delete", "削除", "Excluir", "删除"), "Del", true, a));
        }
    }

    // the rows by kind
    let open_label = if f.target == Target::App { label("Запустить", "Run", "実行", "Executar", "运行") } else { label("Открыть", "Open", "開く", "Abrir", "打开") };
    let open = default_item(items).map(|i| Row::line(open_label, if f.target == Target::App { RUN } else { OPEN }, Do::Shell(i.id)).bold().keys(&["Enter"]));
    let open_with = items.iter().find(|i| is_open_with(i)).map(|i| Row::line(label("Открыть с помощью", "Open with", "プログラムから開く", "Abrir com", "打开方式"), OPEN_WITH, Do::Nothing).under(sub_of(i)));
    let send_to = items.iter().find(|i| is_send_to(i)).map(|i| Row::line(label("Отправить", "Send to", "送る", "Enviar para", "发送到"), SEND, Do::Nothing).under(sub_of(i)));
    let copy_path = Row::line(if several { label("Копировать пути", "Copy as paths", "パスのコピー", "Copiar como caminhos", "复制文件地址") } else { label("Копировать как путь", "Copy as path", "パスのコピー", "Copiar como caminho", "复制文件地址") }, PATH, Do::Ours(Act::CopyPath)).keys(&["Ctrl", "Shift", "C"]);
    let zip = Row::line(label("Сжать в ZIP", "Compress to ZIP", "ZIP に圧縮", "Compactar em ZIP", "压缩为 ZIP"), ZIP, Do::Ours(Act::Zip));
    let verb_row = |verb: &str, text: &'static str, d: &'static str| by_verb(items, verb).map(|i| Row::line(text, d, Do::Shell(i.id)));
    let props = by_verb(items, "properties").map(|i| Row::line(label("Свойства", "Properties", "プロパティ", "Propriedades", "属性"), PROPS, Do::Shell(i.id)).keys(&["Alt", "Enter"]));

    let mut rows: Vec<Row> = Vec::new();
    match f.target {
        Target::Text => {
            rows.extend(open);
            rows.extend(verb_row("edit", label("Изменить", "Edit", "編集", "Editar", "编辑"), EDIT));
            rows.extend(open_with);
            rows.push(Row::sep());
            rows.push(copy_path);
            rows.push(zip);
            rows.extend(send_to);
        }
        Target::File => {
            rows.extend(open);
            rows.extend(open_with);
            rows.push(Row::sep());
            rows.push(copy_path);
            rows.push(zip);
            rows.extend(send_to);
        }
        Target::Image => {
            rows.extend(open);
            rows.extend(verb_row("setdesktopwallpaper", label("Сделать фоновым рисунком", "Set as desktop background", "デスクトップの背景として設定", "Definir como plano de fundo", "设置为桌面背景"), WALL));
            rows.extend(verb_row("rotate90", label("Повернуть вправо", "Rotate right", "右へ回転", "Girar para a direita", "向右旋转"), ROT_R));
            rows.extend(verb_row("rotate270", label("Повернуть влево", "Rotate left", "左へ回転", "Girar para a esquerda", "向左旋转"), ROT_L));
            rows.extend(open_with);
            rows.push(Row::sep());
            rows.push(copy_path);
            rows.push(zip);
            rows.extend(send_to);
        }
        Target::Archive => {
            rows.extend(open);
            if f.extract {
                let name = f.stem.clone();
                rows.push(Row::line(tf!("Извлечь в “{}\\”", "Extract to “{}\\”", ja: "“{}\\” に展開", pt: "Extrair para “{}\\”", zh: "解压到“{}\\”", name), EXTRACT, Do::Ours(Act::Extract)));
            }
            rows.extend(open_with);
            rows.push(Row::sep());
            rows.push(copy_path);
            rows.extend(send_to);
        }
        Target::App => {
            rows.extend(open);
            rows.extend(verb_row("runas", label("Запуск от имени администратора", "Run as administrator", "管理者として実行", "Executar como administrador", "以管理员身份运行"), SHIELD).map(|r| r.tinted(Tint::Blue)));
            let loc = verb_row("opencontaining", label("Расположение файла", "Open file location", "ファイルの場所を開く", "Abrir local do arquivo", "打开文件所在的位置"), FOLDER_OPEN)
                .unwrap_or_else(|| Row::line(label("Расположение файла", "Open file location", "ファイルの場所を開く", "Abrir local do arquivo", "打开文件所在的位置"), FOLDER_OPEN, Do::Ours(Act::Reveal)));
            rows.push(loc);
            rows.push(Row::sep());
            rows.push(Row::line(
                if f.start_pinned { label("Открепить от Пуска", "Unpin from Start", "スタートからピン留めを外す", "Desafixar do Iniciar", "从开始菜单取消固定") } else { label("Закрепить в Пуске", "Pin to Start", "スタートにピン留め", "Fixar no Iniciar", "固定到开始菜单") },
                if f.start_pinned { UNPIN } else { PIN },
                Do::PinStart,
            ));
            rows.push(Row::line(
                if f.bar_pinned { label("Открепить от панели", "Unpin from the bar", "バーからピン留めを外す", "Desafixar da barra", "从任务栏取消固定") } else { label("Закрепить на панели", "Pin to the bar", "バーにピン留め", "Fixar na barra", "固定到任务栏") },
                if f.bar_pinned { UNPIN } else { PIN },
                Do::PinBar,
            ));
            rows.push(Row::sep());
            rows.push(copy_path);
        }
        Target::Folder => {
            rows.extend(open);
            if f.flags & rq::TAB != 0 {
                rows.push(Row::line(label("Открыть в новой вкладке", "Open in new tab", "新しいタブで開く", "Abrir em nova guia", "在新标签页中打开"), TAB, Do::Ours(Act::NewTab)).keys(&["Ctrl", "Enter"]));
            }
            rows.push(Row::line(label("Открыть в Терминале", "Open in Terminal", "ターミナルで開く", "Abrir no Terminal", "在终端中打开"), TERM, Do::Ours(Act::Terminal)));
            rows.push(Row::line(
                if f.flags & rq::PINNED != 0 { label("Открепить от быстрого доступа", "Unpin from Quick access", "クイック アクセスからピン留めを解除", "Desafixar do Acesso rápido", "从快速访问取消固定") } else { label("Закрепить в быстром доступе", "Pin to Quick access", "クイック アクセスにピン留め", "Fixar no Acesso rápido", "固定到快速访问") },
                STAR,
                Do::Ours(Act::TogglePin),
            ));
            rows.push(Row::sep());
            rows.push(zip);
            rows.push(copy_path);
        }
        Target::Several => {
            rows.push(zip);
            rows.push(copy_path);
            rows.extend(send_to);
        }
        Target::Drive => {
            rows.extend(open);
            if f.flags & rq::TAB != 0 {
                rows.push(Row::line(label("Открыть в новой вкладке", "Open in new tab", "新しいタブで開く", "Abrir em nova guia", "在新标签页中打开"), TAB, Do::Ours(Act::NewTab)));
            }
        }
        Target::Bin => {
            rows.extend(open);
            let empty = by_verb(items, "empty").map(|i| i.id);
            let (text, act) = if f.bin_items > 0 { (label("Очистить корзину", "Empty Recycle Bin", "ごみ箱を空にする", "Esvaziar Lixeira", "清空回收站"), empty.map_or(Do::Nothing, Do::Shell)) } else { (label("Корзина пуста", "Recycle Bin is empty", "ごみ箱は空です", "A Lixeira está vazia", "回收站为空"), Do::Nothing) };
            let mut r = Row::line(text, DEL, act);
            if f.bin_items > 0 && empty.is_some() {
                r = r.danger().badge(tf!("{} эл.", "{} items", ja: "{} 個", pt: "{} itens", zh: "{} 项", f.bin_items));
            } else {
                r = r.off();
            }
            rows.push(r);
        }
    }
    // the tail: the whole list and Properties (a drive and the Recycle Bin have only Properties)
    rows.push(Row::sep());
    if !matches!(f.target, Target::Drive | Target::Bin) {
        rows.push(Row::line(label("Показать дополнительные параметры", "Show more options", "その他のオプションを確認", "Mostrar mais opções", "显示更多选项"), MORE, Do::More).keys(&["Shift", "F10"]));
    }
    rows.extend(props);
    // no separator at either end or twice in a row (a verb the shell did not offer leaves its neighbours)
    let mut tidy: Vec<Row> = Vec::new();
    for r in rows {
        if r.kind == Kind::Sep && !tidy.last().is_some_and(Row::is_item) {
            continue;
        }
        tidy.push(r);
    }
    while tidy.last().is_some_and(|r| r.kind == Kind::Sep) {
        tidy.pop();
    }
    let full = full_rows(items, rename_ok);
    Menu { head: Head { name: f.name.clone(), meta: f.meta.clone(), icon: head }, quick, rows: tidy, full }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(id: u32, verb: &str, label: &str) -> ShellItem {
        ShellItem::new(id, verb, label)
    }

    fn bitmap(c: u32) -> Icon {
        Icon { w: 16, h: 16, px: vec![c; 256] }
    }

    fn sub(label: &str, h: u32) -> ShellItem {
        ShellItem { sub: Sub::Lazy(h), ..it(0, "", label) }
    }

    /// What this machine's shell gave for a text file (see `shellmenu::tests`), with a long third-party label and a bitmap.
    fn text_items() -> Vec<ShellItem> {
        let mut open = it(136, "open", "Open");
        open.flags = fl::DEFAULT;
        let mut cursor = it(139, "Cursor", "Open with Cursor");
        cursor.icon = Some(bitmap(0xff0f_0f10));
        let mut long = it(142, "Send with DashBeam", "Send with DashBeam to a nearby device that is on the same network and nowhere else at all");
        long.icon = Some(bitmap(0xff0d_2b1d));
        vec![
            open,
            it(138, "edit", "Edit"),
            cursor,
            it(140, "pintohomefile", "Add to Favorites"),
            long,
            sub("7-Zip", 1),
            sub("Open with", 2),
            ShellItem::sep(),
            sub("Give access to", 3),
            it(79, "copyaspath", "Copy as path"),
            it(78, "Windows.ModernShare", "Share"),
            it(28, "PreviousVersions", "Restore previous versions"),
            ShellItem::sep(),
            sub("Send to", 4),
            ShellItem::sep(),
            it(25, "cut", "Cut"),
            it(26, "copy", "Copy"),
            ShellItem::sep(),
            it(17, "link", "Create shortcut"),
            it(18, "delete", "Delete"),
            ShellItem::sep(),
            it(20, "properties", "Properties"),
        ]
    }

    fn facts(target: Target) -> Facts {
        Facts { target, name: "New text document.txt".into(), meta: "Text document · 2 KB".into(), stem: "New text document".into(), start_pinned: false, bar_pinned: false, bin_items: 0, extract: false, flags: rq::DESKTOP | rq::CAN_RENAME }
    }

    fn head() -> HeadIcon {
        HeadIcon::Plate(DOC, 0xeaf2ff, 0xeaf2ff, 0x2557c4)
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| if r.is_item() { r.label.clone() } else { "-".into() }).collect()
    }

    #[test]
    fn a_text_file_gets_the_designs_rows_wired_to_the_shells_own_items() {
        let m = build(&facts(Target::Text), &text_items(), head());
        // under `cargo test` the texts are Russian (lang!); the shape is what counts
        assert_eq!(m.rows.len(), 10, "{:?}", labels(&m.rows));
        let acts: Vec<Do> = m.rows.iter().filter(|r| r.is_item()).map(|r| r.act.clone()).collect();
        assert_eq!(acts[0], Do::Shell(136), "Open is the shell's default item");
        assert!(m.rows[0].bold && m.rows[0].keys == ["Enter"]);
        assert_eq!(acts[1], Do::Shell(138), "Edit by its verb");
        assert!(matches!(m.rows[2].sub, Some(Under::Lazy(2))), "Open with opens the shell's submenu by its handle");
        assert_eq!(m.rows[3].kind, Kind::Sep);
        assert_eq!(acts[3], Do::Ours(Act::CopyPath));
        assert_eq!(acts[4], Do::Ours(Act::Zip));
        assert!(matches!(m.rows[6].sub, Some(Under::Lazy(4))), "Send to");
        assert_eq!(m.rows[7].kind, Kind::Sep);
        assert_eq!(m.rows[8].act, Do::More);
        assert_eq!(m.rows[8].keys, ["Shift", "F10"]);
        assert_eq!(m.rows[9].act, Do::Shell(20));
        assert_eq!(m.rows[9].keys, ["Alt", "Enter"]);
        // the tiles: Cut, Copy, Rename (ours), Share, Delete
        let tiles: Vec<Do> = m.quick.iter().map(|q| q.act.clone()).collect();
        assert_eq!(tiles, vec![Do::Shell(25), Do::Shell(26), Do::Ours(Act::Rename), Do::Shell(78), Do::Shell(18)]);
        assert!(m.quick[4].danger && !m.quick[0].danger);
    }

    #[test]
    fn a_verb_the_shell_does_not_offer_leaves_its_row_out_and_no_separator_is_left_dangling() {
        let items: Vec<ShellItem> = text_items().into_iter().filter(|i| !matches!(i.verb.as_str(), "edit" | "Windows.ModernShare" | "properties") && i.label != "Open with" && i.label != "Send to").collect();
        let m = build(&facts(Target::Text), &items, head());
        let l = labels(&m.rows);
        assert_eq!(l.len(), 6, "{l:?}: Open, rule, Copy as path, Compress, rule, More");
        assert_eq!(m.rows[1].kind, Kind::Sep);
        assert!(m.rows.last().unwrap().is_item() && m.rows.first().unwrap().is_item());
        assert_eq!(m.quick.len(), 4, "no Share tile");
        // two rules never stand together
        assert!(m.rows.windows(2).all(|w| w[0].is_item() || w[1].is_item()));
    }

    #[test]
    fn the_full_list_holds_everything_the_shell_gave_with_our_rename_after_delete() {
        let m = build(&facts(Target::Text), &text_items(), head());
        assert_eq!(m.full[0].act, Do::Less);
        assert_eq!(m.full[1].kind, Kind::Caption);
        let names: Vec<&str> = m.full.iter().filter(|r| r.is_item()).map(|r| r.label.as_str()).collect();
        for n in ["Open with Cursor", "Add to Favorites", "7-Zip", "Give access to", "Restore previous versions", "Create shortcut"] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        let delete = m.full.iter().position(|r| r.label == "Delete").unwrap();
        assert_eq!(m.full[delete + 1].act, Do::Ours(Act::Rename), "Rename follows Delete");
        // the long third-party label keeps its whole text (the panel cuts it with an ellipsis and tips it)
        assert!(m.full.iter().any(|r| r.label.starts_with("Send with DashBeam to a nearby device") && matches!(r.pic, Pic::Bitmap(_))));
        // the default item is bold, copy as path says its keys, submenus are there
        assert!(m.full.iter().find(|r| r.act == Do::Shell(136)).unwrap().bold);
        assert_eq!(m.full.iter().find(|r| r.act == Do::Shell(79)).unwrap().keys, ["Ctrl", "Shift", "C"]);
        assert!(matches!(m.full.iter().find(|r| r.label == "7-Zip").unwrap().sub, Some(Under::Lazy(1))));
        // no rule at the ends
        assert!(m.full.last().unwrap().is_item());
    }

    #[test]
    fn a_selection_of_several_items_has_no_rename_or_open() {
        let mut f = facts(Target::Several);
        f.flags = rq::DESKTOP;
        f.name = items_text(3);
        let m = build(&f, &text_items(), head());
        assert!(m.quick.iter().all(|q| q.act != Do::Ours(Act::Rename)));
        let l = labels(&m.rows);
        assert_eq!(l.len(), 6, "{l:?}: Compress, Copy as paths, Send to, rule, More, Properties");
        assert!(m.rows.iter().all(|r| r.act != Do::Shell(136)));
        assert!(m.full.iter().all(|r| r.act != Do::Ours(Act::Rename)));
    }

    #[test]
    fn an_app_has_run_as_administrator_pins_and_where_it_lives() {
        let mut items = text_items();
        items.insert(1, it(137, "runas", "Run as administrator"));
        items.insert(2, it(131, "opencontaining", "Open file location"));
        let mut f = facts(Target::App);
        f.start_pinned = true;
        let m = build(&f, &items, head());
        let acts: Vec<(String, Do)> = m.rows.iter().filter(|r| r.is_item()).map(|r| (r.label.clone(), r.act.clone())).collect();
        assert_eq!(acts[0].1, Do::Shell(136));
        assert_eq!(acts[1].1, Do::Shell(137));
        assert!(matches!(m.rows[1].pic, Pic::Line(_, Tint::Blue)), "the shield is blue");
        assert_eq!(acts[2].1, Do::Shell(131), "the shell's own Open file location when it has one");
        assert_eq!(acts[3].1, Do::PinStart);
        assert!(matches!(m.rows.iter().find(|r| r.act == Do::PinStart).unwrap().pic, Pic::Line(UNPIN, _)), "pinned: it offers to unpin");
        assert_eq!(acts[4].1, Do::PinBar);
        // an exe has no such item: ours reveals it
        let m = build(&facts(Target::App), &text_items(), head());
        assert!(m.rows.iter().any(|r| r.act == Do::Ours(Act::Reveal)));
    }

    #[test]
    fn a_folder_offers_tabs_only_where_there_are_tabs_and_pin_or_unpin() {
        let mut f = facts(Target::Folder);
        f.flags = rq::TAB | rq::PINNED;
        let m = build(&f, &text_items(), head());
        assert!(m.rows.iter().any(|r| r.act == Do::Ours(Act::NewTab)));
        assert!(m.rows.iter().any(|r| r.act == Do::Ours(Act::TogglePin)));
        assert!(m.quick.iter().all(|q| q.d != SHARE), "no Share tile on a folder");
        f.flags = rq::DESKTOP;
        let m = build(&f, &text_items(), head());
        assert!(m.rows.iter().all(|r| r.act != Do::Ours(Act::NewTab)), "the desktop has no tabs");
    }

    #[test]
    fn the_recycle_bin_has_open_empty_in_red_and_properties() {
        let mut items = vec![it(1, "open", "Open"), it(2, "empty", "Empty Recycle Bin"), ShellItem::sep(), it(3, "properties", "Properties")];
        items[0].flags = fl::DEFAULT;
        let mut f = facts(Target::Bin);
        f.name = "Recycle Bin".into();
        f.bin_items = 14;
        let m = build(&f, &items, head());
        assert!(m.quick.is_empty());
        let l = labels(&m.rows);
        assert_eq!(l.len(), 4, "{l:?}: Open, Empty, rule, Properties");
        assert_eq!(m.rows[1].act, Do::Shell(2));
        assert!(m.rows[1].danger && !m.rows[1].off && m.rows[1].badge.contains("14"));
        f.bin_items = 0;
        let m = build(&f, &items, head());
        assert!(m.rows[1].off && m.rows[1].act == Do::Nothing && !m.rows[1].danger, "an empty bin cannot be emptied");
    }

    #[test]
    fn a_submenu_keeps_bitmaps_and_drops_a_third_level_and_what_cannot_run() {
        let mut app = it(100, "", "Visual Studio Code");
        app.icon = Some(bitmap(0xff2c_8be6));
        let items = vec![ShellItem::sep(), app, it(101, "", "Cursor"), ShellItem::sep(), ShellItem { sub: Sub::Lazy(9), ..it(0, "", "CRC SHA") }, it(0, "", "garbage"), ShellItem::sep(), it(98, "openas", "Choose another app")];
        let rows = sub_rows(&items);
        let l = labels(&rows);
        assert_eq!(l, ["Visual Studio Code", "Cursor", "-", "Choose another app"], "no leading rule; no third level; the doubled rule is one");
        assert!(matches!(rows[0].pic, Pic::Bitmap(_)));
        assert!(matches!(rows[3].pic, Pic::Line(CHOOSE, _)), "Choose another app has the design's lens");
    }

    #[test]
    fn the_submenus_are_found_by_the_words_windows_uses_in_each_language() {
        let w = |l: &str| sub(l, 1);
        for l in ["Open with", "Открыть с помощью", "プログラムから開く", "Abrir com", "打开方式", "open with…"] {
            assert!(is_open_with(&w(l)), "{l}");
        }
        for l in ["Send to", "Отправить", "送る", "Enviar para", "发送到"] {
            assert!(is_send_to(&w(l)), "{l}");
        }
        assert!(!is_open_with(&it(1, "", "Open with")), "a plain item is not a submenu");
        assert!(!is_open_with(&w("7-Zip")) && !is_send_to(&w("Give access to")));
    }

    #[test]
    fn targets_are_told_by_what_they_are_called() {
        for (p, d, want) in [
            (r"C:\a\x.TXT", false, Target::Text),
            (r"C:\a\x.png", false, Target::Image),
            (r"C:\a\x.zip", false, Target::Archive),
            (r"C:\a\x.lnk", false, Target::App),
            (r"C:\a\x.EXE", false, Target::App),
            (r"C:\a\x.reg", false, Target::File),
            (r"C:\a\noext", false, Target::File),
            (r"C:\a\folder", true, Target::Folder),
            (r"C:\", true, Target::Drive),
            ("::{645FF040-5081-101B-9F08-00AA002F954E}", true, Target::Bin),
        ] {
            assert_eq!(classify(p, d), want, "{p}");
        }
    }

    #[test]
    fn sizes_read_like_explorers() {
        assert_eq!(size_text(1), "1 КБ");
        assert_eq!(size_text(18 * 1024), "18 КБ");
        assert_eq!(size_text(3_800_000), "3,6 МБ", "the Russian decimal comma");
        assert_eq!(meta_of("Text document", Some(2048)), "Text document · 2 КБ");
        assert_eq!(meta_of("Folder", None), "Folder");
        assert_eq!(keys_of("Ctrl + Shift+C"), ["Ctrl", "Shift", "C"]);
    }
}
