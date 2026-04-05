use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

// ── Report types ──────────────────────────────────────────────────────────

/// The result of a startup integrity check.
#[derive(Debug, Clone, Serialize)]
pub struct IntegrityReport {
    /// True if everything is healthy and the app can start normally.
    pub is_healthy: bool,
    /// True if the found issues are recoverable without data loss.
    pub is_recoverable: bool,
    /// List of issues found. Empty if `is_healthy` is true.
    pub issues: Vec<IntegrityIssue>,
    /// Seed schema version found in `system_config` (None if not yet seeded).
    pub seed_schema_version: Option<i32>,
    /// Total number of lookup domains found.
    pub domain_count: i32,
    /// Total number of lookup values found.
    pub value_count: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrityIssue {
    /// Machine-readable issue code.
    pub code: String,
    /// Human-readable description (French).
    pub description: String,
    /// Whether this issue can be auto-repaired by re-running the seeder.
    pub is_auto_repairable: bool,
    /// The `domain_key` or table name that has the issue.
    pub subject: String,
}

// ── Required seed domain keys — must all be present for the app to start ─

const REQUIRED_DOMAINS: &[&str] = &[
    "equipment.criticality",
    "equipment.lifecycle_status",
    "intervention_request.urgency",
    "intervention_request.status",
    "work_order.status",
    "work_order.priority",
    "personnel.skill_proficiency",
];

/// Minimum number of active values that must be present in each required domain.
const REQUIRED_DOMAIN_MIN_VALUES: &[(&str, i32)] = &[
    ("equipment.criticality", 2),
    ("equipment.lifecycle_status", 3),
    ("intervention_request.urgency", 2),
    ("intervention_request.status", 4),
    ("work_order.status", 4),
    ("work_order.priority", 2),
    ("personnel.skill_proficiency", 3),
];

// ── Integrity check ───────────────────────────────────────────────────────

/// Runs the full startup integrity check and returns a report.
/// Does not modify any data — purely diagnostic.
pub async fn run_integrity_check(db: &DatabaseConnection) -> AppResult<IntegrityReport> {
    let mut issues: Vec<IntegrityIssue> = Vec::new();

    // ── Check 1: required tables exist ───────────────────────────────────
    // Table names are compile-time constants — safe to interpolate.
    for table in &["lookup_domains", "lookup_values", "system_config"] {
        let sql = format!("SELECT COUNT(*) as cnt FROM {table}");
        if let Err(e) = db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await {
            issues.push(IntegrityIssue {
                code: "MISSING_TABLE".into(),
                description: format!("Table '{table}' introuvable : {e}"),
                is_auto_repairable: false,
                subject: (*table).to_string(),
            });
        }
    }

    // If tables are missing, further checks would fail — return early.
    if !issues.is_empty() {
        return Ok(IntegrityReport {
            is_healthy: false,
            is_recoverable: false,
            issues,
            seed_schema_version: None,
            domain_count: 0,
            value_count: 0,
        });
    }

    // ── Check 2: seed schema version ─────────────────────────────────────
    let seed_version: Option<i32> = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = 'seed_schema_version'".to_string(),
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String>("", "value").ok())
        .and_then(|s| s.parse::<i32>().ok());

    if seed_version.is_none() {
        issues.push(IntegrityIssue {
            code: "SEED_NOT_APPLIED".into(),
            description: "Les donn\u{00e9}es syst\u{00e8}me de base n'ont pas \u{00e9}t\u{00e9} initialis\u{00e9}es."
                .into(),
            is_auto_repairable: true,
            subject: "system_config::seed_schema_version".into(),
        });
    }

    // ── Check 3: aggregate counts ────────────────────────────────────────
    let domain_count = query_count(
        db,
        "SELECT COUNT(*) as cnt FROM lookup_domains WHERE deleted_at IS NULL",
    )
    .await;

    let value_count = query_count(db, "SELECT COUNT(*) as cnt FROM lookup_values WHERE deleted_at IS NULL").await;

    // ── Check 4: required domains present ────────────────────────────────
    for &domain_key in REQUIRED_DOMAINS {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL",
                [domain_key.into()],
            ))
            .await
            .ok()
            .flatten();

        if row.is_none() {
            issues.push(IntegrityIssue {
                code: "MISSING_DOMAIN".into(),
                description: format!(
                    "Domaine syst\u{00e8}me requis absent : '{domain_key}'. \
                     La r\u{00e9}paration automatique permettra de le restaurer."
                ),
                is_auto_repairable: true,
                subject: domain_key.to_string(),
            });
        }
    }

    // ── Check 5: minimum value counts per required domain ────────────────
    for &(domain_key, min_count) in REQUIRED_DOMAIN_MIN_VALUES {
        let actual = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"SELECT COUNT(*) as cnt FROM lookup_values lv
                   INNER JOIN lookup_domains ld ON ld.id = lv.domain_id
                   WHERE ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
                [domain_key.into()],
            ))
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get::<i32>("", "cnt").ok())
            .unwrap_or(0);

        if actual < min_count {
            issues.push(IntegrityIssue {
                code: "INSUFFICIENT_VALUES".into(),
                description: format!(
                    "Domaine '{domain_key}' : {actual} valeur(s) active(s) trouv\u{00e9}e(s), \
                     minimum requis : {min_count}."
                ),
                is_auto_repairable: true,
                subject: domain_key.to_string(),
            });
        }
    }

    let is_recoverable = issues.iter().all(|i| i.is_auto_repairable);
    let is_healthy = issues.is_empty();

    Ok(IntegrityReport {
        is_healthy,
        is_recoverable,
        issues,
        seed_schema_version: seed_version,
        domain_count,
        value_count,
    })
}

// ── Private helper ────────────────────────────────────────────────────────

/// Execute a `SELECT COUNT(*) as cnt` query and return the result, or 0 on any error.
async fn query_count(db: &DatabaseConnection, sql: &str) -> i32 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<i32>("", "cnt").ok())
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_domains_are_non_empty() {
        assert!(
            !REQUIRED_DOMAINS.is_empty(),
            "At least one required domain must be defined"
        );
    }

    #[test]
    fn min_values_cover_all_required_domains() {
        for &domain in REQUIRED_DOMAINS {
            let has_min = REQUIRED_DOMAIN_MIN_VALUES.iter().any(|&(key, _)| key == domain);
            assert!(
                has_min,
                "Required domain '{domain}' must have a minimum value count entry"
            );
        }
    }

    #[test]
    fn min_values_are_positive() {
        for &(key, min) in REQUIRED_DOMAIN_MIN_VALUES {
            assert!(min > 0, "Minimum for '{key}' must be positive, got {min}");
        }
    }
}
