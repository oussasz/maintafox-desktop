// ADR-003 compliant: all IPC calls for user profile operations go through this file.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for user profile operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  ChangePasswordInput,
  SessionHistoryEntry,
  UpdateProfileInput,
  UserProfile,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────

const UserProfileSchema = z.object({
  id: z.number(),
  username: z.string(),
  display_name: z.string().nullable(),
  email: z.string().nullable(),
  phone: z.string().nullable(),
  language: z.string().nullable(),
  identity_mode: z.string(),
  created_at: z.string(),
  password_changed_at: z.string().nullable(),
  pin_configured: z.boolean(),
  role_name: z.string().nullable(),
});

const SessionHistorySchema = z.object({
  id: z.union([z.number(), z.string()]),
  device_label: z.string().nullable(),
  started_at: z.string(),
  ended_at: z.string().nullable(),
  duration_seconds: z.number().nullable(),
  status: z.string(),
});

// ── Service functions ─────────────────────────────────────────────────────

export async function getMyProfile(): Promise<UserProfile> {
  const raw = await invoke<unknown>("get_my_profile");
  return UserProfileSchema.parse(raw);
}

export async function updateMyProfile(input: UpdateProfileInput): Promise<UserProfile> {
  const raw = await invoke<unknown>("update_my_profile", { payload: input });
  return UserProfileSchema.parse(raw);
}

export async function changePassword(input: ChangePasswordInput): Promise<void> {
  await invoke<void>("change_password", { payload: input });
}

export async function getSessionHistory(limit?: number): Promise<SessionHistoryEntry[]> {
  const raw = await invoke<unknown[]>("get_session_history", { limit: limit ?? 10 });
  return z.array(SessionHistorySchema).parse(raw);
}
