#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unnecessary_literal_bound)]
// Pedantic lints — standard allows for DB-centric code patterns (Rust 1.94+)
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::large_stack_arrays)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::unnecessary_debug_formatting)]
// Nursery lints — unstable, often false positives
#![allow(clippy::option_if_let_else)]
#![allow(clippy::significant_drop_tightening)]

pub mod audit;
pub mod auth;
pub mod background;
pub mod backup;
pub mod commands;
pub mod db;
pub mod diagnostics;
pub mod errors;
pub mod locale;
pub mod migrations;
pub mod models;
pub mod org;
pub mod repository;
pub mod security;
pub mod services;
pub mod settings;
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

#[allow(clippy::large_stack_frames)]
pub fn run() {
    // Record application start time before anything else so uptime is accurate.
    diagnostics::record_start_time();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Single-instance: focus existing window on second launch
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                w.set_focus().ok();
                w.show().ok();
            }
        }))
        .setup(|app| {
            // Initialize file-based logging as the very first setup action so that
            // all subsequent startup messages are captured in the rolling log file.
            // The WorkerGuard flushes and closes the file on drop; Box::leak keeps
            // it alive for the entire process lifetime (acceptable in Phase 1).
            let log_dir_path = app
                .handle()
                .path()
                .app_log_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("logs"));
            std::fs::create_dir_all(&log_dir_path)?;
            let guard = diagnostics::init_file_logging(log_dir_path);
            Box::leak(Box::new(guard));

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
            commands::auth::login,
            commands::auth::logout,
            commands::auth::get_session_info,
            commands::auth::get_device_trust_status,
            commands::auth::revoke_device_trust,
            // SP04-F05 — auth UI support
            commands::auth::unlock_session,
            commands::auth::force_change_password,
            commands::lookup::list_lookup_domains,
            commands::lookup::get_lookup_values,
            commands::lookup::get_lookup_value_by_id,
            commands::diagnostics::run_integrity_check,
            commands::diagnostics::repair_seed_data,
            commands::rbac::get_my_permissions,
            commands::rbac::verify_step_up,
            commands::locale::get_locale_preference,
            commands::locale::set_locale_preference,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_policy_snapshot,
            commands::settings::get_session_policy,
            commands::settings::list_setting_change_events,
            commands::updater::check_for_update,
            commands::updater::install_pending_update,
            commands::diagnostics::get_diagnostics_info,
            commands::diagnostics::generate_support_bundle,
            commands::backup::run_manual_backup,
            commands::backup::list_backup_runs,
            commands::backup::validate_backup_file,
            commands::backup::factory_reset_stub,
            // ── Organization (SP01-F01) ──────────────────────────────────
            commands::org::list_org_structure_models,
            commands::org::get_active_org_structure_model,
            commands::org::create_org_structure_model,
            commands::org::publish_org_structure_model,
            commands::org::archive_org_structure_model,
            commands::org::list_org_node_types,
            commands::org::create_org_node_type,
            commands::org::deactivate_org_node_type,
            commands::org::list_org_relationship_rules,
            commands::org::create_org_relationship_rule,
            commands::org::delete_org_relationship_rule,
        ])
        .run(tauri::generate_context!())
        // EXPECT: If the Tauri context cannot be loaded, the application binary is corrupt or
        // the tauri.conf.json is missing. Panic at startup is the correct behavior.
        .expect("error while running Maintafox application");
}
