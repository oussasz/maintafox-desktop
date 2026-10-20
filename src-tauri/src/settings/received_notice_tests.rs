//! Two devices: a site's four-eyes opt-out or a permit type's three-person setting changed on one
//! PC is announced to the settings administrators of the other once applied there
//! (`settings::received_notice`): once per change, never for a refused or waiting row, never for
//! a change made on the PC itself.

use sea_orm::{ConnectionTrait, DatabaseConnection};

use super::received_notice::RECEIVED_RULE_CATEGORY;
use crate::reliability::study::decisions::SELF_APPROVAL_SETTING;
use crate::sync::materialize::ApplyOutcome;
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

async fn notices(db: &DatabaseConnection, key: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM notification_events WHERE category_code = ? AND message_key = ?",
        vec![RECEIVED_RULE_CATEGORY.into(), key.into()],
    )
    .await
}

async fn all_notices(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM notification_events WHERE category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await
}

async fn admin(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'", vec![]).await
}

/// A planning site on `db` (an org node whose type can own work), synced like any node.
async fn planning_site(db: &DatabaseConnection) -> i64 {
    let node = RowFactory::for_device(1)
        .create(db, "org_nodes")
        .await
        .expect("org node")
        .rowid;
    exec(
        db,
        "UPDATE org_node_types SET can_own_work = 1 WHERE id = (SELECT node_type_id FROM org_nodes WHERE id = ?)",
        vec![node.into()],
    )
    .await;
    // A live node (the generic row may come with a deletion time).
    exec(
        db,
        "UPDATE org_nodes SET deleted_at = NULL WHERE id = ?",
        vec![node.into()],
    )
    .await;
    node
}

async fn set_opt_out(db: &DatabaseConnection, site: i64, allowed: bool, expected_row_version: Option<i64>) {
    crate::settings::site::set_site_setting(
        db,
        &crate::settings::site::SetSiteSettingInput {
            site_node_id: site,
            key: SELF_APPROVAL_SETTING.into(),
            value: allowed,
            reason: "Single permit issuer on this site".into(),
            expected_row_version,
        },
        i32::try_from(admin(db).await).expect("id"),
        true,
    )
    .await
    .expect("site rule");
}

#[tokio::test]
async fn a_site_opt_out_changed_on_another_pc_is_announced_once_to_the_settings_administrators() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    let site = planning_site(&a).await;
    set_opt_out(&a, site, true, None).await;
    assert_eq!(all_notices(&a).await, 0, "a change made here is not announced here");

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");
    assert_eq!(notices(&b, "site_rule_received_on").await, 1);
    // Sent to the settings administrators of B, with who made the change and why.
    let recipients = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    assert!(recipients >= 1, "at least one settings administrator");
    let params = scalar_opt_text(
        &b,
        "SELECT message_params_json FROM notification_events WHERE category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await
    .expect("params");
    assert!(params.contains("Single permit issuer on this site"), "{params}");
    assert!(params.contains("\"changed_by\":\""), "{params}");

    // Read and archived on B: stays on B (its event never travels, so nothing would wait for it).
    exec(
        &b,
        "UPDATE notifications SET delivery_state = 'read', read_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
         archived_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE notification_event_id IN (SELECT id FROM notification_events WHERE category_code = ?)",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type IN ('notifications', 'notification_events')",
            vec![],
        )
        .await,
        0,
        "nothing of the notice is captured"
    );

    // Exchanged again: nothing new, nothing announced twice, nothing waiting on A.
    server.exchange("B", &b).await;
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert_eq!(notices(&b, "site_rule_received_on").await, 1);
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM sync_inbox WHERE entity_type IN ('notifications', 'notification_events') \
             AND apply_status IN ('pending', 'deferred', 'conflict')",
            vec![],
        )
        .await,
        0,
        "nothing waits on the PC where the change was made"
    );

    // Back to four-eyes on A: announced once too.
    let rv = scalar_i64(
        &a,
        "SELECT row_version FROM site_settings WHERE site_node_id = ? AND setting_key = ?",
        vec![site.into(), SELF_APPROVAL_SETTING.into()],
    )
    .await;
    set_opt_out(&a, site, false, Some(rv)).await;
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert_eq!(notices(&b, "site_rule_received_off").await, 1);
    assert_eq!(all_notices(&a).await, 0);
}

#[tokio::test]
async fn a_refused_or_waiting_site_rule_is_not_announced() {
    let a = started_device_db().await;
    // A relaxing rule received without its audited change waits (or is refused): never announced.
    let site = planning_site(&a).await;
    let site_sync = scalar_opt_text(
        &a,
        "SELECT entity_sync_id FROM org_nodes WHERE id = ?",
        vec![site.into()],
    )
    .await
    .expect("site sync id");
    let payload = serde_json::json!({
        "site_node_sync_id": site_sync,
        "setting_key": SELF_APPROVAL_SETTING,
        "value_json": "true",
        "updated_at": "2026-10-20T08:00:00Z",
        "row_version": 1,
    })
    .to_string();
    exec(
        &a,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) \
         VALUES ('b-forged', 'c1', 'site_settings', 'site-setting-forged', 'upsert', 1, ?, 'h-forged', 'pending')",
        vec![payload.into()],
    )
    .await;
    let result = crate::sync::materialize::materialize_inbox_batch(&a, None)
        .await
        .expect("materialize");
    assert_eq!(result.applied, 0, "{result:?}");
    assert_eq!(all_notices(&a).await, 0);
}

#[tokio::test]
async fn a_three_person_setting_changed_on_another_pc_is_announced_once() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    // Both PCs hold the shipped permit types before the change (as after a first sync).
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    let types = crate::permit::queries::list_permit_types(&a).await.expect("types");
    let hot = types.iter().find(|t| t.code == "hot_work").expect("hot work").clone();
    let input = crate::permit::domain::PermitTypeUpsertInput {
        id: Some(hot.id),
        code: hot.code.clone(),
        name: hot.name.clone(),
        description: Some(hot.description.clone()),
        requires_hse_approval: hot.requires_hse_approval,
        requires_operations_approval: hot.requires_operations_approval,
        requires_atmospheric_test: hot.requires_atmospheric_test,
        max_duration_hours: hot.max_duration_hours,
        mandatory_ppe_ids_json: None,
        mandatory_control_rules_json: None,
        requires_energy_isolation: None,
        disable_isolation_reason: None,
        requires_three_person_removal: Some(true),
        three_person_change_reason: Some("Hot work on the tank farm needs a third person".into()),
    };
    crate::permit::queries::upsert_permit_type(
        &a,
        input,
        crate::permit::queries::PermitTypeWriter {
            actor_id: admin(&a).await,
            may_relax_isolation: true,
            may_change_three_person: true,
        },
    )
    .await
    .expect("three-person on");
    assert_eq!(all_notices(&a).await, 0, "a change made here is not announced here");

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");
    assert_eq!(notices(&b, "three_person_received_on").await, 1);
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert_eq!(notices(&b, "three_person_received_on").await, 1, "announced once");
    // The other shipped types arrived unchanged: nothing announced for them.
    assert_eq!(all_notices(&b).await, 1);
}

// ── One PC: what each claim rests on (review of dd8e1128) ──────────────────────────────────

type Payload = serde_json::Map<String, serde_json::Value>;

/// Applies a received row as the inbox pass does: in a transaction kept only when it is applied.
async fn apply(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
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

fn conflict_code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

async fn wire(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    crate::permit::sync_check::local_wire(db, entity_type, sync_id)
        .await
        .expect("payload")
        .expect("row")
}

fn with(mut p: Payload, changes: &[(&str, serde_json::Value)]) -> Payload {
    for (k, v) in changes {
        p.insert((*k).to_string(), v.clone());
    }
    p
}

/// A planning site of this PC whose opt-out was switched on here (a local change: no notice), and
/// the rule's sync id.
async fn opted_out_site(db: &DatabaseConnection) -> (i64, String) {
    let site = planning_site(db).await;
    set_opt_out(db, site, true, None).await;
    let sync = scalar_opt_text(
        db,
        "SELECT entity_sync_id FROM site_settings WHERE site_node_id = ? AND setting_key = ?",
        vec![site.into(), SELF_APPROVAL_SETTING.into()],
    )
    .await
    .expect("rule sync id");
    assert_eq!(all_notices(db).await, 0);
    (site, sync)
}

/// The received return to four-eyes of that rule, as another PC sends it at `at`.
async fn back_to_four_eyes(db: &DatabaseConnection, rule_sync: &str, at: &str) -> Payload {
    with(
        wire(db, "site_settings", rule_sync).await,
        &[
            ("value_json", serde_json::json!("false")),
            ("updated_at", serde_json::json!(at)),
        ],
    )
}

async fn recipients(db: &DatabaseConnection) -> Vec<i64> {
    db.query_all(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Sqlite,
        "SELECT n.recipient_user_id AS u FROM notifications n \
         JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = ? ORDER BY n.recipient_user_id",
        [RECEIVED_RULE_CATEGORY.into()],
    ))
    .await
    .expect("recipients")
    .iter()
    .map(|r| r.try_get::<i64>("", "u").expect("id"))
    .collect()
}

/// A user holding `adm.settings` through a role assigned plant-wide (`site` None) or on a site.
async fn settings_admin(db: &DatabaseConnection, name: &str, site: Option<i64>) -> i64 {
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) VALUES (?, ?, ?, 'local', 1, 0, 0, datetime('now'), datetime('now'))",
        vec![format!("{name}-sync").into(), name.into(), name.into()],
    )
    .await;
    let user = scalar_i64(db, "SELECT id FROM user_accounts WHERE username = ?", vec![name.into()]).await;
    if let Some(perm) = (name != "plain_user").then_some(crate::rbac::permissions::ADM_SETTINGS) {
        exec(
            db,
            "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at) \
             VALUES (?, ?, '', 0, 'custom', 'active', datetime('now'), datetime('now'))",
            vec![format!("{name}-role").into(), format!("{name}-role").into()],
        )
        .await;
        let role = scalar_i64(
            db,
            "SELECT id FROM roles WHERE name = ?",
            vec![format!("{name}-role").into()],
        )
        .await;
        exec(
            db,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) \
             SELECT ?, id, datetime('now') FROM permissions WHERE name = ?",
            vec![role.into(), perm.into()],
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
    }
    user
}

/// Who receives it: the plant-wide holders of `adm.settings` and the elevated administrators; not
/// a site-level holder (the setting is changed plant-wide only) nor a user without the right.
#[tokio::test]
async fn the_notice_goes_to_the_plant_wide_settings_administrators_only() {
    let db = crate::sync::test_support::started_db().await;
    let (site, rule) = opted_out_site(&db).await;
    let plant = settings_admin(&db, "plant_admin", None).await;
    let _site_level = settings_admin(&db, "site_admin", Some(site)).await;
    let _plain = settings_admin(&db, "plain_user", None).await;
    let received = back_to_four_eyes(&db, &rule, "2026-10-20T09:00:00Z").await;
    let outcome = apply(&db, "site_settings", &rule, &received).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let mut expected = crate::auth::rbac::elevated_admin_user_ids(&db).await.expect("admins");
    expected.push(plant);
    expected.sort_unstable();
    expected.dedup();
    assert_eq!(recipients(&db).await, expected);
}

/// The seeded identity alone keeps one notice when the hook runs twice for the same change (the
/// row not yet written in between, as a second pass or "take theirs" could).
#[tokio::test]
async fn the_same_change_notified_twice_gives_one_notice() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    let received = back_to_four_eyes(&db, &rule, "2026-10-20T09:00:00Z").await;
    for _ in 0..2 {
        super::received_notice::notify_received_rule_change(&db, "site_settings", &rule, &received)
            .await
            .expect("notify");
    }
    assert_eq!(notices(&db, "site_rule_received_off").await, 1);
    let delivered = recipients(&db).await;
    assert!(!delivered.is_empty());
    let mut unique = delivered.clone();
    unique.dedup();
    assert_eq!(delivered, unique, "one notification per recipient");
}

/// The notice is written inside the apply: a row that passed its safety check and then waits
/// (here for the person it names, not on this PC yet) leaves no notice behind.
#[tokio::test]
async fn a_row_held_back_after_the_notice_leaves_no_notice() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    let received = with(
        back_to_four_eyes(&db, &rule, "2026-10-20T09:00:00Z").await,
        &[("updated_by_sync_id", serde_json::json!("user-not-on-this-pc"))],
    );
    let outcome = apply(&db, "site_settings", &rule, &received).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(all_notices(&db).await, 0);
}

/// The reason quoted is the one of this change (its audit event at the row's time, same value),
/// never an older change's; without that event, no reason.
#[tokio::test]
async fn the_reason_is_the_one_of_this_change_or_none() {
    let db = crate::sync::test_support::started_db().await;
    let (site, rule) = opted_out_site(&db).await;
    let event = |sync: &str, at: &str, reason: &str| {
        exec(
            &db,
            "INSERT INTO site_setting_events (entity_sync_id, site_node_id, setting_key, old_value_json, \
             new_value_json, reason, changed_by_id, required_step_up, changed_at) \
             VALUES (?, ?, ?, 'true', 'false', ?, NULL, 1, ?)",
            vec![
                sync.into(),
                site.into(),
                SELF_APPROVAL_SETTING.into(),
                reason.into(),
                at.into(),
            ],
        )
    };
    // An older return to four-eyes, with its reason.
    event("sse-old", "2026-01-01T08:00:00Z", "Old restoration reason").await;
    let at = "2026-10-20T09:00:00Z";
    let params = || {
        scalar_opt_text(
            &db,
            "SELECT message_params_json FROM notification_events WHERE category_code = ? ORDER BY id DESC LIMIT 1",
            vec![RECEIVED_RULE_CATEGORY.into()],
        )
    };
    super::received_notice::notify_received_rule_change(
        &db,
        "site_settings",
        &rule,
        &back_to_four_eyes(&db, &rule, at).await,
    )
    .await
    .expect("notify");
    let first = params().await.expect("params");
    assert!(first.contains("\"reason\":null"), "{first}");
    // Its own event, written in another format of the same instant: quoted.
    event("sse-this", "2026-10-20T09:00:00.000Z", "Second issuer joined the site").await;
    super::received_notice::notify_received_rule_change(
        &db,
        "site_settings",
        &rule,
        &back_to_four_eyes(&db, &rule, "2026-10-20T09:00:00+00:00").await,
    )
    .await
    .expect("notify");
    let second = params().await.expect("params");
    assert!(second.contains("Second issuer joined the site"), "{second}");
}

/// Its rule switched off and re-routed (an older PC, a direct edit): still delivered to its fixed
/// audience. A local edit cannot do it, and a received row that would is refused.
#[tokio::test]
async fn the_notice_cannot_be_muted_or_re_routed() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    let rule_id = scalar_i64(
        &db,
        "SELECT id FROM notification_rules WHERE category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    for input in [
        crate::notifications::config::UpdateRuleInput {
            rule_id,
            is_active: Some(false),
            audiences: None,
            requires_ack: None,
            escalation_policy_id: None,
            lead_time_minutes: None,
        },
        crate::notifications::config::UpdateRuleInput {
            rule_id,
            is_active: None,
            audiences: Some(vec!["assignee".into()]),
            requires_ack: None,
            escalation_policy_id: None,
            lead_time_minutes: None,
        },
    ] {
        assert!(
            crate::notifications::config::update_rule(&db, &input).await.is_err(),
            "{input:?}"
        );
    }

    // Received from another PC: switched off, re-routed, made mutable, moved: refused.
    let rule_sync = scalar_opt_text(
        &db,
        "SELECT entity_sync_id FROM notification_rules WHERE id = ?",
        vec![rule_id.into()],
    )
    .await
    .expect("rule sync id");
    let held_rule = wire(&db, "notification_rules", &rule_sync).await;
    for change in [
        ("is_active", serde_json::json!(0)),
        ("audiences_json", serde_json::json!("[\"assignee\"]")),
        ("audiences_json", serde_json::json!("[]")),
    ] {
        let outcome = apply(
            &db,
            "notification_rules",
            &rule_sync,
            &with(held_rule.clone(), &[change.clone()]),
        )
        .await;
        assert_eq!(
            conflict_code(&outcome),
            Some("NOTIFICATION_CATEGORY_LOCKED"),
            "{change:?}: {outcome:?}"
        );
    }
    let category_sync = scalar_opt_text(
        &db,
        "SELECT entity_sync_id FROM notification_categories WHERE code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await
    .expect("category sync id");
    let held_category = wire(&db, "notification_categories", &category_sync).await;
    for change in [
        ("is_user_configurable", serde_json::json!(1)),
        ("module_code", serde_json::json!("fin")),
    ] {
        let outcome = apply(
            &db,
            "notification_categories",
            &category_sync,
            &with(held_category.clone(), &[change.clone()]),
        )
        .await;
        assert_eq!(
            conflict_code(&outcome),
            Some("NOTIFICATION_CATEGORY_LOCKED"),
            "{change:?}: {outcome:?}"
        );
    }

    // Muted anyway in the stored row (as an older version could leave it): still delivered.
    exec(
        &db,
        "UPDATE notification_rules SET is_active = 0, audiences_json = '[\"assignee\"]' WHERE id = ?",
        vec![rule_id.into()],
    )
    .await;
    exec(
        &db,
        "UPDATE notification_categories SET is_user_configurable = 1 WHERE code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    let admin_id = admin(&db).await;
    exec(
        &db,
        "INSERT INTO notification_preferences (user_id, category_code, in_app_enabled) VALUES (?, ?, 0)",
        vec![admin_id.into(), RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    let received = back_to_four_eyes(&db, &rule, "2026-10-20T10:00:00Z").await;
    let outcome = apply(&db, "site_settings", &rule, &received).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(
        recipients(&db).await.contains(&admin_id),
        "delivered to the administrator"
    );
}

/// A notice that cannot be written (a database failure) never holds back a stricter rule (back to
/// four-eyes is applied, the notice left out); it holds back a relaxed one (the apply fails).
#[tokio::test]
async fn a_failing_notice_never_holds_back_a_stricter_rule() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    exec(
        &db,
        "CREATE TEMP TRIGGER fail_notice BEFORE INSERT ON notifications BEGIN SELECT RAISE(ABORT, 'injected'); END",
        vec![],
    )
    .await;
    let off = back_to_four_eyes(&db, &rule, "2026-10-20T11:00:00Z").await;
    super::received_notice::notify_received_rule_change(&db, "site_settings", &rule, &off)
        .await
        .expect("a stricter rule goes on");
    assert_eq!(all_notices(&db).await, 0, "the savepoint undid the partial notice");
    let on = with(off, &[("value_json", serde_json::json!("true"))]);
    // Judged against the rule held here (on): set it off first so "on" is a change.
    exec(
        &db,
        "UPDATE site_settings SET value_json = 'false' WHERE entity_sync_id = ?",
        vec![rule.clone().into()],
    )
    .await;
    assert!(
        super::received_notice::notify_received_rule_change(&db, "site_settings", &rule, &on)
            .await
            .is_err(),
        "a relaxed rule is never applied without its notice"
    );
    exec(&db, "DROP TRIGGER fail_notice", vec![]).await;
}

/// A three-person change refused on arrival (no record of who, when, why) is not announced.
#[tokio::test]
async fn a_refused_three_person_change_is_not_announced() {
    let db = crate::sync::test_support::started_db().await;
    let sync = scalar_opt_text(
        &db,
        "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'",
        vec![],
    )
    .await
    .expect("type");
    let received = with(
        wire(&db, "permit_types", &sync).await,
        &[("requires_three_person_removal", serde_json::json!(1))],
    );
    let outcome = apply(&db, "permit_types", &sync, &received).await;
    assert!(conflict_code(&outcome).is_some(), "{outcome:?}");
    assert_eq!(all_notices(&db).await, 0);
}
