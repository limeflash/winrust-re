//! The window's side of the file manager: the events of `winrust-ui` (pointer, wheel, keys, text, messages from workers, drops) become
//! what the model does. Nothing here knows of an `HWND`; the keys that are listed at the top of `main.rs` are all here.
use std::path::PathBuf;

use winrust_ui::{
    anim::now_ms,
    core::{App, DropFx},
    frame::{Button, Cx, Id, Mods},
    gfx::{Canvas, Rect},
    widgets::{
        field,
        field_model::Ev,
        list::{ColItem, col_of, grid_key, list_key, row_of},
        overlay::TOAST_UNDO,
        scroll::{bar_down, bar_drag, bar_id, bar_up, wheel},
    },
};

use super::{
    backend::Do,
    live::{MSG_DONE, MSG_DRAG_DONE, MSG_MENU_OWN, MSG_BACK, MSG_FORWARD, MSG_NEWTAB, MSG_OPEN, MSG_REGISTER, MSG_PLACES, MSG_RELOAD, MSG_RENAME, MSG_THUMB, MSG_TICK, MSG_WATCH, ThumbMsg},
    model::*,
};

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DELETE: u32 = 0x2E;
const VK_F2: u32 = 0x71;
const VK_F4: u32 = 0x73;
const VK_F5: u32 = 0x74;
const VK_F10: u32 = 0x79;
const VK_APPS: u32 = 0x5D;

impl Fm {
    fn rows_in_view(&self) -> usize {
        self.geo.rows_per_page.max(1)
    }

    /// A key for the list (and the window's own keys); true when it was used.
    fn list_keys(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        let ctrl = m.ctrl && !cx.alt;
        let (shift, alt) = (m.shift, cx.alt);
        let n = self.rows.len();
        // Alt: the history and Properties (the old window took them before the list could)
        if alt {
            match vk {
                VK_LEFT => self.back(cx),
                VK_RIGHT => self.forward(cx),
                VK_UP => self.up(cx),
                VK_RETURN => self.props(),
                _ => return false,
            }
            return true;
        }
        match vk {
            VK_RETURN => {
                if let Some(i) = self.current() {
                    self.open_row(i, false, cx);
                }
            }
            VK_BACK => self.up(cx),
            VK_F2 => {
                if let Some(i) = self.current() {
                    self.rename_begin(i, cx);
                }
            }
            VK_DELETE => self.delete(),
            VK_F5 => self.reload(cx),
            VK_F4 => self.addr_begin(cx),
            VK_ESCAPE => {
                if self.be.busy() {
                    self.be.run(self.hwnd, Do::JobCancel);
                } else if self.searching() {
                    self.clear_search(cx);
                } else {
                    self.list.sel.clear();
                }
            }
            VK_SPACE if !ctrl => {
                // a quick look at the picked file; with nothing to open the preview pane is shown or hidden
                self.skip_space = true;
                match self.one().filter(|&i| !self.rows[i].dir && !self.at_pc()) {
                    Some(i) => self.be.run(self.hwnd, Do::Open(self.row_path(i))),
                    None => self.toggle_pv(cx),
                }
            }
            VK_APPS => self.item_or_folder_menu_from_key(cx),
            VK_F10 if shift => self.item_or_folder_menu_from_key(cx),
            0x43 if ctrl => self.copy(false, cx),
            0x58 if ctrl => self.copy(true, cx),
            0x56 if ctrl => self.paste(),
            0x4E if ctrl => self.new_entry(true, cx),
            0x54 if ctrl => self.tab_new(cx),
            0x57 if ctrl => self.tab_close(self.active, cx),
            VK_TAB if ctrl => self.tab_next(shift, cx),
            0x4D if ctrl => self.toggle_view(cx),
            0x4C if ctrl => self.addr_begin(cx),
            0x46 if ctrl => self.search_focus(cx),
            0x48 if ctrl => {
                self.hidden = !self.hidden;
                self.reload(cx);
            }
            _ => {
                let used = if self.view == View::Icons {
                    let lines = self.geo.lines_per_page;
                    grid_key(&mut self.list, vk, shift, ctrl, n, lines)
                } else {
                    let per = self.rows_in_view();
                    list_key(&mut self.list, vk, shift, ctrl, n, per)
                };
                return used;
            }
        }
        true
    }

    fn item_or_folder_menu_from_key(&mut self, cx: &mut Cx) {
        if self.list.sel.is_empty() {
            let at = Rect::new(self.geo.content.x + 40.0, self.geo.content.y + 60.0, 0.0, 0.0);
            self.folder_menu(at, cx);
            return;
        }
        // from the keyboard the menu opens at the lower middle of the picked row
        let at = self
            .list
            .cursor
            .and_then(|i| self.geo.row_rect(i))
            .map(|r| (r.cx(), r.y + r.h - 10.0));
        self.item_menu(at, cx);
    }

    /// Typed text: the focused box takes it, otherwise it is a name being typed to jump to.
    fn typed(&mut self, s: &str, cx: &mut Cx) {
        let now = now_ms();
        match cx.focus {
            Some(ID_ADDR) => {
                field::text(&mut self.addr, s, now);
                self.addr_bad = false;
            }
            Some(ID_SEARCH) => field::text(&mut self.srch, s, now),
            Some(ID_REN) => {
                if let Some(r) = self.ren.as_mut() {
                    field::text(&mut r.field, s, now);
                }
            }
            _ => {
                if self.skip_space && s == " " {
                    self.skip_space = false;
                    return;
                }
                self.skip_space = false;
                let n = self.rows.len();
                for ch in s.chars() {
                    let names: Vec<String> = (0..n).map(|i| self.shown_name(i)).collect();
                    if self.list.typeahead(ch, now, n, |i| names.get(i).cloned()).is_some() {
                        self.list.shown = None;
                    }
                }
            }
        }
    }

    fn row_at_id(&self, id: Id) -> Option<usize> {
        row_of(ID_LIST, id).filter(|&i| i < self.rows.len() && id < ID_LIST + 2 + self.rows.len() as Id)
    }

    /// A tab dragged over its neighbours takes their place.
    fn drag_tab(&mut self, x: f32) {
        let Some(i) = self.tab_drag else { return };
        let target = self.geo.tabs.iter().position(|r| x >= r.x && x < r.x + r.w);
        if let Some(j) = target.filter(|&j| j != i) {
            self.tabs.swap(i, j);
            if self.active == i {
                self.active = j;
            } else if self.active == j {
                self.active = i;
            }
            self.tab_drag = Some(j);
        }
    }

    /// Where a drop at `(x, y)` would land, if anywhere: a crumb or place, a folder row, or the folder shown.
    fn drop_at(&self, x: f32, y: f32) -> Option<DropAt> {
        if let Some((_, _, p)) = self.geo.crumbs.iter().find(|(_, r, _)| r.contains(x, y)) {
            // the last crumb is the folder shown
            if *p != self.dir && *p != pc_dir() {
                return Some(DropAt::Place(p.clone()));
            }
            return None;
        }
        if let Some((_, p)) = self.geo.places.iter().find(|(r, _)| r.contains(x, y)) {
            return Some(DropAt::Place(p.clone()));
        }
        if let Some((_, i)) = self.geo.rows.iter().find(|(r, i)| r.contains(x, y) && self.rows.get(*i).is_some_and(|r| r.dir)) {
            return Some(DropAt::Row(*i, self.row_path(*i)));
        }
        if self.geo.content.contains(x, y) && !self.at_pc() && !self.in_zip() && !self.searching() {
            return Some(DropAt::Here);
        }
        None
    }

    fn drop_path(&self, d: &DropAt) -> PathBuf {
        match d {
            DropAt::Place(p) | DropAt::Row(_, p) => p.clone(),
            DropAt::Here => self.dir.clone(),
        }
    }

    /// The first message after a drag that started here finished.
    fn drag_over_done(&mut self) {
        self.dragging = false;
        self.press = None;
    }

    fn open_folder_menu_at_pointer(&mut self, x: f32, y: f32, cx: &mut Cx) {
        self.folder_menu(Rect::new(x, y, 0.0, 0.0), cx);
    }
}

impl App for Fm {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn min_size(&self) -> (f32, f32) {
        (680.0, 420.0)
    }

    fn caption_height(&self) -> f32 {
        super::draw::CAPTION_H
    }

    fn start(&mut self, hwnd: isize) {
        self.hwnd = hwnd;
        self.be.run(hwnd, Do::Start);
        // the folder was listed before the window was there: it is watched for the window now
        if !self.at_pc() && !self.searching() {
            self.be.run(hwnd, Do::Navigated(self.dir.clone()));
        }
    }

    fn demo_clicks(&self) -> Vec<Id> {
        match self.hook {
            Hook::None => Vec::new(),
            _ => vec![ID_DEMO],
        }
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut winrust_ui::frame::Frame<'_>) {
        self.paint(c, f);
    }

    fn down(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        let now = now_ms();
        self.pointer_down_on(id, cx);
        if let Some(i) = self.row_at_id(id) {
            match b {
                Button::Left => {
                    self.list.press(i, cx.mods.ctrl, cx.mods.shift);
                    self.press = Some((i, x, y));
                }
                Button::Right => {
                    if !self.list.sel.contains(&i) {
                        self.list.click(i, false, false);
                    }
                }
                Button::Middle => {}
            }
        } else if id == bar_id(ID_LIST) {
            bar_down(&mut self.list.scroll, y, now);
        } else if id == bar_id(ID_SIDE) {
            bar_down(&mut self.side, y, now);
        } else if let Some(ColItem::Grip(k)) = col_of(ID_COLS, id) {
            self.cols.grip_down(k, x);
        } else if id == ID_ADDR {
            self.addr.mouse_down(x, cx.mods.shift, now);
        } else if id == ID_SEARCH {
            self.srch.mouse_down(x, cx.mods.shift, now);
        } else if id == ID_REN {
            if let Some(r) = self.ren.as_mut() {
                r.field.mouse_down(x, cx.mods.shift, now);
            }
        } else if (ID_TAB..ID_TAB + MAX_TABS as Id).contains(&id) && b == Button::Left {
            self.tab_drag = Some((id - ID_TAB) as usize);
        } else if id == ID_LIST && b == Button::Left && !cx.mods.ctrl && !cx.mods.shift {
            // a press on empty space of the list lets go of the selection
            self.list.sel.clear();
        }
        cx.invalidate();
    }

    fn drag(&mut self, id: Id, x: f32, y: f32, cx: &mut Cx) {
        let now = now_ms();
        if id == bar_id(ID_LIST) {
            bar_drag(&mut self.list.scroll, y, now);
        } else if id == bar_id(ID_SIDE) {
            bar_drag(&mut self.side, y, now);
        } else if matches!(col_of(ID_COLS, id), Some(ColItem::Grip(_))) {
            self.cols.grip_drag(x);
        } else if id == ID_ADDR {
            self.addr.mouse_drag(x, now);
        } else if id == ID_SEARCH {
            self.srch.mouse_drag(x, now);
        } else if id == ID_REN {
            if let Some(r) = self.ren.as_mut() {
                r.field.mouse_drag(x, now);
            }
        } else if self.tab_drag.is_some() {
            self.drag_tab(x);
        } else if let (Some((i, px, py)), false) = (self.press, self.dragging)
            && row_of(ID_LIST, id) == Some(i)
            && winrust_ui::widgets::list_model::drag_threshold(x - px, y - py)
        {
            // the rows are dragged out: the press was no click
            self.list.drop_held();
            self.dragging = true;
            if !self.list.sel.contains(&i) {
                self.list.click(i, false, false);
            }
            let paths = self.sel_paths();
            if !paths.is_empty() {
                self.be.run(self.hwnd, Do::Drag(paths));
            }
        }
        cx.invalidate();
    }

    fn release(&mut self, id: Id, cx: &mut Cx) {
        if id == bar_id(ID_LIST) {
            bar_up(&mut self.list.scroll);
        } else if id == bar_id(ID_SIDE) {
            bar_up(&mut self.side);
        } else if matches!(col_of(ID_COLS, id), Some(ColItem::Grip(_))) {
            self.cols.grip_up();
        } else if id == ID_ADDR {
            self.addr.mouse_up();
        } else if id == ID_SEARCH {
            self.srch.mouse_up();
        } else if id == ID_REN
            && let Some(r) = self.ren.as_mut()
        {
            r.field.mouse_up();
        }
        self.tab_drag = None;
        if !self.dragging {
            self.press = None;
        }
        cx.invalidate();
    }

    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        if let Some(i) = self.row_at_id(id) {
            match b {
                Button::Left => self.list.lift(i),
                Button::Middle => {
                    if self.rows.get(i).is_some_and(|r| r.dir) {
                        self.open_row(i, true, cx);
                    }
                }
                Button::Right => self.item_menu(None, cx),
            }
            cx.invalidate();
            return;
        }
        match id {
            ID_DEMO => match self.hook {
                Hook::ItemMenu => {
                    // the menu of the picked row, where the design puts it (420, 300)
                    self.item_menu_own(Some((420.0, 300.0)), cx);
                }
                Hook::FolderMenu => self.open_folder_menu_at_pointer(640.0, 520.0, cx),
                Hook::None => {}
            },
            ID_BACK => self.back(cx),
            ID_FWD => self.forward(cx),
            ID_UP => self.up(cx),
            ID_ADDR_PILL => {
                if b == Button::Left && !self.addr_edit {
                    self.addr_begin(cx);
                }
            }
            i if i == ID_SEARCH + 1 => {
                self.clear_search(cx);
                cx.focus(ID_SEARCH);
            }
            ID_VIEW => self.set_view(View::List, cx),
            i if i == ID_VIEW + 1 => self.set_view(View::Icons, cx),
            ID_PV => self.toggle_pv(cx),
            ID_MORE => {
                let r = self.geo.more;
                self.folder_menu(r, cx);
            }
            ID_NEWTAB => self.tab_new(cx),
            ID_REFRESH => self.reload(cx),
            i if (ID_TAB..ID_TAB + MAX_TABS as Id).contains(&i) => {
                let k = (i - ID_TAB) as usize;
                if b == Button::Middle { self.tab_close(k, cx) } else { self.tab_select(k, cx) }
            }
            i if (ID_TAB_X..ID_TAB_X + MAX_TABS as Id).contains(&i) => self.tab_close((i - ID_TAB_X) as usize, cx),
            i if (ID_CRUMB..ID_CRUMB + 40).contains(&i) => {
                if let Some(p) = self.geo.crumbs.iter().find(|(id, ..)| *id == i).map(|(.., p)| p.clone()) {
                    if b == Button::Middle { self.tab_open(p, cx) } else if p != self.dir {
                        self.go(p, cx);
                    }
                }
            }
            ID_PC => {
                if b == Button::Middle { self.tab_open(pc_dir(), cx) } else { self.go(pc_dir(), cx) }
            }
            i if (ID_PLACE..ID_PLACE + 90).contains(&i) => {
                let k = (i - ID_PLACE) as usize;
                if let Some(p) = self.place(k).map(|p| p.path.clone()) {
                    match b {
                        Button::Left => self.go(p, cx),
                        Button::Middle => self.tab_open(p, cx),
                        Button::Right => self.place_menu(k, (x, y), cx),
                    }
                }
            }
            i if (ID_DRIVE..ID_DRIVE + 40).contains(&i) => {
                let k = (i - ID_DRIVE) as usize;
                if let Some(p) = self.places.drives.get(k).map(|d| d.root.clone()) {
                    if b == Button::Middle { self.tab_open(p, cx) } else { self.go(p, cx) }
                }
            }
            i if (ID_EJECT..ID_EJECT + 40).contains(&i) => {
                if let Some(d) = self.places.drives.get((i - ID_EJECT) as usize).map(|d| d.root.clone()) {
                    self.be.run(self.hwnd, Do::Eject(d));
                    // ejecting itself comes in the next part of the redesign; until then the notification area's "Safely remove" does it
                    cx.toast(winrust_common::t!("Извлечение скоро появится: пока используйте значок в области уведомлений", "Eject is coming soon: use the tray icon for now", ja: "取り出しは近日対応: 今は通知領域のアイコンを使ってください", pt: "Ejetar em breve: use o ícone da bandeja por enquanto", zh: "弹出功能即将推出：请先使用托盘图标"), None);
                }
            }
            ID_JOB_CANCEL => self.be.run(self.hwnd, Do::JobCancel),
            ID_PV_OPEN => {
                if let Some(i) = self.one() {
                    self.open_row(i, false, cx);
                }
            }
            ID_PV_MORE => {
                if self.one().is_some() {
                    self.item_menu(Some((x, y)), cx);
                }
            }
            ID_EMPTY_BTN => {
                if self.searching() { self.clear_search(cx) } else { self.new_entry(true, cx) }
            }
            ID_LIST | ID_CONTENT | ID_PV_BODY => {
                if b == Button::Right {
                    self.list.sel.clear();
                    self.open_folder_menu_at_pointer(x, y, cx);
                } else if let Some(ColItem::Title(k)) = col_of(ID_COLS, id) {
                    self.sort_by(k, cx);
                }
            }
            _ => {
                if let (Some(ColItem::Title(k)), Button::Left) = (col_of(ID_COLS, id), b) {
                    self.sort_by(k, cx);
                }
            }
        }
        self.settle(cx);
        cx.invalidate();
    }

    fn double(&mut self, id: Id, cx: &mut Cx) {
        if let Some(i) = self.row_at_id(id) {
            if self.ren.is_none() {
                self.open_row(i, false, cx);
            }
        } else if id == ID_LIST || id == ID_CONTENT {
            // a double click on empty space goes up one folder
            self.up(cx);
        }
        self.settle(cx);
    }

    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        let now = now_ms();
        if !wheel(&mut self.list.scroll, cx.pointer, dy, ROW_STEP, now) {
            wheel(&mut self.side, cx.pointer, dy, 32.0, now);
        }
        cx.invalidate();
    }

    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        let now = now_ms();
        // Alt and a key is the history whatever has the focus (the old window took them before any box could)
        if cx.alt && matches!(vk, VK_LEFT | VK_RIGHT | VK_UP | VK_RETURN) {
            let used = self.list_keys(vk, m, cx);
            self.settle(cx);
            return used;
        }
        match cx.focus {
            Some(ID_ADDR) => {
                let ev = field::key(&mut self.addr, cx, vk, m, now);
                match ev {
                    Ev::Enter => self.addr_enter(cx),
                    Ev::Esc => self.addr_cancel(cx),
                    Ev::Edited => self.addr_bad = false,
                    _ => {}
                }
                if ev.used() || vk == VK_TAB {
                    self.settle(cx);
                    return ev.used();
                }
            }
            Some(ID_SEARCH) => {
                let ev = field::key(&mut self.srch, cx, vk, m, now);
                match ev {
                    Ev::Enter => {
                        let q = self.srch.text.clone();
                        if q.trim().is_empty() { self.clear_search(cx) } else { self.run_search(&q, cx) }
                        cx.focus(ID_LIST);
                    }
                    Ev::Esc => {
                        if self.srch.text.is_empty() && !self.searching() {
                            cx.focus(ID_LIST);
                        } else {
                            self.clear_search(cx);
                            cx.focus(ID_LIST);
                        }
                    }
                    _ => {}
                }
                if ev.used() {
                    self.settle(cx);
                    return true;
                }
                // Ctrl+F in the box selects it again; Down goes to the results
                if vk == 0x28 {
                    cx.focus(ID_LIST);
                    if self.list.cursor.is_none() && !self.rows.is_empty() {
                        self.list.click(0, false, false);
                    }
                    return true;
                }
            }
            Some(ID_REN) => {
                let ev = match self.ren.as_mut() {
                    Some(r) => field::key(&mut r.field, cx, vk, m, now),
                    None => Ev::Ignored,
                };
                match ev {
                    Ev::Enter => self.rename_commit(cx),
                    Ev::Esc => self.rename_cancel(cx),
                    _ => {}
                }
                if ev.used() {
                    self.settle(cx);
                    return true;
                }
            }
            _ => {}
        }
        let used = self.list_keys(vk, m, cx);
        self.settle(cx);
        cx.invalidate();
        used
    }

    fn text(&mut self, s: &str, cx: &mut Cx) {
        self.typed(s, cx);
        cx.invalidate();
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        match tag {
            TAG_FOLDER => self.folder_command(choice, cx),
            TAG_ITEM => self.item_command(choice, cx),
            TAG_PLACE => self.place_command(choice, cx),
            _ => {}
        }
        if choice == TOAST_UNDO as usize {
            // no Undo is offered yet
        }
        self.settle(cx);
    }

    fn message(&mut self, msg: u32, wp: usize, lp: isize, cx: &mut Cx) {
        match msg {
            MSG_WATCH => self.watch_event(wp == 1, cx),
            MSG_REGISTER => self.be.run(self.hwnd, Do::Register),
            MSG_BACK => self.back(cx),
            MSG_FORWARD => self.forward(cx),
            MSG_RENAME => {
                if let Some(i) = self.current() {
                    self.rename_begin(i, cx);
                }
            }
            MSG_OPEN => {
                // SAFETY: `lp` is the `Box<PathBuf>` the bar's menu posted for this message, taken back exactly once.
                let p = unsafe { *Box::from_raw(lp as *mut PathBuf) };
                let dir = p.is_dir();
                if dir || (crate::look_zip(&p) && self.be.is_target(&p)) { self.go(p, cx) } else { self.be.run(self.hwnd, Do::Open(p)) }
            }
            MSG_NEWTAB => {
                // SAFETY: `lp` is the `Box<PathBuf>` the bar's menu posted for this message, taken back exactly once.
                let p = unsafe { *Box::from_raw(lp as *mut PathBuf) };
                self.tab_open(p, cx);
            }
            MSG_THUMB => {
                // SAFETY: `wp` is a `Box<ThumbMsg>` leaked by the picture worker for this window, taken back exactly once.
                let m = unsafe { *Box::from_raw(wp as *mut ThumbMsg) };
                super::live::count_picture();
                self.got_picture(m.era, m.path, m.icon, (m.w, m.h), m.px);
                cx.invalidate();
            }
            m if m == crate::WM_JOB => {
                // SAFETY: `wp` is the `Box<Option<String>>` the job's worker posted, taken back exactly once.
                let err = unsafe { *Box::from_raw(wp as *mut Option<String>) };
                self.job = None;
                self.reload(cx);
                if let Some(e) = err {
                    cx.toast(e, None);
                }
            }
            m if m == crate::shellwin::WM_SHELL_SELECT => {
                // SAFETY: `lp` is the `Box<PathBuf>` the shell's thread posted, taken back exactly once.
                let p = unsafe { *Box::from_raw(lp as *mut PathBuf) };
                if let (Some(dir), Some(name)) = (p.parent(), p.file_name()) {
                    if !self.dir.as_os_str().eq_ignore_ascii_case(dir.as_os_str()) || self.searching() {
                        self.load(dir.to_path_buf(), true, false, cx);
                    }
                    let name = name.to_string_lossy().into_owned();
                    self.reveal_names(std::slice::from_ref(&name));
                    if self.list.sel.is_empty() {
                        // a file that has just been written (a download that finished) is not in the listing yet
                        self.reload(cx);
                        self.reveal_names(&[name]);
                    }
                }
                super::live::raise(self.hwnd);
            }
            m if m == crate::WM_DIRSIZE => {
                // SAFETY: `wp` is the `Box<(u32, String, u64)>` the size worker posted, taken back exactly once.
                let (era, name, n) = unsafe { *Box::from_raw(wp as *mut (u32, String, u64)) };
                if era == self.era
                    && !self.searching()
                    && let Some(r) = self.rows.iter_mut().find(|r| r.dir && r.name == name)
                {
                    r.size = n;
                }
            }
            MSG_TICK => self.job = self.be.job(),
            MSG_RELOAD | MSG_DONE => {
                self.job = self.be.job();
                self.places = self.be.places(); // what was copied or deleted changed the free space of a drive
                self.reload(cx);
            }
            MSG_PLACES => self.places = self.be.places(),
            MSG_MENU_OWN => self.item_menu_own(None, cx),
            MSG_DRAG_DONE => self.drag_over_done(),
            _ => {}
        }
        let _ = lp;
        self.settle(cx);
        cx.invalidate();
    }

    fn wake(&mut self, cx: &mut Cx) {
        if self.watch_pending {
            self.watch_event(false, cx);
            self.settle(cx);
        }
    }

    fn drag_over(&mut self, paths: &[String], x: f32, y: f32) -> bool {
        let at = self.drop_at(x, y);
        // dragging the rows over the folder they come from is no drop
        let own_dir = paths.first().and_then(|p| std::path::Path::new(p).parent()).map(std::path::Path::to_path_buf);
        let ok = match &at {
            None => false,
            Some(d) => {
                let to = self.drop_path(d);
                // a folder cannot go into itself
                !paths.iter().any(|p| std::path::Path::new(p) == to || to.starts_with(p)) && own_dir.as_deref() != Some(to.as_path())
            }
        };
        let cross = paths.first().zip(at.as_ref()).is_some_and(|(p, d)| std::path::Path::new(p).components().next() != self.drop_path(d).components().next());
        self.fx_move.set(!cross);
        self.drop_hl = if ok { at.filter(|d| !matches!(d, DropAt::Here)) } else { None };
        ok
    }

    fn drag_leave(&mut self, _cx: &mut Cx) {
        self.drop_hl = None;
    }

    fn drag_effect(&self, _paths: &[String], ctrl: bool) -> DropFx {
        self.fx_ctrl.set(ctrl);
        if ctrl || !self.fx_move.get() { DropFx::Copy } else { DropFx::Move }
    }

    fn drop(&mut self, paths: Vec<String>, x: f32, y: f32, cx: &mut Cx) {
        let at = self.drop_at(x, y);
        self.drop_hl = None;
        let Some(at) = at else { return };
        let to = self.drop_path(&at);
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        if paths.is_empty() || paths.iter().any(|p| *p == to || to.starts_with(p)) || paths[0].parent() == Some(to.as_path()) {
            return;
        }
        // like Explorer: the same drive moves, another one copies, Ctrl forces a copy
        let copy = self.fx_ctrl.get() || paths[0].components().next() != to.components().next();
        self.be.run(self.hwnd, Do::DropOn { paths, to, copy });
        cx.invalidate();
    }

    fn close(&mut self, _cx: &mut Cx) -> bool {
        true
    }
}

/// How far a notch of the wheel moves the list (3 rows).
const ROW_STEP: f32 = ROW_H_STEP;
const ROW_H_STEP: f32 = 37.0;
