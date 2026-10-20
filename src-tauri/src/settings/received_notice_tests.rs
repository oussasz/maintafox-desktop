//! Two devices: a site's four-eyes opt-out or a permit type's three-person setting changed on one
//! PC is announced to the settings administrators of the other once applied there
//! (`settings::received_notice`): once per change, never for a refused or waiting row, never for
//! a change made on the PC itself.

use sea_orm::DatabaseConnection;

use super::received_notice::RECEIVED_RULE_CATEGORY;
use crate::reliability::study::decisions::SELF_APPROVAL_SETTING;
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

    // Exchanged again: nothing new, nothing announced twice.
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert_eq!(notices(&b, "site_rule_received_on").await, 1);

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
