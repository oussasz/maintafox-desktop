//! L4-11 — a request flagged "sécurité" (CMMS Advisor decision, 2026-11-06): urgent at least, its
//! reviewers and the permit issuers of its site told at once (a locked safety notice, once per
//! request on each PC, also on sync arrival), first in the work queues, and validated less urgent
//! only with the reason of the review, audited.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{Map, Value};

use crate::di::queries::{
    create_intervention_request, list_intervention_requests, update_di_draft_fields, DiCreateInput, DiDraftUpdateInput,
    DiListFilter,
};
use crate::di::review::{
    screen_di, DiScreenInput, AUDIT_SAFETY_URGENCY_LOWERED, GATE_DI_SAFETY_URGENCY_REASON_REQUIRED,
};
use crate::errors::AppError;
use crate::settings::received_notice::{di_safety_alert_key, DI_SAFETY_ALERT_CATEGORY};
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
        .expect("number")
}

async fn text(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql)
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar(
        db,
        "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1",
        vec![],
    )
    .await
}

/// A PC: a copy of the seeded template (the same reference rows and sync ids on every copy), the
/// test equipment (EQ-SAF-001 on site 1) and a second site (2).
async fn pc() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, \
         updated_at) VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, \
         installed_at_node_id, created_at, updated_at) VALUES (1, 'test-eq-saf-001', 'EQ-SAF-001', \
         'Safety Test Equipment', 'active_in_service', 1, datetime('now'), datetime('now'))",
    ] {
        exec(&db, sql, vec![]).await;
    }
    exec(
        &db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (2, 'test-org-002', 'SITE-002', 'Other Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        vec![],
    )
    .await;
    db
}

async fn input(db: &DatabaseConnection, safety_flag: bool, urgency: &str) -> DiCreateInput {
    DiCreateInput {
        equipment_id: 1,
        org_node_id: 1,
        title: "Fuite de vapeur".into(),
        description: "Fuite de vapeur sur la bride, zone de passage".into(),
        origin_type: "operator".into(),
        request_type: "repair".into(),
        symptom_code_id: crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("symptom"),
        impact_level: "unknown".into(),
        production_impact: false,
        safety_flag,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: urgency.into(),
        observed_at: None,
        submitter_id: admin_id(db).await,
        source_inspection_anomaly_id: None,
    }
}

/// A user holding `permission` through a role assigned on `site` (or plant-wide when `None`).
async fn holder(db: &DatabaseConnection, name: &str, permission: Option<&str>, site: Option<i64>) -> i64 {
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) VALUES (?, ?, ?, 'local', 1, 0, 0, datetime('now'), datetime('now'))",
        vec![format!("{name}-sync").into(), name.into(), name.into()],
    )
    .await;
    let user = scalar(db, "SELECT id FROM user_accounts WHERE username = ?", vec![name.into()]).await;
    let Some(permission) = permission else {
        return user;
    };
    exec(
        db,
        "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at) \
         VALUES (?, ?, '', 0, 'custom', 'active', datetime('now'), datetime('now'))",
        vec![format!("{name}-role").into(), format!("{name}-role").into()],
    )
    .await;
    let role = scalar(
        db,
        "SELECT id FROM roles WHERE name = ?",
        vec![format!("{name}-role").into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO role_permissions (role_id, permission_id, granted_at) \
         SELECT ?, id, datetime('now') FROM permissions WHERE name = ?",
        vec![role.into(), permission.into()],
    )
    .await;
    let (scope_type, reference) = match site {
        Some(node) => ("org_node", Some(node.to_string())),
        None => ("tenant", None),
    };
    exec(
        db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, datetime('now'), datetime('now'))",
        vec![
            format!("{name}-usa").into(),
            user.into(),
            role.into(),
            scope_type.into(),
            reference.into(),
        ],
    )
    .await;
    user
}

async fn alert_events(db: &DatabaseConnection) -> i64 {
    scalar(
        db,
        "SELECT COUNT(*) FROM notification_events WHERE category_code = ?",
        vec![DI_SAFETY_ALERT_CATEGORY.into()],
    )
    .await
}

async fn recipients(db: &DatabaseConnection) -> Vec<i64> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT n.recipient_user_id AS u FROM notifications n \
         JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = ? ORDER BY n.recipient_user_id",
        [DI_SAFETY_ALERT_CATEGORY.into()],
    ))
    .await
    .expect("recipients")
    .iter()
    .map(|r| r.try_get::<i64>("", "u").expect("id"))
    .collect()
}

async fn sync_id(db: &DatabaseConnection, di_id: i64) -> String {
    text(
        db,
        "SELECT entity_sync_id FROM intervention_requests WHERE id = ?",
        vec![di_id.into()],
    )
    .await
    .expect("sync id")
}

async fn row_version(db: &DatabaseConnection, di_id: i64) -> i64 {
    scalar(
        db,
        "SELECT row_version FROM intervention_requests WHERE id = ?",
        vec![di_id.into()],
    )
    .await
}

fn draft(id: i64, expected_row_version: i64) -> DiDraftUpdateInput {
    DiDraftUpdateInput {
        id,
        expected_row_version,
        title: None,
        description: None,
        request_type: None,
        symptom_code_id: None,
        impact_level: None,
        production_impact: None,
        safety_flag: None,
        environmental_flag: None,
        quality_flag: None,
        reported_urgency: None,
        observed_at: None,
    }
}

// ── (a) urgency ─────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_flagged_request_is_stored_urgent_at_least_and_an_emergency_is_kept() {
    let db = pc().await;
    let raised = create_intervention_request(&db, input(&db, true, "normal").await)
        .await
        .expect("flagged normal");
    assert_eq!(raised.reported_urgency, "urgent", "raised, never refused");
    assert!(raised.safety_flag);

    let low = create_intervention_request(&db, input(&db, true, "low").await)
        .await
        .expect("flagged low");
    assert_eq!(low.reported_urgency, "urgent");

    let emergency = create_intervention_request(&db, input(&db, true, "emergency").await)
        .await
        .expect("flagged emergency");
    assert_eq!(
        emergency.reported_urgency, "emergency",
        "the requester may choose more urgent"
    );

    let plain = create_intervention_request(&db, input(&db, false, "normal").await)
        .await
        .expect("not flagged");
    assert_eq!(
        plain.reported_urgency, "normal",
        "an unflagged request keeps its urgency"
    );
}

#[tokio::test]
async fn ticking_the_flag_later_raises_the_urgency_and_alerts_once() {
    let db = pc().await;
    let di = create_intervention_request(&db, input(&db, false, "normal").await)
        .await
        .expect("request");
    assert_eq!(alert_events(&db).await, 0, "no alert without the flag");

    let mut tick = draft(di.id, row_version(&db, di.id).await);
    tick.safety_flag = Some(true);
    let ticked = update_di_draft_fields(&db, tick, Some(admin_id(&db).await))
        .await
        .expect("tick");
    assert_eq!(ticked.reported_urgency, "urgent");
    assert_eq!(alert_events(&db).await, 1);

    // A lower urgency sent with the flag held is raised too.
    let mut lower = draft(di.id, row_version(&db, di.id).await);
    lower.reported_urgency = Some("low".into());
    let kept = update_di_draft_fields(&db, lower, None).await.expect("edit");
    assert_eq!(kept.reported_urgency, "urgent");

    // Ticked again with an edit: never a second alert for the request on this PC.
    let mut again = draft(di.id, row_version(&db, di.id).await);
    again.safety_flag = Some(true);
    again.title = Some("Fuite de vapeur importante".into());
    update_di_draft_fields(&db, again, None).await.expect("tick again");
    assert_eq!(alert_events(&db).await, 1, "once per request");
    assert_eq!(
        text(
            &db,
            "SELECT dedupe_key FROM notification_events WHERE category_code = ?",
            vec![DI_SAFETY_ALERT_CATEGORY.into()],
        )
        .await,
        Some(di_safety_alert_key(&sync_id(&db, di.id).await))
    );
}

#[tokio::test]
async fn a_saved_flagged_request_cannot_be_unticked_and_keeps_its_urgency() {
    let db = pc().await;
    let di = create_intervention_request(&db, input(&db, true, "normal").await)
        .await
        .expect("flagged request");
    assert_eq!(di.reported_urgency, "urgent");
    let version = row_version(&db, di.id).await;

    // Untick with a lower urgency in the same change: refused, nothing written.
    let mut untick = draft(di.id, version);
    untick.safety_flag = Some(false);
    untick.reported_urgency = Some("low".into());
    match update_di_draft_fields(&db, untick, None).await {
        Err(AppError::ValidationFailed(errors)) => assert!(
            errors.iter().any(|e| e.starts_with("GATE_DI_SAFETY_FLAG_KEPT")),
            "{errors:?}"
        ),
        other => panic!("untick must be refused, got {other:?}"),
    }
    let kept = scalar(
        &db,
        "SELECT safety_flag FROM intervention_requests WHERE id = ?",
        vec![di.id.into()],
    )
    .await;
    assert_eq!(kept, 1, "the flag stays");
    assert_eq!(
        text(
            &db,
            "SELECT reported_urgency FROM intervention_requests WHERE id = ?",
            vec![di.id.into()],
        )
        .await
        .as_deref(),
        Some("urgent"),
        "the urgency is unchanged"
    );
    assert_eq!(row_version(&db, di.id).await, version, "nothing written");

    // Untick alone: refused the same way.
    let mut untick = draft(di.id, version);
    untick.safety_flag = Some(false);
    assert!(update_di_draft_fields(&db, untick, None).await.is_err());

    // Sending the flag as it is (ticked) with other edits is accepted.
    let mut edit = draft(di.id, version);
    edit.safety_flag = Some(true);
    edit.title = Some("Fuite de vapeur (bride sud)".into());
    let edited = update_di_draft_fields(&db, edit, None).await.expect("edit");
    assert!(edited.safety_flag);
    assert_eq!(edited.reported_urgency, "urgent");
}

// ── (b) who is told ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_alert_goes_critical_to_the_reviewers_and_permit_issuers_of_its_site_only() {
    let db = pc().await;
    let reviewer = holder(&db, "site_reviewer", Some(crate::rbac::permissions::DI_REVIEW), Some(1)).await;
    let issuer = holder(&db, "site_issuer", Some(crate::rbac::permissions::PTW_ISSUE), Some(1)).await;
    let plant_issuer = holder(&db, "plant_issuer", Some(crate::rbac::permissions::PTW_ISSUE), None).await;
    let other_site = holder(&db, "other_issuer", Some(crate::rbac::permissions::PTW_ISSUE), Some(2)).await;
    let other_reviewer = holder(
        &db,
        "other_reviewer",
        Some(crate::rbac::permissions::DI_REVIEW),
        Some(2),
    )
    .await;
    let plain = holder(&db, "plain_user", None, None).await;

    let di = create_intervention_request(&db, input(&db, true, "normal").await)
        .await
        .expect("flagged");

    let got = recipients(&db).await;
    let mut expected = crate::auth::rbac::elevated_admin_user_ids(&db).await.expect("admins");
    expected.extend([reviewer, issuer, plant_issuer]);
    expected.sort_unstable();
    expected.dedup();
    assert_eq!(got, expected);
    for not_told in [other_site, other_reviewer, plain] {
        assert!(!got.contains(&not_told), "{not_told} is not told");
    }

    let (severity, key, url) = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT e.severity, e.message_key, n.action_url FROM notification_events e \
                 JOIN notifications n ON n.notification_event_id = e.id WHERE e.category_code = ? LIMIT 1",
                [DI_SAFETY_ALERT_CATEGORY.into()],
            ))
            .await
            .expect("event")
            .expect("row");
        (
            row.try_get::<String>("", "severity").unwrap(),
            row.try_get::<String>("", "message_key").unwrap(),
            row.try_get::<String>("", "action_url").unwrap(),
        )
    };
    assert_eq!(severity, "critical");
    assert_eq!(key, DI_SAFETY_ALERT_CATEGORY);
    assert_eq!(url, format!("/requests?openDi={}", di.id));

    // Kept for each recipient until they acknowledge it (a locked safety notice).
    let notices = crate::settings::received_notice::notices_for(&db, reviewer)
        .await
        .expect("notices");
    assert_eq!(notices.len(), 1);
}

#[tokio::test]
async fn nobody_holding_either_right_at_the_site_the_alert_goes_to_its_responsibles() {
    let db = pc().await;
    let owner = holder(&db, "site_owner", None, None).await;
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('T-SAF', 'Owner', 'available')",
        vec![],
    )
    .await;
    let person = scalar(&db, "SELECT id FROM personnel WHERE employee_code = 'T-SAF'", vec![]).await;
    exec(
        &db,
        "UPDATE user_accounts SET personnel_id = ? WHERE id = ?",
        vec![person.into(), owner.into()],
    )
    .await;
    exec(
        &db,
        "INSERT INTO org_node_responsibilities (node_id, responsibility_type, person_id, valid_from, valid_to, \
         created_at, updated_at) VALUES (1, 'MAINTENANCE_OWNER', ?, NULL, NULL, 'now', 'now')",
        vec![person.into()],
    )
    .await;
    let other_site = holder(&db, "other_issuer", Some(crate::rbac::permissions::PTW_ISSUE), Some(2)).await;
    // No administrator active here either (they hold every right).
    let request_input = input(&db, true, "normal").await;
    exec(
        &db,
        "UPDATE user_accounts SET is_active = 0 WHERE id <> ? AND id <> ?",
        vec![owner.into(), other_site.into()],
    )
    .await;
    assert!(crate::auth::rbac::elevated_admin_user_ids(&db)
        .await
        .expect("admins")
        .is_empty());

    create_intervention_request(&db, request_input).await.expect("flagged");
    assert_eq!(recipients(&db).await, vec![owner]);
}

#[tokio::test]
async fn the_alert_cannot_be_muted_and_is_never_emailed() {
    let db = pc().await;
    let reviewer = holder(&db, "site_reviewer", Some(crate::rbac::permissions::DI_REVIEW), Some(1)).await;
    // The reviewer switched the category off and asked for emails: neither applies.
    exec(
        &db,
        "INSERT INTO notification_preferences (user_id, category_code, in_app_enabled, email_enabled) \
         VALUES (?, ?, 0, 1)",
        vec![reviewer.into(), DI_SAFETY_ALERT_CATEGORY.into()],
    )
    .await;
    // Re-routing its rule here changes nothing either.
    exec(
        &db,
        "UPDATE notification_rules SET audiences_json = '[\"requester\"]', is_active = 0 WHERE category_code = ?",
        vec![DI_SAFETY_ALERT_CATEGORY.into()],
    )
    .await;

    create_intervention_request(&db, input(&db, true, "urgent").await)
        .await
        .expect("flagged");
    assert!(
        recipients(&db).await.contains(&reviewer),
        "delivered whatever the preference"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM notification_deliveries d JOIN notifications n ON n.id = d.notification_id \
             JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.category_code = ? AND d.channel = 'email'",
            vec![DI_SAFETY_ALERT_CATEGORY.into()],
        )
        .await,
        0,
        "never emailed"
    );
    // Nothing of the category can be changed through the rules editor.
    assert!(crate::notifications::catalog::always_delivered_audience(DI_SAFETY_ALERT_CATEGORY).is_some());
}

// ── (b) sync arrival ────────────────────────────────────────────────────────────────────────────

async fn wire(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    crate::permit::sync_check::local_wire(db, entity_type, sync_id)
        .await
        .expect("payload")
        .expect("row")
}

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

#[tokio::test]
async fn a_flagged_request_received_from_another_pc_alerts_here_once() {
    let source = pc().await;
    let target = pc().await;
    let reviewer = holder(
        &target,
        "site_reviewer",
        Some(crate::rbac::permissions::DI_REVIEW),
        Some(1),
    )
    .await;

    let di = create_intervention_request(&source, input(&source, true, "normal").await)
        .await
        .expect("flagged on the other PC");
    let sid = sync_id(&source, di.id).await;
    assert_eq!(alert_events(&source).await, 1, "told on the PC that created it");

    let outcome = apply(
        &target,
        "intervention_requests",
        &sid,
        &wire(&source, "intervention_requests", &sid).await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(alert_events(&target).await, 1, "told on arrival");
    assert!(recipients(&target).await.contains(&reviewer));
    assert_eq!(
        text(
            &target,
            "SELECT dedupe_key FROM notification_events WHERE category_code = ?",
            vec![DI_SAFETY_ALERT_CATEGORY.into()],
        )
        .await,
        Some(di_safety_alert_key(&sid))
    );

    // A later change of the request received: no second alert.
    exec(
        &source,
        "UPDATE intervention_requests SET title = 'Fuite de vapeur (bride nord)', row_version = row_version + 1, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
        vec![di.id.into()],
    )
    .await;
    let outcome = apply(
        &target,
        "intervention_requests",
        &sid,
        &wire(&source, "intervention_requests", &sid).await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(alert_events(&target).await, 1, "once per request on this PC");

    // An unflagged request received: no alert.
    let plain = create_intervention_request(&source, input(&source, false, "normal").await)
        .await
        .expect("plain");
    let plain_sid = sync_id(&source, plain.id).await;
    apply(
        &target,
        "intervention_requests",
        &plain_sid,
        &wire(&source, "intervention_requests", &plain_sid).await,
    )
    .await;
    assert_eq!(alert_events(&target).await, 1);
}

#[tokio::test]
async fn a_closed_or_converted_flagged_request_received_raises_no_alert_an_open_one_does_once() {
    let source = pc().await;
    let target = pc().await;
    holder(
        &target,
        "site_reviewer",
        Some(crate::rbac::permissions::DI_REVIEW),
        Some(1),
    )
    .await;

    // Closed on the other PC (an old flagged request touched after the upgrade): nothing here.
    let closed = create_intervention_request(&source, input(&source, true, "normal").await)
        .await
        .expect("flagged");
    exec(
        &source,
        "UPDATE intervention_requests SET status = 'closed', closed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
         row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
        vec![closed.id.into()],
    )
    .await;
    let closed_sid = sync_id(&source, closed.id).await;
    let outcome = apply(
        &target,
        "intervention_requests",
        &closed_sid,
        &wire(&source, "intervention_requests", &closed_sid).await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(alert_events(&target).await, 0, "a closed request raises no alert");

    // Turned into a work order on the other PC: nothing here either.
    let converted = create_intervention_request(&source, input(&source, true, "normal").await)
        .await
        .expect("flagged");
    exec(
        &source,
        "UPDATE intervention_requests SET status = 'approved', converted_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
         row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
        vec![converted.id.into()],
    )
    .await;
    let converted_sid = sync_id(&source, converted.id).await;
    let outcome = apply(
        &target,
        "intervention_requests",
        &converted_sid,
        &wire(&source, "intervention_requests", &converted_sid).await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(alert_events(&target).await, 0, "a converted request raises no alert");

    // Open (in review): alerted once, not again on a later change.
    let open = create_intervention_request(&source, input(&source, true, "normal").await)
        .await
        .expect("flagged");
    exec(
        &source,
        "UPDATE intervention_requests SET status = 'in_review', row_version = row_version + 1, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
        vec![open.id.into()],
    )
    .await;
    let open_sid = sync_id(&source, open.id).await;
    for _ in 0..2 {
        let outcome = apply(
            &target,
            "intervention_requests",
            &open_sid,
            &wire(&source, "intervention_requests", &open_sid).await,
        )
        .await;
        assert!(
            matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
            "{outcome:?}"
        );
        exec(
            &source,
            "UPDATE intervention_requests SET title = title || '.', row_version = row_version + 1, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            vec![open.id.into()],
        )
        .await;
    }
    assert_eq!(alert_events(&target).await, 1, "an open request is alerted once");
    assert_eq!(
        text(
            &target,
            "SELECT dedupe_key FROM notification_events WHERE category_code = ?",
            vec![DI_SAFETY_ALERT_CATEGORY.into()],
        )
        .await,
        Some(di_safety_alert_key(&open_sid))
    );
}

// ── (a) review queue and validated urgency ──────────────────────────────────────────────────────

#[tokio::test]
async fn flagged_requests_come_first_in_the_work_queues() {
    let db = pc().await;
    let emergency = create_intervention_request(&db, input(&db, false, "emergency").await)
        .await
        .expect("emergency");
    let flagged = create_intervention_request(&db, input(&db, true, "normal").await)
        .await
        .expect("flagged");
    let page = list_intervention_requests(
        &db,
        DiListFilter {
            queue: Some("to_screen".into()),
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .expect("queue");
    let order: Vec<i64> = page.items.iter().map(|d| d.id).collect();
    assert_eq!(order, vec![flagged.id, emergency.id]);
}

#[tokio::test]
async fn a_flagged_request_is_validated_below_urgent_only_with_a_reason_audited() {
    let db = pc().await;
    let actor = admin_id(&db).await;
    let di = create_intervention_request(&db, input(&db, true, "normal").await)
        .await
        .expect("flagged");
    exec(
        &db,
        "UPDATE intervention_requests SET status = 'in_review', row_version = row_version + 1 WHERE id = ?",
        vec![di.id.into()],
    )
    .await;
    let screen = |note: Option<&str>, version: i64| DiScreenInput {
        di_id: di.id,
        actor_id: actor,
        expected_row_version: version,
        validated_urgency: "normal".into(),
        classification_code_id: None,
        reviewer_note: note.map(str::to_string),
    };

    for note in [None, Some("   ")] {
        let refused = screen_di(&db, screen(note, row_version(&db, di.id).await)).await;
        match refused {
            Err(AppError::ValidationFailed(msgs)) => {
                assert!(
                    msgs.iter()
                        .any(|m| m.starts_with(GATE_DI_SAFETY_URGENCY_REASON_REQUIRED)),
                    "{msgs:?}"
                );
            }
            other => panic!("refused without a reason: {other:?}"),
        }
    }
    assert_eq!(
        text(
            &db,
            "SELECT status FROM intervention_requests WHERE id = ?",
            vec![di.id.into()]
        )
        .await
        .as_deref(),
        Some("in_review")
    );

    let screened = screen_di(
        &db,
        screen(
            Some("Fuite isolée et balisée, aucun risque résiduel"),
            row_version(&db, di.id).await,
        ),
    )
    .await
    .expect("with a reason");
    assert_eq!(screened.validated_urgency.as_deref(), Some("normal"));
    assert!(screened.safety_flag, "the flag is kept");
    let audited = text(
        &db,
        "SELECT details_json FROM di_change_events WHERE di_id = ? AND action = ?",
        vec![di.id.into(), AUDIT_SAFETY_URGENCY_LOWERED.into()],
    )
    .await
    .expect("audited");
    let details: Value = serde_json::from_str(&audited).expect("json");
    assert_eq!(details["validated_urgency"], "normal");
    assert_eq!(details["reason"], "Fuite isolée et balisée, aucun risque résiduel");
}

#[tokio::test]
async fn validating_a_flagged_request_urgent_or_more_needs_no_reason() {
    let db = pc().await;
    let di = create_intervention_request(&db, input(&db, true, "normal").await)
        .await
        .expect("flagged");
    exec(
        &db,
        "UPDATE intervention_requests SET status = 'in_review', row_version = row_version + 1 WHERE id = ?",
        vec![di.id.into()],
    )
    .await;
    screen_di(
        &db,
        DiScreenInput {
            di_id: di.id,
            actor_id: admin_id(&db).await,
            expected_row_version: row_version(&db, di.id).await,
            validated_urgency: "urgent".into(),
            classification_code_id: None,
            reviewer_note: None,
        },
    )
    .await
    .expect("urgent needs no reason");
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM di_change_events WHERE di_id = ? AND action = ?",
            vec![di.id.into(), AUDIT_SAFETY_URGENCY_LOWERED.into()],
        )
        .await,
        0
    );
}
