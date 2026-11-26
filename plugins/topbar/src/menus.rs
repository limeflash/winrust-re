//! topbar: the menus of the bar on the panel frame (`ui::menu`): the right click on an empty spot of the bar, on the clock, and
//! Win+X (the right click on Start). The jump list of an app button has its own panel (`ui::jump`).
//! (split out of main.rs; shares its items through `super::*`)
use super::*;
use ui::menu::{Item, MenuPanel};
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

/// Win+X: (icon, label in ru / en / ja / pt / zh, target). "!" targets are actions of ours, the rest goes through `open`.
type Tool = (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str);
const TOOLS: [Tool; 15] = [
    ("M4 4h6.5v6.5H4zM13.5 4H20v6.5h-6.5zM4 13.5h6.5V20H4zM13.5 13.5H20V20h-6.5z", "Приложения и возможности", "Apps and Features", "アプリと機能", "Aplicativos e Recursos", "应用和功能", "ms-settings:appsfeatures"),
    ("M6 7h9a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2zM20 10.5v3M10 9.5l-2 3h4l-2 3", "Электропитание", "Power Options", "電源オプション", "Opções de Energia", "电源选项", "powercfg.cpl"),
    ("M6 3h12v18H6zM9.5 8h5M9.5 12h5M9.5 16h3", "Просмотр событий", "Event Viewer", "イベント ビューアー", "Visualizador de Eventos", "事件查看器", "eventvwr.msc"),
    ("M3 5h18v11H3zM8 20h8M12 16v4", "Система", "System", "システム", "Sistema", "系统", "ms-settings:about"),
    ("M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4", "Диспетчер устройств", "Device Manager", "デバイス マネージャー", "Gerenciador de Dispositivos", "设备管理器", "devmgmt.msc"),
    ("M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18", "Сетевые подключения", "Network Connections", "ネットワーク接続", "Conexões de Rede", "网络连接", "ms-settings:network"),
    ("M3 14h18v5H3zM3 14l2.5-8h13L21 14M17 16.5h.01", "Управление дисками", "Disk Management", "ディスクの管理", "Gerenciamento de Disco", "磁盘管理", "diskmgmt.msc"),
    (GEAR, "Управление компьютером", "Computer Management", "コンピューターの管理", "Gerenciamento do Computador", "计算机管理", "compmgmt.msc"),
    ("M4 5h16v14H4zM7.5 10l3 2.5-3 2.5M13 15h4", "Терминал", "Terminal", "ターミナル", "Terminal", "终端", "!term"),
    (CHART, "Диспетчер задач", "Task Manager", "タスク マネージャー", "Gerenciador de Tarefas", "任务管理器", "taskmgr.exe"),
    ("M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12h2M3 12h2M12 3v2M12 19v2M17 7l1.5-1.5M5.5 18.5L7 17M17 17l1.5 1.5M5.5 5.5L7 7", "Параметры", "Settings", "設定", "Configurações", "设置", "ms-settings:"),
    ("M3.5 7.5V18h17V9h-9l-2-2h-6z", "Проводник", "File Explorer", "エクスプローラー", "Explorador de Arquivos", "文件资源管理器", "shell:MyComputerFolder"),
    ("M5 4h14v16H5zM9 9l3 3-3 3M14 15h2", "Выполнить", "Run", "ファイル名を指定して実行", "Executar", "运行", "!run"),
    ("M2.5 5h19v12h-19zM8 21h8", "Рабочий стол", "Desktop", "デスクトップ", "Área de Trabalho", "桌面", "!desktop"),
    (ui::icons::TASK_VIEW, "Представление задач", "Task View", "タスク ビュー", "Visão de Tarefas", "任务视图", "!taskview"),
];
const POWER: [Tool; 4] = [
    (LOCK, "Блокировка", "Lock", "ロック", "Bloquear", "锁定", "!lock"),
    (MOON, "Сон", "Sleep", "スリープ", "Suspender", "睡眠", "!sleep"),
    (ui::icons::RESTART, "Перезагрузка", "Restart", "再起動", "Reiniciar", "重启", "!restart"),
    (ui::icons::POWER, "Завершение работы", "Shut down", "シャットダウン", "Desligar", "关机", "!shutdown"),
];

/// The rows of a bar menu (`ctx`: an empty spot of the bar, `clock`, `winx`) and its width in DIPs.
pub(crate) fn bar_menu_items(kind: &str) -> (Vec<Item>, f32) {
    match kind {
        "clock" => (
            vec![
                Item::new(CALENDAR, t!("Настройка даты и времени", "Adjust date and time", ja: "日付と時刻を調整する", pt: "Ajustar data e hora", zh: "调整日期和时间")),
                Item::new(BELL, t!("Параметры уведомлений", "Notification settings", ja: "通知の設定", pt: "Configurações de notificação", zh: "通知设置")),
            ],
            272.0,
        ),
        "winx" => {
            let row = |i: usize, (icon, ru, en, ja, pt, zh, _): &Tool| {
                let it = Item::new(icon, t!(*ru, *en, ja: *ja, pt: *pt, zh: *zh));
                if i == TOOLS.len() { it.rule() } else { it }
            };
            (TOOLS.iter().chain(POWER.iter()).enumerate().map(|(i, t)| row(i, t)).collect(), 288.0)
        }
        _ => {
            let mut items = vec![
                Item::new(GEAR, t!("Параметры Windows", "Windows Settings", ja: "Windows の設定", pt: "Configurações do Windows", zh: "Windows 设置")).keys(&["Win", "I"]),
                Item::new(PALETTE, t!("Персонализация", "Personalization", ja: "個人用設定", pt: "Personalização", zh: "个性化")),
                Item::new(CHART, t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器")).keys(&["Ctrl", "Shift", "Esc"]).rule(),
                Item::new(BROOM, t!("Чистильщик диска", "Disk Cleaner", ja: "ディスク クリーナー", pt: "Limpador de Disco", zh: "磁盘清理")),
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
        "winx" => {
            let Some((.., to)) = TOOLS.iter().chain(POWER.iter()).nth(i) else { return };
            let quiet = |c: &mut Command| {
                let _ = c.creation_flags(NO_WINDOW).spawn();
            };
            match *to {
                "!term" => {
                    let wt = std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join(r"Microsoft\WindowsApps\wt.exe")).filter(|p| p.is_file());
                    open(&wide(if wt.is_some() { "wt.exe" } else { "powershell.exe" }));
                }
                "!run" => run_box(),
                "!desktop" => winrust_common::desktop::toggle(),
                "!taskview" => wins::taskview_toggle(),
                "!lock" => quiet(Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"])),
                "!sleep" => apps::sleep(),
                "!restart" => power_click(POW_R), // asks first, like the bar's own button
                "!shutdown" => power_click(POW_S),
                _ => open(&wide(to)),
            }
        }
        _ => {
            let pu = purge_installed();
            let i = if pu && i == 4 { 99 } else { i };
            match i {
                0 => open(&wide("ms-settings:")),
                1 => open(&wide("ms-settings:personalization")),
                // with rtaskmgr-win32 installed this is our task manager; a full path because explorer.exe does not search PATH
                2 => open(&wide(&format!(r"{}\System32\taskmgr.exe", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())))),
                3 => open_cleaner(),
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
        Side::Bottom => a.bar.1 as f32 / s - 8.0 - 12.0,
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
