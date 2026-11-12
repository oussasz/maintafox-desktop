//! L4-12: isolation points kept on an equipment and proposed on its permits.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::isolation_points::{self, EquipmentIsolationPointActiveInput, EquipmentIsolationPointUpsertInput};
use crate::errors::AppError;
use crate::permit::domain::{PermitIsolationUpsertInput, WorkPermit, WorkPermitCreateInput};

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("exec");
}

/// An equipment installed on a site (the permit's site comes from it).
async fn equipment(db: &DatabaseConnection, code: &str) -> i64 {
    exec(
        db,
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at)          VALUES (1, 'l412-model-001', 1, 'active', datetime('now'), datetime('now'))",
    )
    .await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active,          created_at, updated_at) VALUES (1, 'l412-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
    )
    .await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at,          structure_model_id) VALUES (1, 'l412-org-001', 'SITE-L412', 'Site L4-12', 1, 'active', datetime('now'),          datetime('now'), 1)",
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,              created_at, updated_at) VALUES ('eq-sync-{code}', '{code}', 'Pompe de reprise', 'active_in_service', 1,              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
        ),
    )
    .await;
    scalar(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
}

async fn loto_permit(db: &DatabaseConnection, asset: i64) -> WorkPermit {
    let type_id = scalar(db, "SELECT id FROM permit_types WHERE code = 'loto'").await;
    let author = scalar(db, "SELECT id FROM user_accounts ORDER BY id LIMIT 1").await;
    crate::permit::queries::create_work_permit(
        db,
        WorkPermitCreateInput {
            permit_type_id: type_id,
            asset_id: Some(asset),
            entity_id: Some(1),
            linked_work_order_id: None,
            expires_at: Some((chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339()),
        },
        author,
    )
    .await
    .expect("permit")
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn list_value(db: &DatabaseConnection, domain: &str, code: &str) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains d ON d.id = rs.domain_id WHERE d.code = '{domain}' AND rv.code = '{code}'"
        ),
    )
    .await
}

fn code_of(err: &AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues[0].code.clone(),
        AppError::ValidationFailed(messages) => messages[0].split(':').next().unwrap_or_default().to_string(),
        other => panic!("unexpected error {other:?}"),
    }
}

async fn input(db: &DatabaseConnection, equipment_id: i64, code: &str) -> EquipmentIsolationPointUpsertInput {
    EquipmentIsolationPointUpsertInput {
        id: None,
        equipment_id,
        expected_row_version: None,
        point_code: code.into(),
        label: "Sectionneur armoire MCC-3".into(),
        energy_type_id: Some(list_value(db, crate::permit::energy_reference::ENERGY_TYPES_DOMAIN, "ELECTRICAL").await),
        isolation_method_id: Some(
            list_value(
                db,
                crate::permit::energy_reference::ISOLATION_METHODS_DOMAIN,
                "BREAKER_LOCKOUT",
            )
            .await,
        ),
        normal_state: Some(" Fermé, sous tension ".into()),
    }
}

#[tokio::test]
async fn points_are_kept_on_the_equipment_with_reference_values_and_unique_codes() {
    let db = crate::db::test_db::seeded().await;
    let asset = equipment(&db, "EQ-L412-1").await;
    let point = isolation_points::upsert_equipment_isolation_point(&db, input(&db, asset, " ip-01 ").await)
        .await
        .expect("create");
    assert_eq!(point.point_code, "IP-01");
    assert_eq!(point.normal_state.as_deref(), Some("Fermé, sous tension"));
    assert_eq!(point.energy_type_code.as_deref(), Some("ELECTRICAL"));
    assert!(point.is_active);

    // The same code twice on the equipment: refused.
    let err = isolation_points::upsert_equipment_isolation_point(&db, input(&db, asset, "IP-01").await)
        .await
        .expect_err("taken");
    assert_eq!(code_of(&err), "GATE_ASSET_ISOLATION_POINT_CODE_TAKEN");
    // An energy type that is not a value of the list: refused with the list's code.
    let mut wrong = input(&db, asset, "IP-02").await;
    wrong.energy_type_id = wrong.isolation_method_id;
    let err = isolation_points::upsert_equipment_isolation_point(&db, wrong)
        .await
        .expect_err("not an energy type");
    assert_eq!(code_of(&err), "GATE_PERMIT_ENERGY_TYPE_UNKNOWN");
    let mut blank = input(&db, asset, "  ").await;
    blank.label = "x".into();
    let err = isolation_points::upsert_equipment_isolation_point(&db, blank)
        .await
        .expect_err("no code");
    assert_eq!(code_of(&err), "GATE_ASSET_ISOLATION_POINT_CODE_REQUIRED");

    // Edited with its version, then deactivated: no longer listed by default.
    let mut edit = input(&db, asset, "IP-01").await;
    edit.id = Some(point.id);
    edit.expected_row_version = Some(point.row_version);
    edit.label = "Sectionneur général armoire MCC-3".into();
    let edited = isolation_points::upsert_equipment_isolation_point(&db, edit)
        .await
        .expect("edit");
    assert_eq!(edited.row_version, point.row_version + 1);
    let off = isolation_points::set_equipment_isolation_point_active(
        &db,
        EquipmentIsolationPointActiveInput {
            id: edited.id,
            expected_row_version: edited.row_version,
            is_active: false,
        },
    )
    .await
    .expect("deactivate");
    assert!(!off.is_active);
    assert!(isolation_points::list_equipment_isolation_points(&db, asset, false)
        .await
        .expect("list")
        .is_empty());
    assert_eq!(
        isolation_points::list_equipment_isolation_points(&db, asset, true)
            .await
            .expect("list all")
            .len(),
        1
    );
}

#[tokio::test]
async fn a_permit_on_the_equipment_proposes_its_active_points() {
    let db = crate::db::test_db::seeded().await;
    let asset = equipment(&db, "EQ-L412-2").await;
    let point = isolation_points::upsert_equipment_isolation_point(&db, input(&db, asset, "IP-10").await)
        .await
        .expect("create");
    let wp = loto_permit(&db, asset).await;
    let proposed = isolation_points::list_proposed_isolation_points(&db, wp.id)
        .await
        .expect("proposed");
    assert_eq!(proposed.len(), 1);
    assert_eq!(proposed[0].isolation_point_text, "IP-10 — Sectionneur armoire MCC-3");
    assert!(!proposed[0].already_on_permit);

    // Recorded on the permit as proposed (the screen pre-fills the form): marked as already there.
    crate::permit::queries::upsert_permit_isolation(
        &db,
        PermitIsolationUpsertInput {
            id: None,
            permit_id: wp.id,
            isolation_point: proposed[0].isolation_point_text.clone(),
            energy_type_id: Some(point.energy_type_id),
            isolation_method_id: Some(point.isolation_method_id),
            lock_number: None,
        },
    )
    .await
    .expect("isolation");
    let proposed = isolation_points::list_proposed_isolation_points(&db, wp.id)
        .await
        .expect("proposed");
    assert!(proposed[0].already_on_permit);

    // A permit on another equipment proposes nothing of this one.
    let other = equipment(&db, "EQ-L412-3").await;
    let elsewhere = loto_permit(&db, other).await;
    assert!(isolation_points::list_proposed_isolation_points(&db, elsewhere.id)
        .await
        .expect("proposed")
        .is_empty());
}
