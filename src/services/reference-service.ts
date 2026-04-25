// ADR-003: all invoke() calls live exclusively in src/services/.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  ReferenceDomain,
  CreateReferenceDomainPayload,
  UpdateReferenceDomainPayload,
  ReferenceSet,
  ReferenceValue,
  CreateReferenceValuePayload,
  UpdateReferenceValuePayload,
  ReferenceValueMigration,
  ReferenceUsageMigrationResult,
  ReferenceAlias,
  CreateReferenceAliasPayload,
  UpdateReferenceAliasPayload,
  RefImportBatchSummary,
  RefImportPreview,
  ImportRowInput,
  RefImportApplyPolicy,
  RefImportApplyResult,
  RefExportResult,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export const ReferenceDomainSchema = z.object({
  id: z.number().int(),
  code: z.string().min(1),
  name: z.string().min(1),
  structure_type: z.string().min(1),
  governance_level: z.string().min(1),
  is_extendable: z.boolean(),
  validation_rules_json: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const ReferenceSetSchema = z.object({
  id: z.number().int(),
  domain_id: z.number().int(),
  version_no: z.number().int(),
  status: z.string().min(1),
  effective_from: z.string().nullable(),
  created_by_id: z.number().int().nullable(),
  created_at: z.string(),
  published_at: z.string().nullable(),
});

export const ReferenceValueSchema = z.object({
  id: z.number().int(),
  set_id: z.number().int(),
  parent_id: z.number().int().nullable(),
  code: z.string().min(1),
  label: z.string().min(1),
  description: z.string().nullable(),
  sort_order: z.number().int().nullable(),
  color_hex: z.string().nullable(),
  icon_name: z.string().nullable(),
  semantic_tag: z.string().nullable(),
  external_code: z.string().nullable(),
  is_active: z.boolean(),
  metadata_json: z.string().nullable(),
});

export const ReferenceValueMigrationSchema = z.object({
  id: z.number().int(),
  domain_id: z.number().int(),
  from_value_id: z.number().int(),
  to_value_id: z.number().int(),
  reason_code: z.string().nullable(),
  migrated_by_id: z.number().int().nullable(),
  migrated_at: z.string(),
});

export const ReferenceUsageMigrationResultSchema = z.object({
  migration: ReferenceValueMigrationSchema,
  source_value: ReferenceValueSchema,
  target_value: ReferenceValueSchema,
  remapped_references: z.number().int(),
  source_deactivated: z.boolean(),
});

// â”€â”€ Domain commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listReferenceDomains(): Promise<ReferenceDomain[]> {
  const raw = await invoke<unknown[]>("list_reference_domains");
  return z.array(ReferenceDomainSchema).parse(raw);
}

/**
 * Resolve active values from the latest published set of a domain code.
 * Returns an empty array if the domain or published set does not exist.
 */
export async function listPublishedReferenceValuesByDomainCode(
  domainCode: string,
): Promise<ReferenceValue[]> {
  const code = domainCode.trim().toUpperCase();
  if (!code) return [];
  const domains = await listReferenceDomains();
  const domain = domains.find((d) => d.code.trim().toUpperCase() === code);
  if (!domain) return [];
  const sets = await listReferenceSets(domain.id);
  const published =
    sets.filter((s) => s.status === "published").sort((a, b) => b.version_no - a.version_no)[0] ??
    null;
  if (!published) return [];
  const values = await listReferenceValues(published.id);
  return values
    .filter((v) => v.is_active)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

export async function getReferenceDomain(domainId: number): Promise<ReferenceDomain> {
  const raw = await invoke<unknown>("get_reference_domain", { domainId });
  return ReferenceDomainSchema.parse(raw);
}

export async function createReferenceDomain(
  payload: CreateReferenceDomainPayload,
): Promise<ReferenceDomain> {
  const raw = await invoke<unknown>("create_reference_domain", { payload });
  return ReferenceDomainSchema.parse(raw);
}

export async function updateReferenceDomain(
  domainId: number,
  payload: UpdateReferenceDomainPayload,
): Promise<ReferenceDomain> {
  const raw = await invoke<unknown>("update_reference_domain", { domainId, payload });
  return ReferenceDomainSchema.parse(raw);
}

// â”€â”€ Set commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listReferenceSets(domainId: number): Promise<ReferenceSet[]> {
  const raw = await invoke<unknown[]>("list_reference_sets", { domainId });
  return z.array(ReferenceSetSchema).parse(raw);
}

export async function getReferenceSet(setId: number): Promise<ReferenceSet> {
  const raw = await invoke<unknown>("get_reference_set", { setId });
  return ReferenceSetSchema.parse(raw);
}

export async function createDraftReferenceSet(domainId: number): Promise<ReferenceSet> {
  const raw = await invoke<unknown>("create_draft_reference_set", { domainId });
  return ReferenceSetSchema.parse(raw);
}

export async function validateReferenceSet(setId: number): Promise<ReferenceSet> {
  const raw = await invoke<unknown>("validate_reference_set", { setId });
  return ReferenceSetSchema.parse(raw);
}

export async function publishReferenceSet(setId: number): Promise<ReferenceSet> {
  const raw = await invoke<unknown>("publish_reference_set", { setId });
  return ReferenceSetSchema.parse(raw);
}

// â”€â”€ Value commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listReferenceValues(setId: number): Promise<ReferenceValue[]> {
  const raw = await invoke<unknown[]>("list_reference_values", { setId });
  return z.array(ReferenceValueSchema).parse(raw);
}

export async function getReferenceValue(valueId: number): Promise<ReferenceValue> {
  const raw = await invoke<unknown>("get_reference_value", { valueId });
  return ReferenceValueSchema.parse(raw);
}

export async function createReferenceValue(
  payload: CreateReferenceValuePayload,
): Promise<ReferenceValue> {
  const raw = await invoke<unknown>("create_reference_value", { payload });
  return ReferenceValueSchema.parse(raw);
}

export async function updateReferenceValue(
  valueId: number,
  payload: UpdateReferenceValuePayload,
): Promise<ReferenceValue> {
  const raw = await invoke<unknown>("update_reference_value", { valueId, payload });
  return ReferenceValueSchema.parse(raw);
}

export async function deactivateReferenceValue(valueId: number): Promise<ReferenceValue> {
  const raw = await invoke<unknown>("deactivate_reference_value", { valueId });
  return ReferenceValueSchema.parse(raw);
}

export async function moveReferenceValueParent(
  valueId: number,
  newParentId: number | null,
): Promise<ReferenceValue> {
  const raw = await invoke<unknown>("move_reference_value_parent", { valueId, newParentId });
  return ReferenceValueSchema.parse(raw);
}

// â”€â”€ Migration commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function mergeReferenceValues(
  domainId: number,
  fromValueId: number,
  toValueId: number,
): Promise<ReferenceUsageMigrationResult> {
  const raw = await invoke<unknown>("merge_reference_values", {
    domainId,
    fromValueId,
    toValueId,
  });
  return ReferenceUsageMigrationResultSchema.parse(raw);
}

export async function migrateReferenceUsage(
  domainId: number,
  fromValueId: number,
  toValueId: number,
): Promise<ReferenceUsageMigrationResult> {
  const raw = await invoke<unknown>("migrate_reference_usage", {
    domainId,
    fromValueId,
    toValueId,
  });
  return ReferenceUsageMigrationResultSchema.parse(raw);
}

export async function listReferenceMigrations(
  domainId: number,
  limit = 50,
): Promise<ReferenceValueMigration[]> {
  const raw = await invoke<unknown[]>("list_reference_migrations", { domainId, limit });
  return z.array(ReferenceValueMigrationSchema).parse(raw);
}

// â”€â”€ Alias commands (SP03-F03-S1) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export const ReferenceAliasSchema = z.object({
  id: z.number().int(),
  reference_value_id: z.number().int(),
  alias_label: z.string().min(1),
  locale: z.string().min(1),
  alias_type: z.string().min(1),
  is_preferred: z.boolean(),
  created_at: z.string(),
});

export async function listReferenceAliases(referenceValueId: number): Promise<ReferenceAlias[]> {
  const raw = await invoke<unknown[]>("list_reference_aliases", { referenceValueId });
  return z.array(ReferenceAliasSchema).parse(raw);
}

export async function getReferenceAlias(aliasId: number): Promise<ReferenceAlias> {
  const raw = await invoke<unknown>("get_reference_alias", { aliasId });
  return ReferenceAliasSchema.parse(raw);
}

export async function createReferenceAlias(
  payload: CreateReferenceAliasPayload,
): Promise<ReferenceAlias> {
  const raw = await invoke<unknown>("create_reference_alias", { payload });
  return ReferenceAliasSchema.parse(raw);
}

export async function updateReferenceAlias(
  aliasId: number,
  payload: UpdateReferenceAliasPayload,
): Promise<ReferenceAlias> {
  const raw = await invoke<unknown>("update_reference_alias", { aliasId, payload });
  return ReferenceAliasSchema.parse(raw);
}

export async function deleteReferenceAlias(aliasId: number): Promise<void> {
  await invoke<void>("delete_reference_alias", { aliasId });
}

// â”€â”€â”€ Reference imports / exports (SP03-F03-S2) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const ImportRowMessageSchema = z.object({
  category: z.string(),
  severity: z.string(),
  message: z.string(),
});

const RefImportBatchSummarySchema = z.object({
  id: z.number(),
  domain_id: z.number(),
  source_filename: z.string(),
  source_sha256: z.string(),
  status: z.string(),
  total_rows: z.number(),
  valid_rows: z.number(),
  warning_rows: z.number(),
  error_rows: z.number(),
  initiated_by_id: z.number().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

const RefImportRowSchema = z.object({
  id: z.number(),
  batch_id: z.number(),
  row_no: z.number(),
  raw_json: z.string(),
  normalized_code: z.string().nullable(),
  validation_status: z.string(),
  messages: z.array(ImportRowMessageSchema),
  proposed_action: z.string().nullable(),
});

const RefImportPreviewSchema = z.object({
  batch: RefImportBatchSummarySchema,
  rows: z.array(RefImportRowSchema),
});

const RefImportApplyResultSchema = z.object({
  batch: RefImportBatchSummarySchema,
  created: z.number(),
  updated: z.number(),
  skipped: z.number(),
  errored: z.number(),
});

const RefExportRowSchema = z.object({
  value: ReferenceValueSchema,
  aliases: z.array(ReferenceAliasSchema),
});

const RefExportResultSchema = z.object({
  domain: ReferenceDomainSchema,
  set: ReferenceSetSchema,
  rows: z.array(RefExportRowSchema),
});

export async function createRefImportBatch(
  domainId: number,
  sourceFilename: string,
  sourceSha256: string,
): Promise<RefImportBatchSummary> {
  const raw = await invoke<unknown>("create_ref_import_batch", {
    domainId,
    sourceFilename,
    sourceSha256,
  });
  return RefImportBatchSummarySchema.parse(raw);
}

export async function stageRefImportRows(
  batchId: number,
  rows: ImportRowInput[],
): Promise<RefImportBatchSummary> {
  const raw = await invoke<unknown>("stage_ref_import_rows", { batchId, rows });
  return RefImportBatchSummarySchema.parse(raw);
}

export async function validateRefImportBatch(batchId: number): Promise<RefImportBatchSummary> {
  const raw = await invoke<unknown>("validate_ref_import_batch", { batchId });
  return RefImportBatchSummarySchema.parse(raw);
}

export async function applyRefImportBatch(
  batchId: number,
  policy: RefImportApplyPolicy,
): Promise<RefImportApplyResult> {
  const raw = await invoke<unknown>("apply_ref_import_batch", { batchId, policy });
  return RefImportApplyResultSchema.parse(raw);
}

export async function getRefImportPreview(batchId: number): Promise<RefImportPreview> {
  const raw = await invoke<unknown>("get_ref_import_preview", { batchId });
  return RefImportPreviewSchema.parse(raw);
}

export async function exportRefDomainSet(setId: number): Promise<RefExportResult> {
  const raw = await invoke<unknown>("export_ref_domain_set", { setId });
  return RefExportResultSchema.parse(raw);
}

export async function listRefImportBatches(
  domainId: number,
  statusFilter?: string,
  limit?: number,
): Promise<RefImportBatchSummary[]> {
  const raw = await invoke<unknown>("list_ref_import_batches", {
    domainId,
    statusFilter,
    limit,
  });
  return z.array(RefImportBatchSummarySchema).parse(raw);
}

// â”€â”€â”€ Reference search (SP03-F03-S3) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

import type { ReferenceSearchHit } from "../../shared/ipc-types";
import type {
  ReferencePublishReadiness,
  ReferenceImpactSummary,
  ReferencePublishResult,
} from "../../shared/ipc-types";

const ReferenceSearchHitSchema = z.object({
  value_id: z.number(),
  code: z.string(),
  label: z.string(),
  matched_text: z.string(),
  match_source: z.string(),
  alias_type: z.string().nullable(),
  rank: z.number(),
});

export async function searchReferenceValues(
  domainCode: string,
  query: string,
  locale: string,
  limit?: number,
): Promise<ReferenceSearchHit[]> {
  const raw = await invoke<unknown>("search_reference_values", {
    domainCode,
    query,
    locale,
    limit,
  });
  return z.array(ReferenceSearchHitSchema).parse(raw);
}

// â”€â”€ Publish governance (SP03-F04-S1) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const ReferencePublishIssueSchema = z.object({
  check: z.string(),
  message: z.string(),
  severity: z.string(),
});

const ReferencePublishReadinessSchema = z.object({
  set_id: z.number(),
  domain_id: z.number(),
  is_ready: z.boolean(),
  is_protected: z.boolean(),
  issues: z.array(ReferencePublishIssueSchema),
  impact_preview_required: z.boolean(),
  impact_preview_available: z.boolean(),
});

const ModuleImpactSchema = z.object({
  module: z.string(),
  status: z.string(),
  affected_count: z.number(),
  details: z.string().nullable(),
});

const ReferenceImpactSummarySchema = z.object({
  set_id: z.number(),
  domain_id: z.number(),
  domain_code: z.string(),
  total_affected: z.number(),
  dimensions: z.array(ModuleImpactSchema),
  computed_at: z.string(),
});

const ReferencePublishResultSchema = z.object({
  set: ReferenceSetSchema,
  superseded_set_id: z.number().nullable(),
  readiness: ReferencePublishReadinessSchema,
});

export async function computeRefPublishReadiness(
  setId: number,
): Promise<ReferencePublishReadiness> {
  const raw = await invoke<unknown>("compute_ref_publish_readiness", { setId });
  return ReferencePublishReadinessSchema.parse(raw);
}

export async function previewRefPublishImpact(setId: number): Promise<ReferenceImpactSummary> {
  const raw = await invoke<unknown>("preview_ref_publish_impact", { setId });
  return ReferenceImpactSummarySchema.parse(raw);
}

export async function governedPublishReferenceSet(setId: number): Promise<ReferencePublishResult> {
  const raw = await invoke<unknown>("governed_publish_reference_set", {
    setId,
  });
  return ReferencePublishResultSchema.parse(raw);
}
