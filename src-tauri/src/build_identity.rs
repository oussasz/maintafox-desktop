//! Keeps development builds apart from the installed app on the same PC.
//!
//! A build with `debug_assertions` (`tauri dev`, `cargo test`) uses its own Tauri identifier
//! (`<identifier>.dev`) and its own OS credential-store service. So its data folder (database,
//! attachments, backups, update journal), its WebView storage, its logs, its single-instance lock,
//! its database key, its device secret and its activation never touch those of an installed
//! version (`tauri build`, release profile), which keeps `tauri.conf.json`'s identifier.

/// Appended to the identifier of builds with `debug_assertions`.
pub const DEV_IDENTIFIER_SUFFIX: &str = ".dev";

/// OS credential-store service for the database key, device secret and activation secrets.
pub const KEYRING_SERVICE: &str = if cfg!(debug_assertions) {
    "maintafox-desktop-dev"
} else {
    "maintafox-desktop"
};

/// The identifier this build runs under, given `tauri.conf.json`'s identifier.
pub fn identifier_for_build(configured: &str) -> String {
    if cfg!(debug_assertions) && !configured.ends_with(DEV_IDENTIFIER_SUFFIX) {
        format!("{configured}{DEV_IDENTIFIER_SUFFIX}")
    } else {
        configured.to_string()
    }
}

/// Sets the identifier for this build before the app starts (every app path derives from it).
pub fn apply<R: tauri::Runtime>(context: &mut tauri::Context<R>) {
    let identifier = identifier_for_build(&context.config().identifier);
    context.config_mut().identifier = identifier;
}

#[cfg(test)]
mod tests {
    use super::*;

    // The test binary is a `debug_assertions` build, so it shows the development behaviour.
    #[test]
    fn dev_build_gets_its_own_identifier_and_credential_service() {
        assert_eq!(
            identifier_for_build("systems.maintafox.desktop"),
            "systems.maintafox.desktop.dev"
        );
        assert_eq!(
            identifier_for_build("systems.maintafox.desktop.dev"),
            "systems.maintafox.desktop.dev"
        );
        assert_eq!(KEYRING_SERVICE, "maintafox-desktop-dev");
    }
}
