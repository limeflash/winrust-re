//! topbar: status (split out of main.rs; shares its items through `super::*`).
use super::*;

thread_local! {
    static EP_ID: RefCell<String> = const { RefCell::new(String::new()) }; // the device `EP` was made for
}

pub(crate) unsafe fn endpoint() -> Option<IAudioEndpointVolume> {
    EP.with_borrow_mut(|e| {
        if e.is_none() {
            *e = (|| -> windows::core::Result<IAudioEndpointVolume> {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let d = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
                EP_ID.set(crate::audio::id_of(&d).unwrap_or_default());
                d.Activate(CLSCTX_ALL, None)
            })()
            .ok();
        }
        e.clone()
    })
}

pub(crate) unsafe fn read_volume() {
    // the default output can change under a working endpoint (headphones, the sound settings, another app): a device that is
    // still there answers happily for itself, so the default is looked at each time
    let id = (|| -> Option<String> {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        crate::audio::id_of(&en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?)
    })();
    if id.is_some_and(|id| EP_ID.with_borrow(|c| *c != id)) {
        EP.set(None);
    }
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
