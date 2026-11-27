//! The field, the scroll area, the list and its header driven through the core without a window, as a user's mouse and keyboard
//! would drive them: a fake app with one of each, drawn for real, and a clock the test moves by hand. The clipboard is a variable.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

use super::field::{self, FieldStyle, field_with};
use super::field_model::{Ev, MemClip, TextField};
use super::list::{ColItem, col_of, columns, list, list_key, row_id, row_of};
use super::list_model::{Col, Columns, ListState};
use super::look::BODY;
use super::scroll::{self, bar_id};
use super::scroll_model::Scroll;
use crate::core::{App, Core};
use crate::frame::{Button, Cx, Frame, Id, Mods, Req};
use crate::gfx::{Align, Canvas, Gfx, Rect, Rgba, Surface};

const FIELD: Id = 1;
const SC: Id = 10;
const COLS: Id = 500;
const LIST: Id = 1000;
const N: usize = 10_000;
const ROW_H: f32 = 30.0;

struct St {
    tf: TextField,
    clip: MemClip,
    sc: Scroll,
    list: ListState,
    cols: Columns,
    n: usize,
}

struct Fake {
    st: Rc<RefCell<St>>,
    clock: Rc<Cell<u64>>,
}

impl App for Fake {
    fn title(&self) -> String {
        "fake".into()
    }
    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        c.fill(Rect::new(0.0, 0.0, f.size.0, f.size.1), 0.0, Rgba::hex(0x16171b));
        field_with(c, f, FIELD, Rect::new(20.0, 60.0, 240.0, 34.0), &mut st.tf, "", &FieldStyle::INPUT);
        scroll::scroll(c, f, SC, Rect::new(560.0, 60.0, 200.0, 300.0), &mut st.sc, 2000.0, |c, _f, v| c.fill(Rect::new(v.r.x, v.at(0.0), v.r.w, 2000.0), 0.0, Rgba::hex(0x223344)));
        columns(c, f, COLS, Rect::new(20.0, 110.0, 400.0, 34.0), &st.cols, 10.0);
        let n = st.n;
        list(c, f, LIST, Rect::new(20.0, 150.0, 400.0, 300.0), &mut st.list, n, ROW_H, |c, _f, i, r, _| c.text(&format!("row {i}"), r, BODY, Rgba::pure_white(1.0), Align::Start));
    }
    fn down(&mut self, id: Id, _b: Button, x: f32, y: f32, cx: &mut Cx) {
        let (mut st, now) = (self.st.borrow_mut(), self.clock.get());
        let n = st.n;
        match id {
            FIELD => st.tf.mouse_down(x, cx.mods.shift, now),
            i if i == bar_id(SC) => {
                scroll::bar_down(&mut st.sc, y, now);
            }
            i if i == bar_id(LIST) => {
                scroll::bar_down(&mut st.list.scroll, y, now);
            }
            i if i >= LIST + 2 => {
                if let Some(r) = row_of(LIST, i).filter(|&r| r < n) {
                    st.list.press(r, cx.mods.ctrl, cx.mods.shift);
                }
            }
            i => {
                if let Some(ColItem::Grip(k)) = col_of(COLS, i) {
                    st.cols.grip_down(k, x);
                }
            }
        }
    }
    fn click(&mut self, id: Id, _b: Button, _x: f32, _y: f32, _cx: &mut Cx) {
        let mut st = self.st.borrow_mut();
        let n = st.n;
        match id {
            i if i >= LIST + 2 => {
                if let Some(r) = row_of(LIST, i).filter(|&r| r < n) {
                    st.list.lift(r);
                }
            }
            i => {
                if let Some(ColItem::Title(k)) = col_of(COLS, i) {
                    st.cols.sort_by(k);
                }
            }
        }
    }
    fn drag(&mut self, id: Id, x: f32, y: f32, _cx: &mut Cx) {
        let (mut st, now) = (self.st.borrow_mut(), self.clock.get());
        match id {
            FIELD => st.tf.mouse_drag(x, now),
            i if i == bar_id(SC) => scroll::bar_drag(&mut st.sc, y, now),
            i if i == bar_id(LIST) => scroll::bar_drag(&mut st.list.scroll, y, now),
            _ => {
                st.cols.grip_drag(x);
            }
        }
    }
    fn release(&mut self, id: Id, _cx: &mut Cx) {
        let mut st = self.st.borrow_mut();
        match id {
            FIELD => st.tf.mouse_up(),
            i if i == bar_id(SC) => scroll::bar_up(&mut st.sc),
            i if i == bar_id(LIST) => scroll::bar_up(&mut st.list.scroll),
            _ => st.cols.grip_up(),
        }
    }
    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        let (mut st, now) = (self.st.borrow_mut(), self.clock.get());
        scroll::wheel(&mut st.sc, cx.pointer, dy, ROW_H, now);
        scroll::wheel(&mut st.list.scroll, cx.pointer, dy, ROW_H, now);
    }
    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        let (mut st, now) = (self.st.borrow_mut(), self.clock.get());
        let st = &mut *st;
        if cx.focus == Some(FIELD) {
            return st.tf.key(vk, m, cx.alt, now, &mut st.clip).used();
        }
        list_key(&mut st.list, vk, m.shift, m.ctrl, st.n, 9)
    }
    fn text(&mut self, s: &str, cx: &mut Cx) {
        let (mut st, now) = (self.st.borrow_mut(), self.clock.get());
        let st = &mut *st;
        if cx.focus == Some(FIELD) {
            field::text(&mut st.tf, s, now);
        } else {
            let n = st.n;
            for ch in s.chars() {
                st.list.typeahead(ch, now, n, |i| Some(format!("row {i}")));
            }
        }
    }
}

struct Rig {
    core: Core,
    gfx: Gfx,
    surf: Surface,
    st: Rc<RefCell<St>>,
    clock: Rc<Cell<u64>>,
    t: u64,
}

const W: i32 = 800;

impl Rig {
    fn new() -> Self {
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let col = |t: &str, w: f32, min: f32| Col { title: t.into(), w, min, align_right: false };
        let st = Rc::new(RefCell::new(St {
            tf: TextField::new(),
            clip: MemClip::default(),
            sc: Scroll::default(),
            list: ListState::default(),
            cols: Columns::new(vec![col("Name", 0.0, 100.0), col("Date", 100.0, 60.0), col("Type", 100.0, 60.0)]),
            n: N,
        }));
        let clock = Rc::new(Cell::new(100_000));
        let core = Core::new(Box::new(Fake { st: st.clone(), clock: clock.clone() }), (W as f32, 500.0), 1.0, 0);
        let mut r = Self { core, gfx: Gfx::new(1.0).expect("direct2d"), surf: Surface::new(), st, clock, t: 100_000 };
        r.frame();
        r
    }
    /// A frame `ms` later.
    fn after(&mut self, ms: u64) {
        self.t += ms;
        self.clock.set(self.t);
        let (core, t) = (&mut self.core, self.t);
        self.gfx.paint(&mut self.surf, W, 500, |c| core.draw(c, t)).expect("a frame");
    }
    fn frame(&mut self) {
        self.after(20);
    }
    fn centre(&self, id: Id) -> (f32, f32) {
        let r = self.core.hits().get(id).unwrap_or_else(|| panic!("item {id} was not named")).r;
        (r.cx(), r.cy())
    }
    fn press(&mut self, x: f32, y: f32) {
        self.core.pointer_move(x, y, self.t);
        self.core.down(Button::Left, x, y, self.t);
    }
    fn release(&mut self, x: f32, y: f32) {
        let _ = self.core.up(Button::Left, x, y, self.t);
        self.frame();
    }
    fn click_at(&mut self, x: f32, y: f32) {
        self.press(x, y);
        self.frame();
        self.release(x, y);
    }
    fn click(&mut self, id: Id) {
        let (x, y) = self.centre(id);
        self.click_at(x, y);
    }
    fn key(&mut self, vk: u32, ctrl: bool, shift: bool) -> bool {
        let used = self.core.key(vk, Mods { ctrl, shift }, false, self.t);
        self.frame();
        used
    }
    fn typed(&mut self, s: &str) {
        self.core.text(s, self.t);
        self.frame();
    }
    fn text(&self) -> String {
        self.st.borrow().tf.text.clone()
    }
    fn caret(&self) -> (usize, usize) {
        let st = self.st.borrow();
        (st.tf.anchor, st.tf.caret)
    }
    fn selected(&self) -> String {
        self.st.borrow().tf.selected_text().to_owned()
    }
    fn px(&self, x: i32, y: i32) -> u32 {
        self.surf.pixels()[(y * W + x) as usize]
    }
    fn width_of(&mut self, s: &str) -> f32 {
        self.gfx.measure(s, BODY)
    }
}

/// Where the text of the field starts: the box's x and its padding.
const TEXT_X: f32 = 30.0;
const FIELD_Y: f32 = 77.0;

#[test]
fn a_click_puts_the_caret_between_the_letters_a_double_click_takes_the_word_a_drag_the_stretch() {
    let mut r = Rig::new();
    r.click_at(100.0, FIELD_Y);
    assert_eq!(r.core.hits().focus, Some(FIELD), "a click on the field focuses it");
    r.typed("hello world");
    assert_eq!(r.text(), "hello world");
    assert_eq!(r.caret(), (11, 11), "typing leaves the caret at the end");
    let (he, hello, hello_, hello_wor) = (r.width_of("he"), r.width_of("hello"), r.width_of("hello "), r.width_of("hello wor"));
    r.click_at(250.0, FIELD_Y);
    assert_eq!(r.caret(), (11, 11), "right of the text: the end");
    r.click_at(22.0, FIELD_Y);
    assert_eq!(r.caret(), (0, 0), "left of it: the start");
    r.click_at(TEXT_X + hello_ + 1.0, FIELD_Y);
    assert_eq!(r.caret(), (6, 6), "between the space and the w");
    r.core.pointer_move(0.0, 0.0, r.t);
    r.after(1000); // a pause: the next press is the first of a click
    r.click_at(TEXT_X + hello_wor, FIELD_Y);
    r.click_at(TEXT_X + hello_wor, FIELD_Y);
    assert_eq!(r.selected(), "world", "two presses in a row: the word");
    r.click_at(TEXT_X + hello_wor, FIELD_Y);
    assert_eq!(r.selected(), "hello world", "three: the lot");
    r.after(1000);
    // a drag from between "he" and "llo" to the end of "hello"
    r.press(TEXT_X + he, FIELD_Y);
    r.frame();
    r.core.pointer_move(TEXT_X + hello, FIELD_Y + 3.0, r.t);
    r.frame();
    assert_eq!(r.selected(), "llo");
    // the pointer left the box: the selection follows it to the end
    r.core.pointer_move(700.0, 400.0, r.t);
    r.frame();
    assert_eq!(r.selected(), "llo world");
    r.release(700.0, 400.0);
    r.core.pointer_move(30.0, FIELD_Y, r.t);
    r.frame();
    assert_eq!(r.selected(), "llo world", "a released pointer selects nothing more");
    // Shift+click extends from where the caret was
    r.after(1000);
    r.click_at(TEXT_X + he, FIELD_Y);
    r.core.held = Mods { ctrl: false, shift: true };
    r.after(1000);
    r.click_at(TEXT_X + hello, FIELD_Y);
    assert_eq!(r.selected(), "llo", "Shift+click");
    r.core.held = Mods::default();
}

#[test]
fn the_keys_edit_and_the_clipboard_is_the_one_the_app_gave() {
    let mut r = Rig::new();
    r.click_at(100.0, FIELD_Y);
    r.typed("abc def");
    assert!(r.key(0x41, true, false), "Ctrl+A is the field's");
    assert_eq!(r.selected(), "abc def");
    r.key(0x43, true, false);
    assert_eq!(r.st.borrow().clip.0.as_deref(), Some("abc def"), "Ctrl+C");
    assert!(r.key(0x58, true, false));
    assert_eq!(r.text(), "", "Ctrl+X");
    r.key(0x56, true, false);
    assert_eq!(r.text(), "abc def", "Ctrl+V");
    r.key(0x5A, true, false);
    assert_eq!(r.text(), "", "Ctrl+Z");
    r.key(0x59, true, false);
    assert_eq!(r.text(), "abc def", "Ctrl+Y");
    r.key(0x25, true, false);
    r.key(0x25, true, true);
    assert_eq!(r.selected(), "abc ", "Ctrl+Left twice with Shift on the second");
    r.key(0x24, false, false);
    r.key(0x2E, false, false);
    assert_eq!(r.text(), "bc def", "Home, Delete");
    r.key(0x23, false, false);
    assert_eq!(r.caret(), (6, 6));
    assert!(r.key(0x20, false, false), "Space is used, so the window does not click the field with it");
    r.typed(" ");
    assert_eq!(r.text(), "bc def ");
    r.typed("привет😀");
    assert_eq!(r.text(), "bc def привет😀", "what the IME composed goes in whole");
    r.key(0x08, false, false);
    assert_eq!(r.text(), "bc def привет", "Backspace takes the emoji, both halves");
    assert!(!r.key(0x28, false, false), "Down is not the field's: the app may have it");
    // nothing of this touched the window's clipboard: the field asked its clipboard, which was a variable
    assert!(r.core.take_host().iter().all(|h| !matches!(h, crate::core::HostReq::Clipboard(_))));
}

#[test]
fn the_real_clipboard_path_only_queues_what_it_would_write() {
    let mut f = TextField::new();
    f.insert("secret? no");
    f.select_all();
    let mut cx = Cx::default();
    assert_eq!(field::key(&mut f, &mut cx, 0x43, Mods { ctrl: true, shift: false }, 1), Ev::Used);
    assert!(matches!(cx.reqs.as_slice(), [Req::Clipboard(t)] if t == "secret? no"), "Ctrl+C asks the host to put the selection on the clipboard");
}

#[test]
fn the_caret_blinks_on_one_timer_and_only_in_a_window_that_has_the_focus() {
    let mut r = Rig::new();
    assert_eq!(r.core.deadline(r.t), None, "no field focused: no timer");
    r.press(100.0, FIELD_Y);
    r.frame();
    let touched = r.t; // the first frame that sees the focus starts the blink
    r.release(100.0, FIELD_Y);
    assert_eq!(r.core.deadline(r.t), Some(touched + 530), "one frame is asked for, at the caret's next change");
    assert!(!r.core.wants_frames(r.t + 400), "and none in between: nothing moves");
    let caret = r.core.ime_caret().expect("the focused field says where its caret is");
    let (x, y) = (caret.x as i32, caret.cy() as i32);
    let bg = r.px(x + 40, y);
    assert_ne!(r.px(x, y), bg, "the caret is drawn");
    r.after(540);
    assert_eq!(r.px(x, y), bg, "half a second later it is off");
    assert_eq!(r.core.deadline(r.t), Some(touched + 1060));
    r.after(530);
    assert_ne!(r.px(x, y), bg, "and on again");
    // a key restarts the blink: the caret is on when it moves
    r.after(500);
    r.key(0x25, false, false);
    r.typed("a");
    assert_eq!(r.core.deadline(r.t), Some(r.t + 530 - 20), "from the last change");
    // a window that is not the active one shows no caret and wants no timer
    r.core.active = false;
    r.frame();
    assert_eq!(r.core.deadline(r.t), None);
    assert!(r.core.ime_caret().is_none());
}

#[test]
fn the_ime_is_told_where_the_caret_is_only_while_a_field_has_the_focus() {
    let mut r = Rig::new();
    assert!(r.core.ime_caret().is_none());
    r.click_at(100.0, FIELD_Y);
    let a = r.core.ime_caret().expect("focused");
    assert!((a.x - TEXT_X).abs() < 1.0 && a.y < FIELD_Y && a.y + a.h > FIELD_Y, "at the start of the text: {a:?}");
    r.typed("abc");
    let b = r.core.ime_caret().expect("focused");
    assert!(b.x > a.x + 10.0, "after the typed text: {b:?}");
    r.click(row_id(LIST, 2));
    assert!(r.core.ime_caret().is_none(), "a click on a row takes the focus from the field");
}

#[test]
fn the_wheel_glides_for_a_notch_and_a_touchpad_follows_at_once_and_the_bar_can_be_held() {
    let mut r = Rig::new();
    r.core.pointer_move(600.0, 200.0, r.t);
    r.core.wheel(0.0, -1.0, Mods::default(), r.t);
    assert_eq!(r.st.borrow().sc.target(), 3.0 * ROW_H, "a notch is three rows");
    assert_eq!(r.st.borrow().list.scroll.target(), 0.0, "the list is not under the pointer");
    r.frame();
    assert!(r.core.wants_frames(r.t), "the glide asks for frames while it lasts");
    r.after(300);
    assert!(!r.core.wants_frames(r.t), "and none once it is over");
    assert_eq!(r.st.borrow().sc.offset(r.t), 90.0);
    r.core.wheel(0.0, -0.25, Mods::default(), r.t);
    assert_eq!(r.st.borrow().sc.offset(r.t), 90.0 + 0.25 * 3.0 * ROW_H, "a fraction of a notch: no glide");
    r.frame();
    assert!(!r.core.wants_frames(r.t), "a touchpad's step is one frame");
    r.core.wheel(0.0, 1.0, Mods::default(), r.t);
    r.core.wheel(0.0, 1.0, Mods::default(), r.t);
    r.core.wheel(0.0, 1.0, Mods::default(), r.t);
    assert_eq!(r.st.borrow().sc.target(), 0.0, "up, and never past the top");
    // the bar: named only because there is something to scroll; the thumb is held and dragged
    r.after(400);
    let (strip, track) = (r.core.hits().get(bar_id(SC)).expect("a bar").r, r.st.borrow().sc.track);
    assert_eq!(strip.h, track.1);
    let (t0, th) = r.st.borrow().sc.thumb(track.1, r.t);
    let (x, y) = (strip.cx(), track.0 + t0 + th / 2.0);
    r.press(x, y);
    r.core.pointer_move(x, y + 60.0, r.t);
    let want = 60.0 / (track.1 - th) * (2000.0 - 300.0);
    assert!((r.st.borrow().sc.target() - want).abs() < 1.0, "{} against {want}", r.st.borrow().sc.target());
    r.core.pointer_move(x, y + 5000.0, r.t);
    assert_eq!(r.st.borrow().sc.target(), 1700.0, "dragged past the end of the track: the end");
    r.release(x, y + 5000.0);
    r.core.pointer_move(x, 100.0, r.t);
    assert_eq!(r.st.borrow().sc.target(), 1700.0, "a released bar does not follow");
    // a press beside the thumb is a page
    r.frame();
    let (strip, track) = (r.core.hits().get(bar_id(SC)).expect("a bar").r, r.st.borrow().sc.track);
    r.press(strip.cx(), track.0 + 3.0);
    assert_eq!(r.st.borrow().sc.target(), 1400.0, "a page up");
    r.release(strip.cx(), track.0 + 3.0);
}

#[test]
fn a_list_of_ten_thousand_names_only_the_rows_in_view_and_the_keys_keep_the_cursor_in_view() {
    let mut r = Rig::new();
    let named = r.core.hits().all().len();
    assert!(named < 60, "{named} items named for a list of {N} rows");
    assert!(r.core.hits().get(row_id(LIST, 0)).is_some() && r.core.hits().get(row_id(LIST, 9)).is_some());
    assert!(r.core.hits().get(row_id(LIST, 30)).is_none() && r.core.hits().get(row_id(LIST, N - 1)).is_none());
    // clicks: plain, Ctrl, Shift
    r.click(row_id(LIST, 2));
    assert_eq!(r.st.borrow().list.sel.iter().copied().collect::<Vec<_>>(), vec![2]);
    r.core.held = Mods { ctrl: true, shift: false };
    r.click(row_id(LIST, 4));
    assert_eq!(r.st.borrow().list.sel.iter().copied().collect::<Vec<_>>(), vec![2, 4], "Ctrl adds");
    r.core.held = Mods { ctrl: false, shift: true };
    r.click(row_id(LIST, 7));
    assert_eq!(r.st.borrow().list.sel.iter().copied().collect::<Vec<_>>(), vec![4, 5, 6, 7], "Shift takes the range from the anchor");
    r.core.held = Mods::default();
    // a press on a selected row of several waits for the button
    let (x, y) = r.centre(row_id(LIST, 5));
    r.press(x, y);
    assert_eq!(r.st.borrow().list.sel.len(), 4, "the press alone keeps the selection (it may be a drag of them)");
    r.release(x, y);
    assert_eq!(r.st.borrow().list.sel.iter().copied().collect::<Vec<_>>(), vec![5], "the click selects the one");
    // keys
    assert!(r.key(0x28, false, false));
    assert_eq!(r.st.borrow().list.cursor, Some(6));
    assert!(r.key(0x22, false, false));
    assert_eq!(r.st.borrow().list.cursor, Some(15), "a page is nine rows here");
    assert!(r.key(0x23, false, false));
    assert_eq!(r.st.borrow().list.cursor, Some(N - 1));
    r.after(300);
    assert!(r.core.hits().get(row_id(LIST, N - 1)).is_some(), "End scrolled the last row into view");
    assert!(r.core.hits().all().len() < 60, "and still only the rows in view are named");
    assert!(r.key(0x24, false, false));
    r.after(300);
    assert!(r.core.hits().get(row_id(LIST, 0)).is_some(), "Home scrolled back");
    assert!(r.key(0x41, true, false));
    assert_eq!(r.st.borrow().list.sel.len(), N, "Ctrl+A");
    assert!(!r.key(0x70, false, false), "F1 is not the list's");
    // typing a name jumps to it
    r.click(row_id(LIST, 0));
    for ch in ["r", "o", "w", " ", "7", "7"] {
        r.core.text(ch, r.t);
    }
    r.frame();
    assert_eq!(r.st.borrow().list.cursor, Some(77));
    r.after(300);
    assert!(r.core.hits().get(row_id(LIST, 77)).is_some(), "and shows it");
}

#[test]
fn the_header_sorts_and_the_grips_move_the_edges() {
    let mut r = Rig::new();
    r.click(COLS + 1);
    assert_eq!(r.st.borrow().cols.sort, Some((1, true)));
    r.click(COLS + 1);
    assert_eq!(r.st.borrow().cols.sort, Some((1, false)), "a second click turns the order over");
    r.click(COLS);
    assert_eq!(r.st.borrow().cols.sort, Some((0, true)));
    // the grip on the right edge of Name: Date gives way
    let before = r.st.borrow().cols.cols[1].w;
    let (x, y) = r.centre(COLS + 64);
    r.press(x, y);
    r.core.pointer_move(x + 30.0, y, r.t);
    assert_eq!(r.st.borrow().cols.cols[1].w, before - 30.0, "{:?}", r.st.borrow().cols.cols);
    r.core.pointer_move(x - 20.0, y, r.t);
    assert_eq!(r.st.borrow().cols.cols[1].w, before + 20.0, "and back, past where it began");
    r.release(x - 20.0, y);
    r.core.pointer_move(x + 100.0, y, r.t);
    assert_eq!(r.st.borrow().cols.cols[1].w, before + 20.0, "a released grip stays");
    // a click on the grip is not a sort
    assert_eq!(r.st.borrow().cols.sort, Some((0, true)));
}

#[test]
fn a_list_scrolled_through_keeps_no_trace_of_the_rows_it_passed() {
    let mut r = Rig::new();
    let mut most = 0;
    for i in 0..400 {
        {
            let mut st = r.st.borrow_mut();
            let y = (i * 733 % 9000) as f32 * ROW_H / 4.0;
            st.list.scroll.scroll_to(y, r.t);
            st.list.scroll.settle();
        }
        r.core.pointer_move(200.0, 160.0 + (i % 9) as f32 * 31.0, r.t);
        r.frame();
        most = most.max(r.core.mo.tracked());
    }
    assert!(most < 700, "{most} transitions were kept while 400 screens of rows went by");
    assert!(r.core.hits().all().len() < 60);
}
