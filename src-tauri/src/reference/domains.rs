//! Reference domain catalog service.
//!
//! Phase 2 - Sub-phase 03 - File 01 - Sprint S1.
//!
//! Provides governed CRUD for `reference_domains` (migration 013).
//! Each domain represents a named, governed vocabulary with a structure type
//! and governance level that control editing workflows and downstream impact.
//!
//! Validation rules:
//!   - `code`: uppercase ASCII letters, digits, underscores, or dots; 1–64 chars
//!   - `structure_type`: one of the PRD 6.13 types
//!   - `governance_level`: one of the PRD 6.13 levels

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Constants ────────────────────────────────────────────────────────────────

/// PRD 6.13 allowed structure types.
pub const STRUCTURE_TYPES: &[&str] = &[
    "flat",
    "hierarchical",
    "versioned_code_set",
    "unit_set",
    "external_code_set",
];

/// PRD 6.13 allowed governance levels.
pub const GOVERNANCE_LEVELS: &[&str] = &[
    "protected_analytical",
    "tenant_managed",
    "system_seeded",
    "erp_synced",
];

// ─── Types ────────────────────────────────────────────────────────────────────

/// Complete reference domain record for reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceDomain {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub structure_type: String,
    pub governance_level: String,
    pub is_extendable: bool,
    pub validation_rules_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Payload for creating a reference domain.
#[derive(Debug, Deserialize)]
pub struct CreateReferenceDomainPayload {
    pub code: String,
    pub name: String,
    pub structure_type: String,
    pub governance_level: String,
    pub is_extendable: Option<bool>,
    pub validation_rules_json: Option<String>,
}

/// Payload for updating a reference domain. Only provided fields are changed.
#[derive(Debug, Deserialize)]
pub struct UpdateReferenceDomainPayload {
    pub name: Option<String>,
    pub structure_type: Option<String>,
    pub governance_level: Option<String>,
    pub is_extendable: Option<bool>,
    pub validation_rules_json: Option<Option<String>>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_domains row decode failed for column '{column}': {e}"
    ))
}

fn map_domain(row: &QueryResult) -> AppResult<ReferenceDomain> {
    Ok(ReferenceDomain {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
        name: row
            .try_get::<String>("", "name")
            .map_err(|e| decode_err("name", e))?,
        structure_type: row
            .try_get::<String>("", "structure_type")
            .map_err(|e| decode_err("structure_type", e))?,
        governance_level: row
            .try_get::<String>("", "governance_level")
            .map_err(|e| decode_err("governance_level", e))?,
        is_extendable: i64_to_bool(
            row.try_get::<i64>("", "is_extendable")
                .map_err(|e| decode_err("is_extendable", e))?,
        ),
        validation_rules_json: row
            .try_get::<Option<String>>("", "validation_rules_json")
            .map_err(|e| decode_err("validation_rules_json", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// Validates a reference domain code.
/// Must be uppercase ASCII letters, digits, underscores, or dots; 1–64 characters.
fn validate_code(code: &str) -> AppResult<()> {
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::ValidationFailed(vec![
            "Le code de domaine doit comporter entre 1 et 64 caractères.".into(),
        ]));
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '.')
    {
        return Err(AppError::ValidationFailed(vec![
            "Le code de domaine ne peut contenir que des lettres majuscules ASCII, \
             des chiffres, des underscores et des points."
                .into(),
        ]));
    }
    // Must start with a letter
    if !code.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Err(AppError::ValidationFailed(vec![
            "Le code de domaine doit commencer par une lettre majuscule.".into(),
        ]));
    }
    Ok(())
}

fn validate_structure_type(st: &str) -> AppResult<()> {
    if !STRUCTURE_TYPES.contains(&st) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type de structure '{st}' invalide. Valeurs autorisées : {}.",
            STRUCTURE_TYPES.join(", ")
        )]));
    }
    Ok(())
}

fn validate_governance_level(gl: &str) -> AppResult<()> {
    if !GOVERNANCE_LEVELS.contains(&gl) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Niveau de gouvernance '{gl}' invalide. Valeurs autorisées : {}.",
            GOVERNANCE_LEVELS.join(", ")
        )]));
    }
    Ok(())
}

fn validate_name(name: &str) -> AppResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err(AppError::ValidationFailed(vec![
            "Le nom du domaine doit comporter entre 1 et 255 caractères.".into(),
        ]));
    }
    Ok(())
}

/// If `validation_rules_json` is provided, verify it is valid JSON.
fn validate_rules_json(json: &Option<String>) -> AppResult<()> {
    if let Some(raw) = json {
        if !raw.is_empty() {
            serde_json::from_str::<serde_json::Value>(raw).map_err(|e| {
                AppError::ValidationFailed(vec![format!(
                    "Le champ validation_rules_json n'est pas du JSON valide : {e}"
                )])
            })?;
        }
    }
    Ok(())
}

/// Normalizes a domain code to uppercase (caller may pass mixed case).
fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Returns all reference domains ordered by name.
pub async fn list_reference_domains(
    db: &DatabaseConnection,
) -> AppResult<Vec<ReferenceDomain>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, structure_type, governance_level, \
                    is_extendable, validation_rules_json, created_at, updated_at \
             FROM reference_domains \
             ORDER BY name ASC",
        ))
        .await?;

    rows.iter().map(map_domain).collect()
}

/// Returns a single reference domain by id.
pub async fn get_reference_domain(
    db: &DatabaseConnection,
    domain_id: i64,
) -> AppResult<ReferenceDomain> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, structure_type, governance_level, \
                    is_extendable, validation_rules_json, created_at, updated_at \
             FROM reference_domains WHERE id = ?",
            [domain_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceDomain".into(),
            id: domain_id.to_string(),
        })?;

    map_domain(&row)
}

/// Creates a new reference domain. Returns the created domain.
///
/// Validates code format, structure type, governance level, and name.
/// Code is normalized to uppercase before insertion.
pub async fn create_reference_domain(
    db: &DatabaseConnection,
    payload: CreateReferenceDomainPayload,
    _actor_id: i64,
) -> AppResult<ReferenceDomain> {
    let code = normalize_code(&payload.code);
    validate_code(&code)?;
    validate_name(&payload.name)?;
    validate_structure_type(&payload.structure_type)?;
    validate_governance_level(&payload.governance_level)?;
    validate_rules_json(&payload.validation_rules_json)?;

    let now = Utc::now().to_rfc3339();
    let is_extendable: i32 = if payload.is_extendable.unwrap_or(true) {
        1
    } else {
        0
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_domains \
             (code, name, structure_type, governance_level, is_extendable, \
              validation_rules_json, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            code.clone().into(),
            payload.name.trim().to_string().into(),
            payload.structure_type.clone().into(),
            payload.governance_level.clone().into(),
            is_extendable.into(),
            payload.validation_rules_json.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| {
        // SQLite unique constraint violation on code
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![format!(
                "Un domaine avec le code '{code}' existe déjà."
            )])
        } else {
            AppError::Database(e)
        }
    })?;

    // Return the newly created domain by code lookup (safe — code is unique).
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, structure_type, governance_level, \
                    is_extendable, validation_rules_json, created_at, updated_at \
             FROM reference_domains WHERE code = ?",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "reference_domains row missing after insert"
            ))
        })?;

    map_domain(&row)
}

/// Updates an existing reference domain. Returns the updated domain.
///
/// Only provided (Some) fields are changed; `None` fields are left untouched.
/// Code is immutable after creation — it is not present in the update payload.
pub async fn update_reference_domain(
    db: &DatabaseConnection,
    domain_id: i64,
    payload: UpdateReferenceDomainPayload,
    _actor_id: i64,
) -> AppResult<ReferenceDomain> {
    // Verify the domain exists first.
    let existing = get_reference_domain(db, domain_id).await?;

    // Build the SET clause dynamically for only provided fields.
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref name) = payload.name {
        validate_name(name)?;
        sets.push("name = ?".into());
        values.push(name.trim().to_string().into());
    }

    if let Some(ref st) = payload.structure_type {
        validate_structure_type(st)?;
        sets.push("structure_type = ?".into());
        values.push(st.clone().into());
    }

    if let Some(ref gl) = payload.governance_level {
        validate_governance_level(gl)?;
        sets.push("governance_level = ?".into());
        values.push(gl.clone().into());
    }

    if let Some(ext) = payload.is_extendable {
        sets.push("is_extendable = ?".into());
        values.push(if ext { 1i32 } else { 0i32 }.into());
    }

    if let Some(ref rules) = payload.validation_rules_json {
        validate_rules_json(rules)?;
        sets.push("validation_rules_json = ?".into());
        values.push(rules.clone().into());
    }

    if sets.is_empty() {
        // Nothing to update — return existing domain as-is.
        return Ok(existing);
    }

    let now = Utc::now().to_rfc3339();
    sets.push("updated_at = ?".into());
    values.push(now.into());
    values.push(domain_id.into());

    let sql = format!(
        "UPDATE reference_domains SET {} WHERE id = ?",
        sets.join(", ")
    );

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &sql,
        values,
    ))
    .await?;

    get_reference_domain(db, domain_id).await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_codes() {
        assert!(validate_code("FAILURE_CLASS").is_ok());
        assert!(validate_code("EQUIPMENT.FAMILY").is_ok());
        assert!(validate_code("A").is_ok());
        assert!(validate_code("UOM_SET_1").is_ok());
    }

    #[test]
    fn invalid_codes() {
        // lowercase
        assert!(validate_code("failure_class").is_err());
        // empty
        assert!(validate_code("").is_err());
        // starts with digit
        assert!(validate_code("1ABC").is_err());
        // too long
        assert!(validate_code(&"A".repeat(65)).is_err());
        // spaces
        assert!(validate_code("FAILURE CLASS").is_err());
        // hyphens
        assert!(validate_code("FAILURE-CLASS").is_err());
    }

    #[test]
    fn valid_structure_types() {
        for st in STRUCTURE_TYPES {
            assert!(validate_structure_type(st).is_ok());
        }
    }

    #[test]
    fn invalid_structure_type() {
        assert!(validate_structure_type("tree").is_err());
        assert!(validate_structure_type("FLAT").is_err()); // case-sensitive
        assert!(validate_structure_type("").is_err());
    }

    #[test]
    fn valid_governance_levels() {
        for gl in GOVERNANCE_LEVELS {
            assert!(validate_governance_level(gl).is_ok());
        }
    }

    #[test]
    fn invalid_governance_level() {
        assert!(validate_governance_level("public").is_err());
        assert!(validate_governance_level("PROTECTED_ANALYTICAL").is_err());
        assert!(validate_governance_level("").is_err());
    }

    #[test]
    fn normalize_code_uppercases() {
        assert_eq!(normalize_code("  failure.class "), "FAILURE.CLASS");
    }

    #[test]
    fn valid_rules_json() {
        assert!(validate_rules_json(&None).is_ok());
        assert!(validate_rules_json(&Some(String::new())).is_ok());
        assert!(
            validate_rules_json(&Some(r#"{"max_depth": 3}"#.into())).is_ok()
        );
    }

    #[test]
    fn invalid_rules_json() {
        assert!(validate_rules_json(&Some("not json".into())).is_err());
    }
}
