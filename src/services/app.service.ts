import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { AppInfoResponse, HealthCheckResponse, TaskStatusEntry } from "@shared/ipc-types";

// â”€â”€ Zod schemas for runtime shape validation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export const HealthCheckResponseSchema = z.object({
  status: z.enum(["ok", "degraded"]),
  version: z.string().min(1),
  db_connected: z.boolean(),
  locale: z.string().min(2),
});

export const AppInfoResponseSchema = z.object({
  version: z.string().min(1),
  build_mode: z.enum(["debug", "release"]),
  os: z.string().min(1),
  arch: z.string().min(1),
  app_name: z.string().min(1),
  default_locale: z.string().min(2),
});

export const TaskStatusEntrySchema = z.object({
  id: z.string().min(1),
  status: z.enum(["running", "cancelled", "finished"]),
});

export const TaskStatusArraySchema = z.array(TaskStatusEntrySchema);

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Health check â€” confirms the IPC bridge and DB are live.
 * Called once during startup sequence listening.
 */
export async function healthCheck(): Promise<HealthCheckResponse> {
  const raw = await invoke<unknown>("health_check");
  return HealthCheckResponseSchema.parse(raw) as HealthCheckResponse;
}

/**
 * Returns application build info and runtime environment.
 */
export async function getAppInfo(): Promise<AppInfoResponse> {
  const raw = await invoke<unknown>("get_app_info");
  return AppInfoResponseSchema.parse(raw) as AppInfoResponse;
}

/**
 * Returns the status of all tracked background tasks.
 * Returns an empty array in Phase 1 (no tasks spawned yet).
 */
export async function getTaskStatus(): Promise<TaskStatusEntry[]> {
  const raw = await invoke<unknown>("get_task_status");
  return TaskStatusArraySchema.parse(raw) as TaskStatusEntry[];
}
