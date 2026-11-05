//! Validation list (L7-32, Advisor decision 2): skills recorded without validation (an import
//! brings them in that way) and certificates still waiting for verification, listed together so a
//! `trn.certify` holder can validate a filtered selection at once. Each line is validated in its
//! own transaction together with its audit event, exactly as it was shown: a line changed since
//! the list was read is refused, never validated in its new state.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

pub const KIND_SKILL: &str = "skill";
pub const KIND_CERTIFICATION: &str = "certification";

/// One line waiting for validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingQualification {
    /// `skill` or `certification`.
    pub kind: String,
    /// `personnel_skills.id` or `personnel_certifications.id`.
    pub id: i64,
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub team_name: Option<String>,
    /// Skill code (PERSONNEL.SKILLS) or certification type code.
    pub item_code: String,
    pub item_label: String,
    /// Skill level 1–5 (skills only).
    pub proficiency_level: Option<i64>,
    pub issued_at: Option<String>,
    /// Skill end of validity or certificate expiry.
    pub valid_to: Option<String>,
    pub issuing_body: Option<String>,
    pub certificate_ref: Option<String>,
    /// Skills: `manual` or `import`. Certificates carry no source.
    pub source_type: Option<String>,
    /// Certificates: row version shown, sent back when validating.
    pub row_version: Option<i64>,
}

/// A line to validate, as it was shown: the server validates it only if it still reads so.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationValidationItem {
    pub kind: String,
    pub id: i64,
    /// Skills: the level shown.
    pub proficiency_level: Option<i64>,
    /// Skills: the end of validity shown.
    pub valid_to: Option<String>,
    /// Certificates: the row version shown.
    pub row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationValidationResult {
    pub kind: String,
    pub id: i64,
    pub validated: bool,
    /// Refusal code (`training:errors.<code>`) when not validated.
    pub code: Option<String>,
}

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("pending qualification decode '{field}': {e}"))
}

/// Every unvalidated skill and every pending certificate of active people, people by name.
pub async fn list_pending_qualifications(db: &impl ConnectionTrait) -> AppResult<Vec<PendingQualification>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 'skill' AS kind, ps.id, p.id AS personnel_id, p.employee_code, p.full_name, \
                    t.name AS team_name, rv.code AS item_code, rv.label AS item_label, \
                    ps.proficiency_level, NULL AS issued_at, ps.valid_to, NULL AS issuing_body, \
                    NULL AS certificate_ref, ps.source_type, NULL AS row_version \
             FROM personnel_skills ps \
             JOIN personnel p ON p.id = ps.personnel_id \
             JOIN reference_values rv ON rv.id = ps.reference_value_id \
             LEFT JOIN teams t ON t.id = p.primary_team_id \
             WHERE ps.is_validated = 0 AND p.employment_status = 'active' \
             UNION ALL \
             SELECT 'certification' AS kind, pc.id, p.id AS personnel_id, p.employee_code, p.full_name, \
                    t.name AS team_name, ct.code AS item_code, ct.name AS item_label, \
                    NULL AS proficiency_level, pc.issued_at, pc.expires_at AS valid_to, pc.issuing_body, \
                    pc.certificate_ref, NULL AS source_type, pc.row_version \
             FROM personnel_certifications pc \
             JOIN personnel p ON p.id = pc.personnel_id \
             JOIN certification_types ct ON ct.id = pc.certification_type_id \
             LEFT JOIN teams t ON t.id = p.primary_team_id \
             WHERE pc.verification_status = 'pending' AND p.employment_status = 'active' \
             ORDER BY full_name, kind, item_label"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PendingQualification {
                kind: r.try_get("", "kind").map_err(|e| decode_err("kind", e))?,
                id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                personnel_id: r
                    .try_get("", "personnel_id")
                    .map_err(|e| decode_err("personnel_id", e))?,
                employee_code: r
                    .try_get("", "employee_code")
                    .map_err(|e| decode_err("employee_code", e))?,
                full_name: r.try_get("", "full_name").map_err(|e| decode_err("full_name", e))?,
                team_name: r.try_get("", "team_name").ok().flatten(),
                item_code: r.try_get("", "item_code").map_err(|e| decode_err("item_code", e))?,
                item_label: r.try_get("", "item_label").map_err(|e| decode_err("item_label", e))?,
                proficiency_level: r.try_get("", "proficiency_level").ok().flatten(),
                issued_at: r.try_get("", "issued_at").ok().flatten(),
                valid_to: r.try_get("", "valid_to").ok().flatten(),
                issuing_body: r.try_get("", "issuing_body").ok().flatten(),
                certificate_ref: r.try_get("", "certificate_ref").ok().flatten(),
                source_type: r.try_get("", "source_type").ok().flatten(),
                row_version: r.try_get("", "row_version").ok().flatten(),
            })
        })
        .collect()
}

/// Who validates: the user, and the personnel record linked to the account (a skill names its
/// validator; without a linked record no skill can be validated).
#[derive(Debug, Clone, Copy)]
pub struct Validator {
    pub user_id: i64,
    pub personnel_id: Option<i64>,
}

/// Validates the lines one by one. A line that cannot be validated is reported with its code and
/// the others go on. Duplicates in the request count once.
pub async fn validate_qualifications<C>(
    db: &C,
    items: &[QualificationValidationItem],
    validator: Validator,
) -> AppResult<Vec<QualificationValidationResult>>
where
    C: ConnectionTrait + TransactionTrait,
{
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if !seen.insert((item.kind.clone(), item.id)) {
            continue;
        }
        let outcome = match item.kind.as_str() {
            KIND_SKILL => validate_skill(db, item, validator).await,
            KIND_CERTIFICATION => verify_certification(db, item, validator).await,
            _ => Ok(Err("QUAL_VALIDATION_KIND_UNKNOWN")),
        };
        let (validated, code) = match outcome? {
            Ok(()) => (true, None),
            Err(code) => (false, Some(code.to_string())),
        };
        out.push(QualificationValidationResult {
            kind: item.kind.clone(),
            id: item.id,
            validated,
            code,
        });
    }
    Ok(out)
}

fn trimmed(v: &Option<String>) -> String {
    v.as_deref().map(str::trim).unwrap_or("").to_string()
}

async fn person_label(conn: &impl ConnectionTrait, personnel_id: i64) -> AppResult<(String, String)> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT employee_code, full_name FROM personnel WHERE id = ?",
            [personnel_id.into()],
        ))
        .await?;
    Ok(row
        .map(|r| {
            (
                r.try_get("", "employee_code").unwrap_or_default(),
                r.try_get("", "full_name").unwrap_or_default(),
            )
        })
        .unwrap_or_default())
}

/// `Ok(Err(code))`: refused, nothing written.
async fn validate_skill<C>(
    db: &C,
    item: &QualificationValidationItem,
    validator: Validator,
) -> AppResult<Result<(), &'static str>>
where
    C: ConnectionTrait + TransactionTrait,
{
    let Some(validator_personnel) = validator.personnel_id else {
        return Ok(Err("SKILL_VALIDATOR_NOT_LINKED"));
    };
    let Some(level) = item.proficiency_level else {
        return Ok(Err("QUAL_VALIDATION_CHANGED"));
    };
    let shown_valid_to = trimmed(&item.valid_to);
    let txn = db.begin().await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ps.entity_sync_id, ps.personnel_id, rv.code, ps.source_type \
             FROM personnel_skills ps \
             JOIN personnel p ON p.id = ps.personnel_id \
             JOIN reference_values rv ON rv.id = ps.reference_value_id \
             WHERE ps.id = ? AND ps.is_validated = 0 AND p.employment_status = 'active' \
               AND ps.proficiency_level = ? AND COALESCE(TRIM(ps.valid_to), '') = ?",
            [item.id.into(), level.into(), shown_valid_to.clone().into()],
        ))
        .await?;
    let Some(row) = row else {
        txn.rollback().await?;
        return Ok(Err("QUAL_VALIDATION_CHANGED"));
    };
    let sync_id: Option<String> = row.try_get("", "entity_sync_id").ok().flatten();
    let personnel_id: i64 = row
        .try_get("", "personnel_id")
        .map_err(|e| decode_err("personnel_id", e))?;
    let skill_code: String = row.try_get("", "code").map_err(|e| decode_err("code", e))?;
    let source: Option<String> = row.try_get("", "source_type").ok().flatten();
    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let written = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_skills SET is_validated = 1, verified_by_id = ?, verified_at = ?, updated_at = ? \
             WHERE id = ? AND is_validated = 0 AND proficiency_level = ? \
               AND COALESCE(TRIM(valid_to), '') = ?",
            [
                validator_personnel.into(),
                now.clone().into(),
                now.into(),
                item.id.into(),
                level.into(),
                shown_valid_to.clone().into(),
            ],
        ))
        .await?;
    if written.rows_affected() != 1 {
        txn.rollback().await?;
        return Ok(Err("QUAL_VALIDATION_CHANGED"));
    }
    let (employee_code, full_name) = person_label(&txn, personnel_id).await?;
    crate::audit::writer::write_audit_event_in(
        &txn,
        crate::audit::writer::AuditEventInput {
            action_code: "qualification.skill_validated".to_string(),
            target_type: Some("personnel_skill".to_string()),
            target_id: sync_id,
            actor_id: Some(validator.user_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Skill validated from the validation list",
                "employee_code": employee_code,
                "person": full_name,
                "skill": skill_code,
                "level": level,
                "valid_to": if shown_valid_to.is_empty() { None } else { Some(shown_valid_to) },
                "source": source,
            })),
        },
    )
    .await?;
    txn.commit().await?;
    Ok(Ok(()))
}

async fn verify_certification<C>(
    db: &C,
    item: &QualificationValidationItem,
    validator: Validator,
) -> AppResult<Result<(), &'static str>>
where
    C: ConnectionTrait + TransactionTrait,
{
    let Some(row_version) = item.row_version else {
        return Ok(Err("QUAL_VALIDATION_CHANGED"));
    };
    let txn = db.begin().await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pc.entity_sync_id, pc.personnel_id, ct.code, pc.issued_at, pc.expires_at, \
                    pc.issuing_body, pc.certificate_ref \
             FROM personnel_certifications pc \
             JOIN personnel p ON p.id = pc.personnel_id \
             JOIN certification_types ct ON ct.id = pc.certification_type_id \
             WHERE pc.id = ? AND pc.verification_status = 'pending' AND pc.row_version = ? \
               AND p.employment_status = 'active'",
            [item.id.into(), row_version.into()],
        ))
        .await?;
    let Some(row) = row else {
        txn.rollback().await?;
        return Ok(Err("QUAL_VALIDATION_CHANGED"));
    };
    let sync_id: Option<String> = row.try_get("", "entity_sync_id").ok().flatten();
    let personnel_id: i64 = row
        .try_get("", "personnel_id")
        .map_err(|e| decode_err("personnel_id", e))?;
    let type_code: String = row.try_get("", "code").map_err(|e| decode_err("code", e))?;
    let issued_at: Option<String> = row.try_get("", "issued_at").ok().flatten();
    let expires_at: Option<String> = row.try_get("", "expires_at").ok().flatten();
    let issuing_body: Option<String> = row.try_get("", "issuing_body").ok().flatten();
    let certificate_ref: Option<String> = row.try_get("", "certificate_ref").ok().flatten();
    let written = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_certifications SET verification_status = 'verified', row_version = row_version + 1 \
             WHERE id = ? AND verification_status = 'pending' AND row_version = ?",
            [item.id.into(), row_version.into()],
        ))
        .await?;
    if written.rows_affected() != 1 {
        txn.rollback().await?;
        return Ok(Err("QUAL_VALIDATION_CHANGED"));
    }
    let (employee_code, full_name) = person_label(&txn, personnel_id).await?;
    crate::audit::writer::write_audit_event_in(
        &txn,
        crate::audit::writer::AuditEventInput {
            action_code: "qualification.certificate_verified".to_string(),
            target_type: Some("personnel_certification".to_string()),
            target_id: sync_id,
            actor_id: Some(validator.user_id),
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Certificate verified from the validation list",
                "employee_code": employee_code,
                "person": full_name,
                "certification": type_code,
                "issued_at": issued_at,
                "expires_at": expires_at,
                "issuing_body": issuing_body,
                "certificate_ref": certificate_ref,
            })),
        },
    )
    .await?;
    txn.commit().await?;
    Ok(Ok(()))
}
