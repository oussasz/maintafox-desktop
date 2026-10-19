use crate::errors::AppResult;
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::json;

use crate::sync::capture::{begin_suppression, end_suppression};
use crate::sync::identity::{new_entity_sync_id, optional_entity_sync_id, seeded_entity_sync_id};

const EPS_HOURS: f64 = 0.25;

fn map_err(col: &str, e: sea_orm::DbErr) -> crate::errors::AppError {
    crate::errors::AppError::Internal(anyhow::anyhow!("data_integrity detector {col}: {e}"))
}

async fn insert_finding(
    db: &impl ConnectionTrait,
    severity: &str,
    domain: &str,
    record_class: &str,
    record_id: i64,
    finding_code: &str,
    other_record_id: Option<i64>,
    details: serde_json::Value,
) -> AppResult<()> {
    let esid = finding_sync_id(db, finding_code, record_class, record_id, other_record_id).await?;
    let details_json = details.to_string();
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO data_integrity_findings (entity_sync_id, row_version, severity, domain, \
         record_class, record_id, finding_code, details_json, detected_at, status) \
         VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, 'open')          ON CONFLICT(entity_sync_id) DO NOTHING",
        [
            esid.into(),
            severity.into(),
            domain.into(),
            record_class.into(),
            record_id.into(),
            finding_code.into(),
            details_json.into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Table holding the records a finding of `record_class` points at.
fn record_table(record_class: &str) -> &str {
    match record_class {
        "wo_failure_details" => "work_order_failure_details",
        other => other,
    }
}

/// Identity of a finding: the same check on the same records gives the same id on every device,
/// so a finding a person waived or repaired on one device is not detected again as a new one on
/// another. A record without a sync identity (capture not installed) gets a random id.
async fn finding_sync_id(
    db: &impl ConnectionTrait,
    finding_code: &str,
    record_class: &str,
    record_id: i64,
    other_record_id: Option<i64>,
) -> AppResult<String> {
    let table = record_table(record_class);
    let mut subjects = Vec::new();
    for id in std::iter::once(record_id).chain(other_record_id) {
        match optional_entity_sync_id(db, table, id).await? {
            Some(esid) => subjects.push(esid),
            None => return Ok(new_entity_sync_id()),
        }
    }
    subjects.sort();
    Ok(seeded_entity_sync_id(
        "data_integrity_findings",
        &format!("{finding_code}|{record_class}|{}", subjects.join("|")),
    ))
}

/// Clears prior open findings for detector domains, then re-runs checks.
/// SQL condition: `id_expr` is a value of the governed failure-mode list `WORK.FAILURE_MODES`, in
/// any version of the list (a value of a superseded or deactivated version is still the valid
/// coding of the records saved against it). Since migration 114 `failure_mode_id` holds these
/// reference values (close-out checks the same domain), not `failure_codes` ids.
pub(crate) fn governed_failure_mode_sql(id_expr: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM reference_values rv \
         JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE rv.id = {id_expr} AND UPPER(TRIM(rd.code)) = 'WORK.FAILURE_MODES')"
    )
}

pub async fn run_data_integrity_detectors(db: &DatabaseConnection) -> AppResult<i64> {
    let txn = db.begin().await?;
    // Findings are recomputed on every device that runs the checks: the run itself is not sent.
    // A person's waiver or repair of a finding is captured (and sent) like any other change.
    begin_suppression(&txn).await?;
    txn.execute(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM data_integrity_findings WHERE status = 'open' \
         AND domain IN ('wo_closeout', 'downtime', 'failure_coding', 'permit_site')"
            .to_string(),
    ))
    .await?;

    let mut n: i64 = 0;

    let rows = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT fd.id AS fd_id, fd.work_order_id, fd.failure_mode_id \
                 FROM work_order_failure_details fd \
                 WHERE fd.failure_mode_id IS NOT NULL AND NOT {}",
                governed_failure_mode_sql("fd.failure_mode_id")
            ),
        ))
        .await?;
    for row in &rows {
        let fd_id: i64 = row.try_get("", "fd_id").map_err(|e| map_err("fd_id", e))?;
        let wo_id: i64 = row
            .try_get("", "work_order_id")
            .map_err(|e| map_err("work_order_id", e))?;
        let fmid: i64 = row
            .try_get("", "failure_mode_id")
            .map_err(|e| map_err("failure_mode_id", e))?;
        insert_finding(
            &txn,
            "warning",
            "failure_coding",
            "wo_failure_details",
            fd_id,
            "FK_ORPHAN_FAILURE_MODE",
            None,
            json!({"work_order_id": wo_id, "failure_mode_id": fmid}),
        )
        .await?;
        n += 1;
    }

    let neg = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, work_order_id FROM work_order_downtime_segments \
             WHERE ended_at IS NOT NULL AND started_at IS NOT NULL \
             AND julianday(ended_at) < julianday(started_at)"
                .to_string(),
        ))
        .await?;
    for row in &neg {
        let sid: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let wo_id: i64 = row
            .try_get("", "work_order_id")
            .map_err(|e| map_err("work_order_id", e))?;
        insert_finding(
            &txn,
            "error",
            "downtime",
            "work_order_downtime_segments",
            sid,
            "WO_DOWNTIME_NEGATIVE_DURATION",
            None,
            json!({"work_order_id": wo_id, "segment_id": sid}),
        )
        .await?;
        n += 1;
    }

    let unclosed = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, work_order_id FROM work_order_downtime_segments WHERE ended_at IS NULL".to_string(),
        ))
        .await?;
    for row in &unclosed {
        let sid: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let wo_id: i64 = row
            .try_get("", "work_order_id")
            .map_err(|e| map_err("work_order_id", e))?;
        insert_finding(
            &txn,
            "warning",
            "downtime",
            "work_order_downtime_segments",
            sid,
            "WO_DOWNTIME_UNCLOSED",
            None,
            json!({"work_order_id": wo_id, "segment_id": sid}),
        )
        .await?;
        n += 1;
    }

    let wo_rows = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, actual_start, actual_end FROM work_orders \
             WHERE actual_start IS NOT NULL AND actual_end IS NOT NULL"
                .to_string(),
        ))
        .await?;
    for row in &wo_rows {
        let wo_id: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let a_start: String = row
            .try_get("", "actual_start")
            .map_err(|e| map_err("actual_start", e))?;
        let a_end: String = row.try_get("", "actual_end").map_err(|e| map_err("actual_end", e))?;
        let win_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ROUND((JULIANDAY(?) - JULIANDAY(?)) * 24, 4) AS w",
                [a_end.clone().into(), a_start.clone().into()],
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("win")))?;
        let window_h: f64 = win_row.try_get::<Option<f64>>("", "w").ok().flatten().unwrap_or(0.0);

        let sum_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(SUM(\
                    (JULIANDAY(COALESCE(ended_at, started_at)) - JULIANDAY(started_at)) * 24\
                 ), 0) AS s FROM work_order_downtime_segments WHERE work_order_id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("sum")))?;
        let seg_sum: f64 = sum_row.try_get::<Option<f64>>("", "s").ok().flatten().unwrap_or(0.0);

        if seg_sum > window_h + EPS_HOURS && window_h >= 0.0 {
            insert_finding(
                &txn,
                "warning",
                "downtime",
                "work_orders",
                wo_id,
                "WO_DOWNTIME_SUM_EXCEEDS_WINDOW",
                None,
                json!({
                    "work_order_id": wo_id,
                    "segment_sum_hours": seg_sum,
                    "window_hours": window_h,
                    "epsilon_hours": EPS_HOURS
                }),
            )
            .await?;
            n += 1;
        }
    }

    let ov = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT a.id AS a_id, a.work_order_id AS wo_id, b.id AS b_id \
             FROM work_order_downtime_segments a \
             JOIN work_order_downtime_segments b \
               ON a.work_order_id = b.work_order_id AND a.id < b.id \
             WHERE a.ended_at IS NOT NULL AND b.ended_at IS NOT NULL \
               AND a.started_at < b.ended_at AND b.started_at < a.ended_at"
                .to_string(),
        ))
        .await?;
    for row in &ov {
        let a_id: i64 = row.try_get("", "a_id").map_err(|e| map_err("a_id", e))?;
        let wo_id: i64 = row.try_get("", "wo_id").map_err(|e| map_err("wo_id", e))?;
        let b_id: i64 = row.try_get("", "b_id").map_err(|e| map_err("b_id", e))?;
        insert_finding(
            &txn,
            "warning",
            "downtime",
            "work_order_downtime_segments",
            a_id,
            "WO_DOWNTIME_OVERLAP",
            Some(b_id),
            json!({"work_order_id": wo_id, "segment_id_a": a_id, "segment_id_b": b_id}),
        )
        .await?;
        n += 1;
    }

    n += permit_site_findings(&txn).await?;

    end_suppression(&txn).await?;
    txn.commit().await?;
    Ok(n)
}

/// Permit statuses checked by [`permit_site_findings`]: approved, issued and still in force.
const PERMIT_SITE_CHECKED_STATUSES_SQL: &str = "('approved', 'issued', 'active', 'suspended', 'revalidation_required')";

/// Permits approved, issued or in force whose site is not the one their equipment and work order
/// give them (read-only: never rewritten, CMMS Advisor decision A, point 5), judged by the rule of
/// the creation, an edit and a permit received by sync (`permit::queries::derived_permit_node` and
/// `node_on_permit_chain`): the work order's node when it records one, else the equipment's, or a
/// level above it. Listed: another site, a level below, a sibling, or a site deleted since. Before a
/// permit's site followed its equipment, an edit could change the equipment and keep the former
/// site. A permit without a site, or whose equipment and work order give no usable node (none, or
/// a node deleted or missing here), is not judged here (the permit lists count permits without a
/// site).
async fn permit_site_findings(db: &impl ConnectionTrait) -> AppResult<i64> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT wp.id, wp.code, wp.status, wp.entity_id, wp.asset_id, wp.linked_work_order_id, \
                 e.asset_id_code, e.installed_at_node_id \
                 FROM work_permits wp JOIN equipment e ON e.id = wp.asset_id \
                 WHERE wp.status IN {PERMIT_SITE_CHECKED_STATUSES_SQL} AND wp.entity_id > 0"
            ),
        ))
        .await?;
    let mut n = 0;
    for row in &rows {
        let permit_id: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let permit_node: i64 = row.try_get("", "entity_id").map_err(|e| map_err("entity_id", e))?;
        let asset_id: i64 = row.try_get("", "asset_id").map_err(|e| map_err("asset_id", e))?;
        let linked_wo: Option<i64> = row
            .try_get("", "linked_work_order_id")
            .map_err(|e| map_err("linked_work_order_id", e))?;
        let equipment_node: Option<i64> = row
            .try_get("", "installed_at_node_id")
            .map_err(|e| map_err("installed_at_node_id", e))?;
        let derived = match crate::permit::queries::derived_permit_node(db, asset_id, linked_wo).await {
            Ok(Some(node)) => node,
            // No usable node, or a linked work order that no longer exists: not judged here.
            Ok(None) | Err(crate::errors::AppError::NotFound { .. }) => continue,
            Err(e) => return Err(e),
        };
        // Equipment or work order on a node deleted (or missing) here: no usable node, not judged
        // either (the creation and an edit refuse such a permit as "site undetermined").
        if !crate::permit::queries::live_org_node(db, derived).await? {
            continue;
        }
        if crate::permit::queries::node_on_permit_chain(db, derived, permit_node).await? {
            continue;
        }
        let code: String = row.try_get("", "code").map_err(|e| map_err("code", e))?;
        let status: String = row.try_get("", "status").map_err(|e| map_err("status", e))?;
        let asset_code: Option<String> = row
            .try_get("", "asset_id_code")
            .map_err(|e| map_err("asset_id_code", e))?;
        insert_finding(
            db,
            "warning",
            "permit_site",
            "work_permits",
            permit_id,
            "PERMIT_SITE_NOT_EQUIPMENT_SITE",
            None,
            json!({
                "permit_id": permit_id,
                "permit_code": code,
                "status": status,
                "permit_entity_id": permit_node,
                "equipment_id": asset_id,
                "equipment_code": asset_code,
                "equipment_entity_id": equipment_node,
                "linked_work_order_id": linked_wo,
                "derived_entity_id": derived,
            }),
        )
        .await?;
        n += 1;
    }
    Ok(n)
}
