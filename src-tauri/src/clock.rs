//! Clock of the business services.
//!
//! The services read the current time here instead of calling `Utc::now()` directly. In the
//! shipped app this is exactly the system clock: the override below only exists in builds with
//! the developer-only feature `demo-company`, which the shipped app never enables.
//!
//! With that feature, the demo-company generator (`src/bin/demo_company.rs`) walks a past year day
//! by day and sets the clock of its own thread to each moment it records, so everything it writes
//! through the services (record dates, audit trail, numbering by year, periods) carries the date
//! it happened on. SQL `'now'` follows the same clock through the generator's SQLite VFS
//! ([`simulated::register_vfs`]).

use chrono::{DateTime, Local, Utc};

/// Current time of the business services (UTC).
pub fn now() -> DateTime<Utc> {
    #[cfg(feature = "demo-company")]
    if let Some(at) = simulated::current() {
        return at;
    }
    Utc::now()
}

/// Current time of the business services in the PC's time zone.
pub fn now_local() -> DateTime<Local> {
    now().with_timezone(&Local)
}

/// Simulated clock of the demo-company generator. Developer-only (feature `demo-company`).
#[cfg(feature = "demo-company")]
pub mod simulated {
    use std::cell::RefCell;
    use std::ffi::{c_int, CString};
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    use chrono::{DateTime, TimeZone, Utc};
    use libsqlite3_sys as ffi;

    /// Milliseconds since the Unix epoch; the clock reads as unset until the first [`SimClock::set`].
    const UNSET: i64 = i64::MIN;
    /// Julian day number of the Unix epoch, in milliseconds (SQLite's `xCurrentTimeInt64` unit).
    const UNIX_EPOCH_JULIAN_MS: i64 = 210_866_760_000_000;

    type CurrentTimeInt64 = unsafe extern "C" fn(*mut ffi::sqlite3_vfs, *mut ffi::sqlite3_int64) -> c_int;

    thread_local! {
        static INSTALLED: RefCell<Option<Arc<AtomicI64>>> = const { RefCell::new(None) };
    }

    /// The simulated time of this thread, when a [`SimClock`] is installed on it and has been set.
    pub(crate) fn current() -> Option<DateTime<Utc>> {
        INSTALLED.with(|slot| {
            let ms = slot.borrow().as_ref()?.load(Ordering::SeqCst);
            (ms != UNSET).then(|| Utc.timestamp_millis_opt(ms).single()).flatten()
        })
    }

    /// One simulated clock, shared by the generator's thread and its database connection.
    #[derive(Clone)]
    pub struct SimClock {
        millis: Arc<AtomicI64>,
    }

    impl Default for SimClock {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SimClock {
        pub fn new() -> Self {
            Self {
                millis: Arc::new(AtomicI64::new(UNSET)),
            }
        }

        pub fn set(&self, at: DateTime<Utc>) {
            self.millis.store(at.timestamp_millis(), Ordering::SeqCst);
        }

        /// Makes [`crate::clock::now`] read this clock on the current thread until the guard drops.
        /// The generator runs on a single-threaded runtime, so every service it awaits runs here.
        pub fn install_on_this_thread(&self) -> InstallGuard {
            let previous = INSTALLED.with(|slot| slot.borrow_mut().replace(self.millis.clone()));
            InstallGuard { previous }
        }
    }

    /// Restores the thread's previous clock (normally the system clock) when dropped.
    pub struct InstallGuard {
        previous: Option<Arc<AtomicI64>>,
    }

    impl Drop for InstallGuard {
        fn drop(&mut self) {
            let previous = self.previous.take();
            INSTALLED.with(|slot| *slot.borrow_mut() = previous);
        }
    }

    /// The default VFS of the platform with its clock replaced by a [`SimClock`]. The VFS struct
    /// must be the first field: SQLite hands the callbacks a pointer to it.
    #[repr(C)]
    struct SimVfs {
        base: ffi::sqlite3_vfs,
        millis: Arc<AtomicI64>,
        default_current_time_int64: Option<CurrentTimeInt64>,
        _name: CString,
    }

    unsafe extern "C" fn current_time_int64(vfs: *mut ffi::sqlite3_vfs, out: *mut ffi::sqlite3_int64) -> c_int {
        // SAFETY: SQLite only calls this with the pointer registered in `register_vfs`, which is the
        // first field of a leaked `SimVfs`.
        let sim = &*(vfs as *const SimVfs);
        let ms = sim.millis.load(Ordering::SeqCst);
        if ms != UNSET {
            *out = ms + UNIX_EPOCH_JULIAN_MS;
            return ffi::SQLITE_OK;
        }
        match sim.default_current_time_int64 {
            Some(default) => default(vfs, out),
            None => ffi::SQLITE_ERROR,
        }
    }

    unsafe extern "C" fn current_time(vfs: *mut ffi::sqlite3_vfs, out: *mut f64) -> c_int {
        let mut ms: ffi::sqlite3_int64 = 0;
        let rc = current_time_int64(vfs, &mut ms);
        if rc == ffi::SQLITE_OK {
            *out = ms as f64 / 86_400_000.0;
        }
        rc
    }

    /// Registers a VFS named `name` that behaves as the platform's default one except that SQL
    /// `'now'` reads `clock`. The generator opens its connection with this VFS name.
    pub fn register_vfs(name: &str, clock: &SimClock) -> Result<(), String> {
        let c_name = CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: plain SQLite C API calls. The registered struct is leaked so it outlives every
        // connection that uses it; it copies the default VFS, which keeps its own `pAppData`.
        unsafe {
            let default = ffi::sqlite3_vfs_find(std::ptr::null());
            if default.is_null() {
                return Err("SQLite has no default VFS".into());
            }
            if (*default).iVersion < 2 {
                return Err("the default SQLite VFS has no 64-bit clock".into());
            }
            let mut base = *default;
            let default_current_time_int64 = base.xCurrentTimeInt64;
            base.pNext = std::ptr::null_mut();
            base.zName = c_name.as_ptr();
            base.xCurrentTime = Some(current_time);
            base.xCurrentTimeInt64 = Some(current_time_int64);
            // Leaked on purpose; the pointer handed to SQLite covers the whole `SimVfs`, so the
            // callbacks may read past `base`.
            let sim = Box::into_raw(Box::new(SimVfs {
                base,
                millis: clock.millis.clone(),
                default_current_time_int64,
                _name: c_name,
            }));
            let rc = ffi::sqlite3_vfs_register(sim.cast::<ffi::sqlite3_vfs>(), 0);
            if rc != ffi::SQLITE_OK {
                return Err(format!("sqlite3_vfs_register failed ({rc})"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn without_a_simulated_clock_now_is_the_system_clock() {
        let before = chrono::Utc::now();
        let now = super::now();
        assert!(now >= before && now <= chrono::Utc::now());
    }

    #[cfg(feature = "demo-company")]
    #[test]
    fn a_simulated_clock_applies_to_its_thread_only_while_installed() {
        use chrono::TimeZone;
        let clock = super::simulated::SimClock::new();
        let at = chrono::Utc.with_ymd_and_hms(2025, 11, 3, 8, 15, 0).unwrap();
        {
            let _guard = clock.install_on_this_thread();
            clock.set(at);
            assert_eq!(super::now(), at);
            let other = std::thread::spawn(super::now).join().unwrap();
            assert_ne!(other, at);
        }
        assert_ne!(super::now(), at);
    }
}
