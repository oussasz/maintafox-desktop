//! Personnel photo management — simpler variant of `assets/photos.rs`.
//!
//! Copies source file to `app_data_dir/personnel_photos/{personnel_id}/`
//! and updates `personnel.photo_path`.

use std::path::{Path, PathBuf};

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024; // 5 MB

fn extension_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png"          => Some("image/png"),
        "webp"         => Some("image/webp"),
        "gif"          => Some("image/gif"),
        _ => None,
    }
}

fn sanitize_file_name(path: &Path) -> AppResult<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::ValidationFailed(vec!["Nom de fichier invalide.".into()]))?;
    if name.is_empty() || name.contains("..") {
        return Err(AppError::ValidationFailed(vec!["Nom de fichier invalide.".into()]));
    }
    Ok(name.to_string())
}

/// Upload a personnel photo and update `personnel.photo_path`.
/// Returns the absolute path stored in the record.
pub async fn upload_personnel_photo(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    personnel_id: i64,
    source_path: &str,
) -> AppResult<String> {
    // Guard: personnel must exist
    let exists: bool = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM personnel WHERE id = ? LIMIT 1",
            [personnel_id.into()],
        ))
        .await?
        .is_some();
    if !exists {
        return Err(AppError::NotFound {
            entity: "Personnel".into(),
            id: personnel_id.to_string(),
        });
    }

    let src = Path::new(source_path);
    let _file_name = sanitize_file_name(src)?;
    let Some(_mime) = extension_mime(src) else {
        return Err(AppError::ValidationFailed(vec![
            "Type d'image non autorisé. Utilisez png, jpg, jpeg, webp ou gif.".into(),
        ]));
    };

    let meta = std::fs::metadata(src).map_err(|e| {
        AppError::ValidationFailed(vec![format!("Impossible de lire le fichier source : {e}")])
    })?;
    if !meta.is_file() {
        return Err(AppError::ValidationFailed(vec![
            "Le chemin source n'est pas un fichier.".into(),
        ]));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le fichier dépasse la limite de {} Mo.",
            MAX_FILE_BYTES / (1024 * 1024)
        )]));
    }

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
    let relative = format!("personnel_photos/{personnel_id}/{uuid}.{safe_ext}");
    let absolute: PathBuf = app_data_dir.join(&relative);

    if let Some(parent) = absolute.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bytes = std::fs::read(src).map_err(|e| {
        AppError::ValidationFailed(vec![format!("Impossible de lire le fichier : {e}")])
    })?;
    std::fs::write(&absolute, &bytes)?;

    let mut abs_str = absolute.to_string_lossy().to_string();
    if cfg!(windows) {
        if let Some(stripped) = abs_str.strip_prefix(r"\\?\") {
            abs_str = stripped.to_string();
        }
    }

    tracing::info!(
        source_path,
        relative,
        absolute = %absolute.display(),
        bytes = bytes.len(),
        "personnel photo copied"
    );

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel SET photo_path = ?, updated_at = ? WHERE id = ?",
        [abs_str.clone().into(), now.into(), personnel_id.into()],
    ))
    .await?;

    Ok(abs_str)
}
