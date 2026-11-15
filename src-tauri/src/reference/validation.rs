//! Reference set validation engine.
//!
//! Phase 2 - Sub-phase 03 - File 02 - Sprint S2.
//!
//! Runs structured diagnostics against a draft reference set and produces a
//! persisted validation report. The report gates the draft → validated
//! lifecycle transition: the set can only be validated when `blocking_count = 0`.
//!
//! Validation checks (all return structured issues, not just pass/fail):
//!   - duplicate codes within the set
//!   - missing or blank labels
//!   - hierarchy cycles (A → B → A)
//!   - orphan parent references (parent_id pointing to non-existent value)
//!   - invalid color_hex format
//!   - invalid external_code format for domains that require a pattern
//!   - deactivated values in protected domains without a migration map

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::domains;
use super::protected;
use super::sets;
use super::values;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Severity level for a validation issue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    /// Blocks the draft → validated transition.
    Blocking,
    /// Informational warning; does not block validation.
    Warning,
}

/// A single validation issue found during set diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceValidationIssue {
    /// Machine-readable issue type code.
    pub check: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// Severity: blocking prevents validation, warning is informational.
    pub severity: IssueSeverity,
    /// Optional: the value id that triggered the issue.
    pub value_id: Option<i64>,
    /// Optional: the value code for context.
    pub value_code: Option<String>,
}

/// Complete result of a validation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceValidationResult {
    /// The set that was validated.
    pub set_id: i64,
    /// Overall status: "passed" (no blocking issues) or "failed".
    pub status: String,
    /// Total number of issues (blocking + warning).
    pub issue_count: i64,
    /// Number of blocking issues.
    pub blocking_count: i64,
    /// Full structured issue list.
    pub issues: Vec<ReferenceValidationIssue>,
    /// Persisted report id (from `reference_validation_reports`).
    pub report_id: i64,
}

/// Persisted validation report record for reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceValidationReport {
    pub id: i64,
    pub set_id: i64,
    pub status: String,
    pub issue_count: i64,
    pub blocking_count: i64,
    pub report_json: String,
    pub validated_by_id: Option<i64>,
    pub validated_at: String,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_validation_reports row decode failed for column '{column}': {e}"
    ))
}

fn map_report(row: &QueryResult) -> AppResult<ReferenceValidationReport> {
    Ok(ReferenceValidationReport {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        set_id: row.try_get::<i64>("", "set_id").map_err(|e| decode_err("set_id", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        issue_count: row
            .try_get::<i64>("", "issue_count")
            .map_err(|e| decode_err("issue_count", e))?,
        blocking_count: row
            .try_get::<i64>("", "blocking_count")
            .map_err(|e| decode_err("blocking_count", e))?,
        report_json: row
            .try_get::<String>("", "report_json")
            .map_err(|e| decode_err("report_json", e))?,
        validated_by_id: row
            .try_get::<Option<i64>>("", "validated_by_id")
            .map_err(|e| decode_err("validated_by_id", e))?,
        validated_at: row
            .try_get::<String>("", "validated_at")
            .map_err(|e| decode_err("validated_at", e))?,
    })
}

/// Hex color pattern: 3 or 6 hex digits, optionally prefixed with #.
fn is_valid_color_hex(s: &str) -> bool {
    let s = s.strip_prefix('#').unwrap_or(s);
    let len = s.len();
    (len == 3 || len == 6) && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Runs all diagnostic checks against a draft set and persists the report.
///
/// Returns the structured result including persisted report id. This function
/// does NOT change the set status — the caller (sets::validate_set) uses the
/// result to decide whether the transition is allowed.
pub async fn validate_reference_set(
    db: &DatabaseConnection,
    set_id: i64,
    actor_id: i64,
) -> AppResult<ReferenceValidationResult> {
    // Verify the set exists and is in draft status.
    let set = sets::get_reference_set(db, set_id).await?;
    if set.status != sets::SET_STATUS_DRAFT {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_REF_SET_NOT_VALIDATABLE: Impossible de valider un jeu en statut '{}'. \
             Seul un brouillon ('draft') peut etre valide.",
            set.status
        )]));
    }

    // Load the domain for governance-level and validation_rules_json.
    let domain = domains::get_reference_domain(db, set.domain_id).await?;

    // Load all values in the set.
    let all_values = values::list_values(db, set_id).await?;

    let mut issues: Vec<ReferenceValidationIssue> = Vec::new();

    // ── Check 1: duplicate codes ──────────────────────────────────────────
    check_duplicate_codes(&all_values, &mut issues);

    // ── Check 2: missing labels ───────────────────────────────────────────
    check_missing_labels(&all_values, &mut issues);

    // ── Check 3: hierarchy cycles ─────────────────────────────────────────
    check_hierarchy_cycles(&all_values, &mut issues);

    // ── Check 4: orphan parent references (a parent in an allowed parent domain is not one) ──
    let mut cross_parents: HashSet<i64> = HashSet::new();
    let in_set: HashSet<i64> = all_values.iter().map(|v| v.id).collect();
    for pid in all_values.iter().filter_map(|v| v.parent_id) {
        if !in_set.contains(&pid) && values::is_allowed_cross_domain_parent(db, &domain.code, pid).await? {
            cross_parents.insert(pid);
        }
    }
    check_orphan_parents(&all_values, &cross_parents, &mut issues);

    // ── Check 4b: required parent domain (OT sub-type → maintenance nature) ──
    check_required_parents(db, &domain, &all_values, &mut issues).await?;

    // ── Check 4c: nature tag of request types (EN 13306 nature or empty) ───
    check_nature_tags(db, &domain, &all_values, &mut issues).await?;
    check_unit_dimensions(db, &domain, &all_values, &mut issues).await?;
    check_permit_rules(db, &domain, &all_values, &mut issues).await?;
    check_work_order_type_natures(&domain.code, &all_values, &mut issues);

    // ── Check 5: invalid color format ─────────────────────────────────────
    check_invalid_colors(&all_values, &mut issues);

    // ── Check 6: invalid external-code format ─────────────────────────────
    check_external_code_format(&domain, &all_values, &mut issues);

    // ── Check 7: protected-domain deactivations without migration map ─────
    check_protected_deactivations_without_migration(db, &domain, &all_values, &mut issues).await;

    // Compute summary.
    let blocking_count = issues.iter().filter(|i| i.severity == IssueSeverity::Blocking).count() as i64;
    let issue_count = issues.len() as i64;
    let status = if blocking_count == 0 { "passed" } else { "failed" };

    // Persist the report.
    let report_json = serde_json::to_string(&issues)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to serialize validation issues: {e}")))?;

    let now = crate::clock::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_validation_reports \
             (set_id, status, issue_count, blocking_count, report_json, validated_by_id, validated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            set_id.into(),
            status.into(),
            issue_count.into(),
            blocking_count.into(),
            report_json.clone().into(),
            actor_id.into(),
            now.into(),
        ],
    ))
    .await?;

    // Fetch the persisted report id.
    let report_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_validation_reports \
             WHERE set_id = ? ORDER BY id DESC LIMIT 1",
            [set_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("reference_validation_reports row missing after insert")))?;

    let report_id: i64 = report_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

    Ok(ReferenceValidationResult {
        set_id,
        status: status.to_string(),
        issue_count,
        blocking_count,
        issues,
        report_id,
    })
}

/// Returns the latest validation report for a set, or `NotFound` if none.
pub async fn get_latest_validation_report(
    db: &DatabaseConnection,
    set_id: i64,
) -> AppResult<ReferenceValidationReport> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, set_id, status, issue_count, blocking_count, \
                    report_json, validated_by_id, validated_at \
             FROM reference_validation_reports \
             WHERE set_id = ? ORDER BY id DESC LIMIT 1",
            [set_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceValidationReport".into(),
            id: format!("set_id={set_id}"),
        })?;

    map_report(&row)
}

// ─── Individual checks ────────────────────────────────────────────────────────

/// Check 1: duplicate codes within the set.
///
/// The DB unique index (set_id, code) prevents true duplicates at insert time,
/// but values that differ only in whitespace or were created via raw import
/// could slip through. This check catches case-normalized duplicates.
fn check_duplicate_codes(vals: &[values::ReferenceValue], issues: &mut Vec<ReferenceValidationIssue>) {
    let mut seen: HashMap<String, i64> = HashMap::new();
    for v in vals {
        let normalized = v.code.trim().to_ascii_uppercase();
        if let Some(first_id) = seen.get(&normalized) {
            issues.push(ReferenceValidationIssue {
                check: "duplicate_code".into(),
                message: format!(
                    "Le code '{}' est en double dans ce jeu (conflit avec valeur id={}).",
                    v.code, first_id
                ),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
        } else {
            seen.insert(normalized, v.id);
        }
    }
}

/// Check 2: missing or blank labels.
fn check_missing_labels(vals: &[values::ReferenceValue], issues: &mut Vec<ReferenceValidationIssue>) {
    for v in vals {
        if v.label.trim().is_empty() {
            issues.push(ReferenceValidationIssue {
                check: "missing_label".into(),
                message: format!("La valeur '{}' (id={}) a un libelle vide ou manquant.", v.code, v.id),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
        }
    }
}

/// Check 3: hierarchy cycles.
///
/// Builds an in-memory adjacency map (child → parent) and walks up from each
/// node. If a node is revisited during the walk, a cycle exists.
fn check_hierarchy_cycles(vals: &[values::ReferenceValue], issues: &mut Vec<ReferenceValidationIssue>) {
    // Build id → parent_id map.
    let parent_map: HashMap<i64, Option<i64>> = vals.iter().map(|v| (v.id, v.parent_id)).collect();

    // Track which values are part of a reported cycle to avoid duplicates.
    let mut reported_cycle_members: HashSet<i64> = HashSet::new();

    for v in vals {
        if v.parent_id.is_none() || reported_cycle_members.contains(&v.id) {
            continue;
        }

        let mut visited: HashSet<i64> = HashSet::new();
        let mut current = Some(v.id);

        while let Some(cid) = current {
            if !visited.insert(cid) {
                // Cycle detected — report once for each member not yet reported.
                for &member in &visited {
                    if reported_cycle_members.insert(member) {
                        let code = vals
                            .iter()
                            .find(|x| x.id == member)
                            .map(|x| x.code.clone())
                            .unwrap_or_default();
                        issues.push(ReferenceValidationIssue {
                            check: "hierarchy_cycle".into(),
                            message: format!(
                                "La valeur '{}' (id={}) participe a un cycle dans la hierarchie.",
                                code, member
                            ),
                            severity: IssueSeverity::Blocking,
                            value_id: Some(member),
                            value_code: Some(code),
                        });
                    }
                }
                break;
            }
            current = parent_map.get(&cid).copied().flatten();
        }
    }
}

/// Check 4: orphan parent references.
///
/// A value has parent_id set, but the parent does not exist in this set.
fn check_orphan_parents(
    vals: &[values::ReferenceValue],
    cross_parents: &HashSet<i64>,
    issues: &mut Vec<ReferenceValidationIssue>,
) {
    let id_set: HashSet<i64> = vals.iter().map(|v| v.id).collect();

    for v in vals {
        if let Some(pid) = v.parent_id {
            if !id_set.contains(&pid) && !cross_parents.contains(&pid) {
                issues.push(ReferenceValidationIssue {
                    check: "orphan_parent".into(),
                    message: format!(
                        "La valeur '{}' (id={}) reference un parent id={} qui n'existe pas dans ce jeu.",
                        v.code, v.id, pid
                    ),
                    severity: IssueSeverity::Blocking,
                    value_id: Some(v.id),
                    value_code: Some(v.code.clone()),
                });
            }
        }
    }
}

/// Check 4b: every value of a domain with a required parent domain hangs under a value of that
/// domain (an OT sub-type under one EN 13306 maintenance nature).
async fn check_required_parents(
    db: &DatabaseConnection,
    domain: &crate::reference::domains::ReferenceDomain,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) -> AppResult<()> {
    let Some(required) = values::required_parent_domain(&domain.code) else {
        return Ok(());
    };
    for v in vals {
        let parent_domain = match v.parent_id {
            Some(pid) => values::domain_code_of_value(db, pid).await?,
            None => None,
        };
        if parent_domain.as_deref().map(|d| d.eq_ignore_ascii_case(required)) != Some(true) {
            issues.push(ReferenceValidationIssue {
                check: "parent_required".into(),
                message: format!(
                    "La valeur '{}' doit etre rattachee a une valeur de '{required}'.",
                    v.code
                ),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
        }
    }
    Ok(())
}

/// Check 4d: units of `MEASURE.UNITS`. A standard unit names one of the physical dimensions; a
/// plant unit names an active standard unit of the same dimension as its base and a positive
/// factor (blocking). A unit migration 201 created to review, still without base or factor, is a
/// warning: its links were carried over and nothing about it is assumed.
async fn check_unit_dimensions(
    db: &DatabaseConnection,
    domain: &crate::reference::domains::ReferenceDomain,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) -> AppResult<()> {
    use crate::reference::units;
    if !units::is_units_domain(&domain.code) {
        return Ok(());
    }
    for v in vals {
        let meta: serde_json::Value = v
            .metadata_json
            .as_deref()
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or(serde_json::Value::Null);
        let standard = meta.get("origin").and_then(serde_json::Value::as_str) == Some("system");
        let to_review = meta.get("review_required").and_then(serde_json::Value::as_bool) == Some(true);
        let result = if standard {
            match units::checked_dimension_tag(db, v.semantic_tag.as_deref()).await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => Err(AppError::ValidationFailed(vec![format!(
                    "{}: l'unité standard n'a pas de grandeur physique.",
                    units::GATE_UNIT_DIMENSION_INVALID
                )])),
                Err(e) => Err(e),
            }
        } else if to_review && v.parent_id.is_none() && meta.get("base_factor").is_none() {
            issues.push(ReferenceValidationIssue {
                check: "unit_to_review".into(),
                message: format!(
                    "L'unité '{}' est à revoir : indiquez son unité standard de base et le facteur. \
                     Aucune conversion ne l'utilisera avant.",
                    v.code
                ),
                severity: IssueSeverity::Warning,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
            Ok(())
        } else {
            units::checked_plant_unit(
                db,
                v.set_id,
                &v.code,
                v.parent_id,
                v.semantic_tag.as_deref(),
                v.metadata_json.as_deref(),
            )
            .await
            .map(|_| ())
        };
        match result {
            Ok(()) => {}
            Err(AppError::ValidationFailed(errs)) => issues.push(ReferenceValidationIssue {
                check: "invalid_unit".into(),
                message: format!("L'unité '{}' : {}", v.code, errs.join(" ")),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            }),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Check 4c: the nature tag of a nature-tagged domain (request types) is empty or one of the
/// EN 13306 maintenance natures.
async fn check_nature_tags(
    db: &DatabaseConnection,
    domain: &crate::reference::domains::ReferenceDomain,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) -> AppResult<()> {
    if !values::is_nature_tagged_domain(&domain.code) {
        return Ok(());
    }
    for v in vals {
        if let Err(AppError::ValidationFailed(errs)) =
            values::checked_semantic_tag(db, &domain.code, v.semantic_tag.as_deref()).await
        {
            issues.push(ReferenceValidationIssue {
                check: "invalid_nature_tag".into(),
                message: format!("La valeur '{}' : {}", v.code, errs.join(" ")),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
        }
    }
    Ok(())
}

/// Check 4e: permit rules (`PERMIT.REQUIREMENT_RULES`) name what they match (an equipment class
/// and / or a work type of the published lists) and an existing permit type. An active rule that
/// does not read blocks publishing (a rule cannot be guessed at conversion).
async fn check_permit_rules(
    db: &DatabaseConnection,
    domain: &crate::reference::domains::ReferenceDomain,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) -> AppResult<()> {
    if !crate::permit::requirement_rules::is_rules_domain(&domain.code) {
        return Ok(());
    }
    for v in vals.iter().filter(|v| v.is_active) {
        if let Some(problem) = crate::permit::requirement_rules::rule_problem(db, v.metadata_json.as_deref()).await? {
            issues.push(ReferenceValidationIssue {
                check: "invalid_permit_rule".into(),
                message: format!("Permit rule '{}': {problem}.", v.code),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
        }
    }
    Ok(())
}

/// Check 4f: every active OT type (`WORK.ORDER_TYPES`) carries a maintenance nature: a type
/// without one would count in no work mix (planned / reactive / emergency).
fn check_work_order_type_natures(
    domain_code: &str,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) {
    if !domain_code
        .trim()
        .eq_ignore_ascii_case(crate::wo::types::WORK_ORDER_TYPES_DOMAIN)
    {
        return;
    }
    for v in vals.iter().filter(|v| v.is_active) {
        let known = v
            .semantic_tag
            .as_deref()
            .and_then(crate::wo::types::MaintenanceNature::parse)
            .is_some();
        if !known {
            issues.push(ReferenceValidationIssue {
                check: "work_order_type_without_nature".into(),
                message: format!(
                    "OT type '{}' has no maintenance nature: it would count in no work mix.",
                    v.code
                ),
                severity: IssueSeverity::Blocking,
                value_id: Some(v.id),
                value_code: Some(v.code.clone()),
            });
        }
    }
}

/// Check 5: invalid color_hex format.
///
/// If color_hex is provided it must be a valid 3- or 6-digit hex, optionally
/// prefixed with `#`. Invalid formats produce a warning (not blocking) since
/// they affect UI rendering but not semantic integrity.
fn check_invalid_colors(vals: &[values::ReferenceValue], issues: &mut Vec<ReferenceValidationIssue>) {
    for v in vals {
        if let Some(ref color) = v.color_hex {
            if !color.is_empty() && !is_valid_color_hex(color) {
                issues.push(ReferenceValidationIssue {
                    check: "invalid_color_hex".into(),
                    message: format!(
                        "La valeur '{}' (id={}) a un format de couleur invalide : '{}'. \
                         Attendu : 3 ou 6 chiffres hexadecimaux, optionnellement precedes de '#'.",
                        v.code, v.id, color
                    ),
                    severity: IssueSeverity::Warning,
                    value_id: Some(v.id),
                    value_code: Some(v.code.clone()),
                });
            }
        }
    }
}

/// Check 6: invalid external_code format for domains that require a pattern.
///
/// If the domain's `validation_rules_json` contains an `external_code_pattern`
/// field, values with external_code set must match that regex pattern.
fn check_external_code_format(
    domain: &domains::ReferenceDomain,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) {
    // Extract pattern from domain validation rules if present.
    let pattern = match &domain.validation_rules_json {
        Some(json) if !json.is_empty() => match serde_json::from_str::<serde_json::Value>(json) {
            Ok(obj) => obj
                .get("external_code_pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            Err(_) => None,
        },
        _ => None,
    };

    let pattern = match pattern {
        Some(p) => p,
        None => return, // No pattern required — skip.
    };

    // Compile the regex. If the admin-defined pattern is invalid, report once as warning.
    let re = match regex::Regex::new(&pattern) {
        Ok(r) => r,
        Err(e) => {
            issues.push(ReferenceValidationIssue {
                check: "invalid_external_code_pattern".into(),
                message: format!(
                    "Le domaine '{}' definit un pattern external_code invalide : '{}' ({}). \
                     Verification des codes externes ignoree.",
                    domain.code, pattern, e
                ),
                severity: IssueSeverity::Warning,
                value_id: None,
                value_code: None,
            });
            return;
        }
    };

    for v in vals {
        if let Some(ref ext_code) = v.external_code {
            if !ext_code.is_empty() && !re.is_match(ext_code) {
                issues.push(ReferenceValidationIssue {
                    check: "invalid_external_code".into(),
                    message: format!(
                        "La valeur '{}' (id={}) a un code externe '{}' qui ne correspond pas \
                         au pattern requis '{}'.",
                        v.code, v.id, ext_code, pattern
                    ),
                    severity: IssueSeverity::Blocking,
                    value_id: Some(v.id),
                    value_code: Some(v.code.clone()),
                });
            }
        }
    }
}

/// Check 7: deactivated values in protected domains without a migration map.
///
/// In protected analytical domains, deactivated values should have a migration
/// map entry pointing to their replacement. Missing maps are flagged as warnings
/// (not blocking) since the migration tooling (Sprint S3) enables the fix.
async fn check_protected_deactivations_without_migration(
    db: &DatabaseConnection,
    domain: &domains::ReferenceDomain,
    vals: &[values::ReferenceValue],
    issues: &mut Vec<ReferenceValidationIssue>,
) {
    if !crate::reference::governance::requires_analytical_protection(domain) {
        return;
    }

    for v in vals {
        if !v.is_active {
            let has_map = protected::has_migration_map(db, v.id).await.unwrap_or(false);
            if !has_map {
                issues.push(ReferenceValidationIssue {
                    check: "protected_deactivation_no_migration".into(),
                    message: format!(
                        "La valeur '{}' (id={}) est desactivee dans un domaine protege \
                         mais n'a pas de mapping de migration. Utilisez l'outil de migration \
                         pour definir la valeur de remplacement.",
                        v.code, v.id
                    ),
                    severity: IssueSeverity::Warning,
                    value_id: Some(v.id),
                    value_code: Some(v.code.clone()),
                });
            }
        }
    }
}

#[cfg(test)]
mod work_order_type_nature_tests {
    use super::{check_work_order_type_natures, IssueSeverity};

    fn value(code: &str, tag: Option<&str>, active: bool) -> crate::reference::values::ReferenceValue {
        serde_json::from_value(serde_json::json!({
            "id": 1, "set_id": 1, "parent_id": null, "code": code, "label": code, "description": null,
            "sort_order": 1, "color_hex": null, "icon_name": null, "semantic_tag": tag,
            "external_code": null, "is_active": active, "metadata_json": null
        }))
        .expect("value")
    }

    /// An active OT type without a known nature blocks publishing; an inactive one does not.
    #[test]
    fn an_active_type_without_nature_blocks_publishing() {
        let vals = [
            value("corrective", Some("corrective_deferred"), true),
            value("PLANT_OWN", None, true),
            value("ODD", Some("not_a_nature"), true),
            value("OLD", None, false),
        ];
        let mut issues = Vec::new();
        check_work_order_type_natures("WORK.ORDER_TYPES", &vals, &mut issues);
        let codes: Vec<_> = issues.iter().filter_map(|i| i.value_code.clone()).collect();
        assert_eq!(codes, ["PLANT_OWN", "ODD"]);
        assert!(issues.iter().all(|i| i.severity == IssueSeverity::Blocking));

        let mut other = Vec::new();
        check_work_order_type_natures("DI.REQUEST_TYPE", &vals, &mut other);
        assert!(other.is_empty(), "other lists are not concerned");
    }
}
