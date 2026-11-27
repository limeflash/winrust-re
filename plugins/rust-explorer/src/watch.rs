//! A watcher of one folder, for the file manager window and for the desktop: another program (or we) creates, deletes, renames or
//! changes something in it, and the owner is told, once for a burst, so that it lists the folder again. `FindFirstChangeNotificationW`
//! says only "something changed" and that is all that is needed: the folder is listed again anyway. Not recursive.
//!
//! One thread per watch waits on the change handle and a stop event. A burst (a browser writing a download) is coalesced by
//! [`Burst`]: one notification after [`QUIET`] without a change, and at the latest [`LIMIT`] after the first. Dropping the [`Watch`]
//! sets the stop event, joins the thread and closes the handles. A folder that is gone (deleted, its drive removed) is reported once
//! as [`Event::Gone`] and the thread ends: a failing handle is never waited on again.
use std::{
    path::Path,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification,
        FindFirstChangeNotificationW, FindNextChangeNotification,
    },
    System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects},
};

/// How long the folder must be quiet before the owner is told.
pub const QUIET: Duration = Duration::from_millis(200);
/// The longest a burst of changes may keep the owner waiting.
pub const LIMIT: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Something in the folder changed.
    Changed,
    /// The folder is gone (or its drive): nothing more will come.
    Gone,
}

/// The coalescing of a burst of changes, with no clock of its own (the tests give it the times).
#[derive(Default, Debug)]
pub struct Burst {
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Burst {
    /// A change was seen at `now`.
    pub fn changed(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// How long to wait for more changes before the owner must be told (None: nothing is pending, wait for ever).
    #[must_use]
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        let (first, last) = (self.first?, self.last?);
        let due = (last + QUIET).min(first + LIMIT);
        Some(due.saturating_duration_since(now))
    }

    /// Whether the owner is to be told now; if so the burst is over.
    pub fn due(&mut self, now: Instant) -> bool {
        match self.wait(now) {
            Some(d) if d.is_zero() => {
                *self = Self::default();
                true
            }
            _ => false,
        }
    }
}

pub struct Watch {
    stop: isize,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain([0]).collect()
}

impl Watch {
    /// Starts watching `dir`; `tell` runs on the watch's thread (it posts a message to a window). None when the folder cannot be watched
    /// (it is not a folder, or not on a drive that reports changes).
    pub fn start(dir: &Path, tell: impl Fn(Event) + Send + 'static) -> Option<Self> {
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_ATTRIBUTES;
        // SAFETY: a NUL-terminated path; the handles are closed by the thread (the change) and by the guard (the event).
        let (change, stop) = unsafe { (FindFirstChangeNotificationW(wide(dir).as_ptr(), 0, filter), CreateEventW(std::ptr::null(), 1, 0, std::ptr::null())) };
        if change == INVALID_HANDLE_VALUE || change.is_null() || stop.is_null() {
            // SAFETY: closing what was made.
            unsafe {
                if change != INVALID_HANDLE_VALUE && !change.is_null() {
                    FindCloseChangeNotification(change);
                }
                if !stop.is_null() {
                    CloseHandle(stop);
                }
            }
            return None;
        }
        let (dir, change_h, stop_h) = (dir.to_path_buf(), change as isize, stop as isize);
        let thread = std::thread::Builder::new().name("folder-watch".into()).spawn(move || run(&dir, change_h as HANDLE, stop_h as HANDLE, &tell)).ok()?;
        Some(Self { stop: stop as isize, thread: Some(thread) })
    }
}

fn run(dir: &Path, change: HANDLE, stop: HANDLE, tell: &dyn Fn(Event)) {
    let mut burst = Burst::default();
    loop {
        let now = Instant::now();
        let ms = burst.wait(now).map_or(u32::MAX, |d| d.as_millis().min(u128::from(u32::MAX - 1)) as u32);
        let handles = [change, stop];
        // SAFETY: two live handles.
        let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, ms) };
        if r == WAIT_OBJECT_0 {
            // a change: ask for the next one, and keep the burst
            // SAFETY: the change handle is ours.
            let ok = unsafe { FindNextChangeNotification(change) } != 0;
            burst.changed(Instant::now());
            if !ok {
                tell(Event::Gone);
                break;
            }
        } else if r == WAIT_OBJECT_0 + 1 {
            break; // stopped
        } else if r == 0x102 {
            // timeout: the burst is quiet (or has gone on long enough)
            if burst.due(Instant::now()) {
                if dir.is_dir() {
                    tell(Event::Changed);
                } else {
                    tell(Event::Gone);
                    break;
                }
            }
        } else {
            tell(Event::Gone); // a failing handle: nothing to wait on
            break;
        }
        if burst.due(Instant::now()) {
            if dir.is_dir() {
                tell(Event::Changed);
            } else {
                tell(Event::Gone);
                break;
            }
        }
    }
    // SAFETY: the change handle is the thread's.
    unsafe { FindCloseChangeNotification(change) };
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: the event is the guard's; the thread is told to stop and joined before it is closed.
        unsafe { SetEvent(self.stop as HANDLE) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: the thread is gone.
        unsafe { CloseHandle(self.stop as HANDLE) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_is_told_once_after_a_quiet_moment_and_never_later_than_the_limit() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut b = Burst::default();
        assert_eq!(b.wait(t0), None, "nothing pending: wait for ever");
        b.changed(t0);
        assert_eq!(b.wait(t0), Some(QUIET));
        assert!(!b.due(t0 + ms(100)));
        // another change moves the quiet moment
        b.changed(t0 + ms(150));
        assert!(!b.due(t0 + ms(300)));
        assert!(b.due(t0 + ms(350)), "200 ms after the last change");
        assert_eq!(b.wait(t0 + ms(400)), None, "and the burst is over");
        // a burst that never goes quiet is told 1 s after its first change
        let mut b = Burst::default();
        for k in 0..20 {
            b.changed(t0 + ms(k * 100));
        }
        assert!(!b.due(t0 + ms(999)));
        assert!(b.due(t0 + LIMIT));
    }

    #[test]
    fn a_folder_that_is_not_there_cannot_be_watched() {
        assert!(Watch::start(Path::new(r"C:\no\such\folder\at\all"), |_| {}).is_none());
    }

    #[test]
    fn a_watch_tells_of_a_new_file_once_and_stops_when_dropped() {
        use std::sync::{Arc, Mutex};
        let d = std::env::temp_dir().join(format!("rx-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let w = Watch::start(&d, move |e| s2.lock().unwrap().push(e)).expect("a temp folder can be watched");
        for i in 0..5 {
            std::fs::write(d.join(format!("f{i}")), b"x").unwrap();
        }
        std::thread::sleep(Duration::from_millis(900));
        assert_eq!(*seen.lock().unwrap(), [Event::Changed], "five files, one telling");
        let _ = std::fs::remove_dir_all(&d);
        std::thread::sleep(Duration::from_millis(900));
        assert!(seen.lock().unwrap().contains(&Event::Gone), "the folder went");
        drop(w); // joins: the thread has ended already, or ends now
    }
}
