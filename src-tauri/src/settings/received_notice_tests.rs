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

/// A replay of a row already applied (a second pass, "take theirs" again) stops before the notifier
/// (nothing changes here): one notice, one notification per recipient.
#[tokio::test]
async fn a_replayed_row_gives_no_second_notice() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    let received = back_to_four_eyes(&db, &rule, "2026-10-20T09:00:00Z").await;
    let first = apply(&db, "site_settings", &rule, &received).await;
    assert!(matches!(first, ApplyOutcome::Applied), "{first:?}");
    let again = apply(&db, "site_settings", &rule, &received).await;
    assert!(
        matches!(again, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{again:?}"
    );
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
    let held_rule = rule_payload(RECEIVED_RULE_CATEGORY);
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
    let held_category = category_payload(RECEIVED_RULE_CATEGORY);
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

// ── Local-only: sync never touches anything of the category (review of 4cb9b376) ────────────

/// Rows of the notice are never loaded as payloads here (they never travel): what another PC
/// would send for them is written out.
fn payload(value: serde_json::Value) -> Payload {
    let mut p: Payload = serde_json::from_value(value).expect("payload");
    p.insert("_sync_format".into(), serde_json::json!("row.v1"));
    p
}

fn rule_payload(category: &str) -> Payload {
    payload(serde_json::json!({
        "category_code": category,
        "routing_mode": "manual",
        "requires_ack": 0,
        "dedupe_window_minutes": 0,
        "is_active": 1,
        "audiences_json": "[\"permission:adm.settings\"]",
    }))
}

fn category_payload(code: &str) -> Payload {
    payload(serde_json::json!({
        "code": code,
        "label": "Safety rule changed on another PC",
        "default_severity": "warning",
        "default_requires_ack": 0,
        "is_user_configurable": 0,
        "module_code": "system",
        "is_offered": 1,
    }))
}

async fn delete(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let payload = crate::sync::row_codec::delete_payload(&layout, None);
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        sync_id,
        "delete",
        &serde_json::to_string(&payload).expect("json"),
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

async fn sync_id_of(db: &DatabaseConnection, sql: &str) -> String {
    scalar_opt_text(db, sql, vec![RECEIVED_RULE_CATEGORY.into()])
        .await
        .expect("sync id")
}

/// The category and rule rows of the notice are never deleted, renamed or taken over by sync.
#[tokio::test]
async fn the_notice_rows_are_never_deleted_renamed_or_taken_over_by_sync() {
    let db = crate::sync::test_support::started_db().await;
    let rule = sync_id_of(
        &db,
        "SELECT entity_sync_id FROM notification_rules WHERE category_code = ?",
    )
    .await;
    let category = sync_id_of(&db, "SELECT entity_sync_id FROM notification_categories WHERE code = ?").await;
    for (entity_type, sync) in [("notification_rules", &rule), ("notification_categories", &category)] {
        let outcome = delete(&db, entity_type, sync).await;
        assert_eq!(
            conflict_code(&outcome),
            Some("NOTIFICATION_CATEGORY_LOCKED"),
            "{entity_type}: {outcome:?}"
        );
    }
    // Renamed through their own sync ids: judged on the row held here.
    let renamed_rule = rule_payload("sync_conflict");
    let outcome = apply(&db, "notification_rules", &rule, &renamed_rule).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("NOTIFICATION_CATEGORY_LOCKED"),
        "{outcome:?}"
    );
    let renamed_category = category_payload("zzz");
    let outcome = apply(&db, "notification_categories", &category, &renamed_category).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("NOTIFICATION_CATEGORY_LOCKED"),
        "{outcome:?}"
    );
    // Another row taking its code.
    let other = sync_id_of(
        &db,
        "SELECT entity_sync_id FROM notification_rules WHERE category_code <> ? LIMIT 1",
    )
    .await;
    let taken = with(
        wire(&db, "notification_rules", &other).await,
        &[("category_code", serde_json::json!(RECEIVED_RULE_CATEGORY))],
    );
    let outcome = apply(&db, "notification_rules", &other, &taken).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("NOTIFICATION_CATEGORY_LOCKED"),
        "{outcome:?}"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM notification_categories c JOIN notification_rules r ON r.category_code = c.code \
             WHERE c.code = ? AND r.is_active = 1",
            vec![RECEIVED_RULE_CATEGORY.into()],
        )
        .await,
        1,
        "both rows kept as they were"
    );
}

/// The event id is random and the notice is keyed locally: an event planted under the id the
/// earlier version would have used does not take the notice over; an event, notification or
/// acknowledgement of the category received from another PC is refused; nothing of it is captured.
#[tokio::test]
async fn a_planted_or_received_notice_row_never_replaces_or_joins_the_notice() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    let at = "2026-10-20T09:00:00Z";
    // Planted under the formerly predictable id, in another category.
    let basis = format!("{RECEIVED_RULE_CATEGORY}|site|{rule}|{at}|false");
    exec(
        &db,
        "INSERT INTO notification_events (entity_sync_id, source_module, event_code, category_code, severity) \
         VALUES (?, 'sync', 'planted', 'sync_conflict', 'info')",
        vec![crate::sync::identity::seeded_entity_sync_id("notification_events", &basis).into()],
    )
    .await;
    let outcome = apply(&db, "site_settings", &rule, &back_to_four_eyes(&db, &rule, at).await).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        notices(&db, "site_rule_received_off").await,
        1,
        "its own event, created here"
    );
    assert!(recipients(&db).await.contains(&admin(&db).await));

    // Rows of the category received from another PC: refused, whatever their sync id.
    let event = sync_id_of(
        &db,
        "SELECT entity_sync_id FROM notification_events WHERE category_code = ?",
    )
    .await;
    let received_event = payload(serde_json::json!({
        "source_module": "settings",
        "event_code": RECEIVED_RULE_CATEGORY,
        "category_code": RECEIVED_RULE_CATEGORY,
        "severity": "warning",
    }));
    for sync in [event.as_str(), "event-planted-by-another-pc"] {
        let outcome = apply(&db, "notification_events", sync, &received_event).await;
        assert_eq!(
            conflict_code(&outcome),
            Some("NOTIFICATION_CATEGORY_LOCKED"),
            "{sync}: {outcome:?}"
        );
    }
    let notification = sync_id_of(
        &db,
        "SELECT n.entity_sync_id FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = ? LIMIT 1",
    )
    .await;
    let received_notification = payload(serde_json::json!({
        "notification_event_sync_id": event,
        "delivery_state": "pending",
        "title": "Received",
    }));
    for sync in [notification.as_str(), "notification-planted-by-another-pc"] {
        let outcome = apply(&db, "notifications", sync, &received_notification).await;
        assert_eq!(
            conflict_code(&outcome),
            Some("NOTIFICATION_CATEGORY_LOCKED"),
            "{sync}: {outcome:?}"
        );
    }
    // Deleted by another PC: refused (an event is history, already never deleted by sync).
    for (entity_type, sync, refusal) in [
        ("notification_events", &event, "APPEND_ONLY_DELETE"),
        ("notifications", &notification, "NOTIFICATION_CATEGORY_LOCKED"),
    ] {
        let outcome = delete(&db, entity_type, sync).await;
        assert_eq!(conflict_code(&outcome), Some(refusal), "{entity_type}: {outcome:?}");
    }
    let ack = payload(serde_json::json!({
        "notification_sync_id": notification,
        "acknowledged_at": "2026-10-20T09:05:00Z",
    }));
    let outcome = apply(&db, "notification_acknowledgements", "ack-planted-by-another-pc", &ack).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("NOTIFICATION_CATEGORY_LOCKED"),
        "{outcome:?}"
    );

    // Acknowledged here: stays here (nothing captured for another PC to wait on).
    exec(
        &db,
        "INSERT INTO notification_acknowledgements (notification_id, acknowledged_by_id) \
         SELECT n.id, n.recipient_user_id FROM notifications n JOIN notification_events e \
           ON e.id = n.notification_event_id WHERE e.category_code = ? LIMIT 1",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type IN \
             ('notification_acknowledgements', 'notifications', 'notification_events', 'notification_rules', \
              'notification_categories') AND entity_sync_id IS NOT ?",
            // The planted event of another category is ordinary data: it travels.
            vec![crate::sync::identity::seeded_entity_sync_id("notification_events", &basis).into()],
        )
        .await,
        0
    );
}

/// Nothing of its rule can be changed here: acknowledgement and escalation included.
#[tokio::test]
async fn nothing_of_the_notice_rule_can_be_changed_here() {
    let db = crate::sync::test_support::started_db().await;
    let rule_id = scalar_i64(
        &db,
        "SELECT id FROM notification_rules WHERE category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    for input in [
        crate::notifications::config::UpdateRuleInput {
            rule_id,
            is_active: None,
            audiences: None,
            requires_ack: Some(true),
            escalation_policy_id: None,
            lead_time_minutes: None,
        },
        crate::notifications::config::UpdateRuleInput {
            rule_id,
            is_active: None,
            audiences: None,
            requires_ack: None,
            escalation_policy_id: Some(None),
            lead_time_minutes: None,
        },
    ] {
        assert!(
            crate::notifications::config::update_rule(&db, &input).await.is_err(),
            "{input:?}"
        );
    }
    let config = crate::notifications::config::load(&db).await.expect("config");
    let shown = config
        .rules
        .iter()
        .find(|r| r.category_code == RECEIVED_RULE_CATEGORY)
        .expect("listed");
    assert!(shown.locked && shown.is_active && !shown.requires_ack);
}

// ── Review of 024c13aa ─────────────────────────────────────────────────────────────────────

/// An audited opt-out change (the record another PC sends with it): `on` at `at`, by the admin.
async fn audited_event(db: &DatabaseConnection, site: i64, sync: &str, value: bool, at: &str) {
    exec(
        db,
        "INSERT INTO site_setting_events (entity_sync_id, site_node_id, setting_key, old_value_json, \
         new_value_json, reason, changed_by_id, required_step_up, changed_at) \
         VALUES (?, ?, ?, ?, ?, 'Single permit issuer on this site', ?, 1, ?)",
        vec![
            sync.into(),
            site.into(),
            SELF_APPROVAL_SETTING.into(),
            (if value { "false" } else { "true" }).into(),
            (if value { "true" } else { "false" }).into(),
            admin(db).await.into(),
            at.into(),
        ],
    )
    .await;
}

async fn site_rule_payload(db: &DatabaseConnection, site: i64, value: bool, at: &str) -> Payload {
    let site_sync = scalar_opt_text(
        db,
        "SELECT entity_sync_id FROM org_nodes WHERE id = ?",
        vec![site.into()],
    )
    .await
    .expect("site");
    let admin_sync = scalar_opt_text(
        db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![admin(db).await.into()],
    )
    .await
    .expect("admin");
    payload(serde_json::json!({
        "site_node_sync_id": site_sync,
        "setting_key": SELF_APPROVAL_SETTING,
        "value_json": if value { "true" } else { "false" },
        "updated_by_sync_id": admin_sync,
        "updated_at": at,
        "row_version": 1,
    }))
}

async fn site_notices(db: &DatabaseConnection) -> Vec<String> {
    db.query_all(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Sqlite,
        "SELECT message_key AS k FROM notification_events WHERE category_code = ? ORDER BY id",
        [RECEIVED_RULE_CATEGORY.into()],
    ))
    .await
    .expect("events")
    .iter()
    .map(|r| r.try_get::<String>("", "k").expect("key"))
    .collect()
}

/// A site lifted at T1 (audited), restored without a record, then lifted again at the same T1:
/// three changes applied here, three notices, the last one saying the site is relaxed.
#[tokio::test]
async fn lifted_restored_and_lifted_again_at_the_same_instant_gives_three_notices() {
    let db = crate::sync::test_support::started_db().await;
    let site = planning_site(&db).await;
    let t1 = "2026-10-20T08:00:00Z";
    audited_event(&db, site, "sse-lift-t1", true, t1).await;
    let rule = "site-rule-received";
    for (value, at) in [(true, t1), (false, "2026-10-20T09:00:00Z"), (true, t1)] {
        let outcome = apply(
            &db,
            "site_settings",
            rule,
            &site_rule_payload(&db, site, value, at).await,
        )
        .await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{value} at {at}: {outcome:?}");
    }
    assert_eq!(
        site_notices(&db).await,
        vec![
            "site_rule_received_on".to_string(),
            "site_rule_received_off".to_string(),
            "site_rule_received_on".to_string(),
        ]
    );
}

/// The fixed category and rule rows come back: wiped (as a tenant data reset does), a relaxing
/// change received afterwards still shows in the administrator's inbox; the startup seed puts them
/// back too.
#[tokio::test]
async fn the_notice_shows_in_the_inbox_after_its_rows_were_wiped() {
    let db = crate::sync::test_support::started_db().await;
    let site = planning_site(&db).await;
    for table in ["notification_rules", "notification_categories"] {
        let column = if table == "notification_rules" {
            "category_code"
        } else {
            "code"
        };
        exec(
            &db,
            &format!("DELETE FROM {table} WHERE {column} = ?"),
            vec![RECEIVED_RULE_CATEGORY.into()],
        )
        .await;
    }
    let admin_id = admin(&db).await;
    let vis = crate::notifications::inbox::visible_to(&db, i32::try_from(admin_id).expect("id"))
        .await
        .expect("visibility");
    let before = crate::notifications::inbox::unread_count(&db, admin_id, &vis)
        .await
        .expect("count");
    audited_event(&db, site, "sse-wiped", true, "2026-10-20T08:00:00Z").await;
    let outcome = apply(
        &db,
        "site_settings",
        "site-rule-after-wipe",
        &site_rule_payload(&db, site, true, "2026-10-20T08:00:00Z").await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let vis = crate::notifications::inbox::visible_to(&db, i32::try_from(admin_id).expect("id"))
        .await
        .expect("visibility");
    assert_eq!(
        crate::notifications::inbox::unread_count(&db, admin_id, &vis)
            .await
            .expect("count"),
        before + 1,
        "shown in the inbox"
    );
    // Wiped again, the startup seed (also run after a tenant data reset) brings them back.
    exec(
        &db,
        "DELETE FROM notification_rules WHERE category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    crate::db::seeder::seed_system_data(&db).await.expect("seed");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM notification_rules r JOIN notification_categories c ON c.code = r.category_code \
             WHERE c.code = ? AND r.is_active = 1 AND c.module_code = 'system' AND c.is_user_configurable = 0",
            vec![RECEIVED_RULE_CATEGORY.into()],
        )
        .await,
        1
    );
}

/// The post-write check: an event of the category already sitting under the id the notice takes
/// is never adopted; a relaxing change then fails (a conflict for a person), a stricter one goes on.
#[tokio::test]
async fn an_event_already_under_the_notice_id_is_never_adopted() {
    let db = crate::sync::test_support::started_db().await;
    let sync = scalar_opt_text(
        &db,
        "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'",
        vec![],
    )
    .await
    .expect("type");
    exec(
        &db,
        "UPDATE permit_types SET requires_three_person_removal = 1 WHERE code = 'hot_work'",
        vec![],
    )
    .await;
    let planted = "event-planted-under-the-notice-id";
    exec(
        &db,
        "INSERT INTO notification_events (entity_sync_id, source_module, event_code, category_code, severity, \
         message_key, payload_json) VALUES (?, 'settings', ?, ?, 'info', 'site_rule_received_off', '{}')",
        vec![
            planted.into(),
            RECEIVED_RULE_CATEGORY.into(),
            RECEIVED_RULE_CATEGORY.into(),
        ],
    )
    .await;
    super::received_notice::FORCED_EVENT_ID.with(|f| *f.borrow_mut() = Some(planted.to_string()));
    let off = payload(serde_json::json!({
        "code": "hot_work",
        "requires_three_person_removal": 0,
        "three_person_changed_at": "2026-10-20T08:00:00Z",
    }));
    assert!(
        super::received_notice::notify_received_rule_change(&db, "permit_types", &sync, &off)
            .await
            .is_err(),
        "a relaxed rule is never applied with an adopted notice"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.entity_sync_id = ?",
            vec![planted.into()],
        )
        .await,
        0,
        "nothing attached to the planted event"
    );
    super::received_notice::FORCED_EVENT_ID.with(|f| *f.borrow_mut() = None);
}

/// A local change of the category or rule rows, of a preference of the category, is never captured
/// (even outside the apply's suppression); "keep mine" on a conflict about them stages nothing.
#[tokio::test]
async fn the_notice_rows_are_never_captured_nor_restaged() {
    let db = crate::sync::test_support::started_db().await;
    let admin_id = admin(&db).await;
    exec(
        &db,
        "UPDATE notification_categories SET label = 'Changed here' WHERE code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    exec(
        &db,
        "UPDATE notification_rules SET dedupe_window_minutes = 5 WHERE category_code = ?",
        vec![RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    exec(
        &db,
        "INSERT INTO notification_preferences (user_id, category_code, in_app_enabled) VALUES (?, ?, 1)",
        vec![admin_id.into(), RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type IN \
             ('notification_categories', 'notification_rules', 'notification_preferences')",
            vec![],
        )
        .await,
        0
    );
    let rule = sync_id_of(
        &db,
        "SELECT entity_sync_id FROM notification_rules WHERE category_code = ?",
    )
    .await;
    crate::sync::capture::restage_current_state(&db, "notification_rules", &rule, &[])
        .await
        .expect("keep mine");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM sync_outbox WHERE entity_type = 'notification_rules' AND entity_sync_id = ?",
            vec![rule.into()],
        )
        .await,
        0,
        "nothing sent"
    );
    // A preference of the category received from another PC: refused.
    let received = payload(serde_json::json!({
        "user_sync_id": "user-sync-admin",
        "category_code": RECEIVED_RULE_CATEGORY,
        "in_app_enabled": 0,
        "os_enabled": 0,
    }));
    let outcome = apply(&db, "notification_preferences", "pref-from-another-pc", &received).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("NOTIFICATION_CATEGORY_LOCKED"),
        "{outcome:?}"
    );
}

/// Delivery ignores preferences on every channel: muted, toast off, daily digest and email on, the
/// notice is still toasted, never folded into the digest, never emailed.
#[tokio::test]
async fn delivery_of_the_notice_ignores_preferences_on_every_channel() {
    let db = crate::sync::test_support::started_db().await;
    let (_, rule) = opted_out_site(&db).await;
    let admin_id = admin(&db).await;
    exec(
        &db,
        "INSERT INTO notification_preferences (user_id, category_code, in_app_enabled, os_enabled, email_enabled, \
         digest_mode, muted_until) VALUES (?, ?, 0, 0, 1, 'daily_digest', '2999-01-01T00:00:00Z')",
        vec![admin_id.into(), RECEIVED_RULE_CATEGORY.into()],
    )
    .await;
    let outcome = apply(
        &db,
        "site_settings",
        &rule,
        &back_to_four_eyes(&db, &rule, "2026-10-20T09:00:00Z").await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let notification = scalar_i64(
        &db,
        "SELECT n.id FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = ? AND n.recipient_user_id = ?",
        vec![RECEIVED_RULE_CATEGORY.into(), admin_id.into()],
    )
    .await;
    let toasts = crate::notifications::os_toast::pending_os_toasts(&db, admin_id, Some(0))
        .await
        .expect("toasts");
    assert!(
        toasts.items.iter().any(|t| t.notification_id == notification),
        "toasted"
    );
    assert!(
        !crate::notifications::email::enqueue_email(&db, notification)
            .await
            .expect("email"),
        "never emailed"
    );
    let digests = crate::notifications::digest::build_due_digests(
        &db,
        &chrono::Utc,
        chrono::Utc::now() + chrono::Duration::days(2),
        "en",
    )
    .await
    .expect("digest");
    assert_eq!(digests, 0, "never folded into the digest");
}
