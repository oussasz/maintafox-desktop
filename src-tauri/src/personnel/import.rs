//! Personnel bulk import engine (PRD §6.6).
//!
//! Supports CSV/XLSX parsing, preview/validation, and transactional apply.

use std::collections::HashMap;
use std::io::Cursor;

use calamine::{Reader, Xlsx};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelImportBatchSummary {
    pub id: i64,
    pub source_filename: String,
    pub source_sha256: String,
    pub source_kind: String,
    pub mode: String,
    pub status: String,
    pub total_rows: i64,
    pub valid_rows: i64,
    pub warning_rows: i64,
    pub error_rows: i64,
    pub initiated_by_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelImportMessage {
    pub category: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelImportPreviewRow {
    pub id: i64,
    pub row_no: i64,
    pub employee_code: Option<String>,
    pub hr_external_id: Option<String>,
    pub target_personnel_id: Option<i64>,
    pub target_row_version: Option<i64>,
    pub validation_status: String,
    pub messages: Vec<PersonnelImportMessage>,
    pub proposed_action: Option<String>,
    pub raw_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelImportPreview {
    pub batch: PersonnelImportBatchSummary,
    pub rows: Vec<PersonnelImportPreviewRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelImportApplyResult {
    pub batch: PersonnelImportBatchSummary,
    pub created: i64,
    pub updated: i64,
    pub skipped: i64,
    pub protected_ignored: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelImportCreateInput {
    pub filename: String,
    pub source_sha256: String,
    pub source_kind: String,
    pub mode: String,
    pub file_content: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NormalizedPersonnelRow {
    employee_code: Option<String>,
    hr_external_id: Option<String>,
    full_name: Option<String>,
    employment_type: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    availability_status: Option<String>,
    notes: Option<String>,
    hire_date: Option<String>,
    termination_date: Option<String>,
    position_code: Option<String>,
    entity_code: Option<String>,
    team_code: Option<String>,
    supervisor_employee_code: Option<String>,
    external_company_name: Option<String>,
}

#[derive(Debug, Clone)]
struct ValidationOutcome {
    status: String,
    proposed_action: Option<String>,
    target_personnel_id: Option<i64>,
    target_row_version: Option<i64>,
    normalized_json: String,
    messages: Vec<PersonnelImportMessage>,
}

pub async fn create_import_batch(
    db: &DatabaseConnection,
    input: PersonnelImportCreateInput,
    actor_id: Option<i64>,
) -> AppResult<PersonnelImportBatchSummary> {
    let source_kind = input.source_kind.trim().to_lowercase();
    if source_kind != "csv" && source_kind != "xlsx" {
        return Err(AppError::ValidationFailed(vec![
            "source_kind must be csv or xlsx.".to_string(),
        ]));
    }

    let mode = normalize_mode(&input.mode)?;
    let rows = parse_file_rows(&source_kind, &input.file_content)?;
    if rows.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Import file does not contain any data rows.".to_string(),
        ]));
    }

    let now = Utc::now().to_rfc3339();
    let txn = db.begin().await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_import_batches (
            source_filename, source_sha256, source_kind, mode, status,
            total_rows, valid_rows, warning_rows, error_rows, initiated_by_id, created_at, updated_at
         ) VALUES (?, ?, ?, ?, 'uploaded', 0, 0, 0, 0, ?, ?, ?)",
        [
            input.filename.into(),
            input.source_sha256.into(),
            source_kind.clone().into(),
            mode.clone().into(),
            actor_id.into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;

    let batch_id: i64 = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to read created import batch id.")))?
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to decode batch id: {e}")))?;

    for (index, row_fields) in rows.iter().enumerate() {
        let raw_json = serde_json::to_string(row_fields)?;
        let normalized = normalize_row(row_fields);
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO personnel_import_rows (
                batch_id, row_no, raw_json, employee_code, hr_external_id, validation_status,
                messages_json, proposed_action, target_personnel_id, target_row_version, normalized_json
             ) VALUES (?, ?, ?, ?, ?, 'pending', '[]', NULL, NULL, NULL, '{}')",
            [
                batch_id.into(),
                ((index + 1) as i64).into(),
                raw_json.into(),
                normalized.employee_code.into(),
                normalized.hr_external_id.into(),
            ],
        ))
        .await?;
    }

    validate_staged_rows(&txn, batch_id, &mode).await?;
    txn.commit().await?;
    get_batch_summary(db, batch_id).await
}

pub async fn get_import_preview(
    db: &DatabaseConnection,
    batch_id: i64,
) -> AppResult<PersonnelImportPreview> {
    let batch = get_batch_summary(db, batch_id).await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                id, row_no, employee_code, hr_external_id, target_personnel_id, target_row_version,
                validation_status, messages_json, proposed_action, raw_json
             FROM personnel_import_rows
             WHERE batch_id = ?
             ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let messages_json: String = row
            .try_get("", "messages_json")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("messages_json decode failed: {e}")))?;
        out.push(PersonnelImportPreviewRow {
            id: decode_col(&row, "id")?,
            row_no: decode_col(&row, "row_no")?,
            employee_code: decode_col(&row, "employee_code")?,
            hr_external_id: decode_col(&row, "hr_external_id")?,
            target_personnel_id: decode_col(&row, "target_personnel_id")?,
            target_row_version: decode_col(&row, "target_row_version")?,
            validation_status: decode_col(&row, "validation_status")?,
            messages: serde_json::from_str(&messages_json).unwrap_or_default(),
            proposed_action: decode_col(&row, "proposed_action")?,
            raw_json: decode_col(&row, "raw_json")?,
        });
    }

    Ok(PersonnelImportPreview { batch, rows: out })
}

pub async fn apply_import_batch(
    db: &DatabaseConnection,
    batch_id: i64,
    actor_id: Option<i64>,
) -> AppResult<PersonnelImportApplyResult> {
    let batch = get_batch_summary(db, batch_id).await?;
    if batch.status != "validated" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Batch status must be validated before apply (current={}).",
            batch.status
        )]));
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                id, row_no, normalized_json, proposed_action, validation_status,
                target_personnel_id, target_row_version, employee_code
             FROM personnel_import_rows
             WHERE batch_id = ?
             ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

    let mut created = 0_i64;
    let mut updated = 0_i64;
    let mut skipped = 0_i64;
    let mut protected_ignored = 0_i64;
    let now = Utc::now().to_rfc3339();

    let txn = db.begin().await?;
    for row in rows {
        let validation_status: String = decode_col(&row, "validation_status")?;
        let proposed_action: Option<String> = decode_col(&row, "proposed_action")?;
        if validation_status == "error" {
            skipped += 1;
            continue;
        }
        if proposed_action.as_deref() == Some("skip") {
            skipped += 1;
            continue;
        }

        let normalized_json: String = decode_col(&row, "normalized_json")?;
        let normalized: NormalizedPersonnelRow = serde_json::from_str(&normalized_json)?;
        let mapped = resolve_mapped_fields(&txn, &normalized).await?;
        protected_ignored += i64::from(normalized.termination_date.is_some());

        let target_personnel_id: Option<i64> = decode_col(&row, "target_personnel_id")?;
        let target_row_version: Option<i64> = decode_col(&row, "target_row_version")?;
        match proposed_action.as_deref() {
            Some("create") => {
                let generated_code = if let Some(code) = normalized.employee_code.clone() {
                    code
                } else {
                    super::domain::generate_personnel_code(&txn).await?
                };
                let full_name = normalized.full_name.clone().ok_or_else(|| {
                    AppError::ValidationFailed(vec![
                        "full_name is required when creating a personnel record.".to_string(),
                    ])
                })?;
                let employment_type = normalized
                    .employment_type
                    .clone()
                    .unwrap_or_else(|| "employee".to_string());
                txn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO personnel (
                        employee_code, full_name, employment_type, position_id, primary_entity_id, primary_team_id,
                        supervisor_id, availability_status, hire_date, email, phone, hr_external_id, external_company_id,
                        notes, row_version, created_at, updated_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                    [
                        generated_code.into(),
                        full_name.into(),
                        employment_type.into(),
                        mapped.position_id.into(),
                        mapped.entity_id.into(),
                        mapped.team_id.into(),
                        mapped.supervisor_id.into(),
                        normalized.availability_status.clone().unwrap_or_else(|| "available".to_string()).into(),
                        normalized.hire_date.into(),
                        normalized.email.into(),
                        normalized.phone.into(),
                        normalized.hr_external_id.into(),
                        mapped.external_company_id.into(),
                        normalized.notes.into(),
                        now.clone().into(),
                        now.clone().into(),
                    ],
                ))
                .await?;
                created += 1;
            }
            Some("update") => {
                let personnel_id = target_personnel_id.ok_or_else(|| {
                    AppError::ValidationFailed(vec![
                        "Missing target_personnel_id for update action.".to_string(),
                    ])
                })?;
                let expected_version = target_row_version.ok_or_else(|| {
                    AppError::ValidationFailed(vec![
                        "Missing target_row_version for update action.".to_string(),
                    ])
                })?;

                let result = txn
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "UPDATE personnel
                         SET full_name = COALESCE(?, full_name),
                             employment_type = COALESCE(?, employment_type),
                             email = COALESCE(?, email),
                             phone = COALESCE(?, phone),
                             availability_status = COALESCE(?, availability_status),
                             notes = COALESCE(?, notes),
                             position_id = COALESCE(?, position_id),
                             primary_entity_id = COALESCE(?, primary_entity_id),
                             primary_team_id = COALESCE(?, primary_team_id),
                             supervisor_id = COALESCE(?, supervisor_id),
                             external_company_id = COALESCE(?, external_company_id),
                             row_version = row_version + 1,
                             updated_at = ?
                         WHERE id = ? AND row_version = ?",
                        [
                            normalized.full_name.into(),
                            normalized.employment_type.into(),
                            normalized.email.into(),
                            normalized.phone.into(),
                            normalized.availability_status.into(),
                            normalized.notes.into(),
                            mapped.position_id.into(),
                            mapped.entity_id.into(),
                            mapped.team_id.into(),
                            mapped.supervisor_id.into(),
                            mapped.external_company_id.into(),
                            now.clone().into(),
                            personnel_id.into(),
                            expected_version.into(),
                        ],
                    ))
                    .await?;
                if result.rows_affected() == 0 {
                    return Err(AppError::ValidationFailed(vec![format!(
                        "Concurrency conflict on personnel id={personnel_id}. Import was prepared with stale row_version."
                    )]));
                }
                updated += 1;
            }
            _ => {
                skipped += 1;
            }
        }
    }

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel_import_batches
         SET status = 'applied', updated_at = ?
         WHERE id = ?",
        [now.clone().into(), batch_id.into()],
    ))
    .await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO activity_events (
            event_class, event_code, source_module, source_record_type, source_record_id,
            actor_id, happened_at, severity, summary_json, visibility_scope
         ) VALUES (
            'operational', 'personnel.import.applied', 'personnel', 'personnel_import_batch', ?,
            ?, ?, 'info', ?, 'global'
         )",
        [
            batch_id.to_string().into(),
            actor_id.into(),
            now.clone().into(),
            serde_json::json!({
                "created": created,
                "updated": updated,
                "skipped": skipped,
                "protected_ignored": protected_ignored
            })
            .to_string()
            .into(),
        ],
    ))
    .await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO audit_events (
            action_code, target_type, target_id, actor_id, auth_context, result,
            retention_class, details_json, happened_at
         ) VALUES (
            'personnel.import.apply', 'personnel_import_batch', ?, ?, 'step_up', 'success',
            'standard', ?, ?
         )",
        [
            batch_id.to_string().into(),
            actor_id.into(),
            serde_json::json!({
                "created": created,
                "updated": updated,
                "skipped": skipped,
                "protected_ignored": protected_ignored
            })
            .to_string()
            .into(),
            now.into(),
        ],
    ))
    .await?;

    txn.commit().await?;
    let batch = get_batch_summary(db, batch_id).await?;
    Ok(PersonnelImportApplyResult {
        batch,
        created,
        updated,
        skipped,
        protected_ignored,
    })
}

async fn validate_staged_rows(txn: &impl ConnectionTrait, batch_id: i64, mode: &str) -> AppResult<()> {
    let rows = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_no, raw_json FROM personnel_import_rows WHERE batch_id = ? ORDER BY row_no ASC",
            [batch_id.into()],
        ))
        .await?;

    let mut valid_rows = 0_i64;
    let mut warning_rows = 0_i64;
    let mut error_rows = 0_i64;

    for row in rows {
        let id: i64 = decode_col(&row, "id")?;
        let raw_json: String = decode_col(&row, "raw_json")?;
        let fields: HashMap<String, String> = serde_json::from_str(&raw_json)?;
        let normalized = normalize_row(&fields);
        let outcome = validate_row(txn, &normalized, mode).await?;
        let messages_json = serde_json::to_string(&outcome.messages)?;
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_import_rows
             SET employee_code = ?,
                 hr_external_id = ?,
                 target_personnel_id = ?,
                 target_row_version = ?,
                 validation_status = ?,
                 messages_json = ?,
                 proposed_action = ?,
                 normalized_json = ?
             WHERE id = ?",
            [
                normalized.employee_code.into(),
                normalized.hr_external_id.into(),
                outcome.target_personnel_id.into(),
                outcome.target_row_version.into(),
                outcome.status.clone().into(),
                messages_json.into(),
                outcome.proposed_action.into(),
                outcome.normalized_json.into(),
                id.into(),
            ],
        ))
        .await?;

        match outcome.status.as_str() {
            "valid" => valid_rows += 1,
            "warning" => warning_rows += 1,
            _ => error_rows += 1,
        }
    }

    let now = Utc::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel_import_batches
         SET status = 'validated', total_rows = ?, valid_rows = ?, warning_rows = ?, error_rows = ?, updated_at = ?
         WHERE id = ?",
        [
            (valid_rows + warning_rows + error_rows).into(),
            valid_rows.into(),
            warning_rows.into(),
            error_rows.into(),
            now.into(),
            batch_id.into(),
        ],
    ))
    .await?;

    Ok(())
}

async fn validate_row(
    db: &impl ConnectionTrait,
    normalized: &NormalizedPersonnelRow,
    mode: &str,
) -> AppResult<ValidationOutcome> {
    let mut messages = Vec::<PersonnelImportMessage>::new();
    let mut severity = "valid".to_string();

    let existing = resolve_existing_personnel(db, normalized).await?;
    let proposed_action = if existing.is_some() { "update" } else { "create" };

    if mode == "create_only" && existing.is_some() {
        push_message(
            &mut messages,
            "CreateOnly",
            "warning",
            "Row targets an existing personnel record; it will be skipped in create-only mode.",
        );
    }

    if proposed_action == "create" && normalized.full_name.is_none() {
        push_message(
            &mut messages,
            "RequiredField",
            "error",
            "full_name is required when creating a personnel record.",
        );
    }

    if let Some(kind) = normalized.employment_type.as_deref() {
        let allowed = ["employee", "contractor", "temp", "vendor"];
        if !allowed.contains(&kind) {
            push_message(
                &mut messages,
                "FieldRule.CreateAndUpdate",
                "error",
                "employment_type must be one of employee/contractor/temp/vendor.",
            );
        }
    }

    if normalized.hire_date.is_some() && existing.is_some() {
        push_message(
            &mut messages,
            "FieldRule.CreateOnly",
            "warning",
            "hire_date is CreateOnly and will not overwrite existing rows.",
        );
    }
    if normalized.termination_date.is_some() {
        push_message(
            &mut messages,
            "FieldRule.Protected",
            "warning",
            "termination_date is protected and ignored by import apply.",
        );
    }

    validate_mapped_fields(db, normalized, &mut messages).await?;

    if messages.iter().any(|m| m.severity == "error") {
        severity = "error".to_string();
    } else if messages.iter().any(|m| m.severity == "warning") {
        severity = "warning".to_string();
    }

    let mut action = Some(proposed_action.to_string());
    if mode == "create_only" && proposed_action == "update" {
        action = Some("skip".to_string());
    }

    Ok(ValidationOutcome {
        status: severity,
        proposed_action: action,
        target_personnel_id: existing.as_ref().map(|r| r.0),
        target_row_version: existing.as_ref().map(|r| r.1),
        normalized_json: serde_json::to_string(normalized)?,
        messages,
    })
}

async fn resolve_existing_personnel(
    db: &impl ConnectionTrait,
    normalized: &NormalizedPersonnelRow,
) -> AppResult<Option<(i64, i64)>> {
    if let Some(code) = normalized.employee_code.as_deref() {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, row_version FROM personnel WHERE employee_code = ? LIMIT 1",
                [code.into()],
            ))
            .await?;
        if let Some(row) = row {
            return Ok(Some((decode_col(&row, "id")?, decode_col(&row, "row_version")?)));
        }
    }
    if let Some(hr_external_id) = normalized.hr_external_id.as_deref() {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, row_version FROM personnel WHERE hr_external_id = ? LIMIT 1",
                [hr_external_id.into()],
            ))
            .await?;
        if let Some(row) = row {
            return Ok(Some((decode_col(&row, "id")?, decode_col(&row, "row_version")?)));
        }
    }
    Ok(None)
}

async fn validate_mapped_fields(
    db: &impl ConnectionTrait,
    normalized: &NormalizedPersonnelRow,
    messages: &mut Vec<PersonnelImportMessage>,
) -> AppResult<()> {
    if let Some(position_code) = normalized.position_code.as_deref() {
        let found = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM positions WHERE code = ? LIMIT 1",
                [position_code.into()],
            ))
            .await?;
        if found.is_none() {
            push_message(
                messages,
                "FieldRule.Mapped",
                "error",
                &format!("Unknown position_code '{position_code}'."),
            );
        }
    }
    if let Some(entity_code) = normalized.entity_code.as_deref() {
        let found = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes WHERE code = ? LIMIT 1",
                [entity_code.into()],
            ))
            .await?;
        if found.is_none() {
            push_message(
                messages,
                "FieldRule.Mapped",
                "error",
                &format!("Unknown entity_code '{entity_code}'."),
            );
        }
    }
    if let Some(team_code) = normalized.team_code.as_deref() {
        let found = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes WHERE code = ? LIMIT 1",
                [team_code.into()],
            ))
            .await?;
        if found.is_none() {
            push_message(
                messages,
                "FieldRule.Mapped",
                "error",
                &format!("Unknown team_code '{team_code}'."),
            );
        }
    }
    if let Some(supervisor_code) = normalized.supervisor_employee_code.as_deref() {
        let found = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM personnel WHERE employee_code = ? LIMIT 1",
                [supervisor_code.into()],
            ))
            .await?;
        if found.is_none() {
            push_message(
                messages,
                "FieldRule.Mapped",
                "error",
                &format!("Unknown supervisor_employee_code '{supervisor_code}'."),
            );
        }
    }
    if let Some(company_name) = normalized.external_company_name.as_deref() {
        let found = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM external_companies WHERE name = ? LIMIT 1",
                [company_name.into()],
            ))
            .await?;
        if found.is_none() {
            push_message(
                messages,
                "FieldRule.Mapped",
                "error",
                &format!("Unknown external_company_name '{company_name}'."),
            );
        }
    }
    Ok(())
}

struct MappedIds {
    position_id: Option<i64>,
    entity_id: Option<i64>,
    team_id: Option<i64>,
    supervisor_id: Option<i64>,
    external_company_id: Option<i64>,
}

async fn resolve_mapped_fields(
    db: &impl ConnectionTrait,
    normalized: &NormalizedPersonnelRow,
) -> AppResult<MappedIds> {
    Ok(MappedIds {
        position_id: map_code_to_id(db, "positions", "code", normalized.position_code.as_deref()).await?,
        entity_id: map_code_to_id(db, "org_nodes", "code", normalized.entity_code.as_deref()).await?,
        team_id: map_code_to_id(db, "org_nodes", "code", normalized.team_code.as_deref()).await?,
        supervisor_id: map_code_to_id(
            db,
            "personnel",
            "employee_code",
            normalized.supervisor_employee_code.as_deref(),
        )
        .await?,
        external_company_id: map_code_to_id(
            db,
            "external_companies",
            "name",
            normalized.external_company_name.as_deref(),
        )
        .await?,
    })
}

async fn map_code_to_id(
    db: &impl ConnectionTrait,
    table: &str,
    column: &str,
    value: Option<&str>,
) -> AppResult<Option<i64>> {
    let Some(value) = value else { return Ok(None) };
    let sql = format!("SELECT id FROM {table} WHERE {column} = ? LIMIT 1");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [value.into()],
        ))
        .await?;
    row.map(|r| decode_col::<i64>(&r, "id")).transpose()
}

fn parse_file_rows(source_kind: &str, file_content: &[u8]) -> AppResult<Vec<HashMap<String, String>>> {
    match source_kind {
        "csv" => parse_csv_rows(file_content),
        "xlsx" => parse_xlsx_rows(file_content),
        _ => Err(AppError::ValidationFailed(vec![
            "Unsupported source_kind.".to_string(),
        ])),
    }
}

fn parse_csv_rows(file_content: &[u8]) -> AppResult<Vec<HashMap<String, String>>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(file_content);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| AppError::ValidationFailed(vec![format!("Cannot read CSV headers: {e}")]))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record
            .map_err(|e| AppError::ValidationFailed(vec![format!("Cannot parse CSV row: {e}")]))?;
        let mut map = HashMap::new();
        for (index, value) in record.iter().enumerate() {
            if let Some(header) = headers.get(index) {
                let key = normalize_header_name(header);
                if let Some(key) = key {
                    map.insert(key, value.trim().to_string());
                }
            }
        }
        rows.push(map);
    }
    Ok(rows)
}

fn parse_xlsx_rows(file_content: &[u8]) -> AppResult<Vec<HashMap<String, String>>> {
    let cursor = Cursor::new(file_content.to_vec());
    let mut workbook = Xlsx::new(cursor)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Cannot parse XLSX file: {e}")]))?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| AppError::ValidationFailed(vec!["XLSX file has no worksheet.".to_string()]))?
        .map_err(|e| AppError::ValidationFailed(vec![format!("Cannot read XLSX worksheet: {e}")]))?;

    let mut rows_iter = range.rows();
    let Some(header_row) = rows_iter.next() else {
        return Ok(Vec::new());
    };
    let headers: Vec<String> = header_row.iter().map(ToString::to_string).collect();
    let mut out = Vec::new();
    for row in rows_iter {
        let mut map = HashMap::new();
        for (index, cell) in row.iter().enumerate() {
            if let Some(header) = headers.get(index) {
                if let Some(key) = normalize_header_name(header) {
                    map.insert(key, cell.to_string().trim().to_string());
                }
            }
        }
        out.push(map);
    }
    Ok(out)
}

fn normalize_mode(mode: &str) -> AppResult<String> {
    let normalized = mode.trim().to_lowercase();
    match normalized.as_str() {
        "create_and_update" | "createandupdate" => Ok("create_and_update".to_string()),
        "create_only" | "createonly" => Ok("create_only".to_string()),
        _ => Err(AppError::ValidationFailed(vec![
            "mode must be CreateAndUpdate or CreateOnly.".to_string(),
        ])),
    }
}

fn normalize_header_name(header: &str) -> Option<String> {
    let key = header.trim().to_lowercase().replace(' ', "_");
    let normalized = match key.as_str() {
        "employee_code" | "code" | "matricule" => "employee_code",
        "hr_external_id" | "external_id" | "hr_id" => "hr_external_id",
        "full_name" | "name" | "nom" => "full_name",
        "employment_type" | "contract_type" => "employment_type",
        "email" => "email",
        "phone" | "telephone" => "phone",
        "availability_status" | "status" => "availability_status",
        "notes" | "note" => "notes",
        "hire_date" => "hire_date",
        "termination_date" => "termination_date",
        "position_code" | "position" => "position_code",
        "entity_code" | "entity" => "entity_code",
        "team_code" | "team" => "team_code",
        "supervisor_employee_code" | "supervisor_code" => "supervisor_employee_code",
        "external_company_name" | "company_name" => "external_company_name",
        _ => return None,
    };
    Some(normalized.to_string())
}

fn normalize_row(fields: &HashMap<String, String>) -> NormalizedPersonnelRow {
    let value = |key: &str| -> Option<String> {
        fields.get(key).and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    };
    NormalizedPersonnelRow {
        employee_code: value("employee_code"),
        hr_external_id: value("hr_external_id"),
        full_name: value("full_name"),
        employment_type: value("employment_type"),
        email: value("email"),
        phone: value("phone"),
        availability_status: value("availability_status"),
        notes: value("notes"),
        hire_date: value("hire_date"),
        termination_date: value("termination_date"),
        position_code: value("position_code"),
        entity_code: value("entity_code"),
        team_code: value("team_code"),
        supervisor_employee_code: value("supervisor_employee_code"),
        external_company_name: value("external_company_name"),
    }
}

fn push_message(
    messages: &mut Vec<PersonnelImportMessage>,
    category: &str,
    severity: &str,
    message: &str,
) {
    messages.push(PersonnelImportMessage {
        category: category.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
    });
}

async fn get_batch_summary(
    db: &impl ConnectionTrait,
    batch_id: i64,
) -> AppResult<PersonnelImportBatchSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                id, source_filename, source_sha256, source_kind, mode, status,
                total_rows, valid_rows, warning_rows, error_rows, initiated_by_id, created_at, updated_at
             FROM personnel_import_batches
             WHERE id = ?",
            [batch_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "personnel_import_batch".to_string(),
            id: batch_id.to_string(),
        })?;

    Ok(PersonnelImportBatchSummary {
        id: decode_col(&row, "id")?,
        source_filename: decode_col(&row, "source_filename")?,
        source_sha256: decode_col(&row, "source_sha256")?,
        source_kind: decode_col(&row, "source_kind")?,
        mode: decode_col(&row, "mode")?,
        status: decode_col(&row, "status")?,
        total_rows: decode_col(&row, "total_rows")?,
        valid_rows: decode_col(&row, "valid_rows")?,
        warning_rows: decode_col(&row, "warning_rows")?,
        error_rows: decode_col(&row, "error_rows")?,
        initiated_by_id: decode_col(&row, "initiated_by_id")?,
        created_at: decode_col(&row, "created_at")?,
        updated_at: decode_col(&row, "updated_at")?,
    })
}

fn decode_col<T: sea_orm::TryGetable>(row: &QueryResult, col: &str) -> AppResult<T> {
    row.try_get("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to decode column {col}: {e}")))
}
