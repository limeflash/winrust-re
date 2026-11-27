//! Windows Update through the Windows Update Agent COM API (documented): look for updates, download and install them.
//! Searching and installing take minutes, so they run in a thread; the window shows `state()`. Installing needs administrator
//! rights (Windows says "access denied" otherwise, which is reported as such).
use std::sync::Mutex;
use windows::{
    Win32::System::{
        Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
        UpdateAgent::*,
    },
    core::BSTR,
};

#[derive(Clone, Default)]
pub struct State {
    pub busy: Option<String>,   // what is going on right now
    pub found: Option<Vec<String>>, // titles of the updates waiting; None: not looked yet
    pub note: String,           // the result of the last action
}

static STATE: Mutex<State> = Mutex::new(State { busy: None, found: None, note: String::new() });

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or_default()
}

fn with<T>(f: impl FnOnce(&mut State) -> T) {
    if let Ok(mut s) = STATE.lock() {
        f(&mut s);
    }
}

fn session() -> windows::core::Result<IUpdateSession> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)
    }
}

fn search(s: &IUpdateSession) -> windows::core::Result<IUpdateCollection> {
    // no Type filter: drivers and firmware too, as Windows Update itself offers them (#100)
    unsafe { s.CreateUpdateSearcher()?.Search(&BSTR::from("IsInstalled=0 and IsHidden=0"))?.Updates() }
}

fn titles(c: &IUpdateCollection) -> Vec<String> {
    unsafe { (0..c.Count().unwrap_or(0)).filter_map(|i| c.get_Item(i).ok()).filter_map(|u| u.Title().ok()).map(|t| t.to_string()).collect() }
}

fn err_text(e: &windows::core::Error) -> String {
    if e.code().0 as u32 == 0x8007_0005 {
        t!("Нужны права администратора", "Administrator rights are needed", ja: "管理者権限が必要です", pt: "São necessários direitos de administrador", zh: "需要管理员权限").into()
    } else {
        e.message()
    }
}

fn start(what: &str, done: impl FnOnce() + Send + 'static, job: impl FnOnce() -> String + Send + 'static) {
    let mut busy = true;
    with(|s| {
        busy = s.busy.is_some();
        if !busy {
            s.busy = Some(what.to_string());
        }
    });
    if busy {
        return;
    }
    std::thread::spawn(move || {
        let note = job();
        with(|s| (s.busy, s.note) = (None, note));
        done();
    });
}

/// Looks for updates.
pub fn check(done: impl FnOnce() + Send + 'static) {
    start(t!("Поиск обновлений…", "Checking for updates…", ja: "更新プログラムを確認しています…", pt: "Verificando atualizações…", zh: "正在检查更新…"), done, || match session().and_then(|s| search(&s)) {
        Ok(c) => {
            let t = titles(&c);
            CHECKED.store(now_unix(), std::sync::atomic::Ordering::Relaxed);
            let note = if t.is_empty() { t!("Обновлений нет: установлены все", "You're up to date", ja: "最新の状態です", pt: "Você está atualizado", zh: "已安装所有更新").into() } else { tf!("Найдено обновлений: {}", "Updates found: {}", ja: "見つかった更新プログラム: {}", pt: "Atualizações encontradas: {}", zh: "找到更新：{}", t.len()) };
            with(|s| s.found = Some(t));
            note
        }
        Err(e) => tf!("Не удалось проверить: {}", "Could not check: {}", ja: "確認できませんでした: {}", pt: "Não foi possível verificar: {}", zh: "无法检查：{}", err_text(&e)),
    });
}

/// Downloads and installs every update found by `check`.
pub fn install(done: impl FnOnce() + Send + 'static) {
    start(t!("Загрузка и установка обновлений…", "Downloading and installing updates…", ja: "更新プログラムをダウンロードしてインストールしています…", pt: "Baixando e instalando atualizações…", zh: "正在下载并安装更新…"), done, || {
        let run = || -> windows::core::Result<String> {
            unsafe {
                let s = session()?;
                let found = search(&s)?;
                if found.Count()? == 0 {
                    return Ok(t!("Обновлений нет: установлены все", "You're up to date", ja: "最新の状態です", pt: "Você está atualizado", zh: "已安装所有更新").into());
                }
                for i in 0..found.Count()? {
                    let u = found.get_Item(i)?;
                    if !u.EulaAccepted()?.as_bool() {
                        u.AcceptEula()?;
                    }
                }
                let d = s.CreateUpdateDownloader()?;
                d.SetUpdates(&found)?;
                d.Download()?;
                let inst = s.CreateUpdateInstaller()?;
                inst.SetUpdates(&found)?;
                let r = inst.Install()?;
                Ok(if r.RebootRequired()?.as_bool() { t!("Установлено. Нужна перезагрузка", "Installed. Restart to finish", ja: "インストールしました。完了するには再起動してください", pt: "Instalado. Reinicie para concluir", zh: "安装完成，需要重启").into() } else { t!("Обновления установлены", "Updates installed", ja: "更新プログラムをインストールしました", pt: "Atualizações instaladas", zh: "更新已安装").into() })
            }
        };
        match run() {
            Ok(m) => {
                with(|s| s.found = Some(vec![]));
                m
            }
            Err(e) => tf!("Не удалось установить: {}", "Could not install: {}", ja: "インストールできませんでした: {}", pt: "Não foi possível instalar: {}", zh: "无法安装：{}", err_text(&e)),
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// what Windows knows besides a search of our own: a restart is waiting, when it last looked, when it last installed

/// Seconds since 1970 (UTC).
type Unix = i64;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Info {
    pub restart: bool,
    /// Windows' own last successful search and installation.
    pub search: Option<Unix>,
    pub install: Option<Unix>,
    /// Our last successful search in this session.
    pub checked: Option<Unix>,
}

static INFO: Mutex<Option<(std::time::Instant, Info)>> = Mutex::new(None);
static READING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CHECKED: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn now_unix() -> Unix {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as Unix)
}

/// A restart is waiting: the Update Agent says so, or one of the two keys Windows leaves for it.
fn reboot_pending() -> bool {
    use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE as HKLM;
    let key = |parent: &str, name: &str| crate::sys::subkeys(HKLM, parent).iter().any(|k| k == name);
    // SAFETY: COM objects made and used on this thread.
    let com = || unsafe { CoCreateInstance::<_, ISystemInformation>(&SystemInformation, None, CLSCTX_INPROC_SERVER).ok()?.RebootRequired().ok().map(|b| b.as_bool()) };
    key(r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update", "RebootRequired")
        || key(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing", "RebootPending")
        || com().unwrap_or(false)
}

/// An OLE date (days since 1899-12-30, UTC) as Unix seconds.
fn unix_of(date: f64) -> Option<Unix> {
    (date > 25_569.0).then(|| ((date - 25_569.0) * 86_400.0).round() as Unix)
}

fn read_info() -> Info {
    // SAFETY: COM objects made and used on this thread.
    let dates = || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let r = CoCreateInstance::<_, IAutomaticUpdates2>(&AutomaticUpdates, None, CLSCTX_INPROC_SERVER).ok()?.Results().ok()?;
        let date = |v: windows::core::Result<windows::Win32::System::Variant::VARIANT>| {
            use windows::Win32::System::Variant::{VAR_CHANGE_FLAGS, VARIANT, VT_R8, VariantChangeType};
            // the date comes as a VT_DATE: as a double it is the OLE date itself
            let v = v.ok()?;
            let mut out = VARIANT::default();
            VariantChangeType(&mut out, &v, VAR_CHANGE_FLAGS(0), VT_R8).ok()?;
            f64::try_from(&out).ok().and_then(unix_of)
        };
        Some((date(r.LastSearchSuccessDate()), date(r.LastInstallationSuccessDate())))
    };
    let (search, install) = dates().unwrap_or((None, None));
    Info { restart: reboot_pending(), search, install, checked: Some(CHECKED.load(std::sync::atomic::Ordering::Relaxed)).filter(|&t| t > 0) }
}

/// What is known now; the reading itself (a COM call that can take a second) runs in a thread and is repeated at most every 20 s.
pub fn info() -> Info {
    use std::sync::atomic::Ordering::Relaxed;
    let (old, last) = INFO.lock().ok().and_then(|g| *g).map_or((true, Info::default()), |(at, i)| (at.elapsed().as_secs() >= 20, i));
    if old && !READING.swap(true, Relaxed) {
        std::thread::spawn(|| {
            let i = read_info();
            if let Ok(mut g) = INFO.lock() {
                *g = Some((std::time::Instant::now(), i));
            }
            READING.store(false, Relaxed);
        });
    }
    // our own search can be newer than anything Windows wrote down
    Info { checked: Some(CHECKED.load(Relaxed)).filter(|&t| t > 0), ..last }
}

/// Reboots the PC (the user asked, and confirmed): the shutdown privilege, then `ExitWindowsEx`; true when Windows accepted it.
pub fn restart() -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LUID},
        Security::{AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY},
        System::{
            Shutdown::{EWX_REBOOT, ExitWindowsEx, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OPERATINGSYSTEM, SHTDN_REASON_MINOR_UPGRADE},
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };
    // SAFETY: the process's own token, a privilege that is only switched on, then the documented reboot call.
    unsafe {
        let mut tok = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok) == 0 {
            return false;
        }
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        let name: Vec<u16> = "SeShutdownPrivilege\0".encode_utf16().collect();
        let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
        let ok = LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) != 0;
        let tp = TOKEN_PRIVILEGES { Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: tp.Privileges[0].Attributes }], ..tp };
        let ok = ok && AdjustTokenPrivileges(tok, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0;
        CloseHandle(tok);
        ok && ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OPERATINGSYSTEM | SHTDN_REASON_MINOR_UPGRADE | SHTDN_REASON_FLAG_PLANNED) != 0
    }
}

#[cfg(test)]
mod live {
    /// Reads only, by hand: `cargo test -p rust-settings --release --offline -- --ignored print_info --nocapture` (restart() is never called).
    #[test]
    #[ignore = "reads the real machine"]
    fn print_info() {
        println!("{:?}", super::read_info());
    }
}
