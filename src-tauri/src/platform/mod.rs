//! Platform services that differ between the desktop and the Android field app.
//!
//! Business rules never live here: this module only gives the engine the same building blocks
//! (secret storage, a stable installation identity) on every platform.
//!
//! - Desktop (Windows/macOS/Linux): the OS credential store through `keyring`, unchanged.
//! - Android: secrets wrapped by an AES-256-GCM key held in the Android Keystore
//!   (`gen/android/.../SecureStorePlugin.kt`). The phone must have a screen lock: without one the
//!   store refuses to read or write, so the app cannot activate or open its database.

pub mod secret_store;

#[cfg(target_os = "android")]
pub mod installation_id;
