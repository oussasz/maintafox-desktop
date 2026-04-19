//! DI query and mutation functions.
//!
//! Phase 2 – Sub-phase 04 – File 01 – Sprint S2.
//!
//! All functions use sea-orm raw SQL via `Statement::from_sql_and_values`
//! to stay consistent with the codebase's established query pattern.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use super::domain::{
    generate_di_code, map_intervention_request, DiOriginType, DiStatus, InterventionRequest,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Input / output types
// ═══════════════════════════════════════════════════════════════════════════════

/// Paginated list filter for intervention requests.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiListFilter {
    pub status: Option<Vec<String>>,
    pub asset_id: Option<i64>,
    pub org_node_id: Option<i64>,
    pub submitter_id: Option<i64>,
    pub reviewer_id: Option<i64>,
    pub origin_type: Option<String>,
    pub urgency: Option<String>,
    pub search: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

/// Paginated list result.
#[derive(Debug, Clone, Serialize)]
pub struct DiListPage {
    pub items: Vec<InterventionRequest>,
    pub total: i64,
}

/// Row from `di_state_transition_log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiTransitionRow {
    pub id: i64,
    pub from_status: String,
    pub to_status: String,
    pub action: String,
    pub actor_id: Option<i64>,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
    pub acted_at: String,
}

/// Lightweight DI summary for recurrence/similar-DI lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiSummaryRow {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub status: String,
    pub submitted_at: String,
}

/// Input for creating a new intervention request.
#[derive(Debug, Clone, Deserialize)]
pub struct DiCreateInput {
    pub asset_id: i64,
    pub org_node_id: i64,
    pub title: String,
    pub description: String,
    pub origin_type: String,
    pub symptom_code_id: Option<i64>,
    pub impact_level: String,
    pub production_impact: bool,
    pub safety_flag: bool,
    pub environmental_flag: bool,
    pub quality_flag: bool,
    pub reported_urgency: String,
    pub observed_at: Option<String>,
    pub submitter_id: i64,
    #[serde(default)]
    pub source_inspection_anomaly_id: Option<i64>,
}

/// Input for updating draft/editable fields on an existing DI.
#[derive(Debug, Clone, Deserialize)]
pub struct DiDraftUpdateInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub symptom_code_id: Option<Option<i64>>,
    pub impact_level: Option<String>,
    pub production_impact: Option<bool>,
    pub safety_flag: Option<bool>,
    pub environmental_flag: Option<bool>,
    pub quality_flag: Option<bool>,
    pub reported_urgency: Option<String>,
    pub observed_at: Option<Option<String>>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// All columns from `intervention_requests` for SELECT reuse.
const IR_COLS: &str = "\
    ir.id, ir.code, ir.asset_id, ir.sub_asset_ref, ir.org_node_id, \
    ir.status, ir.title, ir.description, ir.origin_type, ir.symptom_code_id, \
    ir.impact_level, ir.production_impact, ir.safety_flag, ir.environmental_flag, \
    ir.quality_flag, ir.reported_urgency, ir.validated_urgency, \
    ir.observed_at, ir.submitted_at, \
    ir.review_team_id, ir.reviewer_id, ir.screened_at, ir.approved_at, \
    ir.deferred_until, ir.declined_at, ir.closed_at, ir.archived_at, \
    ir.converted_to_wo_id, ir.converted_at, \
    ir.reviewer_note, ir.classification_code_id, \
    ir.is_recurrence_flag, ir.recurrence_di_id, \
    ir.source_inspection_anomaly_id, \
    ir.row_version, ir.submitter_id, ir.created_at, ir.updated_at";

// ═══════════════════════════════════════════════════════════════════════════════
// Row mappers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "di query row decode failed for column '{column}': {e}"
    ))
}

fn map_transition_row(row: &QueryResult) -> AppResult<DiTransitionRow> {
    Ok(DiTransitionRow {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        from_status: row
            .try_get::<String>("", "from_status")
            .map_err(|e| decode_err("from_status", e))?,
        to_status: row
            .try_get::<String>("", "to_status")
            .map_err(|e| decode_err("to_status", e))?,
        action: row
            .try_get::<String>("", "action")
            .map_err(|e| decode_err("action", e))?,
        actor_id: row
            .try_get::<Option<i64>>("", "actor_id")
            .map_err(|e| decode_err("actor_id", e))?,
        reason_code: row
            .try_get::<Option<String>>("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        acted_at: row
            .try_get::<String>("", "acted_at")
            .map_err(|e| decode_err("acted_at", e))?,
    })
}

fn map_summary_row(row: &QueryResult) -> AppResult<DiSummaryRow> {
    Ok(DiSummaryRow {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
        title: row
            .try_get::<String>("", "title")
            .map_err(|e| decode_err("title", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        submitted_at: row
            .try_get::<String>("", "submitted_at")
            .map_err(|e| decode_err("submitted_at", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_intervention_requests — paginated, filtered
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_intervention_requests(
    db: &DatabaseConnection,
    filter: DiListFilter,
) -> AppResult<DiListPage> {
    let mut where_clauses: Vec<String> = vec!["1 = 1".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    // ── Status filter (multi-select) ──────────────────────────────────────
    if let Some(ref statuses) = filter.status {
        if !statuses.is_empty() {
            let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("ir.status IN ({placeholders})"));
            for s in statuses {
                binds.push(s.clone().into());
            }
        }
    }

    // ── Scalar FK filters ─────────────────────────────────────────────────
    if let Some(asset_id) = filter.asset_id {
        where_clauses.push("ir.asset_id = ?".to_string());
        binds.push(asset_id.into());
    }
    if let Some(org_node_id) = filter.org_node_id {
        where_clauses.push("ir.org_node_id = ?".to_string());
        binds.push(org_node_id.into());
    }
    if let Some(submitter_id) = filter.submitter_id {
        where_clauses.push("ir.submitter_id = ?".to_string());
        binds.push(submitter_id.into());
    }
    if let Some(reviewer_id) = filter.reviewer_id {
        where_clauses.push("ir.reviewer_id = ?".to_string());
        binds.push(reviewer_id.into());
    }

    // ── Enum string filters ───────────────────────────────────────────────
    if let Some(ref origin_type) = filter.origin_type {
        if !origin_type.is_empty() {
            where_clauses.push("ir.origin_type = ?".to_string());
            binds.push(origin_type.clone().into());
        }
    }
    if let Some(ref urgency) = filter.urgency {
        if !urgency.is_empty() {
            where_clauses.push("ir.reported_urgency = ?".to_string());
            binds.push(urgency.clone().into());
        }
    }

    // ── Free-text search (title + description + code) ─────────────────────
    if let Some(ref search) = filter.search {
        let trimmed = search.trim();
        if !trimmed.is_empty() {
            where_clauses
                .push("(ir.title LIKE ? OR ir.description LIKE ? OR ir.code LIKE ?)".to_string());
            let pattern = format!("%{trimmed}%");
            binds.push(pattern.clone().into());
            binds.push(pattern.clone().into());
            binds.push(pattern.into());
        }
    }

    let where_sql = where_clauses.join(" AND ");

    // ── Count query (same WHERE, no joins needed for count) ───────────────
    let count_sql = format!(
        "SELECT COUNT(*) AS total FROM intervention_requests ir WHERE {where_sql}"
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
            AppError::Internal(anyhow::anyhow!("DI count query returned no rows"))
        })?;
    let total: i64 = count_row
        .try_get::<i64>("", "total")
        .map_err(|e| decode_err("total", e))?;

    // ── Data query with LIMIT/OFFSET ──────────────────────────────────────
    let row_limit = filter.limit.min(200).max(1);
    let offset = filter.offset.max(0);

    let data_sql = format!(
        "SELECT {IR_COLS} \
         FROM intervention_requests ir \
         WHERE {where_sql} \
         ORDER BY ir.submitted_at DESC \
         LIMIT {row_limit} OFFSET {offset}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &data_sql,
            binds,
        ))
        .await?;

    let items: Vec<InterventionRequest> = rows
        .iter()
        .map(map_intervention_request)
        .collect::<AppResult<Vec<_>>>()?;

    Ok(DiListPage { items, total })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_intervention_request — single row by id
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_intervention_request(
    db: &DatabaseConnection,
    id: i64,
) -> AppResult<Option<InterventionRequest>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {IR_COLS} FROM intervention_requests ir WHERE ir.id = ?"),
            [id.into()],
        ))
        .await?;

    match row {
        Some(r) => Ok(Some(map_intervention_request(&r)?)),
        None => Ok(None),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) get_di_transition_log — append-only log for a DI
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_di_transition_log(
    db: &DatabaseConnection,
    di_id: i64,
) -> AppResult<Vec<DiTransitionRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, from_status, to_status, action, actor_id, reason_code, notes, acted_at \
             FROM di_state_transition_log \
             WHERE di_id = ? \
             ORDER BY acted_at ASC",
            [di_id.into()],
        ))
        .await?;

    rows.iter().map(map_transition_row).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) get_recent_similar_dis — recurrence context for reviewers
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_recent_similar_dis(
    db: &DatabaseConnection,
    asset_id: i64,
    symptom_code_id: Option<i64>,
    days: i64,
) -> AppResult<Vec<DiSummaryRow>> {
    let mut where_clauses = vec![
        "asset_id = ?".to_string(),
        format!(
            "submitted_at >= datetime('now', '-{} days')",
            days.max(1)
        ),
    ];
    let mut binds: Vec<sea_orm::Value> = vec![asset_id.into()];

    if let Some(symptom_id) = symptom_code_id {
        where_clauses.push("symptom_code_id = ?".to_string());
        binds.push(symptom_id.into());
    }

    let where_sql = where_clauses.join(" AND ");
    let sql = format!(
        "SELECT id, code, title, status, submitted_at \
         FROM intervention_requests \
         WHERE {where_sql} \
         ORDER BY submitted_at DESC \
         LIMIT 5"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    rows.iter().map(map_summary_row).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) create_intervention_request — full insert + transition log
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn create_intervention_request(
    db: &DatabaseConnection,
    input: DiCreateInput,
) -> AppResult<InterventionRequest> {
    // Validate origin_type is a legal enum value
    DiOriginType::try_from_str(&input.origin_type).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    let code = generate_di_code(db).await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO intervention_requests (\
            code, asset_id, org_node_id, status, title, description, origin_type, \
            symptom_code_id, impact_level, production_impact, safety_flag, \
            environmental_flag, quality_flag, reported_urgency, observed_at, \
            submitted_at, submitter_id, source_inspection_anomaly_id, row_version, created_at, updated_at\
         ) VALUES (?, ?, ?, 'submitted', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            code.clone().into(),
            input.asset_id.into(),
            input.org_node_id.into(),
            input.title.into(),
            input.description.into(),
            input.origin_type.into(),
            input.symptom_code_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<i64>)),
            input.impact_level.into(),
            (i64::from(input.production_impact)).into(),
            (i64::from(input.safety_flag)).into(),
            (i64::from(input.environmental_flag)).into(),
            (i64::from(input.quality_flag)).into(),
            input.reported_urgency.into(),
            input.observed_at.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            now.clone().into(),
            input.submitter_id.into(),
            input
                .source_inspection_anomaly_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![
                "Un code DI en doublon a été généré. Veuillez réessayer.".into(),
            ])
        } else if e.to_string().contains("FOREIGN KEY") {
            AppError::ValidationFailed(vec![
                "Référence invalide (asset_id, org_node_id, ou submitter_id).".into(),
            ])
        } else {
            AppError::Database(e)
        }
    })?;

    // Fetch the inserted row by code (code is UNIQUE)
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {IR_COLS} FROM intervention_requests ir WHERE ir.code = ?"),
            [code.clone().into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-read DI after insert: code={code}"
            ))
        })?;

    let di = map_intervention_request(&row)?;

    // Write initial transition log entry
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_state_transition_log (di_id, from_status, to_status, action, actor_id, acted_at) \
         VALUES (?, 'none', 'submitted', 'submit', ?, ?)",
        [di.id.into(), di.submitter_id.into(), now.into()],
    ))
    .await?;

    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) update_di_draft_fields — optimistic concurrency + status guard
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn update_di_draft_fields(
    db: &DatabaseConnection,
    input: DiDraftUpdateInput,
) -> AppResult<InterventionRequest> {
    // 1. Fetch current row to validate status
    let current = get_intervention_request(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.id.to_string(),
        })?;

    let status = DiStatus::try_from_str(&current.status).map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}"))
    })?;

    // 2. Guard: only allow draft edits on mutable intake states
    if !matches!(
        status,
        DiStatus::Submitted | DiStatus::ReturnedForClarification
    ) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Les champs ne peuvent être modifiés qu'au statut 'submitted' ou \
             'returned_for_clarification'. Statut actuel : '{}'.",
            current.status
        )]));
    }

    // 3. Build dynamic SET clause
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref title) = input.title {
        sets.push("title = ?".into());
        values.push(title.clone().into());
    }
    if let Some(ref description) = input.description {
        sets.push("description = ?".into());
        values.push(description.clone().into());
    }
    if let Some(ref symptom_code_id) = input.symptom_code_id {
        sets.push("symptom_code_id = ?".into());
        values.push(
            symptom_code_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
        );
    }
    if let Some(ref impact_level) = input.impact_level {
        sets.push("impact_level = ?".into());
        values.push(impact_level.clone().into());
    }
    if let Some(production_impact) = input.production_impact {
        sets.push("production_impact = ?".into());
        values.push(i64::from(production_impact).into());
    }
    if let Some(safety_flag) = input.safety_flag {
        sets.push("safety_flag = ?".into());
        values.push(i64::from(safety_flag).into());
    }
    if let Some(environmental_flag) = input.environmental_flag {
        sets.push("environmental_flag = ?".into());
        values.push(i64::from(environmental_flag).into());
    }
    if let Some(quality_flag) = input.quality_flag {
        sets.push("quality_flag = ?".into());
        values.push(i64::from(quality_flag).into());
    }
    if let Some(ref reported_urgency) = input.reported_urgency {
        sets.push("reported_urgency = ?".into());
        values.push(reported_urgency.clone().into());
    }
    if let Some(ref observed_at) = input.observed_at {
        sets.push("observed_at = ?".into());
        values.push(
            observed_at
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        );
    }

    if sets.is_empty() {
        // Nothing to update — return current row as-is
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
        "UPDATE intervention_requests SET {} WHERE id = ? AND row_version = ?",
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
    get_intervention_request(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.id.to_string(),
        })
}
