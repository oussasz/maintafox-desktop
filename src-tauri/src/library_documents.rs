//! Tenant document library — filesystem storage + SQLite metadata (WO/DI attachment pattern).

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_FILE_SIZE_BYTES: usize = 25 * 1024 * 1024;

/// Valid `library_documents.category` values.
pub const LIBRARY_CATEGORIES: &[&str] = &[
    "technical_manuals",
    "sops",
    "safety_protocols",
    "compliance_certificates",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDocumentRow {
    pub id: i64,
    pub category: String,
    pub equipment_id: Option<i64>,
    pub equipment_code: Option<String>,
    pub equipment_name: Option<String>,
    pub title: String,
    pub file_name: String,
    pub relative_path: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uploaded_by_id: Option<i64>,
    pub uploaded_at: String,
    pub notes: Option<String>,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "library_documents row decode failed for column '{column}': {e}"
    ))
}

fn map_row(row: &QueryResult) -> AppResult<LibraryDocumentRow> {
    Ok(LibraryDocumentRow {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        category: row
            .try_get::<String>("", "category")
            .map_err(|e| decode_err("category", e))?,
        equipment_id: row
            .try_get::<Option<i64>>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        equipment_code: row
            .try_get::<Option<String>>("", "equipment_code")
            .map_err(|e| decode_err("equipment_code", e))?,
        equipment_name: row
            .try_get::<Option<String>>("", "equipment_name")
            .map_err(|e| decode_err("equipment_name", e))?,
        title: row
            .try_get::<String>("", "title")
            .map_err(|e| decode_err("title", e))?,
        file_name: row
            .try_get::<String>("", "file_name")
            .map_err(|e| decode_err("file_name", e))?,
        relative_path: row
            .try_get::<String>("", "relative_path")
            .map_err(|e| decode_err("relative_path", e))?,
        mime_type: row
            .try_get::<String>("", "mime_type")
            .map_err(|e| decode_err("mime_type", e))?,
        size_bytes: row
            .try_get::<i64>("", "size_bytes")
            .map_err(|e| decode_err("size_bytes", e))?,
        uploaded_by_id: row
            .try_get::<Option<i64>>("", "uploaded_by_id")
            .map_err(|e| decode_err("uploaded_by_id", e))?,
        uploaded_at: row
            .try_get::<String>("", "uploaded_at")
            .map_err(|e| decode_err("uploaded_at", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

fn validate_category(cat: &str) -> AppResult<()> {
    if LIBRARY_CATEGORIES.contains(&cat) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Invalid document category: {cat}"
        )]))
    }
}

/// List library documents with optional filters.
pub async fn list_library_documents(
    db: &impl ConnectionTrait,
    category: Option<String>,
    equipment_id: Option<i64>,
) -> AppResult<Vec<LibraryDocumentRow>> {
    let mut sql = String::from(
        "SELECT ld.id, ld.category, ld.equipment_id, ld.title, ld.file_name, ld.relative_path, \
         ld.mime_type, ld.size_bytes, ld.uploaded_by_id, ld.uploaded_at, ld.notes, \
         e.asset_id_code AS equipment_code, e.name AS equipment_name \
         FROM library_documents ld \
         LEFT JOIN equipment e ON e.id = ld.equipment_id \
         WHERE 1=1",
    );
    let mut params: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref c) = category {
        validate_category(c)?;
        sql.push_str(" AND ld.category = ?");
        params.push(c.clone().into());
    }
    if let Some(eid) = equipment_id {
        sql.push_str(" AND ld.equipment_id = ?");
        params.push(eid.into());
    }

    sql.push_str(" ORDER BY ld.uploaded_at DESC");

    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params);
    let rows = db.query_all(stmt).await?;
    rows.iter().map(map_row).collect()
}

#[derive(Debug, Deserialize)]
pub struct LibraryDocumentUploadInput {
    pub category: String,
    pub equipment_id: Option<i64>,
    pub title: String,
    pub file_name: String,
    pub file_bytes: Vec<u8>,
    pub mime_type: String,
    pub notes: Option<String>,
    pub uploaded_by_id: i64,
}

pub async fn save_library_document(
    db: &impl ConnectionTrait,
    app_data_dir: &Path,
    input: LibraryDocumentUploadInput,
) -> AppResult<LibraryDocumentRow> {
    validate_category(&input.category)?;

    if input.file_bytes.len() > MAX_FILE_SIZE_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "File exceeds {} MB limit.",
            MAX_FILE_SIZE_BYTES / (1024 * 1024)
        )]));
    }

    let sanitized_name = Path::new(&input.file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    if sanitized_name.is_empty() || sanitized_name.contains("..") {
        return Err(AppError::ValidationFailed(vec!["Invalid file name.".into()]));
    }

    if let Some(eid) = input.equipment_id {
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
                [eid.into()],
            ))
            .await?
            .is_some();
        if !exists {
            return Err(AppError::NotFound {
                entity: "Equipment".into(),
                id: eid.to_string(),
            });
        }
    }

    let uuid = Uuid::new_v4();
    let relative_path = format!(
        "library_documents/{}/{}-{}",
        input.category, uuid, sanitized_name
    );

    let absolute_path: PathBuf = app_data_dir.join(&relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&absolute_path, &input.file_bytes)?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let size_bytes = input.file_bytes.len() as i64;
    let title = input.title.trim().to_string();
    let title_db = if title.is_empty() {
        sanitized_name.to_string()
    } else {
        title
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO library_documents \
            (category, equipment_id, title, file_name, relative_path, mime_type, size_bytes, \
             uploaded_by_id, uploaded_at, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.category.into(),
            input
                .equipment_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            title_db.into(),
            sanitized_name.to_string().into(),
            relative_path.clone().into(),
            input.mime_type.into(),
            size_bytes.into(),
            input.uploaded_by_id.into(),
            now.into(),
            input
                .notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ld.id, ld.category, ld.equipment_id, ld.title, ld.file_name, ld.relative_path, \
             ld.mime_type, ld.size_bytes, ld.uploaded_by_id, ld.uploaded_at, ld.notes, \
             e.asset_id_code AS equipment_code, e.name AS equipment_name \
             FROM library_documents ld \
             LEFT JOIN equipment e ON e.id = ld.equipment_id \
             WHERE ld.relative_path = ?",
            [relative_path.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Failed to re-fetch library_documents row"))
        })?;

    map_row(&row)
}

/// Read file bytes for a document (path validated against stored prefix).
pub async fn read_library_document_file(
    db: &impl ConnectionTrait,
    app_data_dir: &Path,
    id: i64,
) -> AppResult<Vec<u8>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT relative_path FROM library_documents WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "LibraryDocument".into(),
            id: id.to_string(),
        })?;

    let relative_path: String = row
        .try_get::<String>("", "relative_path")
        .map_err(|e| decode_err("relative_path", e))?;

    if !relative_path.starts_with("library_documents/") || relative_path.contains("..") {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Invalid stored path for library document"
        )));
    }

    let absolute = app_data_dir.join(&relative_path);
    std::fs::read(&absolute).map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Failed to read library file {relative_path}: {e}"))
    })
}

pub async fn delete_library_document_record(
    db: &impl ConnectionTrait,
    id: i64,
) -> AppResult<()> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM library_documents WHERE id = ?",
            [id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "LibraryDocument".into(),
            id: id.to_string(),
        });
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLibraryDocumentInput {
    pub id: i64,
    pub title: Option<String>,
    pub equipment_id: Option<i64>,
    /// When true, clears `equipment_id` (ignores `equipment_id`).
    pub clear_equipment_link: bool,
}

pub async fn update_library_document_metadata(
    db: &impl ConnectionTrait,
    input: UpdateLibraryDocumentInput,
) -> AppResult<LibraryDocumentRow> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM library_documents WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .is_some();
    if !exists {
        return Err(AppError::NotFound {
            entity: "LibraryDocument".into(),
            id: input.id.to_string(),
        });
    }

    if !input.clear_equipment_link {
        if let Some(eid) = input.equipment_id {
            let ok = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
                    [eid.into()],
                ))
                .await?
                .is_some();
            if !ok {
                return Err(AppError::NotFound {
                    entity: "Equipment".into(),
                    id: eid.to_string(),
                });
            }
        }
    }

    let new_title = input.title.map(|s| s.trim().to_string());

    match (
        new_title.as_ref(),
        input.clear_equipment_link,
        input.equipment_id,
    ) {
        (Some(t), true, _) => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE library_documents SET title = ?, equipment_id = NULL WHERE id = ?",
                [t.clone().into(), input.id.into()],
            ))
            .await?;
        }
        (None, true, _) => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE library_documents SET equipment_id = NULL WHERE id = ?",
                [input.id.into()],
            ))
            .await?;
        }
        (Some(t), false, Some(eid)) => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE library_documents SET title = ?, equipment_id = ? WHERE id = ?",
                [t.clone().into(), eid.into(), input.id.into()],
            ))
            .await?;
        }
        (None, false, Some(eid)) => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE library_documents SET equipment_id = ? WHERE id = ?",
                [eid.into(), input.id.into()],
            ))
            .await?;
        }
        (Some(t), false, None) => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE library_documents SET title = ? WHERE id = ?",
                [t.clone().into(), input.id.into()],
            ))
            .await?;
        }
        (None, false, None) => {}
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ld.id, ld.category, ld.equipment_id, ld.title, ld.file_name, ld.relative_path, \
             ld.mime_type, ld.size_bytes, ld.uploaded_by_id, ld.uploaded_at, ld.notes, \
             e.asset_id_code AS equipment_code, e.name AS equipment_name \
             FROM library_documents ld \
             LEFT JOIN equipment e ON e.id = ld.equipment_id \
             WHERE ld.id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("library document row missing after update"))
        })?;

    map_row(&row)
}
