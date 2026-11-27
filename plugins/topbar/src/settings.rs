//! topbar: the gear's panel (the Obsidian "Bar settings") and what it asks of the bar (split out of main.rs; shares its items
//! through `super::*`). The panel itself is `ui::settings_panel`; it writes the look through `winrust_common::style` and asks
//! for everything that needs the bar through [`act`].
use super::*;
use std::sync::atomic::{AtomicU64, AtomicU8};
use ui::settings_panel::{Act, Info, Page, SettingsPanel, System};
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};

/// The updater's scheduled task (`src/update`: SYSTEM, at boot and once a day).
const UPDATE_TASK: &str = "WinRust-update";

/// 0 not read yet, 1 on, 2 off, 3 there is no task.
static AUTO: AtomicU8 = AtomicU8::new(0);

/// The gear was clicked: open the settings panel above it, or close it.
pub(crate) unsafe fn settings_toggle() {
    ui::panel::toggle(ui::panel::Id::Bar, || {
        let a = anchor(GEAR)?;
        thread::spawn(auto_update_read); // Task Scheduler is slow to answer: the panel shows the switch when it knows
        Some((Box::new(SettingsPanel::new(info(), i18n::style::themes(), Page::Main)), a))
    });
}

/// What the panel needs to know about the program around it.
fn info() -> Info {
    Info { version: version(), auto: auto_update, act, system, wallpaper }
}

/// The desktop wallpaper's file, for the previews.
fn wallpaper() -> String {
    // SAFETY: a plain system query into a local buffer.
    unsafe { vdesk::system_wallpaper() }
}

/// The version of the root crate (the installer's `CARGO_PKG_VERSION`, read from its manifest at build time).
fn release() -> &'static str {
    include_str!("../../../Cargo.toml").lines().find_map(|l| l.strip_prefix("version = \"")?.split('"').next()).unwrap_or("0")
}

/// The installed WinRust version: the one the updater wrote when it last swapped files, else this release's.
pub(crate) fn version() -> String {
    let file = std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join(r"WinRust\version.txt"));
    file.and_then(|f| std::fs::read_to_string(f).ok())
        .map(|s| s.trim().trim_start_matches('v').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| release().to_string())
}

fn auto_update() -> Option<bool> {
    match AUTO.load(Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Asks Task Scheduler whether the updater's task is enabled (its XML says so in every language).
fn auto_update_read() {
    let out = Command::new("schtasks.exe").args(["/query", "/tn", UPDATE_TASK, "/xml"]).creation_flags(NO_WINDOW).output();
    let state = match out {
        Ok(o) if o.status.success() => {
            let xml = String::from_utf8_lossy(&o.stdout).replace(['\0', ' '], "");
            if xml.contains("<Enabled>false</Enabled>") { 2 } else { 1 }
        }
        _ => 3,
    };
    AUTO.store(state, Relaxed);
}

fn reg_dword(path: &str, name: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut n = 4u32;
    // SAFETY: reads one DWORD into a local; the strings are NUL-terminated.
    let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, wide(path).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as *mut _, &mut n) };
    (r == 0).then_some(v)
}

/// %ProgramData%\WinRust\alt-space.txt: `0` leaves Alt+Space to the program in front (and to rs-launcher's own hotkey), anything
/// else, or no file, lets the bar's hook open its search with it. Read again at most once a second: the hook asks on each Alt+Space.
fn alt_space_file() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("WinRust").join("alt-space.txt"))
}

static ALT_SPACE_ON: AtomicU8 = AtomicU8::new(0); // 0 not read yet, 1 on, 2 off
static ALT_SPACE_AT: AtomicU64 = AtomicU64::new(0);

pub(crate) fn alt_space() -> bool {
    // SAFETY: a plain clock read.
    let now = unsafe { GetTickCount64() };
    if ALT_SPACE_ON.load(Relaxed) == 0 || now.saturating_sub(ALT_SPACE_AT.load(Relaxed)) > 1000 {
        let off = alt_space_file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|s| s.trim() == "0");
        ALT_SPACE_ON.store(if off { 2 } else { 1 }, Relaxed);
        ALT_SPACE_AT.store(now, Relaxed);
    }
    ALT_SPACE_ON.load(Relaxed) == 1
}

fn set_alt_space(on: bool) {
    ALT_SPACE_ON.store(if on { 1 } else { 2 }, Relaxed);
    // SAFETY: a plain clock read.
    ALT_SPACE_AT.store(unsafe { GetTickCount64() }, Relaxed);
    if let Some(f) = alt_space_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
}

/// What the advanced page can tell about this machine (it only shows it: the installer changes these).
fn system() -> System {
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_default();
    System {
        shell: !EXPLORER_UP.load(Relaxed),
        tray: None,
        search_key: alt_space(),
        taskmgr_key: pf.join(r"rtaskmgr\rtaskmgr-win32.exe").is_file(),
        clipboard: true, // the bar itself keeps the history (hist.rs)
        wsearch_off: reg_dword(r"SYSTEM\CurrentControlSet\Services\WSearch", "Start").map(|s| s == 4),
        taskbar_hidden: true, // while the bar runs, Windows' own taskbar is hidden (hide_taskbar)
    }
}

/// What the panel asks the bar for; runs after the click that asked, with the panel still open.
fn act(a: Act) {
    // SAFETY: the bar's own UI thread (the panel's window procedure runs the click).
    unsafe {
        match a {
            Act::Relook => relook(),
            Act::Language => _ = InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0),
            Act::Tile => tile_windows(),
            Act::OpenLog => open_log(),
            Act::Restart => restart_bar(),
            Act::Remove => remove_plugins(),
            Act::CheckUpdate => check_update(),
            Act::AutoUpdate(on) => set_auto_update(on),
            Act::AltSpace(on) => {
                set_alt_space(on);
                say(if on {
                    t!("Alt+Space открывает поиск WinRust:RE", "Alt+Space opens WinRust:RE search", ja: "Alt+Space で WinRust:RE 検索を開きます", pt: "Alt+Space abre a busca do WinRust:RE", zh: "Alt+Space 打开 WinRust:RE 搜索")
                } else {
                    t!("Alt+Space оставлен программам", "Alt+Space is left to other programs", ja: "Alt+Space は他のプログラムに任せます", pt: "Alt+Space fica para outros programas", zh: "Alt+Space 交给其他程序")
                });
            }
            Act::Say(text) => say(text),
        }
    }
}

/// Puts the bar in the new look: its place and size may have changed (the renderer glides to them), the colours too; the panel
/// follows the bar.
unsafe fn relook() {
    bars::sync(); // every bar: the place and size, and the bars of the other monitors that the settings ask for or no longer do
    if let Some(a) = anchor(GEAR) {
        ui::panel::reanchor(ui::panel::Id::Bar, a);
    }
}

/// A toast over the bar.
pub(crate) unsafe fn say(text: &str) {
    if let Some(a) = anchor(NONE) {
        ui::toast::show(text, a.side, a.bar, (a.origin.0, a.origin.1, a.screen.0, a.screen.1));
    }
}

unsafe fn open_log() {
    let log = std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join(r"plugin-launcher\install.log")).filter(|p| p.is_file());
    match log {
        Some(p) => {
            let _ = Command::new("notepad.exe").arg(p).spawn();
            say(t!("Журнал открыт в Блокноте", "The log opened in Notepad", ja: "ログをメモ帳で開きました", pt: "O log foi aberto no Bloco de Notas", zh: "日志已在记事本中打开"));
        }
        None => say(t!("Журнала установки ещё нет", "There is no install log yet", ja: "インストール ログはまだありません", pt: "Ainda não há log de instalação", zh: "还没有安装日志")),
    }
}

/// Starts the bar again once this one has gone (the single-instance guard lets only one live), and closes this one after the
/// toast has been seen.
unsafe fn restart_bar() {
    let Ok(exe) = std::env::current_exe() else { return };
    let line = format!("/c ping -n 4 127.0.0.1 >nul & start \"\" \"{}\"", exe.display());
    if Command::new("cmd.exe").raw_arg(line).creation_flags(NO_WINDOW).spawn().is_err() {
        return;
    }
    say(t!("Панель перезапущена", "The bar restarted", ja: "バーを再起動しました", pt: "A barra foi reiniciada", zh: "任务栏已重启"));
    let bar = BAR.load(Relaxed);
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(1400));
        // SAFETY: posts WM_CLOSE to our own window (a window handle is valid from any thread to post to).
        unsafe { PostMessageW(bar as HWND, WM_CLOSE, 0, 0) };
    });
}

/// "Remove every plugin": the installer's own rollback, which asks for UAC.
unsafe fn remove_plugins() {
    let exe = std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join(r"WinRust\WinRust.exe")).filter(|p| p.is_file());
    match exe {
        Some(exe) => {
            ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide(&exe.to_string_lossy()).as_ptr(), wide("rollback --yes").as_ptr(), null(), SW_SHOWNORMAL);
            say(t!("Все плагины будут удалены после подтверждения UAC", "Every plugin is removed after the UAC prompt", ja: "UAC の確認後にすべてのプラグインが削除されます", pt: "Todos os plugins serão removidos após a confirmação do UAC", zh: "确认 UAC 后将删除所有插件"));
        }
        None => say(t!("Установщик WinRust не найден", "The WinRust installer was not found", ja: "WinRust インストーラーが見つかりません", pt: "O instalador do WinRust não foi encontrado", zh: "未找到 WinRust 安装程序")),
    }
}

/// "Check now": runs the updater's scheduled task (it checks the signed manifest and swaps the files that changed).
unsafe fn check_update() {
    let run = Command::new("schtasks.exe").args(["/run", "/tn", UPDATE_TASK]).creation_flags(NO_WINDOW).output();
    if !run.is_ok_and(|o| o.status.success()) {
        say(t!("Задача автообновления не установлена или нет прав", "The update task is not installed, or needs administrator rights", ja: "更新タスクがないか、管理者権限が必要です", pt: "A tarefa de atualização não está instalada ou exige administrador", zh: "未安装更新任务，或需要管理员权限"));
    }
}

unsafe fn set_auto_update(on: bool) {
    let flag = if on { "/enable" } else { "/disable" };
    let done = Command::new("schtasks.exe").args(["/change", "/tn", UPDATE_TASK, flag]).creation_flags(NO_WINDOW).output().is_ok_and(|o| o.status.success());
    if done {
        AUTO.store(if on { 1 } else { 2 }, Relaxed);
    } else {
        say(t!("Нужны права администратора", "Administrator rights are needed", ja: "管理者権限が必要です", pt: "São necessários direitos de administrador", zh: "需要管理员权限"));
    }
}
