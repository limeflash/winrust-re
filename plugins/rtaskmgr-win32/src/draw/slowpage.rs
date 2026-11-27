//! The "What's slow" page, as TaskManager.dc.html draws it: a banner with the verdict, then three cards (Windows startup, Apps right now,
//! Drivers) that fold; the ones that are wrong are open and come first. What is in them is `slow.rs`; what a click does is `app.rs`.
use super::*;
use crate::slow::{AppRow, Boot, CHART_MS, Chip, Delay, Drivers, Slow, Tone};

pub const ID_SLOW: Id = 600; // + the card (0 startup, 1 apps, 2 drivers)
pub const ID_SLOW_DEL: Id = 610; // + the row of "What delays it"
pub const ID_SLOW_ALL: Id = 620;
pub const ID_SLOW_END: Id = 630; // + the row of "Apps right now"
pub const ID_SLOW_ADMIN: Id = 640;
/// How long "Check again" looks (the design's 1.4 s).
pub const CHECK_MS: u64 = 1400;

const SPIN: &str = "M12 3a9 9 0 1 0 9 9";
const WARN: &str = "M12 8v5M12 16.5h.01M10.3 3.9L2.4 18a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z";
const APPS: &str = "M4 5h16v11H4zM8 20h8M12 16v4";

fn tone_colors(t: Tone) -> (Rgba, Rgba) {
    match t {
        Tone::Fine => (GREEN, GREEN.alpha(0.14)),
        Tone::Warn => (AMBER, AMBER.alpha(0.16)),
        Tone::Quiet => (T2, ink(0.08)),
    }
}

/// A pill of the cards' headers: a dot and a word (`height:22px; border-radius:11px; 12px/600`), right edge at `right`.
fn chip(c: &mut Canvas<'_>, right: f32, cy: f32, ch: &Chip) {
    let (fg, bg) = tone_colors(ch.tone);
    let tw = c.measure(&ch.text, Font::new(12.0, 600)).ceil();
    let w = 18.0 + 6.0 + 6.0 + tw;
    let r = Rect::new(right - w, cy - 11.0, w, 22.0);
    c.fill(r, 11.0, bg);
    c.circle(r.x + 9.0 + 3.0, cy, 3.0, fg);
    c.text(&ch.text, Rect::new(r.x + 21.0, r.y, tw + 2.0, r.h), Font::new(12.0, 600), fg, Align::Start);
}

/// A tile with the first letter, for what has no icon to show (the five gradients of the design).
fn letter_tile(c: &mut Canvas<'_>, r: Rect, name: &str) {
    const P: [(u32, u32); 5] = [(0x3cc4f2, 0x1c74c4), (0xef6b6b, 0xa32d3a), (0x8b7cf6, 0x4c3fb0), (0xf0b43c, 0xa8701c), (0xe08a5a, 0xa4522c)];
    let ch = name.chars().next().map_or('?', |c| c.to_ascii_uppercase());
    let i = match ch {
        'D' => 0,
        'C' => 1,
        'B' => 2,
        'P' | 'T' => 3,
        'R' => 4,
        o => o as usize % 5,
    };
    c.gradient_angle(r, 6.0, 160.0, &[(0.0, rgb(P[i].0)), (1.0, rgb(P[i].1))]);
    c.text(&ch.to_string(), r, Font::new(11.0, 700), Rgba::pure_white(1.0), Align::Center);
}

/// A button of 13 px/500 text (`.btn`, height 30, padding 12).
#[allow(clippy::too_many_arguments)] // where, what, the colours at rest and under the pointer
fn btn(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, label: &str, bg: Rgba, fg: Rgba, hover: Rgba, font: Font) {
    let hot = f.focusable(id, r);
    f.hand();
    c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
        c.fill(r, 9.0, bg.mix(hover, hot.hover));
        c.text(label, r, font, fg, Align::Center);
    });
    if hot.focus {
        c.ring(r.inset(-3.0), 12.0, 2.0, focus_ring().fade(0.9));
    }
}

fn dashed(c: &mut Canvas<'_>, x: f32, y: f32, w: f32, col: Rgba) {
    let mut p = 0.0;
    while p < w {
        c.fill(Rect::new(x + p, y, 3.0f32.min(w - p), 1.0), 0.0, col);
        p += 6.0;
    }
}

impl Tm {
    /// The height of what is under a card's header when it is open.
    fn slow_body_h(&self, c: &mut Canvas<'_>, k: usize, bw: f32) -> f32 {
        let s = &self.slow;
        match k {
            0 if !s.boot.have => 46.0,
            0 => {
                let n = s.boot.delays.len();
                let rows = if n == 0 { 28.0 } else { n as f32 * 52.0 + (n - 1) as f32 * 2.0 };
                150.0 + 14.0 + 14.0 + 22.0 + 2.0 + rows + 2.0 + 8.0 + 30.0
            }
            1 if s.apps.rows.is_empty() => 28.0,
            1 => 19.0 + s.apps.rows.iter().map(|r| 2.0 + 48.0 + if r.bad { 2.0 + 40.0 } else { 0.0 }).sum::<f32>(),
            _ => 105.0 + if s.drivers.bad { 14.0 + self.slow_note_h(c, bw) } else { 0.0 },
        }
    }

    fn slow_note_h(&self, c: &mut Canvas<'_>, bw: f32) -> f32 {
        (c.lines(self.slow.drivers.note, Font::new(12.5, 400), bw - 24.0) as f32 * 19.0 + 22.0).max(50.0)
    }

    pub(super) fn page_insights(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        let s: Slow = self.slow.clone();
        // "Check again" runs for 1.4 s: the page is dimmed and the banner spins
        let mut checking = false;
        if let Some(ch) = self.slow_chk.as_mut() {
            let born = *ch.born.get_or_insert(f.now);
            if f.now.saturating_sub(born) >= CHECK_MS {
                self.slow_chk = None;
                self.slow_just = true;
            } else {
                f.tween(-5001, 9, ch.serial as f32, CHECK_MS as u32, LINEAR);
                f.due(f.now + 16);
                checking = true;
            }
        }
        let (x, w) = (body.x + 24.0, body.w - 48.0);
        let bw = w - 36.0;
        // the order: what is wrong first, each group in the design's order
        let mut order = [0usize, 1, 2];
        let bad = [s.boot.bad, s.apps.bad, s.drivers.bad];
        order.sort_by_key(|&k| (!bad[k], k));
        let open = |me: &Self, k: usize| me.slow_open.get(&(k as u8)).copied().unwrap_or(bad[k]);
        let heights: [f32; 3] = std::array::from_fn(|k| 65.0 + if open(self, k) { 16.0 + self.slow_body_h(c, k, bw) } else { 0.0 });
        let content = 72.0 + order.iter().map(|&k| 12.0 + heights[k]).sum::<f32>() + 24.0;
        let dim = if checking { 0.45 } else { 1.0 };
        let mut sc = std::mem::take(&mut self.ins_sc);
        scroll(c, f, ID_SCROLL_INS, body, &mut sc, content, |c, f, v| {
            self.slow_banner(c, &s, Rect::new(x, v.at(0.0), w, 72.0), checking, f.now);
            let mut y = 72.0 + 12.0;
            for &k in &order {
                let card = Rect::new(x, v.at(y), w, heights[k]);
                let is_open = open(self, k);
                c.faded(dim, |c| {
                    c.fill(card, 16.0, CARD);
                    c.ring(card, 16.0, 1.0, ink(0.05));
                });
                self.slow_head(c, f, &s, k, card, is_open, dim);
                if is_open {
                    let b = Rect::new(card.x + 18.0, card.y + 16.0 + 33.0 + 16.0, bw, heights[k] - 65.0 - 16.0);
                    c.faded(dim, |c| match k {
                        0 => self.slow_boot(c, f, &s.boot, b),
                        1 => self.slow_apps(c, f, &s, b),
                        _ => self.slow_drivers(c, &s.drivers, b),
                    });
                }
                y += heights[k] + 12.0;
            }
        });
        self.ins_sc = sc;
    }

    fn slow_banner(&self, c: &mut Canvas<'_>, s: &Slow, r: Rect, checking: bool, now: u64) {
        let warn = s.n > 0 && !checking;
        c.fill(r, 16.0, if warn { CARD.mix(AMBER, 0.06) } else { CARD });
        c.ring(r, 16.0, 1.0, if warn { AMBER.alpha(0.22) } else { ink(0.05) });
        let (fg, bg) = if checking { (accent(), accent().alpha(0.16)) } else if s.n > 0 { (AMBER, AMBER.alpha(0.16)) } else { (GREEN, GREEN.alpha(0.14)) };
        let t = Rect::new(r.x + 18.0, r.y + 16.0, 40.0, 40.0);
        c.fill(t, 12.0, bg);
        let (d, turn) = if checking { (SPIN, (now % 900) as f32 / 900.0 * 360.0) } else if s.n > 0 { (WARN, 0.0) } else { (I_CHECK, 0.0) };
        c.icon(d, (t.cx(), t.cy()), 20.0, 2.0, fg, turn);
        let when = if checking {
            String::new()
        } else if self.slow_just {
            t!("Проверено только что", "Checked just now", ja: "たった今確認しました", pt: "Verificado agora há pouco", zh: "刚刚检查").to_string()
        } else {
            tf!("Проверено {}", "Checked {}", ja: "{} に確認", pt: "Verificado às {}", zh: "{} 检查", self.slow_at)
        };
        let ww = c.measure(&when, F12).ceil();
        c.text(&when, Rect::new(r.x + r.w - 18.0 - ww, r.y, ww + 2.0, r.h), F12, T3, Align::Start);
        let tx = t.x + 40.0 + 14.0;
        let tw = (r.x + r.w - 18.0 - ww - 14.0 - tx).max(60.0);
        let (title, sub) = if checking {
            (t!("Проверяем приложения, драйверы и запуск…", "Checking apps, drivers and startup…", ja: "アプリ、ドライバー、起動を確認しています…", pt: "Verificando apps, drivers e inicialização…", zh: "正在检查应用、驱动程序和启动…").to_string(), t!("Это займёт пару секунд", "Takes a couple of seconds", ja: "数秒かかります", pt: "Leva alguns segundos", zh: "需要几秒钟").to_string())
        } else {
            (s.title.clone(), s.sub.clone())
        };
        c.text_fit(&title, Rect::new(tx, r.y + 17.5, tw, 19.0), Font::new(15.0, 600), T1, Align::Start);
        c.text_fit(&sub, Rect::new(tx, r.y + 39.5, tw, 15.0), Font::new(12.5, 400), T2, Align::Start);
    }

    #[allow(clippy::too_many_arguments)] // the card, its place, whether it is open and how dim the page is
    fn slow_head(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, s: &Slow, k: usize, card: Rect, is_open: bool, dim: f32) {
        let (title, verdict, chip_v, icon) = match k {
            0 => (t!("Запуск Windows", "Windows startup", ja: "Windows の起動", pt: "Inicialização do Windows", zh: "Windows 启动"), &s.boot.verdict, &s.boot.chip, I_POWER),
            1 => (t!("Приложения сейчас", "Apps right now", ja: "いまのアプリ", pt: "Apps agora", zh: "应用现状"), &s.apps.verdict, &s.apps.chip, APPS),
            _ => (t!("Драйверы", "Drivers", ja: "ドライバー", pt: "Drivers", zh: "驱动程序"), &s.drivers.verdict, &s.drivers.chip, I_CORE),
        };
        let hr = Rect::new(card.x + 10.0, card.y + 10.0, card.w - 20.0, 45.0);
        let hot = f.focusable(ID_SLOW + k as Id, hr);
        f.hand();
        let turn = f.tween(ID_SLOW + k as Id, 2, f32::from(is_open), 250, SPRING);
        c.faded(dim, |c| {
            c.fill(hr, 12.0, ink(0.035 * hot.hover));
            let (fg, bg) = tone_colors(chip_v.tone);
            let t = Rect::new(card.x + 18.0, card.y + 16.5, 32.0, 32.0);
            c.fill(t, 9.0, bg);
            c.icon(icon, (t.cx(), t.cy()), 16.0, 2.0, fg, 0.0);
            let tx = card.x + 62.0;
            let right = card.x + card.w - 44.0;
            let cw = 18.0 + 12.0 + c.measure(&chip_v.text, Font::new(12.0, 600)).ceil();
            let tw = (right - cw - 12.0 - tx).max(60.0);
            c.text_fit(title, Rect::new(tx, card.y + 16.0, tw, 16.0), Font::new(13.0, 600), T1, Align::Start);
            c.text_fit(verdict, Rect::new(tx, card.y + 34.0, tw, 15.0), Font::new(12.5, 400), T2, Align::Start);
            chip(c, right, card.y + 32.5, chip_v);
            c.icon(I_CHEV_DOWN, (card.x + card.w - 25.0, card.y + 32.5), 14.0, 2.2, T3, 180.0 * turn);
        });
        if hot.focus {
            c.ring(hr.inset(-2.0), 14.0, 2.0, focus_ring().fade(0.9));
        }
    }

    // ---- Windows startup

    fn slow_boot(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, b: &Boot, r: Rect) {
        if !b.have {
            c.text_fit(&b.verdict, Rect::new(r.x, r.y, r.w, 18.0), F13, T2, Align::Start);
            if !self.d.is_admin {
                let l = t!("Перезапустить от имени администратора", "Restart as administrator", ja: "管理者として再起動", pt: "Reiniciar como administrador", zh: "以管理员身份重启");
                let bwid = c.measure(l, F13M).ceil() + 24.0;
                btn(c, f, ID_SLOW_ADMIN, Rect::new(r.x, r.y + 16.0, bwid, 30.0), l, ink(0.08), T1, ink(0.12), F13M);
            }
            return;
        }
        // the last startup: the time, the number, how it splits
        c.text(&tf!("Последний запуск · {}", "Last startup · {}", ja: "前回の起動 · {}", pt: "Última inicialização · {}", zh: "最近一次启动 · {}", b.when), Rect::new(r.x, r.y, 200.0, 15.0), F12, T3, Align::Start);
        let big = Font::new(34.0, 600).tracking(-0.02).tabular();
        let tw = c.measure(&b.total, big).ceil();
        c.text(&b.total, Rect::new(r.x, r.y + 17.0, tw + 2.0, 41.0), big, T1, Align::Start);
        if let Some(dl) = &b.delta {
            let pw = c.measure(dl, Font::new(12.0, 600).tabular()).ceil() + 16.0;
            let pr = Rect::new(r.x + tw + 10.0, r.y + 17.0 + 20.5 - 11.0, pw, 22.0);
            c.fill(pr, 11.0, AMBER.alpha(0.16));
            c.text(dl, pr, Font::new(12.0, 600).tabular(), AMBER, Align::Center);
        }
        let leg = [(accent(), t!("До рабочего стола", "Until the desktop", ja: "デスクトップまで", pt: "Até a área de trabalho", zh: "到桌面为止"), &b.desk), (accent().alpha(0.4), t!("Приложения ещё грузятся", "Apps still loading", ja: "アプリを読み込み中", pt: "Apps ainda carregando", zh: "应用仍在加载"), &b.after)];
        for (i, (col, label, val)) in leg.into_iter().enumerate() {
            let y = r.y + 70.0 + i as f32 * 21.0;
            c.fill(Rect::new(r.x, y + 2.5, 10.0, 10.0), 3.0, col);
            c.text(label, Rect::new(r.x + 18.0, y, 150.0, 15.0), Font::new(12.5, 400), T2, Align::Start);
            c.text(val, Rect::new(r.x, y, 200.0, 15.0), Font::new(12.5, 600).tabular(), T1, Align::End);
        }
        // the chart of the last startups
        let (cx0, ch) = (r.x + 228.0, 150.0);
        let plot = Rect::new(cx0 + 76.0, r.y, r.w - 228.0 - 76.0, ch);
        let base = r.y + 128.0;
        c.fill(Rect::new(plot.x, base - 1.0, plot.w, 1.0), 0.0, ink(0.1));
        let fast_y = r.y + ch - (22.0 + b.fast_ms as f32 / CHART_MS as f32 * 100.0) - 1.0;
        dashed(c, plot.x, fast_y, plot.w, GREEN.alpha(0.55));
        let lab = t!("лучший", "fastest", ja: "最速", pt: "mais rápido", zh: "最快");
        c.text(lab, Rect::new(cx0, fast_y - 13.75, 68.0, 13.75), Font::new(11.0, 400), GREEN, Align::End);
        c.text(&b.fast, Rect::new(cx0, fast_y, 68.0, 13.75), Font::new(11.0, 600), GREEN, Align::End);
        let n = b.bars.len().max(1) as f32;
        let space = (plot.w - n * 56.0) / n;
        for (i, bar) in b.bars.iter().enumerate() {
            let last = i + 1 == b.bars.len();
            let sx = plot.x + space / 2.0 + i as f32 * (56.0 + space);
            let h = (bar.ms as f32 / CHART_MS as f32 * 100.0).min(125.0);
            let col = Rect::new(sx + 14.0, base - h, 28.0, h);
            if last && bar.desk > 0 && bar.desk < bar.ms {
                let inner = (h - 2.0).max(2.0);
                let after = inner * (bar.ms - bar.desk) as f32 / bar.ms as f32;
                c.fill(Rect::new(col.x, col.y, 28.0, after), 4.0, accent().alpha(0.38));
                c.fill(Rect::new(col.x, col.y + after + 2.0, 28.0, inner - after), 4.0, accent());
            } else {
                c.fill(Rect::new(col.x, col.y + 2.0, 28.0, (h - 2.0).max(1.0)), 6.0, if last { accent() } else { ink(0.13) });
            }
            let vt = secs_text(bar.ms);
            let (vf, vc) = if last { (Font::new(12.0, 600).tabular(), T1) } else { (Font::new(12.0, 500).tabular(), T3) };
            c.text(&vt, Rect::new(sx, col.y - 6.0 - 15.0, 56.0, 15.0), vf, vc, Align::Center);
            let (lf, lc) = if last { (Font::new(11.5, 600), T1) } else { (Font::new(11.5, 500), T3) };
            c.text(&bar.label, Rect::new(sx - 12.0, base + 8.0, 80.0, 14.0), lf, lc, Align::Center);
        }
        // what delays it
        let top = r.y + ch + 14.0;
        c.fill(Rect::new(r.x, top, r.w, 1.0), 0.0, ink(0.06));
        let hy = top + 15.0;
        c.text(t!("Что замедляет запуск", "What delays it", ja: "起動を遅くしているもの", pt: "O que atrasa", zh: "是什么拖慢了启动"), Rect::new(r.x + 8.0, hy, 300.0, 16.0), Font::new(13.0, 600), T1, Align::Start);
        let note = t!("Задержки, записанные Windows за последние запуски, поэтому в сумме больше одного запуска", "Delays Windows logged over recent startups, so they add up to more than one boot", ja: "最近の起動で Windows が記録した遅延です。合計は 1 回の起動より大きくなります", pt: "Atrasos que o Windows registrou nas últimas inicializações, por isso somam mais que uma inicialização", zh: "Windows 在最近几次启动中记录的延迟，因此总和会超过一次启动");
        let nw = c.measure(note, F12).ceil().min(r.w - 8.0 - 8.0 - 200.0);
        c.text_fit(note, Rect::new(r.x + r.w - 8.0 - nw, hy + 1.0, nw, 15.0), F12, T3, Align::End);
        let mut y = hy + 22.0 + 2.0;
        let maxms = b.delays.iter().map(|d| d.ms).max().unwrap_or(1).max(1) as f32;
        if b.delays.is_empty() {
            c.text(t!("Windows не записала задержек.", "Windows logged no delays.", ja: "Windows は遅延を記録していません。", pt: "O Windows não registrou atrasos.", zh: "Windows 没有记录到延迟。"), Rect::new(r.x + 8.0, y + 4.0, r.w - 16.0, 20.0), F13, T3, Align::Start);
            y += 28.0;
        }
        let delays = b.delays.clone();
        for (i, dl) in delays.iter().enumerate() {
            self.slow_delay(c, f, i, dl, Rect::new(r.x, y, r.w, 52.0), maxms);
            y += 54.0;
        }
        let l = t!("Все приложения автозагрузки", "All startup apps", ja: "すべてのスタートアップ アプリ", pt: "Todos os apps de inicialização", zh: "所有启动应用");
        let bwid = c.measure(l, F13M).ceil() + 24.0 + 6.0 + 13.0;
        let br = Rect::new(r.x + 8.0, y + 8.0, bwid, 30.0);
        let hot = f.focusable(ID_SLOW_ALL, br);
        f.hand();
        c.fill(br, 9.0, ink(0.06 + 0.04 * hot.hover));
        c.text(l, Rect::new(br.x + 12.0, br.y, bwid, 30.0), F13M, T1, Align::Start);
        c.icon(I_CHEV, (br.x + bwid - 12.0 - 6.5, br.cy()), 13.0, 2.2, T1, 0.0);
        if hot.focus {
            c.ring(br.inset(-3.0), 12.0, 2.0, focus_ring().fade(0.9));
        }
    }

    fn slow_delay(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, dl: &Delay, r: Rect, maxms: f32) {
        let off = dl.can && !dl.on;
        c.faded(if off { 0.6 } else { 1.0 }, |c| {
            let ir = Rect::new(r.x + 8.0, r.y + 15.0, 22.0, 22.0);
            match &dl.icon {
                Some(p) if self.has_icon(p) => self.app_icon(c, Some(p), ir),
                _ => letter_tile(c, ir, &dl.name),
            }
            c.text_fit(&dl.name, Rect::new(r.x + 40.0, r.y + 11.0, 228.0, 16.0), F13M, T1, Align::Start);
            c.text_fit(&dl.sub, Rect::new(r.x + 40.0, r.y + 27.0, 228.0, 14.0), Font::new(11.5, 400), T3, Align::Start);
            let bar_x = r.x + 8.0 + 260.0 + 16.0;
            let bar_w = (r.w - 16.0 - 260.0 - 64.0 - 168.0 - 48.0).max(40.0);
            let bar = Rect::new(bar_x, r.cy() - 3.0, bar_w, 6.0);
            c.fill(bar, 3.0, ink(0.06));
            let col = if off { ink(0.14) } else if dl.can { AMBER } else { ink(0.28) };
            c.fill(Rect::new(bar.x, bar.y, (bar_w * dl.ms as f32 / maxms).max(3.0), 6.0), 3.0, col);
            let (vt, vc) = if off { ("—".to_string(), T3) } else { (dl.text.clone(), if dl.can { T1 } else { T2 }) };
            c.text(&vt, Rect::new(bar_x + bar_w + 16.0, r.y, 64.0, r.h), Font::new(13.0, 600).tabular(), vc, Align::End);
            let right = r.x + r.w - 8.0;
            if dl.can {
                let l = if off { t!("Вернуть", "Undo", ja: "元に戻す", pt: "Desfazer", zh: "撤销") } else { t!("Отключить в автозагрузке", "Turn off at startup", ja: "スタートアップで無効にする", pt: "Desativar na inicialização", zh: "禁止随启动运行") };
                let bw = c.measure(l, F13M).ceil() + 24.0;
                let b = Rect::new(right - bw, r.cy() - 15.0, bw, 30.0);
                btn(c, f, ID_SLOW_DEL + i as Id, b, l, if off { Rgba::white(0.0) } else { ink(0.08) }, if off { acc_text() } else { T1 }, ink(0.12), F13M);
            } else {
                let kw = c.measure(dl.keep, F12).ceil();
                let kr = Rect::new(right - kw, r.cy() - 8.0, kw, 16.0);
                f.item(ID_SLOW_DEL + 20 + i as Id, kr);
                f.tip(dl.keep_tip);
                c.text(dl.keep, kr, F12, T3, Align::End);
            }
        });
    }

    // ---- Apps right now

    fn slow_apps(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, s: &Slow, r: Rect) {
        if s.apps.rows.is_empty() {
            c.text(t!("Сейчас нечего показать.", "Nothing to show right now.", ja: "いま表示するものはありません。", pt: "Nada a mostrar agora.", zh: "目前没有可显示的内容。"), Rect::new(r.x + 8.0, r.y + 4.0, r.w - 16.0, 20.0), F13, T3, Align::Start);
            return;
        }
        let (bar_x, vw) = (r.x + 8.0 + 190.0 + 16.0, 270.0);
        let bar_w = (r.w - 16.0 - 190.0 - vw - 32.0).max(40.0);
        c.text(t!("Приложение", "App", ja: "アプリ", pt: "App", zh: "应用"), Rect::new(r.x + 8.0, r.y, 190.0, 15.0), F12, T3, Align::Start);
        let (h2, h3) = if s.apps.compare {
            (t!("Сейчас и за обычную неделю", "Now, compared with its usual week", ja: "いまと通常の 1 週間の比較", pt: "Agora, comparado à semana habitual", zh: "现在与平时一周的对比"), t!("Сейчас · обычно", "Now · usually", ja: "いま · 通常", pt: "Agora · normalmente", zh: "现在 · 通常"))
        } else {
            (t!("Сейчас", "Right now", ja: "いま", pt: "Agora", zh: "现在"), "")
        };
        c.text(h2, Rect::new(bar_x, r.y, bar_w, 15.0), F12, T3, Align::Start);
        c.text(h3, Rect::new(r.x + r.w - 8.0 - vw, r.y, vw, 15.0), F12, T3, Align::End);
        let mut y = r.y + 19.0 + 2.0;
        for (i, row) in s.apps.rows.iter().enumerate() {
            self.slow_app_row(c, f, i, row, Rect::new(r.x, y, r.w, 48.0), bar_x, bar_w, vw);
            y += 50.0;
            if row.bad {
                let wr = Rect::new(r.x, y, r.w, 40.0);
                let l = t!("Снять задачу", "End task", ja: "タスクの終了", pt: "Finalizar tarefa", zh: "结束任务");
                let bw = c.measure(l, Font::new(12.5, 500)).ceil() + 24.0;
                let b = Rect::new(r.x + r.w - 8.0 - bw, y + 2.0, bw, 30.0);
                c.text_fit(&row.why, Rect::new(r.x + 38.0, y + 2.0, (b.x - 10.0 - r.x - 38.0).max(40.0), 30.0), Font::new(12.5, 400), T2, Align::Start);
                btn(c, f, ID_SLOW_END + i as Id, b, l, look::red_soft(0.12), RED, look::red_soft(0.2), Font::new(12.5, 500));
                y += wr.h + 2.0;
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // the row, its place and the two columns it shares with the others
    fn slow_app_row(&mut self, c: &mut Canvas<'_>, _f: &mut Frame<'_>, _i: usize, row: &AppRow, r: Rect, bar_x: f32, bar_w: f32, vw: f32) {
        if row.bad {
            c.fill(r, 10.0, AMBER.alpha(0.07));
        }
        let ir = Rect::new(r.x + 8.0, r.cy() - 10.0, 20.0, 20.0);
        match &row.icon {
            Some(p) if self.has_icon(p) => self.app_icon(c, Some(p), ir),
            _ => letter_tile(c, ir, &row.name),
        }
        c.text_fit(&row.name, Rect::new(r.x + 38.0, r.y + 9.0, 152.0, 16.0), F13M, T1, Align::Start);
        c.text_fit(&row.metric, Rect::new(r.x + 38.0, r.y + 25.0, 152.0, 14.0), Font::new(11.5, 400), T3, Align::Start);
        let bar = Rect::new(bar_x, r.cy() - 4.0, bar_w, 8.0);
        c.fill(bar, 4.0, ink(0.07));
        c.fill(Rect::new(bar.x, bar.y, (bar_w * row.now_frac).max(4.0), 8.0), 4.0, if row.bad { AMBER } else { accent().alpha(0.7) });
        if let Some(u) = row.usual_frac {
            c.fill(Rect::new(bar.x + bar_w * u - 1.0, bar.y - 4.0, 2.0, 16.0), 1.0, T1.alpha(0.55));
        }
        let mut right = r.x + r.w - 8.0;
        let num = Font::new(13.0, 400).tabular();
        if let Some(u) = &row.usual {
            let t = format!("· {u}");
            let w = c.measure(&t, num).ceil();
            c.text(&t, Rect::new(right - w, r.y, w + 1.0, r.h), num, T3, Align::Start);
            right -= w + 8.0;
        }
        let nf = Font::new(13.0, 500).tabular();
        let nw = c.measure(&row.now, nf).ceil();
        c.text(&row.now, Rect::new(right - nw, r.y, nw + 1.0, r.h), nf, T1, Align::Start);
        right -= nw + 8.0;
        let _ = vw;
        if row.times > 0 {
            let t = tf!("{}× обычного", "{}× usual", ja: "通常の {} 倍", pt: "{}× o normal", zh: "平时的 {} 倍", row.times);
            let w = c.measure(&t, Font::new(12.0, 600)).ceil() + 14.0;
            let pr = Rect::new(right - w, r.cy() - 10.0, w, 20.0);
            c.fill(pr, 6.0, AMBER.alpha(0.16));
            c.text(&t, pr, Font::new(12.0, 600), AMBER, Align::Center);
        }
    }

    // ---- Drivers

    fn slow_drivers(&self, c: &mut Canvas<'_>, d: &Drivers, r: Rect) {
        let cw = (r.w - 12.0) / 2.0;
        for (i, m) in d.meters.iter().enumerate() {
            let cr = Rect::new(r.x + i as f32 * (cw + 12.0), r.y, cw, 105.0);
            c.fill(cr, 12.0, ink(0.035));
            c.text_fit(m.title, Rect::new(cr.x + 14.0, cr.y + 12.0, cw - 28.0 - 70.0, 16.0), F13M, T1, Align::Start);
            c.text_fit(m.sub, Rect::new(cr.x + 14.0, cr.y + 30.0, cw - 28.0 - 70.0, 15.0), F12, T3, Align::Start);
            c.text(&m.val, Rect::new(cr.x + 14.0, cr.y + 12.0, cw - 28.0, 24.0), Font::new(20.0, 600).tracking(-0.01).tabular(), if m.hot { AMBER } else { T1 }, Align::End);
            let bar = Rect::new(cr.x + 14.0, cr.y + 61.0, cw - 28.0, 8.0);
            c.fill(bar, 4.0, ink(0.07));
            c.fill(Rect::new(bar.x, bar.y, (bar.w * m.frac).max(4.0), 8.0), 4.0, if m.hot { AMBER } else { accent().alpha(0.7) });
            c.fill(Rect::new(bar.x + bar.w * 0.4 - 1.0, bar.y - 4.0, 2.0, 16.0), 1.0, AMBER.alpha(0.7));
            let sm = Font::new(11.5, 400).tabular();
            c.text("0%", Rect::new(bar.x, cr.y + 79.0, 40.0, 14.0), sm, T3, Align::Start);
            c.text("5%", Rect::new(bar.x + bar.w - 40.0, cr.y + 79.0, 40.0, 14.0), sm, T3, Align::End);
            c.text(t!("2% · начало подтормаживаний", "2% · stutter starts", ja: "2% · カクつき始め", pt: "2% · início dos engasgos", zh: "2% · 开始卡顿"), Rect::new(bar.x + bar.w * 0.4 - 100.0, cr.y + 79.0, 200.0, 14.0), sm, T3, Align::Center);
        }
        if d.bad {
            let h = self.slow_note_h(c, r.w);
            let nr = Rect::new(r.x, r.y + 105.0 + 14.0, r.w, h);
            c.fill(nr, 12.0, AMBER.alpha(0.09));
            c.text_wrapped(d.note, Rect::new(nr.x + 12.0, nr.y + 11.0, nr.w - 24.0, h - 14.0), Font::new(12.5, 400), T2, 19.0, 0);
        }
    }
}

fn secs_text(ms: u32) -> String {
    tf!("{} с", "{} s", ja: "{} 秒", pt: "{} s", zh: "{} 秒", (f64::from(ms) / 1000.0).round() as u64)
}
