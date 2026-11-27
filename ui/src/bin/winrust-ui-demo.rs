//! `winrust-ui-demo`: one window with the design's caption, buttons, a menu / hint / toast / dialog and a scrollable body. It exists
//! to look at the host and the widgets without an application, and to measure them.
//!
//! ```text
//! winrust-ui-demo                                          the live window
//! winrust-ui-demo --render out.png [light] [scale=1.5] [state=scrolled] [click=1,50] [hover=10] [focus=11] [max]
//! winrust-ui-demo --render-all <dir>                       every state, dark and light, scale 1 and 1.5
//! winrust-ui-demo --selftest window | stage <png> | memory  checks on windows (off the screen, or a stage for under 2 s)
//! winrust-ui-demo --bench [open|hover|scroll|resize|rest|all]
//! ```
//!
//! Item ids (for `click=`, `hover=`, `focus=`): 1 More menu, 2 Details, 3 Delete, 10..13 the four buttons, 20 switch, 30..32
//! segments, 40..42 chips, 50 Toast, 51 Toast with Undo, 52 Dialog, 53 Menu, 100.. rows of the list.
#![windows_subsystem = "windows"]

use winrust_ui::anim::EASE;
use winrust_ui::core::App;
use winrust_ui::frame::{Button, Cx, Frame, Id, Mods};
use winrust_ui::gfx::{Align, Canvas, Font, Rect, Rgba};
use winrust_ui::widgets::button::{self, Kind};
use winrust_ui::widgets::caption::{Caption, caption};
use winrust_ui::widgets::look;
use winrust_ui::widgets::overlay::{Dialog, MenuItem};
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
const ROW: Id = 100;
const ROWS: usize = 60;
const ROW_H: f32 = 36.0;

const M_TAG: u32 = 1;
const D_TAG: u32 = 2;
const UNDO_TAG: u32 = 3;

const TRASH: &str = "M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13";

struct Demo {
    on: bool,
    seg: usize,
    chip: usize,
    row: Option<usize>,
    /// Where the list is scrolled to, DIPs; the picture glides there.
    to: f32,
    max_scroll: f32,
    info: bool,
    said: String,
}

impl Demo {
    fn new(state: &str) -> Self {
        let scrolled = state == "scrolled";
        Self { on: true, seg: 1, chip: 0, row: Some(3), to: if scrolled { 540.0 } else { 0.0 }, max_scroll: 0.0, info: false, said: String::new() }
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

        // the scrolling list in a card
        let card = Rect::new(24.0, 218.0, w - 48.0, (h - 218.0 - 24.0).max(40.0));
        c.fill(card, 12.0, look::CARD);
        c.ring(card, 12.0, 1.0, Rgba::white(0.06));
        let view = card.inset(1.0);
        let content = ROWS as f32 * ROW_H;
        self.max_scroll = (content - view.h).max(0.0);
        self.to = self.to.clamp(0.0, self.max_scroll);
        let y = f.tween(9000, 0, self.to, 150, EASE);
        f.clipped(c, view, |c, f| {
            let first = (y / ROW_H).floor().max(0.0) as usize;
            for i in first..ROWS {
                let r = Rect::new(view.x + 6.0, view.y + 6.0 + i as f32 * ROW_H - y, view.w - 12.0 - 10.0, ROW_H - 2.0);
                if r.y > view.y + view.h {
                    break;
                }
                let hot = f.item(ROW + i as Id, r);
                let sel = self.row == Some(i);
                c.fill(r, 8.0, Rgba::white(0.06 * hot.hover + if sel { 0.1 } else { 0.0 }));
                c.text(&format!("Row {}", i + 1), Rect::new(r.x + 12.0, r.y, 120.0, r.h), look::BODY, look::T1, Align::Start);
                c.text("A line of text to scroll past, with figures 0123456789", Rect::new(r.x + 120.0, r.y, r.w - 240.0, r.h), look::BODY, look::T3, Align::Start);
                c.text(&format!("{} KB", 120 + i * 37 % 900), Rect::new(r.x, r.y, r.w - 12.0, r.h), Font::new(12.0, 400).tabular(), look::T3, Align::End);
            }
            // the thin scrollbar
            if self.max_scroll > 0.0 {
                let th = (view.h * view.h / content).max(30.0);
                let ty = view.y + 2.0 + (view.h - 4.0 - th) * (y / self.max_scroll);
                c.fill(Rect::new(view.x + view.w - 8.0, ty, 4.0, th), 2.0, Rgba::white(0.2));
            }
        });

        // the caption is drawn last: its hairline lies on the body
        let tools = caption(
            c,
            f,
            Rect::new(0.0, 0.0, w, 48.0),
            &Caption {
                bg: BAR,
                title: "WinRust UI demo",
                sub: &format!("{ROWS} rows"),
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

    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        match (id, b) {
            (TB_MORE, _) => cx.menu(M_TAG, self.menu(), Rect::new(x - 16.0, 44.0, 32.0, 0.0)),
            (B_MENU, _) => cx.menu(M_TAG, self.menu(), Rect::new(x - 40.0, y + 18.0, 80.0, 0.0)),
            (TB_INFO, _) => self.info = !self.info,
            (TB_DEL, _) | (B_DIALOG, _) => cx.dialog(D_TAG, Dialog::new("Move 3 files to the Recycle Bin?", "They stay there until you empty it, so this can be undone.", &["Cancel", "Move"]).icon(TRASH, true)),
            (B_TOAST, _) => cx.toast("Copied to the clipboard", None),
            (B_UNDO, _) => cx.toast("Moved Sunset.jpg to the Recycle Bin", Some(UNDO_TAG)),
            (TOGGLE, _) => self.on = !self.on,
            (i, _) if (SEG..SEG + 3).contains(&i) => self.seg = (i - SEG) as usize,
            (i, _) if (CHIP..CHIP + 3).contains(&i) => self.chip = (i - CHIP) as usize,
            (i, Button::Right) if i >= ROW => cx.menu(M_TAG, self.menu(), Rect::new(x, y, 0.0, 0.0)),
            (i, _) if i >= ROW => self.row = Some((i - ROW) as usize),
            _ => {}
        }
        cx.invalidate();
    }

    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        self.to = (self.to - dy * 3.0 * ROW_H).clamp(0.0, self.max_scroll);
        cx.invalidate();
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
        cx.invalidate();
    }

    fn key(&mut self, vk: u32, _m: Mods, cx: &mut Cx) -> bool {
        match vk {
            0x28 | 0x26 => {
                let r = self.row.unwrap_or(0) as isize + if vk == 0x28 { 1 } else { -1 };
                self.row = Some(r.clamp(0, ROWS as isize - 1) as usize);
                cx.invalidate();
                true
            }
            _ => false,
        }
    }
}

const STATES: [&str; 2] = ["start", "scrolled"];

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
