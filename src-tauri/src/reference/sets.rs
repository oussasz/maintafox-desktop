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
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
async fn next_version_no(db: &impl ConnectionTrait, domain_id: i64) -> AppResult<i64> {
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

/// Latest published set for a domain, if any.
async fn find_published_set_id(
    db: &impl ConnectionTrait,
    domain_id: i64,
) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets \
             WHERE domain_id = ? AND status = 'published' \
             ORDER BY version_no DESC LIMIT 1",
            [domain_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => Some(
            r.try_get::<i64>("", "id")
                .map_err(|e| decode_err("id", e))?,
        ),
        None => None,
    })
}

const VALUE_CLONE_COLS: &str =
    "id, parent_id, code, label, description, sort_order, \
     color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json";

/// Clone all values (+ aliases) from a published set into a new draft set.
/// Preserves codes/labels/ordering/active/metadata; remaps parent_id; new IDs.
async fn clone_published_values_into_draft(
    db: &impl ConnectionTrait,
    source_set_id: i64,
    draft_set_id: i64,
) -> AppResult<()> {
    let source_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {VALUE_CLONE_COLS} FROM reference_values WHERE set_id = ? \
                 ORDER BY sort_order ASC, code ASC"
            ),
            [source_set_id.into()],
        ))
        .await?;

    // Pass 1: insert scalars with parent_id NULL; build old→new id map via code.
    let mut id_map: HashMap<i64, i64> = HashMap::with_capacity(source_rows.len());
    let mut parent_jobs: Vec<(i64, i64)> = Vec::new(); // (new_id, old_parent_id)

    for row in &source_rows {
        let old_id: i64 = row
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        let old_parent: Option<i64> = row
            .try_get("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?;
        let code: String = row
            .try_get("", "code")
            .map_err(|e| decode_err("code", e))?;
        let label: String = row
            .try_get("", "label")
            .map_err(|e| decode_err("label", e))?;
        let description: Option<String> = row
            .try_get("", "description")
            .map_err(|e| decode_err("description", e))?;
        let sort_order: Option<i64> = row
            .try_get("", "sort_order")
            .map_err(|e| decode_err("sort_order", e))?;
        let color_hex: Option<String> = row
            .try_get("", "color_hex")
            .map_err(|e| decode_err("color_hex", e))?;
        let icon_name: Option<String> = row
            .try_get("", "icon_name")
            .map_err(|e| decode_err("icon_name", e))?;
        let semantic_tag: Option<String> = row
            .try_get("", "semantic_tag")
            .map_err(|e| decode_err("semantic_tag", e))?;
        let external_code: Option<String> = row
            .try_get("", "external_code")
            .map_err(|e| decode_err("external_code", e))?;
        let is_active: i64 = row
            .try_get("", "is_active")
            .map_err(|e| decode_err("is_active", e))?;
        let metadata_json: Option<String> = row
            .try_get("", "metadata_json")
            .map_err(|e| decode_err("metadata_json", e))?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values \
                 (set_id, parent_id, code, label, description, sort_order, \
                  color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                draft_set_id.into(),
                code.clone().into(),
                label.into(),
                description.into(),
                sort_order.into(),
                color_hex.into(),
                icon_name.into(),
                semantic_tag.into(),
                external_code.into(),
                is_active.into(),
                metadata_json.into(),
            ],
        ))
        .await?;

        let inserted = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM reference_values WHERE set_id = ? AND code = ?",
                [draft_set_id.into(), code.into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "cloned reference_values row missing after insert"
                ))
            })?;
        let new_id: i64 = inserted
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        id_map.insert(old_id, new_id);
        if let Some(pid) = old_parent {
            parent_jobs.push((new_id, pid));
        }
    }

    // Pass 2: remap parents; fail if parent was not in the cloned set (orphan).
    for (new_id, old_parent_id) in parent_jobs {
        let Some(&new_parent_id) = id_map.get(&old_parent_id) else {
            return Err(AppError::ValidationFailed(vec![format!(
                "Impossible de cloner le jeu : parent_id={old_parent_id} \
                 introuvable dans le jeu publié (hiérarchie orpheline)."
            )]));
        };
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET parent_id = ? WHERE id = ?",
            [new_parent_id.into(), new_id.into()],
        ))
        .await?;
    }

    // Pass 3: clone aliases onto new value IDs.
    let now = Utc::now().to_rfc3339();
    if id_map.is_empty() {
        return Ok(());
    }

    let old_ids: Vec<i64> = id_map.keys().copied().collect();
    let placeholders = old_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let mut params: Vec<sea_orm::Value> = old_ids.iter().map(|id| (*id).into()).collect();

    let alias_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT reference_value_id, alias_label, locale, alias_type, is_preferred \
                 FROM reference_aliases WHERE reference_value_id IN ({placeholders})"
            ),
            params.drain(..),
        ))
        .await?;

    for row in alias_rows {
        let old_value_id: i64 = row
            .try_get("", "reference_value_id")
            .map_err(|e| decode_err("reference_value_id", e))?;
        let Some(&new_value_id) = id_map.get(&old_value_id) else {
            continue;
        };
        let alias_label: String = row
            .try_get("", "alias_label")
            .map_err(|e| decode_err("alias_label", e))?;
        let locale: String = row
            .try_get("", "locale")
            .map_err(|e| decode_err("locale", e))?;
        let alias_type: String = row
            .try_get("", "alias_type")
            .map_err(|e| decode_err("alias_type", e))?;
        let is_preferred: i64 = row
            .try_get("", "is_preferred")
            .map_err(|e| decode_err("is_preferred", e))?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_aliases \
                 (reference_value_id, alias_label, locale, alias_type, is_preferred, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
            [
                new_value_id.into(),
                alias_label.into(),
                locale.into(),
                alias_type.into(),
                is_preferred.into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }

    Ok(())
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
///
/// When a published set exists, all values (and aliases) are cloned into the
/// new draft in one transaction. When none exists (bootstrap), the draft is empty.
pub async fn create_draft_set(
    db: &DatabaseConnection,
    domain_id: i64,
    actor_id: i64,
) -> AppResult<ReferenceSet> {
    let domain = super::domains::get_reference_domain(db, domain_id).await?;
    super::governance::assert_allows_create_draft_set(&domain)?;
    assert_no_active_draft(db, domain_id).await?;

    let txn = db.begin().await?;

    let version_no = next_version_no(&txn, domain_id).await?;
    let now = Utc::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
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

    let row = txn
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

    let draft = map_set(&row)?;

    if let Some(published_id) = find_published_set_id(&txn, domain_id).await? {
        clone_published_values_into_draft(&txn, published_id, draft.id).await?;
    }

    txn.commit().await?;
    Ok(draft)
}

/// Hard-deletes a draft set and its values/aliases/validation reports.
/// Never affects published or superseded sets.
pub async fn discard_draft_set(
    db: &DatabaseConnection,
    set_id: i64,
) -> AppResult<()> {
    let set = get_set_by_id(db, set_id).await?;
    let domain = super::domains::get_reference_domain(db, set.domain_id).await?;
    super::governance::assert_allows_create_draft_set(&domain)?;

    if set.status != SET_STATUS_DRAFT {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de supprimer un jeu en statut '{}'. \
             Seul un brouillon ('draft') peut être abandonné.",
            set.status
        )]));
    }

    let txn = db.begin().await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reference_aliases \
         WHERE reference_value_id IN \
             (SELECT id FROM reference_values WHERE set_id = ?)",
        [set_id.into()],
    ))
    .await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reference_validation_reports WHERE set_id = ?",
        [set_id.into()],
    ))
    .await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reference_values WHERE set_id = ?",
        [set_id.into()],
    ))
    .await?;

    let deleted = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM reference_sets WHERE id = ? AND status = 'draft'",
            [set_id.into()],
        ))
        .await?;

    if deleted.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Le brouillon n'a pas pu être supprimé (statut modifié pendant l'opération)."
                .into(),
        ]));
    }

    txn.commit().await?;
    Ok(())
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
    let domain = super::domains::get_reference_domain(db, set.domain_id).await?;
    super::governance::assert_allows_publish(&domain)?;

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
