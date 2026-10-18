//! Sync apply hooks exercised with REAL payloads: the payload is read from the database by the
//! row codec (`test_support::current_payload`, the sync keys such as `<column>_sync_id`), changed as
//! a peer would, and applied through the real inbound path (`apply_inbound_payload` →
//! `row_codec::apply_row` → `apply_hooks::before_upsert_checked`). A hook that read a key the
//! wire never carries would let the change through here.
//!
//! Other hooks are covered next to their module: weekly schedules
//! (`planning::tests::a_received_week_never_goes_back_in_its_lifecycle`) and document categories
//! (`personnel::legacy_values_tests`).

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};

async fn exec(db: &sea_orm::DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn text(db: &sea_orm::DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<String>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// app_settings: a peer moving the fiscal-year start after postings exist is a SETTING_LOCKED
/// conflict; the local value stays.
#[tokio::test]
async fn a_locked_fiscal_start_received_by_sync_is_a_conflict() {
    let db = crate::db::test_db::seeded().await;
    let key = crate::finance::fiscal::FISCAL_START_SETTING;
    exec(
        &db,
        &format!(
            "INSERT OR IGNORE INTO app_settings (setting_key, category, setting_scope, setting_value_json, \
             setting_risk, validation_status, last_modified_at) \
             VALUES ('{key}', 'locale', 'tenant', '1', 'high', 'valid', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE app_settings SET setting_value_json = '1' WHERE setting_key = '{key}' AND setting_scope = 'tenant'"
        ),
    )
    .await;
    // A closed period: the start month is now in use.
    exec(
        &db,
        "INSERT INTO budget_periods (entity_sync_id, fiscal_year, period_month, status, closed_at, created_at, updated_at) \
         VALUES ('hk-period', 2026, 1, 'closed', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z')",
    )
    .await;
    // A row a seeder writes without an identity gets its seeded one when startup installs the
    // capture (`sync::seeded_identity`, `SeededScope::MissingIdentity`), exactly as on a device.
    crate::sync::capture::install_capture_in(&db)
        .await
        .expect("capture + seeded identities");
    let sync_id = text(
        &db,
        &format!("SELECT entity_sync_id FROM app_settings WHERE setting_key = '{key}' AND setting_scope = 'tenant'"),
    )
    .await;
    let mut payload = crate::sync::test_support::current_payload(&db, "app_settings", &sync_id, &Default::default())
        .await
        .expect("payload")
        .expect("row");
    assert_eq!(payload.get("setting_key").and_then(|v| v.as_str()), Some(key));
    payload.insert("setting_value_json".into(), "7".into());

    let outcome = apply_inbound_payload(
        &db,
        "app_settings",
        &sync_id,
        "upsert",
        2,
        &serde_json::Value::Object(payload).to_string(),
    )
    .await
    .expect("apply");
    assert!(
        matches!(&outcome, ApplyOutcome::Conflict { code, .. } if code == "SETTING_LOCKED"),
        "{outcome:?}"
    );
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT setting_value_json FROM app_settings WHERE setting_key = '{key}' AND setting_scope = 'tenant'"
            )
        )
        .await,
        "1",
        "the local value stays"
    );
}
