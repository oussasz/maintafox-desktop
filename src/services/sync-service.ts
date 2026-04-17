import { invoke } from "@tauri-apps/api/core";
import { z, ZodError } from "zod";

import type {
  ApplySyncBatchInput,
  ApplySyncBatchResult,
  ExecuteSyncRepairInput,
  ListOutboxFilter,
  ReplaySyncFailuresInput,
  ReplaySyncFailuresResult,
  ResolveSyncConflictInput,
  StageOutboxItemInput,
  SyncConflictFilter,
  SyncConflictRecord,
  SyncObservabilityReport,
  SyncInboxItem,
  SyncOutboxItem,
  SyncPushPayload,
  SyncRepairActionRecord,
  SyncRepairExecutionResult,
  SyncRepairPreview,
  SyncRepairPreviewInput,
  SyncReplayRun,
  SyncStateSummary,
} from "@shared/ipc-types";

const SyncOutboxItemSchema = z.object({
  id: z.number(),
  idempotency_key: z.string(),
  entity_type: z.string(),
  entity_sync_id: z.string(),
  operation: z.string(),
  row_version: z.number(),
  payload_json: z.string(),
  payload_hash: z.string(),
  status: z.string(),
  acknowledged_at: z.string().nullable(),
  rejection_code: z.string().nullable(),
  rejection_message: z.string().nullable(),
  origin_machine_id: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

const SyncInboxItemSchema = z.object({
  id: z.number(),
  server_batch_id: z.string(),
  checkpoint_token: z.string(),
  entity_type: z.string(),
  entity_sync_id: z.string(),
  operation: z.string(),
  row_version: z.number(),
  payload_json: z.string(),
  payload_hash: z.string(),
  apply_status: z.string(),
  rejection_code: z.string().nullable(),
  rejection_message: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

const SyncCheckpointSchema = z.object({
  id: z.number(),
  checkpoint_token: z.string().nullable(),
  last_idempotency_key: z.string().nullable(),
  protocol_version: z.string(),
  policy_metadata_json: z.string().nullable(),
  last_sync_at: z.string().nullable(),
  updated_at: z.string(),
});

const SyncTypedRejectionSchema = z.object({
  scope: z.string(),
  entity_sync_id: z.string(),
  operation: z.string(),
  rejection_code: z.string(),
  rejection_message: z.string(),
});

const ApplySyncBatchResultSchema = z.object({
  protocol_version: z.string(),
  checkpoint_token: z.string().nullable(),
  checkpoint_advanced: z.boolean(),
  acknowledged_count: z.number(),
  rejected_count: z.number(),
  inbound_applied_count: z.number(),
  inbound_duplicate_count: z.number(),
  typed_rejections: z.array(SyncTypedRejectionSchema),
});

const SyncPushPayloadSchema = z.object({
  protocol_version: z.string(),
  checkpoint_token: z.string().nullable(),
  outbox_batch: z.array(SyncOutboxItemSchema),
});

const SyncStateSummarySchema = z.object({
  protocol_version: z.string(),
  checkpoint: SyncCheckpointSchema.nullable(),
  pending_outbox_count: z.number(),
  rejected_outbox_count: z.number(),
  inbox_error_count: z.number(),
});

const SyncConflictRecordSchema = z.object({
  id: z.number(),
  conflict_key: z.string(),
  source_scope: z.string(),
  source_batch_id: z.string().nullable(),
  linked_outbox_id: z.number().nullable(),
  linked_inbox_id: z.number().nullable(),
  entity_type: z.string(),
  entity_sync_id: z.string(),
  operation: z.string(),
  conflict_type: z.string(),
  local_payload_json: z.string().nullable(),
  inbound_payload_json: z.string().nullable(),
  authority_side: z.string(),
  checkpoint_token: z.string().nullable(),
  auto_resolution_policy: z.string(),
  requires_operator_review: z.boolean(),
  recommended_action: z.string(),
  status: z.string(),
  resolution_action: z.string().nullable(),
  resolution_note: z.string().nullable(),
  resolved_by_id: z.number().nullable(),
  resolved_at: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const SyncReplayRunSchema = z.object({
  id: z.number(),
  replay_key: z.string(),
  mode: z.string(),
  status: z.string(),
  reason: z.string(),
  requested_by_id: z.number(),
  scope_json: z.string().nullable(),
  pre_replay_checkpoint: z.string().nullable(),
  post_replay_checkpoint: z.string().nullable(),
  result_json: z.string().nullable(),
  created_at: z.string(),
  started_at: z.string().nullable(),
  finished_at: z.string().nullable(),
});

const ReplaySyncFailuresResultSchema = z.object({
  run: SyncReplayRunSchema,
  requeued_outbox_count: z.number(),
  transitioned_conflict_count: z.number(),
  checkpoint_token_after: z.string().nullable(),
  guard_applied: z.boolean(),
});

const SyncRepairPreviewSchema = z.object({
  plan_id: z.string(),
  mode: z.string(),
  reason: z.string(),
  affected_outbox_count: z.number(),
  affected_conflict_count: z.number(),
  projected_checkpoint_token: z.string().nullable(),
  warnings: z.array(z.string()),
  requires_confirmation: z.boolean(),
  risk_level: z.string(),
});

const SyncRepairExecutionResultSchema = z.object({
  plan_id: z.string(),
  mode: z.string(),
  status: z.string(),
  requeued_outbox_count: z.number(),
  transitioned_conflict_count: z.number(),
  checkpoint_token_after: z.string().nullable(),
  executed_at: z.string(),
});

const SyncRepairActionRecordSchema = z.object({
  id: z.number(),
  plan_id: z.string(),
  mode: z.string(),
  status: z.string(),
  reason: z.string(),
  created_by_id: z.number(),
  executed_by_id: z.number().nullable(),
  scope_json: z.string().nullable(),
  preview_json: z.string().nullable(),
  result_json: z.string().nullable(),
  created_at: z.string(),
  executed_at: z.string().nullable(),
});

const SyncObservabilityReportSchema = z.object({
  metrics: z.object({
    generated_at: z.string(),
    pending_outbox_count: z.number(),
    rejected_outbox_count: z.number(),
    unresolved_conflict_count: z.number(),
    replay_runs_last_24h: z.number(),
    repair_runs_last_24h: z.number(),
    checkpoint_token: z.string().nullable(),
  }),
  alerts: z.array(
    z.object({
      code: z.string(),
      severity: z.string(),
      message: z.string(),
      runbook_url: z.string(),
    }),
  ),
  recovery_proofs: z.array(
    z.object({
      workflow: z.string(),
      reference_id: z.string(),
      failure_at: z.string(),
      recovered_at: z.string(),
      duration_seconds: z.number(),
    }),
  ),
  diagnostics_links: z.array(z.string()),
});

function formatDecodeError(scope: string, err: unknown): Error {
  if (err instanceof ZodError) {
    return new Error(`${scope} response validation failed: ${err.message}`);
  }
  return err instanceof Error ? err : new Error(String(err));
}

export async function stageOutboxItem(input: StageOutboxItemInput): Promise<SyncOutboxItem> {
  try {
    const result = await invoke<unknown>("stage_outbox_item", { input });
    return SyncOutboxItemSchema.parse(result) as SyncOutboxItem;
  } catch (err) {
    throw formatDecodeError("stage_outbox_item", err);
  }
}

export async function listOutboxItems(filter: ListOutboxFilter = {}): Promise<SyncOutboxItem[]> {
  try {
    const result = await invoke<unknown>("list_outbox_items", { filter });
    return z.array(SyncOutboxItemSchema).parse(result) as SyncOutboxItem[];
  } catch (err) {
    throw formatDecodeError("list_outbox_items", err);
  }
}

export async function getSyncPushPayload(limit?: number): Promise<SyncPushPayload> {
  try {
    const result = await invoke<unknown>("get_sync_push_payload", { limit });
    return SyncPushPayloadSchema.parse(result) as SyncPushPayload;
  } catch (err) {
    throw formatDecodeError("get_sync_push_payload", err);
  }
}

export async function applySyncBatch(input: ApplySyncBatchInput): Promise<ApplySyncBatchResult> {
  try {
    const result = await invoke<unknown>("apply_sync_batch", { input });
    return ApplySyncBatchResultSchema.parse(result) as ApplySyncBatchResult;
  } catch (err) {
    throw formatDecodeError("apply_sync_batch", err);
  }
}

export async function listInboxItems(applyStatus?: string, limit?: number): Promise<SyncInboxItem[]> {
  try {
    const result = await invoke<unknown>("list_inbox_items", {
      applyStatus: applyStatus ?? null,
      limit: limit ?? null,
    });
    return z.array(SyncInboxItemSchema).parse(result) as SyncInboxItem[];
  } catch (err) {
    throw formatDecodeError("list_inbox_items", err);
  }
}

export async function getSyncStateSummary(): Promise<SyncStateSummary> {
  try {
    const result = await invoke<unknown>("get_sync_state_summary");
    return SyncStateSummarySchema.parse(result) as SyncStateSummary;
  } catch (err) {
    throw formatDecodeError("get_sync_state_summary", err);
  }
}

export async function listSyncConflicts(filter: SyncConflictFilter = {}): Promise<SyncConflictRecord[]> {
  try {
    const result = await invoke<unknown>("list_sync_conflicts", { filter });
    return z.array(SyncConflictRecordSchema).parse(result) as SyncConflictRecord[];
  } catch (err) {
    throw formatDecodeError("list_sync_conflicts", err);
  }
}

export async function resolveSyncConflict(input: ResolveSyncConflictInput): Promise<SyncConflictRecord> {
  try {
    const result = await invoke<unknown>("resolve_sync_conflict", { input });
    return SyncConflictRecordSchema.parse(result) as SyncConflictRecord;
  } catch (err) {
    throw formatDecodeError("resolve_sync_conflict", err);
  }
}

export async function replaySyncFailures(input: ReplaySyncFailuresInput): Promise<ReplaySyncFailuresResult> {
  try {
    const result = await invoke<unknown>("replay_sync_failures", { input });
    return ReplaySyncFailuresResultSchema.parse(result) as ReplaySyncFailuresResult;
  } catch (err) {
    throw formatDecodeError("replay_sync_failures", err);
  }
}

export async function listSyncReplayRuns(limit?: number): Promise<SyncReplayRun[]> {
  try {
    const result = await invoke<unknown>("list_sync_replay_runs", { limit: limit ?? null });
    return z.array(SyncReplayRunSchema).parse(result) as SyncReplayRun[];
  } catch (err) {
    throw formatDecodeError("list_sync_replay_runs", err);
  }
}

export async function previewSyncRepair(input: SyncRepairPreviewInput): Promise<SyncRepairPreview> {
  try {
    const result = await invoke<unknown>("preview_sync_repair", { input });
    return SyncRepairPreviewSchema.parse(result) as SyncRepairPreview;
  } catch (err) {
    throw formatDecodeError("preview_sync_repair", err);
  }
}

export async function executeSyncRepair(input: ExecuteSyncRepairInput): Promise<SyncRepairExecutionResult> {
  try {
    const result = await invoke<unknown>("execute_sync_repair", { input });
    return SyncRepairExecutionResultSchema.parse(result) as SyncRepairExecutionResult;
  } catch (err) {
    throw formatDecodeError("execute_sync_repair", err);
  }
}

export async function listSyncRepairActions(limit?: number): Promise<SyncRepairActionRecord[]> {
  try {
    const result = await invoke<unknown>("list_sync_repair_actions", { limit: limit ?? null });
    return z.array(SyncRepairActionRecordSchema).parse(result) as SyncRepairActionRecord[];
  } catch (err) {
    throw formatDecodeError("list_sync_repair_actions", err);
  }
}

export async function getSyncObservabilityReport(): Promise<SyncObservabilityReport> {
  try {
    const result = await invoke<unknown>("get_sync_observability_report");
    return SyncObservabilityReportSchema.parse(result) as SyncObservabilityReport;
  } catch (err) {
    throw formatDecodeError("get_sync_observability_report", err);
  }
}
