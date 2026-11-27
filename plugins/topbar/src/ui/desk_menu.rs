//! The desktop's right-click menu (the mockup's `deskCtx`, `dm`): a 300 px glass box that opens from the corner nearest the
//! pointer. A row of three tiles (new folder, text file, paste); the sort order and the icon size as segmented controls with a
//! gliding thumb; the switches "arrange automatically" and "show desktop icons"; Screenshot, Record screen, Refresh, the disk's
//! usage bar and Properties. The tiles, the actions and the segments go to rust-explorer's desktop ([`Cmd`]); the segments and
//! the switches leave the menu open, like the mockup's (it only changes its state), the rest close it.
//!
//! The box is placed by [`place`] from the pointer and the room there is, flipping to the left of the pointer and above it
//! the way the mockup does, and grows from that corner.

use std::rc::Rc;

use winrust_common::deskmenu::{Cmd, IconSize, Sort};

use super::anim::{EASE, SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::menu::{DIM, Hint, INK, MUTED, draw_row_ink};
use super::notice::accent;
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Inset, Look, Origin, Place, Pose, Shadow};
use super::settings_panel::switch;

pub const W: f32 = 300.0;
const PAD: f32 = 8.0;
const GAP: f32 = 6.0;
const TILE_H: f32 = 68.0;
const ROW: f32 = 38.0;
const DISK_H: f32 = 46.0;
const SEG_W: f32 = 176.0;
const SEG_H: f32 = 34.0;
/// `.dmIn .dmi:nth-child(n)`: when each block of the menu rises in, ms.
const DELAYS: [u32; 5] = [0, 25, 45, 60, 70];
const RED: Rgba = Rgba::css(229, 72, 77, 1.0);

const NEW_FOLDER: &str = "M3.5 7.5V18h17V9h-9l-2-2h-6zM12 11.5v4M10 13.5h4";
const TEXT_FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h3";
const PASTE: &str = "M9 4h6v3H9zM7 5.5H5.5V20h13V5.5H17";
const GRID: &str = "M4 4h6.5v6.5H4zM13.5 4H20v6.5h-6.5zM4 13.5h6.5V20H4zM13.5 13.5H20V20h-6.5z";
const ARRANGE: &str = "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z";
const EYE: &str = "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z";
const SHOT: &str = "M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4M12 14.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z";
const REC_RING: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z";
const REFRESH: &str = "M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5";
const DISK: &str = "M3 14h18v5H3zM3 14l2.5-8h13L21 14M17 16.5h.01";
const PROPS: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01";

// the ids of what can be hit
const TILE: i32 = 0; // 0..3
const SORT: i32 = 10; // 10..13
const SIZE: i32 = 20; // 20..23
const AUTO: i32 = 30;
const SHOWN: i32 = 31;
const SHOT_ROW: i32 = 40;
const REC_ROW: i32 = 41;
const REFRESH_ROW: i32 = 42;
const DISK_ROW: i32 = 43;
const PROPS_ROW: i32 = 44;

/// What the menu asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    /// Something for the desktop to do.
    Cmd(Cmd),
    /// The bar's own: the screenshot tool, and the recording (start, or stop when one runs).
    Shot,
    Record,
}

/// The system drive's room: GB used and GB in all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Disk {
    pub used: u32,
    pub total: u32,
}

impl Disk {
    /// From the volume's bytes (what Explorer's "GB" is: 2^30).
    #[must_use]
    pub fn from_bytes(total: u64, free: u64) -> Option<Disk> {
        let gib = |b: u64| (b as f64 / f64::from(1u32 << 30)).round() as u32;
        (total > 0).then(|| Disk { used: gib(total.saturating_sub(free)), total: gib(total) })
    }

    /// How much of the bar is filled.
    #[must_use]
    pub fn frac(self) -> f32 {
        if self.total == 0 { 0.0 } else { (self.used as f32 / self.total as f32).clamp(0.0, 1.0) }
    }
}

/// What the menu shows: the desktop's state (it tells the bar with each request) and the bar's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct State {
    pub sort: Sort,
    pub size: IconSize,
    pub auto: bool,
    pub shown: bool,
    /// A screen recording runs: the row offers to stop it.
    pub recording: bool,
    /// `None`: no WinRust Disk to open, the row is left out.
    pub disk: Option<Disk>,
}

/// Which corner of the box is at the pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Corner {
    /// The box stands above the pointer.
    pub up: bool,
    /// The box stands to the left of it.
    pub left: bool,
}

/// Where the `size` px box goes for a pointer at `at`, in a `room` of (left, top, right, bottom) px, with the mockup's rules:
/// it flips to the left when it would reach within `margin` of the right edge and above the pointer when it would pass the
/// bottom (never higher than `top + 8`); and, which the mockup leaves to its screen, it is kept inside whatever it still
/// overflows. Returns its top left corner and the corner at the pointer.
#[must_use]
pub fn place(at: (i32, i32), size: (i32, i32), room: (i32, i32, i32, i32), margin: i32, s: f32) -> ((i32, i32), Corner) {
    let (x, y) = at;
    let left = x + size.0 > room.2 - margin;
    let up = y + size.1 > room.3;
    let top = room.1 + (8.0 * s).round() as i32;
    let bx = if left { x - size.0 } else { x };
    let by = if up { (y - size.1).max(top) } else { y };
    let bx = bx.min(room.2 - size.0).max(room.0);
    let by = by.min(room.3 - size.1).max(room.1);
    ((bx, by), Corner { up, left })
}

/// Where everything stands, in DIPs from the box's corner.
struct Layout {
    size: (f32, f32),
    tiles: [Rect; 3],
    sort: Rect,
    size_ctl: Rect,
    toggles: [Rect; 2],
    rule: f32,
    /// The action rows by id, top to bottom.
    rows: Vec<(i32, Rect)>,
    /// The top of each block of the menu (the five that rise in), for the rise's centre.
    tops: [f32; 5],
}

fn layout(disk: bool) -> Layout {
    let inner = W - 2.0 * PAD;
    let tw = (inner - 2.0 * 4.0) / 3.0;
    let tiles = [0, 1, 2].map(|i| Rect::new(PAD + i as f32 * (tw + 4.0), PAD, tw, TILE_H));
    let sort_y = PAD + TILE_H + GAP;
    // padding 10 8 8, rows of 34 with 8 between
    let ctl_x = W - PAD - 8.0 - SEG_W;
    let sort = Rect::new(ctl_x, sort_y + 10.0, SEG_W, SEG_H);
    let size_ctl = Rect::new(ctl_x, sort_y + 10.0 + SEG_H + 8.0, SEG_W, SEG_H);
    let tog_y = sort_y + 10.0 + 2.0 * SEG_H + 8.0 + 8.0 + GAP;
    let toggles = [0, 1].map(|i| Rect::new(PAD, tog_y + i as f32 * (ROW + 1.0), inner, ROW));
    let rule = tog_y + 2.0 * ROW + 1.0 + GAP;
    let mut y = rule + 1.0 + GAP;
    let tops = [PAD, sort_y, tog_y, rule, y];
    let mut rows = Vec::new();
    for (id, h) in [(SHOT_ROW, ROW), (REC_ROW, ROW), (REFRESH_ROW, ROW), (DISK_ROW, DISK_H), (PROPS_ROW, ROW)] {
        if id == DISK_ROW && !disk {
            continue;
        }
        rows.push((id, Rect::new(PAD, y, inner, h)));
        y += h + 1.0;
    }
    Layout { size: (W, y - 1.0 + PAD), tiles, sort, size_ctl, toggles, rule, rows, tops }
}

/// The three options of a segmented control at `r`: the thumb's cell of each.
fn options(r: Rect) -> [Rect; 3] {
    let w = (r.w - 6.0) / 3.0;
    [0, 1, 2].map(|i| Rect::new(r.x + 3.0 + i as f32 * w, r.y + 3.0, w, r.h - 6.0))
}

pub struct DeskPanel {
    st: State,
    lay: Layout,
    /// The box's top left corner on the screen, device pixels, and the corner it grows from.
    at: (i32, i32),
    corner: Corner,
    pick: Rc<dyn Fn(Pick)>,
}

impl DeskPanel {
    /// `at` is where the box goes ([`place`] found it), `corner` the one that is at the pointer.
    #[must_use]
    pub fn new(st: State, at: (i32, i32), corner: Corner, pick: impl Fn(Pick) + 'static) -> Self {
        Self { st, lay: layout(st.disk.is_some()), at, corner, pick: Rc::new(pick) }
    }

    /// The box's size in DIPs (what [`place`] needs before the box has a place).
    #[must_use]
    pub fn size_for(disk: bool) -> (f32, f32) {
        layout(disk).size
    }

    /// The menu in the state it is in now (the segments and the switches change it as they are pressed).
    #[must_use]
    pub const fn state(&self) -> State {
        self.st
    }

    fn origin(&self) -> (f32, f32) {
        (if self.corner.left { W } else { 0.0 }, if self.corner.up { self.lay.size.1 } else { 0.0 })
    }

    /// `dmTL` / `dmTR` / `dmBL` / `dmBR`: from the corner, a 28 px stub, scaled .92 and see-through, to the whole box.
    fn clip(&self) -> Inset {
        let (w, h) = self.lay.size;
        let (far_x, far_y) = (w - 28.0, h - 28.0);
        match (self.corner.up, self.corner.left) {
            (false, false) => Inset::new(0.0, far_x, far_y, 0.0, 10.0),
            (false, true) => Inset::new(0.0, 0.0, far_y, far_x, 10.0),
            (true, false) => Inset::new(far_y, far_x, 0.0, 0.0, 10.0),
            (true, true) => Inset::new(far_y, 0.0, 0.0, far_x, 10.0),
        }
    }

    /// The ids in the order the arrow keys walk them, one list a line: a tile row, the two controls, the switches, the actions.
    fn lines(&self) -> Vec<Vec<i32>> {
        let mut v = vec![vec![TILE, TILE + 1, TILE + 2], vec![SORT, SORT + 1, SORT + 2], vec![SIZE, SIZE + 1, SIZE + 2], vec![AUTO], vec![SHOWN]];
        v.extend(self.lay.rows.iter().map(|(id, _)| vec![*id]));
        v
    }

    fn choose(&mut self, id: i32, cx: &mut Cx) {
        let pick = Rc::clone(&self.pick);
        let stay = |p: Pick| pick(p);
        match id {
            i if (TILE..TILE + 3).contains(&i) => {
                let c = [Cmd::NewFolder, Cmd::NewText, Cmd::Paste][(i - TILE) as usize];
                cx.close_then(move || pick(Pick::Cmd(c)));
            }
            i if (SORT..SORT + 3).contains(&i) => {
                self.st.sort = Sort::ALL[(i - SORT) as usize];
                stay(Pick::Cmd(Cmd::Sort(self.st.sort)));
            }
            i if (SIZE..SIZE + 3).contains(&i) => {
                self.st.size = IconSize::ALL[(i - SIZE) as usize];
                stay(Pick::Cmd(Cmd::Size(self.st.size)));
            }
            AUTO => {
                self.st.auto = !self.st.auto;
                stay(Pick::Cmd(Cmd::Auto(self.st.auto)));
            }
            SHOWN => {
                self.st.shown = !self.st.shown;
                stay(Pick::Cmd(Cmd::Show(self.st.shown)));
            }
            _ => {
                let p = match id {
                    SHOT_ROW => Pick::Shot,
                    REC_ROW => Pick::Record,
                    REFRESH_ROW => Pick::Cmd(Cmd::Refresh),
                    DISK_ROW => Pick::Cmd(Cmd::Disk),
                    PROPS_ROW => Pick::Cmd(Cmd::Properties),
                    _ => return,
                };
                cx.close_then(move || pick(p));
            }
        }
    }
}

/// A segmented control (`role=radiogroup`): the track, the thumb gliding to the chosen option (`.sthumb`, .32 s) and what
/// `content` draws in each option in the colour it gets (the muted one, the ink for the chosen and the hovered).
fn segments(c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, base: i32, cur: usize, mut content: impl FnMut(&mut Canvas<'_>, Rect, usize, Rgba)) {
    winrust_ui::widgets::look::seg_track(c, r, 10.0); // `--segTrack`
    let cells = options(r);
    let w = cells[0].w;
    let pos = f.tween(base, 7, cur as f32, 320, SNAP);
    let thumb = Rect::new(r.x + 3.0 + pos * w, r.y + 3.0, w, r.h - 6.0);
    winrust_ui::widgets::look::seg_thumb(c, thumb, 7.0); // `--segThumb`
    for (k, cell) in cells.iter().enumerate() {
        let id = base + k as i32;
        let sel = f.tween(id, 8, f32::from(k == cur), 200, EASE);
        let hot = f.hover(id, 200, EASE);
        content(c, *cell, k, MUTED.mix(INK, sel.max(hot)));
    }
}

impl Content for DeskPanel {
    fn id(&self) -> Id {
        Id::Menu
    }

    /// `rgba(16,17,24,.82)`, blur 40, saturate 1.6, radius 18, `0 30px 80px rgba(0,0,0,.6)`.
    fn look(&self) -> Look {
        Look::glass(18.0, 0.82, Shadow { dy: 30.0, blur: 80.0, color: Rgba::css(0, 0, 0, 0.6) })
    }

    fn entry(&self) -> Entry {
        let (ox, oy) = self.origin();
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 0.92, clip: Some(self.clip()) }, dur: 300, ease: SNAP, origin: Origin::Px(ox, oy) }
    }

    /// `dmOut`: to `opacity 0, scale(.96)` in 140 ms, `ease`.
    fn exit(&self) -> Exit {
        let (ox, oy) = self.origin();
        Exit { to: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 0.96, clip: None }, dur: 140, ease: EASE, origin: Origin::Px(ox, oy) }
    }

    fn place(&self) -> Place {
        Place { along: Along::Abs(self.at.0, self.at.1), gap: 0.0 }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.lay.size
    }

    fn settle(&self) -> u32 {
        500
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (st, lay) = (self.st, &self.lay);
        let rise = |f: &Frame<'_>, i: usize| f.rise(DELAYS[i], 260, SNAP, 6.0, 1.0);

        // the tiles
        let labels = [
            (NEW_FOLDER, t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹")),
            (TEXT_FILE, t!("Текст. файл", "Text file", ja: "テキスト", pt: "Arq. de texto", zh: "文本文件")),
            (PASTE, t!("Вставить", "Paste", ja: "貼り付け", pt: "Colar", zh: "粘贴")),
        ];
        rise(f, 0).draw(c, (W / 2.0, lay.tops[0]), |c| {
            for (i, ((icon, label), r)) in labels.iter().zip(&lay.tiles).enumerate() {
                let hot = f.hover(TILE + i as i32, 140, SPRING);
                c.fill(*r, 12.0, Rgba::white(0.05 + 0.04 * hot));
                c.icon(icon, (r.cx(), r.y + 13.5 + 10.0), 20.0, 1.7 * 24.0 / 20.0, INK, 0.0);
                c.text_fit(label, Rect::new(r.x + 4.0, r.y + 40.0, r.w - 8.0, 15.0), Font::new(12.0, 400), INK, Align::Center);
            }
        });

        // the sort order and the icon size
        let sizes = [11.0_f32, 14.0, 18.0];
        let names = [t!("Имя", "Name", ja: "名前", pt: "Nome", zh: "名称"), t!("Дата", "Date", ja: "日付", pt: "Data", zh: "日期"), t!("Размер", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小")];
        rise(f, 1).draw(c, (W / 2.0, lay.tops[1]), |c| {
            for (r, base, title, cur) in [
                (lay.sort, SORT, t!("Сортировка", "Sort by", ja: "並べ替え", pt: "Classificar por", zh: "排序方式"), st.sort as usize),
                (lay.size_ctl, SIZE, t!("Размер значков", "Icon size", ja: "アイコンのサイズ", pt: "Tamanho dos ícones", zh: "图标大小"), st.size as usize),
            ] {
                c.text(title, Rect::new(PAD + 8.0, r.y, 120.0, r.h), Font::new(13.0, 400), DIM, Align::Start);
                if base == SORT {
                    segments(c, f, r, base, cur, |c, cell, k, ink| c.text(names[k], cell, Font::new(12.0, 500), ink, Align::Center));
                } else {
                    // the same four squares at three sizes; their stroke stays 1.6 px whatever the size
                    segments(c, f, r, base, cur, |c, cell, k, ink| c.icon(GRID, (cell.cx(), cell.cy()), sizes[k], 24.0 / sizes[k] * 1.6, ink, 0.0));
                }
            }
        });

        // the switches
        let toggles = [
            (AUTO, ARRANGE, t!("Упорядочить автоматически", "Arrange automatically", ja: "自動的に整列", pt: "Organizar automaticamente", zh: "自动排列"), st.auto),
            (SHOWN, EYE, t!("Значки рабочего стола", "Show desktop icons", ja: "デスクトップ アイコンを表示", pt: "Mostrar ícones do desktop", zh: "显示桌面图标"), st.shown),
        ];
        rise(f, 2).draw(c, (W / 2.0, lay.tops[2]), |c| {
            for ((id, icon, label, on), r) in toggles.iter().zip(&lay.toggles) {
                let hot = f.hover(*id, 140, SPRING);
                c.fill(*r, 10.0, Rgba::white(0.08 * hot));
                c.icon(icon, (r.x + 10.0 + 8.5, r.cy()), 17.0, 1.8, DIM, 0.0);
                c.text_fit(label, Rect::new(r.x + 10.0 + 17.0 + 12.0, r.y, r.w - 10.0 - 17.0 - 12.0 - 12.0 - 36.0 - 10.0, r.h), Font::new(13.0, 400), INK, Align::Start);
                switch(c, f, *id, (r.x + r.w - 10.0 - 36.0, r.cy() - 10.0), *on, accent());
            }
        });

        rise(f, 3).draw(c, (W / 2.0, lay.rule), |c| c.fill(Rect::new(PAD + 8.0, lay.rule, W - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.07)));

        // the actions
        rise(f, 4).draw(c, (W / 2.0, lay.tops[4]), |c| {
            for (id, r) in &lay.rows {
                let hot = f.hover(*id, 140, SPRING);
                let plate = Rgba::white(0.08 * hot);
                let (icon, ink, label, keys): (_, _, String, &'static [&'static str]) = match *id {
                    SHOT_ROW => (SHOT, DIM, t!("Скриншот", "Screenshot", ja: "スクリーンショット", pt: "Captura de tela", zh: "截图").into(), &["Win Shift S"]),
                    REC_ROW if st.recording => (REC_RING, RED, t!("Остановить запись", "Stop recording", ja: "録画を停止", pt: "Parar gravação", zh: "停止录制").into(), &["Win Shift R"]),
                    REC_ROW => (REC_RING, DIM, t!("Запись экрана", "Record screen", ja: "画面を録画", pt: "Gravar a tela", zh: "录制屏幕").into(), &["Win Shift R"]),
                    REFRESH_ROW => (REFRESH, DIM, t!("Обновить", "Refresh", ja: "最新の情報に更新", pt: "Atualizar", zh: "刷新").into(), &["F5"]),
                    DISK_ROW => (DISK, DIM, t!("Занятое место", "Disk usage", ja: "ディスク使用量", pt: "Uso do disco", zh: "磁盘占用").into(), &[]),
                    _ => (PROPS, DIM, t!("Свойства", "Properties", ja: "プロパティ", pt: "Propriedades", zh: "属性").into(), &[]),
                };
                if *id == DISK_ROW {
                    let disk = st.disk.unwrap_or(Disk { used: 0, total: 0 });
                    c.fill(*r, 10.0, plate);
                    let cy = r.y + 9.0 + 8.5;
                    c.icon(icon, (r.x + 10.0 + 8.5, cy), 17.0, 1.8, ink, 0.0);
                    let (used, total) = (disk.used, disk.total);
                    let amount = tf!("{used} из {total} ГБ", "{used} of {total} GB", ja: "{used} / {total} GB", pt: "{used} de {total} GB", zh: "{used} / {total} GB");
                    let aw = c.measure(&amount, Font::new(12.0, 400)).ceil() + 2.0;
                    c.text(&amount, Rect::new(r.x + r.w - 10.0 - aw, cy - 8.0, aw, 16.0), Font::new(12.0, 400), MUTED, Align::End);
                    let lx = r.x + 10.0 + 17.0 + 12.0;
                    c.text_fit(&label, Rect::new(lx, cy - 8.0, (r.x + r.w - 10.0 - aw - 8.0 - lx).max(10.0), 16.0), Font::new(13.0, 400), INK, Align::Start);
                    let bar = Rect::new(lx, r.y + 9.0 + 17.0 + 6.0, r.x + r.w - 10.0 - lx, 4.0);
                    c.fill(bar, 2.0, Rgba::white(0.1));
                    c.fill(Rect::new(bar.x, bar.y, bar.w * disk.frac(), bar.h), 2.0, accent());
                } else {
                    draw_row_ink(c, *r, plate, icon, ink, INK, &label, if keys.is_empty() { Hint::None } else { Hint::Keys(keys) });
                    if *id == REC_ROW {
                        c.circle(r.x + 10.0 + 8.5, r.cy(), 3.4 * 17.0 / 24.0, ink); // the record dot is filled, the ring around it is not
                    }
                }
            }
        });
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        let l = &self.lay;
        if let Some(i) = l.tiles.iter().position(|r| r.contains(x, y)) {
            return Some(TILE + i as i32);
        }
        for (base, ctl) in [(SORT, l.sort), (SIZE, l.size_ctl)] {
            if let Some(i) = options(ctl).iter().position(|r| r.contains(x, y)) {
                return Some(base + i as i32);
            }
        }
        if let Some(i) = l.toggles.iter().position(|r| r.contains(x, y)) {
            return Some(AUTO + i as i32);
        }
        l.rows.iter().find(|(_, r)| r.contains(x, y)).map(|(id, _)| *id)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if b == Button::Left {
            self.choose(id, cx);
        }
    }

    fn enter(&mut self, hover: Option<i32>, _mods: super::panel::Mods, cx: &mut Cx) {
        if let Some(id) = hover {
            self.choose(id, cx);
        }
    }

    /// What a size's button is called (`title`).
    fn tip(&self, id: i32) -> Option<(String, Rect)> {
        let i = usize::try_from(id - SIZE).ok().filter(|&i| i < 3)?;
        let label = [t!("Мелкие значки", "Small icons", ja: "小さいアイコン", pt: "Ícones pequenos", zh: "小图标"), t!("Средние значки", "Medium icons", ja: "中アイコン", pt: "Ícones médios", zh: "中等图标"), t!("Крупные значки", "Large icons", ja: "大きいアイコン", pt: "Ícones grandes", zh: "大图标")][i];
        Some((label.into(), options(self.lay.size_ctl)[i]))
    }

    /// Up and Down go line by line (round, to the same place along it), Left and Right along a line.
    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let lines = self.lines();
        let at = from.and_then(|f| lines.iter().enumerate().find_map(|(l, v)| v.iter().position(|&i| i == f).map(|k| (l, k))));
        let n = lines.len();
        let (l, k) = match (at, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => (0, 0),
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => (n - 1, 0),
            (Some((l, k)), Dir::Down) => ((l + 1) % n, k),
            (Some((l, k)), Dir::Up) => ((l + n - 1) % n, k),
            (Some((l, k)), Dir::Right) => (l, (k + 1).min(lines[l].len() - 1)),
            (Some((l, k)), Dir::Left) => (l, k.saturating_sub(1)),
        };
        lines.get(l).map(|v| v[k.min(v.len() - 1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const STATE: State = State { sort: Sort::Name, size: IconSize::Medium, auto: true, shown: true, recording: false, disk: Some(Disk { used: 604, total: 953 }) };

    fn panel() -> (DeskPanel, Rc<RefCell<Vec<Pick>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = Rc::clone(&log);
        (DeskPanel::new(STATE, (0, 0), Corner { up: false, left: false }, move |p| l.borrow_mut().push(p)), log)
    }

    #[test]
    fn the_box_is_laid_out_as_the_mockups_is() {
        // measured in the mockup at 1440 x 900: 8 + tiles 68 + 6 + 94 + 6 + 77 + 6 + 1 + 6 + 202 + 8 = 482
        let l = layout(true);
        assert_eq!(l.size, (300.0, 482.0));
        assert_eq!((l.tiles[0].x, l.tiles[0].y, l.tiles[0].w, l.tiles[0].h), (8.0, 8.0, 92.0, 68.0));
        assert_eq!(l.tiles[2].x + l.tiles[2].w, 292.0);
        assert_eq!((l.sort.x, l.sort.y, l.sort.w, l.sort.h), (108.0, 92.0, 176.0, 34.0));
        assert_eq!(l.size_ctl.y, 134.0);
        assert_eq!((l.toggles[0].y, l.toggles[1].y), (182.0, 221.0));
        assert_eq!(l.rule, 265.0);
        let ys: Vec<(i32, f32, f32)> = l.rows.iter().map(|(id, r)| (*id, r.y, r.h)).collect();
        assert_eq!(ys, vec![(SHOT_ROW, 272.0, 38.0), (REC_ROW, 311.0, 38.0), (REFRESH_ROW, 350.0, 38.0), (DISK_ROW, 389.0, 46.0), (PROPS_ROW, 436.0, 38.0)]);
        // without WinRust Disk the row and its pixel are gone
        assert_eq!(layout(false).size.1, 482.0 - 47.0);
        assert_eq!(DeskPanel::size_for(true), (300.0, 482.0));
    }

    #[test]
    fn the_box_flips_at_the_right_and_the_bottom_like_the_mockups() {
        let (size, room, s) = ((300, 482), (0, 0, 1440, 836), 1.0);
        assert_eq!(place((400, 300), size, room, 12, s), ((400, 300), Corner { up: false, left: false }));
        // the right edge: 12 px of margin
        assert!(!place((1128, 300), size, room, 12, s).1.left);
        assert_eq!(place((1129, 300), size, room, 12, s), ((829, 300), Corner { up: false, left: true }));
        // the bottom: the box goes above the pointer, ending at it
        assert_eq!(place((400, 500), size, room, 12, s), ((400, 18), Corner { up: true, left: false }));
        assert_eq!(place((1400, 830), size, room, 12, s), ((1100, 348), Corner { up: true, left: true }));
        // a short room: never above its top + 8, and kept inside what it still overflows
        let ((_, y), c) = place((100, 300), (300, 482), (0, 0, 1440, 400), 12, 1.0);
        assert!(c.up && y == 0, "{y}");
        let ((x, _), _) = place((50, 300), (300, 100), (0, 0, 200, 800), 12, 1.0);
        assert_eq!(x, 0, "wider than the room: against its left edge");
        // a monitor that is not at (0, 0)
        assert_eq!(place((-1000, 100), size, (-1920, 0, 0, 1080), 12, 1.0).0, (-1000, 100));
        assert_eq!(place((-200, 100), size, (-1920, 0, 0, 1080), 12, 1.0).0, (-500, 100));
    }

    #[test]
    fn a_pointer_finds_what_is_under_it_and_the_gaps_are_nothing() {
        let (p, _) = panel();
        let l = &p.lay;
        assert_eq!(p.hit(l.tiles[1].cx(), l.tiles[1].cy()), Some(TILE + 1));
        assert_eq!(p.hit(options(l.sort)[2].cx(), l.sort.cy()), Some(SORT + 2));
        assert_eq!(p.hit(options(l.size_ctl)[0].cx(), l.size_ctl.cy()), Some(SIZE));
        assert_eq!(p.hit(l.toggles[1].x + 40.0, l.toggles[1].cy()), Some(SHOWN));
        let disk = l.rows.iter().find(|(id, _)| *id == DISK_ROW).unwrap().1;
        assert_eq!(p.hit(disk.cx(), disk.cy()), Some(DISK_ROW));
        assert_eq!(p.hit(1.0, 1.0), None);
        assert_eq!(p.hit(l.rule + 0.0, l.rule - 3.0), None);
    }

    #[test]
    fn the_segments_and_the_switches_change_the_menu_and_tell_the_desktop_without_closing() {
        let (mut p, log) = panel();
        let mut cx = Cx::default();
        p.click(SORT + 2, Button::Left, &mut cx);
        p.click(SIZE + 2, Button::Left, &mut cx);
        p.click(AUTO, Button::Left, &mut cx);
        p.click(SHOWN, Button::Left, &mut cx);
        assert!(!cx.close, "the menu stays open");
        assert_eq!((p.state().sort, p.state().size, p.state().auto, p.state().shown), (Sort::Size, IconSize::Large, false, false));
        assert_eq!(
            *log.borrow(),
            vec![Pick::Cmd(Cmd::Sort(Sort::Size)), Pick::Cmd(Cmd::Size(IconSize::Large)), Pick::Cmd(Cmd::Auto(false)), Pick::Cmd(Cmd::Show(false))]
        );
        p.click(AUTO, Button::Left, &mut cx);
        assert_eq!(log.borrow().last(), Some(&Pick::Cmd(Cmd::Auto(true))), "a switch says the value it ends on");
    }

    #[test]
    fn the_tiles_and_the_actions_close_the_menu_and_then_ask() {
        for (id, want) in [
            (TILE, Pick::Cmd(Cmd::NewFolder)),
            (TILE + 1, Pick::Cmd(Cmd::NewText)),
            (TILE + 2, Pick::Cmd(Cmd::Paste)),
            (SHOT_ROW, Pick::Shot),
            (REC_ROW, Pick::Record),
            (REFRESH_ROW, Pick::Cmd(Cmd::Refresh)),
            (DISK_ROW, Pick::Cmd(Cmd::Disk)),
            (PROPS_ROW, Pick::Cmd(Cmd::Properties)),
        ] {
            let (mut p, log) = panel();
            let mut cx = Cx::default();
            p.click(id, Button::Left, &mut cx);
            assert!(cx.close && log.borrow().is_empty(), "{id}: closes first");
            cx.run_for_test();
            assert_eq!(*log.borrow(), vec![want], "{id}");
        }
        let (mut p, log) = panel();
        let mut cx = Cx::default();
        p.click(TILE, Button::Right, &mut cx);
        assert!(!cx.close && log.borrow().is_empty(), "only the left button chooses");
    }

    #[test]
    fn the_disk_row_is_a_bar_of_what_is_used() {
        assert_eq!(Disk { used: 604, total: 953 }.frac().to_bits(), (604.0f32 / 953.0).to_bits());
        assert_eq!(Disk { used: 5, total: 0 }.frac(), 0.0);
        assert_eq!(Disk { used: 9, total: 3 }.frac(), 1.0);
        assert_eq!(Disk::from_bytes(0, 0), None);
        assert_eq!(Disk::from_bytes(1 << 30, 0), Some(Disk { used: 1, total: 1 }));
        assert_eq!(Disk::from_bytes(10 << 30, 4 << 30), Some(Disk { used: 6, total: 10 }));
        assert_eq!(Disk::from_bytes(3 << 29, 3 << 29).map(|d| (d.used, d.total)), Some((0, 2)), "1.5 GB rounds up, nothing used");
    }

    #[test]
    fn arrows_walk_the_lines_and_stay_on_the_same_place_along_them() {
        let (p, _) = panel();
        assert_eq!(p.nav(None, Dir::Down), Some(TILE));
        assert_eq!(p.nav(Some(TILE + 1), Dir::Down), Some(SORT + 1));
        assert_eq!(p.nav(Some(SIZE + 2), Dir::Down), Some(AUTO), "a line of one has only its place");
        assert_eq!(p.nav(Some(TILE + 2), Dir::Right), Some(TILE + 2));
        assert_eq!(p.nav(Some(SORT + 1), Dir::Left), Some(SORT));
        assert_eq!(p.nav(Some(PROPS_ROW), Dir::Down), Some(TILE), "round");
        assert_eq!(p.nav(Some(TILE), Dir::Up), Some(PROPS_ROW));
        assert_eq!(p.nav(None, Dir::Up), Some(PROPS_ROW));
    }

    #[test]
    fn the_box_grows_from_the_corner_at_the_pointer() {
        let at = |up, left| DeskPanel::new(STATE, (0, 0), Corner { up, left }, |_| {});
        let (tl, br) = (at(false, false), at(true, true));
        assert_eq!(tl.origin(), (0.0, 0.0));
        assert_eq!(br.origin(), (300.0, 482.0));
        assert_eq!(tl.clip(), Inset::new(0.0, 272.0, 454.0, 0.0, 10.0));
        assert_eq!(br.clip(), Inset::new(454.0, 0.0, 0.0, 272.0, 10.0));
        assert_eq!(at(false, true).clip(), Inset::new(0.0, 0.0, 454.0, 272.0, 10.0));
        assert_eq!(at(true, false).clip(), Inset::new(454.0, 272.0, 0.0, 0.0, 10.0));
    }
}
