//! Painting: the caption with the tabs, the toolbar, the sidebar, the table (or the tiles), the status line, the preview, the empty states.
//! Every size is the design's (Explorer.v2.html), in DIPs.
use std::{collections::HashMap, path::Path};

use winrust_common::{t, tf};
use winrust_ui::{
    anim::EASE,
    frame::{Frame, Id},
    gfx::{Align, Canvas, Font, Picture, Rect, Rgba, Xf},
    widgets::{
        caption::window_buttons,
        field::{FieldStyle, field_with},
        list::{ListLook, columns, grid, list_grouped},
        scroll::scroll,
    },
};

use super::{
    backend::DriveInfo,
    look::{self, BAR, BODY, BODY_M, DriveKind, IconKind, SIDE, SMALL, T1, T2, T3, T4, WELL, WIN, accent, ink},
    model::*,
    text,
};
use crate::Row;

pub const CAPTION_H: f32 = 46.0;
pub const TOOLBAR_H: f32 = 52.0;
/// The toolbar is 52 high and has a hairline under it (1): the body starts at 99.
pub const TOP: f32 = CAPTION_H + TOOLBAR_H + 1.0;
pub const SIDE_W: f32 = 216.0;
/// The preview pane: 264 wide and a hairline (1) on its left.
pub const PV_W: f32 = 265.0;
pub const PV_INNER: f32 = 264.0;
/// The table's header is 34 high and a hairline (1) under it; the status line 30 and a hairline (1) over it.
pub const HEAD_H: f32 = 35.0;
pub const STATUS_H: f32 = 31.0;
pub const ROW_H: f32 = 36.0;
pub const GROUP_H: f32 = 32.0;
pub const INFO_H: f32 = 37.0;

const SEARCH_IC: &str = "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4";
const CHEVRON: &str = "M9 6l6 6-6 6";
const CLOSE_X: &str = "M6 6l12 12M18 6L6 18";
const PLUS: &str = "M12 5v14M5 12h14";

/// The sidebar's stroke icon of a place.
fn place_glyph(k: IconKind) -> &'static str {
    use look::Special::*;
    match k {
        IconKind::Home => "M4 11l8-7 8 7v9h-5v-6H9v6H4z",
        IconKind::Folder(Some(Desktop)) => "M3 5h18v11H3zM8 20h8M12 16v4",
        IconKind::Folder(Some(Downloads)) => "M12 4v11M7 10.5l5 5 5-5M5 20h14",
        IconKind::Folder(Some(Documents)) => "M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h5",
        IconKind::Folder(Some(Pictures)) => "M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15.5 9.5h.01",
        IconKind::Folder(Some(Music)) => "M9 18V6l10-2v12M9 18a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0zM19 16a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z",
        IconKind::Folder(Some(Videos)) => "M4 6h12v12H4zM16 10l5-3v10l-5-3",
        _ => "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    }
}

fn drive_glyph(k: DriveKind) -> &'static str {
    match k {
        DriveKind::Ssd | DriveKind::Hdd => "M3 14h18v5H3zM3 14l2.5-8h13L21 14M17 16.5h.01",
        DriveKind::Usb => "M9 3h6v5H9zM7 8h10v11a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2zM10.5 5.5h.01M13.5 5.5h.01",
        DriveKind::Net => "M4 4h16v8H4zM12 12v4M5 20h14M12 16a2 2 0 1 0 0 4 2 2 0 0 0 0-4z",
    }
}

fn kind_text(k: DriveKind) -> String {
    match k {
        DriveKind::Ssd => t!("Локальный диск · SSD", "Local disk \u{00B7} SSD", ja: "ローカル ディスク · SSD", pt: "Disco local · SSD", zh: "本地磁盘 · SSD").into(),
        DriveKind::Hdd => t!("Локальный диск · HDD", "Local disk \u{00B7} HDD", ja: "ローカル ディスク · HDD", pt: "Disco local · HDD", zh: "本地磁盘 · HDD").into(),
        DriveKind::Usb => t!("USB-накопитель", "USB drive", ja: "USB ドライブ", pt: "Unidade USB", zh: "USB 驱动器").into(),
        DriveKind::Net => t!("Сетевой диск", "Network drive", ja: "ネットワーク ドライブ", pt: "Unidade de rede", zh: "网络驱动器").into(),
    }
}

/// A back-and-front icon without its glyph (the tabs and the address pill).
fn plates_icon(c: &mut Canvas<'_>, kind: IconKind, centre: (f32, f32), size: f32) {
    let p = look::plates(kind);
    c.glyph(p.back, centre, size, p.c1);
    c.glyph(p.front, centre, size, p.c2);
}

/// An icon of a row from what it is.
fn row_kind(rows: &[Row], drives: &[DriveInfo], places: &super::backend::Places, dir: &Path, pc: bool, i: usize) -> IconKind {
    let Some(r) = rows.get(i) else { return IconKind::File("") };
    if pc {
        return drives.get(i).map_or(IconKind::Folder(None), |d| IconKind::Drive(d.kind));
    }
    if r.dir {
        let p = dir.join(&r.name);
        let special = places.quick.iter().find(|q| q.path == p).and_then(|q| match q.icon {
            IconKind::Folder(s) => s,
            _ => None,
        });
        return IconKind::Folder(special);
    }
    IconKind::File(look::ext_of(&r.name))
}

/// The one picture of a thumbnail, made on this device (again after the device was lost).
fn picture_of<'a>(c: &mut Canvas<'_>, thumbs: &'a mut HashMap<std::path::PathBuf, Thumb>, p: &Path) -> Option<&'a Picture> {
    let t = thumbs.get_mut(p)?;
    if t.pic.as_ref().is_none_or(|pic| c.picture_stale(pic)) {
        t.pic = c.pixels(&t.px, t.w, t.h);
    }
    t.pic.as_ref()
}

/// A thumbnail in the box `r`: a photograph covers it with rounded corners and a hairline (`object-fit: cover`); the icon of a program
/// is drawn whole, as it is.
fn show(c: &mut Canvas<'_>, pic: &Picture, icon: bool, r: Rect, radius: f32) {
    if icon {
        c.picture_in(pic, r, 0.0, 1.0, winrust_ui::gfx::Sample::Smooth);
    } else {
        cover(c, pic, r, radius);
        c.ring(r, radius, 1.0, ink(0.12));
    }
}

/// A picture covering `r` with rounded corners (`object-fit: cover`).
fn cover(c: &mut Canvas<'_>, pic: &Picture, r: Rect, radius: f32) {
    let k = (r.w / pic.w).max(r.h / pic.h);
    let (w, h) = (pic.w * k, pic.h * k);
    let dst = Rect::new(r.cx() - w / 2.0, r.cy() - h / 2.0, w, h);
    c.masked(r, radius, Xf::NONE, |c| c.picture_in(pic, dst, 0.0, 1.0, winrust_ui::gfx::Sample::Smooth));
}

/// What a toolbar button looks like: its icon, hint and ink, and the plate it keeps while it is on.
struct Tool {
    icon: &'static str,
    size: f32,
    stroke: f32,
    tip: &'static str,
    ink: Rgba,
    plate: f32,
}

impl Fm {
    /// One frame.
    pub fn paint(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        self.geo.tabs.clear();
        self.geo.crumbs.clear();
        self.geo.places.clear();
        self.geo.rows.clear();
        self.geo.size = (w, h);
        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, WIN);

        // the preview pane opens and closes by width (its content keeps its size and is cut)
        let pv_t = f.tween(ID_PV_BODY, 0, f32::from(self.pv && w >= 700.0), 320, EASE);
        let pv_w = (PV_W * pv_t).round();
        let body_h = h - TOP;
        let content = Rect::new(SIDE_W, TOP, (w - SIDE_W - pv_w).max(100.0), body_h);
        self.geo.content = content;
        self.content(c, f, content);
        if pv_w > 0.5 {
            self.preview(c, f, Rect::new(w - pv_w, TOP, pv_w, body_h));
        }
        self.sidebar(c, f, Rect::new(0.0, TOP, SIDE_W, body_h));
        self.caption(c, f, w);
        self.toolbar(c, f, w);
        self.demo_ids(c, f);
    }

    /// The invisible item a render clicks to open the menu a state of the design shows.
    fn demo_ids(&mut self, _c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        if self.hook != Hook::None {
            f.item(ID_DEMO, Rect::new(0.0, 0.0, 1.0, 1.0));
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the caption: tabs

    fn caption(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        let bar = Rect::new(0.0, 0.0, w, CAPTION_H);
        c.fill(bar, 0.0, BAR);
        let btns_x = window_buttons(c, f, bar);
        let x0 = 10.0;
        let avail = (btns_x - 8.0 - x0 - 30.0 - 2.0).max(60.0);
        let n = self.tabs.len();
        let labels: Vec<String> = self.tabs.iter().enumerate().map(|(i, p)| if i == self.active { self.label_of(&self.dir.clone()) } else { self.label_of(p) }).collect();
        let kinds: Vec<IconKind> = self.tabs.iter().enumerate().map(|(i, p)| if i == self.active { self.icon_of_dir(&self.dir.clone()) } else { self.icon_of_dir(p) }).collect();
        let natural: Vec<f32> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let font = Font::new(13.0, if i == self.active { 600 } else { 500 });
                (10.0 + 16.0 + 8.0 + c.measure(l, font) + 8.0 + 22.0 + 4.0).min(220.0)
            })
            .collect();
        let total: f32 = natural.iter().sum::<f32>() + 2.0 * n.saturating_sub(1) as f32;
        let k = if total > avail { (avail - 2.0 * n.saturating_sub(1) as f32) / natural.iter().sum::<f32>() } else { 1.0 };
        let mut x = x0;
        for i in 0..n {
            let tw = (natural[i] * k).max(44.0);
            let r = Rect::new(x, 7.0, tw, 32.0);
            self.geo.tabs.push(r);
            let on = i == self.active;
            let hot = f.item(ID_TAB + i as Id, r);
            let xr = Rect::new(r.x + r.w - 4.0 - 22.0, r.cy() - 11.0, 22.0, 22.0);
            let xhot = f.item(ID_TAB_X + i as Id, xr);
            f.hand();
            let hv = hot.hover.max(xhot.hover);
            let bg = if on { ink(0.10) } else { ink(0.06 * hv) };
            c.fill(r, 9.0, bg);
            let dragging = self.tab_drag == Some(i);
            if dragging {
                c.ring(r, 9.0, 1.0, accent().alpha(0.5));
            }
            plates_icon(c, kinds[i], (r.x + 10.0 + 8.0, r.cy()), 16.0);
            let label_w = (tw - 10.0 - 16.0 - 8.0 - 8.0 - 22.0 - 4.0).max(0.0);
            let font = Font::new(13.0, if on { 600 } else { 500 });
            c.text_fit(&labels[i], Rect::new(r.x + 34.0, r.y, label_w, r.h), font, if on { T1 } else { T2 }, Align::Start);
            // the close button shows on the tab that is open and under the pointer
            let show = if on { 1.0 } else { hv };
            if show > 0.0 {
                c.fill(xr, 6.0, ink(0.12 * xhot.hover * show));
                c.icon(CLOSE_X, (xr.cx(), xr.cy()), 10.0, 2.8, T3.fade(show), 0.0);
            }
            if hot.focus {
                c.ring(r.inset(-1.0), 10.0, 2.0, look_ring());
            }
            f.tip_hint(&labels[i]);
            x += tw + 2.0;
        }
        let r = Rect::new(x, 8.0, 30.0, 30.0);
        let hot = f.item(ID_NEWTAB, r);
        f.hand();
        f.tip(t!("Новая вкладка (Ctrl+T)", "New tab (Ctrl+T)", ja: "新しいタブ (Ctrl+T)", pt: "Nova guia (Ctrl+T)", zh: "新标签页 (Ctrl+T)"));
        c.transformed(r.cx(), r.cy(), 1.0 - 0.06 * hot.press, 0.0, 0.0, |c| {
            c.fill(r, 8.0, ink(0.08 * hot.hover));
            c.icon(PLUS, (r.cx(), r.cy()), 14.0, 2.4, T2, 0.0);
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the toolbar

    /// A square toolbar button.
    fn tool_button(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, t: Tool) {
        let hot = f.item(id, r);
        f.hand();
        f.tip(t.tip);
        c.transformed(r.cx(), r.cy(), 1.0 - 0.06 * hot.press, 0.0, 0.0, |c| {
            c.fill(r, 9.0, ink(t.plate.max(0.08 * hot.hover)));
            c.icon(t.icon, (r.cx(), r.cy()), t.size, t.stroke, t.ink, 0.0);
        });
        if hot.focus {
            c.ring(r.inset(-1.0), 10.0, 2.0, look_ring());
        }
    }

    fn toolbar(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        let bar = Rect::new(0.0, CAPTION_H, w, TOOLBAR_H);
        c.fill(bar, 0.0, BAR);
        c.fill(Rect::new(0.0, CAPTION_H + TOOLBAR_H - 1.0, w, 1.0), 0.0, Rgba::css(0, 0, 0, 0.35));
        let y = CAPTION_H + (TOOLBAR_H - 34.0) / 2.0;
        // the three arrows
        let nav = [
            (ID_BACK, "M15 5l-7 7 7 7", self.can_back(), t!("Назад (Alt+←)", "Back (Alt+Left)", ja: "戻る (Alt+←)", pt: "Voltar (Alt+←)", zh: "后退 (Alt+←)")),
            (ID_FWD, "M9 5l7 7-7 7", self.can_forward(), t!("Вперёд (Alt+→)", "Forward (Alt+Right)", ja: "進む (Alt+→)", pt: "Avançar (Alt+→)", zh: "前进 (Alt+→)")),
            (ID_UP, "M12 19V5M6 11l6-6 6 6", self.can_up(), t!("Вверх (Alt+↑)", "Up one level (Alt+Up)", ja: "上へ (Alt+↑)", pt: "Subir (Alt+↑)", zh: "上一级 (Alt+↑)")),
        ];
        for (i, (id, d, on, tip)) in nav.into_iter().enumerate() {
            let r = Rect::new(10.0 + i as f32 * 38.0, y, 34.0, 34.0);
            Self::tool_button(c, f, id, r, Tool { icon: d, size: 17.0, stroke: 2.0, tip, ink: if on { T1 } else { T4 }, plate: 0.0 });
        }
        // the right end, from the right: More, the preview, the view switch, the search box
        let more = Rect::new(w - 10.0 - 34.0, y, 34.0, 34.0);
        self.geo.more = more;
        Self::tool_button(c, f, ID_MORE, more, Tool { icon: "M6 12h.01M12 12h.01M18 12h.01", size: 18.0, stroke: 3.0, tip: t!("Сортировка, группировка и другое", "Sort, group and more", ja: "並べ替え、グループ化など", pt: "Classificar, agrupar e mais", zh: "排序、分组等"), ink: T2, plate: 0.0 });
        let pv = Rect::new(more.x - 4.0 - 34.0, y, 34.0, 34.0);
        Self::tool_button(c, f, ID_PV, pv, Tool { icon: "M4 5h16v14H4zM14 5v14", size: 17.0, stroke: 2.0, tip: t!("Панель просмотра (Пробел)", "Preview pane (Space)", ja: "プレビュー ウィンドウ (Space)", pt: "Painel de visualização (Espaço)", zh: "预览窗格 (空格)"), ink: if self.pv { T1 } else { T2 }, plate: if self.pv { 0.1 } else { 0.0 } });
        let seg = Rect::new(pv.x - 8.0 - 68.0, y, 68.0, 34.0);
        c.fill(seg, 10.0, WELL);
        let views = [
            (View::List, "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01", t!("Список", "List", ja: "リスト", pt: "Lista", zh: "列表")),
            (View::Icons, "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z", t!("Крупные значки (Ctrl+M)", "Large icons (Ctrl+M)", ja: "大きいアイコン (Ctrl+M)", pt: "Ícones grandes (Ctrl+M)", zh: "大图标 (Ctrl+M)")),
        ];
        for (i, (v, d, tip)) in views.into_iter().enumerate() {
            let b = Rect::new(seg.x + 2.0 + i as f32 * 32.0, y + 2.0, 32.0, 30.0);
            f.item(ID_VIEW + i as Id, b);
            f.hand();
            f.tip(tip);
            let on = f.tween(ID_VIEW + i as Id, 2, f32::from(self.view == v), 200, EASE);
            c.fill(b, 8.0, ink(0.12 * on));
            c.icon(d, (b.cx(), b.cy()), 16.0, 2.0, T3.mix(T1, on), 0.0);
        }
        // the search box grows when it has the focus or text
        let focused = f.focused(ID_SEARCH);
        let sw = f.tween(ID_SEARCH, 7, if focused || !self.srch.text.is_empty() { 300.0 } else { 220.0 }, 300, EASE);
        let sr = Rect::new(seg.x - 6.0 - 4.0 - sw, y, sw, 34.0);
        let ph = tf!("Поиск: {}", "Search {}", ja: "{} を検索", pt: "Pesquisar {}", zh: "搜索 {}", self.label_of(&self.dir));
        field_with(c, f, ID_SEARCH, sr, &mut self.srch, &ph, &FieldStyle::SEARCH);
        // the address pill takes the rest
        let ax = 10.0 + 3.0 * 38.0 + 6.0;
        let ar = Rect::new(ax, y, (sr.x - 4.0 - 6.0 - ax).max(80.0), 34.0);
        self.address(c, f, ar);
    }

    fn address(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        let pill = f.item(ID_ADDR_PILL, r);
        f.cursor(winrust_ui::frame::Cursor::IBeam);
        let editing = self.addr_edit;
        c.fill(r, 10.0, WELL);
        let ring = if editing { accent().alpha(0.70) } else { ink(0.06) };
        c.ring(r, 10.0, 1.0, ring);
        let _ = pill;
        if editing {
            let inner = Rect::new(r.x + 6.0, r.y, r.w - 12.0, r.h);
            let style = FieldStyle { font: Font::new(12.5, 400), bare: true, pad: 6.0, ..FieldStyle::INPUT };
            if self.addr_bad {
                let shake = ((f.now / 40) % 2) as f32 * 4.0 - 2.0;
                c.transformed(inner.cx(), inner.cy(), 1.0, shake, 0.0, |c| {
                    field_with(c, f, ID_ADDR, inner, &mut self.addr, "", &style);
                });
            } else {
                field_with(c, f, ID_ADDR, inner, &mut self.addr, "", &style);
            }
            return;
        }
        // the folder's icon, then the parts of the path
        let inner = Rect::new(r.x + 6.0, r.y, r.w - 12.0, r.h);
        let kind = self.icon_of_dir(&self.dir.clone());
        plates_icon(c, kind, (inner.x + 2.0 + 7.5, r.cy()), 15.0);
        let parts = self.crumb_list();
        let font_of = |last: bool| Font::new(13.0, if last { 600 } else { 500 });
        let widths: Vec<f32> = parts.iter().enumerate().map(|(i, (l, _))| (c.measure(l, font_of(i + 1 == parts.len())).ceil() + 14.0).min(180.0)).collect();
        // from the right: as many parts as fit, the rest folded into "…" that goes to the first one left out
        let start_x = inner.x + 2.0 + 15.0 + 4.0 + 2.0;
        let room = inner.x + inner.w - start_x;
        let mut used = 0.0;
        let mut first = parts.len();
        for i in (0..parts.len()).rev() {
            let need = widths[i] + if i + 1 < parts.len() { 2.0 + 12.0 + 2.0 } else { 0.0 };
            if used + need > room - if i > 0 { 28.0 } else { 0.0 } && first < parts.len() {
                break;
            }
            used += need;
            first = i;
        }
        let mut x = start_x;
        c.clipped(inner, |c| {
            if first > 0 {
                let b = Rect::new(x, r.cy() - 13.0, 26.0, 26.0);
                f.item(ID_CRUMB + first as Id - 1, b);
                f.hand();
                c.text("\u{2026}", b, BODY_M, T2, Align::Center);
                x += 26.0 + 2.0 + 12.0 + 2.0;
                c.icon(CHEVRON, (x - 2.0 - 6.0, r.cy()), 12.0, 2.4, T4, 0.0);
                self.geo.crumbs.push((ID_CRUMB + first as Id - 1, b, parts[first - 1].1.clone()));
            }
            for i in first..parts.len() {
                let last = i + 1 == parts.len();
                let b = Rect::new(x, r.cy() - 13.0, widths[i], 26.0);
                let hot = f.item(ID_CRUMB + i as Id, b);
                if !last {
                    f.hand();
                }
                let drop = matches!(&self.drop_hl, Some(DropAt::Place(p)) if *p == parts[i].1) && !last;
                if drop {
                    c.fill(b, 7.0, accent().alpha(0.14));
                    c.ring(b, 7.0, 1.5, accent());
                } else if !last {
                    c.fill(b, 7.0, ink(0.08 * hot.hover));
                }
                let col = if last { T1 } else { T2.mix(T1, hot.hover) };
                c.text_fit(&parts[i].0, Rect::new(b.x + 7.0, b.y, b.w - 14.0, b.h), font_of(last), col, Align::Start);
                self.geo.crumbs.push((ID_CRUMB + i as Id, b, parts[i].1.clone()));
                x += widths[i];
                if !last {
                    c.icon(CHEVRON, (x + 2.0 + 6.0, r.cy()), 12.0, 2.4, T4, 0.0);
                    x += 2.0 + 12.0 + 2.0;
                }
            }
        });
    }

    /// The parts of the path: This PC for a drive, then one part for each folder.
    fn crumb_list(&self) -> Vec<(String, std::path::PathBuf)> {
        let mut v = Vec::new();
        if self.at_pc() {
            v.push((self.label_of(&pc_dir()), pc_dir()));
            return v;
        }
        let mut acc = std::path::PathBuf::new();
        let mut first = true;
        for comp in self.dir.components() {
            acc.push(comp.as_os_str());
            match comp {
                std::path::Component::RootDir => {}
                std::path::Component::Prefix(p) => {
                    acc.push("\\");
                    if first {
                        v.push((self.label_of(&pc_dir()), pc_dir()));
                    }
                    let root = acc.clone();
                    let label = self.places.drives.iter().find(|d| d.root == root).map_or_else(|| p.as_os_str().to_string_lossy().into_owned(), |d| d.label.clone());
                    v.push((label, root));
                }
                _ => v.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone())),
            }
            first = false;
        }
        v
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the sidebar

    fn sidebar(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.fill(r, 0.0, SIDE);
        c.fill(Rect::new(r.x + r.w - 1.0, r.y, 1.0, r.h), 0.0, ink(0.05));
        let (np, nd) = (self.places.quick.len(), self.places.drives.len());
        let content_h = 10.0 + 28.0 + np as f32 * 33.0 + 10.0 + 33.0 + nd as f32 * 54.0 + 10.0;
        let mut side = std::mem::take(&mut self.side);
        let pc_cur = self.at_pc() && !self.searching();
        let (dir, searching) = (self.dir.clone(), self.searching());
        let hl = self.drop_hl.clone();
        let mut places_geo: Vec<(Rect, std::path::PathBuf)> = Vec::new();
        scroll(c, f, ID_SIDE, r, &mut side, content_h, |c, f, v| {
            let x = r.x + 8.0;
            let wd = r.w - 16.0 - 1.0;
            let mut y = 10.0;
            c.text(t!("Быстрый доступ", "Quick access", ja: "クイック アクセス", pt: "Acesso rápido", zh: "快速访问"), Rect::new(x + 10.0, v.at(y), wd - 20.0, 28.0), Font::new(12.0, 500), T3, Align::Start);
            y += 28.0;
            for (i, p) in self.places.quick.iter().enumerate() {
                let b = Rect::new(x, v.at(y), wd, 32.0);
                y += 33.0;
                let cur = p.path == dir && !searching;
                let hot = f.item(ID_PLACE + i as Id, b);
                f.hand();
                let drop = matches!(&hl, Some(DropAt::Place(q)) if *q == p.path);
                plate(c, b, cur, hot.hover, drop);
                let (col, icol) = (if cur { T1 } else { T2 }, if cur { accent() } else { T3 });
                c.icon(place_glyph(p.icon), (b.x + 10.0 + 8.0, b.cy()), 16.0, 1.9, icol, 0.0);
                c.text_fit(&p.title, Rect::new(b.x + 10.0 + 16.0 + 10.0, b.y, b.w - 46.0 - 10.0, b.h), BODY_M, col, Align::Start);
                places_geo.push((b, p.path.clone()));
            }
            y += 10.0;
            let b = Rect::new(x, v.at(y), wd, 32.0);
            y += 33.0;
            let hot = f.item(ID_PC, b);
            f.hand();
            plate(c, b, pc_cur, hot.hover, false);
            c.icon("M3 4h18v12H3zM8 20h8M12 16v4", (b.x + 10.0 + 8.0, b.cy()), 16.0, 1.9, if pc_cur { accent() } else { T3 }, 0.0);
            c.text_fit(&self.label_of(&pc_dir()), Rect::new(b.x + 36.0, b.y, b.w - 46.0, b.h), BODY_M, if pc_cur { T1 } else { T2 }, Align::Start);
            for (i, d) in self.places.drives.iter().enumerate() {
                let b = Rect::new(x, v.at(y), wd, 53.0);
                y += 54.0;
                let cur = dir == d.root && !searching;
                let hot = f.item(ID_DRIVE + i as Id, b);
                f.hand();
                f.tip(format!("{} \u{00B7} {}", tf!("{} из {}", "{} of {}", ja: "{} / {}", pt: "{} de {}", zh: "{} / {}", text::free_text(d.free), text::gb(d.total)), d.fs));
                let drop = matches!(&hl, Some(DropAt::Place(q)) if *q == d.root);
                plate(c, b, cur, hot.hover, drop);
                let icol = if cur { accent() } else { T3 };
                c.icon(drive_glyph(d.kind), (b.x + 10.0 + 8.0, b.y + 8.0 + 8.0), 16.0, 1.9, icol, 0.0);
                let name_w = b.w - 36.0 - 10.0 - if d.removable { 16.0 } else { 0.0 };
                c.text_fit(&d.label, Rect::new(b.x + 36.0, b.y + 7.0, name_w, 18.0), BODY_M, if cur { T1 } else { T2 }, Align::Start);
                if d.removable {
                    let eb = Rect::new(b.x + b.w - 10.0 - 22.0 + 6.0, b.y + 7.0 - 2.0, 22.0, 22.0);
                    let eh = f.item(ID_EJECT + i as Id, eb);
                    f.hand();
                    f.tip(t!("Извлечь", "Eject", ja: "取り出し", pt: "Ejetar", zh: "弹出"));
                    c.fill(eb, 6.0, ink(0.08 * eh.hover));
                    c.icon("M5 14h14L12 6zM5 18.5h14", (eb.cx(), eb.cy()), 13.0, 2.2, T2, 0.0);
                }
                // the bar of how full it is, and the free space
                let used = d.used_pct();
                let free = text::free_text(d.free);
                let fw = c.measure(&free, Font::new(11.5, 400).tabular()).ceil();
                let by = b.y + 7.0 + 18.0 + 6.0;
                let track = Rect::new(b.x + 36.0, by + 5.5, b.w - 36.0 - 10.0 - 8.0 - fw, 3.0);
                c.fill(track, 2.0, ink(0.10));
                let fill_w = f.tween(ID_DRIVE + i as Id, 8, track.w * used as f32 / 100.0, 500, EASE);
                c.fill(Rect::new(track.x, track.y, fill_w, 3.0), 2.0, look::bar_colour(used));
                c.text(&free, Rect::new(track.x + track.w + 8.0, by, fw + 2.0, 14.0), Font::new(11.5, 400).tabular(), look::free_colour(used, T3), Align::End);
                places_geo.push((b, d.root.clone()));
            }
        });
        self.side = side;
        self.geo.places = places_geo;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the content: header, rows or tiles, status, empty

    fn content(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        // a click on empty space is the content's (its menu, clearing the selection)
        f.item(ID_CONTENT, r);
        let mut y = r.y;
        if self.searching() {
            c.fill(Rect::new(r.x, y + INFO_H - 1.0, r.w, 1.0), 0.0, ink(0.05));
            c.icon(SEARCH_IC, (r.x + 20.0 + 6.5, y + INFO_H / 2.0), 13.0, 2.2, T3, 0.0);
            c.text_fit(&self.search_info, Rect::new(r.x + 20.0 + 13.0 + 8.0, y, r.w - 60.0, INFO_H), SMALL, T3, Align::Start);
            y += INFO_H;
        }
        let status_y = r.y + r.h - STATUS_H;
        let area = Rect::new(r.x, y, r.w, status_y - y);
        // what slides in when the folder changes
        if self.nav_fresh {
            self.nav_fresh = false;
            self.nav_at = f.now;
        }
        let age = f.now.saturating_sub(self.nav_at) as f32;
        let t = if self.nav_at == 0 { 1.0 } else { (age / 260.0).clamp(0.0, 1.0) };
        let e = EASE.ease(t);
        if t < 1.0 {
            f.due(f.now + 8);
        }
        let list_area = if self.view == View::List {
            let head = Rect::new(area.x, area.y, area.w, HEAD_H);
            columns(c, f, ID_COLS, head, &self.cols, 14.0);
            Rect::new(area.x, area.y + HEAD_H, area.w, area.h - HEAD_H)
        } else {
            area
        };
        let (dx, op) = (14.0 * (1.0 - e), e);
        c.faded(op, |c| {
            c.transformed(list_area.cx(), list_area.cy(), 1.0, dx, 0.0, |c| {
                if self.view == View::List {
                    self.rows_list(c, f, list_area);
                } else {
                    self.tiles(c, f, list_area);
                }
            });
        });
        if self.rows.is_empty() {
            // the box the mockup centres it in: under the header (34) and the search line (36), 30 above the window's bottom
            let top = r.y + if self.view == View::List { 34.0 } else { 0.0 } + if self.searching() { 36.0 } else { 0.0 };
            self.empty_state(c, f, Rect::new(r.x, top, r.w, r.y + r.h - 30.0 - top));
        }
        self.paint_status(c, f, Rect::new(r.x, status_y, r.w, STATUS_H));
    }

    fn rows_list(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        let mut list = std::mem::take(&mut self.list);
        self.geo.list = r;
        self.geo.rows_per_page = (((r.h - 16.0) / (ROW_H + 1.0)).floor() as usize).max(1);
        let lay = self.cols.layout(r.w - 28.0);
        let heads = self.headings();
        let head_info: Vec<(String, usize)> = self.heads.iter().map(|h| (h.label.clone(), h.count)).collect();
        let n = self.rows.len();
        let pc = self.at_pc();
        let hits = self.searching();
        let single = list.sel.len() == 1;
        let hl_row = match &self.drop_hl {
            Some(DropAt::Row(i, _)) => Some(*i),
            _ => None,
        };
        let clock = self.clock;
        let size_cols = self.cols.cols.len();
        let _ = size_cols;
        let mut want: Vec<usize> = Vec::new();
        let mut vis: Vec<(Rect, usize)> = Vec::new();
        let dir = self.dir.clone();
        {
            let Fm { rows, drives, places, thumbs, ren, .. } = self;
            let kinds = |i: usize| row_kind(rows, drives, places, &dir, pc, i);
            list_grouped(
                c,
                f,
                ID_LIST,
                r,
                &mut list,
                n,
                ROW_H,
                GROUP_H,
                &heads,
                &ListLook::ROWS,
                |c, _f, k, rect| {
                    let (label, count) = &head_info[k];
                    let font = Font::new(12.0, 600);
                    let lw = c.measure(label, font).ceil();
                    c.text(label, Rect::new(rect.x + 14.0, rect.y + 10.0, lw + 2.0, 15.0), font, T2, Align::Start);
                    c.text(&count.to_string(), Rect::new(rect.x + 14.0 + lw + 8.0, rect.y + 10.0, 60.0, 15.0), Font::new(12.0, 400), T3, Align::Start);
                },
                |c, f, i, rect, st| {
                    let row = &rows[i];
                    let x0 = rect.x + 6.0;
                    vis.push((rect, i));
                    if hl_row == Some(i) {
                        c.fill(rect, 8.0, accent().alpha(0.14));
                        c.ring(rect, 8.0, 1.5, accent());
                    } else if st.selected && single {
                        c.ring(rect, 8.0, 1.0, accent().alpha(0.45));
                    }
                    // the name cell: icon, name
                    let ic = (x0 + 8.0 + 11.0, rect.cy());
                    let path = dir.join(&row.name);
                    let mut shown = false;
                    if !pc && !row.dir {
                        if thumbs.contains_key(&path) {
                            let is_icon = thumbs.get(&path).is_some_and(|t| t.icon);
                            if let Some(pic) = picture_of(c, thumbs, &path) {
                                show(c, pic, is_icon, Rect::new(ic.0 - 11.0, ic.1 - 11.0, 22.0, 22.0), 4.0);
                                shown = true;
                            }
                        } else {
                            want.push(i);
                        }
                    }
                    if !shown {
                        look::icon(c, kinds(i), ic, 22.0, 1.6, false);
                    }
                    let name_x = x0 + 8.0 + 22.0 + 10.0;
                    let name_w = (lay[0].1 - 8.0 - 22.0 - 10.0 - 8.0).max(10.0);
                    let name = if pc {
                        drives.get(i).map_or_else(String::new, |d| d.label.clone())
                    } else if hits {
                        std::path::Path::new(&row.name).file_name().map_or_else(|| row.name.clone(), |n| n.to_string_lossy().into_owned())
                    } else {
                        row.name.clone()
                    };
                    let renaming = ren.as_ref().is_some_and(|rn| rn.name == row.name) && !pc && st.cursor;
                    if renaming {
                        if let Some(rn) = ren.as_mut() {
                            let b = Rect::new(name_x - 6.0, rect.cy() - 13.0, name_w + 6.0, 26.0);
                            let shake = if rn.bad > 0 { (((f.now / 40) % 2) as f32 * 8.0 - 4.0) * f32::from(f.now % 480 < 240) } else { 0.0 };
                            c.transformed(b.cx(), b.cy(), 1.0, shake, 0.0, |c| {
                                c.fill(b, 6.0, WELL);
                                c.ring(b, 6.0, 1.5, accent());
                                let st = FieldStyle { font: BODY_M, bare: true, pad: 6.0, ..FieldStyle::INPUT };
                                field_with(c, f, ID_REN, b, &mut rn.field, "", &st);
                            });
                        }
                    } else {
                        c.text_fit(&name, Rect::new(name_x, rect.y, name_w, rect.h), BODY_M, T1, Align::Start);
                    }
                    // the date (or the free space), the type, the size
                    let tab = BODY.tabular();
                    let (cx1, cw1) = (x0 + lay[1].0, lay[1].1);
                    if let Some(d) = drives.get(i).filter(|_| pc) {
                        let used = d.used_pct();
                        let free = text::free_text(d.free);
                        let fw = c.measure(&free, tab).ceil();
                        let track = Rect::new(cx1 + 8.0, rect.cy() - 2.0, (cw1 - 16.0 - 10.0 - fw).max(20.0), 4.0);
                        c.fill(track, 2.0, ink(0.10));
                        c.fill(Rect::new(track.x, track.y, track.w * used as f32 / 100.0, 4.0), 2.0, look::bar_colour(used));
                        c.text(&free, Rect::new(track.x + track.w + 10.0, rect.y, fw + 2.0, rect.h), tab, look::free_colour(used, T2), Align::Start);
                    } else {
                        c.text_fit(&text::date(row.time, clock), Rect::new(cx1 + 8.0, rect.y, cw1 - 16.0, rect.h), tab, T2, Align::Start);
                    }
                    let (cx2, cw2) = (x0 + lay[2].0, lay[2].1);
                    let third = if pc {
                        drives.get(i).map_or_else(String::new, |d| kind_text(d.kind))
                    } else if hits {
                        let p = std::path::Path::new(&row.name).parent().map(Path::to_path_buf).unwrap_or_default();
                        let nm = p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
                        format!("{nm} \u{00B7} {}", p.display())
                    } else {
                        text::kind(&row.name, row.dir)
                    };
                    c.text_fit(&third, Rect::new(cx2 + 8.0, rect.y, cw2 - 16.0, rect.h), BODY, T3, Align::Start);
                    let (cx3, cw3) = (x0 + lay[3].0, lay[3].1);
                    let size = if pc {
                        drives.get(i).map_or_else(String::new, |d| text::gb(d.total))
                    } else if row.dir && row.size == 0 {
                        String::new()
                    } else {
                        crate::size_text(row.size)
                    };
                    c.text_fit(&size, Rect::new(cx3 + 8.0, rect.y, cw3 - 16.0, rect.h), tab, T2, Align::End);
                },
            );
        }
        self.list = list;
        self.geo.rows = vis;
        for i in want {
            self.want_picture(i, 64);
        }
    }

    fn tiles(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        let mut list = std::mem::take(&mut self.list);
        self.geo.list = r;
        let n = self.rows.len();
        let pc = self.at_pc();
        let hits = self.searching();
        let dir = self.dir.clone();
        let hl_row = match &self.drop_hl {
            Some(DropAt::Row(i, _)) => Some(*i),
            _ => None,
        };
        let mut want: Vec<usize> = Vec::new();
        let mut vis: Vec<(Rect, usize)> = Vec::new();
        let single = list.sel.len() == 1;
        let cols = {
            let Fm { rows, drives, places, thumbs, ren, .. } = self;
            grid(c, f, ID_LIST, r, &mut list, n, 112.0, 124.0, |c, f, i, rect, st| {
                let row = &rows[i];
                vis.push((rect, i));
                if hl_row == Some(i) {
                    c.fill(rect, 12.0, accent().alpha(0.14));
                    c.ring(rect, 12.0, 1.5, accent());
                } else if st.selected && single {
                    c.ring(rect, 12.0, 1.0, accent().alpha(0.45));
                }
                // the icon lifts a little under the pointer
                let lift = -2.0 * st.hover;
                let path = dir.join(&row.name);
                let name = if pc {
                    drives.get(i).map_or_else(String::new, |d| d.label.clone())
                } else if hits {
                    std::path::Path::new(&row.name).file_name().map_or_else(|| row.name.clone(), |n| n.to_string_lossy().into_owned())
                } else {
                    row.name.clone()
                };
                let renaming = ren.as_ref().is_some_and(|rn| rn.name == row.name) && !pc && st.cursor;
                let nfont = Font::new(12.0, 500);
                let name_lines = if renaming { 1 } else { c.lines(&name, nfont, rect.w - 16.0).clamp(1, 2) };
                let has_pic = !pc && !row.dir && thumbs.contains_key(&path);
                let pic_h = if has_pic { 54.0 } else { 56.0 };
                let name_h = if renaming { 22.0 } else { name_lines as f32 * 15.6 };
                let block = pic_h + 8.0 + name_h + if pc { 8.0 + 3.0 } else { 0.0 };
                let top = rect.y + ((rect.h - block) / 2.0).max(4.0);
                let centre = (rect.cx(), top + 28.0 + lift);
                let mut shown = false;
                if !pc && !row.dir {
                    if thumbs.contains_key(&path) {
                        let is_icon = thumbs.get(&path).is_some_and(|t| t.icon);
                        if let Some(pic) = picture_of(c, thumbs, &path) {
                            if is_icon {
                                show(c, pic, true, Rect::new(rect.cx() - 28.0, top + lift, 56.0, 56.0), 0.0);
                            } else {
                                let b = Rect::new(rect.cx() - 36.0, top + lift, 72.0, 54.0);
                                c.shadow(b, 6.0, 4.0, 12.0, Rgba::css(0, 0, 0, 0.3));
                                show(c, pic, false, b, 6.0);
                            }
                            shown = true;
                        }
                    } else {
                        want.push(i);
                    }
                }
                if !shown {
                    look::icon(c, row_kind(rows, drives, places, &dir, pc, i), centre, 56.0, 1.25, true);
                }
                let ty = top + pic_h + 8.0;
                if renaming {
                    if let Some(rn) = ren.as_mut() {
                        let b = Rect::new(rect.x + 8.0, ty, rect.w - 16.0, 22.0);
                        c.fill(b, 5.0, WELL);
                        c.ring(b, 5.0, 1.5, accent());
                        let st = FieldStyle { font: Font::new(12.0, 500), bare: true, pad: 4.0, ..FieldStyle::INPUT };
                        field_with(c, f, ID_REN, b, &mut rn.field, "", &st);
                    }
                } else {
                    let tw = rect.w - 16.0;
                    if name_lines <= 1 {
                        c.text_fit(&name, Rect::new(rect.x + 8.0, ty, tw, 15.6), nfont, T1, Align::Center);
                    } else {
                        c.text_wrapped(&name, Rect::new(rect.x + 8.0, ty, tw, 31.2), nfont, T1, 15.6, 2);
                    }
                }
                if let Some(d) = drives.get(i).filter(|_| pc) {
                    let tr = Rect::new(rect.cx() - rect.w * 0.4, ty + name_h + 8.0, rect.w * 0.8, 3.0);
                    c.fill(tr, 2.0, ink(0.10));
                    c.fill(Rect::new(tr.x, tr.y, tr.w * d.used_pct() as f32 / 100.0, 3.0), 2.0, look::bar_colour(d.used_pct()));
                }
            })
        };
        let _ = cols;
        self.list = list;
        self.geo.lines_per_page = (((r.h - 12.0) / 130.0).floor() as usize).max(1);
        self.geo.rows = vis;
        for i in want {
            self.want_picture(i, 96);
        }
    }

    fn empty_state(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        let (title, sub, btn, kind): (String, String, Option<&str>, IconKind) = if self.searching() {
            (
                t!("Ничего не найдено", "Nothing found", ja: "見つかりません", pt: "Nada encontrado", zh: "未找到").into(),
                tf!("В «{}» и вложенных папках нет имён с «{}»", "No names in {} and its folders contain \u{201C}{}\u{201D}", ja: "{} とその下に「{}」を含む名前はありません", pt: "Nenhum nome em {} e subpastas contém \u{201C}{}\u{201D}", zh: "{} 及其子文件夹中没有包含「{}」的名称", self.label_of(&self.dir), self.query.clone().unwrap_or_default()),
                Some(t!("Очистить поиск", "Clear search", ja: "検索をクリア", pt: "Limpar pesquisa", zh: "清除搜索")),
                IconKind::Folder(None),
            )
        } else {
            (
                t!("Эта папка пуста", "This folder is empty", ja: "このフォルダーは空です", pt: "Esta pasta está vazia", zh: "此文件夹为空").into(),
                t!("Перетащите сюда файлы или создайте новое", "Drop files here or create something new", ja: "ここにファイルをドロップするか、新規作成してください", pt: "Solte arquivos aqui ou crie algo novo", zh: "将文件拖到此处或新建").into(),
                if self.in_zip() || self.at_pc() { None } else { Some(t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹")) },
                self.icon_of_dir(&self.dir.clone()),
            )
        };
        // centred in the box, as a column: the icon, the title, the line under it, the button
        let btn_h = if btn.is_some() { 6.0 + 10.0 + 32.0 } else { 0.0 };
        let total = 56.0 + 10.0 + 19.0 + 10.0 + 16.0 + btn_h;
        let y0 = r.y + (r.h - total) / 2.0;
        c.faded(0.55, |c| plates_icon(c, kind, (r.cx(), y0 + 28.0), 56.0));
        c.text(&title, Rect::new(r.x, y0 + 56.0 + 10.0, r.w, 19.0), Font::new(15.0, 600), T1, Align::Center);
        c.text_fit(&sub, Rect::new(r.x + 20.0, y0 + 56.0 + 10.0 + 19.0 + 10.0, r.w - 40.0, 16.0), BODY, T3, Align::Center);
        if let Some(label) = btn {
            let w = c.measure(label, BODY_M).ceil() + 28.0;
            let b = Rect::new(r.cx() - w / 2.0, y0 + 56.0 + 10.0 + 19.0 + 10.0 + 16.0 + 16.0, w, 32.0);
            let hot = f.focusable(ID_EMPTY_BTN, b);
            f.hand();
            c.transformed(b.cx(), b.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                c.fill(b, 9.0, ink(0.08 + 0.04 * hot.hover));
                c.text(label, b, BODY_M, T1, Align::Center);
            });
        }
    }

    fn paint_status(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.fill(Rect::new(r.x, r.y, r.w, 1.0), 0.0, ink(0.05));
        let r = Rect::new(r.x, r.y + 1.0, r.w, r.h - 1.0); // the text and the job are centred under the hairline
        let job = self.job.clone();
        let mut right = r.x + r.w - 16.0;
        if let Some(j) = &job {
            // Cancel, the bar, the label
            let cw = c.measure(t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消"), SMALL).ceil() + 16.0;
            let cb = Rect::new(right - cw, r.cy() - 11.0, cw, 22.0);
            let hot = f.item(ID_JOB_CANCEL, cb);
            f.hand();
            c.fill(cb, 6.0, ink(0.08 * hot.hover));
            c.text(t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消"), cb, SMALL, T2, Align::Center);
            right = cb.x - 10.0;
            let track = Rect::new(right - 120.0, r.cy() - 2.0, 120.0, 4.0);
            c.fill(track, 2.0, ink(0.10));
            let frac = f.tween(ID_JOB_CANCEL, 1, j.frac.unwrap_or(0.0), 200, winrust_ui::anim::LINEAR);
            c.clipped(track, |c| c.fill(Rect::new(track.x, track.y, track.w * frac, 4.0), 2.0, accent()));
            right = track.x - 10.0;
            let lw = (c.measure(&j.label, SMALL.tabular()).ceil() + 2.0).min(right - r.x - 200.0).max(40.0);
            c.text_fit(&j.label, Rect::new(right - lw, r.y, lw, r.h), SMALL.tabular(), T2, Align::End);
            right -= lw + 12.0;
        }
        let s = self.status();
        c.text_fit(&s, Rect::new(r.x + 20.0, r.y, (right - r.x - 20.0).max(40.0), r.h), SMALL.tabular(), T3, Align::Start);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the preview

    fn preview(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.fill(r, 0.0, SIDE);
        c.fill(Rect::new(r.x, r.y, 1.0, r.h), 0.0, ink(0.05));
        let full = Rect::new(r.x + r.w - PV_INNER, r.y, PV_INNER, r.h); // laid out at its own width, cut by the pane
        let one = self.one();
        c.clipped(r, |c| {
            let Some(i) = one else {
                let (t1, t2) = if self.list.sel.len() > 1 {
                    let bytes: u64 = self.list.sel.iter().filter_map(|&k| self.rows.get(k)).filter(|x| !x.dir).map(|x| x.size).sum();
                    (tf!("Выбрано: {}", "{} items selected", ja: "{} 個選択", pt: "{} itens selecionados", zh: "已选择 {} 项", self.list.sel.len()), tf!("Всего {}", "{} in total", ja: "合計 {}", pt: "{} no total", zh: "共 {}", crate::size_text(bytes)))
                } else {
                    let sub = if !self.rows.is_empty() { t!("Выберите файл, чтобы увидеть его здесь", "Select a file to see it here", ja: "ファイルを選択するとここに表示されます", pt: "Selecione um arquivo para vê-lo aqui", zh: "选择文件即可在此查看").into() } else if self.searching() { String::new() } else { t!("Пустая папка", "Empty folder", ja: "空のフォルダー", pt: "Pasta vazia", zh: "空文件夹").into() };
                    (self.label_of(&self.dir.clone()), sub)
                };
                c.text_fit(&t1, Rect::new(full.x + 18.0, r.cy() - 14.0, full.w - 36.0, 20.0), BODY_M, T2, Align::Center);
                c.text_fit(&t2, Rect::new(full.x + 18.0, r.cy() + 6.0, full.w - 36.0, 18.0), SMALL, T3, Align::Center);
                return;
            };
            let x = full.x + 18.0;
            let wd = full.w - 36.0;
            let mut y = r.y + 20.0;
            // the picture: a thumbnail, or the big icon
            let box_ = Rect::new(x, y, wd, 150.0);
            c.fill(box_, 12.0, ink(0.035));
            let path = self.row_path(i);
            let row_dir = self.rows[i].dir;
            let pc = self.at_pc();
            let kind = row_kind(&self.rows, &self.drives, &self.places, &self.dir, pc, i);
            let mut shown = false;
            if !pc && !row_dir {
                if self.thumbs.contains_key(&path) {
                    let is_icon = self.thumbs.get(&path).is_some_and(|t| t.icon);
                    if let Some(pic) = picture_of(c, &mut self.thumbs, &path) {
                        if is_icon {
                            show(c, pic, true, Rect::new(box_.cx() - 42.0, box_.cy() - 42.0, 84.0, 84.0), 0.0);
                        } else {
                            cover(c, pic, box_, 12.0);
                        }
                        shown = true;
                    }
                } else {
                    self.want_picture(i, 256);
                }
            }
            if !shown {
                look::icon(c, kind, (box_.cx(), box_.cy()), 84.0, 1.1, true);
            }
            y += 150.0 + 14.0;
            let name = self.shown_name(i);
            let nf = Font::new(15.0, 600);
            let lines = c.lines(&name, nf, wd).clamp(1, 3);
            let nh = lines as f32 * 19.5;
            c.text_wrapped(&name, Rect::new(x, y, wd, nh), nf, T1, 19.5, 3);
            y += nh + 3.0;
            let kind_s = if let Some(d) = self.drive(i) { kind_text(d.kind) } else { text::kind(&self.rows[i].name, row_dir) };
            c.text_fit(&kind_s, Rect::new(x, y, wd, 16.0), BODY, T3, Align::Start);
            y += 16.0 + 14.0;
            // the facts
            c.fill(Rect::new(x, y, wd, 1.0), 0.0, ink(0.06));
            y += 1.0 + 12.0;
            let facts = self.fact_rows(i);
            for (k, v) in facts {
                let kw = c.measure(&k, SMALL).ceil();
                c.text(&k, Rect::new(x, y, kw + 2.0, 15.0), SMALL, T3, Align::Start);
                c.text_fit(&v, Rect::new(x + kw + 12.0, y, (wd - kw - 12.0).max(10.0), 15.0), SMALL.tabular(), T1, Align::End);
                y += 15.0 + 8.0;
            }
            y += 14.0 - 8.0;
            // Open, and the "more" button
            let ob = Rect::new(x, y, wd - 32.0 - 6.0, 32.0);
            let hot = f.focusable(ID_PV_OPEN, ob);
            f.hand();
            c.transformed(ob.cx(), ob.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                c.fill(ob, 9.0, ink(0.10 + 0.04 * hot.hover));
                c.text(t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开"), ob, BODY_M, T1, Align::Center);
            });
            let mb = Rect::new(ob.x + ob.w + 6.0, y, 32.0, 32.0);
            let mh = f.item(ID_PV_MORE, mb);
            f.hand();
            c.transformed(mb.cx(), mb.cy(), 1.0 - 0.03 * mh.press, 0.0, 0.0, |c| {
                c.fill(mb, 9.0, ink(0.08 + 0.04 * mh.hover));
                c.icon("M6 12h.01M12 12h.01M18 12h.01", (mb.cx(), mb.cy()), 16.0, 3.0, T2, 0.0);
            });
        });
    }

    /// The facts of the preview: what the design lists for a drive, a folder, a file.
    fn fact_rows(&mut self, i: usize) -> Vec<(String, String)> {
        let clock = self.clock;
        let mut v: Vec<(String, String)> = Vec::new();
        if let Some(d) = self.drive(i).cloned() {
            let used = d.total.saturating_sub(d.free);
            v.push((t!("Занято", "Used", ja: "使用済み", pt: "Usado", zh: "已用").into(), format!("{} \u{00B7} {}%", text::gb(used), d.used_pct())));
            v.push((t!("Свободно", "Free", ja: "空き", pt: "Livre", zh: "可用").into(), text::gb(d.free)));
            v.push((t!("Ёмкость", "Capacity", ja: "容量", pt: "Capacidade", zh: "容量").into(), text::gb(d.total)));
            v.push((t!("Файловая система", "File system", ja: "ファイル システム", pt: "Sistema de arquivos", zh: "文件系统").into(), d.fs));
            return v;
        }
        let (row_dir, size, time, name) = {
            let r = &self.rows[i];
            (r.dir, r.size, r.time, r.name.clone())
        };
        let facts = self.facts_of(i);
        if row_dir {
            if let Some((d, n, more)) = facts.contains {
                let plus = if more { "+" } else { "" };
                let s = match (d, n) {
                    (0, 0) => t!("Пусто", "Empty", ja: "空", pt: "Vazio", zh: "空").to_string(),
                    (d, 0) => format!("{}{plus}", text::folders(d)),
                    (0, n) => format!("{}{plus}", text::files(n)),
                    (d, n) => format!("{}, {}{plus}", text::folders(d), text::files(n)),
                };
                v.push((t!("Содержит", "Contains", ja: "内容", pt: "Contém", zh: "包含").into(), s));
            }
            let total = if size > 0 { size } else { facts.size.unwrap_or(0) };
            if total > 0 {
                v.push((t!("Размер", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小").into(), crate::size_text(total)));
            }
        } else {
            v.push((t!("Размер", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小").into(), crate::size_text(size)));
        }
        v.push((t!("Изменён", "Modified", ja: "更新日時", pt: "Modificado", zh: "修改时间").into(), text::long_date(time, clock)));
        if let Some((w, h)) = facts.dims {
            v.push((t!("Размеры", "Dimensions", ja: "大きさ", pt: "Dimensões", zh: "尺寸").into(), format!("{w} \u{00D7} {h}")));
        }
        let place = if self.searching() { std::path::Path::new(&name).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default() } else { self.dir.to_string_lossy().into_owned() };
        v.push((t!("Где", "Where", ja: "場所", pt: "Onde", zh: "位置").into(), place));
        v
    }
}

/// The plate of a place or a drive: the accent's tint when it is where we are, a faint one under the pointer, a ring when something is
/// about to be dropped on it.
fn plate(c: &mut Canvas<'_>, b: Rect, cur: bool, hover: f32, drop: bool) {
    if drop {
        c.fill(b, 8.0, accent().alpha(0.14));
        c.ring(b, 8.0, 1.5, accent());
    } else if cur {
        c.fill(b, 8.0, accent().alpha(0.16));
    } else {
        c.fill(b, 8.0, ink(0.05 * hover));
    }
}

/// The focus ring of the design.
fn look_ring() -> Rgba {
    winrust_ui::widgets::look::focus_ring().fade(0.9)
}

/// Frame helper: a hint over an item (used for a tab's full name).
trait TipHint {
    fn tip_hint(&mut self, s: &str);
}

impl TipHint for Frame<'_> {
    fn tip_hint(&mut self, s: &str) {
        self.tip(s.to_string());
    }
}

