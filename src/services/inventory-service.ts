import { z, ZodError } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  ApproveInventoryCountLineInput,
  CreateInventoryCountSessionInput,
  ArticleFamily,
  CreateProcurementRequisitionInput,
  CreatePurchaseOrderFromRequisitionInput,
  CreateRepairableOrderInput,
  CreateArticleFamilyInput,
  CreateStockLocationInput,
  CreateWarehouseInput,
  GoodsReceipt,
  GoodsReceiptLine,
  InventoryArticle,
  InventoryArticleFilter,
  InventoryArticleInput,
  InventoryIssueInput,
  InventoryCountLine,
  InventoryCountSession,
  InventoryReconciliationFinding,
  InventoryReconciliationRun,
  InventoryStateEvent,
  InventoryReleaseReservationInput,
  InventoryReorderRecommendation,
  InventoryReplenishmentRecommendation,
  InventoryReserveInput,
  InventoryReturnInput,
  InventoryStockAdjustInput,
  InventoryStockBalance,
  InventoryStockFilter,
  InventoryTaxCategory,
  InventoryTaxCategoryInput,
  InventoryTransaction,
  InventoryTransactionFilter,
  InventoryTransferInput,
  ProcurementRequisition,
  ProcurementRequisitionLine,
  ProcurementSupplier,
  PurchaseOrder,
  PurchaseOrderDetail,
  PurchaseOrderLine,
  ReceiveGoodsInput,
  RepairableOrder,
  RepairableOrderDetail,
  RepairVsReplaceResult,
  ReverseInventoryCountSessionInput,
  RunInventoryReconciliationInput,
  StockImpactProjection,
  StockLocation,
  StockReservation,
  StockReservationFilter,
  TransitionProcurementRequisitionInput,
  TransitionPurchaseOrderInput,
  TransitionRepairableOrderInput,
  TransitionInventoryCountSessionInput,
  UpsertInventoryCountLineInput,
  UpdatePostingStateInput,
  PostInventoryCountSessionInput,
  UpdateArticleFamilyInput,
  UpdateStockLocationInput,
  UpdateWarehouseInput,
  ValuationCostResult,
  Warehouse,
  // Supplier types
  InventorySupplier,
  InventorySupplierInput,
  SupplierArticleSource,
  SupplierArticleSourceInput,
  SupplierContact,
  SupplierContactInput,
  SupplierPrice,
  SupplierPriceInput,
  SupplierPurchaseHistoryRow,
  SupplierScorecard,
  // Article / procurement analytics
  ArticleEquivalent,
  ArticleEquivalentInput,
  ArticlePurchaseHistoryRow,
  ProcurementDashboardSummary,
  ProcurementAlert,
  ArticleConsumptionMonth,
  ArticleRepairableHistory,
  InventoryDocumentLink,
  InventoryDocumentLinkInput,
  WoMaterialReadiness,
} from "@shared/ipc-types";

const ArticleFamilySchema = z.object({
  id: z.number(),
  code: z.string(),
  name: z.string(),
  description: z.string().nullable(),
  is_active: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const WarehouseSchema = z.object({
  id: z.number(),
  code: z.string(),
  name: z.string(),
  is_active: z.number(),
  created_at: z.string(),
});

const ValuationCostResultSchema = z.object({
  unit_cost: z.number(),
  currency_value_id: z.number(),
  source_type: z.string(),
  source_ref: z.string().nullable(),
  effective_at: z.string(),
  is_provisional: z.boolean(),
  confidence: z.string(),
});

const StockLocationSchema = z.object({
  id: z.number(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  code: z.string(),
  name: z.string(),
  is_default: z.number(),
  is_active: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  row_version: z.number(),
});

const InventoryArticleSchema = z.object({
  id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  family_id: z.number().nullable(),
  family_code: z.string().nullable(),
  family_name: z.string().nullable(),
  unit_value_id: z.number(),
  unit_code: z.string(),
  unit_label: z.string(),
  criticality_value_id: z.number().nullable(),
  criticality_code: z.string().nullable(),
  criticality_label: z.string().nullable(),
  stocking_type_value_id: z.number(),
  stocking_type_code: z.string(),
  stocking_type_label: z.string(),
  tax_category_value_id: z.number(),
  tax_category_code: z.string(),
  tax_category_label: z.string(),
  procurement_category_value_id: z.number().nullable(),
  procurement_category_code: z.string().nullable(),
  procurement_category_label: z.string().nullable(),
  preferred_warehouse_id: z.number().nullable(),
  preferred_warehouse_code: z.string().nullable(),
  preferred_warehouse_name: z.string().nullable(),
  preferred_location_id: z.number().nullable(),
  preferred_location_code: z.string().nullable(),
  preferred_location_name: z.string().nullable(),
  min_stock: z.number(),
  max_stock: z.number().nullable(),
  reorder_point: z.number(),
  safety_stock: z.number(),
  manufacturer_name: z.string().nullable(),
  manufacturer_part_number: z.string().nullable(),
  oem_part_number: z.string().nullable(),
  replenishment_policy_code: z.string().nullable(),
  economic_order_qty: z.number().nullable(),
  minimum_order_qty: z.number().nullable(),
  maximum_order_qty: z.number().nullable(),
  order_multiple_qty: z.number().nullable(),
  lead_time_days: z.number().nullable(),
  review_period_days: z.number().nullable(),
  abc_class_code: z.string().nullable(),
  xyz_class_code: z.string().nullable(),
  is_critical_spare: z.number(),
  requires_expiration: z.number(),
  shelf_life_days: z.number().nullable(),
  requires_batch_tracking: z.number(),
  is_active: z.number(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
  avg_unit_cost: z.number().nullable(),
  stock_value: z.number().nullable(),
  unvalued_qty: z.number(),
  barcode: z.string().nullable(),
  requires_serial_tracking: z.number(),
});

/**
 * Article master row as returned by `list_inventory_articles`: the item master plus its stock
 * totals summed over every location (one backend query, no per-article balance calls) and its
 * valuation — weighted-average unit cost and stock value in the tenant currency.
 */
export type InventoryArticleRow = InventoryArticle & {
  on_hand_qty: number;
  reserved_qty: number;
  available_qty: number;
  /** Weighted-average unit cost over valued locations; null when never costed. */
  avg_unit_cost: number | null;
  /** On hand × location average cost over valued locations; null when nothing is valued. */
  stock_value: number | null;
  /** On-hand quantity at locations without a cost yet (not in `stock_value`). */
  unvalued_qty: number;
  /** Barcode printed on the item / bin label (scanned at the issue desk). */
  barcode: string | null;
  /** 1 when each unit carries its own serial number. */
  requires_serial_tracking: number;
};

/** Article payload with the scan and serial-tracking fields. */
export type InventoryArticleInputFull = InventoryArticleInput & {
  barcode?: string | null;
  requires_serial_tracking?: boolean | null;
};

/** True when stock of the article is held by lot or serial number. */
export function isLotTracked(
  article: Pick<InventoryArticleRow, "requires_batch_tracking" | "requires_serial_tracking">,
): boolean {
  return article.requires_batch_tracking === 1 || article.requires_serial_tracking === 1;
}

/** True when the article has a reorder point and its available stock is at or below it. */
export function isBelowReorderPoint(
  article: Pick<InventoryArticleRow, "reorder_point" | "available_qty" | "is_active">,
): boolean {
  return (
    article.is_active === 1 &&
    article.reorder_point > 0 &&
    article.available_qty <= article.reorder_point
  );
}

const InventoryArticleInputSchema = z.object({
  article_code: z.string().trim().min(1),
  article_name: z.string().trim().min(1),
  family_id: z.number().nullable().optional(),
  unit_value_id: z.number().int().positive(),
  criticality_value_id: z.number().int().positive().nullable().optional(),
  stocking_type_value_id: z.number().int().positive(),
  tax_category_value_id: z.number().int().positive(),
  procurement_category_value_id: z.number().int().positive().nullable().optional(),
  preferred_warehouse_id: z.number().int().positive().nullable().optional(),
  preferred_location_id: z.number().int().positive().nullable().optional(),
  min_stock: z.number().min(0),
  max_stock: z.number().min(0).nullable().optional(),
  reorder_point: z.number().min(0),
  safety_stock: z.number().min(0),
  manufacturer_name: z.string().trim().nullable().optional(),
  manufacturer_part_number: z.string().trim().nullable().optional(),
  oem_part_number: z.string().trim().nullable().optional(),
  replenishment_policy_code: z.string().nullable().optional(),
  economic_order_qty: z.number().min(0).nullable().optional(),
  minimum_order_qty: z.number().min(0).nullable().optional(),
  maximum_order_qty: z.number().min(0).nullable().optional(),
  order_multiple_qty: z.number().min(0).nullable().optional(),
  lead_time_days: z.number().int().min(0).nullable().optional(),
  review_period_days: z.number().int().min(0).nullable().optional(),
  abc_class_code: z.string().nullable().optional(),
  xyz_class_code: z.string().nullable().optional(),
  is_critical_spare: z.boolean().nullable().optional(),
  requires_expiration: z.boolean().nullable().optional(),
  shelf_life_days: z.number().int().min(0).nullable().optional(),
  requires_batch_tracking: z.boolean().nullable().optional(),
  is_active: z.boolean().optional(),
  barcode: z.string().trim().nullable().optional(),
  requires_serial_tracking: z.boolean().nullable().optional(),
});

/**
 * Full update payload for an existing article. `update_inventory_article` replaces every
 * item-master column, so edits and deactivation must start from the stored values.
 */
export function articleToInput(
  article: InventoryArticle &
    Partial<Pick<InventoryArticleRow, "barcode" | "requires_serial_tracking">>,
  overrides: Partial<InventoryArticleInputFull> = {},
): InventoryArticleInputFull {
  return {
    article_code: article.article_code,
    article_name: article.article_name,
    family_id: article.family_id,
    unit_value_id: article.unit_value_id,
    criticality_value_id: article.criticality_value_id,
    stocking_type_value_id: article.stocking_type_value_id,
    tax_category_value_id: article.tax_category_value_id,
    procurement_category_value_id: article.procurement_category_value_id,
    preferred_warehouse_id: article.preferred_warehouse_id,
    preferred_location_id: article.preferred_location_id,
    min_stock: article.min_stock,
    max_stock: article.max_stock,
    reorder_point: article.reorder_point,
    safety_stock: article.safety_stock,
    manufacturer_name: article.manufacturer_name,
    manufacturer_part_number: article.manufacturer_part_number,
    oem_part_number: article.oem_part_number,
    replenishment_policy_code: article.replenishment_policy_code,
    economic_order_qty: article.economic_order_qty,
    minimum_order_qty: article.minimum_order_qty,
    maximum_order_qty: article.maximum_order_qty,
    order_multiple_qty: article.order_multiple_qty,
    lead_time_days: article.lead_time_days,
    review_period_days: article.review_period_days,
    abc_class_code: article.abc_class_code,
    xyz_class_code: article.xyz_class_code,
    is_critical_spare: article.is_critical_spare === 1,
    requires_expiration: article.requires_expiration === 1,
    shelf_life_days: article.shelf_life_days,
    requires_batch_tracking: article.requires_batch_tracking === 1,
    is_active: article.is_active === 1,
    barcode: article.barcode ?? null,
    requires_serial_tracking: article.requires_serial_tracking === 1,
    ...overrides,
  };
}

const InventoryStockBalanceSchema = z.object({
  id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  warehouse_name: z.string().nullable(),
  location_id: z.number(),
  location_code: z.string(),
  location_name: z.string().nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
  moving_avg_unit_cost: z.number().nullable(),
  updated_at: z.string(),
});

/** Stock balance of one location, with its weighted-average unit cost (tenant currency). */
export type InventoryStockBalanceRow = InventoryStockBalance & {
  moving_avg_unit_cost: number | null;
};

const InventoryTaxCategorySchema = z.object({
  id: z.number(),
  code: z.string(),
  label: z.string(),
  fr_label: z.string().nullable(),
  en_label: z.string().nullable(),
  description: z.string().nullable(),
  sort_order: z.number(),
  is_active: z.number(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const InventoryTaxCategoryInputSchema = z.object({
  code: z.string().trim().min(1),
  label: z.string().trim().min(1),
  fr_label: z.string().nullable().optional(),
  en_label: z.string().nullable().optional(),
  description: z.string().nullable().optional(),
});

const StockReservationSchema = z.object({
  id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  warehouse_name: z.string().nullable(),
  location_id: z.number(),
  location_code: z.string(),
  location_name: z.string().nullable(),
  source_type: z.string(),
  source_id: z.number().nullable(),
  source_ref: z.string().nullable(),
  work_order_code: z.string().nullable(),
  quantity_reserved: z.number(),
  quantity_issued: z.number(),
  status: z.string(),
  notes: z.string().nullable(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  released_at: z.string().nullable(),
});

const InventoryTransactionSchema = z.object({
  id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  warehouse_name: z.string().nullable(),
  location_id: z.number(),
  location_code: z.string(),
  location_name: z.string().nullable(),
  reservation_id: z.number().nullable(),
  movement_type: z.string(),
  quantity: z.number(),
  source_type: z.string(),
  source_id: z.number().nullable(),
  source_ref: z.string().nullable(),
  reason_code: z.string().nullable(),
  notes: z.string().nullable(),
  performed_by_id: z.number().nullable(),
  performed_at: z.string(),
  lot_id: z.number().nullable(),
  lot_number: z.string().nullable(),
  serial_number: z.string().nullable(),
  posting_date: z.string().nullable(),
  unit_cost: z.number().nullable(),
  extended_cost: z.number().nullable(),
});

/** Ledger movement with its lot / serial, business date and valuation. */
export type InventoryTransactionRow = InventoryTransaction & {
  lot_id: number | null;
  lot_number: string | null;
  serial_number: string | null;
  posting_date: string | null;
  unit_cost: number | null;
  extended_cost: number | null;
};

const InventoryReorderRecommendationSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  min_stock: z.number(),
  reorder_point: z.number(),
  max_stock: z.number().nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
  suggested_reorder_qty: z.number(),
  trigger_type: z.string(),
});

const ProcurementSupplierSchema = z.object({
  id: z.number(),
  company_code: z.string(),
  company_name: z.string(),
  is_active: z.number(),
});

const InventorySupplierSchema = z.object({
  id: z.number(),
  code: z.string(),
  name: z.string(),
  external_company_id: z.number().nullable(),
  status_code: z.string(),
  payment_terms_code: z.string().nullable(),
  currency_value_id: z.number().nullable(),
  currency_code: z.string().nullable(),
  incoterms_code: z.string().nullable(),
  default_lead_time_days: z.number().nullable(),
  default_buyer_person_id: z.number().nullable(),
  is_active: z.number(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

/** Supplier with its invoicing currency (null = tenant currency). */
export type InventorySupplierRow = InventorySupplier & { currency_code: string | null };
export type InventorySupplierInputFull = InventorySupplierInput & {
  currency_code?: string | null;
};

const SupplierArticleSourceSchema = z.object({
  id: z.number(),
  supplier_id: z.number(),
  supplier_code: z.string(),
  supplier_name: z.string(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  is_preferred: z.number(),
  priority: z.number(),
  lead_time_days: z.number().nullable(),
  unit_price_hint: z.number().nullable(),
  min_order_qty: z.number().nullable(),
  supplier_article_code: z.string().nullable(),
  is_active: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  last_price: z.number().nullable(),
  avg_price: z.number().nullable(),
  last_purchase_at: z.string().nullable(),
  currency_value_id: z.number().nullable(),
  currency_label: z.string().nullable(),
  risk_level: z.string().nullable(),
});

const SupplierPriceSchema = z.object({
  id: z.number(),
  supplier_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  unit_price: z.number(),
  currency_value_id: z.number().nullable(),
  price_unit_value_id: z.number().nullable(),
  min_order_qty: z.number().nullable(),
  valid_from: z.string(),
  valid_to: z.string().nullable(),
  is_active: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const SupplierRecentDeliverySchema = z.object({
  received_at: z.string().nullable(),
  po_number: z.string(),
  article_code: z.string(),
  article_name: z.string(),
  ordered_qty: z.number().nullable(),
  accepted_qty: z.number(),
  rejected_qty: z.number(),
  actual_lead_time_days: z.number().nullable(),
});

const SupplierScorecardSchema = z.object({
  supplier_id: z.number(),
  supplier_code: z.string(),
  supplier_name: z.string(),
  on_time_delivery_pct: z.number().nullable(),
  avg_lead_time_days: z.number().nullable(),
  delivery_accuracy_pct: z.number().nullable(),
  avg_price: z.number().nullable(),
  open_po_count: z.number(),
  completed_po_count: z.number(),
  last_purchase_at: z.string().nullable(),
  rejected_pct: z.number().nullable(),
  risk_level: z.string(),
  last_deliveries: z.array(SupplierRecentDeliverySchema),
});

const SupplierContactSchema = z.object({
  id: z.number(),
  supplier_id: z.number(),
  contact_name: z.string(),
  contact_role: z.string().nullable(),
  phone: z.string().nullable(),
  email: z.string().nullable(),
  is_primary: z.number(),
  created_at: z.string(),
});

const SupplierPurchaseHistoryRowSchema = z.object({
  purchase_order_id: z.number(),
  po_number: z.string(),
  ordered_at: z.string().nullable(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  ordered_qty: z.number(),
  unit_price: z.number().nullable(),
  status: z.string(),
});

const ArticleEquivalentSchema = z.object({
  id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  equivalent_article_id: z.number(),
  equivalent_code: z.string(),
  equivalent_name: z.string(),
  equivalence_type: z.string(),
  notes: z.string().nullable(),
  is_bidirectional: z.number(),
  created_at: z.string(),
});

const InventoryReplenishmentRecommendationSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  min_stock: z.number(),
  reorder_point: z.number(),
  max_stock: z.number().nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
  suggested_reorder_qty: z.number(),
  trigger_type: z.string(),
  suggestion_type: z.string(),
  suggested_supplier_id: z.number().nullable(),
  suggested_supplier_name: z.string().nullable(),
  estimated_cost: z.number().nullable(),
  expected_arrival: z.string().nullable(),
  reason: z.string().nullable(),
  transfer_options: z.array(
    z.object({
      warehouse_id: z.number(),
      warehouse_code: z.string(),
      available_qty: z.number(),
    }),
  ),
});

const ProcurementAlertSchema = z.object({
  kind: z.string(),
  severity: z.string(),
  title: z.string(),
  detail: z.string().nullable(),
  entity_type: z.string().nullable(),
  entity_id: z.number().nullable(),
  entity_code: z.string().nullable(),
});

const ArticleConsumptionMonthSchema = z.object({
  year_month: z.string(),
  issued_qty: z.number(),
});

const ArticlePurchaseHistoryRowSchema = z.object({
  purchase_order_id: z.number(),
  po_number: z.string(),
  ordered_at: z.string().nullable(),
  supplier_id: z.number().nullable(),
  supplier_name: z.string().nullable(),
  unit_price: z.number().nullable(),
  ordered_qty: z.number(),
  received_qty: z.number(),
  status: z.string(),
});

const ProcurementDashboardSummarySchema = z.object({
  open_requisitions: z.number(),
  pending_approval_pos: z.number(),
  open_pos: z.number(),
  overdue_pos: z.number(),
  pending_receipts: z.number(),
  low_stock_articles: z.number(),
  critical_low_stock_articles: z.number(),
  repairables_in_repair: z.number(),
  active_suppliers_count: z.number(),
  receiving_today_count: z.number(),
});

const ArticleRepairableHistorySchema = z.object({
  order_id: z.number(),
  order_code: z.string(),
  quantity: z.number(),
  status: z.string(),
  reason: z.string().nullable(),
  repair_cost: z.number().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

const InventoryDocumentLinkSchema = z.object({
  id: z.number(),
  entity_type: z.string(),
  entity_id: z.number(),
  document_ref: z.string(),
  link_purpose: z.string(),
  is_primary: z.number(),
  valid_from: z.string(),
  valid_to: z.string().nullable(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

const WoPartShortageSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  requested_qty: z.number(),
  available_qty: z.number(),
  shortage_qty: z.number(),
  preferred_location_id: z.number().nullable(),
});

const WoMaterialReadinessSchema = z.object({
  work_order_id: z.number(),
  work_order_code: z.string(),
  total_parts: z.number(),
  available_parts: z.number(),
  reserved_parts: z.number(),
  missing_parts: z.number(),
  ready_pct: z.number(),
  reserved_pct: z.number(),
  expected_arrival: z.string().nullable(),
  is_fully_available: z.boolean(),
  shortages: z.array(WoPartShortageSchema),
});

const ProcurementRequisitionSchema = z.object({
  id: z.number(),
  req_number: z.string(),
  demand_source_type: z.string(),
  demand_source_id: z.number().nullable(),
  demand_source_ref: z.string().nullable(),
  purchase_priority: z.string().nullable(),
  status: z.string(),
  posting_state: z.string(),
  posting_error: z.string().nullable(),
  requested_by_id: z.number().nullable(),
  requested_by_name: z.string().nullable(),
  line_count: z.number(),
  estimated_total: z.number().nullable(),
  estimate_complete: z.boolean(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

/** Requisition as listed by the backend, with the requester's name and its line totals. */
export type ProcurementRequisitionRow = ProcurementRequisition & {
  requested_by_name: string | null;
  line_count: number;
  /** Sum of qty × estimated unit cost over the estimated lines (tenant currency). */
  estimated_total: number | null;
  /** False when at least one line has no estimated cost. */
  estimate_complete: boolean;
};

const ProcurementRequisitionLineSchema = z.object({
  id: z.number(),
  requisition_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  preferred_location_id: z.number().nullable(),
  preferred_location_code: z.string().nullable(),
  requested_qty: z.number(),
  estimated_unit_cost: z.number().nullable(),
  source_reservation_id: z.number().nullable(),
  source_reorder_trigger: z.string().nullable(),
  status: z.string(),
  created_at: z.string(),
});

const PurchaseOrderSchema = z.object({
  id: z.number(),
  po_number: z.string(),
  requisition_id: z.number().nullable(),
  supplier_id: z.number().nullable(),
  supplier_name: z.string().nullable(),
  supplier_company_id: z.number().nullable(),
  supplier_company_name: z.string().nullable(),
  status: z.string(),
  posting_state: z.string(),
  posting_error: z.string().nullable(),
  ordered_by_id: z.number().nullable(),
  ordered_by_name: z.string().nullable(),
  ordered_at: z.string().nullable(),
  approved_by_id: z.number().nullable(),
  approved_at: z.string().nullable(),
  expected_delivery_date: z.string().nullable(),
  currency_code: z.string().nullable(),
  line_count: z.number(),
  total_amount: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

/** Purchase order as listed by the backend, with the buyer's name, currency and totals. */
export type PurchaseOrderRow = PurchaseOrder & {
  ordered_by_name: string | null;
  /** Currency the PO is priced in; null = tenant currency. */
  currency_code: string | null;
  line_count: number;
  /** Sum of qty × price over priced lines, in the PO currency. */
  total_amount: number | null;
};

const PurchaseOrderLineSchema = z.object({
  id: z.number(),
  purchase_order_id: z.number(),
  requisition_line_id: z.number().nullable(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  ordered_qty: z.number(),
  received_qty: z.number(),
  unit_price: z.number().nullable(),
  demand_source_type: z.string(),
  demand_source_id: z.number().nullable(),
  demand_source_ref: z.string().nullable(),
  source_reservation_id: z.number().nullable(),
  status: z.string(),
  remaining_qty: z.number(),
  line_total: z.number().nullable(),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

const StockImpactProjectionSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number().nullable(),
  warehouse_code: z.string().nullable(),
  current_on_hand: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
  incoming_open_po_qty: z.number(),
  delta_qty: z.number(),
  projected_on_hand: z.number(),
});

const GoodsReceiptSchema = z.object({
  id: z.number(),
  gr_number: z.string(),
  purchase_order_id: z.number(),
  status: z.string(),
  posting_state: z.string(),
  posting_error: z.string().nullable(),
  received_by_id: z.number().nullable(),
  received_at: z.string().nullable(),
  currency_code: z.string().nullable(),
  exchange_rate: z.number().nullable(),
  exchange_rate_source: z.string().nullable(),
  posting_date: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const GoodsReceiptLineSchema = z.object({
  id: z.number(),
  goods_receipt_id: z.number(),
  po_line_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  received_qty: z.number(),
  accepted_qty: z.number(),
  rejected_qty: z.number(),
  rejection_reason: z.string().nullable(),
  unit_price: z.number().nullable(),
  unit_cost_tenant: z.number().nullable(),
  status: z.string(),
  created_at: z.string(),
});

/** Goods receipt with the currency and the rate captured when it was posted. */
export type GoodsReceiptRow = GoodsReceipt & {
  currency_code: string | null;
  exchange_rate: number | null;
  /** TENANT_CURRENCY, ENTERED or RATE_TABLE. */
  exchange_rate_source: string | null;
  posting_date: string | null;
};
export type GoodsReceiptLineRow = GoodsReceiptLine & {
  /** PO price in the PO currency. */
  unit_price: number | null;
  /** Price converted at the receipt rate (tenant currency) — the stock cost. */
  unit_cost_tenant: number | null;
};

/** Lot or serial received on a receipt line (tracked articles). */
export interface ReceiptLotInput {
  lot_number: string | null;
  serial_number: string | null;
  expiry_date: string | null;
  quantity: number;
}

export type ReceiveGoodsLineInput = ReceiveGoodsInput["lines"][number] & {
  lots?: ReceiptLotInput[] | null;
};

export type ReceiveGoodsInputFull = Omit<ReceiveGoodsInput, "lines"> & {
  lines: ReceiveGoodsLineInput[];
  /** Rate PO currency → tenant currency; null = stored rate valid on the posting date. */
  exchange_rate?: number | null;
  /** Business date of the receipt (YYYY-MM-DD); null = today. */
  posting_date?: string | null;
};

const RepairableOrderSchema = z.object({
  id: z.number(),
  order_code: z.string(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  quantity: z.number(),
  source_location_id: z.number(),
  source_location_code: z.string(),
  return_location_id: z.number().nullable(),
  return_location_code: z.string().nullable(),
  linked_po_line_id: z.number().nullable(),
  linked_reservation_id: z.number().nullable(),
  status: z.string(),
  reason: z.string().nullable(),
  serial_number: z.string().nullable(),
  vendor_supplier_id: z.number().nullable(),
  vendor_supplier_code: z.string().nullable(),
  vendor_supplier_name: z.string().nullable(),
  sent_at: z.string().nullable(),
  returned_at: z.string().nullable(),
  warranty_active: z.number(),
  warranty_until: z.string().nullable(),
  repair_cost: z.number().nullable(),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  created_by_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const RepairableHistoryStatsSchema = z.object({
  repair_count: z.number(),
  avg_cost: z.number().nullable(),
  avg_turnaround_days: z.number().nullable(),
});

const RepairVsReplaceResultSchema = z.object({
  repair_cost: z.number().nullable(),
  replacement_cost: z.number().nullable(),
  threshold_ratio: z.number(),
  recommendation: z.enum(["REPAIR", "REPLACE", "REVIEW", "INSUFFICIENT_DATA"]),
  reason: z.string().nullable(),
});

const InventoryStateEventSchema = z.object({
  id: z.number(),
  entity_type: z.string(),
  entity_id: z.number(),
  from_status: z.string().nullable(),
  to_status: z.string(),
  actor_id: z.number().nullable(),
  reason: z.string().nullable(),
  note: z.string().nullable(),
  changed_at: z.string(),
});

const PurchaseOrderDetailSchema = z.object({
  order: PurchaseOrderSchema,
  lines: z.array(PurchaseOrderLineSchema),
  goods_receipts: z.array(GoodsReceiptSchema),
  state_events: z.array(InventoryStateEventSchema),
  document_links: z.array(InventoryDocumentLinkSchema),
  grand_total: z.number().nullable(),
  grand_total_partial: z.boolean(),
});

const RepairableOrderDetailSchema = z.object({
  order: RepairableOrderSchema,
  state_events: z.array(InventoryStateEventSchema),
  document_links: z.array(InventoryDocumentLinkSchema),
  history_stats: RepairableHistoryStatsSchema,
  repair_vs_replace: RepairVsReplaceResultSchema,
});

const InventoryCountSessionSchema = z.object({
  id: z.number(),
  session_code: z.string(),
  warehouse_id: z.number(),
  location_id: z.number().nullable(),
  status: z.string(),
  critical_abs_threshold: z.number(),
  submitted_by_id: z.number().nullable(),
  submitted_at: z.string().nullable(),
  posted_by_id: z.number().nullable(),
  posted_at: z.string().nullable(),
  reversed_by_id: z.number().nullable(),
  reversed_at: z.string().nullable(),
  reversal_reason: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const InventoryCountLineSchema = z.object({
  id: z.number(),
  session_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  location_id: z.number(),
  location_code: z.string(),
  system_qty: z.number(),
  counted_qty: z.number(),
  variance_qty: z.number(),
  variance_reason_code: z.string().nullable(),
  is_critical: z.number(),
  approval_required: z.number(),
  approved_by_id: z.number().nullable(),
  approved_by_name: z.string().nullable(),
  approved_at: z.string().nullable(),
  approval_note: z.string().nullable(),
  posted_transaction_id: z.number().nullable(),
  reversed_transaction_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

/** Count line with the display name of the reviewer who approved its variance. */
export type InventoryCountLineRow = InventoryCountLine & { approved_by_name: string | null };

const InventoryReconciliationRunSchema = z.object({
  id: z.number(),
  run_code: z.string(),
  run_date: z.string(),
  status: z.string(),
  checked_rows: z.number(),
  drift_rows: z.number(),
  checked_by_id: z.number().nullable(),
  started_at: z.string(),
  finished_at: z.string().nullable(),
});

const InventoryReconciliationFindingSchema = z.object({
  id: z.number(),
  run_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  balance_on_hand: z.number(),
  ledger_expected_on_hand: z.number(),
  drift_qty: z.number(),
  is_break: z.number(),
  created_at: z.string(),
});

interface IpcErrorShape {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcErrorShape {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class InventoryIpcError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "InventoryIpcError";
    this.code = code;
  }
}

function mapInvokeError(err: unknown): never {
  if (isIpcError(err)) throw new InventoryIpcError(err.code, err.message);
  if (err instanceof Error) throw err;
  throw new Error(String(err));
}

async function invokeParsed<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  schema: z.ZodType<T>,
): Promise<T> {
  try {
    const raw = await invoke<unknown>(command, args);
    return schema.parse(raw);
  } catch (err) {
    if (err instanceof ZodError) throw new InventoryIpcError("VALIDATION_FAILED", err.message);
    mapInvokeError(err);
  }
}

export function listInventoryArticleFamilies(): Promise<ArticleFamily[]> {
  return invokeParsed("list_inventory_article_families", undefined, z.array(ArticleFamilySchema));
}

export function createInventoryArticleFamily(
  input: CreateArticleFamilyInput,
): Promise<ArticleFamily> {
  return invokeParsed("create_inventory_article_family", { input }, ArticleFamilySchema);
}

export function updateInventoryArticleFamily(
  familyId: number,
  input: UpdateArticleFamilyInput,
): Promise<ArticleFamily> {
  return invokeParsed("update_inventory_article_family", { familyId, input }, ArticleFamilySchema);
}

export function deactivateInventoryArticleFamily(familyId: number): Promise<ArticleFamily> {
  return invokeParsed("deactivate_inventory_article_family", { familyId }, ArticleFamilySchema);
}

export function listInventoryTaxCategories(): Promise<InventoryTaxCategory[]> {
  return invokeParsed(
    "list_inventory_tax_categories",
    undefined,
    z.array(InventoryTaxCategorySchema),
  );
}

export function createInventoryTaxCategory(
  input: InventoryTaxCategoryInput,
): Promise<InventoryTaxCategory> {
  return invokeParsed(
    "create_inventory_tax_category",
    { input: InventoryTaxCategoryInputSchema.parse(input) },
    InventoryTaxCategorySchema,
  );
}

export function updateInventoryTaxCategory(
  taxCategoryId: number,
  expectedRowVersion: number,
  input: InventoryTaxCategoryInput,
): Promise<InventoryTaxCategory> {
  return invokeParsed(
    "update_inventory_tax_category",
    { taxCategoryId, expectedRowVersion, input: InventoryTaxCategoryInputSchema.parse(input) },
    InventoryTaxCategorySchema,
  );
}

export function deactivateInventoryTaxCategory(
  taxCategoryId: number,
  expectedRowVersion: number,
): Promise<InventoryTaxCategory> {
  return invokeParsed(
    "deactivate_inventory_tax_category",
    { taxCategoryId, expectedRowVersion },
    InventoryTaxCategorySchema,
  );
}

export function listInventoryWarehouses(): Promise<Warehouse[]> {
  return invokeParsed("list_inventory_warehouses", undefined, z.array(WarehouseSchema));
}

export function listInventoryLocations(warehouseId?: number | null): Promise<StockLocation[]> {
  return invokeParsed(
    "list_inventory_locations",
    { warehouseId: warehouseId ?? null },
    z.array(StockLocationSchema),
  );
}

export function createInventoryWarehouse(input: CreateWarehouseInput): Promise<Warehouse> {
  return invokeParsed("create_inventory_warehouse", { input }, WarehouseSchema);
}

export function updateInventoryWarehouse(
  warehouseId: number,
  input: UpdateWarehouseInput,
): Promise<Warehouse> {
  return invokeParsed("update_inventory_warehouse", { warehouseId, input }, WarehouseSchema);
}

export function createInventoryStockLocation(
  input: CreateStockLocationInput,
): Promise<StockLocation> {
  return invokeParsed("create_inventory_stock_location", { input }, StockLocationSchema);
}

export function updateInventoryStockLocation(
  locationId: number,
  expectedRowVersion: number,
  input: UpdateStockLocationInput,
): Promise<StockLocation> {
  return invokeParsed(
    "update_inventory_stock_location",
    { locationId, expectedRowVersion, input },
    StockLocationSchema,
  );
}

export function evaluateInventoryUnitCost(
  articleId: number,
  warehouseId: number,
  locationId: number,
): Promise<ValuationCostResult> {
  return invokeParsed(
    "evaluate_inventory_unit_cost",
    { articleId, warehouseId, locationId },
    ValuationCostResultSchema,
  );
}

export function listInventoryArticles(
  filter: InventoryArticleFilter,
): Promise<InventoryArticleRow[]> {
  return invokeParsed("list_inventory_articles", { filter }, z.array(InventoryArticleSchema));
}

export function createInventoryArticle(
  input: InventoryArticleInputFull,
): Promise<InventoryArticleRow> {
  return invokeParsed(
    "create_inventory_article",
    { input: InventoryArticleInputSchema.parse(input) },
    InventoryArticleSchema,
  );
}

export function updateInventoryArticle(
  articleId: number,
  expectedRowVersion: number,
  input: InventoryArticleInputFull,
): Promise<InventoryArticleRow> {
  return invokeParsed(
    "update_inventory_article",
    { articleId, expectedRowVersion, input: InventoryArticleInputSchema.parse(input) },
    InventoryArticleSchema,
  );
}

export function listInventoryStockBalances(
  filter: InventoryStockFilter,
): Promise<InventoryStockBalanceRow[]> {
  return invokeParsed(
    "list_inventory_stock_balances",
    { filter },
    z.array(InventoryStockBalanceSchema),
  );
}

/**
 * Manual stock movement. `unitCost` (tenant currency) is accepted on entries only: it updates the
 * location's weighted-average cost. Exits and entries without a cost are valued at the average.
 */
export function adjustInventoryStock(
  input: InventoryStockAdjustInput,
  unitCost: number | null = null,
  options: AdjustStockOptions = {},
): Promise<InventoryStockBalanceRow> {
  return invokeParsed(
    "adjust_inventory_stock",
    {
      input,
      unitCost,
      lotId: options.lotId ?? null,
      newLot: options.newLot ?? null,
      postingDate: options.postingDate ?? null,
    },
    InventoryStockBalanceSchema,
  );
}

/** Lot / serial and business date of a manual movement. */
export interface AdjustStockOptions {
  /** Existing lot to move (exits, or an entry into a known lot). */
  lotId?: number | null;
  /** New lot or serial number for an entry of a tracked article. */
  newLot?: NewLotInput | null;
  /** Back-dated movement (YYYY-MM-DD); refused inside a closed valuation period. */
  postingDate?: string | null;
}

export interface NewLotInput {
  lot_number: string | null;
  serial_number: string | null;
  expiry_date: string | null;
}

export function reserveInventoryStock(input: InventoryReserveInput): Promise<StockReservation> {
  return invokeParsed("reserve_inventory_stock", { input }, StockReservationSchema);
}

export function issueInventoryStock(input: InventoryIssueInput): Promise<StockReservation> {
  return invokeParsed("issue_inventory_stock", { input }, StockReservationSchema);
}

export function returnInventoryStock(input: InventoryReturnInput): Promise<StockReservation> {
  return invokeParsed("return_inventory_stock", { input }, StockReservationSchema);
}

export function transferInventoryStock(
  input: InventoryTransferInput,
): Promise<InventoryStockBalance[]> {
  return invokeParsed("transfer_inventory_stock", { input }, z.array(InventoryStockBalanceSchema));
}

export function releaseInventoryReservation(
  input: InventoryReleaseReservationInput,
): Promise<StockReservation> {
  return invokeParsed("release_inventory_reservation", { input }, StockReservationSchema);
}

export function listInventoryReservations(
  filter: StockReservationFilter,
): Promise<StockReservation[]> {
  return invokeParsed("list_inventory_reservations", { filter }, z.array(StockReservationSchema));
}

export function listInventoryTransactions(
  filter: InventoryTransactionFilter,
): Promise<InventoryTransactionRow[]> {
  return invokeParsed(
    "list_inventory_transactions",
    { filter },
    z.array(InventoryTransactionSchema),
  );
}

export function evaluateInventoryReorder(
  warehouseId?: number | null,
): Promise<InventoryReorderRecommendation[]> {
  return invokeParsed(
    "evaluate_inventory_reorder",
    { warehouseId: warehouseId ?? null },
    z.array(InventoryReorderRecommendationSchema),
  );
}

export function listInventoryProcurementSuppliers(): Promise<ProcurementSupplier[]> {
  return invokeParsed(
    "list_inventory_procurement_suppliers",
    undefined,
    z.array(ProcurementSupplierSchema),
  );
}

export function listInventoryProcurementRequisitions(): Promise<ProcurementRequisitionRow[]> {
  return invokeParsed(
    "list_inventory_procurement_requisitions",
    undefined,
    z.array(ProcurementRequisitionSchema),
  );
}

export type ProcurementRequisitionLineRow = ProcurementRequisitionLine & {
  /** Estimated unit cost (tenant currency); used for the approval amount. */
  estimated_unit_cost: number | null;
};

export function listInventoryProcurementRequisitionLines(
  requisitionId: number,
): Promise<ProcurementRequisitionLineRow[]> {
  return invokeParsed(
    "list_inventory_procurement_requisition_lines",
    { requisitionId },
    z.array(ProcurementRequisitionLineSchema),
  );
}

export function createInventoryProcurementRequisition(
  input: CreateProcurementRequisitionInput,
): Promise<ProcurementRequisition> {
  return invokeParsed(
    "create_inventory_procurement_requisition",
    { input },
    ProcurementRequisitionSchema,
  );
}

export function transitionInventoryProcurementRequisition(
  input: TransitionProcurementRequisitionInput,
): Promise<ProcurementRequisitionRow> {
  return invokeParsed(
    "transition_inventory_procurement_requisition",
    { input },
    ProcurementRequisitionSchema,
  );
}

export function createInventoryPurchaseOrderFromRequisition(
  input: CreatePurchaseOrderFromRequisitionInput & { currency_code?: string | null },
): Promise<PurchaseOrderRow> {
  return invokeParsed(
    "create_inventory_purchase_order_from_requisition",
    { input },
    PurchaseOrderSchema,
  );
}

export function transitionInventoryPurchaseOrder(
  input: TransitionPurchaseOrderInput,
): Promise<PurchaseOrderRow> {
  return invokeParsed("transition_inventory_purchase_order", { input }, PurchaseOrderSchema);
}

export function listInventoryPurchaseOrders(): Promise<PurchaseOrderRow[]> {
  return invokeParsed("list_inventory_purchase_orders", undefined, z.array(PurchaseOrderSchema));
}

export function listInventoryPurchaseOrderLines(
  purchaseOrderId: number,
): Promise<PurchaseOrderLine[]> {
  return invokeParsed(
    "list_inventory_purchase_order_lines",
    { purchaseOrderId },
    z.array(PurchaseOrderLineSchema),
  );
}

/** Purchase order detail with the buyer's display name on the order. */
export type PurchaseOrderDetailView = Omit<PurchaseOrderDetail, "order"> & {
  order: PurchaseOrderRow;
};

export function getInventoryPurchaseOrderDetail(
  purchaseOrderId: number,
): Promise<PurchaseOrderDetailView> {
  return invokeParsed(
    "get_inventory_purchase_order_detail",
    { purchaseOrderId },
    PurchaseOrderDetailSchema,
  );
}

export function projectInventoryStockImpact(params: {
  articleId: number;
  warehouseId?: number | null;
  deltaQty: number;
  includeOpenPoQty?: boolean;
}): Promise<StockImpactProjection> {
  return invokeParsed(
    "project_inventory_stock_impact",
    {
      articleId: params.articleId,
      warehouseId: params.warehouseId ?? null,
      deltaQty: params.deltaQty,
      includeOpenPoQty: params.includeOpenPoQty ?? false,
    },
    StockImpactProjectionSchema,
  );
}

export function receiveInventoryPurchaseOrderGoods(
  input: ReceiveGoodsInputFull,
): Promise<GoodsReceiptRow> {
  return invokeParsed("receive_inventory_purchase_order_goods", { input }, GoodsReceiptSchema);
}

export function listInventoryGoodsReceipts(): Promise<GoodsReceiptRow[]> {
  return invokeParsed("list_inventory_goods_receipts", undefined, z.array(GoodsReceiptSchema));
}

export function listInventoryGoodsReceiptLines(
  goodsReceiptId: number,
): Promise<GoodsReceiptLineRow[]> {
  return invokeParsed(
    "list_inventory_goods_receipt_lines",
    { goodsReceiptId },
    z.array(GoodsReceiptLineSchema),
  );
}

export function updateInventoryProcurementPostingState(
  input: UpdatePostingStateInput,
): Promise<void> {
  return invokeParsed("update_inventory_procurement_posting_state", { input }, z.unknown()).then(
    () => undefined,
  );
}

export function createInventoryRepairableOrder(
  input: CreateRepairableOrderInput,
): Promise<RepairableOrder> {
  return invokeParsed("create_inventory_repairable_order", { input }, RepairableOrderSchema);
}

export function transitionInventoryRepairableOrder(
  input: TransitionRepairableOrderInput,
): Promise<RepairableOrder> {
  return invokeParsed("transition_inventory_repairable_order", { input }, RepairableOrderSchema);
}

export function listInventoryRepairableOrders(): Promise<RepairableOrder[]> {
  return invokeParsed(
    "list_inventory_repairable_orders",
    undefined,
    z.array(RepairableOrderSchema),
  );
}

export function getInventoryRepairableOrderDetail(orderId: number): Promise<RepairableOrderDetail> {
  return invokeParsed(
    "get_inventory_repairable_order_detail",
    { orderId },
    RepairableOrderDetailSchema,
  );
}

export function evaluateInventoryRepairVsReplace(orderId: number): Promise<RepairVsReplaceResult> {
  return invokeParsed(
    "evaluate_inventory_repair_vs_replace",
    { orderId },
    RepairVsReplaceResultSchema,
  );
}

export function listInventoryStateEvents(
  entityType?: string | null,
  entityId?: number | null,
): Promise<InventoryStateEvent[]> {
  return invokeParsed(
    "list_inventory_state_events",
    { entityType: entityType ?? null, entityId: entityId ?? null },
    z.array(InventoryStateEventSchema),
  );
}

export function createInventoryCountSession(
  input: CreateInventoryCountSessionInput,
): Promise<InventoryCountSession> {
  return invokeParsed("create_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function transitionInventoryCountSession(
  input: TransitionInventoryCountSessionInput,
): Promise<InventoryCountSession> {
  return invokeParsed("transition_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function upsertInventoryCountLine(
  input: UpsertInventoryCountLineInput,
): Promise<InventoryCountLine> {
  return invokeParsed("upsert_inventory_count_line", { input }, InventoryCountLineSchema);
}

export function approveInventoryCountLine(
  input: ApproveInventoryCountLineInput,
): Promise<InventoryCountLine> {
  return invokeParsed("approve_inventory_count_line", { input }, InventoryCountLineSchema);
}

export function postInventoryCountSession(
  input: PostInventoryCountSessionInput,
): Promise<InventoryCountSession> {
  return invokeParsed("post_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function reverseInventoryCountSession(
  input: ReverseInventoryCountSessionInput,
): Promise<InventoryCountSession> {
  return invokeParsed("reverse_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function listInventoryCountSessions(): Promise<InventoryCountSession[]> {
  return invokeParsed(
    "list_inventory_count_sessions",
    undefined,
    z.array(InventoryCountSessionSchema),
  );
}

export function listInventoryCountLines(sessionId: number): Promise<InventoryCountLineRow[]> {
  return invokeParsed(
    "list_inventory_count_lines",
    { sessionId },
    z.array(InventoryCountLineSchema),
  );
}

export function runInventoryReconciliation(
  input: RunInventoryReconciliationInput,
): Promise<InventoryReconciliationRun> {
  return invokeParsed("run_inventory_reconciliation", { input }, InventoryReconciliationRunSchema);
}

export function listInventoryReconciliationRuns(): Promise<InventoryReconciliationRun[]> {
  return invokeParsed(
    "list_inventory_reconciliation_runs",
    undefined,
    z.array(InventoryReconciliationRunSchema),
  );
}

export function listInventoryReconciliationFindings(
  runId: number,
): Promise<InventoryReconciliationFinding[]> {
  return invokeParsed(
    "list_inventory_reconciliation_findings",
    { runId },
    z.array(InventoryReconciliationFindingSchema),
  );
}

// ── Supplier functions ────────────────────────────────────────────────────────

export function listInventorySuppliers(): Promise<InventorySupplierRow[]> {
  return invokeParsed("list_inventory_suppliers", undefined, z.array(InventorySupplierSchema));
}

export function getInventorySupplier(supplierId: number): Promise<InventorySupplierRow> {
  return invokeParsed("get_inventory_supplier", { supplierId }, InventorySupplierSchema);
}

export function upsertInventorySupplier(
  supplierId: number | null | undefined,
  expectedRowVersion: number | null | undefined,
  input: InventorySupplierInputFull,
): Promise<InventorySupplierRow> {
  return invokeParsed(
    "upsert_inventory_supplier",
    { supplierId: supplierId ?? null, expectedRowVersion: expectedRowVersion ?? null, input },
    InventorySupplierSchema,
  );
}

export function deactivateInventorySupplier(
  supplierId: number,
  expectedRowVersion: number,
): Promise<InventorySupplier> {
  return invokeParsed(
    "deactivate_inventory_supplier",
    { supplierId, expectedRowVersion },
    InventorySupplierSchema,
  );
}

export function listSupplierArticleSources(
  supplierId?: number | null,
  articleId?: number | null,
): Promise<SupplierArticleSource[]> {
  return invokeParsed(
    "list_supplier_article_sources",
    { supplierId: supplierId ?? null, articleId: articleId ?? null },
    z.array(SupplierArticleSourceSchema),
  );
}

export function upsertSupplierArticleSource(
  input: SupplierArticleSourceInput,
): Promise<SupplierArticleSource> {
  return invokeParsed("upsert_supplier_article_source", { input }, SupplierArticleSourceSchema);
}

export function deleteSupplierArticleSource(sourceId: number): Promise<void> {
  return invoke("delete_supplier_article_source", { sourceId });
}

export function listSupplierPrices(
  supplierId: number,
  articleId?: number | null,
): Promise<SupplierPrice[]> {
  return invokeParsed(
    "list_supplier_prices",
    { supplierId, articleId: articleId ?? null },
    z.array(SupplierPriceSchema),
  );
}

export function upsertSupplierPrice(
  priceId: number | null | undefined,
  input: SupplierPriceInput,
): Promise<SupplierPrice> {
  return invokeParsed(
    "upsert_supplier_price",
    { priceId: priceId ?? null, input },
    SupplierPriceSchema,
  );
}

export function getInventorySupplierScorecard(supplierId: number): Promise<SupplierScorecard> {
  return invokeParsed("get_inventory_supplier_scorecard", { supplierId }, SupplierScorecardSchema);
}

// ── Supplier contacts ────────────────────────────────────────────────────────

export function listSupplierContacts(supplierId: number): Promise<SupplierContact[]> {
  return invokeParsed("list_supplier_contacts", { supplierId }, z.array(SupplierContactSchema));
}

export function upsertSupplierContact(
  contactId: number | null | undefined,
  input: SupplierContactInput,
): Promise<SupplierContact> {
  return invokeParsed(
    "upsert_supplier_contact",
    { contactId: contactId ?? null, input },
    SupplierContactSchema,
  );
}

export function deleteSupplierContact(contactId: number): Promise<void> {
  return invoke("delete_supplier_contact", { contactId });
}

export function listSupplierPurchaseHistory(
  supplierId: number,
  limit?: number | null,
): Promise<SupplierPurchaseHistoryRow[]> {
  return invokeParsed(
    "list_supplier_purchase_history",
    { supplierId, limit: limit ?? null },
    z.array(SupplierPurchaseHistoryRowSchema),
  );
}

// ── Article equivalents ──────────────────────────────────────────────────────

export function listInventoryArticleEquivalents(articleId: number): Promise<ArticleEquivalent[]> {
  return invokeParsed(
    "list_inventory_article_equivalents",
    { articleId },
    z.array(ArticleEquivalentSchema),
  );
}

export function upsertInventoryArticleEquivalent(
  input: ArticleEquivalentInput,
): Promise<ArticleEquivalent> {
  return invokeParsed("upsert_inventory_article_equivalent", { input }, ArticleEquivalentSchema);
}

export function deleteInventoryArticleEquivalent(equivalentId: number): Promise<void> {
  return invoke("delete_inventory_article_equivalent", { equivalentId });
}

// ── Purchase history ─────────────────────────────────────────────────────────

export function listInventoryArticlePurchaseHistory(
  articleId: number,
): Promise<ArticlePurchaseHistoryRow[]> {
  return invokeParsed(
    "list_inventory_article_purchase_history",
    { articleId },
    z.array(ArticlePurchaseHistoryRowSchema),
  );
}

// ── Replenishment ─────────────────────────────────────────────────────────────

export function evaluateInventoryReplenishment(
  warehouseId?: number | null,
): Promise<InventoryReplenishmentRecommendation[]> {
  return invokeParsed(
    "evaluate_inventory_replenishment",
    { warehouseId: warehouseId ?? null },
    z.array(InventoryReplenishmentRecommendationSchema),
  );
}

export function calculateInventoryAbc(): Promise<number> {
  return invokeParsed("calculate_inventory_abc", undefined, z.number());
}

export function calculateInventoryXyz(): Promise<number> {
  return invokeParsed("calculate_inventory_xyz", undefined, z.number());
}

// ── Procurement dashboard ─────────────────────────────────────────────────────

export function getProcurementDashboardSummary(): Promise<ProcurementDashboardSummary> {
  return invokeParsed(
    "get_procurement_dashboard_summary",
    undefined,
    ProcurementDashboardSummarySchema,
  );
}

export function getProcurementAlerts(): Promise<ProcurementAlert[]> {
  return invokeParsed("get_procurement_alerts", undefined, z.array(ProcurementAlertSchema));
}

export function getArticleConsumptionMonthly(
  articleId: number,
  months?: number | null,
): Promise<ArticleConsumptionMonth[]> {
  return invokeParsed(
    "get_article_consumption_monthly",
    { articleId, months: months ?? null },
    z.array(ArticleConsumptionMonthSchema),
  );
}

// ── Repairable history ────────────────────────────────────────────────────────

export function getInventoryArticleRepairableHistory(
  articleId: number,
): Promise<ArticleRepairableHistory[]> {
  return invokeParsed(
    "get_inventory_article_repairable_history",
    { articleId },
    z.array(ArticleRepairableHistorySchema),
  );
}

// ── Document links ────────────────────────────────────────────────────────────

export function listInventoryDocumentLinks(
  entityType: string,
  entityId: number,
): Promise<InventoryDocumentLink[]> {
  return invokeParsed(
    "list_inventory_document_links",
    { entityType, entityId },
    z.array(InventoryDocumentLinkSchema),
  );
}

export function upsertInventoryDocumentLink(
  input: InventoryDocumentLinkInput,
): Promise<InventoryDocumentLink> {
  return invokeParsed("upsert_inventory_document_link", { input }, InventoryDocumentLinkSchema);
}

// ── WO material readiness ─────────────────────────────────────────────────────

export function checkWoPartStockAvailability(workOrderId: number): Promise<WoMaterialReadiness> {
  return invokeParsed(
    "check_wo_part_stock_availability",
    { workOrderId },
    WoMaterialReadinessSchema,
  );
}

export function createProcurementRequisitionFromWoPart(
  workOrderId: number,
  articleId: number,
  requestedQty: number,
  preferredLocationId?: number | null,
): Promise<ProcurementRequisition> {
  return invokeParsed(
    "create_procurement_requisition_from_wo_part",
    {
      workOrderId,
      articleId,
      requestedQty,
      preferredLocationId: preferredLocationId ?? null,
    },
    ProcurementRequisitionSchema,
  );
}

export function suggestInventoryInternalTransfer(
  articleId: number,
  targetWarehouseId: number,
): Promise<InventoryStockBalance[]> {
  return invokeParsed(
    "suggest_inventory_internal_transfer",
    { articleId, targetWarehouseId },
    z.array(InventoryStockBalanceSchema),
  );
}

// ── Issue desk, lots, currencies, valuation, approval limits (migration 184) ──

const LotStockSchema = z.object({
  lot_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  lot_number: z.string().nullable(),
  serial_number: z.string().nullable(),
  expiry_date: z.string().nullable(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  quantity: z.number(),
  days_to_expiry: z.number().nullable(),
});
export type LotStock = z.infer<typeof LotStockSchema>;

const IssuedLineSchema = z.object({
  transaction_id: z.number(),
  lot_id: z.number().nullable(),
  lot_number: z.string().nullable(),
  serial_number: z.string().nullable(),
  quantity: z.number(),
  unit_cost: z.number().nullable(),
  extended_cost: z.number().nullable(),
});

const IssueOutcomeSchema = z.object({
  reservation_id: z.number(),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  lines: z.array(IssuedLineSchema),
  total_cost: z.number().nullable(),
});
export type IssueOutcome = z.infer<typeof IssueOutcomeSchema>;

export interface IssueStockRequest {
  article_id: number;
  location_id: number;
  quantity: number;
  /** Work order the parts go to; required without a reservation. */
  work_order_id: number | null;
  /** Planned-part reservation to consume. */
  reservation_id: number | null;
  /** Chosen lot / serial; tracked articles without one are picked first-expired-first-out. */
  lot_id: number | null;
  notes: string | null;
}

const ScanResolutionSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  unit_code: z.string(),
  lot_id: z.number().nullable(),
  /** barcode | article_code | lot */
  matched_on: z.string(),
});
export type ScanResolution = z.infer<typeof ScanResolutionSchema>;

/** Issues stock to a work order from the store — the same ISSUE posting as OT part usage. */
export function issueInventoryStockToWorkOrder(request: IssueStockRequest): Promise<IssueOutcome> {
  return invokeParsed(
    "issue_inventory_stock_to_work_order",
    { request: { ...request, actor_id: null } },
    IssueOutcomeSchema,
  );
}

/** Resolves a scanned barcode, article code, lot or serial number. */
export function resolveInventoryScan(value: string): Promise<ScanResolution> {
  return invokeParsed("resolve_inventory_scan", { value }, ScanResolutionSchema);
}

/** Lots in stock for an article (first-expired first), optionally at one location. */
export function listInventoryLotStock(
  articleId: number,
  locationId: number | null = null,
): Promise<LotStock[]> {
  return invokeParsed(
    "list_inventory_lot_stock",
    { articleId, locationId },
    z.array(LotStockSchema),
  );
}

/** Lots in stock that expire within `withinDays` days (expired lots included). */
export function listInventoryExpiringLots(withinDays: number): Promise<LotStock[]> {
  return invokeParsed("list_inventory_expiring_lots", { withinDays }, z.array(LotStockSchema));
}

const CurrencyExchangeRateSchema = z.object({
  id: z.number(),
  from_currency: z.string(),
  to_currency: z.string(),
  rate: z.number(),
  valid_from: z.string(),
  source: z.string().nullable(),
  is_active: z.number(),
  created_by_name: z.string().nullable(),
  created_at: z.string(),
});
export type CurrencyExchangeRate = z.infer<typeof CurrencyExchangeRateSchema>;

export interface CurrencyExchangeRateInput {
  from_currency: string;
  /** Defaults to the tenant currency. */
  to_currency: string | null;
  rate: number;
  valid_from: string;
  source: string | null;
}

const ResolvedExchangeRateSchema = z.object({
  from_currency: z.string(),
  to_currency: z.string(),
  rate: z.number().nullable(),
  valid_from: z.string().nullable(),
});
export type ResolvedExchangeRate = z.infer<typeof ResolvedExchangeRateSchema>;

export function listCurrencyExchangeRates(): Promise<CurrencyExchangeRate[]> {
  return invokeParsed(
    "list_currency_exchange_rates",
    undefined,
    z.array(CurrencyExchangeRateSchema),
  );
}

export function createCurrencyExchangeRate(
  input: CurrencyExchangeRateInput,
): Promise<CurrencyExchangeRate> {
  return invokeParsed("create_currency_exchange_rate", { input }, CurrencyExchangeRateSchema);
}

export function deactivateCurrencyExchangeRate(rateId: number): Promise<void> {
  return invokeParsed("deactivate_currency_exchange_rate", { rateId }, z.unknown()).then(
    () => undefined,
  );
}

/** Stored rate `fromCurrency` → tenant currency valid on `onDate` (today by default). */
export function resolveCurrencyExchangeRate(
  fromCurrency: string,
  onDate: string | null = null,
): Promise<ResolvedExchangeRate> {
  return invokeParsed(
    "resolve_currency_exchange_rate",
    { fromCurrency, onDate },
    ResolvedExchangeRateSchema,
  );
}

const ValuationReportRowSchema = z.object({
  group_id: z.number().nullable(),
  group_code: z.string(),
  group_name: z.string(),
  quantity: z.number(),
  value: z.number(),
  unvalued_qty: z.number(),
  article_count: z.number(),
});

const ValuationReportSchema = z.object({
  as_at: z.string(),
  group_by: z.string(),
  currency_code: z.string(),
  frozen_period_code: z.string().nullable(),
  total_value: z.number(),
  unvalued_qty: z.number(),
  rows: z.array(ValuationReportRowSchema),
});
export type ValuationReport = z.infer<typeof ValuationReportSchema>;
export type ValuationGroupBy = "article" | "warehouse" | "family";

const InventoryValuationPeriodSchema = z.object({
  id: z.number(),
  period_code: z.string(),
  period_start: z.string(),
  period_end: z.string(),
  status: z.string(),
  currency_code: z.string(),
  total_value: z.number(),
  unvalued_qty: z.number(),
  notes: z.string().nullable(),
  closed_by_name: z.string().nullable(),
  closed_at: z.string(),
});
export type InventoryValuationPeriod = z.infer<typeof InventoryValuationPeriodSchema>;

/** Stock value by article, warehouse or family as at a date (today by default). */
export function getInventoryValuationReport(
  groupBy: ValuationGroupBy,
  asAt: string | null = null,
): Promise<ValuationReport> {
  return invokeParsed("get_inventory_valuation_report", { groupBy, asAt }, ValuationReportSchema);
}

export function listInventoryValuationPeriods(): Promise<InventoryValuationPeriod[]> {
  return invokeParsed(
    "list_inventory_valuation_periods",
    undefined,
    z.array(InventoryValuationPeriodSchema),
  );
}

/** Closes a past period: freezes its valuation and refuses later postings dated inside it. */
export function closeInventoryValuationPeriod(input: {
  period_start: string;
  period_end: string;
  notes: string | null;
}): Promise<InventoryValuationPeriod> {
  return invokeParsed(
    "close_inventory_valuation_period",
    { input: { ...input, actor_id: null } },
    InventoryValuationPeriodSchema,
  );
}

const ProcurementApprovalLimitSchema = z.object({
  id: z.number(),
  role_id: z.number(),
  role_name: z.string(),
  document_type: z.string(),
  max_amount: z.number().nullable(),
  is_active: z.number(),
  row_version: z.number(),
  updated_at: z.string(),
});
export type ProcurementApprovalLimit = z.infer<typeof ProcurementApprovalLimitSchema>;
export type ApprovalDocumentType = "REQUISITION" | "PURCHASE_ORDER";

export function listProcurementApprovalLimits(): Promise<ProcurementApprovalLimit[]> {
  return invokeParsed(
    "list_procurement_approval_limits",
    undefined,
    z.array(ProcurementApprovalLimitSchema),
  );
}

/** Sets a role's approval ceiling for a document type (null = unlimited). */
export function upsertProcurementApprovalLimit(input: {
  role_id: number;
  document_type: ApprovalDocumentType;
  max_amount: number | null;
  is_active: boolean;
}): Promise<ProcurementApprovalLimit> {
  return invokeParsed(
    "upsert_procurement_approval_limit",
    { input: { ...input, actor_id: null } },
    ProcurementApprovalLimitSchema,
  );
}

export interface RequisitionLineInput {
  article_id: number;
  preferred_location_id: number | null;
  requested_qty: number;
  /** Estimated unit cost (tenant currency); sets the approval amount. */
  estimated_unit_cost: number | null;
}

/** Creates a requisition with several lines (the requester is the signed-in user). */
export function createInventoryMultiLineRequisition(input: {
  lines: RequisitionLineInput[];
  demand_source_type: string;
  demand_source_ref: string | null;
  purchase_priority: string | null;
  reason: string | null;
}): Promise<ProcurementRequisitionRow> {
  return invokeParsed(
    "create_inventory_multi_line_requisition",
    { input: { ...input, actor_id: null } },
    ProcurementRequisitionSchema,
  );
}

/** Replaces the lines of a draft requisition. */
export function replaceInventoryRequisitionLines(input: {
  requisition_id: number;
  expected_row_version: number;
  lines: RequisitionLineInput[];
}): Promise<ProcurementRequisitionRow> {
  return invokeParsed(
    "replace_inventory_requisition_lines",
    { input: { ...input, actor_id: null } },
    ProcurementRequisitionSchema,
  );
}

/** Sets the agreed unit price of a draft PO line, in the PO currency (null clears it). */
export function setInventoryPurchaseOrderLinePrice(
  poLineId: number,
  unitPrice: number | null,
): Promise<PurchaseOrderLine> {
  return invokeParsed(
    "set_inventory_purchase_order_line_price",
    { poLineId, unitPrice },
    PurchaseOrderLineSchema,
  );
}
