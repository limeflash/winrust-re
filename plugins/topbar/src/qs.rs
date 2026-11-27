//! topbar: the machine behind the quick settings panel (split out of main.rs; shares its items through `super::*`): what the
//! tiles say and do, the volume and the brightness, the player, and the Wi-Fi networks (the WLAN API: scan, join with a
//! password, connect, disconnect, forget). The panel itself is `ui::quick_popup`; the calendar's opening is here too.
use super::*;
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering::Relaxed};
use ui::quick_popup as qp;

static BRIGHT: AtomicI32 = AtomicI32::new(-1); // -1 = no controllable display (desktop PC)

struct NetInfo {
    ssid: String,
    /// The SSID's bytes as the adapter reported them (not always UTF-8: `ssid` is what is shown, this is what a profile needs).
    raw: Vec<u8>,
    q: i32,
    secure: bool,
    profile: Option<String>,
    connected: bool,
    auth: i32,
    g: windows_sys::core::GUID, // the adapter that sees it
}

struct Conn {
    ssid: String,
    auth: i32,
    q: i32,
    rx: u32, // Mbps
    tx: u32,
    phy: &'static str,
}

thread_local! {
    static NETS: RefCell<Vec<NetInfo>> = const { RefCell::new(Vec::new()) };
    static CONN: RefCell<Option<Conn>> = const { RefCell::new(None) };
    static NOTE: RefCell<String> = const { RefCell::new(String::new()) };
    static CONNECTING: RefCell<Option<(String, u64)>> = const { RefCell::new(None) }; // the network being joined and since when: its row animates
}

fn auth_name(a: i32) -> &'static str {
    match a {
        1 => t!("Открытая", "Open", ja: "オープン", pt: "Aberta", zh: "开放"),
        2 => "WEP",
        3 => "WPA",
        4 => "WPA‑Personal",
        6 => "WPA2",
        7 => "WPA2‑Personal",
        8 => "WPA3",
        9 => "WPA3‑Personal",
        _ => t!("Защищено", "Secured", ja: "保護あり", pt: "Protegida", zh: "已加密"),
    }
}

fn phy_name(t: i32) -> &'static str {
    match t {
        7 => "Wi‑Fi 4",
        8 => "Wi‑Fi 5",
        10 => "Wi‑Fi 6",
        11 => "Wi‑Fi 7",
        4..=6 => "802.11a/b/g",
        _ => "Wi‑Fi",
    }
}

// ---------- brightness ----------

/// The built-in panel through the video driver (`\\.\LCD`, what Windows' own slider talks to): needs no admin rights and no
/// PowerShell. IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS 0x230498 answers (policy, AC %, DC %); IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS
/// 0x23049C takes the same three bytes (policy 3: both). No panel (a desktop PC) = the open fails.
fn lcd_io(code: u32, input: Option<[u8; 3]>) -> Option<[u8; 3]> {
    use windows_sys::Win32::{
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };
    unsafe {
        let h = CreateFileW(wide(r"\\.\LCD").as_ptr(), 0xC000_0000, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let (mut out, mut n) = ([0u8; 3], 0u32);
        let ok = match input {
            Some(i) => DeviceIoControl(h, code, i.as_ptr().cast(), 3, null_mut(), 0, &mut n, null_mut()),
            None => DeviceIoControl(h, code, null(), 0, out.as_mut_ptr().cast(), 3, &mut n, null_mut()),
        } != 0;
        CloseHandle(h);
        ok.then_some(out)
    }
}

/// Reads the brightness: the driver answers at once; where it does not, WMI is asked in a thread for the next time the
/// panel opens (the panel's size depends on whether there is a brightness row, so it cannot wait for PowerShell).
fn read_brightness() {
    let ac = unsafe {
        let mut s: windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS = zeroed();
        windows_sys::Win32::System::Power::GetSystemPowerStatus(&mut s) != 0 && s.ACLineStatus == 1
    };
    let read = |b: [u8; 3]| i32::from(if ac { b[1] } else { b[2] });
    match lcd_io(0x23_0498, None).map(read).filter(|v| *v <= 100) {
        Some(v) => BRIGHT.store(v, Relaxed),
        None => {
            std::thread::spawn(|| {
                let o = Command::new("powershell").args(["-NoProfile", "-Command", "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue).CurrentBrightness"]).creation_flags(NO_WINDOW).output();
                let v = o.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<i32>().ok()).filter(|v| (0..=100).contains(v));
                BRIGHT.store(v.unwrap_or(-1), Relaxed);
            });
        }
    }
}

fn set_brightness(v: i32) {
    std::thread::spawn(move || {
        let b = v.clamp(0, 100) as u8;
        if lcd_io(0x23_049C, Some([3, b, b])).is_none() {
            let c = format!("Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=1;Brightness={v}}}");
            let _ = Command::new("powershell").args(["-NoProfile", "-Command", &c]).creation_flags(NO_WINDOW).output();
        }
    });
}

// ---------- Wi-Fi ----------

/// Runs `f` for every Wi-Fi adapter with one open WLAN handle.
unsafe fn for_each_wlan(mut f: impl FnMut(HANDLE, &windows_sys::core::GUID)) {
    let (mut h, mut ver) = (null_mut(), 0u32);
    if WlanOpenHandle(2, null(), &mut ver, &mut h) != 0 {
        return;
    }
    let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
    if WlanEnumInterfaces(h, null(), &mut list) == 0 {
        let l = &*list;
        for i in 0..l.dwNumberOfItems as usize {
            f(h, &(*l.InterfaceInfo.as_ptr().add(i)).InterfaceGuid);
        }
        WlanFreeMemory(list as *const _);
    }
    WlanCloseHandle(h, null());
}

/// Runs `f` with an open WLAN handle for the adapter `g`.
unsafe fn with_iface<T>(g: &windows_sys::core::GUID, f: impl FnOnce(HANDLE, &windows_sys::core::GUID) -> T) -> Option<T> {
    let (mut h, mut ver) = (null_mut(), 0u32);
    if WlanOpenHandle(2, null(), &mut ver, &mut h) != 0 {
        return None;
    }
    let r = f(h, g);
    WlanCloseHandle(h, null());
    Some(r)
}

/// The visible networks and the current connection of one adapter.
unsafe fn read_one(h: HANDLE, g: &windows_sys::core::GUID) -> (Vec<NetInfo>, Option<Conn>) {
    let mut out: Vec<NetInfo> = Vec::new();
    let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
    if WlanGetAvailableNetworkList(h, g, 0, null(), &mut list) == 0 {
        let l = &*list;
        for i in 0..l.dwNumberOfItems as usize {
            let n = &*l.Network.as_ptr().add(i);
            let raw = n.dot11Ssid.ucSSID[..(n.dot11Ssid.uSSIDLength as usize).min(32)].to_vec();
            let ssid = String::from_utf8_lossy(&raw).to_string();
            if ssid.is_empty() {
                continue;
            }
            let pn = String::from_utf16_lossy(&n.strProfileName[..n.strProfileName.iter().position(|&c| c == 0).unwrap_or(0)]);
            let net = NetInfo { ssid, raw, q: n.wlanSignalQuality as i32, secure: n.bSecurityEnabled != 0, profile: (n.dwFlags & 2 != 0 && !pn.is_empty()).then_some(pn), connected: n.dwFlags & 1 != 0, auth: n.dot11DefaultAuthAlgorithm as i32, g: *g };
            match out.iter_mut().find(|o| o.ssid == net.ssid) {
                Some(o) if o.q < net.q => *o = net,
                Some(_) => {}
                None => out.push(net),
            }
        }
        WlanFreeMemory(list as *const _);
    }
    out.sort_by_key(|n| (!n.connected, -n.q));
    let (mut size, mut data) = (0u32, null_mut());
    let conn = (WlanQueryInterface(h, g, wlan_intf_opcode_current_connection, null(), &mut size, &mut data, null_mut()) == 0).then(|| {
        let at = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
        let a = &at.wlanAssociationAttributes;
        let ssid = String::from_utf8_lossy(&a.dot11Ssid.ucSSID[..a.dot11Ssid.uSSIDLength as usize]).to_string();
        let c = Conn { ssid, auth: at.wlanSecurityAttributes.dot11AuthAlgorithm, q: a.wlanSignalQuality as i32, rx: a.ulRxRate / 1000, tx: a.ulTxRate / 1000, phy: phy_name(a.dot11PhyType) };
        WlanFreeMemory(data);
        c
    });
    // the network we are on can be missing from the scan list (hidden / not rescanned yet): show it anyway
    if let Some(c) = &conn {
        match out.iter().position(|n| n.ssid == c.ssid) {
            Some(i) => {
                out[i].connected = true;
                out.swap(0, i);
            }
            None => out.insert(0, NetInfo { ssid: c.ssid.clone(), raw: c.ssid.as_bytes().to_vec(), q: c.q, secure: c.auth != 1, profile: Some(c.ssid.clone()), connected: true, auth: c.auth, g: *g }),
        }
    }
    (out, conn)
}

unsafe fn read_nets() {
    let mut nets: Vec<NetInfo> = vec![];
    let mut conn: Option<Conn> = None;
    for_each_wlan(|h, g| {
        let (o, c) = read_one(h, g);
        for n in o {
            // one entry per network: the adapter that is connected to it, else the one with the better signal
            match nets.iter_mut().find(|x| x.ssid == n.ssid) {
                Some(x) if (n.connected && !x.connected) || (!x.connected && x.q < n.q) => *x = n,
                Some(_) => {}
                None => nets.push(n),
            }
        }
        if conn.is_none() {
            conn = c;
        }
    });
    nets.sort_by_key(|n| (!n.connected, -n.q));
    nets.truncate(8);
    NETS.set(nets);
    CONN.set(conn);
    // the joining animation ends when the network shows up as connected, or gives up after 25 s
    let on = NETS.with_borrow(|n| n.iter().find(|n| n.connected).map(|n| n.ssid.clone()));
    if let Some((ssid, t0)) = CONNECTING.with_borrow(|c| c.clone()) {
        if on.as_deref() == Some(ssid.as_str()) {
            CONNECTING.set(None);
            NOTE.set(String::new());
        } else if now_ms().saturating_sub(t0) > 25_000 {
            CONNECTING.set(None);
            NOTE.set(tf!("Не удалось подключиться к «{ssid}»", "Could not connect to “{ssid}”", ja: "「{ssid}」に接続できませんでした", pt: "Não foi possível conectar a “{ssid}”", zh: "无法连接到“{ssid}”"));
        }
    }
}

unsafe fn start_connecting(ssid: &str) {
    CONNECTING.set(Some((ssid.to_string(), now_ms())));
    NOTE.set(String::new());
}

/// Deletes the saved profile of `ssid` (Windows drops the connection if it is the current one).
unsafe fn forget(ssid: &str) {
    let Some(p) = NETS.with_borrow(|n| n.iter().find(|n| n.ssid == ssid).and_then(|n| n.profile.clone())) else { return };
    let w = wide(&p);
    let mut ok = false;
    for_each_wlan(|h, g| ok |= WlanDeleteProfile(h, g, w.as_ptr(), null()) == 0);
    NOTE.set(if ok { tf!("Сеть «{ssid}» забыта", "Forgot “{ssid}”", ja: "「{ssid}」を削除しました", pt: "“{ssid}” esquecida", zh: "已忘记“{ssid}”") } else { tf!("Не удалось забыть «{ssid}»", "Could not forget “{ssid}”", ja: "「{ssid}」を削除できませんでした", pt: "Não foi possível esquecer “{ssid}”", zh: "无法忘记“{ssid}”") });
    read_nets();
}

unsafe fn wifi_connect(profile: &str, g: &windows_sys::core::GUID) {
    let p = wide(profile);
    with_iface(g, |h, g| {
        let params = WLAN_CONNECTION_PARAMETERS { wlanConnectionMode: wlan_connection_mode_profile, strProfile: p.as_ptr(), pDot11Ssid: null_mut(), pDesiredBssidList: null_mut(), dot11BssType: dot11_BSS_type_any, dwFlags: 0 };
        WlanConnect(h, g, &params, null());
    });
}

fn ssid_hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02X}")).collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Saves a profile for a new network and connects. shortcut: only open and WPA/WPA2/WPA3 personal (password) networks; enterprise ones go to Windows settings.
unsafe fn wifi_join(n: &NetInfo, pass: &str) {
    let (auth, key) = match n.auth {
        1 => ("open", String::new()),
        4 => ("WPAPSK", pass.into()),
        7 => ("WPA2PSK", pass.into()),
        _ => ("WPA3SAE", pass.into()),
    };
    let sec = if auth == "open" {
        "<authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption>".to_string()
    } else {
        format!("<authEncryption><authentication>{auth}</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption><sharedKey><keyType>passphrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>", esc(&key))
    };
    let name = esc(&n.ssid);
    let hex = ssid_hex(&n.raw); // the SSID by its bytes: a name that is not UTF-8 would not match its network as text
    let xml = format!("<?xml version=\"1.0\"?><WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\"><name>{name}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security>{sec}</security></MSM></WLANProfile>");
    let x = wide(&xml);
    let mut reason = 0u32;
    let ok = with_iface(&n.g, |h, g| WlanSetProfile(h, g, 0, x.as_ptr(), null(), 1, null_mut(), &mut reason) == 0).unwrap_or(false);
    if ok {
        wifi_connect(&n.ssid, &n.g);
        start_connecting(&n.ssid);
    } else {
        NOTE.set(t!("Не удалось сохранить сеть (неверный формат пароля?)", "Could not save the network (wrong password format?)", ja: "ネットワークを保存できませんでした (パスワードの形式が違いませんか?)", pt: "Não foi possível salvar a rede (formato de senha errado?)", zh: "无法保存网络（密码格式有误？）").into());
    }
}

/// Wi-Fi radio on/off (the Wi-Fi tile), software radio of every adapter (first PHY of each).
unsafe fn wifi_radio(on: bool) {
    crate::quick::set_wifi(on, move || unsafe { wifi_radio_wlan(on) });
}

/// The adapter's own software radio switch (the fallback): it cannot undo what Windows' radio manager switched off.
unsafe fn wifi_radio_wlan(on: bool) {
    for_each_wlan(|h, g| {
        let s = WLAN_PHY_RADIO_STATE { dwPhyIndex: 0, dot11SoftwareRadioState: if on { dot11_radio_state_on } else { dot11_radio_state_off }, dot11HardwareRadioState: dot11_radio_state_on };
        WlanSetInterface(h, g, wlan_intf_opcode_radio_state, size_of::<WLAN_PHY_RADIO_STATE>() as u32, &s as *const _ as *const _, null());
    });
}

// ---------- the host of the panel ----------

/// The sound devices of the last read and a count of the reads. A read takes 8-13 ms (endpoint and topology calls): on the
/// bar's thread it dropped frames of whatever was moving (the sound list sliding open), so it is made on a thread of its own
/// and the panel takes the result on its next frame.
static AUDIO: std::sync::Mutex<Option<(u32, qp::Audio)>> = std::sync::Mutex::new(None);
static AUDIO_READING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads the sound devices again, behind the panel (one read at a time; the open panel repaints when it is in).
pub(crate) fn read_audio_soon() {
    if AUDIO_READING.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
        // SAFETY: COM for the audio endpoints on this thread, ended when the read is done.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let a = crate::audio::read();
        {
            let mut g = AUDIO.lock().unwrap_or_else(|e| e.into_inner());
            let n = g.as_ref().map_or(0, |x| x.0) + 1;
            *g = Some((n, a));
        }
        AUDIO_READING.store(false, Relaxed);
        ui::panel::refresh();
        if com {
            // SAFETY: pairs the CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
    });
}

/// The machine's data and actions for the quick settings panel.
pub(crate) struct Live {
    last: u64,
    /// The sound devices as the panel shows them, which read they came from, and when the next read was asked for.
    audio: Rc<qp::Audio>,
    audio_read: u32,
    audio_at: u64,
}

impl Live {
    unsafe fn new() -> Self {
        // the devices of the last read (the bar reads them once at start), and a fresh read behind them; only a panel opened
        // before that first read is in waits for one here
        let last = AUDIO.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(n, a)| (*n, a.clone()));
        let (n, a) = last.unwrap_or_else(|| (0, crate::audio::read()));
        read_audio_soon();
        Self { last: now_ms(), audio: Rc::new(a), audio_read: n, audio_at: now_ms() }
    }

    /// Takes a read that finished since the last frame.
    fn take_audio(&mut self) {
        let g = AUDIO.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((n, a)) = g.as_ref()
            && *n != self.audio_read
        {
            self.audio_read = *n;
            self.audio = Rc::new(a.clone());
        }
    }
}

/// A state Windows would not tell (`None`) reads as off, as Windows' own quick settings show it.
fn onoff(b: Option<bool>) -> String {
    if b == Some(true) { t!("Вкл.", "On", ja: "オン", pt: "Ligado", zh: "开") } else { t!("Выкл.", "Off", ja: "オフ", pt: "Desligado", zh: "关") }.to_string()
}

impl qp::Host for Live {
    fn tick(&mut self, wifi_page: bool) {
        self.take_audio();
        // SAFETY: reads on the bar's thread; the reads and the clock are what the old flyout did every second.
        unsafe {
            let now = now_ms();
            if now.saturating_sub(self.last) < 1000 {
                return;
            }
            self.last = now;
            if wifi_page {
                read_nets();
            } else {
                crate::quick::refresh();
                if now.saturating_sub(self.audio_at) >= 3000 {
                    read_audio_soon();
                    self.audio_at = now;
                }
            }
        }
    }

    fn state(&mut self) -> qp::State {
        let q = WIFI.get();
        let ssid = NETS.with_borrow(|n| n.iter().find(|n| n.connected).map(|n| n.ssid.clone()));
        let wsub = match (q >= 0, ssid) {
            (true, Some(s)) => s,
            (true, None) => t!("Подключено", "Connected", ja: "接続済み", pt: "Conectado", zh: "已连接").into(),
            // on a cable the tile says so instead of a bare "not connected"
            _ if matches!(net(), Link::Cable) => "Ethernet".into(),
            _ if NETS.with_borrow(|n| n.is_empty()) => t!("Выключен", "Off", ja: "オフ", pt: "Desligado", zh: "关").into(),
            _ => t!("Не подключено", "Not connected", ja: "未接続", pt: "Não conectado", zh: "未连接").into(),
        };
        let (bt, air, acc, eco) = (crate::quick::bluetooth(), crate::quick::airplane(), crate::quick::accessibility_on(), crate::quick::energy_saver());
        let (hot, night) = (crate::quick::hotspot(), crate::quick::night_light());
        let tile = |on: bool, status: String| qp::Tile { status, on };
        let tiles = vec![
            tile(q >= 0, wsub),
            tile(bt == Some(true), onoff(bt)),
            tile(air == Some(true), onoff(air)),
            tile(acc, onoff(Some(acc))),
            tile(eco, onoff(Some(eco))),
            tile(!i18n::style::light(), onoff(Some(!i18n::style::light()))), // Dark style: on while the mode is dark
            tile(hot == Some(true), onoff(hot)),
            tile(night == Some(true), onoff(night)),
            tile(false, "Win+P".into()),
        ];
        let (vol, muted) = VOL.get();
        let (pct, ac) = BAT.get();
        let b = BRIGHT.load(Relaxed);
        qp::State {
            tiles,
            vol: (vol >= 0).then_some((vol, muted)),
            bright: (b >= 0).then_some(b),
            media: crate::quick::MEDIA.lock().unwrap().clone().map(|m| qp::Media { title: m.title, artist: m.artist, playing: m.playing }),
            battery: (pct >= 0).then_some((pct, ac)),
            audio: Rc::clone(&self.audio),
        }
    }

    fn tile(&mut self, i: usize, cx: &mut ui::panel::Cx) {
        // SAFETY: the old flyout's actions, on the bar's thread.
        unsafe {
            match i {
                0 => wifi_radio(WIFI.get() < 0),
                1 => match crate::quick::bluetooth() {
                    Some(on) => crate::quick::set(false, !on),
                    None => {
                        // no answer from the radio list (still reading, or Windows hides it from us): do not sit silent, show Windows' page
                        crate::quick::refresh();
                        open(&wide("ms-settings:bluetooth"));
                        cx.close();
                    }
                },
                2 => crate::quick::set(true, !crate::quick::airplane().unwrap_or(false)),
                3 => cx.close_then(|| accessibility_menu()),
                4 => crate::quick::set_energy_saver(!crate::quick::energy_saver()),
                5 => {
                    i18n::style::toggle_dark_style(); // Windows' own switch while the bar follows it, the Appearance pick otherwise
                    crate::theme_changed();
                }
                6 => match crate::quick::hotspot() {
                    Some(on) => crate::quick::set_hotspot(!on),
                    None => {
                        open(&wide("ms-settings:network-mobilehotspot")); // no internet connection to share
                        cx.close();
                    }
                },
                7 => {
                    if !crate::quick::set_night_light(!crate::quick::night_light().unwrap_or(false)) {
                        open(&wide("ms-settings:nightlight")); // no state of its own yet (never set up) or an unknown layout
                        cx.close();
                    }
                }
                _ => cx.close_then(|| _ = Command::new("DisplaySwitch.exe").spawn()), // Win+P's own panel; it needs no Explorer
            }
        }
    }

    fn volume(&mut self, percent: i32) {
        // SAFETY: the master volume of the default endpoint.
        unsafe { set_volume(percent) }
    }

    fn mute(&mut self) {
        // SAFETY: as above.
        unsafe { toggle_mute() }
    }

    fn audio_set(&mut self, input: bool, id: &str) {
        // SAFETY: the system's policy call on the bar's thread; the master volume is looked up again for the new default.
        unsafe {
            if crate::audio::set_default(id, input) {
                if !input {
                    EP.set(None);
                    read_volume();
                }
                read_audio_soon(); // the tick of mark moves to the new device when the read is in, a frame or two later
                self.audio_at = now_ms();
            }
        }
    }

    fn brightness(&mut self, percent: i32, done: bool) {
        BRIGHT.store(percent.clamp(1, 100), Relaxed);
        if done {
            set_brightness(percent);
        }
    }

    fn media(&mut self, cmd: u8) {
        crate::quick::media(cmd);
    }

    fn settings(&mut self, uri: &'static str, cx: &mut ui::panel::Cx) {
        // SAFETY: opens a settings URI.
        unsafe { open(&wide(uri)) };
        cx.close();
    }

    fn wifi_open(&mut self) {
        // SAFETY: WLAN calls on the bar's thread.
        unsafe {
            for_each_wlan(|h, g| _ = WlanScan(h, g, null(), null(), null()));
            read_nets();
        }
        NOTE.set(String::new());
        // SAFETY: the clock.
        self.last = unsafe { now_ms() };
    }

    fn wifi(&mut self) -> qp::Wifi {
        let on = NETS.with_borrow(|n| !n.is_empty()) || WIFI.get() >= 0;
        let note = NOTE.with_borrow(Clone::clone);
        let status = if !note.is_empty() {
            Some((note, qp::Tone::Dim))
        } else {
            match net() {
                Link::Wifi(_) => None,
                Link::NoInternet(_) => Some((t!("Подключено, но интернета нет", "Connected, no internet", ja: "接続済み、インターネットなし", pt: "Conectado, sem internet", zh: "已连接，无 Internet").to_string(), qp::Tone::Warn)),
                Link::Cable => Some((t!("Интернет по кабелю", "Online over a cable", ja: "有線でオンライン", pt: "Online por cabo", zh: "有线联网").to_string(), qp::Tone::Dim)),
                Link::Offline if on => Some((t!("Не подключено", "Not connected", ja: "未接続", pt: "Não conectado", zh: "未连接").to_string(), qp::Tone::Dim)),
                Link::Offline => Some((t!("Выключен", "Turned off", ja: "オフ", pt: "Desligado", zh: "已关闭").to_string(), qp::Tone::Dim)),
            }
        };
        let nets = NETS.with_borrow(|n| n.iter().map(|n| qp::Network { ssid: n.ssid.clone(), quality: n.q, secure: n.secure, saved: n.profile.is_some(), connected: n.connected }).collect());
        let link = CONN.with_borrow(|c| c.as_ref().map(|c| qp::Link { phy: c.phy.to_string(), down: c.rx, up: c.tx, auth: auth_name(c.auth).to_string() }));
        qp::Wifi { on, nets, link, status, joining: CONNECTING.with_borrow(|c| c.as_ref().map(|c| c.0.clone())), offline: matches!(net(), Link::NoInternet(_)) }
    }

    fn wifi_power(&mut self, on: bool) {
        // SAFETY: the radio switch.
        unsafe { wifi_radio(on) }
    }

    fn wifi_pick(&mut self, ssid: &str) -> qp::Join {
        // SAFETY: WLAN calls on the bar's thread.
        unsafe {
            let n = NETS.with_borrow(|n| n.iter().find(|n| n.ssid == ssid).map(|n| (n.connected, n.profile.clone(), n.auth, n.secure, n.g)));
            let Some((connected, profile, auth, secure, iface)) = n else { return qp::Join::Started };
            if connected {
                // the card is information; its Disconnect button is the action
            } else if let Some(p) = profile {
                wifi_connect(&p, &iface);
                start_connecting(ssid);
            } else if !secure {
                NETS.with_borrow(|l| {
                    if let Some(n) = l.iter().find(|n| n.ssid == ssid) {
                        wifi_join(n, "");
                    }
                });
            } else if matches!(auth, 4 | 7 | 8 | 9) {
                return qp::Join::NeedPassword;
            } else {
                open(&wide("ms-settings:network-wifi"));
                return qp::Join::Settings;
            }
            qp::Join::Started
        }
    }

    fn wifi_join(&mut self, ssid: &str, password: &str) {
        NETS.with_borrow(|l| {
            if let Some(n) = l.iter().find(|n| n.ssid == ssid) {
                // SAFETY: WLAN calls on the bar's thread.
                unsafe { wifi_join(n, password) };
            }
        });
    }

    fn wifi_leave(&mut self) {
        // SAFETY: WLAN calls on the bar's thread.
        unsafe { for_each_wlan(|h, g| _ = WlanDisconnect(h, g, null())) };
        NOTE.set(t!("Отключено", "Disconnected", ja: "切断", pt: "Desconectado", zh: "已断开").into());
    }

    fn wifi_forget(&mut self, ssid: &str) {
        // SAFETY: WLAN calls on the bar's thread.
        unsafe { forget(ssid) };
    }
}

/// The accessibility tile's menu: Narrator, Magnifier, sticky keys, filter keys.
unsafe fn accessibility_menu() {
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let (nar, mag) = (crate::quick::running("Narrator.exe"), crate::quick::running("Magnify.exe"));
    let (sticky, filter) = (crate::quick::sticky_keys(), crate::quick::filter_keys());
    let items = vec![
        fly::MItem::new(t!("Экранный диктор", "Narrator", ja: "ナレーター", pt: "Narrador", zh: "讲述人")).hint("Win+Ctrl+Enter").on(nar),
        fly::MItem::new(t!("Экранная лупа", "Magnifier", ja: "拡大鏡", pt: "Lupa", zh: "放大镜")).hint("Win +/Esc").on(mag),
        fly::MItem::new(t!("Залипание клавиш", "Sticky keys", ja: "固定キー機能", pt: "Teclas de Aderência", zh: "粘滞键")).on(sticky).sep(),
        fly::MItem::new(t!("Фильтрация ввода", "Filter keys", ja: "フィルター キー機能", pt: "Teclas de Filtro", zh: "筛选键")).on(filter),
    ];
    fly::menu(pt.x, None, items, move |i| unsafe {
        match i {
            0 => keys(&[VK_LWIN, VK_CONTROL, VK_RETURN]),
            1 => keys(if mag { &[VK_LWIN, VK_ESCAPE] } else { &[VK_LWIN, VK_OEM_PLUS] }),
            2 => crate::quick::set_keys(false, !sticky),
            _ => crate::quick::set_keys(true, !filter),
        }
    });
}

/// The panel with its host, over the bar button `key`; `wifi` opens it on the Wi-Fi page.
unsafe fn build(key: i32, wifi: bool) -> Option<(Box<dyn ui::panel::Content>, ui::panel::Anchor)> {
    let a = anchor(key)?;
    Some((Box::new(qp::QuickPanel::new(Box::new(Live::new()), a.side, wifi)), a))
}

/// What the panel needs read before it opens (its size depends on the brightness row).
unsafe fn prepare() {
    read_brightness();
    read_nets();
    crate::quick::refresh();
}

/// A click on the network, volume or battery button: open the quick settings, or close them.
pub(crate) unsafe fn quick_toggle(key: i32) {
    ui::panel::toggle(ui::panel::Id::Qs, || {
        prepare();
        build(key, false)
    });
}

/// A right click on the network button: the Wi-Fi page (a second one closes it).
pub(crate) unsafe fn wifi_toggle() {
    if ui::panel::is_open(ui::panel::Id::Qs) && qp::on_wifi_page() {
        return ui::panel::close();
    }
    let open_now = ui::panel::is_open(ui::panel::Id::Qs);
    let make = || {
        prepare();
        build(WIFI_I, true)
    };
    if open_now {
        // the panel is up on its first page: it is replaced by one that opens on the second
        if let Some((c, a)) = make() {
            ui::panel::open(c, a);
        }
    } else {
        ui::panel::toggle(ui::panel::Id::Qs, make);
    }
}

/// The calendar with the notification centre, if it is not open (a click on a pop-up opens it).
pub(crate) unsafe fn cal_open() {
    if !ui::panel::is_open(ui::panel::Id::Cal) {
        cal_toggle();
    }
}

/// A click on the clock: the calendar, or close it.
pub(crate) unsafe fn cal_toggle() {
    ui::panel::toggle(ui::panel::Id::Cal, || {
        let a = anchor(CLOCK)?;
        // the column's room: from 12 below the top of the screen to 12 above the bar (beside a side bar, to the screen's end)
        let s = SCALE.get();
        let (sh, bar_top) = (a.screen.1 as f32 / s, (a.bar.1 - a.origin.1) as f32 / s);
        let room = if a.side == Side::Bottom { bar_top - 12.0 - 12.0 } else { sh - 24.0 };
        Some((Box::new(ui::cal_popup::CalPanel::new(ui::cal_popup::system_now, a.side, room)), a))
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_profile_names_the_ssid_by_its_bytes() {
        assert_eq!(super::ssid_hex(b"Home"), "486F6D65");
        assert_eq!(super::ssid_hex(&[0xC4, 0xE3, 0xBA, 0xC3]), "C4E3BAC3"); // a GBK name: not UTF-8, lossy text would not match it
    }
}
