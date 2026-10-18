//! Permit and parts readiness of a planned job, read from the source of truth (SAP PM: material
//! availability check and permits on the order; Maximo: work plan materials and the work
//! permit), explained finding by finding.
//!
//! - Permit: whether the job needs one (the work order's `requires_permit`, the PM plan's), and
//!   the state of the live permit linked to the work order and its isolations (LOTO).
//! - Parts: each planned line with an article, in stock units (`work_order_parts` quantities are
//!   kept in the article's stock unit; a PM version lists quantities in stock units), against what
//!   is reserved for it and what is available in stock. Lines not used are not counted; a line
//!   without an article or without a quantity is reported, never counted against stock.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("planning readiness decode '{field}': {err}"))
}

/// One readiness finding: what it is and the details that explain it.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub conflict_type: &'static str,
    pub reason_code: &'static str,
    pub details: Value,
}

// ── Permits ──────────────────────────────────────────────────────────────────────────────

/// Statuses in which a permit is being prepared (not yet issued).
const PERMIT_IN_PREPARATION: &[&str] = &["draft", "pending_review"];
/// Statuses in which a permit is issued and can be activated on the day.
const PERMIT_ISSUED: &[&str] = &["approved", "issued"];
/// Statuses in which a permit stopped protecting work until revalidated.
const PERMIT_ON_HOLD: &[&str] = &["suspended", "revalidation_required"];

/// Whether the job needs a permit: the work order's flag, or the PM plan's before its work
/// order exists (`None` = the job is not known).
async fn permit_required(
    db: &impl ConnectionTrait,
    source_type: &str,
    source_id: i64,
) -> AppResult<Option<(bool, Option<i64>)>> {
    let sql = match source_type {
        "work_order" => "SELECT requires_permit AS required, id AS wo_id FROM work_orders WHERE id = ?",
        "pm_occurrence" => {
            "SELECT COALESCE(wo.requires_permit, pp.requires_permit) AS required, po.linked_work_order_id AS wo_id \
               FROM pm_occurrences po JOIN pm_plans pp ON pp.id = po.pm_plan_id \
               LEFT JOIN work_orders wo ON wo.id = po.linked_work_order_id WHERE po.id = ?"
        }
        _ => return Ok(None),
    };
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [source_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let required: i64 = row.try_get("", "required").map_err(|e| decode_err("required", e))?;
    let wo_id: Option<i64> = row.try_get("", "wo_id").map_err(|e| decode_err("wo_id", e))?;
    Ok(Some((required == 1, wo_id)))
}

/// The permit findings of a job whose work ends at `work_end` (`None` when unknown). Nothing
/// when no permit is required.
pub async fn permit_findings(
    db: &impl ConnectionTrait,
    source_type: &str,
    source_id: i64,
    work_end: Option<&str>,
) -> AppResult<Vec<Finding>> {
    let Some((true, wo_id)) = permit_required(db, source_type, source_id).await? else {
        return Ok(Vec::new());
    };
    let Some(wo_id) = wo_id else {
        // A PM whose work order is not generated yet: the permit is requested on that order.
        return Ok(vec![Finding {
            conflict_type: "permit_not_ready",
            reason_code: "PERMIT_TO_REQUEST",
            details: json!({ "on_work_order": false }),
        }]);
    };
    // Every live permit of the work order counts, as at OT start (`permit::wo_gate`).
    let permits = crate::permit::queries::list_live_permits_linked_to_work_order(db, wo_id).await?;
    if permits.is_empty() {
        return Ok(vec![Finding {
            conflict_type: "permit_not_ready",
            reason_code: "PERMIT_TO_REQUEST",
            details: json!({ "on_work_order": true, "work_order_id": wo_id }),
        }]);
    }
    let mut out = Vec::new();
    for permit in &permits {
        out.extend(one_permit_findings(db, permit, wo_id, work_end).await?);
    }
    Ok(out)
}

async fn one_permit_findings(
    db: &impl ConnectionTrait,
    permit: &crate::permit::domain::WorkPermit,
    wo_id: i64,
    work_end: Option<&str>,
) -> AppResult<Vec<Finding>> {
    let base = json!({
        "permit_id": permit.id,
        "permit_code": permit.code,
        "status": permit.status,
        "expires_at": permit.expires_at,
        "work_order_id": wo_id,
    });
    let status = permit.status.as_str();
    let mut out = Vec::new();
    if PERMIT_ON_HOLD.contains(&status) {
        out.push(Finding {
            conflict_type: "permit_not_ready",
            reason_code: "PERMIT_ON_HOLD",
            details: base.clone(),
        });
    } else if PERMIT_IN_PREPARATION.contains(&status) || (!PERMIT_ISSUED.contains(&status) && status != "active") {
        out.push(Finding {
            conflict_type: "permit_not_ready",
            reason_code: "PERMIT_IN_PREPARATION",
            details: base.clone(),
        });
    }
    // Valid until the end of the work (both compared as instants).
    if let (Some(expires), Some(end)) = (permit.expires_at.as_deref(), work_end) {
        if let (Some(expires_at), Some(end_at)) = (
            crate::wo::shifts::planned_instant(expires),
            crate::wo::shifts::planned_instant(end),
        ) {
            if expires_at < end_at {
                out.push(Finding {
                    conflict_type: "permit_not_ready",
                    reason_code: "PERMIT_EXPIRES_BEFORE_WORK",
                    details: base.clone(),
                });
            }
        }
    }
    // LOTO: every isolation point applied and verified (two-person) before the work starts; a
    // permit type that requires energy isolation (`permit_types.requires_energy_isolation`, set
    // for LOTO) needs at least one. Read with the OT start gate's own function: a permit whose
    // type is missing can never start, so it is reported (blocking), never taken as "no isolation".
    let requires_isolation =
        match crate::permit::queries::permit_type_requires_energy_isolation(db, permit.permit_type_id).await {
            Ok(requires) => requires,
            Err(AppError::NotFound { .. }) => {
                out.push(Finding {
                    conflict_type: "permit_not_ready",
                    reason_code: "PERMIT_TYPE_MISSING",
                    details: base,
                });
                return Ok(out);
            }
            Err(err) => return Err(err),
        };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT (SELECT COUNT(*) FROM permit_isolations pi WHERE pi.permit_id = ?) AS isolations, \
                    (SELECT COUNT(*) FROM permit_isolations pi WHERE pi.permit_id = ? \
                        AND pi.verified_at IS NOT NULL AND pi.removal_verified_at IS NULL) AS verified",
            [permit.id.into(), permit.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("permit {} isolation counts missing", permit.id)))?;
    let isolations: i64 = row.try_get("", "isolations").map_err(|e| decode_err("isolations", e))?;
    let verified: i64 = row.try_get("", "verified").map_err(|e| decode_err("verified", e))?;
    if (requires_isolation && isolations == 0) || verified < isolations {
        out.push(Finding {
            conflict_type: "permit_not_ready",
            reason_code: "LOTO_NOT_VERIFIED",
            details: json!({
                "permit_id": permit.id,
                "permit_code": permit.code,
                "isolations": isolations,
                "verified": verified,
            }),
        });
    }
    Ok(out)
}

// ── Parts ────────────────────────────────────────────────────────────────────────────────

/// Stock available for an article over all locations (stock unit).
async fn available_stock(db: &impl ConnectionTrait, article_id: i64) -> AppResult<f64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(available_qty), 0.0) AS available FROM stock_balances WHERE article_id = ?",
            [article_id.into()],
        ))
        .await?;
    match row {
        Some(r) => r.try_get("", "available").map_err(|e| decode_err("available", e)),
        None => Ok(0.0),
    }
}

/// A line to check: the article, what is still needed, what is reserved for it.
struct PartNeed {
    article_id: Option<i64>,
    label: String,
    needed: Option<f64>,
    reserved: f64,
    /// Short and required: blocks; short and optional: warns.
    required: bool,
}

async fn work_order_part_needs(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<PartNeed>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wop.article_id, COALESCE(a.article_code, wop.article_ref, '') AS label, \
                    wop.quantity_planned AS planned, COALESCE(wop.quantity_issued, 0.0) AS issued, \
                    COALESCE(wop.quantity_reserved, 0.0) AS reserved, wop.is_required \
               FROM work_order_parts wop LEFT JOIN articles a ON a.id = wop.article_id \
              WHERE wop.work_order_id = ? AND wop.consumption_status <> 'not_used' \
                AND COALESCE(wop.quantity_planned, 0) > 0 \
              ORDER BY wop.id",
            [wo_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let planned: f64 = r.try_get("", "planned").map_err(|e| decode_err("planned", e))?;
            let issued: f64 = r.try_get("", "issued").map_err(|e| decode_err("issued", e))?;
            let reserved: f64 = r.try_get("", "reserved").map_err(|e| decode_err("reserved", e))?;
            Ok(PartNeed {
                article_id: r.try_get("", "article_id").map_err(|e| decode_err("article_id", e))?,
                label: r.try_get("", "label").map_err(|e| decode_err("label", e))?,
                // What is already issued is no longer needed from stock or reservations.
                needed: Some((planned - issued).max(0.0)),
                reserved: (reserved - issued).max(0.0),
                required: r
                    .try_get::<i64>("", "is_required")
                    .map_err(|e| decode_err("is_required", e))?
                    == 1,
            })
        })
        .collect()
}

/// A PM occurrence's parts: its work order's once generated, else its plan version's list.
enum PmParts {
    WorkOrder(i64),
    Listed(Vec<PartNeed>),
}

async fn pm_part_needs(db: &impl ConnectionTrait, occurrence_id: i64) -> AppResult<Option<PmParts>> {
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.linked_work_order_id, pv.required_parts_json FROM pm_occurrences po \
               LEFT JOIN pm_plan_versions pv ON pv.id = po.plan_version_id WHERE po.id = ?",
            [occurrence_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    if let Some(wo_id) = row
        .try_get::<Option<i64>>("", "linked_work_order_id")
        .map_err(|e| decode_err("linked_work_order_id", e))?
    {
        return Ok(Some(PmParts::WorkOrder(wo_id)));
    }
    let raw: Option<String> = row
        .try_get("", "required_parts_json")
        .map_err(|e| decode_err("required_parts_json", e))?;
    let mut needs = Vec::new();
    for part in crate::pm::wo_content::parse_required_parts(raw.as_deref()) {
        let article = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM articles WHERE UPPER(TRIM(article_code)) = UPPER(TRIM(?)) AND is_active = 1 LIMIT 1",
                [part.code.clone().into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
            .transpose()?;
        needs.push(PartNeed {
            article_id: article,
            label: part.code,
            needed: part.quantity,
            // Reservations are made when the PM's work order is generated.
            reserved: 0.0,
            required: part.required,
        });
    }
    Ok(Some(PmParts::Listed(needs)))
}

/// The parts findings of a job: short in stock, available but not reserved, without an
/// article, or without a quantity.
pub async fn parts_findings(db: &impl ConnectionTrait, source_type: &str, source_id: i64) -> AppResult<Vec<Finding>> {
    let needs = match source_type {
        "work_order" => work_order_part_needs(db, source_id).await?,
        "pm_occurrence" => match pm_part_needs(db, source_id).await? {
            None => return Ok(Vec::new()),
            Some(PmParts::WorkOrder(wo_id)) => work_order_part_needs(db, wo_id).await?,
            Some(PmParts::Listed(needs)) => needs,
        },
        _ => return Ok(Vec::new()),
    };
    let mut short = Vec::new();
    let mut short_optional = Vec::new();
    let mut unreserved = Vec::new();
    let mut unknown = Vec::new();
    let mut no_quantity = Vec::new();
    for need in needs {
        let Some(article_id) = need.article_id else {
            unknown.push(need.label);
            continue;
        };
        let Some(needed) = need.needed else {
            no_quantity.push(need.label);
            continue;
        };
        if need.reserved >= needed {
            continue;
        }
        let missing = needed - need.reserved;
        let available = available_stock(db, article_id).await?;
        if available >= missing {
            unreserved.push(json!({
                "article": need.label,
                "article_id": article_id,
                "to_reserve": missing,
                "available": available,
            }));
        } else {
            let line = json!({
                "article": need.label,
                "article_id": article_id,
                "needed": needed,
                "reserved": need.reserved,
                "available": available,
                "short": missing - available,
            });
            if need.required {
                short.push(line);
            } else {
                short_optional.push(line);
            }
        }
    }
    let mut out = Vec::new();
    if !short.is_empty() {
        let articles: Vec<Value> = short.iter().map(|s| s["article"].clone()).collect();
        out.push(Finding {
            conflict_type: "missing_critical_part",
            reason_code: "PARTS_SHORT",
            details: json!({ "articles": articles, "lines": short }),
        });
    }
    if !short_optional.is_empty() {
        // An optional part that is short only warns (`planning::queries::evaluate_candidate`
        // blocks on PARTS_SHORT alone).
        let articles: Vec<Value> = short_optional.iter().map(|s| s["article"].clone()).collect();
        out.push(Finding {
            conflict_type: "missing_critical_part",
            reason_code: "PARTS_SHORT_OPTIONAL",
            details: json!({ "articles": articles, "lines": short_optional }),
        });
    }
    if !unreserved.is_empty() {
        let articles: Vec<Value> = unreserved.iter().map(|s| s["article"].clone()).collect();
        out.push(Finding {
            conflict_type: "missing_critical_part",
            reason_code: "PARTS_NOT_RESERVED",
            details: json!({ "articles": articles, "lines": unreserved }),
        });
    }
    if !unknown.is_empty() {
        out.push(Finding {
            conflict_type: "missing_critical_part",
            reason_code: "PART_NOT_IN_CATALOGUE",
            details: json!({ "articles": unknown }),
        });
    }
    if !no_quantity.is_empty() {
        out.push(Finding {
            conflict_type: "missing_critical_part",
            reason_code: "PART_QUANTITY_MISSING",
            details: json!({ "articles": no_quantity }),
        });
    }
    Ok(out)
}
