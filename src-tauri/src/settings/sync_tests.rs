//! Two devices: what the settings history carries to another device, and a tenant setting that
//! is locked on the receiving device.

use crate::settings::set_setting;
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, TestServer};

const LICENCE_KEY: &str = "product.license_onboarding";
const SECRET: &str = "activation-token-must-not-travel";

#[tokio::test]
async fn licence_state_history_reaches_another_device_without_its_values() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;

    // History rows the application writes by itself (no author) stay local: changes by a person.
    let admin = i32::try_from(scalar_i64(&a, "SELECT id FROM user_accounts WHERE username = 'admin'", vec![]).await)
        .expect("admin id");
    let onboarding =
        format!(r#"{{"activation_claim":{{"activation_token":"{SECRET}","machine_fingerprint":"fp-1"}}}}"#);
    set_setting(
        &a,
        LICENCE_KEY,
        "device",
        &onboarding,
        Some(admin),
        "licence onboarding stored",
    )
    .await
    .expect("licence state on A");
    set_setting(
        &a,
        "wo.reopen_window_days",
        "tenant",
        "45",
        Some(admin),
        "reopen window",
    )
    .await
    .expect("plant parameter on A");

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");

    assert!(
        !server.state.values().any(|row| row.payload_json.contains(SECRET)),
        "the activation token never leaves device A"
    );
    let count = |db| async move {
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM settings_change_events WHERE setting_key_or_domain = ?",
            vec![LICENCE_KEY.into()],
        )
        .await
    };
    let (on_a, on_b) = (count(&a).await, count(&b).await);
    let on_server = server
        .state
        .values()
        .filter(|row| row.payload_json.contains(LICENCE_KEY))
        .count();
    assert_eq!(
        on_b, on_a,
        "the change itself reaches B's history, hash only (A {on_a}, B {on_b}, server {on_server})"
    );
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM settings_change_events \
             WHERE (setting_key_or_domain = ? AND (old_value_json IS NOT NULL OR new_value_json IS NOT NULL)) \
                OR old_value_json LIKE ? OR new_value_json LIKE ?",
            vec![
                LICENCE_KEY.into(),
                format!("%{SECRET}%").into(),
                format!("%{SECRET}%").into(),
            ],
        )
        .await,
        0
    );
    assert!(on_b >= 1, "the licence change is in B's history, hash only");
    assert_eq!(
        scalar_opt_text(
            &b,
            "SELECT new_value_json FROM settings_change_events WHERE setting_key_or_domain = 'wo.reopen_window_days'",
            vec![],
        )
        .await
        .as_deref(),
        Some("45"),
        "plant parameters keep their values on every device"
    );
}

#[tokio::test]
async fn a_currency_change_is_refused_by_a_device_that_already_values_stock_in_it() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    let before = scalar_opt_text(
        &b,
        "SELECT setting_value_json FROM app_settings WHERE setting_key = 'locale.currency_code'",
        vec![],
    )
    .await;

    // B already closed a valuation period in the current currency.
    exec(
        &b,
        "INSERT INTO inventory_valuation_periods \
         (entity_sync_id, period_code, period_start, period_end, currency_code, closed_at, created_at, updated_at) \
         VALUES ('vp-lock-b', '2026-01', '2026-01-01', '2026-01-31', 'DZD', '2026-02-01', '2026-02-01', '2026-02-01')",
        vec![],
    )
    .await;
    // A has no valued data yet: it may change the currency locally.
    let admin = i32::try_from(scalar_i64(&a, "SELECT id FROM user_accounts WHERE username = 'admin'", vec![]).await)
        .expect("admin id");
    set_setting(
        &a,
        "locale.currency_code",
        "tenant",
        r#""EUR""#,
        Some(admin),
        "currency",
    )
    .await
    .expect("free on A");

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert!(report.materialized.conflicts >= 1, "{report:?}");
    assert_eq!(
        scalar_opt_text(
            &b,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = 'locale.currency_code'",
            vec![],
        )
        .await,
        before,
        "B keeps the currency its postings are valued in"
    );
}
