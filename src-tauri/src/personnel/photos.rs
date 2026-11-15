//! Personnel photo management — simpler variant of `assets/photos.rs`.
//!
//! Copies source file to `app_data_dir/personnel_photos/{personnel_id}/`
//! and updates `personnel.photo_path`.
//! Preview bytes are served via IPC (`data:` URL) — same rationale as asset photos
//! (avoid `convertFileSrc` / asset-protocol WebView edge cases).

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as B64_ENGINE, Engine as _};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024; // 5 MB

/// Inline preview for the webview (`data:` URL).
#[derive(Debug, Clone, Serialize)]
pub struct PersonnelPhotoPreview {
    pub mime_type: String,
    pub data_base64: String,
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
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "GATE_PERS_PHOTO_FILE_NAME_INVALID: Nom de fichier invalide.".into()
        ])
    })?;
    if name.is_empty() || name.contains("..") {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PERS_PHOTO_FILE_NAME_INVALID: Nom de fichier invalide.".into(),
        ]));
    }
    Ok(name.to_string())
}

fn ensure_path_under_app_data(file: &Path, app_data_dir: &Path) -> AppResult<()> {
    let root = app_data_dir
        .canonicalize()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("canonicalize app_data_dir: {e}")))?;
    let abs = file.canonicalize().map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_PERS_PHOTO_PATH_UNRESOLVED: Impossible de résoudre le chemin photo : {e}"
        )])
    })?;
    if !abs.starts_with(&root) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PERS_PHOTO_PATH_OUTSIDE_DATA: Chemin photo hors du répertoire de données de l'application.".into(),
        ]));
    }
    Ok(())
}

/// Read personnel photo bytes for inline webview preview.
pub async fn read_personnel_photo_preview(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    personnel_id: i64,
) -> AppResult<PersonnelPhotoPreview> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT photo_path FROM personnel WHERE id = ? LIMIT 1",
            [personnel_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: personnel_id.to_string(),
        })?;

    let photo_path: Option<String> = row
        .try_get("", "photo_path")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("photo_path decode: {e}")))?;

    let Some(photo_path) = photo_path.filter(|p| !p.trim().is_empty()) else {
        return Err(AppError::NotFound {
            entity: "personnel_photo".into(),
            id: personnel_id.to_string(),
        });
    };

    let abs = PathBuf::from(&photo_path);
    ensure_path_under_app_data(&abs, app_data_dir)?;

    if !abs.is_file() {
        tracing::warn!(
            personnel_id,
            path = %abs.display(),
            "personnel photo preview: file missing on disk"
        );
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PERS_PHOTO_FILE_MISSING: Fichier photo introuvable : {}",
            abs.display()
        )]));
    }

    let mime_type = extension_mime(&abs).unwrap_or("application/octet-stream").to_string();

    let abs_for_read = abs.clone();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(abs_for_read))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("read task join: {e}")))?
        .map_err(|e| {
            tracing::warn!(path = %abs.display(), error = %e, "personnel photo preview: read failed");
            AppError::ValidationFailed(vec![format!(
                "GATE_PERS_PHOTO_READ_FAILED: Impossible de lire le fichier photo : {e}"
            )])
        })?;

    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PERS_PHOTO_PREVIEW_TOO_LARGE: La photo dépasse la limite de taille.".into(),
        ]));
    }

    Ok(PersonnelPhotoPreview {
        mime_type,
        data_base64: B64_ENGINE.encode(bytes),
    })
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
            "GATE_PERS_PHOTO_TYPE_NOT_ALLOWED: Type d'image non autorisé. Utilisez png, jpg, jpeg, webp ou gif.".into(),
        ]));
    };

    let meta = std::fs::metadata(src).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_PERS_PHOTO_READ_FAILED: Impossible de lire le fichier source : {e}"
        )])
    })?;
    if !meta.is_file() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PERS_PHOTO_SOURCE_NOT_FILE: Le chemin source n'est pas un fichier.".into(),
        ]));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PERS_PHOTO_TOO_LARGE:limit_mb={0}: Le fichier dépasse la limite de {0} Mo.",
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
        AppError::ValidationFailed(vec![format!(
            "GATE_PERS_PHOTO_READ_FAILED: Impossible de lire le fichier : {e}"
        )])
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

    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel SET photo_path = ?, updated_at = ? WHERE id = ?",
        [abs_str.clone().into(), now.into(), personnel_id.into()],
    ))
    .await?;

    Ok(abs_str)
}
