//! Inventory IPC commands.
//!
//! Permission gates:
//! - `inv.view`: read article master and stock balances
//! - `inv.manage`: create/update articles and post adjustments

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::inventory::domain::{
    ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, CreateStockLocationInput, CreateWarehouseInput,
    ArticleFamily, CreateArticleFamilyInput, InventoryArticle, InventoryArticleFilter, InventoryArticleInput,
    InventoryIssueInput, InventoryReleaseReservationInput, InventoryReorderRecommendation, InventoryReserveInput, InventoryReturnInput,
    InventoryCountLine, InventoryCountSession, InventoryReconciliationFinding, InventoryReconciliationRun, InventoryStateEvent,
    InventoryStockAdjustInput, InventoryStockBalance, InventoryStockFilter, InventoryTaxCategory,
    InventoryTaxCategoryInput, InventoryTransaction, InventoryTransactionFilter, InventoryTransferInput, ProcurementRequisition,
    ProcurementRequisitionLine, ProcurementSupplier, PurchaseOrder, PurchaseOrderLine, ReceiveGoodsInput, RepairableOrder, StockLocation,
    RunInventoryReconciliationInput, StockReservation, StockReservationFilter, TransitionInventoryCountSessionInput,
    TransitionProcurementRequisitionInput, TransitionPurchaseOrderInput, TransitionRepairableOrderInput, UpdateArticleFamilyInput,
    UpdatePostingStateInput, UpdateStockLocationInput, UpdateWarehouseInput, UpsertInventoryCountLineInput, Warehouse,
    CreateProcurementRequisitionInput,
    CreatePurchaseOrderFromRequisitionInput, CreateRepairableOrderInput, GoodsReceipt, GoodsReceiptLine,
    PostInventoryCountSessionInput, ReverseInventoryCountSessionInput,
};
use crate::inventory::valuation::ValuationCostResult;
use crate::inventory::{controls, procurement, queries, valuation};
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_inventory_article_families(state: State<'_, AppState>) -> AppResult<Vec<ArticleFamily>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_article_families(&state.db).await
}

#[tauri::command]
pub async fn create_inventory_article_family(
    input: CreateArticleFamilyInput,
    state: State<'_, AppState>,
) -> AppResult<ArticleFamily> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::create_article_family(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_article_family(
    family_id: i64,
    input: UpdateArticleFamilyInput,
    state: State<'_, AppState>,
) -> AppResult<ArticleFamily> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::update_article_family(&state.db, family_id, input).await
}

#[tauri::command]
pub async fn deactivate_inventory_article_family(
    family_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ArticleFamily> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::deactivate_article_family(&state.db, family_id).await
}

#[tauri::command]
pub async fn list_inventory_tax_categories(state: State<'_, AppState>) -> AppResult<Vec<InventoryTaxCategory>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_inventory_tax_categories(&state.db).await
}

#[tauri::command]
pub async fn create_inventory_tax_category(
    input: InventoryTaxCategoryInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryTaxCategory> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
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
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::update_inventory_tax_category(&state.db, tax_category_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn deactivate_inventory_tax_category(
    tax_category_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<InventoryTaxCategory> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::deactivate_inventory_tax_category(&state.db, tax_category_id, expected_row_version).await
}

#[tauri::command]
pub async fn list_inventory_warehouses(state: State<'_, AppState>) -> AppResult<Vec<Warehouse>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_warehouses(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_locations(
    warehouse_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<StockLocation>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_locations(&state.db, warehouse_id).await
}

#[tauri::command]
pub async fn create_inventory_warehouse(
    input: CreateWarehouseInput,
    state: State<'_, AppState>,
) -> AppResult<Warehouse> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::create_warehouse(&state.db, input).await
}

#[tauri::command]
pub async fn update_inventory_warehouse(
    warehouse_id: i64,
    input: UpdateWarehouseInput,
    state: State<'_, AppState>,
) -> AppResult<Warehouse> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::update_warehouse(&state.db, warehouse_id, input).await
}

#[tauri::command]
pub async fn create_inventory_stock_location(
    input: CreateStockLocationInput,
    state: State<'_, AppState>,
) -> AppResult<StockLocation> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
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
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
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
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    valuation::evaluate_unit_cost(&state.db, article_id, warehouse_id, location_id).await
}

#[tauri::command]
pub async fn list_inventory_articles(
    filter: InventoryArticleFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryArticle>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_articles(&state.db, filter).await
}

#[tauri::command]
pub async fn create_inventory_article(
    input: InventoryArticleInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryArticle> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
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
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::update_article(&state.db, article_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_inventory_stock_balances(
    filter: InventoryStockFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStockBalance>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_stock_balances(&state.db, filter).await
}

#[tauri::command]
pub async fn adjust_inventory_stock(
    input: InventoryStockAdjustInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryStockBalance> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::adjust_stock(&state.db, input).await
}

#[tauri::command]
pub async fn reserve_inventory_stock(
    input: InventoryReserveInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::reserve_stock(&state.db, input).await
}

#[tauri::command]
pub async fn issue_inventory_stock(
    input: InventoryIssueInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::issue_reserved_stock(&state.db, input).await
}

#[tauri::command]
pub async fn return_inventory_stock(
    input: InventoryReturnInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::return_reserved_stock(&state.db, input).await
}

#[tauri::command]
pub async fn transfer_inventory_stock(
    input: InventoryTransferInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStockBalance>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::transfer_stock(&state.db, input).await
}

#[tauri::command]
pub async fn release_inventory_reservation(
    input: InventoryReleaseReservationInput,
    state: State<'_, AppState>,
) -> AppResult<StockReservation> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    queries::release_stock_reservation(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_reservations(
    filter: StockReservationFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<StockReservation>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_reservations(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inventory_transactions(
    filter: InventoryTransactionFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryTransaction>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::list_transactions(&state.db, filter).await
}

#[tauri::command]
pub async fn evaluate_inventory_reorder(
    warehouse_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReorderRecommendation>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    queries::evaluate_reorder(&state.db, warehouse_id).await
}

#[tauri::command]
pub async fn list_inventory_procurement_suppliers(state: State<'_, AppState>) -> AppResult<Vec<ProcurementSupplier>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_procurement_suppliers(&state.db).await
}

#[tauri::command]
pub async fn create_inventory_procurement_requisition(
    input: CreateProcurementRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::create_procurement_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_procurement_requisition(
    input: TransitionProcurementRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::transition_procurement_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_procurement_requisitions(state: State<'_, AppState>) -> AppResult<Vec<ProcurementRequisition>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_procurement_requisitions(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_procurement_requisition_lines(
    requisition_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ProcurementRequisitionLine>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_procurement_requisition_lines(&state.db, requisition_id).await
}

#[tauri::command]
pub async fn create_inventory_purchase_order_from_requisition(
    input: CreatePurchaseOrderFromRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::create_purchase_order_from_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_purchase_order(
    input: TransitionPurchaseOrderInput,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::transition_purchase_order(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_purchase_orders(state: State<'_, AppState>) -> AppResult<Vec<PurchaseOrder>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_purchase_orders(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_purchase_order_lines(
    purchase_order_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PurchaseOrderLine>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_purchase_order_lines(&state.db, purchase_order_id).await
}

#[tauri::command]
pub async fn receive_inventory_purchase_order_goods(
    input: ReceiveGoodsInput,
    state: State<'_, AppState>,
) -> AppResult<GoodsReceipt> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::receive_purchase_order_goods(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_goods_receipts(state: State<'_, AppState>) -> AppResult<Vec<GoodsReceipt>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_goods_receipts(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_goods_receipt_lines(
    goods_receipt_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<GoodsReceiptLine>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_goods_receipt_lines(&state.db, goods_receipt_id).await
}

#[tauri::command]
pub async fn update_inventory_procurement_posting_state(
    input: UpdatePostingStateInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "erp.reconcile", PermissionScope::Global);
    procurement::update_procurement_posting_state(&state.db, input).await
}

#[tauri::command]
pub async fn create_inventory_repairable_order(
    input: CreateRepairableOrderInput,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::create_repairable_order(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_repairable_order(
    input: TransitionRepairableOrderInput,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.procure", PermissionScope::Global);
    procurement::transition_repairable_order(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_repairable_orders(state: State<'_, AppState>) -> AppResult<Vec<RepairableOrder>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_repairable_orders(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_state_events(
    entity_type: Option<String>,
    entity_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryStateEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    procurement::list_inventory_state_events(&state.db, entity_type, entity_id).await
}

#[tauri::command]
pub async fn create_inventory_count_session(
    input: CreateInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.count", PermissionScope::Global);
    controls::create_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_count_session(
    input: TransitionInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.count", PermissionScope::Global);
    controls::transition_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn upsert_inventory_count_line(
    input: UpsertInventoryCountLineInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountLine> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.count", PermissionScope::Global);
    controls::upsert_count_line(&state.db, input).await
}

#[tauri::command]
pub async fn approve_inventory_count_line(
    input: ApproveInventoryCountLineInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountLine> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    controls::approve_count_line(&state.db, input).await
}

#[tauri::command]
pub async fn post_inventory_count_session(
    input: PostInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.count", PermissionScope::Global);
    controls::post_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn reverse_inventory_count_session(
    input: ReverseInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.manage", PermissionScope::Global);
    controls::reverse_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_count_sessions(state: State<'_, AppState>) -> AppResult<Vec<InventoryCountSession>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    controls::list_count_sessions(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_count_lines(
    session_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryCountLine>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    controls::list_count_lines(&state.db, session_id).await
}

#[tauri::command]
pub async fn run_inventory_reconciliation(
    input: RunInventoryReconciliationInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryReconciliationRun> {
    let user = require_session!(state);
    require_permission!(state, &user, "erp.reconcile", PermissionScope::Global);
    controls::run_reconciliation(&state.db, input).await
}

#[tauri::command]
pub async fn list_inventory_reconciliation_runs(
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReconciliationRun>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    controls::list_reconciliation_runs(&state.db).await
}

#[tauri::command]
pub async fn list_inventory_reconciliation_findings(
    run_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<InventoryReconciliationFinding>> {
    let user = require_session!(state);
    require_permission!(state, &user, "inv.view", PermissionScope::Global);
    controls::list_reconciliation_findings(&state.db, run_id).await
}
