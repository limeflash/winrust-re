//! The Task Manager's window: what is on the screen and what a click or a key does. It is an [`App`] of `winrust-ui`: it never sees
//! a window, so `--render`, `--bench`, `--selftest` and the tests run it with none, on a [`Machine`] that touches nothing. How it is
//! drawn is `draw.rs`; the numbers it shows, `data.rs`, `model.rs`, `perfview.rs`; what changes the system, `backend.rs`.
use crate::{
    backend::{MSG_HOTKEY, MSG_START, MSG_TRAY_CMD, MSG_TRAY_SHOW, Machine, TrayItem},
    boost,
    data::*,
    demo::Demo,
    model::*,
    perfview::*,
    store, sys,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use winrust_common::style;
use winrust_ui::{
    anim::now_ms,
    core::App,
    frame::{Button, Cx, Frame, Id, Mods},
    gfx::{Canvas, Rect, Rgba},
    widgets::{
        field,
        field_model::{Ev, TextField},
        list::{ColItem, col_of, row_of},
        list_model::{Col as LCol, Columns, ListState},
        overlay::{Dialog, MenuItem},
        scroll,
        scroll_model::Scroll,
    },
};

// ---- ids of the window's items (0 and up; the window's own buttons and the overlays are negative)
pub const ID_NAV: Id = 10;
pub const ID_SEARCH: Id = 30;
pub const ID_ACT: Id = 40;
pub const ID_ALERT_SHOW: Id = 50;
pub const ID_ALERT_END: Id = 51;
pub const ID_ALERT_HIDE: Id = 52;
pub const ID_PHEAD: Id = 60;
pub const ID_PERF: Id = 70;
pub const ID_PROC: Id = 100;
pub const ID_SCROLL_USERS: Id = 110;
pub const ID_SCROLL_SPEED: Id = 120;
pub const ID_SCROLL_SET: Id = 130;
pub const ID_SCROLL_INS: Id = 140;
pub const ID_COLS: Id = 150;
pub const ID_CHART: Id = 160;
pub const ID_UMET: Id = 200;
pub const ID_USER: Id = 210;
pub const ID_UEND: Id = 230;
pub const ID_UBTN: Id = 290;
pub const ID_PROF: Id = 300;
pub const ID_MEM_CLEAR: Id = 310;
pub const ID_MEM_AUTO: Id = 311;
pub const ID_TWEAK: Id = 320;
pub const ID_SETSEG: Id = 400;
pub const ID_SETSW: Id = 410;
pub const ID_SETBTN: Id = 420;
pub const ID_PROW: Id = 10_000;
pub const ID_CHEV: Id = 50_000;
pub const ID_TABLE: Id = 100_000;
/// The switch in row `i` of the startup table is `ID_SWITCH + i`.
pub const ID_SWITCH: Id = 300_000;
/// The column of a table that holds no model cell: the startup table's switches.
pub const SWITCH_COL: usize = 99;
/// A 1 x 1 item in the corner that `--render` clicks to open the menu or the dialog of a state of the design (see `demo`).
pub const ID_DEMO: Id = 9999;

pub const TAG_ROW_MENU: u32 = 1;
pub const TAG_MORE_MENU: u32 = 2;
pub const TAG_END: u32 = 3;
pub const TAG_TRAY: u32 = 4;
pub const TAG_BOOST: u32 = 5;

/// What the pages of the sidebar are, in its order.
pub const NAV_PAGES: [Page; 11] = [Page::Processes, Page::Performance, Page::History, Page::Startup, Page::Users, Page::Details, Page::Services, Page::Boost, Page::Insights, Page::Rules, Page::Settings];

pub const ROW_H: f32 = 40.0;
pub const KID_H: f32 = 36.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UMetric {
    Mem,
    Cpu,
    Disk,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    RunTask,
    EndTask,
    EndTree,
    Efficiency,
    Suspend(bool),
    Prio(u32),
    Affinity(bool),
    OpenDir,
    CopyCmd,
    RuleLower,
    RuleNotify,
    RuleDelete,
    SvcStart,
    SvcStop,
    StartEnable,
    StartDisable,
    Export,
    CheckSlow,
    ClearHistory,
    SetTheme(u8),
    ToggleTray,
    ToggleAutostart,
    ToggleEtw,
    ToggleGpuTemp,
    ShowWindow,
    RelaunchAdmin,
    Exit,
    Toggle,
    Go(Page),
}

/// What a button of the head does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HeadAct {
    Do(Act),
    More,
}

/// What waits for a "yes" in a dialog.
#[derive(Clone, Debug)]
pub enum Ask {
    /// End these processes (a group, or a tree).
    End(Act, Vec<u32>),
    /// Switch a thing of the Speed page on.
    Boost(boost::Id),
}

/// "Check again" of the What's slow page, running: the page waits a moment, as the design's does.
#[derive(Clone, Copy, Debug)]
pub struct SlowCheck {
    pub serial: u32,
    /// The frame clock when it was first drawn.
    pub born: Option<u64>,
}

pub struct Toast {
    pub text: String,
    pub dot: Rgba,
    /// The frame clock when it was first drawn.
    pub born: Option<u64>,
    pub serial: u32,
}

/// How long a toast stays (`tm_toast` of the mockup).
pub const TOAST_MS: u64 = 2200;

/// What the Speed page shows of a thing it can switch.
#[derive(Clone, Debug)]
pub struct SpeedItem {
    pub id: boost::Id,
    pub state: boost::State,
}

pub struct Tm {
    pub m: Arc<dyn Machine>,
    pub d: Data,
    /// The made-up extras of a render, a benchmark or a test (memory numbers, the users): `None` for the real thing.
    pub demo: Option<Demo>,
    pub hwnd: isize,
    /// Started with `--tray`: the window waits in the tray.
    pub hidden_start: bool,
    pub page: Page,
    pub rows: Vec<Row>,
    pub sel: Option<String>,
    pub expanded: HashSet<String>,
    pub sorts: HashMap<Page, (usize, bool)>,
    pub search: TextField,
    pub proc_sc: Scroll,
    pub list: ListState,
    pub cols: Columns,
    pub cols_page: Option<Page>,
    pub perf_sel: PerfKind,
    pub u_open: HashSet<String>,
    pub u_metric: UMetric,
    pub u_sc: Scroll,
    pub speed_sc: Scroll,
    pub tw_open: HashSet<usize>,
    pub set_sc: Scroll,
    pub ins_sc: Scroll,
    /// What the What's slow page says, made again with every tick.
    pub slow: crate::slow::Slow,
    /// The cards of that page the user folded or unfolded (0 startup, 1 apps, 2 drivers); the others are open when they are wrong.
    pub slow_open: HashMap<u8, bool>,
    pub slow_chk: Option<SlowCheck>,
    slow_serial: u32,
    /// "Check again" has finished: the banner says "just now" instead of the time.
    pub slow_just: bool,
    /// When the page was looked at (HH:MM).
    pub slow_at: String,
    pub alert_off: HashSet<String>,
    pub toast: Option<Toast>,
    toast_serial: u32,
    pub ask: Option<Ask>,
    pub menu_acts: Vec<Act>,
    pub quitting: bool,
    pub speed: Vec<SpeedItem>,
    /// The window's size from the last frame (what a page-sized key step needs).
    pub size: (f32, f32),
    /// Where the buttons of the head stood in the last frame (a menu hangs from one).
    pub head_rects: Vec<Rect>,
    /// The window waits in the tray or is minimized: only the numbers are kept, nothing is drawn.
    pub away: bool,
    /// The row the keys moved to last, that the next frame brings into view.
    pub reveal: Option<usize>,
}

/// The clock as the page says when it looked: `17:02`.
fn clock() -> String {
    let (_, _, _, h, m, _) = sys::local_time();
    format!("{h:02}:{m:02}")
}

pub fn nav_label(p: Page) -> &'static str {
    match p {
        Page::Processes => t!("Процессы", "Processes", ja: "プロセス", pt: "Processos", zh: "进程"),
        Page::Performance => t!("Производительность", "Performance", ja: "パフォーマンス", pt: "Desempenho", zh: "性能"),
        Page::History => t!("Журнал приложений", "App history", ja: "アプリの履歴", pt: "Histórico de apps", zh: "应用历史记录"),
        Page::Startup => t!("Автозагрузка", "Startup apps", ja: "スタートアップ アプリ", pt: "Apps de inicialização", zh: "启动应用"),
        Page::Users => t!("Пользователи", "Users", ja: "ユーザー", pt: "Usuários", zh: "用户"),
        Page::Details => t!("Подробности", "Details", ja: "詳細", pt: "Detalhes", zh: "详细信息"),
        Page::Services => t!("Службы", "Services", ja: "サービス", pt: "Serviços", zh: "服务"),
        Page::Boost => t!("Ускорение", "Speed", ja: "速度", pt: "Velocidade", zh: "速度"),
        Page::Rules => t!("Правила", "Rules", ja: "ルール", pt: "Regras", zh: "规则"),
        Page::Settings => t!("Параметры", "Settings", ja: "設定", pt: "Configurações", zh: "设置"),
        Page::Insights => t!("Что тормозит", "What's slow", ja: "遅い原因", pt: "O que está lento", zh: "什么在变慢"),
    }
}

/// The CSV's file name for the page.
fn page_file(p: Page) -> String {
    nav_label(p).chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect()
}

/// The generic tables of the design: which column of the model each shows, its width (0: what the others leave) and its least.
pub fn table_spec(p: Page) -> &'static [(usize, f32, f32)] {
    match p {
        Page::History => &[(0, 96.0, 70.0), (1, 220.0, 90.0), (2, 220.0, 90.0), (3, 110.0, 80.0), (4, 120.0, 80.0), (5, 0.0, 140.0)],
        Page::Startup => &[(0, 301.0, 160.0), (1, 180.0, 100.0), (3, 120.0, 90.0), (4, 0.0, 140.0), (SWITCH_COL, 52.0, 52.0)],
        Page::Details => &[(0, 200.0, 120.0), (1, 72.0, 50.0), (2, 110.0, 80.0), (3, 64.0, 44.0), (4, 96.0, 70.0), (5, 84.0, 60.0), (6, 0.0, 140.0)],
        Page::Rules => &[(0, 290.0, 120.0), (1, 0.0, 140.0), (2, 200.0, 120.0), (3, 140.0, 90.0)],
        _ => &[(0, 160.0, 100.0), (2, 0.0, 140.0), (1, 80.0, 50.0), (3, 120.0, 90.0)],
    }
}

impl Tm {
    #[must_use]
    pub fn new(m: Arc<dyn Machine>, d: Data, demo: Option<Demo>, hidden_start: bool, page: Page) -> Self {
        let mut sorts = HashMap::new();
        sorts.insert(Page::Processes, (d.cfg.sort.min(7), d.cfg.desc));
        let slow = crate::slow::build(&d);
        let mut me = Self {
            m,
            d,
            demo,
            hwnd: 0,
            hidden_start,
            page,
            rows: Vec::new(),
            sel: None,
            expanded: HashSet::new(),
            sorts,
            search: TextField::new(),
            proc_sc: Scroll::default(),
            list: ListState::default(),
            cols: Columns::default(),
            cols_page: None,
            perf_sel: PerfKind::Cpu,
            u_open: HashSet::new(),
            u_metric: UMetric::Mem,
            u_sc: Scroll::default(),
            speed_sc: Scroll::default(),
            tw_open: HashSet::new(),
            set_sc: Scroll::default(),
            ins_sc: Scroll::default(),
            slow,
            slow_open: HashMap::new(),
            slow_chk: None,
            slow_serial: 0,
            slow_just: false,
            slow_at: String::new(),
            alert_off: HashSet::new(),
            toast: None,
            toast_serial: 0,
            ask: None,
            menu_acts: Vec::new(),
            quitting: false,
            speed: Vec::new(),
            size: (1280.0, 780.0),
            head_rects: Vec::new(),
            away: hidden_start,
            reveal: None,
        };
        me.enter_page();
        me
    }

    pub fn sort_of(&self, p: Page) -> (usize, bool) {
        self.sorts.get(&p).copied().unwrap_or_else(|| default_sort(p))
    }

    /// The pages whose body is a table of `rows`, with a search box.
    pub fn page_has_search(&self) -> bool {
        matches!(self.page, Page::Processes | Page::Details | Page::Services | Page::Startup | Page::History | Page::Rules)
    }

    pub fn is_demo(&self) -> bool {
        self.demo.is_some()
    }

    // ---------------------------------------------------------------------------------------------------------------
    // pages and rows

    /// What a page needs when it is entered: its data read, its rows built, its scrolling and selection fresh.
    fn enter_page(&mut self) {
        match self.page {
            Page::Startup => self.m.load_startup(&mut self.d),
            Page::History => self.m.load_history(&mut self.d),
            Page::Insights => {
                self.m.load_startup(&mut self.d);
                (self.slow_just, self.slow_at) = (false, clock());
            }
            _ => {}
        }
        self.sel = None;
        self.proc_sc = Scroll::default();
        self.list = ListState::default();
        self.cols_page = None;
        self.rebuild();
    }

    pub fn go_page(&mut self, p: Page) {
        if self.page == p {
            return;
        }
        self.page = p;
        self.search.set_text("");
        self.enter_page();
        self.save_settings();
    }

    pub fn rebuild(&mut self) {
        self.slow = crate::slow::build(&self.d);
        if self.page == Page::Boost {
            let admin = self.d.is_admin;
            self.speed = boost::ALL.iter().map(|&id| SpeedItem { id, state: self.m.boost_state(id, admin) }).collect();
        }
        if !self.page_has_search() {
            return;
        }
        self.rows = {
            let ctx = BuildCtx { page: self.page, search: &self.search.text, expanded: &self.expanded, sort: self.sort_of(self.page) };
            build_rows(&self.d, &ctx)
        };
        if let Some(dm) = &self.demo {
            dm.post_rows(self.page, &mut self.rows);
        }
        if let Some(k) = self.sel.clone()
            && !self.rows.iter().any(|r| r.key == k)
        {
            self.sel = None;
        }
    }

    pub fn row_index(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.key == key)
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.sel.as_ref().and_then(|k| self.rows.iter().find(|r| &r.key == k))
    }

    fn toggle_expand(&mut self, key: &str) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_string());
        }
        self.rebuild();
    }

    /// The columns of a generic table: the design's widths with the model's titles; one column takes what the rest leave.
    pub fn table_cols(&mut self, p: Page) -> &Columns {
        if self.cols_page != Some(p) {
            let mc = columns(p);
            let spec = table_spec(p);
            let cols: Vec<LCol> = spec.iter().map(|&(mi, w, min)| LCol { title: mc.get(mi).map_or(String::new(), |c| c.title.to_string()), w, min: min.min(mc.get(mi).map_or(min, |c| c.w as f32)), align_right: mc.get(mi).is_some_and(|c| c.align == Align::Right) }).collect();
            let flex = cols.iter().position(|c| c.w == 0.0).unwrap_or(0);
            let mut c = Columns::new(cols);
            c.flex = flex;
            let (ci, desc) = self.sort_of(p);
            c.sort = spec.iter().position(|s| s.0 == ci && ci != SWITCH_COL).map(|k| (k, !desc));
            self.cols = c;
            self.cols_page = Some(p);
        }
        &self.cols
    }

    // ---------------------------------------------------------------------------------------------------------------
    // the clock

    pub fn hidden(&self) -> bool {
        self.m.window_hidden(self.hwnd)
    }

    /// One tick of the numbers (the window's, and the tray's every 5 s).
    pub fn tick(&mut self) {
        self.d.want_gpu_temp = self.page == Page::Performance && matches!(self.perf_sel, PerfKind::Gpu(_));
        let mut notes = self.m.refresh(&mut self.d);
        notes.extend(self.m.boost_done());
        if !notes.is_empty() {
            let msg = notes.join("; ");
            self.say(&msg, crate::look::DOT_OK);
            if self.away {
                self.m.tray_balloon(&msg);
            }
        }
        if self.away {
            return; // the table and the page lists wait until the window is shown
        }
        let ticks = self.d.ticks;
        if matches!(self.page, Page::Startup | Page::Insights) && ticks.is_multiple_of(10) {
            self.m.load_startup(&mut self.d);
        }
        if self.page == Page::History && ticks.is_multiple_of(30) {
            self.m.load_history(&mut self.d);
        }
        self.rebuild();
    }

    /// "Check again": the startup entries are read again and the page waits a moment.
    pub fn slow_check(&mut self) {
        if self.slow_chk.is_some() {
            return;
        }
        self.slow_serial += 1;
        self.slow_chk = Some(SlowCheck { serial: self.slow_serial, born: None });
        self.slow_just = false;
        self.slow_at = clock();
        self.m.load_startup(&mut self.d);
        self.rebuild();
    }

    pub fn say(&mut self, text: &str, dot: Rgba) {
        self.toast_serial += 1;
        self.toast = Some(Toast { text: text.to_string(), dot, born: None, serial: self.toast_serial });
    }

    // ---------------------------------------------------------------------------------------------------------------
    // settings, theme, the tray

    pub fn save_settings(&mut self) {
        let d = &mut self.d;
        d.cfg.view = NAV.iter().position(|n| n.0 == self.page).unwrap_or(0);
        let (sc, sd) = self.sorts.get(&Page::Processes).copied().unwrap_or((2, true));
        d.cfg.sort = sc;
        d.cfg.desc = sd;
        d.cfg.rules = d.rules.clone();
        d.cfg.etw = d.etw.is_some();
        self.m.save_settings(&d.cfg);
    }

    /// The colours the user chose: Light and Dark are theirs, "System" is Windows' (the host follows it).
    pub fn apply_theme(&self) {
        match self.d.cfg.theme {
            0 => {
                style::set_light(true);
            }
            1 => {
                style::set_light(false);
            }
            _ => {}
        }
    }

    pub fn tray_items(&self) -> Vec<TrayItem> {
        let d = &self.d;
        let mut v = vec![
            TrayItem::new(t!("Показать окно", "Show window", ja: "ウィンドウを表示", pt: "Mostrar janela", zh: "显示窗口"), 1, false),
            TrayItem::sep(),
            TrayItem::new(t!("Закрытие окна сворачивает в трей", "Closing the window minimizes to tray", ja: "ウィンドウを閉じるとトレイに最小化", pt: "Fechar a janela minimiza para a bandeja", zh: "关闭窗口时最小化到托盘"), 2, d.cfg.to_tray),
            TrayItem::new(t!("Запускать при входе в Windows", "Start with Windows", ja: "Windows と同時に起動", pt: "Iniciar com o Windows", zh: "随 Windows 启动"), 3, self.m.self_autostart()),
            TrayItem::new(t!("Сеть по процессам (ETW, администратор)", "Per-process network (ETW, administrator)", ja: "プロセスごとのネットワーク (ETW、管理者)", pt: "Rede por processo (ETW, administrador)", zh: "按进程统计网络（ETW，管理员）"), 4, d.etw.is_some()),
            TrayItem::sep(),
        ];
        if !d.is_admin {
            v.push(TrayItem::new(t!("Перезапустить от имени администратора", "Restart as administrator", ja: "管理者として再起動", pt: "Reiniciar como administrador", zh: "以管理员身份重启"), 5, false));
        }
        v.push(TrayItem::new(t!("Выход", "Exit", ja: "終了", pt: "Sair", zh: "退出"), 6, false));
        v
    }

    fn refresh_tray_menu(&self) {
        self.m.tray_menu(self.tray_items());
    }

    /// The window comes back from the tray (the icon, the hotkey).
    fn show_window(&mut self, cx: &mut Cx) {
        self.m.show_window(self.hwnd);
        self.away = false;
        self.tick();
        cx.invalidate();
        cx.wake_in(1000);
    }

    /// Hides the window and keeps running: history and rules go on.
    fn hide_to_tray(&mut self) {
        self.save_settings();
        self.away = true;
        self.m.hide_window(self.hwnd);
    }

    // ---------------------------------------------------------------------------------------------------------------
    // menus

    pub fn row_menu(&mut self) -> Vec<MenuItem> {
        let acts = std::cell::RefCell::new(Vec::new());
        let it = |label: &str, icon: &'static str, a: Act, kbd: &str, danger: bool| -> MenuItem {
            acts.borrow_mut().push(a);
            let mut m = MenuItem::new(label).icon(icon).kbd(kbd);
            if danger {
                m = m.danger();
            }
            m
        };
        let items = match self.page {
            Page::Processes | Page::Users | Page::Details => {
                let (expandable, expanded, pids) = self.selected_row().map(|r| (r.expandable, r.expanded, r.pids.clone())).unwrap_or_default();
                let mut v = Vec::new();
                if expandable {
                    v.push(it(if expanded { t!("Свернуть", "Collapse", ja: "折りたたむ", pt: "Recolher", zh: "折叠") } else { t!("Развернуть", "Expand", ja: "展開", pt: "Expandir", zh: "展开") }, crate::look::I_EXPAND, Act::Toggle, "", false));
                    v.push(MenuItem::sep());
                }
                let eco = !pids.is_empty() && pids.iter().all(|p| self.d.efficiency.contains(p));
                let susp = !pids.is_empty() && pids.iter().all(|p| self.d.suspended.contains(p));
                v.push(it(t!("Снять задачу", "End task", ja: "タスクの終了", pt: "Finalizar tarefa", zh: "结束任务"), crate::look::I_END, Act::EndTask, "Del", true));
                v.push(it(t!("Завершить дерево процессов", "End process tree", ja: "プロセス ツリーの終了", pt: "Finalizar árvore de processos", zh: "结束进程树"), crate::look::I_TREE, Act::EndTree, "", true));
                v.push(it(if eco { t!("Выключить режим эффективности", "Turn off efficiency mode", ja: "効率モードをオフにする", pt: "Desligar o modo de eficiência", zh: "关闭能效模式") } else { t!("Режим эффективности", "Efficiency mode", ja: "効率モード", pt: "Modo de eficiência", zh: "能效模式") }, crate::look::I_LEAF, Act::Efficiency, "", false));
                v.push(if susp { it(t!("Возобновить", "Resume", ja: "再開", pt: "Retomar", zh: "继续"), crate::look::I_START, Act::Suspend(false), "", false) } else { it(t!("Приостановить", "Suspend", ja: "中断", pt: "Suspender", zh: "挂起"), crate::look::I_PAUSE, Act::Suspend(true), "", false) });
                v.push(MenuItem::sep());
                let prio = pids.first().map(|&p| self.m.priority_name(p)).unwrap_or_default();
                let mut sub = Vec::new();
                for p in sys::PRIORITIES.iter().rev() {
                    acts.borrow_mut().push(Act::Prio(p.1));
                    sub.push(MenuItem::new(p.0).radio(p.0 == prio));
                }
                v.push(MenuItem::new(t!("Приоритет", "Priority", ja: "優先度", pt: "Prioridade", zh: "优先级")).icon(crate::look::I_PRIO).submenu(sub));
                acts.borrow_mut().push(Act::Affinity(false));
                acts.borrow_mut().push(Act::Affinity(true));
                let sub = vec![MenuItem::new(t!("Все ядра", "All cores", ja: "すべてのコア", pt: "Todos os núcleos", zh: "所有核心")).radio(true), MenuItem::new(t!("Только половина ядер", "Half of the cores only", ja: "コアの半分のみ", pt: "Apenas metade dos núcleos", zh: "仅一半核心")).radio(false)];
                v.push(MenuItem::new(t!("Ядра процессора", "CPU cores", ja: "CPU コア", pt: "Núcleos da CPU", zh: "CPU 核心")).icon(crate::look::I_CORE).submenu(sub));
                v.push(MenuItem::sep());
                v.push(it(t!("Открыть расположение файла", "Open file location", ja: "ファイルの場所を開く", pt: "Abrir local do arquivo", zh: "打开文件所在位置"), crate::look::I_FOLDER, Act::OpenDir, "", false));
                v.push(it(t!("Копировать командную строку", "Copy command line", ja: "コマンド ラインをコピー", pt: "Copiar linha de comando", zh: "复制命令行"), crate::look::I_COPY, Act::CopyCmd, "", false));
                v.push(MenuItem::sep());
                acts.borrow_mut().push(Act::RuleLower);
                acts.borrow_mut().push(Act::RuleNotify);
                let sub = vec![
                    MenuItem::new(t!("Понижать приоритет при ЦП > 25% дольше 60 с", "Lower priority if CPU > 25% for 60 s", ja: "CPU > 25% が 60 秒続いたら優先度を下げる", pt: "Reduzir a prioridade se CPU > 25% por 60 s", zh: "CPU > 25% 持续 60 秒则降低优先级")),
                    MenuItem::new(t!("Уведомлять при ЦП > 25% дольше 60 с", "Notify if CPU > 25% for 60 s", ja: "CPU > 25% が 60 秒続いたら通知", pt: "Avisar se CPU > 25% por 60 s", zh: "CPU > 25% 持续 60 秒则通知")),
                ];
                v.push(MenuItem::new(t!("Добавить правило", "Add rule", ja: "ルールを追加", pt: "Adicionar regra", zh: "添加规则")).icon(crate::look::I_RULE).submenu(sub));
                v
            }
            Page::Services => vec![it(t!("Запустить службу", "Start service", ja: "サービスを開始", pt: "Iniciar serviço", zh: "启动服务"), crate::look::I_START, Act::SvcStart, "", false), it(t!("Остановить службу", "Stop service", ja: "サービスを停止", pt: "Parar serviço", zh: "停止服务"), crate::look::I_STOP, Act::SvcStop, "", true)],
            Page::Startup => vec![
                it(t!("Включить", "Enable", ja: "有効にする", pt: "Ativar", zh: "启用"), crate::look::I_CHECK, Act::StartEnable, "", false),
                it(t!("Отключить", "Disable", ja: "無効にする", pt: "Desativar", zh: "禁用"), crate::look::I_END, Act::StartDisable, "", false),
                MenuItem::sep(),
                it(t!("Открыть расположение файла", "Open file location", ja: "ファイルの場所を開く", pt: "Abrir local do arquivo", zh: "打开文件所在位置"), crate::look::I_FOLDER, Act::OpenDir, "", false),
            ],
            Page::Rules => vec![it(t!("Удалить правило", "Delete rule", ja: "ルールを削除", pt: "Excluir regra", zh: "删除规则"), crate::look::I_TRASH, Act::RuleDelete, "", true)],
            _ => vec![],
        };
        self.menu_acts = acts.into_inner();
        items
    }

    pub fn more_menu(&mut self) -> Vec<MenuItem> {
        let mut acts = Vec::new();
        let mut it = |label: &str, icon: &'static str, a: Act| -> MenuItem {
            acts.push(a);
            MenuItem::new(label).icon(icon)
        };
        let theme = self.d.cfg.theme;
        let mut v = vec![
            it(t!("Запустить новую задачу", "Run new task", ja: "新しいタスクの実行", pt: "Executar nova tarefa", zh: "运行新任务"), crate::look::I_PLUS, Act::RunTask),
            it(t!("Экспорт в CSV", "Export to CSV", ja: "CSV にエクスポート", pt: "Exportar para CSV", zh: "导出为 CSV"), crate::look::I_CSV, Act::Export),
            it(t!("Что тормозит", "What's slow", ja: "遅い原因", pt: "O que está lento", zh: "什么在变慢"), crate::look::NAV_SLOW, Act::Go(Page::Insights)),
            MenuItem::sep(),
        ];
        acts.extend([Act::SetTheme(0), Act::SetTheme(1), Act::SetTheme(2)]);
        let sub = vec![
            MenuItem::new(t!("Светлая", "Light", ja: "ライト", pt: "Claro", zh: "浅色")).radio(theme == 0),
            MenuItem::new(t!("Тёмная", "Dark", ja: "ダーク", pt: "Escuro", zh: "深色")).radio(theme == 1),
            MenuItem::new(t!("Системная", "System", ja: "システム", pt: "Sistema", zh: "系统")).radio(theme == 2),
        ];
        v.push(MenuItem::new(t!("Тема", "Theme", ja: "テーマ", pt: "Tema", zh: "主题")).icon(crate::look::I_THEME).submenu(sub));
        acts.push(Act::Go(Page::Settings));
        v.push(MenuItem::new(t!("Параметры", "Settings", ja: "設定", pt: "Configurações", zh: "设置")).icon(crate::look::NAV_SET));
        if !self.d.is_admin {
            acts.push(Act::RelaunchAdmin);
            v.push(MenuItem::sep());
            v.push(MenuItem::new(t!("Перезапустить от имени администратора", "Restart as administrator", ja: "管理者として再起動", pt: "Reiniciar como administrador", zh: "以管理员身份重启")).icon(crate::look::I_SHIELD));
        }
        self.menu_acts = acts;
        v
    }

    // ---------------------------------------------------------------------------------------------------------------
    // actions (what the old window did, with its toasts and its questions)

    /// A menu, a button or the tray asked for `act`.
    pub fn do_act(&mut self, act: Act, cx: &mut Cx) {
        match act {
            Act::ShowWindow => return self.show_window(cx),
            Act::Exit => {
                self.quitting = true;
                return cx.close_window();
            }
            Act::RelaunchAdmin => {
                self.save_settings(); // the new instance reads them at start: same page, same columns
                if self.m.relaunch_admin() {
                    self.quitting = true;
                    return cx.close_window();
                }
                return self.say(t!("Запуск от имени администратора отменён", "Running as administrator was cancelled", ja: "管理者としての実行がキャンセルされました", pt: "A execução como administrador foi cancelada", zh: "已取消以管理员身份运行"), crate::look::DOT_WARN);
            }
            Act::Go(p) => return self.go_page(p),
            Act::RunTask => return self.m.run_dialog(),
            Act::SetTheme(th) => {
                self.d.cfg.theme = th;
                if th == 2 {
                    style::set_light(!self.m.system_dark());
                }
                self.apply_theme();
                return self.save_settings();
            }
            Act::ToggleTray => {
                let on = {
                    let d = &mut self.d;
                    d.cfg.to_tray = !d.cfg.to_tray;
                    d.cfg.tray_asked = true;
                    d.cfg.to_tray
                };
                self.say(if on { t!("Закрытие окна будет сворачивать в трей", "Closing the window will minimize to tray", ja: "ウィンドウを閉じるとトレイに最小化されます", pt: "Fechar a janela vai minimizar para a bandeja", zh: "关闭窗口将最小化到托盘") } else { t!("Закрытие окна завершит программу", "Closing the window will quit the program", ja: "ウィンドウを閉じるとプログラムが終了します", pt: "Fechar a janela vai encerrar o programa", zh: "关闭窗口将退出程序") }, crate::look::DOT_OK);
                self.refresh_tray_menu();
                return self.save_settings();
            }
            Act::ToggleAutostart => {
                let want = !self.m.self_autostart();
                let ok = self.m.set_self_autostart(want);
                let (text, dot) = match (ok, self.m.self_autostart()) {
                    (true, true) => (t!("Программа будет запускаться при входе в Windows (сразу в трей)", "The program will start with Windows (straight to the tray)", ja: "プログラムは Windows と同時に起動します (トレイに直接)", pt: "O programa vai iniciar com o Windows (direto na bandeja)", zh: "程序将随 Windows 启动（直接进托盘）"), crate::look::DOT_OK),
                    (true, false) => (t!("Автозапуск программы отключён", "Autostart is disabled", ja: "自動起動は無効です", pt: "A inicialização automática está desativada", zh: "已禁用自动启动"), crate::look::DOT_OK),
                    _ => (t!("Не удалось изменить автозапуск", "Could not change autostart", ja: "自動起動を変更できませんでした", pt: "Não foi possível alterar a inicialização automática", zh: "无法更改自动启动"), crate::look::DOT_WARN),
                };
                self.say(text, dot);
                self.refresh_tray_menu();
                return;
            }
            Act::ToggleEtw => {
                let (msg, dot) = {
                    if !self.d.is_admin {
                        (t!("Сеть по процессам требует прав администратора — запустите программу от имени администратора", "Per-process network needs administrator rights - run the program as administrator", ja: "プロセスごとのネットワークには管理者権限が必要です。管理者として実行してください", pt: "A rede por processo exige direitos de administrador - execute o programa como administrador", zh: "按进程统计网络需要管理员权限——请以管理员身份运行程序").to_string(), crate::look::DOT_WARN)
                    } else if let Some(s) = self.d.etw.take() {
                        self.m.etw_stop(s);
                        self.d.net.clear();
                        self.d.cfg.etw = false;
                        (t!("Сеть по процессам выключена", "Per-process network is off", ja: "プロセスごとのネットワークはオフです", pt: "A rede por processo está desligada", zh: "按进程统计网络已关闭").to_string(), crate::look::DOT_OK)
                    } else {
                        self.d.etw = self.m.etw_start();
                        self.d.cfg.etw = self.d.etw.is_some();
                        if self.d.etw.is_some() { (t!("Сеть по процессам включена (колонка «Сеть»)", "Per-process network is on ('Network' column)", ja: "プロセスごとのネットワークはオンです (「ネットワーク」列)", pt: "A rede por processo está ligada (coluna 'Rede')", zh: "按进程统计网络已开启（「网络」列）").to_string(), crate::look::DOT_OK) } else { (t!("Не удалось запустить ETW", "Could not start ETW", ja: "ETW を開始できませんでした", pt: "Não foi possível iniciar o ETW", zh: "无法启动 ETW").to_string(), crate::look::DOT_WARN) }
                    }
                };
                self.say(&msg, dot);
                self.refresh_tray_menu();
                return self.save_settings();
            }
            Act::ToggleGpuTemp => {
                self.d.cfg.gpu_temp_always = !self.d.cfg.gpu_temp_always;
                let on = self.d.cfg.gpu_temp_always;
                self.say(if on { t!("Температура GPU читается постоянно", "GPU temperature is read continuously", ja: "GPU 温度を常時読み取ります", pt: "A temperatura da GPU é lida continuamente", zh: "正在持续读取 GPU 温度") } else { t!("Температура GPU читается только на странице GPU", "GPU temperature is read only on the GPU page", ja: "GPU 温度は GPU ページでのみ読み取ります", pt: "A temperatura da GPU é lida só na página da GPU", zh: "仅在 GPU 页面读取 GPU 温度") }, crate::look::DOT_OK);
                return self.save_settings();
            }
            Act::Export => return self.export(),
            Act::CheckSlow => return self.slow_check(),
            Act::ClearHistory => {
                self.m.clear_history();
                self.d.hist.clear();
                self.rebuild();
                return self.say(t!("История использования очищена", "Usage history cleared", ja: "使用履歴を消去しました", pt: "Histórico de uso apagado", zh: "已清除使用历史记录"), crate::look::DOT_OK);
            }
            _ => {}
        }
        let Some(row) = self.selected_row().cloned() else {
            return self.say(t!("Сначала выберите строку в списке", "Select a row in the list first", ja: "先にリストで行を選択してください", pt: "Selecione primeiro uma linha na lista", zh: "请先在列表中选择一行"), crate::look::DOT_WARN);
        };
        if act == Act::Toggle {
            return self.toggle_expand(&row.key);
        }
        match self.page {
            Page::Services => return self.act_service(&row, act),
            Page::Startup => return self.act_startup(&row, act),
            Page::Rules => {
                if act == Act::RuleDelete {
                    if let Ok(i) = row.key.parse::<usize>()
                        && i < self.d.rules.len()
                    {
                        self.d.rules.remove(i);
                    }
                    self.save_settings();
                    self.say(t!("Правило удалено", "Rule deleted", ja: "ルールを削除しました", pt: "Regra excluída", zh: "已删除规则"), crate::look::DOT_BAD);
                    self.rebuild();
                }
                return;
            }
            _ => {}
        }
        self.act_process(&row, act, cx);
    }

    fn act_process(&mut self, row: &Row, act: Act, cx: &mut Cx) {
        let mut pids: Vec<u32> = row.pids.iter().copied().filter(|&p| p != 0).collect();
        if pids.is_empty() {
            return;
        }
        if matches!(act, Act::EndTask | Act::EndTree | Act::Suspend(true)) {
            let total = pids.len();
            pids.retain(|&p| self.d.procs.iter().find(|x| x.pid == p).is_none_or(|x| !CRITICAL.contains(&x.name.to_lowercase().as_str())));
            if pids.len() < total {
                self.say(t!("Критичные системные процессы (csrss, lsass, smss и т.п.) защищены и пропущены", "Critical system processes (csrss, lsass, smss, etc.) are protected and skipped", ja: "重要なシステム プロセス (csrss、lsass、smss など) は保護されているためスキップされます", pt: "Processos críticos do sistema (csrss, lsass, smss etc.) são protegidos e ignorados", zh: "关键系统进程（csrss、lsass、smss 等）受保护，将被跳过"), crate::look::DOT_WARN);
                if pids.is_empty() {
                    return;
                }
            }
        }
        if act == Act::EndTree || (act == Act::EndTask && pids.len() > 1) {
            // what cannot be undone is asked first: nothing is ended until the dialog's red button is pressed
            let first = row.cells.first().cloned().unwrap_or_default();
            let name = first.rsplit_once(" (").map_or(first.clone(), |(n, _)| n.to_string());
            let n = pids.len();
            let (title, text, ok) = if act == Act::EndTree {
                (tf!("Завершить «{name}» и всё, что оно запустило?", "End {name} and everything it started?", ja: "{name} と、それが起動したものすべてを終了しますか?", pt: "Finalizar {name} e tudo o que ele iniciou?", zh: "结束 {name} 及其启动的所有进程？"), tf!("Дочерние процессы закроются вместе с «{name}». Несохранённая работа в них будет потеряна.", "Child processes close together with {name}. Unsaved work in them will be lost.", ja: "子プロセスも {name} と一緒に閉じられます。保存していない作業は失われます。", pt: "Os processos filhos fecham junto com {name}. O trabalho não salvo neles será perdido.", zh: "子进程将与 {name} 一起关闭。其中未保存的工作将丢失。"), t!("Завершить дерево", "End process tree", ja: "プロセス ツリーを終了", pt: "Finalizar árvore", zh: "结束进程树").to_string())
            } else {
                (tf!("Завершить все процессы «{name}» ({n})?", "End all {n} {name} processes?", ja: "{name} の {n} 個のプロセスをすべて終了しますか?", pt: "Finalizar todos os {n} processos de {name}?", zh: "结束 {name} 的全部 {n} 个进程？"), tf!("Вся группа закроется сразу. Несохранённая работа в «{name}» будет потеряна.", "The whole group closes at once. Unsaved work in {name} will be lost.", ja: "グループ全体が一度に閉じられます。{name} の保存していない作業は失われます。", pt: "O grupo inteiro fecha de uma vez. O trabalho não salvo em {name} será perdido.", zh: "整个分组将一次性关闭。{name} 中未保存的工作将丢失。"), tf!("Завершить ({n})", "End {n} processes", ja: "{n} 個を終了", pt: "Finalizar {n}", zh: "结束 {n} 个"))
            };
            self.ask = Some(Ask::End(act, pids));
            cx.dialog(TAG_END, Dialog::new(title, text, &[t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消"), &ok]).icon(crate::look::I_TREE, true));
            return;
        }
        self.run_process_act(act, &pids, cx);
    }

    /// Does `act` to `pids` (the questions are asked already) and says how it went.
    pub fn run_process_act(&mut self, act: Act, pids: &[u32], cx: &mut Cx) {
        let n = pids.len();
        let report = |me: &mut Self, what: &str, ok: usize| {
            let text = if ok == n { tf!("{what}: готово ({ok})", "{what}: done ({ok})", ja: "{what}: 完了 ({ok})", pt: "{what}: concluído ({ok})", zh: "{what}：完成（{ok}）") } else { tf!("{what}: удалось {ok} из {} — для остальных нужны права администратора{}", "{what}: {ok} of {} succeeded - the rest need administrator rights{}", ja: "{what}: {} 件中 {ok} 件が成功しました。残りは管理者権限が必要です{}", pt: "{what}: {ok} de {} concluídos - o restante exige direitos de administrador{}", zh: "{what}：{ok} 个成功，共 {} 个——其余需要管理员权限{}", n, if me.d.is_admin { "" } else { t!(" (меню «…» → «Перезапустить от имени администратора»)", " (menu '…' → 'Restart as administrator')", ja: " (メニュー「…」→「管理者として再起動」)", pt: " (menu '…' → 'Reiniciar como administrador')", zh: " （菜单「…」→「以管理员身份重启」）") }) };
            me.say(&text, if ok == n { crate::look::DOT_OK } else { crate::look::DOT_WARN });
        };
        match act {
            Act::EndTask => {
                let ok = pids.iter().filter(|&&p| self.m.kill(&self.d, p)).count();
                self.sel = None;
                report(self, t!("Завершение", "Ending", ja: "終了中", pt: "Finalizando", zh: "正在结束"), ok);
            }
            Act::EndTree => {
                let parents: HashMap<u32, u32> = self.d.procs.iter().map(|p| (p.pid, p.ppid)).collect();
                let all = descendants(&parents, pids);
                let ok = all.iter().filter(|&&p| self.m.kill(&self.d, p)).count();
                self.sel = None;
                let text = tf!("Завершено {ok} из {} (с потомками)", "Ended {ok} of {} (with children)", ja: "{} 件中 {ok} 件を終了しました (子プロセス含む)", pt: "Finalizados {ok} de {} (com os filhos)", zh: "已结束 {ok} 个，共 {} 个（含子进程）", all.len());
                self.say(&text, crate::look::DOT_BAD);
            }
            Act::Prio(c) => {
                let ok = pids.iter().filter(|&&p| self.m.set_priority(p, c)).count();
                report(self, t!("Приоритет", "Priority", ja: "優先度", pt: "Prioridade", zh: "优先级"), ok);
            }
            Act::Suspend(on) => {
                let ok: Vec<u32> = pids.iter().copied().filter(|&p| self.m.suspend(p, on)).collect();
                for p in &ok {
                    if on { self.d.suspended.insert(*p) } else { self.d.suspended.remove(p) };
                }
                report(self, if on { t!("Приостановка", "Suspending", ja: "中断中", pt: "Suspendendo", zh: "正在挂起") } else { t!("Возобновление", "Resuming", ja: "再開中", pt: "Retomando", zh: "正在继续") }, ok.len());
            }
            Act::Efficiency => {
                let on = !pids.iter().all(|p| self.d.efficiency.contains(p));
                let ok: Vec<u32> = pids.iter().copied().filter(|&p| self.m.set_efficiency(p, on)).collect();
                for p in &ok {
                    if on { self.d.efficiency.insert(*p) } else { self.d.efficiency.remove(p) };
                }
                report(self, if on { t!("Режим эффективности включён", "Efficiency mode on", ja: "効率モードをオンにしました", pt: "Modo de eficiência ligado", zh: "能效模式已开启") } else { t!("Режим эффективности выключен", "Efficiency mode off", ja: "効率モードをオフにしました", pt: "Modo de eficiência desligado", zh: "能效模式已关闭") }, ok.len());
            }
            Act::Affinity(half) => {
                let ok = pids.iter().filter(|&&p| self.m.set_affinity(p, half)).count();
                report(self, t!("Привязка к ядрам", "Core affinity", ja: "コア アフィニティ", pt: "Afinidade de núcleos", zh: "CPU 相关性"), ok);
            }
            Act::OpenDir => {
                let path = pids.iter().find_map(|&p| self.d.procs.iter().find(|x| x.pid == p).and_then(|x| x.exe.clone()));
                match path {
                    Some(p) => self.m.reveal(&p.replace("\\..\\", "\\")),
                    None => self.say(t!("Путь к файлу недоступен (системный или защищённый процесс)", "File path unavailable (system or protected process)", ja: "ファイルのパスを取得できません (システムまたは保護されたプロセス)", pt: "Caminho do arquivo indisponível (processo do sistema ou protegido)", zh: "无法获取文件路径（系统进程或受保护进程）"), crate::look::DOT_WARN),
                }
            }
            Act::CopyCmd => {
                let cmd = process_cmd(&self.d, pids[0]);
                if cmd.is_empty() {
                    self.say(t!("Командная строка недоступна для этого процесса", "Command line unavailable for this process", ja: "このプロセスのコマンド ラインを取得できません", pt: "Linha de comando indisponível para este processo", zh: "此进程无法获取命令行"), crate::look::DOT_WARN);
                } else {
                    cx.set_clipboard_text(cmd);
                    self.say(t!("Командная строка скопирована", "Command line copied", ja: "コマンド ラインをコピーしました", pt: "Linha de comando copiada", zh: "已复制命令行"), crate::look::DOT_OK);
                }
            }
            Act::RuleLower | Act::RuleNotify => {
                let name = self.d.procs.iter().find(|p| p.pid == pids[0]).map(|p| p.name.to_lowercase());
                let Some(name) = name else { return };
                let act = if act == Act::RuleLower { store::RuleAct::Lower } else { store::RuleAct::Notify };
                let (msg, dot) = if self.d.rules.iter().any(|r| r.proc == name && r.act == act) {
                    (t!("Такое правило уже есть (страница «Правила»)", "This rule already exists ('Rules' page)", ja: "このルールはすでに存在します (「ルール」ページ)", pt: "Esta regra já existe (página 'Regras')", zh: "该规则已存在（「规则」页面）").to_string(), crate::look::DOT_WARN)
                } else {
                    self.d.rules.push(store::Rule { proc: name.clone(), cpu: 25.0, secs: 60, act, hits: 0 });
                    (tf!("Правило добавлено: «{name}» — ЦП > 25% дольше 60 с", "Rule added: '{name}' - CPU > 25% for over 60 s", ja: "ルールを追加しました: '{name}' - CPU が 60 秒を超えて 25% 超", pt: "Regra adicionada: '{name}' - CPU > 25% por mais de 60 s", zh: "已添加规则：「{name}」——CPU 超过 25% 持续 60 秒以上"), crate::look::DOT_OK)
                };
                self.save_settings();
                self.say(&msg, dot);
            }
            _ => {}
        }
    }

    fn act_service(&mut self, row: &Row, act: Act) {
        let start = match act {
            Act::SvcStart => true,
            Act::SvcStop => false,
            _ => return,
        };
        let n = row.aux.clone();
        let (text, dot) = match self.m.service_ctl(&n, start) {
            Ok(()) => (tf!("Служба {n}: команда отправлена", "Service {n}: command sent", ja: "サービス {n}: コマンドを送信しました", pt: "Serviço {n}: comando enviado", zh: "服务 {n}：命令已发送"), if start { crate::look::DOT_OK } else { crate::look::DOT_BAD }),
            Err(5) => (tf!("Служба {n}: отказано в доступе — запустите от администратора", "Service {n}: access denied - run as administrator", ja: "サービス {n}: アクセスが拒否されました。管理者として実行してください", pt: "Serviço {n}: acesso negado - execute como administrador", zh: "服务 {n}：拒绝访问——请以管理员身份运行"), crate::look::DOT_WARN),
            Err(e) => (tf!("Служба {n}: ошибка Win32 {e}", "Service {n}: Win32 error {e}", ja: "サービス {n}: Win32 エラー {e}", pt: "Serviço {n}: erro Win32 {e}", zh: "服务 {n}：Win32 错误 {e}"), crate::look::DOT_WARN),
        };
        self.say(&text, dot);
    }

    fn act_startup(&mut self, row: &Row, act: Act) {
        let on = match act {
            Act::StartEnable => true,
            Act::StartDisable => false,
            Act::OpenDir => {
                if let Some(p) = &row.icon_path {
                    self.m.reveal(p);
                }
                return;
            }
            _ => return,
        };
        let (msg, dot) = match self.d.startup.iter().find(|s| s.key() == row.key) {
            Some(it) => match self.m.startup_set(it, on) {
                Ok(()) => (format!("«{}»: {}", it.name, if on { t!("включено", "enabled", ja: "有効", pt: "ativado", zh: "已启用") } else { t!("отключено — не будет запускаться при входе в Windows", "disabled - will not start with Windows", ja: "無効 - Windows と同時に起動しません", pt: "desativado - não vai iniciar com o Windows", zh: "已禁用——不会随 Windows 启动") }), crate::look::DOT_OK),
                Err(5) => (tf!("«{}»: нужны права администратора", "'{}': administrator rights needed", ja: "'{}': 管理者権限が必要です", pt: "'{}': direitos de administrador necessários", zh: "「{}」：需要管理员权限", it.name), crate::look::DOT_WARN),
                Err(e) => (tf!("«{}»: ошибка Win32 {e}", "'{}': Win32 error {e}", ja: "'{}': Win32 エラー {e}", pt: "'{}': erro Win32 {e}", zh: "「{}」：Win32 错误 {e}", it.name), crate::look::DOT_WARN),
            },
            None => return,
        };
        self.m.load_startup(&mut self.d);
        self.rebuild();
        self.say(&msg, dot);
    }

    /// Switches a thing of the Speed page (a profile, the cache, a tweak); the Defender one asks first.
    pub fn boost_set(&mut self, id: boost::Id, on: bool, cx: &mut Cx) {
        if on && id == boost::Id::Defender {
            self.ask = Some(Ask::Boost(id));
            cx.dialog(
                TAG_BOOST,
                Dialog::new(
                    t!("Исключения Defender", "Defender exclusions", ja: "Defender の除外", pt: "Exclusões do Defender", zh: "Defender 排除项"),
                    t!("Defender перестанет проверять кэши сборки и библиотеки игр (cargo, npm, pip, Steam, Epic…). Вредоносный файл в этих папках не будет найден при открытии. Включить?", "Defender will stop scanning build caches and game libraries (cargo, npm, pip, Steam, Epic…). A malicious file in those folders will not be caught when opened. Enable?", ja: "Defender はビルド キャッシュとゲーム ライブラリ (cargo、npm、pip、Steam、Epic…) のスキャンを停止します。これらのフォルダー内の悪意のあるファイルは、開いても検出されません。有効にしますか?", pt: "O Defender vai deixar de verificar caches de compilação e bibliotecas de jogos (cargo, npm, pip, Steam, Epic…). Um arquivo malicioso nessas pastas não será detectado ao ser aberto. Ativar?", zh: "Defender 将不再扫描编译缓存和游戏库（cargo、npm、pip、Steam、Epic…）。这些文件夹中的恶意文件在被打开时不会被拦截。要启用吗？"),
                    &[t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消"), t!("Включить", "Enable", ja: "有効にする", pt: "Ativar", zh: "启用")],
                )
                .icon(crate::look::I_SHIELD, true),
            );
            return;
        }
        self.run_boost(id, on);
    }

    fn run_boost(&mut self, id: boost::Id, on: bool) {
        // the donor of the token for programs the profile starts back: sihost.exe runs in every session, not elevated
        let donors: Vec<u32> = self.d.procs.iter().filter(|p| p.name.eq_ignore_ascii_case("sihost.exe")).map(|p| p.pid).collect();
        match self.m.boost_start(id, on, donors) {
            Ok(()) => self.say(&tf!("{}: выполняется…", "{}: running…", ja: "{}: 実行中…", pt: "{}: executando…", zh: "{}：正在运行…", id.title()), crate::look::DOT_OK),
            Err(e) => self.say(&format!("{}: {e}", id.title()), crate::look::DOT_WARN),
        }
        self.rebuild();
    }

    pub fn boost_on(&self, id: boost::Id) -> bool {
        self.speed.iter().find(|s| s.id == id).is_some_and(|s| s.state == boost::State::On)
    }

    fn export(&mut self) {
        if !self.page_has_search() {
            return self.say(t!("На этой странице нечего экспортировать", "Nothing to export on this page", ja: "このページにはエクスポートするものがありません", pt: "Nada para exportar nesta página", zh: "此页面没有可导出的内容"), crate::look::DOT_WARN);
        }
        let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = String::from("\u{feff}");
        out += &columns(self.page).iter().map(|c| esc(c.title)).collect::<Vec<_>>().join(";");
        for r in &self.rows {
            out += "\r\n";
            out += &r.cells.iter().map(|c| esc(c)).collect::<Vec<_>>().join(";");
        }
        let (y, mo, d, h, mi, s) = sys::local_time();
        let file = format!("rtaskmgr-{}-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}.csv", page_file(self.page));
        match self.m.export(&file, &out) {
            Ok(path) => self.say(&tf!("Сохранено: {}", "Saved: {}", ja: "保存しました: {}", pt: "Salvo: {}", zh: "已保存：{}", path), crate::look::DOT_OK),
            Err(e) => self.say(&tf!("Не удалось сохранить CSV: {e}", "Could not save CSV: {e}", ja: "CSV を保存できませんでした: {e}", pt: "Não foi possível salvar o CSV: {e}", zh: "无法保存 CSV：{e}"), crate::look::DOT_WARN),
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // selection and keys of the tables

    /// Selects row `i` and asks for it to be brought into view.
    pub fn select_at(&mut self, i: usize) {
        if let Some(r) = self.rows.get(i) {
            self.sel = Some(r.key.clone());
            self.list.cursor = Some(i);
            self.reveal = Some(i);
        }
    }

    fn move_sel(&mut self, vk: u32) -> bool {
        let n = self.rows.len();
        if n == 0 || !self.page_has_search() {
            return false;
        }
        let cur = self.sel.as_deref().and_then(|k| self.row_index(k));
        let step = (((self.size.1 - 200.0) / ROW_H) as usize).max(1);
        let to = match vk {
            0x28 => cur.map_or(0, |c| (c + 1).min(n - 1)),
            0x26 => cur.map_or(0, |c| c.saturating_sub(1)),
            0x22 => cur.map_or(0, |c| (c + step).min(n - 1)),
            0x21 => cur.map_or(0, |c| c.saturating_sub(step)),
            0x24 => 0,
            0x23 => n - 1,
            _ => return false,
        };
        self.select_at(to);
        true
    }

    fn on_search_edited(&mut self) {
        self.proc_sc = Scroll::default();
        self.list.scroll = Scroll::default();
        self.rebuild();
    }

    /// What `Delete` means on this page.
    fn delete_key(&mut self, cx: &mut Cx) -> bool {
        match self.page {
            Page::Processes | Page::Details => self.do_act(Act::EndTask, cx),
            Page::Rules => self.do_act(Act::RuleDelete, cx),
            _ => return false,
        }
        true
    }

    /// The buttons of the head of this page: what each does, its label, its icon, whether it can be pressed.
    pub fn head_actions(&self) -> Vec<(HeadAct, &'static str, &'static str, bool)> {
        let has = self.sel.is_some();
        let run = (HeadAct::Do(Act::RunTask), t!("Запустить новую задачу", "Run new task", ja: "新しいタスクの実行", pt: "Executar nova tarefa", zh: "运行新任务"), crate::look::I_PLUS, true);
        let end = (HeadAct::Do(Act::EndTask), t!("Снять задачу", "End task", ja: "タスクの終了", pt: "Finalizar tarefa", zh: "结束任务"), crate::look::I_END, has);
        let more = (HeadAct::More, "", crate::look::I_MORE, true);
        let running = t!("Выполняется", "Running", ja: "実行中", pt: "Em execução", zh: "正在运行");
        let svc_running = self.selected_row().is_some_and(|r| r.cells.get(3).is_some_and(|c| c == running));
        match self.page {
            Page::Processes => vec![run, end, (HeadAct::Do(Act::Efficiency), t!("Режим эффективности", "Efficiency mode", ja: "効率モード", pt: "Modo de eficiência", zh: "能效模式"), crate::look::I_LEAF, has), more],
            Page::Details => vec![run, end, more],
            Page::Services => vec![
                (HeadAct::Do(Act::SvcStart), t!("Запустить", "Start", ja: "開始", pt: "Iniciar", zh: "启动"), crate::look::I_START, has && !svc_running),
                (HeadAct::Do(Act::SvcStop), t!("Остановить", "Stop", ja: "停止", pt: "Parar", zh: "停止"), crate::look::I_STOP, has && svc_running),
                more,
            ],
            Page::Startup | Page::Users => vec![more],
            Page::Insights => vec![(HeadAct::Do(Act::CheckSlow), if self.slow_chk.is_some() { t!("Проверяем…", "Checking\u{2026}", ja: "確認中…", pt: "Verificando…", zh: "检查中…") } else { t!("Проверить снова", "Check again", ja: "もう一度確認", pt: "Verificar de novo", zh: "重新检查") }, crate::look::I_TW_UPD, self.slow_chk.is_none())],
            Page::History => vec![(HeadAct::Do(Act::ClearHistory), t!("Удалить историю использования", "Delete usage history", ja: "使用履歴を削除", pt: "Excluir histórico de uso", zh: "删除使用历史记录"), crate::look::I_TRASH, !self.d.hist.is_empty()), more],
            Page::Rules => vec![(HeadAct::Do(Act::RuleDelete), t!("Удалить правило", "Delete rule", ja: "ルールを削除", pt: "Excluir regra", zh: "删除规则"), crate::look::I_TRASH, has), more],
            _ => vec![],
        }
    }

    fn head_button(&mut self, k: usize, cx: &mut Cx) {
        let acts = self.head_actions();
        let Some(&(a, _, _, enabled)) = acts.get(k) else { return };
        match a {
            HeadAct::Do(act) if !enabled => {
                let _ = act;
                self.say(t!("Сначала выберите строку в списке", "Select a row in the list first", ja: "先にリストで行を選択してください", pt: "Selecione primeiro uma linha na lista", zh: "请先在列表中选择一行"), crate::look::DOT_WARN);
            }
            HeadAct::Do(act) => self.do_act(act, cx),
            HeadAct::More => {
                let items = self.more_menu();
                let b = self.head_rects.get(k).copied().unwrap_or(Rect::new(self.size.0 - 200.0, 15.0, 34.0, 34.0));
                // the menu's right edge is the button's (the mockup: `x = right - 268`)
                cx.menu(TAG_MORE_MENU, items, Rect::new(b.x + b.w - 232.0, b.y, 1.0, b.h + 2.0));
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // what a click on an item is

    fn on_click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        if (ID_NAV..ID_NAV + NAV_PAGES.len() as Id).contains(&id) {
            return self.go_page(NAV_PAGES[(id - ID_NAV) as usize]);
        }
        match id {
            ID_DEMO => {
                match self.demo.as_ref().map(|d| d.hook) {
                    Some(crate::demo::Hook::ProcessMenu) => {
                        // where the design puts it: (520, 300) of its stage, the window standing 40 in from its edge
                        let items = self.row_menu();
                        cx.menu(TAG_ROW_MENU, items, Rect::new(480.0, 260.0, 1.0, 0.0));
                    }
                    Some(crate::demo::Hook::EndTree) => self.do_act(Act::EndTree, cx),
                    Some(crate::demo::Hook::CloseAsk) => {
                        self.close(cx);
                    }
                    _ => {}
                }
                return;
            }
            ID_SEARCH => return,
            ID_ALERT_SHOW => {
                let pid = self.alert_key();
                self.sorts.insert(Page::Processes, (7, true));
                self.rebuild();
                if let Some(i) = pid.and_then(|p| self.rows.iter().position(|r| r.pids.contains(&p))) {
                    self.select_at(i);
                }
                return;
            }
            ID_ALERT_END => {
                if let Some(pid) = self.alert_key() {
                    if let Some(i) = self.rows.iter().position(|r| r.pids.contains(&pid)) {
                        self.select_at(i);
                    }
                    self.do_act(Act::EndTask, cx);
                }
                return;
            }
            ID_ALERT_HIDE => {
                if let Some(pid) = self.alert_key() {
                    let name = self.alert_name(pid);
                    self.alert_off.insert(name);
                }
                return;
            }
            _ => {}
        }
        if (ID_ACT..ID_ACT + 8).contains(&id) {
            return self.head_button((id - ID_ACT) as usize, cx);
        }
        match self.page {
            Page::Processes => self.click_processes(id, b, x, y, cx),
            Page::Performance => {
                if (ID_PERF..ID_PERF + 32).contains(&id) {
                    let kinds: Vec<PerfKind> = self.perf_list().iter().map(|i| i.kind).collect();
                    if let Some(&k) = kinds.get((id - ID_PERF) as usize) {
                        self.perf_sel = k;
                        self.d.want_gpu_temp = matches!(k, PerfKind::Gpu(_)); // the temperature at once, not at the next tick
                    }
                }
            }
            Page::Users => self.click_users(id, cx),
            Page::Boost => self.click_speed(id, cx),
            Page::Settings => self.click_settings(id, cx),
            Page::Insights => self.click_slow(id, cx),
            _ => self.click_table(id, b, x, y, cx),
        }
    }

    fn click_table(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        if let Some(ColItem::Title(k)) = col_of(ID_COLS, id) {
            let model = table_spec(self.page).get(k).map_or(0, |s| s.0);
            if model == SWITCH_COL {
                return;
            }
            let cur = self.sort_of(self.page);
            let numeric = columns(self.page).get(model).is_some_and(|c| c.align == Align::Right);
            let ns = if cur.0 == model { (model, !cur.1) } else { (model, numeric) };
            self.sorts.insert(self.page, ns);
            self.cols_page = None;
            self.rebuild();
            return;
        }
        if (ID_SWITCH..ID_SWITCH + 10_000).contains(&id) {
            let i = (id - ID_SWITCH) as usize;
            self.select_at(i);
            let on = self.rows.get(i).is_some_and(|r| self.d.startup.iter().find(|s| s.key() == r.key).is_some_and(|s| s.enabled));
            return self.do_act(if on { Act::StartDisable } else { Act::StartEnable }, cx);
        }
        if let Some(i) = row_of(ID_TABLE, id).filter(|&i| i < self.rows.len()) {
            self.select_at(i);
            if b == Button::Right {
                let items = self.row_menu();
                cx.menu(TAG_ROW_MENU, items, Rect::new(x, y, 1.0, 0.0));
            }
        }
    }

    fn click_processes(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        if (ID_PHEAD..ID_PHEAD + 8).contains(&id) {
            let col = (id - ID_PHEAD) as usize;
            if col == 1 || col == 5 {
                return; // the status is not a measure, and the network one needs rights the page cannot give itself
            }
            let cur = self.sort_of(Page::Processes);
            let ns = if cur.0 == col { (col, !cur.1) } else { (col, col != 0) };
            self.sorts.insert(Page::Processes, ns);
            self.rebuild();
            return self.save_settings();
        }
        if (ID_CHEV..ID_CHEV + 100_000).contains(&id) {
            if let Some(k) = self.rows.get((id - ID_CHEV) as usize).map(|r| r.key.clone()) {
                self.toggle_expand(&k);
            }
            return;
        }
        // a row, or the pill of what is wrong with it (named on top of the row so that its hint shows)
        let row = if (ID_PROW..ID_PROW + 20_000).contains(&id) {
            Some((id - ID_PROW) as usize)
        } else {
            (ID_PROW + 20_000..ID_PROW + 40_000).contains(&id).then(|| (id - ID_PROW - 20_000) as usize)
        };
        if let Some(i) = row {
            if i >= self.rows.len() {
                return;
            }
            self.select_at(i);
            if b == Button::Right {
                let items = self.row_menu();
                cx.menu(TAG_ROW_MENU, items, Rect::new(x, y, 1.0, 0.0));
            } else if let Some(r) = self.selected_row().filter(|r| r.sev > 0) {
                // the mockup says what is wrong with a process the moment it is picked
                let text = format!("{}: {}", r.cells.first().cloned().unwrap_or_default(), crate::draw::tip_line(&r.tip));
                let dot = if r.sev >= 3 { crate::look::DOT_BAD } else { crate::look::DOT_WARN };
                self.say(&text, dot);
            }
        }
    }

    fn click_users(&mut self, id: Id, cx: &mut Cx) {
        if (ID_UMET..ID_UMET + 3).contains(&id) {
            self.u_metric = [UMetric::Mem, UMetric::Cpu, UMetric::Disk][(id - ID_UMET) as usize];
            return;
        }
        let users = self.users();
        if (ID_USER..ID_USER + 20).contains(&id) {
            if let Some(u) = users.get((id - ID_USER) as usize)
                && !self.u_open.remove(&u.key)
            {
                self.u_open.insert(u.key.clone());
            }
            return;
        }
        if (ID_UBTN..ID_UBTN + 40).contains(&id) {
            // signing someone out or messaging them is not something this program does (yet)
            return self.say(t!("Это пока недоступно", "This is not available yet", ja: "この機能はまだ利用できません", pt: "Isso ainda não está disponível", zh: "此功能暂不可用"), crate::look::DOT_WARN);
        }
        if (ID_UEND..ID_UEND + 60).contains(&id) {
            let k = (id - ID_UEND) as usize;
            let pids: Vec<u32> = users.iter().filter(|u| self.u_open.contains(&u.key)).flat_map(|u| u.apps.iter()).nth(k).map(|a| a.pids.clone()).unwrap_or_default();
            if pids.is_empty() {
                return;
            }
            let name = self.d.procs.iter().find(|p| pids.contains(&p.pid)).map(|p| p.name.clone()).unwrap_or_default();
            let row = Row { pids, cells: vec![name], ..Row::default() };
            self.act_process(&row, Act::EndTask, cx);
        }
    }

    fn click_slow(&mut self, id: Id, cx: &mut Cx) {
        use crate::draw::slowpage::{ID_SLOW, ID_SLOW_ADMIN, ID_SLOW_ALL, ID_SLOW_DEL, ID_SLOW_END};
        if (ID_SLOW..ID_SLOW + 3).contains(&id) {
            let k = (id - ID_SLOW) as usize;
            let bad = [self.slow.boot.bad, self.slow.apps.bad, self.slow.drivers.bad][k];
            let now = self.slow_open.get(&(k as u8)).copied().unwrap_or(bad);
            self.slow_open.insert(k as u8, !now);
        } else if (ID_SLOW_DEL..ID_SLOW_DEL + 8).contains(&id) {
            // the startup entry behind a row: off, or on again (the Startup page's own action)
            if let Some(d) = self.slow.boot.delays.get((id - ID_SLOW_DEL) as usize).filter(|d| d.can) {
                let row = Row { key: d.key.clone(), ..Row::default() };
                self.act_startup(&row, if d.on { Act::StartDisable } else { Act::StartEnable });
            }
        } else if id == ID_SLOW_ALL {
            self.go_page(Page::Startup);
        } else if id == ID_SLOW_ADMIN {
            self.do_act(Act::RelaunchAdmin, cx);
        } else if (ID_SLOW_END..ID_SLOW_END + 8).contains(&id)
            && let Some(r) = self.slow.apps.rows.get((id - ID_SLOW_END) as usize)
        {
            let row = Row { pids: r.pids.clone(), cells: vec![r.name.clone()], ..Row::default() };
            self.act_process(&row, Act::EndTask, cx);
        }
    }

    fn click_speed(&mut self, id: Id, cx: &mut Cx) {
        use boost::Id as B;
        if (ID_PROF..ID_PROF + 3).contains(&id) {
            let pick = (id - ID_PROF) as usize;
            let (game, work) = (self.boost_on(B::Game), self.boost_on(B::Work));
            let cur = if game { 1 } else if work { 2 } else { 0 };
            if pick == cur {
                return;
            }
            match pick {
                0 => {
                    if game {
                        self.run_boost(B::Game, false);
                    }
                    if work {
                        self.run_boost(B::Work, false);
                    }
                    self.say(t!("Профиль выключен · всё запущено снова", "Profile off · everything was started again", ja: "プロファイルをオフにしました · すべて再開しました", pt: "Perfil desligado · tudo foi iniciado de novo", zh: "配置已关闭 · 一切已重新启动"), crate::look::DOT_OK);
                }
                1 => self.boost_set(B::Game, true, cx),
                _ => self.boost_set(B::Work, true, cx),
            }
            return;
        }
        match id {
            ID_MEM_CLEAR => self.boost_set(B::MemNow, true, cx),
            ID_MEM_AUTO => {
                let on = !self.boost_on(B::MemAuto);
                self.boost_set(B::MemAuto, on, cx);
            }
            _ if (ID_TWEAK..ID_TWEAK + 20).contains(&id) => {
                let k = (id - ID_TWEAK) as usize;
                if k.is_multiple_of(2) {
                    let i = k / 2;
                    if !self.tw_open.remove(&i) {
                        self.tw_open.insert(i);
                    }
                } else if let Some(t) = crate::draw::TWEAKS.get(k / 2) {
                    let (id, on) = (t.id, !self.boost_on(t.id));
                    self.boost_set(id, on, cx);
                }
            }
            _ => {}
        }
    }

    fn click_settings(&mut self, id: Id, cx: &mut Cx) {
        if (ID_SETSEG..ID_SETSEG + 3).contains(&id) {
            return self.do_act(Act::SetTheme([0, 1, 2][(id - ID_SETSEG) as usize]), cx);
        }
        match id {
            i if i == ID_SETSW + 1 => self.do_act(Act::ToggleTray, cx),
            i if i == ID_SETSW + 2 => self.do_act(Act::ToggleAutostart, cx),
            i if i == ID_SETSW + 3 => self.do_act(Act::ToggleEtw, cx),
            i if i == ID_SETSW + 4 => self.do_act(Act::ToggleGpuTemp, cx),
            ID_SETBTN => self.do_act(Act::RelaunchAdmin, cx),
            _ => {}
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // the alert of a high-risk process

    /// The pid of the process the banner is about: the first one rated High that was not dismissed.
    pub fn alert_key(&self) -> Option<u32> {
        if self.page != Page::Processes {
            return None;
        }
        self.d.procs.iter().filter(|p| p.reasons.iter().map(|r| r.sev).max().unwrap_or(0) >= 3).find(|p| !self.alert_off.contains(&p.name.to_lowercase())).map(|p| p.pid)
    }

    pub fn alert_name(&self, pid: u32) -> String {
        self.d.procs.iter().find(|p| p.pid == pid).map(|p| p.name.to_lowercase()).unwrap_or_default()
    }

    pub fn perf_list(&self) -> Vec<PerfItem<'_>> {
        match &self.demo {
            Some(_) => crate::demo::perf_items(&self.d),
            None => perf_items(&self.d),
        }
    }

    pub fn users(&self) -> Vec<crate::draw::UserM> {
        match &self.demo {
            Some(dm) => dm.users.clone(),
            None => crate::draw::users_of(&self.d),
        }
    }
}

/// pid + all of its descendants (by the table pid -> parent).
pub fn descendants(parent_of: &HashMap<u32, u32>, roots: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = roots.to_vec();
    let mut i = 0;
    while i < out.len() {
        let cur = out[i];
        for (&pid, &par) in parent_of {
            if par == cur && !out.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}

impl App for Tm {
    fn title(&self) -> String {
        t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器").into()
    }

    fn min_size(&self) -> (f32, f32) {
        (860.0, 560.0)
    }

    fn caption_height(&self) -> f32 {
        crate::draw::HEAD_H
    }

    fn start(&mut self, hwnd: isize) {
        self.hwnd = hwnd;
        self.apply_theme();
        if !self.is_demo() {
            self.tick(); // the first numbers, so that the first frame has a table
            self.m.tray_add(hwnd);
        }
        self.refresh_tray_menu();
        // the timer needs a `Cx`: the first message gives it one
        winrust_ui::window::post(hwnd, MSG_START, 0, 0);
    }

    fn start_hidden(&self) -> bool {
        self.hidden_start
    }

    fn demo_clicks(&self) -> Vec<Id> {
        self.demo_click_ids()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        self.paint(c, f);
    }

    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        self.on_click(id, b, x, y, cx);
        cx.invalidate();
    }

    fn down(&mut self, id: Id, _b: Button, x: f32, y: f32, cx: &mut Cx) {
        let now = now_ms();
        if id == ID_SEARCH {
            self.search.mouse_down(x, cx.mods.shift, now);
        } else if id == scroll::bar_id(ID_PROC) {
            scroll::bar_down(&mut self.proc_sc, y, now);
        } else if id == scroll::bar_id(ID_TABLE) {
            scroll::bar_down(&mut self.list.scroll, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_USERS) {
            scroll::bar_down(&mut self.u_sc, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_SPEED) {
            scroll::bar_down(&mut self.speed_sc, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_SET) {
            scroll::bar_down(&mut self.set_sc, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_INS) {
            scroll::bar_down(&mut self.ins_sc, y, now);
        } else if let Some(ColItem::Grip(k)) = col_of(ID_COLS, id) {
            self.cols.grip_down(k, x);
        }
        cx.invalidate();
    }

    fn drag(&mut self, id: Id, x: f32, y: f32, cx: &mut Cx) {
        let now = now_ms();
        if id == ID_SEARCH {
            self.search.mouse_drag(x, now);
        } else if id == scroll::bar_id(ID_PROC) {
            scroll::bar_drag(&mut self.proc_sc, y, now);
        } else if id == scroll::bar_id(ID_TABLE) {
            scroll::bar_drag(&mut self.list.scroll, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_USERS) {
            scroll::bar_drag(&mut self.u_sc, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_SPEED) {
            scroll::bar_drag(&mut self.speed_sc, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_SET) {
            scroll::bar_drag(&mut self.set_sc, y, now);
        } else if id == scroll::bar_id(ID_SCROLL_INS) {
            scroll::bar_drag(&mut self.ins_sc, y, now);
        } else if matches!(col_of(ID_COLS, id), Some(ColItem::Grip(_))) {
            self.cols.grip_drag(x);
        }
        cx.invalidate();
    }

    fn release(&mut self, id: Id, cx: &mut Cx) {
        if id == ID_SEARCH {
            self.search.mouse_up();
        } else if id == scroll::bar_id(ID_PROC) {
            scroll::bar_up(&mut self.proc_sc);
        } else if id == scroll::bar_id(ID_TABLE) {
            scroll::bar_up(&mut self.list.scroll);
        } else if id == scroll::bar_id(ID_SCROLL_USERS) {
            scroll::bar_up(&mut self.u_sc);
        } else if id == scroll::bar_id(ID_SCROLL_SPEED) {
            scroll::bar_up(&mut self.speed_sc);
        } else if id == scroll::bar_id(ID_SCROLL_SET) {
            scroll::bar_up(&mut self.set_sc);
        } else if id == scroll::bar_id(ID_SCROLL_INS) {
            scroll::bar_up(&mut self.ins_sc);
        } else if matches!(col_of(ID_COLS, id), Some(ColItem::Grip(_))) {
            self.cols.grip_up();
        }
        cx.invalidate();
    }

    fn double(&mut self, id: Id, _cx: &mut Cx) {
        if self.page == Page::Processes
            && (ID_PROW..ID_PROW + 20_000).contains(&id)
            && let Some(k) = self.rows.get((id - ID_PROW) as usize).filter(|r| r.expandable).map(|r| r.key.clone())
        {
            self.toggle_expand(&k);
        }
    }

    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        let now = now_ms();
        let p = cx.pointer;
        match self.page {
            Page::Processes => scroll::wheel(&mut self.proc_sc, p, dy, ROW_H, now),
            Page::Users => scroll::wheel(&mut self.u_sc, p, dy, 56.0, now),
            Page::Boost => scroll::wheel(&mut self.speed_sc, p, dy, 60.0, now),
            Page::Settings => scroll::wheel(&mut self.set_sc, p, dy, 68.0, now),
            Page::Insights => scroll::wheel(&mut self.ins_sc, p, dy, 60.0, now),
            Page::Performance => false,
            _ => scroll::wheel(&mut self.list.scroll, p, dy, ROW_H, now),
        };
        cx.invalidate();
    }

    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        const ESC: u32 = 0x1B;
        const DEL: u32 = 0x2E;
        const F5: u32 = 0x74;
        const F: u32 = 0x46;
        const LEFT: u32 = 0x25;
        const RIGHT: u32 = 0x27;
        let now = now_ms();
        if cx.focus == Some(ID_SEARCH) && self.page_has_search() {
            match field::key(&mut self.search, cx, vk, m, now) {
                Ev::Edited => {
                    self.on_search_edited();
                    cx.invalidate();
                    return true;
                }
                Ev::Esc => {
                    if !self.search.text.is_empty() {
                        self.search.set_text("");
                        self.on_search_edited();
                    }
                    cx.invalidate();
                    return true;
                }
                Ev::Ignored => {}
                _ => {
                    cx.invalidate();
                    return true;
                }
            }
        }
        if m.ctrl && vk == F && self.page_has_search() {
            cx.focus(ID_SEARCH);
            cx.invalidate();
            return true;
        }
        let used = match vk {
            ESC => self.sel.take().is_some(),
            DEL => self.delete_key(cx),
            F5 => {
                self.tick();
                true
            }
            LEFT | RIGHT if self.page == Page::Processes => {
                if let Some((open, expandable, key)) = self.selected_row().map(|r| (r.expanded, r.expandable, r.key.clone()))
                    && expandable
                    && (vk == RIGHT) != open
                {
                    self.toggle_expand(&key);
                    true
                } else {
                    self.selected_row().is_some()
                }
            }
            _ => self.move_sel(vk),
        };
        if used {
            cx.invalidate();
        }
        used
    }

    fn text(&mut self, s: &str, cx: &mut Cx) {
        if cx.focus == Some(ID_SEARCH) && self.page_has_search() {
            field::text(&mut self.search, s, now_ms());
            self.on_search_edited();
            cx.invalidate();
        }
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        match tag {
            TAG_ROW_MENU | TAG_MORE_MENU => {
                if let Some(&a) = self.menu_acts.get(choice) {
                    self.do_act(a, cx);
                }
            }
            TAG_END => {
                if choice == 1
                    && let Some(Ask::End(act, pids)) = self.ask.take()
                {
                    self.run_process_act(act, &pids, cx);
                }
                self.ask = None;
            }
            TAG_BOOST => {
                if choice == 1
                    && let Some(Ask::Boost(id)) = self.ask.take()
                {
                    self.run_boost(id, true);
                }
                self.ask = None;
            }
            TAG_TRAY => {
                // "keep it in the tray when the window is closed?": 0 cancel, 1 quit, 2 keep
                if choice >= 1 {
                    self.d.cfg.to_tray = choice == 2;
                    self.d.cfg.tray_asked = true;
                    self.refresh_tray_menu();
                    if choice == 2 {
                        self.hide_to_tray();
                    } else {
                        self.quitting = true;
                        cx.close_window();
                    }
                }
            }
            _ => {}
        }
        cx.invalidate();
    }

    fn message(&mut self, msg: u32, wp: usize, _lp: isize, cx: &mut Cx) {
        match msg {
            MSG_START => {
                if !self.hidden_start {
                    cx.invalidate();
                }
                cx.wake_in(if self.hidden_start { 5000 } else { 1000 });
            }
            MSG_TRAY_SHOW | MSG_HOTKEY => self.show_window(cx),
            MSG_TRAY_CMD => {
                let act = match wp {
                    1 => Act::ShowWindow,
                    2 => Act::ToggleTray,
                    3 => Act::ToggleAutostart,
                    4 => Act::ToggleEtw,
                    5 => Act::RelaunchAdmin,
                    6 => Act::Exit,
                    _ => return,
                };
                self.do_act(act, cx);
                cx.invalidate();
            }
            _ => {}
        }
    }

    fn wake(&mut self, cx: &mut Cx) {
        // a window shown from outside (not by us) is not "away" any more; one that was hidden or minimized is
        self.away = self.hidden();
        self.tick();
        if !self.away {
            cx.invalidate();
        }
        cx.wake_in(if self.away { 5000 } else { 1000 });
    }

    fn close(&mut self, cx: &mut Cx) -> bool {
        if self.quitting {
            return self.finish();
        }
        let (asked, to_tray) = (self.d.cfg.tray_asked, self.d.cfg.to_tray);
        if !asked {
            // the first close asks: keep it in the tray, or quit?
            cx.dialog(
                TAG_TRAY,
                Dialog::new(
                    t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器"),
                    t!(
                        "Оставить диспетчер задач работать в трее при закрытии окна?\n\nВ трее он продолжает собирать историю и применять правила. Изменить это можно в настройках или в меню значка в трее.",
                        "Keep Task Manager running in the tray when you close the window?\n\nIn the tray it keeps collecting history and applying rules. You can change this later in Settings or in the tray icon's menu.",
                        ja: "ウィンドウを閉じたときにタスク マネージャーをトレイで動作させたままにしますか?\n\nトレイでは履歴の収集とルールの適用を続けます。この設定は後で [設定] またはトレイ アイコンのメニューで変更できます。",
                        pt: "Manter o Gerenciador de Tarefas em execução na bandeja ao fechar a janela?\n\nNa bandeja ele continua coletando o histórico e aplicando as regras. Você pode mudar isso depois em Configurações ou no menu do ícone da bandeja.",
                        zh: "关闭窗口时让任务管理器继续在托盘中运行吗？\n\n在托盘中它会继续收集历史记录并执行规则。以后可以在设置或托盘图标的菜单中更改。"
                    ),
                    &[t!("Отмена", "Cancel", ja: "キャンセル", pt: "Cancelar", zh: "取消"), t!("Выйти", "Quit", ja: "終了", pt: "Sair", zh: "退出"), t!("Оставить в трее", "Keep in tray", ja: "トレイに残す", pt: "Manter na bandeja", zh: "留在托盘")],
                ),
            );
            cx.invalidate();
            return false;
        }
        if to_tray {
            self.hide_to_tray();
            return false;
        }
        self.finish()
    }
}

impl Tm {
    /// The window really closes: the settings are written, the ETW session stopped, the icon taken away.
    fn finish(&mut self) -> bool {
        self.save_settings();
        if let Some(s) = self.d.etw.take() {
            self.m.etw_stop(s);
        }
        self.m.tray_remove();
        true
    }
}
