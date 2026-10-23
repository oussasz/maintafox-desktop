//! Scheduled reports and template library (PRD §6.11).

use cron::Schedule;
use std::str::FromStr;
use tauri::{AppHandle, Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reports::artifacts::{persist_run_artifact, read_run_document};
use crate::reports::domain::{ExportReportInput, ReportRun, ReportSchedule, ReportTemplate, UpsertReportScheduleInput};
use crate::reports::export::{export_report_document, ReportAudience, ReportLocale};
use crate::reports::queries::{
    delete_schedule, get_template_by_code, get_template_by_id, insert_run, list_my_runs, list_my_schedules,
    list_report_templates as query_list_report_templates, upsert_schedule,
};
use crate::reports::scheduler::next_run_rfc3339;
use crate::state::AppState;
use crate::{require_permission, require_session};

fn validate_export_format(s: &str) -> AppResult<()> {
    let x = s.to_ascii_lowercase();
    if x == "pdf" || x == "xlsx" {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![
            "export_format must be pdf or xlsx.".into()
        ]))
    }
}

fn app_data_dir(app: &AppHandle) -> AppResult<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))
}

#[tauri::command]
pub async fn list_report_templates(state: State<'_, AppState>) -> AppResult<Vec<ReportTemplate>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_VIEW,
        PermissionScope::Global
    );
    query_list_report_templates(&state.db).await
}

#[tauri::command]
pub async fn list_my_report_schedules(state: State<'_, AppState>) -> AppResult<Vec<ReportSchedule>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_VIEW,
        PermissionScope::Global
    );
    list_my_schedules(&state.db, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn upsert_my_report_schedule(input: UpsertReportScheduleInput, state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_MANAGE,
        PermissionScope::Global
    );
    validate_export_format(&input.export_format)?;
    let template = get_template_by_id(&state.db, input.template_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "report_template".into(),
            id: input.template_id.to_string(),
        })?;
    if !template.is_active {
        return Err(AppError::ValidationFailed(vec!["template inactive.".into()]));
    }
    let schedule = Schedule::from_str(input.cron_expr.trim())
        .map_err(|e| AppError::ValidationFailed(vec![format!("cron: {e}")]))?;
    let next = next_run_rfc3339(&schedule);
    upsert_schedule(&state.db, i64::from(user.user_id), &input, &next).await
}

#[tauri::command]
pub async fn delete_my_report_schedule(schedule_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_MANAGE,
        PermissionScope::Global
    );
    let n = delete_schedule(&state.db, i64::from(user.user_id), schedule_id).await?;
    if n == 0 {
        return Err(AppError::NotFound {
            entity: "report_schedule".into(),
            id: schedule_id.to_string(),
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn list_my_report_runs(limit: Option<i64>, state: State<'_, AppState>) -> AppResult<Vec<ReportRun>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_VIEW,
        PermissionScope::Global
    );
    list_my_runs(&state.db, i64::from(user.user_id), limit.unwrap_or(50)).await
}

#[tauri::command]
pub async fn export_report_now(
    input: ExportReportInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ExportedBinaryDocument> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_EXPORT,
        PermissionScope::Global
    );
    validate_export_format(&input.export_format)?;
    let t = get_template_by_code(&state.db, &input.template_code)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "report_template".into(),
            id: input.template_code.clone(),
        })?;
    if !t.is_active {
        return Err(AppError::ValidationFailed(vec!["template inactive.".into()]));
    }
    let fmt = input.export_format.to_ascii_lowercase();
    let locale = ReportLocale::resolve(&state.db, input.locale.as_deref()).await;
    let audience = ReportAudience::of_user(&state.db, &state.permission_cache, user.user_id).await?;
    let doc = export_report_document(&state.db, &t.code, &fmt, locale, audience).await?;
    let run_id = insert_run(&state.db, None, t.id, i64::from(user.user_id), "running", &fmt).await?;
    let data_dir = app_data_dir(&app)?;
    if let Err(e) = persist_run_artifact(&state.db, &data_dir, run_id, &fmt, &doc.file_name, &doc.bytes).await {
        let _ = crate::reports::queries::finish_run_failed(&state.db, run_id, &e.to_string()).await;
        return Err(e);
    }
    Ok(doc)
}

#[tauri::command]
pub async fn download_report_run_artifact(
    run_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ExportedBinaryDocument> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_EXPORT,
        PermissionScope::Global
    );
    let data_dir = app_data_dir(&app)?;
    read_run_document(&state.db, &data_dir, run_id, i64::from(user.user_id)).await
}
