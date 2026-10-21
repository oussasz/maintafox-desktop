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

/// Folder of the desktop updater public key (master plan Phase 5.1). The owner creates the key pair
/// with `scripts/release/setup-updater-key.ps1`, which writes the public key there; the private key
/// never leaves the owner's machine and the GitHub secret `TAURI_SIGNING_PRIVATE_KEY`.
const UPDATER_KEY_DIR: &str = "updater";
const UPDATER_PUBLIC_KEY_FILE: &str = "maintafox-updater.pub";

/// The updater public key (base64 text printed by `tauri signer generate`), or None when the owner
/// has not created it yet: that build has no in-app updates (it says so) instead of a fake key.
fn updater_public_key(manifest_dir: &Path) -> Option<String> {
    let dir = manifest_dir.join(UPDATER_KEY_DIR);
    // The folder (with its README) always exists, so watching it does not rebuild every time.
    println!("cargo:rerun-if-changed={}", dir.display());
    let path = dir.join(UPDATER_PUBLIC_KEY_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let key = text.trim();
    assert!(
        !key.is_empty() && !key.contains(char::is_whitespace),
        "{} must hold only the one-line public key written by `tauri signer generate`",
        path.display()
    );
    Some(key.to_string())
}

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .expect("src-tauri has a parent directory")
        .to_path_buf();
    let url = control_plane_api_base_url(&root);
    println!("cargo:rustc-env=MAINTAFOX_CONTROL_PLANE_API_BASE_URL={url}");
    match updater_public_key(manifest_dir) {
        Some(key) => println!("cargo:rustc-env=MAINTAFOX_UPDATER_PUBKEY={key}"),
        None => println!(
            "cargo:warning=No updater public key ({UPDATER_KEY_DIR}/{UPDATER_PUBLIC_KEY_FILE}): this build has no in-app updates. The owner creates it with scripts/release/setup-updater-key.ps1."
        ),
    }
    tauri_build::build()
}
