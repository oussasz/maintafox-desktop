//! Settlement of a closed work order (OT) into the budget — SAP PM "order settlement".
//!
//! Called from `wo::closeout::close_wo`, `reopen_wo` and `wo::queries::cancel_work_order`, inside
//! their transaction:
//! - close: the OT's cost breakdown (`wo_costing`) is written back on `work_orders` (labour, parts,
//!   services + other, total) so every reader (asset cost, SMRP cost/RAV, RAMS) sees the same
//!   figures, then posted once to `budget_actuals` against the OT's cost centre, the approved
//!   budget version of the year and the first open month from the close date. One `posted` row per
//!   bucket (labour regular / overtime, parts, services, other), `source_type = 'wo_settlement'`.
//! - When the cost centre, an approved budget version, an open month or the tenant currency is
//!   missing, nothing is booked: the settlement is recorded `pending` with its reason, shown on
//!   the OT and in the Finance worklist, and Finance settles it once the cause is fixed.
//! - Idempotent and sync-safe: while an OT has a `posted` or `pending` settlement, closing again
//!   does nothing; the settlement and every actual carry a deterministic sync identity (the OT sync id
//!   and close cycle; the actual source id), inserted `OR IGNORE`, so two devices settling the same OT
//!   produce the same rows and the budget counts them once.
//! - cancel: an OT cancelled after consuming labour, parts or services is settled like a closed one.
//! - late costs: when a settled OT's cost changes (receipt, issue, return or labour recorded after
//!   the close, or synced after it), the difference with what is booked is posted (`wo_late_cost`)
//!   by the inventory hooks, the sync apply hook and Finance's catch-up.
//! - reopen: the posted actuals (settlement and late costs) are reversed (negated rows, status
//!   `reversed`) and the settlement becomes `reversed`; the next close settles the new cycle.

use std::collections::HashMap;

use chrono::{Datelike, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::finance::wo_costing::{
    compute_wo_cost_breakdown, decode, resolve_wo_cost_center, round_money, WoCostBreakdown, BUCKET_LABOR,
    BUCKET_OTHER, BUCKET_PARTS, BUCKET_SERVICES,
};
use crate::finance::{fiscal, periods};

pub const SETTLEMENT_SOURCE_TYPE: &str = "wo_settlement";

pub const PENDING_NO_CURRENCY: &str = "no_currency";
pub const PENDING_NO_COST_CENTER: &str = "no_cost_center";
pub const PENDING_NO_BUDGET_VERSION: &str = "no_budget_version";
pub const PENDING_CURRENCY_MISMATCH: &str = "currency_mismatch";
pub const PENDING_PERIOD_CLOSED: &str = "period_closed";
pub const PENDING_NO_FISCAL_CALENDAR: &str = "no_fiscal_calendar";
pub const PENDING_UNPRICED_LABOR: &str = "unpriced_labor";
/// Cost documents reaching an OT after its settlement (late receipts, issues, returns).
pub const SETTLEMENT_LATE_SOURCE_TYPE: &str = "wo_late_cost";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoCostSettlement {
    pub id: i64,
    pub entity_sync_id: String,
    pub work_order_id: i64,
    pub work_order_code: String,
    pub work_order_title: Option<String>,
    pub equipment_id: Option<i64>,
    pub equipment_code: Option<String>,
    pub settlement_cycle: i64,
    /// `posted` | `pending` | `reversed`
    pub status: String,
    pub pending_reason: Option<String>,
    pub cost_center_id: Option<i64>,
    pub cost_center_code: Option<String>,
    pub cost_center_name: Option<String>,
    pub budget_version_id: Option<i64>,
    pub budget_version_title: Option<String>,
    pub fiscal_year: Option<i64>,
    pub period_month: Option<i64>,
    pub currency_code: String,
    pub labor_amount: f64,
    pub labor_overtime_amount: f64,
    pub parts_amount: f64,
    pub services_amount: f64,
    pub other_amount: f64,
    pub total_amount: f64,
    pub overtime_captured: bool,
    pub unpriced_labor_hours: f64,
    pub unvalued_parts_qty: f64,
    pub closed_at: Option<String>,
    pub posted_at: Option<String>,
    pub reversed_at: Option<String>,
    pub reversal_reason: Option<String>,
    pub row_version: i64,
    /// More than one posted / pending settlement exists for the OT (for instance two devices closed
    /// it offline in different close cycles): flagged in the worklist for Finance to review.
    pub duplicate_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WoSettlementFilter {
    /// `posted` | `pending` | `reversed`; None = all.
    pub status: Option<String>,
    pub work_order_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub fiscal_year: Option<i64>,
    pub limit: Option<i64>,
}

const SETTLEMENT_SELECT: &str = "SELECT s.id, s.entity_sync_id, s.work_order_id, wo.code AS work_order_code,
        wo.title AS work_order_title, wo.equipment_id, e.asset_id_code AS equipment_code,
        s.settlement_cycle, s.status, s.pending_reason, s.cost_center_id, cc.code AS cost_center_code,
        cc.name AS cost_center_name, s.budget_version_id, bv.title AS budget_version_title,
        s.fiscal_year, s.period_month, s.currency_code, s.labor_amount, s.labor_overtime_amount,
        s.parts_amount, s.services_amount, s.other_amount, s.total_amount, s.overtime_captured,
        s.unpriced_labor_hours, s.unvalued_parts_qty, s.closed_at, s.posted_at, s.reversed_at,
        s.reversal_reason, s.row_version,
        (SELECT COUNT(*) FROM wo_cost_settlements s2
          WHERE s2.work_order_id = s.work_order_id AND s2.status IN ('posted', 'pending')) AS active_count
     FROM wo_cost_settlements s
     JOIN work_orders wo ON wo.id = s.work_order_id
     LEFT JOIN equipment e ON e.id = wo.equipment_id
     LEFT JOIN cost_centers cc ON cc.id = s.cost_center_id
     LEFT JOIN budget_versions bv ON bv.id = s.budget_version_id";

fn map_settlement(r: &QueryResult) -> AppResult<WoCostSettlement> {
    Ok(WoCostSettlement {
        id: decode(r, "id")?,
        entity_sync_id: decode(r, "entity_sync_id")?,
        work_order_id: decode(r, "work_order_id")?,
        work_order_code: decode(r, "work_order_code")?,
        work_order_title: decode(r, "work_order_title")?,
        equipment_id: decode(r, "equipment_id")?,
        equipment_code: decode(r, "equipment_code")?,
        settlement_cycle: decode(r, "settlement_cycle")?,
        status: decode(r, "status")?,
        pending_reason: decode(r, "pending_reason")?,
        cost_center_id: decode(r, "cost_center_id")?,
        cost_center_code: decode(r, "cost_center_code")?,
        cost_center_name: decode(r, "cost_center_name")?,
        budget_version_id: decode(r, "budget_version_id")?,
        budget_version_title: decode(r, "budget_version_title")?,
        fiscal_year: decode(r, "fiscal_year")?,
        period_month: decode(r, "period_month")?,
        currency_code: decode(r, "currency_code")?,
        labor_amount: decode(r, "labor_amount")?,
        labor_overtime_amount: decode(r, "labor_overtime_amount")?,
        parts_amount: decode(r, "parts_amount")?,
        services_amount: decode(r, "services_amount")?,
        other_amount: decode(r, "other_amount")?,
        total_amount: decode(r, "total_amount")?,
        overtime_captured: decode::<i64>(r, "overtime_captured")? != 0,
        unpriced_labor_hours: decode(r, "unpriced_labor_hours")?,
        unvalued_parts_qty: decode(r, "unvalued_parts_qty")?,
        closed_at: decode(r, "closed_at")?,
        posted_at: decode(r, "posted_at")?,
        reversed_at: decode(r, "reversed_at")?,
        reversal_reason: decode(r, "reversal_reason")?,
        row_version: decode(r, "row_version")?,
        duplicate_active: decode::<i64>(r, "active_count")? > 1,
    })
}

pub async fn get_settlement<C: ConnectionTrait>(db: &C, id: i64) -> AppResult<WoCostSettlement> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SETTLEMENT_SELECT} WHERE s.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoCostSettlement".into(),
            id: id.to_string(),
        })?;
    map_settlement(&row)
}

pub async fn list_wo_settlements<C: ConnectionTrait>(
    db: &C,
    filter: WoSettlementFilter,
) -> AppResult<Vec<WoCostSettlement>> {
    let mut sql = format!("{SETTLEMENT_SELECT} WHERE 1 = 1");
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(status) = filter.status.filter(|s| !s.trim().is_empty()) {
        sql.push_str(" AND s.status = ?");
        values.push(status.into());
    }
    if let Some(wo) = filter.work_order_id {
        sql.push_str(" AND s.work_order_id = ?");
        values.push(wo.into());
    }
    if let Some(cc) = filter.cost_center_id {
        sql.push_str(" AND s.cost_center_id = ?");
        values.push(cc.into());
    }
    if let Some(fy) = filter.fiscal_year {
        sql.push_str(
            " AND (s.fiscal_year = ? OR (s.fiscal_year IS NULL AND CAST(strftime('%Y', s.closed_at) AS INTEGER) = ?))",
        );
        values.push(fy.into());
        values.push(fy.into());
    }
    sql.push_str(" ORDER BY COALESCE(s.closed_at, s.created_at) DESC, s.id DESC LIMIT ?");
    values.push(filter.limit.unwrap_or(500).clamp(1, 5000).into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_settlement).collect()
}

/// The OT's `posted` or `pending` settlement, if any (at most one by construction).
pub async fn active_settlement<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Option<WoCostSettlement>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{SETTLEMENT_SELECT} WHERE s.work_order_id = ? AND s.status IN ('posted', 'pending')
                 ORDER BY s.id DESC LIMIT 1"
            ),
            [wo_id.into()],
        ))
        .await?;
    row.as_ref().map(map_settlement).transpose()
}

/// Latest settlement of the OT whatever its status (for the OT cost panel).
pub async fn latest_settlement<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Option<WoCostSettlement>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SETTLEMENT_SELECT} WHERE s.work_order_id = ? ORDER BY s.id DESC LIMIT 1"),
            [wo_id.into()],
        ))
        .await?;
    row.as_ref().map(map_settlement).transpose()
}

/// Approved budget version a year's actuals post to: frozen or approved, not a what-if; the
/// approved scenario first, then original, then reforecast; latest version number first.
pub async fn posting_budget_version<C: ConnectionTrait>(db: &C, fiscal_year: i64) -> AppResult<Option<(i64, String)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, currency_code FROM budget_versions
             WHERE fiscal_year = ? AND status IN ('approved', 'frozen') AND scenario_type <> 'what_if'
             ORDER BY CASE scenario_type WHEN 'approved' THEN 0 WHEN 'original' THEN 1
                                         WHEN 'reforecast' THEN 2 ELSE 3 END,
                      version_no DESC, id DESC
             LIMIT 1",
            [fiscal_year.into()],
        ))
        .await?;
    row.map(|r| Ok((decode(&r, "id")?, decode(&r, "currency_code")?)))
        .transpose()
}

/// Where a settlement lands, or why it cannot be posted yet.
struct Placement {
    cost_center_id: Option<i64>,
    budget_version_id: Option<i64>,
    fiscal_year: i64,
    period_month: Option<i64>,
    pending_reason: Option<&'static str>,
}

async fn place<C: ConnectionTrait>(db: &C, wo_id: i64, currency: &str, year: i64, month: i64) -> AppResult<Placement> {
    let mut placement = Placement {
        cost_center_id: resolve_wo_cost_center(db, wo_id).await?.map(|c| c.cost_center_id),
        budget_version_id: None,
        fiscal_year: year,
        period_month: None,
        pending_reason: None,
    };
    if placement.cost_center_id.is_none() {
        placement.pending_reason = Some(PENDING_NO_COST_CENTER);
        return Ok(placement);
    }
    let Some((version_id, version_currency)) = posting_budget_version(db, year).await? else {
        placement.pending_reason = Some(PENDING_NO_BUDGET_VERSION);
        return Ok(placement);
    };
    placement.budget_version_id = Some(version_id);
    if version_currency != currency {
        placement.pending_reason = Some(PENDING_CURRENCY_MISMATCH);
        return Ok(placement);
    }
    match periods::first_open_month_from(db, year, month).await? {
        Some(m) => placement.period_month = Some(m),
        None => placement.pending_reason = Some(PENDING_PERIOD_CLOSED),
    }
    Ok(placement)
}

fn now_text() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Cost columns on `work_orders` follow the settlement figures.
async fn write_back_wo_costs<C: ConnectionTrait>(
    db: &C,
    breakdown: &WoCostBreakdown,
    bump_version: bool,
) -> AppResult<()> {
    let sql = if bump_version {
        "UPDATE work_orders SET labor_cost = ?, parts_cost = ?, service_cost = ?, total_cost = ?,
                row_version = row_version + 1, updated_at = ?
         WHERE id = ?"
    } else {
        "UPDATE work_orders SET labor_cost = ?, parts_cost = ?, service_cost = ?, total_cost = ?, updated_at = ?
         WHERE id = ?"
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [
            breakdown.labor.actual.into(),
            breakdown.parts.actual.into(),
            round_money(breakdown.services.actual + breakdown.other.actual).into(),
            breakdown.total.actual.into(),
            now_text().into(),
            breakdown.work_order_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Deterministic sync identity of a budget actual: two devices posting the same thing produce the
/// same row, so duplicates collapse on arrival (UNIQUE `entity_sync_id`) instead of double-counting.
fn actual_sync_id(source_id: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("budget_actuals", source_id)
}

/// One budget actual row (INSERT OR IGNORE on its deterministic identity).
#[allow(clippy::too_many_arguments)]
async fn insert_actual<C: ConnectionTrait>(
    db: &C,
    source_type: &str,
    source_id: &str,
    version_id: i64,
    cost_center_id: i64,
    period: i64,
    bucket: &str,
    lane: Option<&str>,
    amount: f64,
    currency: &str,
    work_order_id: i64,
    equipment_id: Option<i64>,
    actor_id: Option<i64>,
) -> AppResult<()> {
    let now = now_text();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO budget_actuals
            (entity_sync_id, budget_version_id, cost_center_id, period_month, budget_bucket,
             amount_source, source_currency, amount_base, base_currency, source_type, source_id,
             work_order_id, equipment_id, posting_status, posted_at, posted_by_id, rate_card_lane,
             event_at, row_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'posted', ?, ?, ?, ?, 1, ?, ?)",
        [
            actual_sync_id(source_id).into(),
            version_id.into(),
            cost_center_id.into(),
            period.into(),
            bucket.into(),
            amount.into(),
            currency.into(),
            amount.into(),
            currency.into(),
            source_type.into(),
            source_id.into(),
            work_order_id.into(),
            equipment_id.into(),
            now.clone().into(),
            actor_id.into(),
            lane.map(str::to_string).into(),
            now.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Books the actual rows of a posted settlement.
async fn insert_settlement_actuals<C: ConnectionTrait>(
    db: &C,
    settlement_sync_id: &str,
    breakdown: &WoCostBreakdown,
    placement: &Placement,
    currency: &str,
    actor_id: i64,
) -> AppResult<()> {
    let (Some(version_id), Some(cost_center_id), Some(period)) = (
        placement.budget_version_id,
        placement.cost_center_id,
        placement.period_month,
    ) else {
        return Err(AppError::Internal(anyhow::anyhow!("posting without placement")));
    };
    for (bucket, lane, amount) in settlement_rows(breakdown) {
        if amount.abs() < 0.005 {
            continue;
        }
        let source_id = match lane {
            Some(lane) => format!("{settlement_sync_id}:{bucket}:{lane}"),
            None => format!("{settlement_sync_id}:{bucket}"),
        };
        insert_actual(
            db,
            SETTLEMENT_SOURCE_TYPE,
            &source_id,
            version_id,
            cost_center_id,
            period,
            bucket,
            lane,
            amount,
            currency,
            breakdown.work_order_id,
            breakdown.equipment_id,
            Some(actor_id),
        )
        .await?;
    }
    Ok(())
}

/// Labour rates frozen by a posted settlement (see `wo_costing::LaborRateSnapshot`); None while
/// the settlement is pending.
fn labor_snapshot_json(breakdown: &WoCostBreakdown, posted: bool) -> AppResult<Option<String>> {
    if !posted {
        return Ok(None);
    }
    serde_json::to_string(&crate::finance::wo_costing::labor_rate_snapshot(&breakdown.labor_lines))
        .map(Some)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("labour snapshot: {e}")))
}

/// Close cycle, posting date (close, else cancellation) and sync identity of the OT.
async fn settlement_context<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<(i64, Option<String>, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(reopen_count, 0) AS reopen_count, COALESCE(closed_at, cancelled_at) AS closed_at,
                    COALESCE(entity_sync_id, 'wo-' || id) AS wo_sync_id
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    Ok((
        decode(&row, "reopen_count")?,
        decode(&row, "closed_at")?,
        decode(&row, "wo_sync_id")?,
    ))
}

/// Fiscal year and period of a timestamp in the tenant's local calendar (else today).
fn posting_period(at: Option<&str>, fiscal_start: i64) -> (i64, i64) {
    let date = at
        .and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok())
        .map(|d| d.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    fiscal::fiscal_period(i64::from(date.year()), i64::from(date.month()), fiscal_start)
}

/// Settles a just-closed OT (close-out hook). Never fails for a business reason: anything that
/// prevents posting yields a `pending` settlement. Returns the OT's active settlement.
pub async fn settle_closed_work_order<C: ConnectionTrait>(
    db: &C,
    wo_id: i64,
    actor_id: i64,
) -> AppResult<WoCostSettlement> {
    settle_work_order(db, wo_id, actor_id, false)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("settlement row missing")))
}

/// Cancel hook: a cancelled OT that consumed labour, parts or services is charged like a closed
/// one; an OT cancelled before any cost is not settled. Returns the settlement when one exists.
pub async fn settle_cancelled_work_order<C: ConnectionTrait>(
    db: &C,
    wo_id: i64,
    actor_id: i64,
) -> AppResult<Option<WoCostSettlement>> {
    settle_work_order(db, wo_id, actor_id, true).await
}

async fn settle_work_order<C: ConnectionTrait>(
    db: &C,
    wo_id: i64,
    actor_id: i64,
    only_with_cost: bool,
) -> AppResult<Option<WoCostSettlement>> {
    if let Some(existing) = active_settlement(db, wo_id).await? {
        return Ok(Some(existing));
    }
    let (cycle, closed_at, wo_sync_id) = settlement_context(db, wo_id).await?;
    let closed_at = closed_at.unwrap_or_else(now_text);
    let now = now_text();
    let sync_id = crate::sync::identity::seeded_entity_sync_id("wo_cost_settlements", &format!("{wo_sync_id}:{cycle}"));

    // Without the tenant currency or fiscal calendar nothing can be dated or valued: pending.
    let currency = crate::inventory::fx::tenant_currency(db).await.ok();
    let fiscal_start = fiscal::fiscal_start_month(db).await.ok();
    let (currency, fiscal_start) = match (currency, fiscal_start) {
        (Some(currency), Some(fiscal_start)) => (currency, fiscal_start),
        (_, missing_calendar) => {
            if only_with_cost {
                return Ok(None);
            }
            let reason = if missing_calendar.is_none() {
                PENDING_NO_FISCAL_CALENDAR
            } else {
                PENDING_NO_CURRENCY
            };
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO wo_cost_settlements
                    (entity_sync_id, work_order_id, settlement_cycle, status, pending_reason, fiscal_year,
                     currency_code, closed_at, row_version, created_at, updated_at)
                 VALUES (?, ?, ?, 'pending', ?, ?, '', ?, 1, ?, ?)",
                [
                    sync_id.into(),
                    wo_id.into(),
                    cycle.into(),
                    reason.into(),
                    i64::from(chrono::Local::now().year()).into(),
                    closed_at.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
            return active_settlement(db, wo_id).await;
        }
    };

    let breakdown = compute_wo_cost_breakdown(db, wo_id).await?;
    if only_with_cost && breakdown.total.actual.abs() < 0.005 {
        return Ok(None);
    }
    write_back_wo_costs(db, &breakdown, false).await?;
    let (year, month) = posting_period(Some(&closed_at), fiscal_start);
    let mut placement = place(db, wo_id, &currency, year, month).await?;
    if placement.pending_reason.is_none() && breakdown.unpriced_labor_hours > 0.0 {
        placement.pending_reason = Some(PENDING_UNPRICED_LABOR);
    }
    let posted = placement.pending_reason.is_none();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO wo_cost_settlements
            (entity_sync_id, work_order_id, settlement_cycle, status, pending_reason, cost_center_id,
             budget_version_id, fiscal_year, period_month, currency_code, labor_amount, labor_overtime_amount,
             parts_amount, services_amount, other_amount, total_amount, overtime_captured, unpriced_labor_hours,
             unvalued_parts_qty, closed_at, posted_at, posted_by_id, labor_snapshot_json, row_version, created_at,
             updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            sync_id.clone().into(),
            wo_id.into(),
            cycle.into(),
            (if posted { "posted" } else { "pending" }).into(),
            placement.pending_reason.map(str::to_string).into(),
            placement.cost_center_id.into(),
            placement.budget_version_id.into(),
            placement.fiscal_year.into(),
            placement.period_month.into(),
            currency.clone().into(),
            breakdown.labor.actual.into(),
            breakdown.labor_overtime_amount.into(),
            breakdown.parts.actual.into(),
            breakdown.services.actual.into(),
            breakdown.other.actual.into(),
            breakdown.total.actual.into(),
            i64::from(breakdown.overtime_captured).into(),
            breakdown.unpriced_labor_hours.into(),
            breakdown.unvalued_parts_qty.into(),
            closed_at.into(),
            posted.then(|| now.clone()).into(),
            posted.then_some(actor_id).into(),
            labor_snapshot_json(&breakdown, posted)?.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    if posted {
        insert_settlement_actuals(db, &sync_id, &breakdown, &placement, &currency, actor_id).await?;
    }
    active_settlement(db, wo_id).await
}

/// Finance retries a pending settlement once its cause is fixed (cost centre set on the OT or
/// asset, budget version approved, period reopened, currency configured, rate card created).
/// Figures are recomputed. Returns the settlement, still pending (with the current reason) or
/// posted.
pub async fn retry_pending_settlement<C: ConnectionTrait>(
    db: &C,
    settlement_id: i64,
    actor_id: i64,
) -> AppResult<WoCostSettlement> {
    let current = get_settlement(db, settlement_id).await?;
    if current.status != "pending" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_SETTLEMENT_NOT_PENDING: only a pending settlement can be retried".to_string(),
        ]));
    }
    let currency = crate::inventory::fx::tenant_currency(db).await?;
    let fiscal_start = fiscal::fiscal_start_month(db).await?;
    let (year, month) = posting_period(current.closed_at.as_deref(), fiscal_start);
    let breakdown = compute_wo_cost_breakdown(db, current.work_order_id).await?;
    write_back_wo_costs(db, &breakdown, true).await?;
    let mut placement = place(db, current.work_order_id, &currency, year, month).await?;
    if placement.pending_reason.is_none() && breakdown.unpriced_labor_hours > 0.0 {
        placement.pending_reason = Some(PENDING_UNPRICED_LABOR);
    }
    let posted = placement.pending_reason.is_none();
    let now = now_text();
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE wo_cost_settlements SET status = ?, pending_reason = ?, cost_center_id = ?, budget_version_id = ?,
                    fiscal_year = ?, period_month = ?, currency_code = ?, labor_amount = ?, labor_overtime_amount = ?,
                    parts_amount = ?, services_amount = ?, other_amount = ?, total_amount = ?, overtime_captured = ?,
                    unpriced_labor_hours = ?, unvalued_parts_qty = ?, posted_at = ?, posted_by_id = ?,
                    labor_snapshot_json = ?, row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND status = 'pending' AND row_version = ?",
            [
                (if posted { "posted" } else { "pending" }).into(),
                placement.pending_reason.map(str::to_string).into(),
                placement.cost_center_id.into(),
                placement.budget_version_id.into(),
                placement.fiscal_year.into(),
                placement.period_month.into(),
                currency.clone().into(),
                breakdown.labor.actual.into(),
                breakdown.labor_overtime_amount.into(),
                breakdown.parts.actual.into(),
                breakdown.services.actual.into(),
                breakdown.other.actual.into(),
                breakdown.total.actual.into(),
                i64::from(breakdown.overtime_captured).into(),
                breakdown.unpriced_labor_hours.into(),
                breakdown.unvalued_parts_qty.into(),
                posted.then(|| now.clone()).into(),
                posted.then_some(actor_id).into(),
                labor_snapshot_json(&breakdown, posted)?.into(),
                now.into(),
                settlement_id.into(),
                current.row_version.into(),
            ],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_SETTLEMENT_CHANGED: the settlement was changed meanwhile, reload it".to_string(),
        ]));
    }
    if posted {
        insert_settlement_actuals(db, &current.entity_sync_id, &breakdown, &placement, &currency, actor_id).await?;
    }
    get_settlement(db, settlement_id).await
}

/// What a settlement's actuals add up to, by bucket and lane: labour regular / overtime, parts,
/// services, other.
fn settlement_rows(breakdown: &WoCostBreakdown) -> [(&'static str, Option<&'static str>, f64); 5] {
    [
        (
            BUCKET_LABOR,
            Some("regular"),
            round_money(breakdown.labor.actual - breakdown.labor_overtime_amount),
        ),
        (BUCKET_LABOR, Some("overtime"), breakdown.labor_overtime_amount),
        (BUCKET_PARTS, None, breakdown.parts.actual),
        (BUCKET_SERVICES, None, breakdown.services.actual),
        (BUCKET_OTHER, None, breakdown.other.actual),
    ]
}

/// Late costs of a settled OT: whatever changed its cost after the settlement was posted — a
/// receipt, issue or return recorded later, one created offline before the close and synced after
/// it, a labour entry synced late. The OT's cost is recomputed and compared, bucket by bucket,
/// with everything already booked on its settlement (original rows and earlier deltas); the
/// difference is booked today. No document date is trusted.
///
/// A delta's identity derives from the settlement, the bucket, the delta's rank and its amount:
/// devices computing the same delta from the same data book one row, and devices that saw
/// different documents converge once their rows are exchanged (each recomputes against what it
/// holds). A delta that cannot be placed yet (no approved version in the currency, or no open month
/// this fiscal year) is left for the next run. Returns the amount booked.
pub async fn post_late_costs<C: ConnectionTrait>(db: &C, wo_id: i64, actor_id: Option<i64>) -> AppResult<f64> {
    let Some(active) = active_settlement(db, wo_id).await? else {
        return Ok(0.0);
    };
    let Some(cost_center_id) = active.cost_center_id.filter(|_| active.status == "posted") else {
        return Ok(0.0);
    };
    let currency = crate::inventory::fx::tenant_currency(db).await?;
    let fiscal_start = fiscal::fiscal_start_month(db).await?;
    let breakdown = compute_wo_cost_breakdown(db, wo_id).await?;
    let prefix = format!("{}:", active.entity_sync_id);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT budget_bucket, rate_card_lane, COALESCE(SUM(amount_base), 0.0) AS amount,
                    SUM(CASE WHEN source_type = ? THEN 1 ELSE 0 END) AS deltas
             FROM budget_actuals
             WHERE source_type IN (?, ?) AND work_order_id = ? AND posting_status = 'posted'
               AND substr(source_id, 1, ?) = ?
             GROUP BY budget_bucket, rate_card_lane",
            [
                SETTLEMENT_LATE_SOURCE_TYPE.into(),
                SETTLEMENT_SOURCE_TYPE.into(),
                SETTLEMENT_LATE_SOURCE_TYPE.into(),
                wo_id.into(),
                (prefix.chars().count() as i64).into(),
                prefix.clone().into(),
            ],
        ))
        .await?;
    let mut booked: HashMap<(String, Option<String>), (f64, i64)> = HashMap::new();
    for r in rows {
        booked.insert(
            (decode(&r, "budget_bucket")?, decode(&r, "rate_card_lane")?),
            (decode(&r, "amount")?, decode(&r, "deltas")?),
        );
    }

    let mut placement: Option<(i64, i64)> = None;
    let mut total = 0.0;
    for (bucket, lane, amount) in settlement_rows(&breakdown) {
        let (already, rank) = booked
            .get(&(bucket.to_string(), lane.map(str::to_string)))
            .copied()
            .unwrap_or((0.0, 0));
        let delta = round_money(amount - already);
        if delta.abs() < 0.005 {
            continue;
        }
        let (version_id, period) = match placement {
            Some(p) => p,
            None => {
                let (year, month) = posting_period(None, fiscal_start);
                let Some((version_id, version_currency)) = posting_budget_version(db, year).await? else {
                    return Ok(0.0);
                };
                if version_currency != currency {
                    return Ok(0.0);
                }
                let Some(period) = periods::first_open_month_from(db, year, month).await? else {
                    return Ok(0.0);
                };
                placement = Some((version_id, period));
                (version_id, period)
            }
        };
        let lane_part = lane.map(|l| format!(":{l}")).unwrap_or_default();
        let cents = (delta * 100.0).round() as i64;
        let source_id = format!("{prefix}late:{bucket}{lane_part}:{}:{cents}", rank + 1);
        insert_actual(
            db,
            SETTLEMENT_LATE_SOURCE_TYPE,
            &source_id,
            version_id,
            cost_center_id,
            period,
            bucket,
            lane,
            delta,
            &currency,
            wo_id,
            breakdown.equipment_id,
            actor_id,
        )
        .await?;
        total += delta;
    }
    if total.abs() >= 0.005 {
        write_back_wo_costs(db, &breakdown, true).await?;
    }
    Ok(round_money(total))
}

/// Sync hook (in the apply transaction): an inbound movement, receipt line or labour entry that
/// belongs to a settled OT books the OT's late costs. Best effort, like the inventory hooks: a
/// failure is logged and Finance's catch-up books what is left.
pub async fn post_late_costs_for_synced_row<C: ConnectionTrait>(conn: &C, entity_type: &str, row_id: i64) {
    let sql = match entity_type {
        "inventory_transactions" => format!(
            "SELECT CASE WHEN t.source_type IN ({types}) THEN t.source_id
                         ELSE (SELECT r.source_id FROM stock_reservations r
                                WHERE r.id = t.reservation_id AND r.source_type IN ({types})) END AS wo_id
             FROM inventory_transactions t WHERE t.id = ? AND t.movement_type IN ('ISSUE', 'RETURN')",
            types = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL
        ),
        "goods_receipt_lines" => "SELECT pol.demand_source_id AS wo_id
             FROM goods_receipt_lines grl JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
             WHERE grl.id = ? AND pol.demand_source_type = 'WORK_ORDER'"
            .to_string(),
        "work_order_interveners" => {
            "SELECT work_order_id AS wo_id FROM work_order_interveners WHERE id = ?".to_string()
        }
        _ => return,
    };
    let result = async {
        let row = conn
            .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [row_id.into()]))
            .await?;
        let wo_id: Option<i64> = match row {
            Some(r) => decode(&r, "wo_id")?,
            None => None,
        };
        if let Some(wo_id) = wo_id {
            post_late_costs(conn, wo_id, None).await?;
        }
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(e) = result {
        tracing::warn!(target: "maintafox", "late cost posting after sync of {entity_type} {row_id}: {e}");
    }
}

/// Inventory hook, after a receipt, issue or return is committed: books the late costs of the OTs
/// it touched. Best effort: the stock movement is already committed and Finance's catch-up
/// (`post_all_late_costs`) books anything left, so a failure is logged, never returned.
pub async fn post_late_costs_best_effort(db: &sea_orm::DatabaseConnection, wo_ids: &[i64], actor_id: i64) {
    for &wo_id in wo_ids {
        let result = async {
            let txn = sea_orm::TransactionTrait::begin(db).await?;
            post_late_costs(&txn, wo_id, Some(actor_id)).await?;
            txn.commit().await?;
            Ok::<(), AppError>(())
        }
        .await;
        if let Err(e) = result {
            tracing::warn!(target: "maintafox", "late cost posting for work order {wo_id}: {e}");
        }
    }
}

/// OT a reservation or movement belongs to, from its source.
pub fn work_order_of_source(source_type: &str, source_id: Option<i64>) -> Option<i64> {
    let is_wo = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL
        .split(',')
        .any(|t| t.trim().trim_matches('\'') == source_type);
    if is_wo {
        source_id
    } else {
        None
    }
}

/// OTs whose PO lines a goods receipt received against.
pub async fn work_orders_of_receipt<C: ConnectionTrait>(db: &C, goods_receipt_id: i64) -> AppResult<Vec<i64>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT pol.demand_source_id AS wo_id
             FROM goods_receipt_lines grl
             JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
             WHERE grl.goods_receipt_id = ? AND pol.demand_source_type = 'WORK_ORDER'
               AND pol.demand_source_id IS NOT NULL",
            [goods_receipt_id.into()],
        ))
        .await?;
    rows.iter().map(|r| decode(r, "wo_id")).collect()
}

/// Late costs booked on a settlement (shown on the OT cost panel).
pub async fn late_cost_total<C: ConnectionTrait>(db: &C, settlement_sync_id: &str) -> AppResult<f64> {
    let prefix = format!("{settlement_sync_id}:late:");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(amount_base), 0.0) AS total FROM budget_actuals
             WHERE source_type = ? AND posting_status = 'posted' AND substr(source_id, 1, ?) = ?",
            [
                SETTLEMENT_LATE_SOURCE_TYPE.into(),
                (prefix.chars().count() as i64).into(),
                prefix.into(),
            ],
        ))
        .await?;
    Ok(match row {
        Some(r) => round_money(decode(&r, "total")?),
        None => 0.0,
    })
}

/// Catch-up for every settled OT: books the late costs not yet booked (see [`post_late_costs`]).
/// Returns the number of OTs updated.
pub async fn post_all_late_costs<C: ConnectionTrait>(db: &C, actor_id: i64) -> AppResult<i64> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT work_order_id FROM wo_cost_settlements WHERE status = 'posted'".to_string(),
        ))
        .await?;
    let mut touched = 0;
    for r in rows {
        let wo_id: i64 = decode(&r, "work_order_id")?;
        if post_late_costs(db, wo_id, Some(actor_id)).await?.abs() >= 0.005 {
            touched += 1;
        }
    }
    Ok(touched)
}

/// Period a reversal lands in: the first open month of the settlement's fiscal year from today
/// (else the latest earlier open month); when that whole year is closed, the first open month of
/// the current fiscal year in its posting version (prior-year correction booked now, as in
/// accounting practice). None only when no open month exists in either year.
async fn reversal_target<C: ConnectionTrait>(
    db: &C,
    settlement_fy: i64,
    settlement_version: i64,
) -> AppResult<Option<(i64, i64)>> {
    let (this_year, this_month) = fiscal::current_period(fiscal::fiscal_start_month(db).await?);
    let from_month = match this_year.cmp(&settlement_fy) {
        std::cmp::Ordering::Equal => this_month,
        std::cmp::Ordering::Greater => 12,
        std::cmp::Ordering::Less => 1,
    };
    if let Some(m) = periods::first_open_month_from(db, settlement_fy, from_month).await? {
        return Ok(Some((settlement_version, m)));
    }
    for m in (1..from_month).rev() {
        if periods::is_period_open(db, settlement_fy, m).await? {
            return Ok(Some((settlement_version, m)));
        }
    }
    if this_year != settlement_fy {
        if let Some((version_id, _)) = posting_budget_version(db, this_year).await? {
            if let Some(m) = periods::first_open_month_from(db, this_year, this_month).await? {
                return Ok(Some((version_id, m)));
            }
        }
    }
    Ok(None)
}

/// Reopen hook: undoes the OT's active settlement. Posted actuals (settlement and late costs) get
/// a negated `reversed` row in an open month (see [`reversal_target`]); the originals are marked
/// `reversed`. Nothing to undo (OT reopened from completed, or never settled) is not an error.
pub async fn reverse_work_order_settlement<C: ConnectionTrait>(
    db: &C,
    wo_id: i64,
    actor_id: i64,
    reason: &str,
) -> AppResult<Option<WoCostSettlement>> {
    let Some(active) = active_settlement(db, wo_id).await? else {
        return Ok(None);
    };
    let now = now_text();
    if active.status == "posted" {
        let (Some(fiscal_year), Some(version)) = (active.fiscal_year, active.budget_version_id) else {
            return Err(AppError::Internal(anyhow::anyhow!(
                "posted settlement without placement"
            )));
        };
        let Some((target_version, target_month)) = reversal_target(db, fiscal_year, version).await? else {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_FIN_REVERSAL_NO_OPEN_PERIOD: every budget period of {fiscal_year} and of the current year is closed"
            )]));
        };
        let prefix = format!("{}:", active.entity_sync_id);
        let originals = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, cost_center_id, budget_bucket, amount_source, source_currency,
                        amount_base, base_currency, source_id, work_order_id, equipment_id, rate_card_lane
                 FROM budget_actuals
                 WHERE source_type IN (?, ?) AND work_order_id = ? AND posting_status = 'posted'
                   AND substr(source_id, 1, ?) = ?",
                [
                    SETTLEMENT_SOURCE_TYPE.into(),
                    SETTLEMENT_LATE_SOURCE_TYPE.into(),
                    wo_id.into(),
                    (prefix.chars().count() as i64).into(),
                    prefix.clone().into(),
                ],
            ))
            .await?;
        for row in originals {
            let id: i64 = decode(&row, "id")?;
            let source_id: String = decode(&row, "source_id")?;
            let reversal_source = format!("REV-{source_id}");
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO budget_actuals
                    (entity_sync_id, budget_version_id, cost_center_id, period_month, budget_bucket,
                     amount_source, source_currency, amount_base, base_currency, source_type, source_id,
                     work_order_id, equipment_id, posting_status, posted_at, posted_by_id,
                     reversal_of_actual_id, reversal_reason, rate_card_lane, event_at, row_version,
                     created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'reversed', ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                [
                    actual_sync_id(&reversal_source).into(),
                    target_version.into(),
                    decode::<i64>(&row, "cost_center_id")?.into(),
                    target_month.into(),
                    decode::<String>(&row, "budget_bucket")?.into(),
                    (-decode::<f64>(&row, "amount_source")?).into(),
                    decode::<String>(&row, "source_currency")?.into(),
                    (-decode::<f64>(&row, "amount_base")?).into(),
                    decode::<String>(&row, "base_currency")?.into(),
                    SETTLEMENT_SOURCE_TYPE.into(),
                    reversal_source.into(),
                    decode::<Option<i64>>(&row, "work_order_id")?.into(),
                    decode::<Option<i64>>(&row, "equipment_id")?.into(),
                    now.clone().into(),
                    actor_id.into(),
                    id.into(),
                    reason.to_string().into(),
                    decode::<Option<String>>(&row, "rate_card_lane")?.into(),
                    now.clone().into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE budget_actuals SET posting_status = 'reversed', reversal_reason = ?,
                        row_version = row_version + 1, updated_at = ?
                 WHERE id = ?",
                [reason.to_string().into(), now.clone().into(), id.into()],
            ))
            .await?;
        }
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE wo_cost_settlements SET status = 'reversed', reversed_at = ?, reversed_by_id = ?,
                reversal_reason = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            now.clone().into(),
            actor_id.into(),
            reason.to_string().into(),
            now.into(),
            active.id.into(),
        ],
    ))
    .await?;
    Ok(Some(get_settlement(db, active.id).await?))
}

/// Everything the OT cost panel shows: the live breakdown and the settlement history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoCostView {
    pub breakdown: WoCostBreakdown,
    pub settlements: Vec<WoCostSettlement>,
    /// Late costs booked on the active settlement (receipts, issues, returns after the close).
    pub late_cost_total: f64,
    /// True when the per-person labour lines (names, hourly rates) were withheld: they need
    /// `fin.view`; the labour totals stay visible.
    pub labor_lines_hidden: bool,
}

pub async fn get_wo_cost_view<C: ConnectionTrait>(db: &C, wo_id: i64, show_labor_lines: bool) -> AppResult<WoCostView> {
    let mut breakdown = compute_wo_cost_breakdown(db, wo_id).await?;
    let labor_lines_hidden = !show_labor_lines && !breakdown.labor_lines.is_empty();
    if !show_labor_lines {
        breakdown.labor_lines.clear();
    }
    let late_cost_total = match active_settlement(db, wo_id).await? {
        Some(active) if active.status == "posted" => late_cost_total(db, &active.entity_sync_id).await?,
        _ => 0.0,
    };
    Ok(WoCostView {
        breakdown,
        settlements: list_wo_settlements(
            db,
            WoSettlementFilter {
                work_order_id: Some(wo_id),
                limit: Some(50),
                ..WoSettlementFilter::default()
            },
        )
        .await?,
        late_cost_total,
        labor_lines_hidden,
    })
}
