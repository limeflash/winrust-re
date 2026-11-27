//! The core driven without a window: a caption, two buttons and a menu opener, drawn for real and clicked by hand.

use super::*;
use crate::frame::{WIN_CLOSE, WIN_MAX, WIN_MIN};
use crate::gfx::{Align, Font, Gfx, Surface};
use crate::widgets::button::{self, Kind};
use crate::widgets::caption::{Caption, caption};
use crate::widgets::overlay::{MenuItem, menu_row_id};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

#[derive(Default)]
struct Log {
    clicks: Vec<(Id, Button)>,
    drags: Vec<Id>,
    commands: Vec<(u32, usize)>,
    doubles: u32,
}

struct Fake(Rc<RefCell<Log>>);

impl App for Fake {
    fn title(&self) -> String {
        "fake".into()
    }
    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        c.fill(Rect::new(0.0, 0.0, f.size.0, f.size.1), 0.0, Rgba::hex(0x16171b));
        button::button(c, f, 10, Rect::new(20.0, 80.0, 80.0, 34.0), "Save", Kind::Accent);
        button::button(c, f, 11, Rect::new(110.0, 80.0, 80.0, 34.0), "Open", Kind::Quiet);
        c.text("under", Rect::new(20.0, 200.0, 100.0, 20.0), Font::new(13.0, 400), Rgba::pure_white(1.0), Align::Start);
        let cap = Caption { bg: Rgba::hex(0x1c1d22), title: "fake", sub: "", glyph: "M6 12h12", tile: [Rgba::hex(0xffb46b); 3], max: f.maximized(), tools_w: 0.0 };
        caption(c, f, Rect::new(0.0, 0.0, f.size.0, 48.0), &cap);
    }
    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        self.0.borrow_mut().clicks.push((id, b));
        if id == 11 {
            cx.menu(7, vec![MenuItem::new("one"), MenuItem::new("two")], Rect::new(x, y, 0.0, 0.0));
        }
    }
    fn double(&mut self, _id: Id, _cx: &mut Cx) {
        self.0.borrow_mut().doubles += 1;
    }
    fn drag(&mut self, id: Id, _x: f32, _y: f32, _cx: &mut Cx) {
        self.0.borrow_mut().drags.push(id);
    }
    fn command(&mut self, tag: u32, choice: usize, _cx: &mut Cx) {
        self.0.borrow_mut().commands.push((tag, choice));
    }
}

struct Rig {
    core: Core,
    gfx: Gfx,
    surf: Surface,
    log: Rc<RefCell<Log>>,
    t: u64,
}

impl Rig {
    fn new() -> Self {
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let log = Rc::new(RefCell::new(Log::default()));
        let mut r = Self { core: Core::new(Box::new(Fake(log.clone())), (400.0, 300.0), 1.0, 0), gfx: Gfx::new(1.0).expect("direct2d"), surf: Surface::new(), log, t: 1000 };
        r.frame();
        r
    }
    fn frame(&mut self) {
        self.t += 50;
        let (core, t) = (&mut self.core, self.t);
        self.gfx.paint(&mut self.surf, 400, 300, |c| core.draw(c, t)).expect("a frame");
    }
    fn centre(&self, id: Id) -> (f32, f32) {
        let r = self.core.hits().get(id).expect("the item was named").r;
        (r.cx(), r.cy())
    }
    fn click(&mut self, id: Id) -> Option<Act> {
        let (x, y) = self.centre(id);
        self.core.pointer_move(x, y, self.t);
        self.core.down(Button::Left, x, y, self.t);
        let act = self.core.up(Button::Left, x, y, self.t);
        self.frame();
        act
    }
}

#[test]
fn windows_questions_about_a_point_are_answered_from_the_frame_just_drawn() {
    let mut r = Rig::new();
    let (mx, my) = r.centre(WIN_MAX);
    let (cx, cy) = r.centre(WIN_CLOSE);
    assert_eq!(r.core.nc_test(mx, my, false), Nc::MaxButton, "Snap Layouts hangs from this");
    assert_eq!(r.core.nc_test(cx, cy, false), Nc::Client, "close is clicked with ordinary messages");
    assert_eq!(r.core.nc_test(200.0, 24.0, false), Nc::Caption, "nothing named there: it drags the window");
    assert_eq!(r.core.nc_test(200.0, 2.0, false), Nc::Top);
    assert_eq!(r.core.nc_test(2.0, 2.0, false), Nc::TopLeft);
    assert_eq!(r.core.nc_test(398.0, 2.0, false), Nc::TopRight);
    assert_eq!(r.core.nc_test(200.0, 250.0, false), Nc::Client);
    // without the system's invisible borders (the fallback) the strips are ours on all four sides
    assert_eq!(r.core.nc_test(2.0, 150.0, true), Nc::Left);
    assert_eq!(r.core.nc_test(200.0, 298.0, true), Nc::Bottom);
    assert_eq!(r.core.nc_test(398.0, 298.0, true), Nc::BottomRight);
    assert_eq!(r.core.nc_test(2.0, 150.0, false), Nc::Client, "with them it is the system's to answer");
    // maximized: no resize strip, the caption starts at the edge
    r.core.maximized = true;
    r.frame();
    assert_eq!(r.core.nc_test(200.0, 2.0, true), Nc::Caption);
    // a menu's scrim covers the caption: nothing there is Windows' any more
    r.core.maximized = false;
    r.click(11);
    assert!(r.core.ov.modal());
    assert_eq!(r.core.nc_test(mx, my, false), Nc::Client);
    assert_eq!(r.core.nc_test(200.0, 24.0, false), Nc::Client);
}

#[test]
fn a_click_needs_the_button_to_go_down_and_up_on_the_same_item() {
    let mut r = Rig::new();
    assert_eq!(r.click(10), None);
    assert_eq!(r.log.borrow().clicks, vec![(10, Button::Left)]);
    // down on one, up on another: no click, but the first hears the drag
    let (x1, y1) = r.centre(10);
    let (x2, y2) = r.centre(11);
    r.core.pointer_move(x1, y1, r.t);
    r.core.down(Button::Left, x1, y1, r.t);
    r.core.pointer_move(x2, y2, r.t + 1);
    assert_eq!(r.core.up(Button::Left, x2, y2, r.t + 1), None);
    assert_eq!(r.log.borrow().clicks.len(), 1);
    assert_eq!(r.log.borrow().drags, vec![10]);
}

#[test]
fn the_window_buttons_ask_for_their_actions() {
    let mut r = Rig::new();
    assert_eq!(r.click(WIN_CLOSE), Some(Act::Close));
    assert_eq!(r.click(WIN_MAX), Some(Act::ToggleMaximize));
    assert_eq!(r.click(WIN_MIN), Some(Act::Minimize));
    assert!(r.log.borrow().clicks.is_empty(), "the app is not told of the window's own buttons");
}

#[test]
fn a_menu_opened_by_a_click_is_chosen_with_the_keys_or_the_pointer_and_comes_back_as_a_command() {
    let mut r = Rig::new();
    r.click(11);
    assert!(r.core.ov.modal());
    let none = Mods::default();
    assert!(r.core.key(0x28, none, false, r.t), "down");
    assert!(r.core.key(0x28, none, false, r.t), "down");
    assert!(r.core.key(0x0D, none, false, r.t), "enter");
    assert_eq!(r.log.borrow().commands, vec![(7, 1)], "the second row's value");
    assert!(!r.core.ov.modal());
    r.frame();
    r.click(11);
    r.frame();
    r.click(menu_row_id(0, 0));
    assert_eq!(r.log.borrow().commands.last(), Some(&(7, 0)));
}

#[test]
fn tab_walks_the_focusable_items_and_enter_presses_the_one_with_the_ring() {
    let mut r = Rig::new();
    let none = Mods::default();
    assert!(r.core.key(0x09, none, false, r.t));
    r.frame();
    assert_eq!((r.core.hits().focus, r.core.hits().focus_visible), (Some(10), true));
    assert!(r.core.key(0x09, none, false, r.t));
    assert_eq!(r.core.hits().focus, Some(11));
    assert!(r.core.key(0x09, Mods { shift: true, ..none }, false, r.t));
    assert_eq!(r.core.hits().focus, Some(10), "Shift+Tab goes back");
    assert!(r.core.key(0x0D, none, false, r.t));
    assert_eq!(r.log.borrow().clicks, vec![(10, Button::Left)]);
    r.click(11);
    r.core.up(Button::Left, 0.0, 0.0, r.t);
    assert!(!r.core.hits().focus_visible, "a click with the mouse hides the ring again");
}

#[test]
fn a_second_click_in_the_same_place_is_a_double_click() {
    let mut r = Rig::new();
    r.click(10);
    r.t += 100;
    r.click(10);
    assert_eq!(r.log.borrow().doubles, 1);
}

#[test]
fn frames_are_wanted_only_while_something_moves() {
    let mut r = Rig::new();
    r.frame();
    assert!(!r.core.wants_frames(r.t + 5000), "nothing moves, no frames: the window is quiet at rest");
    let (x, y) = r.centre(10);
    r.core.pointer_move(x, y, r.t);
    r.frame();
    assert!(r.core.wants_frames(r.t + 10), "a hover is easing in");
    assert!(!r.core.wants_frames(r.t + 5000), "and has arrived");
}
