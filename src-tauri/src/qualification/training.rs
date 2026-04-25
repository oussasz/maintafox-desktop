use chrono::{Months, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    StageOutboxItemInput, TrainingAttendanceSyncPayload, TrainingSessionSyncPayload, SYNC_ENTITY_TRAINING_ATTENDANCE,
    SYNC_ENTITY_TRAINING_SESSIONS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    DocumentAcknowledgement, DocumentAcknowledgementListFilter, DocumentAcknowledgementUpsertInput,
    PersonnelCertificationUpsertInput, TrainingAttendance, TrainingAttendanceListFilter, TrainingAttendanceUpsertInput,
    TrainingSession, TrainingSessionUpsertInput,
};
use super::queries::{get_certification_type_by_id, upsert_personnel_certification};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode training field '{field}': {err}"))
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

fn parse_iso_date_prefix(s: &str) -> Option<NaiveDate> {
    let prefix: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&prefix, "%Y-%m-%d").ok()
}

fn expires_from_issued(issued: &str, validity_months: Option<i64>) -> Option<String> {
    let d = parse_iso_date_prefix(issued)?;
    let m = validity_months.filter(|x| *x > 0).map(|x| x as u32).unwrap_or(0);
    if m == 0 {
        return None;
    }
    d.checked_add_months(Months::new(m))
        .map(|x| x.format("%Y-%m-%d").to_string())
}

const ATT_STATUSES: &[&str] = &["registered", "attended", "passed", "failed", "no_show"];

fn validate_attendance_status(s: &str) -> AppResult<()> {
    if ATT_STATUSES.contains(&s) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "attendance_status must be one of {:?}",
            ATT_STATUSES
        )]))
    }
}

async fn stage_training_session(db: &DatabaseConnection, row: &TrainingSession) -> AppResult<()> {
    let payload = TrainingSessionSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        course_code: row.course_code.clone(),
        scheduled_start: row.scheduled_start.clone(),
        scheduled_end: row.scheduled_end.clone(),
        location: row.location.clone(),
        instructor_id: row.instructor_id,
        certification_type_id: row.certification_type_id,
        min_pass_score: row.min_pass_score,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("training_sessions:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_TRAINING_SESSIONS.to_string(),
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

async fn stage_training_attendance(db: &DatabaseConnection, row: &TrainingAttendance) -> AppResult<()> {
    let payload = TrainingAttendanceSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        session_id: row.session_id,
        personnel_id: row.personnel_id,
        attendance_status: row.attendance_status.clone(),
        completed_at: row.completed_at.clone(),
        score: row.score,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("training_attendance:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_TRAINING_ATTENDANCE.to_string(),
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

fn map_session(row: &sea_orm::QueryResult) -> AppResult<TrainingSession> {
    Ok(TrainingSession {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        course_code: row.try_get("", "course_code").map_err(|e| decode_err("course_code", e))?,
        scheduled_start: row
            .try_get("", "scheduled_start")
            .map_err(|e| decode_err("scheduled_start", e))?,
        scheduled_end: row
            .try_get("", "scheduled_end")
            .map_err(|e| decode_err("scheduled_end", e))?,
        location: row.try_get("", "location").ok(),
        instructor_id: row.try_get("", "instructor_id").ok(),
        certification_type_id: row.try_get("", "certification_type_id").ok(),
        min_pass_score: row.try_get("", "min_pass_score").map_err(|e| decode_err("min_pass_score", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_attendance(row: &sea_orm::QueryResult) -> AppResult<TrainingAttendance> {
    Ok(TrainingAttendance {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        session_id: row.try_get("", "session_id").map_err(|e| decode_err("session_id", e))?,
        personnel_id: row.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?,
        attendance_status: row
            .try_get("", "attendance_status")
            .map_err(|e| decode_err("attendance_status", e))?,
        completed_at: row.try_get("", "completed_at").ok(),
        score: row.try_get("", "score").ok(),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_ack(row: &sea_orm::QueryResult) -> AppResult<DocumentAcknowledgement> {
    Ok(DocumentAcknowledgement {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        personnel_id: row.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?,
        document_version_id: row
            .try_get("", "document_version_id")
            .map_err(|e| decode_err("document_version_id", e))?,
        acknowledged_at: row
            .try_get("", "acknowledged_at")
            .map_err(|e| decode_err("acknowledged_at", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

async fn get_session_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<TrainingSession>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, course_code, scheduled_start, scheduled_end, location, \
             instructor_id, certification_type_id, min_pass_score, row_version \
             FROM training_sessions WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_session(&r)?)),
        None => Ok(None),
    }
}

async fn maybe_issue_cert_from_pass(
    db: &DatabaseConnection,
    session: &TrainingSession,
    personnel_id: i64,
    completed_at: &Option<String>,
    score: Option<f64>,
) -> AppResult<()> {
    if session.certification_type_id.is_none() {
        return Ok(());
    }
    let min = session.min_pass_score as f64;
    let Some(sc) = score else {
        return Ok(());
    };
    if sc < min {
        return Ok(());
    }
    let ct_id = session.certification_type_id.expect("checked");
    let ct = get_certification_type_by_id(db, ct_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "CertificationType".into(),
            id: ct_id.to_string(),
        })?;

    let issued = completed_at
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    let issued_day = parse_iso_date_prefix(&issued)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    let expires = expires_from_issued(&issued_day, ct.default_validity_months);

    let cref = format!("training_session:{}", session.id);

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_version FROM personnel_certifications \
             WHERE personnel_id = ? AND certification_type_id = ? AND certificate_ref = ?",
            [personnel_id.into(), ct_id.into(), cref.clone().into()],
        ))
        .await?;

    if let Some(erow) = existing {
        let id: i64 = erow.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let rv: i64 = erow.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?;
        upsert_personnel_certification(
            db,
            PersonnelCertificationUpsertInput {
                id: Some(id),
                personnel_id,
                certification_type_id: ct_id,
                issued_at: Some(issued_day.clone()),
                expires_at: expires.clone(),
                issuing_body: Some("training_session".to_string()),
                certificate_ref: Some(cref),
                verification_status: "verified".to_string(),
                expected_row_version: Some(rv),
            },
        )
        .await?;
    } else {
        upsert_personnel_certification(
            db,
            PersonnelCertificationUpsertInput {
                id: None,
                personnel_id,
                certification_type_id: ct_id,
                issued_at: Some(issued_day),
                expires_at: expires,
                issuing_body: Some("training_session".to_string()),
                certificate_ref: Some(cref),
                verification_status: "verified".to_string(),
                expected_row_version: None,
            },
        )
        .await?;
    }
    Ok(())
}

pub async fn list_training_sessions(db: &DatabaseConnection) -> AppResult<Vec<TrainingSession>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, course_code, scheduled_start, scheduled_end, location, \
             instructor_id, certification_type_id, min_pass_score, row_version \
             FROM training_sessions ORDER BY scheduled_start DESC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_session(&r)?);
    }
    Ok(out)
}

pub async fn upsert_training_session(db: &DatabaseConnection, input: TrainingSessionUpsertInput) -> AppResult<TrainingSession> {
    if input.course_code.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["course_code is required.".into()]));
    }
    let min_pass = input.min_pass_score.unwrap_or(70).clamp(0, 100);

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let ev = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required for update.".into()]))?;
        let current = get_session_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "TrainingSession".into(),
                id: id.to_string(),
            })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec!["Row version mismatch.".into()]));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE training_sessions SET course_code = ?, scheduled_start = ?, scheduled_end = ?, \
                 location = ?, instructor_id = ?, certification_type_id = ?, min_pass_score = ?, row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.course_code.into(),
                    input.scheduled_start.into(),
                    input.scheduled_end.into(),
                    opt_string(input.location),
                    opt_i64(input.instructor_id),
                    opt_i64(input.certification_type_id),
                    min_pass.into(),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec!["Concurrent update on training_sessions.".into()]));
        }
        let updated = get_session_by_id(db, id).await?.expect("row");
        stage_training_session(db, &updated).await?;
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO training_sessions (entity_sync_id, course_code, scheduled_start, scheduled_end, \
         location, instructor_id, certification_type_id, min_pass_score, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.course_code.into(),
            input.scheduled_start.into(),
            input.scheduled_end.into(),
            opt_string(input.location),
            opt_i64(input.instructor_id),
            opt_i64(input.certification_type_id),
            min_pass.into(),
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
    let created = get_session_by_id(db, new_id).await?.expect("row");
    stage_training_session(db, &created).await?;
    Ok(created)
}

async fn get_attendance_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<TrainingAttendance>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, session_id, personnel_id, attendance_status, completed_at, score, row_version \
             FROM training_attendance WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_attendance(&r)?)),
        None => Ok(None),
    }
}

pub async fn list_training_attendance(
    db: &DatabaseConnection,
    filter: TrainingAttendanceListFilter,
) -> AppResult<Vec<TrainingAttendance>> {
    let limit = filter.limit.unwrap_or(500).clamp(1, 5000);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, session_id, personnel_id, attendance_status, completed_at, score, row_version \
         FROM training_attendance WHERE 1=1",
    );
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(sid) = filter.session_id {
        sql.push_str(" AND session_id = ?");
        params.push(sid.into());
    }
    if let Some(pid) = filter.personnel_id {
        sql.push_str(" AND personnel_id = ?");
        params.push(pid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    params.push(limit.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_attendance(&r)?);
    }
    Ok(out)
}

pub async fn upsert_training_attendance(
    db: &DatabaseConnection,
    input: TrainingAttendanceUpsertInput,
) -> AppResult<TrainingAttendance> {
    validate_attendance_status(&input.attendance_status)?;
    let session = get_session_by_id(db, input.session_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "TrainingSession".into(),
            id: input.session_id.to_string(),
        })?;

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let ev = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required for update.".into()]))?;
        let current = get_attendance_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "TrainingAttendance".into(),
                id: id.to_string(),
            })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec!["Row version mismatch.".into()]));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE training_attendance SET session_id = ?, personnel_id = ?, attendance_status = ?, \
                 completed_at = ?, score = ?, row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.session_id.into(),
                    input.personnel_id.into(),
                    input.attendance_status.clone().into(),
                    opt_string(input.completed_at.clone()),
                    opt_f64(input.score),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec!["Concurrent update on training_attendance.".into()]));
        }
        let updated = get_attendance_by_id(db, id).await?.expect("row");
        stage_training_attendance(db, &updated).await?;
        if input.attendance_status == "passed" {
            maybe_issue_cert_from_pass(
                db,
                &session,
                input.personnel_id,
                &input.completed_at,
                input.score,
            )
            .await?;
        }
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO training_attendance (entity_sync_id, session_id, personnel_id, attendance_status, \
         completed_at, score, row_version) VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.session_id.into(),
            input.personnel_id.into(),
            input.attendance_status.clone().into(),
            opt_string(input.completed_at.clone()),
            opt_f64(input.score),
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
    let created = get_attendance_by_id(db, new_id).await?.expect("row");
    stage_training_attendance(db, &created).await?;
    if input.attendance_status == "passed" {
        maybe_issue_cert_from_pass(
            db,
            &session,
            input.personnel_id,
            &input.completed_at,
            input.score,
        )
        .await?;
    }
    Ok(created)
}

async fn get_ack_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<DocumentAcknowledgement>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, personnel_id, document_version_id, acknowledged_at, row_version \
             FROM document_acknowledgements WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_ack(&r)?)),
        None => Ok(None),
    }
}

pub async fn list_document_acknowledgements(
    db: &DatabaseConnection,
    filter: DocumentAcknowledgementListFilter,
) -> AppResult<Vec<DocumentAcknowledgement>> {
    let limit = filter.limit.unwrap_or(500).clamp(1, 5000);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, personnel_id, document_version_id, acknowledged_at, row_version \
         FROM document_acknowledgements WHERE 1=1",
    );
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(pid) = filter.personnel_id {
        sql.push_str(" AND personnel_id = ?");
        params.push(pid.into());
    }
    sql.push_str(" ORDER BY acknowledged_at DESC LIMIT ?");
    params.push(limit.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_ack(&r)?);
    }
    Ok(out)
}

pub async fn upsert_document_acknowledgement(
    db: &DatabaseConnection,
    input: DocumentAcknowledgementUpsertInput,
) -> AppResult<DocumentAcknowledgement> {
    if input.document_version_id <= 0 || input.personnel_id <= 0 {
        return Err(AppError::ValidationFailed(vec!["personnel_id and document_version_id must be positive.".into()]));
    }

    if let Some(id) = input.id {
        let ev = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required for update.".into()]))?;
        let current = get_ack_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "DocumentAcknowledgement".into(),
                id: id.to_string(),
            })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec!["Row version mismatch.".into()]));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE document_acknowledgements SET personnel_id = ?, document_version_id = ?, \
                 acknowledged_at = ?, row_version = ? WHERE id = ? AND row_version = ?",
                [
                    input.personnel_id.into(),
                    input.document_version_id.into(),
                    input.acknowledged_at.into(),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on document_acknowledgements.".into(),
            ]));
        }
        return Ok(get_ack_by_id(db, id).await?.expect("row"));
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO document_acknowledgements (entity_sync_id, personnel_id, document_version_id, \
         acknowledged_at, row_version) VALUES (?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.personnel_id.into(),
            input.document_version_id.into(),
            input.acknowledged_at.into(),
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
    Ok(get_ack_by_id(db, new_id).await?.expect("row"))
}
