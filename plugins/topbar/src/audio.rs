//! topbar: the sound devices of the quick settings (Core Audio): the active playback and recording devices with a name, a line
//! about each (the adapter and how it is connected) and a kind, and the switch of the default one. Every call asks Windows
//! again; all of them come from the bar's UI thread, where COM is initialized.
#![allow(non_snake_case)] // the method names of the COM interface below
use std::ffi::c_void;

use windows::{
    Win32::{
        Foundation::PROPERTYKEY,
        Media::Audio::{
            DEVICE_STATE_ACTIVE, EDataFlow, IDeviceTopology, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications, eConsole, eMultimedia, eRender,
        },
        System::Com::{
            CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ,
            StructuredStorage::{PropVariantToStringAlloc, PropVariantToUInt32WithDefault},
        },
    },
    core::{GUID, HRESULT, PCWSTR},
};

use crate::ui::quick_popup::{Audio, Device, Kind};

const fn key(fmtid: u128, pid: u32) -> PROPERTYKEY {
    PROPERTYKEY { fmtid: GUID::from_u128(fmtid), pid }
}

/// `PKEY_Device_DeviceDesc`: "Speakers", the endpoint's own name (its friendly name adds the adapter in brackets).
const DEVICE_DESC: PROPERTYKEY = key(0xa45c254e_df1c_4efd_8020_67d146a850e0, 2);
const DEVICE_FRIENDLY_NAME: PROPERTYKEY = key(0xa45c254e_df1c_4efd_8020_67d146a850e0, 14);
/// `PKEY_DeviceInterface_FriendlyName`: the adapter, "Realtek(R) Audio".
const ADAPTER_NAME: PROPERTYKEY = key(0x026e516e_b814_414b_83cd_856d6fef4822, 2);
/// `PKEY_AudioEndpoint_FormFactor`: 1 speakers, 3 headphones, 4 microphone, 5 headset, 9 a display's speakers, ...
const FORM_FACTOR: PROPERTYKEY = key(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e, 0);

fn enumerator() -> Option<IMMDeviceEnumerator> {
    // SAFETY: creates the system's enumerator; COM is up on the bar's thread.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

pub(crate) fn id_of(d: &IMMDevice) -> Option<String> {
    // SAFETY: `GetId` hands out a string we free.
    unsafe {
        let p = d.GetId().ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// Where the device hangs: Bluetooth, USB, or the machine's own audio chip, from the device it is connected to.
fn link_of(d: &IMMDevice) -> &'static str {
    // SAFETY: the topology of an endpoint has one connector; the id it names is freed.
    let id = unsafe {
        (|| {
            let topo: IDeviceTopology = d.Activate(CLSCTX_ALL, None).ok()?;
            let p = topo.GetConnector(0).ok()?.GetDeviceIdConnectedTo().ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        })()
    };
    let id = id.unwrap_or_default().to_lowercase();
    if id.contains("bth") {
        "bluetooth"
    } else if id.contains("usb") {
        "usb"
    } else if id.contains("hdaudio") || id.contains("hdaudbus") || id.contains("intelaudio") || id.contains("acp") {
        "builtin"
    } else {
        ""
    }
}

fn device(d: &IMMDevice, capture: bool, current: Option<&str>) -> Option<Device> {
    let id = id_of(d)?;
    // SAFETY: reads properties of the device; every string is freed.
    let (name, adapter, form) = unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let text = |k: &PROPERTYKEY| {
            let v = store.GetValue(k).ok()?;
            let p = PropVariantToStringAlloc(&v).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s.filter(|s| !s.is_empty())
        };
        let form = store.GetValue(&FORM_FACTOR).ok().map_or(10, |v| PropVariantToUInt32WithDefault(&v, 10));
        (text(&DEVICE_DESC).or_else(|| text(&DEVICE_FRIENDLY_NAME)).unwrap_or_else(|| id.clone()), text(&ADAPTER_NAME).unwrap_or_default().replace("(R)", "").replace("(TM)", ""), form)
    };
    let link = link_of(d);
    let kind = match (capture, form, link) {
        (true, 5 | 6, _) | (true, _, "bluetooth") => Kind::Headphones,
        (true, ..) => Kind::Microphone,
        (false, 3 | 5, _) | (false, _, "bluetooth") => Kind::Headphones,
        (false, 9, _) => Kind::Monitor,
        _ => Kind::Speakers,
    };
    let adapter = adapter.trim().to_string();
    let sub = match (link, kind) {
        ("bluetooth", Kind::Headphones) if capture => t!("Bluetooth-гарнитура", "Bluetooth headset", ja: "Bluetooth ヘッドセット", pt: "Headset Bluetooth", zh: "蓝牙耳机").to_string(),
        ("bluetooth", _) => "Bluetooth".to_string(),
        (_, Kind::Monitor) => format!("{} \u{b7} {}", t!("Динамики монитора", "Monitor speakers", ja: "モニターのスピーカー", pt: "Alto-falantes do monitor", zh: "显示器扬声器"), adapter).trim_end_matches(" \u{b7} ").to_string(),
        ("usb", _) => format!("{adapter} \u{b7} USB"),
        ("builtin", _) => format!("{adapter} \u{b7} {}", t!("встроенный", "built-in", ja: "内蔵", pt: "integrado", zh: "内置")),
        _ => adapter,
    };
    Some(Device { current: current == Some(id.as_str()), id, name, sub: sub.trim_start_matches(" \u{b7} ").to_string(), kind })
}

fn devices(e: &IMMDeviceEnumerator, capture: bool) -> Vec<Device> {
    let flow: EDataFlow = if capture { eCapture } else { eRender };
    // SAFETY: plain enumeration calls.
    unsafe {
        let cur = e.GetDefaultAudioEndpoint(flow, eConsole).ok().and_then(|d| id_of(&d));
        let Ok(col) = e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        (0..col.GetCount().unwrap_or(0)).filter_map(|i| col.Item(i).ok()).filter_map(|d| device(&d, capture, cur.as_deref())).collect()
    }
}

/// The active playback and recording devices, the default of each flagged.
pub(crate) fn read() -> Audio {
    enumerator().map(|e| Audio { outputs: devices(&e, false), inputs: devices(&e, true) }).unwrap_or_default()
}

// The system's own switch of the default device. It has no documented API: this is the COM class Windows' sound panel and every
// sound switcher use (PolicyConfigClient); only the one method SetDefaultEndpoint is called, the rest are placeholders that
// keep the vtable in place.
#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, dev: PCWSTR, fmt: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, dev: PCWSTR, default: i32, fmt: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, dev: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, dev: PCWSTR, a: *mut c_void, b: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, dev: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, dev: PCWSTR, a: *mut i64) -> HRESULT;
    fn GetShareMode(&self, dev: PCWSTR, m: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, dev: PCWSTR, m: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, dev: PCWSTR, fx: i32, key: *const c_void, v: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, dev: PCWSTR, fx: i32, key: *const c_void, v: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, dev: PCWSTR, role: i32) -> HRESULT;
    fn SetEndpointVisibility(&self, dev: PCWSTR, visible: i32) -> HRESULT;
}

const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// Makes device `id` the default one of its kind: for apps (the console role) and for media always, and for calls only if the
/// calls used the very device that apps did (a separate headset for calls stays what the user set).
pub(crate) fn set_default(id: &str, capture: bool) -> bool {
    let Some(e) = enumerator() else { return false };
    let flow: EDataFlow = if capture { eCapture } else { eRender };
    // SAFETY: the system's policy class, asked for the one method it is used for; the id is NUL-terminated.
    unsafe {
        let same_calls = {
            let of = |role| e.GetDefaultAudioEndpoint(flow, role).ok().and_then(|d| id_of(&d));
            let calls = of(eCommunications);
            calls.is_none() || calls == of(eConsole)
        };
        let Ok(pc) = CoCreateInstance::<_, IPolicyConfig>(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) else { return false };
        let w: Vec<u16> = id.encode_utf16().chain([0]).collect();
        let roles = [eConsole, eMultimedia].into_iter().chain(same_calls.then_some(eCommunications));
        // every role is set (no short circuit), and the call is a success if each one was
        let done: Vec<bool> = roles.map(|r| pc.SetDefaultEndpoint(PCWSTR(w.as_ptr()), r.0).is_ok()).collect();
        done.into_iter().all(|ok| ok)
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

    /// Reads the machine's own devices (none on a box without sound): at most one is the default of its kind.
    #[test]
    fn the_machines_devices_have_at_most_one_default_each() {
        // SAFETY: COM for this test's thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let a = super::read();
        for list in [&a.outputs, &a.inputs] {
            assert!(list.iter().filter(|d| d.current).count() <= 1);
            assert!(list.iter().all(|d| !d.id.is_empty() && !d.name.is_empty()));
        }
        for d in a.outputs.iter().chain(&a.inputs) {
            eprintln!("{:?} | {} | {} | current={}", d.kind, d.name, d.sub, d.current);
        }
    }
}
