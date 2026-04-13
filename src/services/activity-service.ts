import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const ActivityEventSummarySchema = z.object({
  id: z.number(),
  event_class: z.string(),
  event_code: z.string(),
  source_module: z.string(),
  source_record_type: z.string().nullable(),
  source_record_id: z.string().nullable(),
  entity_scope_id: z.number().nullable(),
  actor_id: z.number().nullable(),
  actor_username: z.string().nullable(),
  happened_at: z.string(),
  severity: z.string(),
  summary_json: z.unknown().nullable(),
  correlation_id: z.string().nullable(),
  visibility_scope: z.string(),
});

const ActivityEventDetailSchema = z.object({
  event: ActivityEventSummarySchema,
  correlated_events: z.array(ActivityEventSummarySchema),
  source_record_link: z.string().nullable(),
});

const SavedActivityFilterSchema = z.object({
  id: z.number(),
  user_id: z.number(),
  view_name: z.string(),
  filter_json: z.unknown(),
  is_default: z.boolean(),
});

const EventChainNodeSchema = z.object({
  table: z.string(),
  event_id: z.number(),
  happened_at: z.string(),
  event_code: z.string().nullable(),
  action_code: z.string().nullable(),
  source_module: z.string().nullable(),
  link_type: z.string().nullable(),
});

const EventChainSchema = z.object({
  events: z.array(EventChainNodeSchema),
});

const AuditEventSummarySchema = z.object({
  id: z.number(),
  action_code: z.string(),
  target_type: z.string().nullable(),
  target_id: z.string().nullable(),
  actor_id: z.number().nullable(),
  actor_username: z.string().nullable(),
  auth_context: z.string(),
  result: z.string(),
  happened_at: z.string(),
  retention_class: z.string(),
});

const AuditEventDetailSchema = z.object({
  id: z.number(),
  action_code: z.string(),
  target_type: z.string().nullable(),
  target_id: z.string().nullable(),
  actor_id: z.number().nullable(),
  actor_username: z.string().nullable(),
  auth_context: z.string(),
  result: z.string(),
  before_hash: z.string().nullable(),
  after_hash: z.string().nullable(),
  happened_at: z.string(),
  retention_class: z.string(),
  details_json: z.unknown().nullable(),
});

const ExportResultSchema = z.object({
  event_export_run_id: z.number(),
  row_count: z.number(),
  rows_json: z.unknown(),
});

export type ActivityEventSummary = z.infer<typeof ActivityEventSummarySchema>;
export type ActivityEventDetail = z.infer<typeof ActivityEventDetailSchema>;
export type SavedActivityFilter = z.infer<typeof SavedActivityFilterSchema>;
export type EventChain = z.infer<typeof EventChainSchema>;
export type AuditEventSummary = z.infer<typeof AuditEventSummarySchema>;
export type AuditEventDetail = z.infer<typeof AuditEventDetailSchema>;
export type ExportResult = z.infer<typeof ExportResultSchema>;

export interface ActivityFilter {
  event_class?: string;
  event_code?: string;
  source_module?: string;
  source_record_type?: string;
  source_record_id?: string;
  entity_scope_id?: number;
  actor_id?: number;
  severity?: string;
  date_from?: string;
  date_to?: string;
  correlation_id?: string;
  limit?: number;
  offset?: number;
}

export interface SaveFilterInput {
  view_name: string;
  filter_json: unknown;
  is_default: boolean;
}

export interface AuditFilter {
  action_code?: string;
  actor_id?: number;
  target_type?: string;
  result?: string;
  date_from?: string;
  date_to?: string;
  retention_class?: string;
  limit?: number;
  offset?: number;
}

export interface ExportAuditInput {
  filter: AuditFilter;
  export_reason: string;
}

export async function listActivityEvents(
  filter?: ActivityFilter,
): Promise<ActivityEventSummary[]> {
  const raw = await invoke<ActivityEventSummary[]>("list_activity_events", { filter });
  return z.array(ActivityEventSummarySchema).parse(raw);
}

export async function getActivityEvent(event_id: number): Promise<ActivityEventDetail> {
  const raw = await invoke<ActivityEventDetail>("get_activity_event", { event_id });
  return ActivityEventDetailSchema.parse(raw);
}

export async function saveActivityFilter(payload: SaveFilterInput): Promise<void> {
  await invoke<void>("save_activity_filter", { payload });
}

export async function listSavedActivityFilters(): Promise<SavedActivityFilter[]> {
  const raw = await invoke<SavedActivityFilter[]>("list_saved_activity_filters");
  return z.array(SavedActivityFilterSchema).parse(raw);
}

export async function getEventChain(
  root_event_id: number,
  root_table: string,
): Promise<EventChain> {
  const raw = await invoke<EventChain>("get_event_chain", {
    payload: { root_event_id, root_table },
  });
  return EventChainSchema.parse(raw);
}

export async function listAuditEvents(filter?: AuditFilter): Promise<AuditEventSummary[]> {
  const raw = await invoke<AuditEventSummary[]>("list_audit_events", { filter });
  return z.array(AuditEventSummarySchema).parse(raw);
}

export async function getAuditEvent(event_id: number): Promise<AuditEventDetail> {
  const raw = await invoke<AuditEventDetail>("get_audit_event", { event_id });
  return AuditEventDetailSchema.parse(raw);
}

export async function exportAuditLog(payload: ExportAuditInput): Promise<ExportResult> {
  const raw = await invoke<ExportResult>("export_audit_log", { payload });
  return ExportResultSchema.parse(raw);
}
