// ADR-003 compliant: all IPC calls for authentication go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for auth operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  ClearPinInput,
  LoginRequest,
  LoginResponse,
  PinUnlockInput,
  SessionInfo,
  SetPinInput,
} from "@shared/ipc-types";

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
  password_expires_in_days: z.number().nullable().default(null),
  pin_configured: z.boolean().nullable().default(null),
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

/**
 * Unlock an idle-locked session by verifying the user's password.
 * Returns the updated session info on success.
 */
export async function unlockSession(password: string): Promise<SessionInfo> {
  const raw = await invoke<unknown>("unlock_session", {
    payload: { password },
  });
  return sessionInfoSchema.parse(raw);
}

/**
 * Change the password for a user with force_password_change = true.
 * Returns the updated session info on success.
 */
export async function forceChangePassword(newPassword: string): Promise<SessionInfo> {
  const raw = await invoke<unknown>("force_change_password", {
    payload: { new_password: newPassword },
  });
  // The response wraps session_info
  const parsed = z.object({ session_info: sessionInfoSchema }).parse(raw);
  return parsed.session_info;
}

/**
 * Set or change the user's quick-unlock PIN.
 * Requires the current password for verification.
 */
export async function setPin(input: SetPinInput): Promise<void> {
  await invoke<void>("set_pin", { payload: input });
}

/**
 * Clear the user's PIN (disable PIN unlock).
 * Requires the current password for verification.
 */
export async function clearPin(input: ClearPinInput): Promise<void> {
  await invoke<void>("clear_pin", { payload: input });
}

/**
 * Unlock an idle-locked session using a PIN instead of the full password.
 * Only valid when the session is locked and the user has a PIN configured.
 */
export async function unlockSessionWithPin(input: PinUnlockInput): Promise<SessionInfo> {
  const raw = await invoke<unknown>("unlock_session_with_pin", { payload: input });
  return sessionInfoSchema.parse(raw);
}
