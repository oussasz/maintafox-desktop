//! Asset import pipeline — parse, stage, validate, and preview.
//!
//! Phase 2 - Sub-phase 02 - File 04 - Sprint S1.
//!
//! This module implements the staged import workflow:
//!
//!   1. `create_import_batch` — registers the source file identity (name + SHA-256)
//!      and creates the batch row in `uploaded` state.
//!   2. `parse_and_stage_csv` — reads CSV bytes, normalizes field names, and inserts
//!      staging rows with raw JSON and normalized identifiers.
//!   3. `validate_import_batch` — runs the governance engine against each staging row
//!      and populates per-row validation outcomes + batch summary counts.
//!   4. `get_import_preview` — returns the validation preview for UI display.
//!
//! The import pipeline never touches the `equipment` table directly during
//! parse/validate. The `apply_import_batch` function (Sprint S2) performs
//! controlled upserts from validated staging rows into the equipment registry.

use crate::assets::governance::{self, NormalizedImportRow, ValidationMessage};
use crate::assets::identity;
use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Batch summary returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportBatchSummary {
    pub id: i64,
    pub source_filename: String,
    pub source_sha256: String,
    pub initiated_by_id: Option<i64>,
    pub status: String,
    pub total_rows: i64,
    pub valid_rows: i64,
    pub warning_rows: i64,
    pub error_rows: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Single staging row for the validation preview table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreviewRow {
    pub id: i64,
    pub row_no: i64,
    pub normalized_asset_code: Option<String>,
    pub normalized_external_key: Option<String>,
    pub validation_status: String,
    pub validation_messages: Vec<ValidationMessage>,
    pub proposed_action: Option<String>,
    pub raw_json: String,
}

/// Full preview response: batch summary + staging rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub batch: ImportBatchSummary,
    pub rows: Vec<ImportPreviewRow>,
}

/// Import event record for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEvent {
    pub id: i64,
    pub batch_id: i64,
    pub event_type: String,
    pub summary_json: Option<String>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

// ─── Row mapping ──────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "import row decode failed for column '{column}': {e}"
    ))
}

fn map_batch_summary(row: &QueryResult) -> AppResult<ImportBatchSummary> {
    Ok(ImportBatchSummary {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        source_filename: row
            .try_get::<String>("", "source_filename")
            .map_err(|e| decode_err("source_filename", e))?,
        source_sha256: row
            .try_get::<String>("", "source_sha256")
            .map_err(|e| decode_err("source_sha256", e))?,
        initiated_by_id: row
            .try_get::<Option<i64>>("", "initiated_by_id")
            .map_err(|e| decode_err("initiated_by_id", e))?,
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
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_preview_row(row: &QueryResult) -> AppResult<ImportPreviewRow> {
    let messages_json: String = row
        .try_get("", "validation_messages_json")
        .map_err(|e| decode_err("validation_messages_json", e))?;
    let messages: Vec<ValidationMessage> =
        serde_json::from_str(&messages_json).unwrap_or_default();

    Ok(ImportPreviewRow {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        row_no: row
            .try_get::<i64>("", "row_no")
            .map_err(|e| decode_err("row_no", e))?,
        normalized_asset_code: row
            .try_get::<Option<String>>("", "normalized_asset_code")
            .map_err(|e| decode_err("normalized_asset_code", e))?,
        normalized_external_key: row
            .try_get::<Option<String>>("", "normalized_external_key")
            .map_err(|e| decode_err("normalized_external_key", e))?,
        validation_status: row
            .try_get::<String>("", "validation_status")
            .map_err(|e| decode_err("validation_status", e))?,
        validation_messages: messages,
        proposed_action: row
            .try_get::<Option<String>>("", "proposed_action")
            .map_err(|e| decode_err("proposed_action", e))?,
        raw_json: row
            .try_get::<String>("", "raw_json")
            .map_err(|e| decode_err("raw_json", e))?,
    })
}

fn map_import_event(row: &QueryResult) -> AppResult<ImportEvent> {
    Ok(ImportEvent {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        batch_id: row
            .try_get::<i64>("", "batch_id")
            .map_err(|e| decode_err("batch_id", e))?,
        event_type: row
            .try_get::<String>("", "event_type")
            .map_err(|e| decode_err("event_type", e))?,
        summary_json: row
            .try_get::<Option<String>>("", "summary_json")
            .map_err(|e| decode_err("summary_json", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

// ─── CSV field normalization ──────────────────────────────────────────────────

/// Known CSV header aliases mapped to our normalized field names.
/// Supports French (primary) and English headers.
fn normalize_header(header: &str) -> Option<&'static str> {
    let h = header.trim().to_lowercase();
    match h.as_str() {
        // Asset code
        "asset_code" | "code_equipement" | "code_equip" | "equipment_code" | "code" => {
            Some("asset_code")
        }
        // External key
        "external_key" | "cle_externe" | "external_id" | "id_externe" | "erp_id" => {
            Some("external_key")
        }
        // Name
        "asset_name" | "nom" | "name" | "designation" | "nom_equipement" => Some("asset_name"),
        // Class
        "class_code" | "classe" | "code_classe" | "equipment_class" => Some("class_code"),
        // Family
        "family_code" | "famille" | "code_famille" | "equipment_family" => Some("family_code"),
        // Criticality
        "criticality_code" | "criticite" | "code_criticite" | "criticality" => {
            Some("criticality_code")
        }
        // Status
        "status_code" | "statut" | "lifecycle_status" | "status" => Some("status_code"),
        // Org node
        "org_node_id" | "noeud_org" | "site" | "location" | "emplacement" => Some("org_node_id"),
        // Parent
        "parent_asset_code" | "code_parent" | "parent_code" | "parent" => {
            Some("parent_asset_code")
        }
        // Manufacturer
        "manufacturer" | "fabricant" | "constructeur" => Some("manufacturer"),
        // Model
        "model" | "modele" => Some("model"),
        // Serial number
        "serial_number" | "numero_serie" | "no_serie" | "serial" => Some("serial_number"),
        // Maintainable boundary
        "maintainable_boundary" | "frontiere_maintenable" | "maintainable" => {
            Some("maintainable_boundary")
        }
        // Commissioned at
        "commissioned_at" | "date_mise_en_service" | "commissioning_date" => {
            Some("commissioned_at")
        }
        _ => None,
    }
}

/// Parse a single CSV record (HashMap of header→value) into a NormalizedImportRow.
fn normalize_record(
    fields: &std::collections::HashMap<String, String>,
) -> NormalizedImportRow {
    let get = |key: &str| -> Option<String> {
        fields.get(key).and_then(|v| {
            let t = v.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
    };

    NormalizedImportRow {
        asset_code: get("asset_code"),
        external_key: get("external_key"),
        asset_name: get("asset_name"),
        class_code: get("class_code"),
        family_code: get("family_code"),
        criticality_code: get("criticality_code"),
        status_code: get("status_code"),
        org_node_id: get("org_node_id").and_then(|v| v.parse::<i64>().ok()),
        parent_asset_code: get("parent_asset_code"),
        manufacturer: get("manufacturer"),
        model: get("model"),
        serial_number: get("serial_number"),
        maintainable_boundary: get("maintainable_boundary").map(|v| {
            matches!(v.to_lowercase().as_str(), "1" | "true" | "oui" | "yes")
        }),
        commissioned_at: get("commissioned_at"),
    }
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Create a new import batch in `uploaded` state.
///
/// Records the source file identity (filename + SHA-256 hash) and the
/// initiating user. Does NOT parse the file — that happens via
/// `parse_and_stage_csv`.
pub async fn create_import_batch(
    db: &DatabaseConnection,
    filename: &str,
    file_sha256: &str,
    actor_id: Option<i64>,
) -> AppResult<ImportBatchSummary> {
    let now = Utc::now().to_rfc3339();

    let txn = db.begin().await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_import_batches \
         (source_filename, source_sha256, initiated_by_id, status, \
          total_rows, valid_rows, warning_rows, error_rows, created_at, updated_at) \
         VALUES (?, ?, ?, 'uploaded', 0, 0, 0, 0, ?, ?)",
        [
            filename.into(),
            file_sha256.into(),
            actor_id.map(|id| sea_orm::Value::BigInt(Some(id))).unwrap_or(sea_orm::Value::BigInt(None)),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;

    let batch_id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to get batch id")))?;
    let batch_id: i64 = batch_id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

    // Record upload event
    record_import_event_txn(
        &txn,
        batch_id,
        "uploaded",
        Some(&serde_json::json!({
            "filename": filename,
            "sha256": file_sha256,
        }).to_string()),
        actor_id,
    )
    .await?;

    txn.commit().await?;

    get_batch_by_id(db, batch_id).await
}

/// Parse CSV content and insert staging rows for a batch.
///
/// The batch must be in `uploaded` state. After staging, the batch status
/// remains `uploaded` — call `validate_import_batch` to advance it.
///
/// # Arguments
/// - `csv_content` — raw CSV bytes (UTF-8 expected)
/// - `batch_id` — target batch
pub async fn parse_and_stage_csv(
    db: &DatabaseConnection,
    batch_id: i64,
    csv_content: &[u8],
) -> AppResult<ImportBatchSummary> {
    // Verify batch exists and is in correct state
    let batch = get_batch_by_id(db, batch_id).await?;
    if batch.status != "uploaded" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le lot d'import est en statut '{}'; seul le statut 'uploaded' permet le chargement.",
            batch.status
        )]));
    }

    // Parse CSV headers and normalize them
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_content);

    let raw_headers: Vec<String> = reader
        .headers()
        .map_err(|e| AppError::ValidationFailed(vec![format!(
            "Impossible de lire les en-tetes CSV: {e}"
        )]))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    // Map raw headers to normalized field names
    let header_map: Vec<Option<&'static str>> = raw_headers
        .iter()
        .map(|h| normalize_header(h))
        .collect();

    // Check that at least asset_code or external_key is in headers
    let has_identity = header_map.iter().any(|h| {
        matches!(h, Some("asset_code") | Some("external_key"))
    });
    if !has_identity {
        return Err(AppError::ValidationFailed(vec![
            "Le fichier CSV doit contenir au moins une colonne 'asset_code' ou 'external_key' (ou leurs equivalents).".into(),
        ]));
    }

    let txn = db.begin().await?;

    let mut row_no: i64 = 0;
    for result in reader.records() {
        let record = result.map_err(|e| {
            AppError::ValidationFailed(vec![format!("Erreur CSV ligne {}: {e}", row_no + 2)])
        })?;
        row_no += 1;

        // Build normalized field map
        let mut fields = std::collections::HashMap::new();
        for (i, value) in record.iter().enumerate() {
            if let Some(Some(field_name)) = header_map.get(i) {
                fields.insert(field_name.to_string(), value.to_string());
            }
        }

        let normalized = normalize_record(&fields);
        let raw_json = serde_json::to_string(&fields)?;
        let norm_code = normalized.asset_code.clone();
        let norm_ext = normalized.external_key.clone();

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO asset_import_staging \
             (batch_id, row_no, raw_json, normalized_asset_code, normalized_external_key, \
              validation_status, validation_messages_json, proposed_action) \
             VALUES (?, ?, ?, ?, ?, 'pending', '[]', NULL)",
            [
                batch_id.into(),
                row_no.into(),
                raw_json.into(),
                norm_code.map(|c| sea_orm::Value::String(Some(Box::new(c)))).unwrap_or(sea_orm::Value::String(None)),
                norm_ext.map(|c| sea_orm::Value::String(Some(Box::new(c)))).unwrap_or(sea_orm::Value::String(None)),
            ],
        ))
        .await?;
    }

    // Update batch total_rows
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE asset_import_batches SET total_rows = ?, updated_at = ? WHERE id = ?",
        [row_no.into(), Utc::now().to_rfc3339().into(), batch_id.into()],
    ))
    .await?;

    txn.commit().await?;

    get_batch_by_id(db, batch_id).await
}

/// Run governance validation on all staging rows of a batch.
///
/// The batch must be in `uploaded` state. After validation, the batch status
/// advances to `validated` and summary counts are populated.
pub async fn validate_import_batch(
    db: &DatabaseConnection,
    batch_id: i64,
    actor_id: Option<i64>,
) -> AppResult<ImportBatchSummary> {
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

    // Load all staging rows for this batch
    let staging_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_no, raw_json, normalized_asset_code, normalized_external_key \
             FROM asset_import_staging \
             WHERE batch_id = ? ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

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

        // Deserialize raw_json back to field map and normalize
        let fields: std::collections::HashMap<String, String> =
            serde_json::from_str(&raw_json).unwrap_or_default();
        let normalized = normalize_record(&fields);

        // Run governance validation
        let outcome = governance::validate_import_row(&txn, &normalized, batch_id, row_no).await?;

        let messages_json = serde_json::to_string(&outcome.messages)?;

        // Update staging row with validation results
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE asset_import_staging \
             SET validation_status = ?, validation_messages_json = ?, \
                 proposed_action = ?, \
                 normalized_asset_code = ?, normalized_external_key = ? \
             WHERE id = ?",
            [
                outcome.status.clone().into(),
                messages_json.into(),
                outcome.proposed_action.clone()
                    .map(|a| sea_orm::Value::String(Some(Box::new(a))))
                    .unwrap_or(sea_orm::Value::String(None)),
                normalized.asset_code.clone()
                    .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                    .unwrap_or(sea_orm::Value::String(None)),
                normalized.external_key.clone()
                    .map(|c| sea_orm::Value::String(Some(Box::new(c))))
                    .unwrap_or(sea_orm::Value::String(None)),
                staging_id.into(),
            ],
        ))
        .await?;

        match outcome.status.as_str() {
            "valid" => valid_count += 1,
            "warning" => warning_count += 1,
            _ => error_count += 1,
        }
    }

    let now = Utc::now().to_rfc3339();

    // Update batch with summary counts and advance to validated
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE asset_import_batches \
         SET status = 'validated', valid_rows = ?, warning_rows = ?, error_rows = ?, \
             updated_at = ? \
         WHERE id = ?",
        [
            valid_count.into(),
            warning_count.into(),
            error_count.into(),
            now.clone().into(),
            batch_id.into(),
        ],
    ))
    .await?;

    // Record validation event with summary
    record_import_event_txn(
        &txn,
        batch_id,
        "validated",
        Some(
            &serde_json::json!({
                "total_rows": batch.total_rows,
                "valid_rows": valid_count,
                "warning_rows": warning_count,
                "error_rows": error_count,
            })
            .to_string(),
        ),
        actor_id,
    )
    .await?;

    txn.commit().await?;

    get_batch_by_id(db, batch_id).await
}

/// Get the full validation preview for a batch.
///
/// Returns the batch summary and all staging rows with their validation
/// outcomes, ordered by row number.
pub async fn get_import_preview(
    db: &DatabaseConnection,
    batch_id: i64,
) -> AppResult<ImportPreview> {
    let batch = get_batch_by_id(db, batch_id).await?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_no, raw_json, normalized_asset_code, normalized_external_key, \
                    validation_status, validation_messages_json, proposed_action \
             FROM asset_import_staging \
             WHERE batch_id = ? ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

    let preview_rows: Vec<ImportPreviewRow> = rows.iter().map(map_preview_row).collect::<Result<_, _>>()?;

    Ok(ImportPreview {
        batch,
        rows: preview_rows,
    })
}

/// List all import batches, optionally filtered by status.
pub async fn list_import_batches(
    db: &DatabaseConnection,
    status_filter: Option<String>,
    limit: Option<u64>,
) -> AppResult<Vec<ImportBatchSummary>> {
    let row_limit = limit.unwrap_or(50).min(200);

    let (sql, binds): (String, Vec<sea_orm::Value>) = if let Some(ref status) = status_filter {
        (
            format!(
                "SELECT * FROM asset_import_batches WHERE status = ? \
                 ORDER BY created_at DESC LIMIT {row_limit}"
            ),
            vec![status.clone().into()],
        )
    } else {
        (
            format!(
                "SELECT * FROM asset_import_batches \
                 ORDER BY created_at DESC LIMIT {row_limit}"
            ),
            vec![],
        )
    };

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    rows.iter().map(map_batch_summary).collect()
}

/// List import events for a batch (audit trail).
pub async fn list_import_events(
    db: &DatabaseConnection,
    batch_id: i64,
) -> AppResult<Vec<ImportEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, batch_id, event_type, summary_json, created_by_id, created_at \
             FROM asset_import_events \
             WHERE batch_id = ? ORDER BY created_at ASC",
            [batch_id.into()],
        ))
        .await?;

    rows.iter().map(map_import_event).collect()
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Fetch a single batch by id.
pub(crate) async fn get_batch_by_id(
    db: &impl ConnectionTrait,
    batch_id: i64,
) -> AppResult<ImportBatchSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM asset_import_batches WHERE id = ?",
            [batch_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "asset_import_batch".into(),
            id: batch_id.to_string(),
        })?;
    map_batch_summary(&row)
}

/// Record an import event within an existing transaction.
pub(crate) async fn record_import_event_txn(
    txn: &impl ConnectionTrait,
    batch_id: i64,
    event_type: &str,
    summary_json: Option<&str>,
    actor_id: Option<i64>,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_import_events \
         (batch_id, event_type, summary_json, created_by_id, created_at) \
         VALUES (?, ?, ?, ?, ?)",
        [
            batch_id.into(),
            event_type.into(),
            summary_json
                .map(|s| sea_orm::Value::String(Some(Box::new(s.to_string()))))
                .unwrap_or(sea_orm::Value::String(None)),
            actor_id
                .map(|id| sea_orm::Value::BigInt(Some(id)))
                .unwrap_or(sea_orm::Value::BigInt(None)),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

// ─── Sprint S2: Apply engine ──────────────────────────────────────────────────

/// Policy controlling which staging rows are eligible for apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPolicy {
    /// When true, rows with `validation_status = 'warning'` are applied
    /// alongside `valid` rows. Error rows are always skipped.
    pub include_warnings: bool,
    /// External system code stored in `asset_external_ids` when an
    /// `external_key` is present on an imported row. Defaults to `"import"`.
    pub external_system_code: Option<String>,
}

/// Summary returned after applying a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub batch: ImportBatchSummary,
    pub created: i64,
    pub updated: i64,
    pub skipped: i64,
    pub errored: i64,
}

/// Apply validated staging rows to the equipment registry.
///
/// The batch must be in `validated` state. After a successful apply the batch
/// transitions to `applied`.
///
/// # Idempotency
///
/// If the batch is already in `applied` state the function returns an error
/// rather than silently re-applying, preventing accidental duplicates.
///
/// # Row processing
///
/// For each eligible staging row:
///   1. Resolve the existing equipment row by `external_key` (via
///      `asset_external_ids`) then fall back to `asset_code`.
///   2. If found → **update** mutable identity fields.
///   3. If not found → **insert** a new equipment row and, when an
///      `external_key` is present, link it in `asset_external_ids`.
///   4. Row-level errors are caught and counted; they do not abort the
///      entire batch.
pub async fn apply_import_batch(
    db: &DatabaseConnection,
    batch_id: i64,
    policy: &ApplyPolicy,
    actor_id: Option<i64>,
) -> AppResult<ApplyResult> {
    let batch = get_batch_by_id(db, batch_id).await?;

    // ── Idempotency guard ────────────────────────────────────────────────
    if batch.status == "applied" {
        return Err(AppError::ValidationFailed(vec![
            "Ce lot d'import a déjà été appliqué.".into(),
        ]));
    }
    if batch.status != "validated" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le lot d'import est en statut '{}'; seul le statut 'validated' permet l'application.",
            batch.status
        )]));
    }

    // ── Load eligible staging rows ───────────────────────────────────────
    let status_filter = if policy.include_warnings {
        "('valid','warning')"
    } else {
        "('valid')"
    };
    let staging_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT id, row_no, raw_json, normalized_asset_code, normalized_external_key, \
                        validation_status, proposed_action \
                 FROM asset_import_staging \
                 WHERE batch_id = {batch_id} AND validation_status IN {status_filter} \
                 ORDER BY row_no ASC"
            ),
        ))
        .await?;

    let ext_system = policy
        .external_system_code
        .as_deref()
        .unwrap_or("import");

    let txn = db.begin().await?;

    let mut created: i64 = 0;
    let mut updated: i64 = 0;
    let mut skipped: i64 = 0;
    let mut errored: i64 = 0;

    for staging_row in &staging_rows {
        let row_no: i64 = staging_row
            .try_get("", "row_no")
            .map_err(|e| decode_err("row_no", e))?;
        let raw_json: String = staging_row
            .try_get("", "raw_json")
            .map_err(|e| decode_err("raw_json", e))?;
        let norm_ext_key: Option<String> = staging_row
            .try_get("", "normalized_external_key")
            .map_err(|e| decode_err("normalized_external_key", e))?;
        let norm_asset_code: Option<String> = staging_row
            .try_get("", "normalized_asset_code")
            .map_err(|e| decode_err("normalized_asset_code", e))?;

        // Re-normalize from raw_json
        let fields: std::collections::HashMap<String, String> =
            serde_json::from_str(&raw_json).unwrap_or_default();
        let normalized = normalize_record(&fields);

        // Attempt to apply this single row; row-level errors are counted, not propagated.
        match apply_single_row(
            &txn,
            &normalized,
            norm_ext_key.as_deref(),
            norm_asset_code.as_deref(),
            ext_system,
        )
        .await
        {
            Ok(RowOutcome::Created) => created += 1,
            Ok(RowOutcome::Updated) => updated += 1,
            Err(e) => {
                tracing::warn!(row_no, error = %e, "import apply row error");
                errored += 1;
            }
        }
    }

    // Also count rows that were already ineligible (error status)
    let total_ineligible = batch.total_rows - staging_rows.len() as i64;
    skipped += total_ineligible;

    let now = Utc::now().to_rfc3339();

    // ── Transition batch to applied ──────────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE asset_import_batches SET status = 'applied', updated_at = ? WHERE id = ?",
        [now.into(), batch_id.into()],
    ))
    .await?;

    // ── Record audit event ───────────────────────────────────────────────
    record_import_event_txn(
        &txn,
        batch_id,
        "applied",
        Some(
            &serde_json::json!({
                "created": created,
                "updated": updated,
                "skipped": skipped,
                "errored": errored,
                "include_warnings": policy.include_warnings,
            })
            .to_string(),
        ),
        actor_id,
    )
    .await?;

    txn.commit().await?;

    let batch = get_batch_by_id(db, batch_id).await?;
    Ok(ApplyResult {
        batch,
        created,
        updated,
        skipped,
        errored,
    })
}

// ─── Row-level apply helpers ──────────────────────────────────────────────────

enum RowOutcome {
    Created,
    Updated,
}

/// Resolve an existing equipment id by external key, then by asset code.
async fn resolve_existing_equipment(
    txn: &impl ConnectionTrait,
    ext_key: Option<&str>,
    asset_code: Option<&str>,
) -> AppResult<Option<i64>> {
    // 1. Try external_key via asset_external_ids
    if let Some(ek) = ext_key {
        let row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT aei.asset_id FROM asset_external_ids aei \
                 INNER JOIN equipment e ON e.id = aei.asset_id \
                 WHERE aei.external_id = ? AND e.deleted_at IS NULL \
                 LIMIT 1",
                [ek.into()],
            ))
            .await?;
        if let Some(r) = row {
            let id: i64 = r.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?;
            return Ok(Some(id));
        }
    }

    // 2. Fallback to asset_id_code
    if let Some(ac) = asset_code {
        let row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM equipment WHERE asset_id_code = ? AND deleted_at IS NULL",
                [ac.into()],
            ))
            .await?;
        if let Some(r) = row {
            let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
            return Ok(Some(id));
        }
    }

    Ok(None)
}

/// Apply a single staging row: resolve references, then create or update.
async fn apply_single_row(
    txn: &impl ConnectionTrait,
    normalized: &NormalizedImportRow,
    ext_key: Option<&str>,
    asset_code: Option<&str>,
    ext_system: &str,
) -> AppResult<RowOutcome> {
    // ── Resolve classification references ────────────────────────────────
    let class_code = normalized
        .class_code
        .as_deref()
        .ok_or_else(|| AppError::ValidationFailed(vec!["class_code requis.".into()]))?;
    let (class_id, class_parent_id) = identity::resolve_class_code(txn, class_code).await?;

    if let Some(ref fc) = normalized.family_code {
        identity::validate_family_code(txn, fc, class_id, class_parent_id).await?;
    }

    let criticality_code = normalized
        .criticality_code
        .as_deref()
        .ok_or_else(|| AppError::ValidationFailed(vec!["criticality_code requis.".into()]))?;
    let criticality_value_id =
        identity::resolve_criticality_code(txn, criticality_code).await?;

    let status_code = normalized.status_code.as_deref().unwrap_or("IN_SERVICE");
    identity::validate_status_code(txn, status_code).await?;

    if let Some(org_id) = normalized.org_node_id {
        identity::assert_org_node_active(txn, org_id).await?;
    }

    let existing_id = resolve_existing_equipment(txn, ext_key, asset_code).await?;

    match existing_id {
        Some(equip_id) => {
            update_equipment_from_import(txn, equip_id, normalized, class_id, criticality_value_id, status_code).await?;
            Ok(RowOutcome::Updated)
        }
        None => {
            // Validate and ensure uniqueness of asset_code for new rows
            let code = asset_code
                .ok_or_else(|| AppError::ValidationFailed(vec!["asset_code requis pour la création.".into()]))?;
            let validated_code = identity::validate_asset_code(code)?;
            identity::assert_asset_code_unique(txn, &validated_code, None).await?;

            let equip_id = insert_equipment_from_import(
                txn,
                &validated_code,
                normalized,
                class_id,
                criticality_value_id,
                status_code,
            )
            .await?;

            // Link external key if provided
            if let Some(ek) = ext_key {
                let now = Utc::now().to_rfc3339();
                txn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO asset_external_ids \
                     (asset_id, system_code, external_id, is_primary, valid_from, created_at) \
                     VALUES (?, ?, ?, 1, ?, ?)",
                    [
                        equip_id.into(),
                        ext_system.into(),
                        ek.into(),
                        now.clone().into(),
                        now.into(),
                    ],
                ))
                .await?;
            }

            Ok(RowOutcome::Created)
        }
    }
}

/// Insert a new equipment row from an import staging record.
async fn insert_equipment_from_import(
    txn: &impl ConnectionTrait,
    asset_code: &str,
    n: &NormalizedImportRow,
    class_id: i64,
    criticality_value_id: i64,
    status_code: &str,
) -> AppResult<i64> {
    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    let decommissioned_at: Option<String> = if status_code == "DECOMMISSIONED" {
        Some(now.clone())
    } else {
        None
    };

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO equipment
          (sync_id, asset_id_code, name, class_id,
           lifecycle_status, criticality_value_id,
           installed_at_node_id, manufacturer, model, serial_number,
           maintainable_boundary, commissioning_date, decommissioned_at,
           created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.clone().into(),
            asset_code.into(),
            n.asset_name.clone().unwrap_or_default().into(),
            class_id.into(),
            status_code.into(),
            criticality_value_id.into(),
            n.org_node_id
                .map(|id| sea_orm::Value::BigInt(Some(id)))
                .unwrap_or(sea_orm::Value::BigInt(None)),
            n.manufacturer.clone().into(),
            n.model.clone().into(),
            n.serial_number.clone().into(),
            i64::from(n.maintainable_boundary.unwrap_or(true)).into(),
            n.commissioned_at.clone().into(),
            decommissioned_at.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let id_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("import insert succeeded but row not found"))
        })?;
    id_row
        .try_get::<i64>("", "id")
        .map_err(|e| decode_err("id", e))
}

/// Update mutable identity fields on an existing equipment row.
async fn update_equipment_from_import(
    txn: &impl ConnectionTrait,
    equip_id: i64,
    n: &NormalizedImportRow,
    class_id: i64,
    criticality_value_id: i64,
    status_code: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();

    let decommissioned_at: Option<String> = if status_code == "DECOMMISSIONED" {
        Some(now.clone())
    } else {
        None
    };

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE equipment SET
            name = COALESCE(?, name),
            class_id = ?,
            lifecycle_status = ?,
            criticality_value_id = ?,
            installed_at_node_id = COALESCE(?, installed_at_node_id),
            manufacturer = COALESCE(?, manufacturer),
            model = COALESCE(?, model),
            serial_number = COALESCE(?, serial_number),
            maintainable_boundary = COALESCE(?, maintainable_boundary),
            commissioning_date = COALESCE(?, commissioning_date),
            decommissioned_at = ?,
            updated_at = ?,
            row_version = row_version + 1
          WHERE id = ? AND deleted_at IS NULL",
        [
            n.asset_name.clone().map(|s| sea_orm::Value::String(Some(Box::new(s)))).unwrap_or(sea_orm::Value::String(None)),
            class_id.into(),
            status_code.into(),
            criticality_value_id.into(),
            n.org_node_id.map(|id| sea_orm::Value::BigInt(Some(id))).unwrap_or(sea_orm::Value::BigInt(None)),
            n.manufacturer.clone().map(|s| sea_orm::Value::String(Some(Box::new(s)))).unwrap_or(sea_orm::Value::String(None)),
            n.model.clone().map(|s| sea_orm::Value::String(Some(Box::new(s)))).unwrap_or(sea_orm::Value::String(None)),
            n.serial_number.clone().map(|s| sea_orm::Value::String(Some(Box::new(s)))).unwrap_or(sea_orm::Value::String(None)),
            n.maintainable_boundary.map(|b| sea_orm::Value::BigInt(Some(i64::from(b)))).unwrap_or(sea_orm::Value::BigInt(None)),
            n.commissioned_at.clone().map(|s| sea_orm::Value::String(Some(Box::new(s)))).unwrap_or(sea_orm::Value::String(None)),
            decommissioned_at.map(|s| sea_orm::Value::String(Some(Box::new(s)))).unwrap_or(sea_orm::Value::String(None)),
            now.into(),
            equip_id.into(),
        ],
    ))
    .await?;

    Ok(())
}
