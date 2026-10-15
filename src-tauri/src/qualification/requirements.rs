//! Qualification requirements of jobs (`qualification_requirements`) — what a work order, a
//! work-order type, a permit type or a LOTO role asks of the people doing it.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

use super::gate::validate_role;

pub const TARGET_KINDS: &[&str] = &["work_order", "work_order_type", "permit_type", "loto_role"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationRequirement {
    pub id: i64,
    pub entity_sync_id: String,
    pub target_kind: String,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub target_code: Option<String>,
    /// Work-order type label, permit type name, work-order title (LOTO roles are translated on
    /// the client).
    pub target_label: Option<String>,
    pub certification_type_id: Option<i64>,
    pub certification_code: Option<String>,
    pub certification_name: Option<String>,
    pub skill_reference_value_id: Option<i64>,
    pub skill_code: Option<String>,
    pub skill_label: Option<String>,
    pub min_proficiency_level: Option<i64>,
    pub coverage: String,
    pub notes: Option<String>,
    pub is_active: bool,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QualificationRequirementFilter {
    pub target_kind: Option<String>,
    pub work_order_id: Option<i64>,
    pub target_code: Option<String>,
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QualificationRequirementUpsertInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub target_kind: String,
    pub work_order_id: Option<i64>,
    pub target_code: Option<String>,
    pub certification_type_id: Option<i64>,
    pub skill_reference_value_id: Option<i64>,
    pub min_proficiency_level: Option<i64>,
    pub coverage: Option<String>,
    pub notes: Option<String>,
    pub is_active: Option<bool>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("qualification requirement decode '{field}': {err}"))
}

/// The target's code and label are read from the type itself (linked by id): renaming a
/// work-order or permit type keeps its requirements.
const SELECT: &str = "SELECT qr.id, qr.entity_sync_id, qr.target_kind, qr.work_order_id, wo.code AS wo_code, \
        qr.work_order_type_id, qr.permit_type_id, \
        CASE qr.target_kind \
            WHEN 'work_order_type' THEN wot.code WHEN 'permit_type' THEN pt.code ELSE qr.target_code END AS target_code, \
        CASE qr.target_kind \
            WHEN 'work_order_type' THEN wot.label \
            WHEN 'permit_type' THEN pt.name \
            WHEN 'work_order' THEN wo.title \
            ELSE NULL END AS target_label, \
        qr.certification_type_id, ct.code AS cert_code, ct.name AS cert_name, \
        qr.skill_reference_value_id, rv.code AS skill_code, rv.label AS skill_label, \
        qr.min_proficiency_level, qr.coverage, qr.notes, qr.is_active, qr.row_version, qr.created_at, qr.updated_at \
     FROM qualification_requirements qr \
     LEFT JOIN work_orders wo ON wo.id = qr.work_order_id \
     LEFT JOIN work_order_types wot ON wot.id = qr.work_order_type_id \
     LEFT JOIN permit_types pt ON pt.id = qr.permit_type_id \
     LEFT JOIN certification_types ct ON ct.id = qr.certification_type_id \
     LEFT JOIN reference_values rv ON rv.id = qr.skill_reference_value_id";

fn map_row(r: &sea_orm::QueryResult) -> AppResult<QualificationRequirement> {
    Ok(QualificationRequirement {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        target_kind: r.try_get("", "target_kind").map_err(|e| decode_err("target_kind", e))?,
        work_order_id: r
            .try_get("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        work_order_code: r.try_get("", "wo_code").ok().flatten(),
        target_code: r.try_get("", "target_code").ok().flatten(),
        target_label: r.try_get("", "target_label").ok().flatten(),
        certification_type_id: r
            .try_get("", "certification_type_id")
            .map_err(|e| decode_err("certification_type_id", e))?,
        certification_code: r.try_get("", "cert_code").ok().flatten(),
        certification_name: r.try_get("", "cert_name").ok().flatten(),
        skill_reference_value_id: r
            .try_get("", "skill_reference_value_id")
            .map_err(|e| decode_err("skill_reference_value_id", e))?,
        skill_code: r.try_get("", "skill_code").ok().flatten(),
        skill_label: r.try_get("", "skill_label").ok().flatten(),
        min_proficiency_level: r
            .try_get("", "min_proficiency_level")
            .map_err(|e| decode_err("min_proficiency_level", e))?,
        coverage: r.try_get("", "coverage").map_err(|e| decode_err("coverage", e))?,
        notes: r.try_get("", "notes").map_err(|e| decode_err("notes", e))?,
        is_active: r
            .try_get::<i64>("", "is_active")
            .map_err(|e| decode_err("is_active", e))?
            != 0,
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: r.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_requirements(
    db: &impl ConnectionTrait,
    filter: QualificationRequirementFilter,
) -> AppResult<Vec<QualificationRequirement>> {
    let mut sql = format!("{SELECT} WHERE 1 = 1");
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if !filter.include_inactive {
        sql.push_str(" AND qr.is_active = 1");
    }
    if let Some(kind) = filter.target_kind {
        sql.push_str(" AND qr.target_kind = ?");
        values.push(kind.into());
    }
    if let Some(wo) = filter.work_order_id {
        sql.push_str(" AND qr.work_order_id = ?");
        values.push(wo.into());
    }
    if let Some(code) = filter.target_code {
        sql.push_str(
            " AND UPPER(CASE qr.target_kind WHEN 'work_order_type' THEN wot.code WHEN 'permit_type' THEN pt.code \
             ELSE qr.target_code END) = UPPER(?)",
        );
        values.push(code.into());
    }
    sql.push_str(" ORDER BY qr.target_kind, target_code, qr.work_order_id, qr.id");
    db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .iter()
        .map(map_row)
        .collect()
}

async fn get_requirement(db: &impl ConnectionTrait, id: i64) -> AppResult<QualificationRequirement> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SELECT} WHERE qr.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "QualificationRequirement".into(),
            id: id.to_string(),
        })?;
    map_row(&row)
}

async fn exists(db: &impl ConnectionTrait, sql: &str, value: sea_orm::Value) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [value]))
        .await?
        .is_some())
}

fn target_required(what: &str) -> AppError {
    super::issue(
        "REQUIREMENT_TARGET_REQUIRED",
        "Choose what the requirement applies to.",
        &[("target", what.to_string())],
    )
}

fn target_unknown(code: &str) -> AppError {
    super::issue(
        "REQUIREMENT_TARGET_UNKNOWN",
        "The work-order type, permit type or role does not exist.",
        &[("target", code.to_string())],
    )
}

/// What a requirement points at, resolved and normalised.
#[derive(Debug, Clone, PartialEq, Default)]
struct Target {
    work_order_id: Option<i64>,
    work_order_type_id: Option<i64>,
    permit_type_id: Option<i64>,
    /// LOTO role code (other kinds point by id).
    role_code: Option<String>,
}

async fn type_id(db: &impl ConnectionTrait, table: &str, code: &str) -> AppResult<Option<i64>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT id FROM {table} WHERE UPPER(code) = UPPER(?)"),
            [code.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "id").ok()))
}

/// Validates the target and the qualification asked for; returns the resolved target.
async fn validate(db: &impl ConnectionTrait, input: &QualificationRequirementUpsertInput) -> AppResult<Target> {
    if !TARGET_KINDS.contains(&input.target_kind.as_str()) {
        return Err(target_unknown(&input.target_kind));
    }
    let code = input
        .target_code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    let target = match input.target_kind.as_str() {
        "work_order" => {
            let wo = input.work_order_id.ok_or_else(|| target_required("work_order"))?;
            if !exists(db, "SELECT 1 AS x FROM work_orders WHERE id = ?", wo.into()).await? {
                return Err(AppError::NotFound {
                    entity: "WorkOrder".into(),
                    id: wo.to_string(),
                });
            }
            Target {
                work_order_id: Some(wo),
                ..Target::default()
            }
        }
        "work_order_type" => {
            let c = code.ok_or_else(|| target_required("work_order_type"))?;
            let id = type_id(db, "work_order_types", &c)
                .await?
                .ok_or_else(|| target_unknown(&c))?;
            Target {
                work_order_type_id: Some(id),
                ..Target::default()
            }
        }
        "permit_type" => {
            let c = code.ok_or_else(|| target_required("permit_type"))?;
            let id = type_id(db, "permit_types", &c)
                .await?
                .ok_or_else(|| target_unknown(&c))?;
            Target {
                permit_type_id: Some(id),
                ..Target::default()
            }
        }
        _ => {
            let c = code.ok_or_else(|| target_required("loto_role"))?;
            validate_role(&c)?;
            if input.coverage.as_deref() == Some("any") {
                return Err(super::issue(
                    "REQUIREMENT_ROLE_COVERAGE",
                    "A LOTO role requirement applies to the person acting in the role.",
                    &[],
                ));
            }
            Target {
                role_code: Some(c),
                ..Target::default()
            }
        }
    };

    match (input.certification_type_id, input.skill_reference_value_id) {
        (Some(ct), None) => {
            if !exists(db, "SELECT 1 AS x FROM certification_types WHERE id = ?", ct.into()).await? {
                return Err(AppError::NotFound {
                    entity: "CertificationType".into(),
                    id: ct.to_string(),
                });
            }
        }
        (None, Some(skill)) => {
            if !exists(
                db,
                "SELECT 1 AS x FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE rv.id = ? AND rd.code = 'PERSONNEL.SKILLS'",
                skill.into(),
            )
            .await?
            {
                return Err(super::issue(
                    "REQUIREMENT_SKILL_UNKNOWN",
                    "This skill is not in the skills catalogue.",
                    &[],
                ));
            }
            if !input.min_proficiency_level.is_some_and(|l| (1..=5).contains(&l)) {
                return Err(super::issue(
                    "REQUIREMENT_LEVEL_RANGE",
                    "Choose the minimum skill level (1 to 5).",
                    &[],
                ));
            }
        }
        _ => {
            return Err(super::issue(
                "REQUIREMENT_QUALIFICATION_REQUIRED",
                "Choose one certification or one skill.",
                &[],
            ))
        }
    }
    if let Some(cov) = &input.coverage {
        if cov != "each" && cov != "any" {
            return Err(super::issue(
                "REQUIREMENT_COVERAGE_UNKNOWN",
                "Coverage must be 'every person' or 'at least one person'.",
                &[],
            ));
        }
    }
    Ok(target)
}

/// Refuses a second active requirement for the same target and qualification (offline devices
/// could still both add one; the gate merges duplicates).
async fn refuse_duplicate(
    db: &impl ConnectionTrait,
    input: &QualificationRequirementUpsertInput,
    target: &Target,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM qualification_requirements \
             WHERE is_active = 1 AND target_kind = ? AND id <> ? \
               AND COALESCE(work_order_id, 0) = COALESCE(?, 0) \
               AND COALESCE(work_order_type_id, 0) = COALESCE(?, 0) \
               AND COALESCE(permit_type_id, 0) = COALESCE(?, 0) \
               AND UPPER(COALESCE(target_code, '')) = UPPER(COALESCE(?, '')) \
               AND COALESCE(certification_type_id, 0) = COALESCE(?, 0) \
               AND COALESCE(skill_reference_value_id, 0) = COALESCE(?, 0)",
            [
                input.target_kind.clone().into(),
                input.id.unwrap_or(0).into(),
                target.work_order_id.into(),
                target.work_order_type_id.into(),
                target.permit_type_id.into(),
                target.role_code.clone().into(),
                input.certification_type_id.into(),
                input.skill_reference_value_id.into(),
            ],
        ))
        .await?;
    if row.is_some() {
        return Err(super::issue(
            "REQUIREMENT_DUPLICATE",
            "This requirement already exists for this target.",
            &[],
        ));
    }
    Ok(())
}

pub async fn upsert_requirement(
    db: &DatabaseConnection,
    input: QualificationRequirementUpsertInput,
    actor_id: i64,
) -> AppResult<QualificationRequirement> {
    let txn = db.begin().await?;
    let target = validate(&txn, &input).await?;
    let is_active = input.is_active.unwrap_or(true);
    if is_active {
        refuse_duplicate(&txn, &input, &target).await?;
    }
    let before = match input.id {
        Some(id) => Some(get_requirement(&txn, id).await?),
        None => None,
    };
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // A level only means something for a skill.
    let level = input.skill_reference_value_id.and(input.min_proficiency_level);
    let notes = input
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let coverage = input.coverage.clone().unwrap_or_else(|| "each".to_string());

    let id = if let Some(id) = input.id {
        let current = get_requirement(&txn, id).await?;
        let expected = input.expected_row_version.unwrap_or(current.row_version);
        let res = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE qualification_requirements SET target_kind = ?, work_order_id = ?, work_order_type_id = ?, \
                 permit_type_id = ?, target_code = ?, \
                 certification_type_id = ?, skill_reference_value_id = ?, min_proficiency_level = ?, coverage = ?, \
                 notes = ?, is_active = ?, row_version = row_version + 1, updated_at = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.target_kind.clone().into(),
                    target.work_order_id.into(),
                    target.work_order_type_id.into(),
                    target.permit_type_id.into(),
                    target.role_code.clone().into(),
                    input.certification_type_id.into(),
                    input.skill_reference_value_id.into(),
                    level.into(),
                    coverage.into(),
                    notes.into(),
                    i64::from(is_active).into(),
                    now.into(),
                    id.into(),
                    expected.into(),
                ],
            ))
            .await?;
        if res.rows_affected() == 0 {
            return Err(super::issue(
                "RECORD_CHANGED_ELSEWHERE",
                "This record was changed by someone else; reload and try again.",
                &[],
            ));
        }
        id
    } else {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO qualification_requirements \
             (entity_sync_id, target_kind, work_order_id, work_order_type_id, permit_type_id, target_code, \
              certification_type_id, skill_reference_value_id, \
              min_proficiency_level, coverage, notes, is_active, row_version, created_by_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                input.target_kind.clone().into(),
                target.work_order_id.into(),
                target.work_order_type_id.into(),
                target.permit_type_id.into(),
                target.role_code.clone().into(),
                input.certification_type_id.into(),
                input.skill_reference_value_id.into(),
                level.into(),
                coverage.into(),
                notes.into(),
                i64::from(is_active).into(),
                actor_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
        txn.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("qualification requirement: no row id")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?
    };
    let saved = get_requirement(&txn, id).await?;
    // A requirement protects people: every change is audited, before and after.
    crate::audit::writer::write_audit_event_in(
        &txn,
        crate::audit::writer::AuditEventInput {
            action_code: if before.is_some() {
                "qualification.requirement.update"
            } else {
                "qualification.requirement.create"
            }
            .to_string(),
            target_type: Some("qualification_requirement".to_string()),
            target_id: Some(id.to_string()),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "before": before.as_ref().map(audit_view),
                "after": audit_view(&saved),
            })),
        },
    )
    .await?;
    txn.commit().await?;
    Ok(saved)
}

fn audit_view(r: &QualificationRequirement) -> serde_json::Value {
    serde_json::json!({
        "target_kind": r.target_kind,
        "work_order_id": r.work_order_id,
        "target": r.target_code,
        "certification": r.certification_code,
        "skill": r.skill_code,
        "min_level": r.min_proficiency_level,
        "coverage": r.coverage,
        "active": r.is_active,
    })
}

/// Whether an edit weakens a requirement in force — deactivating it, moving it to another target
/// or another qualification, lowering its level or narrowing its coverage: such a change needs
/// `trn.certify` and a fresh step-up, like an override.
pub async fn is_weakening_change(
    db: &impl ConnectionTrait,
    input: &QualificationRequirementUpsertInput,
) -> AppResult<bool> {
    let Some(id) = input.id else {
        return Ok(false);
    };
    let current = get_requirement(db, id).await?;
    if !current.is_active {
        return Ok(false);
    }
    if !input.is_active.unwrap_or(true) {
        return Ok(true);
    }
    let same_target = current.target_kind == input.target_kind
        && match input.target_kind.as_str() {
            "work_order" => current.work_order_id == input.work_order_id,
            _ => current
                .target_code
                .as_deref()
                .zip(input.target_code.as_deref())
                .is_some_and(|(a, b)| a.trim().eq_ignore_ascii_case(b.trim())),
        };
    let same_qualification = current.certification_type_id == input.certification_type_id
        && current.skill_reference_value_id == input.skill_reference_value_id;
    let not_lower = match (current.min_proficiency_level, input.min_proficiency_level) {
        (Some(was), Some(now)) => now >= was,
        _ => true,
    };
    let not_narrower = !(current.coverage == "each" && input.coverage.as_deref() == Some("any"));
    Ok(!(same_target && same_qualification && not_lower && not_narrower))
}
