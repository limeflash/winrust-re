//! How the Settings window looks: Settings.html of the mockups, line by line. The model is main.rs (what the pages hold), the state and
//! what a click does is app.rs; this file lays a page out (`Lay`: pure, from the model and what is open) and draws it, naming every
//! item with `Frame::item` so the host can hit-test it.
use winrust_ui::{
    anim::{EASE, LINEAR, SPRING},
    frame::{Cursor, Frame, Id, WIN_CLOSE, WIN_MAX, WIN_MIN},
    gfx::{Align, Canvas, Font, Picture, Rect, Rgba, Sample, Xf},
    widgets::{
        button::{self, segmented_width},
        field::field_with,
        look::BODY,
        scroll::{self, View},
    },
};

use crate::{
    Box4, Cmd, Drive, Extra, Fill, Hero, Hue, Line, Op, Row, Tile, Tone, WU_PAGE, is_seg,
    app::{
        Fid, ID_FIRST_SELECT, ID_ARR, ID_BACK, ID_DLG_KEEP, ID_DLG_REVERT, ID_DLG_SCRIM, ID_GACT, ID_HERO, ID_HIT, ID_IDENTIFY, ID_NAV, ID_PROFILE, ID_SCROLL, ID_SEARCH, ID_WALL, ID_AQ, P_ACT, P_BODY, P_CTL, P_FIELD, P_MUTE, P_SEG, Settings, row_id,
        style_search,
    },
    look::{self, AMBER, CARD, GREEN, RED, SIDE, T1, T2, T3, T4, WIN, acc, acc_at, ink},
    os::{Battery, os},
    PAGES,
};

const SIDE_W: f32 = 236.0;
const HEAD_H: f32 = 56.0;
const VIEW_W: f32 = 640.0;
const GAP: f32 = 22.0;
/// The scrollbar the mockup's browser reserves beside a scrolled page.
const BAR_W: f32 = 10.0;

/// One cell of the accent swatches.
const SWATCH: f32 = 30.0;

const BODY_M: Font = Font::new(13.0, 500);
const SMALL: Font = Font::new(12.0, 400);

fn tone_ink(t: Tone) -> Rgba {
    match t {
        Tone::Dim => T3,
        Tone::Good => GREEN,
        Tone::Warn => AMBER,
        Tone::Bad => RED,
        Tone::Accent => acc().accent_text(),
        Tone::Faint => T4,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// layout

/// What a line unfolds into.
#[derive(Clone, Debug)]
pub enum ExpItem {
    Fact(String, String),
    Field(Fid, &'static str),
    Err(String),
    Note(String),
    Acts(Vec<Act>),
}

/// A button under an unfolded line.
#[derive(Clone, Debug)]
pub struct Act {
    pub label: String,
    pub kind: BKind,
    pub busy: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BKind {
    Quiet,
    Primary,
    Danger,
    /// The one of a few ways that is chosen: the accent's tint.
    Chosen,
    /// One of a few ways that is not chosen.
    Soft,
}

impl ExpItem {
    fn height(&self) -> f32 {
        match self {
            ExpItem::Fact(..) | ExpItem::Err(_) | ExpItem::Note(_) => 15.0,
            ExpItem::Field(..) => 32.0,
            ExpItem::Acts(_) => 30.0,
        }
    }
}

/// The height of what unfolds: its items, 8 apart, and 14 under them.
#[must_use]
pub fn exp_height(items: &[ExpItem]) -> f32 {
    if items.is_empty() { 0.0 } else { items.iter().map(ExpItem::height).sum::<f32>() + 8.0 * (items.len() - 1) as f32 + 14.0 }
}

pub struct RowLay {
    /// From the top of the card.
    pub y: f32,
    pub h: f32,
    pub exp: Vec<ExpItem>,
}

pub struct GLay {
    pub y: f32,
    pub head: bool,
    pub card_y: f32,
    pub card_h: f32,
    /// The top of the extra, of the rows, of the radar, of the empty line (from the card's top).
    pub extra_h: f32,
    pub radar_y: Option<f32>,
    pub empty_y: Option<f32>,
    pub rows: Vec<RowLay>,
    pub foot_y: f32,
    pub foot_h: f32,
    pub h: f32,
}

pub struct Lay {
    pub groups: Vec<GLay>,
    pub h: f32,
}

/// The height of the extra of a group.
fn extra_height(e: &Extra) -> f32 {
    match e {
        Extra::None => 0.0,
        Extra::Arrange => 228.0,
        Extra::Hero(_) => 84.0,
        Extra::Battery(_) => 81.0,
        Extra::Drives(d) => d.iter().enumerate().map(|(i, _)| 76.0 + if i > 0 { 1.0 } else { 0.0 }).sum(),
        Extra::Walls(_) => 88.0,
        Extra::Search(_) => 52.0,
    }
}

/// The height of a line's own row.
fn row_height(l: &Line) -> f32 {
    if let Some(h) = l.h {
        h
    } else if l.big {
        72.0
    } else if matches!(l.row, Row::Cycle(_, _, Op::Proxy | Op::Pac | Op::Try)) {
        40.0
    } else if l.sub.is_empty() {
        48.0
    } else {
        58.0
    }
}

impl Settings {
    /// What a line unfolds into, now (empty when it is not open).
    #[must_use]
    pub fn expansion(&self, l: &Line) -> Vec<ExpItem> {
        let open = self.open.as_deref() == Some(l.key.as_str());
        let err = |v: &mut Vec<ExpItem>| {
            if !self.field_err.is_empty() && self.field_err_key == l.key {
                v.push(ExpItem::Err(self.field_err.clone()));
            }
        };
        let mut v = Vec::new();
        match &l.row {
            Row::Adapter(a, true) => v.extend(a.details().into_iter().map(|(k, val)| ExpItem::Fact(k, val))),
            Row::Wifi(n, _) if open => {
                let busy = self.connecting.as_ref().is_some_and(|c| c.0 == n.ssid);
                if n.connected {
                    v.push(ExpItem::Acts(vec![Act { label: t!("Отключить", "Disconnect", ja: "切断", pt: "Desconectar", zh: "断开连接").into(), kind: BKind::Quiet, busy: false }, Act { label: t!("Забыть", "Forget").into(), kind: BKind::Danger, busy: false }]));
                } else {
                    if n.profile.is_none() {
                        if n.sec.is_some_and(|s| s.needs_key()) {
                            v.push(ExpItem::Field(Fid::Pass, t!("Пароль Wi-Fi", "Wi-Fi password")));
                        } else if n.sec.is_none() && n.eap.is_some() {
                            v.push(ExpItem::Field(Fid::User, t!("Имя пользователя", "User name")));
                            v.push(ExpItem::Field(Fid::Pass, t!("Пароль", "Password")));
                        } else if n.sec.is_some() {
                            v.push(ExpItem::Note(t!("Открытая сеть: другие люди рядом могут видеть, что вы отправляете.", "Open network: other people nearby can see what you send.").into()));
                        }
                    }
                    err(&mut v);
                    if n.profile.is_some() || n.sec.is_some() || n.eap.is_some() {
                        let mut acts = vec![Act { label: if busy { t!("Подключение…", "Connecting…").into() } else { t!("Подключить", "Connect", ja: "接続", pt: "Conectar", zh: "连接").into() }, kind: BKind::Primary, busy }];
                        if n.profile.is_some() {
                            acts.push(Act { label: t!("Забыть", "Forget").into(), kind: BKind::Danger, busy: false });
                        }
                        v.push(ExpItem::Acts(acts));
                    }
                }
            }
            Row::Cycle(_, _, Op::Hidden) if open => {
                v.push(ExpItem::Field(Fid::Ssid, t!("Имя сети", "Network name")));
                v.push(ExpItem::Field(Fid::Pass2, t!("Пароль (пусто: открытая сеть)", "Password (leave empty for an open network)")));
                err(&mut v);
                v.push(ExpItem::Acts(vec![Act { label: t!("Подключить", "Connect").into(), kind: BKind::Primary, busy: false }]));
            }
            Row::Cycle(_, _, Op::Proxy) => {
                v.push(ExpItem::Field(Fid::Proxy, "proxy.example.com:8080"));
                err(&mut v);
            }
            Row::Cycle(_, _, Op::Pac) => {
                v.push(ExpItem::Field(Fid::Pac, t!("Адрес скрипта (PAC), необязательно", "Setup script (PAC) address, optional")));
                err(&mut v);
            }
            Row::Item(_, _, Cmd::Rename) if open => {
                v.push(ExpItem::Field(Fid::Pc, t!("Имя компьютера", "Computer name")));
                err(&mut v);
                v.push(ExpItem::Note(t!("До 15 букв, цифр или дефисов. Применится после перезагрузки.", "Up to 15 letters, digits or hyphens. Takes effect after a restart.").into()));
                v.push(ExpItem::Acts(vec![Act { label: t!("Сохранить", "Save").into(), kind: BKind::Primary, busy: false }, Act { label: t!("Отмена", "Cancel").into(), kind: BKind::Quiet, busy: false }]));
            }
            Row::Cycle(_, _, Op::Try) => v.push(ExpItem::Field(Fid::Try, t!("Удерживайте клавишу здесь, чтобы почувствовать скорость повтора", "Hold down a key here to feel the repeat speed"))),
            Row::Item(_, _, Cmd::NotifyApp(_)) if open => {
                let cur = l.cur.unwrap_or(0);
                v.push(ExpItem::Acts(crate::more::mode_labels().iter().enumerate().map(|(i, name)| Act { label: (*name).into(), kind: if i == cur { if i == 2 { BKind::Danger } else { BKind::Chosen } } else { BKind::Soft }, busy: false }).collect()));
            }
            Row::Item(_, _, Cmd::BtUnpair(_)) if open => {
                let armed = self.armed.as_deref() == Some(l.key.as_str());
                v.push(ExpItem::Acts(vec![Act { label: if armed { t!("Нажмите ещё раз, чтобы удалить", "Click again to remove").into() } else { t!("Удалить устройство", "Remove device").into() }, kind: BKind::Danger, busy: false }]));
            }
            _ => {}
        }
        v
    }

    /// Numbers the lines of the page in the order they are drawn.
    pub fn number_lines(&mut self) {
        self.flat.clear();
        self.flat_base.clear();
        for (g, grp) in self.pg.groups.iter().enumerate() {
            self.flat_base.push(self.flat.len());
            for r in 0..grp.rows.len() {
                self.flat.push((g, r));
            }
        }
    }

    /// Lays the page out: from the model, what is open and the width-independent heights of the mockup.
    fn layout(&self) -> Lay {
        let mut y = 4.0; // the scroll area's padding
        let mut groups = Vec::new();
        for (gi, g) in self.pg.groups.iter().enumerate() {
            let head = !g.title.is_empty() || g.act.is_some();
            let top = y;
            let mut at = if head { 22.0 + 8.0 } else { 0.0 };
            let card_y = top + at;
            let extra_h = extra_height(&g.extra);
            let mut rows = Vec::new();
            let mut cy = extra_h;
            for (ri, l) in g.rows.iter().enumerate() {
                let first = ri == 0 && extra_h == 0.0;
                let sep = if first { 0.0 } else { 1.0 };
                let flat_exp = self.expansion(l);
                let h = row_height(l);
                rows.push(RowLay { y: cy + sep, h, exp: flat_exp });
                cy += sep + h + exp_height(&rows[rows.len() - 1].exp);
            }
            let radar_y = g.radar.then(|| {
                let v = cy + 1.0;
                cy = v + 58.0;
                v
            });
            let empty_y = (!g.empty.is_empty()).then(|| {
                let v = cy;
                cy += 48.0;
                v
            });
            let card_h = cy;
            at += if card_h > 0.0 { card_h } else { 0.0 };
            let mut foot_h = 0.0;
            let mut foot_y = 0.0;
            if !g.foot.is_empty() {
                foot_y = top + at + 8.0;
                foot_h = 18.0 * (g.foot.chars().count() as f32 / 95.0).ceil().max(1.0);
                at += 8.0 + foot_h;
            }
            let _ = gi;
            groups.push(GLay { y: top, head, card_y, card_h, extra_h, radar_y, empty_y, rows, foot_y, foot_h, h: at });
            y = top + at + GAP;
        }
        Lay { h: (y - GAP).max(4.0) + 32.0, groups }
    }

    /// The content y of the line with `key` (the search flashes it and scrolls to it).
    fn line_y(&self, lay: &Lay, key: &str) -> Option<(f32, f32)> {
        for (gi, g) in self.pg.groups.iter().enumerate() {
            for (ri, l) in g.rows.iter().enumerate() {
                if l.key == key {
                    let gl = &lay.groups[gi];
                    let r = &gl.rows[ri];
                    return Some((gl.card_y + r.y, r.h));
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the frame

    pub(crate) fn paint(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        self.anchors.clear();
        self.first_select = None;
        self.tracks.clear();
        self.arr_box = None;
        if !f.menu_open() {
            self.pick = None;
        }
        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, WIN);
        self.paint_content(c, f, Rect::new(SIDE_W, HEAD_H, w - SIDE_W, h - HEAD_H));
        self.paint_header(c, f, w);
        self.paint_sidebar(c, f, h);
        if self.confirm.is_some() {
            self.paint_confirm(c, f);
        }
        if self.demo_identify {
            // the render of the Identify button's result: the numbers as the mockup lays them on its one screen (the real ones are windows
            // of their own, one on each display)
            crate::identify::tile(c, Rect::new(60.0, 60.0, 120.0, 120.0), 1);
            crate::identify::tile(c, Rect::new(w - 180.0, 60.0, 120.0, 120.0), 2);
        }
    }

    fn paint_sidebar(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, h: f32) {
        c.fill(Rect::new(0.0, 0.0, SIDE_W, h), 0.0, SIDE);
        c.fill(Rect::new(SIDE_W - 1.0, 0.0, 1.0, h), 0.0, ink(0.06));
        // the search box
        let s = Rect::new(10.0, 14.0, 215.0, 34.0);
        let ring = f.tween(ID_SEARCH, 8, f32::from(!self.q.text.trim().is_empty()), 160, EASE);
        field_with(c, f, ID_SEARCH, s, &mut self.q, t!("Поиск", "Search"), &style_search());
        if ring > 0.0 {
            c.ring(s, 9.0, 1.0, acc_at(0.6 * ring));
        }
        // the profile
        let p = Rect::new(10.0, 60.0, 215.0, 52.0);
        let hot = f.focusable(ID_PROFILE, p);
        f.hand();
        c.fill(p, 10.0, ink(0.05 * hot.hover));
        let o = os();
        let (user, pc) = (o.user_name(), o.computer_name());
        let initial = user.chars().next().map_or('?', |ch| ch.to_ascii_uppercase()).to_string();
        let av = Rect::new(p.x + 8.0, p.y + 8.0, 36.0, 36.0);
        c.circle(av.cx(), av.cy(), 18.0, acc_at(0.22));
        c.text(&initial, av, Font::new(15.0, 600), acc().accent_text(), Align::Center);
        c.text_fit(&user, Rect::new(p.x + 54.0, p.y + 10.5, p.w - 62.0, 16.0), BODY_M.weight600(), T1, Align::Start);
        c.text_fit(&pc, Rect::new(p.x + 54.0, p.y + 26.5, p.w - 62.0, 15.0), SMALL, T3, Align::Start);
        // the pages
        let searching = !self.q.text.trim().is_empty();
        let wu_count = self.wu_badge;
        for i in 0..PAGES {
            let y = 122.0 + 36.0 * i as f32 + 10.0 * [5, 9, 11].iter().filter(|&&g| i >= g).count() as f32;
            let r = Rect::new(10.0, y, 215.0, 34.0);
            let cur = !searching && i == self.page;
            let hot = f.focusable(ID_NAV + i as Id, r);
            f.hand();
            let on = f.tween(ID_NAV + i as Id, 9, f32::from(cur), 160, EASE);
            c.fill(r, 8.0, ink(0.05 * hot.hover));
            c.fill(r, 8.0, acc_at(0.18 * on));
            let (tile, glyph) = look::NAV[i];
            let t = Rect::new(r.x + 8.0, r.y + 6.0, 22.0, 22.0);
            c.fill(t, 6.0, look::rgb(tile));
            c.icon(glyph, (t.cx(), t.cy()), 13.0, 2.2, Rgba::pure_white(1.0), 0.0);
            let title = crate::page_title(i);
            c.text_fit(&title, Rect::new(r.x + 40.0, r.y, r.w - 48.0 - if i == WU_PAGE && wu_count > 0 { 26.0 } else { 0.0 }, r.h), Font::new(13.0, if cur { 600 } else { 500 }), T1, Align::Start);
            if i == WU_PAGE && wu_count > 0 {
                let b = Rect::new(r.x + r.w - 8.0 - 18.0, r.cy() - 9.0, 18.0, 18.0);
                c.fill(b, 9.0, look::BADGE);
                c.text(&wu_count.to_string(), b, Font::new(11.0, 600), Rgba::pure_white(1.0), Align::Center);
            }
            if hot.focus {
                c.ring(r.inset(-3.0), 11.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
            }
        }
    }

    fn paint_header(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        // the back button
        let b = Rect::new(SIDE_W + 22.0, 14.0, 28.0, 28.0);
        let can = !self.hist.is_empty();
        let hot = f.focusable(ID_BACK, b);
        if can {
            f.hand();
            c.fill(b, 8.0, ink(0.08 * hot.hover));
        }
        c.icon(look::CHEVRON_LEFT, (b.cx(), b.cy()), 16.0, 2.2, if can { T1 } else { T4 }, 0.0);
        f.tip(t!("Назад", "Back"));
        if hot.focus {
            c.ring(b.inset(-3.0), 11.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
        }
        let title = if self.q.text.trim().is_empty() { crate::page_title(self.page) } else { t!("Поиск", "Search").into() };
        c.text(&title, Rect::new(SIDE_W + 62.0, 0.0, (w - SIDE_W - 62.0 - 120.0).max(0.0), HEAD_H), Font::new(18.0, 600).tracking(-0.01), T1, Align::Start);
        // the window's buttons, as `winBtns` of every mockup
        let x0 = w - 12.0 - 94.0;
        for (i, (id, d)) in [(WIN_MIN, "M6 12h12"), (WIN_MAX, if f.maximized() { "M8.5 8.5h9v9h-9zM6 15V6h9" } else { "M6.5 6.5h11v11h-11z" }), (WIN_CLOSE, "M7 7l10 10M17 7L7 17")].into_iter().enumerate() {
            let r = Rect::new(x0 + i as f32 * 34.0, 15.0, 26.0, 26.0);
            let hot = f.item(id, r);
            f.hand();
            let glide = f.tween(id, 2, f32::from(f.hover == Some(id)), 280, SPRING);
            let (bg, fg) = if id == WIN_CLOSE { (ink(0.07).mix(Rgba::css(229, 72, 77, 0.22), hot.hover), T2.mix(RED, hot.hover)) } else { (ink(0.07 + 0.09 * hot.hover), T2.mix(T1, hot.hover)) };
            c.transformed(r.cx(), r.cy(), 1.0 - 0.14 * hot.press, 0.0, 0.0, |c| {
                c.fill(r, 13.0, bg);
                let (dy, k, turn) = match id {
                    WIN_MIN => (2.0 * glide, 1.0, 0.0),
                    WIN_MAX => (0.0, 1.0 + 0.2 * glide, 0.0),
                    _ => (0.0, 1.0, 90.0 * glide),
                };
                c.icon(d, (r.cx(), r.cy() + dy), 12.0 * k, 2.4, fg, turn);
            });
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the page

    fn paint_content(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, vp: Rect) {
        self.number_lines();
        let searching = !self.q.text.trim().is_empty();
        let lay = if searching { self.search_layout() } else { self.layout() };
        let now = f.now;
        // a line the search sent us to is scrolled to
        let mut sc = std::mem::take(&mut self.scroll);
        sc.set_extent(lay.h, vp.h);
        if let Some(key) = self.reveal.take()
            && let Some((y, h)) = self.line_y(&lay, &key)
        {
            sc.scroll_to((y + h / 2.0 - vp.h / 2.0).max(0.0), now);
        }
        let scrollable = lay.h > vp.h;
        let avail = vp.w - 64.0 - if scrollable { BAR_W } else { 0.0 };
        let vw = VIEW_W.min(avail);
        let x = vp.x + 32.0 + ((avail - vw) / 2.0).max(0.0);
        scroll::scroll(c, f, ID_SCROLL, vp, &mut sc, lay.h, |c, f, view| {
            if searching {
                self.paint_hits(c, f, view, x, vw, &lay);
            } else {
                for gi in 0..self.pg.groups.len() {
                    self.paint_group(c, f, view, x, vw, gi, &lay);
                }
            }
        });
        self.scroll = sc;
        if self.flash.as_ref().is_some_and(|(_, t0)| now.saturating_sub(*t0) >= 1600) {
            self.flash = None;
        }
    }

    #[allow(clippy::too_many_arguments)] // the frame, the view, where the column is, and which group
    fn paint_group(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, view: &View, x: f32, w: f32, gi: usize, lay: &Lay) {
        let gl = &lay.groups[gi];
        if view.at(gl.y + gl.h) < view.r.y || view.at(gl.y) > view.r.y + view.r.h {
            // off the screen: its lines still need their numbers, but nothing is drawn or named
            return;
        }
        let top = view.at(gl.y);
        // the title row
        if gl.head {
            let g = &self.pg.groups[gi];
            let (title, note, act) = (g.title.clone(), g.note.clone(), g.act.as_ref().map(|a| (a.label.clone(), a.busy)));
            let tw = c.measure(&title, Font::new(13.0, 600)).ceil();
            c.text(&title, Rect::new(x + 4.0, top, tw, 22.0), Font::new(13.0, 600), T1, Align::Start);
            if !note.is_empty() {
                c.text_fit(&note, Rect::new(x + 4.0 + tw + 8.0, top + 1.0, (w - tw - 130.0).max(0.0), 22.0), SMALL, T3, Align::Start);
            }
            if let Some((label, busy)) = act {
                let bw = c.measure(&label, Font::new(12.0, 500)).ceil() + 20.0 + if busy { 17.0 } else { 0.0 };
                let r = Rect::new(x + w - bw, top - 2.0, bw, 26.0);
                self.small_button(c, f, ID_GACT + gi as Id, r, &label, busy, false);
            }
        }
        let g = &self.pg.groups[gi];
        let card = Rect::new(x, view.at(gl.card_y), w, gl.card_h);
        if gl.card_h > 0.0 {
            c.fill(card, 14.0, CARD);
            c.ring(card, 14.0, 1.0, ink(0.06));
        }
        // what is inside, clipped to the card
        let flash_here = self.flash.as_ref().is_some_and(|(k, _)| g.rows.iter().any(|l| &l.key == k));
        let draw = |me: &mut Self, c: &mut Canvas<'_>, f: &mut Frame<'_>| {
            f.clipped(c, card, |c, f| {
                let ex = Rect::new(card.x, card.y, card.w, gl.extra_h);
                match &me.pg.groups[gi].extra {
                    Extra::None => {}
                    Extra::Arrange => me.paint_arrange(c, f, ex),
                    Extra::Hero(h) => paint_hero(c, f, ex, h),
                    Extra::Battery(b) => paint_battery(c, ex, b.as_ref()),
                    Extra::Drives(d) => paint_drives(c, ex, d),
                    Extra::Walls(wl) => paint_walls(c, f, ex, wl, &mut me.wall_pics),
                    Extra::Search(count) => {
                        let count = count.clone();
                        me.paint_app_search(c, f, ex, &count);
                    }
                }
                for ri in 0..me.pg.groups[gi].rows.len() {
                    me.paint_line(c, f, view, gi, ri, card, lay);
                }
                if let Some(ry) = gl.radar_y {
                    paint_radar(c, f, Rect::new(card.x, card.y + ry, card.w, 58.0), ry > 0.0);
                }
                if let Some(ey) = gl.empty_y {
                    c.text(&me.pg.groups[gi].empty, Rect::new(card.x + 16.0, card.y + ey + 16.0, card.w - 32.0, 16.0), BODY, T3, Align::Start);
                }
            });
        };
        if flash_here {
            // the card is overflow-hidden with round corners: a flash on its first or last line must not square them
            c.masked(card, 14.0, winrust_ui::gfx::Xf::NONE, |c| draw(self, c, f));
        } else {
            draw(self, c, f);
        }
        let g = &self.pg.groups[gi];
        if !g.foot.is_empty() {
            c.text_wrapped(&g.foot, Rect::new(x + 4.0, view.at(gl.foot_y), w - 8.0, gl.foot_h), SMALL, T3, 18.0, 0);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // lines

    #[allow(clippy::too_many_arguments)]
    fn paint_line(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, view: &View, gi: usize, ri: usize, card: Rect, lay: &Lay) {
        let rl = &lay.groups[gi].rows[ri];
        let y = card.y + rl.y;
        // a line above or below what is in view is neither drawn nor named
        if y + rl.h + exp_height(&rl.exp) < view.r.y || y > view.r.y + view.r.h {
            return;
        }
        let flat = self.flat_base[gi] + ri;
        let g = &self.pg.groups[gi];
        let l = &g.rows[ri];
        let r = Rect::new(card.x, y, card.w, rl.h);
        let first = ri == 0 && lay.groups[gi].extra_h == 0.0;
        if !first {
            c.fill(Rect::new(card.x, y - 1.0, card.w, 1.0), 0.0, ink(0.06));
        }
        let clickable = matches!(l.row, Row::Wifi(..) | Row::Adapter(..)) || matches!(l.row, Row::Cycle(_, _, Op::Hidden)) || matches!(l.row, Row::Item(_, _, Cmd::BtUnpair(_) | Cmd::NotifyApp(_) | Cmd::PrivGo(_) | Cmd::OpenSecurity));
        let mut hover = 0.0;
        if clickable {
            let hot = f.item(row_id(flat, P_BODY), r);
            f.hand();
            hover = hot.hover;
        }
        c.fill(r, 0.0, ink(0.03 * hover));
        if let Some((k, t0)) = &self.flash
            && *k == l.key
        {
            let p = (f.now.saturating_sub(*t0) as f32 / 1600.0).clamp(0.0, 1.0);
            c.fill(r, 0.0, acc_at(0.22 * (1.0 - EASE.ease(p))));
            f.due(f.now + 16);
        }
        self.paint_row_main(c, f, flat, gi, ri, r);
        // what unfolds
        if !rl.exp.is_empty() {
            self.paint_exp(c, f, flat, gi, ri, Rect::new(card.x, y + rl.h, card.w, exp_height(&rl.exp)), &rl.exp);
        }
    }

    fn paint_row_main(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, flat: usize, gi: usize, ri: usize, r: Rect) {
        let l = &self.pg.groups[gi].rows[ri];
        // the items at the right, from the right edge
        let items = self.right_items(l);
        let mut widths = Vec::with_capacity(items.len());
        for it in &items {
            widths.push(item_width(c, it));
        }
        let mut x = r.x + r.w - 16.0;
        let mut spots = vec![0.0; items.len()];
        for i in (0..items.len()).rev() {
            x -= widths[i];
            spots[i] = x;
            x -= 12.0;
        }
        let right_edge = if items.is_empty() { r.x + r.w - 16.0 } else { x + 12.0 - 12.0 };
        // the tile
        let mut lx = r.x + 16.0;
        if l.tile != Tile::None {
            let t = Rect::new(lx, r.cy() - 15.0, 30.0, 30.0);
            paint_tile(c, t, l.tile);
            if l.lock {
                lock_badge(c, t);
            }
            lx += 42.0;
        }
        // the label and what is under it
        let has_sub = !l.sub.is_empty() || self.sub_override(l).is_some();
        let sub_text = self.sub_override(l).unwrap_or_else(|| l.sub.clone());
        let block_h = if has_sub { 33.0 } else { 16.0 };
        let top = r.cy() - block_h / 2.0;
        let avail = (right_edge - 12.0 - lx).max(0.0);
        let label = self.label_of(l);
        let font = Font::new(13.0, l.weight);
        let lw = c.measure(&label, font).ceil();
        let (badge_w, badge) = match &l.badge {
            Some((t, _)) => (c.measure(t, Font::new(11.0, 500)).ceil() + 12.0, Some(t.clone())),
            None => (0.0, None),
        };
        let fit = lw.min((avail - if badge.is_some() { badge_w + 8.0 } else { 0.0 }).max(0.0));
        let ink_label = if self.is_armed(l) { RED } else { T1 };
        c.text_fit(&label, Rect::new(lx, top, fit, 16.0), font, ink_label, Align::Start);
        if let (Some(b), Some((_, tone))) = (badge, &l.badge) {
            let br = Rect::new(lx + fit + 8.0, top - 1.0, badge_w, 18.0);
            c.fill(br, 5.0, tone_ink(*tone).alpha(0.16));
            c.text(&b, br, Font::new(11.0, 500), tone_ink(*tone), Align::Center);
        }
        if has_sub {
            let tone = self.sub_tone(l);
            c.text_fit(&sub_text, Rect::new(lx, top + 18.0, avail, 15.0), SMALL, tone_ink(tone), Align::Start);
        }
        // the items
        for (i, it) in items.iter().enumerate() {
            self.paint_item(c, f, flat, it, spots[i], widths[i], r);
        }
    }

    fn label_of(&self, l: &Line) -> String {
        if let Row::Slider(label, _) = &l.row {
            return label.clone();
        }
        l.row.label().to_string()
    }

    fn is_armed(&self, l: &Line) -> bool {
        self.armed.as_deref() == Some(l.key.as_str()) && matches!(l.row, Row::Item(_, _, Cmd::Uninstall(_)))
    }

    /// What the app's state says under a line instead of the model's text (a connection under way, a pairing, a PIN).
    fn sub_override(&self, l: &Line) -> Option<String> {
        match &l.row {
            Row::Wifi(n, _) if self.connecting.as_ref().is_some_and(|c| c.0 == n.ssid) => Some(t!("Подключение…", "Connecting…", ja: "接続中…", pt: "Conectando…", zh: "正在连接…").into()),
            Row::Item(_, _, Cmd::BtPair(id)) if self.pin_err.as_deref() == Some(id.as_str()) => Some(t!("Нужен PIN · сопрягите в параметрах Windows", "Needs a PIN \u{b7} pair it in Windows\u{2019} own settings").into()),
            Row::Item(_, _, Cmd::BtPair(id)) if self.pairing.as_deref() == Some(id.as_str()) => Some(t!("Сопряжение…", "Pairing…").into()),
            Row::Item(_, _, Cmd::Uninstall(_)) if self.is_armed(l) => None,
            _ => None,
        }
    }

    fn sub_tone(&self, l: &Line) -> Tone {
        match &l.row {
            Row::Wifi(n, _) if self.connecting.as_ref().is_some_and(|c| c.0 == n.ssid) => Tone::Accent,
            Row::Item(_, _, Cmd::BtPair(id)) if self.pin_err.as_deref() == Some(id.as_str()) => Tone::Warn,
            _ => l.tone,
        }
    }

    /// What stands at the right of a line, left to right.
    fn right_items(&self, l: &Line) -> Vec<RItem> {
        let mut v = Vec::new();
        match &l.row {
            Row::Toggle { .. } | Row::Switch(..) | Row::Flag(..) => v.push(RItem::Switch(l.on, l.dis)),
            Row::Gauge(..) => v.push(RItem::Slider { w: 260.0, v: l.vol, mute: None }),
            Row::Cycle(_, _, Op::Try) => {}
            Row::Cycle(_, _, Op::Accent) => v.push(RItem::Swatches(l.opts.len(), l.cur.unwrap_or(usize::MAX))),
            Row::Cycle(_, _, op) if is_seg(*op) && !l.opts.is_empty() && !matches!(op, Op::Topology | Op::Plan) => v.push(RItem::Seg(l.opts.clone(), l.cur.unwrap_or(usize::MAX))),
            Row::Info(..) => {
                if !l.val.is_empty() {
                    v.push(RItem::Val(l.val.clone()));
                }
            }
            Row::Cycle(_, _, Op::Primary) => {
                if l.badge.is_none() {
                    v.push(RItem::Btns(vec![RBtn { label: l.val.clone(), kind: BKind::Quiet, busy: false, dis: false }]));
                }
            }
            Row::Cycle(_, _, Op::Topology) => v.push(RItem::Seg(l.opts.clone(), l.cur.unwrap_or(usize::MAX))),
            Row::Cycle(_, _, Op::Plan) if l.opts.len() <= 3 => v.push(RItem::Seg(l.opts.clone(), l.cur.unwrap_or(usize::MAX))),
            Row::Cycle(_, _, Op::Night) => v.push(RItem::Steps(l.cur.unwrap_or(1).saturating_sub(1), l.val.clone())),
            Row::Cycle(_, _, Op::Hidden) => v.push(RItem::Chev(self.open.as_deref() == Some(l.key.as_str()))),
            Row::Cycle(_, _, Op::Proxy | Op::Pac) => v.push(RItem::Val(l.val.clone())),
            Row::Cycle(_, _, _) => v.push(RItem::Select(l.val.clone())),
            Row::Slider(_, src) => {
                let w = if matches!(src, crate::audio::Src::App(_)) { 300.0 } else { 320.0 };
                v.push(RItem::Slider { w, v: l.vol, mute: l.mute });
            }
            Row::Item(_, _, cmd) => match cmd {
                Cmd::BtUnpair(_) | Cmd::NotifyApp(_) => v.push(RItem::Chev(self.open.as_deref() == Some(l.key.as_str()))),
                Cmd::PrivGo(_) | Cmd::OpenSecurity => v.push(RItem::Chev(false)),
                Cmd::Rename => v.push(RItem::Btns(vec![RBtn { label: l.val.clone(), kind: BKind::Quiet, busy: false, dis: false }])),
                Cmd::QuickScan => v.push(RItem::Btns(vec![RBtn { label: l.val.clone(), kind: BKind::Quiet, busy: l.busy, dis: l.busy }])),
                Cmd::SyncTime => v.push(RItem::Btns(vec![RBtn { label: if self.syncing { t!("Синхронизация", "Syncing").into() } else { l.val.clone() }, kind: BKind::Quiet, busy: self.syncing, dis: self.syncing }])),
                Cmd::NotifyMode(..) => {}
                Cmd::CleanTemp => {
                    if l.busy {
                        v.push(RItem::Val(l.val.clone()));
                        v.push(RItem::Spin(14.0));
                    } else if self.cleaning {
                        v.push(RItem::Btns(vec![RBtn { label: t!("Очистка…", "Cleaning…").into(), kind: BKind::Quiet, busy: true, dis: true }]));
                    } else {
                        v.push(RItem::Btns(vec![RBtn { label: t!("Очистить", "Clean up").into(), kind: BKind::Quiet, busy: false, dis: false }]));
                    }
                }
                Cmd::EmptyBin => {
                    if l.sub.is_empty() {
                        v.push(RItem::Val(t!("Пуста", "Empty").into()));
                    } else {
                        v.push(RItem::Btns(vec![RBtn { label: t!("Очистить", "Empty").into(), kind: BKind::Quiet, busy: false, dis: false }]));
                    }
                }
                Cmd::Uninstall(_) => {
                    let armed = self.is_armed(l);
                    v.push(RItem::Btns(vec![RBtn { label: if armed { t!("Нажмите ещё раз, чтобы удалить", "Click again to uninstall").into() } else { t!("Удалить", "Uninstall").into() }, kind: if armed { BKind::Danger } else { BKind::Quiet }, busy: false, dis: false }]));
                }
                Cmd::Vpn(v2) => v.push(RItem::Btns(vec![RBtn { label: if v2.on { t!("Отключить", "Disconnect").into() } else { t!("Подключить", "Connect").into() }, kind: BKind::Quiet, busy: false, dis: false }])),
                Cmd::BtPair(id) => {
                    let me = self.pairing.as_deref() == Some(id.as_str());
                    v.push(RItem::Btns(vec![RBtn { label: if me { t!("Сопряжение", "Pairing").into() } else { l.val.clone() }, kind: BKind::Primary, busy: me, dis: self.pairing.is_some() }]));
                }
                Cmd::Check | Cmd::Install => {
                    if l.busy {
                        v.push(RItem::Btns(vec![RBtn { label: l.val.clone(), kind: BKind::Quiet, busy: true, dis: true }]));
                    } else if l.restart {
                        v.push(RItem::Btns(vec![RBtn { label: t!("Позже", "Later").into(), kind: BKind::Quiet, busy: false, dis: false }, RBtn { label: if self.armed.as_deref() == Some("wu-restart") { t!("Нажмите ещё раз: перезагрузка", "Click again to restart").into() } else { t!("Перезагрузить", "Restart now").into() }, kind: if self.armed.as_deref() == Some("wu-restart") { BKind::Danger } else { BKind::Primary }, busy: false, dis: false }]));
                    } else if matches!(cmd, Cmd::Install) {
                        v.push(RItem::Btns(vec![RBtn { label: t!("Скачать и установить", "Download and install", ja: "ダウンロードしてインストール", pt: "Baixar e instalar", zh: "下载并安装").into(), kind: BKind::Primary, busy: false, dis: false }]));
                    } else {
                        v.push(RItem::Btns(vec![RBtn { label: t!("Проверить наличие обновлений", "Check for updates", ja: "更新プログラムのチェック", pt: "Verificar atualizações", zh: "检查更新").into(), kind: BKind::Primary, busy: false, dis: false }]));
                    }
                }
                _ => {}
            },
            Row::Adapter(_, open) => v.push(RItem::Chev(*open)),
            Row::Wifi(n, _) => {
                if let Some(b) = l.bars {
                    v.push(RItem::Sig(b));
                }
                if self.connecting.as_ref().is_some_and(|c| c.0 == n.ssid) {
                    v.push(RItem::Spin(14.0));
                }
                v.push(RItem::Chev(self.open.as_deref() == Some(l.key.as_str())));
            }
        }
        v
    }

    #[allow(clippy::too_many_arguments)] // the frame, the line, the item, and where it stands
    fn paint_item(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, flat: usize, it: &RItem, x: f32, w: f32, r: Rect) {
        let id = |part: Id| row_id(flat, part);
        match it {
            RItem::Sig(n) => {
                let (bx, by) = (x, r.cy() - 7.0);
                for (k, (dx, dy, bh)) in [(0.0, 10.0, 4.0), (5.0, 7.0, 7.0), (10.0, 4.0, 10.0), (15.0, 0.0, 14.0)].into_iter().enumerate() {
                    let lit = (k as u32) < *n;
                    c.fill(Rect::new(bx + dx, by + dy, 3.0, bh), 1.0, if lit { T1 } else { ink(0.18) });
                }
            }
            RItem::Val(s) => c.text_fit(s, Rect::new(x, r.y, w, r.h), BODY.tabular(), T3, Align::End),
            RItem::Spin(d) => spinner(c, f, (x + d / 2.0, r.cy()), *d, acc()),
            RItem::Chev(open) => {
                let k = f.tween(id(P_BODY) + 100_000, 6, f32::from(*open), 250, SPRING);
                c.icon(look::CHEVRON_RIGHT, (x + 6.0, r.cy()), 12.0, 2.4, T4, 90.0 * k);
            }
            RItem::Switch(on, dim) => {
                let tr = Rect::new(x, r.cy() - 11.0, 38.0, 22.0);
                let (state, dis) = (on.unwrap_or(false), on.is_none() || *dim);
                c.faded(if dis { 0.45 } else { 1.0 }, |c| {
                    button::toggle(c, f, id(P_CTL), tr, state);
                });
            }
            RItem::Select(text) => {
                let b = Rect::new(x, r.cy() - 14.0, w, 28.0);
                let sid = if self.first_select.is_none() {
                    self.first_select = Some(flat);
                    ID_FIRST_SELECT
                } else {
                    id(P_CTL)
                };
                let hot = f.focusable(sid, b);
                f.hand();
                self.anchors.push((flat, b));
                c.fill(b, 7.0, ink(0.07 + 0.03 * hot.hover));
                c.text_fit(text, Rect::new(b.x + 11.0, b.y, b.w - 11.0 - 8.0 - 8.0 - 10.0, b.h), Font::new(12.5, 500), T1, Align::Start);
                c.icon("M9 10l3-3 3 3M9 14l3 3 3-3", (b.x + b.w - 8.0 - 5.0, b.cy()), 20.57, 1.8, T1.fade(0.6), 0.0);
                if hot.focus {
                    c.ring(b.inset(-3.0), 10.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
                }
            }
            RItem::Seg(labels, sel) => {
                let b = Rect::new(x, r.cy() - 15.0, w, 30.0);
                let names: Vec<&str> = labels.iter().map(String::as_str).collect();
                button::segmented(c, f, id(P_SEG), b, &names, *sel);
            }
            RItem::Btns(list) => {
                let mut bx = x;
                for (k, b) in list.iter().enumerate() {
                    let bw = btn_width(c, b);
                    let rect = Rect::new(bx, r.cy() - 14.0, bw, 28.0);
                    self.row_button(c, f, id(P_CTL + k as Id), rect, b);
                    bx += bw + 6.0;
                }
            }
            RItem::Slider { w: sw, v, mute } => {
                let mut sx = x;
                if let Some((op, muted)) = mute {
                    let b = Rect::new(sx, r.cy() - 13.0, 26.0, 26.0);
                    let hot = f.focusable(id(P_MUTE), b);
                    f.hand();
                    c.fill(b, 7.0, ink(0.08 * hot.hover));
                    let d = match (op, *muted) {
                        (Op::MicMute, true) => look::MIC_OFF,
                        (Op::MicMute, false) => look::MIC,
                        (_, true) => look::SPEAKER_OFF,
                        (_, false) => look::SPEAKER,
                    };
                    c.icon(d, (b.cx(), b.cy()), 15.0, 2.0, if *muted { RED } else { T3 }, 0.0);
                    sx += 36.0;
                }
                let track = Rect::new(sx, r.cy() - 10.0, *sw - (sx - x) - 44.0, 20.0);
                let val = if mute.is_some_and(|m| m.1) { 0.0 } else { v.unwrap_or(0.0) };
                self.slider(c, f, id(P_CTL), track, val, v.is_some(), flat, mute.is_some_and(|m| m.1));
                let pct = format!("{}", (val * 100.0).round() as u32);
                c.text(&pct, Rect::new(x + *sw - 34.0, r.y, 34.0, r.h), BODY.tabular(), T2, Align::End);
            }
            RItem::Swatches(n, sel) => {
                let b = Rect::new(x, r.cy() - 15.0, *n as f32 * SWATCH, 30.0);
                c.fill(b, 8.0, look::ui::WELL);
                for i in 0..*n {
                    let cell = Rect::new(b.x + i as f32 * SWATCH + 2.0, b.y + 2.0, SWATCH - 4.0, 26.0);
                    let hot = f.focusable(id(P_SEG + i as Id), cell);
                    f.hand();
                    let on = f.tween(id(P_SEG + i as Id), 2, f32::from(i == *sel), 200, EASE);
                    if on > 0.0 {
                        c.faded(on, |c| {
                            c.shadow(cell, 6.0, 1.0, 2.0, Rgba::css(0, 0, 0, 0.25));
                            c.fill(cell, 6.0, CARD);
                            c.ring(cell, 6.0, 1.0, ink(0.08));
                        });
                    }
                    c.fill(cell, 6.0, ink(0.05 * hot.hover));
                    let col = winrust_common::style::Accent::ALL.get(i).map_or(T2, |a| Rgba::colorref(a.color()));
                    c.circle(cell.cx(), cell.cy(), 3.5, col);
                    if hot.focus {
                        c.ring(cell.inset(-1.0), 7.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
                    }
                }
            }
            RItem::Steps(step, label) => {
                let track = Rect::new(x, r.cy() - 10.0, w - 62.0, 20.0);
                self.slider(c, f, id(P_CTL), track, *step as f32 / 3.0, true, flat, false);
                let k = label.split(" \u{b7} ").nth(1).unwrap_or(label);
                c.text(k, Rect::new(x + w - 52.0, r.y, 52.0, r.h), BODY.tabular(), T2, Align::End);
            }
        }
    }

    /// The range input of the mockup: a 4-px track filled to `v`, an 18-px thumb.
    #[allow(clippy::too_many_arguments)]
    fn slider(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, v: f32, live: bool, flat: usize, dim: bool) {
        let hot = f.focusable(id, r);
        f.cursor(Cursor::Hand);
        self.tracks.push((flat, r.x, r.x + r.w));
        let held = f.tween(id, 10, f32::from(f.press == Some(id)), 150, SPRING);
        let op = if dim { 0.45 } else { 1.0 };
        c.faded(op, |c| {
            let tr = Rect::new(r.x, r.cy() - 2.0, r.w, 4.0);
            c.fill(tr, 2.0, ink(0.14));
            c.fill(Rect::new(tr.x, tr.y, tr.w * v.clamp(0.0, 1.0), 4.0), 2.0, acc());
            if live {
                let cx = r.x + 9.0 + (r.w - 18.0) * v.clamp(0.0, 1.0);
                let k = 1.0 + 0.12 * held;
                c.shadow(Rect::new(cx - 9.0, r.cy() - 9.0, 18.0, 18.0), 9.0, 1.0, 3.0, Rgba::css(0, 0, 0, 0.4));
                c.circle(cx, r.cy(), 9.0 * k, Rgba::pure_white(1.0));
                c.ring(Rect::new(cx - 9.0 * k, r.cy() - 9.0 * k, 18.0 * k, 18.0 * k), 9.0 * k, 0.5, Rgba::css(0, 0, 0, 0.2));
            }
        });
        if hot.focus {
            c.ring(r.inset(-1.0), 6.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
        }
    }

    /// `.btn` of a row: 28 high, radius 7, 12.5 px.
    fn row_button(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, b: &RBtn) {
        let hot = f.focusable(id, r);
        if !b.dis {
            f.hand();
        }
        let lift = if b.dis { 0.0 } else { hot.hover };
        c.faded(if b.dis { 0.45 } else { 1.0 }, |c| {
            c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                let (bg, fg) = btn_colors(b.kind);
                c.fill(r, 7.0, bg);
                if lift > 0.0 {
                    c.fill(r, 7.0, Rgba::pure_white(0.08 * lift));
                }
                let mut tx = r.x + 12.0;
                if b.busy {
                    spinner(c, f, (tx + 5.5, r.cy()), 11.0, fg);
                    tx += 17.0;
                }
                c.text(&b.label, Rect::new(tx, r.y, r.w - (tx - r.x) - 12.0, r.h), Font::new(12.5, 500), fg, Align::Start);
            });
        });
        if hot.focus {
            c.ring(r.inset(-3.0), 10.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
        }
    }

    /// The small button of a group's title (26 high, 12 px).
    #[allow(clippy::too_many_arguments)]
    fn small_button(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, label: &str, busy: bool, dis: bool) {
        let hot = f.focusable(id, r);
        f.hand();
        c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
            c.fill(r, 7.0, ink(0.07 + 0.03 * hot.hover));
            let mut tx = r.x + 10.0;
            if busy {
                spinner(c, f, (tx + 5.5, r.cy()), 11.0, acc());
                tx += 17.0;
            }
            c.text(label, Rect::new(tx, r.y, r.w, r.h), Font::new(12.0, 500), if dis { T3 } else { T1 }, Align::Start);
        });
        if hot.focus {
            c.ring(r.inset(-3.0), 10.0, 2.0, winrust_ui::widgets::look::focus_ring().fade(0.9));
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // what unfolds under a line

    #[allow(clippy::too_many_arguments)] // the frame, the line, and what unfolds under it
    fn paint_exp(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, flat: usize, gi: usize, ri: usize, r: Rect, items: &[ExpItem]) {
        let age = f.tween(row_id(flat, P_BODY) + 200_000, 7, 1.0, 240, SPRING);
        let key = self.pg.groups[gi].rows[ri].key.clone();
        let mut y = r.y;
        let x = r.x + 58.0;
        let w = r.w - 58.0 - 16.0;
        let fields: Vec<Fid> = self.fields_of(flat);
        let mut fk = 0;
        c.faded(age, |c| {
            for it in items {
                let h = it.height();
                match it {
                    ExpItem::Fact(k, v) => {
                        c.text_fit(k, Rect::new(x, y, 120.0, h), SMALL, T3, Align::Start);
                        c.text_fit(v, Rect::new(x + 132.0, y, w - 132.0, h), Font::new(11.5, 400), T2, Align::Start);
                    }
                    ExpItem::Field(fid, ph) => {
                        let id = row_id(flat, P_FIELD + fk as Id);
                        let _ = fields.get(fk);
                        fk += 1;
                        let fr = Rect::new(x, y, w, 32.0);
                        let st = winrust_ui::widgets::field::FieldStyle { radius: 8.0, ..winrust_ui::widgets::field::FieldStyle::INPUT };
                        field_with(c, f, id, fr, self.fields.get(*fid), ph, &st);
                        if !self.field_err.is_empty() && self.field_err_key == key && matches!(fid, Fid::Pass | Fid::Pass2 | Fid::Pc) {
                            c.ring(fr, 8.0, 1.0, RED);
                        }
                    }
                    ExpItem::Err(e) => c.text_fit(e, Rect::new(x, y, w, h), SMALL, RED, Align::Start),
                    ExpItem::Note(n) => c.text_fit(n, Rect::new(x, y, w, h), SMALL, T3, Align::Start),
                    ExpItem::Acts(acts) => {
                        let mut bx = x;
                        for (k, a) in acts.iter().enumerate() {
                            let bw = c.measure(&a.label, BODY_M).ceil() + 28.0 + if a.busy { 18.0 } else { 0.0 };
                            let rect = Rect::new(bx, y, bw, 30.0);
                            let id = row_id(flat, P_ACT + k as Id);
                            let hot = f.focusable(id, rect);
                            f.hand();
                            c.transformed(rect.cx(), rect.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                                let (bg, fg) = match a.kind {
                                    BKind::Primary => (acc().accent_fill(), Rgba::pure_white(1.0)),
                                    BKind::Danger => (Rgba::css(229, 72, 77, 0.14), RED),
                                    BKind::Chosen => (acc_at(0.20), T1),
                                    BKind::Soft => (ink(0.06), T2),
                                    BKind::Quiet => (ink(0.08), T1),
                                };
                                c.fill(rect, 8.0, bg);
                                if hot.hover > 0.0 {
                                    c.fill(rect, 8.0, Rgba::pure_white(0.08 * hot.hover));
                                }
                                let mut tx = rect.x + 14.0;
                                if a.busy {
                                    spinner(c, f, (tx + 6.0, rect.cy()), 12.0, Rgba::pure_white(1.0));
                                    tx += 18.0;
                                }
                                c.text(&a.label, Rect::new(tx, rect.y, rect.w, rect.h), BODY_M, fg, Align::Start);
                            });
                            bx += bw + 6.0;
                        }
                    }
                }
                y += h + 8.0;
            }
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the picture of the displays

    /// The Apps page's own search box: a well with a magnifier, the text, "3 of 12" and a clear button once something is typed.
    fn paint_app_search(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, ex: Rect, count: &str) {
        let b = Rect::new(ex.x + 12.0, ex.y + 12.0, ex.w - 24.0, 36.0);
        let has = !self.aq.text.trim().is_empty();
        let ring = f.tween(ID_AQ, 8, f32::from(has), 160, EASE);
        c.fill(b, 10.0, look::ui::WELL);
        c.ring(b, 10.0, 1.0, ink(0.08).mix(acc_at(0.55), ring));
        let mut right = b.x + b.w - 6.0;
        if has {
            let cb = Rect::new(right - 24.0, b.cy() - 12.0, 24.0, 24.0);
            let hot = f.item(ID_AQ + 1, cb);
            f.hand();
            c.fill(cb, 7.0, ink(0.08 + 0.06 * hot.hover));
            c.icon("M7 7l10 10M17 7L7 17", (cb.cx(), cb.cy()), 11.0, 2.6, T2, 0.0);
            let cw = c.measure(count, SMALL.tabular()).ceil();
            c.text(count, Rect::new(cb.x - 8.0 - cw, b.y, cw, b.h), SMALL.tabular(), T3, Align::Start);
            right = cb.x - 8.0 - cw - 8.0;
        }
        let st = winrust_ui::widgets::field::FieldStyle { font: BODY, icon: Some(look::SEARCH), clear: false, bare: true, radius: 10.0, pad: 0.0 };
        let fr = Rect::new(b.x + 12.0, b.y, (right - b.x - 12.0).max(40.0), b.h);
        field_with(c, f, ID_AQ, fr, &mut self.aq, t!("Поиск приложений по названию или издателю", "Search apps by name or publisher", ja: "名前または発行元でアプリを検索", pt: "Pesquisar apps por nome ou editor", zh: "按名称或发布者搜索应用"), &st);
    }

    fn paint_arrange(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, ex: Rect) {
        let bx = Rect::new(ex.x + 14.0, ex.y + 14.0, ex.w - 28.0, 200.0);
        c.fill(bx, 10.0, ink(0.03));
        let b4 = Box4 { left: bx.x as i32, top: bx.y as i32, right: (bx.x + bx.w) as i32, bottom: (bx.y + bx.h - 26.0) as i32 };
        self.arr_box = Some(b4);
        let map = crate::arr_map(&self.mons, b4);
        let sel = self.sel_mon;
        // the dragged one last, so it is over the others
        let mut order: Vec<usize> = (0..self.mons.len()).collect();
        if let Some(d) = &self.arr {
            order.sort_by_key(|&j| j == d.idx);
        }
        for j in order {
            let m = self.mons[j];
            let (x, y) = match &self.arr {
                Some(d) if d.idx == j => d.cur,
                _ => (m.x, m.y),
            };
            let (x0, y0) = map.dev(x, y);
            let (x1, y1) = map.dev(x + m.w, y + m.h);
            let r = Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32);
            let hot = f.item(ID_ARR + j as Id, r);
            f.cursor(Cursor::Move);
            let on = j == sel;
            c.fill(r, 8.0, if on { acc_at(0.22) } else { ink(0.08 + 0.02 * hot.hover) });
            c.ring(r, 8.0, if on { 2.0 } else { 1.0 }, if on { acc() } else { ink(0.16) });
            let ink_n = if on { acc().accent_text() } else { T2 };
            c.text(&(j + 1).to_string(), Rect::new(r.x, r.cy() - 20.0, r.w, 24.0), Font::new(20.0, 600), ink_n, Align::Center);
            c.text(&format!("{}\u{d7}{}", m.w, m.h), Rect::new(r.x, r.cy() + 4.0, r.w, 14.0), Font::new(11.0, 400), T3, Align::Center);
            if m.primary {
                c.fill(Rect::new(r.cx() - 14.0, r.y + 6.0, 28.0, 3.0), 2.0, ink_n);
            }
        }
        c.text(t!("Перетащите дисплей так, как они стоят на столе", "Drag a display to match how they sit on your desk"), Rect::new(bx.x + 12.0, bx.y + bx.h - 24.0, bx.w - 100.0, 14.0), Font::new(11.5, 400), T3, Align::Start);
        let label = t!("Определить", "Identify");
        let bw = c.measure(label, Font::new(12.0, 500)).ceil() + 20.0;
        self.small_button(c, f, ID_IDENTIFY, Rect::new(bx.x + bx.w - 10.0 - bw, bx.y + bx.h - 8.0 - 26.0, bw, 26.0), label, false, false);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the search

    fn search_layout(&self) -> Lay {
        let mut y = 4.0;
        let mut groups = Vec::new();
        for f in &self.found {
            let n = f.hits.len() as f32;
            let card_h = n * 58.0 + (n - 1.0).max(0.0);
            let rows = (0..f.hits.len()).map(|i| RowLay { y: i as f32 * 59.0, h: 58.0, exp: Vec::new() }).collect();
            groups.push(GLay { y, head: true, card_y: y + 30.0, card_h, extra_h: 0.0, radar_y: None, empty_y: None, rows, foot_y: 0.0, foot_h: 0.0, h: 30.0 + card_h });
            y += 30.0 + card_h + GAP;
        }
        let h = if self.found.is_empty() { 4.0 + 60.0 + 16.0 + 60.0 } else { y - GAP + 32.0 };
        Lay { groups, h }
    }

    fn paint_hits(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, view: &View, x: f32, w: f32, lay: &Lay) {
        if self.found.is_empty() {
            let msg = tf!("Ничего не найдено по запросу «{}»", "No settings match \u{201c}{}\u{201d}", self.q.text.trim());
            c.text(&msg, Rect::new(x, view.at(4.0 + 60.0), w, 16.0), BODY, T3, Align::Center);
            return;
        }
        let mut n = 0usize;
        for (fi, found) in self.found.iter().enumerate() {
            let gl = &lay.groups[fi];
            let top = view.at(gl.y);
            c.text(&found.title, Rect::new(x + 4.0, top, w, 22.0), Font::new(13.0, 600), T1, Align::Start);
            let card = Rect::new(x, view.at(gl.card_y), w, gl.card_h);
            c.fill(card, 14.0, CARD);
            c.ring(card, 14.0, 1.0, ink(0.06));
            f.clipped(c, card, |c, f| {
                for (i, h) in found.hits.iter().enumerate() {
                    let r = Rect::new(card.x, card.y + i as f32 * 59.0, card.w, 58.0);
                    if i > 0 {
                        c.fill(Rect::new(r.x, r.y - 1.0, r.w, 1.0), 0.0, ink(0.06));
                    }
                    let hot = f.item(ID_HIT + n as Id, r);
                    f.hand();
                    c.fill(r, 0.0, ink(0.03 * hot.hover));
                    c.text_fit(&h.label, Rect::new(r.x + 16.0, r.cy() - 16.5, r.w - 60.0, 16.0), BODY_M, T1, Align::Start);
                    c.text_fit(&h.sub, Rect::new(r.x + 16.0, r.cy() + 1.5, r.w - 60.0, 15.0), SMALL, T3, Align::Start);
                    c.icon(look::CHEVRON_RIGHT, (r.x + r.w - 22.0, r.cy()), 12.0, 2.4, T4, 0.0);
                    n += 1;
                }
            });
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // "Keep these display settings?"

    fn paint_confirm(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let Some(cf) = self.confirm.as_ref() else { return };
        let (w, h) = f.size;
        let win = Rect::new(0.0, 0.0, w, h);
        let age = f.tween(ID_DLG_SCRIM + 300_000, 11, 1.0, 220, SPRING);
        f.item(ID_DLG_SCRIM, win); // takes every click behind the dialog
        c.fill(win, 0.0, Rgba::css(0, 0, 0, 0.35 * age));
        let (dw, dh) = (380.0, 22.0 + 44.0 + 12.0 + 39.0 + 12.0 + 6.0 + 32.0 + 22.0);
        let r = Rect::new(((w - dw) / 2.0).round(), ((h - dh) / 2.0).round(), dw, dh);
        let (left, what) = (cf.left, cf.what.clone());
        // the ring runs 1 s linear to the next step, as the mockup's `transition: stroke-dashoffset 1s linear`
        let to = left as f32 / crate::CONFIRM_SECS as f32;
        let ring = f.tween(ID_DLG_SCRIM + 400_000, 12, to, 1000, LINEAR);
        c.faded(age, |c| {
            c.transformed(r.cx(), r.cy(), 0.95 + 0.05 * age, 0.0, 0.0, |c| {
                c.outside(r, 16.0, |c| c.shadow(r, 16.0, 30.0, 80.0, Rgba::css(0, 0, 0, 0.5)));
                c.fill(r, 16.0, winrust_ui::widgets::look::MENU);
                c.ring(r.inset(-1.0), 17.0, 1.0, Rgba::css(0, 0, 0, 0.6));
                c.ring(r, 16.0, 1.0, ink(0.09));
                let mut y = r.y + 22.0;
                let (cx, cy) = (r.x + 22.0 + 22.0, y + 22.0);
                c.arc((cx, cy), 19.0, 0.0, 360.0, 3.0, Rgba::css(127, 127, 127, 0.2));
                c.arc((cx, cy), 19.0, -90.0, 360.0 * ring.clamp(0.0, 1.0), 3.0, acc());
                c.text(t!("Сохранить эти настройки экрана?", "Keep these display settings?"), Rect::new(r.x + 22.0 + 58.0, y + 1.0, dw - 80.0, 20.0), Font::new(15.0, 600), T1, Align::Start);
                c.text_fit(&what, Rect::new(r.x + 22.0 + 58.0, y + 24.0, dw - 80.0, 16.0), BODY, T3, Align::Start);
                y += 44.0 + 12.0;
                // "Going back to the previous settings in <b>15 s</b>, so a picture ...": the seconds stand out
                let secs = tf!("{} с", "{} s", left);
                let parts = [
                    (t!("Вернёмся к прежним настройкам через ", "Going back to the previous settings in "), false),
                    (secs.as_str(), true),
                    (t!(", чтобы картинка, которая не помещается на экран, исправилась сама.", ", so a picture that doesn\u{2019}t fit your screen fixes itself."), false),
                ];
                paint_rich(c, Rect::new(r.x + 22.0, y, dw - 44.0, 39.0), &parts);
                y += 39.0 + 12.0 + 6.0;
                let (revert, keep) = (t!("Вернуть", "Revert"), t!("Сохранить", "Keep changes"));
                let kf = Font::new(11.0, 400);
                let (rw, kw) = (c.measure(revert, BODY_M).ceil() + 6.0 + c.measure("Esc", kf).ceil() + 28.0, c.measure(keep, Font::new(13.0, 600)).ceil() + 6.0 + c.measure("Enter", kf).ceil() + 28.0);
                let kb = Rect::new(r.x + r.w - 22.0 - kw, y, kw, 32.0);
                let rb = Rect::new(kb.x - 8.0 - rw, y, rw, 32.0);
                for (id, b, label, hint, primary) in [(ID_DLG_REVERT, rb, revert, "Esc", false), (ID_DLG_KEEP, kb, keep, "Enter", true)] {
                    let hot = f.item(id, b);
                    f.hand();
                    c.transformed(b.cx(), b.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                        c.fill(b, 9.0, if primary { acc().accent_fill() } else { ink(0.08) });
                        if hot.hover > 0.0 {
                            c.fill(b, 9.0, Rgba::pure_white(0.1 * hot.hover));
                        }
                        let (fg, hint_ink) = if primary { (Rgba::pure_white(1.0), Rgba::pure_white(0.75)) } else { (T1, T3) };
                        let lw = c.measure(label, Font::new(13.0, if primary { 600 } else { 500 })).ceil();
                        c.text(label, Rect::new(b.x + 14.0, b.y, lw, 32.0), Font::new(13.0, if primary { 600 } else { 500 }), fg, Align::Start);
                        c.text(hint, Rect::new(b.x + 14.0 + lw + 6.0, b.y, 60.0, 32.0), kf, hint_ink, Align::Start);
                    });
                }
            });
        });
    }
}

/// Words of two weights wrapped into `r` line by line (19.5 apart): the seconds of the dialog are bold. A part that starts with
/// punctuation sticks to the word before it.
fn paint_rich(c: &mut Canvas<'_>, r: Rect, parts: &[(&str, bool)]) {
    let space = c.measure(" ", BODY);
    let (mut x, mut y) = (0.0, 0.0);
    for (text, bold) in parts {
        let font = if *bold { Font::new(13.0, 600).tabular() } else { BODY };
        let ink = if *bold { T1 } else { T2 };
        let glued = text.starts_with([',', '.', ';', ':']);
        for (i, word) in text.split_whitespace().enumerate() {
            let w = c.measure(word, font).ceil();
            let sticks = glued && i == 0;
            let lead = if x == 0.0 || sticks { 0.0 } else { space };
            if x + lead + w > r.w && x > 0.0 {
                (x, y) = (0.0, y + 19.5);
            }
            let at = if x == 0.0 { 0.0 } else { x + lead };
            c.text(word, Rect::new(r.x + at, r.y + y, w + 2.0, 19.5), font, ink, Align::Start);
            x = at + w;
        }
    }
}

trait Weight {
    fn weight600(self) -> Self;
}

impl Weight for Font {
    fn weight600(self) -> Self {
        Font { weight: 600, ..self }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the pieces of a line

enum RItem {
    Sig(u32),
    Val(String),
    Spin(f32),
    Chev(bool),
    Switch(Option<bool>, bool),
    Select(String),
    Seg(Vec<String>, usize),
    Btns(Vec<RBtn>),
    Slider { w: f32, v: Option<f32>, mute: Option<(Op, bool)> },
    Steps(usize, String),
    /// The accent swatches: how many, which is chosen.
    Swatches(usize, usize),
}

struct RBtn {
    label: String,
    kind: BKind,
    busy: bool,
    dis: bool,
}

fn btn_width(c: &mut Canvas<'_>, b: &RBtn) -> f32 {
    c.measure(&b.label, Font::new(12.5, 500)).ceil() + 24.0 + if b.busy { 17.0 } else { 0.0 }
}

fn item_width(c: &mut Canvas<'_>, it: &RItem) -> f32 {
    match it {
        RItem::Sig(_) => 18.0,
        RItem::Val(s) => c.measure(s, BODY.tabular()).ceil(),
        RItem::Spin(d) => *d,
        RItem::Chev(_) => 12.0,
        RItem::Switch(..) => 38.0,
        RItem::Select(s) => (c.measure(s, Font::new(12.5, 500)).ceil() + 11.0 + 8.0 + 10.0 + 8.0).min(280.0),
        RItem::Seg(l, _) => {
            let names: Vec<&str> = l.iter().map(String::as_str).collect();
            segmented_width(c, &names)
        }
        RItem::Btns(list) => list.iter().map(|b| btn_width(c, b)).sum::<f32>() + 6.0 * (list.len().saturating_sub(1)) as f32,
        RItem::Slider { w, .. } => *w,
        RItem::Steps(..) => 300.0,
        RItem::Swatches(n, _) => *n as f32 * SWATCH,
    }
}

fn btn_colors(k: BKind) -> (Rgba, Rgba) {
    match k {
        BKind::Quiet => (ink(0.08), T1),
        BKind::Primary => (acc().accent_fill(), Rgba::pure_white(1.0)),
        BKind::Danger => (Rgba::css(229, 72, 77, 0.16), RED),
        BKind::Chosen => (acc_at(0.20), T1),
        BKind::Soft => (ink(0.06), T2),
    }
}

fn paint_tile(c: &mut Canvas<'_>, r: Rect, t: Tile) {
    let (bg, fg, d): (Rgba, Rgba, &'static str) = match t {
        Tile::None => return,
        Tile::Quiet(d) => (ink(0.06), T2, d),
        Tile::Lit(d) => (acc_at(0.20), acc().accent_text(), d),
        Tile::Solid(d, col) => (look::rgb(col), Rgba::pure_white(1.0), d),
        Tile::Soft(d, tone) => {
            let (bg, fg) = match tone {
                Tone::Good => (Rgba::css(76, 195, 138, 0.16), GREEN),
                Tone::Warn => (Rgba::css(255, 163, 72, 0.16), AMBER),
                Tone::Bad => (Rgba::css(229, 72, 77, 0.14), RED),
                Tone::Accent => (acc_at(0.18), acc().accent_text()),
                Tone::Dim | Tone::Faint => (ink(0.06), T2),
            };
            (bg, fg, d)
        }
    };
    c.fill(r, 8.0, bg);
    c.icon(d, (r.cx(), r.cy()), 16.0, 2.0, fg, 0.0);
}

fn lock_badge(c: &mut Canvas<'_>, tile: Rect) {
    let (x, y) = (tile.x + tile.w + 2.0 - 11.0, tile.y + tile.h + 2.0 - 11.0);
    c.fill(Rect::new(x + 2.5, y + 5.5, 6.0, 5.0), 1.0, CARD);
    c.ring(Rect::new(x + 2.5, y + 5.5, 6.0, 5.0), 1.0, 1.1, T2);
    c.icon("M8 11V8a4 4 0 0 1 8 0v3", (x + 5.5, y + 3.7), 11.0 * 0.58, 2.2, T2, 0.0);
}

/// A spinning ring: a faint circle and an accent quarter of it, once round in 0.8 s.
fn spinner(c: &mut Canvas<'_>, f: &mut Frame<'_>, (cx, cy): (f32, f32), d: f32, col: Rgba) {
    let (r, w) = (d * 6.0 / 16.0, d * 2.4 / 16.0);
    c.arc((cx, cy), r, 0.0, 360.0, w, Rgba::css(127, 127, 127, 0.3));
    c.arc((cx, cy), r, -90.0 + (f.now % 800) as f32 / 800.0 * 360.0, 90.0, w, col);
    f.due(f.now + 16);
}

fn paint_hero(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, h: &Hero) {
    let t = Rect::new(r.x + 18.0, r.y + 18.0, 48.0, 48.0);
    let (bg, fg) = match h.tone {
        Tone::Good => (Rgba::css(76, 195, 138, 0.16), GREEN),
        Tone::Bad => (Rgba::css(229, 72, 77, 0.14), RED),
        _ => (acc_at(0.18), acc().accent_text()),
    };
    c.fill(t, 12.0, bg);
    c.icon(h.d, (t.cx(), t.cy()), 24.0, 1.9, fg, 0.0);
    let mut right = r.x + r.w - 18.0;
    if let Some((label, _)) = &h.act {
        let bw = c.measure(label, BODY_M).ceil() + 24.0;
        let b = Rect::new(right - bw, r.cy() - 15.0, bw, 30.0);
        let hot = f.focusable(ID_HERO, b);
        f.hand();
        c.transformed(b.cx(), b.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
            c.fill(b, 8.0, acc().accent_fill());
            c.fill(b, 8.0, Rgba::pure_white(0.1 * hot.hover));
            c.text(label, b, BODY_M, Rgba::pure_white(1.0), Align::Center);
        });
        right = b.x - 16.0;
    }
    let tx = t.x + 48.0 + 16.0;
    c.text_fit(&h.title, Rect::new(tx, r.cy() - 19.5, right - tx, 20.0), Font::new(17.0, 600).tracking(-0.01), T1, Align::Start);
    c.text_fit(&h.sub, Rect::new(tx, r.cy() + 3.5, right - tx, 16.0), BODY, T3, Align::Start);
}

/// The battery card's words: (the big label, the line under it, whether it is low).
#[must_use]
pub fn battery_words(b: &Battery) -> (String, String, bool) {
    let low = b.pct <= 20 && !b.ac;
    let left = b.left.map(|s| {
        let (h, m) = (s / 3600, s % 3600 / 60);
        if h > 0 { tf!("около {} ч {} мин", "about {} h {} min", h, m) } else { tf!("около {} мин", "about {} min", m.max(1)) }
    });
    let sub = if b.ac {
        if b.charging { t!("От сети · идёт зарядка", "Plugged in \u{b7} charging").to_string() } else { t!("От сети", "Plugged in").to_string() }
    } else {
        let mut s = t!("От батареи", "On battery").to_string();
        if let Some(l) = left {
            s = format!("{s} \u{b7} {}", tf!("осталось {}", "{} left", l));
        }
        if low {
            s.push_str(t!(" · скоро подключите питание", " \u{b7} plug in soon"));
        }
        s
    };
    (format!("{}%", b.pct), sub, low)
}

fn paint_battery(c: &mut Canvas<'_>, r: Rect, b: Option<&Battery>) {
    let ic = Rect::new(r.x + 18.0, r.cy() - 20.0, 86.0, 40.0);
    let tx = if b.is_some() { ic.x + 86.0 + 18.0 } else { r.x + 18.0 + 48.0 + 18.0 };
    match b {
        Some(b) => {
            let (label, sub, low) = battery_words(b);
            c.ring(ic, 9.0, 2.0, ink(0.28));
            let fill = Rect::new(ic.x + 4.0, ic.y + 4.0, (86.0 - 8.0) * f32::from(b.pct) / 100.0, 32.0);
            c.fill(fill, 5.0, if low { Rgba::hex(0xffa348) } else { GREEN });
            c.fill(Rect::new(ic.x + 88.0, ic.y + 13.0, 4.0, 14.0), 2.0, ink(0.28));
            if b.charging {
                let bc = (ic.x + 35.0 + 8.0, ic.y + 12.0 + 8.0);
                c.glyph("M13 2L5 13h6l-1 9 8-12h-6z", bc, 16.0, Rgba::pure_white(1.0));
                c.icon("M13 2L5 13h6l-1 9 8-12h-6z", bc, 16.0, 1.0, Rgba::css(0, 0, 0, 0.4), 0.0);
            }
            c.text(&label, Rect::new(tx, r.cy() - 22.5, 500.0, 26.0), Font::new(22.0, 600).tracking(-0.01), if low { AMBER } else { T1 }, Align::Start);
            c.text_fit(&sub, Rect::new(tx, r.cy() + 3.5, r.x + r.w - 18.0 - tx, 16.0), BODY, T3, Align::Start);
        }
        None => {
            let t = Rect::new(r.x + 18.0, r.cy() - 24.0, 48.0, 48.0);
            c.fill(t, 12.0, ink(0.06));
            c.icon(look::PLUG, (t.cx(), t.cy()), 24.0, 1.8, T2, 0.0);
            c.text(t!("От сети", "Plugged in"), Rect::new(tx, r.cy() - 22.5, 500.0, 26.0), Font::new(22.0, 600).tracking(-0.01), T1, Align::Start);
            c.text_fit(t!("Этот ПК работает от сети и не имеет батареи", "This PC runs on mains power and has no battery"), Rect::new(tx, r.cy() + 3.5, r.x + r.w - 18.0 - tx, 16.0), BODY, T3, Align::Start);
        }
    }
}

fn hue(h: Hue) -> Rgba {
    match h {
        Hue::Accent => acc(),
        Hue::Soft => acc_at(0.45),
        Hue::Yellow => Rgba::hex(0xf2cc6b),
    }
}

fn paint_drives(c: &mut Canvas<'_>, r: Rect, drives: &[Drive]) {
    let mut y = r.y;
    for (i, d) in drives.iter().enumerate() {
        if i > 0 {
            c.fill(Rect::new(r.x, y, r.w, 1.0), 0.0, ink(0.06));
            y += 1.0;
        }
        let x = r.x + 16.0;
        let w = r.w - 32.0;
        c.icon(look::DRIVE, (x + 8.0, y + 12.0 + 8.0), 16.0, 1.9, T2, 0.0);
        let nw = c.measure(&d.name, BODY_M).ceil();
        c.text(&d.name, Rect::new(x + 26.0, y + 12.0, nw, 16.0), BODY_M, T1, Align::Start);
        c.text(&d.txt, Rect::new(x, y + 12.0 + 0.5, w, 15.0), SMALL.tabular(), T3, Align::End);
        let bar = Rect::new(x, y + 12.0 + 16.0 + 8.0, w, 6.0);
        c.fill(bar, 3.0, ink(0.08));
        c.clipped(bar, |c| {
            let mut px = bar.x;
            for (share, h, _) in &d.parts {
                let pw = bar.w * share.clamp(0.0, 1.0);
                c.fill(Rect::new(px, bar.y, pw, 6.0), 0.0, hue(*h));
                px += pw;
            }
        });
        let mut lx = x;
        for (_, h, text) in &d.parts {
            c.fill(Rect::new(lx, bar.y + 6.0 + 8.0 + 3.5, 7.0, 7.0), 2.0, hue(*h));
            let tw = c.measure(text, Font::new(11.5, 400)).ceil();
            c.text(text, Rect::new(lx + 12.0, bar.y + 6.0 + 8.0, tw, 14.0), Font::new(11.5, 400), T3, Align::Start);
            lx += 12.0 + tw + 14.0;
        }
        y += 76.0;
    }
}

fn paint_walls(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, walls: &[crate::Wall], pics: &mut Vec<(usize, Picture)>) {
    let mut x = r.x + 14.0;
    let y = r.y + 14.0;
    for (i, w) in walls.iter().enumerate() {
        let b = Rect::new(x, y, 92.0, 60.0);
        let hot = f.item(ID_WALL + i as Id, b);
        f.hand();
        let lift = f.tween(ID_WALL + i as Id, 13, hot.hover, 200, SPRING);
        let b = b.shift(0.0, -2.0 * lift);
        // the real wallpaper's thumbnail is made into a picture once (again after a lost device)
        let pic = w.pic.as_ref().and_then(|t| {
            let key = std::sync::Arc::as_ptr(t) as usize;
            let at = pics.iter().position(|(k, p)| *k == key && c.is_current(p));
            at.or_else(|| {
                pics.retain(|(k, _)| *k != key);
                c.pixels(&t.px, t.w as i32, t.h as i32).map(|p| {
                    pics.push((key, p));
                    pics.len() - 1
                })
            })
        });
        match w.fill {
            _ if pic.is_some() => {
                let p = &pics[pic.unwrap_or(0)].1;
                c.masked(b, 9.0, Xf::NONE, |c| c.picture_in(p, b, 0.0, 1.0, Sample::Smooth));
            }
            Fill::Solid(col) => c.fill(b, 9.0, look::rgb(col)),
            Fill::Gradient(a, m, z, at) => c.gradient_angle(b, 9.0, 160.0, &[(0.0, look::rgb(a)), (at, look::rgb(m)), (1.0, look::rgb(z))]),
        }
        if w.on {
            c.ring(b.inset(-2.0), 11.0, 2.0, CARD);
            c.ring(b.inset(-4.0), 13.0, 2.0, acc());
        } else {
            c.ring(b, 9.0, 1.0, ink(0.1));
        }
        x += 102.0;
    }
    let b = Rect::new(x, y, 92.0, 60.0);
    let hot = f.item(ID_WALL + walls.len() as Id, b);
    f.hand();
    let lift = f.tween(ID_WALL + walls.len() as Id, 13, hot.hover, 200, SPRING);
    let b = b.shift(0.0, -2.0 * lift);
    c.fill(b, 9.0, ink(0.05));
    c.ring(b, 9.0, 1.0, ink(0.14));
    c.icon(look::PLUS, (b.cx(), b.cy() - 8.0 - 2.0 + 2.0), 16.0, 2.0, T2, 0.0);
    c.text(t!("Выбрать…", "Choose\u{2026}"), Rect::new(b.x, b.cy() + 4.0, b.w, 14.0), Font::new(11.5, 400), T2, Align::Center);
}

/// The Bluetooth radar: two rings growing out of a dot, one half a period behind the other.
fn paint_radar(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, _on: bool) {
    c.fill(Rect::new(r.x, r.y - 1.0, r.w, 1.0), 0.0, ink(0.06));
    let (cx, cy) = (r.x + 16.0 + 15.0, r.cy());
    for k in 0..2 {
        let t = ((f.now + 1600 - 800 * k) % 1600) as f32 / 1600.0;
        let e = 1.0 - (1.0 - t).powi(3);
        let (s, op) = (0.4 + 1.8 * e, 0.8 * (1.0 - e));
        let rr = 8.0 * s;
        c.ring(Rect::new(cx - rr, cy - rr, 2.0 * rr, 2.0 * rr), rr, 2.0, acc().fade(op));
    }
    c.circle(cx, cy, 4.0, acc());
    f.due(f.now + 16);
    c.text(t!("Ищем устройства поблизости. Переведите своё в режим сопряжения.", "Looking for devices nearby. Put yours in pairing mode."), Rect::new(r.x + 16.0 + 30.0 + 14.0, r.y, r.w - 80.0, r.h), BODY, T2, Align::Start);
}
