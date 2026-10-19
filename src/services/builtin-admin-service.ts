// ADR-003: all invoke() calls live exclusively in src/services/.
//
// The built-in "admin" account (`auth::builtin_admin`): whether the session of this PC is that
// account, whether another administrator exists, and whether its password was ever changed. The
// shell warns that account to create a named administrator.

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

/** The warning is shown while the built-in account is signed in and is the only administrator. */
export function builtinAdminWarningVisible(warning: BuiltinAdminWarning | null): boolean {
  return (
    warning !== null && warning.signed_in_as_builtin_admin && !warning.other_administrator_exists
  );
}
