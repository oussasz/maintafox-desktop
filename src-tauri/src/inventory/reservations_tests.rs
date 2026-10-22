//! Reservations at the end of a work order (D13), the deterministic release movement across PCs,
//! and the goods receipt that receives the ordered article only (AU-74).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::errors::AppError;
use crate::inventory::domain::{
    CreateProcurementRequisitionInput, CreatePurchaseOrderFromRequisitionInput, InventoryIssueInput,
    InventoryReleaseReservationInput, InventoryReserveInput, InventoryReturnInput, InventoryStockAdjustInput,
    ReceiveGoodsInput, ReceivePurchaseOrderLineInput, TransitionProcurementRequisitionInput,
    TransitionPurchaseOrderInput,
};
use crate::inventory::{procurement, queries};
use crate::wo::closeout::{self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput};
use crate::wo::domain::{WoCancelInput, WoCreateInput};
use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput};
use crate::wo::labor::{self, AddLaborInput};
use crate::wo::parts::{self, AddPartInput};
use crate::wo::workflow::actions::mark_ready::{mark_wo_ready, WoMarkReadyInput};
use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn scalar_text(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> String {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<String>(0)
        .expect("text")
}

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("exec");
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1",
        vec![],
    )
    .await
}

async fn second_user(db: &DatabaseConnection) -> i64 {
    exec(
        db,
        "INSERT OR IGNORE INTO user_accounts \
         (entity_sync_id, username, display_name, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES ('test-resv-verifier', 'resv_verifier', 'Reservation Verifier', 'local', 'no-login-needed', \
                 1, 0, 0, 0, datetime('now'), datetime('now'), 1)",
        vec![],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = 'resv_verifier'",
        vec![],
    )
    .await
}

async fn lookup_value_id(db: &DatabaseConnection, domain_key: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
         WHERE ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL \
         ORDER BY lv.sort_order, lv.id LIMIT 1",
        vec![domain_key.into()],
    )
    .await
}

async fn main_location(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1",
        vec![],
    )
    .await
}

/// Article with `qty` on hand at MAIN-BIN. Returns its id.
async fn stock_article(db: &DatabaseConnection, code: &str, qty: f64) -> i64 {
    let unit = crate::reference::units::unit_id_by_code(db, "U").await.expect("unit U");
    let stocking = lookup_value_id(db, "inventory.stocking_type").await;
    let tax = lookup_value_id(db, "inventory.tax_category").await;
    exec(
        db,
        "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id, \
         stocking_type_value_id, tax_category_value_id, min_stock, reorder_point, safety_stock, \
         is_active, row_version) VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 1, 1)",
        vec![
            format!("art-{code}").into(),
            code.into(),
            format!("Article {code}").into(),
            unit.into(),
            stocking.into(),
            tax.into(),
        ],
    )
    .await;
    let article_id = scalar_i64(db, "SELECT id FROM articles WHERE article_code = ?", vec![code.into()]).await;
    if qty > 0.0 {
        queries::adjust_stock(
            db,
            InventoryStockAdjustInput {
                article_id,
                location_id: main_location(db).await,
                delta_qty: qty,
                reason_code: Some("seed for reservation test".to_string()),
                notes: None,
                source_ref: None,
            },
        )
        .await
        .expect("seed stock");
    }
    article_id
}

/// (on hand, reserved) of the article at MAIN-BIN.
async fn balance(db: &DatabaseConnection, article_id: i64) -> (f64, f64) {
    let location_id = main_location(db).await;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT on_hand_qty, reserved_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await
        .expect("query")
        .expect("balance row");
    (
        row.try_get("", "on_hand_qty").expect("on hand"),
        row.try_get("", "reserved_qty").expect("reserved"),
    )
}

async fn reserve(db: &DatabaseConnection, article_id: i64, qty: f64, source_type: &str, source_id: Option<i64>) -> i64 {
    queries::reserve_stock(
        db,
        InventoryReserveInput {
            article_id,
            location_id: main_location(db).await,
            quantity: qty,
            source_type: source_type.to_string(),
            source_id,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("reserve")
    .id
}

async fn issue(db: &DatabaseConnection, reservation_id: i64, qty: f64) {
    queries::issue_reserved_stock(
        db,
        InventoryIssueInput {
            reservation_id,
            quantity: qty,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("issue");
}

async fn reservation_status(db: &DatabaseConnection, reservation_id: i64) -> String {
    scalar_text(
        db,
        "SELECT status FROM stock_reservations WHERE id = ?",
        vec![reservation_id.into()],
    )
    .await
}

async fn release_movements(db: &DatabaseConnection, reservation_id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM inventory_transactions WHERE reservation_id = ? AND movement_type = 'RELEASE'",
        vec![reservation_id.into()],
    )
    .await
}

/// The audit event of the release of `reservation_id` with `action`, as JSON details.
async fn release_audit(db: &DatabaseConnection, action: &str, reservation_id: i64) -> Vec<serde_json::Value> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT actor_id, details_json FROM audit_events WHERE action_code = ? AND target_type = 'stock_reservation' \
         AND target_id = ?",
        [action.into(), reservation_id.to_string().into()],
    ))
    .await
    .expect("audit query")
    .into_iter()
    .map(|row| {
        let mut details: serde_json::Value =
            serde_json::from_str(&row.try_get::<String>("", "details_json").expect("details")).expect("json");
        details["actor_id"] = row.try_get::<Option<i64>>("", "actor_id").expect("actor").into();
        details
    })
    .collect()
}

async fn seed_equipment(db: &DatabaseConnection) {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
         VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
         VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', datetime('now'), datetime('now'), 1)",
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at) \
         VALUES (1, 'test-eq-resv', 'EQ-RESV-001', 'Reservation equipment', 'active_in_service', 1, datetime('now'), datetime('now'))",
    ] {
        exec(db, sql, vec![]).await;
    }
}

/// A corrective work order in planning: (id, row_version).
async fn wo_in_planning(db: &DatabaseConnection) -> (i64, i64) {
    seed_equipment(db).await;
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
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: "Reservation WO".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(8.0),
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
    .expect("submit to planning");
    (wo.id, wo.row_version)
}

/// A corrective work order in progress: (id, row_version).
async fn wo_in_progress(db: &DatabaseConnection) -> (i64, i64) {
    let actor = admin_id(db).await;
    let (wo_id, rv) = wo_in_planning(db).await;
    let wo = execution::plan_wo(
        db,
        WoPlanInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            planner_id: actor,
            planned_start: Some("2026-04-10T08:00:00Z".to_string()),
            planned_end: Some("2026-04-10T16:00:00Z".to_string()),
            shift: None,
            shift_class_version_id: None,
            clear_shift: false,
            expected_duration_hours: Some(8.0),
            planned_crew_size: None,
            clear_crew_size: false,
            planned_downtime_hours: None,
            urgency_code: None,
        },
    )
    .await
    .expect("plan_wo");
    let wo = execution::assign_wo(
        db,
        WoAssignInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            assigned_team_id: None,
            primary_responsible_id: Some(actor),
            scheduled_at: None,
            override_reason: "test assign".to_string(),
        },
    )
    .await
    .expect("assign_wo");
    let wo = mark_wo_ready(
        db,
        WoMarkReadyInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("mark_wo_ready");
    let wo = execution::start_wo(
        db,
        WoStartInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("start_wo");
    (wo_id, wo.row_version)
}

fn refusal_code(err: AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => panic!("expected a coded refusal, got {other:?}"),
    }
}

// ── D13: close and cancel release what is still reserved ─────────────────────

/// Closing a work order releases, in the close, the stock still reserved for it: a reservation
/// made for the work order (partly issued: only the rest is released) and a reservation reached
/// through one of its part lines. Each release is audited (who, reservation, article, location,
/// quantity, work order); an unrelated reservation is left alone.
#[tokio::test]
async fn closing_a_work_order_releases_both_kinds_of_reservation_with_an_audit_each() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let verifier = second_user(&db).await;
    let a = stock_article(&db, "RESV-CLOSE-A", 10.0).await;
    let b = stock_article(&db, "RESV-CLOSE-B", 10.0).await;
    let (wo_id, rv) = wo_in_progress(&db).await;

    // Made for the work order: 4 reserved, 1 issued → 3 left reserved.
    let for_wo = reserve(&db, a, 4.0, "WO", Some(wo_id)).await;
    issue(&db, for_wo, 1.0).await;
    // Reached through a part line only (made at the store desk, no work-order source).
    let via_line = reserve(&db, b, 2.0, "MANUAL", None).await;
    // Someone else's reservation stays.
    let other = reserve(&db, b, 1.0, "MANUAL", None).await;
    assert_eq!(balance(&db, a).await, (9.0, 3.0));
    assert_eq!(balance(&db, b).await, (10.0, 3.0));

    // Labour, a consumed part (the line the store reservation is attached to), RAMS, completion.
    labor::add_labor_entry(
        &db,
        AddLaborInput {
            wo_id,
            intervener_id: actor,
            skill_id: None,
            started_at: Some("2026-04-10T08:00:00Z".into()),
            ended_at: Some("2026-04-10T10:00:00Z".into()),
            hours_worked: Some(2.0),
            hourly_rate: Some(50.0),
            notes: None,
        },
    )
    .await
    .expect("labour");
    let part = parts::add_planned_part(
        &db,
        AddPartInput {
            wo_id,
            article_id: None,
            article_ref: Some("BEARING-6205".into()),
            quantity_planned: 1.0,
            unit_cost: Some(20.0),
            stock_location_id: None,
            auto_reserve: Some(false),
            notes: None,
            origin: None,
            is_required: true,
        },
    )
    .await
    .expect("part line");
    parts::record_actual_usage(&db, part.id, 1.0, Some(20.0))
        .await
        .expect("usage");
    exec(
        &db,
        "UPDATE work_order_parts SET reservation_id = ? WHERE id = ?",
        vec![via_line.into(), part.id.into()],
    )
    .await;
    let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(&db, "vibration")
        .await
        .expect("lookup symptom")
        .expect("seeded DI.SYMPTOM vibration");
    closeout::save_failure_detail(
        &db,
        SaveFailureDetailInput {
            wo_id,
            symptom_id: Some(symptom_id),
            failure_mode_id: None,
            failure_cause_id: None,
            failure_effect_id: None,
            is_temporary_repair: false,
            is_permanent_repair: true,
            cause_not_determined: true,
            notes: Some("Failure detail for the reservation close test".into()),
        },
    )
    .await
    .expect("failure detail");
    closeout::update_wo_rca(
        &db,
        UpdateWoRcaInput {
            wo_id,
            root_cause_summary: Some("Root cause".into()),
            corrective_action_summary: Some("Corrective action".into()),
        },
    )
    .await
    .expect("rca");
    let wo = execution::complete_wo_mechanically(
        &db,
        WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("complete");
    let (_ver, wo) = closeout::save_verification(
        &db,
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
    .expect("verification");

    let closed = closeout::close_wo(
        &db,
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

    assert_eq!(reservation_status(&db, for_wo).await, "released");
    assert_eq!(reservation_status(&db, via_line).await, "released");
    assert_eq!(reservation_status(&db, other).await, "active", "not this work order's");
    assert_eq!(balance(&db, a).await, (9.0, 0.0), "the 3 left reserved are free again");
    assert_eq!(balance(&db, b).await, (10.0, 1.0), "only the other reservation stays");

    let audit = release_audit(&db, "inventory.reservation.released_at_wo_close", for_wo).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["actor_id"], actor);
    assert_eq!(audit[0]["article_id"], a);
    assert_eq!(audit[0]["location_id"], main_location(&db).await);
    assert_eq!(audit[0]["quantity_released"], 3.0);
    assert_eq!(audit[0]["work_order_id"], wo_id);
    let audit = release_audit(&db, "inventory.reservation.released_at_wo_close", via_line).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["quantity_released"], 2.0);
    assert_eq!(audit[0]["article_id"], b);
    assert!(release_audit(&db, "inventory.reservation.released_at_wo_close", other)
        .await
        .is_empty());
    assert_eq!(release_movements(&db, for_wo).await, 1);
    assert_eq!(release_movements(&db, via_line).await, 1);
}

/// Cancelling a work order releases, in the cancellation, both kinds of reservation, each with its
/// audit event; nothing is released twice.
#[tokio::test]
async fn cancelling_a_work_order_releases_both_kinds_of_reservation_with_an_audit_each() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let a = stock_article(&db, "RESV-CANCEL-A", 10.0).await;
    let b = stock_article(&db, "RESV-CANCEL-B", 10.0).await;
    let (wo_id, rv) = wo_in_planning(&db).await;
    let location_id = main_location(&db).await;

    // A planned part line that reserves (both link kinds at once) and a store reservation that
    // only a part line points at.
    let line = parts::add_planned_part(
        &db,
        AddPartInput {
            wo_id,
            article_id: Some(a),
            article_ref: None,
            quantity_planned: 3.0,
            unit_cost: Some(5.0),
            stock_location_id: Some(location_id),
            auto_reserve: Some(true),
            notes: None,
            origin: None,
            is_required: true,
        },
    )
    .await
    .expect("planned part");
    let planned = line.reservation_id.expect("the planned line reserves");
    let store = reserve(&db, b, 2.0, "MANUAL", None).await;
    let other_line = parts::add_planned_part(
        &db,
        AddPartInput {
            wo_id,
            article_id: None,
            article_ref: Some("REF-ONLY".into()),
            quantity_planned: 1.0,
            unit_cost: Some(1.0),
            stock_location_id: None,
            auto_reserve: Some(false),
            notes: None,
            origin: None,
            is_required: false,
        },
    )
    .await
    .expect("reference line");
    exec(
        &db,
        "UPDATE work_order_parts SET reservation_id = ? WHERE id = ?",
        vec![store.into(), other_line.id.into()],
    )
    .await;
    assert_eq!(balance(&db, a).await.1, 3.0);
    assert_eq!(balance(&db, b).await.1, 2.0);

    let wo = crate::wo::queries::cancel_work_order(
        &db,
        WoCancelInput {
            id: wo_id,
            expected_row_version: rv,
            actor_id: actor,
            cancel_reason: "Doublon".into(),
        },
    )
    .await
    .expect("cancel");
    assert_eq!(wo.status_code.as_deref(), Some("cancelled"));

    assert_eq!(reservation_status(&db, planned).await, "released");
    assert_eq!(reservation_status(&db, store).await, "released");
    assert_eq!(balance(&db, a).await, (10.0, 0.0));
    assert_eq!(balance(&db, b).await, (10.0, 0.0));
    for (reservation, qty, article) in [(planned, 3.0, a), (store, 2.0, b)] {
        let audit = release_audit(&db, "inventory.reservation.released_at_wo_cancel", reservation).await;
        assert_eq!(audit.len(), 1, "one audit event per release");
        assert_eq!(audit[0]["actor_id"], actor);
        assert_eq!(audit[0]["quantity_released"], qty);
        assert_eq!(audit[0]["article_id"], article);
        assert_eq!(audit[0]["work_order_id"], wo_id);
        assert_eq!(release_movements(&db, reservation).await, 1, "released once");
    }
}

/// After a work order is closed (or cancelled), parts returned against its reservation go back to
/// free stock: on hand rises, reserved does not, and the reservation is released. On an open work
/// order a return is still reserved again for the job.
#[tokio::test]
async fn a_return_after_the_work_order_ended_goes_to_free_stock_not_back_to_the_reservation() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let a = stock_article(&db, "RESV-RET-A", 10.0).await;
    let (wo_id, rv) = wo_in_planning(&db).await;

    // Open work order: a return re-reserves (unchanged behaviour).
    let open = reserve(&db, a, 2.0, "WO", Some(wo_id)).await;
    issue(&db, open, 2.0).await;
    assert_eq!(balance(&db, a).await, (8.0, 0.0));
    queries::return_reserved_stock(
        &db,
        InventoryReturnInput {
            reservation_id: open,
            quantity: 1.0,
            notes: None,
        },
    )
    .await
    .expect("return on an open work order");
    assert_eq!(balance(&db, a).await, (9.0, 1.0), "reserved again for the job");
    assert_eq!(reservation_status(&db, open).await, "partial");

    // Fully issued reservation (consumed: not released at the end) and a partly issued one.
    let consumed = reserve(&db, a, 3.0, "WO", Some(wo_id)).await;
    issue(&db, consumed, 3.0).await;
    let partial = reserve(&db, a, 2.0, "WO", Some(wo_id)).await;
    issue(&db, partial, 1.0).await;
    assert_eq!(balance(&db, a).await, (5.0, 2.0));

    crate::wo::queries::cancel_work_order(
        &db,
        WoCancelInput {
            id: wo_id,
            expected_row_version: rv,
            actor_id: actor,
            cancel_reason: "Ended".into(),
        },
    )
    .await
    .expect("cancel");
    // `open` (1 reserved) and `partial` (1 left) are released; `consumed` holds nothing.
    assert_eq!(balance(&db, a).await, (5.0, 0.0));
    assert_eq!(reservation_status(&db, consumed).await, "consumed");

    // The work order is cancelled: returns are free stock, for both states.
    for (reservation, qty) in [(consumed, 2.0), (partial, 1.0)] {
        let before = balance(&db, a).await;
        let after_return = queries::return_reserved_stock(
            &db,
            InventoryReturnInput {
                reservation_id: reservation,
                quantity: qty,
                notes: None,
            },
        )
        .await
        .expect("return after the end of the work order");
        assert_eq!(after_return.status, "released");
        let after = balance(&db, a).await;
        assert_eq!(after.0, before.0 + qty, "the parts are back on the shelf");
        assert_eq!(after.1, before.1, "nothing is reserved again for a closed work order");
    }
    assert_eq!(balance(&db, a).await, (8.0, 0.0));
    // The ledger replays to the same balance on another PC: RETURN (+q on hand, +q reserved)
    // followed by a RELEASE of the same quantity (−q reserved).
    let reserved_from_ledger: f64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE movement_type WHEN 'RESERVE' THEN quantity WHEN 'RETURN' THEN quantity \
                    WHEN 'RELEASE' THEN -quantity WHEN 'ISSUE' THEN -quantity ELSE 0 END), 0) AS r \
             FROM inventory_transactions WHERE article_id = ?",
            [a.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "r")
        .expect("sum");
    assert!(
        reserved_from_ledger.abs() < 1e-9,
        "the ledger replays to 0 reserved, got {reserved_from_ledger}"
    );
}

/// A work order that ended before D13 (status set directly, nothing released then).
async fn end_wo_without_release(db: &DatabaseConnection, wo_id: i64, code: &str) {
    exec(
        db,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = ?) WHERE id = ?",
        vec![code.into(), wo_id.into()],
    )
    .await;
}

async fn seeded_release_count(db: &DatabaseConnection, reservation_id: i64) -> i64 {
    let reservation_sync = scalar_text(
        db,
        "SELECT entity_sync_id FROM stock_reservations WHERE id = ?",
        vec![reservation_id.into()],
    )
    .await;
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM inventory_transactions WHERE entity_sync_id = ?",
        vec![crate::sync::identity::seeded_entity_sync_id("reservation_release", &reservation_sync).into()],
    )
    .await
}

async fn seeded_release_qty(db: &DatabaseConnection, reservation_id: i64) -> f64 {
    let reservation_sync = scalar_text(
        db,
        "SELECT entity_sync_id FROM stock_reservations WHERE id = ?",
        vec![reservation_id.into()],
    )
    .await;
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT quantity FROM inventory_transactions WHERE entity_sync_id = ?",
        [crate::sync::identity::seeded_entity_sync_id("reservation_release", &reservation_sync).into()],
    ))
    .await
    .expect("query")
    .expect("seeded release")
    .try_get("", "quantity")
    .expect("quantity")
}

/// B1: a reservation still open on a work order cancelled before D13 (4 reserved, 1 issued). A
/// return of 1 first releases its unissued 3 (the reservation's seeded RELEASE, audited), then
/// puts the returned part in free stock: reserved 0, on hand +1, one seeded RELEASE(3) and the
/// return's own RELEASE(1).
#[tokio::test]
async fn a_return_on_a_still_open_reservation_of_an_ended_work_order_frees_its_remainder_first() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let a = stock_article(&db, "RESV-B1-A", 10.0).await;
    let (wo_id, _rv) = wo_in_planning(&db).await;
    let reservation = reserve(&db, a, 4.0, "WO", Some(wo_id)).await;
    issue(&db, reservation, 1.0).await;
    assert_eq!(balance(&db, a).await, (9.0, 3.0));
    end_wo_without_release(&db, wo_id, "cancelled").await;
    assert_eq!(
        reservation_status(&db, reservation).await,
        "partial",
        "legacy: still open"
    );

    let returned = queries::return_reserved_stock_as(
        &db,
        InventoryReturnInput {
            reservation_id: reservation,
            quantity: 1.0,
            notes: None,
        },
        Some(actor),
    )
    .await
    .expect("return");
    assert_eq!(returned.status, "released");
    assert_eq!(
        balance(&db, a).await,
        (10.0, 0.0),
        "remainder freed, returned part on the shelf"
    );
    assert_eq!(seeded_release_count(&db, reservation).await, 1);
    assert_eq!(seeded_release_qty(&db, reservation).await, 3.0);
    assert_eq!(
        release_movements(&db, reservation).await,
        2,
        "seeded RELEASE(3) + the return's RELEASE(1)"
    );
    let audit = release_audit(
        &db,
        "inventory.reservation.released_at_return_after_wo_end",
        reservation,
    )
    .await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["actor_id"], actor);
    assert_eq!(audit[0]["quantity_released"], 3.0);
    assert_eq!(audit[0]["work_order_id"], wo_id);
}

/// S2: an issue against a reservation still open on a closed or cancelled work order is refused
/// with INV_ISSUE_WO_ENDED and moves nothing.
#[tokio::test]
async fn an_issue_against_a_reservation_of_an_ended_work_order_is_refused() {
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-ISS-A", 10.0).await;
    let (wo_id, _rv) = wo_in_planning(&db).await;
    let reservation = reserve(&db, a, 2.0, "WO", Some(wo_id)).await;
    end_wo_without_release(&db, wo_id, "closed").await;
    let err = queries::issue_reserved_stock(
        &db,
        InventoryIssueInput {
            reservation_id: reservation,
            quantity: 1.0,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect_err("closed work order");
    assert_eq!(refusal_code(err), queries::reservation_codes::ISSUE_WO_ENDED);
    assert_eq!(balance(&db, a).await, (10.0, 2.0), "nothing issued");
}

/// A movement made on another PC arrives here: the row as the materializer writes it, then the
/// balance hook of `sync::apply_hooks` (`ledger::apply_received_movement`). The reservation row
/// itself is not updated (its new state from the other PC is not here yet).
async fn receive_movement(db: &DatabaseConnection, reservation_id: i64, movement_type: &str, qty: f64, sync_id: &str) {
    exec(
        db,
        "INSERT INTO inventory_transactions (entity_sync_id, article_id, warehouse_id, location_id, reservation_id,
             movement_type, quantity, source_type, source_id, row_version)
         SELECT ?, r.article_id, r.warehouse_id, r.location_id, r.id, ?, ?, r.source_type, r.source_id, 1
         FROM stock_reservations r WHERE r.id = ?",
        vec![sync_id.into(), movement_type.into(), qty.into(), reservation_id.into()],
    )
    .await;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_id, warehouse_id, location_id FROM stock_reservations WHERE id = ?",
            [reservation_id.into()],
        ))
        .await
        .expect("query")
        .expect("reservation");
    crate::inventory::ledger::apply_received_movement(
        db,
        row.try_get("", "article_id").unwrap(),
        row.try_get("", "warehouse_id").unwrap(),
        row.try_get("", "location_id").unwrap(),
        movement_type,
        qty,
    )
    .await
    .expect("apply received movement");
}

// ── Reserved stock is a projection of the ledger ────────────────────────────

fn mv(sync_id: &str, movement_type: &str, quantity: f64) -> crate::inventory::reservations::ReservationMovement {
    crate::inventory::reservations::ReservationMovement {
        entity_sync_id: Some(sync_id.to_string()),
        movement_type: movement_type.to_string(),
        quantity,
    }
}

/// The same movements give exactly the same reserved quantity whatever order they arrived in
/// (summed in sync-id order: the same floating-point result, bit for bit), each reservation is held
/// at 0 at least, its one release makes it 0, and the baseline RESERVE counts only while the
/// reservation has no RESERVE of its own.
#[test]
fn the_projection_is_the_same_whatever_the_order_of_the_movements() {
    use crate::inventory::reservations::{reservation_reserved_from_ledger, BASELINE_SEED_ENTITY, RELEASE_SEED_ENTITY};
    let r = "resv-sync-1";
    let movements = vec![
        mv("m-1", "RESERVE", 0.7),
        mv("m-2", "ISSUE", 0.1),
        mv("m-3", "ISSUE", 0.2),
        mv("m-4", "RETURN", 0.1),
        mv("m-5", "ISSUE", 0.3),
    ];
    let expected = reservation_reserved_from_ledger(Some(r), &movements);
    assert!((expected - 0.2).abs() < 1e-9);
    let mut reversed = movements.clone();
    reversed.reverse();
    let mut rotated = movements.clone();
    rotated.rotate_left(2);
    for order in [reversed, rotated] {
        assert_eq!(
            reservation_reserved_from_ledger(Some(r), &order).to_bits(),
            expected.to_bits(),
            "same bits whatever the order"
        );
    }

    // Over-issue (two PCs issued the same reserved parts): held at 0, never negative.
    let over = vec![mv("a", "RESERVE", 2.0), mv("b", "ISSUE", 2.0), mv("c", "ISSUE", 2.0)];
    assert_eq!(reservation_reserved_from_ledger(Some(r), &over), 0.0);

    // Its one release (any quantity) makes it 0, before or after a late issue.
    let release = crate::sync::identity::seeded_entity_sync_id(RELEASE_SEED_ENTITY, r);
    let released = vec![
        mv("a", "RESERVE", 5.0),
        mv(&release, "RELEASE", 5.0),
        mv("z", "ISSUE", 2.0),
    ];
    assert_eq!(reservation_reserved_from_ledger(Some(r), &released), 0.0);
    let returned_after = vec![
        mv("a", "RESERVE", 5.0),
        mv(&release, "RELEASE", 3.0),
        mv("y", "RETURN", 1.0),
    ];
    assert_eq!(reservation_reserved_from_ledger(Some(r), &returned_after), 0.0);

    // Baseline: counted alone, ignored once the reservation's own RESERVE is there.
    let baseline = crate::sync::identity::seeded_entity_sync_id(BASELINE_SEED_ENTITY, r);
    assert_eq!(
        reservation_reserved_from_ledger(Some(r), &[mv(&baseline, "RESERVE", 3.0)]),
        3.0
    );
    assert_eq!(
        reservation_reserved_from_ledger(
            Some(r),
            &[
                mv(&baseline, "RESERVE", 3.0),
                mv("own", "RESERVE", 4.0),
                mv("i", "ISSUE", 1.0)
            ]
        ),
        3.0,
        "own RESERVE 4 − issue 1, the baseline not counted twice"
    );
}

/// Two PCs issued the same reserved parts: the second issue arrives by sync and is applied (a
/// physical fact, decision D12), the reservation counts 0, never below; another reservation at the
/// same location keeps its whole quantity (the old running total let the over-issue eat it).
#[tokio::test]
async fn an_over_issue_from_two_pcs_leaves_the_reservation_at_zero_not_negative() {
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-OVER-A", 10.0).await;
    let shared = reserve(&db, a, 2.0, "MANUAL", None).await;
    let other = reserve(&db, a, 3.0, "MANUAL", None).await;
    issue(&db, shared, 2.0).await;
    assert_eq!(balance(&db, a).await, (8.0, 3.0));
    receive_movement(&db, shared, "ISSUE", 2.0, "other-pc-issue-1").await;
    assert_eq!(
        balance(&db, a).await,
        (6.0, 3.0),
        "on hand −2, reserved: only the other reservation"
    );
    assert_eq!(reservation_status(&db, other).await, "active");
}

/// A late issue arriving after the reservation's release, or before it, gives the same reserved
/// stock (0): the release closes the reservation whatever arrives later.
#[tokio::test]
async fn a_late_issue_after_or_before_the_release_gives_the_same_reserved_stock() {
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-ORDER-A", 10.0).await;
    let b = stock_article(&db, "RESV-ORDER-B", 10.0).await;
    let late_after = reserve(&db, a, 5.0, "MANUAL", None).await;
    let late_before = reserve(&db, b, 5.0, "MANUAL", None).await;
    let release = |reservation_id: i64| {
        let db = db.clone();
        async move {
            queries::release_stock_reservation(
                &db,
                InventoryReleaseReservationInput {
                    reservation_id,
                    notes: None,
                },
            )
            .await
            .expect("release");
        }
    };
    release(late_after).await;
    receive_movement(&db, late_after, "ISSUE", 2.0, "other-pc-issue-a").await;
    receive_movement(&db, late_before, "ISSUE", 2.0, "other-pc-issue-b").await;
    release(late_before).await;
    assert_eq!(balance(&db, a).await, (8.0, 0.0));
    assert_eq!(balance(&db, b).await, (8.0, 0.0));
}

/// A balance that drifted from the ledger no longer blocks a release: the reservation is released,
/// its one release movement carries what its movements held, and reserved stock is recomputed
/// from the ledger (the drift is repaired).
#[tokio::test]
async fn a_release_with_a_drifted_balance_is_not_refused() {
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-DRIFT-A", 10.0).await;
    let reservation = reserve(&db, a, 3.0, "MANUAL", None).await;
    let other = reserve(&db, a, 2.0, "MANUAL", None).await;
    exec(
        &db,
        "UPDATE stock_balances SET reserved_qty = 1, available_qty = on_hand_qty - 1 WHERE article_id = ?",
        vec![a.into()],
    )
    .await;
    queries::release_stock_reservation(
        &db,
        InventoryReleaseReservationInput {
            reservation_id: reservation,
            notes: None,
        },
    )
    .await
    .expect("released despite the drifted balance");
    assert_eq!(reservation_status(&db, reservation).await, "released");
    assert_eq!(seeded_release_qty(&db, reservation).await, 3.0);
    assert_eq!(
        balance(&db, a).await,
        (10.0, 2.0),
        "recomputed: only the other reservation"
    );
    assert_eq!(reservation_status(&db, other).await, "active");
}

/// Cancelling a work order whose reservation drifted (an issue made on another PC arrived, the
/// reservation row from that PC did not) is not blocked: the reservation is released and the
/// audit shows the difference between the row and what the movements held.
#[tokio::test]
async fn cancelling_with_a_drifted_reservation_succeeds_and_audits_the_shortfall() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let a = stock_article(&db, "RESV-SHORT-A", 10.0).await;
    let (wo_id, rv) = wo_in_planning(&db).await;
    let reservation = reserve(&db, a, 3.0, "WO", Some(wo_id)).await;
    receive_movement(&db, reservation, "ISSUE", 2.0, "other-pc-issue-short").await;
    assert_eq!(balance(&db, a).await, (8.0, 1.0));

    crate::wo::queries::cancel_work_order(
        &db,
        WoCancelInput {
            id: wo_id,
            expected_row_version: rv,
            actor_id: actor,
            cancel_reason: "Drift".into(),
        },
    )
    .await
    .expect("cancel is not blocked");
    assert_eq!(reservation_status(&db, reservation).await, "released");
    assert_eq!(balance(&db, a).await, (8.0, 0.0));
    let audit = release_audit(&db, "inventory.reservation.released_at_wo_cancel", reservation).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["quantity_released"], 1.0);
    assert_eq!(audit[0]["quantity_short"], 2.0);
}

/// Baseline on a database populated before it runs: an open reservation recorded without its
/// RESERVE movement (received through the sync engine before Phase 2.4, which dropped the link)
/// gets one seeded RESERVE for its open quantity; a reservation with its own movements, and a
/// released one, get none. A second run writes nothing. When the reservation's own RESERVE
/// arrives later, the baseline is no longer counted (never twice).
#[tokio::test]
async fn the_baseline_restores_reservations_recorded_without_movements_once() {
    use crate::inventory::reservations::{startup_reservation_upkeep, BASELINE_SEED_ENTITY};
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-BASE-A", 20.0).await;
    let location_id = main_location(&db).await;
    let normal = reserve(&db, a, 2.0, "MANUAL", None).await;
    // Legacy rows: no RESERVE movement linked to them.
    for (sync_id, reserved, issued, status) in [
        ("legacy-open-resv", 4.0, 1.0, "partial"),
        ("legacy-released-resv", 3.0, 0.0, "released"),
    ] {
        exec(
            &db,
            "INSERT INTO stock_reservations (entity_sync_id, article_id, warehouse_id, location_id, source_type,
                 quantity_reserved, quantity_issued, status)
             SELECT ?, ?, warehouse_id, id, 'MANUAL', ?, ?, ? FROM stock_locations WHERE id = ?",
            vec![
                sync_id.into(),
                a.into(),
                reserved.into(),
                issued.into(),
                status.into(),
                location_id.into(),
            ],
        )
        .await;
    }
    let legacy = scalar_i64(
        &db,
        "SELECT id FROM stock_reservations WHERE entity_sync_id = 'legacy-open-resv'",
        vec![],
    )
    .await;
    // The balance kept by increments did not know them either.
    assert_eq!(balance(&db, a).await, (20.0, 2.0));

    let (report, errors) = startup_reservation_upkeep(&db).await;
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(report.baselines, 1, "only the open legacy reservation");
    let baseline_id = crate::sync::identity::seeded_entity_sync_id(BASELINE_SEED_ENTITY, "legacy-open-resv");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM inventory_transactions WHERE entity_sync_id = ? AND reservation_id = ?
               AND movement_type = 'RESERVE' AND quantity = 3",
            vec![baseline_id.into(), legacy.into()],
        )
        .await,
        1,
        "seeded RESERVE of the open quantity 4 − 1"
    );
    assert_eq!(balance(&db, a).await, (20.0, 5.0), "2 + the legacy 3");
    let _ = normal;

    let (again, errors) = startup_reservation_upkeep(&db).await;
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(again.baselines, 0, "idempotent");
    assert_eq!(balance(&db, a).await, (20.0, 5.0));

    // The reservation's own movements arrive after all: counted once.
    receive_movement(&db, legacy, "RESERVE", 4.0, "late-own-reserve").await;
    receive_movement(&db, legacy, "ISSUE", 1.0, "late-own-issue").await;
    assert_eq!(
        balance(&db, a).await,
        (19.0, 5.0),
        "own RESERVE 4 − ISSUE 1; the baseline ignored"
    );
}

/// S2: the startup sweep on a database populated before it runs. Work orders closed or cancelled
/// before D13 keep open reservations (both link kinds); one of those releases had already arrived
/// from another PC (seeded movement present, reservation row still open); one reservation drifted
/// (an issue from another PC arrived without its reservation row): released all the same, the
/// shortfall audited; an open work order is not touched. Each release is audited with the system
/// actor and the reason "D13 upgrade"; a second run does nothing.
#[tokio::test]
async fn the_d13_upgrade_releases_open_reservations_of_ended_work_orders_once() {
    use crate::inventory::reservations::{release_reservations_of_ended_work_orders, D13_UPGRADE_REASON};
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-UPG-A", 20.0).await;
    let b = stock_article(&db, "RESV-UPG-B", 20.0).await;
    let c = stock_article(&db, "RESV-UPG-C", 20.0).await;
    let d = stock_article(&db, "RESV-UPG-D", 20.0).await;
    let location_id = main_location(&db).await;

    // Cancelled before D13: a WO-sourced partial reservation and a planned-line reservation.
    let (cancelled_wo, _) = wo_in_planning(&db).await;
    let wo_sourced = reserve(&db, a, 4.0, "WO", Some(cancelled_wo)).await;
    issue(&db, wo_sourced, 1.0).await;
    let line = parts::add_planned_part(
        &db,
        AddPartInput {
            wo_id: cancelled_wo,
            article_id: Some(b),
            article_ref: None,
            quantity_planned: 2.0,
            unit_cost: Some(5.0),
            stock_location_id: Some(location_id),
            auto_reserve: Some(true),
            notes: None,
            origin: None,
            is_required: true,
        },
    )
    .await
    .expect("planned part");
    let line_reservation = line.reservation_id.expect("reserved");
    // Closed before D13: a store reservation linked through a part line only, whose release
    // already arrived from another PC (seeded movement here, reservation row still active).
    let (closed_wo, _) = wo_in_planning(&db).await;
    let arrived = reserve(&db, c, 5.0, "MANUAL", None).await;
    let ref_line = parts::add_planned_part(
        &db,
        AddPartInput {
            wo_id: closed_wo,
            article_id: None,
            article_ref: Some("REF-UPG".into()),
            quantity_planned: 1.0,
            unit_cost: Some(1.0),
            stock_location_id: None,
            auto_reserve: Some(false),
            notes: None,
            origin: None,
            is_required: false,
        },
    )
    .await
    .expect("reference line");
    exec(
        &db,
        "UPDATE work_order_parts SET reservation_id = ? WHERE id = ?",
        vec![arrived.into(), ref_line.id.into()],
    )
    .await;
    queries::release_stock_reservation(
        &db,
        InventoryReleaseReservationInput {
            reservation_id: arrived,
            notes: None,
        },
    )
    .await
    .expect("stands for the release received from another PC");
    exec(
        &db,
        "UPDATE stock_reservations SET status = 'active', released_at = NULL WHERE id = ?",
        vec![arrived.into()],
    )
    .await;
    // Cancelled before D13, drifted: 3 reserved here, 2 issued on another PC (the issue arrived,
    // the reservation row from that PC did not).
    let (drifted_wo, _) = wo_in_planning(&db).await;
    let drifted = reserve(&db, d, 3.0, "WO", Some(drifted_wo)).await;
    receive_movement(&db, drifted, "ISSUE", 2.0, "other-pc-issue-upg").await;
    // Still open: untouched.
    let (open_wo, _) = wo_in_planning(&db).await;
    let open = reserve(&db, a, 2.0, "WO", Some(open_wo)).await;

    end_wo_without_release(&db, cancelled_wo, "cancelled").await;
    end_wo_without_release(&db, closed_wo, "closed").await;
    end_wo_without_release(&db, drifted_wo, "cancelled").await;
    assert_eq!(balance(&db, a).await, (19.0, 5.0));
    assert_eq!(balance(&db, b).await, (20.0, 2.0));
    assert_eq!(balance(&db, c).await, (20.0, 0.0));
    assert_eq!(balance(&db, d).await, (18.0, 1.0));

    let report = release_reservations_of_ended_work_orders(&db).await.expect("sweep");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.reservations, 4);
    assert_eq!(report.work_orders, 3);

    for reservation in [wo_sourced, line_reservation, arrived, drifted] {
        assert_eq!(reservation_status(&db, reservation).await, "released");
    }
    assert_eq!(reservation_status(&db, open).await, "active", "open work order");
    assert_eq!(balance(&db, a).await, (19.0, 2.0), "3 freed, the open WO keeps its 2");
    assert_eq!(balance(&db, b).await, (20.0, 0.0));
    assert_eq!(
        balance(&db, c).await,
        (20.0, 0.0),
        "the arrived release is not counted twice"
    );
    assert_eq!(balance(&db, d).await, (18.0, 0.0));
    assert_eq!(seeded_release_count(&db, wo_sourced).await, 1);
    assert_eq!(seeded_release_qty(&db, wo_sourced).await, 3.0);
    assert_eq!(seeded_release_count(&db, line_reservation).await, 1);
    assert_eq!(release_movements(&db, arrived).await, 1, "no second movement");

    for (reservation, qty, short, wo) in [
        (wo_sourced, 3.0, 0.0, cancelled_wo),
        (line_reservation, 2.0, 0.0, cancelled_wo),
        (arrived, 0.0, 0.0, closed_wo),
        (drifted, 1.0, 2.0, drifted_wo),
    ] {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT actor_id, auth_context, details_json FROM audit_events \
                 WHERE action_code = 'inventory.reservation.released_d13_upgrade' AND target_id = ?",
                [reservation.to_string().into()],
            ))
            .await
            .expect("audit");
        assert_eq!(rows.len(), 1, "one audit event per release");
        assert_eq!(
            rows[0].try_get::<Option<i64>>("", "actor_id").unwrap(),
            None,
            "system actor"
        );
        assert_eq!(rows[0].try_get::<String>("", "auth_context").unwrap(), "system");
        let details: serde_json::Value =
            serde_json::from_str(&rows[0].try_get::<String>("", "details_json").unwrap()).unwrap();
        assert_eq!(details["reason"], D13_UPGRADE_REASON);
        assert_eq!(details["quantity_released"], qty);
        assert_eq!(details["quantity_short"], short);
        assert_eq!(details["work_order_id"], wo);
    }

    // Idempotent: a second run releases nothing more.
    let again = release_reservations_of_ended_work_orders(&db)
        .await
        .expect("sweep again");
    assert_eq!(again.reservations, 0);
    assert!(again.failures.is_empty());
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.reservation.released_d13_upgrade'",
            vec![],
        )
        .await,
        4
    );
}

// ── Release movement is the same on every PC ────────────────────────────────

/// The RELEASE movement of a reservation carries the sync id derived from the reservation, so the
/// same release made on another PC (same id, even with another quantity) is not applied again here
/// and the reserved stock is lowered once.
#[tokio::test]
async fn a_release_has_the_seeded_sync_id_and_the_same_release_received_again_changes_nothing() {
    use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-SYNC-A", 10.0).await;
    let reservation = reserve(&db, a, 5.0, "MANUAL", None).await;
    queries::release_stock_reservation(
        &db,
        InventoryReleaseReservationInput {
            reservation_id: reservation,
            notes: None,
        },
    )
    .await
    .expect("release");
    assert_eq!(balance(&db, a).await, (10.0, 0.0));

    let reservation_sync = scalar_text(
        &db,
        "SELECT entity_sync_id FROM stock_reservations WHERE id = ?",
        vec![reservation.into()],
    )
    .await;
    let release_sync = scalar_text(
        &db,
        "SELECT entity_sync_id FROM inventory_transactions WHERE reservation_id = ? AND movement_type = 'RELEASE'",
        vec![reservation.into()],
    )
    .await;
    assert_eq!(
        release_sync,
        crate::sync::identity::seeded_entity_sync_id("reservation_release", &reservation_sync)
    );

    // The other PC's release of the same reservation arrives (it had issued nothing: 5, or it had
    // issued 2 first: 3). Same identity: not applied, the reserved stock stays.
    let payload =
        crate::sync::test_support::current_payload(&db, "inventory_transactions", &release_sync, &Default::default())
            .await
            .expect("payload")
            .expect("movement row");
    for quantity in [5.0, 3.0] {
        let mut p = payload.clone();
        p.insert("quantity".into(), quantity.into());
        let outcome = apply_inbound_payload(
            &db,
            "inventory_transactions",
            &release_sync,
            "upsert",
            1,
            &serde_json::Value::Object(p).to_string(),
        )
        .await
        .expect("apply");
        assert!(matches!(outcome, ApplyOutcome::Unchanged), "a known release is a no-op");
        assert_eq!(balance(&db, a).await, (10.0, 0.0), "reserved lowered once");
    }
    assert_eq!(release_movements(&db, reservation).await, 1);
}

/// The other PC's release movement arrived before its reservation row (still active here):
/// releasing here (closing the work order on this PC) closes the reservation without lowering the
/// reserved stock a second time and without a second movement.
#[tokio::test]
async fn releasing_here_after_the_same_release_arrived_from_another_pc_does_not_count_it_twice() {
    let db = crate::db::test_db::seeded().await;
    let a = stock_article(&db, "RESV-SYNC-B", 10.0).await;
    let reservation = reserve(&db, a, 5.0, "MANUAL", None).await;
    queries::release_stock_reservation(
        &db,
        InventoryReleaseReservationInput {
            reservation_id: reservation,
            notes: None,
        },
    )
    .await
    .expect("release (stands for the movement received from the other PC)");
    // The reservation row from the other PC is not applied here yet: still active locally.
    exec(
        &db,
        "UPDATE stock_reservations SET status = 'active', released_at = NULL WHERE id = ?",
        vec![reservation.into()],
    )
    .await;
    assert_eq!(balance(&db, a).await, (10.0, 0.0), "the received movement lowered it");

    let txn = db.begin().await.expect("begin");
    let release = queries::release_reservation_in(&txn, reservation, None, false)
        .await
        .expect("release here");
    txn.commit().await.expect("commit");
    assert!(release.was_open);
    assert!(release.release_received_from_sync);
    assert_eq!(release.quantity_released, 0.0);
    assert_eq!(release.movement_id, None);
    assert_eq!(reservation_status(&db, reservation).await, "released");
    assert_eq!(balance(&db, a).await, (10.0, 0.0), "not lowered twice");
    assert_eq!(release_movements(&db, reservation).await, 1);
}

// ── AU-74: a goods receipt receives the ordered article ─────────────────────

/// The article received is the one on the PO line: another article sent by the screen is refused
/// with INV_GR_ARTICLE_MISMATCH and nothing is received; the ordered article is received.
#[tokio::test]
async fn a_goods_receipt_for_another_article_than_ordered_is_refused() {
    let db = crate::db::test_db::seeded().await;
    let ordered = stock_article(&db, "RESV-GR-ORD", 0.0).await;
    let other = stock_article(&db, "RESV-GR-OTHER", 0.0).await;
    let location_id = main_location(&db).await;
    let supplier_id = match db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM external_companies WHERE is_active = 1 ORDER BY id LIMIT 1".to_string(),
        ))
        .await
        .expect("supplier query")
    {
        Some(row) => row.try_get::<i64>("", "id").expect("id"),
        None => {
            exec(
                &db,
                "INSERT INTO external_companies (name, service_domain, is_active) VALUES ('Supplier GR', 'inventory', 1)",
                vec![],
            )
            .await;
            scalar_i64(
                &db,
                "SELECT id FROM external_companies WHERE name = 'Supplier GR'",
                vec![],
            )
            .await
        }
    };
    let mut req = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id: ordered,
            preferred_location_id: Some(location_id),
            requested_qty: 4.0,
            demand_source_type: "REORDER".to_string(),
            demand_source_id: Some(7474),
            demand_source_ref: Some("ROR-7474".to_string()),
            demand_source_line_id: None,
            source_reservation_id: None,
            source_reorder_trigger: Some("threshold_crossed".to_string()),
            purchase_priority: None,
            reason: Some("article check".to_string()),
            actor_id: None,
        },
    )
    .await
    .expect("requisition");
    for (status, actor) in [("SUBMITTED", None), ("APPROVED", Some(1))] {
        req = procurement::transition_procurement_requisition(
            &db,
            TransitionProcurementRequisitionInput {
                requisition_id: req.id,
                expected_row_version: req.row_version,
                next_status: status.to_string(),
                reason: None,
                note: None,
                actor_id: actor,
            },
        )
        .await
        .expect("requisition transition");
    }
    let mut po = procurement::create_purchase_order_from_requisition(
        &db,
        CreatePurchaseOrderFromRequisitionInput {
            requisition_id: req.id,
            supplier_id: None,
            supplier_company_id: Some(supplier_id),
            actor_id: None,
            currency_code: None,
        },
    )
    .await
    .expect("po");
    for (status, actor) in [("SUBMITTED", None), ("APPROVED", Some(1))] {
        po = procurement::transition_purchase_order(
            &db,
            TransitionPurchaseOrderInput {
                purchase_order_id: po.id,
                expected_row_version: po.row_version,
                next_status: status.to_string(),
                reason: None,
                note: None,
                actor_id: actor,
            },
        )
        .await
        .expect("po transition");
    }
    let po_line = procurement::list_purchase_order_lines(&db, po.id)
        .await
        .expect("po lines")
        .remove(0);
    let receive = |article_id: i64| ReceiveGoodsInput {
        purchase_order_id: po.id,
        lines: vec![ReceivePurchaseOrderLineInput {
            po_line_id: po_line.id,
            article_id,
            location_id,
            received_qty: 4.0,
            accepted_qty: 4.0,
            rejected_qty: 0.0,
            rejection_reason: None,
            lots: None,
            entered_unit_value_id: None,
        }],
        fulfillment_action: None,
        actor_id: None,
        exchange_rate: None,
        posting_date: None,
    };
    let receipts_before = scalar_i64(&db, "SELECT COUNT(*) FROM goods_receipts", vec![]).await;

    let err = procurement::receive_purchase_order_goods(&db, receive(other))
        .await
        .expect_err("another article than ordered");
    assert_eq!(refusal_code(err), procurement::gr_codes::ARTICLE_MISMATCH);
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM goods_receipts", vec![]).await,
        receipts_before,
        "nothing received"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM stock_balances WHERE article_id = ? AND on_hand_qty <> 0",
            vec![other.into()],
        )
        .await,
        0,
        "no stock on the article nobody ordered"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM inventory_transactions WHERE article_id = ? AND movement_type = 'GR_ACCEPT'",
            vec![other.into()],
        )
        .await,
        0
    );

    procurement::receive_purchase_order_goods(&db, receive(ordered))
        .await
        .expect("the ordered article is received");
    assert_eq!(balance(&db, ordered).await.0, 4.0);
}
