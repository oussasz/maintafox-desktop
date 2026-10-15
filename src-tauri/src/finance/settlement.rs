//! Settlement of a closed work order (OT) into the budget — SAP PM "order settlement".
//!
//! Called from `wo::closeout::close_wo` and `reopen_wo`, inside their transaction:
//! - close: the OT's cost breakdown (`wo_costing`) is written back on `work_orders` (labour, parts,
//!   services + other, total) so every reader (asset cost, SMRP cost/RAV, RAMS) sees the same
//!   figures, then posted once to `budget_actuals` against the OT's cost centre, the approved
//!   budget version of the year and the first open month from the close date. One `posted` row per
//!   bucket (labour regular / overtime, parts, services, other), `source_type = 'wo_settlement'`.
//! - When the cost centre, an approved budget version, an open month or the tenant currency is
//!   missing, nothing is booked: the settlement is recorded `pending` with its reason, shown on
//!   the OT and in the Finance worklist, and Finance settles it once the cause is fixed.
//! - Idempotent: while an OT has a `posted` or `pending` settlement, closing again does nothing.
//! - reopen: the posted actuals are reversed (negated rows, status `reversed`) and the settlement
//!   becomes `reversed`; the next close settles the new cycle.

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
        s.reversal_reason, s.row_version
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

/// Books the actual rows of a posted settlement.
async fn insert_settlement_actuals<C: ConnectionTrait>(
    db: &C,
    settlement_sync_id: &str,
    breakdown: &WoCostBreakdown,
    placement: &Placement,
    currency: &str,
    actor_id: i64,
) -> AppResult<()> {
    let labor_regular = round_money(breakdown.labor.actual - breakdown.labor_overtime_amount);
    let rows: [(&str, Option<&str>, f64); 5] = [
        (BUCKET_LABOR, Some("regular"), labor_regular),
        (BUCKET_LABOR, Some("overtime"), breakdown.labor_overtime_amount),
        (BUCKET_PARTS, None, breakdown.parts.actual),
        (BUCKET_SERVICES, None, breakdown.services.actual),
        (BUCKET_OTHER, None, breakdown.other.actual),
    ];
    let now = now_text();
    for (bucket, lane, amount) in rows {
        if amount.abs() < 0.005 {
            continue;
        }
        let source_id = match lane {
            Some(lane) => format!("{settlement_sync_id}:{bucket}:{lane}"),
            None => format!("{settlement_sync_id}:{bucket}"),
        };
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO budget_actuals
                (entity_sync_id, budget_version_id, cost_center_id, period_month, budget_bucket,
                 amount_source, source_currency, amount_base, base_currency, source_type, source_id,
                 work_order_id, equipment_id, posting_status, posted_at, posted_by_id, rate_card_lane,
                 event_at, row_version, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'posted', ?, ?, ?, ?, 1, ?, ?)",
            [
                uuid::Uuid::new_v4().to_string().into(),
                placement.budget_version_id.into(),
                placement.cost_center_id.into(),
                placement.period_month.into(),
                bucket.into(),
                amount.into(),
                currency.into(),
                amount.into(),
                currency.into(),
                SETTLEMENT_SOURCE_TYPE.into(),
                source_id.into(),
                breakdown.work_order_id.into(),
                breakdown.equipment_id.into(),
                now.clone().into(),
                actor_id.into(),
                lane.map(str::to_string).into(),
                now.clone().into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

async fn reopen_count_and_close<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<(i64, Option<String>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(reopen_count, 0) AS reopen_count, closed_at FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    Ok((decode(&row, "reopen_count")?, decode(&row, "closed_at")?))
}

/// Fiscal year and period a settlement is dated in: the OT's close date, else today.
fn posting_period(closed_at: Option<&str>, fiscal_start: i64) -> (i64, i64) {
    let date = closed_at
        .and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    fiscal::fiscal_period(i64::from(date.year()), i64::from(date.month()), fiscal_start)
}

/// Settles a just-closed OT (close-out hook). Never fails for a business reason: anything that
/// prevents posting yields a `pending` settlement. Returns the OT's active settlement.
pub async fn settle_closed_work_order<C: ConnectionTrait>(
    db: &C,
    wo_id: i64,
    actor_id: i64,
) -> AppResult<WoCostSettlement> {
    if let Some(existing) = active_settlement(db, wo_id).await? {
        return Ok(existing);
    }
    let (cycle, closed_at) = reopen_count_and_close(db, wo_id).await?;
    let closed_at = closed_at.unwrap_or_else(now_text);
    let now = now_text();
    let sync_id = uuid::Uuid::new_v4().to_string();

    // Without the tenant currency or fiscal calendar nothing can be dated or valued: pending.
    let currency = crate::inventory::fx::tenant_currency(db).await.ok();
    let fiscal_start = fiscal::fiscal_start_month(db).await.ok();
    let (currency, fiscal_start) = match (currency, fiscal_start) {
        (Some(currency), Some(fiscal_start)) => (currency, fiscal_start),
        (_, fiscal_start) => {
            let reason = if fiscal_start.is_none() {
                PENDING_NO_FISCAL_CALENDAR
            } else {
                PENDING_NO_CURRENCY
            };
            let year = i64::from(Utc::now().year());
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO wo_cost_settlements
                (entity_sync_id, work_order_id, settlement_cycle, status, pending_reason, fiscal_year,
                 currency_code, closed_at, row_version, created_at, updated_at)
             VALUES (?, ?, ?, 'pending', ?, ?, '', ?, 1, ?, ?)",
                [
                    sync_id.into(),
                    wo_id.into(),
                    cycle.into(),
                    reason.into(),
                    year.into(),
                    closed_at.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
            return active_settlement(db, wo_id)
                .await?
                .ok_or_else(|| AppError::Internal(anyhow::anyhow!("settlement row missing")));
        }
    };

    let (year, month) = posting_period(Some(&closed_at), fiscal_start);
    let breakdown = compute_wo_cost_breakdown(db, wo_id).await?;
    write_back_wo_costs(db, &breakdown, false).await?;
    let placement = place(db, wo_id, &currency, year, month).await?;
    let posted = placement.pending_reason.is_none();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_cost_settlements
            (entity_sync_id, work_order_id, settlement_cycle, status, pending_reason, cost_center_id,
             budget_version_id, fiscal_year, period_month, currency_code, labor_amount, labor_overtime_amount,
             parts_amount, services_amount, other_amount, total_amount, overtime_captured, unpriced_labor_hours,
             unvalued_parts_qty, closed_at, posted_at, posted_by_id, row_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
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
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    if posted {
        insert_settlement_actuals(db, &sync_id, &breakdown, &placement, &currency, actor_id).await?;
    }
    active_settlement(db, wo_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("settlement row missing")))
}

/// Finance retries a pending settlement once its cause is fixed (cost centre set on the OT or
/// asset, budget version approved, period reopened, currency configured). Figures are recomputed.
/// Returns the settlement, still pending (with the current reason) or posted.
pub async fn retry_pending_settlement<C: ConnectionTrait>(
    db: &C,
    settlement_id: i64,
    actor_id: i64,
) -> AppResult<WoCostSettlement> {
    let current = get_settlement(db, settlement_id).await?;
    if current.status != "pending" {
        return Err(AppError::ValidationFailed(vec![
            "Seule une imputation en attente peut être relancée. (Only a pending settlement can be retried.)"
                .to_string(),
        ]));
    }
    let currency = crate::inventory::fx::tenant_currency(db).await?;
    let fiscal_start = fiscal::fiscal_start_month(db).await?;
    let (year, month) = posting_period(current.closed_at.as_deref(), fiscal_start);
    let breakdown = compute_wo_cost_breakdown(db, current.work_order_id).await?;
    write_back_wo_costs(db, &breakdown, true).await?;
    let placement = place(db, current.work_order_id, &currency, year, month).await?;
    let posted = placement.pending_reason.is_none();
    let now = now_text();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE wo_cost_settlements SET status = ?, pending_reason = ?, cost_center_id = ?, budget_version_id = ?,
                fiscal_year = ?, period_month = ?, currency_code = ?, labor_amount = ?, labor_overtime_amount = ?,
                parts_amount = ?, services_amount = ?, other_amount = ?, total_amount = ?, overtime_captured = ?,
                unpriced_labor_hours = ?, unvalued_parts_qty = ?, posted_at = ?, posted_by_id = ?,
                row_version = row_version + 1, updated_at = ?
         WHERE id = ? AND status = 'pending'",
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
            now.into(),
            settlement_id.into(),
        ],
    ))
    .await?;
    if posted {
        insert_settlement_actuals(db, &current.entity_sync_id, &breakdown, &placement, &currency, actor_id).await?;
    }
    get_settlement(db, settlement_id).await
}

/// Reopen hook: undoes the OT's active settlement. Posted actuals get a negated `reversed` row in
/// the first open month from today (same fiscal year); the originals are marked `reversed`.
/// Nothing to undo (OT reopened from completed, or never settled) is not an error.
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
        let (this_year, this_month) = fiscal::current_period(fiscal::fiscal_start_month(db).await?);
        let fiscal_year = active.fiscal_year.unwrap_or(this_year);
        let from_month = match this_year.cmp(&fiscal_year) {
            std::cmp::Ordering::Equal => this_month,
            std::cmp::Ordering::Greater => 12,
            std::cmp::Ordering::Less => 1,
        };
        let mut target = periods::first_open_month_from(db, fiscal_year, from_month).await?;
        if target.is_none() {
            // Later months closed: the latest earlier open month still keeps the year balanced.
            for m in (1..from_month).rev() {
                if periods::is_period_open(db, fiscal_year, m).await? {
                    target = Some(m);
                    break;
                }
            }
        }
        let Some(target_month) = target else {
            return Err(AppError::ValidationFailed(vec![format!(
                "Toutes les périodes budgétaires {fiscal_year} sont clôturées : rouvrez une période pour annuler \
                 l'imputation de cet OT. (Every {fiscal_year} budget period is closed: reopen one to reverse this \
                 work order's settlement.)"
            )]));
        };
        let prefix = format!("{}:", active.entity_sync_id);
        let originals = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, budget_version_id, cost_center_id, budget_bucket, amount_source, source_currency,
                        amount_base, base_currency, source_id, work_order_id, equipment_id, rate_card_lane
                 FROM budget_actuals
                 WHERE source_type = ? AND work_order_id = ? AND posting_status = 'posted'
                   AND substr(source_id, 1, ?) = ?",
                [
                    SETTLEMENT_SOURCE_TYPE.into(),
                    wo_id.into(),
                    (prefix.chars().count() as i64).into(),
                    prefix.clone().into(),
                ],
            ))
            .await?;
        for row in originals {
            let id: i64 = decode(&row, "id")?;
            let source_id: String = decode(&row, "source_id")?;
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO budget_actuals
                    (entity_sync_id, budget_version_id, cost_center_id, period_month, budget_bucket,
                     amount_source, source_currency, amount_base, base_currency, source_type, source_id,
                     work_order_id, equipment_id, posting_status, posted_at, posted_by_id,
                     reversal_of_actual_id, reversal_reason, rate_card_lane, event_at, row_version,
                     created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'reversed', ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                [
                    uuid::Uuid::new_v4().to_string().into(),
                    decode::<i64>(&row, "budget_version_id")?.into(),
                    decode::<i64>(&row, "cost_center_id")?.into(),
                    target_month.into(),
                    decode::<String>(&row, "budget_bucket")?.into(),
                    (-decode::<f64>(&row, "amount_source")?).into(),
                    decode::<String>(&row, "source_currency")?.into(),
                    (-decode::<f64>(&row, "amount_base")?).into(),
                    decode::<String>(&row, "base_currency")?.into(),
                    SETTLEMENT_SOURCE_TYPE.into(),
                    format!("REV-{source_id}").into(),
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
}

pub async fn get_wo_cost_view<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<WoCostView> {
    Ok(WoCostView {
        breakdown: compute_wo_cost_breakdown(db, wo_id).await?,
        settlements: list_wo_settlements(
            db,
            WoSettlementFilter {
                work_order_id: Some(wo_id),
                limit: Some(50),
                ..WoSettlementFilter::default()
            },
        )
        .await?,
    })
}
