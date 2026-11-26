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
            let at = (a.button.0 as f32 / s).clamp(8.0, (a.screen.0 as f32 / s - 8.0 - 296.0).max(8.0));
            (a.bar.1 as f32 / s - 8.0 - 12.0, at + 296.0 + 6.0 + 210.0 + 8.0 > a.screen.0 as f32 / s)
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
