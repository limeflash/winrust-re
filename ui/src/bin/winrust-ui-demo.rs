//! `winrust-ui-demo`: one window with the design's caption, buttons, a menu / hint / toast / dialog, a search field and a list of
//! 10,000 rows with sortable columns (or a grid of tiles). It exists to look at the host and the widgets without an application, and
//! to measure them.
//!
//! ```text
//! winrust-ui-demo                                          the live window
//! winrust-ui-demo --render out.png [light] [scale=1.5] [state=search] [click=1,50] [hover=1006] [focus=200] [max]
//! winrust-ui-demo --render-all <dir>                       every state, dark and light, scale 1 and 1.5
//! winrust-ui-demo --selftest window | widgets | stage <png> | memory   checks on windows (off the screen, or a stage for under 2 s)
//! winrust-ui-demo --bench [open|hover|scroll|glide|type|resize|rest|all]
//! ```
//!
//! Item ids (for `click=`, `hover=`, `focus=`): 1 More menu, 2 Details, 3 Delete, 10..13 the four buttons, 20 switch, 30..32
//! segments, 40..42 chips, 50 Toast, 51 Toast with Undo, 52 Dialog, 53 Menu, 200 the search field (201 its clear button), 210 the
//! password field, 300.. the column titles (364.. their grips), 1000 the list (1001 its bar, 1002.. its rows).
//!
//! States: start, search, selection, scrolled, sorted, grid, password. `--selftest widgets` makes the demo (state `selftest`) keep
//! its clipboard in a variable and report what it holds in its window title.
#![windows_subsystem = "windows"]

use winrust_ui::anim::{self, EASE};
use winrust_ui::core::App;
use winrust_ui::frame::{Button, Cx, Frame, Id, Mods};
use winrust_ui::gfx::{Align, Canvas, Font, Rect, Rgba};
use winrust_ui::widgets::button::{self, Kind};
use winrust_ui::widgets::caption::{Caption, caption};
use winrust_ui::widgets::field::{self, FieldStyle, field_with};
use winrust_ui::widgets::field_model::{Ev, MemClip, TextField};
use winrust_ui::widgets::list::{ColItem, col_of, columns, grid, grid_key, list, list_key, row_of};
use winrust_ui::widgets::list_model::{Col, Columns, ListState};
use winrust_ui::widgets::look;
use winrust_ui::widgets::overlay::{Dialog, MenuItem};
use winrust_ui::widgets::scroll;
use winrust_ui::window::{Options, run};

const WIN: Rgba = Rgba::pair(0x16171b, 0xf7f7f9);
const BAR: Rgba = Rgba::pair(0x1c1d22, 0xeef0f3);

const TB_MORE: Id = 1;
const TB_INFO: Id = 2;
const TB_DEL: Id = 3;
const B_SAVE: Id = 10;
const TOGGLE: Id = 20;
const SEG: Id = 30;
const CHIP: Id = 40;
const B_TOAST: Id = 50;
const B_UNDO: Id = 51;
const B_DIALOG: Id = 52;
const B_MENU: Id = 53;
const SEARCH: Id = 200;
const PASS: Id = 210;
const COLS: Id = 300;
const LIST: Id = 1000;
const ROWS: usize = 10_000;
const ROW_H: f32 = 36.0;
const HEAD_H: f32 = 34.0;

const M_TAG: u32 = 1;
const D_TAG: u32 = 2;
const UNDO_TAG: u32 = 3;

const TRASH: &str = "M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13";
const FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4";

/// What a row of the list is: made once, drawn as often as it is in view.
struct Item {
    name: String,
    lower: String,
    kind: u8,
    kb: u32,
    mins: u32,
}

const STEMS: [&str; 12] = ["Report", "Invoice", "Holiday", "Notes", "Budget", "Sunset", "Backup", "Design", "Meeting", "Archive", "Photo", "Draft"];
const KINDS: [(&str, &str, u32); 7] = [("JPEG image", "jpg", 0xe86a8e), ("PNG image", "png", 0x7b6cf0), ("Text document", "txt", 0x8a90a2), ("PDF document", "pdf", 0xe5484d), ("Rust source", "rs", 0xffb46b), ("Archive", "zip", 0x62a0ea), ("Spreadsheet", "xlsx", 0x7fe0ad)];

fn items() -> Vec<Item> {
    (0..ROWS)
        .map(|k| {
            let kind = (k * 7 / 3 % 7) as u8;
            let name = format!("{} {:05}.{}", STEMS[k % 12], k, KINDS[kind as usize].1);
            Item { lower: name.to_lowercase(), name, kind, kb: ((k as u64 * 2_654_435_761) % 900_000) as u32 + 1_000, mins: ((k as u64 * 7919) % 525_600) as u32 }
        })
        .collect()
}

fn date(mins: u32) -> String {
    format!("2026-{:02}-{:02} {:02}:{:02}", mins / 43_200 + 1, mins % 43_200 / 1440 + 1, mins % 1440 / 60, mins % 60)
}

/// 10,000, as the mockups write counts.
fn grouped(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn size(kb: u32) -> String {
    if kb >= 1000 { format!("{:.1} MB", f64::from(kb) / 1000.0) } else { format!("{kb} KB") }
}

struct Demo {
    on: bool,
    seg: usize,
    chip: usize,
    info: bool,
    said: String,
    search: TextField,
    pass: TextField,
    cols: Columns,
    list: ListState,
    items: Vec<Item>,
    /// The items shown, in order: the search's matches, sorted by the header.
    order: Vec<u32>,
    /// Where the list is to be scrolled to once it knows its size (a state of `--render`).
    start_y: Option<f32>,
    /// The self-test's: the clipboard is a variable, and the window title says what the demo holds.
    mem: Option<MemClip>,
    probe: bool,
}

impl Demo {
    fn new(state: &str) -> Self {
        let col = |t: &str, w: f32, min: f32, right: bool| Col { title: t.into(), w, min, align_right: right };
        let mut cols = Columns::new(vec![col("Name", 0.0, 160.0, false), col("Modified", 152.0, 100.0, false), col("Type", 128.0, 80.0, false), col("Size", 84.0, 60.0, true)]);
        cols.sort = Some((0, true));
        let mut d = Self {
            on: true,
            seg: 0,
            chip: 0,
            info: false,
            said: String::new(),
            search: TextField::new(),
            pass: TextField::password(),
            cols,
            list: ListState::default(),
            items: items(),
            order: Vec::new(),
            start_y: None,
            mem: (state == "selftest").then(MemClip::default),
            probe: state == "selftest",
        };
        match state {
            "search" => d.search.set_text("report 000"),
            "selection" => {
                d.search.set_text("holiday 00");
                d.search.anchor = 8;
                d.search.caret = 10;
            }
            "scrolled" => d.start_y = Some(ROW_H * 120.0),
            "sorted" => {
                d.cols.sort = Some((3, false));
                d.start_y = Some((ROW_H + 1.0) * 9.0);
            }
            "grid" => d.seg = 1,
            "password" => d.pass.set_text("hunter2!"),
            _ => {}
        }
        d.resort();
        match state {
            "start" => d.list.click(3, false, false),
            "selection" => {
                d.list.click(2, false, false);
                d.list.click(6, false, true);
                d.list.click(9, true, false);
            }
            "scrolled" => d.list.click(125, false, false),
            "sorted" => d.list.click(11, false, false),
            "grid" => d.list.click(5, false, false),
            _ => {}
        }
        d
    }

    /// `order`: what the search matches, in the header's order.
    fn resort(&mut self) {
        let q = self.search.text.trim().to_lowercase();
        let it = &self.items;
        let mut v: Vec<u32> = (0..ROWS as u32).filter(|&k| q.is_empty() || it[k as usize].lower.contains(&q)).collect();
        if let Some((col, asc)) = self.cols.sort {
            match col {
                0 => v.sort_by(|&a, &b| it[a as usize].lower.cmp(&it[b as usize].lower)),
                1 => v.sort_by_key(|&k| it[k as usize].mins),
                2 => v.sort_by_key(|&k| (it[k as usize].kind, it[k as usize].mins)),
                _ => v.sort_by_key(|&k| it[k as usize].kb),
            }
            if !asc {
                v.reverse();
            }
        }
        self.order = v;
        self.list.set_len(self.order.len());
    }

    fn menu(&self) -> Vec<MenuItem> {
        vec![
            MenuItem::new("Set as wallpaper").icon("M3 5h18v12H3zM8 21h8M12 17v4M3 14l5-4 4 3 3-2 6 4"),
            MenuItem::new("Copy picture").icon("M8 8h11v11H8zM5 15V5h10").kbd("Ctrl+C"),
            MenuItem::new("Copy path").icon("M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"),
            MenuItem::new("Show in folder").icon("M3.5 7.5V18h17V9h-9l-2-2h-6z"),
            MenuItem::sep(),
            MenuItem::new("View").icon("M4 6h16M4 12h16M4 18h16").submenu(vec![MenuItem::new("List").radio(self.seg == 0), MenuItem::new("Icons").radio(self.seg == 1), MenuItem::new("Large icons").radio(self.seg == 2).disabled(true)]),
            MenuItem::new("Rotate left").icon("M4 11a8 8 0 1 1 2.3 5.7M4 4v7h7").kbd("Shift+R"),
            MenuItem::new("Move to Recycle Bin").icon(TRASH).kbd("Del").danger(),
        ]
    }

    /// The window's title is the self-test's window onto what the demo holds.
    fn report(&self, cx: &mut Cx) {
        if self.probe {
            let sel: Vec<String> = self.list.sel.iter().take(6).map(ToString::to_string).collect();
            let clip = self.mem.as_ref().and_then(|m| m.0.clone()).unwrap_or_default();
            cx.set_title(format!(
                "q={}|caret={}|anchor={}|pass={}|rows={}|sel={}|cursor={:?}|sort={:?}|w1={}|y={:.0}|clip={}|said={}",
                self.search.text, self.search.caret, self.search.anchor, self.pass.text, self.order.len(), sel.join(","), self.list.cursor, self.cols.sort, self.cols.cols[1].w, self.list.scroll.target(), clip, self.said
            ));
        }
    }

    fn after_query(&mut self, now: u64) {
        self.resort();
        self.list.scroll.scroll_to(0.0, now);
    }
}

impl App for Demo {
    fn title(&self) -> String {
        "WinRust UI demo".into()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, WIN);

        // the four kinds of button
        let mut x = 24.0;
        for (i, (label, kind)) in [("Save", Kind::Accent), ("Cancel", Kind::Quiet), ("Details", Kind::Outline), ("Delete", Kind::Danger)].into_iter().enumerate() {
            let bw = c.measure(label, Font::new(13.0, 500)).ceil() + 28.0;
            button::button(c, f, B_SAVE + i as Id, Rect::new(x, 72.0, bw, 34.0), label, kind);
            x += bw + 8.0;
        }

        // a switch, a segmented control, chips
        button::toggle(c, f, TOGGLE, Rect::new(24.0, 124.0, 38.0, 22.0), self.on);
        c.text(if self.on { "Switch is on" } else { "Switch is off" }, Rect::new(72.0, 120.0, 160.0, 30.0), look::BODY, look::T1, Align::Start);
        let labels = ["List", "Icons", "Large"];
        let sw = button::segmented_width(c, &labels);
        button::segmented(c, f, SEG, Rect::new(260.0, 120.0, sw, 30.0), &labels, self.seg);
        let mut x = 260.0 + sw + 24.0;
        for (i, l) in ["All", "Photos", "Videos"].into_iter().enumerate() {
            let cw = button::chip_width(c, l);
            button::chip(c, f, CHIP + i as Id, Rect::new(x, 122.0, cw, 26.0), l, self.chip == i);
            x += cw + 8.0;
        }

        // what opens the overlays
        let mut x = 24.0;
        for (id, l) in [(B_TOAST, "Toast"), (B_UNDO, "Toast with Undo"), (B_DIALOG, "Dialog"), (B_MENU, "Menu")] {
            let bw = c.measure(l, Font::new(13.0, 500)).ceil() + 28.0;
            button::button(c, f, id, Rect::new(x, 164.0, bw, 34.0), l, Kind::Quiet);
            x += bw + 8.0;
        }
        c.text(&self.said, Rect::new(x + 12.0, 164.0, (w - x - 36.0).max(0.0), 34.0), look::BODY, look::T3, Align::Start);

        // the search box grows when it has the focus (`.srch:focus-within{width:300px}`), a password field beside it
        let wide = f32::from(f.focused(SEARCH) || !self.search.text.is_empty());
        let sw = f.tween(SEARCH, 7, 220.0 + 80.0 * wide, 300, EASE);
        field_with(c, f, SEARCH, Rect::new(24.0, 212.0, sw, 34.0), &mut self.search, "Search files", &FieldStyle::SEARCH);
        field_with(c, f, PASS, Rect::new(24.0 + 300.0 + 16.0, 212.0, 220.0, 34.0), &mut self.pass, "Wi-Fi password", &FieldStyle::INPUT);
        c.text(&format!("{} items", grouped(self.order.len())), Rect::new(w - 24.0 - 200.0, 212.0, 200.0, 34.0), Font::new(12.0, 400).tabular(), look::T3, Align::End);

        // the list in a card: a header with sorting and grips, then only the rows that are in view
        let card = Rect::new(24.0, 262.0, w - 48.0, (h - 262.0 - 24.0).max(80.0));
        c.fill(card, 12.0, look::CARD);
        c.ring(card, 12.0, 1.0, Rgba::white(0.06));
        let n = self.order.len();
        let (items, order) = (&self.items, &self.order);
        if self.seg == 0 {
            let body = Rect::new(card.x + 1.0, card.y + HEAD_H, card.w - 2.0, card.h - HEAD_H - 1.0);
            if let Some(y) = self.start_y.take() {
                self.list.scroll.set_extent(4.0 + n as f32 * (ROW_H + 1.0) + 12.0, body.h);
                self.list.scroll.scroll_to(y, f.now);
            }
            columns(c, f, COLS, Rect::new(card.x + 1.0, card.y + 1.0, card.w - 2.0, HEAD_H - 1.0), &self.cols, 13.0);
            let cols = &self.cols;
            list(c, f, LIST, body, &mut self.list, n, ROW_H, |c, _f, i, r, rs| {
                let it = &items[order[i] as usize];
                let lay = cols.layout(r.w - 12.0);
                let x0 = r.x + 6.0;
                let (kind, _, tint) = KINDS[it.kind as usize];
                let (nx, nw) = lay[0];
                let tint = Rgba::hex(tint);
                c.fill(Rect::new(x0 + nx + 8.0 + 2.0, r.cy() - 11.0, 18.0, 22.0), 4.0, tint.alpha(0.2));
                c.icon(FILE, (x0 + nx + 8.0 + 11.0, r.cy()), 14.0, 2.0, tint, 0.0);
                let name_ink = look::T2.mix(look::T1, 0.6 + 0.4 * rs.hover.max(f32::from(rs.selected)));
                c.text_fit(&it.name, Rect::new(x0 + nx + 8.0 + 32.0, r.y, (nw - 16.0 - 32.0).max(0.0), r.h), Font::new(13.0, 500), name_ink, Align::Start);
                let cell = |c: &mut Canvas<'_>, k: usize, s: &str, ink: Rgba, right: bool| {
                    let (x, w) = lay[k];
                    c.text_fit(s, Rect::new(x0 + x + 8.0, r.y, (w - 16.0).max(0.0), r.h), Font::new(13.0, 400).tabular(), ink, if right { Align::End } else { Align::Start });
                };
                cell(c, 1, &date(it.mins), look::T2, false);
                cell(c, 2, kind, look::T3, false);
                cell(c, 3, &size(it.kb), look::T2, true);
            });
        } else {
            let body = Rect::new(card.x + 1.0, card.y + 1.0, card.w - 2.0, card.h - 2.0);
            let (min_w, cell_h, big) = if self.seg == 1 { (112.0, 112.0, 40.0) } else { (168.0, 150.0, 64.0) };
            grid(c, f, LIST, body, &mut self.list, n, min_w, cell_h, |c, _f, i, r, _| {
                let it = &items[order[i] as usize];
                let tint = Rgba::hex(KINDS[it.kind as usize].2);
                c.fill(Rect::new(r.cx() - big * 0.4, r.y + 14.0, big * 0.8, big), 6.0, tint.alpha(0.22));
                c.icon(FILE, (r.cx(), r.y + 14.0 + big / 2.0), big * 0.5, 1.6, tint, 0.0);
                c.text_fit(&it.name, Rect::new(r.x + 6.0, r.y + 14.0 + big + 4.0, r.w - 12.0, 20.0), Font::new(12.0, 500), look::T1, Align::Center);
                c.text(&size(it.kb), Rect::new(r.x + 6.0, r.y + 14.0 + big + 22.0, r.w - 12.0, 16.0), Font::new(11.5, 400).tabular(), look::T3, Align::Center);
            });
        }

        // the caption is drawn last: its hairline lies on the body
        let tools = caption(
            c,
            f,
            Rect::new(0.0, 0.0, w, 48.0),
            &Caption {
                bg: BAR,
                title: "WinRust UI demo",
                sub: &format!("{} rows", grouped(ROWS)),
                glyph: "M4 18l5-6 4 4 3-3 4 5M15.5 8.5h.01",
                tile: [Rgba::hex(0xffb46b), Rgba::hex(0xe86a8e), Rgba::hex(0x7b6cf0)],
                max: f.maximized(),
                tools_w: 3.0 * 32.0 + 2.0 * 2.0 + 4.0,
            },
        );
        let mut x = tools.x + 2.0;
        c.fill(Rect::new(tools.x, 8.0, tools.w, 32.0), 10.0, Rgba::white(0.05));
        for (id, icon, tip, on) in [
            (TB_MORE, "M5 12h.6M11.7 12h.6M18.4 12h.6", "More", false),
            (TB_INFO, "M12 11v5M12 8h.01M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z", "Details (I)", self.info),
            (TB_DEL, TRASH, "Move to Recycle Bin (Del)", false),
        ] {
            button::icon_button(c, f, id, Rect::new(x, 8.0, 32.0, 32.0), icon, tip, on);
            x += 34.0;
        }
    }

    fn down(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        let now = anim::now_ms();
        let n = self.order.len();
        match id {
            SEARCH => self.search.mouse_down(x, cx.mods.shift, now),
            PASS => self.pass.mouse_down(x, cx.mods.shift, now),
            i if i == scroll::bar_id(LIST) => {
                scroll::bar_down(&mut self.list.scroll, y, now);
            }
            i if i >= LIST + 2 => {
                if let Some(r) = row_of(LIST, i).filter(|&r| r < n) {
                    if b == Button::Right && !self.list.sel.contains(&r) {
                        self.list.click(r, false, false);
                    } else if b == Button::Left {
                        self.list.press(r, cx.mods.ctrl, cx.mods.shift);
                    }
                }
            }
            i => {
                if let Some(ColItem::Grip(k)) = col_of(COLS, i) {
                    self.cols.grip_down(k, x);
                }
            }
        }
        self.report(cx);
        cx.invalidate();
    }

    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        let now = anim::now_ms();
        match (id, b) {
            (TB_MORE, _) => cx.menu(M_TAG, self.menu(), Rect::new(x - 16.0, 44.0, 32.0, 0.0)),
            (B_MENU, _) => cx.menu(M_TAG, self.menu(), Rect::new(x - 40.0, y + 18.0, 80.0, 0.0)),
            (TB_INFO, _) => self.info = !self.info,
            (TB_DEL, _) | (B_DIALOG, _) => cx.dialog(D_TAG, Dialog::new("Move 3 files to the Recycle Bin?", "They stay there until you empty it, so this can be undone.", &["Cancel", "Move"]).icon(TRASH, true)),
            (B_TOAST, _) => cx.toast("Copied to the clipboard", None),
            (B_UNDO, _) => cx.toast("Moved Sunset.jpg to the Recycle Bin", Some(UNDO_TAG)),
            (TOGGLE, _) => self.on = !self.on,
            (i, _) if (SEG..SEG + 3).contains(&i) => {
                self.seg = (i - SEG) as usize;
                self.list.shown = None;
            }
            (i, _) if (CHIP..CHIP + 3).contains(&i) => self.chip = (i - CHIP) as usize,
            (i, _) if i == SEARCH + 1 => {
                self.search.set_text("");
                self.after_query(now);
                cx.focus(SEARCH);
            }
            (i, _) if i >= LIST + 2 => {
                if let Some(r) = row_of(LIST, i).filter(|&r| r < self.order.len()) {
                    self.list.lift(r);
                    if b == Button::Right {
                        cx.menu(M_TAG, self.menu(), Rect::new(x, y, 0.0, 0.0));
                    }
                }
            }
            (i, Button::Left) => {
                if let Some(ColItem::Title(k)) = col_of(COLS, i) {
                    self.cols.sort_by(k);
                    self.after_query(now);
                }
            }
            _ => {}
        }
        self.report(cx);
        cx.invalidate();
    }

    fn double(&mut self, id: Id, cx: &mut Cx) {
        if let Some(r) = row_of(LIST, id).filter(|&r| id >= LIST + 2 && r < self.order.len()) {
            self.said = format!("opened {}", self.items[self.order[r] as usize].name);
            self.report(cx);
            cx.invalidate();
        }
    }

    fn drag(&mut self, id: Id, x: f32, y: f32, cx: &mut Cx) {
        let now = anim::now_ms();
        match id {
            SEARCH => self.search.mouse_drag(x, now),
            PASS => self.pass.mouse_drag(x, now),
            i if i == scroll::bar_id(LIST) => scroll::bar_drag(&mut self.list.scroll, y, now),
            _ => {
                self.cols.grip_drag(x);
            }
        }
        self.report(cx);
        cx.invalidate();
    }

    fn release(&mut self, id: Id, cx: &mut Cx) {
        match id {
            SEARCH => self.search.mouse_up(),
            PASS => self.pass.mouse_up(),
            i if i == scroll::bar_id(LIST) => scroll::bar_up(&mut self.list.scroll),
            _ => self.cols.grip_up(),
        }
        cx.invalidate();
    }

    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        scroll::wheel(&mut self.list.scroll, cx.pointer, dy, ROW_H, anim::now_ms());
        self.report(cx);
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        self.said = match tag {
            M_TAG => format!("menu: choice {choice}"),
            D_TAG => format!("dialog: button {choice}"),
            _ => "Undo".into(),
        };
        if tag == D_TAG && choice == 1 {
            cx.toast("Moved 3 files to the Recycle Bin", Some(UNDO_TAG));
        }
        self.report(cx);
        cx.invalidate();
    }

    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        let now = anim::now_ms();
        let focus = cx.focus;
        let n = self.order.len();
        if matches!(focus, Some(SEARCH | PASS)) {
            let search = focus == Some(SEARCH);
            let tf = if search { &mut self.search } else { &mut self.pass };
            let ev = match self.mem.as_mut() {
                Some(clip) => tf.key(vk, m, cx.alt, now, clip),
                None => field::key(tf, cx, vk, m, now),
            };
            match ev {
                Ev::Edited if search => self.after_query(now),
                Ev::Esc if search && !self.search.text.is_empty() => {
                    self.search.set_text("");
                    self.after_query(now);
                }
                _ => {}
            }
            // Down leaves the search for the list
            let down = !ev.used() && vk == 0x28 && search && n > 0;
            if down {
                cx.focus(LIST);
                self.list.step(1, false, false, n);
            }
            self.report(cx);
            cx.invalidate();
            return ev.used() || down;
        }
        let paging = (self.list.scroll.view() / ROW_H) as usize;
        let used = if self.seg == 0 { list_key(&mut self.list, vk, m.shift, m.ctrl, n, paging) } else { grid_key(&mut self.list, vk, m.shift, m.ctrl, n, paging) };
        if used {
            self.report(cx);
            cx.invalidate();
        }
        used
    }

    fn text(&mut self, s: &str, cx: &mut Cx) {
        let now = anim::now_ms();
        match cx.focus {
            Some(SEARCH) => {
                field::text(&mut self.search, s, now);
                self.after_query(now);
            }
            Some(PASS) => field::text(&mut self.pass, s, now),
            _ => {
                // typing a name in the list jumps to it
                let (items, order, n) = (&self.items, &self.order, self.order.len());
                for ch in s.chars() {
                    self.list.typeahead(ch, now, n, |i| order.get(i).map(|&k| items[k as usize].name.clone()));
                }
            }
        }
        self.report(cx);
        cx.invalidate();
    }
}

const STATES: [&str; 7] = ["start", "search", "selection", "scrolled", "sorted", "grid", "password"];

fn demo(state: &str) -> Box<dyn App> {
    Box::new(Demo::new(state))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = winrust_ui::render::cli(&args, demo, &STATES) {
        winrust_ui::window::quit(code as u32);
    }
    let code = run(demo("start"), Options { class: "winrust_ui_demo", size: (1160.0, 740.0), icon: 0, place_file: None });
    winrust_ui::window::quit(code as u32);
}
