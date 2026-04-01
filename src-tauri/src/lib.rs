#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod auth;
pub mod background;
pub mod commands;
pub mod db;
pub mod errors;
pub mod models;
pub mod security;
pub mod services;
pub mod sync;

use tracing_subscriber::EnvFilter;

#[allow(clippy::large_stack_frames)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("maintafox=info")))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![commands::health_check,])
        .run(tauri::generate_context!())
        // EXPECT: If the Tauri context cannot be loaded, the application binary is corrupt or
        // the tauri.conf.json is missing. Panic at startup is the correct behavior.
        .expect("error while running Maintafox application");
}
