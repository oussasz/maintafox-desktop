//! DI query and mutation functions.
//!
//! Phase 2 – Sub-phase 04 – File 01 – Sprint S2.
//!
//! All functions use sea-orm raw SQL via `Statement::from_sql_and_values`
//! to stay consistent with the codebase's established query pattern.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::domain::{
    generate_di_code, guard_transition, map_intervention_request, DiStatus, InterventionRequest, IR_COLS, IR_JOINS,
    IR_JOIN_COLS,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Input / output types
// ═══════════════════════════════════════════════════════════════════════════════

/// Paginated list filter for intervention requests.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiListFilter {
    pub status: Option<Vec<String>>,
    pub equipment_id: Option<i64>,
    pub org_node_id: Option<i64>,
    pub submitter_id: Option<i64>,
    pub reviewer_id: Option<i64>,
    pub origin_type: Option<String>,
    pub urgency: Option<String>,
    pub disposition_code: Option<String>,
    pub search: Option<String>,
    /// Work queue of the requests screen ([`DI_QUEUES`]); `None` lists every request.
    #[serde(default)]
    pub queue: Option<String>,
    /// Only the open requests (`di::stats::OPEN_SQL`, the "open DI" count's rule).
    #[serde(default)]
    pub open_only: Option<bool>,
    /// Only the open requests submitted at least this many days ago (`di::stats::WAITING_SQL`, the
    /// "waiting more than N days" count of the home / analytics overdue figure). Implies open.
    #[serde(default)]
    pub waiting_days: Option<i64>,
    pub limit: i64,
    pub offset: i64,
}

/// Work queues of the requests screen, in the order of the request lifecycle.
///
/// - `to_screen` — submitted, waiting to be admitted for review;
/// - `to_review` — in review, waiting for the screening decision;
/// - `awaiting_info` — sent back to the requester for information;
/// - `to_approve` — screened, waiting for approval;
/// - `approved_not_converted` — approved, no work order yet;
/// - `deferred_due` — deferred and the deferral date has come;
/// - `sla_at_risk` — open with the SLA at risk or breached;
/// - `mine` — the open requests of the signed-in user (the command sets the submitter); their
///   closed requests stay reachable through the "all" list and its filters.
pub const DI_QUEUES: &[&str] = &[
    "to_screen",
    "to_review",
    "awaiting_info",
    "to_approve",
    "approved_not_converted",
    "deferred_due",
    "sla_at_risk",
    "mine",
];

/// SQL predicate (alias `ir`) of a work queue. For `mine` the caller adds the submitter (list:
/// `submitter_id` filter; count: the signed-in user), so list and count share one predicate.
fn queue_predicate(queue: &str) -> AppResult<Option<String>> {
    let sql = match queue {
        "to_screen" => "ir.status = 'submitted'".to_string(),
        "to_review" => "ir.status = 'in_review'".to_string(),
        "awaiting_info" => "ir.status = 'returned_for_clarification'".to_string(),
        "to_approve" => "ir.status = 'awaiting_approval'".to_string(),
        "approved_not_converted" => "(ir.status = 'approved' AND ir.converted_to_wo_id IS NULL)".to_string(),
        "deferred_due" => "(ir.status = 'deferred' AND ir.deferred_until IS NOT NULL \
             AND date(ir.deferred_until) <= date('now'))"
            .to_string(),
        "sla_at_risk" => super::sla::SLA_AT_RISK_OR_BREACHED_SQL.to_string(),
        "mine" => format!("ir.status NOT IN {}", super::stats::TERMINAL_STATUSES_SQL),
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_DI_QUEUE_UNKNOWN: File de travail inconnue : '{other}'."
            )]))
        }
    };
    Ok(Some(sql))
}

/// Most urgent first on the shared scale (validated urgency, else reported), from the scale's ranks.
fn urgency_rank_sql() -> String {
    let arms: String = crate::reference::urgency_scale::UrgencyCode::ALL
        .iter()
        .map(|c| format!(" WHEN '{}' THEN {}", c.as_str(), c.rank()))
        .collect();
    format!("CASE COALESCE(ir.validated_urgency, ir.reported_urgency){arms} ELSE 99 END")
}

/// Size of every work queue (the badges of the queue bar). `mine` counts the requests of `user_id`.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct DiQueueCounts {
    pub to_screen: i64,
    pub to_review: i64,
    pub awaiting_info: i64,
    pub to_approve: i64,
    pub approved_not_converted: i64,
    pub deferred_due: i64,
    pub sla_at_risk: i64,
    /// Open requests of the signed-in user.
    pub mine: i64,
    /// Requests of the signed-in user waiting for their answer.
    pub mine_awaiting_info: i64,
}

pub async fn count_di_queues(db: &impl ConnectionTrait, user_id: i64) -> AppResult<DiQueueCounts> {
    count_di_queues_in_scope(db, user_id, &super::access::ScopeSql::everywhere()).await
}

/// [`count_di_queues`] over the requests of the reader's scope (`di::access`): a site user's
/// badges count the requests of their sites only.
pub async fn count_di_queues_in_scope(
    db: &impl ConnectionTrait,
    user_id: i64,
    scope: &super::access::ScopeSql,
) -> AppResult<DiQueueCounts> {
    let mut arms: Vec<String> = Vec::new();
    for queue in DI_QUEUES.iter().filter(|q| **q != "mine") {
        if let Some(pred) = queue_predicate(queue)? {
            arms.push(format!(
                "COALESCE(SUM(CASE WHEN {pred} THEN 1 ELSE 0 END), 0) AS {queue}"
            ));
        }
    }
    let mine = queue_predicate("mine")?.unwrap_or_else(|| "1 = 1".to_string());
    let sql = format!(
        "SELECT {}, \
                COALESCE(SUM(CASE WHEN ir.submitter_id = ? AND {mine} THEN 1 ELSE 0 END), 0) AS mine, \
                COALESCE(SUM(CASE WHEN ir.submitter_id = ? AND ir.status = 'returned_for_clarification' \
                                  THEN 1 ELSE 0 END), 0) AS mine_awaiting_info \
           FROM intervention_requests ir \
          WHERE {}",
        arms.join(", "),
        scope.predicate
    );
    // The submitter binds (SELECT) come before the scope's (WHERE).
    let mut values: Vec<sea_orm::Value> = vec![user_id.into(), user_id.into()];
    values.extend(scope.binds.iter().cloned());
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("DI queue count returned no row")))?;
    let get = |col: &str| row.try_get::<i64>("", col).map_err(|e| decode_err(col, e));
    Ok(DiQueueCounts {
        to_screen: get("to_screen")?,
        to_review: get("to_review")?,
        awaiting_info: get("awaiting_info")?,
        to_approve: get("to_approve")?,
        approved_not_converted: get("approved_not_converted")?,
        deferred_due: get("deferred_due")?,
        sla_at_risk: get("sla_at_risk")?,
        mine: get("mine")?,
        mine_awaiting_info: get("mine_awaiting_info")?,
    })
}

/// Paginated list result.
#[derive(Debug, Clone, Serialize)]
pub struct DiListPage {
    pub items: Vec<InterventionRequest>,
    pub total: i64,
    /// Requests that have an org node but that the reader's sites do not cover (rule §2.4: shown
    /// as a count, never silently left out). Counted over ALL requests, ignoring every user
    /// filter and the search, so a site user cannot probe other sites' requests by filtering.
    /// Always 0 for a plant-wide reader.
    pub hidden_outside_scope: i64,
    /// Requests without an org node (rows from before decision O7), which only a plant-wide
    /// reader can see. Counted over ALL requests, ignoring every user filter and the search.
    /// Always 0 for a plant-wide reader.
    pub hidden_without_site: i64,
}

/// Row from `di_state_transition_log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiTransitionRow {
    pub id: i64,
    pub from_status: String,
    pub to_status: String,
    pub action: String,
    pub actor_id: Option<i64>,
    pub actor_display_name: Option<String>,
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
    pub equipment_id: i64,
    pub org_node_id: i64,
    pub title: String,
    pub description: String,
    pub origin_type: String,
    /// Category A `DI.REQUEST_TYPE` code. Defaults to `repair` when omitted.
    #[serde(default = "default_request_type")]
    pub request_type: String,
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

impl DiCreateInput {
    /// The requester is the signed-in user, never a value sent by the screen (a `di.create.own`
    /// holder could otherwise file a request in someone else's name).
    pub fn bind_requester(&mut self, session_user_id: i32) {
        self.submitter_id = i64::from(session_user_id);
    }
}

fn default_request_type() -> String {
    crate::di::reference_catalog::DEFAULT_DI_REQUEST_TYPE.to_string()
}

/// Supervisor triage/resubmission: move a DI into the review queue (`in_review`).
#[derive(Debug, Clone, Deserialize)]
pub struct DiTriageSubmittedInput {
    pub di_id: i64,
    pub expected_row_version: i64,
}

/// Input for updating draft/editable fields on an existing DI.
#[derive(Debug, Clone, Deserialize)]
pub struct DiDraftUpdateInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub request_type: Option<String>,
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
// Row mappers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("di query row decode failed for column '{column}': {e}"))
}

fn map_transition_row(row: &QueryResult) -> AppResult<DiTransitionRow> {
    Ok(DiTransitionRow {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
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
        actor_display_name: row
            .try_get::<Option<String>>("", "actor_display_name")
            .map_err(|e| decode_err("actor_display_name", e))?,
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
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        code: row.try_get::<String>("", "code").map_err(|e| decode_err("code", e))?,
        title: row.try_get::<String>("", "title").map_err(|e| decode_err("title", e))?,
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

pub async fn list_intervention_requests(db: &DatabaseConnection, filter: DiListFilter) -> AppResult<DiListPage> {
    list_intervention_requests_in_scope(db, filter, &crate::auth::scope::AccessScope::Everywhere).await
}

/// [`list_intervention_requests`] limited to the reader's scope (where they hold `di.view`), in
/// SQL before paging, with the count of the matching requests left out and why.
pub async fn list_intervention_requests_in_scope(
    db: &DatabaseConnection,
    filter: DiListFilter,
    scope: &crate::auth::scope::AccessScope,
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
    if let Some(equipment_id) = filter.equipment_id {
        where_clauses.push("ir.equipment_id = ?".to_string());
        binds.push(equipment_id.into());
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
    if let Some(ref disposition_code) = filter.disposition_code {
        if !disposition_code.is_empty() {
            where_clauses.push("ir.disposition_code = ?".to_string());
            binds.push(disposition_code.clone().into());
        }
    }

    // ── Shared scopes: the KPI counts' own predicates, so a drill lists exactly what was counted ──
    if filter.open_only.unwrap_or(false) || filter.waiting_days.is_some() {
        where_clauses.push(super::stats::OPEN_SQL.to_string());
    }
    if let Some(days) = filter.waiting_days {
        where_clauses.push(super::stats::WAITING_SQL.to_string());
        binds.push(super::stats::waiting_since(chrono::Utc::now(), days).into());
    }

    // ── Free-text search (title + description + code) ─────────────────────
    // Work queue
    let queue = filter.queue.as_deref().map(str::trim).filter(|q| !q.is_empty());
    if let Some(q) = queue {
        if let Some(pred) = queue_predicate(q)? {
            where_clauses.push(pred);
        }
    }

    if let Some(ref search) = filter.search {
        let trimmed = search.trim();
        if !trimmed.is_empty() {
            where_clauses.push("(ir.title LIKE ? OR ir.description LIKE ? OR ir.code LIKE ?)".to_string());
            let pattern = format!("%{trimmed}%");
            binds.push(pattern.clone().into());
            binds.push(pattern.clone().into());
            binds.push(pattern.into());
        }
    }

    let filters_sql = where_clauses.join(" AND ");

    // ── Site scope (rule §2.5): last predicate, so its binds follow the filters' ──
    let scope_sql = super::access::ScopeSql::of(scope);
    let (hidden_outside_scope, hidden_without_site) = if scope.is_everywhere() {
        (0, 0)
    } else {
        // Every request the scope leaves out (a NULL predicate — no node — counts as left out),
        // whatever the user's filters: a count that followed the search or the person filters
        // would let a site user probe other sites' request texts and people.
        let hidden_sql = format!(
            "SELECT COALESCE(SUM(CASE WHEN {without} THEN 1 ELSE 0 END), 0) AS without_site, \
                    COALESCE(SUM(CASE WHEN {without} THEN 0 ELSE 1 END), 0) AS outside_scope \
               FROM intervention_requests ir \
              WHERE NOT COALESCE(({scope_pred}), 0)",
            without = super::access::DI_WITHOUT_SITE_SQL,
            scope_pred = scope_sql.predicate,
        );
        let hidden_binds = scope_sql.binds.clone();
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                &hidden_sql,
                hidden_binds,
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("DI hidden count query returned no rows")))?;
        (
            row.try_get::<i64>("", "outside_scope")
                .map_err(|e| decode_err("outside_scope", e))?,
            row.try_get::<i64>("", "without_site")
                .map_err(|e| decode_err("without_site", e))?,
        )
    };
    let where_sql = format!("{filters_sql} AND ({})", scope_sql.predicate);
    binds.extend(scope_sql.binds);

    // ── Count query (same WHERE, no joins needed for count) ───────────────
    let count_sql = format!("SELECT COUNT(*) AS total FROM intervention_requests ir WHERE {where_sql}");
    let count_binds = binds.clone();
    let count_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &count_sql,
            count_binds,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("DI count query returned no rows")))?;
    let total: i64 = count_row
        .try_get::<i64>("", "total")
        .map_err(|e| decode_err("total", e))?;

    // ── Data query with LIMIT/OFFSET ──────────────────────────────────────
    let row_limit = filter.limit.clamp(1, 200);
    let offset = filter.offset.max(0);

    // A work queue is worked most urgent first, then oldest first (first in, first out); the
    // other lists show the latest requests first.
    let order_sql = match queue {
        Some(q) if q != "mine" => format!("{} ASC, ir.submitted_at ASC, ir.id ASC", urgency_rank_sql()),
        _ => "ir.submitted_at DESC, ir.id DESC".to_string(),
    };
    let data_sql = format!(
        "SELECT {IR_COLS}, {IR_JOIN_COLS} \
         FROM intervention_requests ir \
         {IR_JOINS} \
         WHERE {where_sql} \
         ORDER BY {order_sql} \
         LIMIT {row_limit} OFFSET {offset}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &data_sql, binds))
        .await?;

    let items: Vec<InterventionRequest> = rows
        .iter()
        .map(map_intervention_request)
        .collect::<AppResult<Vec<_>>>()?;

    Ok(DiListPage {
        items,
        total,
        hidden_outside_scope,
        hidden_without_site,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_intervention_request — single row by id
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_intervention_request(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<InterventionRequest>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE ir.id = ?"
            ),
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

pub async fn get_di_transition_log(db: &DatabaseConnection, di_id: i64) -> AppResult<Vec<DiTransitionRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.id, t.from_status, t.to_status, t.action, t.actor_id, \
                    COALESCE(ua.display_name, ua.username) AS actor_display_name, \
                    t.reason_code, t.notes, t.acted_at \
             FROM di_state_transition_log t \
             LEFT JOIN user_accounts ua ON ua.id = t.actor_id \
             WHERE t.di_id = ? \
             ORDER BY t.acted_at ASC",
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
    equipment_id: i64,
    symptom_code_id: Option<i64>,
    days: i64,
) -> AppResult<Vec<DiSummaryRow>> {
    get_recent_similar_dis_in_scope(
        db,
        equipment_id,
        symptom_code_id,
        days,
        &crate::auth::scope::AccessScope::Everywhere,
    )
    .await
}

/// [`get_recent_similar_dis`] among the requests the reader may see.
pub async fn get_recent_similar_dis_in_scope(
    db: &DatabaseConnection,
    equipment_id: i64,
    symptom_code_id: Option<i64>,
    days: i64,
    scope: &crate::auth::scope::AccessScope,
) -> AppResult<Vec<DiSummaryRow>> {
    let mut where_clauses = vec![
        "equipment_id = ?".to_string(),
        format!("submitted_at >= datetime('now', '-{} days')", days.max(1)),
    ];
    let mut binds: Vec<sea_orm::Value> = vec![equipment_id.into()];

    if let Some(symptom_id) = symptom_code_id {
        where_clauses.push("symptom_code_id = ?".to_string());
        binds.push(symptom_id.into());
    }
    let scope_sql = super::access::ScopeSql::on(scope, "intervention_requests.org_node_id");
    where_clauses.push(format!("({})", scope_sql.predicate));
    binds.extend(scope_sql.binds);

    let where_sql = where_clauses.join(" AND ");
    let sql = format!(
        "SELECT id, code, title, status, submitted_at \
         FROM intervention_requests \
         WHERE {where_sql} \
         ORDER BY submitted_at DESC \
         LIMIT 5"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
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
    // The DI row, its logs and its sync outbox entry commit together.
    let txn = db.begin().await?;
    let di = create_intervention_request_in(&txn, input).await?;
    txn.commit().await?;
    Ok(di)
}

/// [`create_intervention_request`] in the caller's transaction: what the request is raised from is
/// linked to it in the same transaction (`inspection::routing`), never left half done.
pub async fn create_intervention_request_in(
    db: &impl ConnectionTrait,
    input: DiCreateInput,
) -> AppResult<InterventionRequest> {
    // Catalog membership (Category B) — same philosophy as equipment taxonomy.
    let origin_type = crate::di::reference_catalog::validate_di_origin_code(db, &input.origin_type).await?;
    let request_type = crate::di::reference_catalog::validate_di_request_type(db, &input.request_type).await?;
    let symptom_code_id = crate::di::reference_catalog::require_di_symptom_id(db, input.symptom_code_id).await?;
    // Shared DI / OT urgency scale (`WORK.URGENCY`).
    let reported_urgency = crate::reference::urgency_scale::validate_urgency_code(&input.reported_urgency)?;

    // Decision O7: every new request names its org node (its anchor for site-scoped roles).
    super::access::require_org_node(input.org_node_id)?;
    crate::org::model_scope::assert_org_node_active(db, input.org_node_id).await?;

    let code = generate_di_code(db).await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO intervention_requests (\
            entity_sync_id, code, equipment_id, org_node_id, status, title, description, origin_type, request_type, \
            symptom_code_id, impact_level, production_impact, safety_flag, \
            environmental_flag, quality_flag, reported_urgency, observed_at, \
            submitted_at, submitter_id, source_inspection_anomaly_id, row_version, created_at, updated_at\
         ) VALUES (?, ?, ?, ?, 'submitted', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            entity_sync_id.into(),
            code.clone().into(),
            input.equipment_id.into(),
            input.org_node_id.into(),
            input.title.into(),
            input.description.into(),
            origin_type.into(),
            request_type.into(),
            symptom_code_id.into(),
            input.impact_level.into(),
            (i64::from(input.production_impact)).into(),
            (i64::from(input.safety_flag)).into(),
            (i64::from(input.environmental_flag)).into(),
            (i64::from(input.quality_flag)).into(),
            reported_urgency.as_str().into(),
            input
                .observed_at
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
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
                "GATE_DI_CODE_DUPLICATE: Un code DI en doublon a été généré. Veuillez réessayer.".into(),
            ])
        } else if e.to_string().contains("FOREIGN KEY") {
            AppError::ValidationFailed(vec![
                "GATE_DI_REFERENCE_INVALID: Référence invalide (equipment_id, org_node_id, ou submitter_id).".into(),
            ])
        } else {
            AppError::Database(e)
        }
    })?;

    // Fetch the inserted row by code (code is UNIQUE)
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE ir.code = ?"
            ),
            [code.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read DI after insert: code={code}")))?;

    let di = map_intervention_request(&row)?;

    // Write initial transition log entry
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_state_transition_log (di_id, from_status, to_status, action, actor_id, acted_at) \
         VALUES (?, 'none', 'submitted', 'intake_submitted', ?, ?)",
        [di.id.into(), di.submitter_id.into(), now.clone().into()],
    ))
    .await?;

    // Freeze immutable SLA on the DI, then persist full audit snapshot.
    let di = super::sla::freeze_sla_on_di(db, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_review_events \
            (di_id, event_type, actor_id, acted_at, from_status, to_status, \
             reason_code, notes, sla_target_hours, sla_deadline, \
             sla_resolution_target_hours, sla_resolution_deadline, step_up_used) \
         VALUES (?, 'sla_initialized', ?, ?, 'none', 'submitted', NULL, NULL, ?, ?, ?, ?, 0)",
        [
            di.id.into(),
            di.submitter_id.into(),
            now.into(),
            snap.response_target_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            snap.response_deadline
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            snap.resolution_target_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            snap.resolution_deadline
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) update_di_draft_fields — optimistic concurrency + status guard
// ═══════════════════════════════════════════════════════════════════════════════

/// `actor_id`: who makes the change (recorded on the work orders a newly set safety flag protects).
pub async fn update_di_draft_fields(
    db: &DatabaseConnection,
    input: DiDraftUpdateInput,
    actor_id: Option<i64>,
) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;

    // 1. Fetch current row to validate status
    let current = get_intervention_request(&txn, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.id.to_string(),
        })?;

    let status = DiStatus::try_from_str(&current.status)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}")))?;

    // 2. Guard: only allow draft edits on mutable intake states
    if !matches!(status, DiStatus::Submitted | DiStatus::ReturnedForClarification) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_EDIT_STATUS_INVALID: Les champs ne peuvent être modifiés qu'au statut 'submitted' ou \
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
    if let Some(ref request_type) = input.request_type {
        let validated = crate::di::reference_catalog::validate_di_request_type(&txn, request_type).await?;
        sets.push("request_type = ?".into());
        values.push(validated.into());
    }
    if let Some(ref symptom_code_id) = input.symptom_code_id {
        match symptom_code_id {
            None => {
                return Err(AppError::ValidationFailed(vec![
                    "GATE_DI_SYMPTOM_REQUIRED: Le symptôme est obligatoire.".into(),
                ]));
            }
            Some(id) => {
                // Validate only when writing a new value (legacy rows keep existing id until changed).
                crate::di::reference_catalog::validate_di_symptom_id(&txn, *id).await?;
                sets.push("symptom_code_id = ?".into());
                values.push((*id).into());
            }
        }
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
        let level = crate::reference::urgency_scale::validate_urgency_code(reported_urgency)?;
        sets.push("reported_urgency = ?".into());
        values.push(level.as_str().into());
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
        txn.commit().await?;
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

    let result = txn
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    // A request flagged "sécurité" now: its work orders not started yet require a permit, protected
    // like a converted one (audit finding AU-01, permit_from_safety_di), in this transaction.
    if input.safety_flag == Some(true) && !current.safety_flag {
        crate::wo::workflow::actions::permit_requirement::raise_work_orders_of_safety_request(&txn, input.id, actor_id)
            .await?;
    }

    // 5. Re-fetch and return updated row
    let di = get_intervention_request(&txn, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.id.to_string(),
        })?;
    txn.commit().await?;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) triage_submitted_di — Submitted → InReview (supervisor / planner triage)
// ═══════════════════════════════════════════════════════════════════════════════

/// Move a DI from `submitted` or `returned_for_clarification` to `in_review`.
pub async fn triage_submitted_di(
    db: &DatabaseConnection,
    input: DiTriageSubmittedInput,
    actor_id: i64,
) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let current = get_intervention_request(&txn, input.di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    let status = DiStatus::try_from_str(&current.status)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}")))?;

    if !matches!(status, DiStatus::Submitted | DiStatus::ReturnedForClarification) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_SUBMIT_STATUS_INVALID: Seules les demandes au statut « soumis » ou « retourné pour clarification » \
             peuvent être (re)soumises vers la revue. \
             Statut actuel : '{}'.",
            current.status
        )]));
    }

    guard_transition(&status, &DiStatus::InReview).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'in_review', \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_state_transition_log \
            (di_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.di_id.into(),
            status.as_str().into(),
            DiStatus::InReview.as_str().into(),
            if status == DiStatus::ReturnedForClarification {
                "re_submitted".into()
            } else {
                "triage_accept".into()
            },
            actor_id.into(),
            sea_orm::Value::from(None::<String>),
            sea_orm::Value::from(None::<String>),
            now.into(),
        ],
    ))
    .await?;

    let updated = get_intervention_request(&txn, input.di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;
    txn.commit().await?;
    super::notifications::notify_entered_in_review(db, &updated).await;
    Ok(updated)
}
