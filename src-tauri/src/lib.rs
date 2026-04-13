#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unnecessary_literal_bound)]

pub mod assets;
pub mod activity;
pub mod archive;
pub mod audit;
pub mod auth;
pub mod background;
pub mod backup;
pub mod commands;
pub mod di;
pub mod db;
pub mod wo;
pub mod diagnostics;
pub mod errors;
pub mod locale;
pub mod migrations;
pub mod models;
pub mod notifications;
pub mod org;
pub mod rbac;
pub mod reference;
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
#[cfg(test)]
pub mod observability;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            // ── Core ──────────────────────────────────────────────────────
            commands::health_check,
            commands::app::get_app_info,
            commands::app::get_task_status,
            commands::app::shutdown_app,
            // ── Auth ──────────────────────────────────────────────────────
            commands::auth::login,
            commands::auth::logout,
            commands::auth::get_session_info,
            commands::auth::touch_session,
            commands::auth::get_device_trust_status,
            commands::auth::revoke_device_trust,
            commands::auth::unlock_session,
            commands::auth::set_pin,
            commands::auth::clear_pin,
            commands::auth::unlock_session_with_pin,
            commands::auth::force_change_password,
            // ── Profile ───────────────────────────────────────────────────
            commands::profile::get_my_profile,
            commands::profile::update_my_profile,
            commands::profile::change_password,
            commands::profile::get_session_history,
            commands::profile::list_trusted_devices,
            commands::profile::revoke_my_device,
            // ── RBAC ──────────────────────────────────────────────────────
            commands::rbac::get_my_permissions,
            commands::rbac::verify_step_up,
            commands::rbac::get_rbac_settings,
            commands::rbac::update_rbac_setting,
            commands::rbac::get_password_policy,
            // ── Admin Stats ────────────────────────────────────────────────
            commands::admin_stats::get_admin_stats,
            // ── Admin Users & Roles ───────────────────────────────────────
            commands::admin_users::list_users,
            commands::admin_users::get_user,
            commands::admin_users::create_user,
            commands::admin_users::update_user,
            commands::admin_users::deactivate_user,
            commands::admin_users::assign_role_scope,
            commands::admin_users::revoke_role_scope,
            commands::admin_users::list_roles,
            commands::admin_users::get_role,
            commands::admin_users::create_role,
            commands::admin_users::update_role,
            commands::admin_users::delete_role,
            commands::admin_users::list_role_templates,
            commands::admin_users::simulate_access,
            commands::admin_users::grant_emergency_elevation,
            commands::admin_users::revoke_emergency_elevation,
            commands::admin_users::unlock_user_account,
            commands::admin_users::get_user_presence,
            // ── Admin Governance (SP06-F03) ─────────────────────────────
            commands::admin_governance::list_active_sessions,
            commands::admin_governance::revoke_session,
            commands::admin_governance::list_delegation_policies,
            commands::admin_governance::create_delegation_policy,
            commands::admin_governance::update_delegation_policy,
            commands::admin_governance::delete_delegation_policy,
            commands::admin_governance::list_emergency_grants,
            commands::admin_governance::export_role_model,
            commands::admin_governance::import_role_model,
            // ── Admin Audit (SP06-F04) ──────────────────────────────────
            commands::admin_audit::list_admin_events,
            commands::admin_audit::get_admin_event,
            // ── Admin Permissions ─────────────────────────────────────────
            commands::admin_permissions::list_permissions,
            commands::admin_permissions::get_permission_dependencies,
            commands::admin_permissions::create_custom_permission,
            commands::admin_permissions::validate_role_permissions,
            // ── Locale ────────────────────────────────────────────────────
            commands::locale::get_locale_preference,
            commands::locale::set_locale_preference,
            // ── Settings ──────────────────────────────────────────────────
            commands::settings::list_all_settings,
            commands::settings::list_settings_by_category,
            commands::settings::list_settings_categories,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_policy_snapshot,
            commands::settings::get_session_policy,
            commands::settings::list_setting_change_events,
            // ── Notifications ─────────────────────────────────────────────
            commands::notifications::list_notifications,
            commands::notifications::get_unread_count,
            commands::notifications::mark_notification_read,
            commands::notifications::acknowledge_notification,
            commands::notifications::snooze_notification,
            commands::notifications::get_notification_preferences,
            commands::notifications::update_notification_preference,
            commands::notifications::list_notification_rules,
            commands::notifications::update_notification_rule,
            commands::notifications::list_notification_categories,
            // ── Archive ───────────────────────────────────────────────────
            commands::archive::list_archive_items,
            commands::archive::get_archive_item,
            commands::archive::restore_archive_item,
            commands::archive::export_archive_items,
            commands::archive::purge_archive_items,
            commands::archive::set_legal_hold,
            commands::archive::list_retention_policies,
            commands::archive::update_retention_policy,
            // ── Activity Feed & Audit Log ─────────────────────────────────
            commands::activity_feed::list_activity_events,
            commands::activity_feed::get_activity_event,
            commands::activity_feed::save_activity_filter,
            commands::activity_feed::list_saved_activity_filters,
            commands::activity_feed::get_event_chain,
            commands::audit_log::list_audit_events,
            commands::audit_log::get_audit_event,
            commands::audit_log::export_audit_log,
            // ── Lookup ────────────────────────────────────────────────────
            commands::lookup::list_lookup_domains,
            commands::lookup::get_lookup_values,
            commands::lookup::get_lookup_value_by_id,
            // ── Backup ────────────────────────────────────────────────────
            commands::backup::run_manual_backup,
            commands::backup::list_backup_runs,
            commands::backup::validate_backup_file,
            commands::backup::factory_reset_stub,
            // ── Diagnostics ───────────────────────────────────────────────
            commands::diagnostics::run_integrity_check,
            commands::diagnostics::repair_seed_data,
            commands::diagnostics::get_diagnostics_info,
            commands::diagnostics::generate_support_bundle,
            // ── Updater ───────────────────────────────────────────────────
            commands::updater::check_for_update,
            commands::updater::install_pending_update,
            // ── Org ───────────────────────────────────────────────────────
            commands::org::list_org_structure_models,
            commands::org::get_active_org_structure_model,
            commands::org::create_org_structure_model,
            commands::org::publish_org_structure_model,
            commands::org::archive_org_structure_model,
            commands::org::list_org_node_types,
            commands::org::create_org_node_type,
            commands::org::deactivate_org_node_type,
            commands::org::update_org_node_type,
            commands::org::get_org_node_type_usage_count,
            commands::org::list_org_relationship_rules,
            commands::org::create_org_relationship_rule,
            commands::org::delete_org_relationship_rule,
            commands::org::list_org_tree,
            commands::org::get_org_node,
            commands::org::list_org_node_responsibilities,
            commands::org::list_org_entity_bindings,
            commands::org::create_org_node,
            commands::org::update_org_node_metadata,
            commands::org::assign_org_node_responsibility,
            commands::org::end_org_node_responsibility,
            commands::org::upsert_org_entity_binding,
            commands::org::expire_org_entity_binding,
            commands::org::move_org_node,
            commands::org::deactivate_org_node,
            commands::org::get_org_designer_snapshot,
            commands::org::search_org_designer_nodes,
            commands::org::preview_org_change,
            commands::org::validate_org_model_for_publish,
            commands::org::publish_org_model,
            commands::org::list_org_change_events,
            commands::org::list_org_node_equipment,
            commands::org::search_unassigned_equipment,
            commands::org::assign_equipment_to_node,
            commands::org::unassign_equipment_from_node,
            // ── Reference ─────────────────────────────────────────────────
            commands::reference::list_reference_domains,
            commands::reference::get_reference_domain,
            commands::reference::create_reference_domain,
            commands::reference::update_reference_domain,
            commands::reference::list_reference_sets,
            commands::reference::get_reference_set,
            commands::reference::create_draft_reference_set,
            commands::reference::validate_reference_set,
            commands::reference::publish_reference_set,
            commands::reference::list_reference_values,
            commands::reference::get_reference_value,
            commands::reference::create_reference_value,
            commands::reference::update_reference_value,
            commands::reference::deactivate_reference_value,
            commands::reference::move_reference_value_parent,
            commands::reference::merge_reference_values,
            commands::reference::migrate_reference_usage,
            commands::reference::list_reference_migrations,
            commands::reference::list_reference_aliases,
            commands::reference::get_reference_alias,
            commands::reference::create_reference_alias,
            commands::reference::update_reference_alias,
            commands::reference::delete_reference_alias,
            commands::reference::create_ref_import_batch,
            commands::reference::stage_ref_import_rows,
            commands::reference::validate_ref_import_batch,
            commands::reference::apply_ref_import_batch,
            commands::reference::get_ref_import_preview,
            commands::reference::export_ref_domain_set,
            commands::reference::list_ref_import_batches,
            commands::reference::search_reference_values,
            commands::reference::compute_ref_publish_readiness,
            commands::reference::preview_ref_publish_impact,
            commands::reference::governed_publish_reference_set,
            // ── Assets ────────────────────────────────────────────────────
            commands::assets::list_assets,
            commands::assets::get_asset_by_id,
            commands::assets::list_asset_children,
            commands::assets::list_asset_parents,
            commands::assets::search_assets,
            commands::assets::suggest_asset_codes,
            commands::assets::suggest_asset_names,
            commands::assets::get_asset_binding_summary,
            commands::assets::create_asset,
            commands::assets::update_asset_identity,
            commands::assets::link_asset_hierarchy,
            commands::assets::unlink_asset_hierarchy,
            commands::assets::move_asset_org_node,
            commands::assets::list_asset_lifecycle_events,
            commands::assets::record_lifecycle_event,
            commands::assets::list_asset_meters,
            commands::assets::create_asset_meter,
            commands::assets::record_meter_reading,
            commands::assets::get_latest_meter_value,
            commands::assets::list_meter_readings,
            commands::assets::list_asset_document_links,
            commands::assets::upsert_asset_document_link,
            commands::assets::expire_asset_document_link,
            commands::assets::create_asset_import_batch,
            commands::assets::validate_asset_import_batch,
            commands::assets::get_asset_import_preview,
            commands::assets::apply_asset_import_batch,
            commands::assets::list_asset_import_batches,
            // ── DI (Intervention Requests) ────────────────────────────────
            commands::di::list_di,
            commands::di::get_di,
            commands::di::create_di,
            commands::di::update_di_draft,
            commands::di::screen_di,
            commands::di::return_di,
            commands::di::reject_di,
            commands::di::approve_di,
            commands::di::defer_di,
            commands::di::reactivate_di,
            commands::di::get_di_review_events,
            // ── DI File 03 — SLA, Attachments, Conversion ─────────────────
            commands::di::upload_di_attachment,
            commands::di::list_di_attachments,
            commands::di::delete_di_attachment,
            commands::di::convert_di_to_wo,
            commands::di::get_sla_status,
            commands::di::list_sla_rules,
            commands::di::update_sla_rule,
            // ── DI File 04 — Audit Trail ──────────────────────────────────
            commands::di::list_di_change_events,
            commands::di::list_all_di_change_events,
            // ── WO (Work Orders) ──────────────────────────────────────────
            commands::wo::list_wo,
            commands::wo::get_wo,
            commands::wo::create_wo,
            commands::wo::update_wo_draft,
            commands::wo::cancel_wo,
            // ── WO Execution (File 02) ────────────────────────────────────
            commands::wo::plan_wo,
            commands::wo::assign_wo,
            commands::wo::start_wo,
            commands::wo::pause_wo,
            commands::wo::resume_wo,
            commands::wo::hold_wo,
            commands::wo::complete_wo_mechanically,
            // ── WO Labor ──────────────────────────────────────────────────
            commands::wo::add_labor,
            commands::wo::close_labor,
            commands::wo::list_labor,
            // ── WO Parts ──────────────────────────────────────────────────
            commands::wo::add_part,
            commands::wo::record_part_usage,
            commands::wo::confirm_no_parts,
            commands::wo::list_wo_parts,
            // ── WO Tasks ──────────────────────────────────────────────────
            commands::wo::add_task,
            commands::wo::complete_task,
            commands::wo::list_tasks,
            // ── WO Downtime / Delay ───────────────────────────────────────
            commands::wo::open_downtime,
            commands::wo::close_downtime,
            commands::wo::list_delay_segments,
            commands::wo::list_downtime_segments,
            // ── WO Close-Out (File 03) ────────────────────────────────────
            commands::wo::save_failure_detail,
            commands::wo::save_verification,
            commands::wo::close_wo,
            commands::wo::reopen_wo,
            commands::wo::update_wo_rca,
            // ── WO Attachments ────────────────────────────────────────────
            commands::wo::upload_wo_attachment,
            commands::wo::list_wo_attachments,
            commands::wo::delete_wo_attachment,
            // ── WO Costs ──────────────────────────────────────────────────
            commands::wo::get_cost_summary,
            commands::wo::update_service_cost,
            commands::wo::get_cost_posting_hook,
            // ── WO Analytics ──────────────────────────────────────────────
            commands::wo::get_wo_analytics_snapshot,
            // ── WO Stats (Dashboard) ──────────────────────────────────────
            commands::wo::get_wo_stats,
            // ── WO Audit ──────────────────────────────────────────────────
            commands::wo::list_wo_change_events,
            commands::wo::list_all_wo_change_events,
            // ── Dashboard ─────────────────────────────────────────────────
            commands::dashboard::get_dashboard_kpis,
            commands::dashboard::get_dashboard_workload_chart,
        ])
        .run(tauri::generate_context!())
        // EXPECT: If the Tauri context cannot be loaded, the application binary is corrupt or
        // the tauri.conf.json is missing. Panic at startup is the correct behavior.
        .expect("error while running Maintafox application");
}
