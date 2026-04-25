import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const ArchiveItemSummarySchema = z.object({
  id: z.number(),
  source_module: z.string(),
  source_record_id: z.string(),
  archive_class: z.string(),
  source_state: z.string().nullable(),
  archive_reason_code: z.string(),
  archived_at: z.string(),
  archived_by_id: z.number().nullable(),
  retention_policy_id: z.number().nullable(),
  restore_policy: z.string(),
  restore_until_at: z.string().nullable(),
  legal_hold: z.boolean(),
  checksum_sha256: z.string().nullable(),
  search_text: z.string().nullable(),
});

const ArchiveActionSchema = z.object({
  id: z.number(),
  archive_item_id: z.number(),
  action: z.string(),
  action_by_id: z.number().nullable(),
  action_at: z.string(),
  reason_note: z.string().nullable(),
  result_status: z.string(),
});

const RetentionPolicySchema = z.object({
  id: z.number(),
  module_code: z.string(),
  archive_class: z.string(),
  retention_years: z.number(),
  purge_mode: z.string(),
  allow_restore: z.boolean(),
  allow_purge: z.boolean(),
  requires_legal_hold_check: z.boolean(),
});

const ArchivePayloadRowSchema = z.object({
  id: z.number(),
  archive_item_id: z.number(),
  payload_json: z.unknown(),
  workflow_history_json: z.string().nullable(),
  attachment_manifest_json: z.string().nullable(),
  config_version_refs_json: z.string().nullable(),
  payload_size_bytes: z.number(),
});

const ArchiveItemDetailSchema = z.object({
  item: ArchiveItemSummarySchema,
  payload: ArchivePayloadRowSchema.nullable(),
  actions: z.array(ArchiveActionSchema),
  retention_policy: RetentionPolicySchema.nullable(),
  checksum_valid: z.boolean(),
});

const ArchiveRestoreResultSchema = z.object({
  archive_item_id: z.number(),
  restore_action_id: z.number(),
  message: z.string(),
});

const ExportPayloadSchema = z.object({
  items: z.array(
    z.object({
      archive_item_id: z.number(),
      source_module: z.string(),
      source_record_id: z.string(),
      archive_class: z.string(),
      payload_json: z.unknown(),
    }),
  ),
});

const PurgeResultSchema = z.object({
  strict_mode: z.boolean(),
  purged_item_ids: z.array(z.number()),
  blocked_items: z.array(
    z.object({
      archive_item_id: z.number(),
      reason: z.string(),
    }),
  ),
});

export type ArchiveItemSummary = z.infer<typeof ArchiveItemSummarySchema>;
export type ArchiveActionRow = z.infer<typeof ArchiveActionSchema>;
export type RetentionPolicy = z.infer<typeof RetentionPolicySchema>;
export type ArchivePayloadRow = z.infer<typeof ArchivePayloadRowSchema>;
export type ArchiveItemDetail = z.infer<typeof ArchiveItemDetailSchema>;
export type ArchiveRestoreResult = z.infer<typeof ArchiveRestoreResultSchema>;
export type ExportPayload = z.infer<typeof ExportPayloadSchema>;
export type PurgeResult = z.infer<typeof PurgeResultSchema>;

export interface ArchiveFilterInput {
  source_module?: string;
  archive_class?: string;
  legal_hold?: boolean;
  search_text?: string;
  date_from?: string;
  date_to?: string;
  limit?: number;
  offset?: number;
}

export interface RestoreInput {
  archive_item_id: number;
  reason_note: string;
}

export interface ExportInput {
  archive_item_ids: number[];
  export_reason?: string;
}

export interface PurgeInput {
  archive_item_ids: number[];
  purge_reason: string;
}

export interface LegalHoldInput {
  archive_item_id: number;
  enable: boolean;
  reason_note: string;
}

export interface UpdateRetentionInput {
  policy_id: number;
  retention_years?: number;
  purge_mode?: string;
  allow_restore?: boolean;
  allow_purge?: boolean;
  requires_legal_hold_check?: boolean;
}

export async function listArchiveItems(filter: ArchiveFilterInput): Promise<ArchiveItemSummary[]> {
  const raw = await invoke<ArchiveItemSummary[]>("list_archive_items", { filter });
  return z.array(ArchiveItemSummarySchema).parse(raw);
}

export async function getArchiveItem(archive_item_id: number): Promise<ArchiveItemDetail> {
  const raw = await invoke<ArchiveItemDetail>("get_archive_item", { archive_item_id });
  return ArchiveItemDetailSchema.parse(raw);
}

export async function restoreArchiveItem(payload: RestoreInput): Promise<ArchiveRestoreResult> {
  const raw = await invoke<ArchiveRestoreResult>("restore_archive_item", { payload });
  return ArchiveRestoreResultSchema.parse(raw);
}

export async function exportArchiveItems(payload: ExportInput): Promise<ExportPayload> {
  const raw = await invoke<ExportPayload>("export_archive_items", { payload });
  return ExportPayloadSchema.parse(raw);
}

export async function purgeArchiveItems(payload: PurgeInput): Promise<PurgeResult> {
  const raw = await invoke<PurgeResult>("purge_archive_items", { payload });
  return PurgeResultSchema.parse(raw);
}

export async function setLegalHold(payload: LegalHoldInput): Promise<void> {
  await invoke<void>("set_legal_hold", { payload });
}

export async function listRetentionPolicies(): Promise<RetentionPolicy[]> {
  const raw = await invoke<RetentionPolicy[]>("list_retention_policies");
  return z.array(RetentionPolicySchema).parse(raw);
}

export async function updateRetentionPolicy(payload: UpdateRetentionInput): Promise<void> {
  await invoke<void>("update_retention_policy", { payload });
}
