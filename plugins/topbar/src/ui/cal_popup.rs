//! The calendar panel (the clock on the bar): the big clock and the date, a month card (title, "Today", arrows, a week from
//! Monday, six rows of days with today in the accent circle and the picked day in a ring) and a card for the picked day's
//! events. Dense glass, radius 22, `qsIn` from the bottom right corner, 380 px wide, against the right edge of the screen.
//! Above it, 10 px apart on a glass of its own, stands the notification centre ([`super::notice::Centre`]); the two are one
//! window (the column of the mockup) whose box is both cards.
//!
//! The month changes at once, like in the mockup (it animates nothing but the hover and the fill of the day circles). The
//! names of days and months and the clock come from Windows' own locale; the panel's own words from the interface language.
//! Windows has no calendar events for a program to read, so the events card always says there are none.

use super::anim::EASE;
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::notice::Centre;
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Frame, Id, Look, Mods, Origin, Place, Shadow};
use winrust_common::style::{Side, pal};

const W: f32 = 380.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const COL: f32 = 48.0;
const CELL: f32 = 34.0;
const CLOCK_H: f32 = 34.0;
const HEAD_H: f32 = 8.0 + CLOCK_H + 2.0 + 17.0 + 6.0;
const CARD_Y: f32 = PAD + HEAD_H + GAP;
const CARD_H: f32 = 10.0 + 32.0 + 6.0 + 26.0 + 2.0 + 6.0 * CELL + 5.0 * 2.0 + 12.0;
const EV_Y: f32 = CARD_Y + CARD_H + GAP;
const EV_H: f32 = 58.0;
const H: f32 = EV_Y + EV_H + PAD;

/// Ids from here on belong to the notification centre.
const CENTRE: i32 = 1000;
/// The gap between the two cards of the column.
const COLUMN_GAP: f32 = 10.0;
const TODAY: i32 = 50;
const PREV: i32 = 51;
const NEXT: i32 = 52;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const DIM: Rgba = Rgba::css(163, 169, 184, 1.0);
const WEEKEND: Rgba = Rgba::css(138, 144, 162, 1.0);
const OTHER_MONTH: Rgba = Rgba::css(111, 116, 134, 1.0);
/// Text on an accent fill (the design's `onA`): ink on the dark style, white on the light one.
const ON_ACCENT: Rgba = Rgba::pair(0x0b1220, 0xffffff);

const CHEVRON_LEFT: &str = "M15 6l-6 6 6 6";
const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";

/// A calendar day.
pub type Date = (i32, i32, i32);

/// What the clock says now: today's date and the time as Windows writes it.
pub struct Now {
    pub date: Date,
    pub time: String,
    /// The clock of the notifications (`GetTickCount64`), ms.
    pub tick: u64,
}

/// Days in month `m` (1..=12) of year `y`.
fn days_in(y: i32, m: i32) -> i32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Day of the week, Monday = 0.
fn weekday((y, m, d): Date) -> i32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7 + 6) % 7
}

/// The month `n` months from `(y, m)`.
fn shift(y: i32, m: i32, n: i32) -> (i32, i32) {
    let i = y * 12 + (m - 1) + n;
    (i.div_euclid(12), i.rem_euclid(12) + 1)
}

/// The day in cell `i` (0..42) of the month `(y, m)`'s grid: a week from Monday, the neighbours' days around the month.
fn cell((y, m): (i32, i32), i: i32) -> Date {
    let off = i - weekday((y, m, 1));
    let dim = days_in(y, m);
    if off < 0 {
        let (py, pm) = shift(y, m, -1);
        (py, pm, days_in(py, pm) + off + 1)
    } else if off >= dim {
        let (ny, nm) = shift(y, m, 1);
        (ny, nm, off - dim + 1)
    } else {
        (y, m, off + 1)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Windows' locale

mod locale {
    use super::Date;
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::Globalization::{GetDateFormatEx, GetLocaleInfoEx, GetTimeFormatEx};
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    const DATE_YEARMONTH: u32 = 8;
    const TIME_NOSECONDS: u32 = 2;
    const LOCALE_SMONTHDAY: u32 = 0x78;

    fn text(f: impl FnOnce(*mut u16, i32) -> i32) -> String {
        let mut b = [0u16; 96];
        let n = f(b.as_mut_ptr(), b.len() as i32);
        String::from_utf16_lossy(&b[..(n.max(1) as usize - 1).min(b.len())])
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn st((y, m, d): Date) -> SYSTEMTIME {
        SYSTEMTIME { wYear: y as u16, wMonth: m as u16, wDayOfWeek: 0, wDay: d as u16, wHour: 0, wMinute: 0, wSecond: 0, wMilliseconds: 0 }
    }

    /// The date written by `pattern` in the user's locale (`None`: the format `flags` asks for).
    fn date(d: Date, flags: u32, pattern: Option<&str>) -> String {
        let (t, p) = (st(d), pattern.map(wide));
        // SAFETY: a null locale name is the user's default; `t` and `p` outlive the call and the buffer is the one given.
        text(|b, n| unsafe { GetDateFormatEx(std::ptr::null(), flags, &t, p.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()), b, n, std::ptr::null()) })
    }

    fn capital(s: String) -> String {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
    }

    /// "November 2026".
    pub fn month_title((y, m): (i32, i32)) -> String {
        capital(date((y, m, 1), DATE_YEARMONTH, None))
    }

    /// "Thursday, November 26": the weekday and the locale's month and day ("26 ноября", "11月26日 木曜日").
    pub fn day_title(d: Date) -> String {
        // SAFETY: a null locale name is the user's default; the buffer is the one given.
        let md = text(|b, n| unsafe { GetLocaleInfoEx(std::ptr::null(), LOCALE_SMONTHDAY, b, n) });
        let md = if md.is_empty() { "MMMM d".to_string() } else { md };
        let pattern = if md.contains('月') { format!("{md} dddd") } else { format!("dddd, {md}") };
        capital(date(d, 0, Some(&pattern)))
    }

    /// Two letters of a weekday's short name, Monday first: "Mo", "Пн", "月".
    pub fn weekdays() -> [String; 7] {
        // 2024-01-01 was a Monday
        std::array::from_fn(|i| capital(date((2024, 1, 1 + i as i32), 0, Some("ddd"))).chars().take(2).collect())
    }

    /// The time as the bar writes it (the user's format, no seconds).
    pub fn clock(h: u16, m: u16) -> String {
        let t = SYSTEMTIME { wYear: 2000, wMonth: 1, wDayOfWeek: 0, wDay: 1, wHour: h, wMinute: m, wSecond: 0, wMilliseconds: 0 };
        // SAFETY: a null locale name is the user's default; the buffer is the one given.
        text(|b, n| unsafe { GetTimeFormatEx(std::ptr::null(), TIME_NOSECONDS, &t, std::ptr::null(), b, n) })
    }

    /// Today's date and the time now.
    pub fn now() -> super::Now {
        let mut t = SYSTEMTIME { wYear: 0, wMonth: 0, wDayOfWeek: 0, wDay: 0, wHour: 0, wMinute: 0, wSecond: 0, wMilliseconds: 0 };
        // SAFETY: fills a local.
        unsafe { GetLocalTime(&mut t) };
        // SAFETY: a plain clock read.
        let tick = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        super::Now { date: (i32::from(t.wYear), i32::from(t.wMonth), i32::from(t.wDay)), time: clock(t.wHour, t.wMinute), tick }
    }
}

/// The clock and the date from the machine.
#[must_use]
pub fn system_now() -> Now {
    locale::now()
}

// ---------------------------------------------------------------------------------------------------------------------

pub struct CalPanel {
    now: fn() -> Now,
    side: Side,
    /// The month on show.
    view: (i32, i32),
    /// The picked day.
    sel: Date,
    weekdays: [String; 7],
    /// Titles are asked from Windows once per month and day, not once per frame.
    title: Option<((i32, i32), String)>,
    day: Option<(Date, String)>,
    /// How wide the "Today" button is in the current language.
    today_w: f32,
    centre: Centre,
    /// Height of the centre's card, and where the calendar's card begins.
    centre_h: f32,
    /// The room the whole column has.
    room: f32,
    /// The time of the last frame, for what a click needs to know.
    t: u64,
}

impl CalPanel {
    /// `room` is the height the whole column may take: the notification centre gives way to the calendar.
    #[must_use]
    pub fn new(now: fn() -> Now, side: Side, room: f32) -> Self {
        let today = now().date;
        let centre_room = (room - COLUMN_GAP - H).max(0.0);
        let mut me = Self {
            now,
            side,
            view: (today.0, today.1),
            sel: today,
            weekdays: locale::weekdays(),
            title: None,
            day: None,
            today_w: 56.0,
            centre: Centre::new(centre_room),
            centre_h: 0.0,
            room,
            t: 0,
        };
        me.t = (me.now)().tick;
        me
    }

    /// The month on show and the picked day (for tests and the renderer).
    #[must_use]
    pub fn pick(mut self, view: (i32, i32), sel: Date) -> Self {
        self.view = view;
        self.sel = sel;
        self
    }

    fn day_rect(i: i32) -> Rect {
        let (col, row) = ((i % 7) as f32, (i / 7) as f32);
        Rect::new(PAD + 10.0 + col * COL + (COL - CELL) / 2.0, CARD_Y + 10.0 + 32.0 + 6.0 + 26.0 + 2.0 + row * (CELL + 2.0), CELL, CELL)
    }

    /// The buttons of the month card's header, from the right: next, previous (32 each, 2 apart), "Today".
    fn button(&self, id: i32) -> Rect {
        let right = PAD + 10.0 + 336.0 - 2.0;
        let y = CARD_Y + 10.0;
        match id {
            NEXT => Rect::new(right - 32.0, y, 32.0, 32.0),
            PREV => Rect::new(right - 66.0, y, 32.0, 32.0),
            _ => Rect::new(right - 68.0 - self.today_w, y, self.today_w, 32.0),
        }
    }
}

impl Content for CalPanel {
    fn id(&self) -> Id {
        Id::Cal
    }

    fn look(&self) -> Look {
        Look::dense(22.0, Shadow { dy: 30.0, blur: 80.0, color: Rgba::css(4, 6, 20, 0.65) })
    }

    fn entry(&self) -> Entry {
        Entry { origin: if self.side == Side::Bottom { Origin::Frac(1.0, 1.0) } else { Origin::Button }, ..Entry::pop() }
    }

    fn place(&self) -> Place {
        Place { along: Along::ScreenEnd, ..Place::default() }
    }

    fn size(&mut self, g: &mut Gfx) -> (f32, f32) {
        let now = (self.now)().tick;
        self.lay_out(g, now)
    }

    fn max_size(&mut self, _g: &mut Gfx) -> Option<(f32, f32)> {
        Some((W, self.room))
    }

    /// The column's height follows the notification centre's.
    fn resized(&mut self, g: &mut Gfx, now: u64) -> Option<(f32, f32)> {
        self.t = now;
        let before = self.centre_h;
        let size = self.lay_out(g, now);
        ((self.centre_h - before).abs() > 0.01).then_some(size)
    }

    fn cards(&self) -> Vec<Rect> {
        vec![Rect::new(0.0, 0.0, W, self.centre_h), Rect::new(0.0, self.centre_h + COLUMN_GAP, W, H)]
    }

    fn pulse(&self) -> u32 {
        if self.centre.typing() { 500 } else { 1000 }
    }

    fn animating(&self, now: u64) -> bool {
        self.centre.animating(now)
    }

    fn typing(&self) -> bool {
        self.centre.typing()
    }

    fn text(&mut self, ch: char, _cx: &mut Cx) {
        self.centre.text(ch);
    }

    fn escape(&mut self) -> bool {
        self.centre.escape()
    }

    fn enter(&mut self, hover: Option<i32>, _mods: Mods, cx: &mut Cx) {
        if self.centre.typing() {
            self.centre.enter(self.t, cx);
        } else if let Some(id) = hover {
            self.click(id, Button::Left, cx);
        }
    }

    fn wheel(&mut self, _x: f32, y: f32, up: bool) {
        if y < self.centre_h {
            self.centre.wheel(up);
        }
    }

    fn tip(&self, id: i32) -> Option<(String, Rect)> {
        self.centre.tip(id - CENTRE)
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let now = f.now;
        let hover = f.hover.filter(|&h| h >= CENTRE).map(|h| h - CENTRE);
        self.centre.draw(c, f, hover, now);
        let top = self.centre_h + COLUMN_GAP;
        c.transformed(0.0, 0.0, 1.0, 0.0, top, |c| self.draw_calendar(c, f));
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        if y < self.centre_h {
            return self.centre.hit(x, y).map(|i| i + CENTRE);
        }
        self.hit_calendar(x, y - self.centre_h - COLUMN_GAP)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if id >= CENTRE {
            self.centre.click(id - CENTRE, b, self.t, cx);
        } else {
            self.click_calendar(id, b);
        }
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        self.nav_calendar(from, dir)
    }
}

impl CalPanel {
    /// Lays the centre out and returns the column's box.
    fn lay_out(&mut self, g: &mut Gfx, now: u64) -> (f32, f32) {
        self.centre_h = self.centre.relayout(g, now);
        (W, self.centre_h + COLUMN_GAP + H)
    }

    fn draw_calendar(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let now = (self.now)();
        let today = now.date;

        // the clock and the date
        let head = Rect::new(PAD + 8.0, PAD + 8.0, W - 2.0 * PAD - 16.0, CLOCK_H);
        c.text(&now.time, head, Font::new(34.0, 600).tracking(-0.03).tabular(), INK, Align::Start);
        let dt = match &self.day {
            Some((d, s)) if *d == today => s.clone(),
            _ => {
                let s = locale::day_title(today);
                self.day = Some((today, s.clone()));
                s
            }
        };
        c.text(&dt, Rect::new(PAD + 8.0, PAD + 8.0 + CLOCK_H + 2.0, W - 2.0 * PAD - 16.0, 17.0), Font::new(14.0, 400), DIM, Align::Start);

        // the month card
        let card = Rect::new(PAD, CARD_Y, W - 2.0 * PAD, CARD_H);
        c.fill(card, 16.0, Rgba::white(0.07));
        let title = match &self.title {
            Some((v, s)) if *v == self.view => s.clone(),
            _ => {
                let s = locale::month_title(self.view);
                self.title = Some((self.view, s.clone()));
                s
            }
        };
        c.text(&title, Rect::new(PAD + 10.0 + 8.0, CARD_Y + 10.0, 200.0, 32.0), Font::new(14.0, 600), INK, Align::Start);
        let label = t!("Сегодня", "Today", ja: "今日", pt: "Hoje", zh: "今天");
        self.today_w = c.measure(label, Font::new(12.0, 500)).ceil() + 20.0;
        for id in [TODAY, PREV, NEXT] {
            let r = self.button(id);
            let hot = f.hover(id, 250, EASE);
            c.fill(r, 8.0, Rgba::white(0.1 * hot));
            match id {
                TODAY => c.text(label, r, Font::new(12.0, 500), DIM, Align::Center),
                PREV => c.icon(CHEVRON_LEFT, (r.cx(), r.cy()), 14.0, 2.4, INK, 0.0),
                _ => c.icon(CHEVRON_RIGHT, (r.cx(), r.cy()), 14.0, 2.4, INK, 0.0),
            }
        }

        // the week and the days
        for (i, name) in self.weekdays.iter().enumerate() {
            let col = if i > 4 { WEEKEND } else { DIM };
            let r = Rect::new(PAD + 10.0 + i as f32 * COL, CARD_Y + 10.0 + 32.0 + 6.0, COL, 26.0);
            c.text(name, r, Font::new(12.0, 600), col, Align::Center);
        }
        for i in 0..42 {
            let d = cell(self.view, i);
            let r = Self::day_rect(i);
            let (is_today, in_month) = (d == today, (d.0, d.1) == self.view);
            // `.day{transition: background .2s}`: the hover and a circle that appears or goes
            let hot = f.hover(i, 200, EASE);
            let lit = f.tween(i, 1, f32::from(is_today), 200, EASE);
            let ink = if is_today {
                ON_ACCENT
            } else if in_month {
                if i % 7 > 4 { DIM } else { INK }
            } else {
                OTHER_MONTH
            };
            c.circle(r.cx(), r.cy(), CELL / 2.0, accent.accent_fill().fade(lit));
            // a hovered today stays itself (the mockup paints it grey under dark ink), brighter by a tenth
            c.circle(r.cx(), r.cy(), CELL / 2.0, Rgba::white(if is_today { 0.1 } else { 0.12 } * hot));
            if d == self.sel && !is_today {
                c.ring(r, CELL / 2.0, 2.0, accent);
            }
            c.text(&d.2.to_string(), r, Font::new(13.0, if is_today { 600 } else { 500 }).tabular(), ink, Align::Center);
        }

        // the picked day's events: none to read
        let ev = Rect::new(PAD, EV_Y, W - 2.0 * PAD, EV_H);
        c.fill(ev, 16.0, Rgba::white(0.07));
        let name = if self.sel == today { t!("Сегодня", "Today", ja: "今日", pt: "Hoje", zh: "今天").to_string() } else { locale::day_title(self.sel) };
        c.text(&name, Rect::new(ev.x + 16.0, ev.cy() - 16.0, ev.w - 32.0, 16.0), Font::new(13.0, 600), INK, Align::Start);
        c.text(
            t!("Нет событий", "No events", ja: "予定はありません", pt: "Sem eventos", zh: "没有日程"),
            Rect::new(ev.x + 16.0, ev.cy() + 1.0, ev.w - 32.0, 15.0),
            Font::new(12.0, 400),
            DIM,
            Align::Start,
        );
    }

    fn hit_calendar(&self, x: f32, y: f32) -> Option<i32> {
        if let Some(id) = [TODAY, PREV, NEXT].into_iter().find(|&id| self.button(id).contains(x, y)) {
            return Some(id);
        }
        (0..42).find(|&i| Self::day_rect(i).contains(x, y)).filter(|&i| {
            let r = Self::day_rect(i);
            (x - r.cx()).hypot(y - r.cy()) <= CELL / 2.0
        })
    }

    fn click_calendar(&mut self, id: i32, b: Button) {
        if b != Button::Left {
            return;
        }
        match id {
            TODAY => {
                let t = (self.now)().date;
                self.view = (t.0, t.1);
                self.sel = t;
            }
            PREV | NEXT => self.view = shift(self.view.0, self.view.1, if id == NEXT { 1 } else { -1 }),
            i => self.sel = cell(self.view, i),
        }
    }

    fn nav_calendar(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let start = (0..42).find(|&i| cell(self.view, i) == self.sel).unwrap_or(0);
        let i = match (from.filter(|&f| f < 42), dir) {
            (_, Dir::First) => 0,
            (_, Dir::Last) => 41,
            (None, _) => start,
            (Some(i), Dir::Left) => i - 1,
            (Some(i), Dir::Right) => i + 1,
            (Some(i), Dir::Up) => i - 7,
            (Some(i), Dir::Down) => i + 7,
        };
        Some(i.clamp(0, 41))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mockups_november() {
        assert_eq!(weekday((2026, 11, 26)), 3); // a Thursday
        assert_eq!(weekday((2026, 11, 1)), 6); // the month starts on a Sunday: six days of October first
        assert_eq!(cell((2026, 11), 0), (2026, 10, 26));
        assert_eq!(cell((2026, 11), 6), (2026, 11, 1));
        assert_eq!(cell((2026, 11), 35), (2026, 11, 30));
        assert_eq!(cell((2026, 11), 36), (2026, 12, 1));
        assert_eq!(cell((2026, 11), 41), (2026, 12, 6));
    }

    #[test]
    fn the_column_is_the_centre_over_the_calendar_and_each_has_its_own_card() {
        use super::super::panel::{Anchor, Popup};
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        fn now() -> Now {
            Now { date: (2026, 11, 26), time: "03:57".into(), tick: 1_000_000 }
        }
        crate::notify::with(|s| *s = crate::notify::Store::new());
        let mut g = Gfx::new(1.0).unwrap();
        let anchor = Anchor { side: Side::Bottom, button: (1330, 850, 1430, 894), bar: (0, 846, 1440, 900), screen: (1440, 900), origin: (0, 0) };
        let mut grab = |_x, _y, w: i32, h: i32| Some(vec![0xff10_1010u32; (w * h) as usize]);
        let mut p = Popup::new(&mut g, Box::new(CalPanel::new(now, Side::Bottom, 822.0)), anchor, 0, &mut grab);
        let (x, y, w, h) = p.rect();
        // an empty centre is 52 + 128 high, 10 above the calendar's 467; the column's right edge is 12 from the screen's, its
        // bottom 12 above the bar
        assert_eq!((w, h), (380, 52 + 128 + 10 + 467));
        assert_eq!((x + w, y + h), (1440 - 12, 846 - 12));
        let shown = p.frame(&mut g, 2000);
        assert!(shown.alpha > 0 && !p.surfaces().1.pixels().is_empty());
        // the gap between the two cards lets the screen through: no pixel of the glass window is lit there
        let (wx, wy, ww, _) = shown.win;
        let at = |gx: i32, gy: i32| p.surfaces().0.pixels()[((gy - wy) * ww + (gx - wx)) as usize];
        assert_eq!(at(x + 190, y + 52 + 128 + 5) >> 24, 0, "the gap is clear");
        assert!(at(x + 190, y + 52 + 128 + 10 + 30) >> 24 > 200, "the calendar's card is there");
        assert!(at(x + 190, y + 20) >> 24 > 200, "the centre's card is there");
    }

    #[test]
    fn months_and_leap_years() {
        assert_eq!(shift(2026, 12, 1), (2027, 1));
        assert_eq!(shift(2026, 1, -1), (2025, 12));
        assert_eq!(shift(2026, 3, -15), (2024, 12));
        assert_eq!((days_in(2024, 2), days_in(2026, 2), days_in(1900, 2), days_in(2000, 2)), (29, 28, 28, 29));
        assert_eq!(cell((2024, 3), 0), (2024, 2, 26)); // March 2024 starts on a Friday
    }
}
