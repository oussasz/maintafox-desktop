import { invoke } from "@tauri-apps/api/core";
import { z, ZodError } from "zod";

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
  PurchaseOrderLine,
  ReceiveGoodsInput,
  RepairableOrder,
  ReverseInventoryCountSessionInput,
  RunInventoryReconciliationInput,
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
  preferred_location_id: z.number().nullable(),
  preferred_location_code: z.string().nullable(),
  min_stock: z.number(),
  max_stock: z.number().nullable(),
  reorder_point: z.number(),
  safety_stock: z.number(),
  is_active: z.number(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

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
  is_active: z.boolean().optional(),
});

const InventoryStockBalanceSchema = z.object({
  id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  warehouse_id: z.number(),
  warehouse_code: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
  updated_at: z.string(),
});

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
  location_id: z.number(),
  location_code: z.string(),
  source_type: z.string(),
  source_id: z.number().nullable(),
  source_ref: z.string().nullable(),
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
  location_id: z.number(),
  location_code: z.string(),
  reservation_id: z.number().nullable(),
  movement_type: z.string(),
  quantity: z.number(),
  source_type: z.string(),
  source_id: z.number().nullable(),
  source_ref: z.string().nullable(),
  reason: z.string().nullable(),
  performed_by_id: z.number().nullable(),
  performed_at: z.string(),
});

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

const ProcurementRequisitionSchema = z.object({
  id: z.number(),
  req_number: z.string(),
  demand_source_type: z.string(),
  demand_source_id: z.number().nullable(),
  demand_source_ref: z.string().nullable(),
  status: z.string(),
  posting_state: z.string(),
  posting_error: z.string().nullable(),
  requested_by_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ProcurementRequisitionLineSchema = z.object({
  id: z.number(),
  requisition_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  preferred_location_id: z.number().nullable(),
  preferred_location_code: z.string().nullable(),
  requested_qty: z.number(),
  source_reservation_id: z.number().nullable(),
  source_reorder_trigger: z.string().nullable(),
  status: z.string(),
  created_at: z.string(),
});

const PurchaseOrderSchema = z.object({
  id: z.number(),
  po_number: z.string(),
  requisition_id: z.number().nullable(),
  supplier_company_id: z.number().nullable(),
  supplier_company_name: z.string().nullable(),
  status: z.string(),
  posting_state: z.string(),
  posting_error: z.string().nullable(),
  ordered_by_id: z.number().nullable(),
  ordered_at: z.string().nullable(),
  approved_by_id: z.number().nullable(),
  approved_at: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

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
  created_at: z.string(),
  updated_at: z.string(),
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
  status: z.string(),
  created_at: z.string(),
});

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
  created_by_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
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
  approved_at: z.string().nullable(),
  approval_note: z.string().nullable(),
  posted_transaction_id: z.number().nullable(),
  reversed_transaction_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

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

export function createInventoryArticleFamily(input: CreateArticleFamilyInput): Promise<ArticleFamily> {
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
  return invokeParsed("list_inventory_tax_categories", undefined, z.array(InventoryTaxCategorySchema));
}

export function createInventoryTaxCategory(input: InventoryTaxCategoryInput): Promise<InventoryTaxCategory> {
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
  return invokeParsed("list_inventory_locations", { warehouseId: warehouseId ?? null }, z.array(StockLocationSchema));
}

export function createInventoryWarehouse(input: CreateWarehouseInput): Promise<Warehouse> {
  return invokeParsed("create_inventory_warehouse", { input }, WarehouseSchema);
}

export function updateInventoryWarehouse(warehouseId: number, input: UpdateWarehouseInput): Promise<Warehouse> {
  return invokeParsed("update_inventory_warehouse", { warehouseId, input }, WarehouseSchema);
}

export function createInventoryStockLocation(input: CreateStockLocationInput): Promise<StockLocation> {
  return invokeParsed("create_inventory_stock_location", { input }, StockLocationSchema);
}

export function updateInventoryStockLocation(
  locationId: number,
  expectedRowVersion: number,
  input: UpdateStockLocationInput,
): Promise<StockLocation> {
  return invokeParsed("update_inventory_stock_location", { locationId, expectedRowVersion, input }, StockLocationSchema);
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

export function listInventoryArticles(filter: InventoryArticleFilter): Promise<InventoryArticle[]> {
  return invokeParsed("list_inventory_articles", { filter }, z.array(InventoryArticleSchema));
}

export function createInventoryArticle(input: InventoryArticleInput): Promise<InventoryArticle> {
  return invokeParsed("create_inventory_article", { input: InventoryArticleInputSchema.parse(input) }, InventoryArticleSchema);
}

export function updateInventoryArticle(
  articleId: number,
  expectedRowVersion: number,
  input: InventoryArticleInput,
): Promise<InventoryArticle> {
  return invokeParsed(
    "update_inventory_article",
    { articleId, expectedRowVersion, input: InventoryArticleInputSchema.parse(input) },
    InventoryArticleSchema,
  );
}

export function listInventoryStockBalances(filter: InventoryStockFilter): Promise<InventoryStockBalance[]> {
  return invokeParsed("list_inventory_stock_balances", { filter }, z.array(InventoryStockBalanceSchema));
}

export function adjustInventoryStock(input: InventoryStockAdjustInput): Promise<InventoryStockBalance> {
  return invokeParsed("adjust_inventory_stock", { input }, InventoryStockBalanceSchema);
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

export function transferInventoryStock(input: InventoryTransferInput): Promise<InventoryStockBalance[]> {
  return invokeParsed("transfer_inventory_stock", { input }, z.array(InventoryStockBalanceSchema));
}

export function releaseInventoryReservation(input: InventoryReleaseReservationInput): Promise<StockReservation> {
  return invokeParsed("release_inventory_reservation", { input }, StockReservationSchema);
}

export function listInventoryReservations(filter: StockReservationFilter): Promise<StockReservation[]> {
  return invokeParsed("list_inventory_reservations", { filter }, z.array(StockReservationSchema));
}

export function listInventoryTransactions(filter: InventoryTransactionFilter): Promise<InventoryTransaction[]> {
  return invokeParsed("list_inventory_transactions", { filter }, z.array(InventoryTransactionSchema));
}

export function evaluateInventoryReorder(warehouseId?: number | null): Promise<InventoryReorderRecommendation[]> {
  return invokeParsed(
    "evaluate_inventory_reorder",
    { warehouseId: warehouseId ?? null },
    z.array(InventoryReorderRecommendationSchema),
  );
}

export function listInventoryProcurementSuppliers(): Promise<ProcurementSupplier[]> {
  return invokeParsed("list_inventory_procurement_suppliers", undefined, z.array(ProcurementSupplierSchema));
}

export function listInventoryProcurementRequisitions(): Promise<ProcurementRequisition[]> {
  return invokeParsed("list_inventory_procurement_requisitions", undefined, z.array(ProcurementRequisitionSchema));
}

export function listInventoryProcurementRequisitionLines(
  requisitionId: number,
): Promise<ProcurementRequisitionLine[]> {
  return invokeParsed(
    "list_inventory_procurement_requisition_lines",
    { requisitionId },
    z.array(ProcurementRequisitionLineSchema),
  );
}

export function createInventoryProcurementRequisition(
  input: CreateProcurementRequisitionInput,
): Promise<ProcurementRequisition> {
  return invokeParsed("create_inventory_procurement_requisition", { input }, ProcurementRequisitionSchema);
}

export function transitionInventoryProcurementRequisition(
  input: TransitionProcurementRequisitionInput,
): Promise<ProcurementRequisition> {
  return invokeParsed("transition_inventory_procurement_requisition", { input }, ProcurementRequisitionSchema);
}

export function createInventoryPurchaseOrderFromRequisition(
  input: CreatePurchaseOrderFromRequisitionInput,
): Promise<PurchaseOrder> {
  return invokeParsed("create_inventory_purchase_order_from_requisition", { input }, PurchaseOrderSchema);
}

export function transitionInventoryPurchaseOrder(input: TransitionPurchaseOrderInput): Promise<PurchaseOrder> {
  return invokeParsed("transition_inventory_purchase_order", { input }, PurchaseOrderSchema);
}

export function listInventoryPurchaseOrders(): Promise<PurchaseOrder[]> {
  return invokeParsed("list_inventory_purchase_orders", undefined, z.array(PurchaseOrderSchema));
}

export function listInventoryPurchaseOrderLines(purchaseOrderId: number): Promise<PurchaseOrderLine[]> {
  return invokeParsed("list_inventory_purchase_order_lines", { purchaseOrderId }, z.array(PurchaseOrderLineSchema));
}

export function receiveInventoryPurchaseOrderGoods(input: ReceiveGoodsInput): Promise<GoodsReceipt> {
  return invokeParsed("receive_inventory_purchase_order_goods", { input }, GoodsReceiptSchema);
}

export function listInventoryGoodsReceipts(): Promise<GoodsReceipt[]> {
  return invokeParsed("list_inventory_goods_receipts", undefined, z.array(GoodsReceiptSchema));
}

export function listInventoryGoodsReceiptLines(goodsReceiptId: number): Promise<GoodsReceiptLine[]> {
  return invokeParsed("list_inventory_goods_receipt_lines", { goodsReceiptId }, z.array(GoodsReceiptLineSchema));
}

export function updateInventoryProcurementPostingState(input: UpdatePostingStateInput): Promise<void> {
  return invokeParsed("update_inventory_procurement_posting_state", { input }, z.unknown()).then(() => undefined);
}

export function createInventoryRepairableOrder(input: CreateRepairableOrderInput): Promise<RepairableOrder> {
  return invokeParsed("create_inventory_repairable_order", { input }, RepairableOrderSchema);
}

export function transitionInventoryRepairableOrder(input: TransitionRepairableOrderInput): Promise<RepairableOrder> {
  return invokeParsed("transition_inventory_repairable_order", { input }, RepairableOrderSchema);
}

export function listInventoryRepairableOrders(): Promise<RepairableOrder[]> {
  return invokeParsed("list_inventory_repairable_orders", undefined, z.array(RepairableOrderSchema));
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

export function createInventoryCountSession(input: CreateInventoryCountSessionInput): Promise<InventoryCountSession> {
  return invokeParsed("create_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function transitionInventoryCountSession(
  input: TransitionInventoryCountSessionInput,
): Promise<InventoryCountSession> {
  return invokeParsed("transition_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function upsertInventoryCountLine(input: UpsertInventoryCountLineInput): Promise<InventoryCountLine> {
  return invokeParsed("upsert_inventory_count_line", { input }, InventoryCountLineSchema);
}

export function approveInventoryCountLine(input: ApproveInventoryCountLineInput): Promise<InventoryCountLine> {
  return invokeParsed("approve_inventory_count_line", { input }, InventoryCountLineSchema);
}

export function postInventoryCountSession(input: PostInventoryCountSessionInput): Promise<InventoryCountSession> {
  return invokeParsed("post_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function reverseInventoryCountSession(
  input: ReverseInventoryCountSessionInput,
): Promise<InventoryCountSession> {
  return invokeParsed("reverse_inventory_count_session", { input }, InventoryCountSessionSchema);
}

export function listInventoryCountSessions(): Promise<InventoryCountSession[]> {
  return invokeParsed("list_inventory_count_sessions", undefined, z.array(InventoryCountSessionSchema));
}

export function listInventoryCountLines(sessionId: number): Promise<InventoryCountLine[]> {
  return invokeParsed("list_inventory_count_lines", { sessionId }, z.array(InventoryCountLineSchema));
}

export function runInventoryReconciliation(
  input: RunInventoryReconciliationInput,
): Promise<InventoryReconciliationRun> {
  return invokeParsed("run_inventory_reconciliation", { input }, InventoryReconciliationRunSchema);
}

export function listInventoryReconciliationRuns(): Promise<InventoryReconciliationRun[]> {
  return invokeParsed("list_inventory_reconciliation_runs", undefined, z.array(InventoryReconciliationRunSchema));
}

export function listInventoryReconciliationFindings(runId: number): Promise<InventoryReconciliationFinding[]> {
  return invokeParsed(
    "list_inventory_reconciliation_findings",
    { runId },
    z.array(InventoryReconciliationFindingSchema),
  );
}
