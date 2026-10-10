//! Tenant document branding assets (logo) under `app_data_dir/tenant_branding/`.
//!
//! Preview bytes are served via IPC as base64 — same pattern as personnel/asset photos
//! (avoid `convertFileSrc` / asset-protocol WebView edge cases).

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};

use super::{get_setting, set_setting};

pub const DOCUMENT_HEADER_SETTING_KEY: &str = "documents.default_header";
pub const DOCUMENT_HEADER_SCOPE: &str = "tenant";
const BRANDING_DIR: &str = "tenant_branding";
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024; // 5 MB

/// Inline logo preview for the webview (`data:` URL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentBrandingLogoPreview {
    pub mime_type: String,
    pub data_base64: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentBrandingLogoUploadResult {
    pub relative_path: String,
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

fn ensure_path_under_app_data(file: &Path, app_data_dir: &Path) -> AppResult<()> {
    let root = app_data_dir
        .canonicalize()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("canonicalize app_data_dir: {e}")))?;
    let abs = file
        .canonicalize()
        .map_err(|e| AppError::ValidationFailed(vec![format!("Impossible de résoudre le chemin logo : {e}")]))?;
    if !abs.starts_with(&root) {
        return Err(AppError::ValidationFailed(vec![
            "Chemin logo hors du répertoire de données de l'application.".into(),
        ]));
    }
    Ok(())
}

fn empty_header_value() -> Value {
    json!({
        "version": 1,
        "content_json": { "type": "doc", "content": [{ "type": "paragraph" }] },
        "html": "",
        "logo_relative_path": Value::Null
    })
}

fn parse_header_value(raw: &str) -> Value {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => empty_header_value(),
    }
}

async fn load_header_json(db: &DatabaseConnection) -> AppResult<Value> {
    let row = get_setting(db, DOCUMENT_HEADER_SETTING_KEY, DOCUMENT_HEADER_SCOPE).await?;
    Ok(match row {
        Some(s) => parse_header_value(&s.setting_value_json),
        None => empty_header_value(),
    })
}

async fn persist_header_json(
    db: &DatabaseConnection,
    value: &Value,
    changed_by_id: i32,
    summary: &str,
) -> AppResult<()> {
    let value_json = serde_json::to_string(value)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Invalid header JSON: {e}")]))?;
    set_setting(
        db,
        DOCUMENT_HEADER_SETTING_KEY,
        DOCUMENT_HEADER_SCOPE,
        &value_json,
        Some(changed_by_id),
        summary,
    )
    .await
}

fn branding_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(BRANDING_DIR)
}

fn resolve_logo_path(app_data_dir: &Path, relative: &str) -> AppResult<PathBuf> {
    let rel = relative.replace('\\', "/");
    if rel.is_empty() || rel.contains("..") || Path::new(&rel).is_absolute() {
        return Err(AppError::ValidationFailed(vec![
            "Chemin relatif de logo invalide.".into()
        ]));
    }
    let abs = app_data_dir.join(&rel);
    // Parent must exist for canonicalize; create check uses starts_with after write.
    if abs.exists() {
        ensure_path_under_app_data(&abs, app_data_dir)?;
    } else {
        let parent = abs.parent().unwrap_or(app_data_dir);
        let parent_canon = parent
            .canonicalize()
            .map_err(|e| AppError::ValidationFailed(vec![format!("Impossible de résoudre le dossier logo : {e}")]))?;
        let root = app_data_dir
            .canonicalize()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("canonicalize app_data_dir: {e}")))?;
        if !parent_canon.starts_with(&root) {
            return Err(AppError::ValidationFailed(vec![
                "Chemin logo hors du répertoire de données de l'application.".into(),
            ]));
        }
    }
    Ok(abs)
}

fn read_preview_bytes(abs: &Path) -> AppResult<(String, String)> {
    let mime = extension_mime(abs).ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "Type d'image non autorisé. Utilisez png, jpg, jpeg, webp ou gif.".into()
        ])
    })?;
    let bytes = std::fs::read(abs)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Impossible de lire le fichier logo : {e}")]))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::ValidationFailed(vec![
            "Le logo dépasse la limite de taille.".into()
        ]));
    }
    Ok((mime.to_string(), B64.encode(bytes)))
}

fn remove_logo_files(app_data_dir: &Path) {
    let dir = branding_dir(app_data_dir);
    if !dir.exists() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("logo.") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// Upload / replace the tenant document branding logo and persist `logo_relative_path`.
pub async fn upload_document_branding_logo(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    source_path: &str,
    changed_by_id: i32,
) -> AppResult<DocumentBrandingLogoUploadResult> {
    let src = Path::new(source_path);
    let Some(mime) = extension_mime(src) else {
        return Err(AppError::ValidationFailed(vec![
            "Type d'image non autorisé. Utilisez png, jpg, jpeg, webp ou gif.".into(),
        ]));
    };

    let meta = std::fs::metadata(src)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Impossible de lire le fichier source : {e}")]))?;
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
        .unwrap_or("png")
        .to_ascii_lowercase();
    let safe_ext = if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif") {
        ext
    } else {
        "png".into()
    };

    let dir = branding_dir(app_data_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("create tenant_branding dir: {e}")))?;

    remove_logo_files(app_data_dir);

    let relative = format!("{BRANDING_DIR}/logo.{safe_ext}");
    let absolute = app_data_dir.join(&relative);
    let bytes = std::fs::read(src)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Impossible de lire le fichier : {e}")]))?;
    std::fs::write(&absolute, &bytes).map_err(|e| AppError::Internal(anyhow::anyhow!("write logo file: {e}")))?;

    let mut header = load_header_json(db).await?;
    if let Some(obj) = header.as_object_mut() {
        obj.insert("logo_relative_path".into(), Value::String(relative.clone()));
        obj.entry("version").or_insert(json!(1));
    }
    persist_header_json(db, &header, changed_by_id, "Document branding logo uploaded").await?;

    Ok(DocumentBrandingLogoUploadResult {
        relative_path: relative,
        mime_type: mime.to_string(),
        data_base64: B64.encode(bytes),
    })
}

/// Clear the tenant document branding logo file and null `logo_relative_path`.
pub async fn clear_document_branding_logo(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    changed_by_id: i32,
) -> AppResult<()> {
    let mut header = load_header_json(db).await?;
    let previous = header
        .get("logo_relative_path")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    if let Some(rel) = previous {
        if let Ok(abs) = resolve_logo_path(app_data_dir, &rel) {
            if abs.exists() {
                let _ = std::fs::remove_file(&abs);
            }
        }
    }

    remove_logo_files(app_data_dir);

    if let Some(obj) = header.as_object_mut() {
        obj.insert("logo_relative_path".into(), Value::Null);
    }
    persist_header_json(db, &header, changed_by_id, "Document branding logo cleared").await
}

/// Read the current document branding logo for inline preview.
pub async fn read_document_branding_logo_preview(
    db: &DatabaseConnection,
    app_data_dir: &Path,
) -> AppResult<Option<DocumentBrandingLogoPreview>> {
    let header = load_header_json(db).await?;
    let Some(rel) = header
        .get("logo_relative_path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return Ok(None);
    };
    if rel.trim().is_empty() {
        return Ok(None);
    }

    let abs = match resolve_logo_path(app_data_dir, &rel) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    if !abs.exists() {
        return Ok(None);
    }

    let (mime_type, data_base64) = read_preview_bytes(&abs)?;
    Ok(Some(DocumentBrandingLogoPreview {
        mime_type,
        data_base64,
        relative_path: rel,
    }))
}
