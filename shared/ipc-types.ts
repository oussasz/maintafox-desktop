// IPC contract types shared between src/ (frontend) and the Tauri command layer.
// Types defined here must be kept in sync with Rust structs in src-tauri/src/.

export interface HealthCheckResponse {
  status: "ok" | "error";
  version: string;
}
