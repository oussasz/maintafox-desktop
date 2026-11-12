//! Contractor work on a work order (L3-09, L3-10): the external company doing the job, its order
//! or contract reference, the hours it declares and the cost it invoices.
//!
//! The cost is the work order's service cost (`service_cost_input`, the same value the close-out
//! panel and the cost summary use), so the budget and the contract's consumed amount
//! (`personnel::queries::list_external_companies`) read one figure. The contract state
//! (none / active / expiring / expired) comes from the company's contract end and its alert delay.

use crate::errors::{AppError, AppResult};
use crate::wo::workflow::state_machine::status_i18n_key;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};

/// The contractor facts of one work order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoSubcontracting {
    pub wo_id: i64,
    pub row_version: i64,
    pub external_company_id: Option<i64>,
    pub company_name: Option<String>,
    pub service_order_ref: Option<String>,
    pub subcontractor_hours: Option<f64>,
    pub service_cost: f64,
    pub contract_end: Option<String>,
    /// `none` (no company or no contract end), `active`, `expiring` or `expired`.
    pub contract_state: String,
    pub contract_days_left: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetWoSubcontractingInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    pub external_company_id: Option<i64>,
    pub service_order_ref: Option<String>,
    pub subcontractor_hours: Option<f64>,
    pub service_cost: Option<f64>,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "work order subcontracting: cannot read `{column}`: {e}"
    ))
}

/// State of a contract at `today` (`YYYY-MM-DD`): `(state, days left)`. A contract without an end
/// date has no state.
pub fn contract_status(contract_end: Option<&str>, alert_days: i64, today: &str) -> (String, Option<i64>) {
    let parse = |value: &str| {
        let value = value.trim();
        chrono::NaiveDate::parse_from_str(value.get(..10).unwrap_or(value), "%Y-%m-%d").ok()
    };
    let (Some(end), Some(now)) = (contract_end.and_then(parse), parse(today)) else {
        return ("none".to_string(), None);
    };
    let days = (end - now).num_days();
    let state = if days < 0 {
        "expired"
    } else if days <= alert_days {
        "expiring"
    } else {
        "active"
    };
    (state.to_string(), Some(days))
}

pub async fn get_wo_subcontracting(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoSubcontracting> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.id, wo.row_version, wo.external_company_id, ec.name AS company_name,
                    wo.service_order_ref, wo.subcontractor_hours,
                    COALESCE(wo.service_cost_input, wo.service_cost, 0.0) AS service_cost,
                    ec.contract_end, COALESCE(ec.contract_alert_days, 30) AS alert_days
             FROM work_orders wo
             LEFT JOIN external_companies ec ON ec.id = wo.external_company_id
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let contract_end: Option<String> = row
        .try_get("", "contract_end")
        .map_err(|e| decode_err("contract_end", e))?;
    let alert_days: i64 = row.try_get("", "alert_days").map_err(|e| decode_err("alert_days", e))?;
    let today = crate::clock::now().format("%Y-%m-%d").to_string();
    let (contract_state, contract_days_left) = contract_status(contract_end.as_deref(), alert_days, &today);
    Ok(WoSubcontracting {
        wo_id,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        external_company_id: row
            .try_get("", "external_company_id")
            .map_err(|e| decode_err("external_company_id", e))?,
        company_name: row
            .try_get("", "company_name")
            .map_err(|e| decode_err("company_name", e))?,
        service_order_ref: row
            .try_get("", "service_order_ref")
            .map_err(|e| decode_err("service_order_ref", e))?,
        subcontractor_hours: row
            .try_get("", "subcontractor_hours")
            .map_err(|e| decode_err("subcontractor_hours", e))?,
        service_cost: row
            .try_get("", "service_cost")
            .map_err(|e| decode_err("service_cost", e))?,
        contract_end,
        contract_state,
        contract_days_left,
    })
}

/// Saves the contractor, its order reference, hours and cost on an open work order. The work order
/// must still be at `expected_row_version`; a closed or cancelled one takes no change.
pub async fn set_wo_subcontracting(
    db: &DatabaseConnection,
    input: SetWoSubcontractingInput,
) -> AppResult<WoSubcontracting> {
    let current = get_wo_subcontracting(db, input.wo_id).await?;
    let status_code: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code
             FROM work_orders wo JOIN work_order_statuses wos ON wos.id = wo.status_id
             WHERE wo.id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?
        .try_get("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        let status_key = status_i18n_key(&status_code);
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_WO_SUBCONTRACT_LOCKED:status={status_key}: The contractor details of a work order in status '{status_code}' cannot be changed."
        )]));
    }
    if input.service_cost.is_some_and(|c| !c.is_finite() || c < 0.0) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_SUBCONTRACT_COST_NEGATIVE: The contractor cost cannot be negative.".to_string(),
        ]));
    }
    if input.subcontractor_hours.is_some_and(|h| !h.is_finite() || h < 0.0) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_SUBCONTRACT_HOURS_NEGATIVE: The contractor hours cannot be negative.".to_string(),
        ]));
    }
    // A contractor that is not the one already on the work order must be an active company.
    if let Some(company_id) = input.external_company_id {
        if current.external_company_id != Some(company_id) {
            let active: Option<i64> = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT is_active FROM external_companies WHERE id = ?",
                    [company_id.into()],
                ))
                .await?
                .map(|r| r.try_get::<i64>("", "is_active"))
                .transpose()
                .map_err(|e| decode_err("is_active", e))?;
            match active {
                None => {
                    return Err(AppError::ValidationFailed(vec![
                        "GATE_WO_SUBCONTRACT_COMPANY_UNKNOWN: This contractor does not exist.".to_string(),
                    ]))
                }
                Some(0) => {
                    return Err(AppError::ValidationFailed(vec![
                        "GATE_WO_SUBCONTRACT_COMPANY_INACTIVE: This contractor is inactive: choose an active one."
                            .to_string(),
                    ]))
                }
                Some(_) => {}
            }
        }
    }
    let order_ref = input
        .service_order_ref
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders
             SET external_company_id = ?, service_order_ref = ?, subcontractor_hours = ?, service_cost_input = ?,
                 row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                input.external_company_id.map_or(Value::BigInt(None), Value::from),
                order_ref.map_or(Value::String(None), Value::from),
                input.subcontractor_hours.map_or(Value::Double(None), Value::from),
                input.service_cost.map_or(Value::Double(None), Value::from),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    crate::wo::closeout::check_concurrency(result.rows_affected())?;
    get_wo_subcontracting(db, input.wo_id).await
}

/// Active contractors a work order can be given (id and name), with their contract state, for the
/// picker of the work order screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoContractorOption {
    pub id: i64,
    pub name: String,
    pub contract_number: Option<String>,
    pub contract_end: Option<String>,
    pub contract_state: String,
}

pub async fn list_wo_contractor_options(db: &DatabaseConnection) -> AppResult<Vec<WoContractorOption>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, name, contract_number, contract_end, COALESCE(contract_alert_days, 30) AS alert_days
             FROM external_companies WHERE is_active = 1 ORDER BY name ASC"
                .to_string(),
        ))
        .await?;
    let today = crate::clock::now().format("%Y-%m-%d").to_string();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let contract_end: Option<String> = row
            .try_get("", "contract_end")
            .map_err(|e| decode_err("contract_end", e))?;
        let alert_days: i64 = row.try_get("", "alert_days").map_err(|e| decode_err("alert_days", e))?;
        let (contract_state, _) = contract_status(contract_end.as_deref(), alert_days, &today);
        out.push(WoContractorOption {
            id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
            name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
            contract_number: row
                .try_get("", "contract_number")
                .map_err(|e| decode_err("contract_number", e))?,
            contract_end,
            contract_state,
        });
    }
    Ok(out)
}
