/**
 * asset-lifecycle-service.ts
 *
 * IPC wrappers for asset lifecycle events, meter readings, and document links.
 * All invoke() calls for SP02-F02 operations are isolated here.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  Asset,
  AssetBindingSummary,
  AssetDocumentLink,
  AssetLifecycleEvent,
  AssetMeter,
  AssetPhoto,
  AssetPhotoPreview,
  CreateAssetMeterPayload,
  DecommissionAssetPayload,
  MeterReading,
  RecordLifecycleEventPayload,
  RecordMeterReadingPayload,
  UploadAssetPhotoPayload,
  UpsertDocumentLinkPayload,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Lifecycle event commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Meter commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Document link commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Binding summary commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Decommission commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DecommissionResultSchema = z
  .object({
    id: z.number(),
    sync_id: z.string(),
    asset_code: z.string(),
    asset_name: z.string(),
    status_code: z.string(),
    decommissioned_at: z.string().nullable(),
    row_version: z.number(),
  })
  .passthrough();

export async function decommissionAsset(payload: DecommissionAssetPayload): Promise<Asset> {
  const raw = await invoke<unknown>("decommission_asset", { payload });
  return DecommissionResultSchema.parse(raw) as unknown as Asset;
}

// â”€â”€ Photo commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export const AssetPhotoSchema = z.object({
  id: z.number(),
  asset_id: z.number(),
  file_name: z.string(),
  file_path: z.string(),
  mime_type: z.string(),
  file_size_bytes: z.number(),
  caption: z.string().nullable(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

export const AssetPhotoPreviewSchema = z.object({
  mime_type: z.string(),
  data_base64: z.string(),
});

export async function listAssetPhotos(assetId: number): Promise<AssetPhoto[]> {
  const raw = await invoke<unknown>("list_asset_photos", { assetId });
  return z.array(AssetPhotoSchema).parse(raw) as AssetPhoto[];
}

export async function uploadAssetPhoto(payload: UploadAssetPhotoPayload): Promise<AssetPhoto> {
  const raw = await invoke<unknown>("upload_asset_photo", { payload });
  return AssetPhotoSchema.parse(raw) as AssetPhoto;
}

export async function readAssetPhotoPreview(photoId: number): Promise<AssetPhotoPreview> {
  const raw = await invoke<unknown>("read_asset_photo_preview", { photoId });
  return AssetPhotoPreviewSchema.parse(raw) as AssetPhotoPreview;
}

export async function deleteAssetPhoto(photoId: number): Promise<void> {
  await invoke<unknown>("delete_asset_photo", { photoId });
}
