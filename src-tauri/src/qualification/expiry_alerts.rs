use chrono::{Datelike, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    StageOutboxItemInput, TrainingExpiryAlertEventSyncPayload, SYNC_ENTITY_TRAINING_EXPIRY_ALERT_EVENTS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    CertificationExpiryDrilldownRow, TrainingExpiryAlertEvent, TrainingExpiryAlertEventListFilter,
};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("expiry_alerts decode '{field}': {err}"))
}

fn week_bucket(d: NaiveDate) -> String {
    let iso = d.iso_week();
    format!("{}-W{:02}", iso.year(), iso.week())
}

fn hash_dedupe(personnel_id: i64, certification_type_id: i64, week_bucket: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("{personnel_id}:{certification_type_id}:{week_bucket}").as_bytes());
    format!("{:x}", h.finalize())
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    let p: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&p, "%Y-%m-%d").ok()
}

fn today() -> NaiveDate {
    Utc::now().date_naive()
}

async fn has_active_work_order_for_personnel(db: &DatabaseConnection, personnel_id: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_orders wo \
             INNER JOIN user_accounts ua ON ua.id = wo.primary_responsible_id \
             WHERE ua.personnel_id = ? AND ua.deleted_at IS NULL \
             AND wo.closed_at IS NULL AND wo.cancelled_at IS NULL",
            [personnel_id.into()],
        ))
        .await?;
    let c: i64 = row
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count")))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    Ok(c > 0)
}

fn map_event(row: &sea_orm::QueryResult) -> AppResult<TrainingExpiryAlertEvent> {
    Ok(TrainingExpiryAlertEvent {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        certification_id: row
            .try_get("", "certification_id")
            .map_err(|e| decode_err("certification_id", e))?,
        alert_dedupe_key: row
            .try_get("", "alert_dedupe_key")
            .map_err(|e| decode_err("alert_dedupe_key", e))?,
        fired_at: row.try_get("", "fired_at").map_err(|e| decode_err("fired_at", e))?,
        severity: row.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

async fn stage_event(db: &DatabaseConnection, row: &TrainingExpiryAlertEvent) -> AppResult<()> {
    let payload = TrainingExpiryAlertEventSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        certification_id: row.certification_id,
        alert_dedupe_key: row.alert_dedupe_key.clone(),
        fired_at: row.fired_at.clone(),
        severity: row.severity.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "training_expiry_alert_events:{}:v{}",
                row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_TRAINING_EXPIRY_ALERT_EVENTS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

async fn get_event_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<TrainingExpiryAlertEvent>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, certification_id, alert_dedupe_key, fired_at, severity, row_version \
             FROM training_expiry_alert_events WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_event(&r)?)),
        None => Ok(None),
    }
}

pub async fn list_training_expiry_alert_events(
    db: &DatabaseConnection,
    filter: TrainingExpiryAlertEventListFilter,
) -> AppResult<Vec<TrainingExpiryAlertEvent>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 2000);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, certification_id, alert_dedupe_key, fired_at, severity, row_version \
         FROM training_expiry_alert_events WHERE 1=1",
    );
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(ref sev) = filter.severity {
        sql.push_str(" AND severity = ?");
        params.push(sev.clone().into());
    }
    sql.push_str(" ORDER BY fired_at DESC LIMIT ?");
    params.push(limit.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_event(&r)?);
    }
    Ok(out)
}

pub async fn scan_training_expiry_alerts(db: &DatabaseConnection, lookahead_days: i64) -> AppResult<Vec<TrainingExpiryAlertEvent>> {
    let lookahead = lookahead_days.clamp(1, 730);
    let horizon = today() + chrono::Duration::days(lookahead);

    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT pc.id, pc.personnel_id, pc.certification_type_id, pc.expires_at, pc.verification_status \
             FROM personnel_certifications pc \
             WHERE pc.expires_at IS NOT NULL AND pc.expires_at != ''"
                .to_string(),
        ))
        .await?;

    let wb = week_bucket(today());
    let now_ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut created: Vec<TrainingExpiryAlertEvent> = Vec::new();

    for r in rows {
        let cert_id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let personnel_id: i64 = r.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?;
        let cert_type_id: i64 = r
            .try_get("", "certification_type_id")
            .map_err(|e| decode_err("certification_type_id", e))?;
        let exp_s: String = r.try_get("", "expires_at").map_err(|e| decode_err("expires_at", e))?;
        let verification_status: String = r
            .try_get("", "verification_status")
            .map_err(|e| decode_err("verification_status", e))?;

        if verification_status == "rejected" {
            continue;
        }

        let Some(exp_d) = parse_day(&exp_s) else {
            continue;
        };

        if exp_d > horizon {
            continue;
        }

        let dedupe = hash_dedupe(personnel_id, cert_type_id, &wb);

        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM training_expiry_alert_events WHERE alert_dedupe_key = ? LIMIT 1",
                [dedupe.clone().into()],
            ))
            .await?;
        if exists.is_some() {
            continue;
        }

        let expired = exp_d < today();
        let active_wo = has_active_work_order_for_personnel(db, personnel_id).await?;

        let severity = if expired && active_wo {
            "critical"
        } else if expired {
            "warning"
        } else {
            "info"
        };

        let sync_id = Uuid::new_v4().to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO training_expiry_alert_events (entity_sync_id, certification_id, alert_dedupe_key, \
             fired_at, severity, row_version) VALUES (?, ?, ?, ?, ?, 1)",
            [
                sync_id.into(),
                cert_id.into(),
                dedupe.into(),
                now_ts.clone().into(),
                severity.into(),
            ],
        ))
        .await?;

        let id_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
        let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let row = get_event_by_id(db, new_id).await?.expect("row");
        stage_event(db, &row).await?;
        created.push(row);
    }

    Ok(created)
}

pub async fn list_certification_expiry_drilldown(
    db: &DatabaseConnection,
    entity_id: Option<i64>,
    lookahead_days: i64,
) -> AppResult<Vec<CertificationExpiryDrilldownRow>> {
    let lookahead = lookahead_days.clamp(1, 730);
    let horizon = today() + chrono::Duration::days(lookahead);
    let horizon_s = horizon.format("%Y-%m-%d").to_string();

    let mut sql = String::from(
        "SELECT pc.id AS certification_id, pc.personnel_id, p.employee_code, p.full_name, p.primary_entity_id, \
         pc.certification_type_id, ct.code AS type_code, pc.expires_at, pc.verification_status \
         FROM personnel_certifications pc \
         INNER JOIN personnel p ON p.id = pc.personnel_id \
         INNER JOIN certification_types ct ON ct.id = pc.certification_type_id \
         WHERE pc.expires_at IS NOT NULL AND pc.expires_at != '' \
         AND substr(pc.expires_at,1,10) <= ?",
    );
    let mut params: Vec<sea_orm::Value> = vec![horizon_s.into()];

    if let Some(eid) = entity_id {
        sql.push_str(" AND p.primary_entity_id = ?");
        params.push(eid.into());
    }

    sql.push_str(" ORDER BY pc.expires_at ASC LIMIT 500");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let expires_at: Option<String> = r.try_get("", "expires_at").ok();
        let verification_status: String = r
            .try_get("", "verification_status")
            .map_err(|e| decode_err("verification_status", e))?;
        let readiness_status = {
            let exp = expires_at.as_deref();
            if verification_status != "verified" {
                "pending".to_string()
            } else if let Some(e) = exp {
                if let Some(d) = parse_day(e) {
                    if d < today() {
                        "expired".to_string()
                    } else if d <= today() + chrono::Duration::days(30) {
                        "expiring_soon".to_string()
                    } else {
                        "valid".to_string()
                    }
                } else {
                    "valid".to_string()
                }
            } else {
                "valid".to_string()
            }
        };

        out.push(CertificationExpiryDrilldownRow {
            certification_id: r.try_get("", "certification_id").map_err(|e| decode_err("certification_id", e))?,
            personnel_id: r.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?,
            employee_code: r.try_get("", "employee_code").map_err(|e| decode_err("employee_code", e))?,
            full_name: r.try_get("", "full_name").map_err(|e| decode_err("full_name", e))?,
            primary_entity_id: r.try_get("", "primary_entity_id").ok(),
            certification_type_id: r
                .try_get("", "certification_type_id")
                .map_err(|e| decode_err("certification_type_id", e))?,
            certification_type_code: r.try_get("", "type_code").map_err(|e| decode_err("type_code", e))?,
            expires_at,
            verification_status,
            readiness_status,
        });
    }
    Ok(out)
}
