//! Android installation identity: the phone's replacement for the desktop machine id.
//!
//! Desktop builds hash the OS machine id (`MachineGuid`, `/etc/machine-id`, `IOPlatformUUID`).
//! Android gives apps no stable hardware id they should use (and reading one would be tracking on
//! a personal phone), so the app makes its own: a random UUID created at first launch and kept in
//! the Keystore-protected secret store. It survives app updates and reboots; uninstalling the app
//! or clearing its data makes a new one, exactly like reinstalling the OS on a PC.

use crate::build_identity::KEYRING_SERVICE;
use crate::platform::secret_store::{Entry, Error};

const INSTALLATION_ID_ENTRY: &str = "android-installation-id";

/// The installation id, created on first call. Fails (instead of returning a shared placeholder)
/// when the secret store is unavailable, e.g. no screen lock: two phones must never share an id.
pub fn get_or_create() -> Result<String, Error> {
    let entry = Entry::new(KEYRING_SERVICE, INSTALLATION_ID_ENTRY)?;
    match entry.get_password() {
        Ok(id) => Ok(id),
        Err(Error::NoEntry) => {
            let id = uuid::Uuid::new_v4().to_string();
            entry.set_password(&id)?;
            Ok(id)
        }
        Err(e) => Err(e),
    }
}

/// Replaces the installation id with a new one, as a reinstall would: done once a removed phone
/// has erased its data (`sync::transport::removal`), so its next enrolment is a new phone for the
/// licence server and never meets the removed activation again.
pub fn regenerate() -> Result<String, Error> {
    let entry = Entry::new(KEYRING_SERVICE, INSTALLATION_ID_ENTRY)?;
    let id = uuid::Uuid::new_v4().to_string();
    entry.set_password(&id)?;
    Ok(id)
}
