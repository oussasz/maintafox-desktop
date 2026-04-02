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
pub mod migrations;
pub mod models;
pub mod security;
pub mod services;
pub mod startup;
pub mod state;
pub mod sync;
pub mod tray;
pub mod window;

#[cfg(test)]
mod background_tests;
#[cfg(test)]
mod errors_tests;
#[cfg(test)]
mod startup_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod window_tests;

use tauri::Manager;
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
        // Single-instance: focus existing window on second launch
        .plugin(
            tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(w) = app.get_webview_window("main") {
                    w.set_focus().ok();
                    w.show().ok();
                }
            }),
        )
        .setup(|app| {
            // Tray icon (non-fatal if tray is not supported on this platform)
            if let Err(e) = tray::setup_tray(app) {
                tracing::warn!("System tray unavailable: {e}");
            }

            // Window state (restore previous size/position)
            window::restore_window_state(app)?;

            // Graceful shutdown on main window destruction
            let shutdown_handle = app.handle().clone();
            app.get_webview_window("main")
                .expect("main window must exist")
                .on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        let h = shutdown_handle.clone();
                        tauri::async_runtime::block_on(async move {
                            if let Some(state) = h.try_state::<crate::state::AppState>() {
                                state.tasks.shutdown(5).await;
                            }
                        });
                    }
                });

            // Launch async startup sequence
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = startup::run_startup_sequence(handle).await {
                    tracing::error!("Startup sequence failed: {e}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::app::get_app_info,
            commands::app::get_task_status,
            commands::app::shutdown_app,
        ])
        .run(tauri::generate_context!())
        // EXPECT: If the Tauri context cannot be loaded, the application binary is corrupt or
        // the tauri.conf.json is missing. Panic at startup is the correct behavior.
        .expect("error while running Maintafox application");
}
