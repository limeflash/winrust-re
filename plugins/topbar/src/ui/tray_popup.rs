//! The hidden tray icons panel (the arrow on the bar): a grid of four 40 px buttons with 4 px gaps, padding 8, radius 16, on the
//! dense glass; `qsIn` opens it. The icons are the programs' own, the clicks go to them as in the real tray.

use windows::Win32::UI::WindowsAndMessaging::HICON;

use super::anim::EASE;
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::panel::{Button, Content, Cx, Dir, Frame, Id, Look, Shadow};

const COLS: usize = 4;
const CELL: f32 = 40.0;
const GAP: f32 = 4.0;
const PAD: f32 = 8.0;

/// One icon of the panel: the program's icon, its tooltip, and what the owner of the panel needs to find it again.
pub struct Cell {
    pub icon: HICON,
    pub tip: String,
    pub token: usize,
}

pub struct TrayPanel {
    cells: Vec<Cell>,
    read: fn() -> Vec<Cell>,
    /// A click on the cell with this token.
    act: fn(usize, Button),
    rows: usize,
}

impl TrayPanel {
    #[must_use]
    pub fn new(read: fn() -> Vec<Cell>, act: fn(usize, Button)) -> Self {
        Self { cells: read(), read, act, rows: 1 }
    }

    fn cell_rect(i: usize) -> Rect {
        let (col, row) = ((i % COLS) as f32, (i / COLS) as f32);
        Rect::new(PAD + col * (CELL + GAP), PAD + row * (CELL + GAP), CELL, CELL)
    }

    fn empty_text() -> &'static str {
        t!("Больше значков нет", "No more icons", ja: "他のアイコンはありません", pt: "Sem mais ícones", zh: "没有更多图标")
    }
}

impl Content for TrayPanel {
    fn id(&self) -> Id {
        Id::Tray
    }

    fn look(&self) -> Look {
        Look::dense(16.0, Shadow { dy: 20.0, blur: 50.0, color: Rgba::css(4, 6, 20, 0.6) })
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.rows = self.cells.len().div_ceil(COLS).max(1);
        let r = self.rows as f32;
        (2.0 * PAD + COLS as f32 * CELL + (COLS as f32 - 1.0) * GAP, 2.0 * PAD + r * CELL + (r - 1.0) * GAP)
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        // the icons come and go while the panel is open; the box keeps the size it opened with
        self.cells = (self.read)();
        self.cells.truncate(self.rows * COLS);
        if self.cells.is_empty() {
            let r = Rect::new(PAD, PAD, f.size.0 - 2.0 * PAD, CELL);
            c.text(Self::empty_text(), r, Font::new(12.0, 500), Rgba::css(138, 144, 162, 1.0), Align::Center);
            return;
        }
        for (i, cell) in self.cells.iter().enumerate() {
            let r = Self::cell_rect(i);
            let t = f.hover(i as i32, 250, EASE);
            c.fill(r, 10.0, Rgba::white(0.1 * t));
            c.hicon(cell.icon, Rect::new(r.cx() - 9.0, r.cy() - 9.0, 18.0, 18.0), 1.0);
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        (0..self.cells.len()).find(|&i| Self::cell_rect(i).contains(x, y)).map(|i| i as i32)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        let Some(token) = self.cells.get(id as usize).map(|c| c.token) else { return };
        let act = self.act;
        // the program answers a click with its own window or menu: the panel must be gone first
        cx.close_then(move || act(token, b));
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let n = self.cells.len() as i32;
        let cols = COLS as i32;
        let i = match (from, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n - 1,
            (Some(i), Dir::Left) => i - 1,
            (Some(i), Dir::Right) => i + 1,
            (Some(i), Dir::Up) => i - cols,
            (Some(i), Dir::Down) => i + cols,
        };
        (n > 0).then(|| i.clamp(0, n - 1))
    }

    fn tip(&self, id: i32) -> Option<(String, Rect)> {
        let cell = self.cells.get(id as usize)?;
        let hint = t!("средняя кнопка: на панель", "middle click: pin to the bar", ja: "中クリック: バーに固定", pt: "clique do meio: fixar na barra", zh: "中键：固定到任务栏");
        let text = if cell.tip.is_empty() { hint.to_string() } else { format!("{}  ·  {hint}", cell.tip) };
        Some((text, Self::cell_rect(id as usize)))
    }
}
