//! Approval limits and separation of duties for requisitions and purchase orders.
//!
//! Enforced by the backend at the transition to APPROVED:
//! 1. Separation of duties: the requester of a requisition, or the buyer who created a purchase
//!    order, cannot approve it.
//! 2. Amount limit (only when limits are configured for that document type): the approver must
//!    hold a role whose limit (`procurement_approval_limits`, tenant currency, NULL = unlimited)
//!    covers the document amount. A document whose amount cannot be established (line without a
//!    price, foreign currency without a stored rate) can only be approved with an unlimited role.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{ProcurementApprovalLimit, ProcurementApprovalLimitInput};

pub const DOC_REQUISITION: &str = "REQUISITION";
pub const DOC_PURCHASE_ORDER: &str = "PURCHASE_ORDER";

/// Amount of a document in the tenant currency; None when it cannot be established, with the
/// reason shown to the approver.
#[derive(Debug, Clone)]
pub struct DocumentAmount {
    pub amount: Option<f64>,
    pub unknown_reason: Option<String>,
}

fn map_limit(row: sea_orm::QueryResult) -> AppResult<ProcurementApprovalLimit> {
    Ok(ProcurementApprovalLimit {
        id: row.try_get("", "id")?,
        role_id: row.try_get("", "role_id")?,
        role_name: row.try_get("", "role_name")?,
        document_type: row.try_get("", "document_type")?,
        max_amount: row.try_get("", "max_amount")?,
        is_active: row.try_get("", "is_active")?,
        row_version: row.try_get("", "row_version")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

const LIMIT_SELECT: &str = "SELECT l.id, l.role_id, r.name AS role_name, l.document_type, l.max_amount, l.is_active,
            l.row_version, l.updated_at
     FROM procurement_approval_limits l
     JOIN roles r ON r.id = l.role_id";

pub async fn list_limits(db: &DatabaseConnection) -> AppResult<Vec<ProcurementApprovalLimit>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{LIMIT_SELECT} ORDER BY l.document_type, r.name"),
        ))
        .await?;
    rows.into_iter().map(map_limit).collect()
}

fn ensure_document_type(value: &str) -> AppResult<()> {
    if value != DOC_REQUISITION && value != DOC_PURCHASE_ORDER {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_APPROVAL_DOC_TYPE: document_type must be REQUISITION or PURCHASE_ORDER.".to_string(),
        ]));
    }
    Ok(())
}

/// Creates or replaces the active limit of a role for a document type.
pub async fn upsert_limit(
    db: &DatabaseConnection,
    input: ProcurementApprovalLimitInput,
) -> AppResult<ProcurementApprovalLimit> {
    ensure_document_type(&input.document_type)?;
    if let Some(max) = input.max_amount {
        if !max.is_finite() || max < 0.0 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_INV_APPROVAL_LIMIT_NEGATIVE: The approval limit must be 0 or more (leave empty for unlimited)."
                    .to_string(),
            ]));
        }
    }
    let role = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE id = ?",
            [input.role_id.into()],
        ))
        .await?;
    if role.is_none() {
        return Err(AppError::NotFound {
            entity: "roles".to_string(),
            id: input.role_id.to_string(),
        });
    }
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let is_active = input.is_active.unwrap_or(true) as i64;
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM procurement_approval_limits WHERE role_id = ? AND document_type = ?
             ORDER BY is_active DESC, id DESC LIMIT 1",
            [input.role_id.into(), input.document_type.clone().into()],
        ))
        .await?;
    let id: i64 = if let Some(row) = existing {
        let id: i64 = row.try_get("", "id")?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE procurement_approval_limits
             SET max_amount = ?, is_active = ?, row_version = row_version + 1, updated_at = ?
             WHERE id = ?",
            [
                input.max_amount.map_or(Value::Double(None), Value::from),
                is_active.into(),
                now.into(),
                id.into(),
            ],
        ))
        .await?;
        id
    } else {
        let inserted = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO procurement_approval_limits
                (entity_sync_id, role_id, document_type, max_amount, is_active, row_version, created_by_id,
                 created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?)",
                [
                    uuid::Uuid::new_v4().to_string().into(),
                    input.role_id.into(),
                    input.document_type.clone().into(),
                    input.max_amount.map_or(Value::Double(None), Value::from),
                    is_active.into(),
                    input.actor_id.map_or(Value::BigInt(None), Value::from),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        crate::db::inserted_id(&inserted)?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LIMIT_SELECT} WHERE l.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("approval limit not found")))?;
    map_limit(row)
}

/// Requisition amount: requested quantity × estimated unit cost, falling back to the article's
/// weighted-average stock cost.
pub async fn requisition_amount<C: ConnectionTrait>(db: &C, requisition_id: i64) -> AppResult<DocumentAmount> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.article_code, rl.requested_qty,
                    COALESCE(rl.projected_unit_cost,
                             (SELECT SUM(sb.on_hand_qty * sb.moving_avg_unit_cost) / NULLIF(SUM(sb.on_hand_qty), 0)
                              FROM stock_balances sb
                              WHERE sb.article_id = rl.article_id AND sb.moving_avg_unit_cost IS NOT NULL)) AS unit_cost
             FROM procurement_requisition_lines rl
             JOIN articles a ON a.id = rl.article_id
             WHERE rl.requisition_id = ?",
            [requisition_id.into()],
        ))
        .await?;
    let mut total = 0.0;
    for row in rows {
        let qty: f64 = row.try_get("", "requested_qty")?;
        let cost: Option<f64> = row.try_get("", "unit_cost").ok().flatten();
        match cost {
            Some(c) => total += qty * c,
            None => {
                let code: String = row.try_get("", "article_code")?;
                return Ok(DocumentAmount {
                    amount: None,
                    unknown_reason: Some(format!(
                        "line {code} has no estimated price and the article has no stock cost"
                    )),
                });
            }
        }
    }
    Ok(DocumentAmount {
        amount: Some(total),
        unknown_reason: None,
    })
}

/// PO amount in the tenant currency (lines × PO price, converted at the rate valid today).
pub async fn purchase_order_amount<C: ConnectionTrait>(db: &C, purchase_order_id: i64) -> AppResult<DocumentAmount> {
    let currency: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT currency_code FROM purchase_orders WHERE id = ?",
            [purchase_order_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "currency_code").ok())
        .flatten();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.article_code, pol.ordered_qty, pol.unit_price
             FROM purchase_order_lines pol JOIN articles a ON a.id = pol.article_id
             WHERE pol.purchase_order_id = ?",
            [purchase_order_id.into()],
        ))
        .await?;
    let mut total = 0.0;
    for row in rows {
        let qty: f64 = row.try_get("", "ordered_qty")?;
        let price: Option<f64> = row.try_get("", "unit_price").ok().flatten();
        match price {
            Some(p) => total += qty * p,
            None => {
                let code: String = row.try_get("", "article_code")?;
                return Ok(DocumentAmount {
                    amount: None,
                    unknown_reason: Some(format!("line {code} has no price")),
                });
            }
        }
    }
    let rate = crate::inventory::fx::rate_to_tenant(db, currency.as_deref(), &crate::inventory::fx::today()).await?;
    Ok(match rate {
        Some(rate) => DocumentAmount {
            amount: Some(total * rate),
            unknown_reason: None,
        },
        None => DocumentAmount {
            amount: None,
            unknown_reason: Some(format!(
                "no exchange rate {} → tenant currency is stored",
                currency.unwrap_or_default()
            )),
        },
    })
}

/// Largest approval limit the user holds for a document type through active role assignments:
/// `Ok(None)` when limits are not configured for that type at all, `Ok(Some(None))` unlimited,
/// `Ok(Some(Some(x)))` capped at x, or an error when limits exist but none of the user's roles has one.
async fn approver_limit<C: ConnectionTrait>(
    db: &C,
    user_id: i64,
    document_type: &str,
) -> AppResult<Option<Option<f64>>> {
    let configured: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM procurement_approval_limits WHERE document_type = ? AND is_active = 1",
            [document_type.into()],
        ))
        .await?
        .map(|r| r.try_get("", "n"))
        .transpose()?
        .unwrap_or(0);
    if configured == 0 {
        return Ok(None);
    }
    // The shared rule and clock: stored bounds are `YYYY-MM-DDTHH:MM:SSZ`, which
    // `datetime('now')` (`YYYY-MM-DD HH:MM:SS`) does not compare with, so a lapsed grant used
    // to keep its limit until midnight.
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(crate::rbac::resolver::in_force_binds(&now));
    values.push(document_type.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT l.max_amount FROM procurement_approval_limits l
                 JOIN user_scope_assignments usa ON usa.role_id = l.role_id
                 WHERE usa.user_id = ? AND {}
                   AND l.document_type = ? AND l.is_active = 1",
                crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            values,
        ))
        .await?;
    if rows.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_NO_APPROVAL_LIMIT: none of your roles may approve a {} (no approval limit is set for them).",
            label(document_type)
        )]));
    }
    let mut best: Option<f64> = Some(0.0);
    for row in rows {
        match row.try_get::<Option<f64>>("", "max_amount")? {
            None => return Ok(Some(None)),
            Some(v) => best = Some(best.unwrap_or(0.0).max(v)),
        }
    }
    Ok(Some(best))
}

fn label(document_type: &str) -> &'static str {
    if document_type == DOC_PURCHASE_ORDER {
        "purchase order"
    } else {
        "requisition"
    }
}

/// Checks separation of duties and the approver's limit before an approval.
pub async fn ensure_can_approve<C: ConnectionTrait>(
    db: &C,
    document_type: &str,
    document_id: i64,
    author_id: Option<i64>,
    approver_id: Option<i64>,
) -> AppResult<()> {
    let Some(approver) = approver_id else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_APPROVER_REQUIRED: An approval needs a signed-in approver.".to_string(),
        ]));
    };
    if author_id == Some(approver) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_SELF_APPROVAL: you created this {}; another person must approve it.",
            label(document_type)
        )]));
    }
    let Some(limit) = approver_limit(db, approver, document_type).await? else {
        return Ok(());
    };
    let Some(max) = limit else {
        return Ok(()); // unlimited role
    };
    let amount = if document_type == DOC_PURCHASE_ORDER {
        purchase_order_amount(db, document_id).await?
    } else {
        requisition_amount(db, document_id).await?
    };
    match amount.amount {
        Some(value) if value <= max + 1e-6 => Ok(()),
        Some(value) => Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_APPROVAL_LIMIT: this {} amounts to {value:.2}, above your approval limit of {max:.2}.",
            label(document_type)
        )])),
        None => Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_AMOUNT_UNKNOWN: the amount of this {} cannot be established ({}); only an unlimited \
             approver can approve it.",
            label(document_type),
            amount.unknown_reason.unwrap_or_default()
        )])),
    }
}
