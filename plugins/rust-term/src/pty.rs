//! Windows pseudo console and its terminal reader.
use crate::vt::term::Term;
use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
    InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTUPINFOEXW,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};

pub const PWSH_INIT: &str = include_str!("term-init.ps1");
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (null_mut(), null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedHandle::from_raw_handle(read), OwnedHandle::from_raw_handle(write)) })
}
fn dimensions(cols: u16, rows: u16) -> io::Result<COORD> {
    if cols == 0 || rows == 0 || cols > i16::MAX as u16 || rows > i16::MAX as u16 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid console dimensions"));
    }
    Ok(COORD {
        X: cols as i16,
        Y: rows as i16,
    })
}
fn hresult(hr: i32) -> io::Result<()> {
    if hr < 0 {
        Err(io::Error::from_raw_os_error(hr))
    } else {
        Ok(())
    }
}
struct Console(HPCON);
impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            ClosePseudoConsole(self.0);
        }
    }
}
struct Attributes(*mut c_void);
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.0);
        }
    }
}

pub struct Pty {
    console: Option<Console>,
    input: File,
    output: Option<PtyReader>,
    process: OwnedHandle,
}
pub struct PtyReader(File);
impl Read for PtyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}
impl Pty {
    pub fn spawn(cmdline: &str, cwd: Option<&Path>, cols: u16, rows: u16, env: &[(String, String)]) -> io::Result<Self> {
        let size = dimensions(cols, rows)?;
        if cmdline.contains('\0')
            || cwd.is_some_and(|p| p.as_os_str().encode_wide().any(|c| c == 0))
            || env
                .iter()
                .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL or invalid environment key"));
        }
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;
        let mut console = 0;
        hresult(unsafe {
            CreatePseudoConsole(
                size,
                input_read.as_raw_handle(),
                output_write.as_raw_handle(),
                0,
                &mut console,
            )
        })?;
        let console = Console(console);
        drop(input_read);
        drop(output_write);
        let mut bytes = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
        }
        let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        let ptr = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(ptr, 1, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let attrs = Attributes(ptr);
        if unsafe {
            UpdateProcThreadAttribute(
                attrs.0,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console.0 as *const c_void,
                std::mem::size_of::<HPCON>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // empty std handles: without this a child of a process whose stdout is redirected (cargo test, a service) writes
        // there instead of into the pseudo console
        startup.StartupInfo.dwFlags = windows_sys::Win32::System::Threading::STARTF_USESTDHANDLES;
        startup.lpAttributeList = attrs.0;
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let mut command = wide(std::ffi::OsStr::new(cmdline));
        let directory = cwd.map(|p| wide(p.as_os_str()));
        let mut vars: Vec<(String, String)> = std::env::vars().collect();
        for (key, value) in env {
            vars.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
            vars.push((key.clone(), value.clone()));
        }
        vars.sort_by_key(|(k, _)| k.to_uppercase());
        let mut environment: Vec<u16> = vars
            .iter()
            .flat_map(|(k, v)| format!("{k}={v}\0").encode_utf16().collect::<Vec<_>>())
            .collect();
        environment.push(0);
        let ok = unsafe {
            CreateProcessW(
                null(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                directory.as_ref().map_or(null(), |v| v.as_ptr()),
                &startup.StartupInfo,
                &mut process,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            CloseHandle(process.hThread);
        }
        Ok(Self {
            console: Some(console),
            input: File::from(input_write),
            output: Some(PtyReader(File::from(output_read))),
            process: unsafe { OwnedHandle::from_raw_handle(process.hProcess) },
        })
    }
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        (&self.input).write_all(bytes)
    }
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        hresult(unsafe { ResizePseudoConsole(self.console.as_ref().unwrap().0, dimensions(cols, rows)?) })
    }
    pub fn reader(&mut self) -> Option<PtyReader> {
        self.output.take()
    }
    pub fn exit_code(&self) -> Option<u32> {
        self.wait(0)
    }
    pub fn wait(&self, timeout_ms: u32) -> Option<u32> {
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), timeout_ms) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
            None
        } else {
            Some(code)
        }
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        if self.exit_code().is_none() {
            unsafe {
                TerminateProcess(self.process.as_raw_handle(), 1);
            }
        }
        drop(self.output.take());
        drop(self.console.take());
    }
}

pub struct Session {
    pty: Pty,
    term: Arc<Mutex<Term>>,
    reader: Option<JoinHandle<()>>,
}
impl Session {
    pub fn new(cmdline: &str, cwd: Option<&Path>, cols: u16, rows: u16, notify: Box<dyn Fn() + Send>) -> io::Result<Self> {
        let mut pty = Pty::spawn(cmdline, cwd, cols, rows, &[])?;
        let mut output = pty.reader().unwrap();
        let input = pty.input.try_clone()?;
        let term = Arc::new(Mutex::new(Term::new(cols as usize, rows as usize)));
        let shared = Arc::clone(&term);
        let reader = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match output.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let replies = {
                            let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
                            t.feed(&buf[..n]);
                            t.take_replies()
                        };
                        if !replies.is_empty() {
                            let _ = (&input).write_all(&replies);
                        }
                        notify();
                    }
                }
            }
        });
        Ok(Self {
            pty,
            term,
            reader: Some(reader),
        })
    }
    pub fn term(&self) -> Arc<Mutex<Term>> {
        Arc::clone(&self.term)
    }
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        self.pty.write(bytes)
    }
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.pty.resize(cols, rows)?;
        self.term
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resize(cols as usize, rows as usize);
        Ok(())
    }
    pub fn exited(&self) -> Option<u32> {
        self.pty.exit_code()
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.pty.exit_code().is_none() {
            unsafe {
                TerminateProcess(self.pty.process.as_raw_handle(), 1);
            }
        }
        drop(self.pty.console.take());
        if let Some(thread) = self.reader.take() {
            let _ = thread.join();
        }
    }
}

/// Windows PowerShell or pwsh with the shell integration (PWSH_INIT). The script goes in as -EncodedCommand: no file to
/// ship, no quoting, and the execution policy (Restricted on a fresh Windows) does not apply to it as it would to a .ps1.
pub fn pwsh_cmdline(exe: &str) -> String {
    let utf16le: Vec<u8> = PWSH_INIT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!("\"{exe}\" -NoLogo -NoExit -EncodedCommand {}", base64(&utf16le))
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}
pub fn cmd_prompt_env() -> (String, String) {
    ("PROMPT".into(), "$e]133;D$e\\$e]133;A$e\\$P$G$e]133;B$e\\".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vt::blocks::Blocks;
    use std::time::{Duration, Instant};
    fn session(cmd: &str) -> Session {
        Session::new(cmd, None, 100, 30, Box::new(|| {})).unwrap()
    }
    fn until(mut f: impl FnMut() -> bool, seconds: u64) {
        let end = Instant::now() + Duration::from_secs(seconds);
        while !f() {
            assert!(Instant::now() < end, "timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    #[ignore = "needs ConPTY: run with --ignored"]
    fn echo_and_exit() {
        let s = session("cmd.exe /d /c echo hello");
        until(|| s.exited().is_some(), 10);
        until(
            || {
                let term = s.term();
                let t = term.lock().unwrap();
                (t.first_line()..t.end_line()).any(|n| t.line_text(n).is_some_and(|x| x.contains("hello")))
            },
            10,
        );
        assert_eq!(s.exited(), Some(0));
    }
    #[test]
    #[ignore = "needs ConPTY: run with --ignored"]
    fn exit_three() {
        let s = session("cmd.exe /d /c exit 3");
        until(|| s.exited().is_some(), 10);
        assert_eq!(s.exited(), Some(3));
    }
    #[test]
    #[ignore = "needs ConPTY: run with --ignored"]
    fn resize_and_write() {
        let s = session("cmd.exe /d /k");
        s.resize(80, 24).unwrap();
        s.write(b"exit 5\r").unwrap();
        until(|| s.exited().is_some(), 10);
        assert_eq!(s.exited(), Some(5));
    }
    #[test]
    #[ignore = "needs ConPTY: run with --ignored"]
    fn powershell_block() {
        let s = session(&pwsh_cmdline("powershell.exe"));
        let mut blocks = Blocks::default();
        until(
            || {
                let term = s.term();
                let mut t = term.lock().unwrap();
                let marks = t.take_marks();
                blocks.ingest(marks, &t, 0);
                blocks.last().is_some_and(|b| b.input.is_some())
            },
            15,
        );
        s.write(b"cmd /c exit 3\r").unwrap();
        until(
            || {
                let term = s.term();
                let mut t = term.lock().unwrap();
                let marks = t.take_marks();
                blocks.ingest(marks, &t, 0);
                blocks
                    .list()
                    .iter()
                    .any(|b| b.exit == Some(3) && b.command == "cmd /c exit 3")
            },
            15,
        );
        s.write(b"exit\r").unwrap();
        until(|| s.exited().is_some(), 10);
    }
    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xFF, 0xFE, 0xFD]), "//79");
    }
    #[test]
    fn shell_helpers() {
        assert_eq!(cmd_prompt_env().0, "PROMPT");
        let line = pwsh_cmdline("C:\\Program Files\\pwsh.exe");
        assert!(line.starts_with("\"C:\\Program Files\\pwsh.exe\" -NoLogo -NoExit -EncodedCommand "));
        assert!(line.len() < 32_000, "fits a command line");
    }
}
