//! Publish readiness engine and impact preview.
//!
//! Phase 2 - Sub-phase 03 - File 04 - Sprint S1.
//!
//! Provides governed publish with pre-flight readiness checks:
//!
//!   1. Set must exist and be in `validated` status.
//!   2. Latest validation report must exist and have zero blocking issues.
//!   3. No deactivated values without a migration map entry.
//!   4. For protected-analytical domains, impact preview must have been computed.
//!
//! Impact preview probes downstream module dimensions and returns explicit
//! status for each, including placeholder "unavailable" for modules not yet wired.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::domains;
use super::sets;
use super::validation;

// ─── Types ────────────────────────────────────────────────────────────────────

/// A single publish readiness issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencePublishIssue {
    pub check: String,
    pub message: String,
    pub severity: String, // "blocker" or "warning"
}

/// Full readiness assessment returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencePublishReadiness {
    pub set_id: i64,
    pub domain_id: i64,
    pub is_ready: bool,
    pub is_protected: bool,
    pub issues: Vec<ReferencePublishIssue>,
    pub impact_preview_required: bool,
    pub impact_preview_available: bool,
}

/// Records of one column that use a changed value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactTable {
    pub table: String,
    pub column: String,
    pub count: i64,
}

/// Impact summary for a single downstream module dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleImpact {
    pub module: String,
    pub status: String, // "available" | "unavailable" | "no_impact"
    pub affected_count: i64,
    pub details: Option<String>,
    /// Where the affected records are (empty when none).
    #[serde(default)]
    pub tables: Vec<ImpactTable>,
}

/// Full impact analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceImpactSummary {
    pub set_id: i64,
    pub domain_id: i64,
    pub domain_code: String,
    /// Codes of the published set that the candidate removes.
    #[serde(default)]
    pub removed_codes: Vec<String>,
    /// Codes that the candidate deactivates.
    #[serde(default)]
    pub deactivated_codes: Vec<String>,
    pub total_affected: i64,
    pub dimensions: Vec<ModuleImpact>,
    pub computed_at: String,
}

/// Result of a governed publish operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencePublishResult {
    pub set: sets::ReferenceSet,
    pub superseded_set_id: Option<i64>,
    pub readiness: ReferencePublishReadiness,
}

// ─── Impact preview cache key ─────────────────────────────────────────────────

/// In-memory flag tracking whether impact preview was computed for a set.
/// In a production system this would be persisted; for now the publish engine
/// accepts the preview result directly as proof of completion.
static IMPACT_CACHE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<i64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Mark impact preview as computed for a set.
fn mark_impact_computed(set_id: i64) {
    if let Ok(mut cache) = IMPACT_CACHE.lock() {
        cache.insert(set_id);
    }
}

/// Check if impact preview was computed for a set.
fn is_impact_computed(set_id: i64) -> bool {
    IMPACT_CACHE
        .lock()
        .map(|cache| cache.contains(&set_id))
        .unwrap_or(false)
}

/// Clear impact cache (useful for tests).
#[cfg(test)]
pub fn clear_impact_cache() {
    if let Ok(mut cache) = IMPACT_CACHE.lock() {
        cache.clear();
    }
}

// ─── Module dimension constants ───────────────────────────────────────────────

/// Dimensions not computed on the desktop (reported as `unavailable`).
const UNAVAILABLE_DIMENSIONS: &[&str] = &["external_integrations"];

// ─── Public API ───────────────────────────────────────────────────────────────

/// Compute publish readiness for a reference set.
///
/// Returns a structured assessment of all readiness gates. The `is_ready`
/// flag is `false` when any blocker issue exists.
pub async fn compute_publish_readiness(db: &DatabaseConnection, set_id: i64) -> AppResult<ReferencePublishReadiness> {
    let set = sets::get_reference_set(db, set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    let is_protected = crate::reference::governance::requires_publish_impact_preview(&domain);

    let mut issues: Vec<ReferencePublishIssue> = Vec::new();

    // ── Check 1: set status must be 'validated' ───────────────────────────
    if set.status != "validated" {
        issues.push(ReferencePublishIssue {
            check: "set_status".into(),
            message: format!(
                "Le jeu est en statut '{}'; seul le statut 'validated' permet la publication.",
                set.status
            ),
            severity: "blocker".into(),
        });
    }

    // ── Check 2: latest validation report with zero blockers ──────────────
    match validation::get_latest_validation_report(db, set_id).await {
        Ok(report) => {
            if report.blocking_count > 0 {
                issues.push(ReferencePublishIssue {
                    check: "validation_blockers".into(),
                    message: format!(
                        "Le dernier rapport de validation contient {} probleme(s) bloquant(s).",
                        report.blocking_count
                    ),
                    severity: "blocker".into(),
                });
            }
            if report.status == "failed" {
                issues.push(ReferencePublishIssue {
                    check: "validation_failed".into(),
                    message: "Le dernier rapport de validation est en echec.".into(),
                    severity: "blocker".into(),
                });
            }
        }
        Err(_) => {
            issues.push(ReferencePublishIssue {
                check: "validation_report_missing".into(),
                message: "Aucun rapport de validation n'existe pour ce jeu.".into(),
                severity: "blocker".into(),
            });
        }
    }

    // ── Check 3: no deactivated values without migration map ──────────────
    let unresolved = count_unresolved_deactivations(db, set_id).await?;
    if unresolved > 0 {
        issues.push(ReferencePublishIssue {
            check: "unresolved_migrations".into(),
            message: format!("{unresolved} valeur(s) desactivee(s) sans carte de migration."),
            severity: "blocker".into(),
        });
    }

    // ── Check 4: protected domain → impact preview required ───────────────
    let impact_preview_available = is_impact_computed(set_id);
    let impact_preview_required = is_protected;

    if is_protected && !impact_preview_available {
        issues.push(ReferencePublishIssue {
            check: "impact_preview_required".into(),
            message: "Le domaine est protege; un apercu d'impact est requis avant publication.".into(),
            severity: "blocker".into(),
        });
    }

    let is_ready = !issues.iter().any(|i| i.severity == "blocker");

    Ok(ReferencePublishReadiness {
        set_id,
        domain_id: set.domain_id,
        is_ready,
        is_protected,
        issues,
        impact_preview_required,
        impact_preview_available,
    })
}

/// Preview publish impact across downstream module dimensions.
///
/// Probes each dimension and returns an explicit status. For modules not
/// yet implemented, returns `"unavailable"` with zero affected count.
/// Calling this function marks the impact preview as computed for the set.
pub async fn preview_publish_impact(db: &DatabaseConnection, set_id: i64) -> AppResult<ReferenceImpactSummary> {
    let set = sets::get_reference_set(db, set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;

    let published_set_id = find_published_set_id(db, set.domain_id, set_id).await?;
    let changes = super::impact::value_changes(db, published_set_id, set_id).await?;
    let codes = changes.codes();
    let usage = super::impact::usage_of_changes(db, &domain.code, published_set_id, set_id, &codes).await?;
    let skill_plans = if domain.code.eq_ignore_ascii_case("PERSONNEL.SKILLS") {
        count_pm_plans_requiring_skills(db, &codes).await?
    } else {
        0
    };

    let mut dimensions: Vec<ModuleImpact> = Vec::new();
    let mut total_affected: i64 = 0;
    for &dim in super::impact::DIMENSIONS {
        let mut tables: Vec<ImpactTable> = usage
            .iter()
            .filter(|u| super::impact::module_of(&u.table) == dim)
            .map(|u| ImpactTable {
                table: u.table.clone(),
                column: u.column.clone(),
                count: u.count,
            })
            .collect();
        if dim == "pm_plans" && skill_plans > 0 {
            // Skills are stored as code lists in JSON, not as foreign keys.
            tables.push(ImpactTable {
                table: "pm_plan_versions".into(),
                column: "required_skills_json".into(),
                count: skill_plans,
            });
        }
        let affected_count: i64 = tables.iter().map(|t| t.count).sum();
        total_affected += affected_count;
        dimensions.push(ModuleImpact {
            module: dim.to_string(),
            status: if codes.is_empty() { "no_impact" } else { "available" }.into(),
            affected_count,
            details: None,
            tables,
        });
    }
    for &dim in UNAVAILABLE_DIMENSIONS {
        dimensions.push(ModuleImpact {
            module: dim.to_string(),
            status: "unavailable".into(),
            affected_count: 0,
            details: None,
            tables: Vec::new(),
        });
    }

    let now = Utc::now().to_rfc3339();

    // Mark preview as computed so readiness check passes.
    mark_impact_computed(set_id);

    Ok(ReferenceImpactSummary {
        set_id,
        domain_id: set.domain_id,
        domain_code: domain.code,
        removed_codes: changes.removed,
        deactivated_codes: changes.deactivated,
        total_affected,
        dimensions,
        computed_at: now,
    })
}

/// Governed publish: runs readiness checks, then publishes if all gates pass.
///
/// Supersedes any previously published set in the same domain.
/// Returns the published set, the superseded set id (if any), and readiness.
pub async fn publish_reference_set(
    db: &DatabaseConnection,
    set_id: i64,
    _actor_id: i64,
) -> AppResult<ReferencePublishResult> {
    let set_probe = sets::get_reference_set(db, set_id).await?;
    let domain_probe = domains::get_reference_domain(db, set_probe.domain_id).await?;
    crate::reference::governance::assert_allows_publish(&domain_probe)?;

    // Run full readiness assessment.
    let readiness = compute_publish_readiness(db, set_id).await?;

    if !readiness.is_ready {
        let blocker_msgs: Vec<String> = readiness
            .issues
            .iter()
            .filter(|i| i.severity == "blocker")
            .map(|i| i.message.clone())
            .collect();
        return Err(AppError::ValidationFailed(blocker_msgs));
    }

    let set = sets::get_reference_set(db, set_id).await?;

    // Find current published set (if any) for supersede tracking.
    let prev_published_id = find_published_set_id(db, set.domain_id, set_id).await?;

    let txn = db.begin().await?;

    // Supersede previous published set(s).
    if prev_published_id.is_some() {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_sets SET status = 'superseded' \
             WHERE domain_id = ? AND status = 'published' AND id != ?",
            [set.domain_id.into(), set_id.into()],
        ))
        .await?;
    }

    // Publish the set.
    let now = Utc::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_sets SET status = 'published', published_at = ?, \
         effective_from = ? WHERE id = ?",
        [now.clone().into(), now.into(), set_id.into()],
    ))
    .await?;

    txn.commit().await?;

    let published_set = sets::get_reference_set(db, set_id).await?;

    Ok(ReferencePublishResult {
        set: published_set,
        superseded_set_id: prev_published_id,
        readiness,
    })
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Count deactivated values in a set that have no migration map entry.
async fn count_unresolved_deactivations(db: &impl ConnectionTrait, set_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM reference_values rv \
             WHERE rv.set_id = ? AND rv.is_active = 0 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM reference_value_migrations \
                   WHERE from_value_id = rv.id \
               )",
            [set_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count_unresolved_deactivations: no result")))?;

    let cnt: i64 = row
        .try_get("", "cnt")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("decode cnt: {e}")))?;
    Ok(cnt)
}

/// Find the currently published set in a domain (excluding the candidate).
async fn find_published_set_id(
    db: &impl ConnectionTrait,
    domain_id: i64,
    exclude_set_id: i64,
) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets \
             WHERE domain_id = ? AND status = 'published' AND id != ? \
             ORDER BY version_no DESC LIMIT 1",
            [domain_id.into(), exclude_set_id.into()],
        ))
        .await?;

    match row {
        Some(r) => {
            let id: i64 = r
                .try_get("", "id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("decode set id: {e}")))?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// PM plans whose current requirements list one of the skill codes (stored as JSON code lists).
async fn count_pm_plans_requiring_skills(
    db: &impl ConnectionTrait,
    codes: &std::collections::BTreeSet<String>,
) -> AppResult<i64> {
    if codes.is_empty() {
        return Ok(0);
    }
    let conditions = vec!["pv.required_skills_json LIKE ?"; codes.len()].join(" OR ");
    let values: Vec<sea_orm::Value> = codes.iter().map(|c| format!("%\"{c}\"%").into()).collect();
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(DISTINCT pv.pm_plan_id) AS cnt FROM pm_plan_versions pv                  WHERE pv.required_skills_json IS NOT NULL AND ({conditions})"
            ),
            values,
        ))
        .await?;
    row.map(|r| r.try_get::<i64>("", "cnt"))
        .transpose()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("decode cnt: {e}")))
        .map(|n| n.unwrap_or(0))
}
