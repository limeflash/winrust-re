//! What the window can be told to do: open, delete, copy, paste, rename, make a new folder, the menus. Keys, clicks and menu rows all
//! come here; what touches the machine goes to the [`Backend`].
use std::path::PathBuf;

use winrust_common::{t, tf};
use winrust_ui::{
    frame::{Cx, Id},
    gfx::Rect,
    widgets::{field_model::TextField, overlay::MenuItem},
};

use super::{
    backend::{Do, Flag},
    model::{Fm, ID_LIST, Ren, TAG_FOLDER, TAG_ITEM, TAG_PLACE, View, pc_dir},
};

// the choices of the folder's menu
pub const F_VIEW_LIST: usize = 1;
pub const F_VIEW_ICONS: usize = 2;
pub const F_SORT: usize = 10; // 10..14: name, date, type, size
pub const F_ASC: usize = 20;
pub const F_DESC: usize = 21;
pub const F_FOLDERS_FIRST: usize = 22;
pub const F_GROUP: usize = 23;
pub const F_NEW_FOLDER: usize = 30;
pub const F_NEW_TEXT: usize = 31;
pub const F_PASTE: usize = 32;
pub const F_REFRESH: usize = 33;
pub const F_TERMINAL: usize = 34;
pub const F_DISK: usize = 35;
pub const F_PIN: usize = 36;
pub const F_PROPS: usize = 37;

// the choices of an item's menu
pub const I_OPEN: usize = 1;
pub const I_OPEN_TAB: usize = 2;
pub const I_CUT: usize = 3;
pub const I_COPY: usize = 4;
pub const I_COPY_PATH: usize = 6;
pub const I_RENAME: usize = 7;
pub const I_ZIP: usize = 8;
pub const I_EXTRACT: usize = 9;
pub const I_PIN: usize = 10;
pub const I_DELETE: usize = 11;
pub const I_PROPS: usize = 12;
pub const I_SHOW_IN_FOLDER: usize = 13;
pub const I_RUNAS: usize = 14;
pub const I_OPEN_HERE: usize = 15;

/// The menu icons, as the mockup's `I` writes them.
mod ic {
    pub const OPEN: &str = "M5 12h14M13 6l6 6-6 6";
    pub const TAB: &str = "M3 7h7l2 2h9v10H3zM16 13v4M14 15h4";
    pub const CUT: &str = "M6 7a2.5 2.5 0 1 0 0-.01M6 17a2.5 2.5 0 1 0 0 .01M8 8l12 9M8 16L20 7";
    pub const COPY: &str = "M9 9h11v11H9zM5 15V4h11";
    pub const PASTE: &str = "M8 4h8v3H8zM6 6H5v15h14V6h-1M9 12h6M9 16h4";
    pub const REN: &str = "M4 20h4L19 9l-4-4L4 16zM13 7l4 4";
    pub const ZIP: &str = "M7 3h10v18H7zM12 3v2M12 7v2M12 11v2M11 15h2v3h-2z";
    pub const UNZIP: &str = "M4 8h16v12H4zM4 8l2-4h12l2 4M10 12h4";
    pub const PIN: &str = "M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5M9 15l-4-4 2-2 5 1 4-4 1-3";
    pub const DEL: &str = "M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13";
    pub const PROPS: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v6M12 7.5h.01";
    pub const VIEW: &str = "M4 5h16v14H4zM4 10h16M10 10v9";
    pub const SORT: &str = "M7 4v16M3 16l4 4 4-4M14 6h7M14 11h5M14 16h3";
    pub const GROUP: &str = "M4 5h16M4 10h10M4 15h16M4 20h10";
    pub const NEW: &str = "M12 5v14M5 12h14";
    pub const REFRESH: &str = "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5";
    pub const DISK: &str = "M12 3v9l7 4M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z";
    pub const TERM: &str = "M4 5h16v14H4zM8 10l3 2-3 2M13 15h3";
    pub const COPY_PATH: &str = "M9 9h11v11H9zM5 15V4h11M12 13h5M12 16h3";
}

impl Fm {
    // -----------------------------------------------------------------------------------------------------------------
    // opening

    /// Enter and a double click on row `i`; `new_tab` (a middle click) opens a folder in a new tab.
    pub fn open_row(&mut self, i: usize, new_tab: bool, cx: &mut Cx) {
        let Some(r) = self.rows.get(i) else { return };
        let (p, dir) = (self.row_path(i), r.dir);
        if dir {
            if new_tab { self.tab_open(p, cx) } else { self.go(p, cx) }
        } else if crate::look_zip(&p) && self.be.is_target(&p) {
            // a zip opens like a folder; "Extract here" is in its menu
            if new_tab { self.tab_open(p, cx) } else { self.go(p, cx) }
        } else {
            self.be.run(self.hwnd, Do::Open(p));
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // clipboard, deleting, making

    pub fn copy(&mut self, cut: bool, cx: &mut Cx) {
        let paths = self.sel_paths();
        if paths.is_empty() || self.at_pc() {
            return;
        }
        match self.be.real_paths(&paths) {
            // nothing can be cut out of an archive
            Ok(real) => self.be.run(self.hwnd, Do::Clip { paths: real, cut: cut && !self.in_zip() }),
            Err(e) => cx.toast(e, None),
        }
    }

    pub fn paste(&mut self) {
        if !self.in_zip() && !self.at_pc() {
            self.be.run(self.hwnd, Do::Paste(self.dir.clone()));
        }
    }

    pub fn delete(&mut self) {
        let paths = self.sel_paths();
        if paths.is_empty() || self.in_zip() || self.at_pc() {
            return;
        }
        self.be.run(self.hwnd, Do::Delete { paths, dir: self.dir.clone() });
    }

    /// A new folder (or text document), named as Explorer names them, with its name already open for editing.
    pub fn new_entry(&mut self, folder: bool, cx: &mut Cx) {
        if self.in_zip() || self.at_pc() {
            return;
        }
        if self.searching() {
            self.query = None;
            self.srch.set_text("");
        }
        let Some(name) = self.be.new_entry(&self.dir, folder) else { return };
        let d = self.dir.clone();
        self.load(d, false, false, cx);
        self.select_names(std::slice::from_ref(&name));
        if let Some(i) = self.one() {
            self.rename_begin(i, cx);
        }
    }

    pub fn props(&mut self) {
        let p = self.current().map_or_else(|| self.dir.clone(), |i| self.row_path(i));
        if !self.at_pc() || self.current().is_some() {
            self.be.run(self.hwnd, Do::Props(p));
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // rename

    pub fn rename_begin(&mut self, i: usize, cx: &mut Cx) {
        if self.in_zip() || self.at_pc() || self.searching() {
            return;
        }
        let Some(r) = self.rows.get(i) else { return };
        let mut field = TextField::new();
        field.set_text(&r.name);
        // a file's name is selected without its extension, as in Explorer
        let stem = if r.dir { r.name.len() } else { r.name.rfind('.').filter(|&d| d > 0).unwrap_or(r.name.len()) };
        field.anchor = 0;
        field.caret = stem;
        self.ren = Some(Ren { name: r.name.clone(), field, bad: 0 });
        self.list.click(i, false, false);
        self.list.shown = None; // the row comes into view, wherever the wheel left the list
        cx.focus(super::model::ID_REN);
        cx.invalidate();
    }

    pub fn rename_cancel(&mut self, cx: &mut Cx) {
        if self.ren.take().is_some() {
            // the keys go back to the list
            cx.focus(ID_LIST);
            cx.invalidate();
        }
    }

    /// Enter in the box, or the box losing the pointer: renames when the name is good, shakes and says why when it is not.
    pub fn rename_commit(&mut self, cx: &mut Cx) {
        let Some(r) = self.ren.as_mut() else { return };
        let (old, new) = (r.name.clone(), r.field.text.trim().to_string());
        if new.is_empty() || new == old {
            return self.rename_cancel(cx);
        }
        if new.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
            r.bad += 1;
            cx.toast(t!("В имени не может быть \\ / : * ? \" < > |", "A name can\u{2019}t contain \\ / : * ? \" < > |"), None);
            cx.invalidate();
            return;
        }
        if self.rows.iter().any(|x| x.name.eq_ignore_ascii_case(&new) && x.name != old) {
            r.bad += 1;
            cx.toast(tf!("Здесь уже есть «{}»", "There is already an item called \u{201C}{}\u{201D} here", ja: "「{}」は既に存在します", pt: "Já existe \u{201C}{}\u{201D} aqui", zh: "此处已有「{}」", new), None);
            cx.invalidate();
            return;
        }
        self.ren = None;
        match self.be.rename(&self.dir, &old, &new) {
            Ok(()) => {
                let d = self.dir.clone();
                self.load(d, false, false, cx);
                self.select_names(std::slice::from_ref(&new));
            }
            Err(e) => cx.toast(e, None),
        }
        cx.focus(ID_LIST);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // menus

    /// The menu of the selected rows: the bar draws the shell's own; without a bar the window's own rows are shown.
    pub fn item_menu(&mut self, at: Option<(f32, f32)>, cx: &mut Cx) {
        let paths = self.sel_paths();
        if paths.is_empty() {
            return;
        }
        if self.be.bar() && !self.in_zip() && !self.at_pc() {
            self.be.run(self.hwnd, Do::ItemMenu { dir: self.dir.clone(), paths, at });
            return;
        }
        self.item_menu_own(at, cx);
    }

    /// The window's own menu for the selection (no bar, an archive, a drive).
    pub fn item_menu_own(&mut self, at: Option<(f32, f32)>, cx: &mut Cx) {
        let (x, y) = at.or(cx.pointer).unwrap_or((self.geo.size.0 / 2.0, self.geo.size.1 / 2.0));
        let items = self.item_items();
        if items.is_empty() {
            return;
        }
        cx.menu(TAG_ITEM, items, Rect::new(x, y, 0.0, 0.0));
    }

    fn item_items(&self) -> Vec<MenuItem> {
        let sel = self.list.sel.iter().copied().filter(|&i| i < self.rows.len()).collect::<Vec<_>>();
        let one = (sel.len() == 1).then(|| sel[0]);
        let searching = self.searching();
        let zip = self.in_zip();
        let mut v = Vec::new();
        if self.at_pc() {
            // a drive
            v.push(MenuItem::new(t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开")).icon(ic::OPEN).kbd("Enter").value(I_OPEN));
            v.push(MenuItem::new(t!("Открыть в новой вкладке", "Open in new tab", ja: "新しいタブで開く", pt: "Abrir em nova guia", zh: "在新标签页中打开")).icon(ic::TAB).value(I_OPEN_TAB));
            if self.be.tool("wiztree") {
                v.push(MenuItem::sep());
                v.push(MenuItem::new(t!("Занятое место (WinRust Disk)", "Disk usage (WinRust Disk)", ja: "ディスク使用量 (WinRust Disk)", pt: "Uso do disco (WinRust Disk)", zh: "磁盘占用（WinRust Disk）")).icon(ic::DISK).value(I_OPEN_HERE));
            }
            return v;
        }
        let all_dirs = sel.iter().all(|&i| self.rows[i].dir);
        if let Some(i) = one {
            v.push(MenuItem::new(t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开")).icon(ic::OPEN).kbd("Enter").value(I_OPEN));
            if self.rows[i].dir {
                v.push(MenuItem::new(t!("Открыть в новой вкладке", "Open in new tab", ja: "新しいタブで開く", pt: "Abrir em nova guia", zh: "在新标签页中打开")).icon(ic::TAB).kbd(t!("Средняя кнопка", "Middle click")).value(I_OPEN_TAB));
            } else if matches!(self.rows[i].name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref(), Some("exe" | "bat" | "cmd" | "msi" | "lnk")) && !zip {
                v.push(MenuItem::new(t!("Запуск от имени администратора", "Run as administrator", ja: "管理者として実行", pt: "Executar como administrador", zh: "以管理员身份运行")).icon(ic::OPEN).value(I_RUNAS));
            }
            if searching {
                v.push(MenuItem::new(t!("Показать в папке", "Show in folder", ja: "フォルダーで表示", pt: "Mostrar na pasta", zh: "在文件夹中显示")).icon(ic::OPEN).value(I_SHOW_IN_FOLDER));
            }
        }
        v.push(MenuItem::sep());
        v.push(MenuItem::new(t!("Вырезать", "Cut", ja: "切り取り", pt: "Recortar", zh: "剪切")).icon(ic::CUT).kbd("Ctrl+X").disabled(zip).value(I_CUT));
        v.push(MenuItem::new(t!("Копировать", "Copy", ja: "コピー", pt: "Copiar", zh: "复制")).icon(ic::COPY).kbd("Ctrl+C").value(I_COPY));
        v.push(MenuItem::new(t!("Копировать как путь", "Copy as path", ja: "パスのコピー", pt: "Copiar como caminho", zh: "复制文件地址")).icon(ic::COPY_PATH).value(I_COPY_PATH));
        v.push(MenuItem::sep());
        if let Some(i) = one {
            if !searching && !zip {
                v.push(MenuItem::new(t!("Переименовать", "Rename", ja: "名前の変更", pt: "Renomear", zh: "重命名")).icon(ic::REN).kbd("F2").value(I_RENAME));
            }
            let p = self.row_path(i);
            if crate::look_zip(&p) && !self.rows[i].dir {
                v.push(MenuItem::new(t!("Извлечь сюда", "Extract here", ja: "ここに展開", pt: "Extrair aqui", zh: "解压到此处")).icon(ic::UNZIP).value(I_EXTRACT));
            } else if !zip && !self.be.busy() {
                v.push(MenuItem::new(t!("Сжать в ZIP", "Compress to ZIP", ja: "ZIP に圧縮", pt: "Compactar em ZIP", zh: "压缩为 ZIP")).icon(ic::ZIP).value(I_ZIP));
            }
            if self.rows[i].dir && !zip {
                let pinned = self.places.quick.iter().any(|q| q.path == p && q.pinned);
                v.push(MenuItem::new(if pinned { t!("Открепить от быстрого доступа", "Unpin from Quick access", ja: "クイック アクセスからピン留めを解除", pt: "Desafixar do Acesso rápido", zh: "从快速访问取消固定") } else { t!("Закрепить в быстром доступе", "Pin to Quick access", ja: "クイック アクセスにピン留め", pt: "Fixar no Acesso rápido", zh: "固定到快速访问") }).icon(ic::PIN).value(I_PIN));
            }
        } else if !zip && !self.be.busy() && !sel.is_empty() {
            v.push(MenuItem::new(t!("Сжать в ZIP", "Compress to ZIP", ja: "ZIP に圧縮", pt: "Compactar em ZIP", zh: "压缩为 ZIP")).icon(ic::ZIP).value(I_ZIP));
        }
        if !zip {
            v.push(MenuItem::sep());
            v.push(MenuItem::new(t!("Удалить", "Delete", ja: "削除", pt: "Excluir", zh: "删除")).icon(ic::DEL).kbd("Del").danger().value(I_DELETE));
            v.push(MenuItem::sep());
            v.push(MenuItem::new(t!("Свойства", "Properties", ja: "プロパティ", pt: "Propriedades", zh: "属性")).icon(ic::PROPS).kbd("Alt+Enter").disabled(one.is_none()).value(I_PROPS));
        }
        let _ = all_dirs;
        v
    }

    /// A row of the item menu was chosen.
    pub fn item_command(&mut self, choice: usize, cx: &mut Cx) {
        let paths = self.sel_paths();
        let one = self.one();
        match choice {
            I_OPEN => {
                if let Some(i) = one.or(self.current()) {
                    self.open_row(i, false, cx);
                }
            }
            I_OPEN_TAB => {
                if let Some(i) = one {
                    self.open_row(i, true, cx);
                }
            }
            I_OPEN_HERE => {
                if let Some(i) = one {
                    self.be.run(self.hwnd, Do::DiskUsage(self.row_path(i)));
                }
            }
            I_RUNAS => {
                if let Some(p) = paths.first() {
                    self.be.run(self.hwnd, Do::RunAs(p.clone()));
                }
            }
            I_SHOW_IN_FOLDER => {
                if let Some(p) = paths.first().cloned()
                    && let Some(d) = p.parent()
                {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                    self.load(d.to_path_buf(), true, false, cx);
                    if let Some(n) = name {
                        self.select_names(&[n]);
                    }
                }
            }
            I_CUT => self.copy(true, cx),
            I_COPY => self.copy(false, cx),
            I_COPY_PATH => self.be.run(self.hwnd, Do::CopyPaths(paths)),
            I_RENAME => {
                if let Some(i) = one {
                    self.rename_begin(i, cx);
                }
            }
            I_ZIP => self.be.run(self.hwnd, Do::Zip { dir: self.dir.clone(), paths }),
            I_EXTRACT => self.be.run(self.hwnd, Do::Extract { dir: self.dir.clone(), paths }),
            I_PIN => {
                if let Some(p) = paths.first().cloned() {
                    self.be.run(self.hwnd, Do::Pin(p));
                    self.places = self.be.places();
                }
            }
            I_DELETE => self.delete(),
            I_PROPS => self.props(),
            _ => {}
        }
        cx.invalidate();
    }

    /// The folder's own menu (right click on empty space, "More").
    pub fn folder_menu(&mut self, at: Rect, cx: &mut Cx) {
        let mut v: Vec<MenuItem> = Vec::new();
        let pc = self.at_pc();
        v.push(
            MenuItem::new(t!("Вид", "View", ja: "表示", pt: "Exibir", zh: "查看")).icon(ic::VIEW).submenu(vec![
                MenuItem::new(t!("Список", "List", ja: "リスト", pt: "Lista", zh: "列表")).radio(self.view == View::List).value(F_VIEW_LIST),
                MenuItem::new(t!("Крупные значки", "Large icons", ja: "大きいアイコン", pt: "Ícones grandes", zh: "大图标")).radio(self.view == View::Icons).value(F_VIEW_ICONS),
            ]),
        );
        let (col, asc) = self.sort;
        let by = |i: usize, label: &str| MenuItem::new(label).radio(col == i).value(F_SORT + i);
        v.push(MenuItem::new(t!("Сортировка", "Sort by", ja: "並べ替え", pt: "Classificar por", zh: "排序方式")).icon(ic::SORT).submenu(vec![
            by(0, t!("По имени", "Name", ja: "名前", pt: "Nome", zh: "名称")),
            by(1, t!("По дате изменения", "Date modified", ja: "更新日時", pt: "Data de modificação", zh: "修改日期")),
            by(2, t!("По типу", "Type", ja: "種類", pt: "Tipo", zh: "类型")),
            by(3, t!("По размеру", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小")),
            MenuItem::sep(),
            MenuItem::new(t!("По возрастанию", "Ascending", ja: "昇順", pt: "Crescente", zh: "升序")).radio(asc).value(F_ASC),
            MenuItem::new(t!("По убыванию", "Descending", ja: "降順", pt: "Decrescente", zh: "降序")).radio(!asc).value(F_DESC),
            MenuItem::sep(),
            MenuItem::new(t!("Папки сверху", "Folders first", ja: "フォルダーを先頭に表示", pt: "Pastas primeiro", zh: "文件夹优先")).radio(self.folders_first).value(F_FOLDERS_FIRST),
        ]));
        if !pc {
            v.push(MenuItem::new(if self.group { t!("Не группировать", "Ungroup", ja: "グループ化を解除", pt: "Desagrupar", zh: "取消分组") } else { t!("Группировать по дате", "Group by date", ja: "日付でグループ化", pt: "Agrupar por data", zh: "按日期分组") }).icon(ic::GROUP).value(F_GROUP));
            v.push(MenuItem::sep());
            v.push(MenuItem::new(t!("Создать", "New", ja: "新規作成", pt: "Novo", zh: "新建")).icon(ic::NEW).disabled(self.in_zip() || self.searching()).submenu(vec![
                MenuItem::new(t!("Папку", "Folder", ja: "フォルダー", pt: "Pasta", zh: "文件夹")).value(F_NEW_FOLDER),
                MenuItem::new(t!("Текстовый документ", "Text document", ja: "テキスト ドキュメント", pt: "Documento de texto", zh: "文本文档")).value(F_NEW_TEXT),
            ]));
            v.push(MenuItem::new(t!("Вставить", "Paste", ja: "貼り付け", pt: "Colar", zh: "粘贴")).icon(ic::PASTE).kbd("Ctrl+V").disabled(self.in_zip() || self.searching()).value(F_PASTE));
        } else {
            v.push(MenuItem::sep());
        }
        v.push(MenuItem::new(t!("Обновить", "Refresh", ja: "最新の情報に更新", pt: "Atualizar", zh: "刷新")).icon(ic::REFRESH).kbd("F5").value(F_REFRESH));
        if !pc && !self.in_zip() {
            v.push(MenuItem::sep());
            v.push(MenuItem::new(t!("Открыть в Терминале", "Open in Terminal", ja: "ターミナルで開く", pt: "Abrir no Terminal", zh: "在终端中打开")).icon(ic::TERM).value(F_TERMINAL));
            if self.be.tool("wiztree") {
                v.push(MenuItem::new(t!("Занятое место (WinRust Disk)", "Disk usage (WinRust Disk)", ja: "ディスク使用量 (WinRust Disk)", pt: "Uso do disco (WinRust Disk)", zh: "磁盘占用（WinRust Disk）")).icon(ic::DISK).value(F_DISK));
            }
            let pinned = self.places.quick.iter().any(|q| q.path == self.dir && q.pinned);
            let builtin = self.places.quick.iter().any(|q| q.path == self.dir && !q.pinned);
            if !builtin {
                v.push(MenuItem::new(if pinned { t!("Открепить от быстрого доступа", "Unpin from Quick access", ja: "クイック アクセスからピン留めを解除", pt: "Desafixar do Acesso rápido", zh: "从快速访问取消固定") } else { t!("Закрепить в быстром доступе", "Pin to Quick access", ja: "クイック アクセスにピン留め", pt: "Fixar no Acesso rápido", zh: "固定到快速访问") }).icon(ic::PIN).value(F_PIN));
            }
            v.push(MenuItem::new(t!("Свойства", "Properties", ja: "プロパティ", pt: "Propriedades", zh: "属性")).icon(ic::PROPS).value(F_PROPS));
        }
        cx.menu(TAG_FOLDER, v, at);
    }

    pub fn folder_command(&mut self, choice: usize, cx: &mut Cx) {
        match choice {
            F_VIEW_LIST => self.set_view(View::List, cx),
            F_VIEW_ICONS => self.set_view(View::Icons, cx),
            c if (F_SORT..F_SORT + 4).contains(&c) => {
                let col = c - F_SORT;
                self.set_sort(col, self.sort.1, cx);
            }
            F_ASC => self.set_sort(self.sort.0, true, cx),
            F_DESC => self.set_sort(self.sort.0, false, cx),
            F_FOLDERS_FIRST => self.set_folders_first(!self.folders_first, cx),
            F_GROUP => self.set_group(!self.group, cx),
            F_NEW_FOLDER => self.new_entry(true, cx),
            F_NEW_TEXT => self.new_entry(false, cx),
            F_PASTE => self.paste(),
            F_REFRESH => self.reload(cx),
            F_TERMINAL => self.be.run(self.hwnd, Do::Terminal(self.dir.clone())),
            F_DISK => self.be.run(self.hwnd, Do::DiskUsage(self.dir.clone())),
            F_PIN => {
                self.be.run(self.hwnd, Do::Pin(self.dir.clone()));
                self.places = self.be.places();
            }
            F_PROPS => self.props(),
            _ => {}
        }
        cx.invalidate();
    }

    /// The menu of a place of the sidebar.
    pub fn place_menu(&mut self, i: usize, at: (f32, f32), cx: &mut Cx) {
        let Some(p) = self.places.quick.get(i) else { return };
        let mut v = vec![
            MenuItem::new(t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开")).icon(ic::OPEN).value(I_OPEN),
            MenuItem::new(t!("Открыть в новой вкладке", "Open in new tab", ja: "新しいタブで開く", pt: "Abrir em nova guia", zh: "在新标签页中打开")).icon(ic::TAB).value(I_OPEN_TAB),
        ];
        if p.pinned {
            v.push(MenuItem::sep());
            v.push(MenuItem::new(t!("Открепить от быстрого доступа", "Unpin from Quick access", ja: "クイック アクセスからピン留めを解除", pt: "Desafixar do Acesso rápido", zh: "从快速访问取消固定")).icon(ic::PIN).value(I_PIN));
        }
        self.menu_paths = vec![p.path.clone()];
        cx.menu(TAG_PLACE, v, Rect::new(at.0, at.1, 0.0, 0.0));
    }

    pub fn place_command(&mut self, choice: usize, cx: &mut Cx) {
        let Some(p) = self.menu_paths.first().cloned() else { return };
        match choice {
            I_OPEN => self.go(p, cx),
            I_OPEN_TAB => self.tab_open(p, cx),
            I_PIN => {
                self.be.run(self.hwnd, Do::Pin(p));
                self.places = self.be.places();
            }
            _ => {}
        }
        cx.invalidate();
    }

    /// The sort and grouping the folder's menu may change are the window's: a flag the backend keeps for next time.
    pub fn flags_from_backend(&mut self) {
        self.group = self.be.flag(Flag::Group);
        self.folders_first = self.be.flag(Flag::FoldersFirst);
    }

    /// The preview pane's toggle (the button, and Space with nothing to open).
    pub fn toggle_pv(&mut self, cx: &mut Cx) {
        self.pv = !self.pv;
        cx.invalidate();
    }

    /// Closes what the keys no longer belong to when the pointer goes down elsewhere (the address box, the rename box).
    pub fn pointer_down_on(&mut self, id: Id, cx: &mut Cx) {
        if self.ren.is_some() && id != super::model::ID_REN {
            self.rename_commit(cx);
        }
        if self.addr_edit && id != super::model::ID_ADDR && id != super::model::ID_ADDR_PILL {
            self.addr_edit = false;
            self.addr.set_text(&self.dir_text());
            self.addr_bad = false;
        }
    }

    /// Opens the address box for typing a path (Ctrl+L, F4, a click on the empty part of the pill).
    pub fn addr_begin(&mut self, cx: &mut Cx) {
        self.addr_edit = true;
        self.addr_bad = false;
        let s = self.dir_text();
        self.addr.set_text(&s);
        self.addr.select_all();
        cx.focus(super::model::ID_ADDR);
        cx.invalidate();
    }

    /// Enter in the address box: a folder opens, "? terms" searches, anything else shakes the box.
    pub fn addr_enter(&mut self, cx: &mut Cx) {
        let text = self.addr.text.trim().to_string();
        if let Some(q) = text.strip_prefix('?') {
            let q = q.trim();
            if !q.is_empty() {
                self.addr_edit = false;
                self.srch.set_text(q);
                self.run_search(q, cx);
                cx.focus(ID_LIST);
                return;
            }
        }
        let p = PathBuf::from(text.trim_matches('"'));
        if text.eq_ignore_ascii_case(&self.label_of(&pc_dir())) {
            self.addr_edit = false;
            self.go(pc_dir(), cx);
            cx.focus(ID_LIST);
        } else if !text.is_empty() && self.be.is_target(&p) {
            self.addr_edit = false;
            self.go(p, cx);
            cx.focus(ID_LIST);
        } else {
            self.addr_bad = true;
            cx.toast(tf!("Не удаётся найти «{}»", "Can\u{2019}t find \u{201C}{}\u{201D}", ja: "「{}」が見つかりません", pt: "Não foi possível encontrar \u{201C}{}\u{201D}", zh: "找不到「{}」", text), None);
            cx.invalidate();
        }
    }

    pub fn addr_cancel(&mut self, cx: &mut Cx) {
        self.addr_edit = false;
        self.addr_bad = false;
        self.addr.set_text(&self.dir_text());
        cx.focus(ID_LIST);
        cx.invalidate();
    }

    pub fn search_focus(&mut self, cx: &mut Cx) {
        cx.focus(super::model::ID_SEARCH);
        cx.invalidate();
    }
}
