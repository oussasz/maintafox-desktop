//! Shared "Imports de données" wizard: one staging, preview and apply flow for every kind of plant
//! data that has no dedicated importer (organisation tree first, the others plug in the same way).
//!
//! A file (CSV or Excel) becomes a batch of staged rows, each row is checked against the engine rules
//! and shown with its proposed action (create / update / skip) and its problems, and only a batch
//! without a blocking surprise is applied. Rows are keyed by the business codes the user can see.
//! An import never deletes a row that is missing from the file.

pub mod cost_centre;
#[cfg(test)]
mod cost_centre_tests;
pub mod equipment_meter;
#[cfg(test)]
mod equipment_meter_tests;
pub mod org_tree;
#[cfg(test)]
mod org_tree_tests;
pub mod personnel_certification;
pub mod personnel_skill;
#[cfg(test)]
mod personnel_skill_tests;
pub mod stock_store;
#[cfg(test)]
mod stock_store_tests;
pub mod stock_supplier;
#[cfg(test)]
mod stock_supplier_tests;
pub mod team;
pub mod team_member;
#[cfg(test)]
mod team_tests;

use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use calamine::{Reader, Xlsx};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{AppError, AppResult};

// ─── Entity catalogue ─────────────────────────────────────────────────────────

/// One column of an import file. `aliases` are extra spellings accepted for the header
/// (matching ignores case, accents, spaces and apostrophes).
pub struct Column {
    pub key: &'static str,
    pub header: &'static str,
    pub required: bool,
    pub aliases: &'static [&'static str],
    pub hint: &'static str,
}

/// An importable kind of data.
pub struct EntitySpec {
    pub code: &'static str,
    pub label: &'static str,
    /// Step in the loading order shown to the user (0 = reference lists, loaded first).
    pub order: u8,
    pub needs: &'static str,
    pub permission: &'static str,
    pub columns: &'static [Column],
}

pub const ENTITIES: &[&EntitySpec] = &[
    &org_tree::SPEC,
    &cost_centre::SPEC,
    &team::SPEC,
    &team_member::SPEC,
    &personnel_skill::SPEC,
    &personnel_certification::SPEC,
    &equipment_meter::SPEC,
    &stock_store::STORE_SPEC,
    &stock_store::LOCATION_SPEC,
    &stock_supplier::SPEC,
];

pub fn spec(entity: &str) -> AppResult<&'static EntitySpec> {
    ENTITIES
        .iter()
        .find(|s| s.code == entity)
        .copied()
        .ok_or_else(|| gate("GATE_BULK_IMPORT_ENTITY", "Ce type d'import n'existe pas."))
}

fn gate(code: &str, message: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {message}")])
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub key: String,
    pub header: String,
    pub required: bool,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityInfo {
    pub code: String,
    pub label: String,
    pub order: u8,
    pub needs: String,
    pub columns: Vec<ColumnInfo>,
}

pub fn list_entities() -> Vec<EntityInfo> {
    let mut out: Vec<EntityInfo> = ENTITIES
        .iter()
        .map(|s| EntityInfo {
            code: s.code.to_string(),
            label: s.label.to_string(),
            order: s.order,
            needs: s.needs.to_string(),
            columns: s
                .columns
                .iter()
                .map(|c| ColumnInfo {
                    key: c.key.to_string(),
                    header: c.header.to_string(),
                    required: c.required,
                    hint: c.hint.to_string(),
                })
                .collect(),
        })
        .collect();
    out.sort_by_key(|e| e.order);
    out
}

// ─── Row outcome ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowMessage {
    /// "error" blocks the row, "warning" does not.
    pub severity: String,
    pub code: String,
    pub message: String,
}

impl RowMessage {
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: "error".into(),
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: "warning".into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Result of checking one staged row.
pub struct RowOutcome {
    /// "create", "update" or "skip".
    pub action: &'static str,
    pub messages: Vec<RowMessage>,
    pub normalized: Value,
}

impl RowOutcome {
    pub fn status(&self) -> &'static str {
        if self.messages.iter().any(|m| m.severity == "error") {
            "error"
        } else if self.messages.is_empty() {
            "ok"
        } else {
            "warning"
        }
    }
}

// ─── File parsing ─────────────────────────────────────────────────────────────

/// Lower-case, accents removed, spaces / apostrophes / dashes turned into `_`.
pub fn normalize_header(header: &str) -> String {
    let mut out = String::new();
    for ch in header.trim().chars() {
        let c = match ch {
            'à' | 'â' | 'ä' | 'À' | 'Â' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'î' | 'ï' | 'Î' | 'Ï' => 'i',
            'ô' | 'ö' | 'Ô' | 'Ö' => 'o',
            'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            c if c.is_alphanumeric() => c.to_ascii_lowercase(),
            _ => '_',
        };
        out.push(c);
    }
    let mut collapsed = String::new();
    let mut last_us = false;
    for c in out.chars() {
        if c == '_' {
            if !last_us {
                collapsed.push('_');
            }
            last_us = true;
        } else {
            collapsed.push(c);
            last_us = false;
        }
    }
    collapsed.trim_matches('_').to_string()
}

/// Largest file accepted (a few thousand lines of text is far below this).
const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
/// Most data lines in one file; split bigger files into several.
const MAX_ROWS: usize = 5000;

fn too_many_rows() -> AppError {
    gate(
        "GATE_BULK_IMPORT_FILE",
        &format!("Le fichier a trop de lignes (maximum {MAX_ROWS}) : découpez-le en plusieurs fichiers."),
    )
}

/// Raw cells of the file: the header line and the data lines.
fn read_cells(source_kind: &str, bytes: &[u8]) -> AppResult<(Vec<String>, Vec<Vec<String>>)> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(gate(
            "GATE_BULK_IMPORT_FILE",
            &format!(
                "Le fichier est trop volumineux (maximum {} Mo).",
                MAX_FILE_BYTES / (1024 * 1024)
            ),
        ));
    }
    match source_kind {
        "csv" => {
            let bytes = crate::assets::import::strip_utf8_bom(bytes);
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(true)
                .flexible(true)
                .delimiter(crate::assets::import::detect_csv_delimiter(bytes))
                .trim(csv::Trim::All)
                .from_reader(bytes);
            let headers: Vec<String> = reader
                .headers()
                .map_err(|e| {
                    gate(
                        "GATE_BULK_IMPORT_FILE",
                        &format!("Impossible de lire la ligne d'en-tête du fichier : {e}"),
                    )
                })?
                .iter()
                .map(|h| h.trim().to_string())
                .collect();
            let mut rows = Vec::new();
            for record in reader.records() {
                let record = record.map_err(|e| {
                    gate(
                        "GATE_BULK_IMPORT_FILE",
                        &format!("Une ligne du fichier est illisible : {e}"),
                    )
                })?;
                if rows.len() >= MAX_ROWS {
                    return Err(too_many_rows());
                }
                rows.push(record.iter().map(|v| v.trim().to_string()).collect());
            }
            Ok((headers, rows))
        }
        "xlsx" => {
            let mut workbook = Xlsx::new(Cursor::new(bytes.to_vec())).map_err(|e| {
                gate(
                    "GATE_BULK_IMPORT_FILE",
                    &format!("Le fichier Excel est illisible : {e}"),
                )
            })?;
            let range = workbook
                .worksheet_range_at(0)
                .ok_or_else(|| gate("GATE_BULK_IMPORT_FILE", "Le classeur Excel ne contient aucune feuille."))?
                .map_err(|e| {
                    gate(
                        "GATE_BULK_IMPORT_FILE",
                        &format!("La première feuille du classeur est illisible : {e}"),
                    )
                })?;
            if range.height() > MAX_ROWS + 1 {
                return Err(too_many_rows());
            }
            let mut iter = range.rows();
            let Some(first) = iter.next() else {
                return Ok((Vec::new(), Vec::new()));
            };
            let headers = first.iter().map(|c| c.to_string().trim().to_string()).collect();
            let rows = iter
                .map(|r| r.iter().map(|c| c.to_string().trim().to_string()).collect())
                .collect();
            Ok((headers, rows))
        }
        _ => Err(gate(
            "GATE_BULK_IMPORT_KIND",
            "Le fichier doit être un CSV ou un classeur Excel (.xlsx).",
        )),
    }
}

/// Map the file columns onto the entity columns. Returns the rows keyed by column `key`
/// (empty cells omitted) and the headers of the file that are not part of the import.
fn map_columns(
    spec: &EntitySpec,
    headers: &[String],
    cells: Vec<Vec<String>>,
) -> AppResult<(Vec<HashMap<String, String>>, Vec<String>)> {
    let mut index_to_key: Vec<Option<&'static str>> = Vec::new();
    let mut ignored = Vec::new();
    let mut seen: HashSet<&'static str> = HashSet::new();
    for header in headers {
        let norm = normalize_header(header);
        let found = spec.columns.iter().find(|c| {
            normalize_header(c.header) == norm || c.key == norm || c.aliases.iter().any(|a| normalize_header(a) == norm)
        });
        match found {
            Some(c) if seen.insert(c.key) => index_to_key.push(Some(c.key)),
            _ => {
                if !header.trim().is_empty() {
                    ignored.push(header.clone());
                }
                index_to_key.push(None);
            }
        }
    }
    let missing: Vec<&str> = spec
        .columns
        .iter()
        .filter(|c| c.required && !seen.contains(c.key))
        .map(|c| c.header)
        .collect();
    if !missing.is_empty() {
        return Err(gate(
            "GATE_BULK_IMPORT_COLUMNS",
            &format!(
                "Colonne(s) obligatoire(s) absente(s) du fichier : {}. Téléchargez le modèle pour voir les colonnes attendues.",
                missing.join(", ")
            ),
        ));
    }
    let rows = cells
        .into_iter()
        .filter(|r| r.iter().any(|v| !v.is_empty()))
        .map(|r| {
            let mut map = HashMap::new();
            for (i, v) in r.into_iter().enumerate() {
                if let Some(Some(key)) = index_to_key.get(i) {
                    if !v.is_empty() {
                        map.insert((*key).to_string(), v);
                    }
                }
            }
            map
        })
        .collect();
    Ok((rows, ignored))
}

/// A date cell: `JJ/MM/AAAA` (or `J/M/AAAA`), ISO `AAAA-MM-JJ`, or an Excel date serial; a time
/// after the date is ignored. The year needs four digits. A bare number is read as an Excel date
/// only between 1950 and 2100, so a stray year or count is never turned into a date. `Err` when
/// the cell is not a date.
pub(crate) fn parse_date(raw: Option<&str>) -> Result<Option<chrono::NaiveDate>, ()> {
    use chrono::NaiveDate;
    let Some(text) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let day = text.split(['T', ' ']).next().unwrap_or(text);
    if day.rsplit('/').next().is_some_and(|y| y.len() == 4) {
        if let Ok(d) = NaiveDate::parse_from_str(day, "%d/%m/%Y") {
            return Ok(Some(d));
        }
    }
    if day.split('-').next().is_some_and(|y| y.len() == 4) {
        if let Ok(d) = NaiveDate::parse_from_str(day, "%Y-%m-%d") {
            return Ok(Some(d));
        }
    }
    // Excel stores a date as the number of days since 30/12/1899 (18264 = 01/01/1950,
    // 73051 = 01/01/2100).
    if let Ok(serial) = text.parse::<f64>() {
        if (18_264.0..73_051.0).contains(&serial) {
            let base = NaiveDate::from_ymd_opt(1899, 12, 30).ok_or(())?;
            return base
                .checked_add_signed(chrono::Duration::days(serial.trunc() as i64))
                .map(Some)
                .ok_or(());
        }
    }
    Err(())
}

// ─── Batches ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateInput {
    pub entity: String,
    pub filename: String,
    pub source_sha256: String,
    pub source_kind: String,
    pub file_content: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchSummary {
    pub id: i64,
    pub entity: String,
    pub source_filename: String,
    pub status: String,
    pub total_rows: i64,
    pub valid_rows: i64,
    pub warning_rows: i64,
    pub error_rows: i64,
    pub ignored_columns: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewRow {
    pub row_no: i64,
    pub status: String,
    pub action: String,
    pub raw: HashMap<String, String>,
    pub messages: Vec<RowMessage>,
    pub apply_status: Option<String>,
    pub apply_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub batch: BatchSummary,
    pub rows: Vec<PreviewRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub batch: BatchSummary,
    pub created: i64,
    pub updated: i64,
    pub skipped: i64,
    pub failed: i64,
}

fn batch_from_row(row: &sea_orm::QueryResult) -> AppResult<BatchSummary> {
    let ignored: String = row.try_get("", "ignored_columns_json").unwrap_or_default();
    Ok(BatchSummary {
        id: row.try_get("", "id")?,
        entity: row.try_get("", "entity")?,
        source_filename: row.try_get("", "source_filename")?,
        status: row.try_get("", "status")?,
        total_rows: row.try_get("", "total_rows")?,
        valid_rows: row.try_get("", "valid_rows")?,
        warning_rows: row.try_get("", "warning_rows")?,
        error_rows: row.try_get("", "error_rows")?,
        ignored_columns: serde_json::from_str(&ignored).unwrap_or_default(),
        created_at: row.try_get("", "created_at")?,
    })
}

async fn load_batch(db: &impl ConnectionTrait, batch_id: i64) -> AppResult<BatchSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM bulk_import_batches WHERE id = ?",
            [batch_id.into()],
        ))
        .await?
        .ok_or_else(|| gate("GATE_BULK_IMPORT_NOT_FOUND", "Ce lot d'import n'existe pas."))?;
    batch_from_row(&row)
}

/// Which import type a batch belongs to (read before the permission check, so keep it light).
pub async fn entity_of(db: &DatabaseConnection, batch_id: i64) -> AppResult<String> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT entity FROM bulk_import_batches WHERE id = ?",
        [batch_id.into()],
    ))
    .await?
    .ok_or_else(|| gate("GATE_BULK_IMPORT_NOT_FOUND", "Ce lot d'import n'existe pas."))?
    .try_get("", "entity")
    .map_err(Into::into)
}

/// Checks run when the batch is created, so the screen shows the preview at once.
async fn validate(
    db: &DatabaseConnection,
    entity: &str,
    rows: &[HashMap<String, String>],
) -> AppResult<Vec<RowOutcome>> {
    match entity {
        "org_tree" => org_tree::validate(db, rows).await,
        "cost_centre" => cost_centre::validate(db, rows).await,
        "team" => team::validate(db, rows).await,
        "team_member" => team_member::validate(db, rows).await,
        "personnel_skill" => personnel_skill::validate(db, rows).await,
        "personnel_certification" => personnel_certification::validate(db, rows).await,
        "equipment_meter" => equipment_meter::validate(db, rows).await,
        "stock_store" => stock_store::validate_stores(db, rows).await,
        "stock_location" => stock_store::validate_locations(db, rows).await,
        "stock_supplier" => stock_supplier::validate(db, rows).await,
        _ => Err(gate("GATE_BULK_IMPORT_ENTITY", "Ce type d'import n'existe pas.")),
    }
}

pub async fn create_batch(
    db: &DatabaseConnection,
    input: CreateInput,
    user_id: Option<i64>,
) -> AppResult<BatchSummary> {
    let spec = spec(&input.entity)?;
    let kind = input.source_kind.trim().to_lowercase();
    let (headers, cells) = read_cells(&kind, &input.file_content)?;
    let (rows, ignored) = map_columns(spec, &headers, cells)?;
    if rows.is_empty() {
        return Err(gate(
            "GATE_BULK_IMPORT_EMPTY",
            "Le fichier ne contient aucune ligne à importer.",
        ));
    }
    let outcomes = validate(db, spec.code, &rows).await?;

    let now = crate::clock::now().to_rfc3339();
    let (mut valid, mut warn, mut err) = (0i64, 0i64, 0i64);
    for o in &outcomes {
        match o.status() {
            "ok" => valid += 1,
            "warning" => warn += 1,
            _ => err += 1,
        }
    }
    let txn = db.begin().await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO bulk_import_batches \
         (entity, source_filename, source_sha256, source_kind, status, total_rows, valid_rows, warning_rows, \
          error_rows, ignored_columns_json, initiated_by_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'validated', ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            spec.code.into(),
            input.filename.into(),
            input.source_sha256.into(),
            kind.into(),
            (rows.len() as i64).into(),
            valid.into(),
            warn.into(),
            err.into(),
            serde_json::to_string(&ignored).unwrap_or_else(|_| "[]".into()).into(),
            user_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let batch_id: i64 = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| gate("GATE_BULK_IMPORT_NOT_FOUND", "Lot d'import introuvable après création."))?
        .try_get("", "id")?;
    for (i, (raw, outcome)) in rows.iter().zip(outcomes.iter()).enumerate() {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO bulk_import_rows \
             (batch_id, row_no, raw_json, validation_status, proposed_action, messages_json, normalized_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                batch_id.into(),
                ((i + 2) as i64).into(),
                serde_json::to_string(raw).unwrap_or_else(|_| "{}".into()).into(),
                outcome.status().into(),
                outcome.action.into(),
                serde_json::to_string(&outcome.messages)
                    .unwrap_or_else(|_| "[]".into())
                    .into(),
                outcome.normalized.to_string().into(),
            ],
        ))
        .await?;
    }
    txn.commit().await?;
    load_batch(db, batch_id).await
}

pub async fn get_preview(db: &DatabaseConnection, batch_id: i64) -> AppResult<Preview> {
    let batch = load_batch(db, batch_id).await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_no, raw_json, validation_status, proposed_action, messages_json, apply_status, apply_message \
             FROM bulk_import_rows WHERE batch_id = ? ORDER BY row_no",
            [batch_id.into()],
        ))
        .await?
        .iter()
        .map(|r| {
            let raw: String = r.try_get("", "raw_json").unwrap_or_default();
            let msgs: String = r.try_get("", "messages_json").unwrap_or_default();
            Ok(PreviewRow {
                row_no: r.try_get("", "row_no")?,
                status: r.try_get("", "validation_status")?,
                action: r.try_get("", "proposed_action")?,
                raw: serde_json::from_str(&raw).unwrap_or_default(),
                messages: serde_json::from_str(&msgs).unwrap_or_default(),
                apply_status: r.try_get("", "apply_status").ok(),
                apply_message: r.try_get("", "apply_message").ok(),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(Preview { batch, rows })
}

/// The apply step stopped on a database error part-way: the batch cannot be applied again. Some
/// rows may already be written (the organisation screen and its audit trail show them), but the
/// batch holds no per-row result for them; re-check the file to see what is still to do.
async fn mark_failed(db: &DatabaseConnection, batch_id: i64) {
    let _ = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE bulk_import_batches SET status = 'failed', updated_at = ? WHERE id = ?",
            [crate::clock::now().to_rfc3339().into(), batch_id.into()],
        ))
        .await;
}

/// The rows of a batch that passed the check, ready for the entity's apply step.
async fn staged_rows(db: &DatabaseConnection, batch_id: i64) -> AppResult<Vec<StagedRow>> {
    let staged = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_no, validation_status, proposed_action, normalized_json \
             FROM bulk_import_rows WHERE batch_id = ? ORDER BY row_no",
            [batch_id.into()],
        ))
        .await?;
    let mut todo: Vec<StagedRow> = Vec::new();
    for r in &staged {
        if r.try_get::<String>("", "validation_status")? == "error" {
            continue;
        }
        let normalized: String = r.try_get("", "normalized_json")?;
        todo.push(StagedRow {
            id: r.try_get("", "id")?,
            row_no: r.try_get("", "row_no")?,
            action: r.try_get("", "proposed_action")?,
            normalized: serde_json::from_str(&normalized).unwrap_or(Value::Null),
        });
    }
    Ok(todo)
}

/// A permission needed on top of the type's own one, because of what this particular batch does
/// (for example deactivating organisation nodes, which the organisation screen reserves for
/// administrators). `None` when the type's permission is enough.
pub async fn extra_permission(db: &DatabaseConnection, batch_id: i64) -> AppResult<Option<&'static str>> {
    let batch = load_batch(db, batch_id).await?;
    let rows = staged_rows(db, batch_id).await?;
    Ok(match batch.entity.as_str() {
        "org_tree" if org_tree::deactivates_nodes(&rows) => Some(crate::rbac::permissions::ORG_ADMIN),
        _ => None,
    })
}

pub async fn apply_batch(db: &DatabaseConnection, batch_id: i64, user_id: i32) -> AppResult<ApplyResult> {
    let batch = load_batch(db, batch_id).await?;
    // Claim the batch before writing anything: a second call (double click, retry) finds it
    // already taken and stops here instead of running the import twice.
    let claimed = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE bulk_import_batches SET status = 'applying', updated_at = ? \
             WHERE id = ? AND status = 'validated'",
            [crate::clock::now().to_rfc3339().into(), batch_id.into()],
        ))
        .await?;
    if claimed.rows_affected() != 1 {
        return Err(gate(
            "GATE_BULK_IMPORT_STATUS",
            "Ce lot a déjà été appliqué ou n'est plus valide.",
        ));
    }
    let todo = match staged_rows(db, batch_id).await {
        Ok(t) => t,
        Err(e) => {
            mark_failed(db, batch_id).await;
            return Err(e);
        }
    };

    let applied = match batch.entity.as_str() {
        "org_tree" => org_tree::apply(db, &todo, user_id).await,
        "cost_centre" => cost_centre::apply(db, &todo).await,
        "team" => team::apply(db, &todo).await,
        "team_member" => team_member::apply(db, &todo, user_id).await,
        "personnel_skill" => personnel_skill::apply(db, &todo).await,
        "personnel_certification" => personnel_certification::apply(db, &todo).await,
        "equipment_meter" => equipment_meter::apply(db, &todo, user_id).await,
        "stock_store" => stock_store::apply_stores(db, &todo).await,
        "stock_location" => stock_store::apply_locations(db, &todo).await,
        "stock_supplier" => stock_supplier::apply(db, &todo).await,
        _ => Err(gate("GATE_BULK_IMPORT_ENTITY", "Ce type d'import n'existe pas.")),
    };
    let results = match applied {
        Ok(r) => r,
        Err(e) => {
            mark_failed(db, batch_id).await;
            return Err(e);
        }
    };

    let (mut created, mut updated, mut skipped, mut failed) = (0i64, 0i64, 0i64, 0i64);
    let stored = async {
        let txn = db.begin().await?;
        for res in &results {
            match res.status {
                "created" => created += 1,
                "updated" => updated += 1,
                "skipped" => skipped += 1,
                _ => failed += 1,
            }
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE bulk_import_rows SET apply_status = ?, apply_message = ? WHERE id = ?",
                [res.status.into(), res.message.clone().into(), res.row_id.into()],
            ))
            .await?;
        }
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE bulk_import_batches SET status = 'applied', updated_at = ? WHERE id = ?",
            [crate::clock::now().to_rfc3339().into(), batch_id.into()],
        ))
        .await?;
        txn.commit().await?;
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(e) = stored {
        mark_failed(db, batch_id).await;
        return Err(e);
    }

    crate::audit::writer::write_audit_event(
        db,
        crate::audit::writer::AuditEventInput {
            action_code: "bulk_import.apply".to_string(),
            target_type: Some("bulk_import_batch".to_string()),
            target_id: Some(batch_id.to_string()),
            actor_id: Some(i64::from(user_id)),
            auth_context: "step_up".to_string(),
            result: if failed == 0 { "success" } else { "partial" }.to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "entity": batch.entity,
                "file": batch.source_filename,
                "created": created,
                "updated": updated,
                "skipped": skipped,
                "failed": failed,
                "error_rows_in_file": batch.error_rows,
            })),
        },
    )
    .await?;

    Ok(ApplyResult {
        batch: load_batch(db, batch_id).await?,
        created,
        updated,
        skipped,
        failed,
    })
}

/// A staged row that passed validation, handed to the entity's apply step.
pub struct StagedRow {
    pub id: i64,
    pub row_no: i64,
    pub action: String,
    pub normalized: Value,
}

/// What happened to one row at apply time.
pub struct RowApplied {
    pub row_id: i64,
    /// "created", "updated", "skipped" or "failed".
    pub status: &'static str,
    pub message: String,
}

// ─── Template and export ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ExportTable {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub fn template(entity: &str) -> AppResult<ExportTable> {
    let spec = spec(entity)?;
    Ok(ExportTable {
        header: spec.columns.iter().map(|c| c.header.to_string()).collect(),
        rows: Vec::new(),
    })
}

/// Current data in the same columns as the import file, so it can be edited and re-imported.
pub async fn export(db: &DatabaseConnection, entity: &str) -> AppResult<ExportTable> {
    let spec = spec(entity)?;
    let rows = match spec.code {
        "org_tree" => org_tree::export(db).await?,
        "cost_centre" => cost_centre::export(db).await?,
        "team" => team::export(db).await?,
        "team_member" => team_member::export(db).await?,
        "personnel_skill" => personnel_skill::export(db).await?,
        "personnel_certification" => personnel_certification::export(db).await?,
        "equipment_meter" => equipment_meter::export(db).await?,
        "stock_store" => stock_store::export_stores(db).await?,
        "stock_location" => stock_store::export_locations(db).await?,
        "stock_supplier" => stock_supplier::export(db).await?,
        _ => return Err(gate("GATE_BULK_IMPORT_ENTITY", "Ce type d'import n'existe pas.")),
    };
    Ok(ExportTable {
        header: spec.columns.iter().map(|c| c.header.to_string()).collect(),
        rows,
    })
}
