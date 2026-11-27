//! Makes the file manager findable as a shell window, so `SHOpenFolderAndSelectItems` ("Show in folder" in browsers, chat apps...)
//! selects the file in it. Explorer finds the window in `IShellWindows` by folder, then asks it for `Document`, the folder-view
//! service, and calls `IShellView::SelectItem`. Everything else the shell might ask for answers "not implemented".
//!
//! Only a window that already exists when the app calls is found: when there is none, Windows starts the folder's handler and
//! waits 20 s for a window of its own that never comes. So the always-running desktop process also registers stand-ins for
//! the common folders (Downloads...); their `SelectItem` opens a real window on the file.
use std::{cell::RefCell, ffi::c_void, os::windows::{ffi::OsStrExt, process::CommandExt}, path::{Path, PathBuf}, sync::Mutex};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Ole::*, Variant::*},
        UI::{Controls::*, Shell::Common::ITEMIDLIST, Shell::*, WindowsAndMessaging::*},
    },
};

/// Posted to the window when the shell wants a file selected: lp = Box<PathBuf>. The shell calls us on an RPC thread, so the UI does the work.
pub const WM_SHELL_SELECT: u32 = WM_APP + 6;
/// `WM_COPYDATA` tag: the desktop's stand-in hands an open window on its folder the file to pick (UTF-16 path, no NUL).
pub const COPY_SELECT: usize = 0x5753_454C;

/// The stand-ins are found first by the shell, before any window the user opened later on the same folder: the file goes to such a
/// window (the file manager's title is its folder) when one answers, so "Show in folder" twice does not open the folder twice.
fn hand_to_open_window(dir: &str, path: &Path) -> bool {
    let dir = dir.trim_end_matches('\\');
    let text: Vec<u16> = path.as_os_str().encode_wide().collect();
    let cds = windows::Win32::System::DataExchange::COPYDATASTRUCT { dwData: COPY_SELECT, cbData: (text.len() * 2) as u32, lpData: text.as_ptr() as *mut c_void };
    let mut w = HWND::default();
    // SAFETY: plain window queries; the data outlives the synchronous send, which gives up on a hung window.
    unsafe {
        while let Ok(next) = FindWindowExW(None, (!w.is_invalid()).then_some(w), w!("RustExplorer"), PCWSTR::null()) {
            w = next;
            let mut buf = [0u16; 1024];
            let n = GetWindowTextW(w, &mut buf) as usize;
            if !String::from_utf16_lossy(&buf[..n]).trim_end_matches('\\').eq_ignore_ascii_case(dir) {
                continue;
            }
            let _ = AllowSetForegroundWindow(ASFW_ANY); // the window raises itself: the shell gave the foreground to us
            let mut r = 0usize;
            if SendMessageTimeoutW(w, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG, 2000, Some(&mut r)).0 != 0 && r == 1 {
                return true;
            }
        }
    }
    false
}

struct Reg { sw: IShellWindows, cookie: i32, live: bool, form: usize } // live: the window whose folder changes (not a stand-in); form: which of pidl_forms
thread_local! { static REG: RefCell<Vec<Reg>> = const { RefCell::new(Vec::new()) }; }
static DIR: Mutex<String> = Mutex::new(String::new());

fn nope<T>() -> Result<T> { Err(E_NOTIMPL.into()) }
fn url(dir: &str) -> String { format!("file:///{}", cur(dir).replace('\\', "/")) }
/// The folder an entry stands for: its own, or (empty) the one the live window shows now.
fn cur(dir: &str) -> String { if dir.is_empty() { DIR.lock().unwrap().clone() } else { dir.to_string() } }
fn wide(s: &std::ffi::OsStr) -> Vec<u16> { s.encode_wide().chain([0]).collect() }

/// The folder's ID lists the way callers build them. `SHParseDisplayName` gives a known folder (Downloads, Pictures...) as the
/// known folder; `ILCreateFromPath`, and so Chromium (Brave, Chrome, Edge) and Telegram, as This PC > D: > Downloads. The shell's
/// window list matches only the exact form, so a folder whose two forms differ is registered under both; other folders have one.
fn pidl_forms(dir: &Path) -> Vec<*mut ITEMIDLIST> {
    let w = wide(dir.as_os_str());
    let mut v = Vec::new();
    unsafe {
        let mut pidl = std::ptr::null_mut();
        if SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None).is_ok() && !pidl.is_null() {
            v.push(pidl);
        }
        let fs = ILCreateFromPathW(PCWSTR(w.as_ptr()));
        if !fs.is_null() {
            if v.iter().any(|p| ILIsEqual(*p, fs).as_bool()) { ILFree(Some(fs)); } else { v.push(fs); }
        }
    }
    v
}
fn free_forms(v: Vec<*mut ITEMIDLIST>) { for p in v { unsafe { ILFree(Some(p)) } } }
fn variant(pidl: *mut ITEMIDLIST) -> Option<VARIANT> { unsafe { InitVariantFromBuffer(pidl as *const c_void, ILGetSize(Some(pidl))).ok() } }

fn add(hwnd: isize, dir: &Path, resident: bool) -> Result<()> {
    unsafe {
        let sw: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let me: IWebBrowser2 = Browser { hwnd, dir: if resident { dir.to_string_lossy().into_owned() } else { String::new() }, resident }.into();
        let disp = me.cast::<IDispatch>()?;
        let forms = pidl_forms(dir);
        // a live window keeps an entry for each form whatever it shows now: the next folder may have two
        let n = if resident { forms.len().max(1) } else { 2 };
        for form in 0..n {
            let cookie = match sw.Register(&disp, hwnd as i32, SWC_BROWSER) { Ok(c) => c, Err(e) => { free_forms(forms); return Err(e) } };
            if let Some(v) = forms.get(form).or(forms.first()).and_then(|p| variant(*p)) {
                let _ = sw.OnNavigate(cookie, &v);
            }
            REG.with_borrow_mut(|r| r.push(Reg { sw: sw.clone(), cookie, live: !resident, form }));
        }
        free_forms(forms);
    }
    Ok(())
}

/// Registers the window (call once, after the folder is shown, on the UI thread).
pub fn register(hwnd: isize) { let _ = add(hwnd, Path::new(&DIR.lock().unwrap().clone()), false); }

/// The always-running desktop process: stand-ins for `dirs`, so "Show in folder" there opens a window on the file.
pub fn register_resident(dirs: &[PathBuf]) {
    // the shell may bring this window to the front; a hidden one on its own keeps the desktop where it is
    let h = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("STATIC"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None) };
    if let Ok(h) = h { for d in dirs { let _ = add(h.0 as isize, d, true); } }
}

/// The window now shows `dir`: the shell matches later requests against it.
pub fn navigated(dir: &Path) {
    *DIR.lock().unwrap() = dir.to_string_lossy().into_owned();
    let forms = pidl_forms(dir);
    REG.with_borrow(|rs| {
        for r in rs.iter().filter(|r| r.live) {
            if let Some(v) = forms.get(r.form).or(forms.first()).and_then(|p| variant(*p)) {
                unsafe { let _ = r.sw.OnNavigate(r.cookie, &v); }
            }
        }
    });
    free_forms(forms);
}

pub fn revoke() {
    for r in REG.take() { unsafe { let _ = r.sw.Revoke(r.cookie); } }
}

#[implement(IWebBrowser2)]
struct Browser { hwnd: isize, dir: String, resident: bool }
impl IDispatch_Impl for Browser_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> { Ok(0) }
    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> { nope() }
    fn GetIDsOfNames(&self, _: *const GUID, _: *const PCWSTR, _: u32, _: u32, _: *mut i32) -> Result<()> { nope() }
    fn Invoke(&self, _: i32, _: *const GUID, _: u32, _: DISPATCH_FLAGS, _: *const DISPPARAMS, _: *mut VARIANT, _: *mut EXCEPINFO, _: *mut u32) -> Result<()> { nope() }
}

/// What `Document` returns: only the folder-view service is answered.
#[implement(IDispatch, IServiceProvider)]
struct Doc { hwnd: isize, dir: String, resident: bool }
impl IDispatch_Impl for Doc_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> { Ok(0) }
    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> { nope() }
    fn GetIDsOfNames(&self, _: *const GUID, _: *const PCWSTR, _: u32, _: u32, _: *mut i32) -> Result<()> { nope() }
    fn Invoke(&self, _: i32, _: *const GUID, _: u32, _: DISPATCH_FLAGS, _: *const DISPPARAMS, _: *mut VARIANT, _: *mut EXCEPINFO, _: *mut u32) -> Result<()> { nope() }
}
impl IServiceProvider_Impl for Doc_Impl {
    fn QueryService(&self, _svc: *const GUID, riid: *const GUID, out: *mut *mut c_void) -> Result<()> {
       
        let v: IShellView = View { hwnd: self.hwnd, dir: self.dir.clone(), resident: self.resident }.into();
        unsafe { v.query(&*riid, out).ok() }
    }
}

#[implement(IShellView)]
struct View { hwnd: isize, dir: String, resident: bool }
impl IOleWindow_Impl for View_Impl {
    fn GetWindow(&self) -> Result<HWND> { Ok(HWND(self.hwnd as *mut c_void)) }
    fn ContextSensitiveHelp(&self, _: BOOL) -> Result<()> { Ok(()) }
}
impl IShellView_Impl for View_Impl {
    fn TranslateAccelerator(&self, _: *const MSG) -> HRESULT { E_NOTIMPL }
    fn EnableModeless(&self, _: BOOL) -> Result<()> { Ok(()) }
    fn UIActivate(&self, _: u32) -> Result<()> { Ok(()) }
    fn Refresh(&self) -> Result<()> { Ok(()) }
    fn CreateViewWindow(&self, _: Ref<IShellView>, _: *const FOLDERSETTINGS, _: Ref<IShellBrowser>, _: *const RECT) -> Result<HWND> { nope() }
    fn DestroyViewWindow(&self) -> Result<()> { Ok(()) }
    fn GetCurrentInfo(&self) -> Result<FOLDERSETTINGS> { nope() }
    fn AddPropertySheetPages(&self, _: u32, _: LPFNSVADDPROPSHEETPAGE, _: LPARAM) -> Result<()> { nope() }
    fn SaveViewState(&self) -> Result<()> { Ok(()) }
    fn SelectItem(&self, pidl: *const ITEMIDLIST, _flags: u32) -> Result<()> {
       
        let path = item_path(pidl, &cur(&self.dir)).ok_or(E_FAIL)?;
        if self.resident {
            // a window shows the folder already: it picks the file, as Explorer's own window would (no second window)
            if hand_to_open_window(&cur(&self.dir), &path) {
                return Ok(());
            }
            let exe = std::env::current_exe().map_err(|_| E_FAIL)?;
            std::process::Command::new(exe).raw_arg(format!("/select,\"{}\"", path.display())).spawn().map_err(|_| E_FAIL)?;
        } else {
            unsafe { PostMessageW(Some(HWND(self.hwnd as *mut c_void)), WM_SHELL_SELECT, WPARAM(0), LPARAM(Box::into_raw(Box::new(path)) as isize))? };
        }
        Ok(())
    }
    fn GetItemObject(&self, _: &_SVGIO, _: *const GUID, _: *mut *mut c_void) -> Result<()> { nope() }
}

/// The file system path of the item the shell sent: relative to the folder we showed (or absolute).
fn item_path(pidl: *const ITEMIDLIST, dir: &str) -> Option<PathBuf> {
    let name = |p: *const ITEMIDLIST| unsafe {
        let s = SHGetNameFromIDList(p, SIGDN_FILESYSPATH).ok()?;
        let r = s.to_string().ok();
        CoTaskMemFree(Some(s.0 as *const c_void));
        r.map(PathBuf::from)
    };
    // the item comes relative to whichever form of the folder the caller found us by
    let forms = pidl_forms(Path::new(dir));
    let joined = forms.iter().find_map(|base| unsafe {
        let abs = ILCombine(Some(*base), Some(pidl));
        let p = name(abs);
        ILFree(Some(abs));
        p.filter(|p| p.parent().is_some_and(|d| d.as_os_str().eq_ignore_ascii_case(dir)))
    });
    free_forms(forms);
    joined.or_else(|| name(pidl))
}

// the rest of the browser interface: nothing to offer
impl IWebBrowser_Impl for Browser_Impl {
    fn GoBack(&self) -> Result<()> { nope() }
    fn GoForward(&self) -> Result<()> { nope() }
    fn GoHome(&self) -> Result<()> { nope() }
    fn GoSearch(&self) -> Result<()> { nope() }
    fn Navigate(&self, _url: &BSTR, _flags: *const windows::Win32::System::Variant::VARIANT, _targetframename: *const windows::Win32::System::Variant::VARIANT, _postdata: *const windows::Win32::System::Variant::VARIANT, _headers: *const windows::Win32::System::Variant::VARIANT) -> Result<()> { nope() }
    fn Refresh(&self) -> Result<()> { nope() }
    fn Refresh2(&self, _level: *const windows::Win32::System::Variant::VARIANT) -> Result<()> { nope() }
    fn Stop(&self) -> Result<()> { nope() }
    fn Application(&self) -> Result<windows::Win32::System::Com::IDispatch> { nope() }
    fn Parent(&self) -> Result<windows::Win32::System::Com::IDispatch> { nope() }
    fn Container(&self) -> Result<windows::Win32::System::Com::IDispatch> { nope() }
    fn Document(&self) -> Result<windows::Win32::System::Com::IDispatch> { Ok(Doc { hwnd: self.hwnd, dir: self.dir.clone(), resident: self.resident }.into()) }
    fn TopLevelContainer(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn Type(&self) -> Result<BSTR> { nope() }
    fn Left(&self) -> Result<i32> { nope() }
    fn SetLeft(&self, _left: i32) -> Result<()> { nope() }
    fn Top(&self) -> Result<i32> { nope() }
    fn SetTop(&self, _top: i32) -> Result<()> { nope() }
    fn Width(&self) -> Result<i32> { nope() }
    fn SetWidth(&self, _width: i32) -> Result<()> { nope() }
    fn Height(&self) -> Result<i32> { nope() }
    fn SetHeight(&self, _height: i32) -> Result<()> { nope() }
    fn LocationName(&self) -> Result<BSTR> { Ok(BSTR::from(url(&self.dir))) }
    fn LocationURL(&self) -> Result<BSTR> { Ok(BSTR::from(url(&self.dir))) }
    fn Busy(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
}
impl IWebBrowserApp_Impl for Browser_Impl {
    fn Quit(&self) -> Result<()> { nope() }
    fn ClientToWindow(&self, _pcx: *mut i32, _pcy: *mut i32) -> Result<()> { nope() }
    fn PutProperty(&self, _property: &BSTR, _vtvalue: &windows::Win32::System::Variant::VARIANT) -> Result<()> { nope() }
    fn GetProperty(&self, _property: &BSTR) -> Result<windows::Win32::System::Variant::VARIANT> { nope() }
    fn Name(&self) -> Result<BSTR> { nope() }
    fn HWND(&self) -> Result<windows::Win32::Foundation::SHANDLE_PTR> { Ok(SHANDLE_PTR(self.hwnd)) }
    fn FullName(&self) -> Result<BSTR> { nope() }
    fn Path(&self) -> Result<BSTR> { nope() }
    fn Visible(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { Ok(VARIANT_TRUE) }
    fn SetVisible(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn StatusBar(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetStatusBar(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn StatusText(&self) -> Result<BSTR> { nope() }
    fn SetStatusText(&self, _statustext: &BSTR) -> Result<()> { nope() }
    fn ToolBar(&self) -> Result<i32> { nope() }
    fn SetToolBar(&self, _value: i32) -> Result<()> { nope() }
    fn MenuBar(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetMenuBar(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn FullScreen(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetFullScreen(&self, _bfullscreen: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
}
impl IWebBrowser2_Impl for Browser_Impl {
    fn Navigate2(&self, _url: *const windows::Win32::System::Variant::VARIANT, _flags: *const windows::Win32::System::Variant::VARIANT, _targetframename: *const windows::Win32::System::Variant::VARIANT, _postdata: *const windows::Win32::System::Variant::VARIANT, _headers: *const windows::Win32::System::Variant::VARIANT) -> Result<()> { nope() }
    fn QueryStatusWB(&self, _cmdid: windows::Win32::System::Ole::OLECMDID) -> Result<windows::Win32::System::Ole::OLECMDF> { nope() }
    fn ExecWB(&self, _cmdid: windows::Win32::System::Ole::OLECMDID, _cmdexecopt: windows::Win32::System::Ole::OLECMDEXECOPT, _pvain: *const windows::Win32::System::Variant::VARIANT, _pvaout: *mut windows::Win32::System::Variant::VARIANT) -> Result<()> { nope() }
    fn ShowBrowserBar(&self, _pvaclsid: *const windows::Win32::System::Variant::VARIANT, _pvarshow: *const windows::Win32::System::Variant::VARIANT, _pvarsize: *const windows::Win32::System::Variant::VARIANT) -> Result<()> { nope() }
    fn ReadyState(&self) -> Result<windows::Win32::System::Ole::READYSTATE> { nope() }
    fn Offline(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetOffline(&self, _boffline: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn Silent(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetSilent(&self, _bsilent: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn RegisterAsBrowser(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetRegisterAsBrowser(&self, _bregister: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn RegisterAsDropTarget(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetRegisterAsDropTarget(&self, _bregister: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn TheaterMode(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetTheaterMode(&self, _bregister: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn AddressBar(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetAddressBar(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
    fn Resizable(&self) -> Result<windows::Win32::Foundation::VARIANT_BOOL> { nope() }
    fn SetResizable(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> Result<()> { nope() }
}

#[cfg(test)]
mod tests {
    /// Brave's "Show in folder" for a download found no window: the known folder was registered only in its known-folder form.
    #[test]
    fn a_known_folder_is_registered_in_both_forms_an_ordinary_one_once() {
        unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); }
        let plain = super::pidl_forms(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
        assert_eq!(plain.len(), 1, "an ordinary folder has one form");
        super::free_forms(plain);
        let dl = crate::sh::known_dir(&FOLDERID_Downloads).expect("Downloads");
        let both = super::pidl_forms(&dl);
        assert_eq!(both.len(), 2, "{} as the known folder and as This PC > drive > folder", dl.display());
        // the second is the form Chromium and ILCreateFromPath build
        let w: Vec<u16> = dl.as_os_str().encode_wide().chain([0]).collect();
        let fs = unsafe { ILCreateFromPathW(PCWSTR(w.as_ptr())) };
        assert!(unsafe { ILIsEqual(both[1], fs) }.as_bool());
        unsafe { ILFree(Some(fs)) };
        super::free_forms(both);
    }
    use super::*;
}