//! Asset history timeline aggregation service.
//!
//! Read-only projection that normalizes asset-adjacent events into a single
//! chronology for the Asset History view.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::errors::{AppError, AppResult};

use super::{bindings, health};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetHistoryPeriod {
    Today,
    Last7Days,
    Last30Days,
    ThisYear,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetHistoryEventRef {
    pub entity_type: String,
    pub entity_id: Option<i64>,
    pub entity_code: Option<String>,
    pub route_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetHistoryEvent {
    pub id: String,
    pub asset_id: i64,
    pub event_type: String,
    pub occurred_at: String,
    pub title: String,
    pub description: Option<String>,
    pub actor_label: Option<String>,
    pub duration_minutes: Option<i64>,
    pub status_label: Option<String>,
    pub metadata: BTreeMap<String, JsonValue>,
    pub r#ref: Option<AssetHistoryEventRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetHistorySummary {
    pub asset_id: i64,
    pub created_at: Option<String>,
    pub age_days: Option<i64>,
    pub wo_count: i64,
    pub pm_count: i64,
    pub failure_count: i64,
    pub availability_percent: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetHistoryQuery {
    pub asset_id: i64,
    pub event_types: Option<Vec<String>>,
    pub period: Option<AssetHistoryPeriod>,
    pub search: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

fn parse_dt(input: &str) -> Option<DateTime<Utc>> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(n) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(DateTime::from_naive_utc_and_offset(n, Utc));
    }
    if let Ok(n) = NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(n, Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        let n = d.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::from_naive_utc_and_offset(n, Utc));
    }
    None
}

fn in_period(dt: DateTime<Utc>, period: &AssetHistoryPeriod) -> bool {
    let now = Utc::now();
    match period {
        AssetHistoryPeriod::All => true,
        AssetHistoryPeriod::Today => dt.date_naive() == now.date_naive(),
        AssetHistoryPeriod::Last7Days => dt >= now - Duration::days(7),
        AssetHistoryPeriod::Last30Days => dt >= now - Duration::days(30),
        AssetHistoryPeriod::ThisYear => dt.year() == now.year(),
    }
}

fn event_matches_search(ev: &AssetHistoryEvent, search: &str) -> bool {
    let q = search.to_lowercase();
    ev.title.to_lowercase().contains(&q)
        || ev
            .description
            .as_ref()
            .map(|d| d.to_lowercase().contains(&q))
            .unwrap_or(false)
        || ev
            .status_label
            .as_ref()
            .map(|s| s.to_lowercase().contains(&q))
            .unwrap_or(false)
        || ev
            .r#ref
            .as_ref()
            .and_then(|r| r.entity_code.as_ref())
            .map(|c| c.to_lowercase().contains(&q))
            .unwrap_or(false)
}

fn map_lifecycle_event_type(raw: &str) -> String {
    match raw.to_ascii_uppercase().as_str() {
        "CREATED" | "COMMISSIONED" | "INSTALLED" => "asset_created".into(),
        "DECOMMISSIONED" | "SCRAPPED" => "decommissioned".into(),
        "MOVED" => "org_changed".into(),
        "PARENT_CHANGED" | "REPARENTED" => "parent_changed".into(),
        "CHILD_CHANGED" => "child_changed".into(),
        "ASSIGNMENT_CHANGED" => "assignment_changed".into(),
        "LOCATION_CHANGED" => "location_changed".into(),
        _ => "asset_updated".into(),
    }
}

fn push_event(
    events: &mut Vec<AssetHistoryEvent>,
    id: String,
    asset_id: i64,
    event_type: impl Into<String>,
    occurred_at: String,
    title: impl Into<String>,
    description: Option<String>,
    actor_label: Option<String>,
    duration_minutes: Option<i64>,
    status_label: Option<String>,
    metadata: BTreeMap<String, JsonValue>,
    r#ref: Option<AssetHistoryEventRef>,
) {
    if occurred_at.trim().is_empty() {
        return;
    }
    events.push(AssetHistoryEvent {
        id,
        asset_id,
        event_type: event_type.into(),
        occurred_at,
        title: title.into(),
        description,
        actor_label,
        duration_minutes,
        status_label,
        metadata,
        r#ref,
    });
}

pub async fn get_asset_history_summary(db: &DatabaseConnection, asset_id: i64) -> AppResult<AssetHistorySummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT created_at FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;

    let created_at: Option<String> = row.try_get::<Option<String>>("", "created_at").unwrap_or(None);

    let age_days = created_at
        .as_deref()
        .and_then(parse_dt)
        .map(|dt| (Utc::now() - dt).num_days().max(0));

    let binding = bindings::get_asset_binding_summary(db, asset_id).await?;
    let availability_percent = health::get_asset_health_score(db, asset_id)
        .await?
        .score
        .map(|s| s as f64);

    Ok(AssetHistorySummary {
        asset_id,
        created_at,
        age_days,
        wo_count: binding.linked_wo_count.count.unwrap_or(0),
        pm_count: binding.linked_pm_plan_count.count.unwrap_or(0),
        failure_count: binding.linked_failure_event_count.count.unwrap_or(0),
        availability_percent,
    })
}

pub async fn list_asset_history_events(
    db: &DatabaseConnection,
    query: AssetHistoryQuery,
) -> AppResult<Vec<AssetHistoryEvent>> {
    let mut events: Vec<AssetHistoryEvent> = Vec::new();
    let asset_id = query.asset_id;

    // Ensure asset exists (consistent with summary).
    let _ = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;

    // 1) Lifecycle events.
    let lifecycle_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, event_type, occurred_at, notes, from_status, to_status, reason_code \
             FROM equipment_lifecycle_events \
             WHERE equipment_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in lifecycle_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let event_type_raw: String = row.try_get("", "event_type").unwrap_or_else(|_| "EVENT".into());
        let occurred_at: String = row.try_get("", "occurred_at").unwrap_or_default();
        let notes: Option<String> = row.try_get::<Option<String>>("", "notes").unwrap_or(None);
        let from_status: Option<String> = row.try_get::<Option<String>>("", "from_status").unwrap_or(None);
        let to_status: Option<String> = row.try_get::<Option<String>>("", "to_status").unwrap_or(None);
        let reason_code: Option<String> = row.try_get::<Option<String>>("", "reason_code").unwrap_or(None);
        let mut metadata = BTreeMap::new();
        if let Some(v) = from_status.clone() {
            metadata.insert("from_status".into(), JsonValue::String(v));
        }
        if let Some(v) = to_status.clone() {
            metadata.insert("to_status".into(), JsonValue::String(v));
        }
        if let Some(v) = reason_code {
            metadata.insert("reason_code".into(), JsonValue::String(v));
        }
        push_event(
            &mut events,
            format!("lifecycle:{id}"),
            asset_id,
            map_lifecycle_event_type(&event_type_raw),
            occurred_at,
            event_type_raw,
            notes,
            None,
            None,
            to_status.or(from_status),
            metadata,
            Some(AssetHistoryEventRef {
                entity_type: "asset".into(),
                entity_id: Some(asset_id),
                entity_code: None,
                route_hint: Some(format!("/equipment?assetId={asset_id}")),
            }),
        );
    }

    // 2) Meter readings — real table is `asset_meter_readings` (migration 011).
    let meter_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT mr.id, mr.reading_at, mr.reading_value, mr.quality_flag, \
                    em.name AS meter_name, em.unit AS meter_unit \
             FROM asset_meter_readings mr \
             INNER JOIN equipment_meters em ON em.id = mr.meter_id \
             WHERE em.equipment_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in meter_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let occurred_at: String = row.try_get("", "reading_at").unwrap_or_default();
        let reading_value: f64 = row.try_get("", "reading_value").unwrap_or(0.0);
        let meter_name: String = row.try_get("", "meter_name").unwrap_or_else(|_| "Meter".into());
        let unit: Option<String> = row.try_get::<Option<String>>("", "meter_unit").unwrap_or(None);
        let quality: Option<String> = row.try_get::<Option<String>>("", "quality_flag").unwrap_or(None);
        let mut metadata = BTreeMap::new();
        metadata.insert("reading_value".into(), JsonValue::from(reading_value));
        if let Some(u) = unit.clone() {
            metadata.insert("unit".into(), JsonValue::String(u));
        }
        let desc = match unit {
            Some(u) if !u.is_empty() => format!("{meter_name}: {reading_value} {u}"),
            _ => format!("{meter_name}: {reading_value}"),
        };
        push_event(
            &mut events,
            format!("meter:{id}"),
            asset_id,
            "meter_reading",
            occurred_at,
            "Meter reading",
            Some(desc),
            None,
            None,
            quality,
            metadata,
            None,
        );
    }

    // 3) Documents linked.
    let doc_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, document_ref, link_purpose, created_at \
             FROM asset_document_links WHERE asset_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in doc_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let occurred_at: String = row.try_get("", "created_at").unwrap_or_default();
        let document_ref: String = row.try_get("", "document_ref").unwrap_or_default();
        let purpose: Option<String> = row.try_get::<Option<String>>("", "link_purpose").unwrap_or(None);
        push_event(
            &mut events,
            format!("doc:{id}"),
            asset_id,
            "document_linked",
            occurred_at,
            "Document linked",
            if document_ref.is_empty() {
                None
            } else {
                Some(document_ref.clone())
            },
            None,
            None,
            purpose,
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "document".into(),
                entity_id: Some(id),
                entity_code: Some(document_ref),
                route_hint: Some("/documents".into()),
            }),
        );
    }

    // 4) Photos added.
    let photo_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, file_name, caption, created_at FROM asset_photos WHERE asset_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in photo_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let occurred_at: String = row.try_get("", "created_at").unwrap_or_default();
        let file_name: String = row.try_get("", "file_name").unwrap_or_default();
        let caption: Option<String> = row.try_get::<Option<String>>("", "caption").unwrap_or(None);
        push_event(
            &mut events,
            format!("photo:{id}"),
            asset_id,
            "photo_added",
            occurred_at,
            "Photo added",
            caption.or(if file_name.is_empty() { None } else { Some(file_name) }),
            None,
            None,
            None,
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "photo".into(),
                entity_id: Some(id),
                entity_code: None,
                route_hint: Some(format!("/equipment?assetId={asset_id}")),
            }),
        );
    }

    // 5) Work orders.
    let wo_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.id, wo.code, wo.title, wo.created_at, wo.actual_start, wo.closed_at, \
                    wo.actual_duration_hours, wos.label AS status_label, \
                    COALESCE(resp.display_name, resp.username) AS actor_label \
             FROM work_orders wo \
             LEFT JOIN work_order_statuses wos ON wos.id = wo.status_id \
             LEFT JOIN user_accounts resp ON resp.id = wo.primary_responsible_id \
             WHERE wo.equipment_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in wo_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let code: String = row.try_get("", "code").unwrap_or_else(|_| format!("WO-{id}"));
        let title: Option<String> = row.try_get::<Option<String>>("", "title").unwrap_or(None);
        let created_at: String = row.try_get("", "created_at").unwrap_or_default();
        let actual_start: Option<String> = row.try_get::<Option<String>>("", "actual_start").unwrap_or(None);
        let closed_at: Option<String> = row.try_get::<Option<String>>("", "closed_at").unwrap_or(None);
        let status_label: Option<String> = row.try_get::<Option<String>>("", "status_label").unwrap_or(None);
        let actor_label: Option<String> = row.try_get::<Option<String>>("", "actor_label").unwrap_or(None);
        let duration_hours: Option<f64> = row.try_get::<Option<f64>>("", "actual_duration_hours").unwrap_or(None);
        let duration_minutes = duration_hours.map(|h| (h * 60.0).round() as i64);
        let route = format!("/work-orders?openWo={id}");

        push_event(
            &mut events,
            format!("wo-created:{id}"),
            asset_id,
            "wo_created",
            created_at,
            format!("WO {code} created"),
            title.clone(),
            actor_label.clone(),
            None,
            status_label.clone(),
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "wo".into(),
                entity_id: Some(id),
                entity_code: Some(code.clone()),
                route_hint: Some(route.clone()),
            }),
        );

        if let Some(started) = actual_start {
            push_event(
                &mut events,
                format!("wo-started:{id}"),
                asset_id,
                "wo_started",
                started,
                format!("WO {code} started"),
                title.clone(),
                actor_label.clone(),
                None,
                status_label.clone(),
                BTreeMap::new(),
                Some(AssetHistoryEventRef {
                    entity_type: "wo".into(),
                    entity_id: Some(id),
                    entity_code: Some(code.clone()),
                    route_hint: Some(route.clone()),
                }),
            );
        }

        if let Some(closed) = closed_at {
            push_event(
                &mut events,
                format!("wo-closed:{id}"),
                asset_id,
                "wo_closed",
                closed,
                format!("WO {code} closed"),
                title,
                actor_label,
                duration_minutes,
                status_label,
                BTreeMap::new(),
                Some(AssetHistoryEventRef {
                    entity_type: "wo".into(),
                    entity_id: Some(id),
                    entity_code: Some(code),
                    route_hint: Some(route),
                }),
            );
        }
    }

    // 6) Intervention requests (DI).
    let di_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, title, status, submitted_at, closed_at \
             FROM intervention_requests WHERE asset_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in di_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let code: String = row.try_get("", "code").unwrap_or_else(|_| format!("DI-{id}"));
        let title: Option<String> = row.try_get::<Option<String>>("", "title").unwrap_or(None);
        let status: Option<String> = row.try_get::<Option<String>>("", "status").unwrap_or(None);
        let submitted_at: String = row.try_get("", "submitted_at").unwrap_or_default();
        let closed_at: Option<String> = row.try_get::<Option<String>>("", "closed_at").unwrap_or(None);
        let route = format!("/requests?openDi={id}");

        push_event(
            &mut events,
            format!("di-created:{id}"),
            asset_id,
            "di_created",
            submitted_at,
            format!("DI {code} created"),
            title.clone(),
            None,
            None,
            status.clone(),
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "di".into(),
                entity_id: Some(id),
                entity_code: Some(code.clone()),
                route_hint: Some(route.clone()),
            }),
        );

        if let Some(closed) = closed_at {
            push_event(
                &mut events,
                format!("di-closed:{id}"),
                asset_id,
                "di_closed",
                closed,
                format!("DI {code} closed"),
                title,
                None,
                None,
                status,
                BTreeMap::new(),
                Some(AssetHistoryEventRef {
                    entity_type: "di".into(),
                    entity_id: Some(id),
                    entity_code: Some(code),
                    route_hint: Some(route),
                }),
            );
        }
    }

    // 7) PM scheduled (occurrences) + executed.
    let pm_occ_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.id, COALESCE(po.due_at, po.generated_at) AS due_at, po.status, \
                    p.id AS plan_id, p.code AS plan_code, p.title AS plan_title \
             FROM pm_occurrences po \
             INNER JOIN pm_plans p ON p.id = po.pm_plan_id \
             WHERE LOWER(p.asset_scope_type) = 'equipment' AND p.asset_scope_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in pm_occ_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let plan_id: i64 = row.try_get("", "plan_id").unwrap_or(0);
        let due_at: String = row.try_get("", "due_at").unwrap_or_default();
        let status: Option<String> = row.try_get::<Option<String>>("", "status").unwrap_or(None);
        let plan_code: String = row.try_get("", "plan_code").unwrap_or_else(|_| format!("PM-{plan_id}"));
        let plan_title: Option<String> = row.try_get::<Option<String>>("", "plan_title").unwrap_or(None);
        push_event(
            &mut events,
            format!("pm-scheduled:{id}"),
            asset_id,
            "pm_scheduled",
            due_at,
            format!("PM {plan_code} scheduled"),
            plan_title,
            None,
            None,
            status,
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "pm".into(),
                entity_id: Some(plan_id),
                entity_code: Some(plan_code),
                route_hint: Some(format!("/pm?planId={plan_id}")),
            }),
        );
    }

    let pm_exec_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pe.id, pe.executed_at, pe.execution_result, \
                    p.id AS plan_id, p.code AS plan_code, p.title AS plan_title \
             FROM pm_executions pe \
             INNER JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id \
             INNER JOIN pm_plans p ON p.id = po.pm_plan_id \
             WHERE LOWER(p.asset_scope_type) = 'equipment' AND p.asset_scope_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in pm_exec_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let plan_id: i64 = row.try_get("", "plan_id").unwrap_or(0);
        let executed_at: String = row.try_get("", "executed_at").unwrap_or_default();
        let result: Option<String> = row.try_get::<Option<String>>("", "execution_result").unwrap_or(None);
        let plan_code: String = row.try_get("", "plan_code").unwrap_or_else(|_| format!("PM-{plan_id}"));
        let plan_title: Option<String> = row.try_get::<Option<String>>("", "plan_title").unwrap_or(None);
        push_event(
            &mut events,
            format!("pm-executed:{id}"),
            asset_id,
            "pm_executed",
            executed_at,
            format!("PM {plan_code} executed"),
            plan_title,
            None,
            None,
            result,
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "pm".into(),
                entity_id: Some(plan_id),
                entity_code: Some(plan_code),
                route_hint: Some(format!("/pm?planId={plan_id}")),
            }),
        );
    }

    // 8) Inspection results for this asset's checkpoints.
    let inspection_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ir.id, ir.recorded_at, ir.result_status, ir.comment, ir.round_id, \
                    ic.checkpoint_code AS checkpoint_label \
             FROM inspection_results ir \
             INNER JOIN inspection_checkpoints ic ON ic.id = ir.checkpoint_id \
             WHERE ic.asset_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in inspection_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let round_id: i64 = row.try_get("", "round_id").unwrap_or(0);
        let occurred_at: String = row.try_get("", "recorded_at").unwrap_or_default();
        let status: Option<String> = row.try_get::<Option<String>>("", "result_status").unwrap_or(None);
        let comment: Option<String> = row.try_get::<Option<String>>("", "comment").unwrap_or(None);
        let checkpoint: Option<String> = row.try_get::<Option<String>>("", "checkpoint_label").unwrap_or(None);
        push_event(
            &mut events,
            format!("inspection:{id}"),
            asset_id,
            "inspection_recorded",
            occurred_at,
            checkpoint.unwrap_or_else(|| "Inspection recorded".into()),
            comment,
            None,
            None,
            status,
            BTreeMap::new(),
            Some(AssetHistoryEventRef {
                entity_type: "inspection".into(),
                entity_id: Some(round_id),
                entity_code: None,
                route_hint: Some("/inspections".into()),
            }),
        );
    }

    // 9) Failure events.
    let failure_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, COALESCE(failed_at, detected_at, created_at) AS occurred_at, \
                    source_type, source_id, verification_status, downtime_duration_hours \
             FROM failure_events WHERE equipment_id = ?",
            [asset_id.into()],
        ))
        .await?;
    for row in failure_rows {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let occurred_at: String = row.try_get("", "occurred_at").unwrap_or_default();
        let source_type: Option<String> = row.try_get::<Option<String>>("", "source_type").unwrap_or(None);
        let source_id: Option<i64> = row.try_get::<Option<i64>>("", "source_id").unwrap_or(None);
        let status: Option<String> = row.try_get::<Option<String>>("", "verification_status").unwrap_or(None);
        let downtime_h: Option<f64> = row
            .try_get::<Option<f64>>("", "downtime_duration_hours")
            .unwrap_or(None);
        let mut metadata = BTreeMap::new();
        if let Some(st) = source_type.clone() {
            metadata.insert("source_type".into(), JsonValue::String(st));
        }
        if let Some(sid) = source_id {
            metadata.insert("source_id".into(), JsonValue::from(sid));
        }
        let route = match (source_type.as_deref(), source_id) {
            (Some("work_order"), Some(sid)) => Some(format!("/work-orders?openWo={sid}")),
            _ => Some("/reliability/foundation".into()),
        };
        push_event(
            &mut events,
            format!("failure:{id}"),
            asset_id,
            "failure_recorded",
            occurred_at,
            "Failure recorded",
            source_type.map(|st| format!("Source: {st}")),
            None,
            downtime_h.map(|h| (h * 60.0).round() as i64),
            status,
            metadata,
            Some(AssetHistoryEventRef {
                entity_type: "asset".into(),
                entity_id: Some(id),
                entity_code: None,
                route_hint: route,
            }),
        );
    }

    // Filters.
    if let Some(types) = query.event_types {
        if !types.is_empty() {
            let wanted: BTreeSet<String> = types.into_iter().collect();
            events.retain(|e| wanted.contains(&e.event_type));
        }
    }

    if let Some(period) = query.period {
        events.retain(|e| {
            parse_dt(&e.occurred_at)
                .map(|dt| in_period(dt, &period))
                .unwrap_or(true)
        });
    }

    if let Some(search) = query.search {
        let trimmed = search.trim();
        if !trimmed.is_empty() {
            events.retain(|e| event_matches_search(e, trimmed));
        }
    }

    events.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at).then_with(|| a.id.cmp(&b.id)));

    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(200).min(1000) as usize;
    Ok(events.into_iter().skip(offset).take(limit).collect())
}
