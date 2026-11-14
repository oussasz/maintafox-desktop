//! Approval limits and separation of duties for requisitions and purchase orders.
//!
//! Enforced by the backend at the transition to APPROVED (and, for the right, REJECTED):
//! 1. Approval right: the approver holds `inv.approve` (distinct from `inv.procure`, which
//!    creates and manages documents).
//! 2. Separation of duties: the requester of a requisition, or the buyer who created a purchase
//!    order, cannot approve it.
//! 3. Amount limit, fail closed: the approver must hold a role whose active limit
//!    (`procurement_approval_limits`, tenant currency, NULL = unlimited) covers the document
//!    amount. A user whose roles have no limit for that document type cannot approve it, even when
//!    no limit is configured at all; a document whose amount cannot be established (line without a
//!    price, foreign currency without a stored rate) can only be approved with an unlimited role.
//!    So the plant is never locked out, one visible, editable row "Administrator: unlimited" is
//!    seeded per document type ([`seed_administrator_unlimited_limits`]); every other limit is
//!    left for the administrator to set.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{ProcurementApprovalLimit, ProcurementApprovalLimitInput, ProcurementApproverRoles};

pub const DOC_REQUISITION: &str = "REQUISITION";
pub const DOC_PURCHASE_ORDER: &str = "PURCHASE_ORDER";
pub const DOCUMENT_TYPES: [&str; 2] = [DOC_REQUISITION, DOC_PURCHASE_ORDER];

/// Name of the locked role that receives the seeded unlimited limit.
const ADMINISTRATOR_ROLE: &str = "Administrator";

/// Sync identity of the seeded "Administrator: unlimited" row of a document type: the same on
/// every device, so two devices never hold two copies of it.
pub fn seeded_limit_sync_id(document_type: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id(
        "procurement_approval_limits",
        &format!("{ADMINISTRATOR_ROLE}:{document_type}"),
    )
}

/// Seeds, once per document type, the active unlimited limit of the Administrator role. A row
/// that already exists for that role and type (whatever the administrator made of it: capped,
/// deactivated) is never touched again. Returns the number of rows created. Called by migration
/// 269 (existing databases) and by the startup seeder (new databases: the role is created there).
pub async fn seed_administrator_unlimited_limits<C: ConnectionTrait>(db: &C) -> AppResult<u64> {
    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut created = 0;
    for document_type in DOCUMENT_TYPES {
        let sync_id = seeded_limit_sync_id(document_type);
        created += db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO procurement_approval_limits
                    (entity_sync_id, role_id, document_type, max_amount, is_active, row_version, created_by_id,
                     created_at, updated_at)
                 SELECT ?, r.id, ?, NULL, 1, 1, NULL, ?, ?
                 FROM roles r
                 WHERE r.name = ? AND r.deleted_at IS NULL
                   AND NOT EXISTS (SELECT 1 FROM procurement_approval_limits l
                                   WHERE l.role_id = r.id AND l.document_type = ?)
                   AND NOT EXISTS (SELECT 1 FROM procurement_approval_limits l WHERE l.entity_sync_id = ?)",
                [
                    sync_id.clone().into(),
                    document_type.into(),
                    now.clone().into(),
                    now.clone().into(),
                    ADMINISTRATOR_ROLE.into(),
                    document_type.into(),
                    sync_id.into(),
                ],
            ))
            .await?
            .rows_affected();
    }
    Ok(created)
}

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
        is_seeded_default: {
            let document_type: String = row.try_get("", "document_type")?;
            let sync_id: String = row.try_get("", "entity_sync_id")?;
            sync_id == seeded_limit_sync_id(&document_type)
        },
    })
}

const LIMIT_SELECT: &str =
    "SELECT l.id, l.entity_sync_id, l.role_id, r.name AS role_name, l.document_type, l.max_amount,
            l.is_active, l.row_version, l.updated_at
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
    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
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

/// Largest approval limit the user holds for a document type through role assignments in force:
/// `Ok(None)` unlimited, `Ok(Some(x))` capped at x, or an error when none of the user's roles has
/// an active limit for that type (fail closed: also when no limit is configured at all).
async fn approver_limit<C: ConnectionTrait>(db: &C, user_id: i64, document_type: &str) -> AppResult<Option<f64>> {
    // The shared rule and clock: stored bounds are `YYYY-MM-DDTHH:MM:SSZ`, which
    // `datetime('now')` (`YYYY-MM-DD HH:MM:SS`) does not compare with, so a lapsed grant used
    // to keep its limit until midnight.
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(crate::rbac::resolver::in_force_binds(&now));
    values.push(document_type.into());
    values.push(crate::rbac::permissions::INV_APPROVE.into());
    // Only the limits of roles that carry the approval right count: an unlimited limit left on a
    // role without the right never widens another role's limit.
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT l.max_amount FROM procurement_approval_limits l
                 JOIN user_scope_assignments usa ON usa.role_id = l.role_id
                 WHERE usa.user_id = ? AND {}
                   AND l.document_type = ? AND l.is_active = 1
                   AND EXISTS (SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id
                               WHERE rp.role_id = l.role_id AND p.name = ?)",
                crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            values,
        ))
        .await?;
    if rows.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_NO_APPROVAL_LIMIT: none of your roles has an approval limit for a {}; the administrator \
             sets the limits per role (Approval limits).",
            label(document_type)
        )]));
    }
    let mut best = 0.0_f64;
    for row in rows {
        match row.try_get::<Option<f64>>("", "max_amount")? {
            None => return Ok(None),
            Some(v) => best = best.max(v),
        }
    }
    Ok(Some(best))
}

/// The approval right (`inv.approve`), held through any role assignment in force. Checked here as
/// well as by the commands, so that no caller of the procurement transitions approves or rejects
/// without it.
pub async fn ensure_approval_right<C: ConnectionTrait>(db: &C, approver_id: Option<i64>) -> AppResult<i64> {
    let Some(approver) = approver_id else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_APPROVER_REQUIRED: An approval needs a signed-in approver.".to_string(),
        ]));
    };
    let held = crate::rbac::resolver::all_scope_permissions(db, approver).await?;
    if !held.contains(crate::rbac::permissions::INV_APPROVE) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_APPROVAL_RIGHT: approving or rejecting a purchase needs the right '{}'.",
            crate::rbac::permissions::INV_APPROVE
        )]));
    }
    Ok(approver)
}

async fn document_amount<C: ConnectionTrait>(
    db: &C,
    document_type: &str,
    document_id: i64,
) -> AppResult<DocumentAmount> {
    if document_type == DOC_PURCHASE_ORDER {
        purchase_order_amount(db, document_id).await
    } else {
        requisition_amount(db, document_id).await
    }
}

/// The roles that may approve this document: they hold `inv.approve` and an active limit for its
/// type that covers its amount (or is unlimited). Ordered by name.
pub async fn approver_roles(
    db: &DatabaseConnection,
    document_type: &str,
    document_id: i64,
) -> AppResult<ProcurementApproverRoles> {
    ensure_document_type(document_type)?;
    let amount = document_amount(db, document_type, document_id).await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT r.name AS role_name
             FROM procurement_approval_limits l
             JOIN roles r ON r.id = l.role_id
             WHERE l.document_type = ? AND l.is_active = 1 AND r.deleted_at IS NULL
               AND (l.max_amount IS NULL OR (? IS NOT NULL AND l.max_amount + 0.000001 >= ?))
               AND EXISTS (SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id
                           WHERE rp.role_id = r.id AND p.name = ?)
             ORDER BY r.name",
            [
                document_type.into(),
                amount.amount.map_or(Value::Double(None), Value::from),
                amount.amount.map_or(Value::Double(None), Value::from),
                crate::rbac::permissions::INV_APPROVE.into(),
            ],
        ))
        .await?;
    let role_names = rows
        .iter()
        .map(|r| r.try_get::<String>("", "role_name"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProcurementApproverRoles {
        document_type: document_type.to_string(),
        document_id,
        amount: amount.amount,
        amount_unknown_reason: amount.unknown_reason,
        role_names,
    })
}

fn label(document_type: &str) -> &'static str {
    if document_type == DOC_PURCHASE_ORDER {
        "purchase order"
    } else {
        "requisition"
    }
}

/// Checks the approval right, separation of duties and the approver's limit before an approval.
pub async fn ensure_can_approve<C: ConnectionTrait>(
    db: &C,
    document_type: &str,
    document_id: i64,
    author_id: Option<i64>,
    approver_id: Option<i64>,
) -> AppResult<()> {
    let approver = ensure_approval_right(db, approver_id).await?;
    if author_id == Some(approver) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PROC_SELF_APPROVAL: you created this {}; another person must approve it.",
            label(document_type)
        )]));
    }
    let Some(max) = approver_limit(db, approver, document_type).await? else {
        return Ok(()); // unlimited role
    };
    let amount = document_amount(db, document_type, document_id).await?;
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
