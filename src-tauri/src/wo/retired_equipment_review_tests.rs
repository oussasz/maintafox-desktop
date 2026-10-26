//! AU-73b — work orders and requests crossed in sync with the retirement of their equipment
//! (`wo::retired_equipment_review`), through the real apply path (`sync::row_codec::apply_row`).
//! Seeded databases copied from one template share every reference row (statuses, users, the test
//! equipment), so one plays the PC that sends and the others the PCs that receive.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{Map, Value};

use super::closeout_tests::tests::{admin_id, create_second_user, seed_test_equipment, setup};
use super::retired_equipment_review::{
    self as review, RetiredEquipmentReview, ReviewKey, CLOSURE_DI_CONVERTED, CLOSURE_DI_REJECTED,
    CLOSURE_RECOMMISSIONED, CLOSURE_WO_CANCELLED, CLOSURE_WO_CLOSED, NOTIFICATION_CATEGORY,
};
use crate::auth::scope::AccessScope;
use crate::sync::materialize::ApplyOutcome;

type Payload = Map<String, Value>;

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql)
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("count")
}

async fn text(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql)
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
}

/// A PC: the template, the test equipment (EQ-CO-001, node 1, in service), the status tags.
async fn pc() -> DatabaseConnection {
    let db = setup().await;
    seed_test_equipment(&db).await;
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
        .await
        .expect("status tags");
    db
}

async fn retire_here(db: &DatabaseConnection) {
    exec(
        db,
        "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED', decommissioned_at = ? WHERE id = 1",
        vec![now().into()],
    )
    .await;
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

async fn wire(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    crate::permit::sync_check::local_wire(db, entity_type, sync_id)
        .await
        .expect("payload")
        .expect("row")
}

/// Applies a received row as the inbox pass does: kept only when applied.
async fn apply(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        sync_id,
        "upsert",
        &serde_json::to_string(payload).expect("json"),
    )
    .await
    .expect("apply");
    if matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged) {
        tx.commit().await.expect("commit");
    } else {
        tx.rollback().await.expect("rollback");
    }
    crate::sync::row_codec::clear_ref_cache();
    outcome
}

fn wo_input(actor: i64) -> crate::wo::domain::WoCreateInput {
    crate::wo::domain::WoCreateInput {
        type_code: "corrective".into(),
        equipment_id: Some(1),
        location_id: None,
        source_di_id: None,
        source_inspection_anomaly_id: None,
        source_ram_ishikawa_diagram_id: None,
        source_ishikawa_flow_node_id: None,
        source_rca_cause_text: None,
        entity_id: None,
        planner_id: Some(actor),
        urgency_code: Some("normal".into()),
        subtype_code: None,
        title: "Pump seal".into(),
        description: None,
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: Some(1.0),
        creator_id: actor,
        requires_permit: None,
    }
}

/// A work order created on `source` (equipment in service there): `(id, sync id)`.
async fn work_order_on(source: &DatabaseConnection) -> (i64, String) {
    let wo = crate::wo::queries::create_work_order(source, wo_input(admin_id(source).await))
        .await
        .expect("work order");
    let sid = text(
        source,
        "SELECT entity_sync_id FROM work_orders WHERE id = ?",
        vec![wo.id.into()],
    )
    .await
    .expect("sync id");
    (wo.id, sid)
}

async fn di_input(db: &DatabaseConnection, actor: i64) -> crate::di::queries::DiCreateInput {
    crate::di::queries::DiCreateInput {
        equipment_id: 1,
        org_node_id: 1,
        title: "Pump vibration".into(),
        description: "Vibration on the pump".into(),
        origin_type: "operator".into(),
        request_type: "repair".into(),
        symptom_code_id: crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("symptom"),
        impact_level: "unknown".into(),
        production_impact: false,
        safety_flag: false,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: "normal".into(),
        observed_at: None,
        submitter_id: actor,
        source_inspection_anomaly_id: None,
    }
}

/// A request created on `source` (equipment in service there), its reviewer set: `(id, sync id)`.
async fn request_on(source: &DatabaseConnection) -> (i64, String) {
    let actor = admin_id(source).await;
    let di = crate::di::queries::create_intervention_request(source, di_input(source, actor).await)
        .await
        .expect("request");
    exec(
        source,
        "UPDATE intervention_requests SET reviewer_id = ? WHERE id = ?",
        vec![actor.into(), di.id.into()],
    )
    .await;
    let sid = text(
        source,
        "SELECT entity_sync_id FROM intervention_requests WHERE id = ?",
        vec![di.id.into()],
    )
    .await
    .expect("sync id");
    (di.id, sid)
}

async fn local_id(db: &DatabaseConnection, table: &str, sid: &str) -> i64 {
    scalar(
        db,
        &format!("SELECT id FROM {table} WHERE entity_sync_id = ?"),
        vec![sid.into()],
    )
    .await
}

async fn list(db: &DatabaseConnection, include_closed: bool) -> Vec<RetiredEquipmentReview> {
    review::list_reviews(db, &AccessScope::Everywhere, &AccessScope::Everywhere, include_closed)
        .await
        .expect("list")
        .items
}

async fn item(db: &DatabaseConnection, key: &ReviewKey) -> Vec<RetiredEquipmentReview> {
    list(db, true)
        .await
        .into_iter()
        .filter(|i| i.review_key == key.key())
        .collect()
}

/// One round of the watcher, as the notification scheduler runs it.
async fn watch(db: &DatabaseConnection) -> crate::notifications::watchers::WatchReport {
    let tx = db.begin().await.expect("tx");
    crate::sync::capture::begin_suppression(&tx).await.expect("suppress");
    let report = crate::notifications::watchers::run_watcher(&tx, NOTIFICATION_CATEGORY, chrono::Utc::now(), true)
        .await
        .expect("watcher");
    crate::sync::capture::end_suppression(&tx).await.expect("unsuppress");
    tx.commit().await.expect("commit");
    report
}

/// `(events, open events)` of the watcher.
async fn events(db: &DatabaseConnection) -> (i64, i64) {
    (
        scalar(
            db,
            "SELECT COUNT(*) FROM notification_events WHERE category_code = ?",
            vec![NOTIFICATION_CATEGORY.into()],
        )
        .await,
        scalar(
            db,
            "SELECT COUNT(*) FROM notification_events WHERE category_code = ? AND resolved_at IS NULL",
            vec![NOTIFICATION_CATEGORY.into()],
        )
        .await,
    )
}

async fn notified(db: &DatabaseConnection, user: i64) -> i64 {
    scalar(
        db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = ? AND n.recipient_user_id = ?",
        vec![NOTIFICATION_CATEGORY.into(), user.into()],
    )
    .await
}

async fn admin_name(db: &DatabaseConnection) -> String {
    text(
        db,
        "SELECT display_name FROM user_accounts WHERE id = ?",
        vec![admin_id(db).await.into()],
    )
    .await
    .expect("admin display name")
}

/// The second user is the maintenance owner of node 1 (the equipment's site): the asset's
/// responsible person (`entity_manager` audience).
async fn maintenance_owner(db: &DatabaseConnection) -> i64 {
    let user = create_second_user(db).await;
    exec(
        db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('T-OWN', 'Owner', 'available')",
        vec![],
    )
    .await;
    let person = scalar(db, "SELECT id FROM personnel WHERE employee_code = 'T-OWN'", vec![]).await;
    exec(
        db,
        "UPDATE user_accounts SET personnel_id = ? WHERE id = ?",
        vec![person.into(), user.into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO org_node_responsibilities (node_id, responsibility_type, person_id, valid_from, valid_to, \
         created_at, updated_at) VALUES (1, 'MAINTENANCE_OWNER', ?, NULL, NULL, 'now', 'now')",
        vec![person.into()],
    )
    .await;
    user
}

// ── (a) a received work order on retired equipment ──────────────────────────────────────────────

#[tokio::test]
async fn a_work_order_received_on_retired_equipment_is_applied_with_one_item_and_one_notice() {
    let source = pc().await;
    let target = pc().await;
    let owner = maintenance_owner(&target).await;
    retire_here(&target).await;
    watch(&target).await; // the watch starts before the work order exists
    let (_, sid) = work_order_on(&source).await;

    let outcome = apply(&target, "work_orders", &sid, &wire(&source, "work_orders", &sid).await).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "a field fact: {outcome:?}");
    let key = ReviewKey::work_order(&sid);
    let items = item(&target, &key).await;
    assert_eq!(items.len(), 1, "one item");
    assert_eq!(items[0].status, "open");
    assert_eq!(items[0].equipment_code.as_deref(), Some("EQ-CO-001"));
    assert_eq!(items[0].review_key, format!("wo|{sid}"));

    let report = watch(&target).await;
    assert_eq!(report.raised, 1, "{report:?}");
    assert_eq!(events(&target).await, (1, 1));
    let admin = admin_id(&target).await;
    assert_eq!(notified(&target, admin).await, 1, "the planner");
    assert_eq!(notified(&target, owner).await, 1, "the asset's responsible");
    let message = text(
        &target,
        "SELECT message_key FROM notification_events WHERE category_code = ?",
        vec![NOTIFICATION_CATEGORY.into()],
    )
    .await;
    assert_eq!(message.as_deref(), Some("retired_equipment_wo"));
}

// ── (b) a received request on retired equipment ─────────────────────────────────────────────────

#[tokio::test]
async fn a_request_received_on_retired_equipment_is_applied_with_one_item_and_one_notice() {
    let source = pc().await;
    let target = pc().await;
    let owner = maintenance_owner(&target).await;
    retire_here(&target).await;
    watch(&target).await;
    let (_, sid) = request_on(&source).await;

    let outcome = apply(
        &target,
        "intervention_requests",
        &sid,
        &wire(&source, "intervention_requests", &sid).await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "a field fact: {outcome:?}");
    let key = ReviewKey::request(&sid);
    let items = item(&target, &key).await;
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].status.as_str(), items[0].kind.as_str()),
        ("open", "intervention_request")
    );
    assert_eq!(items[0].review_key, format!("di|{sid}"));

    assert_eq!(watch(&target).await.raised, 1);
    assert_eq!(notified(&target, admin_id(&target).await).await, 1, "the reviewer");
    assert_eq!(notified(&target, owner).await, 1, "the asset's responsible");

    // A request view right held nowhere: the request is not listed.
    let hidden = review::list_reviews(
        &target,
        &AccessScope::Everywhere,
        &AccessScope::Subtrees(Default::default()),
        true,
    )
    .await
    .expect("list");
    assert!(hidden.items.is_empty());
}

// ── the reverse order: the retirement arrives after the open work order ─────────────────────────

#[tokio::test]
async fn a_retirement_received_after_an_open_work_order_opens_its_item() {
    let source = pc().await;
    let target = pc().await;
    let (_, sid) = work_order_on(&source).await;
    apply(&target, "work_orders", &sid, &wire(&source, "work_orders", &sid).await).await;
    assert!(
        item(&target, &ReviewKey::work_order(&sid)).await.is_empty(),
        "in service here"
    );

    retire_here(&source).await;
    let equipment = wire(&source, "equipment", "test-eq-co-001").await;
    let outcome = apply(&target, "equipment", "test-eq-co-001", &equipment).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let open = scalar(
        &target,
        "SELECT COUNT(*) FROM retired_equipment_reviews WHERE record_sync_id = ? AND status = 'open'",
        vec![sid.clone().into()],
    )
    .await;
    assert_eq!(open, 1, "opened by the equipment row's apply");
}

// ── (c) closure: work order closed, cancelled; request rejected, converted; recommissioning ─────

async fn target_with_received_work_order() -> (DatabaseConnection, DatabaseConnection, String) {
    let source = pc().await;
    let target = pc().await;
    retire_here(&target).await;
    let (_, sid) = work_order_on(&source).await;
    apply(&target, "work_orders", &sid, &wire(&source, "work_orders", &sid).await).await;
    assert_eq!(item(&target, &ReviewKey::work_order(&sid)).await[0].status, "open");
    (source, target, sid)
}

#[tokio::test]
async fn cancelling_the_work_order_closes_its_item_with_who_when_and_why() {
    let (_, target, sid) = target_with_received_work_order().await;
    watch(&target).await;
    let wo_id = local_id(&target, "work_orders", &sid).await;
    let wo = crate::wo::queries::get_work_order(&target, wo_id)
        .await
        .expect("get")
        .expect("wo");
    crate::wo::queries::cancel_work_order(
        &target,
        crate::wo::domain::WoCancelInput {
            id: wo_id,
            expected_row_version: wo.row_version,
            actor_id: admin_id(&target).await,
            cancel_reason: "Equipment scrapped".into(),
        },
    )
    .await
    .expect("cancel: an existing action");

    let items = item(&target, &ReviewKey::work_order(&sid)).await;
    assert_eq!(items.len(), 1);
    let closed = &items[0];
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.closure_kind.as_deref(), Some(CLOSURE_WO_CANCELLED));
    assert_eq!(closed.closed_by.as_deref(), Some(admin_name(&target).await.as_str()));
    assert_eq!(closed.closure_note.as_deref(), Some("Equipment scrapped"));
    let cancelled_at = text(
        &target,
        "SELECT cancelled_at FROM work_orders WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    assert_eq!(closed.closed_at, cancelled_at, "the record's own time");
    assert!(list(&target, false).await.is_empty(), "nothing left open");
    let report = watch(&target).await;
    assert_eq!(report.resolved, 1, "the notice is resolved: {report:?}");
}

#[tokio::test]
async fn closing_the_work_order_closes_its_item() {
    let (_, target, sid) = target_with_received_work_order().await;
    let wo_id = local_id(&target, "work_orders", &sid).await;
    let admin = admin_id(&target).await;
    // The close as another PC's rows bring it (the close-out gates are tested on their own).
    exec(
        &target,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'closed'), \
         closed_at = '2026-10-26T10:00:00Z' WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    exec(
        &target,
        "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, actor_id, notes, acted_at) \
         VALUES (?, 'technically_verified', 'closed', 'close', ?, 'Closed out', '2026-10-26T10:00:00Z')",
        vec![wo_id.into(), admin.into()],
    )
    .await;
    let items = item(&target, &ReviewKey::work_order(&sid)).await;
    assert_eq!(items[0].closure_kind.as_deref(), Some(CLOSURE_WO_CLOSED));
    assert_eq!(items[0].closed_at.as_deref(), Some("2026-10-26T10:00:00Z"));
    assert_eq!(items[0].closed_by.as_deref(), Some(admin_name(&target).await.as_str()));
    assert_eq!(items[0].record_code.is_some(), true);
}

#[tokio::test]
async fn returning_the_equipment_to_service_closes_its_items() {
    let (_, target, sid) = target_with_received_work_order().await;
    let row_version = scalar(&target, "SELECT row_version FROM equipment WHERE id = 1", vec![]).await;
    crate::assets::lifecycle::record_lifecycle_event(
        &target,
        crate::assets::lifecycle::RecordLifecycleEventPayload {
            asset_id: 1,
            event_type: "RECOMMISSIONED".into(),
            event_at: Some("2026-10-26T11:00:00Z".into()),
            from_org_node_id: None,
            to_org_node_id: None,
            from_status_code: None,
            to_status_code: Some("ACTIVE_IN_SERVICE".into()),
            from_class_code: None,
            to_class_code: None,
            related_asset_id: None,
            reason_code: None,
            notes: Some("Repaired, back in service".into()),
            approved_by_id: None,
            expected_row_version: Some(row_version),
        },
        admin_id(&target).await as i32,
    )
    .await
    .expect("recommission: the normal lifecycle right");

    let items = item(&target, &ReviewKey::work_order(&sid)).await;
    assert_eq!(items[0].status, "closed");
    assert_eq!(items[0].closure_kind.as_deref(), Some(CLOSURE_RECOMMISSIONED));
    assert_eq!(items[0].closure_note.as_deref(), Some("Repaired, back in service"));
    assert!(items[0].closed_at.is_some());
}

#[tokio::test]
async fn rejecting_the_request_closes_its_item() {
    let source = pc().await;
    let target = pc().await;
    retire_here(&target).await;
    let (source_di, sid) = request_on(&source).await;
    // Taken in review on the other PC (a request is rejected from review).
    exec(
        &source,
        "UPDATE intervention_requests SET status = 'in_review', row_version = row_version + 1 WHERE id = ?",
        vec![source_di.into()],
    )
    .await;
    apply(
        &target,
        "intervention_requests",
        &sid,
        &wire(&source, "intervention_requests", &sid).await,
    )
    .await;
    let di_id = local_id(&target, "intervention_requests", &sid).await;
    let version = scalar(
        &target,
        "SELECT row_version FROM intervention_requests WHERE id = ?",
        vec![di_id.into()],
    )
    .await;
    crate::di::review::reject_di(
        &target,
        crate::di::review::DiRejectInput {
            di_id,
            actor_id: admin_id(&target).await,
            expected_row_version: version,
            reason_code: "Equipment retired, request void".into(),
            notes: None,
        },
    )
    .await
    .expect("reject: an existing action");

    let items = item(&target, &ReviewKey::request(&sid)).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].closure_kind.as_deref(), Some(CLOSURE_DI_REJECTED));
    assert_eq!(items[0].closed_by.as_deref(), Some(admin_name(&target).await.as_str()));
    assert_eq!(items[0].disposition_code.as_deref(), Some("rejected_invalid"));
}

#[tokio::test]
async fn a_request_received_converted_closes_and_its_work_order_carries_its_own_item() {
    let source = pc().await;
    let target = pc().await;
    retire_here(&target).await;
    let (di_id, di_sid) = request_on(&source).await;
    apply(
        &target,
        "intervention_requests",
        &di_sid,
        &wire(&source, "intervention_requests", &di_sid).await,
    )
    .await;
    assert_eq!(item(&target, &ReviewKey::request(&di_sid)).await[0].status, "open");

    // Converted on the other PC before the retirement reached it.
    let (wo_id, wo_sid) = work_order_on(&source).await;
    exec(
        &source,
        "UPDATE intervention_requests SET status = 'closed', disposition_code = 'converted_to_wo', \
         converted_to_wo_id = ?, converted_at = '2026-10-26T09:00:00Z', closed_at = '2026-10-26T09:00:00Z' \
         WHERE id = ?",
        vec![wo_id.into(), di_id.into()],
    )
    .await;
    apply(
        &target,
        "work_orders",
        &wo_sid,
        &wire(&source, "work_orders", &wo_sid).await,
    )
    .await;
    let outcome = apply(
        &target,
        "intervention_requests",
        &di_sid,
        &wire(&source, "intervention_requests", &di_sid).await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    let di_items = item(&target, &ReviewKey::request(&di_sid)).await;
    assert_eq!(di_items[0].closure_kind.as_deref(), Some(CLOSURE_DI_CONVERTED));
    assert_eq!(di_items[0].closed_at.as_deref(), Some("2026-10-26T09:00:00Z"));
    let wo_code = text(
        &target,
        "SELECT code FROM work_orders WHERE entity_sync_id = ?",
        vec![wo_sid.clone().into()],
    )
    .await;
    assert_eq!(
        di_items[0].converted_to_wo_code, wo_code,
        "clos par conversion en OT <n°>"
    );
    let wo_items = item(&target, &ReviewKey::work_order(&wo_sid)).await;
    assert_eq!(wo_items[0].status, "open", "the work order carries its own item");
}

// ── (d) the same key on two PCs, whatever the order of arrival ───────────────────────────────────

#[tokio::test]
async fn two_pcs_derive_the_same_item_key_and_notice_identity() {
    let source = pc().await;
    let (_, sid) = work_order_on(&source).await;
    let wo_row = wire(&source, "work_orders", &sid).await;
    let retired_source = pc().await;
    retire_here(&retired_source).await;
    let equipment = wire(&retired_source, "equipment", "test-eq-co-001").await;

    // PC A: the retirement, then the work order. PC B: the work order, then the retirement.
    let a = pc().await;
    watch(&a).await;
    apply(&a, "equipment", "test-eq-co-001", &equipment).await;
    apply(&a, "work_orders", &sid, &wo_row).await;
    let b = pc().await;
    watch(&b).await;
    apply(&b, "work_orders", &sid, &wo_row).await;
    apply(&b, "equipment", "test-eq-co-001", &equipment).await;

    let key = ReviewKey::work_order(&sid).key();
    for db in [&a, &b] {
        let open = list(db, false).await;
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].review_key, key);
        watch(db).await;
    }
    let identity = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            text(
                &db,
                "SELECT entity_sync_id || '|' || dedupe_key FROM notification_events WHERE category_code = ?",
                vec![NOTIFICATION_CATEGORY.into()],
            )
            .await
        }
    };
    let on_a = identity(&a).await;
    assert!(on_a.is_some());
    assert_eq!(on_a, identity(&b).await, "the same notice on both PCs");
}

// ── (e) idempotent re-apply ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn re_applying_the_same_rows_adds_no_item_and_no_notice() {
    let source = pc().await;
    let target = pc().await;
    retire_here(&target).await;
    watch(&target).await;
    let (_, sid) = work_order_on(&source).await;
    let row = wire(&source, "work_orders", &sid).await;
    for _ in 0..3 {
        apply(&target, "work_orders", &sid, &row).await;
        review::reconcile_all_in(&target).await.expect("reconcile");
        watch(&target).await;
    }
    let rows = scalar(
        &target,
        "SELECT COUNT(*) FROM retired_equipment_reviews WHERE record_sync_id = ?",
        vec![sid.into()],
    )
    .await;
    assert_eq!(rows, 1, "one item");
    assert_eq!(events(&target).await, (1, 1), "one notice");
    assert_eq!(notified(&target, admin_id(&target).await).await, 1, "told once");
}

// ── (f) the local refusal stays ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn on_this_pc_no_work_order_or_request_is_created_on_retired_equipment() {
    let db = pc().await;
    retire_here(&db).await;
    let actor = admin_id(&db).await;
    let wo = crate::wo::queries::create_work_order(&db, wo_input(actor))
        .await
        .expect_err("retired");
    assert!(format!("{wo:?}").contains("GATE_WO_EQUIPMENT_RETIRED"), "{wo:?}");
    let di = crate::di::queries::create_intervention_request(&db, di_input(&db, actor).await)
        .await
        .expect_err("retired");
    assert!(format!("{di:?}").contains("GATE_DI_EQUIPMENT_RETIRED"), "{di:?}");
    assert!(list(&db, true).await.is_empty());
}
