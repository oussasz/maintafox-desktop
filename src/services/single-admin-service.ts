// ADR-003: all invoke() calls live exclusively in src/services/.
//
// The last active administrator (`auth::last_admin`, CMMS Advisor decision 2026-10-19): whether
// the signed-in account is the only active administrator of this plant. The shell then asks for a
// second administrator (two administrators can reset each other's password). The backend refuses
// to disable or demote the last one whatever this warning shows.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

/** Mirrors `SingleAdministratorWarning` in src-tauri/src/auth/last_admin.rs. */
export const SingleAdministratorWarningSchema = z.object({
  sole_active_administrator: z.boolean(),
});

export type SingleAdministratorWarning = z.infer<typeof SingleAdministratorWarningSchema>;

export async function getSingleAdministratorWarning(): Promise<SingleAdministratorWarning> {
  const raw = await invoke<unknown>("get_single_administrator_warning");
  return SingleAdministratorWarningSchema.parse(raw);
}
