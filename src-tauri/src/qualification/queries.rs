use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

use super::domain::{
    CertificationType, CertificationTypeUpsertInput, PersonnelCertification, CERTIFICATION_CATEGORIES, PersonnelCertificationListFilter,
    PersonnelCertificationUpsertInput, QualificationRequirementProfile, QualificationRequirementProfileUpsertInput,
};

const VERIFICATION: &[&str] = &["pending", "verified", "rejected"];

pub(crate) const CERT_TYPE_COLS: &str = "id, entity_sync_id, code, name, default_validity_months, renewal_lead_days, \
     row_version, category, description, is_active";

/// Renewal warning window used when a certification type has no `renewal_lead_days`.
pub(crate) const DEFAULT_RENEWAL_LEAD_DAYS: i64 = 30;

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

pub(crate) fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    let prefix: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&prefix, "%Y-%m-%d").ok()
}

/// Accepts `YYYY-MM-DD` or an RFC 3339 timestamp; returns the calendar day.
fn validate_cert_date(field: &str, value: Option<&str>) -> AppResult<Option<NaiveDate>> {
    let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .or_else(|| chrono::DateTime::parse_from_rfc3339(raw).ok().map(|d| d.date_naive()))
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("{field} must be a date (YYYY-MM-DD).")]))?;
    Ok(Some(day))
}

/// Readiness of one certificate. A verified certificate whose expiry date cannot be read is
/// treated as expired (fail closed): it must never count as valid.
pub(crate) fn readiness_status(
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
                return "expired".to_string();
            };
            let today = Utc::now().date_naive();
            if exp_d < today {
                return "expired".to_string();
            }
            let lead = renewal_lead_days.unwrap_or(DEFAULT_RENEWAL_LEAD_DAYS).max(0);
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
        default_validity_months: row
            .try_get("", "default_validity_months")
            .map_err(|e| decode_err("default_validity_months", e))?,
        renewal_lead_days: row
            .try_get("", "renewal_lead_days")
            .map_err(|e| decode_err("renewal_lead_days", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        category: row.try_get("", "category").map_err(|e| decode_err("category", e))?,
        description: row
            .try_get("", "description")
            .map_err(|e| decode_err("description", e))?,
        is_active: row
            .try_get::<i64>("", "is_active")
            .map_err(|e| decode_err("is_active", e))?
            != 0,
    })
}

fn map_profile(row: &sea_orm::QueryResult) -> AppResult<QualificationRequirementProfile> {
    Ok(QualificationRequirementProfile {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        profile_name: row
            .try_get("", "profile_name")
            .map_err(|e| decode_err("profile_name", e))?,
        required_certification_type_ids_json: row
            .try_get("", "required_certification_type_ids_json")
            .map_err(|e| decode_err("required_certification_type_ids_json", e))?,
        applies_to_permit_type_codes_json: row
            .try_get("", "applies_to_permit_type_codes_json")
            .map_err(|e| decode_err("applies_to_permit_type_codes_json", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
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
    let expires_at: Option<String> = row.try_get("", "expires_at").map_err(|e| decode_err("expires_at", e))?;
    let rs = readiness_status(&verification_status, &expires_at, lead);
    Ok(PersonnelCertification {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        personnel_id: row
            .try_get("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?,
        certification_type_id: row
            .try_get("", "certification_type_id")
            .map_err(|e| decode_err("certification_type_id", e))?,
        issued_at: row.try_get("", "issued_at").map_err(|e| decode_err("issued_at", e))?,
        expires_at,
        issuing_body: row
            .try_get("", "issuing_body")
            .map_err(|e| decode_err("issuing_body", e))?,
        certificate_ref: row
            .try_get("", "certificate_ref")
            .map_err(|e| decode_err("certificate_ref", e))?,
        verification_status,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        readiness_status: rs,
        certification_type_code: type_code,
        certification_type_name: type_name,
    })
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
            return Err(AppError::ValidationFailed(vec![
                "certification type ids must be positive.".into(),
            ]));
        }
    }
    Ok(())
}

fn validate_profile_json_codes(raw: &str) -> AppResult<()> {
    let v: Vec<String> = serde_json::from_str(raw)
        .map_err(|e| AppError::ValidationFailed(vec![format!("JSON array of string: {e}")]))?;
    for c in v {
        if c.trim().is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "permit type codes must be non-empty.".into()
            ]));
        }
    }
    Ok(())
}

pub async fn list_certification_types(db: &DatabaseConnection) -> AppResult<Vec<CertificationType>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT {CERT_TYPE_COLS} FROM certification_types ORDER BY is_active DESC, category ASC, code ASC"),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_cert_type(&r)?);
    }
    Ok(out)
}

pub(crate) async fn get_certification_type_by_id(
    db: &impl ConnectionTrait,
    id: i64,
) -> AppResult<Option<CertificationType>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CERT_TYPE_COLS} FROM certification_types WHERE id = ?"),
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
    let code = input.code.trim().to_uppercase();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(super::issue(
            "CERT_TYPE_CODE_NAME_REQUIRED",
            "A code and a name are required.",
            &[],
        ));
    }
    if let Some(category) = input.category.as_deref() {
        if !CERTIFICATION_CATEGORIES.contains(&category) {
            return Err(super::issue(
                "CERT_TYPE_CATEGORY_UNKNOWN",
                "Unknown certification category.",
                &[("category", category.to_string())],
            ));
        }
    }
    for (field, value) in [
        ("default_validity_months", input.default_validity_months),
        ("renewal_lead_days", input.renewal_lead_days),
    ] {
        if value.is_some_and(|v| v <= 0) {
            return Err(super::issue(
                "CERT_TYPE_DURATION_POSITIVE",
                "Validity and renewal notice must be positive numbers.",
                &[("field", field.to_string())],
            ));
        }
    }
    let description = input
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    let txn = db.begin().await?;

    let clash = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM certification_types WHERE UPPER(code) = ? AND id <> ?",
            [code.clone().into(), input.id.unwrap_or(0).into()],
        ))
        .await?;
    if clash.is_some() {
        return Err(super::issue(
            "CERT_TYPE_CODE_TAKEN",
            "Another certification type already uses this code.",
            &[("code", code)],
        ));
    }

    if let Some(id) = input.id {
        let current = get_certification_type_by_id(&txn, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "CertificationType".into(),
                id: id.to_string(),
            })?;
        let expected = input.expected_row_version.unwrap_or(current.row_version);
        let affected = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE certification_types SET code = ?, name = ?, default_validity_months = ?, \
                 renewal_lead_days = ?, category = ?, description = ?, is_active = ?, row_version = row_version + 1 \
                 WHERE id = ? AND row_version = ?",
                [
                    code.into(),
                    name.into(),
                    opt_i64(input.default_validity_months),
                    opt_i64(input.renewal_lead_days),
                    input.category.unwrap_or(current.category).into(),
                    opt_string(description.or(current.description)),
                    i64::from(input.is_active.unwrap_or(current.is_active)).into(),
                    id.into(),
                    expected.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(super::issue(
                "RECORD_CHANGED_ELSEWHERE",
                "This record was changed by someone else; reload and try again.",
                &[],
            ));
        }
        let updated = get_certification_type_by_id(&txn, id).await?.expect("row");
        txn.commit().await?;
        return Ok(updated);
    }

    let entity_sync_id = Uuid::new_v4().to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO certification_types (entity_sync_id, code, name, default_validity_months, renewal_lead_days, \
         category, description, is_active, row_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            entity_sync_id.into(),
            code.into(),
            name.into(),
            opt_i64(input.default_validity_months),
            opt_i64(input.renewal_lead_days),
            input.category.unwrap_or_else(|| "other".to_string()).into(),
            opt_string(description),
            i64::from(input.is_active.unwrap_or(true)).into(),
        ],
    ))
    .await?;
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let created = get_certification_type_by_id(&txn, new_id).await?.expect("row");
    txn.commit().await?;
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

/// List skill reference_value_ids linked to a profile via `qualification_profile_skills`.
pub async fn list_profile_skill_ids(db: &DatabaseConnection, profile_id: i64) -> AppResult<Vec<i64>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reference_value_id FROM qualification_profile_skills WHERE profile_id = ? ORDER BY id ASC",
            [profile_id.into()],
        ))
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<i64>("", "reference_value_id").ok())
        .collect())
}

/// Replace `qualification_profile_skills` rows for a profile.
/// Call inside a transaction to keep the cert JSON and skills in sync.
pub async fn replace_profile_skills(
    db: &impl ConnectionTrait,
    profile_id: i64,
    skill_reference_value_ids: &[i64],
) -> AppResult<()> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM qualification_profile_skills WHERE profile_id = ?",
        [profile_id.into()],
    ))
    .await?;
    for &rv_id in skill_reference_value_ids {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO qualification_profile_skills \
             (profile_id, reference_value_id, min_proficiency_level, is_required, created_at) \
             VALUES (?, ?, 1, 1, ?)",
            [profile_id.into(), rv_id.into(), now.clone().into()],
        ))
        .await?;
    }
    Ok(())
}

async fn get_profile_by_id(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<QualificationRequirementProfile>> {
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
    let txn = db.begin().await?;

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let current = get_profile_by_id(&txn, id).await?.ok_or_else(|| AppError::NotFound {
            entity: "QualificationRequirementProfile".into(),
            id: id.to_string(),
        })?;
        let new_rv = current.row_version + 1;
        let affected = txn
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
        let updated = get_profile_by_id(&txn, id).await?.expect("row");
        // Sync skills if provided
        if let Some(ref skill_ids) = input.skill_reference_value_ids {
            replace_profile_skills(&txn, id, skill_ids).await?;
        }
        txn.commit().await?;
        return Ok(updated);
    }

    let entity_sync_id = Uuid::new_v4().to_string();
    let skill_ids_snap = input.skill_reference_value_ids.clone();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO qualification_requirement_profiles (entity_sync_id, profile_name, \
         required_certification_type_ids_json, applies_to_permit_type_codes_json, row_version) \
         VALUES (?, ?, ?, ?, 1)",
        [
            entity_sync_id.into(),
            input.profile_name.into(),
            input.required_certification_type_ids_json.into(),
            input.applies_to_permit_type_codes_json.into(),
        ],
    ))
    .await?;
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    if let Some(ref skill_ids) = skill_ids_snap {
        replace_profile_skills(&txn, new_id, skill_ids).await?;
    }
    let created = get_profile_by_id(&txn, new_id).await?.expect("row");
    txn.commit().await?;
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

async fn get_personnel_cert_by_id(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<PersonnelCertification>> {
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

pub async fn upsert_personnel_certification<C>(
    db: &C,
    input: PersonnelCertificationUpsertInput,
) -> AppResult<PersonnelCertification>
where
    C: ConnectionTrait + TransactionTrait,
{
    validate_verification(&input.verification_status)?;
    let issued = validate_cert_date("issued_at", input.issued_at.as_deref())?;
    let expires = validate_cert_date("expires_at", input.expires_at.as_deref())?;
    if let (Some(i), Some(e)) = (issued, expires) {
        if e < i {
            return Err(AppError::ValidationFailed(vec![
                "expires_at must not be before issued_at.".into(),
            ]));
        }
    }
    let txn = db.begin().await?;

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let ev = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required for update.".into()]))?;
        let current = get_personnel_cert_by_id(&txn, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PersonnelCertification".into(),
                id: id.to_string(),
            })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec!["Row version mismatch.".into()]));
        }
        let new_rv = current.row_version + 1;
        let affected = txn
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
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on personnel_certifications.".into(),
            ]));
        }
        let updated = get_personnel_cert_by_id(&txn, id).await?.expect("row");
        txn.commit().await?;
        return Ok(updated);
    }

    let entity_sync_id = Uuid::new_v4().to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_certifications (entity_sync_id, personnel_id, certification_type_id, \
         issued_at, expires_at, issuing_body, certificate_ref, verification_status, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            entity_sync_id.into(),
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
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let created = get_personnel_cert_by_id(&txn, new_id).await?.expect("row");
    txn.commit().await?;
    Ok(created)
}
