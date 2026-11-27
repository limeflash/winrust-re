//! Everything the window reads from or does to the system, behind one trait. The window only ever talks to [`os()`]: the real one
//! ([`Live`], thin calls into display.rs, audio.rs, bt.rs, power.rs, sys.rs, wu.rs, net.rs, ext.rs and wifi.rs) or the demo one
//! (`demo::Demo`: the mockup's data, changes kept in memory, every call written to a log). `--render`, `--bench`, `--selftest` and the
//! tests run on the demo one, so none of them can change a setting or read anything of the machine.
//!
//! Under `cargo test` the default is the demo one too, so a test that forgets to pick a source still touches nothing.
use std::{cell::RefCell, rc::Rc};

use crate::{
    audio, bt, display, ext,
    more::{Assoc, Cap, Consent, DefApp, DefKind, Flag, Gauge, Layout, NotifyApp, Opt, Security},
    net, power, sys, sysx, wifi, wu,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// What a worker thread runs when it is done (the window then reads the page again).
pub type Ping = Box<dyn FnOnce() + Send>;
/// What a worker thread tells the window when it is done (a line for the toast).
pub type Note = Box<dyn FnOnce(String) + Send>;

/// The battery as the Power page shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub pct: u8,
    pub ac: bool,
    pub charging: bool,
    /// Seconds of charge left on battery, when Windows says.
    pub left: Option<u32>,
}

/// What the Windows Update row shows beyond `wu::State`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WuExtra {
    pub progress: Option<u8>,
    /// Windows needs a restart to finish updates.
    pub restart: bool,
    /// When Windows (or this window) last looked for updates, and when it last installed some.
    pub last_check: Option<crate::more::When>,
    pub last_install: Option<crate::more::When>,
}

pub trait Os {
    // ---- display
    fn display_current(&self) -> Option<display::Mode>;
    fn display_modes(&self) -> Vec<display::Mode>;
    fn display_orientation(&self) -> Option<u32>;
    fn display_set_orientation(&self, o: u32, save: bool) -> bool;
    fn display_apply(&self, m: display::Mode, save: bool) -> bool;
    fn display_revert(&self);
    fn display_make_primary(&self) -> bool;
    fn display_topology(&self) -> Option<display::Topo>;
    fn display_connected(&self) -> usize;
    fn display_set_topology(&self, t: display::Topo) -> bool;
    fn display_scales(&self) -> Option<(Vec<u32>, usize)>;
    fn display_scale(&self) -> Option<u32>;
    fn display_set_scale_index(&self, i: usize) -> bool;
    fn display_hdr(&self) -> Option<bool>;
    fn display_set_hdr(&self, on: bool) -> bool;
    fn display_layout(&self) -> Vec<display::Mon>;
    fn display_move_monitor(&self, i: usize, x: i32, y: i32) -> bool;
    fn display_monitor_info(&self) -> Option<(usize, usize, String, bool)>;
    fn display_monitor_names(&self) -> Vec<String>;
    fn display_selected(&self) -> usize;
    fn display_select(&self, i: usize);

    // ---- sound
    fn audio_devices(&self, mic: bool) -> Vec<audio::Dev>;
    fn audio_default_name(&self, mic: bool) -> String;
    fn audio_set_default(&self, id: &str) -> bool;
    fn audio_apps(&self) -> Vec<(u32, String)>;
    fn audio_volume(&self, src: audio::Src) -> Option<f32>;
    fn audio_set_volume(&self, src: audio::Src, v: f32);
    fn audio_muted(&self, mic: bool) -> Option<bool>;
    fn audio_set_mute(&self, mic: bool, m: bool);

    // ---- Bluetooth
    fn bt_state(&self) -> bt::State;
    fn bt_refresh(&self, done: Ping);
    fn bt_set_bluetooth(&self, on: bool, done: Ping);
    fn bt_set_airplane(&self, on: bool, done: Ping);
    fn bt_scan(&self, done: Ping);
    fn bt_pair(&self, id: String, done: Ping);
    fn bt_unpair(&self, id: String, done: Ping);
    fn bt_take_note(&self) -> Option<String>;

    // ---- power and storage
    fn power_battery(&self) -> Option<Battery>;
    fn power_drives(&self) -> Vec<(char, u64, u64)>;
    fn power_plan_text(&self) -> String;
    fn power_plans(&self) -> (Vec<String>, Option<usize>);
    fn power_set_plan(&self, i: usize);
    fn power_timeouts(&self, t: power::Timeout) -> (Vec<String>, Option<usize>);
    fn power_timeout_text(&self, t: power::Timeout) -> String;
    fn power_set_timeout(&self, t: power::Timeout, i: usize);
    fn temp_text(&self) -> String;
    fn temp_measured(&self) -> bool;
    fn temp_measure(&self, done: Ping);
    fn temp_clean(&self, done: Note);
    fn bin_bytes(&self) -> u64;
    fn bin_empty(&self, owner: isize) -> bool;

    // ---- network
    fn net_adapters(&self) -> Vec<net::Adapter>;
    fn wifi_radio(&self) -> Option<(bool, bool)>;
    fn wifi_list(&self) -> Vec<wifi::Net>;
    fn wifi_scan(&self);
    fn wifi_set_radio(&self, on: bool);
    fn wifi_connect(&self, profile: &str, iface: &windows_sys::core::GUID) -> bool;
    fn wifi_disconnect(&self) -> bool;
    fn wifi_forget(&self, profile: &str) -> bool;
    /// The password goes into the profile Windows keeps, nowhere else.
    fn wifi_add_and_connect(&self, ssid: &str, raw: &[u8], sec: wifi::Sec, key: &str, hidden: bool, iface: Option<&windows_sys::core::GUID>) -> Result<String, String>;
    #[allow(clippy::too_many_arguments)]
    fn wifi_add_eap_and_connect(&self, ssid: &str, raw: &[u8], auth: &str, user: &str, pass: &str, hidden: bool, iface: Option<&windows_sys::core::GUID>) -> Result<String, String>;
    fn vpns(&self) -> Vec<ext::Vpn>;
    /// Connects a VPN (the real one waits for the modem layer, so it runs off the window's thread); `done` gets the line to show.
    fn vpn_dial(&self, name: String, pbk: Option<String>, done: Note);
    fn vpn_hang_up(&self, name: String, done: Note);
    fn proxy_text(&self) -> String;
    fn pac_text(&self) -> String;
    fn proxy_set(&self, pac: bool, v: &str) -> bool;
    fn proxy_changed(&self);

    // ---- apps, update
    fn programs(&self) -> Vec<ext::App>;
    fn uninstall(&self, cmd: &str) -> bool;
    fn wu_state(&self) -> wu::State;
    fn wu_extra(&self) -> WuExtra;
    /// Restarts the PC now (the user confirmed); false when Windows refused.
    fn restart_now(&self) -> bool;
    fn wu_check(&self, done: Ping);
    fn wu_install(&self, done: Ping);

    // ---- personalization, about
    fn night_kelvin(&self) -> u32;
    fn night_text(&self) -> String;
    fn night_set(&self, k: u32);
    fn reg_dword(&self, key: &str, name: &str) -> Option<u32>;
    fn reg_set_dword(&self, key: &str, name: &str, v: u32) -> bool;
    fn announce_theme(&self);
    fn wallpaper_name(&self) -> String;
    fn wallpaper_pick(&self, owner: isize) -> Result<(), ()>;
    /// The pictures the Personalization page offers besides the picker (the demo's).
    fn wallpapers(&self) -> Vec<crate::Wall>;
    /// The product / build values of `HKLM\...\Windows NT\CurrentVersion`.
    fn windows_value(&self, name: &str) -> String;
    fn ram_text(&self) -> String;
    fn computer_name(&self) -> String;
    /// Processor, memory, graphics, system type, build revision, WinRust version, a rename waiting for a restart.
    fn about_info(&self) -> crate::more::AboutInfo;
    /// The apps Windows has registered for a kind of file or link and the index of the default (None: nothing is linked).
    fn assoc_apps(&self, a: &Assoc) -> (Vec<DefApp>, Option<usize>);
    /// The user chose app `i` of `assoc_apps`: Windows' own chooser opens so they confirm it (the real one never sets the default itself);
    /// `done` says the dialog closed.
    fn assoc_pick(&self, a: &Assoc, i: usize, owner: isize, done: Ping);
    /// What WinRust's own file manager or terminal is here (not a Windows setting).
    fn own_default(&self, k: DefKind) -> String;
    /// Sets the computer's name (applies after a restart); the error is a line for the user.
    fn rename_pc(&self, name: &str) -> Result<(), String>;
    fn user_name(&self) -> String;
    fn set_lang(&self, l: winrust_common::Lang) -> bool;
    /// Picks the demo's picture `k` of the Personalization page (nothing in the real one: it has only the picker).
    /// Picks tile `k` of `wallpapers()`; a preset is drawn and set off the window's thread and `done` says what came of it.
    fn wallpaper_select(&self, k: usize, done: Note);
    /// Decodes the thumbnail of the picture that is the wallpaper (off the window's thread); `done` says it is ready.
    fn wall_prepare(&self, done: Ping);
    /// The WinRust accent now (an index of `style::Accent::ALL`) and setting it (the bar and every program follow).
    fn accent(&self) -> usize;
    fn accent_set(&self, i: usize);

    // ---- Notifications, Privacy & security, Keyboard & mouse, Date & time (more.rs)
    /// The entries of a list and the current one.
    fn opt_list(&self, o: Opt) -> (Vec<String>, Option<usize>);
    /// Picks entry `i`; false when Windows did not take it (usually: it needs administrator rights).
    fn opt_set(&self, o: Opt, i: usize) -> bool;
    fn gauge(&self, g: Gauge) -> Option<f32>;
    fn gauge_set(&self, g: Gauge, v: f32);
    fn flag(&self, f: &Flag) -> Option<bool>;
    fn flag_set(&self, f: &Flag, on: bool) -> bool;
    fn notify_apps(&self) -> Vec<NotifyApp>;
    fn notify_set(&self, id: &str, mode: usize) -> bool;
    fn consent(&self, cap: Cap) -> Consent;
    fn security(&self) -> Security;
    /// Starts Defender's quick scan off the window's thread; `done` says it ended.
    fn quick_scan(&self, done: Ping);
    fn open_security(&self);
    fn layouts(&self) -> Vec<Layout>;
    /// The bar's clock as the formats say it now.
    fn clock_text(&self) -> String;
    fn region_text(&self) -> String;
    fn numbers_text(&self) -> String;
    /// The line under "Sync now".
    fn time_server(&self) -> String;
    fn time_sync(&self, done: Note);

    // ---- the window
    /// Called once at the start of a normal run (COM for the volume).
    fn init(&self);
    /// The window exists: the Wi-Fi events are posted to it from now on.
    fn start(&self, hwnd: isize);
    /// Shows the number of every display on it for a moment (the Identify button).
    fn identify(&self, mons: &[display::Mon]);
}

thread_local! {
    static OS: RefCell<Option<Rc<dyn Os>>> = const { RefCell::new(None) };
}

/// The source this thread's window uses.
#[must_use]
pub fn os() -> Rc<dyn Os> {
    OS.with(|o| {
        o.borrow_mut()
            .get_or_insert_with(|| if cfg!(test) { Rc::new(crate::demo::Demo::new()) as Rc<dyn Os> } else { Rc::new(Live) })
            .clone()
    })
}

/// Picks the source of this thread (the renderers and the tests pick the demo one, a normal start the real one).
pub fn set_os(o: Rc<dyn Os>) {
    OS.with(|s| *s.borrow_mut() = Some(o));
}

/// The real system.
pub struct Live;

fn hw(owner: isize) -> HWND {
    owner as HWND
}

impl Os for Live {
    fn display_current(&self) -> Option<display::Mode> {
        display::current()
    }
    fn display_modes(&self) -> Vec<display::Mode> {
        display::modes()
    }
    fn display_orientation(&self) -> Option<u32> {
        display::orientation()
    }
    fn display_set_orientation(&self, o: u32, save: bool) -> bool {
        display::set_orientation(o, save)
    }
    fn display_apply(&self, m: display::Mode, save: bool) -> bool {
        display::apply(m, save)
    }
    fn display_revert(&self) {
        display::revert();
    }
    fn display_make_primary(&self) -> bool {
        display::make_primary()
    }
    fn display_topology(&self) -> Option<display::Topo> {
        display::topology()
    }
    fn display_connected(&self) -> usize {
        display::connected()
    }
    fn display_set_topology(&self, t: display::Topo) -> bool {
        display::set_topology_to(t)
    }
    fn display_scales(&self) -> Option<(Vec<u32>, usize)> {
        display::scales()
    }
    fn display_scale(&self) -> Option<u32> {
        display::scale()
    }
    fn display_set_scale_index(&self, i: usize) -> bool {
        display::set_scale_index(i)
    }
    fn display_hdr(&self) -> Option<bool> {
        display::hdr()
    }
    fn display_set_hdr(&self, on: bool) -> bool {
        display::set_hdr(on)
    }
    fn display_layout(&self) -> Vec<display::Mon> {
        display::layout()
    }
    fn display_move_monitor(&self, i: usize, x: i32, y: i32) -> bool {
        display::move_monitor(i, x, y)
    }
    fn display_monitor_info(&self) -> Option<(usize, usize, String, bool)> {
        display::monitor_info()
    }
    fn display_monitor_names(&self) -> Vec<String> {
        display::monitor_names()
    }
    fn display_selected(&self) -> usize {
        display::selected_index()
    }
    fn display_select(&self, i: usize) {
        display::select_monitor(i);
    }

    fn audio_devices(&self, mic: bool) -> Vec<audio::Dev> {
        audio::devices(mic)
    }
    fn audio_default_name(&self, mic: bool) -> String {
        audio::default_name(mic)
    }
    fn audio_set_default(&self, id: &str) -> bool {
        audio::set_default(id)
    }
    fn audio_apps(&self) -> Vec<(u32, String)> {
        audio::apps()
    }
    fn audio_volume(&self, src: audio::Src) -> Option<f32> {
        audio::volume(src)
    }
    fn audio_set_volume(&self, src: audio::Src, v: f32) {
        audio::set_volume(src, v);
    }
    fn audio_muted(&self, mic: bool) -> Option<bool> {
        audio::muted(mic)
    }
    fn audio_set_mute(&self, mic: bool, m: bool) {
        audio::set_mute(mic, m);
    }

    fn bt_state(&self) -> bt::State {
        bt::state()
    }
    fn bt_refresh(&self, done: Ping) {
        bt::refresh(done);
    }
    fn bt_set_bluetooth(&self, on: bool, done: Ping) {
        bt::set_bluetooth(on, done);
    }
    fn bt_set_airplane(&self, on: bool, done: Ping) {
        bt::set_airplane(on, done);
    }
    fn bt_scan(&self, done: Ping) {
        bt::scan(done);
    }
    fn bt_pair(&self, id: String, done: Ping) {
        bt::pair(id, done);
    }
    fn bt_unpair(&self, id: String, done: Ping) {
        bt::unpair(id, done);
    }
    fn bt_take_note(&self) -> Option<String> {
        bt::take_note()
    }

    fn power_battery(&self) -> Option<Battery> {
        power::status()
    }
    fn power_drives(&self) -> Vec<(char, u64, u64)> {
        power::drives()
    }
    fn power_plan_text(&self) -> String {
        power::plan_text()
    }
    fn power_plans(&self) -> (Vec<String>, Option<usize>) {
        power::plans()
    }
    fn power_set_plan(&self, i: usize) {
        power::set_plan(i);
    }
    fn power_timeouts(&self, t: power::Timeout) -> (Vec<String>, Option<usize>) {
        power::timeouts(t)
    }
    fn power_timeout_text(&self, t: power::Timeout) -> String {
        power::timeout_text(t)
    }
    fn power_set_timeout(&self, t: power::Timeout, i: usize) {
        power::set_timeout(t, i);
    }
    fn temp_text(&self) -> String {
        ext::temp_text()
    }
    fn temp_measured(&self) -> bool {
        ext::TEMP_BYTES.load(std::sync::atomic::Ordering::Relaxed) != u64::MAX
    }
    fn temp_measure(&self, done: Ping) {
        ext::measure_temp(done);
    }
    fn temp_clean(&self, done: Note) {
        ext::clean_temp(done);
    }
    fn bin_bytes(&self) -> u64 {
        ext::bin_bytes()
    }
    fn bin_empty(&self, owner: isize) -> bool {
        ext::empty_bin(hw(owner))
    }

    fn net_adapters(&self) -> Vec<net::Adapter> {
        net::adapters()
    }
    fn wifi_radio(&self) -> Option<(bool, bool)> {
        wifi::radio()
    }
    fn wifi_list(&self) -> Vec<wifi::Net> {
        wifi::list()
    }
    fn wifi_scan(&self) {
        wifi::scan();
    }
    fn wifi_set_radio(&self, on: bool) {
        wifi::set_radio(on);
    }
    fn wifi_connect(&self, profile: &str, iface: &windows_sys::core::GUID) -> bool {
        wifi::connect(profile, iface)
    }
    fn wifi_disconnect(&self) -> bool {
        wifi::disconnect()
    }
    fn wifi_forget(&self, profile: &str) -> bool {
        wifi::forget(profile)
    }
    fn wifi_add_and_connect(&self, ssid: &str, raw: &[u8], sec: wifi::Sec, key: &str, hidden: bool, iface: Option<&windows_sys::core::GUID>) -> Result<String, String> {
        wifi::add_and_connect(ssid, raw, sec, key, hidden, iface)
    }
    fn wifi_add_eap_and_connect(&self, ssid: &str, raw: &[u8], auth: &str, user: &str, pass: &str, hidden: bool, iface: Option<&windows_sys::core::GUID>) -> Result<String, String> {
        wifi::add_eap_and_connect(ssid, raw, auth, user, pass, hidden, iface)
    }
    fn vpns(&self) -> Vec<ext::Vpn> {
        ext::vpns()
    }
    fn vpn_dial(&self, name: String, pbk: Option<String>, done: Note) {
        std::thread::spawn(move || done(ext::dial(&name, pbk.as_deref()).map_or_else(|e| e, |_| tf!("VPN «{}» подключён", "VPN \"{}\" connected", ja: "VPN「{}」に接続しました", pt: "VPN \"{}\" conectada", zh: "VPN“{}”已连接", name))));
    }
    fn vpn_hang_up(&self, name: String, done: Note) {
        std::thread::spawn(move || {
            ext::hang_up(&name);
            done(tf!("VPN «{}» отключён", "VPN \"{}\" disconnected", ja: "VPN「{}」を切断しました", pt: "VPN \"{}\" desconectada", zh: "VPN“{}”已断开", name));
        });
    }
    fn proxy_text(&self) -> String {
        ext::proxy_text()
    }
    fn pac_text(&self) -> String {
        ext::pac_text()
    }
    fn proxy_set(&self, pac: bool, v: &str) -> bool {
        ext::set_proxy(pac, v)
    }
    fn proxy_changed(&self) {
        ext::proxy_changed();
    }

    fn programs(&self) -> Vec<ext::App> {
        ext::programs()
    }
    fn uninstall(&self, cmd: &str) -> bool {
        ext::uninstall(cmd)
    }
    fn wu_state(&self) -> wu::State {
        wu::state()
    }
    fn wu_extra(&self) -> WuExtra {
        let i = wu::info();
        // our own search of this session may be newer than what Windows wrote down
        let last = i.checked.into_iter().chain(i.search).max();
        WuExtra { progress: None, restart: i.restart, last_check: last.and_then(sysx::when_unix), last_install: i.install.and_then(sysx::when_unix) }
    }
    fn restart_now(&self) -> bool {
        wu::restart()
    }
    fn wu_check(&self, done: Ping) {
        wu::check(done);
    }
    fn wu_install(&self, done: Ping) {
        wu::install(done);
    }

    fn night_kelvin(&self) -> u32 {
        ext::night_kelvin()
    }
    fn night_text(&self) -> String {
        ext::night_text()
    }
    fn night_set(&self, k: u32) {
        ext::set_night(k);
    }
    fn reg_dword(&self, key: &str, name: &str) -> Option<u32> {
        sys::dword(HKEY_CURRENT_USER, key, name)
    }
    fn reg_set_dword(&self, key: &str, name: &str, v: u32) -> bool {
        sys::set_hkcu_dword(key, name, v)
    }
    fn announce_theme(&self) {
        sys::announce_theme();
    }
    fn wallpaper_name(&self) -> String {
        crate::wall::name()
    }
    fn wallpaper_pick(&self, owner: isize) -> Result<(), ()> {
        sys::pick_wallpaper(hw(owner))
    }
    fn wallpapers(&self) -> Vec<crate::Wall> {
        crate::wall::walls()
    }
    fn windows_value(&self, name: &str) -> String {
        sys::string(HKEY_LOCAL_MACHINE, crate::NT, name).unwrap_or_default()
    }
    fn ram_text(&self) -> String {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: a plain query into a local.
        let mut mem: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut mem) } != 0 { crate::gb(mem.ullTotalPhys) } else { "?".into() }
    }
    fn computer_name(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }
    fn about_info(&self) -> crate::more::AboutInfo {
        sysx::about_info()
    }
    fn assoc_apps(&self, a: &Assoc) -> (Vec<DefApp>, Option<usize>) {
        sysx::assoc_apps(a)
    }
    fn assoc_pick(&self, a: &Assoc, _i: usize, owner: isize, done: Ping) {
        sysx::assoc_pick(a, owner, done);
    }
    fn own_default(&self, k: DefKind) -> String {
        sysx::own_default(k)
    }
    fn rename_pc(&self, name: &str) -> Result<(), String> {
        sysx::rename_pc(name)
    }
    fn user_name(&self) -> String {
        std::env::var("USERNAME").unwrap_or_default()
    }
    fn set_lang(&self, l: winrust_common::Lang) -> bool {
        winrust_common::set_lang(l)
    }
    fn wallpaper_select(&self, k: usize, done: Note) {
        crate::wall::select(k, done);
    }
    fn wall_prepare(&self, done: Ping) {
        crate::wall::prepare(done);
    }
    fn accent(&self) -> usize {
        winrust_common::style::accent() as usize
    }
    fn accent_set(&self, i: usize) {
        if let Some(&a) = winrust_common::style::Accent::ALL.get(i) {
            winrust_common::style::set_accent(a);
        }
    }
    fn opt_list(&self, o: Opt) -> (Vec<String>, Option<usize>) {
        sysx::opt_list(o)
    }
    fn opt_set(&self, o: Opt, i: usize) -> bool {
        sysx::opt_set(o, i)
    }
    fn gauge(&self, g: Gauge) -> Option<f32> {
        sysx::gauge(g)
    }
    fn gauge_set(&self, g: Gauge, v: f32) {
        sysx::gauge_set(g, v);
    }
    fn flag(&self, f: &Flag) -> Option<bool> {
        sysx::flag(f)
    }
    fn flag_set(&self, f: &Flag, on: bool) -> bool {
        sysx::flag_set(f, on)
    }
    fn notify_apps(&self) -> Vec<NotifyApp> {
        sysx::notify_apps()
    }
    fn notify_set(&self, id: &str, mode: usize) -> bool {
        sysx::notify_set(id, mode)
    }
    fn consent(&self, cap: Cap) -> Consent {
        sysx::consent(cap)
    }
    fn security(&self) -> Security {
        sysx::security()
    }
    fn quick_scan(&self, done: Ping) {
        sysx::quick_scan(done);
    }
    fn open_security(&self) {
        sysx::open_security();
    }
    fn layouts(&self) -> Vec<Layout> {
        sysx::layouts()
    }
    fn clock_text(&self) -> String {
        sysx::clock_text()
    }
    fn region_text(&self) -> String {
        sysx::region_text()
    }
    fn numbers_text(&self) -> String {
        sysx::numbers_text()
    }
    fn time_server(&self) -> String {
        sysx::time_server()
    }
    fn time_sync(&self, done: Note) {
        sysx::time_sync(done);
    }
    fn init(&self) {
        audio::init();
    }
    fn start(&self, hwnd: isize) {
        wifi::watch(hw(hwnd));
    }
    fn identify(&self, mons: &[display::Mon]) {
        crate::identify::show(mons);
    }
}
