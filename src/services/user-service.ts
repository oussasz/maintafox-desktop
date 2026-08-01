// ADR-003 compliant: all IPC calls for user profile operations go through this file.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for user profile operations.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  ChangePasswordInput,
  SessionHistoryEntry,
  TrustedDeviceEntry,
  UpdateProfileInput,
  UserProfile,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const UserProfileSchema = z.object({
  id: z.number(),
  username: z.string(),
  personnel_id: z.number().nullable(),
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

const TrustedDeviceSchema = z.object({
  id: z.string(),
  device_label: z.string().nullable(),
  trusted_at: z.string(),
  last_seen_at: z.string().nullable(),
  is_revoked: z.boolean(),
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

export async function listTrustedDevices(): Promise<TrustedDeviceEntry[]> {
  const raw = await invoke<unknown[]>("list_trusted_devices");
  return z.array(TrustedDeviceSchema).parse(raw);
}

export async function revokeMyDevice(deviceId: string): Promise<void> {
  await invoke<void>("revoke_my_device", { deviceId });
}
