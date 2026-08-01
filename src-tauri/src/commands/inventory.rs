//! Inventory IPC commands.
//!
//! Permission gates:
//! - `inv.view`: read article master and stock balances
//! - `inv.manage`: create/update articles and post adjustments

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::inventory::domain::{
    ApproveInventoryCountLineInput,
    ArticleConsumptionMonth,
    // New article / procurement types
    ArticleEquivalent,
    ArticleEquivalentInput,
    ArticleFamily,
    ArticlePurchaseHistoryRow,
    ArticleRepairableHistory,
    CreateArticleFamilyInput,
    CreateInventoryCountSessionInput,
    CreateProcurementRequisitionInput,
    CreatePurchaseOrderFromRequisitionInput,
    CreateRepairableOrderInput,
    CreateStockLocationInput,
    CreateWarehouseInput,
    GoodsReceipt,
    GoodsReceiptLine,
    InventoryArticle,
    InventoryArticleFilter,
    InventoryArticleInput,
    InventoryCountLine,
    InventoryCountSession,
    InventoryDocumentLink,
    InventoryDocumentLinkInput,
    InventoryIssueInput,
    InventoryReconciliationFinding,
    InventoryReconciliationRun,
    InventoryReleaseReservationInput,
    InventoryReorderRecommendation,
    InventoryReplenishmentRecommendation,
    InventoryReserveInput,
    InventoryReturnInput,
    InventoryStateEvent,
    InventoryStockAdjustInput,
    InventoryStockBalance,
    InventoryStockFilter,
    // Supplier types
    InventorySupplier,
    InventorySupplierInput,
    InventoryTaxCategory,
    InventoryTaxCategoryInput,
    InventoryTransaction,
    InventoryTransactionFilter,
    InventoryTransferInput,
    PostInventoryCountSessionInput,
    ProcurementAlert,
    ProcurementDashboardSummary,
    ProcurementRequisition,
    ProcurementRequisitionLine,
    ProcurementSupplier,
    PurchaseOrder,
    PurchaseOrderDetail,
    PurchaseOrderLine,
    ReceiveGoodsInput,
    RepairVsReplaceResult,
    RepairableOrder,
    RepairableOrderDetail,
    ReverseInventoryCountSessionInput,
    RunInventoryReconciliationInput,
    StockImpactProjection,
    StockLocation,
    StockReservation,
    StockReservationFilter,
    SupplierArticleSource,
    SupplierArticleSourceInput,
    SupplierContact,
    SupplierContactInput,
    SupplierPrice,
    SupplierPriceInput,
    SupplierPurchaseHistoryRow,
    SupplierScorecard,
    TransitionInventoryCountSessionInput,
    TransitionProcurementRequisitionInput,
    TransitionPurchaseOrderInput,
    TransitionRepairableOrderInput,
    UpdateArticleFamilyInput,
    UpdatePostingStateInput,
    UpdateStockLocationInput,
    UpdateWarehouseInput,
    UpsertInventoryCountLineInput,
    Warehouse,
    WoMaterialReadiness,
};
use crate::inventory::valuation::ValuationCostResult;
use crate::inventory::{controls, procurement, queries, suppliers, valuation};
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_inventory_article_families(state: State<'_, AppState>) -> AppResult<Vec<ArticleFamily>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_article_families(&state.db).await
}

#[tauri::command]
pub async fn create_inventory_article_family(
    input: CreateArticleFamilyInput,
    state: State<'_, AppState>,
) -> AppResult<ArticleFamily> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::create_article_family(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_article_family(
    family_id: i64,
    input: UpdateArticleFamilyInput,
    state: State<'_, AppState>,
) -> AppResult<ArticleFamily> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::update_article_family(&state.db, family_id, input).await
}

#[tauri::command]
pub async fn deactivate_inventory_article_family(
    family_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ArticleFamily> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::deactivate_article_family(&state.db, family_id).await
}

#[tauri::command]
pub async fn list_inventory_tax_categories(state: State<'_, AppState>) -> AppResult<Vec<InventoryTaxCategory>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_inventory_tax_categories(&state.db).await
}

#[tauri::command]
pub async fn create_inventory_tax_category(
    input: InventoryTaxCategoryInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryTaxCategory> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::create_inventory_tax_category(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_tax_category(
    tax_category_id: i64,
    expected_row_version: i64,
    input: InventoryTaxCategoryInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryTaxCategory> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::update_inventory_tax_category(&state.db, tax_category_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn deactivate_inventory_tax_category(
    tax_category_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<InventoryTaxCategory> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::deactivate_inventory_tax_category(&state.db, tax_category_id, expected_row_version).await
}

#[tauri::command]
pub async fn list_inventory_warehouses(state: State<'_, AppState>) -> AppResult<Vec<Warehouse>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_warehouses(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_locations(
    warehouse_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<StockLocation>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_locations(&state.db, warehouse_id).await
}

#[tauri::command]
pub async fn create_inventory_warehouse(
    input: CreateWarehouseInput,
    state: State<'_, AppState>,
) -> AppResult<Warehouse> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::create_warehouse(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_warehouse(
    warehouse_id: i64,
    input: UpdateWarehouseInput,
    state: State<'_, AppState>,
) -> AppResult<Warehouse> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::update_warehouse(&state.db, warehouse_id, input).await
}

#[tauri::command]
pub async fn create_inventory_stock_location(
    input: CreateStockLocationInput,
    state: State<'_, AppState>,
) -> AppResult<StockLocation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::create_stock_location(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_stock_location(
    location_id: i64,
    expected_row_version: i64,
    input: UpdateStockLocationInput,
    state: State<'_, AppState>,
) -> AppResult<StockLocation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::update_stock_location(&state.db, location_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn evaluate_inventory_unit_cost(
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ValuationCostResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    valuation::evaluate_unit_cost(&state.db, article_id, warehouse_id, location_id).await
}

#[tauri::command]
pub async fn list_inventory_articles(
    filter: InventoryArticleFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryArticle>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_articles(&state.db, filter).await
}

#[tauri::command]
pub async fn create_inventory_article(
    input: InventoryArticleInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryArticle> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::create_article(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_article(
    article_id: i64,
    expected_row_version: i64,
    input: InventoryArticleInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryArticle> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::update_article(&state.db, article_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_inventory_stock_balances(
    filter: InventoryStockFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStockBalance>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_stock_balances(&state.db, filter).await
}

#[tauri::command]
pub async fn adjust_inventory_stock(
    input: InventoryStockAdjustInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryStockBalance> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::adjust_stock(&state.db, input).await
}

#[tauri::command]
pub async fn reserve_inventory_stock(
    input: InventoryReserveInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::reserve_stock(&state.db, input).await
}

#[tauri::command]
pub async fn issue_inventory_stock(
    input: InventoryIssueInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::issue_reserved_stock(&state.db, input).await
}

#[tauri::command]
pub async fn return_inventory_stock(
    input: InventoryReturnInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::return_reserved_stock(&state.db, input).await
}

#[tauri::command]
pub async fn transfer_inventory_stock(
    input: InventoryTransferInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStockBalance>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::transfer_stock(&state.db, input).await
}

#[tauri::command]
pub async fn release_inventory_reservation(
    input: InventoryReleaseReservationInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::release_stock_reservation(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_reservations(
    filter: StockReservationFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<StockReservation>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_reservations(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inventory_transactions(
    filter: InventoryTransactionFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryTransaction>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_transactions(&state.db, filter).await
}

#[tauri::command]
pub async fn evaluate_inventory_reorder(
    warehouse_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReorderRecommendation>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::evaluate_reorder(&state.db, warehouse_id).await
}

#[tauri::command]
pub async fn list_inventory_procurement_suppliers(state: State<'_, AppState>) -> AppResult<Vec<ProcurementSupplier>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_procurement_suppliers(&state.db).await
}

#[tauri::command]
pub async fn create_inventory_procurement_requisition(
    input: CreateProcurementRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::create_procurement_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_procurement_requisition(
    input: TransitionProcurementRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::transition_procurement_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_procurement_requisitions(
    state: State<'_, AppState>,
) -> AppResult<Vec<ProcurementRequisition>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_procurement_requisitions(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_procurement_requisition_lines(
    requisition_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ProcurementRequisitionLine>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_procurement_requisition_lines(&state.db, requisition_id).await
}

#[tauri::command]
pub async fn create_inventory_purchase_order_from_requisition(
    input: CreatePurchaseOrderFromRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::create_purchase_order_from_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_purchase_order(
    input: TransitionPurchaseOrderInput,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::transition_purchase_order(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_purchase_orders(state: State<'_, AppState>) -> AppResult<Vec<PurchaseOrder>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_purchase_orders(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_purchase_order_lines(
    purchase_order_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PurchaseOrderLine>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_purchase_order_lines(&state.db, purchase_order_id).await
}

#[tauri::command]
pub async fn get_inventory_purchase_order_detail(
    purchase_order_id: i64,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrderDetail> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::get_purchase_order_detail(&state.db, purchase_order_id).await
}

#[tauri::command]
pub async fn project_inventory_stock_impact(
    article_id: i64,
    warehouse_id: Option<i64>,
    delta_qty: f64,
    include_open_po_qty: bool,
    state: State<'_, AppState>,
) -> AppResult<StockImpactProjection> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::project_stock_impact(&state.db, article_id, warehouse_id, delta_qty, include_open_po_qty).await
}

#[tauri::command]
pub async fn receive_inventory_purchase_order_goods(
    input: ReceiveGoodsInput,
    state: State<'_, AppState>,
) -> AppResult<GoodsReceipt> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::receive_purchase_order_goods(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_goods_receipts(state: State<'_, AppState>) -> AppResult<Vec<GoodsReceipt>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_goods_receipts(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_goods_receipt_lines(
    goods_receipt_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<GoodsReceiptLine>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_goods_receipt_lines(&state.db, goods_receipt_id).await
}

#[tauri::command]
pub async fn update_inventory_procurement_posting_state(
    input: UpdatePostingStateInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ERP_RECONCILE,
        PermissionScope::Global
    );
    procurement::update_procurement_posting_state(&state.db, input).await
}

#[tauri::command]
pub async fn create_inventory_repairable_order(
    input: CreateRepairableOrderInput,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::create_repairable_order(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_repairable_order(
    input: TransitionRepairableOrderInput,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::transition_repairable_order(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_repairable_orders(state: State<'_, AppState>) -> AppResult<Vec<RepairableOrder>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_repairable_orders(&state.db).await
}

#[tauri::command]
pub async fn get_inventory_repairable_order_detail(
    order_id: i64,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrderDetail> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::get_repairable_order_detail(&state.db, order_id).await
}

#[tauri::command]
pub async fn evaluate_inventory_repair_vs_replace(
    order_id: i64,
    state: State<'_, AppState>,
) -> AppResult<RepairVsReplaceResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::evaluate_repair_vs_replace(&state.db, order_id).await
}

#[tauri::command]
pub async fn list_inventory_state_events(
    entity_type: Option<String>,
    entity_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStateEvent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::list_inventory_state_events(&state.db, entity_type, entity_id).await
}

#[tauri::command]
pub async fn create_inventory_count_session(
    input: CreateInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    controls::create_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_count_session(
    input: TransitionInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    controls::transition_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn upsert_inventory_count_line(
    input: UpsertInventoryCountLineInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountLine> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    controls::upsert_count_line(&state.db, input).await
}

#[tauri::command]
pub async fn approve_inventory_count_line(
    input: ApproveInventoryCountLineInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountLine> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    controls::approve_count_line(&state.db, input).await
}

#[tauri::command]
pub async fn post_inventory_count_session(
    input: PostInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    controls::post_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn reverse_inventory_count_session(
    input: ReverseInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    controls::reverse_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_count_sessions(state: State<'_, AppState>) -> AppResult<Vec<InventoryCountSession>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    controls::list_count_sessions(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_count_lines(
    session_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryCountLine>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    controls::list_count_lines(&state.db, session_id).await
}

#[tauri::command]
pub async fn run_inventory_reconciliation(
    input: RunInventoryReconciliationInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryReconciliationRun> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ERP_RECONCILE,
        PermissionScope::Global
    );
    controls::run_reconciliation(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_reconciliation_runs(
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReconciliationRun>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    controls::list_reconciliation_runs(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_reconciliation_findings(
    run_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReconciliationFinding>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    controls::list_reconciliation_findings(&state.db, run_id).await
}

// ── Supplier commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_inventory_suppliers(state: State<'_, AppState>) -> AppResult<Vec<InventorySupplier>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::list_suppliers(&state.db).await
}

#[tauri::command]
pub async fn get_inventory_supplier(supplier_id: i64, state: State<'_, AppState>) -> AppResult<InventorySupplier> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::get_supplier_by_id(&state.db, supplier_id).await
}

#[tauri::command]
pub async fn upsert_inventory_supplier(
    supplier_id: Option<i64>,
    expected_row_version: Option<i64>,
    input: InventorySupplierInput,
    state: State<'_, AppState>,
) -> AppResult<InventorySupplier> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::upsert_supplier(&state.db, supplier_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn deactivate_inventory_supplier(
    supplier_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<InventorySupplier> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::soft_delete_supplier(&state.db, supplier_id, expected_row_version).await
}

#[tauri::command]
pub async fn list_supplier_article_sources(
    supplier_id: Option<i64>,
    article_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SupplierArticleSource>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::list_supplier_article_sources(&state.db, supplier_id, article_id).await
}

#[tauri::command]
pub async fn upsert_supplier_article_source(
    input: SupplierArticleSourceInput,
    state: State<'_, AppState>,
) -> AppResult<SupplierArticleSource> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::upsert_supplier_article_source(&state.db, input).await
}

#[tauri::command]
pub async fn delete_supplier_article_source(source_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::delete_supplier_article_source(&state.db, source_id).await
}

#[tauri::command]
pub async fn list_supplier_prices(
    supplier_id: i64,
    article_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SupplierPrice>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::list_supplier_prices(&state.db, supplier_id, article_id).await
}

#[tauri::command]
pub async fn upsert_supplier_price(
    price_id: Option<i64>,
    input: SupplierPriceInput,
    state: State<'_, AppState>,
) -> AppResult<SupplierPrice> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::upsert_supplier_price(&state.db, price_id, input).await
}

#[tauri::command]
pub async fn get_inventory_supplier_scorecard(
    supplier_id: i64,
    state: State<'_, AppState>,
) -> AppResult<SupplierScorecard> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::get_supplier_scorecard(&state.db, supplier_id).await
}

#[tauri::command]
pub async fn list_supplier_contacts(supplier_id: i64, state: State<'_, AppState>) -> AppResult<Vec<SupplierContact>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::list_supplier_contacts(&state.db, supplier_id).await
}

#[tauri::command]
pub async fn upsert_supplier_contact(
    contact_id: Option<i64>,
    input: SupplierContactInput,
    state: State<'_, AppState>,
) -> AppResult<SupplierContact> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::upsert_supplier_contact(&state.db, contact_id, input).await
}

#[tauri::command]
pub async fn delete_supplier_contact(contact_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    suppliers::delete_supplier_contact(&state.db, contact_id).await
}

#[tauri::command]
pub async fn list_supplier_purchase_history(
    supplier_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SupplierPurchaseHistoryRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    suppliers::list_supplier_purchase_history(&state.db, supplier_id, limit).await
}

// ── Article equivalent commands ──────────────────────────────────────────────

#[tauri::command]
pub async fn list_inventory_article_equivalents(
    article_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ArticleEquivalent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_article_equivalents(&state.db, article_id).await
}

#[tauri::command]
pub async fn upsert_inventory_article_equivalent(
    input: ArticleEquivalentInput,
    state: State<'_, AppState>,
) -> AppResult<ArticleEquivalent> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::upsert_article_equivalent(&state.db, input).await
}

#[tauri::command]
pub async fn delete_inventory_article_equivalent(equivalent_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::delete_article_equivalent(&state.db, equivalent_id).await
}

// ── Article purchase history ──────────────────────────────────────────────────

#[tauri::command]
pub async fn list_inventory_article_purchase_history(
    article_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ArticlePurchaseHistoryRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_article_purchase_history(&state.db, article_id).await
}

// ── Replenishment ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn evaluate_inventory_replenishment(
    warehouse_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReplenishmentRecommendation>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::evaluate_replenishment(&state.db, warehouse_id).await
}

// ── ABC / XYZ classification ──────────────────────────────────────────────────

#[tauri::command]
pub async fn calculate_inventory_abc(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::calculate_abc_classification(&state.db).await
}

#[tauri::command]
pub async fn calculate_inventory_xyz(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::calculate_xyz_classification(&state.db).await
}

// ── Procurement dashboard ────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_procurement_dashboard_summary(state: State<'_, AppState>) -> AppResult<ProcurementDashboardSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::get_procurement_dashboard_summary(&state.db).await
}

#[tauri::command]
pub async fn get_procurement_alerts(state: State<'_, AppState>) -> AppResult<Vec<ProcurementAlert>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::get_procurement_alerts(&state.db).await
}

#[tauri::command]
pub async fn get_article_consumption_monthly(
    article_id: i64,
    months: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ArticleConsumptionMonth>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::get_article_consumption_monthly(&state.db, article_id, months).await
}

// ── Repairable history ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_inventory_article_repairable_history(
    article_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ArticleRepairableHistory>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::get_article_repairable_history(&state.db, article_id).await
}

// ── Document links ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_inventory_document_links(
    entity_type: String,
    entity_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryDocumentLink>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::list_inventory_document_links(&state.db, &entity_type, entity_id).await
}

#[tauri::command]
pub async fn upsert_inventory_document_link(
    input: InventoryDocumentLinkInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryDocumentLink> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::upsert_inventory_document_link(&state.db, input).await
}

// ── WO material readiness ────────────────────────────────────────────────────

#[tauri::command]
pub async fn check_wo_part_stock_availability(
    work_order_id: i64,
    state: State<'_, AppState>,
) -> AppResult<WoMaterialReadiness> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    procurement::check_wo_part_stock_availability(&state.db, work_order_id).await
}

#[tauri::command]
pub async fn create_procurement_requisition_from_wo_part(
    work_order_id: i64,
    article_id: i64,
    requested_qty: f64,
    preferred_location_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::create_procurement_requisition_from_wo_part(
        &state.db,
        work_order_id,
        article_id,
        requested_qty,
        preferred_location_id,
        Some(i64::from(user.user_id)),
    )
    .await
}

#[tauri::command]
pub async fn suggest_inventory_internal_transfer(
    article_id: i64,
    target_warehouse_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStockBalance>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    queries::suggest_internal_transfer(&state.db, article_id, target_warehouse_id).await
}
