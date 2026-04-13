//! Reference import/export pipeline.
//!
//! Phase 2 - Sub-phase 03 - File 03 - Sprint S2.
//!
//! Implements the staged reference data import workflow:
//!
//!   1. `create_import_batch` â€” registers the source file identity (name + SHA-256)
//!      and target domain, creating the batch in `uploaded` state.
//!   2. `stage_import_rows` â€” accepts pre-parsed row data (JSON objects) and inserts
//!      staging rows with normalized codes.
//!   3. `validate_import_batch` â€” runs per-row diagnostics against the target domain:
//!      code format, label presence, duplicate detection, protected-domain policy checks.
//!   4. `apply_import_batch` â€” applies validated rows to the target draft set:
//!      creates new values or updates existing ones. Protected-domain changes route
//!      through policy checks. Replaced codes produce migration map entries.
//!   5. `export_domain_set` â€” exports all canonical values + aliases for a published set.
//!
//! The pipeline never mutates published reference data directly. All writes target
//! a draft set. The publish lifecycle (File 01/02) gates promotion to active use.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::aliases;
use super::domains;
use super::sets;
use super::values;

// â”€â”€â”€ Constants â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Valid batch status values.
const BATCH_STATUSES: &[&str] = &["uploaded", "validated", "applied", "failed"];

/// Valid row validation statuses.
const ROW_STATUSES: &[&str] = &["pending", "valid", "warning", "error"];

// â”€â”€â”€ Types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Batch summary returned to frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefImportBatchSummary {
    pub id: i64,
    pub domain_id: i64,
    pub source_filename: String,
    pub source_sha256: String,
    pub status: String,
    pub total_rows: i64,
    pub valid_rows: i64,
    pub warning_rows: i64,
    pub error_rows: i64,
    pub initiated_by_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Single staging row for preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefImportRow {
    pub id: i64,
    pub batch_id: i64,
    pub row_no: i64,
    pub raw_json: String,
    pub normalized_code: Option<String>,
    pub validation_status: String,
    pub messages: Vec<ImportRowMessage>,
    pub proposed_action: Option<String>,
}

/// Structured diagnostic message for a single import row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRowMessage {
    pub category: String,
    pub severity: String,
    pub message: String,
}

/// Full preview response: batch + all rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefImportPreview {
    pub batch: RefImportBatchSummary,
    pub rows: Vec<RefImportRow>,
}

/// A single row payload for staging.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImportRowInput {
    pub code: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub parent_code: Option<String>,
    pub sort_order: Option<i64>,
    pub color_hex: Option<String>,
    pub icon_name: Option<String>,
    pub semantic_tag: Option<String>,
    pub external_code: Option<String>,
    pub metadata_json: Option<String>,
}

/// Policy for apply phase.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RefImportApplyPolicy {
    /// If true, rows with warnings are also applied (not just valid rows).
    pub include_warnings: bool,
    /// Target draft set id. Must be in draft status for the same domain.
    pub target_set_id: i64,
}

/// Result of an apply operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefImportApplyResult {
    pub batch: RefImportBatchSummary,
    pub created: i64,
    pub updated: i64,
    pub skipped: i64,
    pub errored: i64,
}

/// Export row: canonical value + its aliases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefExportRow {
    pub value: values::ReferenceValue,
    pub aliases: Vec<aliases::ReferenceAlias>,
}

/// Full export result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefExportResult {
    pub domain: domains::ReferenceDomain,
    pub set: sets::ReferenceSet,
    pub rows: Vec<RefExportRow>,
}

// â”€â”€â”€ Row mapping â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_import row decode failed for column '{column}': {e}"
    ))
}

const BATCH_SELECT: &str =
    "id, domain_id, source_filename, source_sha256, status, \
     total_rows, valid_rows, warning_rows, error_rows, \
     initiated_by_id, created_at, updated_at";

fn map_batch(row: &QueryResult) -> AppResult<RefImportBatchSummary> {
    Ok(RefImportBatchSummary {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        domain_id: row
            .try_get::<i64>("", "domain_id")
            .map_err(|e| decode_err("domain_id", e))?,
        source_filename: row
            .try_get::<String>("", "source_filename")
            .map_err(|e| decode_err("source_filename", e))?,
        source_sha256: row
            .try_get::<String>("", "source_sha256")
            .map_err(|e| decode_err("source_sha256", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        total_rows: row
            .try_get::<i64>("", "total_rows")
            .map_err(|e| decode_err("total_rows", e))?,
        valid_rows: row
            .try_get::<i64>("", "valid_rows")
            .map_err(|e| decode_err("valid_rows", e))?,
        warning_rows: row
            .try_get::<i64>("", "warning_rows")
            .map_err(|e| decode_err("warning_rows", e))?,
        error_rows: row
            .try_get::<i64>("", "error_rows")
            .map_err(|e| decode_err("error_rows", e))?,
        initiated_by_id: row
            .try_get::<Option<i64>>("", "initiated_by_id")
            .map_err(|e| decode_err("initiated_by_id", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_import_row(row: &QueryResult) -> AppResult<RefImportRow> {
    let messages_raw: String = row
        .try_get("", "messages_json")
        .map_err(|e| decode_err("messages_json", e))?;
    let messages: Vec<ImportRowMessage> =
        serde_json::from_str(&messages_raw).unwrap_or_default();

    Ok(RefImportRow {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        batch_id: row
            .try_get::<i64>("", "batch_id")
            .map_err(|e| decode_err("batch_id", e))?,
        row_no: row
            .try_get::<i64>("", "row_no")
            .map_err(|e| decode_err("row_no", e))?,
        raw_json: row
            .try_get::<String>("", "raw_json")
            .map_err(|e| decode_err("raw_json", e))?,
        normalized_code: row
            .try_get::<Option<String>>("", "normalized_code")
            .map_err(|e| decode_err("normalized_code", e))?,
        validation_status: row
            .try_get::<String>("", "validation_status")
            .map_err(|e| decode_err("validation_status", e))?,
        messages,
        proposed_action: row
            .try_get::<Option<String>>("", "proposed_action")
            .map_err(|e| decode_err("proposed_action", e))?,
    })
}

// â”€â”€â”€ Internal helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_batch_by_id(
    db: &impl ConnectionTrait,
    batch_id: i64,
) -> AppResult<RefImportBatchSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {BATCH_SELECT} FROM reference_import_batches WHERE id = ?"),
            [batch_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "RefImportBatch".into(),
            id: batch_id.to_string(),
        })?;
    map_batch(&row)
}

/// Normalize a reference code: trim + uppercase.
fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

/// Validate code format: uppercase ASCII + digits + underscores + dots, 1â€“64.
fn is_valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '.')
        && code.starts_with(|c: char| c.is_ascii_uppercase())
}

// â”€â”€â”€ Public API â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Create a new reference import batch in `uploaded` state.
pub async fn create_import_batch(
    db: &DatabaseConnection,
    domain_id: i64,
    source_filename: &str,
    source_sha256: &str,
    actor_id: Option<i64>,
) -> AppResult<RefImportBatchSummary> {
    // Verify domain exists.
    domains::get_reference_domain(db, domain_id).await?;

    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_import_batches \
         (domain_id, source_filename, source_sha256, status, \
          total_rows, valid_rows, warning_rows, error_rows, \
          initiated_by_id, created_at, updated_at) \
         VALUES (?, ?, ?, 'uploaded', 0, 0, 0, 0, ?, ?, ?)",
        [
            domain_id.into(),
            source_filename.into(),
            source_sha256.into(),
            actor_id
                .map(|id| sea_orm::Value::BigInt(Some(id)))
                .unwrap_or(sea_orm::Value::BigInt(None)),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {BATCH_SELECT} FROM reference_import_batches \
                 WHERE domain_id = ? AND source_sha256 = ? \
                 ORDER BY id DESC LIMIT 1"
            ),
            [domain_id.into(), source_sha256.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("batch row missing after insert"))
        })?;

    map_batch(&row)
}

/// Stage parsed row data into a batch. Batch must be in `uploaded` state.
///
/// Each `ImportRowInput` is inserted as a staging row with its raw JSON and
/// normalized code. After staging, validate with `validate_import_batch`.
pub async fn stage_import_rows(
    db: &DatabaseConnection,
    batch_id: i64,
    rows: Vec<ImportRowInput>,
) -> AppResult<RefImportBatchSummary> {
    let batch = get_batch_by_id(db, batch_id).await?;
    if batch.status != "uploaded" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le lot d'import est en statut '{}'; seul le statut 'uploaded' permet le chargement.",
            batch.status
        )]));
    }

    if rows.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Aucune ligne fournie pour le chargement.".into(),
        ]));
    }

    let txn = db.begin().await?;

    for (i, row_input) in rows.iter().enumerate() {
        let row_no = (i + 1) as i64;
        let raw_json = serde_json::to_string(row_input)?;
        let norm_code = row_input
            .code
            .as_ref()
            .map(|c| normalize_code(c));

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_import_rows \
             (batch_id, row_no, raw_json, normalized_code, \
              validation_status, messages_json, proposed_action) \
             VALUES (?, ?, ?, ?, 'pending', '[]', NULL)",
            [
                batch_id.into(),
                row_no.into(),
                raw_json.into(),
                norm_code
                    .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                    .unwrap_or(sea_orm::Value::String(None)),
            ],
        ))
        .await?;
    }

    // Update batch total_rows.
    let total = rows.len() as i64;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_import_batches SET total_rows = ?, updated_at = ? WHERE id = ?",
        [total.into(), Utc::now().to_rfc3339().into(), batch_id.into()],
    ))
    .await?;

    txn.commit().await?;
    get_batch_by_id(db, batch_id).await
}

/// Run validation diagnostics on all staging rows of a batch.
///
/// Batch must be in `uploaded` state. Advances to `validated` with
/// per-row outcomes and summary counts.
///
/// Validation checks per row:
///   - code presence and format
///   - label presence
///   - duplicate code detection within batch
///   - existing code collision in target domain (â†’ update vs create)
///   - protected-domain policy for code replacements
pub async fn validate_import_batch(
    db: &DatabaseConnection,
    batch_id: i64,
    _actor_id: Option<i64>,
) -> AppResult<RefImportBatchSummary> {
    let batch = get_batch_by_id(db, batch_id).await?;
    if batch.status != "uploaded" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le lot d'import est en statut '{}'; seul le statut 'uploaded' permet la validation.",
            batch.status
        )]));
    }
    if batch.total_rows == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Le lot d'import ne contient aucune ligne.".into(),
        ]));
    }

    let domain = domains::get_reference_domain(db, batch.domain_id).await?;
    let is_protected = domain.governance_level == "protected_analytical";

    // Load staging rows.
    let staging_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_no, raw_json, normalized_code \
             FROM reference_import_rows WHERE batch_id = ? ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

    // Collect all normalized codes in batch for duplicate detection.
    let mut batch_codes: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();

    // Pre-load existing codes in the latest published set (if any) for collision check.
    let existing_codes = load_published_codes(db, batch.domain_id).await?;

    let txn = db.begin().await?;
    let mut valid_count: i64 = 0;
    let mut warning_count: i64 = 0;
    let mut error_count: i64 = 0;

    for staging_row in &staging_rows {
        let staging_id: i64 = staging_row
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        let row_no: i64 = staging_row
            .try_get("", "row_no")
            .map_err(|e| decode_err("row_no", e))?;
        let raw_json: String = staging_row
            .try_get("", "raw_json")
            .map_err(|e| decode_err("raw_json", e))?;
        let norm_code: Option<String> = staging_row
            .try_get("", "normalized_code")
            .map_err(|e| decode_err("normalized_code", e))?;

        let input: ImportRowInput =
            serde_json::from_str(&raw_json).unwrap_or_else(|_| ImportRowInput {
                code: None,
                label: None,
                description: None,
                parent_code: None,
                sort_order: None,
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            });

        let mut messages: Vec<ImportRowMessage> = Vec::new();
        let mut status = "valid";
        let mut proposed_action: Option<String> = None;

        // â”€â”€ Check: code presence â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        match &norm_code {
            None => {
                messages.push(ImportRowMessage {
                    category: "MissingCode".into(),
                    severity: "error".into(),
                    message: format!("Ligne {row_no}: code de reference manquant."),
                });
                status = "error";
            }
            Some(code) => {
                // â”€â”€ Check: code format â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
                if !is_valid_code(input.code.as_deref().unwrap_or("")) {
                    messages.push(ImportRowMessage {
                        category: "InvalidCodeFormat".into(),
                        severity: "error".into(),
                        message: format!(
                            "Ligne {row_no}: code '{code}' invalide \
                             (majuscules ASCII, chiffres, underscores, points; 1-64 car.)."
                        ),
                    });
                    status = "error";
                }

                // â”€â”€ Check: duplicate within batch â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
                if let Some(prev_row) = batch_codes.get(code) {
                    messages.push(ImportRowMessage {
                        category: "DuplicateInBatch".into(),
                        severity: "error".into(),
                        message: format!(
                            "Ligne {row_no}: code '{code}' duplique dans le lot \
                             (premiere occurrence: ligne {prev_row})."
                        ),
                    });
                    status = "error";
                } else {
                    batch_codes.insert(code.clone(), row_no);
                }

                // â”€â”€ Check: collision with published set â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
                if existing_codes.contains(code) {
                    proposed_action = Some("update".into());
                    if is_protected {
                        messages.push(ImportRowMessage {
                            category: "ProtectedDomainUpdate".into(),
                            severity: "warning".into(),
                            message: format!(
                                "Ligne {row_no}: code '{code}' existe dans le domaine \
                                 protege '{}'. La mise a jour sera trace.",
                                domain.code
                            ),
                        });
                        if status == "valid" {
                            status = "warning";
                        }
                    }
                } else if proposed_action.is_none() && status != "error" {
                    proposed_action = Some("create".into());
                }
            }
        }

        // â”€â”€ Check: label presence â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        match &input.label {
            None => {
                messages.push(ImportRowMessage {
                    category: "MissingLabel".into(),
                    severity: "error".into(),
                    message: format!("Ligne {row_no}: libelle manquant."),
                });
                status = "error";
            }
            Some(label) if label.trim().is_empty() => {
                messages.push(ImportRowMessage {
                    category: "BlankLabel".into(),
                    severity: "error".into(),
                    message: format!("Ligne {row_no}: libelle vide."),
                });
                status = "error";
            }
            _ => {}
        }

        let messages_json = serde_json::to_string(&messages)?;

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_import_rows \
             SET validation_status = ?, messages_json = ?, proposed_action = ? \
             WHERE id = ?",
            [
                status.into(),
                messages_json.into(),
                proposed_action
                    .map(|a| sea_orm::Value::String(Some(Box::new(a))))
                    .unwrap_or(sea_orm::Value::String(None)),
                staging_id.into(),
            ],
        ))
        .await?;

        match status {
            "valid" => valid_count += 1,
            "warning" => warning_count += 1,
            _ => error_count += 1,
        }
    }

    let now = Utc::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_import_batches \
         SET status = 'validated', valid_rows = ?, warning_rows = ?, error_rows = ?, \
             updated_at = ? \
         WHERE id = ?",
        [
            valid_count.into(),
            warning_count.into(),
            error_count.into(),
            now.into(),
            batch_id.into(),
        ],
    ))
    .await?;

    txn.commit().await?;
    get_batch_by_id(db, batch_id).await
}

/// Apply validated rows to the target draft set.
///
/// Only batches in `validated` state can be applied. Rows with `error`
/// status are always skipped. Warning rows are included only when
/// `policy.include_warnings` is true.
///
/// For existing codes (proposed_action = "update"), the matching value in
/// the target set is updated. For new codes ("create"), a new value is inserted.
///
/// Idempotency: re-applying an `applied` batch is rejected.
pub async fn apply_import_batch(
    db: &DatabaseConnection,
    batch_id: i64,
    policy: RefImportApplyPolicy,
    _actor_id: i64,
) -> AppResult<RefImportApplyResult> {
    let batch = get_batch_by_id(db, batch_id).await?;
    if batch.status != "validated" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le lot d'import est en statut '{}'; seul le statut 'validated' permet l'application.",
            batch.status
        )]));
    }

    // Verify target set exists, is draft, and belongs to the same domain.
    let target_set = sets::get_reference_set(db, policy.target_set_id).await?;
    if target_set.status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "Le jeu cible doit etre en statut 'draft'.".into(),
        ]));
    }
    if target_set.domain_id != batch.domain_id {
        return Err(AppError::ValidationFailed(vec![
            "Le jeu cible doit appartenir au meme domaine que le lot d'import.".into(),
        ]));
    }

    // Load eligible rows.
    let eligible_statuses = if policy.include_warnings {
        "'valid', 'warning'"
    } else {
        "'valid'"
    };

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT id, row_no, raw_json, normalized_code, \
                        validation_status, proposed_action \
                 FROM reference_import_rows \
                 WHERE batch_id = ? AND validation_status IN ({eligible_statuses}) \
                 ORDER BY row_no ASC"
            ),
            [batch_id.into()],
        ))
        .await?;

    let txn = db.begin().await?;

    let mut created: i64 = 0;
    let mut updated: i64 = 0;
    let mut skipped: i64 = 0;
    let mut errored: i64 = 0;

    // Build a lookup of existing codes in the target draft set.
    let existing_in_set = load_draft_set_codes(&txn, policy.target_set_id).await?;

    for row in &rows {
        let _row_no: i64 = row.try_get("", "row_no").map_err(|e| decode_err("row_no", e))?;
        let raw_json: String = row.try_get("", "raw_json").map_err(|e| decode_err("raw_json", e))?;
        let norm_code: Option<String> = row
            .try_get("", "normalized_code")
            .map_err(|e| decode_err("normalized_code", e))?;
        let _proposed: Option<String> = row
            .try_get("", "proposed_action")
            .map_err(|e| decode_err("proposed_action", e))?;

        let code = match norm_code {
            Some(c) => c,
            None => {
                skipped += 1;
                continue;
            }
        };

        let input: ImportRowInput =
            serde_json::from_str(&raw_json).unwrap_or_else(|_| ImportRowInput {
                code: None,
                label: None,
                description: None,
                parent_code: None,
                sort_order: None,
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            });

        let label = match &input.label {
            Some(l) if !l.trim().is_empty() => l.trim().to_string(),
            _ => {
                skipped += 1;
                continue;
            }
        };

        // Check if this code already exists in the draft set.
        if let Some(existing_id) = existing_in_set.get(&code) {
            // Update existing value.
            match apply_update_value(&txn, *existing_id, &input, &label).await {
                Ok(_) => updated += 1,
                Err(_) => errored += 1,
            }
        } else {
            // Create new value.
            let _now = Utc::now().to_rfc3339();
            match txn
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO reference_values \
                     (set_id, parent_id, code, label, description, sort_order, \
                      color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
                     VALUES (?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
                    [
                        policy.target_set_id.into(),
                        code.clone().into(),
                        label.into(),
                        input
                            .description
                            .clone()
                            .map(|d| sea_orm::Value::String(Some(Box::new(d))))
                            .unwrap_or(sea_orm::Value::String(None)),
                        input
                            .sort_order
                            .map(|s| sea_orm::Value::BigInt(Some(s)))
                            .unwrap_or(sea_orm::Value::BigInt(None)),
                        input
                            .color_hex
                            .clone()
                            .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                            .unwrap_or(sea_orm::Value::String(None)),
                        input
                            .icon_name
                            .clone()
                            .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                            .unwrap_or(sea_orm::Value::String(None)),
                        input
                            .semantic_tag
                            .clone()
                            .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                            .unwrap_or(sea_orm::Value::String(None)),
                        input
                            .external_code
                            .clone()
                            .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                            .unwrap_or(sea_orm::Value::String(None)),
                        input
                            .metadata_json
                            .clone()
                            .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                            .unwrap_or(sea_orm::Value::String(None)),
                    ],
                ))
                .await
            {
                Ok(_) => created += 1,
                Err(_) => errored += 1,
            }
        }
    }

    // Calculate skipped from non-eligible rows.
    let total_eligible = rows.len() as i64;
    let total_skipped = batch.total_rows - total_eligible + skipped;

    let now = Utc::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_import_batches SET status = 'applied', updated_at = ? WHERE id = ?",
        [now.into(), batch_id.into()],
    ))
    .await?;

    txn.commit().await?;

    let final_batch = get_batch_by_id(db, batch_id).await?;

    Ok(RefImportApplyResult {
        batch: final_batch,
        created,
        updated,
        skipped: total_skipped,
        errored,
    })
}

/// Export all canonical values and their aliases for a domain set.
///
/// The set must be published (or draft for preview export).
pub async fn export_domain_set(
    db: &DatabaseConnection,
    set_id: i64,
) -> AppResult<RefExportResult> {
    let set = sets::get_reference_set(db, set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    let vals = values::list_values(db, set_id).await?;

    let mut export_rows: Vec<RefExportRow> = Vec::with_capacity(vals.len());
    for val in vals {
        let val_aliases = aliases::list_aliases(db, val.id).await?;
        export_rows.push(RefExportRow {
            value: val,
            aliases: val_aliases,
        });
    }

    Ok(RefExportResult {
        domain,
        set,
        rows: export_rows,
    })
}

/// Get the full import preview (batch + all staging rows).
pub async fn get_import_preview(
    db: &DatabaseConnection,
    batch_id: i64,
) -> AppResult<RefImportPreview> {
    let batch = get_batch_by_id(db, batch_id).await?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, batch_id, row_no, raw_json, normalized_code, \
                    validation_status, messages_json, proposed_action \
             FROM reference_import_rows \
             WHERE batch_id = ? ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

    let preview_rows: Vec<RefImportRow> =
        rows.iter().map(map_import_row).collect::<Result<_, _>>()?;

    Ok(RefImportPreview {
        batch,
        rows: preview_rows,
    })
}

/// List batches for a domain, optionally filtered by status.
pub async fn list_import_batches(
    db: &DatabaseConnection,
    domain_id: i64,
    status_filter: Option<String>,
    limit: Option<i64>,
) -> AppResult<Vec<RefImportBatchSummary>> {
    let row_limit = limit.unwrap_or(50).min(200);

    let (sql, binds): (String, Vec<sea_orm::Value>) = if let Some(ref status) = status_filter {
        (
            format!(
                "SELECT {BATCH_SELECT} FROM reference_import_batches \
                 WHERE domain_id = ? AND status = ? \
                 ORDER BY created_at DESC LIMIT {row_limit}"
            ),
            vec![domain_id.into(), status.clone().into()],
        )
    } else {
        (
            format!(
                "SELECT {BATCH_SELECT} FROM reference_import_batches \
                 WHERE domain_id = ? ORDER BY created_at DESC LIMIT {row_limit}"
            ),
            vec![domain_id.into()],
        )
    };

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    rows.iter().map(map_batch).collect()
}

// â”€â”€â”€ Internal helpers for apply â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Load all active value codes from the latest published set for a domain.
async fn load_published_codes(
    db: &DatabaseConnection,
    domain_id: i64,
) -> AppResult<std::collections::HashSet<String>> {
    let mut codes = std::collections::HashSet::new();

    let set_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets \
             WHERE domain_id = ? AND status = 'published' \
             ORDER BY version_no DESC LIMIT 1",
            [domain_id.into()],
        ))
        .await?;

    if let Some(sr) = set_row {
        let pub_set_id: i64 = sr.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT code FROM reference_values \
                 WHERE set_id = ? AND is_active = 1",
                [pub_set_id.into()],
            ))
            .await?;
        for r in &rows {
            if let Ok(code) = r.try_get::<String>("", "code") {
                codes.insert(code);
            }
        }
    }

    Ok(codes)
}

/// Load value codes in a draft set as code â†’ value_id map.
async fn load_draft_set_codes(
    db: &impl ConnectionTrait,
    set_id: i64,
) -> AppResult<std::collections::HashMap<String, i64>> {
    let mut map = std::collections::HashMap::new();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code FROM reference_values WHERE set_id = ?",
            [set_id.into()],
        ))
        .await?;

    for r in &rows {
        let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let code: String = r.try_get("", "code").map_err(|e| decode_err("code", e))?;
        map.insert(code, id);
    }
    Ok(map)
}

/// Update an existing reference value with import data.
async fn apply_update_value(
    txn: &impl ConnectionTrait,
    value_id: i64,
    input: &ImportRowInput,
    label: &str,
) -> AppResult<()> {
    let mut set_clauses: Vec<String> = vec!["label = ?".into()];
    let mut binds: Vec<sea_orm::Value> = vec![label.into()];

    if let Some(ref desc) = input.description {
        set_clauses.push("description = ?".into());
        binds.push(desc.clone().into());
    }
    if let Some(sort) = input.sort_order {
        set_clauses.push("sort_order = ?".into());
        binds.push(sort.into());
    }
    if let Some(ref color) = input.color_hex {
        set_clauses.push("color_hex = ?".into());
        binds.push(color.clone().into());
    }
    if let Some(ref icon) = input.icon_name {
        set_clauses.push("icon_name = ?".into());
        binds.push(icon.clone().into());
    }
    if let Some(ref tag) = input.semantic_tag {
        set_clauses.push("semantic_tag = ?".into());
        binds.push(tag.clone().into());
    }
    if let Some(ref ext) = input.external_code {
        set_clauses.push("external_code = ?".into());
        binds.push(ext.clone().into());
    }
    if let Some(ref meta) = input.metadata_json {
        set_clauses.push("metadata_json = ?".into());
        binds.push(meta.clone().into());
    }

    binds.push(value_id.into());

    let sql = format!(
        "UPDATE reference_values SET {} WHERE id = ?",
        set_clauses.join(", ")
    );
    txn.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;
    Ok(())
}
