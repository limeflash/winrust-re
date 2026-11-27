// WinRust installer (plugin-launcher.exe): a window in the Obsidian design and a command line that install / remove the plugins.
// The plugin exes are embedded (payload.txt, build.rs); payload\ next to this exe wins over them. Needs admin (see app.manifest).
//   main.rs     the plugin table (PLUGINS) and what the window's `Backend` does with it
//   install/    what each plugin changes in the system and how it is undone
//   cli.rs      the command line;  ui/  the window (a winrust-ui app: drawn with Direct2D, tested with no window)
#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]
#[macro_use]
extern crate winrust_common;
use winrust_common as i18n;
mod cli;
mod install;
mod selection;
mod ui;
mod update;
use {cli::*, install::*, selection::*, update::*};
use std::{
    env, fs,
    io::Write,
    mem::{size_of, zeroed},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    ptr::null_mut,
    sync::{Arc, Mutex},
    thread,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE},
    UI::WindowsAndMessaging::*,
};

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// A text in all languages (ru, en, ja, pt, zh) for static tables: `.get()` picks the current one.
#[derive(Clone, Copy)]
struct Tr(&'static str, &'static str, &'static str, &'static str, &'static str);
impl Tr {
    fn get(&self) -> &'static str {
        t!(self.0, self.1, ja: self.2, pt: self.3, zh: self.4)
    }
    /// The text in `l`, whatever language the process is in (the window shows the one its switch is on).
    fn in_lang(&self, l: i18n::Lang) -> &'static str {
        match l {
            i18n::Lang::Ru => self.0,
            i18n::Lang::En => self.1,
            i18n::Lang::Ja => self.2,
            i18n::Lang::Pt => self.3,
            i18n::Lang::Zh => self.4,
        }
    }
}

struct Plugin {
    name: &'static str,
    warn: Tr, // shown in a confirmation box before install
    installed: fn() -> bool,
    actions: Option<(fn() -> Result<(), String>, fn() -> Result<(), String>)>, // (install, uninstall); None = stub
    bulk: bool, // visible in the window and eligible for bulk install; false = command line only
}
const PLUGINS: &[Plugin] = &[
    Plugin {
        name: "rust-search",
        warn: Tr("Служба поиска Windows (WSearch) будет ОТКЛЮЧЕНА, SearchHost.exe заблокирован, строка поиска на панели задач скрыта: поиск в Пуске, Проводнике и Outlook перестанет работать.", "The Windows Search service (WSearch) will be DISABLED, SearchHost.exe blocked and the taskbar search box hidden: search in Start, Explorer and Outlook will stop working.", "Windows Search サービス (WSearch) が無効になり、SearchHost.exe がブロックされ、タスクバーの検索ボックスが非表示になります。スタート、エクスプローラー、Outlook での検索は動作しなくなります。", "O serviço Pesquisa do Windows (WSearch) será DESATIVADO, o SearchHost.exe bloqueado e a caixa de pesquisa da barra de tarefas ocultada: a pesquisa no Iniciar, no Explorer e no Outlook vai parar de funcionar.", "Windows 搜索服务（WSearch）将被停用，SearchHost.exe 被拦截，任务栏搜索框被隐藏：开始菜单、资源管理器和 Outlook 里的搜索将不再可用。"),
        installed: || prog_dir().join("rs.exe").is_file(),
        actions: Some((install, uninstall)),
        bulk: true,
    },
    Plugin {
        name: "rtaskmgr-win32",
        warn: Tr("Стандартный диспетчер задач будет ЗАМЕНЁН: любой запуск taskmgr.exe откроет rtaskmgr-win32.", "The stock Task Manager will be REPLACED: any launch of taskmgr.exe opens rtaskmgr-win32.", "標準のタスク マネージャーが置き換えられます: taskmgr.exe を起動すると rtaskmgr-win32 が開きます。", "O Gerenciador de Tarefas padrão será SUBSTITUÍDO: qualquer execução do taskmgr.exe abre o rtaskmgr-win32.", "系统自带的任务管理器将被替换：任何启动 taskmgr.exe 的操作都会打开 rtaskmgr-win32。"),
        installed: tm_installed,
        actions: Some((tm_install, tm_uninstall)),
        bulk: true,
    },
    Plugin {
        name: "topbar",
        warn: Tr("Панель задач Windows будет СКРЫТА и заменена панелью внизу экрана. Удаление плагина возвращает её.", "The Windows taskbar will be HIDDEN and replaced by a bar at the bottom of the screen. Uninstalling the plugin brings it back.", "Windows のタスクバーは非表示になり、画面下部のバーに置き換わります。プラグインをアンインストールすると元に戻ります。", "A barra de tarefas do Windows será OCULTADA e substituída por uma barra na parte inferior da tela. Desinstalar o plugin a traz de volta.", "Windows 任务栏将被隐藏，由屏幕底部的一条栏取而代之。卸载这个插件就会恢复。"),
        installed: || bar_dir().join("topbar.exe").is_file(),
        actions: Some((bar_install, bar_uninstall)),
        bulk: true,
    },
    Plugin {
        name: RUST_EXPLORER,
        warn: Tr("ОБЫЧНЫЙ Explorer будет ОТКЛЮЧЁН: после установки панель задач, рабочий стол и окна папок дают topbar и rust-explorer. Сначала установите topbar, без него не будет панели. Ещё для экономии RAM отключаются службы InventorySvc, lfsvc (геолокация), DoSvc ставится вручную, службы svchost объединяются (полностью после перезагрузки); удаление плагина вернёт их. Если что-то пойдёт не так: Ctrl+Alt+Del, Диспетчер задач, Запустить новую задачу, explorer.exe; затем удалите плагин.", "The REGULAR Explorer will be DISABLED: after install, topbar and rust-explorer provide the taskbar, the desktop and folder windows. Install topbar first, without it there will be no bar. To save RAM the services InventorySvc and lfsvc (geolocation) are also disabled, DoSvc is set to manual, and svchost services are merged (fully after a reboot); uninstalling the plugin restores them. If something goes wrong: Ctrl+Alt+Del, Task Manager, Run new task, explorer.exe; then uninstall the plugin.", "通常のエクスプローラーが無効になります: インストール後は topbar と rust-explorer がタスクバー、デスクトップ、フォルダー ウィンドウを提供します。先に topbar をインストールしてください。ないとバーが表示されません。メモリ節約のため、サービス InventorySvc、lfsvc (位置情報) も無効になり、DoSvc は手動に設定され、svchost のサービスは統合されます (完全に反映されるのは再起動後)。プラグインをアンインストールすると元に戻ります。問題が起きたら: Ctrl+Alt+Del、タスク マネージャー、新しいタスクの実行、explorer.exe の順に操作し、その後プラグインをアンインストールしてください。", "O Explorer COMUM será DESATIVADO: depois da instalação, a topbar e o rust-explorer fornecem a barra de tarefas, a área de trabalho e as janelas de pastas. Instale a topbar primeiro; sem ela não haverá barra. Para economizar RAM, os serviços InventorySvc e lfsvc (geolocalização) também são desativados, o DoSvc passa para manual e os serviços do svchost são unidos (totalmente após reiniciar); desinstalar o plugin os restaura. Se algo der errado: Ctrl+Alt+Del, Gerenciador de Tarefas, Executar nova tarefa, explorer.exe; depois desinstale o plugin.", "常规资源管理器将被停用：安装后，任务栏、桌面和文件夹窗口都由 topbar 和 rust-explorer 提供。请先安装 topbar，没有它就没有任务栏。为了省内存，还会停用 InventorySvc 和 lfsvc（定位）服务，把 DoSvc 设为手动，并合并 svchost 服务（重启后完全生效）；卸载插件会把它们恢复。如果出了问题：按 Ctrl+Alt+Del，打开任务管理器，运行新任务，输入 explorer.exe；然后卸载这个插件。"),
        installed: || rx_dir().join("rust-explorer.exe").is_file(),
        actions: Some((rx_install, rx_uninstall)),
        bulk: true,
    },
    Plugin {
        name: "wiztree",
        warn: Tr("Ничего системного не меняется: файл в Program Files и ярлык в меню Пуск.", "Nothing in the system is changed: a file in Program Files and a Start menu shortcut.", "システムは何も変更されません: Program Files 内のファイルとスタート メニューのショートカットのみです。", "Nada no sistema é alterado: um arquivo em Program Files e um atalho no menu Iniciar.", "不更改系统里的任何东西：只在 Program Files 里放一个文件，并建一个开始菜单快捷方式。"),
        installed: || wz_dir().join("wiztree.exe").is_file(),
        actions: Some((wz_install, wz_uninstall)),
        bulk: true,
    },
    Plugin {
        name: "photoview",
        warn: Tr("Программа добавляется в «Открыть с помощью» для jpg, png, bmp, gif, tiff, webp, heic, avif. Приложением по умолчанию её делаете вы сами в Параметрах.", "The program is added to 'Open with' for jpg, png, bmp, gif, tiff, webp, heic, avif. You make it the default app yourself in Settings.", "jpg、png、bmp、gif、tiff、webp、heic、avif の「プログラムから開く」にプログラムが追加されます。既定のアプリにするのは設定で自分で行ってください。", "O programa é adicionado a 'Abrir com' para jpg, png, bmp, gif, tiff, webp, heic e avif. Você mesmo o define como app padrão nas Configurações.", "程序会被加入 jpg、png、bmp、gif、tiff、webp、heic、avif 的「打开方式」。要不要设为默认应用，由你在「设置」里决定。"),
        installed: || pv_dir().join("photoview.exe").is_file(),
        actions: Some((pv_install, pv_uninstall)),
        bulk: true,
    },
    Plugin {
        name: "rust-settings",
        warn: Tr("Ничего системного не меняется: файл в Program Files и ярлык «WinRust Settings» в меню Пуск. Win+I начинает открывать это окно; удалите плагин, и всё вернётся.", "Nothing in the system is changed: a file in Program Files and a 'WinRust Settings' Start menu shortcut. Win+I starts opening this window; uninstall the plugin and it goes back.", "システムは何も変更されません: Program Files 内のファイルと、スタート メニューの「WinRust Settings」ショートカットのみです。Win+I でこのウィンドウが開くようになります。プラグインをアンインストールすると元に戻ります。", "Nada no sistema é alterado: um arquivo em Program Files e um atalho 'WinRust Settings' no menu Iniciar. O Win+I passa a abrir esta janela; desinstale o plugin e ele volta ao normal.", "不更改系统里的任何东西：只在 Program Files 里放一个文件，并建一个「WinRust Settings」开始菜单快捷方式。Win+I 会开始打开这个窗口；卸载插件后就会恢复原状。"),
        installed: || st_dir().join("rust-settings.exe").is_file(),
        actions: Some((st_install, st_uninstall)),
        bulk: true,
    },
    Plugin {
        name: "debloat",
        warn: Tr("Будут ОТКЛЮЧЕНЫ несколько служб (телеметрия, карты, отчёты об ошибках, диагностика и средства устранения неполадок, AI Fabric) и выставлены политики реестра (Copilot, Recall, реклама, фон Edge; Edge покажет «управляется организацией»). Старые значения сохраняются, uninstall debloat возвращает всё. Приложения не удаляются.", "Several services will be DISABLED (telemetry, maps, error reporting, diagnostics and troubleshooters, AI Fabric) and registry policies set (Copilot, Recall, ads, Edge background; Edge will show 'managed by your organization'). Old values are saved, uninstall debloat restores everything. No apps are removed.", "いくつかのサービス (テレメトリ、マップ、エラー報告、診断とトラブルシューティング ツール、AI Fabric) が無効になり、レジストリ ポリシー (Copilot、Recall、広告、Edge のバックグラウンド動作) が設定されます (Edge には「組織によって管理されています」と表示されます)。元の値は保存され、debloat をアンインストールするとすべて元に戻ります。アプリは削除されません。", "Vários serviços serão DESATIVADOS (telemetria, mapas, relatório de erros, diagnósticos e solucionadores de problemas, AI Fabric) e políticas de registro serão definidas (Copilot, Recall, anúncios, segundo plano do Edge; o Edge mostrará 'gerenciado pela sua organização'). Os valores antigos são salvos; desinstalar o debloat restaura tudo. Nenhum app é removido.", "将停用若干服务（遥测、地图、错误报告、诊断与疑难解答、AI Fabric），并设置注册表策略（Copilot、Recall、广告、后台 Edge；Edge 会显示「由你的组织管理」）。旧值会被保存，卸载 debloat 会全部还原。不会删除任何应用。"),
        installed: || db_dir().join("debloat.exe").is_file(),
        actions: Some((db_install, db_uninstall)),
        bulk: false, // changes system behaviour: only on request, never as part of "install everything"
    },
    Plugin {
        name: "purge",
        warn: Tr("НЕОБРАТИМО: приложения будут удалены для всех пользователей и из образа (новые пользователи их тоже не получат), задачи телеметрии удалены. Вернуть можно только из Store или winget. Магазин, Параметры, Пуск, Калькулятор, Фото и среды выполнения не затрагиваются. OneDrive и Edge не входят: purge.exe --apply --onedrive --edge. Сначала посмотрите список: purge.exe без параметров ничего не меняет.", "IRREVERSIBLE: apps are removed for all users and from the image (new users will not get them), telemetry tasks are deleted. They come back only from the Store or winget. The Store, Settings, Start, Calculator, Photos and runtimes are not touched. OneDrive and Edge are not included: purge.exe --apply --onedrive --edge. Look at the list first: purge.exe with no arguments changes nothing.", "元に戻せません: アプリはすべてのユーザーから、およびイメージから削除され (新しいユーザーにも入りません)、テレメトリ タスクも削除されます。復活するのは Store または winget からのみです。Store、設定、スタート、電卓、フォト、ランタイムは対象外です。OneDrive と Edge は含まれません: purge.exe --apply --onedrive --edge。まず一覧を確認してください: 引数なしの purge.exe は何も変更しません。", "IRREVERSÍVEL: os apps são removidos para todos os usuários e da imagem (novos usuários não os receberão) e as tarefas de telemetria são excluídas. Eles só voltam pela Store ou pelo winget. A Store, as Configurações, o Iniciar, a Calculadora, o Fotos e os runtimes não são afetados. OneDrive e Edge não estão incluídos: purge.exe --apply --onedrive --edge. Veja a lista primeiro: o purge.exe sem argumentos não altera nada.", "不可逆：应用会对所有用户并从镜像中移除（新用户也不会再得到它们），遥测任务会被删除。只能从应用商店或 winget 装回来。应用商店、设置、开始菜单、计算器、照片和运行库不受影响。不包括 OneDrive 和 Edge：`purge.exe --apply --onedrive --edge`。先看清单：不带参数的 purge.exe 不会改动任何东西。"),
        installed: || pu_dir().join("purge.exe").is_file(),
        actions: Some((pu_install, pu_uninstall)),
        bulk: false, // destructive and permanent: only on request, never as part of "install everything"
    },
    Plugin { name: "chrome-rs", warn: Tr("", "", "", "", ""), installed: || false, actions: None, bulk: true }, // a browser in Rust: installation is not available yet (placeholder)
];

/// The installable plugins, in install order (topbar comes before rust-explorer, which needs it).
pub(crate) fn ready() -> impl Iterator<Item = usize> {
    (0..PLUGINS.len()).filter(|&i| PLUGINS[i].actions.is_some() && PLUGINS[i].bulk)
}

/// What the window asks of the machine, answered by the install code the command line uses too: both do exactly the same thing.
struct RealBackend;

impl ui::Backend for RealBackend {
    fn mods(&self) -> Vec<ui::ModInfo> {
        PLUGINS.iter().map(|p| ui::ModInfo { name: p.name, installed: (p.installed)(), bulk: p.bulk, can: p.actions.is_some() }).collect()
    }
    fn install(&self, m: usize) -> Result<String, String> {
        install_plugin(m)
    }
    fn uninstall(&self, m: usize) -> Result<String, String> {
        uninstall_plugin(m)
    }
    fn rollback(&self) -> Result<String, String> {
        rollback()
    }
    fn warning(&self, m: usize, lang: i18n::Lang) -> String {
        PLUGINS[m].warn.in_lang(lang).to_string()
    }
    fn onedrive_hidden(&self) -> bool {
        od_hidden()
    }
    fn set_onedrive_hidden(&self, hide: bool) -> Result<(), String> {
        set_od_hidden(hide)
    }
    fn set_lang(&self, l: i18n::Lang) -> bool {
        i18n::set_lang(l)
    }
    fn version(&self) -> String {
        update::current().to_string()
    }
    fn newer(&self) -> Option<String> {
        update::look_for_update()
    }
    fn open_releases(&self) {
        let _ = Command::new("explorer.exe").arg(format!("{}/latest", update::BASE)).spawn();
    }
    fn sign_out(&self) {
        // the interactive session's, which is not this process's own account when UAC was answered with another administrator's
        let mut session = 0u32;
        // SAFETY: a plain query of this process's session into a local.
        unsafe { windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(windows_sys::Win32::System::Threading::GetCurrentProcessId(), &mut session) };
        let r = Command::new("logoff").arg(session.to_string()).creation_flags(0x0800_0000).spawn();
        if r.is_err() {
            let _ = Command::new("shutdown").arg("/l").creation_flags(0x0800_0000).spawn();
        }
    }
    fn elevated(&self) -> bool {
        winrust_common::proc::elevated()
    }
}

/// The window's size: 1000 x 640 DIPs, less on a screen that cannot hold it (the old window shrank to fit the same way).
fn window_size() -> (f32, f32) {
    // SAFETY: plain queries of the screen's DPI and work area.
    unsafe {
        let dc = GetDC(null_mut());
        let scale = GetDeviceCaps(dc, LOGPIXELSX as i32) as f32 / 96.0;
        ReleaseDC(null_mut(), dc);
        let mut wa: RECT = zeroed();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut _ as *mut _, 0) == 0 {
            return (1000.0, 640.0);
        }
        (1000.0_f32.min((wa.right - wa.left) as f32 / scale - 24.0).max(720.0), 640.0_f32.min((wa.bottom - wa.top) as f32 / scale - 24.0).max(520.0))
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if !args.is_empty() {
        std::process::exit(cli(&args));
    }
    // the GPU's device is made on a thread of its own while the list of what is installed is read
    winrust_ui::gfx::fast_start();
    let app = ui::Installer::new(Arc::new(RealBackend), i18n::lang());
    let code = winrust_ui::window::run(Box::new(app), winrust_ui::window::Options { class: "WinRustSetup", size: window_size(), icon: 1, place_file: None });
    winrust_ui::window::quit(code as u32)
}
