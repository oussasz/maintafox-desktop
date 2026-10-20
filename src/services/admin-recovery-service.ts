// ADR-003: all invoke() calls live exclusively in src/services/.
//
// Vendor administrator recovery code (`auth::recovery_code`, CMMS Advisor decision 2026-10-19).
// Signed out, a one-time code signed by the vendor creates exactly one new named administrator;
// every other administrator then sees a notice after sign-in, on every PC of the plant, until they
// acknowledge it. The backend alone verifies the code (signature, this PC's activation, validity,
// single use) and lets only the notice's recipient acknowledge it.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

export interface RedeemAdminRecoveryCodeInput {
  code: string;
  username: string;
  display_name: string;
  password: string;
  password_confirmation: string;
}

/** Mirrors `RedeemAdminRecoveryCodeResult` in src-tauri/src/auth/recovery_code.rs. */
export const RedeemAdminRecoveryCodeResultSchema = z.object({
  username: z.string(),
});

export type RedeemAdminRecoveryCodeResult = z.infer<typeof RedeemAdminRecoveryCodeResultSchema>;

/** Mirrors `AdminRecoveryNotice` in src-tauri/src/auth/recovery_code.rs. */
export const AdminRecoveryNoticeSchema = z.object({
  id: z.number().int(),
  created_at: z.string(),
  created_username: z.string(),
  created_display_name: z.string(),
  /** The PC where the code was redeemed (its 4-character device code). */
  redeemed_device_code: z.string(),
});

export type AdminRecoveryNotice = z.infer<typeof AdminRecoveryNoticeSchema>;

export async function redeemAdminRecoveryCode(
  input: RedeemAdminRecoveryCodeInput,
): Promise<RedeemAdminRecoveryCodeResult> {
  const raw = await invoke<unknown>("redeem_admin_recovery_code", { input });
  return RedeemAdminRecoveryCodeResultSchema.parse(raw);
}

export async function getAdminRecoveryNotices(): Promise<AdminRecoveryNotice[]> {
  const raw = await invoke<unknown>("get_admin_recovery_notices");
  return z.array(AdminRecoveryNoticeSchema).parse(raw);
}

export async function acknowledgeAdminRecoveryNotice(noticeId: number): Promise<void> {
  await invoke<void>("acknowledge_admin_recovery_notice", { noticeId });
}
