//! Equipment photo gallery — files under `app_data_dir/asset_photos/{asset_id}/`.

use std::path::{Path, PathBuf};

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct UploadAssetPhotoPayload {
    pub asset_id: i64,
    pub source_path: String,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetPhoto {
    pub id: i64,
    pub asset_id: i64,
    pub file_name: String,
    /// Absolute path for `convertFileSrc` in the webview.
    pub file_path: String,
    pub mime_type: String,
    pub file_size_bytes: i64,
    pub caption: Option<String>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "asset_photos row decode failed for column '{column}': {e}"
    ))
}

fn map_row(row: &QueryResult, app_data_dir: &Path) -> AppResult<AssetPhoto> {
    let relative_path: String = row.try_get("", "relative_path").map_err(|e| decode_err("relative_path", e))?;
    let abs: PathBuf = app_data_dir.join(&relative_path);
    let file_path = abs.to_string_lossy().to_string();

    Ok(AssetPhoto {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        asset_id: row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?,
        file_name: row.try_get("", "file_name").map_err(|e| decode_err("file_name", e))?,
        file_path,
        mime_type: row.try_get("", "mime_type").map_err(|e| decode_err("mime_type", e))?,
        file_size_bytes: row.try_get("", "file_size_bytes").map_err(|e| decode_err("file_size_bytes", e))?,
        caption: row
            .try_get::<Option<String>>("", "caption")
            .map_err(|e| decode_err("caption", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
    })
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid missing")))?;
    Ok(row.try_get("", "id").map_err(|e| decode_err("id", e))?)
}

fn extension_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn sanitize_file_name(path: &Path) -> AppResult<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::ValidationFailed(vec!["Invalid file name.".into()]))?;
    if name.is_empty() || name.contains("..") {
        return Err(AppError::ValidationFailed(vec!["Invalid file name.".into()]));
    }
    Ok(name.to_string())
}

pub async fn list_asset_photos(db: &DatabaseConnection, app_data_dir: &Path, asset_id: i64) -> AppResult<Vec<AssetPhoto>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, asset_id, file_name, relative_path, mime_type, file_size_bytes, \
                    caption, created_by_id, created_at \
             FROM asset_photos WHERE asset_id = ? ORDER BY id DESC",
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(|r| map_row(r, app_data_dir)).collect()
}

pub async fn upload_asset_photo(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    payload: UploadAssetPhotoPayload,
    created_by_id: i64,
) -> AppResult<AssetPhoto> {
    let exists = db
        .query_one(
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
                [payload.asset_id.into()],
            ),
        )
        .await?
        .is_some();
    if !exists {
        return Err(AppError::NotFound {
            entity: "equipment".into(),
            id: payload.asset_id.to_string(),
        });
    }

    let src = Path::new(&payload.source_path);
    let file_name = sanitize_file_name(src)?;
    let Some(mime_type) = extension_mime(src) else {
        return Err(AppError::ValidationFailed(vec![
            "Image type not allowed. Use png, jpg, jpeg, webp, or gif.".into(),
        ]));
    };

    let meta = std::fs::metadata(src).map_err(|e| {
        AppError::ValidationFailed(vec![format!("Cannot read source file: {e}")])
    })?;
    if !meta.is_file() {
        return Err(AppError::ValidationFailed(vec!["Source path is not a file.".into()]));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "File exceeds {} MB limit.",
            MAX_FILE_BYTES / (1024 * 1024)
        )]));
    }

    let bytes = std::fs::read(src).map_err(|e| {
        AppError::ValidationFailed(vec![format!("Failed to read file: {e}")])
    })?;
    let size_bytes = bytes.len() as i64;

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let safe_ext = if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif") {
        ext
    } else {
        "jpg".into()
    };

    let uuid = Uuid::new_v4();
    let relative_path = format!("asset_photos/{}/{}.{}", payload.asset_id, uuid, safe_ext);

    let absolute_path = app_data_dir.join(&relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&absolute_path, &bytes)?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let caption = payload
        .caption
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_photos (asset_id, file_name, relative_path, mime_type, file_size_bytes, caption, created_by_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.asset_id.into(),
            file_name.clone().into(),
            relative_path.into(),
            mime_type.to_string().into(),
            size_bytes.into(),
            caption
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
            created_by_id.into(),
            now.clone().into(),
        ],
    ))
    .await?;

    let id = last_insert_id(db).await?;
    let row = db
        .query_one(
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, asset_id, file_name, relative_path, mime_type, file_size_bytes, \
                        caption, created_by_id, created_at \
                 FROM asset_photos WHERE id = ?",
                [id.into()],
            ),
        )
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("asset_photos insert not found")))?;

    map_row(&row, app_data_dir)
}

pub async fn delete_asset_photo(db: &DatabaseConnection, app_data_dir: &Path, photo_id: i64) -> AppResult<()> {
    let row = db
        .query_one(
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT relative_path FROM asset_photos WHERE id = ?",
                [photo_id.into()],
            ),
        )
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "asset_photo".into(),
            id: photo_id.to_string(),
        })?;

    let relative_path: String = row.try_get("", "relative_path").map_err(|e| decode_err("relative_path", e))?;
    let abs = app_data_dir.join(&relative_path);
    let _ = std::fs::remove_file(&abs);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM asset_photos WHERE id = ?",
        [photo_id.into()],
    ))
    .await?;

    Ok(())
}
