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
    pub preferred_warehouse_name: Option<String>,
    pub preferred_location_id: Option<i64>,
    pub preferred_location_code: Option<String>,
    pub preferred_location_name: Option<String>,
    pub min_stock: f64,
    pub max_stock: Option<f64>,
    pub reorder_point: f64,
    pub safety_stock: f64,
    pub manufacturer_name: Option<String>,
    pub manufacturer_part_number: Option<String>,
    pub oem_part_number: Option<String>,
    pub replenishment_policy_code: Option<String>,
    pub economic_order_qty: Option<f64>,
    pub minimum_order_qty: Option<f64>,
    pub maximum_order_qty: Option<f64>,
    pub order_multiple_qty: Option<f64>,
    pub lead_time_days: Option<i64>,
    pub review_period_days: Option<i64>,
    pub abc_class_code: Option<String>,
    pub xyz_class_code: Option<String>,
    pub is_critical_spare: i64,
    pub requires_expiration: i64,
    pub shelf_life_days: Option<i64>,
    pub requires_batch_tracking: i64,
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
    pub warehouse_name: Option<String>,
    pub location_id: i64,
    pub location_code: String,
    pub location_name: Option<String>,
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
    pub warehouse_name: Option<String>,
    pub location_id: i64,
    pub location_code: String,
    pub location_name: Option<String>,
    pub reservation_id: Option<i64>,
    pub movement_type: String,
    pub quantity: f64,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    /// Movement reason lookup code (e.g. inventory.movement_type).
    pub reason_code: Option<String>,
    /// Free-text notes (persisted in `reason` column).
    pub notes: Option<String>,
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
    pub warehouse_name: Option<String>,
    pub location_id: i64,
    pub location_code: String,
    pub location_name: Option<String>,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
    pub work_order_code: Option<String>,
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
pub struct ReplenishmentTransferOption {
    pub warehouse_id: i64,
    pub warehouse_code: String,
    pub available_qty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReplenishmentRecommendation {
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
    pub suggestion_type: String,
    pub suggested_supplier_id: Option<i64>,
    pub suggested_supplier_name: Option<String>,
    pub estimated_cost: Option<f64>,
    pub expected_arrival: Option<String>,
    pub reason: Option<String>,
    pub transfer_options: Vec<ReplenishmentTransferOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcurementAlert {
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<i64>,
    pub entity_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleConsumptionMonth {
    pub year_month: String,
    pub issued_qty: f64,
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
    pub manufacturer_name: Option<String>,
    pub manufacturer_part_number: Option<String>,
    pub oem_part_number: Option<String>,
    pub replenishment_policy_code: Option<String>,
    pub economic_order_qty: Option<f64>,
    pub minimum_order_qty: Option<f64>,
    pub maximum_order_qty: Option<f64>,
    pub order_multiple_qty: Option<f64>,
    pub lead_time_days: Option<i64>,
    pub review_period_days: Option<i64>,
    pub abc_class_code: Option<String>,
    pub xyz_class_code: Option<String>,
    pub is_critical_spare: Option<bool>,
    pub requires_expiration: Option<bool>,
    pub shelf_life_days: Option<i64>,
    pub requires_batch_tracking: Option<bool>,
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
    /// Movement reason lookup code (e.g. ENTREE_ACHAT).
    pub reason_code: Option<String>,
    /// Free-text comment.
    pub notes: Option<String>,
    /// Business document reference (PO, WO, count batch, …).
    pub source_ref: Option<String>,
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
pub struct InventorySupplier {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub external_company_id: Option<i64>,
    pub status_code: String,
    pub payment_terms_code: Option<String>,
    pub currency_value_id: Option<i64>,
    pub incoterms_code: Option<String>,
    pub default_lead_time_days: Option<i64>,
    pub default_buyer_person_id: Option<i64>,
    pub is_active: i64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventorySupplierInput {
    pub code: String,
    pub name: String,
    pub external_company_id: Option<i64>,
    pub status_code: String,
    pub payment_terms_code: Option<String>,
    pub currency_value_id: Option<i64>,
    pub incoterms_code: Option<String>,
    pub default_lead_time_days: Option<i64>,
    pub default_buyer_person_id: Option<i64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierArticleSource {
    pub id: i64,
    pub supplier_id: i64,
    pub supplier_code: String,
    pub supplier_name: String,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub is_preferred: i64,
    pub priority: i64,
    pub lead_time_days: Option<i64>,
    pub unit_price_hint: Option<f64>,
    pub min_order_qty: Option<f64>,
    pub supplier_article_code: Option<String>,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
    // Computed procurement-intelligence fields (populated by list queries).
    pub last_price: Option<f64>,
    pub avg_price: Option<f64>,
    pub last_purchase_at: Option<String>,
    pub currency_value_id: Option<i64>,
    pub currency_label: Option<String>,
    /// LOW|MEDIUM|HIGH risk for this supplier (from scorecard formula).
    pub risk_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierArticleSourceInput {
    pub supplier_id: i64,
    pub article_id: i64,
    pub is_preferred: Option<bool>,
    pub priority: Option<i64>,
    pub lead_time_days: Option<i64>,
    pub unit_price_hint: Option<f64>,
    pub min_order_qty: Option<f64>,
    pub supplier_article_code: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierPrice {
    pub id: i64,
    pub supplier_id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub unit_price: f64,
    pub currency_value_id: Option<i64>,
    pub price_unit_value_id: Option<i64>,
    pub min_order_qty: Option<f64>,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierPriceInput {
    pub supplier_id: i64,
    pub article_id: i64,
    pub unit_price: f64,
    pub currency_value_id: Option<i64>,
    pub price_unit_value_id: Option<i64>,
    pub min_order_qty: Option<f64>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierScorecard {
    pub supplier_id: i64,
    pub supplier_code: String,
    pub supplier_name: String,
    pub on_time_delivery_pct: Option<f64>,
    pub avg_lead_time_days: Option<f64>,
    pub delivery_accuracy_pct: Option<f64>,
    pub avg_price: Option<f64>,
    pub open_po_count: i64,
    pub completed_po_count: i64,
    pub last_purchase_at: Option<String>,
    /// % of received quantity rejected across goods-receipt lines.
    pub rejected_pct: Option<f64>,
    /// LOW|MEDIUM|HIGH risk classification derived from delivery metrics.
    pub risk_level: String,
    pub last_deliveries: Vec<SupplierRecentDelivery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierRecentDelivery {
    pub received_at: Option<String>,
    pub po_number: String,
    pub article_code: String,
    pub article_name: String,
    pub ordered_qty: Option<f64>,
    pub accepted_qty: f64,
    pub rejected_qty: f64,
    pub actual_lead_time_days: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierContact {
    pub id: i64,
    pub supplier_id: i64,
    pub contact_name: String,
    pub contact_role: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub is_primary: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierContactInput {
    pub supplier_id: i64,
    pub contact_name: String,
    pub contact_role: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub is_primary: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierPurchaseHistoryRow {
    pub purchase_order_id: i64,
    pub po_number: String,
    pub ordered_at: Option<String>,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub ordered_qty: f64,
    pub unit_price: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleEquivalent {
    pub id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub equivalent_article_id: i64,
    pub equivalent_code: String,
    pub equivalent_name: String,
    pub equivalence_type: String,
    pub notes: Option<String>,
    pub is_bidirectional: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleEquivalentInput {
    pub article_id: i64,
    pub equivalent_article_id: i64,
    pub equivalence_type: String,
    pub notes: Option<String>,
    pub is_bidirectional: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticlePurchaseHistoryRow {
    pub purchase_order_id: i64,
    pub po_number: String,
    pub ordered_at: Option<String>,
    pub supplier_id: Option<i64>,
    pub supplier_name: Option<String>,
    pub unit_price: Option<f64>,
    pub ordered_qty: f64,
    pub received_qty: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcurementDashboardSummary {
    pub open_requisitions: i64,
    pub pending_approval_pos: i64,
    pub open_pos: i64,
    pub overdue_pos: i64,
    pub pending_receipts: i64,
    pub low_stock_articles: i64,
    pub critical_low_stock_articles: i64,
    pub repairables_in_repair: i64,
    pub active_suppliers_count: i64,
    pub receiving_today_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleRepairableHistory {
    pub order_id: i64,
    pub order_code: String,
    pub quantity: f64,
    pub status: String,
    pub reason: Option<String>,
    pub repair_cost: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryDocumentLink {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: i64,
    pub document_ref: String,
    pub link_purpose: String,
    pub is_primary: i64,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryDocumentLinkInput {
    pub entity_type: String,
    pub entity_id: i64,
    pub document_ref: String,
    pub link_purpose: String,
    pub is_primary: Option<bool>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub created_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoPartShortage {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub requested_qty: f64,
    pub available_qty: f64,
    pub shortage_qty: f64,
    pub preferred_location_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoMaterialReadiness {
    pub work_order_id: i64,
    pub work_order_code: String,
    pub total_parts: i64,
    pub available_parts: i64,
    pub reserved_parts: i64,
    pub missing_parts: i64,
    /// 0–100: share of planned part lines that are stock-available or reserved.
    pub ready_pct: f64,
    /// 0–100: share of planned part lines with sufficient reserved qty.
    pub reserved_pct: f64,
    pub expected_arrival: Option<String>,
    pub is_fully_available: bool,
    pub shortages: Vec<WoPartShortage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcurementRequisition {
    pub id: i64,
    pub req_number: String,
    pub demand_source_type: String,
    pub demand_source_id: Option<i64>,
    pub demand_source_ref: Option<String>,
    pub purchase_priority: String,
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
    pub demand_source_line_id: Option<i64>,
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
    pub supplier_id: Option<i64>,
    pub supplier_name: Option<String>,
    pub supplier_company_id: Option<i64>,
    pub supplier_company_name: Option<String>,
    pub status: String,
    pub posting_state: String,
    pub posting_error: Option<String>,
    pub ordered_by_id: Option<i64>,
    pub ordered_at: Option<String>,
    pub approved_by_id: Option<i64>,
    pub approved_at: Option<String>,
    pub expected_delivery_date: Option<String>,
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
    pub demand_source_line_id: Option<i64>,
    pub source_reservation_id: Option<i64>,
    pub status: String,
    /// ordered_qty − received_qty, floored at zero.
    pub remaining_qty: f64,
    /// ordered_qty × unit_price; `None` when the line has no unit price yet.
    pub line_total: Option<f64>,
    /// Work order behind this line when `demand_source_type` is WORK_ORDER / WORK_ORDER_PART.
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Purchase order aggregate for the PO detail workspace: header, lines, receipts,
/// lifecycle trail and linked documents in a single round trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseOrderDetail {
    pub order: PurchaseOrder,
    pub lines: Vec<PurchaseOrderLine>,
    pub goods_receipts: Vec<GoodsReceipt>,
    pub state_events: Vec<InventoryStateEvent>,
    pub document_links: Vec<InventoryDocumentLink>,
    /// Sum of priced line totals; `None` when no line carries a unit price.
    pub grand_total: Option<f64>,
    /// True when at least one line lacks a unit price, so the total understates the PO.
    pub grand_total_partial: bool,
}

/// Projected stock position for an article after a hypothetical quantity change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockImpactProjection {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_id: Option<i64>,
    pub warehouse_code: Option<String>,
    pub current_on_hand: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
    /// Outstanding quantity on non-closed purchase orders (0 when not requested).
    pub incoming_open_po_qty: f64,
    pub delta_qty: f64,
    pub projected_on_hand: f64,
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
    pub ordered_qty: Option<f64>,
    pub actual_lead_time_days: Option<f64>,
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
    pub serial_number: Option<String>,
    pub vendor_supplier_id: Option<i64>,
    pub vendor_supplier_code: Option<String>,
    pub vendor_supplier_name: Option<String>,
    pub sent_at: Option<String>,
    pub returned_at: Option<String>,
    pub warranty_active: i64,
    pub warranty_until: Option<String>,
    pub repair_cost: Option<f64>,
    /// Work order behind the linked reservation, when the repairable serves WO demand.
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub created_by_id: Option<i64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Repair history aggregates for the article of a repairable order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairableHistoryStats {
    pub repair_count: i64,
    pub avg_cost: Option<f64>,
    pub avg_turnaround_days: Option<f64>,
}

/// Outcome of the repair-vs-replace evaluation for a repairable order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairVsReplaceResult {
    pub repair_cost: Option<f64>,
    pub replacement_cost: Option<f64>,
    /// Repair/replacement cost ratio above which replacement is recommended.
    pub threshold_ratio: f64,
    /// REPAIR | REPLACE | REVIEW | INSUFFICIENT_DATA
    pub recommendation: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairableOrderDetail {
    pub order: RepairableOrder,
    pub state_events: Vec<InventoryStateEvent>,
    pub document_links: Vec<InventoryDocumentLink>,
    pub history_stats: RepairableHistoryStats,
    pub repair_vs_replace: RepairVsReplaceResult,
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
    pub demand_source_line_id: Option<i64>,
    pub source_reservation_id: Option<i64>,
    pub source_reorder_trigger: Option<String>,
    pub purchase_priority: Option<String>,
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
    pub supplier_id: Option<i64>,
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
    pub fulfillment_action: Option<String>,
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
    pub serial_number: Option<String>,
    pub vendor_supplier_id: Option<i64>,
    pub actor_id: Option<i64>,
}

/// Lifecycle transition plus the repair facts captured at that step
/// (vendor and serial on dispatch, cost and warranty on return).
/// Every optional field is only applied when present; `None` leaves the stored value untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRepairableOrderInput {
    pub order_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub actor_id: Option<i64>,
    pub return_location_id: Option<i64>,
    pub serial_number: Option<String>,
    pub vendor_supplier_id: Option<i64>,
    pub repair_cost: Option<f64>,
    pub warranty_active: Option<bool>,
    pub warranty_until: Option<String>,
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
