//! Device-local executive dashboard layouts (separate from Home Overview).

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};

pub const DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON: &str = r#"{"version":1,"widgets":[{"id":"ops_kpis","order":0,"visible":true},{"id":"pm_due","order":1,"visible":true},{"id":"reliability_rams","order":2,"visible":true},{"id":"maintenance_cost","order":3,"visible":true},{"id":"workload","order":4,"visible":true},{"id":"di_status","order":5,"visible":true}]}"#;

fn validate_widget_array(widgets: &[Value]) -> AppResult<()> {
    if widgets.is_empty() || widgets.len() > 32 {
        return Err(AppError::ValidationFailed(vec![
            "layout_json.widgets length invalid.".into()
        ]));
    }
    for w in widgets {
        let wo = w
            .as_object()
            .ok_or_else(|| AppError::ValidationFailed(vec!["each widget must be an object.".into()]))?;
        wo.get("id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .ok_or_else(|| AppError::ValidationFailed(vec!["widget.id invalid.".into()]))?;
        wo.get("order")
            .and_then(|x| x.as_i64())
            .ok_or_else(|| AppError::ValidationFailed(vec!["widget.order invalid.".into()]))?;
        wo.get("visible")
            .and_then(|x| x.as_bool())
            .ok_or_else(|| AppError::ValidationFailed(vec!["widget.visible invalid.".into()]))?;
    }
    Ok(())
}

fn validate_overview_kpis_array(kpis: &[Value]) -> AppResult<()> {
    if kpis.len() > 32 {
        return Err(AppError::ValidationFailed(vec![
            "layout_json.overview_kpis length invalid.".into(),
        ]));
    }
    for w in kpis {
        let wo = w
            .as_object()
            .ok_or_else(|| AppError::ValidationFailed(vec!["each overview_kpi must be an object.".into()]))?;
        wo.get("id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .ok_or_else(|| AppError::ValidationFailed(vec!["overview_kpi.id invalid.".into()]))?;
        wo.get("order")
            .and_then(|x| x.as_i64())
            .ok_or_else(|| AppError::ValidationFailed(vec!["overview_kpi.order invalid.".into()]))?;
        wo.get("visible")
            .and_then(|x| x.as_bool())
            .ok_or_else(|| AppError::ValidationFailed(vec!["overview_kpi.visible invalid.".into()]))?;
        if let Some(accent) = wo.get("accent") {
            if !accent.is_null() {
                let s = accent
                    .as_str()
                    .ok_or_else(|| AppError::ValidationFailed(vec!["overview_kpi.accent invalid.".into()]))?;
                match s {
                    "default" | "brand" | "amber" | "danger" | "teal" => {}
                    _ => {
                        return Err(AppError::ValidationFailed(vec!["overview_kpi.accent unknown.".into()]));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn validate_analytics_layout_json(raw: &str) -> AppResult<()> {
    if raw.len() > 48_000 {
        return Err(AppError::ValidationFailed(vec!["layout_json too large.".into()]));
    }
    let v: Value =
        serde_json::from_str(raw).map_err(|e| AppError::ValidationFailed(vec![format!("layout_json: {e}")]))?;
    let obj = v
        .as_object()
        .ok_or_else(|| AppError::ValidationFailed(vec!["layout_json must be an object.".into()]))?;
    if obj.get("version").and_then(|x| x.as_u64()) != Some(1) {
        return Err(AppError::ValidationFailed(
            vec!["layout_json.version must be 1.".into()],
        ));
    }
    let Some(widgets) = obj.get("widgets").and_then(|x| x.as_array()) else {
        return Err(AppError::ValidationFailed(vec!["layout_json.widgets required.".into()]));
    };
    validate_widget_array(widgets)?;
    if let Some(overview) = obj.get("overview_kpis").and_then(|x| x.as_array()) {
        validate_overview_kpis_array(overview)?;
    }
    Ok(())
}

pub fn merge_overview_kpis_into_layout(existing_json: &str, overview_kpis: Value) -> AppResult<String> {
    let mut root: Value = serde_json::from_str(existing_json)
        .unwrap_or_else(|_| serde_json::from_str(DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON).expect("default layout"));
    if let Some(obj) = root.as_object_mut() {
        obj.insert("overview_kpis".into(), overview_kpis);
        if obj.get("version").is_none() {
            obj.insert("version".into(), json!(1));
        }
        if obj.get("widgets").is_none() {
            let default: Value = serde_json::from_str(DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON)?;
            if let Some(w) = default.get("widgets") {
                obj.insert("widgets".into(), w.clone());
            }
        }
    }
    let out = serde_json::to_string(&root)?;
    validate_analytics_layout_json(&out)?;
    Ok(out)
}

pub async fn get_layout(db: &DatabaseConnection, user_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT layout_json FROM user_analytics_dashboard_layouts WHERE user_id = ?",
            [user_id.into()],
        ))
        .await?;
    if let Some(r) = row {
        let raw: String = r
            .try_get("", "layout_json")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("layout_json: {e}"))))?;
        if validate_analytics_layout_json(&raw).is_ok() {
            return Ok(raw);
        }
    }
    Ok(DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON.to_string())
}

pub async fn save_layout(db: &DatabaseConnection, user_id: i64, layout_json: &str) -> AppResult<()> {
    validate_analytics_layout_json(layout_json)?;
    let ts = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_analytics_dashboard_layouts (user_id, layout_json, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET layout_json = excluded.layout_json, updated_at = excluded.updated_at",
        [user_id.into(), layout_json.into(), ts.into()],
    ))
    .await?;
    Ok(())
}
