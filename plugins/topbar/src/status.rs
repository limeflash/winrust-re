//! topbar: status (split out of main.rs; shares its items through `super::*`).
use super::*;

pub(crate) unsafe fn endpoint() -> Option<IAudioEndpointVolume> {
    EP.with_borrow_mut(|e| {
        if e.is_none() {
            *e = (|| -> windows::core::Result<IAudioEndpointVolume> {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                en.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
            })()
            .ok();
        }
        e.clone()
    })
}

pub(crate) unsafe fn read_volume() {
    let v = endpoint().and_then(|e| Some(((e.GetMasterVolumeLevelScalar().ok()? * 100.0).round() as i32, e.GetMute().ok()?.as_bool())));
    if v.is_none() {
        EP.set(None); // device changed: look the endpoint up again next time
    }
    VOL.set(v.unwrap_or((-1, false)));
}

pub(crate) unsafe fn set_volume(v: i32) {
    if let Some(e) = endpoint() {
        let _ = e.SetMasterVolumeLevelScalar(v.clamp(0, 100) as f32 / 100.0, null());
        let _ = e.SetMute(false, null());
        read_volume();
    }
}

pub(crate) unsafe fn add_volume(delta: i32) {
    if let Some(e) = endpoint() {
        let v = (VOL.get().0.max(0) + delta).clamp(0, 100);
        let _ = e.SetMasterVolumeLevelScalar(v as f32 / 100.0, null());
        let _ = e.SetMute(false, null());
        read_volume();
    }
}

pub(crate) unsafe fn toggle_mute() {
    if let Some(e) = endpoint() {
        let _ = e.SetMute(!VOL.get().1, null());
        read_volume();
    }
}

thread_local! {
    static WLAN: Cell<usize> = const { Cell::new(0) }; // WLAN client handle, opened once (opening it talks to wlansvc every time)
}

pub(crate) unsafe fn read_wifi() {
    let mut h: HANDLE = WLAN.get() as HANDLE;
    let mut q = -1;
    if h.is_null() {
        let mut ver = 0u32;
        if WlanOpenHandle(2, null(), &mut ver, &mut h) != 0 {
            h = null_mut();
        }
        WLAN.set(h as usize);
    }
    if !h.is_null() {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        if WlanEnumInterfaces(h, null(), &mut list) != 0 {
            WlanCloseHandle(h, null()); // stale handle (service restarted): reopen next time
            WLAN.set(0);
        } else {
            let l = &*list;
            for i in 0..l.dwNumberOfItems as usize {
                let ifc = &*l.InterfaceInfo.as_ptr().add(i);
                let (mut size, mut data) = (0u32, null_mut());
                if ifc.isState == wlan_interface_state_connected && WlanQueryInterface(h, &ifc.InterfaceGuid, wlan_intf_opcode_current_connection, null(), &mut size, &mut data, null_mut()) == 0 {
                    q = (*(data as *const WLAN_CONNECTION_ATTRIBUTES)).wlanAssociationAttributes.wlanSignalQuality as i32;
                    WlanFreeMemory(data);
                }
            }
            WlanFreeMemory(list as *const _);
        }
    }
    WIFI.set(q);
    // Windows' own verdict (the same one its tray icon shows): a connection without internet is not "online"
    use windows_sys::Win32::{NetworkManagement::IpHelper::GetNetworkConnectivityHint, Networking::WinSock::{NL_NETWORK_CONNECTIVITY_HINT, NetworkConnectivityLevelHintConstrainedInternetAccess, NetworkConnectivityLevelHintInternetAccess}};
    let mut hint: NL_NETWORK_CONNECTIVITY_HINT = zeroed();
    // a failed call (old Windows) counts as online: never show "no internet" on a guess
    INET.set(GetNetworkConnectivityHint(&mut hint) != 0 || {
        let lvl = hint.ConnectivityLevel;
        lvl == NetworkConnectivityLevelHintInternetAccess || lvl == NetworkConnectivityLevelHintConstrainedInternetAccess
    });
}

/// What the network icon shows.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Link {
    Wifi(i32),      // on wifi with internet, signal 0..100
    NoInternet(i32), // on wifi, but no way out
    Cable,          // internet over ethernet
    Offline,        // no connection at all
}

pub(crate) fn net() -> Link {
    match (WIFI.get(), INET.get()) {
        (q, true) if q >= 0 => Link::Wifi(q),
        (q, false) if q >= 0 => Link::NoInternet(q),
        (_, true) => Link::Cable,
        _ => Link::Offline,
    }
}

/// Wi-Fi arcs lit for a signal of `q` percent: 1..3.
pub(crate) fn arcs_lit(q: i32) -> i32 {
    if q >= 67 { 3 } else if q >= 34 { 2 } else { 1 }
}

/// The Wi-Fi symbol drawn with anti-aliased arcs: `lit` of the 3 arcs in `on`, the rest in `off`, centered on (cx, cy),
/// `s` px tall. `mark`: 0 none, 1 a cross (not connected), 2 an exclamation mark (connected, no internet).
pub(crate) unsafe fn wifi_glyph(dc: HDC, cx: i32, cy: i32, s: i32, lit: i32, on: u32, off: u32, mark: u8) {
    let mut g: *mut GpGraphics = null_mut();
    if GdipCreateFromHDC(dc, &mut g) != 0 || g.is_null() {
        return;
    }
    GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
    let argb = |c: u32| 0xFF00_0000 | (c & 255) << 16 | (c & 0xFF00) | (c >> 16 & 255);
    let pen = |c: u32, w: f32| {
        let mut p: *mut GpPen = null_mut();
        GdipCreatePen1(argb(c), w, UnitPixel, &mut p);
        GdipSetPenStartCap(p, LineCapRound);
        GdipSetPenEndCap(p, LineCapRound);
        p
    };
    let (sf, w) = (s as f32, (s as f32 * 0.12).max(1.6));
    let (bx, by) = (cx as f32, cy as f32 + sf * 0.36); // the point the arcs radiate from
    for i in 0..3 {
        let r = sf * (0.30 + 0.25 * i as f32);
        let p = pen(if i < lit { on } else { off }, w);
        GdipDrawArc(g, p, bx - r, by - r, 2.0 * r, 2.0 * r, 225.0, 90.0);
        GdipDeletePen(p);
    }
    let mut br: *mut GpSolidFill = null_mut();
    GdipCreateSolidFill(argb(if lit > 0 { on } else { off }), &mut br);
    GdipFillEllipse(g, br as *mut GpBrush, bx - w * 0.9, by - w * 0.9, w * 1.8, w * 1.8);
    GdipDeleteBrush(br as *mut GpBrush);
    // the mark sits at the lower right, on a cut-out of the background so it stays readable over the arcs
    let (mx, my, mr) = (bx + sf * 0.42, by - sf * 0.08, sf * 0.26);
    if mark > 0 {
        let mut bg: *mut GpSolidFill = null_mut();
        GdipCreateSolidFill(argb(pal().bg), &mut bg);
        GdipFillEllipse(g, bg as *mut GpBrush, mx - mr, my - mr, 2.0 * mr, 2.0 * mr);
        GdipDeleteBrush(bg as *mut GpBrush);
        let c = if mark == 1 { on } else { WARN };
        let p = pen(c, (w * 0.8).max(1.4));
        let d = mr * 0.5;
        if mark == 1 {
            GdipDrawLine(g, p, mx - d, my - d, mx + d, my + d);
            GdipDrawLine(g, p, mx + d, my - d, mx - d, my + d);
        } else {
            GdipDrawLine(g, p, mx, my - d * 1.2, mx, my + d * 0.2);
            GdipDrawLine(g, p, mx, my + d * 0.95, mx, my + d * 1.0);
        }
        GdipDeletePen(p);
    }
    GdipDeleteGraphics(g);
}
