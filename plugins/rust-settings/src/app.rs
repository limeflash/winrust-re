//! The Settings window as an [`App`] of `winrust-ui`: what is open, what waits for a second click, the fields, the countdown of a
//! display change, and what a click, a key or a message from a worker does. It never sees a window, and it reads and changes the system
//! only through `os()`, so `--render`, `--bench` and the tests run it on the demo source (see os.rs).
//! How it looks is draw.rs.
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

use winrust_ui::{
    anim,
    core::{App, place_path},
    frame::{Button, Cx, Frame, Id, Mods},
    gfx::{Canvas, Rect},
    widgets::{
        field::{self, FieldStyle},
        field_model::{Ev, TextField},
        overlay::MenuItem,
        scroll::{self},
        scroll_model::Scroll,
    },
    window::{Options, post, quit, run as run_window},
};
use windows_sys::Win32::NetworkManagement::WiFi::wlan_notification_acm_connection_complete;

use crate::{
    Cmd, Done, GAct, Line, Op, Page, Row, audio, choices, choose, display, finish_confirm, forget_programs, listed, os::{Note, Ping, os}, page, pages, search, slider_value, toggle_adapter, wifi,
    APPS_PAGE, BT_PAGE, CONFIRM_SECS, NET_PAGE, PAGES, PERSONALIZE, POWER_PAGE,
};

// ---------------------------------------------------------------------------------------------------------------------
// ids of the window's items (0 and up; the host's are negative)

/// The first drop-down of the page: what the host's self-test and benchmark click as their menu.
pub const ID_FIRST_SELECT: Id = 1;
pub const ID_BACK: Id = 2;
pub const ID_PROFILE: Id = 3;
pub const ID_SEARCH: Id = 200; // and 201, its clear button
pub const ID_NAV: Id = 10; // + the page
pub const ID_SCROLL: Id = 20; // and 21, its bar
pub const ID_DLG_SCRIM: Id = 30;
pub const ID_DLG_REVERT: Id = 31;
pub const ID_DLG_KEEP: Id = 32;
pub const ID_ARR: Id = 500; // + the display
pub const ID_IDENTIFY: Id = 510;
pub const ID_HERO: Id = 511;
pub const ID_GACT: Id = 520; // + the group
pub const ID_WALL: Id = 540; // + the picture; the last one is "Choose..."
pub const ID_ROW: Id = 1000; // + 32 per line, + the part
pub const ID_HIT: Id = 100_000; // + the hit of a search

pub const P_BODY: Id = 0;
pub const P_CTL: Id = 1;
pub const P_CTL2: Id = 2;
pub const P_MUTE: Id = 3;
pub const P_SEG: Id = 8;
pub const P_FIELD: Id = 16;
pub const P_ACT: Id = 24;

#[must_use]
pub const fn row_id(flat: usize, part: Id) -> Id {
    ID_ROW + flat as Id * 32 + part
}

pub const TAG_PICK: u32 = 1;

/// A worker thread says a line for the toast (`WM_APP + 3`, `lParam` a `Box<String>`), asks for the page to be read again (`WM_APP + 4`).
pub const MSG_NOTE: u32 = 0x8000 + 3;
pub const MSG_REFRESH: u32 = 0x8000 + 4;
/// Posted to ourselves at the start: the first message asks the host for the timer of the 4-second reading.
pub const MSG_KICK: u32 = 0x8000 + 5;

static MAIN: AtomicIsize = AtomicIsize::new(0);

/// Asks the window (from any thread) to read the page again.
fn ping() -> Ping {
    Box::new(|| {
        let h = MAIN.load(Relaxed);
        if h != 0 {
            post(h, MSG_REFRESH, 0, 0);
        }
    })
}

/// Says `s` in the toast and reads the page again (from any thread).
fn note() -> Note {
    Box::new(|s| {
        let h = MAIN.load(Relaxed);
        if h != 0 {
            post(h, MSG_NOTE, 0, Box::into_raw(Box::new(s)) as isize);
        }
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// the state

/// The text fields of the lines that unfold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fid {
    User,
    Pass,
    Ssid,
    Pass2,
    Proxy,
    Pac,
}

pub struct Fields {
    pub user: TextField,
    pub pass: TextField,
    pub ssid: TextField,
    pub pass2: TextField,
    pub proxy: TextField,
    pub pac: TextField,
}

impl Fields {
    fn new() -> Self {
        Self { user: TextField::new(), pass: TextField::password(), ssid: TextField::new(), pass2: TextField::password(), proxy: TextField::new(), pac: TextField::new() }
    }

    pub fn get(&mut self, f: Fid) -> &mut TextField {
        match f {
            Fid::User => &mut self.user,
            Fid::Pass => &mut self.pass,
            Fid::Ssid => &mut self.ssid,
            Fid::Pass2 => &mut self.pass2,
            Fid::Proxy => &mut self.proxy,
            Fid::Pac => &mut self.pac,
        }
    }

    /// The Wi-Fi fields are emptied and the passwords wiped (they have gone into a profile, or nowhere).
    pub fn wipe_wifi(&mut self) {
        for f in [&mut self.pass, &mut self.pass2] {
            // SAFETY: zero bytes are valid UTF-8; the string is dropped right after.
            unsafe { f.text.as_bytes_mut().fill(0) };
            *f = TextField::password();
        }
        self.user = TextField::new();
        self.ssid = TextField::new();
    }
}

/// A display of the arrangement picture being dragged (desktop pixels).
pub struct ArrDrag {
    pub idx: usize,
    pub grab: (i32, i32), // pointer minus the display's corner
    pub cur: (i32, i32),  // where the corner is now
    pub moved: bool,
}

/// A slider being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    Volume(audio::Src),
    /// The night-light strength.
    Night,
}

/// The countdown of a display change that waits for "Keep".
pub struct Confirm {
    pub left: u32,
    pub what: String,
    /// When the last second began (the ring runs from it).
    pub since: u64,
}

pub struct Settings {
    pub page: usize,
    pub pg: Page,
    pub hist: Vec<usize>,
    pub q: TextField,
    pub found: Vec<search::Found>,
    index: Option<Vec<Page>>,
    pub scroll: Scroll,
    /// The line the search sent us to: flashes, and is scrolled to once the page is laid out.
    pub flash: Option<(String, u64)>,
    pub reveal: Option<String>,
    /// The line that is unfolded.
    pub open: Option<String>,
    /// The line whose destructive action waits for a second click.
    pub armed: Option<String>,
    pub fields: Fields,
    pub field_err: String,
    /// The key of the line the error is under.
    pub field_err_key: String,
    /// (SSID, profile, made just now): a connection is being waited for.
    pub connecting: Option<(String, String, bool)>,
    pub pairing: Option<String>,
    pub pin_err: Option<String>,
    pub cleaning: bool,
    pub confirm: Option<Confirm>,
    pub arr: Option<ArrDrag>,
    pub mons: Vec<display::Mon>,
    /// The display the rows below the picture act on.
    pub sel_mon: usize,
    /// How many updates wait: the red dot of the Windows Update entry.
    pub wu_badge: usize,
    /// `--render state=identify`: the numbers of the Identify button are drawn over the window.
    pub demo_identify: bool,
    pub arr_box: Option<crate::Box4>,
    pub hold: Option<Hold>,
    /// Where the select buttons of the last frame are, by the number of their line (a menu opens under one).
    pub anchors: Vec<(usize, Rect)>,
    /// The line of the drop-down that answers to `ID_FIRST_SELECT`.
    pub first_select: Option<usize>,
    /// The track of each slider of the last frame: (flat number, left, right).
    pub tracks: Vec<(usize, f32, f32)>,
    /// The line of each id of the last frame: flat number -> (group, line).
    pub flat: Vec<(usize, usize)>,
    pub flat_base: Vec<usize>,
    pub pick: Option<Op>,
    pub hwnd: isize,
    ticks: u32,
    sig: String,
    next_refresh: u64,
    follow: Option<u64>,
    give_up: Option<u64>,
    tick_at: Option<u64>,
    pub demo_clicks: Vec<Id>,
    /// The size of the window, DIPs (the picture of the arrangement is mapped from it).
    pub win: (f32, f32),
}

const REFRESH_MS: u64 = 4000;
const FOLLOW_MS: u64 = 1500;
const CONNECT_MS: u64 = 30_000;

pub fn style_search() -> FieldStyle {
    FieldStyle { font: crate::look::ui::BODY, icon: Some(crate::look::SEARCH), clear: true, bare: false, radius: 9.0, pad: 10.0 }
}

impl Settings {
    /// A window on page `start`, with what the system says now.
    #[must_use]
    pub fn new(start: usize) -> Self {
        let mut s = Self {
            page: start.min(PAGES - 1),
            pg: Page { title: String::new(), groups: Vec::new() },
            hist: Vec::new(),
            q: TextField::new(),
            found: Vec::new(),
            index: None,
            scroll: Scroll::default(),
            flash: None,
            reveal: None,
            open: None,
            armed: None,
            fields: Fields::new(),
            field_err: String::new(),
            field_err_key: String::new(),
            connecting: None,
            pairing: None,
            pin_err: None,
            cleaning: false,
            confirm: None,
            arr: None,
            mons: Vec::new(),
            sel_mon: 0,
            wu_badge: 0,
            demo_identify: false,
            arr_box: None,
            hold: None,
            anchors: Vec::new(),
            first_select: None,
            tracks: Vec::new(),
            flat: Vec::new(),
            flat_base: Vec::new(),
            pick: None,
            hwnd: 0,
            ticks: 0,
            sig: String::new(),
            next_refresh: anim::now_ms() + REFRESH_MS,
            follow: None,
            give_up: None,
            tick_at: None,
            demo_clicks: Vec::new(),
            win: (1000.0, 720.0),
        };
        s.read_page();
        s
    }

    /// Reads the page on show again.
    fn read_page(&mut self) {
        self.pg = page(self.page);
        if self.page == 0 {
            self.mons = os().display_layout();
            self.sel_mon = os().display_selected();
        }
        let (wu, extra) = (os().wu_state(), os().wu_extra());
        self.wu_badge = if wu.busy.is_none() && !extra.restart { wu.found.as_ref().map_or(0, Vec::len) } else { 0 };
        self.sig = format!("{}|{}", page_sig(&self.pg), self.wu_badge);
    }

    /// Opens page `i` (the pages that change by themselves are read fresh; what a page needs started starts now).
    pub fn goto(&mut self, i: usize, push: bool) {
        let i = i.min(PAGES - 1);
        if push && i != self.page {
            self.hist.push(self.page);
            if self.hist.len() > 20 {
                self.hist.remove(0);
            }
        }
        (self.open, self.armed, self.field_err) = (None, None, String::new());
        self.fields.wipe_wifi();
        self.page = i;
        self.scroll = Scroll::default();
        if i == APPS_PAGE {
            forget_programs(); // the programs are read again
        }
        self.enter(i);
        self.read_page();
    }

    /// What a page needs started when it opens: a Wi-Fi scan, the Bluetooth lists, the size of the temporary files.
    fn enter(&self, i: usize) {
        let o = os();
        match i {
            NET_PAGE => o.wifi_scan(),
            BT_PAGE => o.bt_refresh(ping()),
            POWER_PAGE if !o.temp_measured() => o.temp_measure(ping()),
            _ => {}
        }
    }

    /// The page is read again (a worker finished, a setting changed): a frame is asked for when something on it moved.
    pub fn refresh(&mut self, cx: &mut Cx) {
        if self.pick.is_some() || self.hold.is_some() || self.arr.is_some() || self.confirm.is_some() {
            return; // not while a list is open or something is being dragged
        }
        let before = std::mem::take(&mut self.sig);
        self.read_page();
        if self.sig != before {
            cx.invalidate();
        }
        // what the page shows of a search is looked up again with it
        if !self.q.text.trim().is_empty() {
            self.index = None;
            self.run_search();
        }
    }

    fn run_search(&mut self) {
        if self.q.text.trim().is_empty() {
            (self.found, self.index) = (Vec::new(), None);
            return;
        }
        let index = self.index.get_or_insert_with(pages);
        self.found = search::search(index, &self.q.text);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // timers: one one-shot timer of the host, asked for the nearest of the deadlines

    fn schedule(&mut self, cx: &mut Cx) {
        let now = anim::now_ms();
        let next = [Some(self.next_refresh), self.follow, self.give_up, self.tick_at].into_iter().flatten().min();
        if let Some(t) = next {
            cx.wake_in(t.saturating_sub(now).clamp(1, u64::from(u32::MAX)) as u32);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // what a worker said

    /// An error under the fields of the line `key`.
    fn fail(&mut self, key: &str, msg: impl Into<String>) {
        self.field_err = msg.into();
        self.field_err_key = key.to_string();
    }

    fn open_key(&self) -> String {
        self.open.clone().unwrap_or_default()
    }

    fn say(&mut self, text: impl Into<String>, cx: &mut Cx) {
        cx.toast(text, None);
    }

    fn start_wait(&mut self, ssid: String, profile: String, new: bool, cx: &mut Cx) {
        self.connecting = Some((ssid, profile, new));
        self.give_up = Some(anim::now_ms() + CONNECT_MS);
        self.follow = Some(anim::now_ms() + FOLLOW_MS); // look again soon: the list changes as the connection goes
        cx.invalidate();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the countdown of a display change

    fn begin_confirm(&mut self, op: Op, cx: &mut Cx) {
        let what = match op {
            Op::Res => tf!("Разрешение: {}", "Resolution: {}", crate::res_text().replace(" x ", " \u{d7} ")),
            Op::Hz => tf!("Частота обновления: {}", "Refresh rate: {}", crate::hz_text()),
            Op::Orient => tf!("Ориентация: {}", "Orientation: {}", crate::orient_text()),
            _ => tf!("Экраны: {}", "Displays: {}", crate::topo_text()),
        };
        self.confirm = Some(Confirm { left: CONFIRM_SECS, what, since: anim::now_ms() });
        self.tick_at = Some(anim::now_ms() + 1000);
        cx.invalidate();
    }

    /// Ends the confirmation: `keep` saves the mode, otherwise the saved one comes back.
    pub fn end_confirm(&mut self, keep: bool, cx: &mut Cx) {
        if self.confirm.take().is_none() {
            return;
        }
        self.tick_at = None;
        finish_confirm(keep);
        self.read_page();
        if keep {
            self.say(t!("Настройки экрана сохранены", "Display settings kept"), cx);
        } else {
            self.say(t!("Вернули прежние настройки экрана", "Went back to the previous display settings"), cx);
        }
        cx.invalidate();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // choosing

    /// Applies entry `i` of `choices(op)` and does what the window must after it.
    pub fn pick_value(&mut self, op: Op, i: usize, cx: &mut Cx) {
        match choose(op, i) {
            Done::Nothing => {}
            Done::Confirm => self.begin_confirm(op, cx),
            Done::Lang(written) => {
                let m = crate::lang_note(written);
                self.say(m, cx);
            }
        }
        self.read_page();
        cx.invalidate();
    }

    /// What a click on a switch or a button that is not a list does (the old `display_or_mute`, and the rest of the plain ops).
    pub fn do_op(&mut self, op: Op, cx: &mut Cx) {
        let o = os();
        match op {
            Op::Mute => o.audio_set_mute(false, !o.audio_muted(false).unwrap_or(false)),
            Op::MicMute => o.audio_set_mute(true, !o.audio_muted(true).unwrap_or(false)),
            Op::Primary => drop(o.display_make_primary()),
            Op::Hdr => {
                if let Some(on) = o.display_hdr() {
                    o.display_set_hdr(!on);
                }
            }
            Op::Bt => {
                if let Some(on) = o.bt_state().bt {
                    o.bt_set_bluetooth(!on, ping());
                }
            }
            Op::Air => {
                if let Some(on) = o.bt_state().air {
                    o.bt_set_airplane(!on, ping());
                }
            }
            Op::Radio => {
                o.wifi_set_radio(!crate::RADIO.get().is_some_and(|r| r.0));
                self.follow = Some(anim::now_ms() + FOLLOW_MS);
            }
            Op::Night => self.pick_value(Op::Night, if o.night_kelvin() == 0 { 1 } else { 0 }, cx),
            _ => {}
        }
        self.read_page();
        cx.invalidate();
    }

    /// A switch of a registry value.
    fn flip(&mut self, key: &'static str, name: &'static str, to: u32, cx: &mut Cx) {
        let o = os();
        if o.reg_set_dword(key, name, to) {
            if key == PERSONALIZE {
                o.announce_theme();
            } else if key == crate::ext::INET {
                o.proxy_changed();
            }
        }
        self.read_page();
        cx.invalidate();
    }

    /// A second click confirms what the first armed.
    fn once_more(&mut self, key: &str) -> bool {
        let again = self.armed.as_deref() == Some(key);
        self.armed = if again { None } else { Some(key.to_string()) };
        again
    }

    /// What a click on a button of a line does (the old `run_cmd`).
    pub fn run_cmd(&mut self, key: &str, label: &str, cmd: Cmd, cx: &mut Cx) {
        let o = os();
        match cmd {
            // destructive ones need a second click on the same row
            Cmd::Uninstall(c) => {
                if self.once_more(key) {
                    let m = if o.uninstall(&c) { tf!("Запущено удаление «{}»", "Uninstalling \"{}\"", ja: "「{}」をアンインストールしています", pt: "Desinstalando \"{}\"", zh: "正在卸载“{}”", label) } else { tf!("Не удалось запустить удаление «{}»", "Could not start uninstalling \"{}\"", ja: "「{}」のアンインストールを開始できませんでした", pt: "Não foi possível iniciar a desinstalação de \"{}\"", zh: "无法开始卸载“{}”", label) };
                    self.say(m, cx);
                }
            }
            Cmd::BtUnpair(id) => {
                if self.once_more(key) {
                    o.bt_unpair(id, ping());
                    self.open = None;
                }
            }
            c => {
                self.armed = None;
                match c {
                    Cmd::Vpn(v) => {
                        if v.on {
                            o.vpn_hang_up(v.name.clone(), note());
                        } else {
                            self.say(tf!("Подключение к «{}»…", "Connecting to \"{}\"…", ja: "「{}」に接続中…", pt: "Conectando a \"{}\"…", zh: "正在连接“{}”…", v.name), cx);
                            o.vpn_dial(v.name.clone(), v.pbk.clone(), note());
                        }
                    }
                    Cmd::CleanTemp => {
                        self.cleaning = true;
                        o.temp_clean(note());
                    }
                    Cmd::Wallpaper => {
                        if o.wallpaper_pick(self.hwnd).is_ok() {
                            self.say(t!("Обои изменены", "Wallpaper changed", ja: "壁紙を変更しました", pt: "Plano de fundo alterado"), cx);
                        }
                    }
                    Cmd::EmptyBin => {
                        if o.bin_empty(self.hwnd) {
                            self.say(t!("Корзина очищена", "Recycle Bin emptied", ja: "ごみ箱を空にしました", pt: "Lixeira esvaziada", zh: "回收站已清空"), cx);
                        }
                    }
                    Cmd::BtPair(id) => {
                        self.pairing = Some(id.clone());
                        self.pin_err = None;
                        self.say(t!("Сопряжение…", "Pairing…", ja: "ペアリング中…", pt: "Pareando…", zh: "正在配对…"), cx);
                        o.bt_pair(id, ping());
                    }
                    Cmd::BtScan => {
                        o.bt_scan(ping());
                        self.pairing = None;
                    }
                    Cmd::Check => o.wu_check(ping()),
                    Cmd::Install => o.wu_install(ping()),
                    Cmd::Uninstall(_) | Cmd::BtUnpair(_) => {}
                }
            }
        }
        self.read_page();
        cx.invalidate();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Wi-Fi: what the unfolded line's buttons do

    /// Connect on the network `n` (a saved one, an open one, one with the password typed in the field).
    pub fn wifi_connect(&mut self, n: &wifi::Net, cx: &mut Cx) {
        let o = os();
        match (&n.profile, n.sec) {
            (Some(p), _) => {
                if o.wifi_connect(p, &n.iface) {
                    self.start_wait(n.ssid.clone(), p.clone(), false, cx);
                }
            }
            (None, Some(s)) if !s.needs_key() => drop(self.add_network(n, s, "", false, cx)),
            (None, Some(s)) => {
                let key = self.fields.pass.text.clone();
                self.add_network(n, s, &key, false, cx);
                wipe(key);
            }
            (None, None) if n.eap.is_some() => {
                let user = self.fields.user.text.trim().to_string();
                if user.is_empty() {
                    let k = self.open_key();
                    self.fail(&k, t!("Введите имя пользователя", "Enter the user name", ja: "ユーザー名を入力してください", pt: "Digite o nome de usuário", zh: "输入用户名"));
                    return;
                }
                let pass = self.fields.pass.text.clone();
                match o.wifi_add_eap_and_connect(&n.ssid, &n.raw, n.eap.unwrap_or("WPA2"), &user, &pass, false, Some(&n.iface)) {
                    Ok(profile) => {
                        self.fields.wipe_wifi();
                        self.field_err.clear();
                        self.start_wait(n.ssid.clone(), profile, true, cx);
                    }
                    Err(e) => {
                        let k = self.open_key();
                        self.fail(&k, e);
                    }
                }
                wipe(pass);
            }
            (None, None) => self.say(t!("Эта сеть входит сертификатом: такие сети не поддерживаются", "This network signs in with a certificate: not supported", ja: "このネットワークは証明書でサインインします: 非対応です", pt: "Esta rede entra com um certificado: sem suporte", zh: "此网络使用证书登录：不支持"), cx),
        }
        cx.invalidate();
    }

    /// A new network: saves its profile and connects, or says why not.
    fn add_network(&mut self, n: &wifi::Net, sec: wifi::Sec, key: &str, hidden: bool, cx: &mut Cx) -> bool {
        match os().wifi_add_and_connect(&n.ssid, &n.raw, sec, key, hidden, Some(&n.iface)) {
            Ok(p) => {
                self.fields.wipe_wifi();
                self.field_err.clear();
                self.start_wait(n.ssid.clone(), p, true, cx);
                true
            }
            Err(e) => {
                let k = self.open_key();
                self.fail(&k, e);
                false
            }
        }
    }

    /// "Other network…": the name leads to the connection; an empty password means an open network.
    fn connect_hidden(&mut self, cx: &mut Cx) {
        let s = self.fields.ssid.text.clone();
        if s.trim().is_empty() || s.len() > 32 {
            let k = self.open_key();
            self.fail(&k, t!("Имя сети: от 1 до 32 байт", "Network name: 1 to 32 bytes", ja: "ネットワーク名: 1 ～ 32 バイト", pt: "Nome da rede: de 1 a 32 bytes", zh: "网络名称：1 到 32 字节"));
            return;
        }
        // SAFETY: the interface id is a plain value; the zero one is what the profile code was always given for a hidden network.
        let n = wifi::Net { raw: s.as_bytes().to_vec(), ssid: s, quality: 0, connected: false, profile: None, sec: Some(wifi::Sec::Wpa2 { tkip: false }), eap: None, iface: unsafe { std::mem::zeroed() } };
        let key = self.fields.pass2.text.clone();
        let sec = if key.is_empty() { wifi::Sec::Open } else { n.sec.unwrap_or(wifi::Sec::Wpa2 { tkip: false }) };
        self.add_network(&n, sec, &key, true, cx);
        wipe(key);
        cx.invalidate();
    }

    fn wifi_disconnect(&mut self, cx: &mut Cx) {
        os().wifi_disconnect();
        self.follow = Some(anim::now_ms() + FOLLOW_MS);
        self.open = None;
        cx.invalidate();
    }

    fn wifi_forget(&mut self, profile: &str, cx: &mut Cx) {
        os().wifi_forget(profile);
        self.follow = Some(anim::now_ms() + FOLLOW_MS);
        self.open = None;
        cx.invalidate();
    }

    /// Enter in a field of the proxy line: the address is saved when it is one.
    fn commit_proxy(&mut self, pac: bool, cx: &mut Cx) {
        let v = if pac { self.fields.pac.text.trim().to_string() } else { self.fields.proxy.text.trim().to_string() };
        let ok = if pac { crate::ext::valid_pac(&v) } else { crate::ext::valid_proxy(&v) };
        if ok {
            os().proxy_set(pac, &v);
            self.field_err.clear();
            self.read_page();
        } else {
            self.fail(if pac { "pac-addr" } else { "proxy-addr" }, t!("Неверный адрес", "Not a valid address", ja: "無効なアドレスです", pt: "Endereço inválido", zh: "地址无效"));
        }
        cx.invalidate();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // lines

    /// The line behind a flat number of the last frame.
    #[must_use]
    pub fn line(&self, flat: usize) -> Option<&Line> {
        let &(g, r) = self.flat.get(flat)?;
        self.pg.groups.get(g)?.rows.get(r)
    }

    /// The group's action (the button at the right of its title).
    fn group_act(&mut self, g: usize, cx: &mut Cx) {
        let Some(what) = self.pg.groups.get(g).and_then(|g| g.act.as_ref()).map(|a| a.what.clone()) else { return };
        match what {
            GAct::WifiRefresh => os().wifi_scan(),
            GAct::Cmd(c) => self.run_cmd("", "", c, cx),
            GAct::Copy => {
                let text = self.pg.groups.iter().flat_map(|g| &g.rows).filter_map(|l| if let Row::Info(a, b) = &l.row { Some(format!("{a}: {b}")) } else { None }).collect::<Vec<_>>().join(" \u{b7} ");
                cx.set_clipboard_text(text);
                self.say(t!("Сведения об устройстве скопированы", "Device details copied"), cx);
            }
        }
        cx.invalidate();
    }

    /// A right click on a line: a network that is saved or connected unfolds (its Disconnect and Forget are there), as the old menu offered.
    fn context(&mut self, flat: usize, part: Id) {
        if part != P_BODY {
            return;
        }
        if let Some(l) = self.line(flat)
            && matches!(&l.row, Row::Wifi(n, _) if n.profile.is_some() || n.connected)
        {
            self.open = Some(l.key.clone());
            self.field_err.clear();
        }
    }

    /// A click on a line (`part`: which of its items).
    fn click_line(&mut self, flat: usize, part: Id, cx: &mut Cx) {
        let Some(&(g, r)) = self.flat.get(flat) else { return };
        let Some(line) = self.pg.groups.get(g).and_then(|g| g.rows.get(r)) else { return };
        let (key, label) = (line.key.clone(), line.row.label().to_string());
        // what the line does, decided on an immutable look so that the state can then change
        enum Do {
            Nothing,
            Toggle,
            Pick(Op),
            Choose(Op, usize),
            Op(Op),
            Flip(&'static str, &'static str, u32),
            Cmd(Cmd),
            WifiConnect(wifi::Net),
            Adapter(String),
            Forget(String),
            Disconnect,
            Hidden,
            Later,
            Restart,
        }
        let seg = (P_SEG..P_SEG + 8).contains(&part);
        let act = match (&line.row, part) {
            (Row::Toggle { key, name, on_when, .. }, P_CTL) => Do::Flip(key, name, if line.on == Some(true) { 1 - on_when } else { *on_when }),
            (Row::Switch(_, _, op), P_CTL) if line.on.is_some() => Do::Op(*op),
            (Row::Cycle(_, _, Op::Primary), P_CTL) => Do::Op(Op::Primary),
            (Row::Cycle(_, _, op @ (Op::Topology | Op::Plan)), p) if seg => Do::Choose(*op, (p - P_SEG) as usize),
            (Row::Cycle(_, _, Op::Hidden), P_BODY) => Do::Toggle,
            (Row::Cycle(_, _, Op::Hidden), P_ACT) => Do::Hidden,
            (Row::Cycle(_, _, op), P_CTL) if listed(*op) && !matches!(op, Op::Night | Op::Topology) => Do::Pick(*op),
            (Row::Slider(..), P_MUTE) => line.mute.map_or(Do::Nothing, |(op, _)| Do::Op(op)),
            (Row::Item(_, _, Cmd::BtUnpair(_)), P_BODY) => Do::Toggle,
            (Row::Item(_, _, cmd @ Cmd::BtUnpair(_)), P_ACT) => Do::Cmd(cmd.clone()),
            (Row::Item(..), P_CTL) if line.restart => Do::Later,
            (Row::Item(..), P_CTL2) if line.restart => Do::Restart,
            (Row::Item(_, _, cmd), P_CTL) if !line.busy && !(matches!(cmd, Cmd::BtPair(_)) && self.pairing.is_some()) => Do::Cmd(cmd.clone()),
            (Row::Adapter(a, _), P_BODY) => Do::Adapter(a.name.clone()),
            (Row::Wifi(..), P_BODY) => Do::Toggle,
            (Row::Wifi(n, _), P_ACT) if n.connected => Do::Disconnect,
            (Row::Wifi(n, _), P_ACT) => Do::WifiConnect(n.clone()),
            (Row::Wifi(n, _), p) if p == P_ACT + 1 => n.profile.clone().map_or(Do::Nothing, Do::Forget),
            _ => Do::Nothing,
        };
        match act {
            Do::Nothing => {}
            Do::Toggle => {
                self.field_err.clear();
                self.armed = None;
                self.fields.wipe_wifi();
                self.open = if self.open.as_deref() == Some(key.as_str()) { None } else { Some(key) };
            }
            Do::Pick(op) => {
                if let Some((names, cur)) = choices(op) {
                    let items = names.iter().enumerate().map(|(i, n)| MenuItem::new(n.clone()).radio(cur == Some(i)).value(i)).collect();
                    let at = self.anchors.iter().find(|(n, _)| *n == flat).map_or(Rect::default(), |a| a.1);
                    self.pick = Some(op);
                    cx.menu(TAG_PICK, items, at);
                }
            }
            Do::Choose(op, i) => self.pick_value(op, i, cx),
            Do::Op(op) => self.do_op(op, cx),
            Do::Flip(k, n, v) => self.flip(k, n, v, cx),
            Do::Cmd(c) => self.run_cmd(&key, &label, c, cx),
            Do::WifiConnect(n) => self.wifi_connect(&n, cx),
            Do::Adapter(name) => {
                toggle_adapter(&name);
                self.read_page();
            }
            Do::Forget(p) => self.wifi_forget(&p, cx),
            Do::Disconnect => self.wifi_disconnect(cx),
            Do::Hidden => self.connect_hidden(cx),
            Do::Later => self.say(t!("Windows завершит обновление при следующей перезагрузке", "Windows will finish the next time you restart"), cx),
            Do::Restart => self.say(t!("Перезагрузка…", "Restarting…"), cx),
        }
        cx.invalidate();
    }

    /// The row of a slider (flat number) was pressed or dragged at x (DIPs): the track is `l..r`.
    pub fn set_slider(&mut self, flat: usize, x: f32, l: f32, r: f32, cx: &mut Cx) {
        let Some(line) = self.line(flat) else { return };
        let v = slider_value(x.round() as i32, l.round() as i32, r.round() as i32);
        match &line.row {
            Row::Slider(_, src) => {
                let src = *src;
                os().audio_set_volume(src, v);
                self.hold = Some(Hold::Volume(src));
                self.set_vol_cache(flat, v);
            }
            Row::Cycle(_, _, Op::Night) => {
                // four steps from Low to Maximum
                let i = 1 + (v * 3.0).round() as usize;
                if os().night_kelvin() != crate::ext::NIGHT_STEPS[i] {
                    self.pick_value(Op::Night, i, cx);
                }
                self.hold = Some(Hold::Night);
            }
            _ => {}
        }
        cx.invalidate();
    }

    fn set_vol_cache(&mut self, flat: usize, v: f32) {
        if let Some(&(g, r)) = self.flat.get(flat)
            && let Some(l) = self.pg.groups.get_mut(g).and_then(|g| g.rows.get_mut(r))
        {
            l.vol = Some(v);
            l.mute = l.mute.map(|(op, _)| (op, false));
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the search

    fn open_hit(&mut self, page: usize, key: Option<String>, cx: &mut Cx) {
        self.q.set_text("");
        self.run_search();
        self.goto(page, true);
        if let Some(k) = key {
            self.flash = Some((k.clone(), anim::now_ms()));
            self.reveal = Some(k);
        }
        cx.invalidate();
    }

    fn edited_query(&mut self, cx: &mut Cx) {
        self.pick = None;
        self.run_search();
        self.scroll = Scroll::default();
        cx.invalidate();
    }

    /// The first hit's line, as Enter in the search box opens it.
    fn first_hit(&self) -> Option<(usize, Option<String>)> {
        self.found.first().and_then(|f| f.hits.first()).map(|h| (h.page, h.key.clone()))
    }

    fn go_back(&mut self, cx: &mut Cx) {
        if let Some(p) = self.hist.pop() {
            self.q.set_text("");
            self.run_search();
            self.goto(p, false);
            cx.invalidate();
        }
    }
}

/// The copy of a network (`wifi::Net` is Clone; this keeps the call sites short).
fn clone_net(n: &wifi::Net) -> wifi::Net {
    n.clone()
}

/// A password that was in a string is overwritten before the string goes.
fn wipe(mut s: String) {
    // SAFETY: zero bytes are valid UTF-8.
    unsafe { s.as_bytes_mut().fill(0) };
}

/// A signature of what a page shows, to tell whether reading it again changed anything.
#[must_use]
pub fn page_sig(pg: &Page) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for g in &pg.groups {
        let _ = write!(s, "[{}|{}|{}|{}", g.title, g.note, g.radar, g.empty);
        if let Some(a) = &g.act {
            let _ = write!(s, "|{}{}", a.label, a.busy);
        }
        match &g.extra {
            crate::Extra::Hero(h) => {
                let _ = write!(s, "|{}{}{:?}", h.title, h.sub, h.tone);
            }
            crate::Extra::Battery(b) => {
                let _ = write!(s, "|{b:?}");
            }
            crate::Extra::Drives(d) => {
                for d in d {
                    let _ = write!(s, "|{}{}{:?}", d.name, d.txt, d.parts.iter().map(|p| p.2.as_str()).collect::<Vec<_>>());
                }
            }
            crate::Extra::Walls(w) => {
                let _ = write!(s, "|{}", w.iter().map(|w| format!("{}{}", w.label, w.on)).collect::<String>());
            }
            _ => {}
        }
        for l in &g.rows {
            let _ = write!(s, "{{{}|{}|{}|{:?}|{:?}|{}|{:?}|{}|{:?}|{:?}", l.key, l.sub, l.val, l.on, l.vol, l.busy, l.bars, l.row.label(), l.cur, l.badge);
            if let Row::Adapter(a, open) = &l.row {
                let _ = write!(s, "|{}{open}", a.summary());
            }
            s.push('}');
        }
        s.push(']');
    }
    s
}

// ---------------------------------------------------------------------------------------------------------------------
// the window

impl App for Settings {
    fn title(&self) -> String {
        t!("Параметры", "Settings", ja: "設定", pt: "Configurações", zh: "设置").into()
    }

    fn min_size(&self) -> (f32, f32) {
        (720.0, 480.0)
    }

    fn caption_height(&self) -> f32 {
        56.0
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        self.win = f.size;
        self.paint(c, f);
    }

    fn start(&mut self, hwnd: isize) {
        self.hwnd = hwnd;
        MAIN.store(hwnd, Relaxed);
        os().start(hwnd);
        self.enter(self.page); // the page the window opens on needs the same as one that is gone to
        if hwnd != 0 {
            post(hwnd, MSG_KICK, 0, 0);
        }
    }

    fn demo_clicks(&self) -> Vec<Id> {
        self.demo_clicks.clone()
    }

    fn down(&mut self, id: Id, _b: Button, x: f32, y: f32, cx: &mut Cx) {
        let now = anim::now_ms();
        match id {
            ID_SEARCH => self.q.mouse_down(x, cx.mods.shift, now),
            i if i == scroll::bar_id(ID_SCROLL) => {
                scroll::bar_down(&mut self.scroll, y, now);
            }
            i if (ID_ARR..ID_ARR + 8).contains(&i) => {
                let idx = (i - ID_ARR) as usize;
                if let (Some(bx), Some(m)) = (self.arr_box, self.mons.get(idx)) {
                    let (px, py) = crate::arr_map(&self.mons, bx).desk(x as i32, y as i32);
                    self.arr = Some(ArrDrag { idx, grab: (px - m.x, py - m.y), cur: (m.x, m.y), moved: false });
                }
            }
            i if (ID_ROW..ID_HIT).contains(&i) => {
                let (flat, part) = ((i - ID_ROW) / 32, (i - ID_ROW) % 32);
                self.down_line(flat as usize, part, x, cx);
            }
            _ => {}
        }
        cx.invalidate();
    }

    fn drag(&mut self, id: Id, x: f32, y: f32, cx: &mut Cx) {
        let now = anim::now_ms();
        match id {
            ID_SEARCH => self.q.mouse_drag(x, now),
            i if i == scroll::bar_id(ID_SCROLL) => scroll::bar_drag(&mut self.scroll, y, now),
            i if (ID_ARR..ID_ARR + 8).contains(&i) => {
                if let Some(bx) = self.arr_box {
                    let (px, py) = crate::arr_map(&self.mons, bx).desk(x as i32, y as i32);
                    let mons = &self.mons;
                    if let Some(d) = self.arr.as_mut() {
                        d.cur = (px - d.grab.0, py - d.grab.1);
                        let m = mons[d.idx];
                        d.moved |= (d.cur.0 - m.x).abs() + (d.cur.1 - m.y).abs() > 60;
                    }
                }
            }
            i if (ID_ROW..ID_HIT).contains(&i) => {
                let (flat, part) = ((i - ID_ROW) / 32, (i - ID_ROW) % 32);
                if part == P_CTL {
                    self.drag_slider(flat as usize, x, cx);
                } else {
                    self.drag_field(flat as usize, part, x, now);
                }
            }
            _ => {}
        }
        cx.invalidate();
    }

    fn release(&mut self, id: Id, cx: &mut Cx) {
        match id {
            ID_SEARCH => self.q.mouse_up(),
            i if i == scroll::bar_id(ID_SCROLL) => scroll::bar_up(&mut self.scroll),
            _ => {}
        }
        self.hold = None;
        for f in [Fid::User, Fid::Pass, Fid::Ssid, Fid::Pass2, Fid::Proxy, Fid::Pac] {
            self.fields.get(f).mouse_up();
        }
        if let Some(d) = self.arr.take() {
            let mons = self.mons.clone();
            let m = mons[d.idx];
            if d.moved {
                // flush against the nearest edge of another display; Windows refuses gaps and overlaps
                let others: Vec<_> = mons.iter().enumerate().filter(|(j, _)| *j != d.idx).map(|(_, m)| *m).collect();
                let (x, y) = display::snap(d.cur.0, d.cur.1, m.w, m.h, &others);
                if (x, y) != (m.x, m.y) && !os().display_move_monitor(d.idx, x, y) {
                    self.say(t!("Windows не приняла такое расположение", "Windows did not accept that arrangement", ja: "Windows がその配置を受け付けませんでした", pt: "O Windows não aceitou essa disposição", zh: "Windows 未接受该排列"), cx);
                }
            } else {
                self.pick_value(Op::Monitor, d.idx, cx); // a click picks the display the rows below act on
            }
            self.read_page();
        }
        cx.invalidate();
    }

    fn click(&mut self, id: Id, b: Button, _x: f32, _y: f32, cx: &mut Cx) {
        match id {
            ID_FIRST_SELECT => {
                if let Some(f) = self.first_select {
                    self.click_line(f, P_CTL, cx);
                }
            }
            ID_BACK => self.go_back(cx),
            ID_PROFILE => {
                self.q.set_text("");
                self.run_search();
                self.goto(PAGES - 1, true);
            }
            i if (ID_NAV..ID_NAV + PAGES as Id).contains(&i) => {
                let p = (i - ID_NAV) as usize;
                let searching = !self.q.text.trim().is_empty();
                if searching {
                    self.q.set_text("");
                    self.run_search();
                }
                if p != self.page || searching {
                    self.goto(p, p != self.page);
                }
            }
            i if i == ID_SEARCH + 1 => {
                self.q.set_text("");
                self.edited_query(cx);
                cx.focus(ID_SEARCH);
            }
            ID_DLG_KEEP => self.end_confirm(true, cx),
            ID_DLG_REVERT => self.end_confirm(false, cx),
            ID_IDENTIFY => {
                let mons = self.mons.clone();
                os().identify(&mons);
            }
            ID_HERO => {
                let op = self.pg.groups.iter().find_map(|g| if let crate::Extra::Hero(h) = &g.extra { h.act.as_ref().map(|a| a.1) } else { None });
                if let Some(op) = op {
                    self.do_op(op, cx);
                }
            }
            i if (ID_GACT..ID_GACT + 16).contains(&i) => self.group_act((i - ID_GACT) as usize, cx),
            i if (ID_WALL..ID_WALL + 16).contains(&i) => {
                let n = self.pg.groups.iter().find_map(|g| if let crate::Extra::Walls(w) = &g.extra { Some(w.len()) } else { None }).unwrap_or(0);
                let k = (i - ID_WALL) as usize;
                if k >= n {
                    self.run_cmd("", "", Cmd::Wallpaper, cx);
                } else {
                    os().wallpaper_select(k);
                    self.read_page();
                }
            }
            i if i >= ID_HIT => {
                let n = (i - ID_HIT) as usize;
                let hit = self.found.iter().flat_map(|f| &f.hits).nth(n).cloned();
                if let Some(h) = hit {
                    self.open_hit(h.page, h.key, cx);
                }
            }
            i if i >= ID_ROW => {
                let (flat, part) = (((i - ID_ROW) / 32) as usize, (i - ID_ROW) % 32);
                if b == Button::Right {
                    self.context(flat, part);
                } else {
                    self.click_line(flat, part, cx);
                }
            }
            _ => {}
        }
        self.schedule(cx);
        cx.invalidate();
    }

    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        scroll::wheel(&mut self.scroll, cx.pointer, dy, 48.0, anim::now_ms());
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        if tag == TAG_PICK
            && let Some(op) = self.pick.take()
        {
            self.pick_value(op, choice, cx);
        }
        self.schedule(cx);
    }

    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        const ENTER: u32 = 0x0D;
        const ESC: u32 = 0x1B;
        const UP: u32 = 0x26;
        const DOWN: u32 = 0x28;
        const LEFT: u32 = 0x25;
        const F: u32 = 0x46;
        let now = anim::now_ms();
        // the confirmation of a display change takes Enter and Esc before anything
        if self.confirm.is_some() {
            match vk {
                ENTER => self.end_confirm(true, cx),
                ESC => self.end_confirm(false, cx),
                _ => return true,
            }
            self.schedule(cx);
            return true;
        }
        // a focused field takes its keys
        if let Some(fid) = self.focus_field(cx.focus) {
            let ev = {
                let tf = self.fields.get(fid);
                field::key(tf, cx, vk, m, now)
            };
            match ev {
                Ev::Enter => self.submit_field(fid, cx),
                Ev::Esc => {
                    if matches!(fid, Fid::Pass | Fid::User | Fid::Ssid | Fid::Pass2) {
                        self.open = None;
                        self.field_err.clear();
                    }
                }
                Ev::Edited => self.field_err.clear(),
                _ => {}
            }
            cx.invalidate();
            return ev.used();
        }
        if cx.focus == Some(ID_SEARCH) {
            let ev = field::key(&mut self.q, cx, vk, m, now);
            match ev {
                Ev::Edited => self.edited_query(cx),
                Ev::Esc => {
                    if self.q.text.is_empty() {
                        return false;
                    }
                    self.q.set_text("");
                    self.edited_query(cx);
                }
                Ev::Enter => {
                    if let Some((p, k)) = self.first_hit() {
                        self.open_hit(p, k, cx);
                    }
                }
                _ => {}
            }
            cx.invalidate();
            return ev.used();
        }
        let used = match vk {
            F if m.ctrl => {
                cx.focus(ID_SEARCH);
                true
            }
            LEFT if cx.alt => {
                self.go_back(cx);
                true
            }
            UP | DOWN if m.ctrl || cx.focus.is_none() => {
                let to = if vk == UP { self.page.saturating_sub(1) } else { (self.page + 1).min(PAGES - 1) };
                self.q.set_text("");
                self.run_search();
                self.goto(to, true);
                true
            }
            ESC if self.open.is_some() => {
                // Esc folds an unfolded line before it closes the window
                self.open = None;
                self.field_err.clear();
                true
            }
            ESC => {
                cx.close_window();
                true
            }
            _ => false,
        };
        if used {
            self.schedule(cx);
            cx.invalidate();
        }
        used
    }

    fn text(&mut self, s: &str, cx: &mut Cx) {
        let now = anim::now_ms();
        if let Some(fid) = self.focus_field(cx.focus) {
            field::text(self.fields.get(fid), s, now);
            self.field_err.clear();
        } else if cx.focus == Some(ID_SEARCH) {
            field::text(&mut self.q, s, now);
            self.edited_query(cx);
        }
        cx.invalidate();
    }

    fn message(&mut self, msg: u32, wp: usize, lp: isize, cx: &mut Cx) {
        match msg {
            wifi::WM_WIFI => {
                if wp as i32 == wlan_notification_acm_connection_complete && lp != 0 {
                    // SAFETY: the Wi-Fi watcher posted a `Box<(u32, String)>` for this message and nobody else takes it back.
                    let (reason, profile) = *unsafe { Box::from_raw(lp as *mut (u32, String)) };
                    // only the connection this window started: Windows also reports its own automatic ones
                    if let Some((ssid, prof, new)) = self.connecting.take_if(|c| c.1 == profile) {
                        self.give_up = None;
                        if reason != 0 {
                            if new {
                                os().wifi_forget(&prof); // a wrong password is not kept: the next click asks again
                            }
                            self.say(tf!("Не удалось подключиться к «{}»: {}", "Could not connect to \"{}\": {}", ja: "「{}」に接続できませんでした: {}", pt: "Não foi possível conectar a \"{}\": {}", zh: "无法连接到“{}”：{}", ssid, wifi::reason_text(reason)), cx);
                        }
                    }
                }
                if self.page == NET_PAGE {
                    self.refresh(cx);
                }
                cx.invalidate();
            }
            MSG_NOTE => {
                // SAFETY: `note()` posted a `Box<String>` for this message and nobody else takes it back.
                let m = *unsafe { Box::from_raw(lp as *mut String) };
                self.cleaning = false;
                self.say(m, cx);
                self.refresh(cx);
                cx.invalidate();
            }
            MSG_REFRESH => {
                self.cleaning = false;
                self.pairing = None;
                if let Some(m) = os().bt_take_note() {
                    self.say(m, cx);
                }
                self.refresh(cx);
                cx.invalidate();
            }
            _ => {}
        }
        self.schedule(cx);
    }

    fn wake(&mut self, cx: &mut Cx) {
        self.tick(anim::now_ms(), cx);
    }
}

impl Settings {
    /// What the timer of the host does at `now` (the tests give their own clock): the countdown of a display change, the Wi-Fi follow-up,
    /// the connection that gave up, the reading of the pages that change by themselves.
    pub fn tick(&mut self, now: u64, cx: &mut Cx) {
        if self.tick_at.is_some_and(|t| t <= now) {
            // the countdown of an unconfirmed display change: at 0 it goes back by itself
            self.tick_at = Some(now + 1000);
            let over = self.confirm.as_mut().is_some_and(|c| {
                c.left = c.left.saturating_sub(1);
                c.since = now;
                c.left == 0
            });
            if over {
                self.end_confirm(false, cx);
            }
            cx.invalidate();
        }
        if self.follow.is_some_and(|t| t <= now) {
            self.follow = None;
            self.refresh(cx);
        }
        if self.give_up.is_some_and(|t| t <= now) {
            self.give_up = None;
            if let Some((ssid, ..)) = self.connecting.take() {
                self.say(tf!("«{}» не ответила за 30 с", "\"{}\" did not answer in 30 s", ja: "「{}」が 30 秒以内に応答しませんでした", pt: "\"{}\" não respondeu em 30 s", zh: "“{}”在 30 s 内未响应", ssid), cx);
                cx.invalidate();
            }
        }
        if self.next_refresh <= now {
            self.ticks += 1;
            let o = os();
            if self.page == NET_PAGE && self.ticks.is_multiple_of(8) {
                o.wifi_scan(); // about every 30 s; the result arrives as an event
            }
            if self.page == BT_PAGE {
                o.bt_refresh(ping());
            }
            if self.page != APPS_PAGE {
                self.refresh(cx); // the pages whose values change by themselves
            }
            self.next_refresh = now + REFRESH_MS;
        }
        self.schedule(cx);
    }
}

/// What the demo (`--render`, the tests) does to a window to bring it to a state of the design.
impl Settings {
    /// Reads the page again (a state has changed what the source says).
    pub fn reread(&mut self) {
        self.read_page();
    }

    /// The "Keep these display settings?" dialog, as the mockup's preview state opens it.
    pub fn demo_confirm(&mut self) {
        self.confirm = Some(Confirm { left: CONFIRM_SECS, what: tf!("Разрешение: {}", "Resolution: {}", "1920 \u{d7} 1200"), since: anim::now_ms() });
    }

    /// A line unfolded, with an error under its fields.
    pub fn demo_open(&mut self, key: &str, err: Option<&str>) {
        self.open = Some(key.to_string());
        self.reveal = Some(key.to_string());
        self.fail(key, err.unwrap_or_default());
    }

    /// The search box holds `q`.
    pub fn demo_search(&mut self, q: &str) {
        self.q.set_text(q);
        self.run_search();
    }

    /// Where the line `pred` picks is (its flat number).
    #[cfg(test)]
    pub fn flat_of(&self, pred: impl Fn(&Line) -> bool) -> Option<usize> {
        let mut flat = 0;
        for g in &self.pg.groups {
            for l in &g.rows {
                if pred(l) {
                    return Some(flat);
                }
                flat += 1;
            }
        }
        None
    }
}

impl Settings {
    fn focus_field(&self, focus: Option<Id>) -> Option<Fid> {
        let id = focus?;
        if !(ID_ROW..ID_HIT).contains(&id) {
            return None;
        }
        let part = (id - ID_ROW) % 32;
        if !(P_FIELD..P_FIELD + 8).contains(&part) {
            return None;
        }
        self.field_of_part(((id - ID_ROW) / 32) as usize, (part - P_FIELD) as usize)
    }

    /// Which field the k-th field of a line is.
    #[must_use]
    pub fn field_of_part(&self, flat: usize, k: usize) -> Option<Fid> {
        self.fields_of(flat).get(k).copied()
    }

    /// The fields a line shows when it is unfolded, in order.
    #[must_use]
    pub fn fields_of(&self, flat: usize) -> Vec<Fid> {
        let Some(l) = self.line(flat) else { return Vec::new() };
        match &l.row {
            Row::Wifi(n, _) if self.open.as_deref() == Some(l.key.as_str()) && !n.connected && n.profile.is_none() => {
                if n.sec.is_some_and(|s| s.needs_key()) {
                    vec![Fid::Pass]
                } else if n.sec.is_none() && n.eap.is_some() {
                    vec![Fid::User, Fid::Pass]
                } else {
                    Vec::new()
                }
            }
            Row::Cycle(_, _, Op::Hidden) if self.open.as_deref() == Some(l.key.as_str()) => vec![Fid::Ssid, Fid::Pass2],
            Row::Cycle(_, _, Op::Proxy) => vec![Fid::Proxy],
            Row::Cycle(_, _, Op::Pac) => vec![Fid::Pac],
            _ => Vec::new(),
        }
    }

    fn submit_field(&mut self, fid: Fid, cx: &mut Cx) {
        match fid {
            Fid::Proxy => self.commit_proxy(false, cx),
            Fid::Pac => self.commit_proxy(true, cx),
            Fid::Ssid | Fid::Pass2 => self.connect_hidden(cx),
            Fid::User | Fid::Pass => {
                let n = self.open.as_deref().and_then(|k| self.pg.groups.iter().flat_map(|g| &g.rows).find(|l| l.key == k)).and_then(|l| if let Row::Wifi(n, _) = &l.row { Some(clone_net(n)) } else { None });
                if let Some(n) = n {
                    self.wifi_connect(&n, cx);
                }
            }
        }
    }

    fn down_line(&mut self, flat: usize, part: Id, x: f32, cx: &mut Cx) {
        let now = anim::now_ms();
        if (P_FIELD..P_FIELD + 8).contains(&part) {
            if let Some(fid) = self.field_of_part(flat, (part - P_FIELD) as usize) {
                self.fields.get(fid).mouse_down(x, cx.mods.shift, now);
            }
        } else if part == P_CTL {
            self.drag_slider(flat, x, cx);
        }
    }

    fn drag_slider(&mut self, flat: usize, x: f32, cx: &mut Cx) {
        let Some((l, r)) = self.slider_track(flat) else { return };
        self.set_slider(flat, x, l + 9.0, r - 9.0, cx);
    }

    fn drag_field(&mut self, flat: usize, part: Id, x: f32, now: u64) {
        if (P_FIELD..P_FIELD + 8).contains(&part)
            && let Some(fid) = self.field_of_part(flat, (part - P_FIELD) as usize)
        {
            self.fields.get(fid).mouse_drag(x, now);
        }
    }

    /// The track of the slider of a line in the last frame (x0, x1), if it has one.
    fn slider_track(&self, flat: usize) -> Option<(f32, f32)> {
        self.tracks.iter().find(|t| t.0 == flat).map(|t| (t.1, t.2))
    }
}

/// The host's options for this window (the design's 1000 x 720).
#[must_use]
pub fn options() -> Options {
    Options { class: "WinRustSettings", size: (1000.0, 720.0), icon: 1, place_file: place_path("rust-settings") }
}

/// A normal start: the real system, on page `start`.
pub fn run(start: usize) {
    os().init();
    let app = Settings::new(start);
    let code = run_window(Box::new(app), options());
    quit(code as u32);
}
