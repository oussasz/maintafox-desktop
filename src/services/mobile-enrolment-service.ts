// Technician phones, as the desktop manages them (Rust: commands/mobile_enrolment.rs).
// All invoke() calls for phone enrolment go through this file. Every command needs `adm.users`;
// issuing and revoking also need a fresh step-up (callers wrap them in `withStepUp`).
//
// The QR payload and the typed code are secrets of a single-use enrolment: they are returned to
// the caller and never logged here.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const EnrolmentPersonSchema = z.object({
  user_id: z.number(),
  sync_id: z.string(),
  label: z.string(),
  username: z.string(),
});

const EnrolmentSiteSchema = z.object({
  node_id: z.number(),
  sync_id: z.string(),
  code: z.string(),
  label: z.string(),
});

const EnrolmentCandidatesSchema = z.object({
  people: z.array(EnrolmentPersonSchema),
  sites: z.array(EnrolmentSiteSchema),
});

const IssuedMobileEnrolmentSchema = z.object({
  enrolment_id: z.string(),
  qr_payload: z.string(),
  code: z.string(),
  issued_at: z.string(),
  expires_at: z.string(),
  person_label: z.string(),
  site_label: z.string(),
});

export const MOBILE_ENROLMENT_STATUSES = ["pending", "redeemed", "cancelled", "expired"] as const;

const MobileEnrolmentStatusSchema = z.object({
  enrolment_id: z.string(),
  status: z.enum(MOBILE_ENROLMENT_STATUSES),
  person_label: z.string(),
  site_label: z.string(),
  expires_at: z.string().nullable(),
  redeemed_at: z.string().nullable(),
});

const MobilePhoneSchema = z.object({
  device_id: z.string(),
  person_sync_id: z.string().nullable(),
  person_label: z.string().nullable(),
  site_label: z.string().nullable(),
  enrolled_by_label: z.string().nullable(),
  enrolled_at: z.string().nullable(),
  last_sync_at: z.string().nullable(),
  app_version: z.string().nullable(),
  status: z.enum(["active", "wipe_pending"]),
  revoked_at: z.string().nullable(),
  revoked_by_label: z.string().nullable(),
});

const MobilePhonesViewSchema = z.object({
  max_mobile_devices: z.number(),
  active_mobile_devices: z.number(),
  open_enrolments: z.number(),
  devices: z.array(MobilePhoneSchema),
  pending_revocations: z.array(z.string()),
});

export type EnrolmentPerson = z.infer<typeof EnrolmentPersonSchema>;
export type EnrolmentSite = z.infer<typeof EnrolmentSiteSchema>;
export type EnrolmentCandidates = z.infer<typeof EnrolmentCandidatesSchema>;
export type IssuedMobileEnrolment = z.infer<typeof IssuedMobileEnrolmentSchema>;
export type MobileEnrolmentStatus = z.infer<typeof MobileEnrolmentStatusSchema>;
export type MobilePhone = z.infer<typeof MobilePhoneSchema>;
export type MobilePhonesView = z.infer<typeof MobilePhonesViewSchema>;

export interface IssueMobileEnrolmentInput {
  user_id: number;
  site_node_id: number;
}

export interface RevokeMobilePhoneInput {
  device_id: string;
  /** The phone is lost or stolen: it uploads nothing before it is cut off. */
  lost: boolean;
}

/** People who may be given a phone, and the sites a phone can be bound to. */
export async function listMobileEnrolmentCandidates(): Promise<EnrolmentCandidates> {
  const raw = await invoke<unknown>("list_mobile_enrolment_candidates", undefined, {
    notifyOnError: false,
  });
  return EnrolmentCandidatesSchema.parse(raw);
}

/** Issues a one-person, one-site enrolment. Step-up required. */
export async function issueMobileEnrolment(
  input: IssueMobileEnrolmentInput,
): Promise<IssuedMobileEnrolment> {
  const raw = await invoke<unknown>("issue_mobile_enrolment", { input });
  return IssuedMobileEnrolmentSchema.parse(raw);
}

/** Where an enrolment stands. Polled by the QR dialog, so a failed read does not toast. */
export async function getMobileEnrolmentStatus(
  enrolmentId: string,
): Promise<MobileEnrolmentStatus> {
  const raw = await invoke<unknown>(
    "get_mobile_enrolment_status",
    { enrolmentId },
    { notifyOnError: false },
  );
  return MobileEnrolmentStatusSchema.parse(raw);
}

/** Cancels an enrolment not used yet (no step-up: it only removes access). */
export async function cancelMobileEnrolment(enrolmentId: string): Promise<MobileEnrolmentStatus> {
  const raw = await invoke<unknown>("cancel_mobile_enrolment", { enrolmentId });
  return MobileEnrolmentStatusSchema.parse(raw);
}

/** The plant's phones, the licence's phone slots and the people whose phones still wait to be revoked. */
export async function listMobilePhones(): Promise<MobilePhonesView> {
  const raw = await invoke<unknown>("list_mobile_phones", undefined, { notifyOnError: false });
  return MobilePhonesViewSchema.parse(raw);
}

/** Revokes a phone. Step-up required. */
export async function revokeMobilePhone(input: RevokeMobilePhoneInput): Promise<void> {
  await invoke<unknown>("revoke_mobile_phone", { input });
}
