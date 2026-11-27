//! The Windows Update page of the design (Settings.v4.html): the status card, the list of updates with checkboxes, the optional ones, what
//! waits for a restart, the history. The data is wu.rs (Live) or the demo's; this file decides what the page shows for it and what the
//! ticks, the "Show" links and the chips remember (they are the window's, not the system's).
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use crate::{
    Cmd, GAct, Group, Line, Row, Tile, Tone, look, more,
    os::os,
    wu::{self, Cat, HGroup, Phase, Upd},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WuKind {
    Quiet,
    Primary,
    /// A toggle that is on (the Schedule button while its chips show).
    On,
}

/// What a button of the status card does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WuAct {
    Check,
    Cancel,
    Install,
    /// Opens the "Restart now?" dialog.
    RestartDlg,
    /// Shows or hides the time chips.
    Sched,
    Unsched,
    /// Shows or hides the error's details.
    Details,
}

#[derive(Clone, Debug)]
pub struct WuBtn {
    pub label: String,
    pub kind: WuKind,
    /// The shield: Windows will ask for administrator rights.
    pub shield: bool,
    pub busy: bool,
    pub dis: bool,
    pub act: WuAct,
}

/// The status card: icon, the words, the buttons, and what unfolds under them.
#[derive(Clone, Debug)]
pub struct WuCard {
    pub d: &'static str,
    pub tone: Tone,
    pub spin: bool,
    pub title: String,
    pub sub: String,
    pub sub2: String,
    pub btns: Vec<WuBtn>,
    /// The bar: how far (0..=100), the text at its left and at its right.
    pub prog: Option<(f32, String, String)>,
    /// "Restart" chips (their labels) and the note under them.
    pub chips: Option<(Vec<String>, String)>,
    /// The error box: title, text, code.
    pub err: Option<(String, String, String)>,
}

/// How a row of the list stands.
#[derive(Clone, Debug)]
pub enum UMode {
    /// Waiting for a pick: ticked or not.
    Pick(bool),
    /// A state with an icon: the glyph, its tone, whether it spins, the words at the right and their tone, the bar (how far, green).
    Icon { d: &'static str, tone: Tone, spin: bool, right: String, right_tone: Tone, bar: Option<(f32, bool)> },
}

#[derive(Clone, Debug)]
pub struct UpdRow {
    pub upd: Upd,
    pub mode: UMode,
}

// ---------------------------------------------------------------------------------------------------------------------
// what the window remembers about the page

thread_local! {
    static SEL: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    static OPT_OPEN: Cell<bool> = const { Cell::new(false) };
    static HIST_ALL: Cell<bool> = const { Cell::new(false) };
    static CHIPS: Cell<bool> = const { Cell::new(false) };
    static ERR_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// Ticked: a pick of the user's, else the required ones.
#[must_use]
pub fn selected(u: &Upd) -> bool {
    SEL.with_borrow(|m| m.get(&u.id).copied()).unwrap_or(!u.optional)
}

pub fn toggle(id: &str, on: bool) {
    SEL.with_borrow_mut(|m| {
        m.insert(id.to_string(), on);
    });
}

pub fn set_required(ups: &[Upd], on: bool) {
    for u in ups.iter().filter(|u| !u.optional) {
        toggle(&u.id, on);
    }
}

/// The page starts from its resting state (the window opens, or a state of the design is set).
pub fn reset() {
    SEL.with_borrow_mut(HashMap::clear);
    OPT_OPEN.set(false);
    HIST_ALL.set(false);
    CHIPS.set(false);
    ERR_OPEN.set(false);
}

pub fn flip_optional() {
    OPT_OPEN.set(!OPT_OPEN.get());
}

pub fn flip_history() {
    HIST_ALL.set(!HIST_ALL.get());
}

pub fn flip_chips() {
    CHIPS.set(!CHIPS.get());
}

pub fn flip_details() {
    ERR_OPEN.set(!ERR_OPEN.get());
}

pub fn set_chips(on: bool) {
    CHIPS.set(on);
}

pub fn set_optional(on: bool) {
    OPT_OPEN.set(on);
}

pub fn set_history_all(on: bool) {
    HIST_ALL.set(on);
}

pub fn set_details(on: bool) {
    ERR_OPEN.set(on);
}

// ---------------------------------------------------------------------------------------------------------------------
// words

fn size_text(mb: u32) -> String {
    if mb >= 1000 { format!("{:.1} GB", f64::from(mb) / 1024.0) } else { format!("{mb} MB") }
}

pub fn cat_name(c: Cat) -> &'static str {
    match c {
        Cat::Security => t!("Безопасность", "Security"),
        Cat::Net => ".NET",
        Cat::Definitions => t!("Определения Defender", "Defender definitions"),
        Cat::Driver => t!("Драйвер", "Driver"),
        Cat::Preview => t!("Предварительное", "Preview"),
        Cat::Cumulative => t!("Накопительное", "Cumulative"),
        Cat::Other => t!("Другое", "Other"),
    }
}

/// Where along a run a row stands: waiting, downloading (0..1), downloaded, installing (0..1), done. Downloads take the first 70 % by size,
/// installations the rest in equal shares (the design's reading of one overall percentage).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    Waiting,
    Down(f32),
    Downloaded,
    Install(f32),
    Done,
}

#[must_use]
pub fn step(list: &[Upd], i: usize, p: f32) -> Step {
    let tot = list.iter().map(|u| u.mb).sum::<u32>().max(1) as f32;
    let before: f32 = list[..i].iter().map(|u| u.mb).sum::<u32>() as f32;
    let (a0, a1) = (before / tot * 70.0, (before + list[i].mb as f32) / tot * 70.0);
    if p < a0 {
        return Step::Waiting;
    }
    if p < a1 {
        return Step::Down(((p - a0) / (a1 - a0).max(0.001)).clamp(0.0, 1.0));
    }
    let seg = 30.0 / list.len().max(1) as f32;
    let (b0, b1) = (70.0 + i as f32 * seg, 70.0 + (i + 1) as f32 * seg);
    if p < b0 {
        Step::Downloaded
    } else if p < b1 {
        Step::Install(((p - b0) / seg).clamp(0.0, 1.0))
    } else {
        Step::Done
    }
}

const D_REFRESH: &str = look::REFRESH;
const D_SPIN: &str = "M12 3a9 9 0 1 0 9 9";
const D_DOWN: &str = look::DOWNLOAD;
const D_CROSS: &str = "M7 7l10 10M17 7L7 17";
const D_CLOCK: &str = look::CLOCK;

fn btn(label: impl Into<String>, kind: WuKind, act: WuAct) -> WuBtn {
    WuBtn { label: label.into(), kind, shield: false, busy: false, dis: false, act }
}

/// How wide a text of Inter 13 / 500 is, near enough to wrap it before it is drawn (the row's height comes first).
fn est_width(s: &str) -> f32 {
    s.chars()
        .map(|c| match c {
            ' ' => 3.7,
            'i' | 'l' | '.' | ',' | ':' | ';' | '|' | '\'' | '!' | 'j' => 3.4,
            't' | 'f' | 'r' | 'I' | '(' | ')' | '-' | '\u{2013}' | '/' => 4.6,
            'm' | 'w' | 'M' | 'W' => 10.4,
            c if c.is_uppercase() => 8.2,
            c if c.is_ascii_digit() => 7.3,
            _ => 6.9,
        })
        .sum::<f32>()
        * 1.065
}

/// How many lines (1 or 2) a title takes in `avail` DIPs.
#[must_use]
pub fn title_lines(title: &str, avail: f32) -> usize {
    let (mut x, mut lines) = (0.0, 1);
    for word in title.split_whitespace() {
        let w = est_width(word);
        let gap = if x == 0.0 { 0.0 } else { est_width(" ") };
        if x + gap + w > avail && x > 0.0 {
            lines += 1;
            x = w;
        } else {
            x += gap + w;
        }
    }
    lines.min(2)
}

/// The width the title has beside a checkbox (or a state icon) and the right column, in the 640-wide column.
#[must_use]
pub fn title_width(pick: bool) -> f32 {
    640.0 - 16.0 - if pick { 34.0 } else { 42.0 } - 14.0 - 96.0 - 16.0
}

fn row(u: &Upd, mode: UMode) -> Line {
    let (pick, bar) = (matches!(mode, UMode::Pick(_)), matches!(mode, UMode::Icon { bar: Some(_), .. }));
    let mut l = Line::new(Row::Upd(Box::new(UpdRow { upd: u.clone(), mode }))).key(format!("upd:{}", u.id));
    // a long title takes two lines; a bar under the line of words adds its own
    l.h = Some(if title_lines(&u.title, title_width(pick)) > 1 { 90.0 } else { 70.0 } + if bar { 12.0 } else { 0.0 });
    l
}

fn hist_row(h: &wu::Hist) -> Line {
    let sub = format!("{}{}{}", if h.ok { String::new() } else { format!("{} \u{b7} {} \u{b7} ", t!("Не удалось", "Failed"), h.code) }, if h.kb.is_empty() { String::new() } else { format!("{} \u{b7} ", h.kb) }, h.when);
    let tile = if h.ok { Tile::Soft(look::CHECK, Tone::Good) } else { Tile::Soft(D_CROSS, Tone::Bad) };
    Line::new(Row::Info(h.title.clone(), String::new())).key(format!("h:{}", h.title)).subtitle(sub).tone(if h.ok { Tone::Dim } else { Tone::Bad }).tile(tile).weight(500)
}

fn hist_title(g: HGroup) -> &'static str {
    match g {
        HGroup::Quality => t!("Исправления качества", "Quality updates"),
        HGroup::Driver => t!("Драйверы", "Driver updates"),
        HGroup::Definition => t!("Определения", "Definition updates"),
        HGroup::Other => t!("Другие обновления", "Other updates"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the page

pub fn wu_rows() -> Vec<Group> {
    let o = os();
    let (st, ex) = (o.wu_state(), o.wu_extra());
    let admin = o.wu_admin();
    let found = st.found.clone().unwrap_or_default();
    let (req, opt): (Vec<Upd>, Vec<Upd>) = found.iter().cloned().partition(|u| !u.optional);
    let chosen: Vec<&Upd> = found.iter().filter(|u| selected(u)).collect();
    let total: u32 = chosen.iter().map(|u| u.mb).sum();
    let need_rs = chosen.iter().filter(|u| u.restart).count();
    let last_check = ex.last_check.map(more::when_sentence);
    let last_install = ex.last_install.map(more::when_text);
    let installed_line = last_install.map(|w| tf!("Последняя установка: {}", "Last installed: {}", w)).unwrap_or_default();
    let run: Vec<Upd> = if st.run.is_empty() { chosen.iter().map(|u| (*u).clone()).collect() } else { st.run.clone() };
    let waiting = run.iter().filter(|u| u.restart).count();
    let p = st.pct;

    enum Mode {
        Stale,
        Checking,
        Ok,
        Found,
        Run,
        Restart,
        Sched,
        Failed,
    }
    let mode = match st.phase {
        Phase::Checking => Mode::Checking,
        Phase::Downloading | Phase::Installing => Mode::Run,
        Phase::Idle if st.failed.is_some() => Mode::Failed,
        Phase::Idle if ex.restart => if st.sched.is_some() { Mode::Sched } else { Mode::Restart },
        Phase::Idle => match &st.found {
            Some(f) if !f.is_empty() => Mode::Found,
            Some(_) => Mode::Ok,
            None => Mode::Stale,
        },
    };
    let mut card = WuCard { d: D_REFRESH, tone: Tone::Dim, spin: false, title: String::new(), sub: String::new(), sub2: String::new(), btns: Vec::new(), prog: None, chips: None, err: None };
    match mode {
        Mode::Stale => {
            card.title = if last_check.is_some() { t!("Обновлений не нашлось в прошлый раз", "No updates found last time").into() } else { t!("Ещё не проверялось", "Not checked yet").into() };
            card.sub = last_check.map_or_else(|| st.note.clone(), |w| tf!("Windows проверяла: {}", "Windows last checked {}", w));
            card.sub2 = installed_line.clone();
            card.btns = vec![btn(t!("Проверить обновления", "Check for updates"), WuKind::Primary, WuAct::Check)];
        }
        Mode::Checking => {
            (card.d, card.tone, card.spin) = (D_SPIN, Tone::Accent, true);
            card.title = t!("Поиск обновлений…", "Checking for updates\u{2026}").into();
            card.sub = t!("Это может занять минуту · можно работать", "This can take a minute \u{b7} you can keep working").into();
            card.btns = vec![btn(t!("Отмена", "Cancel"), WuKind::Quiet, WuAct::Cancel)];
        }
        Mode::Ok => {
            (card.d, card.tone) = (look::CHECK, Tone::Good);
            card.title = t!("Всё установлено", "You\u{2019}re up to date").into();
            card.sub = last_check.map_or_else(String::new, |w| tf!("Последняя проверка: {}", "Last checked {}", w));
            card.sub2 = installed_line.clone();
            card.btns = vec![btn(t!("Проверить снова", "Check again"), WuKind::Quiet, WuAct::Check)];
        }
        Mode::Found => {
            (card.d, card.tone) = (D_DOWN, Tone::Accent);
            card.title = if req.is_empty() { t!("Есть необязательные обновления", "Only optional updates are available").into() } else { tf!("Доступно обновлений: {}", "{} updates are available", req.len()) };
            card.sub = if chosen.is_empty() {
                t!("Ничего не выбрано", "Nothing selected").into()
            } else {
                tf!("{} к загрузке · {}", "{} to download \u{b7} {}", size_text(total), if need_rs > 0 { tf!("нужна перезагрузка: {}", "{} need a restart", need_rs) } else { t!("перезагрузка не нужна", "no restart needed").into() })
            };
            if !admin {
                card.sub2 = t!("Windows запросит права администратора", "Windows will ask for administrator rights").into();
            }
            let n = if chosen.len() == req.len() && opt.iter().all(|u| !selected(u)) { String::new() } else { format!(" ({})", chosen.len()) };
            let mut b = btn(format!("{}{n}", t!("Скачать и установить", "Download and install")), WuKind::Primary, WuAct::Install);
            (b.shield, b.dis) = (!admin, chosen.is_empty());
            card.btns = vec![b];
        }
        Mode::Run => {
            (card.d, card.tone, card.spin) = (D_SPIN, Tone::Accent, true);
            let down = st.phase == Phase::Downloading;
            card.title = if down { t!("Загрузка обновлений…", "Downloading updates\u{2026}").into() } else { t!("Установка обновлений…", "Installing updates\u{2026}").into() };
            card.sub = t!("Можно работать", "You can keep working").into();
            card.btns = if down { vec![btn(t!("Отмена", "Cancel"), WuKind::Quiet, WuAct::Cancel)] } else { vec![WuBtn { busy: true, dis: true, ..btn(t!("Установка", "Installing"), WuKind::Quiet, WuAct::Check) }] };
            let all: u32 = run.iter().map(|u| u.mb).sum();
            let left = if down { tf!("Загрузка · {} из {}", "Downloading \u{b7} {} of {}", size_text((all as f32 * p / 70.0) as u32), size_text(all)) } else { tf!("Установка {} из {}", "Installing {} of {}", (((p - 70.0) / (30.0 / run.len().max(1) as f32)).floor() as usize + 1).min(run.len()), run.len()) };
            card.prog = Some((p, left, format!("{}%", p.round() as u32)));
        }
        Mode::Restart => {
            (card.d, card.tone) = (D_REFRESH, Tone::Warn);
            card.title = t!("Перезагрузите, чтобы завершить установку", "Restart to finish installing").into();
            card.sub = if waiting > 0 { tf!("Ждут перезагрузки: {} · остальное уже установлено", "{} updates are waiting \u{b7} the rest are already installed", waiting) } else { t!("Windows ждёт перезагрузки, чтобы завершить обновления", "Windows is waiting for a restart to finish updates").into() };
            let on = CHIPS.get();
            card.btns = vec![btn(t!("Запланировать…", "Schedule\u{2026}"), if on { WuKind::On } else { WuKind::Quiet }, WuAct::Sched), btn(t!("Перезагрузить сейчас", "Restart now"), WuKind::Primary, WuAct::RestartDlg)];
            if on {
                let note = ex.active.map_or_else(String::new, |(a, b)| tf!("Windows не перезагрузится сама в активные часы, {:02}:00\u{2013}{:02}:00", "Windows won\u{2019}t restart on its own during active hours, {:02}:00\u{2013}{:02}:00", a, b));
                card.chips = Some((vec![t!("Сегодня в 03:00", "Tonight at 03:00").into(), t!("Через час", "In 1 hour").into(), t!("Выбрать время…", "Pick a time\u{2026}").into()], note));
            }
        }
        Mode::Sched => {
            (card.d, card.tone) = (D_CLOCK, Tone::Accent);
            let at = st.sched.clone().unwrap_or_default();
            card.title = tf!("Перезагрузка запланирована на {}", "Restart scheduled for {}", at);
            card.sub = t!("Windows перезагрузится, пока вас нет. Сохраните работу перед уходом.", "Windows restarts while you\u{2019}re away. Save your work before you leave.").into();
            card.btns = vec![btn(t!("Отменить перезагрузку", "Cancel restart"), WuKind::Quiet, WuAct::Unsched), btn(t!("Изменить", "Change"), WuKind::Quiet, WuAct::Sched)];
        }
        Mode::Failed => {
            let f = st.failed.clone().unwrap_or_else(|| wu::Failure { title: String::new(), kb: String::new(), code: String::new(), text: String::new() });
            (card.d, card.tone) = (look::WARN, Tone::Bad);
            card.title = t!("Одно обновление не установилось", "1 update couldn\u{2019}t install").into();
            card.sub = if run.len() > 1 { tf!("Остальные ({}) установились · завершите перезагрузкой позже", "The other {} installed \u{b7} restart later to finish them", run.len() - 1) } else { f.text.clone() };
            let mut again = btn(t!("Повторить", "Try again"), WuKind::Primary, WuAct::Install);
            again.shield = !admin;
            card.btns = vec![btn(if ERR_OPEN.get() { t!("Скрыть подробности", "Hide details") } else { t!("Подробности", "Details") }, WuKind::Quiet, WuAct::Details), again];
            if ERR_OPEN.get() {
                let what = if f.kb.is_empty() { f.title.clone() } else { format!("{} ({})", f.title, f.kb) };
                card.err = Some((tf!("{} не установилось", "{} couldn\u{2019}t install", what), f.text.clone(), tf!("Ошибка {}", "Error {}", f.code)));
            }
        }
    }
    let mut gs = vec![Group::new("").extra(crate::Extra::WuCard(Box::new(card)))];
    // the list
    match mode {
        Mode::Found => {
            let all_on = req.iter().all(selected);
            let mut g = Group::new(t!("Готово к установке", "Ready to install")).note(t!("Снимите галочку, чтобы пропустить обновление в этот раз", "Untick an update to skip it this time")).rows(req.iter().map(|u| row(u, UMode::Pick(selected(u)))).collect());
            if !req.is_empty() {
                g = g.act(if all_on { t!("Снять все", "Clear all") } else { t!("Выбрать все", "Select all") }, false, GAct::WuSel(!all_on));
                gs.push(g);
            }
            if !opt.is_empty() {
                let open = OPT_OPEN.get();
                let mut g = Group::new(tf!("Необязательные обновления ({})", "Optional updates ({})", opt.len())).note(t!("Драйверы и предварительные · ставьте, только если что-то работает не так", "Drivers and previews \u{b7} install only if something doesn\u{2019}t work right")).act(if open { t!("Скрыть", "Hide") } else { t!("Показать", "Show") }, false, GAct::WuOpt);
                if open {
                    g = g.rows(opt.iter().map(|u| row(u, UMode::Pick(selected(u)))).collect());
                }
                gs.push(g);
            }
        }
        Mode::Run => {
            let rows = run
                .iter()
                .enumerate()
                .map(|(i, u)| {
                    let (d, tone, spin, right, rt, bar) = match step(&run, i, p) {
                        Step::Waiting => (D_DOWN, Tone::Dim, false, t!("Ждёт", "Waiting").to_string(), Tone::Faint, None),
                        Step::Down(f) => (D_DOWN, Tone::Dim, false, tf!("Загрузка {}%", "Downloading {}%", (f * 100.0).round() as u32), Tone::Dim, Some((f, false))),
                        Step::Downloaded => (D_DOWN, Tone::Dim, false, t!("Загружено", "Downloaded").to_string(), Tone::Faint, Some((1.0, false))),
                        Step::Install(f) => (D_SPIN, Tone::Dim, true, tf!("Установка {}%", "Installing {}%", (f * 100.0).round() as u32), Tone::Accent, Some((f, false))),
                        Step::Done => (look::CHECK, Tone::Good, false, t!("Готово", "Done").to_string(), Tone::Good, None),
                    };
                    row(u, UMode::Icon { d, tone, spin, right, right_tone: rt, bar })
                })
                .collect();
            gs.push(Group::new(t!("Обновления", "Updates")).rows(rows));
        }
        Mode::Restart | Mode::Sched if waiting > 0 => {
            let rows = run.iter().filter(|u| u.restart).map(|u| row(u, UMode::Icon { d: D_REFRESH, tone: Tone::Warn, spin: false, right: t!("Ждёт перезагрузки", "Waiting for restart").to_string(), right_tone: Tone::Warn, bar: None })).collect();
            gs.push(Group::new(t!("Ждут перезагрузки", "Waiting for the restart")).rows(rows));
        }
        Mode::Failed => {
            let bad = st.failed.as_ref().map(|f| f.kb.clone()).unwrap_or_default();
            let rows = run
                .iter()
                .map(|u| {
                    let failed = !bad.is_empty() && u.kb == bad;
                    row(u, UMode::Icon { d: if failed { D_CROSS } else { look::CHECK }, tone: if failed { Tone::Bad } else { Tone::Good }, spin: false, right: if failed { t!("Не удалось", "Failed").to_string() } else { t!("Установлено", "Installed").to_string() }, right_tone: if failed { Tone::Bad } else { Tone::Good }, bar: None })
                })
                .collect();
            gs.push(Group::new(t!("На этот раз", "This time")).rows(rows));
        }
        _ => {}
    }
    // the history
    let all = HIST_ALL.get();
    if !ex.history.is_empty() {
        if all {
            for (i, g) in [HGroup::Quality, HGroup::Driver, HGroup::Definition, HGroup::Other].into_iter().enumerate() {
                let rows: Vec<Line> = ex.history.iter().filter(|h| h.group == g).map(hist_row).collect();
                if rows.is_empty() {
                    continue;
                }
                let title = if i == 0 { format!("{} \u{b7} {}", t!("История обновлений", "Update history"), hist_title(g)) } else { hist_title(g).to_string() };
                let mut grp = Group::new(title).rows(rows);
                if i == 0 {
                    grp = grp.act(t!("Показать меньше", "Show less"), false, GAct::WuHist);
                }
                gs.push(grp);
            }
        } else {
            gs.push(Group::new(t!("История обновлений", "Update history")).note(t!("Последние", "Latest")).act(t!("Показать все", "Show all"), false, GAct::WuHist).rows(ex.history.iter().take(3).map(hist_row).collect()));
        }
    }
    // more
    let reg = |n: &str| o.windows_value(n);
    let build = o.about_info().build;
    let mut more_rows = vec![Line::new(Row::Item(t!("Удалить обновления", "Uninstall updates").into(), String::new(), Cmd::WuUninstall)).key("wu-uninstall").subtitle(t!("Откроется список установленных обновлений Windows", "Windows\u{2019} own uninstaller opens"))];
    if let Some((a, b)) = ex.active {
        more_rows.push(Line::new(Row::Info(t!("Активные часы", "Active hours").into(), format!("{a:02}:00 \u{2013} {b:02}:00"))).subtitle(t!("В это время Windows не перезагрузится", "Windows won\u{2019}t restart in these hours")));
    }
    more_rows.push(Line::new(Row::Item(t!("Дополнительные параметры в Параметрах Windows", "Advanced options in Windows Settings").into(), String::new(), Cmd::WuAdvanced)).key("wu-advanced").subtitle(t!("Пауза обновлений, оптимизация доставки, Insider", "Pause updates, delivery optimization, Insider")));
    more_rows.push(Line::new(Row::Info(t!("Этот ПК", "This PC").into(), format!("{} \u{b7} {} \u{b7} {}", crate::os_name(&reg("ProductName"), reg("CurrentBuild").parse().unwrap_or(0)), reg("DisplayVersion"), if build.is_empty() { reg("CurrentBuild") } else { build }))));
    gs.push(Group::new(if all { "" } else { t!("Ещё", "More") }).rows(more_rows).foot(t!("Сам WinRust обновляется отдельно: настройки панели → Обновления.", "WinRust itself updates separately: Bar settings \u{2192} Updates.")));
    gs
}

/// The category's colour: (the plate behind it as a tone, the text's tone).
#[must_use]
pub fn cat_tone(c: Cat) -> Tone {
    match c {
        Cat::Security => Tone::Bad,
        Cat::Net => Tone::Accent,
        Cat::Definitions => Tone::Good,
        Cat::Driver => Tone::Dim,
        Cat::Preview => Tone::Warn,
        Cat::Cumulative => Tone::Accent,
        Cat::Other => Tone::Dim,
    }
}

/// The apps a restart would close, for the dialog's line ("File manager, Task Manager, Telegram and 3 more").
#[must_use]
pub fn apps_line(names: &[String]) -> String {
    let first = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    match names.len() {
        0 => String::new(),
        n if n > 3 => tf!("{} и ещё {}", "{} and {} more", first, n - 3),
        _ => first,
    }
}
