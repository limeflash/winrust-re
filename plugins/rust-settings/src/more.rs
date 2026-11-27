//! The pages of the second design round: Notifications, Privacy & security, Keyboard & mouse, Date & time. The types are what the
//! window asks of `os()` for them (the real answers are sysx.rs, the demo's are demo.rs); the builders make the groups of the pages.
//! What Windows has no way to set from a program is not here (see the header of main.rs): the design's Default apps and Shortcuts
//! pages, the automatic Do-not-disturb rules, the layout editor, the touchpad, the screen-recording switches.
use std::cell::Cell;

use crate::{Cmd, Extra, Group, Line, Op, Row, Tile, Tone, is_on, look, os::os};

// ---------------------------------------------------------------------------------------------------------------------
// what the pages ask of the system

/// The things an app can ask for (the folders of `CapabilityAccessManager\ConsentStore`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    Cam,
    Mic,
    Loc,
}

impl Cap {
    pub const ALL: [Cap; 3] = [Cap::Cam, Cap::Mic, Cap::Loc];

    /// The folder of the consent store.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Cap::Cam => "webcam",
            Cap::Mic => "microphone",
            Cap::Loc => "location",
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Cap::Cam => t!("Камера", "Camera", ja: "カメラ", pt: "Câmera", zh: "摄像头"),
            Cap::Mic => t!("Микрофон", "Microphone", ja: "マイク", pt: "Microfone", zh: "麦克风"),
            Cap::Loc => t!("Местоположение", "Location", ja: "位置情報", pt: "Localização", zh: "位置"),
        }
    }

    fn lower(self) -> &'static str {
        match self {
            Cap::Cam => t!("камерой", "camera", ja: "カメラ", pt: "câmera", zh: "摄像头"),
            Cap::Mic => t!("микрофоном", "microphone", ja: "マイク", pt: "microfone", zh: "麦克风"),
            Cap::Loc => t!("местоположением", "location", ja: "位置情報", pt: "localização", zh: "位置"),
        }
    }

    const fn glyph(self) -> &'static str {
        match self {
            Cap::Cam => look::CAMERA,
            Cap::Mic => look::MIC,
            Cap::Loc => look::LOCATION,
        }
    }
}

/// Whose permission a switch of the App access list is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Who {
    /// The whole PC (`HKLM`: needs administrator rights).
    Pc,
    /// This user's apps at all.
    User,
    /// Every desktop app together (they cannot be told apart).
    Desktop,
    /// One Store app (its package family).
    App(String),
}

/// A yes/no the system keeps that is not one DWORD of `HKCU`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Flag {
    /// Do not disturb (the bar's own file `%APPDATA%\WinRust\dnd.txt`).
    Dnd,
    /// Enhance pointer precision.
    MouseAccel,
    /// Windows sets the clock from the internet (the time service; changing it needs administrator rights).
    AutoTime,
    Access(Cap, Who),
}

/// A value picked from a short list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    /// How long a pop-up stays (the accessibility setting Windows' toasts follow).
    Toast,
    /// The keys that switch the layout.
    LangKey,
    /// Which mouse button is the main one.
    Hand,
    /// How far the wheel scrolls.
    Wheel,
    /// 24-hour or 12-hour clock.
    Clock,
    /// The short date.
    Date,
    /// The first day of the week.
    Week,
    /// The time zone.
    Zone,
}

impl Opt {
    /// Shown as a segmented control (the others are drop-downs).
    #[must_use]
    pub const fn segmented(self) -> bool {
        matches!(self, Opt::Toast | Opt::LangKey | Opt::Hand | Opt::Clock)
    }
}

/// A number the system keeps as a step of a scale: 0..=1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gauge {
    KeyDelay,
    KeyRate,
    Pointer,
}

/// One app of the Notifications list. `mode`: 0 banner, 1 only in the notification centre, 2 off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyApp {
    pub id: String,
    pub name: String,
    pub mode: usize,
}

/// One key of the consent store's `NonPackaged` folder as it stands in the registry: a desktop app's exe path with `#` for the
/// backslash, and the two times Windows keeps (a FILETIME each; 0 = none, and for the stop 0 also means "still open").
#[derive(Clone, Debug)]
pub struct RawDesk {
    pub key: String,
    pub start: u64,
    pub stop: u64,
    pub start_when: Option<When>,
    pub stop_when: Option<When>,
}

/// The exe's path of a `NonPackaged` key (`C:#Users#me#Discord#app-1.0.9251#Discord.exe` is the path with backslashes).
#[must_use]
pub fn desk_path(key: &str) -> String {
    key.replace('#', "\\")
}

fn exe_of(key: &str) -> String {
    key.rsplit('#').next().unwrap_or(key).to_lowercase()
}

/// One row per app: the keys of an app that installs every update into a new folder (Discord, Edge WebView) are one app, named by the
/// exe's file name; the key with the latest start speaks for it. It is "in use now" only when that key was never closed AND a process of
/// that exe name runs now (a crash leaves a key open for ever). `describe` gives the exe's own name (its version info) when it is still there.
pub fn group_desktop(raw: Vec<RawDesk>, running: &dyn Fn(&str) -> bool, describe: &dyn Fn(&str) -> Option<String>) -> Vec<CApp> {
    let mut best: Vec<(String, RawDesk)> = Vec::new();
    for r in raw {
        let exe = exe_of(&r.key);
        match best.iter_mut().find(|(e, _)| *e == exe) {
            Some((_, b)) => {
                if r.start > b.start {
                    *b = r;
                }
            }
            None => best.push((exe, r)),
        }
    }
    best.into_iter()
        .map(|(exe, r)| {
            let live = r.start != 0 && r.stop == 0 && running(&exe);
            let last = if r.stop == 0 { r.start_when } else { r.stop_when };
            let name = describe(&desk_path(&r.key)).unwrap_or_else(|| app_name(&r.key));
            CApp { id: r.key, name, desktop: true, last, live }
        })
        .collect()
}

/// When something happened, in the user's time: whole days ago and the wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct When {
    pub days_ago: u32,
    pub hour: u8,
    pub min: u8,
    pub day: u8,
    pub month: u8,
}

/// An app that asked for a camera, a microphone or the location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CApp {
    pub id: String,
    pub name: String,
    pub desktop: bool,
    pub last: Option<When>,
    /// It has it open right now.
    pub live: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Consent {
    pub apps: Vec<CApp>,
}

/// What Windows Security says (None: it does not say / another product is in charge).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Security {
    pub av: Option<bool>,
    pub fw: Option<bool>,
    pub sig: String,
    pub sig_when: Option<When>,
    pub smart: String,
    pub scanning: bool,
    /// A quick scan finished since this window opened.
    pub scanned: bool,
}

/// The facts of the About page that are not in the registry's Windows values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AboutInfo {
    pub cpu: String,
    pub ram: String,
    pub gpus: Vec<String>,
    pub sys_type: String,
    /// "26100.2314": the build and its revision.
    pub build: String,
    pub winrust: String,
    /// The name a rename set, while the running one is still the old (it applies after a restart).
    pub pc_pending: Option<String>,
}

/// A computer name Windows takes: 1 to 15 letters, digits or hyphens (NetBIOS), not starting or ending with a hyphen, not only digits.
#[must_use]
pub fn valid_pc_name(n: &str) -> bool {
    (1..=15).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !n.starts_with('-') && !n.ends_with('-') && !n.chars().all(|c| c.is_ascii_digit())
}

/// A keyboard layout of the sign-in list: the language, the layout, the two letters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub sub: String,
    pub tag: String,
}

// ---------------------------------------------------------------------------------------------------------------------
// what the window itself keeps

thread_local! { static PV_TAB: Cell<Cap> = const { Cell::new(Cap::Mic) } } // the tab of App access

/// The tab of the App access list on show.
#[must_use]
pub fn pv_tab() -> Cap {
    PV_TAB.get()
}

pub fn set_pv_tab(c: Cap) {
    PV_TAB.set(c);
}

// ---------------------------------------------------------------------------------------------------------------------
// registry places the generic rows use

pub const TOAST: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
pub const NOC: &str = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings";

/// The short-date patterns the Date list offers.
pub const DATE_PATS: [&str; 5] = ["dd.MM.yyyy", "dd/MM/yyyy", "MM/dd/yyyy", "yyyy-MM-dd", "d.M.yyyy"];

// ---------------------------------------------------------------------------------------------------------------------
// words

/// `pat` ("dd.MM.yyyy", "M/d/yyyy") for a date.
#[must_use]
pub fn fmt_date(pat: &str, y: u32, m: u32, d: u32) -> String {
    let mut out = String::new();
    let cs: Vec<char> = pat.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let n = cs[i..].iter().take_while(|&&x| x == c).count();
        match c {
            'y' => out.push_str(&if n >= 4 { format!("{y:04}") } else { format!("{:02}", y % 100) }),
            'M' => out.push_str(&if n >= 2 { format!("{m:02}") } else { m.to_string() }),
            'd' => out.push_str(&if n >= 2 { format!("{d:02}") } else { d.to_string() }),
            _ => out.extend(std::iter::repeat_n(c, n)),
        }
        i += n;
    }
    out
}

pub fn month_name(m: u8) -> &'static str {
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const RU: [&str; 12] = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
    let i = usize::from(m.clamp(1, 12)) - 1;
    t!(RU[i], EN[i])
}

/// "Today, 12:40", "Yesterday, 21:40", or the day and month.
#[must_use]
pub fn when_text(w: When) -> String {
    match w.days_ago {
        0 => format!("{}, {:02}:{:02}", t!("Сегодня", "Today"), w.hour, w.min),
        1 => format!("{}, {:02}:{:02}", t!("Вчера", "Yesterday"), w.hour, w.min),
        _ => format!("{} {}", w.day, month_name(w.month)),
    }
}

/// `when_text` inside a sentence: "last checked today, 17:02", "last checked 20 Nov".
#[must_use]
pub fn when_sentence(w: When) -> String {
    if w.days_ago < 2 { lower_first(&when_text(w)) } else { when_text(w) }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_lowercase().chain(c).collect())
}

/// A readable name for an app id: a package family ("Microsoft.WindowsCamera_8wekyb3d8bbwe"), an AppUserModelID
/// ("Foo.Bar_x!App"), or an exe path as the consent store writes it ("C:#Program Files#Discord#Discord.exe").
#[must_use]
pub fn app_name(id: &str) -> String {
    let s = id.split('!').next().unwrap_or(id);
    let s = s.rsplit(['\\', '#', '/']).next().unwrap_or(s);
    let s = if s.len() > 4 && s[s.len() - 4..].eq_ignore_ascii_case(".exe") { &s[..s.len() - 4] } else { s };
    let s = s.split('_').next().unwrap_or(s);
    // "electron.app.Foo" is Foo, "im.riot.app" is riot: the last part that is not just a word of the packaging
    let s = if s.contains('.') { s.rsplit('.').find(|p| !p.is_empty() && !["app", "desktop", "exe", "application", "client", "win", "windows"].contains(&p.to_lowercase().as_str())).unwrap_or(s) } else { s };
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if c.is_uppercase() && prev.is_some_and(char::is_lowercase) {
            out.push(' ');
        }
        out.push(c);
        prev = Some(c);
    }
    let mut cs = out.chars();
    cs.next().map_or_else(|| id.to_string(), |f| f.to_uppercase().chain(cs).collect())
}

/// One of a few plate colours, the same for the same name.
fn plate(name: &str) -> u32 {
    const COLORS: [u32; 8] = [0x3b82f6, 0xe2477a, 0x2f9e5b, 0x8b5cf6, 0xf08c1a, 0x0ea5a4, 0x6b7187, 0x4f46e5];
    COLORS[name.bytes().map(usize::from).sum::<usize>() % COLORS.len()]
}

fn app_tile(name: &str) -> Tile {
    Tile::Solid(look::APP_GRID, plate(name))
}

fn mode_names() -> [&'static str; 3] {
    [t!("Всплывающее окно", "Banner", ja: "バナー", pt: "Banner", zh: "横幅"), t!("Только в центре", "Only in the center", ja: "通知センターのみ", pt: "Só na central", zh: "仅通知中心"), t!("Выключено", "Off", ja: "オフ", pt: "Desligado", zh: "关闭")]
}

fn gauge_sub(g: Gauge, v: f32) -> String {
    match g {
        Gauge::KeyDelay => if v < 0.5 { t!("Короткая", "Short") } else if v < 0.8 { t!("Средняя", "Medium") } else { t!("Длинная", "Long") }.into(),
        Gauge::KeyRate => if v > 0.66 { t!("Быстро", "Fast") } else if v > 0.33 { t!("Средне", "Medium") } else { t!("Медленно", "Slow") }.into(),
        Gauge::Pointer => String::new(),
    }
}

/// The text under a gauge for the value it holds now.
#[must_use]
pub fn gauge_text(g: Gauge, v: Option<f32>) -> String {
    v.map_or_else(String::new, |v| gauge_sub(g, v))
}

/// The label of a segmented or drop-down list `o` of the system, and its entries.
#[must_use]
pub fn opt_label(o: Opt) -> &'static str {
    match o {
        Opt::Toast => t!("Показывать на экране", "Keep on screen", ja: "画面に表示する時間", pt: "Manter na tela", zh: "在屏幕上停留"),
        Opt::LangKey => t!("Переключать", "Switch with", ja: "切り替えキー", pt: "Alternar com", zh: "切换方式"),
        Opt::Hand => t!("Основная кнопка", "Primary button", ja: "主ボタン", pt: "Botão principal", zh: "主按钮"),
        Opt::Wheel => t!("Колесо прокручивает", "Wheel scrolls", ja: "ホイールのスクロール量", pt: "A roda rola", zh: "滚轮滚动"),
        Opt::Clock => t!("Часы", "Clock", ja: "時計", pt: "Relógio", zh: "时钟"),
        Opt::Date => t!("Дата", "Date", ja: "日付", pt: "Data", zh: "日期"),
        Opt::Week => t!("Первый день недели", "First day of the week", ja: "週の最初の曜日", pt: "Primeiro dia da semana", zh: "每周的第一天"),
        Opt::Zone => t!("Часовой пояс", "Time zone", ja: "タイム ゾーン", pt: "Fuso horário", zh: "时区"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Notifications

pub fn notify_rows() -> Vec<Group> {
    let o = os();
    let all = is_on(o.reg_dword(TOAST, "ToastEnabled"), 1, 1);
    let dim = |mut l: Line| {
        l.dis = !all;
        l
    };
    let toggle = |label: &str, name: &'static str| Line::new(crate::toggle_row(label, NOC, name, 1, 1));
    let mut gs = vec![Group::new("").rows(vec![
        Line::new(crate::toggle_row(t!("Уведомления", "Notifications", ja: "通知", pt: "Notificações", zh: "通知"), TOAST, "ToastEnabled", 1, 1))
            .subtitle(if all { t!("Приложения могут показывать окна и добавлять в центр", "Apps can show pop-ups and add to the center") } else { t!("Выключено · ничего не всплывает и не хранится", "Off \u{b7} nothing pops up, nothing is kept") })
            .weight(600),
        dim(Line::new(Row::Flag(t!("Не беспокоить", "Do not disturb", ja: "サイレント モード", pt: "Não perturbe", zh: "勿扰").into(), Flag::Dnd))
            .subtitle(if o.flag(&Flag::Dnd) == Some(true) { t!("Включено · уведомления ждут в центре тихо", "On \u{b7} notifications wait quietly in the center") } else { t!("Всплывающие окна скрыты, ничего не теряется", "Pop-ups are hidden, nothing is lost") })),
    ])];
    gs.push(Group::new(t!("Всплывающие окна", "Pop-ups", ja: "ポップアップ", pt: "Pop-ups", zh: "弹出通知")).rows(vec![
        dim(Line::new(Row::Cycle(opt_label(Opt::Toast).into(), String::new, Op::Opt(Opt::Toast)))),
        dim(toggle(t!("Воспроизводить звук", "Play a sound", ja: "サウンドを再生", pt: "Reproduzir um som", zh: "播放声音"), "NOC_GLOBAL_SETTING_ALLOW_NOTIFICATION_SOUND")),
    ]));
    let names = mode_names();
    let apps = o.notify_apps();
    let none = apps.is_empty();
    let rows = apps
        .into_iter()
        .map(|a| {
            let sub = if all { names[a.mode.min(2)].to_string() } else { t!("Выключено для всех приложений", "Off for every app").to_string() };
            let mut l = Line::new(Row::Item(a.name.clone(), String::new(), Cmd::NotifyApp(a.id.clone()))).key(format!("na:{}", a.id)).subtitle(sub).tile(app_tile(&a.name)).tone(if a.mode >= 2 || !all { Tone::Faint } else { Tone::Dim });
            l.cur = Some(a.mode);
            l
        })
        .collect();
    gs.push(
        Group::new(t!("Приложения", "Apps", ja: "アプリ", pt: "Apps", zh: "应用"))
            .note(t!("Нажмите на приложение, чтобы выбрать, как оно уведомляет", "Click an app to choose how it notifies you"))
            .empty(if none { t!("Ни одно приложение ещё не присылало уведомлений", "No app has sent a notification yet") } else { "" })
            .rows(rows),
    );
    gs
}

/// The labels of an app's three ways to notify (what unfolds under it).
#[must_use]
pub fn mode_labels() -> [&'static str; 3] {
    mode_names()
}

// ---------------------------------------------------------------------------------------------------------------------
// Keyboard & mouse

pub fn keyboard_rows() -> Vec<Group> {
    let o = os();
    let mut rows: Vec<Line> = o
        .layouts()
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let sub = if i == 0 { format!("{} \u{b7} {}", l.sub, t!("по умолчанию", "default")) } else { l.sub.clone() };
            let mut line = Line::new(Row::Info(l.name.clone(), String::new())).key(format!("lay:{}", l.tag)).subtitle(sub).tile(Tile::Quiet(look::KEYBOARD));
            line.badge = Some((l.tag, if i == 0 { Tone::Accent } else { Tone::Dim }));
            line
        })
        .collect();
    rows.push(Line::new(Row::Cycle(opt_label(Opt::LangKey).into(), String::new, Op::Opt(Opt::LangKey))));
    let gauge = |label: &str, g: Gauge| Line::new(Row::Gauge(label.into(), g));
    vec![
        Group::new(t!("Раскладки клавиатуры", "Keyboard layouts", ja: "キーボード レイアウト", pt: "Layouts de teclado", zh: "键盘布局")).note(t!("Первая используется при входе", "The first one is used when you sign in")).rows(rows),
        Group::new(t!("Ввод", "Typing", ja: "入力", pt: "Digitação", zh: "输入")).rows(vec![
            gauge(t!("Задержка перед повтором клавиши", "Delay before a key repeats", ja: "キーが繰り返されるまでの時間", pt: "Atraso antes de repetir uma tecla", zh: "按键重复前的延迟"), Gauge::KeyDelay),
            gauge(t!("Скорость повтора", "Repeat speed", ja: "リピート速度", pt: "Velocidade de repetição", zh: "重复速度"), Gauge::KeyRate),
            Line::new(Row::Cycle(t!("Попробуйте", "Try it", ja: "試してみる", pt: "Experimente", zh: "试一试").into(), String::new, Op::Try)).key("try"),
        ]),
        Group::new(t!("Мышь", "Mouse", ja: "マウス", pt: "Mouse", zh: "鼠标")).rows(vec![
            gauge(t!("Скорость указателя", "Pointer speed", ja: "ポインターの速度", pt: "Velocidade do ponteiro", zh: "指针速度"), Gauge::Pointer),
            Line::new(Row::Flag(t!("Повышенная точность указателя", "Enhance pointer precision", ja: "ポインターの精度を高める", pt: "Aprimorar a precisão do ponteiro", zh: "提高指针精确度").into(), Flag::MouseAccel)).subtitle(t!("Указатель уходит дальше, если двигать мышь быстро", "Moves further when you move the mouse fast")),
            Line::new(Row::Cycle(opt_label(Opt::Hand).into(), String::new, Op::Opt(Opt::Hand))),
            Line::new(Row::Cycle(opt_label(Opt::Wheel).into(), String::new, Op::Opt(Opt::Wheel))),
        ]),
    ]
}

// ---------------------------------------------------------------------------------------------------------------------
// Date & time

pub fn time_rows() -> Vec<Group> {
    let o = os();
    let auto = o.flag(&Flag::AutoTime);
    let mut sync = Line::new(Row::Item(t!("Синхронизировать сейчас", "Sync now", ja: "今すぐ同期", pt: "Sincronizar agora", zh: "立即同步").into(), t!("Синхронизировать", "Sync").into(), Cmd::SyncTime)).key("sync");
    sync.sub = o.time_server();
    vec![
        Group::new(t!("На панели", "On the bar", ja: "バーに表示", pt: "Na barra", zh: "在任务栏上")).rows(vec![
            {
                let mut l = Line::new(Row::Info(o.clock_text(), String::new())).key("preview").subtitle(t!("Так выглядят часы с этими настройками", "How the clock looks with these settings")).tile(Tile::Lit(look::CLOCK)).weight(600);
                l.h = Some(60.0);
                l
            },
            Line::new(Row::Cycle(opt_label(Opt::Clock).into(), String::new, Op::Opt(Opt::Clock))),
        ]),
        Group::new(t!("Время", "Time", ja: "時刻", pt: "Hora", zh: "时间")).rows(if auto == Some(true) {
            vec![auto_row(auto), sync, Line::new(Row::Cycle(opt_label(Opt::Zone).into(), String::new, Op::Opt(Opt::Zone)))]
        } else {
            vec![auto_row(auto), Line::new(Row::Cycle(opt_label(Opt::Zone).into(), String::new, Op::Opt(Opt::Zone)))]
        }),
        Group::new(t!("Регион и форматы", "Region and format", ja: "地域と形式", pt: "Região e formato", zh: "区域和格式")).rows(vec![
            Line::new(Row::Info(t!("Регион", "Region", ja: "地域", pt: "Região", zh: "区域").into(), o.region_text())).subtitle(t!("Форматы ниже берутся из региона Windows", "The formats below follow the Windows region")),
            Line::new(Row::Cycle(opt_label(Opt::Date).into(), String::new, Op::Opt(Opt::Date))),
            Line::new(Row::Cycle(opt_label(Opt::Week).into(), String::new, Op::Opt(Opt::Week))),
            Line::new(Row::Info(t!("Числа и деньги", "Numbers and money", ja: "数値と通貨", pt: "Números e moeda", zh: "数字和货币").into(), o.numbers_text())),
        ]),
    ]
}

fn auto_row(auto: Option<bool>) -> Line {
    Line::new(Row::Flag(t!("Устанавливать время автоматически", "Set time automatically", ja: "時刻を自動的に設定する", pt: "Definir a hora automaticamente", zh: "自动设置时间").into(), Flag::AutoTime)).subtitle(if auto == Some(true) { t!("Часы сверяются с интернетом", "The clock is set from the internet") } else { t!("Выключено · часы могут уходить", "Off \u{b7} the clock may drift") })
}

// ---------------------------------------------------------------------------------------------------------------------
// Privacy & security

pub fn privacy_rows() -> Vec<Group> {
    let o = os();
    let amber_badge = |s: &str| Some((format!("\u{25cf} {s}"), Tone::Warn));
    let now = |cap: Cap| {
        let c = o.consent(cap);
        let blocked = o.flag(&Flag::Access(cap, Who::Pc)) == Some(false) || o.flag(&Flag::Access(cap, Who::User)) == Some(false);
        let live = c.apps.iter().find(|a| a.live);
        let last = c.apps.iter().filter_map(|a| a.last.map(|w| (a, w))).min_by_key(|(_, w)| (w.days_ago, u32::from(23 - w.hour.min(23)) * 60 + u32::from(59 - w.min.min(59))));
        let sub = if blocked {
            t!("Закрыт для всех приложений", "Blocked for every app").to_string()
        } else if let Some(a) = live {
            tf!("Используется: {}", "In use by {}", a.name)
        } else if let Some((a, w)) = last {
            tf!("Последний раз: {} · {}", "Last used: {} \u{b7} {}", a.name, lower_first(&when_text(w)))
        } else {
            t!("Ещё никто не пользовался", "Not used yet").to_string()
        };
        let mut l = Line::new(Row::Item(cap.name().into(), String::new(), Cmd::PrivGo(cap))).key(format!("now:{}", cap.key())).subtitle(sub).tile(if live.is_some() { Tile::Soft(cap.glyph(), Tone::Warn) } else { Tile::Quiet(cap.glyph()) });
        if live.is_some() {
            l.badge = amber_badge(t!("Используется", "In use"));
        } else if blocked {
            l.badge = Some((t!("Выкл.", "Off").into(), Tone::Dim));
        }
        l
    };
    let sec = o.security();
    let good = |on: Option<bool>| if on == Some(true) { Tone::Good } else if on == Some(false) { Tone::Warn } else { Tone::Dim };
    let av = {
        let sub = if sec.scanning {
            t!("Проверяем места, где обычно прячется вредоносное ПО…", "Checking the places malware usually hides\u{2026}").to_string()
        } else if sec.scanned {
            t!("Угроз нет · быстрая проверка только что", "No threats \u{b7} quick scan just now").to_string()
        } else {
            match sec.av {
                Some(true) => t!("Защита в реальном времени включена", "Real-time protection is on").to_string(),
                Some(false) => t!("Защита в реальном времени выключена", "Real-time protection is off").to_string(),
                None => t!("Защитой занимается другая программа", "Another product looks after this").to_string(),
            }
        };
        let tone = if sec.av == Some(false) { Tone::Warn } else if sec.av.is_some() { Tone::Good } else { Tone::Dim };
        let mut l = Line::new(Row::Item(t!("Защита от вирусов и угроз", "Virus and threat protection", ja: "ウイルスと脅威の防止", pt: "Proteção contra vírus e ameaças", zh: "病毒和威胁防护").into(), if sec.scanning { t!("Проверка", "Scanning").into() } else { t!("Быстрая проверка", "Quick scan").into() }, Cmd::QuickScan))
            .key("sec:av")
            .subtitle(sub)
            .tile(Tile::Soft(if tone == Tone::Warn { look::SHIELD_INFO } else { look::SHIELD_CHECK }, tone));
        l.busy = sec.scanning;
        l
    };
    let onoff = |v: Option<bool>| match v {
        Some(true) => t!("Включён", "On").to_string(),
        Some(false) => t!("Выключен", "Off").to_string(),
        None => "\u{2014}".to_string(),
    };
    let sec_rows = vec![
        av,
        Line::new(Row::Info(t!("Брандмауэр", "Firewall", ja: "ファイアウォール", pt: "Firewall", zh: "防火墙").into(), onoff(sec.fw))).subtitle(t!("Частные и общедоступные сети", "Private and public networks")).tile(Tile::Soft(look::FIREWALL, good(sec.fw))),
        Line::new(Row::Info(t!("Обновления защиты", "Protection updates", ja: "保護の更新", pt: "Atualizações de proteção", zh: "保护更新").into(), sec.sig_when.map_or_else(String::new, when_text)))
            .subtitle(if sec.sig.is_empty() { String::new() } else { tf!("Определения {}", "Definitions {}", sec.sig) })
            .tile(Tile::Soft(look::REFRESH, if sec.sig.is_empty() { Tone::Dim } else { Tone::Good })),
        Line::new(Row::Info(t!("Неизвестные приложения (SmartScreen)", "Unknown apps (SmartScreen)", ja: "不明なアプリ (SmartScreen)", pt: "Apps desconhecidos (SmartScreen)", zh: "未知应用 (SmartScreen)").into(), sec.smart.clone()))
            .subtitle(t!("WinRust пока не подписан, поэтому Windows может один раз предупредить при установке", "WinRust isn\u{2019}t signed yet, so Windows may warn once when you install it"))
            .tile(Tile::Quiet(look::SHIELD_INFO)),
        Line::new(Row::Item(t!("Открыть Безопасность Windows", "Open Windows Security", ja: "Windows セキュリティを開く", pt: "Abrir a Segurança do Windows", zh: "打开 Windows 安全中心").into(), String::new(), Cmd::OpenSecurity)).key("sec:open").subtitle(t!("Проверки, исключения, защита от шифровальщиков", "Scans, exclusions, ransomware protection")),
    ];
    let tab = pv_tab();
    let c = o.consent(tab);
    let flag = |who: Who| o.flag(&Flag::Access(tab, who));
    let (dev, user, desk) = (flag(Who::Pc), flag(Who::User), flag(Who::Desktop));
    let all_off = dev == Some(false) || user == Some(false);
    let (store, desktop): (Vec<&CApp>, Vec<&CApp>) = c.apps.iter().partition(|a| !a.desktop);
    let n_on = store.iter().filter(|a| flag(Who::App(a.id.clone())) == Some(true)).count();
    let used = |a: &CApp, on: bool| {
        let when = a.last.map_or_else(|| t!("ещё не использовалось", "never used").to_string(), |w| tf!("использовалось {}", "used {}", if w.days_ago < 2 { lower_first(&when_text(w)) } else { when_text(w) }));
        format!("{} \u{b7} {when}", if on { t!("Разрешено", "Allowed") } else { t!("Не разрешено", "Not allowed") })
    };
    let mut rows = vec![Line::new(Row::Cycle(t!("Показать", "Show", ja: "表示", pt: "Mostrar", zh: "显示").into(), String::new, Op::PvTab)).key("pvtab")];
    let mut pc = Line::new(Row::Flag(tf!("{} на этом ПК", "{} on this PC", tab.name()), Flag::Access(tab, Who::Pc))).key(format!("{}:dev", tab.key())).subtitle(if dev == Some(false) { t!("Выключено для всех, включая Windows", "Off for everyone, including Windows") } else { t!("Windows и любое приложение могут запросить", "Windows itself and every app can ask for it") });
    pc.badge = Some((t!("Админ", "Admin").into(), Tone::Dim));
    rows.push(pc);
    let mut u = Line::new(Row::Flag(tf!("Разрешить приложениям пользоваться {}", "Let apps use the {}", tab.lower()), Flag::Access(tab, Who::User)))
        .key(format!("{}:user", tab.key()))
        .subtitle(if dev == Some(false) { t!("Выключено для всего ПК выше", "Turned off for the whole PC above").to_string() } else if user == Some(true) { tf!("Разрешено магазинных приложений: {} из {} · классические ниже", "{} of {} Store apps allowed \u{b7} desktop apps below", n_on, store.len()) } else { t!("Никому нельзя, но можно разрешить отдельным ниже", "No app gets it, you can still allow it per app later").to_string() });
    u.dis = dev == Some(false);
    rows.push(u);
    for a in &store {
        let on = flag(Who::App(a.id.clone())) == Some(true);
        let mut l = Line::new(Row::Flag(a.name.clone(), Flag::Access(tab, Who::App(a.id.clone())))).key(format!("{}:{}", tab.key(), a.id)).subtitle(if all_off { t!("Заблокировано", "Blocked").to_string() } else { used(a, on) }).tile(app_tile(&a.name));
        l.dis = all_off;
        rows.push(l);
    }
    let mut d = Line::new(Row::Flag(t!("Классические приложения", "Desktop apps", ja: "デスクトップ アプリ", pt: "Apps de desktop", zh: "桌面应用").into(), Flag::Access(tab, Who::Desktop))).key(format!("{}:desk", tab.key())).subtitle(if all_off { t!("Заблокировано", "Blocked") } else { t!("Windows разрешает или запрещает их все вместе, а не по одному", "Windows allows or blocks them together, not one by one") }).tile(Tile::Quiet(look::DESKTOP));
    d.dis = all_off;
    rows.push(d);
    for a in &desktop {
        let blocked = all_off || desk == Some(false);
        let mut l = Line::new(Row::Info(a.name.clone(), if blocked { t!("Заблокировано", "Blocked").to_string() } else if a.live { String::new() } else { a.last.map_or_else(String::new, when_text) }))
            .key(format!("{}:d:{}", tab.key(), a.id))
            .subtitle(t!("Классическое приложение", "Desktop app"))
            .tone(Tone::Faint)
            .tile(app_tile(&a.name));
        if a.live && !blocked {
            l.badge = amber_badge(t!("Используется сейчас", "In use now"));
        }
        rows.push(l);
    }
    vec![
        Group::new(t!("Сейчас", "Right now", ja: "現在", pt: "Agora", zh: "当前")).note(t!("Кто пользуется камерой, микрофоном и местоположением", "Who uses the camera, microphone and location")).rows(Cap::ALL.map(now).into()),
        Group::new(t!("Безопасность Windows", "Windows Security", ja: "Windows セキュリティ", pt: "Segurança do Windows", zh: "Windows 安全中心")).note(t!("Встроена в Windows · WinRust только показывает", "Built into Windows \u{b7} WinRust only shows it")).rows(sec_rows),
        Group::new(t!("Доступ приложений", "App access", ja: "アプリのアクセス", pt: "Acesso dos apps", zh: "应用访问")).rows(rows),
    ]
}

// ---------------------------------------------------------------------------------------------------------------------
// Default apps

/// The kinds of file and link the Default apps page lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefKind {
    Web,
    Files,
    Photos,
    Video,
    Pdf,
    Text,
    Term,
    Mail,
}

impl DefKind {
    pub const ALL: [DefKind; 8] = [DefKind::Web, DefKind::Files, DefKind::Photos, DefKind::Video, DefKind::Pdf, DefKind::Text, DefKind::Term, DefKind::Mail];

    /// What Windows is asked about: a file type (".jpg") or a link scheme ("http"); None for the two WinRust keeps itself.
    #[must_use]
    pub fn assoc(self) -> Option<Assoc> {
        let (name, protocol) = match self {
            DefKind::Web => ("http", true),
            DefKind::Photos => (".jpg", false),
            DefKind::Video => (".mp4", false),
            DefKind::Pdf => (".pdf", false),
            DefKind::Text => (".txt", false),
            DefKind::Mail => ("mailto", true),
            DefKind::Files | DefKind::Term => return None,
        };
        Some(Assoc { name: name.into(), protocol })
    }

    fn label(self) -> &'static str {
        match self {
            DefKind::Web => t!("Браузер", "Web browser", ja: "Web ブラウザー", pt: "Navegador", zh: "网页浏览器"),
            DefKind::Files => t!("Файловый менеджер", "File manager", ja: "ファイル マネージャー", pt: "Gerenciador de arquivos", zh: "文件管理器"),
            DefKind::Photos => t!("Фото", "Photos", ja: "フォト", pt: "Fotos", zh: "照片"),
            DefKind::Video => t!("Видео", "Video", ja: "ビデオ", pt: "Vídeo", zh: "视频"),
            DefKind::Pdf => "PDF",
            DefKind::Text => t!("Текст и код", "Text and code", ja: "テキストとコード", pt: "Texto e código", zh: "文本和代码"),
            DefKind::Term => t!("Терминал", "Terminal", ja: "ターミナル", pt: "Terminal", zh: "终端"),
            DefKind::Mail => t!("Почта", "Email", ja: "メール", pt: "E-mail", zh: "电子邮件"),
        }
    }

    fn sub(self) -> &'static str {
        match self {
            DefKind::Web => t!("Ссылки, .html, .htm", "Links, .html, .htm"),
            DefKind::Files => t!("Папки, Win + E, «Показать в папке»", "Folders, Win + E, \u{201c}Show in folder\u{201d}"),
            DefKind::Photos => ".jpg, .png, .webp, .heic, .gif",
            DefKind::Video => ".mp4, .mkv, .mov",
            DefKind::Pdf => ".pdf",
            DefKind::Text => ".txt, .md, .toml, .json, .log",
            DefKind::Term => t!("Win + T, «Открыть в терминале»", "Win + T, \u{201c}Open in terminal\u{201d}"),
            DefKind::Mail => t!("Ссылки mailto:", "mailto: links"),
        }
    }
}

/// A kind of file (".pdf") or link ("http") whose default app Windows keeps.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Assoc {
    pub name: String,
    pub protocol: bool,
}

/// An app Windows has registered for an association: its name and its exe's file name (lower case).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefApp {
    pub name: String,
    pub exe: String,
}

thread_local! { static EXT_QUERY: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) } } // the Default apps page's own box

pub fn set_ext_query(q: &str) {
    EXT_QUERY.with_borrow_mut(|e| *e = q.to_string());
}

#[must_use]
pub fn ext_query() -> String {
    EXT_QUERY.with_borrow(Clone::clone)
}

/// ".webp", "webp" and "*.webp" are one file type; anything that is not a plain extension (a path, spaces, symbols) is none.
#[must_use]
pub fn clean_ext(q: &str) -> Option<String> {
    let e = q.trim().trim_start_matches("*.").trim_start_matches('.').to_ascii_lowercase();
    (!e.is_empty() && e.len() <= 20 && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+')).then(|| format!(".{e}"))
}

pub fn defaults_rows() -> Vec<Group> {
    let o = os();
    let q = ext_query();
    let select = |label: &str, sub: String, op: Op, key: String, a: &Assoc| {
        let (apps, cur) = o.assoc_apps(a);
        let name = cur.and_then(|c| apps.get(c)).map_or_else(|| "\u{2014}".to_string(), |x| x.name.clone());
        let mut l = Line::new(Row::Cycle(label.into(), String::new, op)).key(key).subtitle(sub).tile(app_tile(&name));
        l.val = name;
        (l, !apps.is_empty())
    };
    let mut head = Group::new(t!("Открыть в", "Open with", ja: "開くアプリ", pt: "Abrir com", zh: "打开方式"))
        .note(t!("Выберите приложение для каждого вида файлов", "Pick an app for each kind of file"))
        .extra(Extra::Search(String::new()))
        .foot(t!("Выбор делается в параметрах Windows: они откроются по нажатию (Windows не даёт приложениям менять это самим).", "The choice is made in Windows\u{2019} settings, which open on a click (Windows lets no app change it on its own)."));
    if q.trim().is_empty() {
        head.rows = DefKind::ALL
            .iter()
            .map(|&k| match k.assoc() {
                Some(a) => {
                    // a link scheme cannot be chosen in the open-with dialog: it opens Windows' own settings (which lends Explorer while open)
                    let sub = if a.protocol { format!("{} \u{b7} {}", k.sub(), t!("откроются параметры Windows", "opens Windows Settings")) } else { k.sub().to_string() };
                    select(k.label(), sub, Op::Def(k), format!("def:{k:?}"), &a).0
                }
                // WinRust's own file manager and terminal are fixed by what is installed, not a setting of Windows: shown, not offered
                None => {
                    let own = o.own_default(k);
                    Line::new(Row::Info(k.label().into(), own.clone())).key(format!("def:{k:?}")).subtitle(format!("{} \u{b7} {}", k.sub(), t!("задаёт WinRust", "set by WinRust"))).tile(app_tile(&own))
                }
            })
            .collect();
    } else if let Some(e) = clean_ext(&q) {
        let (l, known) = select(&e, tf!("Приложение по умолчанию для файлов {}", "Default app for {} files", e), Op::DefExt, format!("def:{e}"), &Assoc { name: e.clone(), protocol: false });
        head.extra = Extra::Search(if known { t!("1 совпадение", "1 match").to_string() } else { t!("нет совпадений", "no match").to_string() });
        if known {
            head.rows = vec![l];
        } else {
            head = head.empty(tf!("{} пока не связан ни с одним приложением. Windows спросит, чем открыть, при первом открытии.", "{} isn\u{2019}t linked to any app yet. Windows will ask which app to use the first time you open one.", e));
        }
    } else {
        head.extra = Extra::Search(t!("нет совпадений", "no match").to_string());
        head = head.empty(tf!("«{}» не похоже на расширение файла.", "\u{201c}{}\u{201d} doesn\u{2019}t look like a file extension.", q.trim()));
    }
    vec![head]
}
