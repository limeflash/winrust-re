//! The real system behind the Notifications, Privacy & security, Keyboard & mouse and Date & time pages (`os::Live` calls these).
//! Only documented Win32 calls and the registry places Windows' own Settings keeps; three fixed programs are started with fixed
//! arguments and nothing else (`tzutil /s <a key read from the registry>`, `w32tm /resync`, Defender's `MpCmdRun -Scan -ScanType 1`).
//! Nothing here is run by `--render`, `--bench`, `--selftest` or a test: they use the demo source.
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering::Relaxed},
};

use windows_sys::Win32::{
    Foundation::*,
    Globalization::*,
    System::{
        Registry::*,
        SystemInformation::GetLocalTime,
        Time::{DYNAMIC_TIME_ZONE_INFORMATION, FileTimeToSystemTime, GetDynamicTimeZoneInformation, SystemTimeToTzSpecificLocalTime},
    },
    UI::WindowsAndMessaging::*,
};

use crate::{
    more::{CApp, Cap, Consent, RawDesk, group_desktop, DATE_PATS, Flag, Gauge, Layout, NOC, NotifyApp, Opt, Security, When, Who, app_name, fmt_date},
    os::{Note, Ping},
    sys::{self, wide},
};

const NO_WINDOW: u32 = 0x0800_0000;
const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
const INTL: &str = r"Control Panel\International";
const TOGGLE: &str = r"Keyboard Layout\Toggle";
const TZ: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Time Zones";
const W32TIME: &str = r"SYSTEM\CurrentControlSet\Services\W32Time\Parameters";
const DEFENDER: &str = r"SOFTWARE\Microsoft\Windows Defender";

static SCANNING: AtomicBool = AtomicBool::new(false);
static SCANNED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------------------------------------------------
// the registry

fn read(root: HKEY, key: &str, name: &str, flags: u32, buf: &mut [u8]) -> Option<usize> {
    let mut n = buf.len() as u32;
    let rc = unsafe { RegGetValueW(root, wide(key).as_ptr(), wide(name).as_ptr(), flags, null_mut(), buf.as_mut_ptr().cast(), &mut n) };
    (rc == ERROR_SUCCESS).then_some(n as usize)
}

fn qword(root: HKEY, key: &str, name: &str) -> Option<u64> {
    let mut b = [0u8; 8];
    read(root, key, name, RRF_RT_REG_QWORD, &mut b).map(|_| u64::from_le_bytes(b))
}

/// A FILETIME kept as 8 bytes of binary (Defender's update times).
fn binary_time(root: HKEY, key: &str, name: &str) -> Option<u64> {
    let mut b = [0u8; 8];
    (read(root, key, name, RRF_RT_REG_BINARY, &mut b)? == 8).then(|| u64::from_le_bytes(b))
}

fn set_string(root: HKEY, key: &str, name: &str, v: &str) -> bool {
    let w = wide(v);
    unsafe { RegSetKeyValueW(root, wide(key).as_ptr(), wide(name).as_ptr(), REG_SZ, w.as_ptr().cast(), (w.len() * 2) as u32) == ERROR_SUCCESS }
}

fn values(root: HKEY, key: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    unsafe {
        let mut h: HKEY = null_mut();
        if RegOpenKeyExW(root, wide(key).as_ptr(), 0, KEY_READ, &mut h) != ERROR_SUCCESS {
            return out;
        }
        for i in 0.. {
            let (mut name, mut data) = ([0u16; 256], [0u16; 256]);
            let (mut nn, mut nd, mut ty) = (name.len() as u32, (data.len() * 2) as u32, 0u32);
            if RegEnumValueW(h, i, name.as_mut_ptr(), &mut nn, null_mut(), &mut ty, data.as_mut_ptr().cast(), &mut nd) != ERROR_SUCCESS {
                break;
            }
            if ty == REG_SZ {
                out.push((String::from_utf16_lossy(&name[..nn as usize]), String::from_utf16_lossy(&data[..(nd as usize / 2).saturating_sub(1)])));
            }
        }
        RegCloseKey(h);
    }
    out
}

/// Tells programs the regional formats changed (in a thread: a hung window can hold the broadcast for seconds).
fn announce_intl() {
    std::thread::spawn(|| {
        let what = wide("intl");
        unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, null_mut()) };
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// times

fn days(y: i64, m: i64, d: i64) -> i64 {
    // days since 1970-01-01 of a civil date (Howard Hinnant's algorithm)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn when(ft: u64) -> Option<When> {
    if ft == 0 {
        return None;
    }
    // SAFETY: plain conversions into locals.
    unsafe {
        let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
        let (mut utc, mut local, mut now): (SYSTEMTIME, SYSTEMTIME, SYSTEMTIME) = (std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed());
        if FileTimeToSystemTime(&f, &mut utc) == 0 || SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) == 0 {
            return None;
        }
        GetLocalTime(&mut now);
        let d = |s: &SYSTEMTIME| days(i64::from(s.wYear), i64::from(s.wMonth), i64::from(s.wDay));
        Some(When { days_ago: (d(&now) - d(&local)).max(0) as u32, hour: local.wHour as u8, min: local.wMinute as u8, day: local.wDay as u8, month: local.wMonth as u8 })
    }
}

fn today() -> (u32, u32, u32) {
    // SAFETY: a plain clock read.
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    (u32::from(t.wYear), u32::from(t.wMonth), u32::from(t.wDay))
}

// ---------------------------------------------------------------------------------------------------------------------
// lists

const TOAST_SECS: [u32; 6] = [5, 7, 15, 30, 60, 300];

fn toast_secs() -> u32 {
    let mut v = 0u32;
    // SAFETY: the value goes into a local.
    if unsafe { SystemParametersInfoW(SPI_GETMESSAGEDURATION, 0, (&mut v as *mut u32).cast(), 0) } != 0 { v } else { 5 }
}

fn spi_u32(get: u32) -> Option<u32> {
    let mut v = 0u32;
    // SAFETY: the value goes into a local.
    (unsafe { SystemParametersInfoW(get, 0, (&mut v as *mut u32).cast(), 0) } != 0).then_some(v)
}

fn spi_set(set: u32, ui: u32, pv: usize) -> bool {
    // SAFETY: a plain setting, kept in the profile and announced.
    unsafe { SystemParametersInfoW(set, ui, pv as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) != 0 }
}

/// Zones as (registry key, display text), by offset.
fn zones() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = sys::subkeys(HKEY_LOCAL_MACHINE, TZ).into_iter().filter_map(|k| sys::string(HKEY_LOCAL_MACHINE, &format!(r"{TZ}\{k}"), "Display").filter(|d| !d.is_empty()).map(|d| (k, d))).collect();
    let off = |d: &str| -> i32 {
        let r = d.strip_prefix("(UTC").unwrap_or("");
        let (sign, r) = match r.chars().next() {
            Some('+') => (1, &r[1..]),
            Some('-' | '\u{2212}') => (-1, &r[r.chars().next().map_or(0, char::len_utf8)..]),
            _ => return 0,
        };
        let h: i32 = r.get(..2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let m: i32 = r.get(3..5).and_then(|s| s.parse().ok()).unwrap_or(0);
        sign * (h * 60 + m)
    };
    v.sort_by(|a, b| (off(&a.1), &a.1).cmp(&(off(&b.1), &b.1)));
    v
}

fn zone_key() -> Option<String> {
    // SAFETY: the zone goes into a local.
    let mut z: DYNAMIC_TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    unsafe { GetDynamicTimeZoneInformation(&mut z) };
    let n = z.TimeZoneKeyName.iter().position(|&c| c == 0).unwrap_or(z.TimeZoneKeyName.len());
    (n > 0).then(|| String::from_utf16_lossy(&z.TimeZoneKeyName[..n]))
}

/// The short-date patterns on offer and the current one: the user's own pattern is added when the list has none like it.
fn date_pats() -> (Vec<String>, Option<usize>) {
    let mut pats: Vec<String> = DATE_PATS.iter().map(|p| (*p).to_string()).collect();
    let cur = sys::string(HKEY_CURRENT_USER, INTL, "sShortDate").filter(|c| !c.is_empty() && c.len() < 40);
    let at = cur.map(|c| pats.iter().position(|p| *p == c).unwrap_or_else(|| {
        pats.push(c);
        pats.len() - 1
    }));
    (pats, at)
}

pub fn opt_list(o: Opt) -> (Vec<String>, Option<usize>) {
    let hkcu = HKEY_CURRENT_USER;
    match o {
        Opt::Toast => {
            let cur = toast_secs();
            (TOAST_SECS.iter().map(|&s| if s >= 60 { tf!("{} мин", "{} min", s / 60) } else { tf!("{} с", "{} s", s) }).collect(), TOAST_SECS.iter().position(|&s| s == cur))
        }
        Opt::LangKey => {
            let cur = sys::string(hkcu, TOGGLE, "Hotkey").unwrap_or_else(|| "1".into());
            (vec!["Alt + Shift".into(), "Ctrl + Shift".into(), t!("Только Win + Пробел", "Win + Space only").into()], ["1", "2", "3"].iter().position(|&k| k == cur))
        }
        Opt::Hand => {
            // SAFETY: a plain metric.
            let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
            (vec![t!("Левая", "Left").into(), t!("Правая", "Right").into()], Some(usize::from(swapped)))
        }
        Opt::Wheel => {
            let cur = spi_u32(SPI_GETWHEELSCROLLLINES);
            let all = [Some(1), Some(3), Some(5), Some(u32::MAX)];
            (vec![t!("1 строка", "1 line").into(), t!("3 строки", "3 lines").into(), t!("5 строк", "5 lines").into(), t!("Целый экран", "A whole screen").into()], all.iter().position(|&x| x == cur))
        }
        Opt::Clock => {
            // what the bar writes is the short time (TIME_NOSECONDS takes it), so that is what says 24 or 12 hours
            let h24 = sys::string(hkcu, INTL, "sShortTime").is_none_or(|f| f.contains('H'));
            (vec![t!("24 часа · 21:30", "24-hour \u{b7} 21:30").into(), t!("12 часов · 9:30 PM", "12-hour \u{b7} 9:30 PM").into()], Some(usize::from(!h24)))
        }
        Opt::Date => {
            let (y, m, d) = today();
            let (pats, cur) = date_pats();
            (pats.iter().map(|p| fmt_date(p, y, m, d)).collect(), cur)
        }
        Opt::Week => {
            let cur = sys::string(hkcu, INTL, "iFirstDayOfWeek").unwrap_or_else(|| "0".into());
            (vec![t!("Понедельник", "Monday").into(), t!("Воскресенье", "Sunday").into(), t!("Суббота", "Saturday").into()], ["0", "6", "5"].iter().position(|&k| k == cur))
        }
        Opt::Zone => {
            let (z, cur) = (zones(), zone_key());
            let at = cur.and_then(|c| z.iter().position(|(k, _)| *k == c));
            (z.into_iter().map(|(_, d)| d).collect(), at)
        }
    }
}

pub fn opt_set(o: Opt, i: usize) -> bool {
    let hkcu = HKEY_CURRENT_USER;
    match o {
        Opt::Toast => TOAST_SECS.get(i).is_some_and(|&s| spi_set(SPI_SETMESSAGEDURATION, 0, s as usize)),
        Opt::LangKey => {
            let Some(v) = ["1", "2", "3"].get(i) else { return false };
            let mut ok = true;
            for n in ["Hotkey", "Language Hotkey", "Layout Hotkey"] {
                ok &= set_string(hkcu, TOGGLE, n, v); // all three, whatever one says
            }
            announce_intl();
            ok
        }
        Opt::Hand => spi_set(SPI_SETMOUSEBUTTONSWAP, u32::from(i == 1), 0),
        Opt::Wheel => [1u32, 3, 5, u32::MAX].get(i).is_some_and(|&n| spi_set(SPI_SETWHEELSCROLLLINES, n, 0)),
        Opt::Clock => {
            let h24 = i == 0;
            let mut ok = set_string(hkcu, INTL, "sTimeFormat", if h24 { "HH:mm:ss" } else { "h:mm:ss tt" });
            ok &= set_string(hkcu, INTL, "sShortTime", if h24 { "HH:mm" } else { "h:mm tt" });
            ok &= set_string(hkcu, INTL, "iTime", if h24 { "1" } else { "0" });
            if !h24 {
                for (n, d) in [("s1159", "AM"), ("s2359", "PM")] {
                    if sys::string(hkcu, INTL, n).is_none_or(|s| s.is_empty()) {
                        ok &= set_string(hkcu, INTL, n, d);
                    }
                }
            }
            announce_intl();
            ok
        }
        Opt::Date => {
            let ok = date_pats().0.get(i).is_some_and(|p| set_string(hkcu, INTL, "sShortDate", p));
            announce_intl();
            ok
        }
        Opt::Week => {
            let ok = ["0", "6", "5"].get(i).is_some_and(|v| set_string(hkcu, INTL, "iFirstDayOfWeek", v));
            announce_intl();
            ok
        }
        Opt::Zone => {
            // the key comes from the registry's own list, never from a typed string
            let Some((key, _)) = zones().into_iter().nth(i) else { return false };
            let exe = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_default()).join(r"System32\tzutil.exe");
            Command::new(exe).args(["/s", &key]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(NO_WINDOW).status().is_ok_and(|s| s.success())
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// gauges and flags

pub fn gauge(g: Gauge) -> Option<f32> {
    match g {
        Gauge::KeyDelay => spi_u32(SPI_GETKEYBOARDDELAY).map(|v| v.min(3) as f32 / 3.0),
        Gauge::KeyRate => spi_u32(SPI_GETKEYBOARDSPEED).map(|v| v.min(31) as f32 / 31.0),
        Gauge::Pointer => spi_u32(SPI_GETMOUSESPEED).map(|v| (v.clamp(1, 20) - 1) as f32 / 19.0),
    }
}

pub fn gauge_set(g: Gauge, v: f32) {
    let v = v.clamp(0.0, 1.0);
    match g {
        Gauge::KeyDelay => drop(spi_set(SPI_SETKEYBOARDDELAY, (v * 3.0).round() as u32, 0)),
        Gauge::KeyRate => drop(spi_set(SPI_SETKEYBOARDSPEED, (v * 31.0).round() as u32, 0)),
        Gauge::Pointer => drop(spi_set(SPI_SETMOUSESPEED, 0, (1.0 + v * 19.0).round() as usize)),
    }
}

fn dnd_file(var: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(var).map(|p| std::path::PathBuf::from(p).join("WinRust").join("dnd.txt"))
}

fn consent_key(cap: Cap, who: &Who) -> String {
    match who {
        Who::Pc | Who::User => format!(r"{CONSENT}\{}", cap.key()),
        Who::Desktop => format!(r"{CONSENT}\{}\NonPackaged", cap.key()),
        Who::App(id) => format!(r"{CONSENT}\{}\{id}", cap.key()),
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && !id.contains(['\\', '/']) && id.len() < 200
}

pub fn flag(f: &Flag) -> Option<bool> {
    match f {
        Flag::Dnd => Some(["APPDATA", "ProgramData"].into_iter().find_map(|v| std::fs::read_to_string(dnd_file(v)?).ok()).is_some_and(|s| s.trim() == "1")),
        Flag::MouseAccel => {
            let mut m = [0i32; 3];
            // SAFETY: SPI_GETMOUSE fills three integers.
            (unsafe { SystemParametersInfoW(SPI_GETMOUSE, 0, m.as_mut_ptr().cast(), 0) } != 0).then_some(m[2] != 0)
        }
        Flag::AutoTime => sys::string(HKEY_LOCAL_MACHINE, W32TIME, "Type").map(|t| !t.eq_ignore_ascii_case("NoSync")),
        Flag::Access(cap, who) => {
            let root = if *who == Who::Pc { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
            sys::string(root, &consent_key(*cap, who), "Value").map(|v| !v.eq_ignore_ascii_case("Deny"))
        }
    }
}

pub fn flag_set(f: &Flag, on: bool) -> bool {
    match f {
        Flag::Dnd => dnd_file("APPDATA").is_some_and(|p| std::fs::create_dir_all(p.parent().unwrap_or(&p)).is_ok() && std::fs::write(p, if on { "1" } else { "0" }).is_ok()),
        Flag::MouseAccel => {
            let mut m = if on { [6i32, 10, 1] } else { [0, 0, 0] };
            // SAFETY: SPI_SETMOUSE takes three integers.
            unsafe { SystemParametersInfoW(SPI_SETMOUSE, 0, m.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) != 0 }
        }
        Flag::AutoTime => set_string(HKEY_LOCAL_MACHINE, W32TIME, "Type", if on { "NTP" } else { "NoSync" }),
        Flag::Access(cap, who) => {
            if let Who::App(id) = who
                && !valid_id(id)
            {
                return false;
            }
            let root = if *who == Who::Pc { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
            set_string(root, &consent_key(*cap, who), "Value", if on { "Allow" } else { "Deny" })
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// notifications

pub fn notify_apps() -> Vec<NotifyApp> {
    let mut v: Vec<NotifyApp> = sys::subkeys(HKEY_CURRENT_USER, NOC)
        .into_iter()
        .filter(|id| !id.starts_with("Windows.SystemToast."))
        .map(|id| {
            let key = format!(r"{NOC}\{id}");
            let mode = if sys::dword(HKEY_CURRENT_USER, &key, "Enabled") == Some(0) { 2 } else { usize::from(sys::dword(HKEY_CURRENT_USER, &key, "ShowBanner") == Some(0)) };
            NotifyApp { name: app_name(&id), id, mode }
        })
        .collect();
    v.sort_by_key(|a| a.name.to_lowercase());
    v
}

pub fn notify_set(id: &str, mode: usize) -> bool {
    if !valid_id(id) || mode > 2 || !sys::subkeys(HKEY_CURRENT_USER, NOC).iter().any(|k| k == id) {
        return false;
    }
    let key = format!(r"{NOC}\{id}");
    let set = |n: &str, v: u32| sys::set_hkcu_dword(&key, n, v);
    match mode {
        0 => set("Enabled", 1) && set("ShowBanner", 1) && set("ShowInActionCenter", 1),
        1 => set("Enabled", 1) && set("ShowBanner", 0) && set("ShowInActionCenter", 1),
        _ => set("Enabled", 0),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// privacy

/// The exe's own name for itself (version info `FileDescription`: "Microsoft Edge WebView2"); None when the file is gone or says nothing.
fn file_description(path: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let w = wide(path);
    let mut h = 0u32;
    // SAFETY: the buffer is as long as Windows said; what VerQueryValueW points at stays inside it while `buf` lives.
    unsafe {
        let n = GetFileVersionInfoSizeW(w.as_ptr(), &mut h);
        if n == 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        if GetFileVersionInfoW(w.as_ptr(), 0, n, buf.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let (mut p, mut len): (*mut std::ffi::c_void, u32) = (null_mut(), 0);
        if VerQueryValueW(buf.as_ptr().cast(), wide(r"\VarFileInfo\Translation").as_ptr(), &mut p, &mut len) == 0 || len < 4 {
            return None;
        }
        let t = std::slice::from_raw_parts(p.cast::<u16>(), 2);
        let sub = wide(&format!(r"\StringFileInfo\{:04x}{:04x}\FileDescription", t[0], t[1]));
        if VerQueryValueW(buf.as_ptr().cast(), sub.as_ptr(), &mut p, &mut len) == 0 || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p.cast::<u16>(), len as usize)).trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

pub fn consent(cap: Cap) -> Consent {
    let base = format!(r"{CONSENT}\{}", cap.key());
    let times = |key: &str| (qword(HKEY_CURRENT_USER, key, "LastUsedTimeStart").unwrap_or(0), qword(HKEY_CURRENT_USER, key, "LastUsedTimeStop").unwrap_or(0));
    let mut apps: Vec<CApp> = sys::subkeys(HKEY_CURRENT_USER, &base)
        .into_iter()
        .filter(|k| k != "NonPackaged")
        .map(|k| {
            let (start, stop) = times(&format!(r"{base}\{k}"));
            let live = start != 0 && stop == 0; // (a Store app's process cannot be named from here)
            CApp { name: app_name(&k), id: k, desktop: false, last: when(if live { start } else { stop }), live }
        })
        .collect();
    let raw = sys::subkeys(HKEY_CURRENT_USER, &format!(r"{base}\NonPackaged"))
        .into_iter()
        .map(|key| {
            let (start, stop) = times(&format!(r"{base}\NonPackaged\{key}"));
            RawDesk { key, start, stop, start_when: when(start), stop_when: when(stop) }
        })
        .collect();
    let running: std::collections::HashSet<String> = winrust_common::proc::list().into_iter().map(|(_, n)| n.to_lowercase()).collect();
    apps.extend(group_desktop(raw, &|exe| running.contains(exe), &file_description));
    // Store apps by name, then desktop apps, the one using it now first, then the latest (a PC holds hundreds of old ones: 30 are shown)
    let recent = |a: &CApp| a.last.map_or((u32::MAX, 0), |w| (w.days_ago, u32::MAX - (u32::from(w.hour) * 60 + u32::from(w.min))));
    apps.sort_by_key(|a| (a.desktop, !a.live, if a.desktop { recent(a) } else { (0, 0) }, a.name.to_lowercase()));
    let mut desk = 0;
    apps.retain(|a| {
        desk += usize::from(a.desktop);
        !a.desktop || desk <= 30
    });
    Consent { apps }
}

pub fn security() -> Security {
    let hklm = HKEY_LOCAL_MACHINE;
    let present = !sys::subkeys(hklm, DEFENDER).is_empty();
    let fw = |p: &str| sys::dword(hklm, &format!(r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\{p}"), "EnableFirewall");
    let fws: Vec<u32> = ["StandardProfile", "PublicProfile"].iter().filter_map(|p| fw(p)).collect();
    let upd = format!(r"{DEFENDER}\Signature Updates");
    Security {
        av: present.then(|| sys::dword(hklm, &format!(r"{DEFENDER}\Real-Time Protection"), "DisableRealtimeMonitoring").unwrap_or(0) == 0),
        fw: (!fws.is_empty()).then(|| fws.iter().all(|&v| v != 0)),
        sig: sys::string(hklm, &upd, "AVSignatureVersion").unwrap_or_default(),
        sig_when: binary_time(hklm, &upd, "SignaturesLastUpdated").or_else(|| binary_time(hklm, &upd, "AVSignatureApplied")).and_then(when),
        smart: sys::string(hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer", "SmartScreenEnabled").unwrap_or_else(|| "Warn".into()), // no value: Windows' default
        scanning: SCANNING.load(Relaxed),
        scanned: SCANNED.load(Relaxed),
    }
}

pub fn quick_scan(done: Ping) {
    if SCANNING.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let exe = std::path::PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_default()).join(r"Windows Defender\MpCmdRun.exe");
        let ok = Command::new(exe).args(["-Scan", "-ScanType", "1"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(NO_WINDOW).status().is_ok_and(|s| s.success());
        SCANNED.store(ok, Relaxed);
        SCANNING.store(false, Relaxed);
        done();
    });
}

pub fn open_security() {
    winrust_common::settings::open("windowsdefender:");
}

// ---------------------------------------------------------------------------------------------------------------------
// keyboard layouts, formats

fn locale_info(name: *const u16, what: u32) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: the text goes into a local buffer.
    let n = unsafe { GetLocaleInfoEx(name, what, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1).min(buf.len())])
}

pub fn layouts() -> Vec<Layout> {
    let mut pre = values(HKEY_CURRENT_USER, r"Keyboard Layout\Preload");
    pre.sort_by_key(|(n, _)| n.parse::<u32>().unwrap_or(u32::MAX));
    pre.into_iter()
        .map(|(_, klid)| {
            let real = sys::string(HKEY_CURRENT_USER, r"Keyboard Layout\Substitutes", &klid).unwrap_or_else(|| klid.clone());
            let lang = u32::from_str_radix(&klid, 16).map_or(0, |k| k & 0xffff);
            let mut name = [0u16; 86];
            // SAFETY: the locale name goes into a local buffer.
            let n = unsafe { LCIDToLocaleName(lang, name.as_mut_ptr(), name.len() as i32, 0) };
            let loc = if n > 0 { name.as_ptr() } else { null() };
            let text = sys::string(HKEY_LOCAL_MACHINE, &format!(r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts\{real}"), "Layout Text").unwrap_or_default();
            Layout { name: if loc.is_null() { klid.clone() } else { locale_info(loc, LOCALE_SLOCALIZEDDISPLAYNAME) }, sub: text, tag: if loc.is_null() { klid.clone() } else { locale_info(loc, LOCALE_SISO639LANGNAME).to_uppercase() } }
        })
        .collect()
}

pub fn clock_text() -> String {
    let (mut t, mut d) = ([0u16; 64], [0u16; 64]);
    // SAFETY: the texts go into local buffers; no time given means now.
    unsafe {
        let nt = GetTimeFormatEx(null(), TIME_NOSECONDS, null(), null(), t.as_mut_ptr(), t.len() as i32);
        let nd = GetDateFormatEx(null(), DATE_SHORTDATE, null(), null(), d.as_mut_ptr(), d.len() as i32, null());
        format!("{}  \u{b7}  {}", String::from_utf16_lossy(&t[..(nt.max(1) as usize - 1)]), String::from_utf16_lossy(&d[..(nd.max(1) as usize - 1)]))
    }
}

pub fn region_text() -> String {
    let mut name = [0u16; 86];
    // SAFETY: the locale name goes into a local buffer.
    unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    locale_info(name.as_ptr(), LOCALE_SLOCALIZEDDISPLAYNAME)
}

pub fn numbers_text() -> String {
    let (mut n, mut c) = ([0u16; 64], [0u16; 64]);
    let v = wide("1234.56");
    // SAFETY: the texts go into local buffers.
    unsafe {
        let a = GetNumberFormatEx(null(), 0, v.as_ptr(), null(), n.as_mut_ptr(), n.len() as i32);
        let b = GetCurrencyFormatEx(null(), 0, v.as_ptr(), null(), c.as_mut_ptr(), c.len() as i32);
        format!("{} \u{b7} {}", String::from_utf16_lossy(&n[..(a.max(1) as usize - 1)]), String::from_utf16_lossy(&c[..(b.max(1) as usize - 1)]))
    }
}

pub fn time_server() -> String {
    sys::string(HKEY_LOCAL_MACHINE, W32TIME, "NtpServer").and_then(|s| s.split([',', ' ']).next().map(str::to_string)).filter(|s| !s.is_empty()).map_or_else(String::new, |s| tf!("Сервер: {}", "Server: {}", s))
}

pub fn time_sync(done: Note) {
    std::thread::spawn(move || {
        let exe = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_default()).join(r"System32\w32tm.exe");
        let ok = Command::new(exe).arg("/resync").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(NO_WINDOW).status().is_ok_and(|s| s.success());
        done(if ok { t!("Часы синхронизированы", "Clock synced").into() } else { t!("Не удалось синхронизировать: нужны права администратора", "Could not sync: it needs administrator rights").into() });
    });
}

#[cfg(test)]
mod live {
    use super::*;

    /// Reads only, by hand: `cargo test -p rust-settings --release --offline -- --ignored print_reads --nocapture`. Nothing is set.
    #[test]
    #[ignore = "reads the real machine"]
    fn print_reads() {
        for o in [Opt::Toast, Opt::LangKey, Opt::Hand, Opt::Wheel, Opt::Clock, Opt::Date, Opt::Week, Opt::Zone] {
            let (l, c) = opt_list(o);
            println!("{o:?}: {} entries, current {:?} = {:?}", l.len(), c, c.and_then(|c| l.get(c)));
        }
        for g in [Gauge::KeyDelay, Gauge::KeyRate, Gauge::Pointer] {
            println!("{g:?}: {:?}", gauge(g));
        }
        for f in [Flag::Dnd, Flag::MouseAccel, Flag::AutoTime] {
            println!("{f:?}: {:?}", flag(&f));
        }
        for cap in Cap::ALL {
            let c = consent(cap);
            println!("{cap:?}: pc {:?} user {:?} desktop {:?}, {} apps", flag(&Flag::Access(cap, Who::Pc)), flag(&Flag::Access(cap, Who::User)), flag(&Flag::Access(cap, Who::Desktop)), c.apps.len());
            for a in c.apps.iter().filter(|a| a.desktop).take(6) {
                println!("   {} ({}) desktop={} last={:?} live={}", a.name, a.id, a.desktop, a.last, a.live);
            }
        }
        println!("{:?}", security());
        println!("layouts {:?}", layouts());
        println!("clock {:?} | region {:?} | numbers {:?} | server {:?}", clock_text(), region_text(), numbers_text(), time_server());
        let n = notify_apps();
        println!("{} notifying apps: {:?}", n.len(), n.iter().take(6).collect::<Vec<_>>());
    }
}
