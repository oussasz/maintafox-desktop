// ADR-003 compliant: all IPC calls for authentication go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for auth operations.

import { z } from "zod";

import { invoke, invokeSilent } from "@/lib/ipc-invoke";
import type {
  ClearPinInput,
  LoginRequest,
  LoginResponse,
  PasswordRecoveryContact,
  PinUnlockInput,
  SessionInfo,
  SetPinInput,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas for runtime validation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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
  idle_timeout_minutes: z.number().nullable().default(null),
  password_expires_in_days: z.number().nullable().default(null),
  pin_configured: z.boolean().nullable().default(null),
  tenant_id: z.string().nullable().default(null),
  token_tenant_id: z.string().nullable().default(null),
});

const loginResponseSchema = z.object({
  session_info: sessionInfoSchema,
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
  // Session polling should not create global toast side effects; UI layers decide how to surface.
  const raw = await invokeSilent<unknown>("get_session_info");
  return sessionInfoSchema.parse(raw);
}

/**
 * The user worked in the app (keyboard, mouse, touch): the only call that keeps the session
 * from idle-locking. Refused with `SESSION_LOCKED` once the idle deadline has passed (the session
 * then stays locked until unlocked). Returns the session info with the new activity time.
 * Silent: the caller decides how a refusal is shown.
 */
export async function touchSession(): Promise<SessionInfo> {
  const raw = await invokeSilent<unknown>("touch_session");
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

const passwordRequirementsSchema = z.object({
  min_length: z.number().int().positive(),
  require_uppercase: z.boolean(),
  require_lowercase: z.boolean(),
  require_digit: z.boolean(),
  require_special: z.boolean(),
});

export type PasswordRequirements = z.infer<typeof passwordRequirementsSchema>;

/**
 * What a new password must satisfy (the plant's password policy). Readable by any signed-in user,
 * also while a password change is required: the change screen shows it.
 */
export async function getPasswordRequirements(): Promise<PasswordRequirements> {
  const raw = await invokeSilent<unknown>("get_password_requirements");
  return passwordRequirementsSchema.parse(raw);
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

const passwordRecoveryContactSchema = z.object({
  display_name: z.string(),
  email: z.string().nullable(),
  phone: z.string().nullable(),
});

/**
 * Pre-login list of elevated administrators for Forgot Password helpdesk contacts.
 * Uses invokeSilent so AUTH failures never open AuthLockLayer on the login screen.
 */
export async function getPasswordRecoveryContacts(): Promise<PasswordRecoveryContact[]> {
  const raw = await invokeSilent<unknown>("get_password_recovery_contacts");
  return z.array(passwordRecoveryContactSchema).parse(raw);
}
