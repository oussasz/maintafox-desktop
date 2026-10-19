// ADR-003: all invoke() calls live exclusively in src/services/.
//
// The built-in "admin" account (`auth::builtin_admin`): whether the session of this PC is that
// account, whether another administrator exists, and whether its password was ever changed. The
// shell warns that account, as long as it is signed in, to create a named administrator and then
// disable it by hand (nothing disables it automatically once its password has been chosen).

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

/** Mirrors `BuiltinAdminWarning` in src-tauri/src/auth/builtin_admin.rs. */
export const BuiltinAdminWarningSchema = z.object({
  signed_in_as_builtin_admin: z.boolean(),
  other_administrator_exists: z.boolean(),
  password_change_pending: z.boolean(),
});

export type BuiltinAdminWarning = z.infer<typeof BuiltinAdminWarningSchema>;

/** Answers also while the forced password change of the session is pending. */
export async function getBuiltinAdminWarning(): Promise<BuiltinAdminWarning> {
  const raw = await invoke<unknown>("get_builtin_admin_warning");
  return BuiltinAdminWarningSchema.parse(raw);
}

/**
 * The warning is shown whenever the session is the built-in account (the backend answers so only
 * while that account is active and not deleted), also once another administrator exists: the
 * account then still has to be disabled.
 */
export function builtinAdminWarningVisible(warning: BuiltinAdminWarning | null): boolean {
  return warning !== null && warning.signed_in_as_builtin_admin;
}

/** Which text the warning shows (keys under `session.builtinAdmin` in the auth namespace). */
export type BuiltinAdminWarningMessage =
  | "otherAdministratorExists"
  | "passwordPending"
  | "passwordChanged";

export function builtinAdminWarningMessage(
  warning: BuiltinAdminWarning,
): BuiltinAdminWarningMessage {
  if (warning.other_administrator_exists) return "otherAdministratorExists";
  return warning.password_change_pending ? "passwordPending" : "passwordChanged";
}
