//! Qualification gate: work-order and LOTO-role requirements, gaps (missing, expired, expires
//! before the work, pending, level too low, self-declared), crew coverage, audited overrides,
//! requirement validation and the FR/EN starter catalogue.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::qualification::gate::{
    assess_work_order, check_person_for_permit_role, check_person_for_work_order, enforce_permit_role,
    enforce_work_order_assignment, list_overrides, QualificationOverrideFilter, QualificationOverrideInput,
};
use crate::qualification::requirements::{upsert_requirement, QualificationRequirementUpsertInput};
use crate::wo::domain::WoCreateInput;

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        // Two in-house technicians and one contractor without any login.
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('G-1', 'Karim Électricien', 'available')",
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('G-2', 'Lina Mécanicienne', 'available')",
        "INSERT INTO personnel (employee_code, full_name, availability_status, employment_type, employment_origin) \
         VALUES ('EXT-1', 'Samir Sous-traitant', 'available', 'contractor', 'external')",
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'gate-test-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'gate-test-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'gate-test-node', 'SITE-G', 'Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('gate-eq-1', 'TR-01', 'Transformateur 20 kV', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("fixture");
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

async fn admin(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

fn day(offset_days: i64) -> String {
    (Utc::now().date_naive() + Duration::days(offset_days))
        .format("%Y-%m-%d")
        .to_string()
}

async fn cert_type(db: &DatabaseConnection, code: &str) -> i64 {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO certification_types (entity_sync_id, code, name, default_validity_months, renewal_lead_days, \
         category, row_version) VALUES (?, ?, ?, 36, 60, 'electrical', 1)",
        [
            format!("ct-{code}").into(),
            code.into(),
            format!("Habilitation {code}").into(),
        ],
    ))
    .await
    .expect("cert type");
    scalar_i64(db, &format!("SELECT id FROM certification_types WHERE code = '{code}'")).await
}

async fn give_cert(db: &DatabaseConnection, personnel_id: i64, type_id: i64, status: &str, expires: Option<String>) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_certifications (entity_sync_id, personnel_id, certification_type_id, issued_at, \
         expires_at, verification_status, row_version) VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            personnel_id.into(),
            type_id.into(),
            day(-400).into(),
            expires.into(),
            status.into(),
        ],
    ))
    .await
    .expect("certification");
}

async fn skill_id(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(
        db,
        &format!(
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rd.code = 'PERSONNEL.SKILLS' AND rv.code = '{code}'"
        ),
    )
    .await
}

async fn give_skill(db: &DatabaseConnection, personnel_id: i64, skill: i64, level: i64, source: &str, validated: bool) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_skills (personnel_id, reference_value_id, proficiency_level, source_type, is_validated, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, datetime('now'), datetime('now'))",
        [
            personnel_id.into(),
            skill.into(),
            level.into(),
            source.into(),
            i64::from(validated).into(),
        ],
    ))
    .await
    .expect("skill");
}

async fn work_order(db: &DatabaseConnection, title: &str, planned_end: Option<String>) -> i64 {
    crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(scalar_i64(db, "SELECT id FROM equipment WHERE asset_id_code = 'TR-01'").await),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: title.into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end,
            shift: None,
            expected_duration_hours: None,
            creator_id: admin(db).await,
            requires_permit: Some(false),
        },
    )
    .await
    .expect("wo")
    .id
}

fn requirement(kind: &str) -> QualificationRequirementUpsertInput {
    QualificationRequirementUpsertInput {
        id: None,
        expected_row_version: None,
        target_kind: kind.into(),
        work_order_id: None,
        target_code: None,
        certification_type_id: None,
        skill_reference_value_id: None,
        min_proficiency_level: None,
        coverage: None,
        notes: None,
        is_active: None,
    }
}

async fn require_cert_on_wo(db: &DatabaseConnection, wo: i64, ct: i64, coverage: &str) {
    upsert_requirement(
        db,
        QualificationRequirementUpsertInput {
            work_order_id: Some(wo),
            certification_type_id: Some(ct),
            coverage: Some(coverage.into()),
            ..requirement("work_order")
        },
        admin(db).await,
    )
    .await
    .expect("requirement");
}

/// Books labour for a person without a login (migration 190: `personnel_id`, no user).
async fn book_labor(db: &DatabaseConnection, wo: i64, personnel_id: i64) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_interveners (work_order_id, intervener_id, personnel_id, hours_worked) \
         VALUES (?, NULL, ?, 2.0)",
        [wo.into(), personnel_id.into()],
    ))
    .await
    .expect("labour without login");
}

fn issue_codes(err: AppError) -> Vec<String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.into_iter().map(|i| i.code).collect(),
        other => panic!("expected coded issues, got {other:?}"),
    }
}

fn override_by(db_admin: i64, reason: &str) -> QualificationOverrideInput {
    QualificationOverrideInput {
        reason: reason.into(),
        approved_by_id: db_admin,
    }
}

#[tokio::test]
async fn missing_or_expired_certification_blocks_assignment() {
    let db = setup().await;
    let karim = person(&db, "G-1").await;
    let lina = person(&db, "G-2").await;
    let samir = person(&db, "EXT-1").await;
    let b2v = cert_type(&db, "HAB-B2V-T").await;
    let wo = work_order(&db, "Remplacement disjoncteur", None).await;
    require_cert_on_wo(&db, wo, b2v, "each").await;

    give_cert(&db, karim, b2v, "verified", Some(day(400))).await;
    give_cert(&db, lina, b2v, "verified", Some(day(-3))).await;

    assert_eq!(
        enforce_work_order_assignment(&db, karim, wo, None)
            .await
            .expect("karim ok"),
        None
    );

    let lina_err = enforce_work_order_assignment(&db, lina, wo, None)
        .await
        .expect_err("lina expired");
    let codes = issue_codes(lina_err);
    assert_eq!(codes[0], "QUALIFICATION_EXPIRED");
    assert_eq!(codes.last().map(String::as_str), Some("QUALIFICATION_GATE_BLOCKED"));

    // The contractor has no login and no habilitation: refused with a clear reason.
    let samir_err = enforce_work_order_assignment(&db, samir, wo, None)
        .await
        .expect_err("samir missing");
    assert_eq!(issue_codes(samir_err)[0], "QUALIFICATION_MISSING");
}

#[tokio::test]
async fn certificate_must_last_until_the_planned_work_and_be_verified() {
    let db = setup().await;
    let karim = person(&db, "G-1").await;
    let lina = person(&db, "G-2").await;
    let ct = cert_type(&db, "CATEC-T").await;
    let wo = work_order(&db, "Inspection cuve", Some(format!("{}T16:00:00Z", day(20)))).await;
    require_cert_on_wo(&db, wo, ct, "each").await;

    give_cert(&db, karim, ct, "verified", Some(day(10))).await;
    give_cert(&db, lina, ct, "pending", Some(day(400))).await;

    let karim_check = check_person_for_work_order(&db, karim, wo).await.expect("check");
    assert!(!karim_check.qualified);
    assert_eq!(karim_check.gaps[0].status, "expires_before_work");

    let lina_check = check_person_for_work_order(&db, lina, wo).await.expect("check");
    assert_eq!(lina_check.gaps[0].status, "pending_verification");

    // A renewed, verified certificate clears the gap; the old one is ignored.
    give_cert(&db, karim, ct, "verified", Some(day(700))).await;
    let renewed = check_person_for_work_order(&db, karim, wo).await.expect("check");
    assert!(renewed.qualified, "{renewed:?}");
    assert!(renewed.warnings.is_empty());
}

#[tokio::test]
async fn skill_level_and_self_declared_skills() {
    let db = setup().await;
    let karim = person(&db, "G-1").await;
    let lina = person(&db, "G-2").await;
    let samir = person(&db, "EXT-1").await;
    let hydraulics = skill_id(&db, "HYDRAULICS").await;
    let wo = work_order(&db, "Fuite vérin presse", None).await;
    upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            work_order_id: Some(wo),
            skill_reference_value_id: Some(hydraulics),
            min_proficiency_level: Some(3),
            ..requirement("work_order")
        },
        admin(&db).await,
    )
    .await
    .expect("skill requirement");

    give_skill(&db, karim, hydraulics, 4, "manual", false).await;
    give_skill(&db, lina, hydraulics, 2, "manual", false).await;
    give_skill(&db, samir, hydraulics, 5, "self_declared", false).await;

    assert!(check_person_for_work_order(&db, karim, wo).await.expect("k").qualified);
    let lina_check = check_person_for_work_order(&db, lina, wo).await.expect("l");
    assert_eq!(lina_check.gaps[0].status, "level_too_low");
    assert_eq!(lina_check.gaps[0].held_level, Some(2));
    assert_eq!(lina_check.gaps[0].required_level, Some(3));
    let samir_err = enforce_work_order_assignment(&db, samir, wo, None)
        .await
        .expect_err("self declared");
    assert_eq!(issue_codes(samir_err)[0], "SKILL_UNVALIDATED");
}

#[tokio::test]
async fn audited_override_lets_the_person_work_until_a_new_gap_appears() {
    let db = setup().await;
    let samir = person(&db, "EXT-1").await;
    let actor = admin(&db).await;
    let b1v = cert_type(&db, "HAB-B1V-T").await;
    let atex = cert_type(&db, "ATEX-T").await;
    let wo = work_order(&db, "Dépannage armoire", None).await;
    require_cert_on_wo(&db, wo, b1v, "each").await;

    let short = enforce_work_order_assignment(&db, samir, wo, Some(&override_by(actor, "urgent")))
        .await
        .expect_err("reason too short");
    assert_eq!(issue_codes(short), vec!["QUALIFICATION_OVERRIDE_REASON_SHORT"]);

    let id = enforce_work_order_assignment(
        &db,
        samir,
        wo,
        Some(&override_by(
            actor,
            "Habilitation B1V en cours de renouvellement, accompagné par K. (B2V)",
        )),
    )
    .await
    .expect("override")
    .expect("override id");

    // Now the person passes without a new override, and the override is visible and audited.
    assert_eq!(
        enforce_work_order_assignment(&db, samir, wo, None)
            .await
            .expect("covered"),
        None
    );
    let check = check_person_for_work_order(&db, samir, wo).await.expect("check");
    assert!(check.qualified);
    assert_eq!(check.override_info.as_ref().map(|o| o.id), Some(id));
    let log = list_overrides(
        &db,
        QualificationOverrideFilter {
            work_order_id: Some(wo),
            ..Default::default()
        },
    )
    .await
    .expect("override log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].gaps[0].status, "missing");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'qualification.override' AND result = 'success'"
        )
        .await,
        1
    );

    // A requirement added afterwards is a new gap: blocked again.
    require_cert_on_wo(&db, wo, atex, "each").await;
    let again = enforce_work_order_assignment(&db, samir, wo, None)
        .await
        .expect_err("new gap");
    assert!(issue_codes(again).contains(&"QUALIFICATION_MISSING".to_string()));
}

#[tokio::test]
async fn crew_assessment_counts_people_without_login_and_any_coverage() {
    let db = setup().await;
    let karim = person(&db, "G-1").await;
    let samir = person(&db, "EXT-1").await;
    let weld = cert_type(&db, "WELD-T").await;
    let b0 = cert_type(&db, "HAB-B0-T").await;
    let wo = work_order(&db, "Reprise soudure châssis", None).await;
    require_cert_on_wo(&db, wo, weld, "any").await;
    require_cert_on_wo(&db, wo, b0, "each").await;

    // Nobody on the job yet: the `any` requirement is uncovered.
    let empty = assess_work_order(&db, wo).await.expect("assess");
    assert!(empty.people.is_empty());
    assert_eq!(empty.uncovered.len(), 1);
    assert!(!empty.ok);

    give_cert(&db, samir, b0, "verified", None).await;
    book_labor(&db, wo, samir).await;
    let one = assess_work_order(&db, wo).await.expect("assess");
    assert_eq!(one.people.len(), 1);
    assert_eq!(one.people[0].personnel_id, samir);
    assert_eq!(one.people[0].roles, vec!["labor".to_string()]);
    assert!(one.people[0].qualified);
    assert_eq!(one.uncovered.len(), 1, "nobody on the job welds");

    give_cert(&db, karim, weld, "verified", Some(day(300))).await;
    book_labor(&db, wo, karim).await;
    let two = assess_work_order(&db, wo).await.expect("assess");
    assert!(two.uncovered.is_empty());
    // Karim lacks the personal B0 requirement.
    assert!(!two.ok);
    assert_eq!(two.blocked_people().count(), 1);
}

#[tokio::test]
async fn work_order_type_requirements_apply_to_every_order_of_the_type() {
    let db = setup().await;
    let lina = person(&db, "G-2").await;
    let ct = cert_type(&db, "WORK-HEIGHT-T").await;
    let type_code = scalar_i64(&db, "SELECT COUNT(*) FROM work_order_types WHERE code = 'corrective'").await;
    assert_eq!(type_code, 1);
    upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            target_code: Some("corrective".into()),
            certification_type_id: Some(ct),
            ..requirement("work_order_type")
        },
        admin(&db).await,
    )
    .await
    .expect("type requirement");
    let wo = work_order(&db, "Changement luminaire", None).await;
    let check = check_person_for_work_order(&db, lina, wo).await.expect("check");
    assert_eq!(check.gaps.len(), 1);
    assert_eq!(check.gaps[0].source_kind, "work_order_type");
}

#[tokio::test]
async fn loto_role_requires_the_authorisation() {
    let db = setup().await;
    let karim = person(&db, "G-1").await;
    let lina = person(&db, "G-2").await;
    let loto = cert_type(&db, "LOTO-AUTH-T").await;
    upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            target_code: Some("isolation_apply".into()),
            certification_type_id: Some(loto),
            ..requirement("loto_role")
        },
        admin(&db).await,
    )
    .await
    .expect("role requirement");
    give_cert(&db, karim, loto, "verified", Some(day(200))).await;

    // No permit is needed to evaluate the person; the override context references one.
    assert!(
        check_person_for_permit_role(&db, karim, 1, "isolation_apply")
            .await
            .expect("k")
            .qualified
    );
    let err = enforce_permit_role(&db, lina, 1, "isolation_apply", None)
        .await
        .expect_err("lina has no LOTO authority");
    assert_eq!(issue_codes(err)[0], "QUALIFICATION_MISSING");
    let unknown = check_person_for_permit_role(&db, lina, 1, "boss")
        .await
        .expect_err("role");
    assert_eq!(issue_codes(unknown), vec!["LOTO_ROLE_UNKNOWN"]);
}

#[tokio::test]
async fn requirement_validation() {
    let db = setup().await;
    let actor = admin(&db).await;
    let ct = cert_type(&db, "SST-T").await;
    let hydraulics = skill_id(&db, "HYDRAULICS").await;
    let wo = work_order(&db, "Graissage", None).await;

    let none = upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            work_order_id: Some(wo),
            ..requirement("work_order")
        },
        actor,
    )
    .await
    .expect_err("no qualification");
    assert_eq!(issue_codes(none), vec!["REQUIREMENT_QUALIFICATION_REQUIRED"]);

    let no_level = upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            work_order_id: Some(wo),
            skill_reference_value_id: Some(hydraulics),
            ..requirement("work_order")
        },
        actor,
    )
    .await
    .expect_err("skill without level");
    assert_eq!(issue_codes(no_level), vec!["REQUIREMENT_LEVEL_RANGE"]);

    let any_role = upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            target_code: Some("isolation_verify".into()),
            certification_type_id: Some(ct),
            coverage: Some("any".into()),
            ..requirement("loto_role")
        },
        actor,
    )
    .await
    .expect_err("role coverage");
    assert_eq!(issue_codes(any_role), vec!["REQUIREMENT_ROLE_COVERAGE"]);

    let unknown_type = upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            target_code: Some("does-not-exist".into()),
            certification_type_id: Some(ct),
            ..requirement("permit_type")
        },
        actor,
    )
    .await
    .expect_err("unknown permit type");
    assert_eq!(issue_codes(unknown_type), vec!["REQUIREMENT_TARGET_UNKNOWN"]);

    require_cert_on_wo(&db, wo, ct, "each").await;
    let dup = upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            work_order_id: Some(wo),
            certification_type_id: Some(ct),
            ..requirement("work_order")
        },
        actor,
    )
    .await
    .expect_err("duplicate");
    assert_eq!(issue_codes(dup), vec!["REQUIREMENT_DUPLICATE"]);
}

#[tokio::test]
async fn starter_catalogue_speaks_the_tenant_language() {
    let db = setup().await;
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("system catalogue");
    crate::qualification::catalog::ensure_qualification_catalog_in(&db, "fr")
        .await
        .expect("catalogue fr");
    // Twice: nothing duplicated.
    crate::qualification::catalog::ensure_qualification_catalog_in(&db, "fr")
        .await
        .expect("catalogue fr again");

    let label = |code: &'static str| {
        let db = &db;
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rv.label FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE rd.code = 'PERSONNEL.SKILLS' AND rv.code = ?",
                [code.into()],
            ))
            .await
            .expect("q")
            .expect("row")
            .try_get::<String>("", "label")
            .expect("label")
        }
    };
    assert_eq!(label("MECH_GENERAL").await, "Mécanique générale");
    assert_eq!(label("PNEUMATICS").await, "Pneumatique");
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM certification_types WHERE code = 'HAB-B2V'").await,
        1
    );
    let name: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM certification_types WHERE code = 'HAB-B2V'".to_string(),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "name")
        .expect("name");
    assert!(name.starts_with("Habilitation électrique B2V"));

    // A label the plant changed is never overwritten.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE reference_values SET label = 'Méca atelier' WHERE code = 'MECH_GENERAL'".to_string(),
    ))
    .await
    .expect("rename");
    crate::qualification::catalog::ensure_qualification_catalog_in(&db, "en")
        .await
        .expect("catalogue en");
    assert_eq!(label("MECH_GENERAL").await, "Méca atelier");
    assert_eq!(label("HYDRAULICS").await, "Hydraulics");
}

/// Electrical habilitations are a company reference list (NF C 18-510, controlled catalogue):
/// seeded once, each published symbol has a certification type usable as a requirement and on a
/// person's sheet; a symbol the company retires is not put back, a new one gets its type.
#[tokio::test]
async fn habilitations_are_a_company_list_each_with_a_certification_type() {
    let db = setup().await;
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("system catalogue");
    let domain = crate::qualification::catalog::HABILITATIONS_DOMAIN;
    let category: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT governance_category FROM reference_domains WHERE code = ?",
            [domain.into()],
        ))
        .await
        .expect("q")
        .expect("domain")
        .try_get("", "governance_category")
        .expect("category");
    assert_eq!(category, "controlled_catalog");
    let values = format!(
        "SELECT COUNT(*) FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains d ON d.id = rs.domain_id WHERE d.code = '{domain}' AND rs.status = 'published'"
    );
    assert_eq!(scalar_i64(&db, &values).await, 18);
    for code in [
        "HAB-B0", "HAB-B1V", "HAB-B2V", "HAB-BR", "HAB-BC", "HAB-H0V", "HAB-HC", "HAB-HE",
    ] {
        assert_eq!(
            scalar_i64(
                &db,
                &format!("SELECT COUNT(*) FROM certification_types WHERE code = '{code}' AND category = 'electrical'")
            )
            .await,
            1,
            "{code}"
        );
    }

    // The company retires BS and adds BF: BS is not put back, BF gets its certification type.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "DELETE FROM reference_values WHERE code = 'BS' AND set_id IN (SELECT rs.id FROM reference_sets rs \
             JOIN reference_domains d ON d.id = rs.domain_id WHERE d.code = '{domain}')"
        ),
    ))
    .await
    .expect("retire BS");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active) \
             SELECT 'hab-bf', rs.id, 'BF', 'BF — chargé de chantier, basse tension', 99, 1 FROM reference_sets rs \
             JOIN reference_domains d ON d.id = rs.domain_id WHERE d.code = '{domain}' AND rs.status = 'published'"
        ),
    ))
    .await
    .expect("add BF");
    crate::qualification::catalog::ensure_qualification_catalog_in(&db, "fr")
        .await
        .expect("catalogue again");
    assert_eq!(scalar_i64(&db, &values).await, 18, "BS stays retired, BF added");
    let bf: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM certification_types WHERE code = 'HAB-BF'".to_string(),
        ))
        .await
        .expect("q")
        .expect("HAB-BF")
        .try_get("", "name")
        .expect("name");
    assert_eq!(bf, "Habilitation électrique BF — chargé de chantier, basse tension");
}

/// A requirement points at its work-order type by id: renaming the type keeps it. Every change is
/// audited; deactivating it, retargeting it or loosening it is a safety change (certify + step-up).
#[tokio::test]
async fn requirements_survive_a_type_rename_are_audited_and_weakening_is_detected() {
    let db = setup().await;
    let lina = person(&db, "G-2").await;
    let ct = cert_type(&db, "WORK-HEIGHT-T").await;
    let actor = admin(&db).await;
    let input = || QualificationRequirementUpsertInput {
        target_code: Some("corrective".into()),
        certification_type_id: Some(ct),
        ..requirement("work_order_type")
    };
    let saved = upsert_requirement(&db, input(), actor).await.expect("type requirement");
    let wo = work_order(&db, "Changement luminaire", None).await;
    let audits = "SELECT COUNT(*) FROM audit_events WHERE action_code LIKE 'qualification.requirement.%'";
    assert_eq!(scalar_i64(&db, audits).await, 1);

    // The plant renames the type's code: the requirement still applies.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE work_order_types SET code = 'CORR-2027' WHERE code = 'corrective'".to_string(),
    ))
    .await
    .expect("rename type");
    let check = check_person_for_work_order(&db, lina, wo).await.expect("check");
    assert_eq!(check.gaps.len(), 1, "still required after the rename");
    let listed = crate::qualification::requirements::list_requirements(
        &db,
        crate::qualification::requirements::QualificationRequirementFilter::default(),
    )
    .await
    .expect("list");
    assert!(listed.iter().any(|r| r.id == saved.id && r.target_code.as_deref() == Some("CORR-2027")));

    let edit = |is_active: bool, coverage: &str| QualificationRequirementUpsertInput {
        id: Some(saved.id),
        target_code: Some("CORR-2027".into()),
        certification_type_id: Some(ct),
        is_active: Some(is_active),
        coverage: Some(coverage.into()),
        ..requirement("work_order_type")
    };
    let weakening = |i: QualificationRequirementUpsertInput| {
        let db = &db;
        async move { crate::qualification::requirements::is_weakening_change(db, &i).await.expect("check") }
    };
    assert!(!weakening(edit(true, "each")).await, "same requirement, notes only");
    assert!(weakening(edit(false, "each")).await, "deactivation");
    assert!(weakening(edit(true, "any")).await, "every person → at least one");
    let mut other = edit(true, "each");
    other.target_code = Some("preventive".into());
    assert!(weakening(other).await, "another target");

    upsert_requirement(&db, edit(false, "each"), actor).await.expect("retire");
    assert_eq!(scalar_i64(&db, audits).await, 2, "the retirement is audited too");
}
