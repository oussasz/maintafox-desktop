import { invoke } from "@tauri-apps/api/core";

import type { AppInfoResponse, HealthCheckResponse, TaskStatusEntry } from "@shared/ipc-types";

/**
 * Health check — confirms the IPC bridge and DB are live.
 * Called once during startup sequence listening.
 */
export async function healthCheck(): Promise<HealthCheckResponse> {
  return invoke<HealthCheckResponse>("health_check");
}

/**
 * Returns application build info and runtime environment.
 */
export async function getAppInfo(): Promise<AppInfoResponse> {
  return invoke<AppInfoResponse>("get_app_info");
}

/**
 * Returns the status of all tracked background tasks.
 * Returns an empty array in Phase 1 (no tasks spawned yet).
 */
export async function getTaskStatus(): Promise<TaskStatusEntry[]> {
  return invoke<TaskStatusEntry[]>("get_task_status");
}
