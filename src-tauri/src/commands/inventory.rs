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
#[allow(clippy::too_many_arguments)]
pub async fn adjust_inventory_stock(
    input: InventoryStockAdjustInput,
    // Unit cost of a stock entry (tenant currency); updates the weighted-average cost.
    unit_cost: Option<f64>,
    // Lot of the movement (tracked articles): an existing lot, or a new lot / serial for an entry.
    lot_id: Option<i64>,
    new_lot: Option<crate::inventory::domain::NewLotInput>,
    // Business date of a back-dated movement (YYYY-MM-DD).
    posting_date: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<InventoryStockBalance> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    queries::adjust_stock_full(
        &state.db,
        input,
        queries::AdjustStockOptions {
            unit_cost,
            lot_id,
            new_lot,
            posting_date,
            actor_id: Some(i64::from(user.user_id)),
        },
    )
    .await
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
    let reservation = queries::issue_reserved_stock(&state.db, input).await?;
    // An issue reaching an already settled OT is a late cost (finance settlement).
    if let Some(wo_id) =
        crate::finance::settlement::work_order_of_source(&reservation.source_type, reservation.source_id)
    {
        crate::finance::settlement::post_late_costs_best_effort(&state.db, &[wo_id], i64::from(user.user_id)).await;
    }
    Ok(reservation)
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
    let reservation = queries::return_reserved_stock_as(&state.db, input, Some(i64::from(user.user_id))).await?;
    // A return to an already settled OT is a late credit (finance settlement).
    if let Some(wo_id) =
        crate::finance::settlement::work_order_of_source(&reservation.source_type, reservation.source_id)
    {
        crate::finance::settlement::post_late_costs_best_effort(&state.db, &[wo_id], i64::from(user.user_id)).await;
    }
    Ok(reservation)
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
    queries::transfer_stock(&state.db, input, Some(i64::from(user.user_id))).await
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
    mut input: CreateProcurementRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
    procurement::create_procurement_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_procurement_requisition(
    mut input: TransitionProcurementRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
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
    mut input: CreatePurchaseOrderFromRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
    procurement::create_purchase_order_from_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_purchase_order(
    mut input: TransitionPurchaseOrderInput,
    state: State<'_, AppState>,
) -> AppResult<PurchaseOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
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
    mut input: ReceiveGoodsInput,
    state: State<'_, AppState>,
) -> AppResult<GoodsReceipt> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
    let receipt = procurement::receive_purchase_order_goods(&state.db, input).await?;
    // A contractor / service receipt reaching an already settled OT is a late cost.
    match crate::finance::settlement::work_orders_of_receipt(&state.db, receipt.id).await {
        Ok(wo_ids) => {
            crate::finance::settlement::post_late_costs_best_effort(&state.db, &wo_ids, i64::from(user.user_id)).await
        }
        Err(e) => tracing::warn!(target: "maintafox", "late cost lookup for receipt {}: {e}", receipt.id),
    }
    Ok(receipt)
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
    mut input: CreateRepairableOrderInput,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
    procurement::create_repairable_order(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_repairable_order(
    mut input: TransitionRepairableOrderInput,
    state: State<'_, AppState>,
) -> AppResult<RepairableOrder> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
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
    mut input: CreateInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
    controls::create_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn transition_inventory_count_session(
    mut input: TransitionInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
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
    mut input: ApproveInventoryCountLineInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountLine> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    // The reviewer is always the signed-in user, never a value sent by the UI.
    input.reviewer_id = i64::from(user.user_id);
    controls::approve_count_line(&state.db, input).await
}

#[tauri::command]
pub async fn post_inventory_count_session(
    mut input: PostInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_COUNT,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
    controls::post_count_session(&state.db, input).await
}

#[tauri::command]
pub async fn reverse_inventory_count_session(
    mut input: ReverseInventoryCountSessionInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryCountSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
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
    mut input: RunInventoryReconciliationInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryReconciliationRun> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ERP_RECONCILE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.actor_id = Some(i64::from(user.user_id));
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
    mut input: InventoryDocumentLinkInput,
    state: State<'_, AppState>,
) -> AppResult<InventoryDocumentLink> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    // Who did it comes from the session, never from the UI payload.
    input.created_by_id = Some(i64::from(user.user_id));
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

// ── Unit conversions (Phase 6.8) ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_unit_conversions(
    article_id: Option<i64>,
    include_inactive: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::inventory::unit_conversions::UnitConversion>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::unit_conversions::list_unit_conversions(&state.db, article_id, include_inactive.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn create_unit_conversion(
    input: crate::inventory::unit_conversions::CreateUnitConversionInput,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::unit_conversions::UnitConversion> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    crate::inventory::unit_conversions::create_unit_conversion(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn update_unit_conversion(
    input: crate::inventory::unit_conversions::UpdateUnitConversionInput,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::unit_conversions::UnitConversion> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    crate::inventory::unit_conversions::update_unit_conversion(&state.db, input).await
}

#[tauri::command]
pub async fn deactivate_unit_conversion(
    id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::unit_conversions::UnitConversion> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    crate::inventory::unit_conversions::deactivate_unit_conversion(&state.db, id, expected_row_version).await
}

#[tauri::command]
pub async fn convert_unit_quantity(
    quantity: f64,
    from_unit_value_id: i64,
    to_unit_value_id: i64,
    article_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::unit_conversions::ConvertedQuantity> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::unit_conversions::convert_quantity(
        &state.db,
        quantity,
        from_unit_value_id,
        to_unit_value_id,
        article_id,
    )
    .await
}

/// The stock quantity a quantity entered in another unit stands for (receipt and issue screens
/// show it while the storekeeper types): same rule and rounding as the posting.
#[tauri::command]
pub async fn preview_inventory_stock_quantity(
    article_id: i64,
    entered_unit_value_id: Option<i64>,
    quantity: f64,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::unit_conversions::StockQuantity> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::unit_conversions::to_stock_quantity(&state.db, article_id, entered_unit_value_id, quantity).await
}

// ── Issue desk (migration 184) ───────────────────────────────────────────────

/// Issues stock to a work order from the store (scan + pick). Same posting path as WO part usage.
#[tauri::command]
pub async fn issue_inventory_stock_to_work_order(
    mut request: crate::inventory::domain::IssueStockRequest,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::IssueOutcome> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    // Who issued comes from the session, never from the UI payload.
    request.actor_id = Some(i64::from(user.user_id));
    let outcome = crate::inventory::issue::issue_stock(&state.db, request).await?;
    if let Some(wo_id) = outcome.work_order_id {
        crate::finance::settlement::post_late_costs_best_effort(&state.db, &[wo_id], i64::from(user.user_id)).await;
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn resolve_inventory_scan(
    value: String,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::ScanResolution> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::issue::resolve_scan(&state.db, value).await
}

#[tauri::command]
pub async fn list_inventory_lot_stock(
    article_id: i64,
    location_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::inventory::domain::LotStock>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::lots::list_lot_stock(&state.db, article_id, location_id).await
}

#[tauri::command]
pub async fn list_inventory_expiring_lots(
    within_days: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::inventory::domain::LotStock>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::lots::list_expiring_lots(&state.db, within_days).await
}

// ── Currencies ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_currency_exchange_rates(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::inventory::domain::CurrencyExchangeRate>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::fx::list_rates(&state.db).await
}

#[tauri::command]
pub async fn create_currency_exchange_rate(
    input: crate::inventory::domain::CurrencyExchangeRateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::CurrencyExchangeRate> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    crate::inventory::fx::create_rate(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn deactivate_currency_exchange_rate(rate_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    crate::inventory::fx::deactivate_rate(&state.db, rate_id).await
}

#[tauri::command]
pub async fn resolve_currency_exchange_rate(
    from_currency: String,
    on_date: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::ResolvedExchangeRate> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::fx::resolve_rate(&state.db, from_currency, on_date).await
}

// ── Valuation reports and periods ────────────────────────────────────────────

#[tauri::command]
pub async fn get_inventory_valuation_report(
    as_at: Option<String>,
    group_by: String,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::ValuationReport> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::periods::valuation_report(&state.db, as_at, group_by).await
}

#[tauri::command]
pub async fn list_inventory_valuation_periods(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::inventory::domain::InventoryValuationPeriod>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::periods::list_periods(&state.db).await
}

#[tauri::command]
pub async fn close_inventory_valuation_period(
    mut input: crate::inventory::domain::CloseValuationPeriodInput,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::InventoryValuationPeriod> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::FIN_POST,
        PermissionScope::Global
    );
    input.actor_id = Some(i64::from(user.user_id));
    crate::inventory::periods::close_period(&state.db, input).await
}

// ── Approval limits ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_procurement_approval_limits(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::inventory::domain::ProcurementApprovalLimit>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    crate::inventory::approvals::list_limits(&state.db).await
}

#[tauri::command]
pub async fn upsert_procurement_approval_limit(
    mut input: crate::inventory::domain::ProcurementApprovalLimitInput,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::ProcurementApprovalLimit> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_ROLES,
        PermissionScope::Global
    );
    input.actor_id = Some(i64::from(user.user_id));
    crate::inventory::approvals::upsert_limit(&state.db, input).await
}

// ── Multi-line requisitions ──────────────────────────────────────────────────

#[tauri::command]
pub async fn create_inventory_multi_line_requisition(
    mut input: crate::inventory::domain::CreateMultiLineRequisitionInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    input.actor_id = Some(i64::from(user.user_id));
    procurement::create_multi_line_requisition(&state.db, input).await
}

#[tauri::command]
pub async fn replace_inventory_requisition_lines(
    mut input: crate::inventory::domain::ReplaceRequisitionLinesInput,
    state: State<'_, AppState>,
) -> AppResult<ProcurementRequisition> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    input.actor_id = Some(i64::from(user.user_id));
    procurement::replace_requisition_lines(&state.db, input).await
}

#[tauri::command]
pub async fn set_inventory_purchase_order_line_price(
    po_line_id: i64,
    unit_price: Option<f64>,
    state: State<'_, AppState>,
) -> AppResult<crate::inventory::domain::PurchaseOrderLine> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_PROCURE,
        PermissionScope::Global
    );
    procurement::set_purchase_order_line_price(&state.db, po_line_id, unit_price).await
}
