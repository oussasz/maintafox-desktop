//! Qualification gate — does a person hold what a job requires?
//!
//! Benchmarks: SAP PM task-list qualifications checked at work-centre assignment, Maximo
//! craft / skill-level / qualification requirements on job plans and labour assignment.
//!
//! A job's requirements come from `qualification_requirements` (this work order, its work-order
//! type, the type of each permit linked to it, a LOTO role) and from the legacy permit-type
//! requirement profiles (`qualification_requirement_profiles` + `qualification_profile_skills`).
//! Each requirement asks for a certification type (verified, not expired on the day of the work)
//! or a skill at a minimum level (not self-declared-only, not expired).
//!
//! `coverage = each`: every person assigned must hold it — a person who does not is blocked.
//! `coverage = any`: at least one person of the crew must hold it.
//!
//! A blocked person can still be assigned when an authorised supervisor records an override
//! (`qualification_overrides`, append-only, plus an `audit_events` row). An override covers the
//! gaps it listed; a new gap (another requirement added later) blocks again.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult, AppValidationIssue};

use super::queries::{parse_iso_date, DEFAULT_RENEWAL_LEAD_DAYS};

/// LOTO roles a qualification can be required for (`qualification_requirements.target_code`
/// when `target_kind = 'loto_role'`).
pub const LOTO_ROLES: &[&str] = &["isolation_apply", "isolation_verify", "removal_verify"];

/// Shortest override justification accepted (mirrors the table CHECK).
pub const MIN_OVERRIDE_REASON_CHARS: usize = 10;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// One thing a job asks of the people doing it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobRequirement {
    /// `qualification_requirements.id`; `None` for a requirement read from a permit profile.
    pub requirement_id: Option<i64>,
    /// `work_order` | `work_order_type` | `permit_type` | `loto_role` | `permit_profile`.
    pub source_kind: String,
    /// Work-order type code, permit type code, LOTO role or profile name.
    pub source_code: Option<String>,
    pub certification_type_id: Option<i64>,
    pub skill_reference_value_id: Option<i64>,
    /// Certification type code or skill code.
    pub code: Option<String>,
    /// Certification type name or skill label.
    pub label: String,
    pub min_proficiency_level: Option<i64>,
    /// `each` | `any`.
    pub coverage: String,
}

impl JobRequirement {
    /// Stable key of what is required (two sources asking for the same thing share it).
    pub fn key(&self) -> String {
        match (self.certification_type_id, self.skill_reference_value_id) {
            (Some(ct), _) => format!("cert:{ct}"),
            (None, Some(skill)) => format!("skill:{skill}"),
            (None, None) => "unknown".to_string(),
        }
    }

    fn kind(&self) -> &'static str {
        if self.certification_type_id.is_some() {
            "certification"
        } else {
            "skill"
        }
    }
}

/// What a person lacks for one requirement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationGap {
    pub requirement_key: String,
    /// `certification` | `skill`.
    pub kind: String,
    pub code: Option<String>,
    pub label: String,
    /// `missing` | `expired` | `expires_before_work` | `pending_verification` | `rejected` |
    /// `level_too_low` | `unvalidated`.
    pub status: String,
    pub expires_at: Option<String>,
    pub held_level: Option<i64>,
    pub required_level: Option<i64>,
    pub source_kind: String,
    pub source_code: Option<String>,
}

/// A certification the person holds but that runs out soon (within its renewal notice).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationWarning {
    pub requirement_key: String,
    pub code: Option<String>,
    pub label: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationOverrideInfo {
    pub id: i64,
    pub reason: String,
    pub approved_by_id: i64,
    pub approved_by_name: Option<String>,
    pub approved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonQualificationCheck {
    pub personnel_id: i64,
    pub full_name: String,
    pub employee_code: String,
    /// How the person is on the job: `labor`, `responsible`, `scheduled`, `candidate`, `role`.
    pub roles: Vec<String>,
    /// No gap, or every gap covered by an override.
    pub qualified: bool,
    pub gaps: Vec<QualificationGap>,
    pub warnings: Vec<QualificationWarning>,
    /// Latest override covering this person on this job, when the gaps are overridden.
    pub override_info: Option<QualificationOverrideInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WoQualificationAssessment {
    pub work_order_id: i64,
    /// Day the qualifications must still be valid on (planned end, never before today).
    pub work_day: String,
    pub requirements: Vec<JobRequirement>,
    pub people: Vec<PersonQualificationCheck>,
    /// `any`-coverage requirements nobody on the job holds (team members count when only a
    /// team is assigned).
    pub uncovered: Vec<JobRequirement>,
    pub assessed_team_id: Option<i64>,
    /// Nobody blocked and nothing uncovered.
    pub ok: bool,
}

impl WoQualificationAssessment {
    /// People blocked (gaps not covered by an override).
    pub fn blocked_people(&self) -> impl Iterator<Item = &PersonQualificationCheck> {
        self.people.iter().filter(|p| !p.qualified)
    }
}

/// Override recorded by a supervisor who passed the permission and step-up checks.
#[derive(Debug, Clone, Deserialize)]
pub struct QualificationOverrideInput {
    pub reason: String,
    pub approved_by_id: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Small helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("qualification gate decode '{field}': {err}"))
}

fn today() -> NaiveDate {
    Utc::now().date_naive()
}

fn now_ts() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The personnel record linked to a login, if any.
pub async fn personnel_for_user(db: &impl ConnectionTrait, user_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ua.personnel_id FROM user_accounts ua \
             JOIN personnel p ON p.id = ua.personnel_id \
             WHERE ua.id = ? AND ua.deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<Option<i64>>("", "personnel_id").ok().flatten()))
}

async fn person_identity(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<(String, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT full_name, employee_code FROM personnel WHERE id = ?",
            [personnel_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: personnel_id.to_string(),
        })?;
    Ok((
        row.try_get("", "full_name").map_err(|e| decode_err("full_name", e))?,
        row.try_get("", "employee_code")
            .map_err(|e| decode_err("employee_code", e))?,
    ))
}

/// Keeps one requirement per key: `each` wins over `any`, the highest level wins.
fn merge_requirements(rows: Vec<JobRequirement>) -> Vec<JobRequirement> {
    let mut by_key: BTreeMap<String, JobRequirement> = BTreeMap::new();
    for row in rows {
        match by_key.get_mut(&row.key()) {
            None => {
                by_key.insert(row.key(), row);
            }
            Some(existing) => {
                if row.coverage == "each" && existing.coverage != "each" {
                    existing.coverage = "each".to_string();
                    existing.source_kind = row.source_kind.clone();
                    existing.source_code = row.source_code.clone();
                    existing.requirement_id = row.requirement_id;
                }
                if row.min_proficiency_level > existing.min_proficiency_level {
                    existing.min_proficiency_level = row.min_proficiency_level;
                }
            }
        }
    }
    by_key.into_values().collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Requirements of a job
// ═══════════════════════════════════════════════════════════════════════════════

const REQUIREMENT_SELECT: &str = "SELECT qr.id, qr.target_kind, qr.target_code, qr.certification_type_id, \
     qr.skill_reference_value_id, qr.min_proficiency_level, qr.coverage, \
     ct.code AS cert_code, ct.name AS cert_name, rv.code AS skill_code, rv.label AS skill_label \
     FROM qualification_requirements qr \
     LEFT JOIN certification_types ct ON ct.id = qr.certification_type_id \
     LEFT JOIN reference_values rv ON rv.id = qr.skill_reference_value_id \
     WHERE qr.is_active = 1";

fn map_requirement(row: &sea_orm::QueryResult) -> AppResult<JobRequirement> {
    let cert_id: Option<i64> = row
        .try_get("", "certification_type_id")
        .map_err(|e| decode_err("certification_type_id", e))?;
    let cert_code: Option<String> = row.try_get("", "cert_code").ok().flatten();
    let cert_name: Option<String> = row.try_get("", "cert_name").ok().flatten();
    let skill_code: Option<String> = row.try_get("", "skill_code").ok().flatten();
    let skill_label: Option<String> = row.try_get("", "skill_label").ok().flatten();
    let (code, label) = if cert_id.is_some() {
        (cert_code.clone(), cert_name.or(cert_code).unwrap_or_default())
    } else {
        (skill_code.clone(), skill_label.or(skill_code).unwrap_or_default())
    };
    Ok(JobRequirement {
        requirement_id: Some(row.try_get("", "id").map_err(|e| decode_err("id", e))?),
        source_kind: row
            .try_get("", "target_kind")
            .map_err(|e| decode_err("target_kind", e))?,
        source_code: row.try_get("", "target_code").ok().flatten(),
        certification_type_id: cert_id,
        skill_reference_value_id: row
            .try_get("", "skill_reference_value_id")
            .map_err(|e| decode_err("skill_reference_value_id", e))?,
        code,
        label,
        min_proficiency_level: row
            .try_get("", "min_proficiency_level")
            .map_err(|e| decode_err("min_proficiency_level", e))?,
        coverage: row.try_get("", "coverage").map_err(|e| decode_err("coverage", e))?,
    })
}

fn unreadable_profile(name: &str, err: impl std::fmt::Display) -> AppError {
    super::issue(
        "QUALIFICATION_PROFILE_UNREADABLE",
        "A qualification profile cannot be read; fix it before assigning people.",
        &[("profile", name.to_string()), ("detail", err.to_string())],
    )
}

/// Requirements of the legacy permit-type profiles (every worker under such a permit).
async fn permit_profile_requirements(
    db: &impl ConnectionTrait,
    permit_type_codes: &[String],
) -> AppResult<Vec<JobRequirement>> {
    if permit_type_codes.is_empty() {
        return Ok(Vec::new());
    }
    let profiles = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, profile_name, required_certification_type_ids_json, applies_to_permit_type_codes_json \
             FROM qualification_requirement_profiles"
                .to_string(),
        ))
        .await?;
    let wanted: BTreeSet<String> = permit_type_codes.iter().map(|c| c.to_uppercase()).collect();
    let mut out = Vec::new();
    for profile in profiles {
        let name: String = profile
            .try_get("", "profile_name")
            .map_err(|e| decode_err("profile_name", e))?;
        let codes_json: String = profile
            .try_get("", "applies_to_permit_type_codes_json")
            .map_err(|e| decode_err("applies_to_permit_type_codes_json", e))?;
        // A malformed stored profile is an error: reading it as "no requirement" would let
        // everybody through.
        let codes: Vec<String> = serde_json::from_str(&codes_json).map_err(|e| unreadable_profile(&name, e))?;
        let Some(matched) = codes.iter().find(|c| wanted.contains(&c.to_uppercase())) else {
            continue;
        };
        let ids_json: String = profile
            .try_get("", "required_certification_type_ids_json")
            .map_err(|e| decode_err("required_certification_type_ids_json", e))?;
        let ids: Vec<i64> = serde_json::from_str(&ids_json).map_err(|e| unreadable_profile(&name, e))?;
        for ct_id in ids {
            let ct = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT code, name FROM certification_types WHERE id = ?",
                    [ct_id.into()],
                ))
                .await?
                .ok_or_else(|| unreadable_profile(&name, format!("certification type {ct_id} does not exist")))?;
            out.push(JobRequirement {
                requirement_id: None,
                source_kind: "permit_profile".to_string(),
                source_code: Some(format!("{name} ({matched})")),
                certification_type_id: Some(ct_id),
                skill_reference_value_id: None,
                code: ct.try_get::<String>("", "code").ok(),
                label: ct.try_get::<String>("", "name").map_err(|e| decode_err("name", e))?,
                min_proficiency_level: None,
                coverage: "each".to_string(),
            });
        }
        let profile_id: i64 = profile.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let skills = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT qps.reference_value_id, qps.min_proficiency_level, rv.code, rv.label \
                 FROM qualification_profile_skills qps \
                 JOIN reference_values rv ON rv.id = qps.reference_value_id \
                 WHERE qps.profile_id = ? AND qps.is_required = 1",
                [profile_id.into()],
            ))
            .await?;
        for s in skills {
            out.push(JobRequirement {
                requirement_id: None,
                source_kind: "permit_profile".to_string(),
                source_code: Some(format!("{name} ({matched})")),
                certification_type_id: None,
                skill_reference_value_id: Some(
                    s.try_get("", "reference_value_id")
                        .map_err(|e| decode_err("reference_value_id", e))?,
                ),
                code: s.try_get("", "code").ok(),
                label: s.try_get("", "label").map_err(|e| decode_err("label", e))?,
                min_proficiency_level: s.try_get("", "min_proficiency_level").ok(),
                coverage: "each".to_string(),
            });
        }
    }
    Ok(out)
}

/// Every requirement that applies to a work order (merged per certification / skill).
pub async fn work_order_requirements(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<JobRequirement>> {
    let permit_codes: Vec<String> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT pt.code FROM work_permits wp \
             JOIN permit_types pt ON pt.id = wp.permit_type_id \
             WHERE wp.linked_work_order_id = ? AND wp.status <> 'cancelled'",
            [wo_id.into()],
        ))
        .await?
        .iter()
        .map(|r| r.try_get::<String>("", "code").map_err(|e| decode_err("code", e)))
        .collect::<AppResult<_>>()?;

    let mut sql = format!(
        "{REQUIREMENT_SELECT} AND ( \
            (qr.target_kind = 'work_order' AND qr.work_order_id = ?) \
         OR (qr.target_kind = 'work_order_type' AND UPPER(qr.target_code) = \
                (SELECT UPPER(wot.code) FROM work_orders wo JOIN work_order_types wot ON wot.id = wo.type_id \
                  WHERE wo.id = ?))"
    );
    let mut values: Vec<sea_orm::Value> = vec![wo_id.into(), wo_id.into()];
    if !permit_codes.is_empty() {
        let marks = vec!["?"; permit_codes.len()].join(", ");
        sql.push_str(&format!(
            " OR (qr.target_kind = 'permit_type' AND UPPER(qr.target_code) IN ({marks}))"
        ));
        values.extend(permit_codes.iter().map(|c| sea_orm::Value::from(c.to_uppercase())));
    }
    sql.push_str(") ORDER BY qr.id");

    let mut rows: Vec<JobRequirement> = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .iter()
        .map(map_requirement)
        .collect::<AppResult<_>>()?;
    rows.extend(permit_profile_requirements(db, &permit_codes).await?);
    Ok(merge_requirements(rows))
}

/// Requirements of a LOTO role (every person acting in that role must hold them).
pub async fn loto_role_requirements(db: &impl ConnectionTrait, role: &str) -> AppResult<Vec<JobRequirement>> {
    let rows: Vec<JobRequirement> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{REQUIREMENT_SELECT} AND qr.target_kind = 'loto_role' AND qr.target_code = ? ORDER BY qr.id"),
            [role.into()],
        ))
        .await?
        .iter()
        .map(map_requirement)
        .collect::<AppResult<_>>()?;
    // A role requirement is always personal.
    Ok(merge_requirements(rows)
        .into_iter()
        .map(|mut r| {
            r.coverage = "each".to_string();
            r
        })
        .collect())
}

// ═══════════════════════════════════════════════════════════════════════════════
// What a person holds
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) struct HeldCert {
    pub certification_type_id: i64,
    pub expires_at: Option<String>,
    pub verification_status: String,
    pub renewal_lead_days: Option<i64>,
}

pub(crate) struct HeldSkill {
    pub reference_value_id: i64,
    pub level: i64,
    pub valid_to: Option<String>,
    pub self_declared_unvalidated: bool,
}

pub(crate) struct Holdings {
    pub certs: Vec<HeldCert>,
    pub skills: Vec<HeldSkill>,
}

pub(crate) async fn load_holdings(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<Holdings> {
    let certs = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pc.certification_type_id, pc.expires_at, pc.verification_status, ct.renewal_lead_days \
             FROM personnel_certifications pc \
             JOIN certification_types ct ON ct.id = pc.certification_type_id \
             WHERE pc.personnel_id = ?",
            [personnel_id.into()],
        ))
        .await?
        .iter()
        .map(|r| {
            Ok(HeldCert {
                certification_type_id: r
                    .try_get("", "certification_type_id")
                    .map_err(|e| decode_err("certification_type_id", e))?,
                expires_at: r
                    .try_get::<Option<String>>("", "expires_at")
                    .map_err(|e| decode_err("expires_at", e))?
                    .filter(|s| !s.trim().is_empty()),
                verification_status: r
                    .try_get("", "verification_status")
                    .map_err(|e| decode_err("verification_status", e))?,
                renewal_lead_days: r
                    .try_get::<Option<i64>>("", "renewal_lead_days")
                    .map_err(|e| decode_err("renewal_lead_days", e))?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let skills = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reference_value_id, proficiency_level, valid_to, source_type, is_validated \
             FROM personnel_skills WHERE personnel_id = ?",
            [personnel_id.into()],
        ))
        .await?
        .iter()
        .map(|r| {
            let source: String = r.try_get("", "source_type").map_err(|e| decode_err("source_type", e))?;
            let validated: i64 = r
                .try_get("", "is_validated")
                .map_err(|e| decode_err("is_validated", e))?;
            Ok(HeldSkill {
                reference_value_id: r
                    .try_get("", "reference_value_id")
                    .map_err(|e| decode_err("reference_value_id", e))?,
                level: r
                    .try_get("", "proficiency_level")
                    .map_err(|e| decode_err("proficiency_level", e))?,
                valid_to: r
                    .try_get::<Option<String>>("", "valid_to")
                    .map_err(|e| decode_err("valid_to", e))?
                    .filter(|s| !s.trim().is_empty()),
                self_declared_unvalidated: source == "self_declared" && validated == 0,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(Holdings { certs, skills })
}

fn gap(req: &JobRequirement, status: &str, expires_at: Option<String>, held_level: Option<i64>) -> QualificationGap {
    QualificationGap {
        requirement_key: req.key(),
        kind: req.kind().to_string(),
        code: req.code.clone(),
        label: req.label.clone(),
        status: status.to_string(),
        expires_at,
        held_level,
        required_level: req.min_proficiency_level,
        source_kind: req.source_kind.clone(),
        source_code: req.source_code.clone(),
    }
}

/// Evaluates one requirement against what the person holds, valid through `work_day`.
/// Returns the gap (if any) and an expiry warning (if the qualifying certificate runs out
/// within its renewal notice).
pub(crate) fn evaluate(
    req: &JobRequirement,
    held: &Holdings,
    work_day: NaiveDate,
    today: NaiveDate,
) -> (Option<QualificationGap>, Option<QualificationWarning>) {
    if let Some(ct_id) = req.certification_type_id {
        let mine: Vec<&HeldCert> = held.certs.iter().filter(|c| c.certification_type_id == ct_id).collect();
        // Expiry of a certificate: `None` = never expires, `Some(None)` = unreadable (fails
        // closed: treated as expired), `Some(Some(day))`.
        let expiry_of = |c: &HeldCert| c.expires_at.as_deref().map(parse_iso_date);
        let best = mine
            .iter()
            .filter(|c| c.verification_status == "verified")
            .max_by_key(|c| match expiry_of(c) {
                None => Some(NaiveDate::MAX),
                Some(parsed) => parsed,
            });
        if let Some(best) = best {
            return match expiry_of(best) {
                None => (None, None),
                Some(None) => (Some(gap(req, "expired", best.expires_at.clone(), None)), None),
                Some(Some(exp)) if exp >= work_day => {
                    let lead = best.renewal_lead_days.unwrap_or(DEFAULT_RENEWAL_LEAD_DAYS).max(0);
                    let warning = (exp <= today + Duration::days(lead)).then(|| QualificationWarning {
                        requirement_key: req.key(),
                        code: req.code.clone(),
                        label: req.label.clone(),
                        expires_at: best.expires_at.clone().unwrap_or_default(),
                    });
                    (None, warning)
                }
                Some(Some(exp)) if exp >= today => (
                    Some(gap(req, "expires_before_work", best.expires_at.clone(), None)),
                    None,
                ),
                Some(Some(_)) => (Some(gap(req, "expired", best.expires_at.clone(), None)), None),
            };
        }
        if mine.iter().any(|c| c.verification_status == "pending") {
            return (Some(gap(req, "pending_verification", None, None)), None);
        }
        if mine.iter().any(|c| c.verification_status == "rejected") {
            return (Some(gap(req, "rejected", None, None)), None);
        }
        return (Some(gap(req, "missing", None, None)), None);
    }

    let Some(skill_id) = req.skill_reference_value_id else {
        return (Some(gap(req, "missing", None, None)), None);
    };
    let Some(skill) = held.skills.iter().find(|s| s.reference_value_id == skill_id) else {
        return (Some(gap(req, "missing", None, None)), None);
    };
    if let Some(valid_to) = &skill.valid_to {
        match parse_iso_date(valid_to) {
            Some(d) if d >= work_day => {}
            _ => {
                return (
                    Some(gap(req, "expired", Some(valid_to.clone()), Some(skill.level))),
                    None,
                )
            }
        }
    }
    if skill.self_declared_unvalidated {
        return (Some(gap(req, "unvalidated", None, Some(skill.level))), None);
    }
    if let Some(min) = req.min_proficiency_level {
        if skill.level < min {
            return (Some(gap(req, "level_too_low", None, Some(skill.level))), None);
        }
    }
    (None, None)
}

/// Gaps and warnings of one person against the `each` requirements of a job.
fn person_gaps(
    requirements: &[JobRequirement],
    held: &Holdings,
    work_day: NaiveDate,
) -> (Vec<QualificationGap>, Vec<QualificationWarning>) {
    let today = today();
    let mut gaps = Vec::new();
    let mut warnings = Vec::new();
    for req in requirements.iter().filter(|r| r.coverage == "each") {
        let (g, w) = evaluate(req, held, work_day, today);
        gaps.extend(g);
        warnings.extend(w);
    }
    (gaps, warnings)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Overrides
// ═══════════════════════════════════════════════════════════════════════════════

enum OverrideContext<'a> {
    WorkOrder(i64),
    PermitRole { permit_id: i64, role: &'a str },
}

/// Latest override for the person on this job and every gap key the overrides listed.
async fn overrides_for(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    context: &OverrideContext<'_>,
) -> AppResult<(Option<QualificationOverrideInfo>, BTreeSet<String>)> {
    let (where_sql, values): (&str, Vec<sea_orm::Value>) = match context {
        OverrideContext::WorkOrder(wo_id) => (
            "qo.context_kind = 'work_order' AND qo.work_order_id = ?",
            vec![personnel_id.into(), (*wo_id).into()],
        ),
        OverrideContext::PermitRole { permit_id, role } => (
            "qo.context_kind = 'permit_role' AND qo.permit_id = ? AND qo.role_code = ?",
            vec![personnel_id.into(), (*permit_id).into(), (*role).into()],
        ),
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT qo.id, qo.reason, qo.approved_by_id, qo.approved_at, qo.gaps_json, \
                        COALESCE(ua.display_name, ua.username) AS approver \
                 FROM qualification_overrides qo \
                 LEFT JOIN user_accounts ua ON ua.id = qo.approved_by_id \
                 WHERE qo.personnel_id = ? AND {where_sql} \
                 ORDER BY qo.approved_at DESC, qo.id DESC"
            ),
            values,
        ))
        .await?;
    let mut covered = BTreeSet::new();
    let mut latest = None;
    for row in rows {
        let gaps_json: String = row.try_get("", "gaps_json").map_err(|e| decode_err("gaps_json", e))?;
        // An unreadable override covers nothing (fail closed).
        let gaps: Vec<QualificationGap> = serde_json::from_str(&gaps_json).unwrap_or_default();
        covered.extend(gaps.into_iter().map(|g| g.requirement_key));
        if latest.is_none() {
            latest = Some(QualificationOverrideInfo {
                id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
                approved_by_id: row
                    .try_get("", "approved_by_id")
                    .map_err(|e| decode_err("approved_by_id", e))?,
                approved_by_name: row.try_get("", "approver").ok().flatten(),
                approved_at: row
                    .try_get("", "approved_at")
                    .map_err(|e| decode_err("approved_at", e))?,
            });
        }
    }
    Ok((latest, covered))
}

async fn check_person(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    requirements: &[JobRequirement],
    work_day: NaiveDate,
    context: &OverrideContext<'_>,
    roles: Vec<String>,
) -> AppResult<PersonQualificationCheck> {
    let (full_name, employee_code) = person_identity(db, personnel_id).await?;
    let held = load_holdings(db, personnel_id).await?;
    let (gaps, warnings) = person_gaps(requirements, &held, work_day);
    let (override_info, covered) = if gaps.is_empty() {
        (None, BTreeSet::new())
    } else {
        overrides_for(db, personnel_id, context).await?
    };
    let overridden = !gaps.is_empty() && gaps.iter().all(|g| covered.contains(&g.requirement_key));
    Ok(PersonQualificationCheck {
        personnel_id,
        full_name,
        employee_code,
        roles,
        qualified: gaps.is_empty() || overridden,
        override_info: if overridden { override_info } else { None },
        gaps,
        warnings,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Work orders
// ═══════════════════════════════════════════════════════════════════════════════

struct WoFacts {
    work_day: NaiveDate,
    assigned_team_id: Option<i64>,
}

async fn wo_facts(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoFacts> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT planned_start, planned_end, assigned_team_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let planned_end: Option<String> = row.try_get("", "planned_end").ok().flatten();
    let planned_start: Option<String> = row.try_get("", "planned_start").ok().flatten();
    let planned = planned_end.or(planned_start).as_deref().and_then(parse_iso_date);
    Ok(WoFacts {
        work_day: planned.map_or(today(), |d| d.max(today())),
        assigned_team_id: row.try_get("", "assigned_team_id").ok().flatten(),
    })
}

/// People on a work order: labour booked, the responsible's personnel record, scheduled assignee.
async fn work_order_people(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<BTreeMap<i64, Vec<String>>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT x.pid, x.role FROM ( \
                SELECT COALESCE(woi.personnel_id, ua.personnel_id) AS pid, 'labor' AS role \
                  FROM work_order_interveners woi \
                  LEFT JOIN user_accounts ua ON ua.id = woi.intervener_id \
                 WHERE woi.work_order_id = ? \
                UNION ALL \
                SELECT ua.personnel_id AS pid, 'responsible' AS role \
                  FROM work_orders wo JOIN user_accounts ua ON ua.id = wo.primary_responsible_id \
                 WHERE wo.id = ? \
                UNION ALL \
                SELECT sc.assigned_personnel_id AS pid, 'scheduled' AS role \
                  FROM schedule_commitments sc \
                 WHERE sc.source_type = 'work_order' AND sc.source_id = ? \
             ) x JOIN personnel p ON p.id = x.pid",
            [wo_id.into(), wo_id.into(), wo_id.into()],
        ))
        .await?;
    let mut people: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for row in rows {
        let pid: i64 = row.try_get("", "pid").map_err(|e| decode_err("pid", e))?;
        let role: String = row.try_get("", "role").map_err(|e| decode_err("role", e))?;
        let roles = people.entry(pid).or_default();
        if !roles.contains(&role) {
            roles.push(role);
        }
    }
    Ok(people)
}

async fn team_members(db: &impl ConnectionTrait, team_id: i64, day: NaiveDate) -> AppResult<Vec<i64>> {
    let day_s = day.to_string();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT p.id FROM personnel p \
             WHERE p.employment_status = 'active' AND ( \
                p.primary_team_id = ? \
                OR EXISTS (SELECT 1 FROM personnel_team_assignments pta \
                           WHERE pta.personnel_id = p.id AND pta.team_id = ? \
                             AND (pta.valid_from IS NULL OR date(pta.valid_from) <= date(?)) \
                             AND (pta.valid_to IS NULL OR date(pta.valid_to) >= date(?))))",
            [team_id.into(), team_id.into(), day_s.clone().into(), day_s.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
        .collect()
}

/// Whole-crew qualification picture of a work order (readiness, planning, the OT screen).
pub async fn assess_work_order(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoQualificationAssessment> {
    let facts = wo_facts(db, wo_id).await?;
    let requirements = work_order_requirements(db, wo_id).await?;
    let people = work_order_people(db, wo_id).await?;
    let context = OverrideContext::WorkOrder(wo_id);

    let mut checks = Vec::with_capacity(people.len());
    let mut holdings: Vec<Holdings> = Vec::with_capacity(people.len());
    for (pid, roles) in people {
        checks.push(check_person(db, pid, &requirements, facts.work_day, &context, roles).await?);
        holdings.push(load_holdings(db, pid).await?);
    }

    // `any` requirements: someone on the job, else (only a team assigned) a team member.
    let any_reqs: Vec<&JobRequirement> = requirements.iter().filter(|r| r.coverage == "any").collect();
    let mut assessed_team_id = None;
    if checks.is_empty() && !any_reqs.is_empty() {
        if let Some(team_id) = facts.assigned_team_id {
            assessed_team_id = Some(team_id);
            for pid in team_members(db, team_id, facts.work_day).await? {
                holdings.push(load_holdings(db, pid).await?);
            }
        }
    }
    let today = today();
    let uncovered: Vec<JobRequirement> = any_reqs
        .into_iter()
        .filter(|req| {
            !holdings
                .iter()
                .any(|held| evaluate(req, held, facts.work_day, today).0.is_none())
        })
        .cloned()
        .collect();

    let ok = checks.iter().all(|c| c.qualified) && uncovered.is_empty();
    Ok(WoQualificationAssessment {
        work_order_id: wo_id,
        work_day: facts.work_day.to_string(),
        requirements,
        people: checks,
        uncovered,
        assessed_team_id,
        ok,
    })
}

/// The tenant writes in English (`locale.default_language`, else the tenant setting); French
/// otherwise, the product default.
pub async fn tenant_writes_english(db: &impl ConnectionTrait) -> AppResult<bool> {
    let from_config = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = 'locale.default_language'".to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<String>("", "value").ok());
    let raw = match from_config {
        Some(v) => Some(v),
        None => db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT setting_value_json FROM app_settings \
                 WHERE setting_key = 'locale.primary_language' AND setting_scope = 'tenant' LIMIT 1"
                    .to_string(),
            ))
            .await?
            .and_then(|r| r.try_get::<String>("", "setting_value_json").ok())
            .map(|json| serde_json::from_str::<String>(&json).unwrap_or(json)),
    };
    Ok(raw.is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("en")))
}

/// Readiness blocker text for a work order whose crew does not meet its requirements
/// (`None` when it does). Names who lacks what, and the `any` requirements nobody covers.
pub fn readiness_message(assessment: &WoQualificationAssessment, english: bool) -> Option<String> {
    if assessment.ok {
        return None;
    }
    let mut parts: Vec<String> = assessment
        .people
        .iter()
        .filter(|p| !p.qualified)
        .map(|p| {
            let missing = p.gaps.iter().map(|g| g.label.as_str()).collect::<Vec<_>>().join(", ");
            if english {
                format!("{} lacks {missing}", p.full_name)
            } else {
                format!("{} : manque {missing}", p.full_name)
            }
        })
        .collect();
    if !assessment.uncovered.is_empty() {
        let labels = assessment
            .uncovered
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(if english {
            format!("nobody on the job holds {labels}")
        } else {
            format!("personne dans l'équipe ne détient {labels}")
        });
    }
    let head = if english {
        "Crew not qualified"
    } else {
        "Équipe non habilitée"
    };
    Some(format!("{head} — {}.", parts.join(" ; ")))
}

/// One person against a work order's personal (`each`) requirements — used before booking or
/// assigning them.
pub async fn check_person_for_work_order(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    wo_id: i64,
) -> AppResult<PersonQualificationCheck> {
    let facts = wo_facts(db, wo_id).await?;
    let requirements = work_order_requirements(db, wo_id).await?;
    check_person(
        db,
        personnel_id,
        &requirements,
        facts.work_day,
        &OverrideContext::WorkOrder(wo_id),
        vec!["candidate".to_string()],
    )
    .await
}

/// One person against a LOTO role on a permit (valid today).
pub async fn check_person_for_permit_role(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    permit_id: i64,
    role: &str,
) -> AppResult<PersonQualificationCheck> {
    validate_role(role)?;
    let requirements = loto_role_requirements(db, role).await?;
    check_person(
        db,
        personnel_id,
        &requirements,
        today(),
        &OverrideContext::PermitRole { permit_id, role },
        vec!["role".to_string()],
    )
    .await
}

pub(crate) fn validate_role(role: &str) -> AppResult<()> {
    if LOTO_ROLES.contains(&role) {
        Ok(())
    } else {
        Err(super::issue(
            "LOTO_ROLE_UNKNOWN",
            "Unknown LOTO role.",
            &[("role", role.to_string())],
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Enforcement
// ═══════════════════════════════════════════════════════════════════════════════

/// Translation code of a gap (`training:errors.<code>`).
pub fn gap_issue_code(gap: &QualificationGap) -> &'static str {
    match (gap.kind.as_str(), gap.status.as_str()) {
        ("certification", "expired") => "QUALIFICATION_EXPIRED",
        ("certification", "expires_before_work") => "QUALIFICATION_EXPIRES_BEFORE_WORK",
        ("certification", "pending_verification") => "QUALIFICATION_PENDING_VERIFICATION",
        ("certification", "rejected") => "QUALIFICATION_REJECTED",
        ("certification", _) => "QUALIFICATION_MISSING",
        (_, "expired") => "SKILL_EXPIRED",
        (_, "level_too_low") => "SKILL_LEVEL_TOO_LOW",
        (_, "unvalidated") => "SKILL_UNVALIDATED",
        _ => "SKILL_MISSING",
    }
}

fn gap_message(person: &str, gap: &QualificationGap) -> String {
    let expires = gap.expires_at.clone().unwrap_or_default();
    match gap_issue_code(gap) {
        "QUALIFICATION_EXPIRED" => format!("{person}: {} expired on {expires}.", gap.label),
        "QUALIFICATION_EXPIRES_BEFORE_WORK" => {
            format!("{person}: {} expires on {expires}, before the planned work.", gap.label)
        }
        "QUALIFICATION_PENDING_VERIFICATION" => {
            format!("{person}: {} is recorded but not verified yet.", gap.label)
        }
        "QUALIFICATION_REJECTED" => format!("{person}: {} was rejected at verification.", gap.label),
        "QUALIFICATION_MISSING" => format!("{person} does not hold {}.", gap.label),
        "SKILL_EXPIRED" => format!("{person}: skill {} is no longer valid.", gap.label),
        "SKILL_LEVEL_TOO_LOW" => format!(
            "{person}: {} level {} required, level {} held.",
            gap.label,
            gap.required_level.unwrap_or_default(),
            gap.held_level.unwrap_or_default()
        ),
        "SKILL_UNVALIDATED" => format!("{person}: skill {} is self-declared and not validated.", gap.label),
        _ => format!("{person} does not have the skill {}.", gap.label),
    }
}

/// The error a blocked assignment returns: one coded issue per gap, then how to proceed.
pub fn blocked_error(check: &PersonQualificationCheck) -> AppError {
    let mut issues: Vec<AppValidationIssue> = check
        .gaps
        .iter()
        .map(|g| {
            let params: HashMap<String, String> = [
                ("person", check.full_name.clone()),
                ("qualification", g.label.clone()),
                ("expiresAt", g.expires_at.clone().unwrap_or_default()),
                (
                    "requiredLevel",
                    g.required_level.map(|l| l.to_string()).unwrap_or_default(),
                ),
                ("heldLevel", g.held_level.map(|l| l.to_string()).unwrap_or_default()),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
            AppValidationIssue::error_with_related(
                gap_issue_code(g),
                gap_message(&check.full_name, g),
                Some(check.personnel_id),
                params,
            )
        })
        .collect();
    issues.push(AppValidationIssue::error_with_related(
        "QUALIFICATION_GATE_BLOCKED",
        format!(
            "{} cannot be assigned: record the missing qualifications, or have an authorised supervisor override with a reason.",
            check.full_name
        ),
        Some(check.personnel_id),
        HashMap::from([("person".to_string(), check.full_name.clone())]),
    ));
    AppError::OrgValidationFailed(issues)
}

async fn record_override(
    db: &impl ConnectionTrait,
    check: &PersonQualificationCheck,
    context: &OverrideContext<'_>,
    input: &QualificationOverrideInput,
) -> AppResult<i64> {
    let reason = input.reason.trim();
    if reason.chars().count() < MIN_OVERRIDE_REASON_CHARS {
        return Err(super::issue(
            "QUALIFICATION_OVERRIDE_REASON_SHORT",
            "Explain the override (at least 10 characters).",
            &[("min", MIN_OVERRIDE_REASON_CHARS.to_string())],
        ));
    }
    let (kind, wo_id, permit_id, role): (&str, Option<i64>, Option<i64>, Option<&str>) = match context {
        OverrideContext::WorkOrder(wo) => ("work_order", Some(*wo), None, None),
        OverrideContext::PermitRole { permit_id, role } => ("permit_role", None, Some(*permit_id), Some(*role)),
    };
    let gaps_json = serde_json::to_string(&check.gaps)?;
    let now = now_ts();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO qualification_overrides \
         (entity_sync_id, personnel_id, context_kind, work_order_id, permit_id, role_code, gaps_json, reason, \
          approved_by_id, approved_at, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            check.personnel_id.into(),
            kind.into(),
            wo_id.into(),
            permit_id.into(),
            role.map(str::to_string).into(),
            gaps_json.into(),
            reason.to_string().into(),
            input.approved_by_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let id: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("qualification override: no row id")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    crate::audit::writer::write_audit_event_in(
        db,
        crate::audit::writer::AuditEventInput {
            action_code: "qualification.override".to_string(),
            target_type: Some(kind.to_string()),
            target_id: wo_id.or(permit_id).map(|v| v.to_string()),
            actor_id: Some(input.approved_by_id),
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "override_id": id,
                "personnel_id": check.personnel_id,
                "person": check.full_name,
                "role": role,
                "gaps": check.gaps.iter().map(|g| serde_json::json!({
                    "code": gap_issue_code(g),
                    "qualification": g.label,
                    "status": g.status,
                    "expires_at": g.expires_at,
                })).collect::<Vec<_>>(),
                "reason": reason,
            })),
        },
    )
    .await?;
    Ok(id)
}

async fn enforce(
    db: &impl ConnectionTrait,
    check: PersonQualificationCheck,
    context: OverrideContext<'_>,
    override_input: Option<&QualificationOverrideInput>,
) -> AppResult<Option<i64>> {
    if check.qualified {
        return Ok(None);
    }
    match override_input {
        None => Err(blocked_error(&check)),
        Some(input) => record_override(db, &check, &context, input).await.map(Some),
    }
}

/// Refuses to book / assign a person who lacks a personal requirement of the work order, unless
/// an override is given (then recorded and audited). Returns the override id when one was
/// recorded. Call inside the transaction that writes the assignment.
pub async fn enforce_work_order_assignment(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    wo_id: i64,
    override_input: Option<&QualificationOverrideInput>,
) -> AppResult<Option<i64>> {
    let check = check_person_for_work_order(db, personnel_id, wo_id).await?;
    enforce(db, check, OverrideContext::WorkOrder(wo_id), override_input).await
}

/// Same as [`enforce_work_order_assignment`] for a LOTO role on a permit.
pub async fn enforce_permit_role(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    permit_id: i64,
    role: &str,
    override_input: Option<&QualificationOverrideInput>,
) -> AppResult<Option<i64>> {
    let check = check_person_for_permit_role(db, personnel_id, permit_id, role).await?;
    enforce(
        db,
        check,
        OverrideContext::PermitRole { permit_id, role },
        override_input,
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Override log
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationOverrideRow {
    pub id: i64,
    pub personnel_id: i64,
    pub person_name: String,
    pub context_kind: String,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub permit_id: Option<i64>,
    pub permit_code: Option<String>,
    pub role_code: Option<String>,
    pub gaps: Vec<QualificationGap>,
    pub reason: String,
    pub approved_by_id: i64,
    pub approved_by_name: Option<String>,
    pub approved_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QualificationOverrideFilter {
    pub work_order_id: Option<i64>,
    pub personnel_id: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn list_overrides(
    db: &impl ConnectionTrait,
    filter: QualificationOverrideFilter,
) -> AppResult<Vec<QualificationOverrideRow>> {
    let mut sql = String::from(
        "SELECT qo.id, qo.personnel_id, p.full_name, qo.context_kind, qo.work_order_id, wo.code AS wo_code, \
                qo.permit_id, wp.code AS permit_code, qo.role_code, qo.gaps_json, qo.reason, qo.approved_by_id, \
                COALESCE(ua.display_name, ua.username) AS approver, qo.approved_at \
         FROM qualification_overrides qo \
         JOIN personnel p ON p.id = qo.personnel_id \
         LEFT JOIN work_orders wo ON wo.id = qo.work_order_id \
         LEFT JOIN work_permits wp ON wp.id = qo.permit_id \
         LEFT JOIN user_accounts ua ON ua.id = qo.approved_by_id \
         WHERE 1 = 1",
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(wo) = filter.work_order_id {
        sql.push_str(" AND qo.work_order_id = ?");
        values.push(wo.into());
    }
    if let Some(pid) = filter.personnel_id {
        sql.push_str(" AND qo.personnel_id = ?");
        values.push(pid.into());
    }
    sql.push_str(" ORDER BY qo.approved_at DESC, qo.id DESC LIMIT ?");
    values.push(filter.limit.unwrap_or(200).clamp(1, 2000).into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;
    rows.iter()
        .map(|r| {
            let gaps_json: String = r.try_get("", "gaps_json").map_err(|e| decode_err("gaps_json", e))?;
            Ok(QualificationOverrideRow {
                id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                personnel_id: r
                    .try_get("", "personnel_id")
                    .map_err(|e| decode_err("personnel_id", e))?,
                person_name: r.try_get("", "full_name").map_err(|e| decode_err("full_name", e))?,
                context_kind: r
                    .try_get("", "context_kind")
                    .map_err(|e| decode_err("context_kind", e))?,
                work_order_id: r.try_get("", "work_order_id").ok().flatten(),
                work_order_code: r.try_get("", "wo_code").ok().flatten(),
                permit_id: r.try_get("", "permit_id").ok().flatten(),
                permit_code: r.try_get("", "permit_code").ok().flatten(),
                role_code: r.try_get("", "role_code").ok().flatten(),
                gaps: serde_json::from_str(&gaps_json).unwrap_or_default(),
                reason: r.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
                approved_by_id: r
                    .try_get("", "approved_by_id")
                    .map_err(|e| decode_err("approved_by_id", e))?,
                approved_by_name: r.try_get("", "approver").ok().flatten(),
                approved_at: r.try_get("", "approved_at").map_err(|e| decode_err("approved_at", e))?,
            })
        })
        .collect()
}
