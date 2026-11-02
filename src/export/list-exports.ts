/**
 * Row builders of the "Exporter (Excel/CSV)" buttons of the main lists.
 *
 * Each builder turns the rows a list shows into the cells of the file, with the same labels the
 * screen shows (status, type, urgency, equipment...): the caller passes the resolvers it already
 * uses for display, so the export never carries raw ids or codes. Pure functions, no I/O.
 */

import type { CsvCell } from "@/export/csv";
import type { InventoryArticleRow } from "@/services/inventory-service";
import type { WorkPermitRecord } from "@/services/permit-service";
import type { AssetSearchResult, InterventionRequest, WorkOrder } from "@shared/ipc-types";

/** The text of an empty cell: nothing, not a dash. */
const EMPTY = "";

function text(value: string | null | undefined): string {
  return value?.trim() ? value : EMPTY;
}

// ── Work orders ─────────────────────────────────────────────────────────────

export interface WorkOrderExportLabels {
  statusLabel: (code: string) => string;
  lifecycleLabel: (code: string) => string | null;
  typeLabel: (code: string, fallback: string | null | undefined) => string;
  /** ISO date to the date the screen shows. */
  formatDate: (iso: string) => string;
}

export function workOrderExportRows(
  items: Pick<
    WorkOrder,
    | "code"
    | "title"
    | "asset_label"
    | "type_code"
    | "type_label"
    | "status_code"
    | "lifecycle_state"
    | "responsible_display_name"
    | "responsible_username"
    | "planned_end"
  >[],
  labels: WorkOrderExportLabels,
): CsvCell[][] {
  return items.map((wo) => [
    wo.code,
    wo.title,
    text(wo.asset_label),
    text(labels.typeLabel(wo.type_code, wo.type_label)),
    labels.statusLabel(wo.status_code ?? "draft"),
    text(wo.lifecycle_state ? labels.lifecycleLabel(wo.lifecycle_state) : null),
    text(wo.responsible_display_name ?? wo.responsible_username),
    wo.planned_end ? labels.formatDate(wo.planned_end) : EMPTY,
  ]);
}

// ── Requests (DI) ───────────────────────────────────────────────────────────

export interface RequestExportLabels {
  statusLabel: (status: InterventionRequest["status"]) => string;
  urgencyLabel: (code: string) => string;
  slaLabel: (state: string) => string;
  personLabel: (name: string | null | undefined) => string;
  formatDate: (iso: string) => string;
}

export function requestExportRows(
  items: Pick<
    InterventionRequest,
    | "code"
    | "title"
    | "asset_label"
    | "asset_code"
    | "status"
    | "validated_urgency"
    | "reported_urgency"
    | "sla_state"
    | "submitter_display_name"
    | "submitted_at"
    | "converted_to_wo_code"
  >[],
  labels: RequestExportLabels,
): CsvCell[][] {
  return items.map((di) => [
    di.code,
    di.title,
    text(di.asset_label ?? di.asset_code),
    labels.statusLabel(di.status),
    labels.urgencyLabel(di.validated_urgency ?? di.reported_urgency),
    di.sla_state && di.sla_state !== "completed" ? labels.slaLabel(di.sla_state) : EMPTY,
    text(labels.personLabel(di.submitter_display_name)),
    di.submitted_at ? labels.formatDate(di.submitted_at) : EMPTY,
    text(di.converted_to_wo_code),
  ]);
}

// ── Equipment ───────────────────────────────────────────────────────────────

export interface AssetExportLabels {
  statusLabel: (code: string) => string;
  criticalityLabel: (code: string) => string;
  /** Meter reading as the screen shows it (number in the French format, then the unit). */
  formatReading: (value: number, unit: string | null) => string;
}

export function assetExportRows(
  items: Pick<
    AssetSearchResult,
    | "asset_code"
    | "asset_name"
    | "class_name"
    | "criticality_code"
    | "status_code"
    | "org_path"
    | "org_node_name"
    | "parent_asset_code"
    | "primary_meter_name"
    | "primary_meter_reading"
    | "primary_meter_unit"
  >[],
  labels: AssetExportLabels,
): CsvCell[][] {
  return items.map((a) => [
    a.asset_code,
    a.asset_name,
    text(a.class_name),
    a.criticality_code?.trim() ? labels.criticalityLabel(a.criticality_code) : EMPTY,
    labels.statusLabel(a.status_code),
    text(a.org_path ?? a.org_node_name),
    text(a.parent_asset_code),
    a.primary_meter_reading != null
      ? labels.formatReading(a.primary_meter_reading, a.primary_meter_unit)
      : EMPTY,
  ]);
}

// ── Stock articles ──────────────────────────────────────────────────────────

export interface ArticleExportLabels {
  formatQty: (value: number) => string;
  formatCurrency: (value: number) => string;
  belowReorderLabel: string;
  activeLabel: string;
  inactiveLabel: string;
  isBelowReorder: (article: InventoryArticleRow) => boolean;
}

export function articleExportRows(
  items: InventoryArticleRow[],
  labels: ArticleExportLabels,
): CsvCell[][] {
  return items.map((a) => {
    let status = labels.inactiveLabel;
    if (a.is_active === 1) {
      status = labels.isBelowReorder(a) ? labels.belowReorderLabel : labels.activeLabel;
    }
    return [
      a.article_code,
      a.article_name,
      text(a.family_name ?? a.family_code),
      `${labels.formatQty(a.on_hand_qty)} ${a.unit_code}`.trim(),
      labels.formatQty(a.reserved_qty),
      labels.formatQty(a.available_qty),
      a.avg_unit_cost != null ? labels.formatCurrency(a.avg_unit_cost) : EMPTY,
      a.stock_value != null ? labels.formatCurrency(a.stock_value) : EMPTY,
      a.reorder_point > 0 ? labels.formatQty(a.reorder_point) : EMPTY,
      status,
    ];
  });
}

// ── Work permits ────────────────────────────────────────────────────────────

export interface PermitExportLabels {
  statusLabel: (code: string) => string;
  typeLabel: (code: string | null, name: string | null) => string;
  assetLabel: (permit: WorkPermitRecord) => string | null;
  formatDateTime: (iso: string) => string;
}

export function permitExportRows(
  items: WorkPermitRecord[],
  labels: PermitExportLabels,
): CsvCell[][] {
  return items.map((p) => [
    p.code,
    labels.statusLabel(p.status),
    p.permit_type_name == null
      ? EMPTY
      : text(labels.typeLabel(p.permit_type_code, p.permit_type_name)),
    text(labels.assetLabel(p)),
    text(p.linked_work_order_code),
    p.expires_at ? labels.formatDateTime(p.expires_at) : EMPTY,
  ]);
}
