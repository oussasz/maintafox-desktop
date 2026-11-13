//! Secret storage with the `keyring` API on every platform.
//!
//! Callers use `Entry::new(service, name)`, `get_password()`, `set_password()` and match
//! `Error::NoEntry` exactly as they did with `keyring`:
//! - desktop: this *is* `keyring` (re-export), so desktop behaviour is byte-for-byte unchanged;
//! - Android: the same calls go to the `maintafox-secure-store` mobile plugin (Kotlin), which keeps
//!   each value encrypted under an Android Keystore key in the app's no-backup storage.

#[cfg(not(target_os = "android"))]
pub use keyring::{Entry, Error};

#[cfg(target_os = "android")]
pub use android::{device_is_secure, plugin, Entry, Error};

#[cfg(target_os = "android")]
mod android {
    use std::sync::OnceLock;

    use serde::{Deserialize, Serialize};
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::Wry;

    /// Kotlin class registered by [`plugin`] (in `gen/android/app/src/main/java/`).
    const PLUGIN_PACKAGE: &str = "systems.maintafox.mobile";
    const PLUGIN_CLASS: &str = "SecureStorePlugin";
    /// Error code the Kotlin side returns when the phone has no screen lock.
    const NO_SCREEN_LOCK_CODE: &str = "NO_SCREEN_LOCK";

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    /// The plugin that connects the store to the Android Keystore. Registered once in `lib.rs`
    /// (mobile builds only). The webview is granted no permission on it: only Rust calls it.
    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("maintafox-secure-store")
            .setup(|_app, api| {
                let handle = api.register_android_plugin(PLUGIN_PACKAGE, PLUGIN_CLASS)?;
                HANDLE
                    .set(handle)
                    .map_err(|_| "maintafox-secure-store registered twice".to_string())?;
                Ok(())
            })
            .build()
    }

    #[derive(Debug)]
    pub enum Error {
        /// No value stored under this name (same meaning as `keyring::Error::NoEntry`).
        NoEntry,
        /// The phone has no screen lock (PIN, pattern, password): the store refuses to work.
        NoScreenLock,
        /// The plugin is not registered yet (called before the Tauri setup ran).
        NotReady,
        /// Keystore or file failure reported by Android.
        PlatformFailure(String),
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::NoEntry => f.write_str("no secret stored under this name"),
                Self::NoScreenLock => f.write_str(
                    "the phone has no screen lock; Maintafox requires one to protect its data \
                     (le téléphone n'a pas de verrouillage d'écran)",
                ),
                Self::NotReady => f.write_str("Android secure store not initialised"),
                Self::PlatformFailure(m) => write!(f, "Android secure store failure: {m}"),
            }
        }
    }

    impl std::error::Error for Error {}

    fn handle() -> Result<&'static PluginHandle<Wry>, Error> {
        HANDLE.get().ok_or(Error::NotReady)
    }

    fn map_invoke_error(e: tauri::plugin::mobile::PluginInvokeError) -> Error {
        use tauri::plugin::mobile::PluginInvokeError;
        match e {
            PluginInvokeError::InvokeRejected(resp) if resp.code.as_deref() == Some(NO_SCREEN_LOCK_CODE) => {
                Error::NoScreenLock
            }
            other => Error::PlatformFailure(other.to_string()),
        }
    }

    #[derive(Serialize)]
    struct NameArgs<'a> {
        name: &'a str,
    }

    #[derive(Serialize)]
    struct SetArgs<'a> {
        name: &'a str,
        value: &'a str,
    }

    #[derive(Deserialize)]
    struct GetResponse {
        value: Option<String>,
    }

    #[derive(Deserialize)]
    struct SecureResponse {
        secure: bool,
    }

    /// Whether the phone has a screen lock (`KeyguardManager.isDeviceSecure`).
    pub fn device_is_secure() -> Result<bool, Error> {
        let resp: SecureResponse = handle()?
            .run_mobile_plugin("deviceSecure", ())
            .map_err(map_invoke_error)?;
        Ok(resp.secure)
    }

    /// One named secret; mirrors `keyring::Entry`.
    pub struct Entry {
        /// `service/name`, so dev and release builds never share a value (as on desktop).
        name: String,
    }

    impl Entry {
        pub fn new(service: &str, name: &str) -> Result<Self, Error> {
            Ok(Self {
                name: format!("{service}/{name}"),
            })
        }

        pub fn get_password(&self) -> Result<String, Error> {
            let resp: GetResponse = handle()?
                .run_mobile_plugin("get", NameArgs { name: &self.name })
                .map_err(map_invoke_error)?;
            resp.value.ok_or(Error::NoEntry)
        }

        pub fn set_password(&self, value: &str) -> Result<(), Error> {
            handle()?
                .run_mobile_plugin::<serde_json::Value>(
                    "set",
                    SetArgs {
                        name: &self.name,
                        value,
                    },
                )
                .map_err(map_invoke_error)?;
            Ok(())
        }
    }
}
