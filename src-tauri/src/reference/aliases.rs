//! Reference alias governance service.
//!
//! Phase 2 - Sub-phase 03 - File 03 - Sprint S1.
//!
//! Provides governed CRUD for `reference_aliases` (migration 015).
//! Each alias record associates a typed, locale-aware label with a reference value.
//!
//! Invariants:
//!   - alias_label must be non-empty (max 255 chars)
//!   - alias_type must be one of: legacy, import, search
//!   - locale must be non-empty (max 10 chars)
//!   - at most one preferred alias per `(reference_value_id, locale, alias_type)`
//!   - unique `(reference_value_id, locale, alias_type, alias_label)` enforced at DB level
//!   - deleting a preferred alias auto-promotes the oldest remaining alias in the same scope

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Allowed alias type values per PRD 6.13.
pub const ALIAS_TYPES: &[&str] = &["legacy", "import", "search"];

// ─── Types ────────────────────────────────────────────────────────────────────

/// Complete reference alias record for reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceAlias {
    pub id: i64,
    pub reference_value_id: i64,
    pub alias_label: String,
    pub locale: String,
    pub alias_type: String,
    pub is_preferred: bool,
    pub created_at: String,
}

/// Payload for creating a reference alias.
#[derive(Debug, Deserialize)]
pub struct CreateReferenceAliasPayload {
    pub reference_value_id: i64,
    pub alias_label: String,
    pub locale: String,
    pub alias_type: String,
    pub is_preferred: Option<bool>,
}

/// Payload for updating a reference alias. Only provided fields are changed.
#[derive(Debug, Deserialize)]
pub struct UpdateReferenceAliasPayload {
    pub alias_label: Option<String>,
    pub locale: Option<String>,
    pub alias_type: Option<String>,
    pub is_preferred: Option<bool>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_aliases row decode failed for column '{column}': {e}"
    ))
}

const SELECT_COLS: &str =
    "id, reference_value_id, alias_label, locale, alias_type, is_preferred, created_at";

fn map_alias(row: &QueryResult) -> AppResult<ReferenceAlias> {
    Ok(ReferenceAlias {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        reference_value_id: row
            .try_get::<i64>("", "reference_value_id")
            .map_err(|e| decode_err("reference_value_id", e))?,
        alias_label: row
            .try_get::<String>("", "alias_label")
            .map_err(|e| decode_err("alias_label", e))?,
        locale: row
            .try_get::<String>("", "locale")
            .map_err(|e| decode_err("locale", e))?,
        alias_type: row
            .try_get::<String>("", "alias_type")
            .map_err(|e| decode_err("alias_type", e))?,
        is_preferred: i64_to_bool(
            row.try_get::<i64>("", "is_preferred")
                .map_err(|e| decode_err("is_preferred", e))?,
        ),
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

// ─── Validation ───────────────────────────────────────────────────────────────

fn validate_alias_label(label: &str) -> AppResult<()> {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err(AppError::ValidationFailed(vec![
            "Le libellé d'alias doit comporter entre 1 et 255 caractères.".into(),
        ]));
    }
    Ok(())
}

fn validate_locale(locale: &str) -> AppResult<()> {
    let trimmed = locale.trim();
    if trimmed.is_empty() || trimmed.len() > 10 {
        return Err(AppError::ValidationFailed(vec![
            "Le code de locale doit comporter entre 1 et 10 caractères.".into(),
        ]));
    }
    Ok(())
}

fn validate_alias_type(alias_type: &str) -> AppResult<()> {
    if !ALIAS_TYPES.contains(&alias_type) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type d'alias '{alias_type}' invalide. Valeurs autorisées : {}.",
            ALIAS_TYPES.join(", ")
        )]));
    }
    Ok(())
}

/// Verify the reference value exists.
async fn assert_value_exists(db: &DatabaseConnection, value_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_values WHERE id = ?",
            [value_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::NotFound {
            entity: "ReferenceValue".into(),
            id: value_id.to_string(),
        });
    }
    Ok(())
}

/// Enforce the preferred-alias invariant: at most one preferred alias
/// per `(reference_value_id, locale, alias_type)`. If the new alias is
/// preferred, demote any existing preferred alias in the same scope.
async fn enforce_preferred_uniqueness(
    db: &DatabaseConnection,
    value_id: i64,
    locale: &str,
    alias_type: &str,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    let sql = match exclude_id {
        Some(eid) => format!(
            "UPDATE reference_aliases SET is_preferred = 0 \
             WHERE reference_value_id = ? AND locale = ? AND alias_type = ? \
             AND is_preferred = 1 AND id != {eid}"
        ),
        None => "UPDATE reference_aliases SET is_preferred = 0 \
             WHERE reference_value_id = ? AND locale = ? AND alias_type = ? \
             AND is_preferred = 1"
            .to_string(),
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &sql,
        [value_id.into(), locale.into(), alias_type.into()],
    ))
    .await?;
    Ok(())
}

/// After deleting a preferred alias, auto-promote the oldest remaining alias
/// in the same `(reference_value_id, locale, alias_type)` scope.
async fn auto_promote_preferred(
    db: &DatabaseConnection,
    value_id: i64,
    locale: &str,
    alias_type: &str,
) -> AppResult<()> {
    // Find the oldest alias in this scope (by id, deterministic).
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_aliases \
             WHERE reference_value_id = ? AND locale = ? AND alias_type = ? \
             ORDER BY id ASC LIMIT 1",
            [value_id.into(), locale.into(), alias_type.into()],
        ))
        .await?;

    if let Some(r) = row {
        let next_id: i64 = r
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_aliases SET is_preferred = 1 WHERE id = ?",
            [next_id.into()],
        ))
        .await?;
    }
    Ok(())
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Returns all aliases for a reference value, ordered by locale, alias_type, then alias_label.
pub async fn list_aliases(
    db: &DatabaseConnection,
    reference_value_id: i64,
) -> AppResult<Vec<ReferenceAlias>> {
    assert_value_exists(db, reference_value_id).await?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_aliases \
                 WHERE reference_value_id = ? \
                 ORDER BY locale ASC, alias_type ASC, alias_label ASC"
            ),
            [reference_value_id.into()],
        ))
        .await?;

    rows.iter().map(map_alias).collect()
}

/// Returns a single alias by id.
pub async fn get_alias(
    db: &DatabaseConnection,
    alias_id: i64,
) -> AppResult<ReferenceAlias> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {SELECT_COLS} FROM reference_aliases WHERE id = ?"),
            [alias_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceAlias".into(),
            id: alias_id.to_string(),
        })?;
    map_alias(&row)
}

/// Creates a reference alias. Returns the created alias.
///
/// Validates label, locale, alias_type, and enforces preferred uniqueness.
pub async fn create_alias(
    db: &DatabaseConnection,
    payload: CreateReferenceAliasPayload,
    _actor_id: i64,
) -> AppResult<ReferenceAlias> {
    let label = payload.alias_label.trim().to_string();
    let locale = payload.locale.trim().to_lowercase();
    let alias_type = payload.alias_type.trim().to_lowercase();
    let is_preferred = payload.is_preferred.unwrap_or(false);

    validate_alias_label(&label)?;
    validate_locale(&locale)?;
    validate_alias_type(&alias_type)?;
    assert_value_exists(db, payload.reference_value_id).await?;

    // Enforce preferred uniqueness before insert.
    if is_preferred {
        enforce_preferred_uniqueness(db, payload.reference_value_id, &locale, &alias_type, None)
            .await?;
    }

    let now = Utc::now().to_rfc3339();
    let preferred_int: i32 = if is_preferred { 1 } else { 0 };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_aliases \
             (reference_value_id, alias_label, locale, alias_type, is_preferred, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            payload.reference_value_id.into(),
            label.clone().into(),
            locale.clone().into(),
            alias_type.clone().into(),
            preferred_int.into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![format!(
                "Un alias '{label}' existe déjà pour cette valeur, locale '{locale}', \
                 et type '{alias_type}'."
            )])
        } else {
            AppError::Database(e)
        }
    })?;

    // Return the created alias by seeking the latest insert for this scope.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_aliases \
                 WHERE reference_value_id = ? AND locale = ? AND alias_type = ? AND alias_label = ?"
            ),
            [
                payload.reference_value_id.into(),
                locale.into(),
                alias_type.into(),
                label.into(),
            ],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "reference_aliases row missing after insert"
            ))
        })?;

    map_alias(&row)
}

/// Updates a reference alias. Only provided fields are changed.
///
/// Enforces preferred uniqueness if is_preferred is set to true.
pub async fn update_alias(
    db: &DatabaseConnection,
    alias_id: i64,
    payload: UpdateReferenceAliasPayload,
    _actor_id: i64,
) -> AppResult<ReferenceAlias> {
    let existing = get_alias(db, alias_id).await?;

    let label = match &payload.alias_label {
        Some(l) => {
            let trimmed = l.trim().to_string();
            validate_alias_label(&trimmed)?;
            trimmed
        }
        None => existing.alias_label.clone(),
    };

    let locale = match &payload.locale {
        Some(l) => {
            let trimmed = l.trim().to_lowercase();
            validate_locale(&trimmed)?;
            trimmed
        }
        None => existing.locale.clone(),
    };

    let alias_type = match &payload.alias_type {
        Some(t) => {
            let trimmed = t.trim().to_lowercase();
            validate_alias_type(&trimmed)?;
            trimmed
        }
        None => existing.alias_type.clone(),
    };

    let is_preferred = payload.is_preferred.unwrap_or(existing.is_preferred);

    // Enforce preferred uniqueness if becoming preferred.
    if is_preferred {
        enforce_preferred_uniqueness(
            db,
            existing.reference_value_id,
            &locale,
            &alias_type,
            Some(alias_id),
        )
        .await?;
    }

    let preferred_int: i32 = if is_preferred { 1 } else { 0 };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_aliases \
         SET alias_label = ?, locale = ?, alias_type = ?, is_preferred = ? \
         WHERE id = ?",
        [
            label.into(),
            locale.into(),
            alias_type.into(),
            preferred_int.into(),
            alias_id.into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![
                "Un alias avec ce libellé existe déjà pour cette combinaison \
                 valeur/locale/type."
                    .into(),
            ])
        } else {
            AppError::Database(e)
        }
    })?;

    get_alias(db, alias_id).await
}

/// Deletes a reference alias by id. If the deleted alias was preferred,
/// auto-promotes the oldest remaining alias in the same scope.
pub async fn delete_alias(
    db: &DatabaseConnection,
    alias_id: i64,
    _actor_id: i64,
) -> AppResult<()> {
    let existing = get_alias(db, alias_id).await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reference_aliases WHERE id = ?",
        [alias_id.into()],
    ))
    .await?;

    // Auto-promote if deleted alias was preferred.
    if existing.is_preferred {
        auto_promote_preferred(
            db,
            existing.reference_value_id,
            &existing.locale,
            &existing.alias_type,
        )
        .await?;
    }

    Ok(())
}
