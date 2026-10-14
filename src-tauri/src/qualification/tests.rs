//! Qualification / certification readiness (PRD §6.20, Phase 6.2): certificate readiness,
//! permit-type requirements, crew gaps, input validation and expiry alerts.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::qualification::domain::{
    CertificationTypeUpsertInput, CrewPermitSkillGapInput, PersonnelCertificationUpsertInput, PersonnelReadinessFilter,
    QualificationRequirementProfileUpsertInput,
};
use crate::qualification::queries::{
    readiness_status, upsert_certification_type, upsert_personnel_certification,
    upsert_qualification_requirement_profile, DEFAULT_RENEWAL_LEAD_DAYS,
};
use crate::qualification::readiness::{evaluate_crew_permit_skill_gaps, list_personnel_readiness};
use crate::wo::domain::WoCreateInput;

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('Q-1', 'Karim Elec', 'available')",
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('Q-2', 'Lina Meca', 'available')",
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at)          VALUES (1, 'qual-test-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active,          created_at, updated_at) VALUES (1, 'qual-test-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at,          updated_at, structure_model_id) VALUES (1, 'qual-test-node', 'SITE-Q', 'Site', 1, 'active',          datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,          created_at, updated_at) VALUES ('qual-eq-1', 'MCC-01', 'Motor control centre', 'active_in_service', 1,          datetime('now'), datetime('now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("personnel");
    }
    db
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn person(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'")).await
}

fn day(offset_days: i64) -> String {
    (Utc::now().date_naive() + Duration::days(offset_days))
        .format("%Y-%m-%d")
        .to_string()
}

async fn cert_type(db: &DatabaseConnection, code: &str) -> i64 {
    upsert_certification_type(
        db,
        CertificationTypeUpsertInput {
            id: None,
            code: code.into(),
            name: format!("{code} habilitation"),
            default_validity_months: Some(36),
            renewal_lead_days: Some(60),
            category: Some("electrical".into()),
            description: None,
            is_active: None,
            expected_row_version: None,
        },
    )
    .await
    .expect("certification type")
    .id
}

fn cert(personnel_id: i64, type_id: i64, status: &str, expires: Option<String>) -> PersonnelCertificationUpsertInput {
    PersonnelCertificationUpsertInput {
        id: None,
        personnel_id,
        certification_type_id: type_id,
        issued_at: Some(day(-400)),
        expires_at: expires,
        issuing_body: Some("APAVE".into()),
        certificate_ref: None,
        verification_status: status.into(),
        expected_row_version: None,
    }
}

async fn loto_profile(db: &DatabaseConnection, required: &[i64]) {
    upsert_qualification_requirement_profile(
        db,
        QualificationRequirementProfileUpsertInput {
            id: None,
            profile_name: "LOTO crew".into(),
            required_certification_type_ids_json: serde_json::to_string(required).expect("json"),
            applies_to_permit_type_codes_json: r#"["loto"]"#.into(),
            skill_reference_value_ids: None,
        },
    )
    .await
    .expect("profile");
}

fn assert_validation<T: std::fmt::Debug>(res: Result<T, AppError>, needle: &str) {
    match res {
        Err(AppError::ValidationFailed(msgs)) => assert!(
            msgs.iter().any(|m| m.contains(needle)),
            "expected {needle:?} in {msgs:?}"
        ),
        other => panic!("expected ValidationFailed({needle:?}), got {other:?}"),
    }
}

#[test]
fn certificate_readiness_states() {
    assert_eq!(readiness_status("verified", &None, None), "valid");
    assert_eq!(readiness_status("verified", &Some(day(-1)), Some(60)), "expired");
    assert_eq!(readiness_status("verified", &Some(day(10)), Some(60)), "expiring_soon");
    assert_eq!(readiness_status("verified", &Some(day(90)), Some(60)), "valid");
    // No lead time on the type: the documented default window applies.
    assert_eq!(
        readiness_status("verified", &Some(day(DEFAULT_RENEWAL_LEAD_DAYS - 1)), None),
        "expiring_soon"
    );
    assert_eq!(readiness_status("pending", &Some(day(90)), None), "pending");
    assert_eq!(readiness_status("rejected", &None, None), "rejected");
    // Fail closed: an unreadable expiry is never "valid".
    assert_eq!(readiness_status("verified", &Some("soon".into()), None), "expired");
}

#[tokio::test]
async fn certificate_dates_are_validated() {
    let db = setup().await;
    let p = person(&db, "Q-1").await;
    let t = cert_type(&db, "B1V").await;
    let mut bad = cert(p, t, "verified", Some("31/12/2027".into()));
    assert_validation(upsert_personnel_certification(&db, bad.clone()).await, "expires_at");
    bad.expires_at = Some(day(-500));
    assert_validation(upsert_personnel_certification(&db, bad).await, "before issued_at");
    let ok = upsert_personnel_certification(&db, cert(p, t, "verified", Some(day(400))))
        .await
        .expect("valid certificate");
    assert_eq!(ok.readiness_status, "valid");
}

#[tokio::test]
async fn permit_readiness_requires_verified_unexpired_certificates() {
    let db = setup().await;
    let karim = person(&db, "Q-1").await;
    let lina = person(&db, "Q-2").await;
    let elec = cert_type(&db, "B1V").await;
    let lock = cert_type(&db, "LOTO-AUTH").await;
    loto_profile(&db, &[elec, lock]).await;

    // Karim: both verified and valid → qualified, earliest expiry reported.
    upsert_personnel_certification(&db, cert(karim, elec, "verified", Some(day(300))))
        .await
        .expect("cert");
    upsert_personnel_certification(&db, cert(karim, lock, "verified", Some(day(100))))
        .await
        .expect("cert");
    // Lina: one expired, one only pending → not qualified.
    upsert_personnel_certification(
        &db,
        PersonnelCertificationUpsertInput {
            issued_at: Some(day(-900)),
            ..cert(lina, elec, "verified", Some(day(-5)))
        },
    )
    .await
    .expect("cert");
    upsert_personnel_certification(&db, cert(lina, lock, "pending", Some(day(300))))
        .await
        .expect("cert");

    let rows = list_personnel_readiness(
        &db,
        PersonnelReadinessFilter {
            personnel_id: Some(karim),
            permit_type_code: Some("loto".into()),
        },
    )
    .await
    .expect("karim");
    assert!(rows[0].is_qualified);
    assert_eq!(rows[0].expires_at.as_deref(), Some(day(100).as_str()));

    let rows = list_personnel_readiness(
        &db,
        PersonnelReadinessFilter {
            personnel_id: Some(lina),
            permit_type_code: None,
        },
    )
    .await
    .expect("lina");
    assert_eq!(rows.len(), 1, "one row per permit type named by a profile");
    assert!(!rows[0].is_qualified);
    assert!(rows[0]
        .blocking_reason
        .as_deref()
        .is_some_and(|r| r.contains("missing_required")));

    // A permit type no profile mentions has no requirement.
    let rows = list_personnel_readiness(
        &db,
        PersonnelReadinessFilter {
            personnel_id: Some(lina),
            permit_type_code: Some("hot_work".into()),
        },
    )
    .await
    .expect("hot work");
    assert!(rows[0].is_qualified);
}

#[tokio::test]
async fn malformed_profile_fails_closed() {
    let db = setup().await;
    let karim = person(&db, "Q-1").await;
    loto_profile(&db, &[]).await;
    // A profile corrupted after validation (e.g. written by an older version) must not read as
    // "no requirement".
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE qualification_requirement_profiles SET required_certification_type_ids_json = 'not json'".to_string(),
    ))
    .await
    .expect("corrupt");
    assert_validation(
        list_personnel_readiness(
            &db,
            PersonnelReadinessFilter {
                personnel_id: Some(karim),
                permit_type_code: Some("loto".into()),
            },
        )
        .await,
        "unreadable",
    );
}

#[tokio::test]
async fn crew_gaps_list_the_missing_certificates() {
    let db = setup().await;
    let karim = person(&db, "Q-1").await;
    let lina = person(&db, "Q-2").await;
    let elec = cert_type(&db, "B1V").await;
    let lock = cert_type(&db, "LOTO-AUTH").await;
    loto_profile(&db, &[elec, lock]).await;
    for (p, t) in [(karim, elec), (karim, lock), (lina, elec)] {
        upsert_personnel_certification(&db, cert(p, t, "verified", None))
            .await
            .expect("cert");
    }

    let admin = scalar_i64(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let wo = crate::wo::queries::create_work_order(
        &db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(scalar_i64(&db, "SELECT id FROM equipment WHERE asset_id_code = 'MCC-01'").await),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "Isolate MCC".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: admin,
            requires_permit: Some(true),
        },
    )
    .await
    .expect("wo");

    // Requires a permit but none is linked and no type given: refused, not "no gap".
    assert_validation(
        evaluate_crew_permit_skill_gaps(
            &db,
            CrewPermitSkillGapInput {
                work_order_id: wo.id,
                personnel_ids: vec![karim],
                permit_type_code: None,
            },
        )
        .await,
        "requires a permit",
    );

    let res = evaluate_crew_permit_skill_gaps(
        &db,
        CrewPermitSkillGapInput {
            work_order_id: wo.id,
            personnel_ids: vec![karim, lina],
            permit_type_code: Some("loto".into()),
        },
    )
    .await
    .expect("gaps");
    assert!(res.rows[0].is_qualified);
    assert!(res.rows[0].missing_certification_type_ids.is_empty());
    assert!(!res.rows[1].is_qualified);
    assert_eq!(res.rows[1].missing_certification_type_ids, vec![lock]);
}

#[tokio::test]
async fn expiry_scan_alerts_once_per_week_and_skips_rejected() {
    let db = setup().await;
    let karim = person(&db, "Q-1").await;
    let lina = person(&db, "Q-2").await;
    let t = cert_type(&db, "B1V").await;
    upsert_personnel_certification(&db, cert(karim, t, "verified", Some(day(20))))
        .await
        .expect("cert");
    upsert_personnel_certification(&db, cert(lina, t, "rejected", Some(day(5))))
        .await
        .expect("cert");

    let first = crate::qualification::expiry_alerts::scan_training_expiry_alerts(&db, 30)
        .await
        .expect("scan");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].severity, "info", "not yet expired");
    let second = crate::qualification::expiry_alerts::scan_training_expiry_alerts(&db, 30)
        .await
        .expect("rescan");
    assert!(second.is_empty(), "deduplicated within the week");
}
