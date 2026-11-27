//! Windows Update through the Windows Update Agent COM API (documented): look for updates, download and install the ticked ones, the
//! restart that waits, the history, the dates Windows keeps. Searching and installing take minutes, so they run in a thread that polls the
//! job (`BeginSearch` / `BeginDownload` / `BeginInstall`, `IsCompleted`, `GetProgress`, `RequestAbort`); the window shows `state()`.
//! Installing needs administrator rights (Windows says "access denied" otherwise, which is reported as such). Nothing here runs in a test.
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering::Relaxed},
    },
};

use windows::{
    Win32::System::{
        Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
        UpdateAgent::*,
        Variant::VARIANT,
    },
    core::BSTR,
};

/// The kinds of update the design names with a colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    Security,
    Net,
    Definitions,
    Driver,
    Preview,
    Cumulative,
    Other,
}

/// One update Windows found: what the list shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upd {
    pub id: String,
    pub title: String,
    /// "KB5046617", or empty (a driver has none).
    pub kb: String,
    pub cat: Cat,
    pub mb: u32,
    /// It finishes after a restart.
    pub restart: bool,
    /// Drivers and previews: installed only when picked.
    pub optional: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Downloading,
    Installing,
}

/// Why an installation did not work (the update, its KB, the error code, a short reason).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub title: String,
    pub kb: String,
    pub code: String,
    pub text: String,
}

/// A line of the update history: group, title, KB, when ("Today, 15:12", "16 Oct 2026"), whether it worked, the error code when it did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hist {
    pub group: HGroup,
    pub title: String,
    pub kb: String,
    pub when: String,
    pub ok: bool,
    pub code: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HGroup {
    Quality,
    Driver,
    Definition,
    Other,
}

#[derive(Clone, Default)]
pub struct State {
    pub phase: Phase,
    /// What was found by the last search of this window; None: not looked yet.
    pub found: Option<Vec<Upd>>,
    /// The result of the last action as a line (an error, mostly).
    pub note: String,
    /// Downloading and installing: 0..=100 over both parts (downloads are the first 70).
    pub pct: f32,
    /// The updates a run works on.
    pub run: Vec<Upd>,
    pub failed: Option<Failure>,
    /// A restart is set for this time ("03:00").
    pub sched: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { phase: Phase::Idle, found: None, note: String::new(), pct: 0.0, run: Vec::new(), failed: None, sched: None });
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or_default()
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.lock().ok().map(|mut s| f(&mut s))
}

fn session() -> windows::core::Result<IUpdateSession> {
    // SAFETY: COM on this thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)
    }
}

/// The kind of an update from the categories Windows gives it, its title and whether it is a driver.
#[must_use]
pub fn categorize(cats: &[String], title: &str, driver: bool) -> Cat {
    let has = |w: &str| cats.iter().any(|c| c.to_lowercase().contains(w));
    let t = title.to_lowercase();
    if has("definition") || t.contains("security intelligence") {
        Cat::Definitions
    } else if driver || has("driver") {
        Cat::Driver
    } else if t.contains(".net") {
        Cat::Net
    } else if t.contains("preview") {
        Cat::Preview
    } else if has("security") || has("critical") {
        Cat::Security
    } else if t.contains("cumulative") {
        Cat::Cumulative
    } else {
        Cat::Other
    }
}

/// "KB5046617" out of a title that ends "(KB5046617)", else empty.
#[must_use]
pub fn kb_in(title: &str) -> String {
    let at = title.find("KB").filter(|&i| title[i + 2..].starts_with(|c: char| c.is_ascii_digit()));
    at.map_or_else(String::new, |i| format!("KB{}", title[i + 2..].chars().take_while(char::is_ascii_digit).collect::<String>()))
}

// The Update Agent wants a callback object for each of its asynchronous calls (a null one is "invalid pointer"); ours do nothing: the
// worker polls the job instead.
#[windows::core::implement(ISearchCompletedCallback)]
struct SearchDone;
impl ISearchCompletedCallback_Impl for SearchDone_Impl {
    fn Invoke(&self, _: windows::core::Ref<'_, ISearchJob>, _: windows::core::Ref<'_, ISearchCompletedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}

#[windows::core::implement(IDownloadProgressChangedCallback)]
struct DownloadStep;
impl IDownloadProgressChangedCallback_Impl for DownloadStep_Impl {
    fn Invoke(&self, _: windows::core::Ref<'_, IDownloadJob>, _: windows::core::Ref<'_, IDownloadProgressChangedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}

#[windows::core::implement(IDownloadCompletedCallback)]
struct DownloadDone;
impl IDownloadCompletedCallback_Impl for DownloadDone_Impl {
    fn Invoke(&self, _: windows::core::Ref<'_, IDownloadJob>, _: windows::core::Ref<'_, IDownloadCompletedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}

#[windows::core::implement(IInstallationProgressChangedCallback)]
struct InstallStep;
impl IInstallationProgressChangedCallback_Impl for InstallStep_Impl {
    fn Invoke(&self, _: windows::core::Ref<'_, IInstallationJob>, _: windows::core::Ref<'_, IInstallationProgressChangedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}

#[windows::core::implement(IInstallationCompletedCallback)]
struct InstallDone;
impl IInstallationCompletedCallback_Impl for InstallDone_Impl {
    fn Invoke(&self, _: windows::core::Ref<'_, IInstallationJob>, _: windows::core::Ref<'_, IInstallationCompletedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}

fn upd_of(u: &IUpdate) -> Option<Upd> {
    // SAFETY: COM objects read on the thread that made them.
    unsafe {
        let title = u.Title().ok()?.to_string();
        let id = u.Identity().ok()?.UpdateID().ok()?.to_string();
        let kbs = u.KBArticleIDs().ok();
        let kb = kbs.and_then(|k| if k.Count().unwrap_or(0) > 0 { k.get_Item(0).ok() } else { None }).map(|k| format!("KB{k}")).unwrap_or_else(|| kb_in(&title));
        let cats: Vec<String> = u.Categories().map(|c| (0..c.Count().unwrap_or(0)).filter_map(|i| c.get_Item(i).ok()).filter_map(|c| c.Name().ok()).map(|n| n.to_string()).collect()).unwrap_or_default();
        let driver = u.Type().is_ok_and(|t| t.0 == 2);
        let cat = categorize(&cats, &title, driver);
        let bytes = u.MaxDownloadSize().ok().map_or(0, |d| d.Anonymous2.Lo64);
        let restart = u.InstallationBehavior().and_then(|b| b.RebootBehavior()).is_ok_and(|r| r.0 != 0);
        let browse_only = windows::core::Interface::cast::<IUpdate3>(u).and_then(|u| u.BrowseOnly()).is_ok_and(|b| b.as_bool());
        let optional = browse_only || matches!(cat, Cat::Driver | Cat::Preview);
        Some(Upd { id, title, kb, cat, mb: ((bytes + 524_288) / 1_048_576) as u32, restart, optional })
    }
}

fn err_text(e: &windows::core::Error) -> String {
    if e.code().0 as u32 == 0x8007_0005 {
        t!("Нужны права администратора", "Administrator rights are needed", ja: "管理者権限が必要です", pt: "São necessários direitos de administrador", zh: "需要管理员权限").into()
    } else {
        e.message()
    }
}

fn code_text(e: &windows::core::Error) -> String {
    format!("0x{:08x}", e.code().0 as u32)
}

fn start(phase: Phase, job: impl FnOnce() + Send + 'static, done: impl FnOnce() + Send + 'static) {
    let mut busy = false;
    with(|s| {
        busy = s.phase != Phase::Idle;
        if !busy {
            (s.phase, s.pct, s.failed) = (phase, 0.0, None);
        }
    });
    if busy {
        return;
    }
    CANCEL.store(false, Relaxed);
    std::thread::spawn(move || {
        job();
        with(|s| s.phase = Phase::Idle);
        done();
    });
}

/// Asks the run that is going on to stop (a search, a download; an installation cannot be taken back).
pub fn cancel() {
    CANCEL.store(true, Relaxed);
}

fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(400));
}

/// Looks for updates (optional ones too).
pub fn check(done: impl FnOnce() + Send + 'static) {
    start(Phase::Checking, || {
        let run = || -> windows::core::Result<Vec<Upd>> {
            // SAFETY: COM objects made and used on this thread.
            unsafe {
                let s = session()?;
                let searcher = s.CreateUpdateSearcher()?;
                let job = {
                    let cb: ISearchCompletedCallback = SearchDone.into();
                    searcher.BeginSearch(&BSTR::from("IsInstalled=0 and IsHidden=0"), &cb, &VARIANT::default())?
                };
                while !job.IsCompleted()?.as_bool() {
                    if CANCEL.load(Relaxed) {
                        let _ = job.RequestAbort();
                        return Ok(Vec::new());
                    }
                    pause();
                }
                let r = searcher.EndSearch(&job)?;
                let c = r.Updates()?;
                Ok((0..c.Count()?).filter_map(|i| c.get_Item(i).ok()).filter_map(|u| upd_of(&u)).collect())
            }
        };
        let cancelled = CANCEL.load(Relaxed);
        match run() {
            Ok(list) if !(cancelled && list.is_empty()) => {
                CHECKED.store(now_unix(), Relaxed);
                with(|s| (s.found, s.note) = (Some(list), String::new()));
            }
            Ok(_) => {}
            Err(e) => {
                with(|s| s.note = tf!("Не удалось проверить: {}", "Could not check: {}", ja: "確認できませんでした: {}", pt: "Não foi possível verificar: {}", zh: "无法检查：{}", err_text(&e)));
            }
        }
        INFO_AT.store(0, Relaxed); // the dates Windows keeps are read again
    }, done);
}

/// Downloads and installs the updates with these ids (the ticked ones); progress goes to `state()`.
pub fn install(ids: Vec<String>, done: impl FnOnce() + Send + 'static) {
    let list: Vec<Upd> = state().found.unwrap_or_default().into_iter().filter(|u| ids.contains(&u.id)).collect();
    if list.is_empty() {
        return;
    }
    with(|s| s.run = list.clone());
    start(Phase::Downloading, move || {
        let run = || -> Result<bool, (windows::core::Error, Option<Upd>)> {
            // SAFETY: COM objects made and used on this thread.
            unsafe {
                let e0 = |e: windows::core::Error| (e, None);
                let s = session().map_err(e0)?;
                // the same search again: the updates are COM objects of the thread that found them
                let found = s.CreateUpdateSearcher().and_then(|x| x.Search(&BSTR::from("IsInstalled=0 and IsHidden=0"))).and_then(|r| r.Updates()).map_err(e0)?;
                let pick = CoCreateInstance::<_, IUpdateCollection>(&UpdateCollection, None, CLSCTX_INPROC_SERVER).map_err(e0)?;
                for i in 0..found.Count().map_err(e0)? {
                    let u = found.get_Item(i).map_err(e0)?;
                    if u.Identity().and_then(|i| i.UpdateID()).is_ok_and(|id| ids.contains(&id.to_string())) {
                        if !u.EulaAccepted().is_ok_and(|b| b.as_bool()) {
                            u.AcceptEula().map_err(e0)?;
                        }
                        pick.Add(&u).map_err(e0)?;
                    }
                }
                let d = s.CreateUpdateDownloader().map_err(e0)?;
                d.SetUpdates(&pick).map_err(e0)?;
                let job = {
                    let (p, c): (IDownloadProgressChangedCallback, IDownloadCompletedCallback) = (DownloadStep.into(), DownloadDone.into());
                    d.BeginDownload(&p, &c, &VARIANT::default()).map_err(e0)?
                };
                while !job.IsCompleted().map_err(e0)?.as_bool() {
                    if CANCEL.load(Relaxed) {
                        let _ = job.RequestAbort();
                        pause();
                        return Ok(false);
                    }
                    if let Ok(p) = job.GetProgress().and_then(|p| p.PercentComplete()) {
                        with(|s| s.pct = p as f32 * 0.7);
                    }
                    pause();
                }
                d.EndDownload(&job).map_err(e0)?;
                with(|s| (s.phase, s.pct) = (Phase::Installing, 70.0));
                let inst = s.CreateUpdateInstaller().map_err(e0)?;
                inst.SetUpdates(&pick).map_err(e0)?;
                let job = {
                    let (p, c): (IInstallationProgressChangedCallback, IInstallationCompletedCallback) = (InstallStep.into(), InstallDone.into());
                    inst.BeginInstall(&p, &c, &VARIANT::default()).map_err(e0)?
                };
                while !job.IsCompleted().map_err(e0)?.as_bool() {
                    if let Ok(p) = job.GetProgress().and_then(|p| p.PercentComplete()) {
                        with(|s| s.pct = 70.0 + p as f32 * 0.3);
                    }
                    pause();
                }
                let r = inst.EndInstall(&job).map_err(e0)?;
                // an update that did not install: which, and the code Windows gave
                for i in 0..pick.Count().unwrap_or(0) {
                    if let Ok(ur) = r.GetUpdateResult(i)
                        && ur.ResultCode().is_ok_and(|c| c.0 >= 4)
                        && let Some(u) = pick.get_Item(i).ok().and_then(|u| upd_of(&u))
                    {
                        let code = windows::core::Error::from_hresult(windows::core::HRESULT(ur.HResult().unwrap_or(0)));
                        return Err((code, Some(u)));
                    }
                }
                Ok(true)
            }
        };
        match run() {
            Ok(true) => {
                with(|s| {
                    s.found = s.found.take().map(|f| f.into_iter().filter(|u| !ids.contains(&u.id)).collect());
                    s.pct = 100.0;
                });
                INFO_AT.store(0, Relaxed);
            }
            Ok(false) => drop(with(|s| s.pct = 0.0)),
            Err((e, u)) => {
                let (title, kb) = u.map_or((String::new(), String::new()), |u| (u.title, u.kb));
                with(|s| s.failed = Some(Failure { title, kb, code: code_text(&e), text: err_text(&e) }));
            }
        }
    }, done);
}

// ---------------------------------------------------------------------------------------------------------------------
// what Windows knows besides a search of our own: a restart is waiting, when it last looked, when it last installed, the history

/// Seconds since 1970 (UTC).
type Unix = i64;

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Info {
    pub restart: bool,
    /// Windows' own last successful search and installation.
    pub search: Option<Unix>,
    pub install: Option<Unix>,
    /// Our last successful search in this session.
    pub checked: Option<Unix>,
    pub history: Vec<(HGroup, String, String, Unix, bool, String)>,
    /// The hours Windows will not restart in (start, end).
    pub active: Option<(u32, u32)>,
}

static INFO: Mutex<Option<Info>> = Mutex::new(None);
static INFO_AT: AtomicI64 = AtomicI64::new(0);
static READING: AtomicBool = AtomicBool::new(false);
static CHECKED: AtomicI64 = AtomicI64::new(0);

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
    use windows::Win32::System::Variant::{VAR_CHANGE_FLAGS, VT_R8, VariantChangeType};
    let date = |v: windows::core::Result<VARIANT>| {
        // the date comes as a VT_DATE: as a double it is the OLE date itself
        let v = v.ok()?;
        let mut out = VARIANT::default();
        // SAFETY: a plain conversion into a local.
        unsafe { VariantChangeType(&mut out, &v, VAR_CHANGE_FLAGS(0), VT_R8) }.ok()?;
        f64::try_from(&out).ok().and_then(unix_of)
    };
    // SAFETY: COM objects made and used on this thread.
    let (search, install) = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance::<_, IAutomaticUpdates2>(&AutomaticUpdates, None, CLSCTX_INPROC_SERVER).ok().and_then(|a| a.Results().ok()).map_or((None, None), |r| (date(r.LastSearchSuccessDate()), date(r.LastInstallationSuccessDate())))
    };
    let history = (|| -> windows::core::Result<_> {
        // SAFETY: COM objects made and used on this thread.
        unsafe {
            let searcher = session()?.CreateUpdateSearcher()?;
            let n = searcher.GetTotalHistoryCount()?.min(60);
            let h = searcher.QueryHistory(0, n)?;
            Ok((0..h.Count()?)
                .filter_map(|i| h.get_Item(i).ok())
                .filter_map(|e| {
                    let title = e.Title().ok()?.to_string();
                    let when = e.Date().ok().and_then(unix_of)?;
                    let ok = e.ResultCode().ok()?.0 == 2;
                    let code = if ok { String::new() } else { format!("0x{:08x}", e.HResult().unwrap_or(0) as u32) };
                    let low = title.to_lowercase();
                    let group = if low.contains("security intelligence") || low.contains("definition") {
                        HGroup::Definition
                    } else if low.contains("cumulative") || low.contains("security update") || low.contains("servicing stack") {
                        HGroup::Quality
                    } else if low.contains("driver") || title.contains(" \u{2013} ") || title.contains(" - ") && !low.contains("windows") {
                        HGroup::Driver
                    } else {
                        HGroup::Other
                    };
                    Some((group, title.clone(), kb_in(&title), when, ok, code))
                })
                .collect::<Vec<_>>())
        }
    })()
    .unwrap_or_default();
    let ux = r"SOFTWARE\Microsoft\WindowsUpdate\UX\Settings";
    let hr = |n: &str| crate::sys::dword(windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE, ux, n);
    Info { restart: reboot_pending(), search, install, checked: None, history, active: hr("ActiveHoursStart").zip(hr("ActiveHoursEnd")) }
}

/// What is known now; the reading itself (COM calls that can take a second) runs in a thread and is repeated at most every 20 s.
pub fn info() -> Info {
    let fresh = now_unix() - INFO_AT.load(Relaxed) < 20;
    if !fresh && !READING.swap(true, Relaxed) {
        std::thread::spawn(|| {
            let i = read_info();
            if let Ok(mut g) = INFO.lock() {
                *g = Some(i);
            }
            INFO_AT.store(now_unix(), Relaxed);
            READING.store(false, Relaxed);
        });
    }
    let mut i = INFO.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
    // our own search can be newer than anything Windows wrote down
    i.checked = Some(CHECKED.load(Relaxed)).filter(|&t| t > 0);
    i
}

/// A restart in `secs` seconds (`shutdown /r /t`), so the user can leave; `cancel_restart` takes it back. Fixed arguments, a number only.
pub fn schedule_restart(secs: u32, at: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let exe = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_default()).join(r"System32\shutdown.exe");
    std::process::Command::new(exe).args(["/r", "/t", &secs.min(315_360_000).to_string(), "/c", "Windows Update"]).creation_flags(0x0800_0000).status().is_ok_and(|s| s.success()).then(|| with(|s| s.sched = Some(at.to_string()))).is_some()
}

pub fn cancel_restart() -> bool {
    use std::os::windows::process::CommandExt;
    let exe = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_default()).join(r"System32\shutdown.exe");
    let ok = std::process::Command::new(exe).arg("/a").creation_flags(0x0800_0000).status().is_ok_and(|s| s.success());
    with(|s| s.sched = None);
    ok
}

/// The names of the apps with a window open now (what a restart would close): the exe's own name, once each.
pub fn open_apps() -> Vec<String> {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumWindows, GWL_EXSTYLE, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW},
    };
    extern "system" fn each(h: HWND, l: LPARAM) -> i32 {
        // SAFETY: plain window queries; `l` is the vector below.
        unsafe {
            if IsWindowVisible(h) != 0 && GetWindowTextLengthW(h) > 0 && GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, &mut pid);
                (*(l as *mut Vec<u32>)).push(pid);
            }
        }
        1
    }
    let mut pids: Vec<u32> = Vec::new();
    // SAFETY: the callback only runs inside this call.
    unsafe { EnumWindows(Some(each), &mut pids as *mut Vec<u32> as LPARAM) };
    let me = std::process::id();
    let mut names: Vec<String> = Vec::new();
    let by_pid: HashMap<u32, String> = winrust_common::proc::list().into_iter().collect();
    for p in pids.into_iter().filter(|&p| p != me) {
        if let Some(n) = by_pid.get(&p) {
            let n = crate::more::app_name(n);
            if !names.contains(&n) && !n.eq_ignore_ascii_case("explorer") {
                names.push(n);
            }
        }
    }
    names
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
        let ok = LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) != 0;
        let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
        let ok = ok && AdjustTokenPrivileges(tok, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0;
        CloseHandle(tok);
        ok && ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OPERATINGSYSTEM | SHTDN_REASON_MINOR_UPGRADE | SHTDN_REASON_FLAG_PLANNED) != 0
    }
}

/// When a restart is wanted: at a time of the day, or after some minutes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sched {
    At(u32, u32),
    In(u32),
}

/// Seconds to wait and the time of day it ends at ("03:00"), from the clock now (hour, minute).
#[must_use]
pub fn plan(now: (u32, u32), s: Sched) -> (u32, String) {
    let at = match s {
        Sched::At(h, m) => (h * 60 + m) % 1440,
        Sched::In(min) => (now.0 * 60 + now.1 + min) % 1440,
    };
    let secs = match s {
        Sched::In(min) => min * 60,
        Sched::At(..) => ((at as i32 - (now.0 * 60 + now.1) as i32).rem_euclid(1440) as u32).max(1) * 60,
    };
    (secs, format!("{:02}:{:02}", at / 60, at % 60))
}

fn local_hm() -> (u32, u32) {
    use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};
    // SAFETY: a plain clock read into a local.
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    (u32::from(t.wHour), u32::from(t.wMinute))
}

/// Sets a restart for `s` (the clock now decides how long); true when Windows took it.
pub fn schedule(s: Sched) -> bool {
    let (secs, at) = plan(local_hm(), s);
    schedule_restart(secs, &at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_at_a_time_waits_until_its_next_turn() {
        assert_eq!(plan((17, 46), Sched::At(3, 0)), (9 * 3600 + 14 * 60, "03:00".into()), "tonight at 03:00 is nine hours and fourteen minutes away");
        assert_eq!(plan((17, 46), Sched::In(60)), (3600, "18:46".into()));
        assert_eq!(plan((23, 30), Sched::In(60)), (3600, "00:30".into()), "past midnight");
        assert_eq!(plan((22, 0), Sched::At(22, 0)).0, 60, "this very minute is the next minute, never zero (zero would restart now)");
    }

    #[test]
    fn the_kind_of_an_update_comes_from_its_categories_and_title() {
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(categorize(&c(&["Definition Updates"]), "Security Intelligence Update for Microsoft Defender Antivirus", false), Cat::Definitions);
        assert_eq!(categorize(&c(&["Drivers"]), "NVIDIA \u{2013} Display", true), Cat::Driver);
        assert_eq!(categorize(&c(&["Security Updates"]), "2026-11 Cumulative Update for .NET Framework 3.5 and 4.8.1 (KB5045934)", false), Cat::Net);
        assert_eq!(categorize(&c(&["Updates"]), "2026-11 Cumulative Update Preview for Windows 11 (KB5046740)", false), Cat::Preview);
        assert_eq!(categorize(&c(&["Security Updates"]), "2026-11 Cumulative Update for Windows 11 Version 24H2 (KB5046617)", false), Cat::Security);
        assert_eq!(categorize(&c(&["Updates"]), "2026-11 Cumulative Update for Windows 11 (KB1)", false), Cat::Cumulative);
        assert_eq!(categorize(&[], "Something else", false), Cat::Other);
    }

    #[test]
    fn a_kb_number_is_read_from_the_title() {
        assert_eq!(kb_in("2026-11 Cumulative Update for Windows 11 Version 24H2 (KB5046617)"), "KB5046617");
        assert_eq!(kb_in("NVIDIA \u{2013} Display \u{2013} 32.0.15.6614"), "");
        assert_eq!(kb_in("Update (KBx)"), "");
    }

    /// Reads only, by hand: `cargo test -p rust-settings --release --offline -- --ignored print_info --nocapture` (nothing is installed or restarted).
    #[test]
    #[ignore = "reads the real machine"]
    fn print_info() {
        let i = read_info();
        println!("restart {} search {:?} install {:?} active {:?} history {}", i.restart, i.search, i.install, i.active, i.history.len());
        for h in i.history.iter().take(6) {
            println!("   {:?} {} | {} | {} | ok={} {}", h.0, h.1, h.2, h.3, h.4, h.5);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        check(move || {
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(std::time::Duration::from_secs(120));
        let s = state();
        println!("phase {:?} note {:?}", s.phase, s.note);
        for u in s.found.unwrap_or_default() {
            println!("   {:?} | {} | {} | {} MB | restart {} | optional {} | {}", u.cat, u.kb, u.title, u.mb, u.restart, u.optional, u.id);
        }
        println!("open apps: {:?}", open_apps());
    }
}
