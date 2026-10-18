//! Tenant document library — filesystem storage + SQLite metadata (WO/DI attachment pattern).
//!
//! Documents replicate like attachments: the row syncs, and its `media_attachments` row (parent
//! `library_documents`) makes the sync worker upload the file and the other devices download it.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_FILE_SIZE_BYTES: usize = 25 * 1024 * 1024;

/// Document categories: the plant's COMPANY reference list (migration 228). It is seeded with the
/// four codes the library used to fix in a CHECK constraint, so stored documents keep their
/// category; every write checks the code against the active values of the list.
pub const DOCUMENT_CATEGORIES_DOMAIN: &str = "DOCUMENT.CATEGORIES";

/// Stable code of the refusal (translated in `training:errors`).
pub const CODE_CATEGORY_NOT_IN_LIST: &str = "DOCUMENT_CATEGORY_NOT_IN_LIST";

/// Seeded categories: (code, label FR, label EN).
pub const LIBRARY_CATEGORIES: &[(&str, &str, &str)] = &[
    ("technical_manuals", "Manuels techniques", "Technical manuals"),
    ("sops", "Modes opératoires", "Standard operating procedures"),
    ("safety_protocols", "Protocoles de sécurité", "Safety protocols"),
    (
        "compliance_certificates",
        "Certificats de conformité",
        "Compliance certificates",
    ),
];

/// The list with its seeded values. Idempotent (integrity pass and migration 228).
pub async fn ensure_document_categories(db: &impl ConnectionTrait) -> AppResult<()> {
    use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};
    ensure_domain(
        db,
        DOCUMENT_CATEGORIES_DOMAIN,
        "Catégories de documents",
        "flat",
        "tenant_managed",
        1,
        crate::reference::audience::DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, DOCUMENT_CATEGORIES_DOMAIN).await?;
    for (i, (code, label_fr, label_en)) in LIBRARY_CATEGORIES.iter().enumerate() {
        let sync_id = crate::sync::identity::seeded_entity_sync_id(
            "reference_values",
            &format!("{DOCUMENT_CATEGORIES_DOMAIN}:{code}"),
        );
        let meta = serde_json::json!({ "origin": "system", "label_en": label_en });
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, \
                description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, \
                metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, NULL, NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [
                sync_id.clone().into(),
                (*code).into(),
                (*label_fr).into(),
                (i64::try_from(i).unwrap_or(0) + 1).into(),
                meta.to_string().into(),
                DOCUMENT_CATEGORIES_DOMAIN.into(),
                sync_id.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// One category of the library (active value of the latest published set).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentCategoryOption {
    pub code: String,
    pub label: String,
    pub label_en: Option<String>,
    /// A category found on stored documents that was not in the list (e.g. received by sync),
    /// kept with its exact code and flagged "à revoir".
    #[serde(default)]
    pub review_required: bool,
}

/// Adopts the categories stored on documents that are not in the list (flagged "à revoir", exact
/// code kept; migration 229 and the integrity pass, so a document received by sync with a
/// category unknown here is never lost nor hidden).
pub async fn adopt_stored_document_categories(db: &impl ConnectionTrait) -> AppResult<Vec<String>> {
    crate::personnel::reference_lists::adopt_stored_values(
        db,
        DOCUMENT_CATEGORIES_DOMAIN,
        "library_documents",
        "category",
        None,
    )
    .await
}

/// The active document categories, in the list order.
pub async fn list_document_categories(db: &impl ConnectionTrait) -> AppResult<Vec<DocumentCategoryOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.metadata_json FROM reference_values rv \
              WHERE rv.is_active = 1 AND rv.set_id = ( \
                    SELECT rs.id FROM reference_sets rs \
                      JOIN reference_domains d ON d.id = rs.domain_id \
                     WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' \
                     ORDER BY rs.version_no DESC LIMIT 1) \
              ORDER BY rv.sort_order, rv.label",
            [DOCUMENT_CATEGORIES_DOMAIN.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let meta: Option<String> = r.try_get("", "metadata_json")?;
            let label_en = meta
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("label_en").and_then(|l| l.as_str()).map(str::to_string));
            let review_required = meta
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("review_required").and_then(serde_json::Value::as_bool))
                .unwrap_or(false);
            Ok(DocumentCategoryOption {
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
                label_en,
                review_required,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

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
        title: row.try_get::<String>("", "title").map_err(|e| decode_err("title", e))?,
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

/// A document category: an active value of `DOCUMENT.CATEGORIES` (its list code). Replaces the
/// former CHECK constraint, so a document never carries a category outside the list.
pub async fn checked_category(db: &impl ConnectionTrait, cat: &str) -> AppResult<String> {
    let wanted = cat.trim();
    list_document_categories(db)
        .await?
        .into_iter()
        .find(|c| c.code.eq_ignore_ascii_case(wanted))
        .map(|c| c.code)
        .ok_or_else(|| {
            crate::errors::org_validation_failed(
                CODE_CATEGORY_NOT_IN_LIST,
                format!("Document category '{wanted}' is not an active value of {DOCUMENT_CATEGORIES_DOMAIN}."),
                std::collections::HashMap::from([("code".to_string(), wanted.to_string())]),
            )
        })
}

/// Storage folder of a category: its code with dots replaced, so a code can never name a parent
/// directory.
fn category_folder(code: &str) -> String {
    code.replace('.', "_")
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
        // A filter may name a retired category (its documents stay listed); bound, never spliced.
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
    db: &DatabaseConnection,
    app_data_dir: &Path,
    input: LibraryDocumentUploadInput,
) -> AppResult<LibraryDocumentRow> {
    let category = checked_category(db, &input.category).await?;
    if input.file_bytes.is_empty() {
        return Err(AppError::ValidationFailed(vec!["The file is empty.".into()]));
    }

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

    // Resolved before the transaction: the helper reads through the pool connection.
    let env = crate::sync::media::resolve_media_deployment_env();
    let tenant_id = crate::sync::media::resolve_media_tenant_id(db).await;
    // The document row and its media row commit together.
    let txn = db.begin().await?;

    if let Some(eid) = input.equipment_id {
        let exists = txn
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
        category_folder(&category),
        uuid,
        sanitized_name
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

    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    let mime_type = input.mime_type.clone();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO library_documents \
            (category, equipment_id, title, file_name, relative_path, mime_type, size_bytes, \
             uploaded_by_id, uploaded_at, notes, entity_sync_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            category.clone().into(),
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
            entity_sync_id.clone().into(),
        ],
    ))
    .await?;

    let media = crate::sync::media::build_media_attachment(
        env,
        &tenant_id,
        "library_documents",
        &entity_sync_id,
        sanitized_name,
        &mime_type,
        &input.file_bytes,
        Some(relative_path.clone()),
        None,
    )?;
    crate::sync::media::upsert_media_attachment_row(&txn, &media).await?;

    let row = txn
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-fetch library_documents row")))?;
    let document = map_row(&row)?;
    txn.commit().await?;
    Ok(document)
}

/// Read file bytes for a document (path validated against stored prefix).
pub async fn read_library_document_file(db: &impl ConnectionTrait, app_data_dir: &Path, id: i64) -> AppResult<Vec<u8>> {
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
    std::fs::read(&absolute)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to read library file {relative_path}: {e}")))
}

/// Delete the document row and its media row (the file stays on disk, like attachments).
pub async fn delete_library_document_record(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    let txn = db.begin().await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT relative_path, entity_sync_id FROM library_documents WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "LibraryDocument".into(),
            id: id.to_string(),
        })?;
    let relative_path: String = row
        .try_get("", "relative_path")
        .map_err(|e| decode_err("relative_path", e))?;
    let entity_sync_id: Option<String> = row
        .try_get("", "entity_sync_id")
        .map_err(|e| decode_err("entity_sync_id", e))?;
    crate::sync::media::delete_media_attachment_if_found(
        &txn,
        Some("library_documents"),
        entity_sync_id.as_deref(),
        Some(relative_path.as_str()),
        None,
    )
    .await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM library_documents WHERE id = ?",
        [id.into()],
    ))
    .await?;
    txn.commit().await?;
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

    match (new_title.as_ref(), input.clear_equipment_link, input.equipment_id) {
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("library document row missing after update")))?;

    map_row(&row)
}
