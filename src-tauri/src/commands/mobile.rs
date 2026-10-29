//! Commands of the Android field app shell (`src/mobile/`).
//!
//! `get_mobile_device_security` needs no session: the shell asks it before activation and sign-in,
//! to refuse a phone without a screen lock (BYOD rule, advisor 2026-10-23). It returns booleans
//! only; no secret ever crosses IPC. `get_last_startup_event` (no session either) gives the shell
//! the startup event it may have missed: the phone's webview loads after the engine has started.

use serde::Serialize;

use crate::errors::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct MobileDeviceSecurity {
    /// `android` on the field app, `desktop` elsewhere.
    pub platform: &'static str,
    /// Whether this platform requires a screen lock before Maintafox may store anything.
    pub screen_lock_required: bool,
    /// Whether the phone has one (`KeyguardManager.isDeviceSecure`); `None` where not applicable.
    pub screen_lock_present: Option<bool>,
}

#[tauri::command]
pub fn get_mobile_device_security() -> AppResult<MobileDeviceSecurity> {
    #[cfg(target_os = "android")]
    {
        let present = crate::platform::secret_store::device_is_secure().map_err(|e| {
            crate::errors::AppError::Internal(anyhow::anyhow!("cannot read the phone's lock status: {e}"))
        })?;
        Ok(MobileDeviceSecurity {
            platform: "android",
            screen_lock_required: true,
            screen_lock_present: Some(present),
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(MobileDeviceSecurity {
            platform: "desktop",
            screen_lock_required: false,
            screen_lock_present: None,
        })
    }
}

/// The last `startup_event` the engine sent (`None` while startup has not reported yet).
#[tauri::command]
pub fn get_last_startup_event() -> Option<crate::startup::StartupEvent> {
    crate::startup::last_startup_event()
}

/// Where this phone's removal stands (`sync::transport::removal`): uploading its pending work,
/// waiting to upload, erasing, done, with what was uploaded and what was dropped. Needs no
/// session: the phone signs its user out when the removal starts. The same status comes as the
/// `phone-removal-status` event at each change. No secret.
#[tauri::command]
pub fn get_phone_removal_status(
    state: tauri::State<'_, crate::state::AppState>,
) -> crate::sync::transport::removal::PhoneRemovalStatus {
    state.sync_worker.removal_status()
}
