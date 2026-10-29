// ADR-003: the field app's IPC wrapper for its enrolment and removal. `redeem_mobile_enrolment`
// (src-tauri/src/commands/phone_enrolment.rs) and `get_phone_removal_status`
// (src-tauri/src/commands/mobile.rs) exist only for the Android field app; no desktop service
// wraps them.
//
// The phone joins its plant with the QR code or typed code its administrator issued, never with
// the licence key. The engine sends the redemption with the phone's own identity; the token, the
// code and the device token never come back.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";

import { invokeSilent } from "@/lib/ipc-invoke";

/** Exactly one: the scanned QR text, or the code typed instead (XXXX-XXXX, case-insensitive). */
export type RedeemMobileEnrolmentInput =
  | { qr_payload: string; code?: never }
  | { code: string; qr_payload?: never };

const MobileEnrolmentRedeemedSchema = z.object({
  tenant_id: z.string(),
  company_display_name: z.string().nullable(),
  person_sync_id: z.string(),
  person_label: z.string(),
  site_sync_id: z.string(),
  site_label: z.string(),
  enrolled_at: z.string(),
});

export type MobileEnrolmentRedeemed = z.infer<typeof MobileEnrolmentRedeemedSchema>;

/**
 * Stable refusal codes of `redeem_mobile_enrolment` (the error's `code` in `details`; the server's
 * own code, when there was one, is the `server_code` parameter).
 */
export const ENROLMENT_ERROR_CODES = [
  "ENROLMENT_INPUT_REQUIRED",
  "ENROLMENT_QR_INVALID",
  "ENROLMENT_QR_OTHER_SERVER",
  "ENROLMENT_CODE_INVALID",
  "ENROLMENT_DESKTOP_REFUSED",
  "ENROLMENT_ALREADY_ACTIVATED",
  "ENROLMENT_PHONE_NOT_EMPTY",
  "ENROLMENT_REMOVAL_IN_PROGRESS",
  "ENROLMENT_DEVICE_ID_UNAVAILABLE",
  "ENROLMENT_INVALID",
  "ENROLMENT_ALREADY_ENROLLED",
  "ENROLMENT_WIPE_PENDING",
  "ENROLMENT_LICENCE_REVOKED",
  "ENROLMENT_LICENCE_EXPIRED",
  "ENROLMENT_LICENCE_INVALID",
  "ENROLMENT_NOT_LICENSED",
  "ENROLMENT_NO_FREE_SLOT",
  "ENROLMENT_DEVICE_CLASS_MISMATCH",
  "ENROLMENT_RATE_LIMITED",
  "ENROLMENT_OFFLINE",
  "ENROLMENT_SERVER_UNAVAILABLE",
  "ENROLMENT_SERVER_REFUSED",
  "ENROLMENT_INVALID_ANSWER",
] as const;

export type EnrolmentErrorCode = (typeof ENROLMENT_ERROR_CODES)[number];

/**
 * Sign-in refusals on a phone (`login`): another person than the enrolled one (parameter
 * `person_label`: "Ce téléphone est attribué à …"), a phone activated with the licence key that
 * now resets for an enrolment, a phone erasing itself.
 */
export const PHONE_SIGN_IN_ERROR_CODES = [
  "PHONE_ASSIGNED_TO_OTHER_PERSON",
  "PHONE_NOT_ENROLLED",
  "PHONE_REMOVAL_IN_PROGRESS",
] as const;

export type PhoneSignInErrorCode = (typeof PHONE_SIGN_IN_ERROR_CODES)[number];

/** Enrols this phone (Android only; needs no session). Refusals carry an `EnrolmentErrorCode`. */
export async function redeemMobileEnrolment(
  input: RedeemMobileEnrolmentInput,
): Promise<MobileEnrolmentRedeemed> {
  const raw = await invokeSilent<unknown>("redeem_mobile_enrolment", { input });
  return MobileEnrolmentRedeemedSchema.parse(raw);
}

const SyncWorkerErrorSchema = z.object({
  code: z.string(),
  kind: z.string(),
  message: z.string(),
  http_status: z.number().nullable(),
  at: z.string(),
});

/**
 * - `none`: no removal.
 * - `uploading`: the pending work is being sent.
 * - `waiting_to_upload`: the upload failed (`last_error`); data kept, phone locked, retried.
 * - `erasing`: the local data is being erased.
 * - `done`: erased; the phone shows "Scanner le QR code" again.
 */
export const PhoneRemovalPhaseSchema = z.enum([
  "none",
  "uploading",
  "waiting_to_upload",
  "erasing",
  "done",
]);

/**
 * - `removed`: removed by its administrator (uploads, erases, confirms).
 * - `revoked`: reported lost, or removed too long ago: erases without uploading.
 * - `unbound`: activated with the licence key before enrolment existed: uploads, then resets.
 */
export const PhoneRemovalKindSchema = z.enum(["unbound", "removed", "revoked"]);

const PhoneRemovalStatusSchema = z.object({
  phase: PhoneRemovalPhaseSchema,
  kind: PhoneRemovalKindSchema.nullable(),
  uploaded_count: z.number(),
  dropped_count: z.number(),
  /** Per entity type, plus `media_files`. */
  dropped_by_kind: z.record(z.string(), z.number()),
  last_error: SyncWorkerErrorSchema.nullable(),
  /** `removed` only, once `done`: whether the server took the wipe confirmation. */
  wipe_confirmed: z.boolean().nullable(),
});

export type PhoneRemovalStatus = z.infer<typeof PhoneRemovalStatusSchema>;

/** Event carrying every change of the removal status. */
export const PHONE_REMOVAL_STATUS_EVENT = "phone-removal-status";

/** Where this phone's removal stands (needs no session). */
export async function getPhoneRemovalStatus(): Promise<PhoneRemovalStatus> {
  const raw = await invokeSilent<unknown>("get_phone_removal_status");
  return PhoneRemovalStatusSchema.parse(raw);
}

/** Calls `onStatus` at each change of the removal status; returns the unsubscribe function. */
export async function onPhoneRemovalStatus(
  onStatus: (status: PhoneRemovalStatus) => void,
): Promise<UnlistenFn> {
  return listen<unknown>(PHONE_REMOVAL_STATUS_EVENT, (event) => {
    const parsed = PhoneRemovalStatusSchema.safeParse(event.payload);
    if (parsed.success) onStatus(parsed.data);
  });
}
