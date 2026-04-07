/**
 * asset-lifecycle-service.ts
 *
 * IPC wrappers for asset lifecycle events, meter readings, and document links.
 * All invoke() calls for SP02-F02 operations are isolated here.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  AssetLifecycleEvent,
  RecordLifecycleEventPayload,
  AssetMeter,
  CreateAssetMeterPayload,
  MeterReading,
  RecordMeterReadingPayload,
  AssetDocumentLink,
  UpsertDocumentLinkPayload,
  AssetBindingSummary,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

export const AssetLifecycleEventSchema = z.object({
  id: z.number(),
  sync_id: z.string(),
  asset_id: z.number(),
  event_type: z.string(),
  event_at: z.string(),
  from_org_node_id: z.number().nullable(),
  to_org_node_id: z.number().nullable(),
  from_status_code: z.string().nullable(),
  to_status_code: z.string().nullable(),
  from_class_code: z.string().nullable(),
  to_class_code: z.string().nullable(),
  related_asset_id: z.number().nullable(),
  reason_code: z.string().nullable(),
  notes: z.string().nullable(),
  approved_by_id: z.number().nullable(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

export const AssetMeterSchema = z.object({
  id: z.number(),
  sync_id: z.string(),
  asset_id: z.number(),
  name: z.string(),
  meter_code: z.string().nullable(),
  meter_type: z.string(),
  unit: z.string().nullable(),
  current_reading: z.number(),
  last_read_at: z.string().nullable(),
  expected_rate_per_day: z.number().nullable(),
  rollover_value: z.number().nullable(),
  is_primary: z.boolean(),
  is_active: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const MeterReadingSchema = z.object({
  id: z.number(),
  meter_id: z.number(),
  reading_value: z.number(),
  reading_at: z.string(),
  source_type: z.string(),
  source_reference: z.string().nullable(),
  quality_flag: z.string(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

export const AssetDocumentLinkSchema = z.object({
  id: z.number(),
  asset_id: z.number(),
  document_ref: z.string(),
  link_purpose: z.string(),
  is_primary: z.boolean(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

// ── Lifecycle event commands ──────────────────────────────────────────────────

export async function listAssetLifecycleEvents(
  assetId: number,
  limit?: number | null,
): Promise<AssetLifecycleEvent[]> {
  const raw = await invoke<unknown>("list_asset_lifecycle_events", {
    assetId,
    limit: limit ?? null,
  });
  return z.array(AssetLifecycleEventSchema).parse(raw) as AssetLifecycleEvent[];
}

export async function recordLifecycleEvent(
  payload: RecordLifecycleEventPayload,
): Promise<AssetLifecycleEvent> {
  const raw = await invoke<unknown>("record_lifecycle_event", { payload });
  return AssetLifecycleEventSchema.parse(raw) as AssetLifecycleEvent;
}

// ── Meter commands ────────────────────────────────────────────────────────────

export async function listAssetMeters(assetId: number): Promise<AssetMeter[]> {
  const raw = await invoke<unknown>("list_asset_meters", { assetId });
  return z.array(AssetMeterSchema).parse(raw) as AssetMeter[];
}

export async function createAssetMeter(payload: CreateAssetMeterPayload): Promise<AssetMeter> {
  const raw = await invoke<unknown>("create_asset_meter", { payload });
  return AssetMeterSchema.parse(raw) as AssetMeter;
}

export async function recordMeterReading(
  payload: RecordMeterReadingPayload,
): Promise<MeterReading> {
  const raw = await invoke<unknown>("record_meter_reading", { payload });
  return MeterReadingSchema.parse(raw) as MeterReading;
}

export async function getLatestMeterValue(meterId: number): Promise<MeterReading | null> {
  const raw = await invoke<unknown>("get_latest_meter_value", { meterId });
  if (raw === null) return null;
  return MeterReadingSchema.parse(raw) as MeterReading;
}

export async function listMeterReadings(
  meterId: number,
  limit?: number | null,
): Promise<MeterReading[]> {
  const raw = await invoke<unknown>("list_meter_readings", {
    meterId,
    limit: limit ?? null,
  });
  return z.array(MeterReadingSchema).parse(raw) as MeterReading[];
}

// ── Document link commands ────────────────────────────────────────────────────

export async function listAssetDocumentLinks(
  assetId: number,
  includeExpired?: boolean,
): Promise<AssetDocumentLink[]> {
  const raw = await invoke<unknown>("list_asset_document_links", {
    assetId,
    includeExpired: includeExpired ?? false,
  });
  return z.array(AssetDocumentLinkSchema).parse(raw) as AssetDocumentLink[];
}

export async function upsertAssetDocumentLink(
  payload: UpsertDocumentLinkPayload,
): Promise<AssetDocumentLink> {
  const raw = await invoke<unknown>("upsert_asset_document_link", { payload });
  return AssetDocumentLinkSchema.parse(raw) as AssetDocumentLink;
}

export async function expireAssetDocumentLink(
  linkId: number,
  validTo?: string | null,
): Promise<AssetDocumentLink> {
  const raw = await invoke<unknown>("expire_asset_document_link", {
    linkId,
    validTo: validTo ?? null,
  });
  return AssetDocumentLinkSchema.parse(raw) as AssetDocumentLink;
}

// ── Binding summary commands ──────────────────────────────────────────────────

export const DomainBindingEntrySchema = z.object({
  status: z.enum(["available", "not_implemented"]),
  count: z.number().nullable(),
});

export const AssetBindingSummarySchema = z.object({
  asset_id: z.number(),
  linked_di_count: DomainBindingEntrySchema,
  linked_wo_count: DomainBindingEntrySchema,
  linked_pm_plan_count: DomainBindingEntrySchema,
  linked_failure_event_count: DomainBindingEntrySchema,
  linked_document_count: DomainBindingEntrySchema,
  linked_iot_signal_count: DomainBindingEntrySchema,
  linked_erp_mapping_count: DomainBindingEntrySchema,
});

export async function getAssetBindingSummary(assetId: number): Promise<AssetBindingSummary> {
  const raw = await invoke<unknown>("get_asset_binding_summary", { assetId });
  return AssetBindingSummarySchema.parse(raw) as AssetBindingSummary;
}
