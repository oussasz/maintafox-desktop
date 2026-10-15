//! Budget periods: month-level close of a fiscal year (fiscal year = calendar year).
//!
//! A month without a `budget_periods` row is open. Closing a month refuses every new actual dated
//! in it (manual actuals and OT settlements); OT settlements then land in the next open month of
//! the same year. A month cannot be closed while OTs closed in it still wait for settlement, so a
//! closed month always holds the complete maintenance cost of its OTs.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::finance::fiscal;
use crate::finance::wo_costing::decode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetPeriod {
    pub fiscal_year: i64,
    pub period_month: i64,
    /// `open` | `closed`
    pub status: String,
    pub closed_at: Option<String>,
    pub closed_by_name: Option<String>,
    pub reopened_at: Option<String>,
    pub notes: Option<String>,
    /// Net posted actuals dated in the month (all versions of the year), tenant currency.
    pub actual_amount: f64,
    /// OTs closed in the month whose settlement is still pending.
    pub pending_settlements: i64,
    /// The month has not ended yet (cannot be closed).
    pub is_future: bool,
    /// Calendar year and month of this fiscal period (fiscal year start month setting).
    pub calendar_year: i64,
    pub calendar_month: i64,
    pub row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseBudgetPeriodInput {
    pub fiscal_year: i64,
    pub period_month: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReopenBudgetPeriodInput {
    pub fiscal_year: i64,
    pub period_month: i64,
    pub reason: String,
}

fn validate_month(month: i64) -> AppResult<()> {
    if (1..=12).contains(&month) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![
            "GATE_FIN_MONTH_INVALID: month must be between 1 and 12".to_string(),
        ]))
    }
}

/// Status of a month: the latest row decides; no row means open.
pub async fn is_period_open<C: ConnectionTrait>(db: &C, fiscal_year: i64, month: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM budget_periods WHERE fiscal_year = ? AND period_month = ?
             ORDER BY id DESC LIMIT 1",
            [fiscal_year.into(), month.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => decode::<String>(&r, "status")? == "open",
        None => true,
    })
}

/// First open month of the year from `from_month` (inclusive) onwards.
pub async fn first_open_month_from<C: ConnectionTrait>(
    db: &C,
    fiscal_year: i64,
    from_month: i64,
) -> AppResult<Option<i64>> {
    for month in from_month.max(1)..=12 {
        if is_period_open(db, fiscal_year, month).await? {
            return Ok(Some(month));
        }
    }
    Ok(None)
}

/// Refuses an actual dated in a closed month.
pub async fn ensure_period_open<C: ConnectionTrait>(db: &C, fiscal_year: i64, month: i64) -> AppResult<()> {
    if is_period_open(db, fiscal_year, month).await? {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "GATE_FIN_PERIOD_CLOSED: budget period {month} of fiscal year {fiscal_year} is closed"
        )]))
    }
}

pub async fn list_budget_periods<C: ConnectionTrait>(db: &C, fiscal_year: i64) -> AppResult<Vec<BudgetPeriod>> {
    let start = fiscal::fiscal_start_month(db).await?;
    let (this_year, this_month) = fiscal::current_period(start);
    let pending_sql = format!(
        "SELECT
            (SELECT COALESCE(SUM(ba.amount_base), 0.0)
               FROM budget_actuals ba JOIN budget_versions bv ON bv.id = ba.budget_version_id
              WHERE bv.fiscal_year = ?1 AND ba.period_month = ?2
                AND ba.posting_status IN ('posted', 'reversed')) AS actual_amount,
            (SELECT COUNT(*) FROM wo_cost_settlements s
              WHERE s.status = 'pending' AND {} = ?1 AND {} = ?2) AS pending",
        fiscal::sql_fiscal_year("s.closed_at", "?3"),
        fiscal::sql_fiscal_period("s.closed_at", "?3"),
    );
    let mut out = Vec::with_capacity(12);
    for month in 1..=12_i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT bp.status, bp.closed_at, bp.reopened_at, bp.notes, bp.row_version,
                        COALESCE(p.full_name, ua.username) AS closed_by_name
                 FROM budget_periods bp
                 LEFT JOIN user_accounts ua ON ua.id = bp.closed_by_id
                 LEFT JOIN personnel p ON p.id = ua.personnel_id
                 WHERE bp.fiscal_year = ? AND bp.period_month = ?
                 ORDER BY bp.id DESC LIMIT 1",
                [fiscal_year.into(), month.into()],
            ))
            .await?;
        let figures = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                pending_sql.clone(),
                [fiscal_year.into(), month.into(), start.into()],
            ))
            .await?;
        let (actual_amount, pending) = match figures {
            Some(f) => (decode::<f64>(&f, "actual_amount")?, decode::<i64>(&f, "pending")?),
            None => (0.0, 0),
        };
        let is_future = fiscal_year > this_year || (fiscal_year == this_year && month >= this_month);
        let (calendar_year, calendar_month) = fiscal::calendar_month(fiscal_year, month, start);
        out.push(match row {
            Some(r) => BudgetPeriod {
                fiscal_year,
                period_month: month,
                status: decode(&r, "status")?,
                closed_at: decode(&r, "closed_at")?,
                closed_by_name: decode(&r, "closed_by_name")?,
                reopened_at: decode(&r, "reopened_at")?,
                notes: decode(&r, "notes")?,
                actual_amount,
                pending_settlements: pending,
                is_future,
                calendar_year,
                calendar_month,
                row_version: decode(&r, "row_version")?,
            },
            None => BudgetPeriod {
                fiscal_year,
                period_month: month,
                status: "open".to_string(),
                closed_at: None,
                closed_by_name: None,
                reopened_at: None,
                notes: None,
                actual_amount,
                pending_settlements: pending,
                is_future,
                calendar_year,
                calendar_month,
                row_version: None,
            },
        });
    }
    Ok(out)
}

async fn period_row_id<C: ConnectionTrait>(db: &C, fiscal_year: i64, month: i64) -> AppResult<Option<i64>> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM budget_periods WHERE fiscal_year = ? AND period_month = ? ORDER BY id DESC LIMIT 1",
        [fiscal_year.into(), month.into()],
    ))
    .await?
    .map(|r| decode::<i64>(&r, "id"))
    .transpose()
}

pub async fn close_budget_period<C: ConnectionTrait>(
    db: &C,
    actor_id: i64,
    input: CloseBudgetPeriodInput,
) -> AppResult<Vec<BudgetPeriod>> {
    validate_month(input.period_month)?;
    let periods = list_budget_periods(db, input.fiscal_year).await?;
    let period = &periods[(input.period_month - 1) as usize];
    if period.status == "closed" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_PERIOD_ALREADY_CLOSED: this period is already closed".to_string(),
        ]));
    }
    if period.is_future {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_PERIOD_NOT_ENDED: a period can only be closed after it has ended".to_string(),
        ]));
    }
    if period.pending_settlements > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_FIN_PERIOD_PENDING_SETTLEMENTS: {n} work order(s) closed in this period are waiting for settlement",
            n = period.pending_settlements
        )]));
    }
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let notes = input.notes.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    match period_row_id(db, input.fiscal_year, input.period_month).await? {
        Some(id) => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE budget_periods SET status = 'closed', closed_at = ?, closed_by_id = ?, notes = ?,
                        row_version = row_version + 1, updated_at = ?
                 WHERE id = ?",
                [now.clone().into(), actor_id.into(), notes.into(), now.into(), id.into()],
            ))
            .await?;
        }
        None => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO budget_periods
                    (entity_sync_id, fiscal_year, period_month, status, closed_at, closed_by_id, notes,
                     row_version, created_at, updated_at)
                 VALUES (?, ?, ?, 'closed', ?, ?, ?, 1, ?, ?)",
                [
                    uuid::Uuid::new_v4().to_string().into(),
                    input.fiscal_year.into(),
                    input.period_month.into(),
                    now.clone().into(),
                    actor_id.into(),
                    notes.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        }
    }
    list_budget_periods(db, input.fiscal_year).await
}

pub async fn reopen_budget_period<C: ConnectionTrait>(
    db: &C,
    actor_id: i64,
    input: ReopenBudgetPeriodInput,
) -> AppResult<Vec<BudgetPeriod>> {
    validate_month(input.period_month)?;
    let reason = input.reason.trim().to_string();
    if reason.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_REOPEN_REASON_REQUIRED: a reopen reason is required".to_string(),
        ]));
    }
    let id = match period_row_id(db, input.fiscal_year, input.period_month).await? {
        Some(id) if !is_period_open(db, input.fiscal_year, input.period_month).await? => id,
        _ => {
            return Err(AppError::ValidationFailed(vec![
                "GATE_FIN_PERIOD_NOT_CLOSED: this period is not closed".to_string(),
            ]))
        }
    };
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE budget_periods SET status = 'open', reopened_at = ?, reopened_by_id = ?,
                notes = TRIM(COALESCE(notes || char(10), '') || ?),
                row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            now.clone().into(),
            actor_id.into(),
            reason.into(),
            now.into(),
            id.into(),
        ],
    ))
    .await?;
    list_budget_periods(db, input.fiscal_year).await
}
