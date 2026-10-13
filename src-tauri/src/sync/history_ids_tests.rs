//! Ids inside history rows read correctly on every device (Phase 2 leftover 3).

use super::test_support::{exec, scalar_i64, scalar_opt_text, started_db, RowFactory, TestServer};

#[tokio::test]
async fn history_ids_point_to_the_same_records_on_the_other_device() {
    let mut server = TestServer::new();
    let a = started_db().await;
    let b = started_db().await;
    let mut factory_a = RowFactory::for_device(1);
    let mut factory_b = RowFactory::for_device(2);
    // B already has rows of its own, so the same record gets other local ids there.
    for _ in 0..3 {
        factory_b.create(&b, "user_accounts").await.expect("user on B");
        factory_b.create(&b, "roles").await.expect("role on B");
    }
    let user = factory_a.create(&a, "user_accounts").await.expect("user on A");
    let role = factory_a.create(&a, "roles").await.expect("role on A");
    exec(
        &a,
        "INSERT INTO audit_events (action_code, target_type, target_id, actor_id, auth_context, result,
                                   retention_class, details_json, happened_at)
         VALUES ('role.assigned', 'user_account', CAST(? AS TEXT), NULL, 'session', 'success', 'standard', ?,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![
            user.rowid.into(),
            serde_json::json!({ "summary": "Role assigned", "detail": { "role_id": role.rowid, "scope_type": "tenant" } })
                .to_string()
                .into(),
        ],
    )
    .await;
    exec(
        &a,
        "INSERT INTO audit_events (action_code, target_type, target_id, auth_context, result, retention_class, happened_at)
         VALUES ('device.trust_revoked', 'trusted_device', 'fp-device-a', 'session', 'success', 'security',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![],
    )
    .await;

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");

    let user_on_b = scalar_i64(
        &b,
        "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
        vec![user.entity_sync_id.clone().into()],
    )
    .await;
    let role_on_b = scalar_i64(
        &b,
        "SELECT id FROM roles WHERE entity_sync_id = ?",
        vec![role.entity_sync_id.clone().into()],
    )
    .await;
    assert_ne!(
        user_on_b, user.rowid,
        "the test needs different local ids on the two devices"
    );
    let target = scalar_opt_text(
        &b,
        "SELECT target_id FROM audit_events WHERE action_code = 'role.assigned'",
        vec![],
    )
    .await;
    assert_eq!(
        target,
        Some(user_on_b.to_string()),
        "the audit target is B's id of the user"
    );
    let details: serde_json::Value = serde_json::from_str(
        &scalar_opt_text(
            &b,
            "SELECT details_json FROM audit_events WHERE action_code = 'role.assigned'",
            vec![],
        )
        .await
        .expect("details"),
    )
    .expect("details JSON");
    assert_eq!(details["detail"]["role_id"], serde_json::json!(role_on_b));
    assert_eq!(
        details["detail"]["scope_type"], "tenant",
        "values that are not ids travel as they are"
    );
    let device_target = scalar_opt_text(
        &b,
        "SELECT target_id FROM audit_events WHERE action_code = 'device.trust_revoked'",
        vec![],
    )
    .await;
    assert_eq!(
        device_target, None,
        "a device-local target is not shown as if it were B's"
    );
}
