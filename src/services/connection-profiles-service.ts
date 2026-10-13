// ADR-003: connection profile IPC goes through this file only.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const ConnectionProfileSchema = z.object({
  id: z.number().int(),
  integration_type: z.string(),
  profile_name: z.string(),
  status: z.string(),
  last_tested_at: z.string().nullable(),
  last_test_result: z.string().nullable(),
  created_at: z.string(),
});

export type ConnectionProfile = z.infer<typeof ConnectionProfileSchema>;

/**
 * List integration connection profiles (read-only).
 * Requires `adm.settings`.
 */
export async function listConnectionProfiles(): Promise<ConnectionProfile[]> {
  const raw = await invoke<unknown>("list_connection_profiles");
  return z.array(ConnectionProfileSchema).parse(raw);
}
