use std::path::Path;

/// Same variable the webview uses (`controlPlaneApiBase()` in product-license-service.ts).
const API_BASE_ENV: &str = "VITE_ADMIN_API_BASE_URL";

/// `KEY=value` from a dotenv file (comments, `export` and surrounding quotes handled like Vite).
fn dotenv_value(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (k, v) = line.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        Some(v.trim().to_string()).filter(|s| !s.is_empty())
    })
}

/// The control API URL the sync worker uses, resolved like Vite resolves it for the webview:
/// process environment, then `.env.[mode].local`, `.env.[mode]`, `.env.local`, `.env` (mode is
/// `development` for debug builds and `production` for release builds, as with `tauri dev` /
/// `tauri build`), then the shared default in `shared/control-plane.json`.
fn control_plane_api_base_url(root: &Path) -> String {
    println!("cargo:rerun-if-env-changed={API_BASE_ENV}");
    if let Some(v) = std::env::var(API_BASE_ENV).ok().filter(|v| !v.trim().is_empty()) {
        return v.trim().to_string();
    }
    let mode = if std::env::var("PROFILE").as_deref() == Ok("release") {
        "production"
    } else {
        "development"
    };
    let files = [
        format!(".env.{mode}.local"),
        format!(".env.{mode}"),
        ".env.local".to_string(),
        ".env".to_string(),
    ];
    for file in &files {
        let path = root.join(file);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
            if let Some(v) = dotenv_value(&path, API_BASE_ENV) {
                return v;
            }
        }
    }
    let shared = root.join("shared").join("control-plane.json");
    println!("cargo:rerun-if-changed={}", shared.display());
    let text = std::fs::read_to_string(&shared).unwrap_or_else(|e| panic!("cannot read {}: {e}", shared.display()));
    let json: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", shared.display()));
    json.get("defaultApiBaseUrl")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| panic!("{} has no defaultApiBaseUrl", shared.display()))
        .trim()
        .to_string()
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent directory")
        .to_path_buf();
    let url = control_plane_api_base_url(&root);
    println!("cargo:rustc-env=MAINTAFOX_CONTROL_PLANE_API_BASE_URL={url}");
    tauri_build::build()
}
