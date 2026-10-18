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

/// A work-order part line received from a PC that has not run migration 231 (no `is_required` in
/// the payload) is stored as required — what it meant there.
#[tokio::test]
async fn a_flagless_part_line_received_by_sync_is_required() {
    let db = crate::db::test_db::seeded().await;
    let actor: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'admin'".to_string(),
        ))
        .await
        .expect("q")
        .expect("admin")
        .try_get_by_index(0)
        .expect("id");
    // A work order needs equipment installed at an org node.
    exec(
        &db,
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'hk-model', 1, 'active', datetime('now'), datetime('now'))",
    )
    .await;
    exec(
        &db,
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
         VALUES (1, 'hk-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
    )
    .await;
    exec(
        &db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
         VALUES (1, 'hk-node', 'HK-SITE', 'Site', 1, 'active', datetime('now'), datetime('now'), 1)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at) \
         VALUES ('hk-eq', 'HK-EQ', 'Pump', 'active_in_service', (SELECT MIN(id) FROM org_nodes), datetime('now'), datetime('now'))",
    )
    .await;
    let equipment: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE asset_id_code = 'HK-EQ'".to_string(),
        ))
        .await
        .expect("q")
        .expect("equipment")
        .try_get_by_index(0)
        .expect("id");
    let wo = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(equipment),
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
            title: "Sync parts".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("work order");
    let part = crate::wo::parts::add_planned_part(
        &db,
        crate::wo::parts::AddPartInput {
            wo_id: wo.id,
            article_id: None,
            article_ref: Some("OPT-LINE".into()),
            quantity_planned: 1.0,
            unit_cost: None,
            stock_location_id: None,
            auto_reserve: Some(false),
            notes: None,
            origin: None,
            is_required: false,
        },
    )
    .await
    .expect("optional line");
    exec(
        &db,
        &format!(
            "UPDATE work_order_parts SET entity_sync_id = COALESCE(entity_sync_id, 'hk-part') WHERE id = {}",
            part.id
        ),
    )
    .await;
    let sync_id = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_order_parts WHERE id = {}", part.id),
    )
    .await;
    let mut payload =
        crate::sync::test_support::current_payload(&db, "work_order_parts", &sync_id, &Default::default())
            .await
            .expect("payload")
            .expect("row");
    assert!(
        matches!(payload.get("is_required"), Some(v) if v == &serde_json::json!(0) || v == &serde_json::json!(false)),
        "the flag travels: {:?}",
        payload.get("is_required")
    );
    // The same line as an older PC sends it: no flag, a new identity.
    payload.remove("is_required");
    let outcome = apply_inbound_payload(
        &db,
        "work_order_parts",
        "hk-part-old-pc",
        "upsert",
        1,
        &serde_json::Value::Object(payload).to_string(),
    )
    .await
    .expect("apply");
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            "SELECT CAST(is_required AS TEXT) FROM work_order_parts WHERE entity_sync_id = 'hk-part-old-pc'"
        )
        .await,
        "1"
    );
}
