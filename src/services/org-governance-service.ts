/**
 * org-governance-service.ts
 *
 * IPC wrappers for org governance: publish validation, model publishing,
 * and the append-only org change audit timeline (SP01-F04).
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { OrgPublishValidationResult, OrgChangeEvent } from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

export const OrgValidationIssueSchema = z.object({
  code: z.string(),
  severity: z.string(),
  message: z.string(),
  related_id: z.number().nullable(),
});

export const OrgPublishValidationResultSchema = z.object({
  model_id: z.number(),
  can_publish: z.boolean(),
  issue_count: z.number(),
  blocking_count: z.number(),
  issues: z.array(OrgValidationIssueSchema),
  remap_count: z.number(),
});

export const OrgChangeEventSchema = z.object({
  id: z.number(),
  entity_kind: z.string(),
  entity_id: z.number().nullable(),
  change_type: z.string(),
  before_json: z.string().nullable(),
  after_json: z.string().nullable(),
  preview_summary_json: z.string().nullable(),
  changed_by_id: z.number().nullable(),
  changed_at: z.string(),
  requires_step_up: z.boolean(),
  apply_result: z.string(),
});

// ── Service functions ─────────────────────────────────────────────────────────

export async function validateOrgModelForPublish(
  modelId: number,
): Promise<OrgPublishValidationResult> {
  const raw = await invoke<unknown>("validate_org_model_for_publish", { modelId });
  return OrgPublishValidationResultSchema.parse(raw) as OrgPublishValidationResult;
}

export async function publishOrgModel(modelId: number): Promise<OrgPublishValidationResult> {
  const raw = await invoke<unknown>("publish_org_model", { modelId });
  return OrgPublishValidationResultSchema.parse(raw) as OrgPublishValidationResult;
}

export async function listOrgChangeEvents(
  limit?: number,
  entityKind?: string,
  entityId?: number,
): Promise<OrgChangeEvent[]> {
  const raw = await invoke<unknown>("list_org_change_events", {
    limit: limit ?? 50,
    entityKind: entityKind ?? null,
    entityId: entityId ?? null,
  });
  return z.array(OrgChangeEventSchema).parse(raw) as OrgChangeEvent[];
}
