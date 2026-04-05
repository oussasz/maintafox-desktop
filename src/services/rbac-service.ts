// ADR-003 compliant: all invoke() calls for RBAC go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for permission operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { PermissionRecord, StepUpRequest, StepUpResponse } from "@shared/ipc-types";

// ── Zod schemas for runtime validation ────────────────────────────────────────

export const PermissionRecordSchema = z.object({
  name: z.string(),
  description: z.string(),
  category: z.string(),
  is_dangerous: z.boolean(),
  requires_step_up: z.boolean(),
});

export const StepUpResponseSchema = z.object({
  success: z.boolean(),
  expires_at: z.string(),
});

// ── Service functions ─────────────────────────────────────────────────────────

/**
 * Fetch the effective permission set for the currently authenticated user.
 * Called after login to populate the `usePermissions` hook so that
 * `<PermissionGate>` can work without round-tripping for every check.
 */
export async function getMyPermissions(): Promise<PermissionRecord[]> {
  const raw = await invoke<unknown[]>("get_my_permissions");
  return z.array(PermissionRecordSchema).parse(raw);
}

/**
 * Verify the user's password for a dangerous-action step-up.
 * On success, the backend records a 120-second verification window.
 * Throws on wrong password (AUTH_ERROR) or no session (AUTH_ERROR).
 */
export async function verifyStepUp(password: string): Promise<StepUpResponse> {
  const payload: StepUpRequest = { password };
  const raw = await invoke<unknown>("verify_step_up", { payload });
  return StepUpResponseSchema.parse(raw);
}
