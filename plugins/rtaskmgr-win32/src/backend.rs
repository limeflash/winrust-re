//! What the Task Manager asks of the machine, and what it asks of Windows around its window: reading the system, ending processes,
//! changing priorities and services and startup entries, the tray icon, the hotkey, the settings file. The window talks to a
//! [`Machine`], so a render, a benchmark, a self-test and a test run it on a fake that touches nothing; only [`Real`] ever reads
//! this computer or changes it.
use std::sync::Mutex;

use crate::{boost, data::Data, net, startup, store, sys, tray::Tray};

/// A row of the tray icon's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub label: String,
    /// What comes back as `wp` of [`MSG_TRAY_CMD`] when it is chosen.
    pub code: usize,
    pub checked: bool,
    pub sep: bool,
}

impl TrayItem {
    #[must_use]
    pub fn new(label: impl Into<String>, code: usize, checked: bool) -> Self {
        Self { label: label.into(), code, checked, sep: false }
    }
    #[must_use]
    pub fn sep() -> Self {
        Self { label: String::new(), code: 0, checked: false, sep: true }
    }
}

/// Messages the app's helper windows (the tray's, the hotkey's) post to the main one: `App::message` gets them.
pub const MSG_START: u32 = 0x8000 + 1;
/// The tray icon was clicked: show the window.
pub const MSG_TRAY_SHOW: u32 = 0x8000 + 2;
/// An item of the tray menu was chosen: `wp` is its `code`.
pub const MSG_TRAY_CMD: u32 = 0x8000 + 3;
/// The hotkey (Ctrl+Alt+T): show the window.
pub const MSG_HOTKEY: u32 = 0x8000 + 4;

pub trait Machine: Send + Sync {
    // ---- what this computer is doing
    /// One tick of the numbers; what the rules said comes back.
    fn refresh(&self, d: &mut Data) -> Vec<String>;
    fn load_startup(&self, d: &mut Data);
    fn load_history(&self, d: &mut Data);
    fn boost_state(&self, id: boost::Id, admin: bool) -> boost::State;
    /// What the actions of the Speed page that ran in the background have finished with.
    fn boost_done(&self) -> Vec<String>;
    fn self_autostart(&self) -> bool;
    fn system_dark(&self) -> bool;

    // ---- changes: every one of them acts on the real system in [`Real`]
    fn kill(&self, d: &Data, pid: u32) -> bool;
    fn set_priority(&self, pid: u32, class: u32) -> bool;
    /// The priority class a process has now, as the menu names it.
    fn priority_name(&self, pid: u32) -> String;
    fn suspend(&self, pid: u32, on: bool) -> bool;
    fn set_efficiency(&self, pid: u32, on: bool) -> bool;
    fn set_affinity(&self, pid: u32, half: bool) -> bool;
    fn service_ctl(&self, name: &str, start: bool) -> Result<(), u32>;
    fn startup_set(&self, it: &startup::Item, on: bool) -> Result<(), u32>;
    fn boost_start(&self, id: boost::Id, on: bool, donors: Vec<u32>) -> Result<(), String>;
    fn reveal(&self, path: &str);
    /// The system's "Run" dialog.
    fn run_dialog(&self);
    /// Starts this program again with administrator rights (UAC); whether the user agreed.
    fn relaunch_admin(&self) -> bool;
    fn set_self_autostart(&self, on: bool) -> bool;
    fn etw_start(&self) -> Option<net::Session>;
    fn etw_stop(&self, s: net::Session);
    fn save_settings(&self, cfg: &store::Settings);
    /// Deletes the usage history (the program's own file).
    fn clear_history(&self);
    /// Writes `text` as `file` on the user's Desktop; where it went.
    fn export(&self, file: &str, text: &str) -> Result<String, String>;

    // ---- the window and the tray
    fn window_hidden(&self, hwnd: isize) -> bool;
    fn show_window(&self, hwnd: isize);
    fn hide_window(&self, hwnd: isize);
    /// The tray icon, with the hotkey: only a program that lives in the tray makes them.
    fn tray_add(&self, hwnd: isize);
    fn tray_menu(&self, items: Vec<TrayItem>);
    fn tray_balloon(&self, msg: &str);
    fn tray_remove(&self);
}

/// This computer.
#[derive(Default)]
pub struct Real {
    tray: Mutex<Option<Tray>>,
}

impl Real {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Machine for Real {
    fn refresh(&self, d: &mut Data) -> Vec<String> {
        d.refresh()
    }
    fn load_startup(&self, d: &mut Data) {
        d.load_startup();
    }
    fn load_history(&self, d: &mut Data) {
        d.load_history();
    }
    fn boost_state(&self, id: boost::Id, admin: bool) -> boost::State {
        boost::state(id, admin)
    }
    fn boost_done(&self) -> Vec<String> {
        boost::take_done()
    }
    fn self_autostart(&self) -> bool {
        startup::self_autostart()
    }
    fn system_dark(&self) -> bool {
        sys::system_dark()
    }

    fn kill(&self, d: &Data, pid: u32) -> bool {
        d.sys.process(sysinfo::Pid::from_u32(pid)).is_some_and(|x| x.kill())
    }
    fn set_priority(&self, pid: u32, class: u32) -> bool {
        sys::set_priority(pid, class)
    }
    fn priority_name(&self, pid: u32) -> String {
        sys::priority_name(pid)
    }
    fn suspend(&self, pid: u32, on: bool) -> bool {
        sys::suspend(pid, on)
    }
    fn set_efficiency(&self, pid: u32, on: bool) -> bool {
        sys::set_efficiency(pid, on)
    }
    fn set_affinity(&self, pid: u32, half: bool) -> bool {
        sys::set_affinity(pid, half)
    }
    fn service_ctl(&self, name: &str, start: bool) -> Result<(), u32> {
        sys::service_ctl(name, start)
    }
    fn startup_set(&self, it: &startup::Item, on: bool) -> Result<(), u32> {
        startup::set_enabled(it, on)
    }
    fn boost_start(&self, id: boost::Id, on: bool, donors: Vec<u32>) -> Result<(), String> {
        boost::start(id, on, donors)
    }
    fn reveal(&self, path: &str) {
        winrust_common::reveal(path);
    }
    fn run_dialog(&self) {
        let _ = std::process::Command::new("explorer.exe").arg("shell:::{2559a1f3-21d7-11d4-bdaf-00c04f60b9f0}").spawn();
    }
    fn relaunch_admin(&self) -> bool {
        sys::relaunch_as_admin("")
    }
    fn set_self_autostart(&self, on: bool) -> bool {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        startup::set_self_autostart(on, &exe)
    }
    fn etw_start(&self) -> Option<net::Session> {
        net::start()
    }
    fn etw_stop(&self, s: net::Session) {
        net::stop(s);
    }
    fn save_settings(&self, cfg: &store::Settings) {
        store::save(cfg);
    }
    fn clear_history(&self) {
        store::clear_history();
    }
    fn export(&self, file: &str, text: &str) -> Result<String, String> {
        let dir = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_default().join("Desktop");
        let path = dir.join(file);
        std::fs::write(&path, text).map(|()| path.display().to_string()).map_err(|e| e.to_string())
    }

    fn window_hidden(&self, hwnd: isize) -> bool {
        use winapi::um::winuser::{IsIconic, IsWindowVisible};
        // SAFETY: plain queries on our own window.
        hwnd == 0 || unsafe { IsWindowVisible(hwnd as _) == 0 || IsIconic(hwnd as _) != 0 }
    }
    fn show_window(&self, hwnd: isize) {
        use winapi::um::winuser::{SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow};
        // SAFETY: calls on our own window.
        unsafe {
            ShowWindow(hwnd as _, SW_SHOW);
            ShowWindow(hwnd as _, SW_RESTORE);
            SetForegroundWindow(hwnd as _);
        }
    }
    fn hide_window(&self, hwnd: isize) {
        use winapi::um::winuser::{SW_HIDE, ShowWindow};
        // SAFETY: a call on our own window.
        unsafe { ShowWindow(hwnd as _, SW_HIDE) };
    }
    fn tray_add(&self, hwnd: isize) {
        let mut t = self.tray.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if t.is_none() {
            *t = Tray::create(hwnd, true);
        }
    }
    fn tray_menu(&self, items: Vec<TrayItem>) {
        if let Some(t) = self.tray.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            t.set_menu(items);
        }
    }
    fn tray_balloon(&self, msg: &str) {
        if let Some(t) = self.tray.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            t.balloon(t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器"), msg);
        }
    }
    fn tray_remove(&self) {
        // dropping it takes the icon and the hotkey away
        self.tray.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    }
}
