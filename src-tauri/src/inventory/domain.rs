use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleFamily {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warehouse {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub is_active: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockLocation {
    pub id: i64,
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub code: String,
    pub name: String,
    pub is_default: i64,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWarehouseInput {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWarehouseInput {
    pub name: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStockLocationInput {
    pub warehouse_id: i64,
    pub code: String,
    pub name: String,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStockLocationInput {
    pub code: Option<String>,
    pub name: Option<String>,
    pub is_default: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryArticle {
    pub id: i64,
    pub article_code: String,
    pub article_name: String,
    pub family_id: Option<i64>,
    pub family_code: Option<String>,
    pub family_name: Option<String>,
    pub unit_value_id: i64,
    pub unit_code: String,
    pub unit_label: String,
    pub criticality_value_id: Option<i64>,
    pub criticality_code: Option<String>,
    pub criticality_label: Option<String>,
    pub stocking_type_value_id: i64,
    pub stocking_type_code: String,
    pub stocking_type_label: String,
    pub tax_category_value_id: i64,
    pub tax_category_code: String,
    pub tax_category_label: String,
    pub procurement_category_value_id: Option<i64>,
    pub procurement_category_code: Option<String>,
    pub procurement_category_label: Option<String>,
    pub preferred_warehouse_id: Option<i64>,
    pub preferred_warehouse_code: Option<String>,
    pub preferred_location_id: Option<i64>,
    pub preferred_location_code: Option<String>,
    pub min_stock: f64,
    pub max_stock: Option<f64>,
    pub reorder_point: f64,
    pub safety_stock: f64,
    pub is_active: i64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryStockBalance {
    pub id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub location_id: i64,
    pub location_code: String,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryTransaction {
    pub id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub location_id: i64,
    pub location_code: String,
    pub reservation_id: Option<i64>,
    pub movement_type: String,
    pub quantity: f64,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    pub reason: Option<String>,
    pub performed_by_id: Option<i64>,
    pub performed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockReservation {
    pub id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub location_id: i64,
    pub location_code: String,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    pub quantity_reserved: f64,
    pub quantity_issued: f64,
    pub status: String,
    pub notes: Option<String>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReorderRecommendation {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub min_stock: f64,
    pub reorder_point: f64,
    pub max_stock: Option<f64>,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
    pub suggested_reorder_qty: f64,
    pub trigger_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateArticleFamilyInput {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateArticleFamilyInput {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryTaxCategory {
    pub id: i64,
    pub code: String,
    pub label: String,
    pub fr_label: Option<String>,
    pub en_label: Option<String>,
    pub description: Option<String>,
    pub sort_order: i64,
    pub is_active: i64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryTaxCategoryInput {
    pub code: String,
    pub label: String,
    pub fr_label: Option<String>,
    pub en_label: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryArticleInput {
    pub article_code: String,
    pub article_name: String,
    pub family_id: Option<i64>,
    pub unit_value_id: i64,
    pub criticality_value_id: Option<i64>,
    pub stocking_type_value_id: i64,
    pub tax_category_value_id: i64,
    pub procurement_category_value_id: Option<i64>,
    pub preferred_warehouse_id: Option<i64>,
    pub preferred_location_id: Option<i64>,
    pub min_stock: f64,
    pub max_stock: Option<f64>,
    pub reorder_point: f64,
    pub safety_stock: f64,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InventoryArticleFilter {
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InventoryStockFilter {
    pub article_id: Option<i64>,
    pub warehouse_id: Option<i64>,
    pub low_stock_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryStockAdjustInput {
    pub article_id: i64,
    pub location_id: i64,
    pub delta_qty: f64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InventoryTransactionFilter {
    pub article_id: Option<i64>,
    pub warehouse_id: Option<i64>,
    pub source_type: Option<String>,
    pub source_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StockReservationFilter {
    pub article_id: Option<i64>,
    pub warehouse_id: Option<i64>,
    pub source_type: Option<String>,
    pub source_id: Option<i64>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReserveInput {
    pub article_id: i64,
    pub location_id: i64,
    pub quantity: f64,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryIssueInput {
    pub reservation_id: i64,
    pub quantity: f64,
    pub source_type: Option<String>,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReturnInput {
    pub reservation_id: i64,
    pub quantity: f64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryTransferInput {
    pub article_id: i64,
    pub from_location_id: i64,
    pub to_location_id: i64,
    pub quantity: f64,
    pub source_type: Option<String>,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReleaseReservationInput {
    pub reservation_id: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcurementSupplier {
    pub id: i64,
    pub company_code: String,
    pub company_name: String,
    pub is_active: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcurementRequisition {
    pub id: i64,
    pub req_number: String,
    pub demand_source_type: String,
    pub demand_source_id: Option<i64>,
    pub demand_source_ref: Option<String>,
    pub status: String,
    pub posting_state: String,
    pub posting_error: Option<String>,
    pub requested_by_id: Option<i64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcurementRequisitionLine {
    pub id: i64,
    pub requisition_id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub preferred_location_id: Option<i64>,
    pub preferred_location_code: Option<String>,
    pub requested_qty: f64,
    pub source_reservation_id: Option<i64>,
    pub source_reorder_trigger: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseOrder {
    pub id: i64,
    pub po_number: String,
    pub requisition_id: Option<i64>,
    pub supplier_company_id: Option<i64>,
    pub supplier_company_name: Option<String>,
    pub status: String,
    pub posting_state: String,
    pub posting_error: Option<String>,
    pub ordered_by_id: Option<i64>,
    pub ordered_at: Option<String>,
    pub approved_by_id: Option<i64>,
    pub approved_at: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseOrderLine {
    pub id: i64,
    pub purchase_order_id: i64,
    pub requisition_line_id: Option<i64>,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub ordered_qty: f64,
    pub received_qty: f64,
    pub unit_price: Option<f64>,
    pub demand_source_type: String,
    pub demand_source_id: Option<i64>,
    pub demand_source_ref: Option<String>,
    pub source_reservation_id: Option<i64>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoodsReceipt {
    pub id: i64,
    pub gr_number: String,
    pub purchase_order_id: i64,
    pub status: String,
    pub posting_state: String,
    pub posting_error: Option<String>,
    pub received_by_id: Option<i64>,
    pub received_at: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoodsReceiptLine {
    pub id: i64,
    pub goods_receipt_id: i64,
    pub po_line_id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub received_qty: f64,
    pub accepted_qty: f64,
    pub rejected_qty: f64,
    pub rejection_reason: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairableOrder {
    pub id: i64,
    pub order_code: String,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub quantity: f64,
    pub source_location_id: i64,
    pub source_location_code: String,
    pub return_location_id: Option<i64>,
    pub return_location_code: Option<String>,
    pub linked_po_line_id: Option<i64>,
    pub linked_reservation_id: Option<i64>,
    pub status: String,
    pub reason: Option<String>,
    pub created_by_id: Option<i64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryStateEvent {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: i64,
    pub from_status: Option<String>,
    pub to_status: String,
    pub actor_id: Option<i64>,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub changed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProcurementRequisitionInput {
    pub article_id: i64,
    pub preferred_location_id: Option<i64>,
    pub requested_qty: f64,
    pub demand_source_type: String,
    pub demand_source_id: Option<i64>,
    pub demand_source_ref: Option<String>,
    pub source_reservation_id: Option<i64>,
    pub source_reorder_trigger: Option<String>,
    pub reason: Option<String>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionProcurementRequisitionInput {
    pub requisition_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePurchaseOrderFromRequisitionInput {
    pub requisition_id: i64,
    pub supplier_company_id: Option<i64>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionPurchaseOrderInput {
    pub purchase_order_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePostingStateInput {
    pub entity_type: String,
    pub entity_id: i64,
    pub posting_state: String,
    pub posting_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivePurchaseOrderLineInput {
    pub po_line_id: i64,
    pub article_id: i64,
    pub location_id: i64,
    pub received_qty: f64,
    pub accepted_qty: f64,
    pub rejected_qty: f64,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveGoodsInput {
    pub purchase_order_id: i64,
    pub lines: Vec<ReceivePurchaseOrderLineInput>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepairableOrderInput {
    pub article_id: i64,
    pub quantity: f64,
    pub source_location_id: i64,
    pub return_location_id: Option<i64>,
    pub linked_po_line_id: Option<i64>,
    pub linked_reservation_id: Option<i64>,
    pub reason: Option<String>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRepairableOrderInput {
    pub order_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub actor_id: Option<i64>,
    pub return_location_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryCountSession {
    pub id: i64,
    pub session_code: String,
    pub warehouse_id: i64,
    pub location_id: Option<i64>,
    pub status: String,
    pub critical_abs_threshold: f64,
    pub submitted_by_id: Option<i64>,
    pub submitted_at: Option<String>,
    pub posted_by_id: Option<i64>,
    pub posted_at: Option<String>,
    pub reversed_by_id: Option<i64>,
    pub reversed_at: Option<String>,
    pub reversal_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryCountLine {
    pub id: i64,
    pub session_id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: i64,
    pub location_id: i64,
    pub location_code: String,
    pub system_qty: f64,
    pub counted_qty: f64,
    pub variance_qty: f64,
    pub variance_reason_code: Option<String>,
    pub is_critical: i64,
    pub approval_required: i64,
    pub approved_by_id: Option<i64>,
    pub approved_at: Option<String>,
    pub approval_note: Option<String>,
    pub posted_transaction_id: Option<i64>,
    pub reversed_transaction_id: Option<i64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReconciliationRun {
    pub id: i64,
    pub run_code: String,
    pub run_date: String,
    pub status: String,
    pub checked_rows: i64,
    pub drift_rows: i64,
    pub checked_by_id: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReconciliationFinding {
    pub id: i64,
    pub run_id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub location_id: i64,
    pub location_code: String,
    pub balance_on_hand: f64,
    pub ledger_expected_on_hand: f64,
    pub drift_qty: f64,
    pub is_break: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInventoryCountSessionInput {
    pub warehouse_id: i64,
    pub location_id: Option<i64>,
    pub critical_abs_threshold: Option<f64>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertInventoryCountLineInput {
    pub session_id: i64,
    pub article_id: i64,
    pub location_id: i64,
    pub counted_qty: f64,
    pub variance_reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionInventoryCountSessionInput {
    pub session_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub reason: Option<String>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveInventoryCountLineInput {
    pub line_id: i64,
    pub expected_row_version: i64,
    pub reviewer_id: i64,
    pub reviewer_evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostInventoryCountSessionInput {
    pub session_id: i64,
    pub expected_row_version: i64,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseInventoryCountSessionInput {
    pub session_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInventoryReconciliationInput {
    pub actor_id: Option<i64>,
    pub drift_break_threshold: Option<f64>,
}
