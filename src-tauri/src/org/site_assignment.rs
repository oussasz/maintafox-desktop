//! "Assign a site" to old records without one (CMMS Advisor decision Q1, step 6 site-scoped roles,
//! `docs/SITE_SCOPED_ROLES.md`).
//!
//! Work orders (`work_orders.entity_id`), requests (`intervention_requests.org_node_id`) and planning
//! windows (`planning_windows.entity_id`) recorded before a site was required may have none (NULL, or
//! the `0` of old request rows). A site-limited user cannot act on them (`SCOPE_RECORD_WITHOUT_SITE`);
//! the lists flag them "To review: no site". This module lets a plant-wide organisation administrator
//! give them a site:
//! - only a record that has no site gets one (a record with a site is refused: never an edit-site
//!   path), whatever its status, closed and cancelled ones included;
//! - a record is given a site this way once: a record that already carries the trace of an
//!   assignment (a planning window a planner set back to plant-wide since) is refused and not
//!   listed, because that trace is fixed for good on the other PCs;
//! - the right is `org.admin` held plant-wide through a permanent role (as the other PCs re-check
//!   it); a site-limited administrator, or one holding it only by an emergency grant, is refused;
//! - the site is a live node of the published organisation structure, valid for the record kind
//!   (the rule used when such a record is created: a live node for a work order, an active node for a
//!   request, the node holding the window's equipment for a planning window);
//! - one reason for the batch (at most [`MAX_REASON_CHARS`] characters), one audit entry per record
//!   (before: no site, after: the node; who, when, why), written in the same transaction; the batch
//!   is applied in full or not at all;
//! - nothing else of the record changes (its row version moves on, as for every edit, and the change
//!   is captured for sync as usual). A planning window flagged "Site to re-enter" is settled by it,
//!   as when a planner re-enters its site.
//!
//! The record keeps who gave the site, when and why (migration 245); that trace travels with the row
//! and the other PCs re-check it on arrival ([`inbound_site_assignment_check`]): the author holds the
//! right plant-wide there and the record still has no site there, else a sync conflict.
//!
//! A planning window without a site is valid (it applies to the whole plant): it is listed so an
//! administrator may give it a site, and counted "to review" only when it is flagged "Site to
//! re-enter" (a window saved with the old dialog, which lost its site).

use std::collections::{BTreeSet, HashMap};

use chrono::Duration;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::commands::admin_users::AssignableScopeNode;
use crate::errors::{org_validation_failed, AppError, AppResult};
use crate::rbac::permissions::ORG_ADMIN;
use crate::settings::catalog::MIN_REASON_CHARS;
use crate::settings::site::{instant, InboundVerdict, MAX_CLOCK_SKEW_MINUTES};

/// Records listed per kind (the counts give the full number; the screen says when it shows fewer).
pub const MAX_LISTED: i64 = 500;

/// Records given a site in one batch.
pub const MAX_BATCH: usize = 500;

/// The reason has at most this many characters (Unicode characters, as [`MIN_REASON_CHARS`] counts
/// them; the screen counts the same way). It is stored on the record and travels with it to every PC.
pub const MAX_REASON_CHARS: usize = 1000;

/// Audit action of an assigned site (`audit_events.action_code`).
pub const AUDIT_ACTION: &str = "record.site_assigned";

/// The kinds of record that can be given a site here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoSiteKind {
    WorkOrder,
    Request,
    PlanningWindow,
}

impl NoSiteKind {
    /// The table (fixed names, never user input).
    fn table(self) -> &'static str {
        match self {
            Self::WorkOrder => "work_orders",
            Self::Request => "intervention_requests",
            Self::PlanningWindow => "planning_windows",
        }
    }

    /// The site column.
    fn site_column(self) -> &'static str {
        match self {
            Self::WorkOrder | Self::PlanningWindow => "entity_id",
            Self::Request => "org_node_id",
        }
    }

    /// The site column as it travels in a sync payload (`sync::row_layout::ref_payload_key`).
    fn site_payload_key(self) -> &'static str {
        match self {
            Self::WorkOrder | Self::PlanningWindow => "entity_sync_id",
            Self::Request => "org_node_sync_id",
        }
    }

    /// `audit_events.target_type` (mapped for sync in `sync::row_spec`).
    fn audit_target(self) -> &'static str {
        match self {
            Self::WorkOrder => "work_order",
            Self::Request => "intervention_request",
            Self::PlanningWindow => "planning_window",
        }
    }

    /// The record's readable reference (alias `t`): its code; a window has none.
    fn code_sql(self) -> &'static str {
        match self {
            Self::WorkOrder | Self::Request => "t.code",
            Self::PlanningWindow => "NULL",
        }
    }

    /// The record without a usable site (alias `t`): NULL, `0` or negative, as the lists count it
    /// (`wo::access::WO_WITHOUT_SITE_SQL`, `di::access::DI_WITHOUT_SITE_SQL`).
    fn without_site_sql(self) -> &'static str {
        match self {
            Self::WorkOrder | Self::PlanningWindow => "(t.entity_id IS NULL OR t.entity_id <= 0)",
            Self::Request => "(t.org_node_id IS NULL OR t.org_node_id <= 0)",
        }
    }

    /// A record (alias `t`) without a site that can be given one here: no site, and no trace of an
    /// earlier assignment ([`assign_site`] refuses a second one).
    fn assignable_sql(self) -> String {
        format!(
            "{} AND t.site_assigned_by_id IS NULL AND t.site_assigned_at IS NULL AND t.site_assign_reason IS NULL",
            self.without_site_sql()
        )
    }

    /// An org node (alias `n`) that can be suggested or given to this kind of record: live, in the
    /// published structure, and active for a request (`org::model_scope::assert_org_node_active`).
    fn usable_node_sql(self) -> &'static str {
        match self {
            Self::Request => {
                "n.deleted_at IS NULL AND n.status = 'active' \
                 AND n.structure_model_id IN (SELECT id FROM org_structure_models WHERE status = 'active')"
            }
            Self::WorkOrder | Self::PlanningWindow => {
                "n.deleted_at IS NULL \
                 AND n.structure_model_id IN (SELECT id FROM org_structure_models WHERE status = 'active')"
            }
        }
    }
}

/// How many records have no site, per kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NoSiteCounts {
    pub work_orders: i64,
    pub requests: i64,
    /// Windows without a site flagged "Site to re-enter" (they lost their site).
    pub planning_windows_to_review: i64,
    /// Windows without a site not flagged: plant-wide windows, valid as they are.
    pub planning_windows_plant_wide: i64,
}

impl NoSiteCounts {
    /// The records "to review: no site" (plant-wide windows are valid, not counted).
    pub fn to_review(&self) -> i64 {
        self.work_orders + self.requests + self.planning_windows_to_review
    }
}

/// A record without a site, as the screen lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoSiteRecord {
    pub kind: NoSiteKind,
    pub id: i64,
    /// Work-order or request code; none for a window.
    pub code: Option<String>,
    /// Work-order or request title; the window type for a window.
    pub title: Option<String>,
    /// Work-order status code or request status; none for a window.
    pub status_code: Option<String>,
    pub created_at: Option<String>,
    /// A window's period.
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub equipment_id: Option<i64>,
    pub equipment_code: Option<String>,
    pub equipment_name: Option<String>,
    /// The equipment's current location, when it is a node this record can be given: a suggestion
    /// the administrator confirms, never applied by itself.
    pub suggested_node_id: Option<i64>,
    pub suggested_node_name: Option<String>,
    /// Counted "to review" (every work order and request; a window flagged "Site to re-enter").
    pub to_review: bool,
}

/// What the screen shows for one kind: the counts of every kind, the records of this one, and the
/// nodes a site can be chosen from.
#[derive(Debug, Serialize)]
pub struct RecordsWithoutSite {
    pub kind: NoSiteKind,
    pub counts: NoSiteCounts,
    pub records: Vec<NoSiteRecord>,
    /// At most this many records are listed (`counts` gives the full number).
    pub listed_limit: i64,
    /// At most this many records are given a site in one batch.
    pub max_batch: usize,
    /// The reason has at least this many characters.
    pub reason_min_chars: usize,
    /// The reason has at most this many characters.
    pub reason_max_chars: usize,
    pub nodes: Vec<AssignableScopeNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssignSiteInput {
    pub kind: NoSiteKind,
    pub record_ids: Vec<i64>,
    pub org_node_id: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignSiteResult {
    pub kind: NoSiteKind,
    pub org_node_id: i64,
    /// The records given the site (sorted).
    pub assigned: Vec<i64>,
    /// The counts after the change.
    pub counts: NoSiteCounts,
}

fn refusal(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    org_validation_failed(
        code,
        message,
        params
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect::<HashMap<_, _>>(),
    )
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "records without site: decode of '{column}' failed: {e}"
    ))
}

async fn count(db: &impl ConnectionTrait, sql: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count returned no row")))?;
    row.try_get::<i64>("", "c").map_err(|e| decode_err("c", e))
}

/// How many records have no site and can be given one here, per kind (every record, whatever its
/// status; a record already given a site once is not counted, see [`NoSiteKind::assignable_sql`]).
pub async fn count_records_without_site(db: &impl ConnectionTrait) -> AppResult<NoSiteCounts> {
    Ok(NoSiteCounts {
        work_orders: count(
            db,
            &format!(
                "SELECT COUNT(*) AS c FROM work_orders t WHERE {}",
                NoSiteKind::WorkOrder.assignable_sql()
            ),
        )
        .await?,
        requests: count(
            db,
            &format!(
                "SELECT COUNT(*) AS c FROM intervention_requests t WHERE {}",
                NoSiteKind::Request.assignable_sql()
            ),
        )
        .await?,
        planning_windows_to_review: count(
            db,
            &format!(
                "SELECT COUNT(*) AS c FROM planning_windows t WHERE {} AND t.scope_review_required = 1",
                NoSiteKind::PlanningWindow.assignable_sql()
            ),
        )
        .await?,
        planning_windows_plant_wide: count(
            db,
            &format!(
                "SELECT COUNT(*) AS c FROM planning_windows t WHERE {} AND t.scope_review_required = 0",
                NoSiteKind::PlanningWindow.assignable_sql()
            ),
        )
        .await?,
    })
}

/// The records of `kind` without a site that can be given one here (most recent first; windows to
/// review first), at most [`MAX_LISTED`].
pub async fn list_records_of_kind(db: &impl ConnectionTrait, kind: NoSiteKind) -> AppResult<Vec<NoSiteRecord>> {
    let usable = kind.usable_node_sql();
    let without = kind.assignable_sql();
    let sql = match kind {
        NoSiteKind::WorkOrder => format!(
            "SELECT t.id, t.code AS code, t.title AS title, s.code AS status_code, t.created_at AS created_at, \
                    NULL AS window_start, NULL AS window_end, t.equipment_id AS equipment_id, \
                    e.asset_id_code AS equipment_code, e.name AS equipment_name, \
                    n.id AS suggested_node_id, n.name AS suggested_node_name, 1 AS to_review \
               FROM work_orders t \
               LEFT JOIN work_order_statuses s ON s.id = t.status_id \
               LEFT JOIN equipment e ON e.id = t.equipment_id \
               LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id AND {usable} \
              WHERE {without} ORDER BY t.id DESC LIMIT ?"
        ),
        NoSiteKind::Request => format!(
            "SELECT t.id, t.code AS code, t.title AS title, t.status AS status_code, t.created_at AS created_at, \
                    NULL AS window_start, NULL AS window_end, t.equipment_id AS equipment_id, \
                    e.asset_id_code AS equipment_code, e.name AS equipment_name, \
                    n.id AS suggested_node_id, n.name AS suggested_node_name, 1 AS to_review \
               FROM intervention_requests t \
               LEFT JOIN equipment e ON e.id = t.equipment_id \
               LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id AND {usable} \
              WHERE {without} ORDER BY t.id DESC LIMIT ?"
        ),
        NoSiteKind::PlanningWindow => format!(
            "SELECT t.id, NULL AS code, t.window_type AS title, NULL AS status_code, t.created_at AS created_at, \
                    t.start_datetime AS window_start, t.end_datetime AS window_end, t.equipment_id AS equipment_id, \
                    e.asset_id_code AS equipment_code, e.name AS equipment_name, \
                    n.id AS suggested_node_id, n.name AS suggested_node_name, \
                    t.scope_review_required AS to_review \
               FROM planning_windows t \
               LEFT JOIN equipment e ON e.id = t.equipment_id \
               LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id AND {usable} \
              WHERE {without} ORDER BY t.scope_review_required DESC, t.start_datetime DESC, t.id DESC LIMIT ?"
        ),
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [MAX_LISTED.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(NoSiteRecord {
                kind,
                id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                code: r.try_get("", "code").map_err(|e| decode_err("code", e))?,
                title: r.try_get("", "title").map_err(|e| decode_err("title", e))?,
                status_code: r.try_get("", "status_code").map_err(|e| decode_err("status_code", e))?,
                created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
                window_start: r
                    .try_get("", "window_start")
                    .map_err(|e| decode_err("window_start", e))?,
                window_end: r.try_get("", "window_end").map_err(|e| decode_err("window_end", e))?,
                equipment_id: r
                    .try_get("", "equipment_id")
                    .map_err(|e| decode_err("equipment_id", e))?,
                equipment_code: r
                    .try_get("", "equipment_code")
                    .map_err(|e| decode_err("equipment_code", e))?,
                equipment_name: r
                    .try_get("", "equipment_name")
                    .map_err(|e| decode_err("equipment_name", e))?,
                suggested_node_id: r
                    .try_get("", "suggested_node_id")
                    .map_err(|e| decode_err("suggested_node_id", e))?,
                suggested_node_name: r
                    .try_get("", "suggested_node_name")
                    .map_err(|e| decode_err("suggested_node_name", e))?,
                to_review: r
                    .try_get::<i64>("", "to_review")
                    .map_err(|e| decode_err("to_review", e))?
                    != 0,
            })
        })
        .collect()
}

/// The screen's data for one kind.
pub async fn list_records_without_site(db: &DatabaseConnection, kind: NoSiteKind) -> AppResult<RecordsWithoutSite> {
    Ok(RecordsWithoutSite {
        kind,
        counts: count_records_without_site(db).await?,
        records: list_records_of_kind(db, kind).await?,
        listed_limit: MAX_LISTED,
        max_batch: MAX_BATCH,
        reason_min_chars: MIN_REASON_CHARS,
        reason_max_chars: MAX_REASON_CHARS,
        nodes: crate::commands::admin_users::list_assignable_scope_nodes_impl(db).await?,
    })
}

/// Refuses unless `user_id` holds `org.admin` plant-wide through a permanent role, or is an
/// administrator: the rule the other PCs apply when the assignment arrives
/// (`settings::site::holds_tenant_right`). An emergency grant is temporary and never lets its
/// holder administer anything lasting (`rbac::resolver::PERMANENT_ASSIGNMENT_IN_FORCE_SQL`); a site
/// given here is for good, so a holder of the right only by an emergency grant gets a coded
/// refusal. Anyone else (no right, or the right on some sites only) is refused as the command
/// refuses them (`require_permission!` with `PermissionScope::Global`, the check of every
/// plant-wide organisation command): permission denied.
pub async fn require_plant_wide_org_admin(conn: &impl ConnectionTrait, user_id: i32) -> AppResult<()> {
    let scope = crate::auth::scope::access_scope(conn, user_id, ORG_ADMIN).await?;
    if !scope.is_everywhere() {
        return Err(AppError::PermissionDenied(format!("Permission requise : {ORG_ADMIN}")));
    }
    if !crate::settings::site::holds_tenant_right(conn, i64::from(user_id), ORG_ADMIN).await? {
        return Err(refusal(
            "SITE_ASSIGN_PERMANENT_RIGHT_REQUIRED",
            "Giving a site to a record without one needs the organisation administration right for the whole \
             plant through a permanent role: an emergency grant is not enough.",
            &[("permission", ORG_ADMIN.to_string())],
        ));
    }
    Ok(())
}

/// The reason, trimmed, when it is long enough and not too long (counted in Unicode characters).
fn checked_reason(reason: &str) -> AppResult<String> {
    let reason = reason.trim();
    let chars = reason.chars().count();
    if chars < MIN_REASON_CHARS {
        return Err(refusal(
            "SITE_ASSIGN_REASON_REQUIRED",
            format!("Give the reason (at least {MIN_REASON_CHARS} characters) for the site you assign."),
            &[("min", MIN_REASON_CHARS.to_string())],
        ));
    }
    if chars > MAX_REASON_CHARS {
        return Err(refusal(
            "SITE_ASSIGN_REASON_TOO_LONG",
            format!("The reason has at most {MAX_REASON_CHARS} characters."),
            &[("max", MAX_REASON_CHARS.to_string())],
        ));
    }
    Ok(reason.to_string())
}

/// The node given: a live node of the published structure, valid for the kind of record.
async fn checked_node(conn: &impl ConnectionTrait, kind: NoSiteKind, node_id: i64) -> AppResult<(String, String)> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.code AS code, n.name AS name FROM org_nodes n \
              WHERE n.id = ? AND n.deleted_at IS NULL \
                AND n.structure_model_id IN (SELECT id FROM org_structure_models WHERE status = 'active')",
            [node_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(refusal(
            "SITE_ASSIGN_NODE_INVALID",
            "The site chosen is not a node of the published organisation (deleted, or still a draft).",
            &[("org_node_id", node_id.to_string())],
        ));
    };
    if kind == NoSiteKind::Request {
        // The rule of a new request (decision O7): an active node of the published structure.
        crate::org::model_scope::assert_org_node_active(conn, node_id).await?;
    }
    Ok((
        row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        row.try_get("", "name").map_err(|e| decode_err("name", e))?,
    ))
}

/// A record about to be given a site.
struct Held {
    id: i64,
    site: Option<i64>,
    code: Option<String>,
    equipment_id: Option<i64>,
    /// It carries the trace of an earlier assignment (who, when or why).
    assigned_before: bool,
}

async fn held_record(conn: &impl ConnectionTrait, kind: NoSiteKind, id: i64) -> AppResult<Held> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT t.id AS id, t.{site} AS site, {code} AS code, t.equipment_id AS equipment_id, \
                        CASE WHEN t.site_assigned_by_id IS NULL AND t.site_assigned_at IS NULL \
                              AND t.site_assign_reason IS NULL THEN 0 ELSE 1 END AS assigned_before \
                   FROM {table} t WHERE t.id = ?",
                site = kind.site_column(),
                code = kind.code_sql(),
                table = kind.table()
            ),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: kind.table().to_string(),
            id: id.to_string(),
        })?;
    Ok(Held {
        id,
        site: row.try_get("", "site").map_err(|e| decode_err("site", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        assigned_before: row
            .try_get::<i64>("", "assigned_before")
            .map_err(|e| decode_err("assigned_before", e))?
            != 0,
    })
}

fn has_site(site: Option<i64>) -> bool {
    site.is_some_and(|s| s > 0)
}

/// Gives the site `input.org_node_id` to the records of `input.kind` that have none, as `actor_id`
/// (checked here: `org.admin` plant-wide through a permanent role). All or nothing: a record that
/// has a site, that was already given one once, or that does not exist, refuses the whole batch.
/// One audit entry per record.
pub async fn assign_site(
    db: &DatabaseConnection,
    actor_id: i32,
    input: AssignSiteInput,
) -> AppResult<AssignSiteResult> {
    require_plant_wide_org_admin(db, actor_id).await?;
    let reason = checked_reason(&input.reason)?;
    let ids: BTreeSet<i64> = input.record_ids.iter().copied().collect();
    if ids.is_empty() {
        return Err(refusal(
            "SITE_ASSIGN_NO_RECORDS",
            "Choose the records to give a site to.",
            &[],
        ));
    }
    if ids.len() > MAX_BATCH {
        return Err(refusal(
            "SITE_ASSIGN_TOO_MANY",
            format!("At most {MAX_BATCH} records can be given a site at once."),
            &[("max", MAX_BATCH.to_string())],
        ));
    }
    let kind = input.kind;
    let node = input.org_node_id;

    let txn = db.begin().await?;
    // Read again with the records, in the same transaction.
    require_plant_wide_org_admin(&txn, actor_id).await?;
    let (node_code, node_name) = checked_node(&txn, kind, node).await?;
    let mut held = Vec::with_capacity(ids.len());
    for id in &ids {
        held.push(held_record(&txn, kind, *id).await?);
    }
    let with_site: Vec<String> = held
        .iter()
        .filter(|h| has_site(h.site))
        .map(|h| h.code.clone().unwrap_or_else(|| format!("#{}", h.id)))
        .collect();
    if !with_site.is_empty() {
        return Err(refusal(
            "SITE_ASSIGN_ALREADY_HAS_SITE",
            format!(
                "These records already have a site, which cannot be changed here: {}.",
                with_site.join(", ")
            ),
            &[("records", with_site.join(", "))],
        ));
    }
    // A record given a site once keeps the trace of it (who, when, why) for good: the other PCs
    // refuse any received row that rewrites or removes it (`SITE_ASSIGNMENT_TRACE_CHANGED`, see
    // [`inbound_site_assignment_check`]). A planning window a planner set back to plant-wide since
    // still carries it: assigning it again here would write a second trace that every other PC
    // refuses, so it is refused here (the planner gives it its site again in the window dialog).
    let assigned_before: Vec<String> = held
        .iter()
        .filter(|h| h.assigned_before)
        .map(|h| h.code.clone().unwrap_or_else(|| format!("#{}", h.id)))
        .collect();
    if !assigned_before.is_empty() {
        return Err(refusal(
            "SITE_ASSIGN_ALREADY_ASSIGNED",
            format!(
                "These records were already given a site once, which is kept for good; they cannot be given one \
                 again here: {}.",
                assigned_before.join(", ")
            ),
            &[("records", assigned_before.join(", "))],
        ));
    }
    if kind == NoSiteKind::PlanningWindow {
        // The rule of a window: its equipment, when it names one, is installed on its site or below.
        for h in &held {
            crate::planning::windows::validate_window_scope(&txn, Some(node), h.equipment_id).await?;
        }
    }

    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // A window flagged "Site to re-enter" is settled by its site, as when a planner re-enters it.
    let settle_review = if kind == NoSiteKind::PlanningWindow {
        ", scope_review_required = 0"
    } else {
        ""
    };
    let update_sql = format!(
        "UPDATE {table} SET {site} = ?, site_assigned_by_id = ?, site_assigned_at = ?, site_assign_reason = ?, \
                row_version = row_version + 1, updated_at = ?{settle_review} \
          WHERE id = ? AND ({site} IS NULL OR {site} <= 0) \
            AND site_assigned_by_id IS NULL AND site_assigned_at IS NULL AND site_assign_reason IS NULL",
        table = kind.table(),
        site = kind.site_column(),
    );
    for h in &held {
        let updated = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                &update_sql,
                [
                    node.into(),
                    i64::from(actor_id).into(),
                    now.clone().into(),
                    reason.clone().into(),
                    now.clone().into(),
                    h.id.into(),
                ],
            ))
            .await?;
        if updated.rows_affected() != 1 {
            return Err(refusal(
                "SITE_ASSIGN_ALREADY_HAS_SITE",
                "A record was given a site in the meantime: reload and try again.",
                &[("records", h.code.clone().unwrap_or_else(|| format!("#{}", h.id)))],
            ));
        }
        crate::audit::writer::write_audit_event_in(
            &txn,
            crate::audit::writer::AuditEventInput {
                action_code: AUDIT_ACTION.to_string(),
                target_type: Some(kind.audit_target().to_string()),
                target_id: Some(h.id.to_string()),
                actor_id: Some(i64::from(actor_id)),
                auth_context: "step_up".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({
                    "kind": kind,
                    "code": h.code,
                    "site_before": h.site,
                    "site_after": node,
                    "site_after_code": node_code,
                    "site_after_name": node_name,
                    "reason": reason,
                })),
            },
        )
        .await?;
    }
    txn.commit().await?;

    Ok(AssignSiteResult {
        kind,
        org_node_id: node,
        assigned: ids.into_iter().collect(),
        counts: count_records_without_site(db).await?,
    })
}

// ── Arrival by synchronisation ─────────────────────────────────────────────────────────

/// A non-empty text field of a received row; a field left out keeps the local value.
fn received_text(payload: &Map<String, Value>, key: &str, local: Option<&str>) -> Option<String> {
    match payload.get(key) {
        None => local.map(str::to_string),
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(_) => None,
    }
}

fn conflict(code: &'static str, message: String) -> AppResult<Option<InboundVerdict>> {
    Ok(Some(InboundVerdict::Conflict { code, message }))
}

/// The local copy of a received record, as the arrival check reads it.
struct LocalCopy {
    site: Option<i64>,
    site_sync: Option<String>,
    equipment_sync: Option<String>,
    trace: [Option<String>; 3],
    code: Option<String>,
}

async fn local_copy(
    conn: &impl ConnectionTrait,
    kind: NoSiteKind,
    entity_sync_id: &str,
) -> AppResult<Option<LocalCopy>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT t.{site} AS site, n.entity_sync_id AS site_sync, eq.entity_sync_id AS equipment_sync, \
                        u.entity_sync_id AS by_sync, t.site_assigned_at AS assigned_at, \
                        t.site_assign_reason AS reason, {code} AS code \
                   FROM {table} t \
                   LEFT JOIN org_nodes n ON n.id = t.{site} \
                   LEFT JOIN equipment eq ON eq.id = t.equipment_id \
                   LEFT JOIN user_accounts u ON u.id = t.site_assigned_by_id \
                  WHERE t.entity_sync_id = ?",
                site = kind.site_column(),
                code = kind.code_sql(),
                table = kind.table()
            ),
            [entity_sync_id.into()],
        ))
        .await?;
    let Some(r) = row else { return Ok(None) };
    let text = |col: &str| -> AppResult<Option<String>> {
        Ok(r.try_get::<Option<String>>("", col)
            .map_err(|e| decode_err(col, e))?
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()))
    };
    Ok(Some(LocalCopy {
        site: r.try_get("", "site").map_err(|e| decode_err("site", e))?,
        site_sync: text("site_sync")?,
        equipment_sync: text("equipment_sync")?,
        trace: [text("by_sync")?, text("assigned_at")?, text("reason")?],
        code: text("code")?,
    }))
}

/// Re-check on arrival of a received work order, request or planning window (`sync::apply_hooks`):
/// - the site-assignment trace recorded here (who, when, why) is never removed nor rewritten;
/// - a new assignment on the row is accepted only when it is complete (who, when, not in the future,
///   a reason of [`MIN_REASON_CHARS`] to [`MAX_REASON_CHARS`] characters, a site), its author holds
///   `org.admin` plant-wide here through a permanent role, the site is a live node here, and the
///   record still has no site here; a person, their rights or a node not applied here yet makes the
///   row wait;
/// - a work order or request that has no site here and arrives with one but without an assignment
///   is a conflict (no edit gives such a record a site), except a work order whose equipment changes
///   with it (a work order moves with its equipment) and a planning window (a planner re-enters its
///   site in the window dialog).
///
/// Anything else is left to the other checks, as before (a record new here, a site that changes on a
/// record that has one).
pub(crate) async fn inbound_site_assignment_check(
    conn: &impl ConnectionTrait,
    kind: NoSiteKind,
    entity_sync_id: &str,
    received: &Map<String, Value>,
) -> AppResult<Option<InboundVerdict>> {
    let Some(local) = local_copy(conn, kind, entity_sync_id).await? else {
        return Ok(None);
    };
    let what = format!(
        "{} {}",
        kind.audit_target().replace('_', " "),
        local.code.as_deref().unwrap_or(entity_sync_id)
    );
    let received_trace = [
        received_text(received, "site_assigned_by_sync_id", local.trace[0].as_deref()),
        received_text(received, "site_assigned_at", local.trace[1].as_deref()),
        received_text(received, "site_assign_reason", local.trace[2].as_deref()),
    ];
    let received_site = received_text(received, kind.site_payload_key(), local.site_sync.as_deref());
    let local_has_site = has_site(local.site);

    if local.trace.iter().any(Option::is_some) {
        if received_trace != local.trace {
            return conflict(
                "SITE_ASSIGNMENT_TRACE_CHANGED",
                format!("{what}: the site assignment recorded here (who, when, why) cannot be removed or rewritten"),
            );
        }
        return Ok(None);
    }

    if received_trace.iter().all(Option::is_none) {
        if local_has_site || received_site.is_none() || kind == NoSiteKind::PlanningWindow {
            return Ok(None);
        }
        if kind == NoSiteKind::WorkOrder
            && received_text(received, "equipment_sync_id", local.equipment_sync.as_deref()) != local.equipment_sync
        {
            // The work order moves with its new equipment (`wo::queries::update_work_order`).
            return Ok(None);
        }
        return conflict(
            "SITE_SET_WITHOUT_ASSIGNMENT",
            format!(
                "{what} has no site here and arrives with one without a site assignment (only an organisation \
                 administrator gives a site to a record without one, with a reason)"
            ),
        );
    }

    // A new assignment.
    let [Some(by_sync), Some(at), Some(reason)] = &received_trace else {
        return conflict(
            "SITE_ASSIGNMENT_INCOMPLETE",
            format!("{what}: its site assignment does not say who gave the site, when and why"),
        );
    };
    if local_has_site {
        return conflict(
            "SITE_ASSIGNMENT_SITE_NOT_EMPTY",
            format!("{what} already has a site here: a site is given only to a record without one"),
        );
    }
    let Some(node_sync) = received_site else {
        return conflict(
            "SITE_ASSIGNMENT_INCOMPLETE",
            format!("{what}: its site assignment names no site"),
        );
    };
    if reason.chars().count() < MIN_REASON_CHARS {
        return conflict(
            "SITE_ASSIGNMENT_INCOMPLETE",
            format!("{what}: its site assignment has no reason of at least {MIN_REASON_CHARS} characters"),
        );
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        return conflict(
            "SITE_ASSIGNMENT_INCOMPLETE",
            format!("{what}: the reason of its site assignment is longer than {MAX_REASON_CHARS} characters"),
        );
    }
    match instant(at) {
        None => {
            return conflict(
                "SITE_ASSIGNMENT_INCOMPLETE",
                format!("{what}: the time of its site assignment is unreadable ({at})"),
            )
        }
        Some(t) if t > crate::clock::now() + Duration::minutes(MAX_CLOCK_SKEW_MINUTES) => {
            return conflict(
                "SITE_ASSIGNMENT_INCOMPLETE",
                format!("{what}: its site assignment is dated in the future ({at})"),
            )
        }
        Some(_) => {}
    }
    let node = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT deleted_at FROM org_nodes WHERE entity_sync_id = ?",
            [node_sync.clone().into()],
        ))
        .await?;
    let Some(node) = node else {
        return Ok(Some(InboundVerdict::Defer(format!(
            "{what}: its site {node_sync} is not applied here yet"
        ))));
    };
    if node
        .try_get::<Option<String>>("", "deleted_at")
        .map_err(|e| decode_err("deleted_at", e))?
        .is_some()
    {
        return conflict(
            "SITE_ASSIGNMENT_NODE_INVALID",
            format!("{what}: the site given ({node_sync}) is deleted here"),
        );
    }
    let author = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
            [by_sync.clone().into()],
        ))
        .await?;
    let Some(author) = author else {
        return Ok(Some(InboundVerdict::Defer(format!(
            "{what}: the person who gave its site ({by_sync}) is not applied here yet"
        ))));
    };
    let author_id: i64 = author.try_get("", "id").map_err(|e| decode_err("id", e))?;
    if !crate::settings::site::holds_tenant_right(conn, author_id, ORG_ADMIN).await? {
        // The right is held before it is used, so its rows (the author's plant-wide role, the
        // role's permission) come before or with this one: while some still wait here, wait too.
        if crate::permit::sync_check::rights_may_still_arrive(conn, by_sync, ORG_ADMIN).await? {
            return Ok(Some(InboundVerdict::Defer(format!(
                "{what}: the rights of the person who gave its site ({by_sync}) are still arriving"
            ))));
        }
        return conflict(
            "SITE_ASSIGNMENT_NOT_ALLOWED",
            format!(
                "{what}: the person who gave its site does not hold the organisation administration right for \
                 the whole plant here"
            ),
        );
    }
    Ok(None)
}
