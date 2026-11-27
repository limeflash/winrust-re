//! topbar: the menus of the bar on the panel frame (`ui::menu`): the right click on an empty spot of the bar and on the clock; and
//! Win+X (the right click on Start) on its own panel (`ui::winx`). The jump list of an app button has its own panel (`ui::jump`).
//! (split out of main.rs; shares its items through `super::*`)
use super::*;
use std::rc::Rc;
use ui::gfx::Rgba;
use ui::menu::{Item, MenuPanel};
use ui::winx::{Glyph, Pick, Row, Tile, WinxPanel};
use ui::panel::{self, Id};

const GEAR: &str = ui::icons::GEAR;
const PALETTE: &str = "M12 3a9 9 0 1 0 0 18c1.1 0 1.5-.8 1.2-1.7-.4-1 .3-1.8 1.3-1.8H17a4 4 0 0 0 4-4c0-5-4-10.5-9-10.5zM7.5 11h.01M10 7.5h.01M14 7.5h.01";
const CHART: &str = "M4 20V4M4 20h16M8 16l3-4 3 2 5-7";
const BROOM: &str = "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6";
const CALENDAR: &str = "M4 6h16v14H4zM4 10h16M8 3v4M16 3v4";
const BELL: &str = "M6 16v-5a6 6 0 0 1 12 0v5l1.5 2h-15zM10 21h4";
pub(crate) const LOCK: &str = "M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3";
pub(crate) const MOON: &str = "M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7z";
const CLOSE_BAR: &str = "M7 7l10 10M17 7L7 17";
const SHOT: &str = "M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4M12 14.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z";
const REC: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 15.4a3.4 3.4 0 1 0 0-6.8 3.4 3.4 0 0 0 0 6.8z";
const REC_STOP: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM9.5 9.5h5v5h-5z";

/// The rows of a bar menu (`ctx`: an empty spot of the bar, `clock`) and its width in DIPs.
pub(crate) fn bar_menu_items(kind: &str) -> (Vec<Item>, f32) {
    match kind {
        "clock" => (
            vec![
                Item::new(CALENDAR, t!("Настройка даты и времени", "Adjust date and time", ja: "日付と時刻を調整する", pt: "Ajustar data e hora", zh: "调整日期和时间")),
                Item::new(BELL, t!("Параметры уведомлений", "Notification settings", ja: "通知の設定", pt: "Configurações de notificação", zh: "通知设置")),
            ],
            272.0,
        ),
        _ => {
            let mut items = vec![
                Item::new(GEAR, t!("Параметры Windows", "Windows Settings", ja: "Windows の設定", pt: "Configurações do Windows", zh: "Windows 设置")).keys(&["Win", "I"]),
                Item::new(PALETTE, t!("Персонализация", "Personalization", ja: "個人用設定", pt: "Personalização", zh: "个性化")),
                Item::new(CHART, t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器")).keys(&["Ctrl", "Shift", "Esc"]).rule(),
                Item::new(BROOM, t!("Чистильщик диска", "Disk Cleaner", ja: "ディスク クリーナー", pt: "Limpador de Disco", zh: "磁盘清理")),
                Item::new(SHOT, t!("Скриншот", "Screenshot", ja: "スクリーンショット", pt: "Captura de tela", zh: "截图")).keys(&["Win", "Shift", "S"]).rule(),
                if snip::recording() {
                    Item::new(REC_STOP, t!("Остановить запись", "Stop recording", ja: "録画を停止", pt: "Parar gravação", zh: "停止录制")).keys(&["Win", "Shift", "R"]).danger()
                } else {
                    Item::new(REC, t!("Запись экрана", "Record screen", ja: "画面を録画", pt: "Gravar a tela", zh: "录制屏幕")).keys(&["Win", "Shift", "R"])
                },
            ];
            if purge_installed() {
                items.push(Item::new(BROOM, t!("Удалить мусор Windows (необратимо)", "Remove Windows junk (irreversible)", ja: "Windows の不要なものを削除 (元に戻せません)", pt: "Remover lixo do Windows (irreversível)", zh: "删除 Windows 垃圾文件（不可恢复）")).danger());
            }
            items.push(Item::new(CLOSE_BAR, t!("Закрыть панель", "Close the bar", ja: "バーを閉じる", pt: "Fechar a barra", zh: "关闭任务栏")).danger().rule());
            (items, if purge_installed() { 340.0 } else { 288.0 })
        }
    }
}

/// What row `i` of a bar menu does.
unsafe fn pick(kind: &str, i: usize) {
    match kind {
        "clock" => {
            if i == 0 {
                // the classic applet opens at once with or without Explorer; the Settings page needs Explorer started first
                let _ = Command::new("control.exe").arg("timedate.cpl").creation_flags(NO_WINDOW).spawn();
            } else {
                open(&wide("ms-settings:notifications"));
            }
        }
        _ => {
            let pu = purge_installed();
            let i = if pu && i == 6 { 99 } else { i };
            match i {
                0 => open(&wide("ms-settings:")),
                1 => open(&wide("ms-settings:personalization")),
                // with rtaskmgr-win32 installed this is our task manager; a full path because explorer.exe does not search PATH
                2 => open(&wide(&format!(r"{}\System32\taskmgr.exe", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())))),
                3 => open_cleaner(),
                4 => snip::open(ui::snip::Kind::Shot),
                5 => snip::toggle_rec(),
                99 => open_purge(),
                _ => _ = PostMessageW(FindWindowW(wide("topbar").as_ptr(), null()), WM_CLOSE, 0, 0), // quits the bar, brings the Windows taskbar back
            }
        }
    }
}

thread_local! {
    static OPEN: Cell<&'static str> = const { Cell::new("") };
}

/// Opens the menu `kind` on the bar anchored at `a`; the same menu again closes it.
pub(crate) unsafe fn bar_menu(kind: &'static str, a: panel::Anchor) {
    if panel::is_open(Id::Menu) && OPEN.get() == kind {
        return panel::close();
    }
    let (items, w) = bar_menu_items(kind);
    menu_at(items, w, a, move |i| unsafe { pick(kind, i) });
    OPEN.set(kind);
}

/// Opens a menu of `items` (`w` DIPs wide) anchored at `a`: the Start menu's own right-click menus stand here too.
pub(crate) unsafe fn menu_at(items: Vec<Item>, w: f32, a: panel::Anchor, pick: impl Fn(usize) + 'static) {
    let s = SCALE.get();
    let room = match a.side {
        Side::Bottom => (a.bar.1 - a.origin.1) as f32 / s - 8.0 - 12.0,
        _ => a.screen.1 as f32 / s - 24.0,
    };
    OPEN.set("");
    panel::open(Box::new(MenuPanel::new(items, w, room, a.side, pick)), a);
}

/// The place of a right click at screen position `x` along the bar (a hair of a button) for a panel to stand at.
pub(crate) unsafe fn anchor_at(x: i32) -> Option<panel::Anchor> {
    let mut a = anchor(NONE)?;
    a.button = if a.side == Side::Bottom { (x, a.bar.1, x + 1, a.bar.3) } else { (a.bar.0, x, a.bar.2, x + 1) };
    Some(a)
}

// ---------------------------------------------------------------------------------------------------------------------
// Win+X: the Start button's menu (`ui::winx`)

/// What a tile, a row or a row of the sign-out submenu does.
#[derive(Clone, Copy)]
enum Do {
    /// Through `open`: a Settings page, a control panel applet, a management console.
    Open(&'static str),
    Terminal,
    TerminalAdmin,
    Search,
    Run,
    Desktop,
    Lock,
    SignOut,
    Sleep,
    Restart,
    Shutdown,
    TaskManager,
    Explorer,
    /// The row that opens the submenu: nothing runs.
    Nothing,
}

/// The program that stands for a terminal: Windows Terminal if it is installed, else PowerShell.
fn terminal() -> &'static str {
    let wt = std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join(r"Microsoft\WindowsApps\wt.exe")).filter(|p| p.is_file());
    if wt.is_some() { "wt.exe" } else { "powershell.exe" }
}

unsafe fn act(what: Do) {
    match what {
        Do::Open(target) => open(&wide(target)),
        Do::Terminal => open(&wide(terminal())),
        Do::TerminalAdmin => _ = ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide(terminal()).as_ptr(), null(), null(), SW_SHOWNORMAL),
        Do::Search => search_toggle(),
        Do::Run => run_box(),
        Do::Desktop => winrust_common::desktop::toggle(),
        Do::Lock => _ = Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]).creation_flags(NO_WINDOW).spawn(),
        Do::SignOut => _ = Command::new("shutdown.exe").arg("/l").creation_flags(NO_WINDOW).spawn(),
        Do::Sleep => apps::sleep(),
        Do::Restart => power_click(POW_R), // asks first, like the bar's own button
        Do::Shutdown => power_click(POW_S),
        // with rtaskmgr-win32 installed this is our task manager; a full path because explorer.exe does not search PATH
        Do::TaskManager => open(&wide(&format!(r"{}\System32\taskmgr.exe", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())))),
        Do::Explorer => open(&wide("shell:MyComputerFolder")),
        Do::Nothing => {}
    }
}

/// The program's own icon as a tile's picture, or a drawing when the shell has none.
unsafe fn tile_icon(path: &str, fallback: &'static str) -> Glyph {
    let ic = item_icon(path, (26.0 * SCALE.get()).round() as i32);
    if ic.is_null() { Glyph::Plate(fallback, Rgba::hex(0x4a4f5c), Rgba::hex(0x2c2f38)) } else { Glyph::Icon(Rc::new(ui::taskview::OwnedIcon(wicon(ic)))) }
}

/// The rows of a menu, each with what it does.
type Rows = Vec<(Row, Do)>;

/// A row and what it does.
fn line(icon: &'static str, label: &str, mn: char, keys: &'static str, what: Do) -> (Row, Do) {
    let it = ui::winx::Item::new(icon, label, mn);
    (Row::Item(if keys.is_empty() { it } else { it.keys(keys) }), what)
}

/// The tiles, the rows and the sign-out submenu of the Start menu, each with what it does, in the mockup's order (`SM_T`,
/// `SM_I`, `SM_U`). A row's key is the letter that chooses it.
unsafe fn winx_lists() -> (Vec<(Tile, Do)>, Rows, Rows) {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let tile = |d: Do, label: &str, glyph: Glyph| (Tile { label: label.into(), glyph }, d);
    let tiles = vec![
        tile(Do::Terminal, t!("Терминал", "Terminal", ja: "ターミナル", pt: "Terminal", zh: "终端"), Glyph::Plate("M4 5h16v14H4zM7.5 10l3 2.5-3 2.5M13 15h4", Rgba::hex(0x3a3f4b), Rgba::hex(0x22252d))),
        tile(Do::TaskManager, t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器"), tile_icon(&format!(r"{sys}\System32\Taskmgr.exe"), CHART)),
        tile(Do::Open("ms-settings:"), t!("Параметры", "Settings", ja: "設定", pt: "Configurações", zh: "设置"), Glyph::Plate(GEAR, Rgba::hex(0x6b7280), Rgba::hex(0x3f4552))),
        tile(Do::Explorer, t!("Проводник", "File Explorer", ja: "エクスプローラー", pt: "Explorador de Arquivos", zh: "文件资源管理器"), tile_icon(&format!(r"{sys}\explorer.exe"), "M3.5 7.5V18h17V9h-9l-2-2h-6z")),
    ];
    let rule = || (Row::Rule, Do::Nothing);
    let rows = vec![
        line("M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z", t!("Установленные приложения", "Installed apps", ja: "インストールされているアプリ", pt: "Aplicativos instalados", zh: "已安装的应用"), 'F', "", Do::Open("ms-settings:appsfeatures")),
        line("M13 2L5 13h6l-1 9 8-12h-6z", t!("Электропитание", "Power options", ja: "電源オプション", pt: "Opções de energia", zh: "电源选项"), 'O', "", Do::Open("powercfg.cpl")),
        line("M3 5h18v11H3zM8 20h8M12 16v4", t!("Система", "System", ja: "システム", pt: "Sistema", zh: "系统"), 'Y', "", Do::Open("ms-settings:about")),
        line("M7 4h10v16H7zM11 17h2M4 8h3M4 12h3M17 8h3M17 12h3", t!("Диспетчер устройств", "Device Manager", ja: "デバイス マネージャー", pt: "Gerenciador de Dispositivos", zh: "设备管理器"), 'M', "", Do::Open("devmgmt.msc")),
        line("M3 9.5a13 13 0 0 1 18 0M6 13a8.5 8.5 0 0 1 12 0M9 16.5a4 4 0 0 1 6 0M12 19.5h.01", t!("Сетевые подключения", "Network connections", ja: "ネットワーク接続", pt: "Conexões de rede", zh: "网络连接"), 'W', "", Do::Open("ms-settings:network")),
        line("M4 7h16v10H4zM7.5 12h.01M11 12h6", t!("Управление дисками", "Disk Management", ja: "ディスクの管理", pt: "Gerenciamento de disco", zh: "磁盘管理"), 'K', "", Do::Open("diskmgmt.msc")),
        line("M4 5h16v11H4zM9 20h6M12 16v4M8 9l2 2-2 2M13 13h3", t!("Управление компьютером", "Computer Management", ja: "コンピューターの管理", pt: "Gerenciamento do computador", zh: "计算机管理"), 'G', "", Do::Open("compmgmt.msc")),
        line("M6 3h9l3 3v15H6zM9 10h6M9 14h6M9 18h3", t!("Просмотр событий", "Event Viewer", ja: "イベント ビューアー", pt: "Visualizador de Eventos", zh: "事件查看器"), 'V', "", Do::Open("eventvwr.msc")),
        rule(),
        line("M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM9.5 11l2 2-2 2", t!("Терминал (администратор)", "Terminal (Admin)", ja: "ターミナル (管理者)", pt: "Terminal (Administrador)", zh: "终端(管理员)"), 'A', "", Do::TerminalAdmin),
        line("M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4", t!("Поиск", "Search", ja: "検索", pt: "Pesquisar", zh: "搜索"), 'S', "Win+S", Do::Search),
        line("M5 12h14M13 6l6 6-6 6", t!("Выполнить", "Run", ja: "ファイル名を指定して実行", pt: "Executar", zh: "运行"), 'R', "Win+R", Do::Run),
        rule(),
        (Row::Item(ui::winx::Item::new(ui::icons::POWER, t!("Завершение работы или выход", "Shut down or sign out", ja: "シャットダウンまたはサインアウト", pt: "Desligar ou sair", zh: "关机或注销"), 'U').submenu()), Do::Nothing),
        line("M3 5h18v12H3zM8 21h8M12 17v4", t!("Рабочий стол", "Desktop", ja: "デスクトップ", pt: "Área de trabalho", zh: "桌面"), 'D', "Win+D", Do::Desktop),
    ];
    let sub = vec![
        line("M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3", t!("Блокировка", "Lock", ja: "ロック", pt: "Bloquear", zh: "锁定"), 'L', "", Do::Lock),
        line("M14 4h5v16h-5M10 8l-4 4 4 4M6 12h10", t!("Выход", "Sign out", ja: "サインアウト", pt: "Sair", zh: "注销"), 'I', "", Do::SignOut),
        rule(),
        line("M20 14.5A8 8 0 1 1 9.5 4 6.5 6.5 0 0 0 20 14.5z", t!("Сон", "Sleep", ja: "スリープ", pt: "Suspender", zh: "睡眠"), 'S', "", Do::Sleep),
        line("M20 12a8 8 0 1 1-2.5-5.8M20 4v5h-5", t!("Перезагрузка", "Restart", ja: "再起動", pt: "Reiniciar", zh: "重启"), 'R', "", Do::Restart),
        (Row::Item(ui::winx::Item::new(ui::icons::POWER, t!("Завершение работы", "Shut down", ja: "シャットダウン", pt: "Desligar", zh: "关机"), 'U').danger()), Do::Shutdown),
    ];
    (tiles, rows, sub)
}

/// The Start menu on the bar anchored at `a` (Win+X, the right click on Start); the same again closes it. Its left edge is 24 px
/// left of the middle of the button (beside a side bar: its top 60 px above the middle).
pub(crate) unsafe fn winx_menu(mut a: panel::Anchor) {
    if panel::is_open(Id::Menu) && OPEN.get() == "winx" {
        return panel::close();
    }
    let s = SCALE.get();
    let (room, left) = match a.side {
        Side::Bottom => {
            let c = (a.button.0 + a.button.2) / 2;
            a.button.0 = c - (24.0 * s).round() as i32;
            let at = ((a.button.0 - a.origin.0) as f32 / s).clamp(8.0, (a.screen.0 as f32 / s - 8.0 - 296.0).max(8.0));
            ((a.bar.1 - a.origin.1) as f32 / s - 8.0 - 12.0, at + 296.0 + 6.0 + 210.0 + 8.0 > a.screen.0 as f32 / s)
        }
        side => {
            let c = (a.button.1 + a.button.3) / 2;
            a.button.1 = c - (60.0 * s).round() as i32;
            (a.screen.1 as f32 / s - 24.0, side == Side::Right)
        }
    };
    panel::open(winx_content(a.side, room, left), a);
    OPEN.set("winx");
}

/// The Start menu's panel: `room` DIPs of height to fit in, the submenu to the `left` of it or to the right.
pub(crate) unsafe fn winx_content(side: Side, room: f32, left: bool) -> Box<dyn panel::Content> {
    let (tiles, rows, sub) = winx_lists();
    let (tiles, tile_acts): (Vec<Tile>, Vec<Do>) = tiles.into_iter().unzip();
    let (rows, row_acts): (Vec<Row>, Vec<Do>) = rows.into_iter().unzip();
    let (sub, sub_acts): (Vec<Row>, Vec<Do>) = sub.into_iter().unzip();
    Box::new(WinxPanel::new(tiles, rows, sub, room, side, left, move |p| {
        // SAFETY: what the Start button's menu has always done, on the bar's thread.
        unsafe {
            match p {
                Pick::Tile(i) => act(tile_acts[i]),
                Pick::Row(i) => act(row_acts[i]),
                Pick::Sub(i) => act(sub_acts[i]),
            }
        }
    }))
}

// ---------------------------------------------------------------------------------------------------------------------
// the desktop's menu (`ui::desk_menu`)

/// GB used and in all on the volume of `drive`.
unsafe fn disk_of(drive: char) -> Option<ui::desk_menu::Disk> {
    let (mut caller, mut total, mut free) = (0u64, 0u64, 0u64);
    let ok = windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide(&format!("{drive}:\\")).as_ptr(), &mut caller, &mut total, &mut free);
    if ok == 0 { None } else { ui::desk_menu::Disk::from_bytes(total, free) }
}

/// What rust-explorer's desktop asked for with a right click at the pointer: the menu and where it goes, or `None` when it
/// cannot be shown now (no panel host). The menu's choices go back to the desktop ([`winrust_common::deskmenu::send`]) or
/// to the bar's own tools: those ask the bar's window for what Win+Shift+S and Win+Shift+R do, 160 ms on (the mockup's
/// wait), when the menu has left the screen the picture is taken of.
pub(crate) unsafe fn desk_menu(r: &winrust_common::deskmenu::Request) -> Option<(Box<dyn panel::Content>, panel::Anchor)> {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    use ui::desk_menu::{DeskPanel, Pick as DeskPick, State};
    use winrust_common::deskmenu;
    if panel::hwnd().is_null() {
        return None;
    }
    let s = SCALE.get();
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(MonitorFromPoint(POINT { x: r.at.0, y: r.at.1 }, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
        return None;
    }
    let (m, work) = (mi.rcMonitor, mi.rcWork);
    // the room: the monitor's work area, less the bar when it floats over the screen without reserving a strip
    let bar = anchor(NONE);
    let mut room = (work.left, work.top, work.right, work.bottom);
    if let Some(a) = bar.filter(|a| a.origin == (m.left, m.top)) {
        match a.side {
            Side::Bottom => room.3 = room.3.min(a.bar.1),
            Side::Left => room.0 = room.0.max(a.bar.2),
            Side::Right => room.2 = room.2.min(a.bar.0),
        }
    }
    let disk = r.drive.and_then(|d| disk_of(d));
    let (w, h) = DeskPanel::size_for(disk.is_some());
    let (at, corner) = ui::desk_menu::place(r.at, ((w * s).round() as i32, (h * s).round() as i32), room, (12.0 * s).round() as i32, s);
    let st = State { sort: r.sort, size: r.size, auto: r.auto, shown: r.shown, recording: snip::recording(), disk };
    let content = DeskPanel::new(st, at, corner, |p| {
        let later = |wp: usize| {
            thread::spawn(move || {
                thread::sleep(std::time::Duration::from_millis(160));
                // SAFETY: posts plain integers to the bar's window.
                unsafe { PostMessageW(BAR.load(Relaxed) as HWND, SNIP, wp, 0) };
            });
        };
        match p {
            DeskPick::Cmd(c) => _ = deskmenu::send(c),
            DeskPick::Shot => later(0),
            DeskPick::Record => later(3),
        }
    });
    let a = panel::Anchor { side: i18n::style::side(), button: (r.at.0, r.at.1, r.at.0 + 1, r.at.1 + 1), bar: bar.map_or((0, 0, 0, 0), |a| a.bar), screen: (m.right - m.left, m.bottom - m.top), origin: (m.left, m.top) };
    OPEN.set("desk");
    Some((Box::new(content), a))
}

// ---------------------------------------------------------------------------------------------------------------------
// the menu for files, apps and folders (`filemenu`, `ui::file_menu`)

thread_local! {
    /// The menu on the screen: its session, and where the items of its submenus go when Windows has filled them.
    static FILE_MENU: RefCell<Option<(u32, ui::file_menu::Inbox)>> = const { RefCell::new(None) };
}

/// Whether `h` is a window of rust-explorer: only it may ask for this menu (the request carries what the menu will do).
pub(crate) unsafe fn is_rust_explorer(h: HWND) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    if h.is_null() {
        return false;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, &mut pid);
    let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if p.is_null() {
        return false;
    }
    let mut buf = [0u16; 520];
    let mut n = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut n) != 0;
    CloseHandle(p);
    ok && String::from_utf16_lossy(&buf[..n as usize]).rsplit('\\').next().is_some_and(|f| f.eq_ignore_ascii_case("rust-explorer.exe"))
}

/// The shell's name for the kind of file ("Text document").
unsafe fn type_name(path: &str) -> String {
    let mut sfi: SHFILEINFOW = zeroed();
    if SHGetFileInfoW(wide(path).as_ptr(), 0, &mut sfi, size_of::<SHFILEINFOW>() as u32, SHGFI_TYPENAME) == 0 {
        return String::new();
    }
    wstr_of(&sfi.szTypeName)
}

/// An icon for the header: the picture of an image where the shell has one made already, else the file's icon, else a drawing.
unsafe fn head_icon(path: &str, target: filemenu::Target, scale: f32) -> filemenu::HeadIcon {
    use filemenu::{HeadIcon, Target};
    let px = (32.0 * scale).round() as i32;
    if target == Target::Image {
        let ic = item_thumb(path, (64.0 * scale).round() as i32);
        if !ic.is_null() {
            return HeadIcon::Thumb(Rc::new(ui::taskview::OwnedIcon(wicon(ic))));
        }
    }
    let ic = item_icon(path, px);
    if ic.is_null() { HeadIcon::Plate(filemenu::FILE, 0xf4f5f7, 0xf4f5f7, 0x6a7080) } else { HeadIcon::Shell(Rc::new(ui::taskview::OwnedIcon(wicon(ic)))) }
}

/// What the menu says about its target, read from the disk: the kind, the name, the second line of the header.
unsafe fn file_facts(req: &winrust_common::filemenu::Request, scale: f32) -> (filemenu::Facts, filemenu::HeadIcon) {
    use filemenu::{HeadIcon, Target, classify, items_text, meta_of, size_text};
    let plate = |d, c: u32| HeadIcon::Plate(d, c, c, 0xa3a9b8);
    let name_of = |p: &str| std::path::Path::new(p).file_name().map_or_else(|| p.to_string(), |n| n.to_string_lossy().into_owned());
    let mut f = filemenu::Facts { target: Target::File, name: String::new(), meta: String::new(), stem: String::new(), start_pinned: false, bar_pinned: false, bin_items: 0, extract: false, flags: req.flags };
    if req.flags & winrust_common::filemenu::rq::BIN != 0 || req.paths.first().is_some_and(|p| classify(p, true) == Target::Bin) {
        let mut info: SHQUERYRBINFO = zeroed();
        info.cbSize = size_of::<SHQUERYRBINFO>() as u32;
        let ok = SHQueryRecycleBinW(null(), &mut info) >= 0;
        f.target = Target::Bin;
        f.name = t!("Корзина", "Recycle Bin", ja: "ごみ箱", pt: "Lixeira", zh: "回收站").to_string();
        f.bin_items = if ok { info.i64NumItems.clamp(0, i64::from(u32::MAX)) as u32 } else { 0 };
        f.meta = if f.bin_items > 0 { format!("{} · {}", items_text(f.bin_items as usize), size_text(info.i64Size.max(0) as u64)) } else { t!("Пусто", "Empty", ja: "空", pt: "Vazia", zh: "空").to_string() };
        return (f, plate(filemenu::BIN, 0x2b2c33));
    }
    if let [one] = req.paths.as_slice() {
        let md = std::fs::metadata(one).ok();
        let dir = md.as_ref().is_some_and(|m| m.is_dir());
        f.target = classify(one, dir);
        f.name = name_of(one);
        f.stem = std::path::Path::new(one).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let ty = type_name(one);
        f.meta = if dir {
            let n = std::fs::read_dir(one).map_or(0, |d| d.take(10_000).count());
            format!("{ty} · {}", items_text(n))
        } else {
            meta_of(&ty, md.as_ref().map(|m| m.len()))
        };
        let lower = one.to_lowercase();
        f.extract = f.target == Target::Archive && (lower.ends_with(".zip") || lower.ends_with(".7z"));
        if f.target == Target::App {
            f.start_pinned = apps::start_pinned(one);
            f.bar_pinned = bar_pin_of(one).is_some();
        }
        let head = head_icon(one, f.target, scale);
        return (f, head);
    }
    f.target = Target::Several;
    f.flags &= !winrust_common::filemenu::rq::CAN_RENAME;
    f.name = items_text(req.paths.len());
    let total: u64 = req.paths.iter().filter_map(|p| std::fs::metadata(p).ok()).filter(|m| m.is_file()).map(|m| m.len()).sum();
    let names: Vec<String> = req.paths.iter().take(2).map(|p| name_of(p)).collect();
    f.meta = format!("{} · {}{}", size_text(total), names.join(", "), if req.paths.len() > 2 { "…" } else { "" });
    (f, plate(filemenu::COPY, 0x2b2c33))
}

/// The bar's pin of the app at `path` (a shortcut or an exe), if there is one.
unsafe fn bar_pin_of(path: &str) -> Option<usize> {
    let lower = path.to_lowercase();
    let lnk = lower.ends_with(".lnk");
    let exe = if lnk { lnk_target(&wide(path)) } else { lower };
    let args = if lnk { lnk_args(&wide(path)) } else { String::new() };
    if exe.is_empty() {
        return None;
    }
    PINS.with_borrow(|p| p.iter().position(|p| p.target == exe && p.args == args))
}

/// What the menu chose: rust-explorer is told (and lets go of the menu once it has been told something, or nothing was chosen);
/// Start's and the bar's own pins are the bar's.
unsafe fn file_pick(from: isize, session: u32, paths: &[String], d: &filemenu::Do) {
    use filemenu::Do;
    use winrust_common::filemenu::{Act, send_act};
    let to = from;
    match d {
        Do::Shell(id) => _ = send_act(to, session, Act::Item(*id)),
        Do::Ours(a) => _ = send_act(to, session, *a),
        Do::PinStart => {
            if let Some(p) = paths.first() {
                apps::start_toggle(p);
            }
            _ = send_act(to, session, Act::Done);
        }
        Do::PinBar => {
            if let Some(p) = paths.first() {
                match bar_pin_of(p) {
                    Some(i) => unpin(i),
                    None => pin_exe(p, ""),
                }
                refresh_buttons();
                bars::invalidate_all();
            }
            _ = send_act(to, session, Act::Done);
        }
        Do::Nothing | Do::More | Do::Less => _ = send_act(to, session, Act::Done),
    }
}

/// The items of a submenu Windows has filled, for the menu that asked: they are turned into rows and the panel redraws.
pub(crate) fn file_sub(r: &winrust_common::filemenu::SubReply) {
    FILE_MENU.with_borrow(|m| {
        if let Some((session, inbox)) = m
            && *session == r.session
        {
            inbox.borrow_mut().push((r.handle, filemenu::sub_rows(&r.items)));
            panel::refresh();
        }
    });
}

/// The menu rust-explorer asked for at `req.at`: where it goes and what it holds, or `None` when it cannot be shown now (no panel
/// host, no monitor). `from` is the window of rust-explorer the choices go back to.
pub(crate) unsafe fn file_menu(req: &winrust_common::filemenu::Request, from: HWND) -> Option<(Box<dyn panel::Content>, panel::Anchor)> {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    use ui::file_menu::{Env, FileMenu, Hooks};
    if panel::hwnd().is_null() {
        return None;
    }
    let s = SCALE.get();
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(MonitorFromPoint(POINT { x: req.at.0, y: req.at.1 }, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
        return None;
    }
    let (m, work) = (mi.rcMonitor, mi.rcWork);
    // the room: the monitor's work area, less the bar when it floats over the screen without reserving a strip
    let bar = anchor(NONE);
    let mut room = (work.left, work.top, work.right, work.bottom);
    if let Some(a) = bar.filter(|a| a.origin == (m.left, m.top)) {
        match a.side {
            Side::Bottom => room.3 = room.3.min(a.bar.1),
            Side::Left => room.0 = room.0.max(a.bar.2),
            Side::Right => room.2 = room.2.min(a.bar.0),
        }
    }
    let (facts, head) = file_facts(req, s);
    let menu = filemenu::build(&facts, &req.items, head);
    let (session, paths, to) = (req.session, req.paths.clone(), from as isize);
    let inbox: ui::file_menu::Inbox = Rc::default();
    FILE_MENU.set(Some((session, Rc::clone(&inbox))));
    let hooks = Hooks {
        // SAFETY: calls into the bar's own functions, on its thread (the panel runs the choice there).
        pick: Box::new(move |d| unsafe { file_pick(to, session, &paths, d) }),
        fill: Box::new(move |h| _ = winrust_common::filemenu::send_act(to, session, winrust_common::filemenu::Act::Fill(h))),
    };
    let env = Env { scale: s, at: req.at, room, monitor_h: (m.bottom - m.top) as f32 / s };
    let content = FileMenu::new(menu, env, hooks, inbox);
    let a = panel::Anchor { side: i18n::style::side(), button: (req.at.0, req.at.1, req.at.0 + 1, req.at.1 + 1), bar: bar.map_or((0, 0, 0, 0), |a| a.bar), screen: (m.right - m.left, m.bottom - m.top), origin: (m.left, m.top) };
    OPEN.set("file");
    Some((Box::new(content), a))
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use filemenu::Target;
    use winrust_common::filemenu::{Request, rq};

    fn req(paths: &[std::path::PathBuf], flags: u32) -> Request {
        Request { session: 1, at: (0, 0), flags, paths: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(), items: vec![] }
    }

    /// What the header says about real files in a temp folder (nothing is run, no menu is built).
    #[test]
    fn the_header_facts_of_real_files_are_read_from_the_disk() {
        // SAFETY: COM for the shell's icons and type names, on this test's own thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d = std::env::temp_dir().join(format!("wr-filemenu-facts-{}", std::process::id()));
            std::fs::create_dir_all(d.join("folder")).unwrap();
            for (n, len) in [("note.txt", 2048usize), ("big.bin", 3_800_000), ("shot.png", 10), ("a.zip", 100)] {
                std::fs::write(d.join(n), vec![b'x'; len]).unwrap();
            }
            std::fs::write(d.join("folder").join("one"), b"1").unwrap();
            let facts = |names: &[&str], flags: u32| file_facts(&req(&names.iter().map(|n| d.join(n)).collect::<Vec<_>>(), flags), 1.0).0;

            let f = facts(&["note.txt"], rq::DESKTOP | rq::CAN_RENAME);
            assert_eq!((f.target, f.name.as_str(), f.stem.as_str()), (Target::Text, "note.txt", "note"));
            assert!(f.meta.contains(" · ") && f.meta.ends_with("2 КБ"), "{}", f.meta);
            let f = facts(&["big.bin"], 0);
            assert_eq!(f.target, Target::File);
            assert!(f.meta.ends_with("3,6 МБ"), "{}", f.meta);
            assert_eq!(facts(&["shot.png"], 0).target, Target::Image);
            let f = facts(&["a.zip"], 0);
            assert!(f.target == Target::Archive && f.extract);
            let f = facts(&["folder"], rq::TAB);
            assert_eq!(f.target, Target::Folder);
            assert!(f.meta.ends_with("1"), "one item in it: {}", f.meta);
            // several: counted, summed, and no Rename
            let f = facts(&["note.txt", "big.bin", "shot.png"], rq::DESKTOP | rq::CAN_RENAME);
            assert_eq!(f.target, Target::Several);
            assert!(f.name.contains('3') && f.meta.contains("note.txt, big.bin…"), "{} / {}", f.name, f.meta);
            assert_eq!(f.flags & rq::CAN_RENAME, 0);
            // the Recycle Bin is told by its address
            let bin = Request { paths: vec!["::{645FF040-5081-101B-9F08-00AA002F954E}".into()], ..req(&[], 0) };
            assert_eq!(file_facts(&bin, 1.0).0.target, Target::Bin);
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn the_bar_only_answers_windows_of_rust_explorer() {
        // SAFETY: plain queries about windows of this process.
        unsafe {
            assert!(!is_rust_explorer(null_mut()));
            // the foreground window of a test run is not rust-explorer's (and may not exist: both are a no)
            assert!(!is_rust_explorer(GetDesktopWindow()));
        }
    }
}
