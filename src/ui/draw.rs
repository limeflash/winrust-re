//! How the window is drawn: the measures are the mockup's (Installer.html, read from its DOM at 1000 × 640), the colours its CSS vars.
use std::collections::HashMap;

use winrust_common::style;
use winrust_ui::anim::{CubicBezier, LINEAR, SPRING};
use winrust_ui::frame::{Cursor, Frame, Hot, Id, WIN_CLOSE};
use winrust_ui::gfx::{Align, Canvas, Font, Rect, Rgba, Sample, Xf};
use winrust_ui::widgets::look as ui_look;
use winrust_ui::widgets::look::{T1, T2, T3};
use winrust_ui::widgets::scroll;

use super::app::{ID_BACK, ID_CANCEL, ID_GROUP, ID_INSTALL, ID_LANG, ID_LIST, ID_NEWER, ID_OD, ID_REMOVE_ALL, ID_ROW, ID_SIGN_OUT, ID_UPDATE, Installer, LANGS, Phase, Run};
use super::catalog::{FOLDER_BACK, FOLDER_FRONT, PULSE_BOX, PULSE_LINE, ROWS, Tile};
use super::look;
use super::text::pick;

pub const TITLE_H: f32 = 56.0;
pub const FOOT_H: f32 = 68.0;
pub const LIST_W: f32 = 430.0;
const CARD_H: f32 = 322.0;

// the keys of the tweens an item does not own
const K_CARD: i32 = 9001; // and the next
const K_TEXT: i32 = 9003; // and the next
const K_OK: i32 = 9005;
const K_BAR: i32 = 9006;

const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const SPIN: &str = "M12 3a9 9 0 1 0 9 9";
const CLOSE: &str = "M7 7l10 10M17 7L7 17";
const INFO: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01";
const LOGO_R: &str = "M14.5 5h3.5a3 3 0 0 1 0 6h-3.5zM14.5 11l5.5 9M14.5 5v15";

/// An animation started at will, as the mockup's `animation: <name> <dur> <ease> <delay>`: 0 to 1, restarted when `fresh`.
fn play(f: &mut Frame<'_>, key: i32, fresh: bool, dur: u32, delay: u32, ease: CubicBezier) -> f32 {
    if fresh {
        f.tween(key, 0, 0.0, 0, LINEAR);
    }
    let total = dur + delay;
    let raw = f.tween(key, 0, 1.0, total, LINEAR);
    ease.ease(((raw * total as f32 - delay as f32) / dur as f32).clamp(0.0, 1.0))
}

fn focus_ring(c: &mut Canvas<'_>, r: Rect, radius: f32, hot: Hot) {
    if hot.focus {
        c.ring(r.inset(-3.0), radius + 3.0, 2.0, ui_look::focus_ring().fade(0.9));
    }
}

/// A pill of text: `bg` behind it, `fg` on it. Returns its width.
#[allow(clippy::too_many_arguments)]
fn pill(c: &mut Canvas<'_>, x: f32, cy: f32, h: f32, pad: f32, radius: f32, text: &str, font: Font, bg: Rgba, fg: Rgba) -> f32 {
    let w = c.measure(text, font).ceil() + 2.0 * pad;
    let r = Rect::new(x, cy - h / 2.0, w, h);
    c.fill(r, radius, bg);
    c.text(text, r, font, fg, Align::Center);
    w
}

/// A button of the footer: `.btn`, 38 high, radius 10, pressed it shrinks to .98.
#[allow(clippy::too_many_arguments)]
fn button(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, label: &str, weight: u16, bg: Rgba, fg: Rgba, lift: Rgba, enabled: bool) {
    let hot = if enabled { f.focusable(id, r) } else { f.item(id, r) };
    if enabled {
        f.hand();
    }
    let k = 1.0 - 0.02 * hot.press;
    c.transformed(r.cx(), r.cy(), k, 0.0, 0.0, |c| {
        c.fill(r, 10.0, bg);
        if enabled && hot.hover > 0.0 {
            c.fill(r, 10.0, lift.fade(hot.hover));
        }
        c.text(label, r, Font::new(13.0, weight), fg, Align::Center);
    });
    focus_ring(c, r, 10.0, hot);
}

impl Installer {
    pub(super) fn paint(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        self.win = (w, h);
        self.drawn_light = style::light();
        self.hover_peek(f.hover);
        let focus = if f.mo.hits.focus_visible { f.mo.hits.focus } else { None };
        self.follow_focus(focus, f.now);
        if self.phase == Phase::Busy {
            // the spinner turns: a frame every 16 ms while the job runs, and none after it
            f.due(f.now + 16);
        }
        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, look::WIN);
        let body_h = h - TITLE_H - FOOT_H;
        self.draw_list(c, f, Rect::new(0.0, TITLE_H, LIST_W, body_h));
        self.draw_pane(c, f, Rect::new(LIST_W, TITLE_H, (w - LIST_W).max(0.0), body_h));
        self.draw_title(c, f, w);
        self.draw_footer(c, f, w, h);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the title bar

    fn draw_title(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        let t = self.t();
        let acc = ui_look::accent();
        c.fill(Rect::new(0.0, 0.0, w, TITLE_H), 0.0, look::BAR);
        c.fill(Rect::new(0.0, TITLE_H, w, 1.0), 0.0, Rgba::white(0.05));
        // the mark: two squares of the accent and an R
        let k = 28.0 / 24.0;
        let (lx, ly) = (18.0, 14.0);
        c.fill(Rect::new(lx + 2.0 * k, ly + 2.0 * k, 9.5 * k, 9.5 * k), 2.4 * k, acc);
        c.fill(Rect::new(lx + 2.0 * k, ly + 12.5 * k, 9.5 * k, 9.5 * k), 2.4 * k, acc.fade(0.72));
        c.icon(LOGO_R, (lx + 14.0, ly + 14.0), 28.0, 2.0, T1, 0.0);
        // the name, the version and the line under them
        let name_f = Font::new(15.0, 600).tracking(-0.01);
        let nw = c.measure("WinRust", name_f).ceil();
        c.text("WinRust", Rect::new(58.0, 10.5, nw + 2.0, 18.0), name_f, T1, Align::Start);
        let mut x = 58.0 + nw + 8.0;
        x += pill(c, x, 19.5, 18.0, 7.0, 9.0, &self.version, Font::new(11.0, 500), Rgba::white(0.07), T2) + 8.0;
        if let Some(v) = self.newer_version() {
            let label = t.newer(&v);
            let fnt = Font::new(11.0, 600);
            let pw = c.measure(&label, fnt).ceil() + 14.0;
            let r = Rect::new(x, 10.5, pw, 18.0);
            let hot = f.focusable(ID_NEWER, r);
            f.hand();
            f.tip(label.clone());
            c.fill(r, 9.0, acc.alpha(0.22 + 0.1 * hot.hover));
            c.text(&label, r, fnt, acc, Align::Center);
            focus_ring(c, r, 9.0, hot);
        }
        c.text_fit(t.tagline, Rect::new(58.0, 29.5, (w - 58.0 - 420.0).max(120.0), 16.0), Font::new(12.0, 400), T3, Align::Start);

        // the close button and the language switch, right to left
        let cb = Rect::new(w - 12.0 - 30.0, 13.0, 30.0, 30.0);
        let hot = f.focusable(WIN_CLOSE, cb);
        f.hand();
        f.tip(t.close);
        c.transformed(cb.cx(), cb.cy(), 1.0 - 0.02 * hot.press, 0.0, 0.0, |c| {
            c.fill(cb, 15.0, Rgba::white(0.06 + 0.03 * hot.hover));
            c.icon(CLOSE, (cb.cx(), cb.cy()), 12.0, 2.4, T2, 0.0);
        });
        focus_ring(c, cb, 15.0, hot);
        let seg_f = Font::new(12.0, 500);
        let widths: Vec<f32> = LANGS.iter().map(|l| c.measure(l.name(), seg_f).ceil() + 22.0).collect();
        let gw = widths.iter().sum::<f32>() + 4.0;
        let g = Rect::new(cb.x - 16.0 - gw, 12.0, gw, 32.0);
        c.fill(g, 10.0, look::WELL);
        c.ring(g, 10.0, 1.0, Rgba::white(0.06));
        let mut sx = g.x + 2.0;
        for (i, l) in LANGS.iter().enumerate() {
            let b = Rect::new(sx, 14.0, widths[i], 28.0);
            sx += widths[i];
            let hot = f.focusable(ID_LANG + i as Id, b);
            f.hand();
            let on = f.tween(ID_LANG + i as Id, 2, f32::from(*l == self.lang), 160, SPRING);
            c.fill(b, 8.0, look::SEG_ON.fade(on));
            c.text(l.name(), b, seg_f, T3.mix(T1, on), Align::Center);
            focus_ring(c, b, 8.0, hot);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the list

    fn draw_list(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.fill(Rect::new(r.x + r.w - 1.0, r.y, 1.0, r.h), 0.0, Rgba::white(0.05));
        let (rows, heads, content_h) = Self::layout();
        let view = Rect::new(r.x, r.y, r.w - 1.0, r.h);
        let runs = if self.phase == Phase::Idle { HashMap::new() } else { self.runs() };
        let mut sc = std::mem::take(&mut self.scroll);
        let row_w = view.w - 20.0 - if sc.scrollable() { 10.0 } else { 0.0 };
        scroll::scroll(c, f, ID_LIST, view, &mut sc, content_h, |c, f, v| {
            for g in 0..3u8 {
                let y = v.at(heads[g as usize]);
                if y > view.y + view.h || y + 36.0 < view.y {
                    continue;
                }
                self.draw_group(c, f, g, Rect::new(10.0, y, row_w, 36.0));
            }
            for &(r, y) in &rows {
                let ry = v.at(y);
                if ry > view.y + view.h || ry + 54.0 < view.y {
                    continue;
                }
                self.draw_row(c, f, r, Rect::new(10.0, ry, row_w, 54.0), &runs);
            }
        });
        self.scroll = sc;
    }

    fn draw_group(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, g: u8, r: Rect) {
        let t = self.t();
        let title = [t.g_shell, t.g_apps, t.g_sys][g as usize].to_uppercase();
        c.text(&title, Rect::new(r.x + 10.0, r.y + 6.0, r.w - 80.0, 30.0), Font::new(11.0, 600).tracking(0.06), look::T4, Align::Start);
        if self.phase == Phase::Idle && self.model.group_rows(g).len() > 1 {
            let label = if self.model.group_all_on(g) { t.none } else { t.all };
            let fnt = Font::new(11.5, 500);
            let bw = c.measure(label, fnt).ceil() + 16.0;
            let b = Rect::new(r.x + r.w - 10.0 - bw, r.y + 10.0, bw, 22.0);
            let hot = f.focusable(ID_GROUP + Id::from(g), b);
            f.hand();
            c.fill(b, 6.0, Rgba::white(0.09 * hot.hover));
            c.text(label, b, fnt, T3, Align::Center);
            focus_ring(c, b, 6.0, hot);
        }
    }

    fn draw_row(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: usize, rect: Rect, runs: &HashMap<usize, Run>) {
        let row = &ROWS[r];
        let t = self.t();
        let id = ID_ROW + r as Id;
        let live = self.model.live(r);
        let soon = !live && self.model.parent(r).is_none();
        let riding = self.model.parent(r).is_some();
        let run = runs.get(&r).copied();
        let busy = self.phase != Phase::Idle;
        let op = if soon {
            0.55
        } else if busy && run.is_none() {
            0.4
        } else {
            1.0
        };
        let hot = f.focusable(id, rect);
        if busy || soon || riding {
            f.cursor(Cursor::Arrow);
        } else {
            f.hand();
        }
        let peeked = r == self.peek && self.phase != Phase::Done;
        let hl = f.tween(id, 3, f32::from(peeked), 160, SPRING);
        let on = self.model.on(r);
        let acc = ui_look::accent();
        let tog = f.tween(id, 2, f32::from(on), 220, SPRING);
        let st_in = f.tween(id, 4, f32::from(run.is_some()), 250, SPRING);
        let peek_now = hl;
        c.faded(op, |c| {
            // the plate: peeked 6 %, hovered 4.5 %, with a hairline ring while peeked
            let a = 0.06 * peek_now * (1.0 - hot.hover) + 0.045 * hot.hover;
            c.fill(rect, 12.0, Rgba::white(a));
            c.ring(rect, 12.0, 1.0, Rgba::white(0.07 * peek_now));
            // the tile
            let tile = Rect::new(rect.x + 10.0, rect.y + 10.0, 34.0, 34.0);
            // an opaque tile hides what is under it, so its shadow needs no cut-out (a layer each: it was most of a frame)
            c.shadow(tile, 10.0, 4.0, 10.0, Rgba::new(0.0, 0.0, 0.0, 0.25));
            match row.tile {
                Tile::Gradient(a, b) => {
                    c.gradient_angle(tile, 10.0, 140.0, &[(0.0, look::rgb(a)), (1.0, look::rgb(b))]);
                    c.icon(row.icon, (tile.cx(), tile.cy()), 17.0, 2.0, Rgba::pure_white(1.0), 0.0);
                }
                Tile::Folder => {
                    c.fill(tile, 10.0, Rgba::white(0.06));
                    c.glyph(FOLDER_BACK, (tile.cx(), tile.cy()), 24.0, look::rgb(0xd99a16));
                    c.glyph(FOLDER_FRONT, (tile.cx(), tile.cy()), 24.0, look::rgb(0xf6c453));
                }
                Tile::Pulse => {
                    c.fill(tile, 10.0, Rgba::white(0.06));
                    c.glyph(PULSE_BOX, (tile.cx(), tile.cy()), 24.0, look::rgb(0x1f6f48));
                    c.icon(PULSE_LINE, (tile.cx(), tile.cy()), 24.0, 2.0, look::rgb(0x7ff0b4), 0.0);
                }
            }
            c.ring(tile, 10.0, 1.0, Rgba::white(0.1));

            // what stands at the right: the switch, a state of the job, or a word
            let right = rect.x + rect.w - 10.0;
            let mut text_end = right;
            if live && !busy {
                let sw = Rect::new(right - 38.0, rect.y + 16.0, 38.0, 22.0);
                c.fill(sw, 11.0, Rgba::white(0.14).mix(acc, tog));
                let k = Rect::new(sw.x + 2.0 + 16.0 * tog, sw.y + 2.0, 18.0, 18.0);
                c.shadow(k, 9.0, 1.0, 2.0, Rgba::new(0.0, 0.0, 0.0, 0.35));
                c.fill(k, 9.0, Rgba::pure_white(1.0));
                text_end = sw.x - 12.0;
            } else if let Some(run) = run {
                // `.st`: slides in 4 px and fades
                let word = match run {
                    Run::Done => t.done_word,
                    Run::Now => "",
                    Run::Queued => t.queued,
                    Run::Failed => t.failed_word,
                };
                let ink = match run {
                    Run::Done => look::OK,
                    Run::Now => acc,
                    Run::Queued => look::T4,
                    Run::Failed => look::BAD,
                };
                let fnt = Font::new(12.0, 500);
                let ww = if word.is_empty() { 0.0 } else { c.measure(word, fnt).ceil() };
                let icon_w = if matches!(run, Run::Queued) { 0.0 } else { 15.0 };
                let gap = if icon_w > 0.0 && ww > 0.0 { 6.0 } else { 0.0 };
                let total = icon_w + gap + ww;
                let x0 = right - total - 4.0 * (1.0 - st_in);
                c.faded(st_in, |c| {
                    match run {
                        Run::Done => c.icon(CHECK, (x0 + 7.5, rect.cy()), 15.0, 2.6, ink, 0.0),
                        Run::Failed => c.icon(CLOSE, (x0 + 7.5, rect.cy()), 15.0, 2.6, ink, 0.0),
                        Run::Now => c.icon(SPIN, (x0 + 7.5, rect.cy()), 15.0, 2.6, ink, (f.now % 900) as f32 / 900.0 * 360.0),
                        Run::Queued => {}
                    }
                    if ww > 0.0 {
                        c.text(word, Rect::new(x0 + icon_w + gap, rect.y + 17.0, ww + 2.0, 20.0), fnt, ink, Align::Start);
                    }
                });
                text_end = right - total - 12.0;
            } else if soon || riding {
                let word = if soon { t.soon } else { t.included };
                let fnt = Font::new(12.0, 400);
                let ww = c.measure(word, fnt).ceil();
                c.text(word, Rect::new(right - ww - 2.0, rect.y + 17.0, ww + 2.0, 20.0), fnt, look::T4, Align::Start);
                text_end = right - ww - 14.0;
            }

            // the name with its tags, and the line under it
            let tx = rect.x + 56.0;
            let name_f = Font::new(13.0, 600);
            let nw = c.measure(row.name, name_f).ceil();
            c.text(row.name, Rect::new(tx, rect.y + 10.0, nw + 2.0, 16.0), name_f, T1, Align::Start);
            let mut x = tx + nw + 8.0;
            let cy = rect.y + 18.0;
            if row.new {
                x += pill(c, x, cy, 17.0, 6.0, 6.0, t.new_tag, Font::new(10.5, 600), acc.alpha(0.22), acc) + 8.0;
            }
            if !row.key.is_empty() {
                let kf = Font::new(10.5, 500);
                let kw = c.measure(row.key, kf).ceil() + 12.0;
                let kr = Rect::new(x, cy - 7.5, kw, 15.0);
                c.fill(kr, 5.0, Rgba::white(0.07));
                c.inset_edge(kr, 5.0, -1.0, Rgba::white(0.07));
                c.text(row.key, kr, kf, T2, Align::Center);
                x += kw + 8.0;
            }
            if self.model.installed(r) && !riding && x + 60.0 < text_end {
                pill(c, x, cy, 17.0, 6.0, 6.0, t.installed_chip, Font::new(10.5, 600), Rgba::white(0.07), T2);
            }
            // a few DIPs of the gap to the switch are the line's: Inter at this size is a little wider here than in the browser, and the
            // mockup's longest lines just fit
            c.text_fit(pick(self.lang, &row.short), Rect::new(tx, rect.y + 29.0, (text_end - tx + 6.0).max(10.0), 15.0), Font::new(12.0, 400), T3, Align::Start);
        });
        focus_ring(c, rect, 12.0, hot);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the preview, or the finished page

    fn draw_pane(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.gradient(r, 0.0, look::PANE_TOP, look::PANE_BOT);
        if self.phase == Phase::Done {
            return self.draw_done(c, f, r);
        }
        // the picture gives way when the text under it needs more (a narrow window, a long translation, a small screen)
        let w = (r.w - 52.0).max(100.0);
        let lines = c.lines(pick(self.lang, &ROWS[self.peek].long), Font::new(13.0, 400), w.min(520.0));
        let need = 23.0 + 10.0 + lines as f32 * 20.15 + 12.0 + 32.0;
        let card_h = (r.h - 22.0 - 20.0 - need - 14.0).clamp(200.0, CARD_H);
        let card = Rect::new(r.x + 26.0, r.y + 22.0, w, card_h);
        let fresh = std::mem::take(&mut self.shot_fresh);
        self.draw_card(c, f, card, fresh);
        self.draw_text_block(c, f, Rect::new(card.x, card.y + card_h + 20.0, card.w, need), fresh);
    }

    fn draw_card(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, card: Rect, fresh: bool) {
        let row = &ROWS[self.peek];
        let key = K_CARD + (self.shots % 2) as i32;
        let e = play(f, key, fresh, 380, 0, SPRING);
        let (op, dy, k) = (e, 8.0 * (1.0 - e), 0.985 + 0.015 * e);
        let light = self.drawn_light;
        let xf = Xf { cx: card.cx(), cy: card.cy(), k, dx: 0.0, dy };
        let s = c.scale();
        let (busy, prog) = (self.phase == Phase::Busy, self.progress());
        let now_step = self.now_step();
        let rolling = self.rolling_back();
        let t = self.t();
        let acc = ui_look::accent();
        let bar_to = if rolling { 0.0 } else { prog.2 };
        let fill = f.tween(K_BAR, 0, bar_to, 500, SPRING);
        let sweep = (f.now % 1400) as f32 / 1400.0;
        let pic = self.picture(c, row.shot, light);
        c.faded(op, |c| {
            c.transformed(card.cx(), card.cy(), k, 0.0, dy, |c| {
                // the card is opaque once it has come in: its shadow shows only outside it, with no cut-out
                if e < 0.999 {
                    c.outside(card, 14.0, |c| c.shadow(card, 14.0, 24.0, 60.0, Rgba::new(0.0, 0.0, 0.0, 0.45)));
                } else {
                    c.shadow(card, 14.0, 24.0, 60.0, Rgba::new(0.0, 0.0, 0.0, 0.45));
                }
            });
            c.masked(card, 14.0, xf, |c| {
                c.transformed(card.cx(), card.cy(), k, 0.0, dy, |c| {
                    c.fill(card, 14.0, look::CARD);
                    if let Some(p) = pic {
                        let (iw, ih) = (p.w * s, p.h * s);
                        let m = (card.w / iw).max(card.h / ih);
                        let (dw, dh) = (iw * m, ih * m);
                        let dst = Rect::new(card.x + (card.w - dw) / 2.0, card.y + (card.h - dh) * row.shot.pos_y(), dw, dh);
                        c.picture_in(p, dst, 0.0, 1.0, Sample::Smooth);
                    }
                    if busy {
                        // the job's line over the bottom of the picture
                        let strip = Rect::new(card.x, card.y + card.h - 70.0, card.w, 70.0);
                        c.gradient_angle(strip, 0.0, 0.0, &[(0.0, Rgba::css(8, 9, 12, 0.9)), (1.0, Rgba::css(8, 9, 12, 0.0))]);
                        let ink = Rgba::pure_white(0.93);
                        let (label, count) = if rolling {
                            (t.rolling_back.to_string(), String::new())
                        } else {
                            (now_step.as_ref().map_or(String::new(), |s| t.busy_on(s.install, ROWS[s.row].name)), format!("{} / {}", prog.0, prog.1))
                        };
                        let inner_w = card.w - 32.0;
                        c.text_fit(&label, Rect::new(card.x + 16.0, card.y + card.h - 42.0, inner_w - 60.0, 16.0), Font::new(13.0, 500).tabular(), ink, Align::Start);
                        c.text(&count, Rect::new(card.x + card.w - 16.0 - 80.0, card.y + card.h - 42.0, 80.0, 16.0), Font::new(13.0, 500).tabular(), Rgba::pure_white(0.66), Align::End);
                        let track = Rect::new(card.x + 16.0, card.y + card.h - 18.0, inner_w, 4.0);
                        c.fill(track, 2.0, Rgba::pure_white(0.14));
                        if rolling {
                            let (a, b) = ((-0.3 + 1.3 * sweep).max(0.0), (1.3 * sweep).min(1.0));
                            c.fill(Rect::new(track.x + track.w * a, track.y, track.w * (b - a).max(0.0), 4.0), 2.0, acc);
                        } else {
                            c.fill(Rect::new(track.x, track.y, track.w * fill.clamp(0.0, 1.0), 4.0), 2.0, acc);
                        }
                    }
                });
            });
            c.transformed(card.cx(), card.cy(), k, 0.0, dy, |c| {
                c.ring(card.inset(-1.0), 15.0, 1.0, Rgba::white(0.08));
            });
        });
    }

    fn draw_text_block(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, fresh: bool) {
        let row = &ROWS[self.peek];
        let t = self.t();
        let key = K_TEXT + (self.shots % 2) as i32;
        let e = play(f, key, fresh, 400, 40, SPRING);
        let (op, dy) = (e, 4.0 * (1.0 - e));
        let title_f = Font::new(19.0, 600).tracking(-0.01);
        let tw = c.measure(row.title, title_f).ceil();
        let long = pick(self.lang, &row.long);
        let desc_w = r.w.min(520.0);
        let lines = c.lines(long, Font::new(13.0, 400), desc_w);
        let desc_h = lines as f32 * 20.15;
        let facts: Vec<(&str, String)> = {
            let mut v = vec![(t.size, if row.size_mb > 0.0 { format!("{:.1} {}", row.size_mb, t.mb) } else { "—".to_string() }), (t.ram, row.ram.to_string())];
            if !row.key.is_empty() {
                v.push((t.key, row.key.replace(' ', " + ")));
            }
            v
        };
        let od_row = self.peek == super::catalog::index_of("topbar").unwrap_or(usize::MAX);
        let od_on = self.od;
        let od_enabled = self.phase == Phase::Idle;
        let (od_title, od_sub) = (t.od_title, t.od_sub);
        let acc = ui_look::accent();
        c.faded(op, |c| {
            c.transformed(r.cx(), r.y, 1.0, 0.0, dy, |c| {
                let mut y = r.y;
                c.text(row.title, Rect::new(r.x, y, tw + 2.0, 23.0), title_f, T1, Align::Start);
                let mut x = r.x + tw + 10.0;
                let mono = Font::new(12.0, 400).tabular();
                let nw = c.measure(row.name, mono).ceil();
                c.text(row.name, Rect::new(x, y + 3.5, nw + 2.0, 16.0), mono, T3, Align::Start);
                x += nw + 10.0;
                if let Some(rep) = &row.replaces {
                    let label = format!("{}{}", t.replaces, pick(self.lang, rep));
                    pill(c, x, y + 11.5, 20.0, 8.0, 10.0, &label, Font::new(11.5, 500), Rgba::white(0.06), look::T1B);
                }
                y += 23.0 + 10.0;
                c.text_wrapped(long, Rect::new(r.x, y, desc_w, desc_h), Font::new(13.0, 400), T2, 20.15, 0);
                y += desc_h + 10.0 + 2.0;
                let mut fx = r.x;
                for (label, value) in &facts {
                    let (lf, vf) = (Font::new(11.5, 400), Font::new(13.0, 500).tabular());
                    let w = c.measure(label, lf).max(c.measure(value, vf)).ceil();
                    c.text(label, Rect::new(fx, y, w + 2.0, 14.0), lf, look::T4, Align::Start);
                    c.text(value, Rect::new(fx, y + 16.0, w + 2.0, 16.0), vf, T1, Align::Start);
                    fx += w + 22.0;
                }
                if od_row {
                    // the bar's one setting, at the right end of the facts' row: a label and a switch
                    let fnt = Font::new(13.0, 500);
                    let lw = c.measure(od_title, fnt).ceil();
                    let rr = Rect::new(r.x + r.w - 38.0 - 10.0 - lw - 16.0, y + 5.0, 38.0 + 10.0 + lw + 16.0, 28.0);
                    if rr.x > fx + 8.0 {
                        let hot = f.focusable(ID_OD, rr);
                        f.hand();
                        f.tip(od_sub);
                        c.fill(rr, 9.0, Rgba::white(0.09 * hot.hover));
                        c.text(od_title, Rect::new(rr.x + 8.0, rr.y, lw + 2.0, rr.h), fnt, T1, Align::Start);
                        let sw = Rect::new(rr.x + rr.w - 8.0 - 38.0, rr.y + 3.0, 38.0, 22.0);
                        let tog = f.tween(ID_OD, 2, f32::from(od_on), 220, SPRING);
                        let dim = if od_enabled { 1.0 } else { 0.5 };
                        c.fill(sw, 11.0, Rgba::white(0.14).mix(acc, tog).fade(dim));
                        let k = Rect::new(sw.x + 2.0 + 16.0 * tog, sw.y + 2.0, 18.0, 18.0);
                        c.fill(k, 9.0, Rgba::pure_white(1.0).fade(dim));
                        focus_ring(c, rr, 9.0, hot);
                    }
                }
            });
        });
    }

    fn draw_done(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        let t = self.t();
        let fin = self.fin.clone().unwrap_or_default();
        let ok = fin.ok();
        let inner = Rect::new(r.x + 26.0, r.y + 22.0, (r.w - 52.0).max(100.0), (r.h - 40.0).max(100.0));
        // the chips wrap at 420 DIPs, centred
        let chip_f = Font::new(12.0, 500);
        let chips: Vec<(usize, bool, f32)> = fin.chips.iter().map(|&(row, good)| (row, good, c.measure(ROWS[row].name, chip_f).ceil() + 4.0 + 18.0 + 6.0 + 10.0)).collect();
        let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
        let mut used = 0.0;
        for (i, ch) in chips.iter().enumerate() {
            let need = ch.2 + if used > 0.0 { 6.0 } else { 0.0 };
            if used + need > 420.0 && used > 0.0 {
                lines.push(Vec::new());
                used = 0.0;
            }
            used += ch.2 + if used > 0.0 { 6.0 } else { 0.0 };
            lines.last_mut().unwrap().push(i);
        }
        let chip_rows = if chips.is_empty() { 0 } else { lines.len() };
        let (title, sub) = if fin.rollback && ok {
            (t.done_rollback.to_string(), t.done_rollback_sub)
        } else if !ok {
            (t.done_bad.to_string(), t.done_bad_sub)
        } else {
            (t.done_title(fin.added, fin.removed), t.done_sub)
        };
        let detail: Vec<String> = fin.errors.iter().chain(fin.notes.iter()).take(3).cloned().collect();
        let detail_h = if detail.is_empty() { 0.0 } else { c.lines(&detail.join("\n"), Font::new(12.0, 400), 460.0).min(4) as f32 * 17.5 + 14.0 };
        let chips_h = if chip_rows == 0 { 0.0 } else { 16.0 + 4.0 + chip_rows as f32 * 26.0 + (chip_rows as f32 - 1.0) * 6.0 };
        let total = 72.0 + 16.0 + (24.0 + 6.0 + 18.0) + chips_h + detail_h;
        let top = (inner.cy() - total / 2.0).round().max(inner.y);
        let cx = inner.cx();
        // the tick pops in: 0 to 1.06 to 1 over half a second
        let e = play(f, K_OK, std::mem::take(&mut self.ok_fresh), 500, 0, LINEAR);
        let (op, sc) = if e < 0.6 {
            let u = SPRING.ease(e / 0.6);
            (u, 0.6 + 0.46 * u)
        } else {
            (1.0, 1.06 - 0.06 * SPRING.ease((e - 0.6) / 0.4))
        };
        let tint = if ok { look::OK } else { look::BAD };
        let disc = Rect::new(cx - 36.0, top, 72.0, 72.0);
        c.faded(op, |c| {
            c.transformed(disc.cx(), disc.cy(), sc, 0.0, 0.0, |c| {
                c.fill(disc, 36.0, tint.alpha(0.18));
                c.ring(disc, 36.0, 1.5, tint.alpha(0.55));
                c.icon(if ok { CHECK } else { CLOSE }, (disc.cx(), disc.cy()), 34.0, 2.4, tint, 0.0);
            });
        });
        let mut y = top + 72.0 + 16.0;
        c.text_fit(&title, Rect::new(inner.x, y, inner.w, 24.0), Font::new(20.0, 600), T1, Align::Center);
        y += 24.0 + 6.0;
        c.text_fit(sub, Rect::new(inner.x, y, inner.w, 18.0), Font::new(13.0, 400), T2, Align::Center);
        y += 18.0;
        if chip_rows > 0 {
            y += 16.0 + 4.0;
            for ln in &lines {
                let rw: f32 = ln.iter().map(|&i| chips[i].2).sum::<f32>() + 6.0 * (ln.len() as f32 - 1.0);
                let mut x = cx - rw / 2.0;
                for &i in ln {
                    let (row, good, w) = chips[i];
                    let b = Rect::new(x, y, w, 26.0);
                    c.fill(b, 13.0, if good { Rgba::white(0.06) } else { look::BAD.alpha(0.16) });
                    let tile = Rect::new(b.x + 4.0, b.y + 4.0, 18.0, 18.0);
                    match ROWS[row].tile {
                        Tile::Gradient(a, bb) => {
                            c.gradient_angle(tile, 6.0, 140.0, &[(0.0, look::rgb(a)), (1.0, look::rgb(bb))]);
                            c.icon(ROWS[row].icon, (tile.cx(), tile.cy()), 11.0, 2.4, Rgba::pure_white(1.0), 0.0);
                        }
                        Tile::Folder => {
                            c.gradient_angle(tile, 6.0, 140.0, &[(0.0, look::rgb(0xf6c453)), (1.0, look::rgb(0xd99a16))]);
                            c.icon(ROWS[row].icon, (tile.cx(), tile.cy()), 11.0, 2.4, Rgba::pure_white(1.0), 0.0);
                        }
                        Tile::Pulse => {
                            c.gradient_angle(tile, 6.0, 140.0, &[(0.0, look::rgb(0x22c55e)), (1.0, look::rgb(0x15803d))]);
                            c.icon(ROWS[row].icon, (tile.cx(), tile.cy()), 11.0, 2.4, Rgba::pure_white(1.0), 0.0);
                        }
                    }
                    c.text(ROWS[row].name, Rect::new(tile.x + 18.0 + 6.0, b.y, w - 4.0 - 18.0 - 6.0 - 6.0, 26.0), chip_f, if good { T1 } else { look::BAD }, Align::Start);
                    x += w + 6.0;
                }
                y += 26.0 + 6.0;
            }
            y -= 6.0;
        }
        if !detail.is_empty() {
            let col = if ok { T3 } else { look::BAD };
            c.text_wrapped(&detail.join("\n"), Rect::new(cx - 230.0, y + 14.0, 460.0, 70.0), Font::new(12.0, 400), col, 17.5, 4);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the footer

    fn draw_footer(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32, h: f32) {
        let t = self.t();
        let r = Rect::new(0.0, h - FOOT_H, w, FOOT_H);
        c.fill(r, 0.0, look::BAR);
        c.fill(Rect::new(0.0, r.y - 1.0, w, 1.0), 0.0, Rgba::white(0.05));
        let ghost = Rgba::white(0.06);
        let ghost_lift = Rgba::white(0.09);
        let none = Rgba::TRANSPARENT;
        let mut put = Puts { right: w - 18.0, y: r.y + 15.0 };
        match self.phase {
            Phase::Idle => {
                let (label, enabled) = self.install_button();
                let (bg, fg) = if enabled { (T1, look::INSTALL_FG) } else { (Rgba::white(0.08), look::T4) };
                put.add(c, f, ID_INSTALL, &label, 600, 18.0, 170.0, bg, fg, Rgba::pure_white(0.1), enabled);
                let can = self.elevated && self.model.any_installed();
                put.add(c, f, ID_UPDATE, t.update, 500, 14.0, 0.0, ghost, if can { T1 } else { look::T4 }, ghost_lift, can);
                put.add(c, f, ID_REMOVE_ALL, t.remove_all, 500, 14.0, 0.0, none, look::REMOVE, Rgba::new(229.0 / 255.0, 72.0 / 255.0, 77.0 / 255.0, 0.14), self.elevated);
                let (n, mb) = self.model.selected();
                let x = 18.0;
                let wmax = (put.right - x).max(40.0);
                c.text_fit(&t.selected(n, mb), Rect::new(x, r.y + 17.5, wmax, 16.0), Font::new(13.0, 500), T1, Align::Start);
                let (note, ink) = if self.elevated { (t.note, T3) } else { (t.not_admin, look::BAD) };
                c.icon(INFO, (x + 6.5, r.y + 43.0), 13.0, 2.0, ink, 0.0);
                c.text_fit(note, Rect::new(x + 19.0, r.y + 35.5, (wmax - 19.0).max(40.0), 15.0), Font::new(12.0, 400), ink, Align::Start);
            }
            Phase::Busy => {
                put.add(c, f, ID_CANCEL, t.cancel, 500, 16.0, 0.0, ghost, if self.cancelling { look::T4 } else { T1 }, ghost_lift, !self.cancelling);
                let x = 18.0;
                let wmax = (put.right - x).max(40.0);
                c.text_fit(if self.rolling_back() { t.busy_rem } else { t.busy_inst }, Rect::new(x, r.y + 17.5, wmax, 16.0), Font::new(13.0, 500), T1, Align::Start);
                c.text_fit(if self.cancelling { t.stopping } else { t.busy_sub }, Rect::new(x, r.y + 35.5, wmax, 15.0), Font::new(12.0, 400), T3, Align::Start);
            }
            Phase::Done => {
                put.add(c, f, ID_SIGN_OUT, t.sign_out, 600, 18.0, 0.0, T1, look::INSTALL_FG, Rgba::pure_white(0.1), true);
                put.add(c, f, ID_BACK, t.back, 500, 16.0, 0.0, ghost, T1, ghost_lift, true);
                let x = 18.0;
                c.text_fit(t.later, Rect::new(x, r.y, (put.right - x).max(40.0), FOOT_H), Font::new(12.0, 400), T3, Align::Start);
            }
        }
    }
}

/// The footer's buttons, laid out from the right edge inwards, 10 apart.
struct Puts {
    right: f32,
    y: f32,
}

impl Puts {
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, label: &str, weight: u16, pad: f32, min: f32, bg: Rgba, fg: Rgba, lift: Rgba, enabled: bool) {
        let bw = (c.measure(label, Font::new(13.0, weight)).ceil() + 2.0 * pad).max(min);
        let b = Rect::new(self.right - bw, self.y, bw, 38.0);
        self.right -= bw + 10.0;
        button(c, f, id, b, label, weight, bg, fg, lift, enabled);
    }
}
