// ADR-003 compliant: all IPC calls for authentication go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for auth operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { LoginRequest, LoginResponse, SessionInfo } from "@shared/ipc-types";

// ── Zod schemas for runtime validation ────────────────────────────────────────
const sessionInfoSchema = z.object({
  is_authenticated: z.boolean(),
  is_locked: z.boolean(),
  user_id: z.number().nullable(),
  username: z.string().nullable(),
  display_name: z.string().nullable(),
  is_admin: z.boolean().nullable(),
  force_password_change: z.boolean().nullable(),
  expires_at: z.string().nullable(),
  last_activity_at: z.string().nullable(),
});

const loginResponseSchema = z.object({
  session_info: sessionInfoSchema,
});

// ── Service functions ─────────────────────────────────────────────────────────

/**
 * Attempt to log in with username and password.
 * Throws a string error message on failure (safe to display to user).
 */
export async function login(request: LoginRequest): Promise<LoginResponse> {
  const raw = await invoke<unknown>("login", { payload: request });
  return loginResponseSchema.parse(raw);
}

/**
 * Log out the current user and clear the session.
 */
export async function logout(): Promise<void> {
  await invoke<void>("logout");
}

/**
 * Get the current session info without requiring authentication.
 * Safe to call on every app startup to determine initial route.
 */
export async function getSessionInfo(): Promise<SessionInfo> {
  const raw = await invoke<unknown>("get_session_info");
  return sessionInfoSchema.parse(raw);
}
