use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    CertificationTypeSyncPayload, PersonnelCertificationSyncPayload, QualificationRequirementProfileSyncPayload,
    StageOutboxItemInput, SYNC_ENTITY_CERTIFICATION_TYPES, SYNC_ENTITY_PERSONNEL_CERTIFICATIONS,
    SYNC_ENTITY_QUALIFICATION_REQUIREMENT_PROFILES,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    CertificationType, CertificationTypeUpsertInput, PersonnelCertification, PersonnelCertificationListFilter,
    PersonnelCertificationUpsertInput, QualificationRequirementProfile, QualificationRequirementProfileUpsertInput,
};

const VERIFICATION: &[&str] = &["pending", "verified", "rejected"];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode qualification field '{field}': {err}"))
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    let prefix: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&prefix, "%Y-%m-%d").ok()
}

fn readiness_status(
    verification_status: &str,
    expires_at: &Option<String>,
    renewal_lead_days: Option<i64>,
) -> String {
    match verification_status {
        "rejected" => "rejected".to_string(),
        "pending" => "pending".to_string(),
        "verified" => {
            let Some(exp) = expires_at else {
                return "valid".to_string();
            };
            let Some(exp_d) = parse_iso_date(exp) else {
                return "valid".to_string();
            };
            let today = Utc::now().date_naive();
            if exp_d < today {
                return "expired".to_string();
            }
            let lead = renewal_lead_days.unwrap_or(30).max(0);
            let window_end = today + Duration::days(lead);
            if exp_d <= window_end {
                "expiring_soon".to_string()
            } else {
                "valid".to_string()
            }
        }
        _ => "pending".to_string(),
    }
}

fn map_cert_type(row: &sea_orm::QueryResult) -> AppResult<CertificationType> {
    Ok(CertificationType {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
        default_validity_months: row.try_get("", "default_validity_months").ok(),
        renewal_lead_days: row.try_get("", "renewal_lead_days").ok(),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_profile(row: &sea_orm::QueryResult) -> AppResult<QualificationRequirementProfile> {
    Ok(QualificationRequirementProfile {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        profile_name: row.try_get("", "profile_name").map_err(|e| decode_err("profile_name", e))?,
        required_certification_type_ids_json: row
            .try_get("", "required_certification_type_ids_json")
            .map_err(|e| decode_err("required_certification_type_ids_json", e))?,
        applies_to_permit_type_codes_json: row
            .try_get("", "applies_to_permit_type_codes_json")
            .map_err(|e| decode_err("applies_to_permit_type_codes_json", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_personnel_cert(
    row: &sea_orm::QueryResult,
    type_code: Option<String>,
    type_name: Option<String>,
    lead: Option<i64>,
) -> AppResult<PersonnelCertification> {
    let verification_status: String = row
        .try_get("", "verification_status")
        .map_err(|e| decode_err("verification_status", e))?;
    let expires_at: Option<String> = row.try_get("", "expires_at").ok();
    let rs = readiness_status(&verification_status, &expires_at, lead);
    Ok(PersonnelCertification {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        personnel_id: row.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?,
        certification_type_id: row
            .try_get("", "certification_type_id")
            .map_err(|e| decode_err("certification_type_id", e))?,
        issued_at: row.try_get("", "issued_at").ok(),
        expires_at,
        issuing_body: row.try_get("", "issuing_body").ok(),
        certificate_ref: row.try_get("", "certificate_ref").ok(),
        verification_status,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        readiness_status: rs,
        certification_type_code: type_code,
        certification_type_name: type_name,
    })
}

async fn stage_certification_type(db: &DatabaseConnection, row: &CertificationType) -> AppResult<()> {
    let payload = CertificationTypeSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        code: row.code.clone(),
        name: row.name.clone(),
        default_validity_months: row.default_validity_months,
        renewal_lead_days: row.renewal_lead_days,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("certification_types:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_CERTIFICATION_TYPES.to_string(),
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

async fn stage_personnel_certification(db: &DatabaseConnection, row: &PersonnelCertification) -> AppResult<()> {
    let payload = PersonnelCertificationSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        personnel_id: row.personnel_id,
        certification_type_id: row.certification_type_id,
        issued_at: row.issued_at.clone(),
        expires_at: row.expires_at.clone(),
        issuing_body: row.issuing_body.clone(),
        certificate_ref: row.certificate_ref.clone(),
        verification_status: row.verification_status.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "personnel_certifications:{}:v{}",
                row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_PERSONNEL_CERTIFICATIONS.to_string(),
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

async fn stage_qualification_profile(
    db: &DatabaseConnection,
    row: &QualificationRequirementProfile,
) -> AppResult<()> {
    let ids: Vec<i64> = serde_json::from_str(&row.required_certification_type_ids_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("required_certification_type_ids_json: {e}")]))?;
    let codes: Vec<String> = serde_json::from_str(&row.applies_to_permit_type_codes_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("applies_to_permit_type_codes_json: {e}")]))?;
    let payload = QualificationRequirementProfileSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        profile_name: row.profile_name.clone(),
        required_certification_type_ids: ids,
        applies_to_permit_type_codes: codes,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "qualification_requirement_profiles:{}:v{}",
                row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_QUALIFICATION_REQUIREMENT_PROFILES.to_string(),
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

fn validate_verification(s: &str) -> AppResult<()> {
    if VERIFICATION.contains(&s) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "verification_status must be one of: {:?}",
            VERIFICATION
        )]))
    }
}

fn validate_profile_json_ids(raw: &str) -> AppResult<()> {
    let v: Vec<i64> =
        serde_json::from_str(raw).map_err(|e| AppError::ValidationFailed(vec![format!("JSON array of i64: {e}")]))?;
    for i in v {
        if i <= 0 {
            return Err(AppError::ValidationFailed(vec!["certification type ids must be positive.".into()]));
        }
    }
    Ok(())
}

fn validate_profile_json_codes(raw: &str) -> AppResult<()> {
    let v: Vec<String> =
        serde_json::from_str(raw).map_err(|e| AppError::ValidationFailed(vec![format!("JSON array of string: {e}")]))?;
    for c in v {
        if c.trim().is_empty() {
            return Err(AppError::ValidationFailed(vec!["permit type codes must be non-empty.".into()]));
        }
    }
    Ok(())
}

pub async fn list_certification_types(db: &DatabaseConnection) -> AppResult<Vec<CertificationType>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, default_validity_months, renewal_lead_days, row_version \
             FROM certification_types ORDER BY code ASC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_cert_type(&r)?);
    }
    Ok(out)
}

pub(crate) async fn get_certification_type_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<CertificationType>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, default_validity_months, renewal_lead_days, row_version \
             FROM certification_types WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_cert_type(&r)?)),
        None => Ok(None),
    }
}

pub async fn upsert_certification_type(
    db: &DatabaseConnection,
    input: CertificationTypeUpsertInput,
) -> AppResult<CertificationType> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["code and name are required.".into()]));
    }

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let current = get_certification_type_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "CertificationType".into(),
                id: id.to_string(),
            })?;
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE certification_types SET code = ?, name = ?, default_validity_months = ?, \
                 renewal_lead_days = ?, row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.code.into(),
                    input.name.into(),
                    opt_i64(input.default_validity_months),
                    opt_i64(input.renewal_lead_days),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec!["Concurrent update on certification_types.".into()]));
        }
        let updated = get_certification_type_by_id(db, id).await?.expect("row");
        stage_certification_type(db, &updated).await?;
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO certification_types (entity_sync_id, code, name, default_validity_months, renewal_lead_days, \
         row_version) VALUES (?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.code.into(),
            input.name.into(),
            opt_i64(input.default_validity_months),
            opt_i64(input.renewal_lead_days),
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
    let created = get_certification_type_by_id(db, new_id).await?.expect("row");
    stage_certification_type(db, &created).await?;
    Ok(created)
}

pub async fn list_qualification_requirement_profiles(
    db: &DatabaseConnection,
) -> AppResult<Vec<QualificationRequirementProfile>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, profile_name, required_certification_type_ids_json, \
             applies_to_permit_type_codes_json, row_version \
             FROM qualification_requirement_profiles ORDER BY profile_name ASC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_profile(&r)?);
    }
    Ok(out)
}

async fn get_profile_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<QualificationRequirementProfile>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, profile_name, required_certification_type_ids_json, \
             applies_to_permit_type_codes_json, row_version \
             FROM qualification_requirement_profiles WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_profile(&r)?)),
        None => Ok(None),
    }
}

pub async fn upsert_qualification_requirement_profile(
    db: &DatabaseConnection,
    input: QualificationRequirementProfileUpsertInput,
) -> AppResult<QualificationRequirementProfile> {
    if input.profile_name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["profile_name is required.".into()]));
    }
    validate_profile_json_ids(&input.required_certification_type_ids_json)?;
    validate_profile_json_codes(&input.applies_to_permit_type_codes_json)?;

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let current = get_profile_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "QualificationRequirementProfile".into(),
                id: id.to_string(),
            })?;
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE qualification_requirement_profiles SET profile_name = ?, \
                 required_certification_type_ids_json = ?, applies_to_permit_type_codes_json = ?, row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.profile_name.into(),
                    input.required_certification_type_ids_json.into(),
                    input.applies_to_permit_type_codes_json.into(),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on qualification_requirement_profiles.".into(),
            ]));
        }
        let updated = get_profile_by_id(db, id).await?.expect("row");
        stage_qualification_profile(db, &updated).await?;
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO qualification_requirement_profiles (entity_sync_id, profile_name, \
         required_certification_type_ids_json, applies_to_permit_type_codes_json, row_version) \
         VALUES (?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.profile_name.into(),
            input.required_certification_type_ids_json.into(),
            input.applies_to_permit_type_codes_json.into(),
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
    let created = get_profile_by_id(db, new_id).await?.expect("row");
    stage_qualification_profile(db, &created).await?;
    Ok(created)
}

pub async fn list_personnel_certifications(
    db: &DatabaseConnection,
    filter: PersonnelCertificationListFilter,
) -> AppResult<Vec<PersonnelCertification>> {
    let limit = filter.limit.unwrap_or(500).clamp(1, 5000);
    let mut sql = String::from(
        "SELECT pc.id, pc.entity_sync_id, pc.personnel_id, pc.certification_type_id, pc.issued_at, pc.expires_at, \
         pc.issuing_body, pc.certificate_ref, pc.verification_status, pc.row_version, \
         ct.code AS ct_code, ct.name AS ct_name, ct.renewal_lead_days AS ct_lead \
         FROM personnel_certifications pc \
         JOIN certification_types ct ON ct.id = pc.certification_type_id \
         WHERE 1=1",
    );
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(pid) = filter.personnel_id {
        sql.push_str(" AND pc.personnel_id = ?");
        params.push(pid.into());
    }
    sql.push_str(" ORDER BY pc.id DESC LIMIT ?");
    params.push(limit.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let type_code: Option<String> = r.try_get("", "ct_code").ok();
        let type_name: Option<String> = r.try_get("", "ct_name").ok();
        let lead: Option<i64> = r.try_get("", "ct_lead").ok();
        out.push(map_personnel_cert(&r, type_code, type_name, lead)?);
    }
    Ok(out)
}

async fn get_personnel_cert_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<PersonnelCertification>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pc.id, pc.entity_sync_id, pc.personnel_id, pc.certification_type_id, pc.issued_at, pc.expires_at, \
             pc.issuing_body, pc.certificate_ref, pc.verification_status, pc.row_version, \
             ct.code AS ct_code, ct.name AS ct_name, ct.renewal_lead_days AS ct_lead \
             FROM personnel_certifications pc \
             JOIN certification_types ct ON ct.id = pc.certification_type_id \
             WHERE pc.id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => {
            let type_code: Option<String> = r.try_get("", "ct_code").ok();
            let type_name: Option<String> = r.try_get("", "ct_name").ok();
            let lead: Option<i64> = r.try_get("", "ct_lead").ok();
            Ok(Some(map_personnel_cert(&r, type_code, type_name, lead)?))
        }
        None => Ok(None),
    }
}

pub async fn upsert_personnel_certification(
    db: &DatabaseConnection,
    input: PersonnelCertificationUpsertInput,
) -> AppResult<PersonnelCertification> {
    validate_verification(&input.verification_status)?;

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let ev = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required for update.".into()]))?;
        let current = get_personnel_cert_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PersonnelCertification".into(),
                id: id.to_string(),
            })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec!["Row version mismatch.".into()]));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE personnel_certifications SET personnel_id = ?, certification_type_id = ?, \
                 issued_at = ?, expires_at = ?, issuing_body = ?, certificate_ref = ?, \
                 verification_status = ?, row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.personnel_id.into(),
                    input.certification_type_id.into(),
                    opt_string(input.issued_at),
                    opt_string(input.expires_at),
                    opt_string(input.issuing_body),
                    opt_string(input.certificate_ref),
                    input.verification_status.into(),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec!["Concurrent update on personnel_certifications.".into()]));
        }
        let updated = get_personnel_cert_by_id(db, id).await?.expect("row");
        stage_personnel_certification(db, &updated).await?;
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_certifications (entity_sync_id, personnel_id, certification_type_id, \
         issued_at, expires_at, issuing_body, certificate_ref, verification_status, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.personnel_id.into(),
            input.certification_type_id.into(),
            opt_string(input.issued_at),
            opt_string(input.expires_at),
            opt_string(input.issuing_body),
            opt_string(input.certificate_ref),
            input.verification_status.into(),
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
    let created = get_personnel_cert_by_id(db, new_id).await?.expect("row");
    stage_personnel_certification(db, &created).await?;
    Ok(created)
}
