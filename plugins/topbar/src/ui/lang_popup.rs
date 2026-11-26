//! The keyboard layout panel (the language button on the bar): a header with the Win + Space keys, one row per installed layout
//! (a code chip, the language in its own tongue, "language · layout", a tick in an accent circle on the current one), a rule,
//! and a way into the system's language settings. Dense glass, radius 20, `qsIn`.

use super::anim::{EASE, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::panel::{Button, Content, Cx, Dir, Frame, Id, Look, Shadow};
use winrust_common::style::pal;

const W: f32 = 290.0;
const PAD: f32 = 8.0;
const ROW: f32 = 54.0;
const GAP: f32 = 2.0;
const HEAD: f32 = 34.0;
const SETTINGS: i32 = 1000;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const DIM: Rgba = Rgba::css(138, 144, 162, 1.0);
const KEY_INK: Rgba = Rgba::css(163, 169, 184, 1.0);
const ON_ACCENT: Rgba = Rgba::css(11, 18, 32, 1.0);

const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const KEYBOARD: &str = "M3 7h18v12H3zM7 11h.01M11 11h.01M15 11h.01M8 15h8";
const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";

/// One installed layout.
pub struct Layout {
    /// What the owner needs to switch to it.
    pub id: isize,
    /// `RUS`, `ENG`, `TUR`.
    pub code: String,
    /// The language in its own tongue.
    pub name: String,
    /// "Language · layout".
    pub sub: String,
    pub on: bool,
}

pub struct LangPanel {
    items: Vec<Layout>,
    pick: fn(isize),
    settings: fn(),
}

impl LangPanel {
    #[must_use]
    pub fn new(items: Vec<Layout>, pick: fn(isize), settings: fn()) -> Self {
        Self { items, pick, settings }
    }

    fn row(i: usize) -> Rect {
        Rect::new(PAD, PAD + HEAD + GAP + i as f32 * (ROW + GAP), W - 2.0 * PAD, ROW)
    }

    /// Top of the divider's box (its margins included).
    fn rule_y(&self) -> f32 {
        PAD + HEAD + GAP + self.items.len() as f32 * (ROW + GAP)
    }

    fn settings_row(&self) -> Rect {
        Rect::new(PAD, self.rule_y() + 13.0 + GAP, W - 2.0 * PAD, 44.0)
    }
}

impl Content for LangPanel {
    fn id(&self) -> Id {
        Id::Lang
    }

    fn look(&self) -> Look {
        Look::dense(20.0, Shadow { dy: 24.0, blur: 60.0, color: Rgba::css(4, 6, 20, 0.6) })
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        (W, self.settings_row().y + 44.0 + PAD)
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);

        // the header: "Keyboard layout" and the two keys
        let head = t!("Раскладка клавиатуры", "Keyboard layout", ja: "キーボード レイアウト", pt: "Layout do teclado", zh: "键盘布局");
        let (cy, kh) = (PAD + 6.0 + 10.0, 20.0);
        c.text(head, Rect::new(PAD + 10.0, cy - 10.0, 180.0, 20.0), Font::new(12.0, 500), DIM, Align::Start);
        let key_font = Font::new(11.0, 600);
        let mut x = W - PAD - 8.0;
        for key in [t!("Пробел", "Space", ja: "Space", pt: "Espaço", zh: "Space"), "Win"] {
            let w = c.measure(key, key_font).ceil() + 14.0;
            x -= w;
            let r = Rect::new(x, cy - kh / 2.0, w, kh);
            c.fill(r, 6.0, Rgba::white(0.08));
            c.text(key, r, key_font, KEY_INK, Align::Center);
            x -= 4.0;
        }

        for (i, it) in self.items.iter().enumerate() {
            let r = Self::row(i);
            let id = i as i32;
            let hot = f.hover(id, 250, EASE);
            let base = if it.on { 0.06 } else { 0.0 };
            c.fill(r, 14.0, Rgba::white(base + (0.07 - base) * hot));
            let chip = Rect::new(r.x + 10.0, r.cy() - 16.0, 40.0, 32.0);
            c.fill(chip, 9.0, if it.on { accent } else { Rgba::white(0.09) });
            c.text(&it.code, chip, Font::new(12.0, 600).tracking(0.03), if it.on { ON_ACCENT } else { INK }, Align::Center);
            let tx = chip.x + chip.w + 12.0;
            let tw = r.x + r.w - 12.0 - 22.0 - 12.0 - tx;
            c.text(&it.name, Rect::new(tx, r.cy() - 17.0, tw, 17.0), Font::new(14.0, 600), INK, Align::Start);
            c.text(&it.sub, Rect::new(tx, r.cy() + 2.0, tw, 15.0), Font::new(12.0, 400), DIM, Align::Start);
            // the tick: opacity .25s ease, scale .45s spring (.4 → 1)
            let on = f32::from(it.on);
            let op = f.tween(id, 1, on, 250, EASE);
            let k = f.tween(id, 2, 0.4 + 0.6 * on, 450, SPRING);
            let (ccx, ccy) = (r.x + r.w - 12.0 - 11.0, r.cy());
            c.faded(op, |c| {
                c.transformed(ccx, ccy, k, 0.0, 0.0, |c| {
                    c.circle(ccx, ccy, 11.0, accent);
                    c.icon(CHECK, (ccx, ccy), 13.0, 3.0, ON_ACCENT, 0.0);
                });
            });
        }

        // the rule and the way into the settings
        c.fill(Rect::new(PAD + 8.0, self.rule_y() + 6.0, W - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.07));
        let r = self.settings_row();
        let hot = f.hover(SETTINGS, 250, EASE);
        c.fill(r, 12.0, Rgba::white(0.07 * hot));
        c.icon(KEYBOARD, (r.x + 12.0 + 8.5, r.cy()), 17.0, 2.0, KEY_INK, 0.0);
        let label = t!("Языковые параметры", "Language settings", ja: "言語設定", pt: "Configurações de idioma", zh: "语言设置");
        c.text(label, Rect::new(r.x + 12.0 + 17.0 + 12.0, r.y, 160.0, r.h), Font::new(13.0, 500), INK, Align::Start);
        c.icon(CHEVRON_RIGHT, (r.x + r.w - 12.0 - 7.0, r.cy()), 14.0, 2.4, DIM, 0.0);
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        if self.settings_row().contains(x, y) {
            return Some(SETTINGS);
        }
        (0..self.items.len()).find(|&i| Self::row(i).contains(x, y)).map(|i| i as i32)
    }

    fn click(&mut self, id: i32, _b: Button, cx: &mut Cx) {
        if id == SETTINGS {
            let f = self.settings;
            return cx.close_then(f);
        }
        let Some(it) = self.items.get(id as usize) else { return };
        let (pick, to) = (self.pick, it.id);
        cx.close_then(move || pick(to));
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let order: Vec<i32> = (0..self.items.len() as i32).chain([SETTINGS]).collect();
        let at = from.and_then(|f| order.iter().position(|&o| o == f));
        let n = order.len();
        let i = match (at, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n - 1,
            (Some(i), Dir::Up | Dir::Left) => i.saturating_sub(1),
            (Some(i), Dir::Down | Dir::Right) => (i + 1).min(n - 1),
        };
        order.get(i).copied()
    }
}
