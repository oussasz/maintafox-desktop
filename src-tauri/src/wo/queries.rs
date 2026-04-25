//! WO query and mutation functions.
//!
//! Phase 2 – Sub-phase 05 – File 01 – Sprint S2.
//!
//! All functions use sea-orm raw SQL via `Statement::from_sql_and_values`
//! to stay consistent with the codebase's established query pattern.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::domain::{
    generate_wo_code, guard_wo_transition, map_wo_transition_row, map_work_order, WoCreateInput,
    WoStatus, WoTransitionRow, WorkOrder,
};
use super::statuses::ensure_work_order_statuses_if_needed;
use super::types::resolve_work_order_type_id_by_code;

/// Ensures at least one closeout policy row exists and returns its id (for COALESCE on new WOs).
async fn ensure_default_closeout_validation_policy_id(db: &DatabaseConnection) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM closeout_validation_policies ORDER BY id LIMIT 1".to_string(),
        ))
        .await?;
    if let Some(row) = existing {
        let id: i64 = row
            .try_get::<i64>("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("closeout policy id decode: {e}")))?;
        return Ok(id);
    }

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO closeout_validation_policies \
         (entity_id, policy_name, applies_when, require_failure_mode_if_unplanned, \
          require_downtime_if_production_impact, allow_close_with_cause_not_determined, \
          allow_close_with_cause_mode_only, require_verification_return_to_service, \
          notes_min_length_when_cnd, entity_sync_id, row_version) \
         VALUES (NULL, 'default_corrective', \
          '{\"maintenance_type\":[\"corrective\",\"emergency\"]}', 1, 1, 1, 0, 1, 10, \
          'closeout_policy:runtime_seed', 1)"
            .to_string(),
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM closeout_validation_policies ORDER BY id LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "closeout_validation_policies insert did not yield a row"
            ))
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("closeout policy id decode: {e}")))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Input / output types
// ═══════════════════════════════════════════════════════════════════════════════

/// Paginated list filter for work orders.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WoListFilter {
    pub status_codes: Option<Vec<String>>,
    pub type_codes: Option<Vec<String>>,
    pub equipment_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub planner_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub urgency_level: Option<i64>,
    pub source_di_id: Option<i64>,
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

/// Paginated list result.
#[derive(Debug, Clone, Serialize)]
pub struct WoListPage {
    pub items: Vec<WorkOrder>,
    pub total: i64,
}

/// Composite detail response for `get_wo`.
#[derive(Debug, Clone, Serialize)]
pub struct WoGetResponse {
    pub wo: WorkOrder,
    pub transitions: Vec<WoTransitionRow>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants — SELECT columns
// ═══════════════════════════════════════════════════════════════════════════════

/// Core work_orders columns aliased with `wo.` prefix.
const WO_COLS: &str = "\
    wo.id, wo.code, wo.type_id, wo.status_id, \
    wo.equipment_id, wo.component_id, wo.location_id, \
    wo.requester_id, wo.source_di_id, wo.source_inspection_anomaly_id, \
    wo.source_ram_ishikawa_diagram_id, wo.source_ishikawa_flow_node_id, wo.source_rca_cause_text, \
    wo.entity_id, \
    wo.planner_id, wo.approver_id, wo.assigned_group_id, wo.primary_responsible_id, \
    wo.urgency_id, wo.title, wo.description, \
    wo.planned_start, wo.planned_end, wo.shift, wo.scheduled_at, \
    wo.actual_start, wo.actual_end, \
    wo.mechanically_completed_at, wo.technically_verified_at, \
    wo.closed_at, wo.cancelled_at, \
    wo.expected_duration_hours, wo.actual_duration_hours, \
    wo.active_labor_hours, wo.total_waiting_hours, wo.downtime_hours, \
    wo.labor_cost, wo.parts_cost, wo.service_cost, wo.total_cost, \
    wo.recurrence_risk_level, wo.production_impact_id, \
    wo.root_cause_summary, wo.corrective_action_summary, wo.verification_method, \
    wo.notes, wo.cancel_reason, wo.parts_actuals_confirmed, \
    wo.service_cost_input, wo.reopen_count, wo.last_closed_at, \
    wo.requires_permit, \
    wo.entity_sync_id, wo.closeout_validation_profile_id, wo.closeout_validation_passed, \
    wo.no_downtime_attestation, wo.no_downtime_attestation_reason, \
    wo.row_version, wo.created_at, wo.updated_at";

/// Join columns appended when full joins are present.
const WO_JOIN_COLS: &str = "\
    wos.code  AS status_code,  wos.label AS status_label,  wos.color AS status_color, \
    wot.code  AS type_code,    wot.label AS type_label, \
    ul.level  AS urgency_level, ul.label AS urgency_label, ul.hex_color AS urgency_color, \
    ar.asset_id_code AS asset_code, ar.name AS asset_label, \
    up.username  AS planner_username, \
    ur.username  AS responsible_username";

/// JOIN clause used by list and get queries.
const WO_JOINS: &str = "\
    LEFT JOIN work_order_statuses wos ON wos.id = wo.status_id \
    LEFT JOIN work_order_types    wot ON wot.id = wo.type_id \
    LEFT JOIN urgency_levels      ul  ON ul.id  = wo.urgency_id \
    LEFT JOIN equipment           ar  ON ar.id  = wo.equipment_id \
    LEFT JOIN user_accounts       up  ON up.id  = wo.planner_id \
    LEFT JOIN user_accounts       ur  ON ur.id  = wo.primary_responsible_id";

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_work_orders — paginated, filtered
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_work_orders(
    db: &DatabaseConnection,
    filter: WoListFilter,
) -> AppResult<WoListPage> {
    let mut where_clauses: Vec<String> = vec!["1 = 1".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    // ── Status filter (multi-select by code) ─────────────────────────────
    if let Some(ref codes) = filter.status_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("wos.code IN ({placeholders})"));
            for c in codes {
                binds.push(c.clone().into());
            }
        }
    }

    // ── Type filter (multi-select by code) ───────────────────────────────
    if let Some(ref codes) = filter.type_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("wot.code IN ({placeholders})"));
            for c in codes {
                binds.push(c.clone().into());
            }
        }
    }

    // ── Scalar FK filters ────────────────────────────────────────────────
    if let Some(equipment_id) = filter.equipment_id {
        where_clauses.push("wo.equipment_id = ?".to_string());
        binds.push(equipment_id.into());
    }
    if let Some(entity_id) = filter.entity_id {
        where_clauses.push("wo.entity_id = ?".to_string());
        binds.push(entity_id.into());
    }
    if let Some(planner_id) = filter.planner_id {
        where_clauses.push("wo.planner_id = ?".to_string());
        binds.push(planner_id.into());
    }
    if let Some(primary_responsible_id) = filter.primary_responsible_id {
        where_clauses.push("wo.primary_responsible_id = ?".to_string());
        binds.push(primary_responsible_id.into());
    }
    if let Some(source_di_id) = filter.source_di_id {
        where_clauses.push("wo.source_di_id = ?".to_string());
        binds.push(source_di_id.into());
    }

    // ── Urgency filter (by level number) ─────────────────────────────────
    if let Some(urgency_level) = filter.urgency_level {
        where_clauses.push("ul.level = ?".to_string());
        binds.push(urgency_level.into());
    }

    // ── Date range filter (on created_at) ────────────────────────────────
    if let Some(ref date_from) = filter.date_from {
        if !date_from.is_empty() {
            where_clauses.push("wo.created_at >= ?".to_string());
            binds.push(date_from.clone().into());
        }
    }
    if let Some(ref date_to) = filter.date_to {
        if !date_to.is_empty() {
            where_clauses.push("wo.created_at <= ?".to_string());
            binds.push(date_to.clone().into());
        }
    }

    // ── Free-text search (code + title + equipment name) ─────────────────
    if let Some(ref search) = filter.search {
        let trimmed = search.trim();
        if !trimmed.is_empty() {
            where_clauses.push(
                "(wo.code LIKE ? OR wo.title LIKE ? OR ar.name LIKE ?)".to_string(),
            );
            let pattern = format!("%{trimmed}%");
            binds.push(pattern.clone().into());
            binds.push(pattern.clone().into());
            binds.push(pattern.into());
        }
    }

    let where_sql = where_clauses.join(" AND ");

    // ── Count query ──────────────────────────────────────────────────────
    let count_sql = format!(
        "SELECT COUNT(*) AS total FROM work_orders wo {WO_JOINS} WHERE {where_sql}"
    );
    let count_binds = binds.clone();
    let count_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &count_sql,
            count_binds,
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("WO count query returned no rows"))
        })?;
    let total: i64 = count_row
        .try_get::<i64>("", "total")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("WO count decode: {e}")))?;

    // ── Data query with LIMIT/OFFSET ─────────────────────────────────────
    let row_limit = filter.limit.min(200).max(1);
    let offset = filter.offset.max(0);

    let data_sql = format!(
        "SELECT {WO_COLS}, {WO_JOIN_COLS} \
         FROM work_orders wo \
         {WO_JOINS} \
         WHERE {where_sql} \
         ORDER BY wo.created_at DESC \
         LIMIT {row_limit} OFFSET {offset}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &data_sql,
            binds,
        ))
        .await?;

    let items: Vec<WorkOrder> = rows
        .iter()
        .map(map_work_order)
        .collect::<AppResult<Vec<_>>>()?;

    Ok(WoListPage { items, total })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_work_order — single row by id with full joins
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_work_order(
    db: &DatabaseConnection,
    id: i64,
) -> AppResult<Option<WorkOrder>> {
    let sql = format!(
        "SELECT {WO_COLS}, {WO_JOIN_COLS} \
         FROM work_orders wo \
         {WO_JOINS} \
         WHERE wo.id = ?"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [id.into()],
        ))
        .await?;

    match row {
        Some(r) => Ok(Some(map_work_order(&r)?)),
        None => Ok(None),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) get_wo_transition_log — append-only log for a WO
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_wo_transition_log(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoTransitionRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, wo_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at \
             FROM wo_state_transition_log \
             WHERE wo_id = ? \
             ORDER BY acted_at ASC",
            [wo_id.into()],
        ))
        .await?;

    rows.iter().map(map_wo_transition_row).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) create_work_order — full insert + transition log
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn create_work_order(
    db: &DatabaseConnection,
    input: WoCreateInput,
) -> AppResult<WorkOrder> {
    ensure_work_order_statuses_if_needed(db).await?;
    let default_closeout_policy_id = ensure_default_closeout_validation_policy_id(db).await?;

    let equipment_id = input.equipment_id.filter(|&e| e > 0);
    let urgency_id = input.urgency_id.filter(|&u| u > 0);

    let creator_ok = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE id = ?",
            [input.creator_id.into()],
        ))
        .await?;
    if creator_ok.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Utilisateur introuvable (creator_id={}).",
            input.creator_id
        )]));
    }

    if let Some(eid) = equipment_id {
        let ex = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM equipment WHERE id = ?",
                [eid.into()],
            ))
            .await?;
        if ex.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Équipement introuvable (equipment_id={eid})."
            )]));
        }
    }

    if let Some(uid) = urgency_id {
        let ex = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM urgency_levels WHERE id = ?",
                [uid.into()],
            ))
            .await?;
        if ex.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Priorité / urgence introuvable (urgency_id={uid})."
            )]));
        }
    }

    let type_id = resolve_work_order_type_id_by_code(db, &input.type_code).await?;

    // If source_di_id is provided, verify the DI exists
    if let Some(di_id) = input.source_di_id {
        let di_exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM intervention_requests WHERE id = ?",
                [di_id.into()],
            ))
            .await?;
        if di_exists.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "DI introuvable (source_di_id={di_id})."
            )]));
        }
    }

    if let Some(aid) = input.source_inspection_anomaly_id {
        let a_exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM inspection_anomalies WHERE id = ?",
                [aid.into()],
            ))
            .await?;
        if a_exists.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Anomalie d'inspection introuvable (source_inspection_anomaly_id={aid})."
            )]));
        }
    }

    if let Some(did) = input.source_ram_ishikawa_diagram_id {
        let d_exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM ram_ishikawa_diagrams WHERE id = ?",
                [did.into()],
            ))
            .await?;
        if d_exists.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Diagramme Ishikawa introuvable (source_ram_ishikawa_diagram_id={did})."
            )]));
        }
    }

    // Resolve status_id for 'draft'
    let draft_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = 'draft'",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "work_order_statuses missing 'draft' row"
            ))
        })?;
    let status_id: i64 = draft_row
        .try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("draft status_id decode: {e}")))?;

    info!(
        target: "maintafox",
        type_code = %input.type_code,
        type_id = type_id,
        status_id_draft = status_id,
        equipment_id = ?equipment_id,
        urgency_id = ?urgency_id,
        creator_id = input.creator_id,
        default_closeout_policy_id = default_closeout_policy_id,
        source_di_id = ?input.source_di_id,
        entity_id = ?input.entity_id,
        "create_work_order: resolved FK targets before INSERT"
    );

    let code = generate_wo_code(db).await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_orders (\
            code, type_id, status_id, equipment_id, location_id, \
            source_di_id, source_inspection_anomaly_id, \
            source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id, source_rca_cause_text, \
            entity_id, planner_id, requester_id, urgency_id, \
            title, description, notes, \
            planned_start, planned_end, shift, expected_duration_hours, \
            requires_permit, \
            row_version, created_at, updated_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            code.clone().into(),
            type_id.into(),
            status_id.into(),
            equipment_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input.location_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input.source_di_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input
                .source_inspection_anomaly_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            input
                .source_ram_ishikawa_diagram_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            input
                .source_ishikawa_flow_node_id
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input
                .source_rca_cause_text
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input.entity_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input.planner_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input.creator_id.into(),
            urgency_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input.title.into(),
            input.description.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.notes.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.planned_start.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.planned_end.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.shift.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.expected_duration_hours.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<f64>)),
            (if input.requires_permit.unwrap_or(false) { 1i64 } else { 0i64 }).into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![
                "Un code OT en doublon a été généré. Veuillez réessayer.".into(),
            ])
        } else if e.to_string().contains("FOREIGN KEY") {
            AppError::ValidationFailed(vec![format!(
                "Référence invalide (contrainte FK). Détail SQLite: {e}"
            )])
        } else {
            AppError::Database(e)
        }
    })?;

    let new_id: i64 = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM work_orders WHERE code = ?",
                [code.clone().into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "Failed to re-read WO after insert: code={code}"
                ))
            })?;
        row.try_get::<i64>("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("WO id decode: {e}")))?
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET \
         entity_sync_id = lower(hex(randomblob(16))), \
         closeout_validation_profile_id = COALESCE(closeout_validation_profile_id, ?) \
         WHERE id = ?",
        [default_closeout_policy_id.into(), new_id.into()],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("FOREIGN KEY") {
            AppError::ValidationFailed(vec![format!(
                "Profil de clôture invalide (closeout_validation_profile_id). Détail: {e}"
            )])
        } else {
            AppError::Database(e)
        }
    })?;

    let wo = get_work_order(db, new_id)
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-read WO after insert: code={code}"
            ))
        })?;

    // Write initial transition log entry
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
         (wo_id, from_status, to_status, action, actor_id, acted_at) \
         VALUES (?, 'none', 'draft', 'create', ?, ?)",
        [wo.id.into(), input.creator_id.into(), now.into()],
    ))
    .await?;

    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) update_wo_draft_fields — optimistic concurrency + draft guard
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn update_wo_draft_fields(
    db: &DatabaseConnection,
    input: super::domain::WoDraftUpdateInput,
) -> AppResult<WorkOrder> {
    // 1. Fetch current row to validate status
    let current = get_work_order(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.id.to_string(),
        })?;

    // 2. Guard: only allow draft edits
    let status_code = current.status_code.as_deref().unwrap_or("unknown");
    if status_code != "draft" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Les champs ne peuvent être modifiés qu'au statut 'draft'. \
             Statut actuel : '{status_code}'."
        )]));
    }

    // 3. Build dynamic SET clause
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref title) = input.title {
        sets.push("title = ?".into());
        values.push(title.clone().into());
    }
    if let Some(type_code) = input.type_code.as_ref() {
        let type_id = resolve_work_order_type_id_by_code(db, type_code).await?;
        sets.push("type_id = ?".into());
        values.push(type_id.into());
    }
    if let Some(equipment_id) = input.equipment_id {
        sets.push("equipment_id = ?".into());
        values.push(equipment_id.into());
    }
    if let Some(location_id) = input.location_id {
        sets.push("location_id = ?".into());
        values.push(location_id.into());
    }
    if let Some(ref description) = input.description {
        sets.push("description = ?".into());
        values.push(description.clone().into());
    }
    if let Some(ref planned_start) = input.planned_start {
        sets.push("planned_start = ?".into());
        values.push(planned_start.clone().into());
    }
    if let Some(ref planned_end) = input.planned_end {
        sets.push("planned_end = ?".into());
        values.push(planned_end.clone().into());
    }
    if let Some(ref shift) = input.shift {
        sets.push("shift = ?".into());
        values.push(shift.clone().into());
    }
    if let Some(expected_duration_hours) = input.expected_duration_hours {
        sets.push("expected_duration_hours = ?".into());
        values.push(expected_duration_hours.into());
    }
    if let Some(ref notes) = input.notes {
        sets.push("notes = ?".into());
        values.push(notes.clone().into());
    }
    if let Some(urgency_id) = input.urgency_id {
        sets.push("urgency_id = ?".into());
        values.push(urgency_id.into());
    }
    if let Some(rp) = input.requires_permit {
        sets.push("requires_permit = ?".into());
        values.push((if rp { 1i64 } else { 0i64 }).into());
    }

    if sets.is_empty() {
        // Nothing to update — return current row
        return Ok(current);
    }

    // Always bump version + updated_at
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    sets.push("row_version = row_version + 1".into());
    sets.push("updated_at = ?".into());
    values.push(now.into());

    // 4. Optimistic concurrency: WHERE id = ? AND row_version = ?
    values.push(input.id.into());
    values.push(input.expected_row_version.into());

    let sql = format!(
        "UPDATE work_orders SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            values,
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    // 5. Re-fetch and return updated row
    get_work_order(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) cancel_work_order — state guard + cancel reason required
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn cancel_work_order(
    db: &DatabaseConnection,
    input: super::domain::WoCancelInput,
) -> AppResult<WorkOrder> {
    // Validate cancel_reason is not empty
    if input.cancel_reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le motif d'annulation est obligatoire.".into(),
        ]));
    }

    // 1. Fetch current row
    let current = get_work_order(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.id.to_string(),
        })?;

    // 2. Guard: check transition is legal
    let from_code = current
        .status_code
        .as_deref()
        .unwrap_or("unknown");
    let from_status = WoStatus::try_from_str(from_code).map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Stored WO has invalid status: {e}"))
    })?;
    guard_wo_transition(&from_status, &WoStatus::Cancelled).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    // 3. Resolve cancelled status_id
    let cancelled_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = 'cancelled'",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "work_order_statuses missing 'cancelled' row"
            ))
        })?;
    let cancelled_id: i64 = cancelled_row
        .try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("cancelled status_id decode: {e}")))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 4. Update
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, cancel_reason = ?, cancelled_at = ?, \
                row_version = row_version + 1, updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                cancelled_id.into(),
                input.cancel_reason.clone().into(),
                now.clone().into(),
                now.clone().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    // 5. Write transition log
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
         (wo_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at) \
         VALUES (?, ?, 'cancelled', 'cancel', ?, NULL, ?, ?)",
        [
            input.id.into(),
            from_code.to_string().into(),
            input.actor_id.into(),
            input.cancel_reason.into(),
            now.into(),
        ],
    ))
    .await?;

    // 6. Re-fetch and return
    get_work_order(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.id.to_string(),
        })
}
