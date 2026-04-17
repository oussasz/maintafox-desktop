//! Reference set version lifecycle service.
//!
//! Phase 2 - Sub-phase 03 - File 01 - Sprint S2.
//!
//! Provides governed lifecycle management for `reference_sets` (migration 013).
//! A reference set is a versioned snapshot of values within a domain. The
//! lifecycle enforces a strict progression:
//!
//!   draft → validated → published → superseded
//!
//! Invariants:
//!   - Only draft sets can move to validated.
//!   - Only validated sets can move to published.
//!   - At most one published set per domain at any time.
//!   - Publishing a new set automatically supersedes the previous published set.
//!   - Published and superseded sets are immutable (no direct edits).

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Allowed lifecycle statuses for reference sets (PRD 6.13).
pub const SET_STATUS_DRAFT: &str = "draft";
pub const SET_STATUS_VALIDATED: &str = "validated";
pub const SET_STATUS_PUBLISHED: &str = "published";
pub const SET_STATUS_SUPERSEDED: &str = "superseded";

pub const SET_STATUSES: &[&str] = &[
    SET_STATUS_DRAFT,
    SET_STATUS_VALIDATED,
    SET_STATUS_PUBLISHED,
    SET_STATUS_SUPERSEDED,
];

// ─── Types ────────────────────────────────────────────────────────────────────

/// Complete reference set record for reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSet {
    pub id: i64,
    pub domain_id: i64,
    pub version_no: i64,
    pub status: String,
    pub effective_from: Option<String>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
    pub published_at: Option<String>,
}

/// Payload for creating a draft set. Domain id and version are determined
/// automatically; actor_id is passed separately.
#[derive(Debug, Deserialize)]
pub struct CreateReferenceSetPayload {
    pub effective_from: Option<String>,
}

/// Payload for the validate transition. Currently carries no extra data
/// but exists for future extensibility (e.g. validation notes).
#[derive(Debug, Deserialize)]
pub struct ValidateReferenceSetPayload {
    // Reserved for future validation metadata (notes, checklist flags).
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_sets row decode failed for column '{column}': {e}"
    ))
}

fn map_set(row: &QueryResult) -> AppResult<ReferenceSet> {
    Ok(ReferenceSet {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        domain_id: row
            .try_get::<i64>("", "domain_id")
            .map_err(|e| decode_err("domain_id", e))?,
        version_no: row
            .try_get::<i64>("", "version_no")
            .map_err(|e| decode_err("version_no", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        effective_from: row
            .try_get::<Option<String>>("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        published_at: row
            .try_get::<Option<String>>("", "published_at")
            .map_err(|e| decode_err("published_at", e))?,
    })
}

const SELECT_COLS: &str =
    "id, domain_id, version_no, status, effective_from, created_by_id, created_at, published_at";

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Fetches a set by id. Returns `NotFound` if absent.
async fn get_set_by_id(db: &DatabaseConnection, set_id: i64) -> AppResult<ReferenceSet> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {SELECT_COLS} FROM reference_sets WHERE id = ?"),
            [set_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceSet".into(),
            id: set_id.to_string(),
        })?;
    map_set(&row)
}

/// Returns the next version number for a domain (max existing + 1, or 1).
async fn next_version_no(db: &DatabaseConnection, domain_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(version_no), 0) AS max_v FROM reference_sets WHERE domain_id = ?",
            [domain_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("MAX query returned no row"))
        })?;

    let max_v: i64 = row
        .try_get("", "max_v")
        .map_err(|e| decode_err("max_v", e))?;
    Ok(max_v + 1)
}

/// Verifies the domain exists. Returns `NotFound` otherwise.
async fn assert_domain_exists(db: &DatabaseConnection, domain_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_domains WHERE id = ?",
            [domain_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::NotFound {
            entity: "ReferenceDomain".into(),
            id: domain_id.to_string(),
        });
    }
    Ok(())
}

/// Ensures no other draft currently exists for this domain.
/// Only one draft-in-progress is allowed to prevent confusion.
async fn assert_no_active_draft(db: &DatabaseConnection, domain_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets WHERE domain_id = ? AND status = 'draft'",
            [domain_id.into()],
        ))
        .await?;
    if row.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Un brouillon existe déjà pour ce domaine. \
             Finalisez ou supprimez le brouillon existant avant d'en créer un nouveau."
                .into(),
        ]));
    }
    Ok(())
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Returns all sets for a domain, ordered by version descending.
pub async fn list_sets_for_domain(
    db: &DatabaseConnection,
    domain_id: i64,
) -> AppResult<Vec<ReferenceSet>> {
    assert_domain_exists(db, domain_id).await?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_sets \
                 WHERE domain_id = ? ORDER BY version_no DESC"
            ),
            [domain_id.into()],
        ))
        .await?;

    rows.iter().map(map_set).collect()
}

/// Returns a single set by id.
pub async fn get_reference_set(
    db: &DatabaseConnection,
    set_id: i64,
) -> AppResult<ReferenceSet> {
    get_set_by_id(db, set_id).await
}

/// Creates a new draft set for the given domain.
///
/// Assigns the next sequential version number. Only one draft per domain
/// is allowed at a time.
pub async fn create_draft_set(
    db: &DatabaseConnection,
    domain_id: i64,
    actor_id: i64,
) -> AppResult<ReferenceSet> {
    assert_domain_exists(db, domain_id).await?;
    assert_no_active_draft(db, domain_id).await?;

    let version_no = next_version_no(db, domain_id).await?;
    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_sets \
             (domain_id, version_no, status, created_by_id, created_at) \
         VALUES (?, ?, 'draft', ?, ?)",
        [
            domain_id.into(),
            version_no.into(),
            actor_id.into(),
            now.into(),
        ],
    ))
    .await?;

    // Fetch the created row via domain+version (unique index guarantees single row).
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_sets \
                 WHERE domain_id = ? AND version_no = ?"
            ),
            [domain_id.into(), version_no.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "reference_sets row missing after insert"
            ))
        })?;

    map_set(&row)
}

/// Transitions a draft set to validated.
///
/// Only sets with status `draft` can be validated. Runs the validation engine
/// first; the transition is blocked when any blocking issues exist
/// (`blocking_count > 0`). The validation report is always persisted regardless
/// of the outcome.
pub async fn validate_set(
    db: &DatabaseConnection,
    set_id: i64,
    actor_id: i64,
) -> AppResult<ReferenceSet> {
    let set = get_set_by_id(db, set_id).await?;

    if set.status != SET_STATUS_DRAFT {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de valider un jeu en statut '{}'. \
             Seul un brouillon ('draft') peut être validé.",
            set.status
        )]));
    }

    // Run the validation engine — persists a report regardless of outcome.
    let result = super::validation::validate_reference_set(db, set_id, actor_id).await?;

    if result.blocking_count > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le jeu contient {} problème(s) bloquant(s). \
             Corrigez-les avant de valider. \
             Rapport de validation : id={}.",
            result.blocking_count, result.report_id
        )]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_sets SET status = 'validated' WHERE id = ?",
        [set_id.into()],
    ))
    .await?;

    get_set_by_id(db, set_id).await
}

/// Publishes a validated set, making it the active reference for the domain.
///
/// Lifecycle rules enforced:
/// - Only `validated` sets can be published.
/// - Publishing automatically supersedes any previously published set in the
///   same domain (at most one published set per domain).
/// - Sets `published_at` timestamp.
pub async fn publish_set(
    db: &DatabaseConnection,
    set_id: i64,
    _actor_id: i64,
) -> AppResult<ReferenceSet> {
    let set = get_set_by_id(db, set_id).await?;

    if set.status != SET_STATUS_VALIDATED {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de publier un jeu en statut '{}'. \
             Seul un jeu validé ('validated') peut être publié.",
            set.status
        )]));
    }

    // Supersede the current published set (if any) before publishing the new one.
    supersede_previous_published(db, set.domain_id, set_id).await?;

    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_sets SET status = 'published', published_at = ? WHERE id = ?",
        [now.into(), set_id.into()],
    ))
    .await?;

    get_set_by_id(db, set_id).await
}

/// Supersedes any currently published set for the given domain.
///
/// This is called automatically during `publish_set` to enforce the
/// one-published-set-per-domain invariant. The `exclude_set_id` parameter
/// is the set about to be published (should not be superseded).
pub async fn supersede_previous_published(
    db: &DatabaseConnection,
    domain_id: i64,
    exclude_set_id: i64,
) -> AppResult<u64> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_sets SET status = 'superseded' \
             WHERE domain_id = ? AND status = 'published' AND id != ?",
            [domain_id.into(), exclude_set_id.into()],
        ))
        .await?;

    Ok(result.rows_affected())
}

/// Guard: ensures the set is in draft status. Used by value mutation
/// operations (Sprint S3) to block edits on published/superseded sets.
pub fn assert_set_is_draft(set: &ReferenceSet) -> AppResult<()> {
    if set.status != SET_STATUS_DRAFT {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier un jeu en statut '{}'. \
             Seuls les brouillons ('draft') peuvent être modifiés.",
            set.status
        )]));
    }
    Ok(())
}

/// Guard: ensures the set is editable (draft or validated).
/// Validated sets can be reverted to draft for corrections before publish.
pub fn assert_set_is_editable(set: &ReferenceSet) -> AppResult<()> {
    match set.status.as_str() {
        SET_STATUS_DRAFT | SET_STATUS_VALIDATED => Ok(()),
        _ => Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier un jeu en statut '{}'. \
             Seuls les brouillons et les jeux validés peuvent être modifiés.",
            set.status
        )])),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_is_editable() {
        let set = ReferenceSet {
            id: 1,
            domain_id: 1,
            version_no: 1,
            status: SET_STATUS_DRAFT.into(),
            effective_from: None,
            created_by_id: Some(1),
            created_at: "2026-04-06T00:00:00Z".into(),
            published_at: None,
        };
        assert!(assert_set_is_draft(&set).is_ok());
        assert!(assert_set_is_editable(&set).is_ok());
    }

    #[test]
    fn validated_is_editable_but_not_draft() {
        let set = ReferenceSet {
            id: 1,
            domain_id: 1,
            version_no: 1,
            status: SET_STATUS_VALIDATED.into(),
            effective_from: None,
            created_by_id: Some(1),
            created_at: "2026-04-06T00:00:00Z".into(),
            published_at: None,
        };
        assert!(assert_set_is_draft(&set).is_err());
        assert!(assert_set_is_editable(&set).is_ok());
    }

    #[test]
    fn published_is_not_editable() {
        let set = ReferenceSet {
            id: 1,
            domain_id: 1,
            version_no: 1,
            status: SET_STATUS_PUBLISHED.into(),
            effective_from: None,
            created_by_id: Some(1),
            created_at: "2026-04-06T00:00:00Z".into(),
            published_at: Some("2026-04-06T01:00:00Z".into()),
        };
        assert!(assert_set_is_draft(&set).is_err());
        assert!(assert_set_is_editable(&set).is_err());
    }

    #[test]
    fn superseded_is_not_editable() {
        let set = ReferenceSet {
            id: 1,
            domain_id: 1,
            version_no: 1,
            status: SET_STATUS_SUPERSEDED.into(),
            effective_from: None,
            created_by_id: Some(1),
            created_at: "2026-04-06T00:00:00Z".into(),
            published_at: Some("2026-04-06T01:00:00Z".into()),
        };
        assert!(assert_set_is_draft(&set).is_err());
        assert!(assert_set_is_editable(&set).is_err());
    }
}
