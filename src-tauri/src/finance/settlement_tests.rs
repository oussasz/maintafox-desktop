//! OT cost flow into the budget, through the real close-out and reopen of a work order:
//! - S1 close posts the OT's labour, parts (ledger issue − return at average cost), contractor
//!   receipts and other costs once, by bucket, and writes the same figures on the OT;
//! - S2 an OT whose cost centre cannot be resolved settles `pending` (no actual), and Finance
//!   posts it once the asset has a cost centre;
//! - S3 reopen reverses the posted actuals; the second close posts the new cycle once;
//! - S4 budget periods: a closed month refuses postings, an unfinished month cannot be closed, a
//!   month with a pending settlement cannot be closed;
//! - S5 cockpit shows budget / committed / actual / forecast and drills down to the OT and PO.

use chrono::{Datelike, Utc};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::finance::cockpit::{self, BudgetCockpitDrilldownFilter, BudgetCockpitFilter};
use crate::finance::periods::{self, CloseBudgetPeriodInput, ReopenBudgetPeriodInput};
use crate::finance::settlement;
use crate::finance::wo_costing;
use crate::inventory::domain::{InventoryArticleInput, InventoryReturnInput, InventoryStockAdjustInput, IssueStockRequest};
use crate::wo::closeout::{
    self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput, WoReopenInput,
};
use crate::wo::domain::WoCreateInput;
use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput};
use crate::wo::labor::{self, AddLaborInput};
use crate::wo::workflow::actions::mark_ready::{mark_wo_ready, WoMarkReadyInput};
use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

const CC_CODE: &str = "CC-MNT-PMP";

// ── Fixtures ───────────────────────────────────────────────────────────────────────────────

async fn setup() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA foreign_keys = ON;".to_string(),
    ))
    .await
    .expect("foreign keys");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db).await.expect("seed");
    db
}

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("exec");
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn scalar_f64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> f64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<f64>(0)
        .expect("f64")
}

fn close_to(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

fn this_year() -> i64 {
    i64::from(Utc::now().year())
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1",
        vec![],
    )
    .await
}

async fn verifier_id(db: &DatabaseConnection) -> i64 {
    exec(
        db,
        "INSERT OR IGNORE INTO user_accounts (entity_sync_id, username, display_name, identity_mode, password_hash,
             is_active, is_admin, force_password_change, failed_login_attempts, created_at, updated_at, row_version)
         VALUES ('fin-verifier', 'fin_verifier', 'Verifier', 'local', 'x', 1, 0, 0, 0,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)",
        vec![],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = 'fin_verifier'",
        vec![],
    )
    .await
}

/// Site, pump asset (optionally on a cost centre), the cost centre and an approved budget for
/// this year: labour 1 200, parts 600, services 2 400 (annual lines).
async fn seed_plant(db: &DatabaseConnection, asset_cost_center: Option<&str>) {
    exec(
        db,
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at)
         VALUES (1, 'fin-model', 1, 'active', datetime('now'), datetime('now'))",
        vec![],
    )
    .await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active,
             created_at, updated_at)
         VALUES (1, 'fin-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        vec![],
    )
    .await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, ancestor_path, depth,
             created_at, updated_at, structure_model_id)
         VALUES (1, 'fin-site', 'SITE-1', 'Site', 1, 'active', '/1/', 0, datetime('now'), datetime('now'), 1)",
        vec![],
    )
    .await;
    exec(
        db,
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status,
             installed_at_node_id, cost_center_code, replacement_value, created_at, updated_at)
         VALUES (1, 'fin-eq-1', 'PMP-001', 'Pompe alimentation', 'active_in_service', 1, ?, 20000.0,
                 datetime('now'), datetime('now'))",
        vec![asset_cost_center.map(str::to_string).into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO cost_centers (entity_sync_id, code, name, is_active) VALUES ('fin-cc', ?, 'Maintenance pompes', 1)",
        vec![CC_CODE.into()],
    )
    .await;
    let currency = crate::inventory::fx::tenant_currency(db).await.expect("tenant currency");
    exec(
        db,
        "INSERT INTO budget_versions (entity_sync_id, fiscal_year, scenario_type, version_no, status, currency_code, title)
         VALUES ('fin-bv', ?, 'approved', 1, 'approved', ?, 'Budget maintenance')",
        vec![this_year().into(), currency.into()],
    )
    .await;
    let version = scalar_i64(
        db,
        "SELECT id FROM budget_versions WHERE entity_sync_id = 'fin-bv'",
        vec![],
    )
    .await;
    let cc = cost_center_id(db).await;
    for (bucket, amount) in [("labor", 1200.0), ("parts", 600.0), ("services", 2400.0)] {
        exec(
            db,
            "INSERT INTO budget_lines (budget_version_id, cost_center_id, period_month, budget_bucket, planned_amount,
                 source_basis)
             VALUES (?, ?, NULL, ?, ?, 'manual')",
            vec![version.into(), cc.into(), bucket.into(), amount.into()],
        )
        .await;
    }
}

async fn cost_center_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM cost_centers WHERE code = ?",
        vec![CC_CODE.into()],
    )
    .await
}

async fn lookup_id(db: &DatabaseConnection, domain_key: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id
         WHERE ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL
         ORDER BY lv.sort_order ASC, lv.id ASC LIMIT 1",
        vec![domain_key.into()],
    )
    .await
}

/// Stock article "bearing": 10 in the main bin at 5.00 (weighted-average cost 5.00).
async fn bearing_article(db: &DatabaseConnection) -> (i64, i64) {
    let input = InventoryArticleInput {
        article_code: "ROUL-6205".to_string(),
        article_name: "Roulement 6205".to_string(),
        family_id: None,
        unit_value_id: lookup_id(db, "inventory.unit_of_measure").await,
        criticality_value_id: None,
        stocking_type_value_id: lookup_id(db, "inventory.stocking_type").await,
        tax_category_value_id: lookup_id(db, "inventory.tax_category").await,
        procurement_category_value_id: None,
        preferred_warehouse_id: None,
        preferred_location_id: None,
        min_stock: 0.0,
        max_stock: None,
        reorder_point: 0.0,
        safety_stock: 0.0,
        manufacturer_name: None,
        manufacturer_part_number: None,
        oem_part_number: None,
        replenishment_policy_code: None,
        economic_order_qty: None,
        minimum_order_qty: None,
        maximum_order_qty: None,
        order_multiple_qty: None,
        lead_time_days: None,
        review_period_days: None,
        abc_class_code: None,
        xyz_class_code: None,
        is_critical_spare: None,
        requires_expiration: Some(false),
        shelf_life_days: None,
        requires_batch_tracking: Some(false),
        is_active: Some(true),
        barcode: None,
        requires_serial_tracking: Some(false),
    };
    let article_id = crate::inventory::queries::create_article(db, input)
        .await
        .expect("article")
        .id;
    let location_id = scalar_i64(
        db,
        "SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1",
        vec![],
    )
    .await;
    crate::inventory::queries::adjust_stock_costed(
        db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 10.0,
            reason_code: Some("ENTREE_ACHAT".to_string()),
            notes: None,
            source_ref: None,
        },
        Some(5.0),
    )
    .await
    .expect("stock in");
    (article_id, location_id)
}

async fn wo_row_version(db: &DatabaseConnection, wo_id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT row_version FROM work_orders WHERE id = ?",
        vec![wo_id.into()],
    )
    .await
}

/// Corrective OT on the pump, in progress, with 2 h of labour at 50.00.
async fn wo_in_progress(db: &DatabaseConnection) -> i64 {
    let actor = admin_id(db).await;
    let wo = crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(1),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "Fuite garniture pompe".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(2.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("create WO");
    let wo = submit_wo(
        db,
        WoSubmitInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit");
    let wo = execution::plan_wo(
        db,
        WoPlanInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            planner_id: actor,
            planned_start: Some("2026-04-10T08:00:00Z".to_string()),
            planned_end: Some("2026-04-10T16:00:00Z".to_string()),
            shift: None,
            expected_duration_hours: Some(2.0),
            planned_downtime_hours: None,
            urgency_id: None,
        },
    )
    .await
    .expect("plan");
    let wo = execution::assign_wo(
        db,
        WoAssignInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            assigned_team_id: None,
            primary_responsible_id: Some(actor),
            scheduled_at: None,
            override_reason: "test".to_string(),
        },
    )
    .await
    .expect("assign");
    let wo = mark_wo_ready(
        db,
        WoMarkReadyInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("ready");
    let wo = execution::start_wo(
        db,
        WoStartInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("start");
    add_labour(db, wo.id, 2.0, "2026-04-10T08:00:00Z", "2026-04-10T10:00:00Z").await;
    wo.id
}

async fn add_labour(db: &DatabaseConnection, wo_id: i64, hours: f64, from: &str, to: &str) {
    labor::add_labor_entry(
        db,
        AddLaborInput {
            wo_id,
            intervener_id: admin_id(db).await,
            skill_id: None,
            started_at: Some(from.into()),
            ended_at: Some(to.into()),
            hours_worked: Some(hours),
            hourly_rate: Some(50.0),
            notes: None,
        },
    )
    .await
    .expect("labour");
}

/// Parts: 3 bearings issued at 5.00, 1 returned → 10.00. Contractor: rewinding received 300.00.
/// Open material order (2 × 40.00) stays a commitment. Other: 50.00 entered on the OT.
async fn add_costs(db: &DatabaseConnection, wo_id: i64) {
    let actor = admin_id(db).await;
    let (article_id, location_id) = bearing_article(db).await;
    let issued = crate::inventory::issue::issue_stock(
        db,
        IssueStockRequest {
            article_id,
            location_id,
            quantity: 3.0,
            work_order_id: Some(wo_id),
            reservation_id: None,
            lot_id: None,
            notes: None,
            actor_id: Some(actor),
        },
    )
    .await
    .expect("issue to OT");
    crate::inventory::queries::return_reserved_stock(
        db,
        InventoryReturnInput {
            reservation_id: issued.reservation_id,
            quantity: 1.0,
            notes: Some("non utilisé".into()),
        },
    )
    .await
    .expect("return");

    exec(
        db,
        "INSERT INTO purchase_orders (po_number, status, approved_at) VALUES ('PO-FIN-1', 'PARTIALLY_RECEIVED', datetime('now'))",
        vec![],
    )
    .await;
    let po = scalar_i64(
        db,
        "SELECT id FROM purchase_orders WHERE po_number = 'PO-FIN-1'",
        vec![],
    )
    .await;
    exec(
        db,
        "INSERT INTO purchase_order_lines (purchase_order_id, article_id, ordered_qty, received_qty, unit_price,
             demand_source_type, demand_source_id, status)
         VALUES (?, ?, 1, 1, 300.0, 'WORK_ORDER', ?, 'CLOSED'),
                (?, ?, 2, 0, 40.0, 'WORK_ORDER_PART', ?, 'OPEN')",
        vec![
            po.into(),
            article_id.into(),
            wo_id.into(),
            po.into(),
            article_id.into(),
            wo_id.into(),
        ],
    )
    .await;
    let service_line = scalar_i64(
        db,
        "SELECT id FROM purchase_order_lines WHERE purchase_order_id = ? AND demand_source_type = 'WORK_ORDER'",
        vec![po.into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO goods_receipts (gr_number, purchase_order_id, status) VALUES ('GR-FIN-1', ?, 'posted')",
        vec![po.into()],
    )
    .await;
    let gr = scalar_i64(
        db,
        "SELECT id FROM goods_receipts WHERE gr_number = 'GR-FIN-1'",
        vec![],
    )
    .await;
    exec(
        db,
        "INSERT INTO goods_receipt_lines (goods_receipt_id, po_line_id, article_id, location_id, received_qty,
             accepted_qty, unit_price, unit_cost_tenant)
         VALUES (?, ?, ?, ?, 1, 1, 300.0, 300.0)",
        vec![
            gr.into(),
            service_line.into(),
            article_id.into(),
            location_id.into(),
        ],
    )
    .await;

    crate::wo::costs::update_service_cost(db, wo_id, 50.0, actor)
        .await
        .expect("other cost");
}

async fn complete_and_close(db: &DatabaseConnection, wo_id: i64) {
    let actor = admin_id(db).await;
    let verifier = verifier_id(db).await;
    let has_detail = scalar_i64(
        db,
        "SELECT COUNT(*) FROM work_order_failure_details WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await;
    if has_detail == 0 {
        crate::wo::parts::confirm_no_parts_used(db, wo_id, actor)
            .await
            .expect("parts confirmed");
        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("symptom lookup")
            .expect("seeded symptom");
        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: Some(symptom_id),
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("Garniture mécanique usée".into()),
            },
        )
        .await
        .expect("failure detail");
        closeout::update_wo_rca(
            db,
            UpdateWoRcaInput {
                wo_id,
                root_cause_summary: Some("Usure".into()),
                corrective_action_summary: Some("Remplacement garniture".into()),
            },
        )
        .await
        .expect("rca");
    }
    let wo = execution::complete_wo_mechanically(
        db,
        WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo_row_version(db, wo_id).await,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("complete");
    let (_v, wo) = closeout::save_verification(
        db,
        SaveVerificationInput {
            wo_id,
            verified_by_id: verifier,
            result: "pass".into(),
            return_to_service_confirmed: true,
            recurrence_risk_level: Some("low".into()),
            notes: None,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("verify");
    let closed = closeout::close_wo(
        db,
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            ..Default::default()
        },
    )
    .await
    .expect("close");
    assert_eq!(closed.status_code.as_deref(), Some("closed"));
}

async fn reopen(db: &DatabaseConnection, wo_id: i64) {
    closeout::reopen_wo(
        db,
        WoReopenInput {
            wo_id,
            actor_id: admin_id(db).await,
            expected_row_version: wo_row_version(db, wo_id).await,
            reason: "Fuite revenue".into(),
            target_status: None,
        },
    )
    .await
    .expect("reopen");
}

async fn settled(db: &DatabaseConnection, wo_id: i64, bucket: &str) -> f64 {
    scalar_f64(
        db,
        "SELECT COALESCE(SUM(amount_base), 0.0) FROM budget_actuals
         WHERE source_type = 'wo_settlement' AND work_order_id = ? AND budget_bucket = ?",
        vec![wo_id.into(), bucket.into()],
    )
    .await
}

async fn net_actuals(db: &DatabaseConnection, wo_id: i64) -> f64 {
    scalar_f64(
        db,
        "SELECT COALESCE(SUM(amount_base), 0.0) FROM budget_actuals WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
}

async fn actual_rows(db: &DatabaseConnection, wo_id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM budget_actuals WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
}

// ── Tests ──────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn s1_close_posts_each_cost_once_and_writes_it_on_the_ot() {
    let db = setup().await;
    seed_plant(&db, Some(CC_CODE)).await;
    let wo_id = wo_in_progress(&db).await;
    add_costs(&db, wo_id).await;

    let before = wo_costing::compute_wo_cost_breakdown(&db, wo_id)
        .await
        .expect("breakdown");
    assert!(close_to(before.labor.actual, 100.0), "labour {:?}", before.labor);
    assert!(
        close_to(before.parts.actual, 10.0),
        "parts = issue − return at average cost: {:?}",
        before.parts_lines
    );
    assert!(close_to(before.services.actual, 300.0), "contractor receipt {:?}", before.services);
    assert!(close_to(before.other.actual, 50.0));
    assert!(close_to(before.parts.committed, 80.0), "open material order {:?}", before.parts);
    assert_eq!(
        before.cost_center.as_ref().map(|c| c.source.as_str()),
        Some("asset")
    );

    complete_and_close(&db, wo_id).await;

    let s = settlement::active_settlement(&db, wo_id)
        .await
        .expect("query")
        .expect("settlement");
    assert_eq!(s.status, "posted", "{s:?}");
    assert_eq!(s.cost_center_code.as_deref(), Some(CC_CODE));
    assert!(close_to(s.total_amount, 460.0), "{s:?}");
    assert!(close_to(settled(&db, wo_id, "labor").await, 100.0));
    assert!(close_to(settled(&db, wo_id, "parts").await, 10.0));
    assert!(close_to(settled(&db, wo_id, "services").await, 300.0));
    assert!(close_to(settled(&db, wo_id, "other").await, 50.0));
    let total_cost = scalar_f64(
        &db,
        "SELECT total_cost FROM work_orders WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    assert!(close_to(total_cost, 460.0), "OT carries the settled total, got {total_cost}");

    // Idempotent: settling the same close again books nothing new.
    let rows = actual_rows(&db, wo_id).await;
    let again = settlement::settle_closed_work_order(&db, wo_id, admin_id(&db).await)
        .await
        .expect("settle again");
    assert_eq!(again.id, s.id);
    assert_eq!(actual_rows(&db, wo_id).await, rows);
}

#[tokio::test]
async fn s2_without_cost_centre_settlement_is_pending_then_posted_by_finance() {
    let db = setup().await;
    seed_plant(&db, None).await;
    let wo_id = wo_in_progress(&db).await;
    complete_and_close(&db, wo_id).await;

    let s = settlement::active_settlement(&db, wo_id)
        .await
        .expect("query")
        .expect("settlement");
    assert_eq!(s.status, "pending");
    assert_eq!(
        s.pending_reason.as_deref(),
        Some(settlement::PENDING_NO_COST_CENTER)
    );
    assert!(close_to(s.total_amount, 100.0), "figures kept on the pending row");
    assert_eq!(actual_rows(&db, wo_id).await, 0, "nothing booked while pending");
    let view = cockpit::get_budget_cockpit(
        &db,
        BudgetCockpitFilter {
            fiscal_year: this_year(),
            ..Default::default()
        },
    )
    .await
    .expect("cockpit");
    assert_eq!(view.pending_settlements, 1, "pending is visible in Finance");

    // Fixing the cause (asset on a cost centre) and retrying posts it.
    exec(
        &db,
        "UPDATE equipment SET cost_center_code = ? WHERE id = 1",
        vec![CC_CODE.into()],
    )
    .await;
    let posted = settlement::retry_pending_settlement(&db, s.id, admin_id(&db).await)
        .await
        .expect("retry");
    assert_eq!(posted.status, "posted");
    assert!(close_to(settled(&db, wo_id, "labor").await, 100.0));
    assert!(
        settlement::retry_pending_settlement(&db, s.id, admin_id(&db).await)
            .await
            .is_err(),
        "a posted settlement cannot be retried"
    );
}

#[tokio::test]
async fn s3_reopen_reverses_and_second_close_posts_once() {
    let db = setup().await;
    seed_plant(&db, Some(CC_CODE)).await;
    let wo_id = wo_in_progress(&db).await;
    complete_and_close(&db, wo_id).await;
    assert!(close_to(settled(&db, wo_id, "labor").await, 100.0));

    reopen(&db, wo_id).await;
    let net = net_actuals(&db, wo_id).await;
    assert!(close_to(net, 0.0), "reversal nets the posted cost to 0, got {net}");
    assert!(settlement::active_settlement(&db, wo_id)
        .await
        .expect("query")
        .is_none());
    let reversed = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM wo_cost_settlements WHERE work_order_id = ? AND status = 'reversed'",
        vec![wo_id.into()],
    )
    .await;
    assert_eq!(reversed, 1);

    // One more hour after reopening, then close again: only the new cycle is posted.
    add_labour(&db, wo_id, 1.0, "2026-04-11T08:00:00Z", "2026-04-11T09:00:00Z").await;
    complete_and_close(&db, wo_id).await;

    let s = settlement::active_settlement(&db, wo_id)
        .await
        .expect("query")
        .expect("second settlement");
    assert_eq!(s.status, "posted");
    assert_eq!(s.settlement_cycle, 1);
    let net = net_actuals(&db, wo_id).await;
    assert!(close_to(net, 150.0), "net = second cycle only (3 h × 50), got {net}");
}

#[tokio::test]
async fn s4_closed_periods_refuse_postings_and_wait_for_pending_settlements() {
    let db = setup().await;
    seed_plant(&db, Some(CC_CODE)).await;
    let wo_id = wo_in_progress(&db).await;
    let last_year = this_year() - 1;
    let actor = admin_id(&db).await;

    assert_eq!(
        periods::first_open_month_from(&db, last_year, 3).await.expect("open"),
        Some(3)
    );
    periods::close_budget_period(
        &db,
        actor,
        CloseBudgetPeriodInput {
            fiscal_year: last_year,
            period_month: 3,
            notes: Some("Clôture mars".into()),
        },
    )
    .await
    .expect("close March of last year");
    assert!(!periods::is_period_open(&db, last_year, 3).await.expect("status"));
    assert_eq!(
        periods::first_open_month_from(&db, last_year, 3).await.expect("open"),
        Some(4),
        "settlements move to the next open month"
    );
    assert!(periods::ensure_period_open(&db, last_year, 3).await.is_err());

    // The current month has not ended: it cannot be closed.
    let month = i64::from(Utc::now().month());
    assert!(periods::close_budget_period(
        &db,
        actor,
        CloseBudgetPeriodInput {
            fiscal_year: this_year(),
            period_month: month,
            notes: None,
        },
    )
    .await
    .is_err());

    // A month with a pending settlement cannot be closed.
    exec(
        &db,
        "INSERT INTO wo_cost_settlements (entity_sync_id, work_order_id, settlement_cycle, status, pending_reason,
             fiscal_year, currency_code, closed_at, created_at, updated_at)
         VALUES ('pend-1', ?, 0, 'pending', 'no_cost_center', ?, 'DZD', ?, datetime('now'), datetime('now'))",
        vec![
            wo_id.into(),
            last_year.into(),
            format!("{last_year}-05-15T10:00:00Z").into(),
        ],
    )
    .await;
    let blocked = periods::close_budget_period(
        &db,
        actor,
        CloseBudgetPeriodInput {
            fiscal_year: last_year,
            period_month: 5,
            notes: None,
        },
    )
    .await;
    assert!(blocked.is_err(), "pending settlement blocks the close");

    let reopened = periods::reopen_budget_period(
        &db,
        actor,
        ReopenBudgetPeriodInput {
            fiscal_year: last_year,
            period_month: 3,
            reason: "Facture tardive".into(),
        },
    )
    .await
    .expect("reopen");
    assert_eq!(reopened[2].status, "open");
}

#[tokio::test]
async fn s5_cockpit_compares_budget_commitment_actual_and_drills_to_documents() {
    let db = setup().await;
    seed_plant(&db, Some(CC_CODE)).await;
    let wo_id = wo_in_progress(&db).await;
    add_costs(&db, wo_id).await;
    complete_and_close(&db, wo_id).await;

    let view = cockpit::get_budget_cockpit(
        &db,
        BudgetCockpitFilter {
            fiscal_year: this_year(),
            ..Default::default()
        },
    )
    .await
    .expect("cockpit");
    let cc = cost_center_id(&db).await;
    let row = |bucket: &str| {
        view.rows
            .iter()
            .find(|r| r.cost_center_id == cc && r.bucket == bucket)
            .cloned()
            .expect("row")
    };
    let services = row("services");
    assert!(close_to(services.budget, 2400.0));
    assert!(close_to(services.actual, 300.0));
    let parts = row("parts");
    assert!(close_to(parts.committed, 80.0), "open PO for the OT is committed: {parts:?}");
    assert!(parts.forecast >= parts.actual + parts.committed - 0.01);
    assert!(close_to(view.totals.actual, 460.0), "{:?}", view.totals);

    let drill = cockpit::get_budget_cockpit_drilldown(
        &db,
        BudgetCockpitDrilldownFilter {
            fiscal_year: this_year(),
            budget_version_id: None,
            cost_center_id: cc,
            bucket: Some("parts".into()),
            period_month: None,
        },
    )
    .await
    .expect("drilldown");
    assert!(drill.actuals.iter().any(|a| a.work_order_id == Some(wo_id)));
    assert!(drill
        .commitments
        .iter()
        .any(|c| c.kind == "purchase_order" && c.reference == "PO-FIN-1"));

    let assets = crate::finance::asset_costs::list_asset_cost_history(&db, Default::default())
        .await
        .expect("asset costs");
    let pump = assets.rows.iter().find(|r| r.equipment_id == 1).expect("pump");
    assert!(close_to(pump.lifetime_cost, 460.0));
    assert!(close_to(pump.cost_12m_to_rav_pct.expect("rav"), 2.3));
    assert_eq!(pump.corrective_work_orders, 1);
}

#[test]
fn forecast_at_completion_follows_the_year() {
    // Past year: what was spent.
    assert!(close_to(
        cockpit::forecast_at_completion(2024, 2026, 1000.0, 0.0, 50.0, 800.0),
        800.0
    ));
    // Current year: actual + committed + budget of the months left.
    assert!(close_to(
        cockpit::forecast_at_completion(2026, 2026, 1200.0, 300.0, 100.0, 700.0),
        1100.0
    ));
    // Future year: the budget unless commitments already exceed it.
    assert!(close_to(
        cockpit::forecast_at_completion(2027, 2026, 1000.0, 0.0, 1500.0, 0.0),
        1500.0
    ));
}
