//! The restart / shut down dialog (the mockup's `<!-- Power dialog -->` and `busy` screen): the whole screen goes dark and blurred
//! (`blur(16px) saturate(.8)` under `rgba(5,7,18,.62)`), and a card in its middle asks. A ring of the accent colour round the
//! icon runs down over a minute; when it is out, or on Enter or the action button, the command runs and a black "Restarting…"
//! screen with a spinner follows. Esc, Cancel and a click beside the card refuse.

use super::anim::{self, EASE, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::icons;
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Frame, Id, Look, Origin, Place, Pose, Shadow};
use winrust_common::style::{self, pal};

/// The card: `width: 400px; padding: 32px 24px 20px`, and what the mockup's DOM measures at 1440x900.
const CARD_W: f32 = 400.0;
const CARD_H: f32 = 338.0;
const PAD_X: f32 = 24.0;

/// `setInterval` of a second, 60 times.
const COUNT_MS: u64 = 60_000;
/// The mockup takes the busy screen away after 2.6 s.
const BUSY_MS: u64 = 2600;

const CANCEL: i32 = 0;
const ACTION: i32 = 1;
const BACKDROP: i32 = 2;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const DIM: Rgba = Rgba::css(163, 169, 184, 1.0);
const ON_ACCENT: Rgba = Rgba::css(11, 18, 32, 1.0);
const WARN: Rgba = Rgba::css(243, 201, 139, 1.0);
const WARN_BG: Rgba = Rgba::css(255, 190, 90, 0.1);
const CARD_BG: Rgba = Rgba::css(30, 31, 38, 0.96);
const BLACK: Rgba = Rgba::css(4, 5, 11, 1.0);
const TRIANGLE: &str = "M12 3l10 18H2zM12 10v5M12 18h.01";

const VK_RETURN: u32 = 0x0D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Restart,
    Shutdown,
}

pub struct PowerPanel {
    kind: Kind,
    /// Runs the command (`shutdown /r|/s /t 0`).
    run: fn(Kind),
    screen: (f32, f32),
    /// The command has run: the busy screen is up.
    done: bool,
    /// The age of the panel (ms) when the busy screen came.
    busy_at: Option<u64>,
}

impl PowerPanel {
    #[must_use]
    pub const fn new(kind: Kind, run: fn(Kind)) -> Self {
        Self { kind, run, screen: (1440.0, 900.0), done: false, busy_at: None }
    }

    /// The panel as it is `at` ms into the busy screen, without running anything (for the picture of it).
    #[must_use]
    pub const fn busy_since(mut self, at: u64) -> Self {
        self.done = true;
        self.busy_at = Some(at);
        self
    }

    fn confirm(&mut self) {
        if !self.done {
            self.done = true;
            (self.run)(self.kind);
        }
    }

    fn card(&self) -> Rect {
        let (w, h) = self.screen;
        Rect::new(((w - CARD_W) / 2.0).round(), ((h - CARD_H) / 2.0).round(), CARD_W, CARD_H)
    }

    /// The two buttons: `flex: 1` each, 10 apart, 46 high, 26 under the warning.
    fn buttons(&self) -> (Rect, Rect) {
        let k = self.card();
        let w = (CARD_W - 2.0 * PAD_X - 10.0) / 2.0;
        let y = k.y + CARD_H - 20.0 - 46.0;
        (Rect::new(k.x + PAD_X, y, w, 46.0), Rect::new(k.x + PAD_X + w + 10.0, y, w, 46.0))
    }

    fn title(&self) -> &'static str {
        match self.kind {
            Kind::Restart => t!("Перезагрузка", "Restart", ja: "再起動", pt: "Reiniciar", zh: "重启"),
            Kind::Shutdown => t!("Завершение работы", "Shut down", ja: "シャットダウン", pt: "Desligar", zh: "关机"),
        }
    }

    fn action(&self) -> &'static str {
        match self.kind {
            Kind::Restart => t!("Перезагрузить", "Restart", ja: "再起動", pt: "Reiniciar", zh: "重启"),
            Kind::Shutdown => t!("Выключить", "Shut down", ja: "シャットダウン", pt: "Desligar", zh: "关机"),
        }
    }

    fn line(&self) -> &'static str {
        match self.kind {
            Kind::Restart => t!("Перезагрузка автоматически через", "Restarting automatically in", ja: "自動再起動まで", pt: "Reiniciando automaticamente em", zh: "自动重启倒计时"),
            Kind::Shutdown => t!("Выключение автоматически через", "Shutting down automatically in", ja: "自動シャットダウンまで", pt: "Desligando automaticamente em", zh: "自动关机倒计时"),
        }
    }

    fn busy_text(&self) -> &'static str {
        match self.kind {
            Kind::Restart => t!("Перезагрузка…", "Restarting…", ja: "再起動しています…", pt: "Reiniciando…", zh: "正在重启…"),
            Kind::Shutdown => t!("Завершение работы…", "Shutting down…", ja: "シャットダウンしています…", pt: "Desligando…", zh: "正在关机…"),
        }
    }

    /// The card, at rest, in the screen's coordinates.
    fn draw_card(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let k = self.card();
        let accent = Rgba::colorref(pal().accent);

        // `0 0 0 1px rgba(0,0,0,.6), inset 0 0 0 1px rgba(255,255,255,.1), 0 40px 120px rgba(0,0,0,.7)`
        c.shadow(k, 28.0, 40.0, 120.0, Rgba::css(0, 0, 0, 0.7));
        c.ring(k.inset(-1.0), 29.0, 1.0, Rgba::css(0, 0, 0, 0.6));
        c.fill(k, 28.0, CARD_BG);
        c.ring(k, 28.0, 1.0, Rgba::white(0.1));

        // the icon: a track, the ring that runs down (`stroke-dashoffset` 0 to 276.5 over 60 s, linear), the soft disc, the glyph
        let ic = (k.cx(), k.y + 32.0 + 48.0);
        c.ring(Rect::new(ic.0 - 46.0, ic.1 - 46.0, 92.0, 92.0), 46.0, 4.0, Rgba::white(0.1));
        let left = (1.0 - f.age as f32 / COUNT_MS as f32).clamp(0.0, 1.0);
        c.arc(ic, 44.0, -90.0, 360.0 * left, 4.0, accent);
        c.circle(ic.0, ic.1, 38.0, accent.alpha(0.2));
        let glyph = match self.kind {
            Kind::Restart => icons::RESTART,
            Kind::Shutdown => icons::POWER,
        };
        // `micIn .7s`: scale .8 to 1 and opacity
        f.rise(0, 700, SPRING, 0.0, 0.8).draw(c, ic, |c| c.icon(glyph, ic, 34.0, 1.8, accent, 0.0));

        c.text(self.title(), Rect::new(k.x, k.y + 148.0, CARD_W, 26.0), Font::new(22.0, 600).tracking(-0.02), INK, Align::Center);

        // "Restarting automatically in [59 s]": the text, a space and the chip, the whole line centred
        let text = self.line();
        let tf = Font::new(14.0, 400);
        let tw = c.measure(text, tf);
        let space = c.measure("n n", tf) - 2.0 * c.measure("n", tf);
        let secs = secs_left(f.age);
        let count = format!("{secs} {}", t!("с", "s", ja: "秒", pt: "s", zh: "秒"));
        let cf = Font::new(14.0, 600).tabular();
        let cw = c.measure(&count, cf).max(34.0) + 14.0;
        let x0 = k.cx() - (tw + space + cw) / 2.0;
        c.text(text, Rect::new(x0, k.y + 181.0, tw + 4.0, 17.0), tf, DIM, Align::Start);
        let chip = Rect::new(x0 + tw + space, k.y + 180.0, cw, 19.0);
        c.fill(chip, 7.0, Rgba::white(0.1));
        c.text(&count, chip, cf, Rgba::pure_white(1.0), Align::Center);

        // the amber warning
        let warn = t!(
            "Несохранённые данные будут потеряны",
            "Unsaved work in open apps will be lost",
            ja: "開いているアプリの未保存の作業は失われます",
            pt: "Trabalho não salvo será perdido",
            zh: "打开的应用中未保存的内容将丢失"
        );
        let wf = Font::new(12.0, 400);
        let wt = c.measure(warn, wf);
        let ww = 12.0 + 15.0 + 8.0 + wt + 12.0;
        let w = Rect::new(k.cx() - ww / 2.0, k.y + 213.0, ww, 33.0);
        c.fill(w, 12.0, WARN_BG);
        c.icon(TRIANGLE, (w.x + 12.0 + 7.5, w.cy()), 15.0, 2.0, WARN, 0.0);
        c.text(warn, Rect::new(w.x + 12.0 + 15.0 + 8.0, w.y, wt + 4.0, w.h), wf, WARN, Align::Start);

        let (cancel, action) = self.buttons();
        button(c, f, CANCEL, cancel, t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消"), Rgba::white(0.09), INK);
        button(c, f, ACTION, action, self.action(), accent, ON_ACCENT);
    }

    /// `.blk`: a black screen that fades in over .5 s with a spinner and what is going on.
    fn draw_busy(&self, c: &mut Canvas<'_>, f: &Frame<'_>, since: u64) {
        let (w, h) = f.size;
        let accent = Rgba::colorref(pal().accent);
        let (dur, _) = anim::timing(style::anim(), 500, 0);
        let e = if dur == 0 { 1.0 } else { EASE.ease((f.age.saturating_sub(since) as f32 / dur as f32).clamp(0.0, 1.0)) };
        c.faded(e, |c| {
            c.fill(Rect::new(0.0, 0.0, w, h), 0.0, BLACK);
            // a 36 px spinner, 20 above a 19 px line, the two centred on the screen
            let top = (h - 75.0) / 2.0;
            let (cx, cy) = (w / 2.0, top + 18.0);
            c.ring(Rect::new(cx - 16.5, cy - 16.5, 33.0, 33.0), 16.5, 3.0, Rgba::white(0.12));
            let turn = (f.now % 900) as f32 / 900.0 * 360.0; // `spin .9s linear infinite`
            c.arc((cx, cy), 15.0, -90.0 + turn, 90.0, 3.0, accent);
            c.text(self.busy_text(), Rect::new(0.0, top + 56.0, w, 19.0), Font::new(15.0, 500), DIM, Align::Center);
        });
    }
}

/// Whole seconds the countdown shows `age` ms after the start.
fn secs_left(age: u64) -> u64 {
    (COUNT_MS / 1000).saturating_sub(age / 1000)
}

/// CSS `filter: brightness(1.15)`.
fn brighter(c: Rgba) -> Rgba {
    let c = c.themed(); // brightness of what is drawn, in either mode
    Rgba::new((c.r * 1.15).min(1.0), (c.g * 1.15).min(1.0), (c.b * 1.15).min(1.0), c.a)
}

/// `.mb`: `filter .2s ease` to brightness 1.15 on hover, `transform .2s` of the mockup's spring to .96 while held.
fn button(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: i32, r: Rect, label: &str, bg: Rgba, ink: Rgba) {
    let hot = f.hover(id, 200, EASE);
    let k = f.tween(id, 3, if f.press == Some(id) { 0.96 } else { 1.0 }, 200, SPRING);
    c.transformed(r.cx(), r.cy(), k, 0.0, 0.0, |c| {
        c.fill(r, 14.0, bg.mix(brighter(bg), hot));
        c.text(label, r, Font::new(14.0, 600), ink.mix(brighter(ink), hot), Align::Center);
    });
}

impl Content for PowerPanel {
    fn id(&self) -> Id {
        Id::Power
    }

    /// The whole screen: `backdrop-filter: blur(16px) saturate(.8)` under `rgba(5,7,18,.62)`; the card draws its own rings and
    /// shadow.
    fn look(&self) -> Look {
        Look {
            radius: 0.0,
            tint: Rgba::css(5, 7, 18, 0.62),
            blur: 16.0,
            saturate: 0.8,
            ring_out: Rgba::TRANSPARENT,
            ring_in: Rgba::TRANSPARENT,
            top_light: Rgba::TRANSPARENT,
            shadow: Shadow { dy: 0.0, blur: 0.0, color: Rgba::TRANSPARENT },
            scrim: None,
        }
    }

    /// `dimIn .35s ease` of the dim layer (the card's own `dlgIn` is in `draw`).
    fn entry(&self) -> Entry {
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 1.0, clip: None }, dur: 350, ease: EASE, origin: Origin::Frac(0.5, 0.5) }
    }

    fn place(&self) -> Place {
        Place { along: Along::Fill, gap: 0.0 }
    }

    fn fit(&mut self, screen: (f32, f32)) {
        self.screen = screen;
    }

    fn modal(&self) -> bool {
        true
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.screen
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        if self.done {
            let since = *self.busy_at.get_or_insert(f.age);
            return self.draw_busy(c, f, since);
        }
        let k = self.card();
        // `dlgIn .5s`: opacity 0, translateY(24px) scale(.9) to rest, around the card's centre
        f.rise(0, 500, SPRING, 24.0, 0.9).draw(c, (k.cx(), k.cy()), |c| self.draw_card(c, f));
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        if self.done {
            return None;
        }
        let (cancel, action) = self.buttons();
        if cancel.contains(x, y) {
            Some(CANCEL)
        } else if action.contains(x, y) {
            Some(ACTION)
        } else if self.card().contains(x, y) {
            None
        } else {
            Some(BACKDROP)
        }
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if b != Button::Left || self.done {
            return;
        }
        if id == ACTION {
            self.confirm();
        } else {
            cx.close();
        }
    }

    /// Enter confirms wherever the hover is; once confirmed nothing asks anything any more.
    fn key(&mut self, vk: u32, _cx: &mut Cx) -> bool {
        if vk == VK_RETURN {
            self.confirm();
        }
        vk == VK_RETURN || self.done
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        Some(match (from, dir) {
            (_, Dir::First) | (Some(ACTION), Dir::Up | Dir::Left) => CANCEL,
            (_, Dir::Last) | (Some(CANCEL), Dir::Down | Dir::Right) => ACTION,
            (None, Dir::Down | Dir::Right) => CANCEL,
            (None, Dir::Up | Dir::Left) => ACTION,
            (Some(id), _) => id,
        })
    }

    fn tick(&mut self, age: u64, cx: &mut Cx) {
        if !self.done {
            if age >= COUNT_MS {
                self.confirm();
                self.busy_at = Some(age);
            }
        } else if self.busy_at.is_some_and(|b| age >= b + BUSY_MS) {
            cx.close();
        }
    }

    /// The icon comes in over .7 s.
    fn settle(&self) -> u32 {
        700
    }

    fn animating(&self, _now: u64) -> bool {
        self.done
    }

    /// The ring runs down; 20 frames a second are plenty for a minute-long arc.
    fn pulse(&self) -> u32 {
        if self.done { 0 } else { 50 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static RUNS: AtomicUsize = AtomicUsize::new(0);

    fn run(_: Kind) {
        RUNS.fetch_add(1, Relaxed);
    }

    #[test]
    fn the_minute_runs_out_by_itself_and_the_busy_screen_goes_after_it() {
        assert_eq!([secs_left(0), secs_left(999), secs_left(1000), secs_left(59_999), secs_left(60_000)], [60, 60, 59, 1, 0]);
        let mut p = PowerPanel::new(Kind::Restart, run);
        let mut cx = Cx::default();
        p.tick(59_999, &mut cx);
        assert_eq!(RUNS.load(Relaxed), 0);
        p.tick(60_000, &mut cx);
        p.tick(61_000, &mut cx);
        assert_eq!(RUNS.load(Relaxed), 1, "the command runs once");
        assert!(!cx.close_asked());
        p.tick(60_000 + BUSY_MS, &mut cx);
        assert!(cx.close_asked());
        assert_eq!(p.hit(720.0, 450.0), None, "nothing answers on the busy screen");
    }

    #[test]
    fn a_click_beside_the_card_refuses_and_the_buttons_are_where_the_mockup_has_them() {
        let p = PowerPanel::new(Kind::Shutdown, run);
        assert_eq!(p.hit(10.0, 10.0), Some(BACKDROP));
        assert_eq!(p.hit(720.0, 300.0), None); // the card's own empty part
        let (cancel, action) = p.buttons();
        assert_eq!((cancel.x, cancel.y, cancel.w), (544.0, 553.0, 171.0));
        assert_eq!((action.x, action.y, action.w, action.h), (725.0, 553.0, 171.0, 46.0));
        assert_eq!(p.hit(cancel.cx(), cancel.cy()), Some(CANCEL));
        assert_eq!(p.hit(action.cx(), action.cy()), Some(ACTION));
    }
}
