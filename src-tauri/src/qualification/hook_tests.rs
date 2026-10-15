//! The qualification gate wired into the jobs: labour booking (people without a login
//! included), WO readiness, responsible assignment, planning commitments and LOTO roles.
//! Each hook is refused without an override and allowed once a supervisor override is
//! recorded (with its audit row).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::qualification::gate::{enforce_permit_role, enforce_work_order_assignment, QualificationOverrideInput};
use crate::qualification::requirements::{upsert_requirement, QualificationRequirementUpsertInput};
use crate::wo::domain::WoCreateInput;

const TECH_USER: i64 = 2;

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn admin(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

/// Site, asset, a technician with a login (user 2) and a contractor without one; the HAB
/// certification type.
async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'hook-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'hook-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, \
         structure_model_id) VALUES (1, 'hook-site', 'SITE-H', 'Usine', 1, 'active', datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, \
         updated_at) VALUES ('hook-eq', 'TGBT-01', 'TGBT', 'active_in_service', 1, datetime('now'), datetime('now'))",
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('H-1', 'Nadia Technicienne', 'available')",
        "INSERT INTO personnel (employee_code, full_name, availability_status, employment_type, employment_origin) \
         VALUES ('EXT-9', 'Yacine Prestataire', 'available', 'contractor', 'external')",
        "INSERT INTO certification_types (entity_sync_id, code, name, category, row_version) \
         VALUES ('ct-hook-hab', 'HAB-T', 'Habilitation BR', 'electrical', 1)",
        "INSERT INTO certification_types (entity_sync_id, code, name, category, row_version) \
         VALUES ('ct-hook-loto', 'LOTO-T', 'Chargé de consignation', 'loto', 1)",
    ] {
        exec(&db, sql).await;
    }
    let tech = scalar_i64(&db, "SELECT id FROM personnel WHERE employee_code = 'H-1'").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, personnel_id, created_at, updated_at) \
         VALUES (?, 'hook-user-2', 'nadia', 'Nadia', 'local', 1, 0, 0, ?, datetime('now'), datetime('now'))",
        [TECH_USER.into(), tech.into()],
    ))
    .await
    .expect("technician login");
    db
}

async fn person(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'")).await
}

async fn cert(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(db, &format!("SELECT id FROM certification_types WHERE code = '{code}'")).await
}

async fn work_order(db: &DatabaseConnection) -> crate::wo::domain::WorkOrder {
    crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(scalar_i64(db, "SELECT id FROM equipment WHERE asset_id_code = 'TGBT-01'").await),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: Some(1),
            planner_id: None,
            urgency_id: Some(3),
            title: "Remplacement disjoncteur départ 4".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: admin(db).await,
            requires_permit: Some(false),
        },
    )
    .await
    .expect("wo")
}

async fn require_on_wo(db: &DatabaseConnection, wo: i64, ct: i64) {
    upsert_requirement(
        db,
        QualificationRequirementUpsertInput {
            id: None,
            expected_row_version: None,
            target_kind: "work_order".into(),
            work_order_id: Some(wo),
            target_code: None,
            certification_type_id: Some(ct),
            skill_reference_value_id: None,
            min_proficiency_level: None,
            coverage: Some("each".into()),
            notes: None,
            is_active: None,
        },
        admin(db).await,
    )
    .await
    .expect("requirement");
}

async fn override_on_wo(db: &DatabaseConnection, personnel_id: i64, wo: i64) {
    let input = QualificationOverrideInput {
        reason: "Habilitation en cours de renouvellement, encadré sur place".into(),
        approved_by_id: admin(db).await,
    };
    enforce_work_order_assignment(db, personnel_id, wo, Some(&input))
        .await
        .expect("override")
        .expect("override recorded");
}

fn codes(err: AppError) -> Vec<String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.into_iter().map(|i| i.code).collect(),
        other => panic!("expected coded issues, got {other:?}"),
    }
}

async fn audit_overrides(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = 'qualification.override'",
    )
    .await
}

fn yesterday(hour: u32) -> String {
    (chrono::Utc::now().date_naive() - chrono::Duration::days(1))
        .and_hms_opt(hour, 0, 0)
        .expect("time")
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[tokio::test]
async fn labour_for_a_person_without_login_is_gated_named_and_costed() {
    let db = setup().await;
    let contractor = person(&db, "EXT-9").await;
    let wo = work_order(&db).await;
    require_on_wo(&db, wo.id, cert(&db, "HAB-T").await).await;
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_rate_cards (personnel_id, effective_from, labor_rate, overtime_rate, source_type, created_at) \
             VALUES ({contractor}, '2026-01-01', 40.0, 60.0, 'manual', datetime('now'))"
        ),
    )
    .await;
    let input = || crate::wo::labor::AddPersonnelLaborInput {
        wo_id: wo.id,
        personnel_id: contractor,
        is_overtime: true,
        skill_id: None,
        started_at: Some(yesterday(18)),
        ended_at: Some(yesterday(20)),
        hours_worked: None,
        hourly_rate: None,
        notes: None,
    };

    let refused = crate::wo::labor::add_personnel_labor_entry(&db, input())
        .await
        .expect_err("unqualified contractor");
    assert_eq!(codes(refused)[0], "QUALIFICATION_MISSING");

    override_on_wo(&db, contractor, wo.id).await;
    let entry = crate::wo::labor::add_personnel_labor_entry(&db, input())
        .await
        .expect("booked under override");
    assert_eq!(entry.intervener_id, None);
    assert_eq!(entry.personnel_id, Some(contractor));
    assert!(entry.is_overtime);
    assert_eq!(entry.hourly_rate, Some(60.0), "overtime rate of the person's card");
    assert_eq!(entry.intervener_display_name.as_deref(), Some("Yacine Prestataire"));
    assert_eq!(audit_overrides(&db).await, 1);
}

#[tokio::test]
async fn readiness_blocks_an_unqualified_crew_until_overridden() {
    let db = setup().await;
    let contractor = person(&db, "EXT-9").await;
    let wo = work_order(&db).await;
    require_on_wo(&db, wo.id, cert(&db, "HAB-T").await).await;
    // Labour recorded before the requirement existed puts the contractor on the job.
    exec(
        &db,
        &format!(
            "INSERT INTO work_order_interveners (work_order_id, personnel_id, hours_worked) VALUES ({}, {contractor}, 1.0)",
            wo.id
        ),
    )
    .await;

    let report = crate::wo::workflow::readiness::evaluate_wo_readiness(&db, wo.id)
        .await
        .expect("readiness");
    let crew = report.checks.iter().find(|c| c.code == "crew_qualified").expect("rule");
    assert_eq!(crew.outcome, "fail");
    assert!(crew.blocking);
    assert!(crew
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("Yacine Prestataire"));

    override_on_wo(&db, contractor, wo.id).await;
    let report = crate::wo::workflow::readiness::evaluate_wo_readiness(&db, wo.id)
        .await
        .expect("readiness");
    let crew = report.checks.iter().find(|c| c.code == "crew_qualified").expect("rule");
    assert_eq!(crew.outcome, "pass");
}

#[tokio::test]
async fn responsible_without_the_qualification_is_refused_until_overridden() {
    let db = setup().await;
    let tech = person(&db, "H-1").await;
    let actor = admin(&db).await;
    let wo = work_order(&db).await;
    require_on_wo(&db, wo.id, cert(&db, "HAB-T").await).await;
    let wo = crate::wo::workflow::actions::submit::submit_wo(
        &db,
        crate::wo::workflow::actions::submit::WoSubmitInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit");
    let reassign = |rv: i64| crate::wo::workflow::actions::assign::WoEmergencyReassignInput {
        wo_id: wo.id,
        actor_id: actor,
        expected_row_version: rv,
        assigned_team_id: None,
        primary_responsible_id: Some(TECH_USER),
        override_reason: "Équipe de nuit indisponible".into(),
    };

    let refused = crate::wo::workflow::actions::assign::emergency_reassign(&db, reassign(wo.row_version))
        .await
        .expect_err("unqualified responsible");
    assert!(codes(refused).contains(&"QUALIFICATION_GATE_BLOCKED".to_string()));

    override_on_wo(&db, tech, wo.id).await;
    let updated = crate::wo::workflow::actions::assign::emergency_reassign(&db, reassign(wo.row_version))
        .await
        .expect("assigned under override");
    assert_eq!(updated.primary_responsible_id, Some(TECH_USER));
    assert_eq!(audit_overrides(&db).await, 1);
}

#[tokio::test]
async fn planning_assignee_is_gated_and_capacity_has_no_default_hours() {
    let db = setup().await;
    let tech = person(&db, "H-1").await;
    let wo = work_order(&db).await;
    require_on_wo(&db, wo.id, cert(&db, "HAB-T").await).await;

    let refused = crate::planning::scheduling::enforce_assignee_qualifications(&db, "work_order", wo.id, tech)
        .await
        .expect_err("unqualified assignee");
    assert_eq!(codes(refused)[0], "QUALIFICATION_MISSING");
    // Other sources carry no qualification requirement.
    crate::planning::scheduling::enforce_assignee_qualifications(&db, "pm_occurrence", wo.id, tech)
        .await
        .expect("no requirement");
    override_on_wo(&db, tech, wo.id).await;
    crate::planning::scheduling::enforce_assignee_qualifications(&db, "work_order", wo.id, tech)
        .await
        .expect("allowed under override");
    assert_eq!(audit_overrides(&db).await, 1);

    exec(
        &db,
        "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
         VALUES ('hook-team', 'EQ-H', 'Électricité', 'maintenance', 'active', datetime('now'), datetime('now'))",
    )
    .await;
    let team = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-H'").await;
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 12).expect("day");
    let empty = crate::planning::scheduling::team_day_hours(&db, team, day)
        .await
        .expect("hours");
    assert_eq!(empty.basis, "no_members");
    assert_eq!(empty.available, 0.0);
    // Hours typed on a capacity rule do not make capacity: only its overtime ceiling counts.
    exec(
        &db,
        &format!(
            "INSERT INTO capacity_rules (team_id, effective_start, available_hours_per_day, max_overtime_hours_per_day, \
             row_version, created_at, updated_at) VALUES ({team}, '2026-01-01', 16, 2, 1, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    exec(
        &db,
        &format!("UPDATE personnel SET primary_team_id = {team} WHERE id = {tech}"),
    )
    .await;
    let unscheduled = crate::planning::scheduling::team_day_hours(&db, team, day)
        .await
        .expect("hours");
    assert_eq!(unscheduled.basis, "no_schedule");
    assert_eq!(unscheduled.available, 0.0);
    assert_eq!(unscheduled.overtime, 2.0);
}

#[tokio::test]
async fn loto_roles_require_the_authorisation_until_overridden() {
    let db = setup().await;
    let tech = person(&db, "H-1").await;
    let loto = cert(&db, "LOTO-T").await;
    upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            id: None,
            expected_row_version: None,
            target_kind: "loto_role".into(),
            work_order_id: None,
            target_code: Some("isolation_apply".into()),
            certification_type_id: Some(loto),
            skill_reference_value_id: None,
            min_proficiency_level: None,
            coverage: None,
            notes: None,
            is_active: None,
        },
        admin(&db).await,
    )
    .await
    .expect("role requirement");

    let asset = scalar_i64(&db, "SELECT id FROM equipment WHERE asset_id_code = 'TGBT-01'").await;
    let mut wp = crate::permit::queries::create_work_permit(
        &db,
        crate::permit::domain::WorkPermitCreateInput {
            permit_type_id: scalar_i64(&db, "SELECT id FROM permit_types WHERE code = 'loto'").await,
            asset_id: Some(asset),
            entity_id: Some(1),
            linked_work_order_id: None,
            expires_at: Some((chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339()),
        },
    )
    .await
    .expect("permit");
    for status in ["pending_review", "approved", "issued"] {
        wp = crate::permit::queries::set_work_permit_status(
            &db,
            crate::permit::domain::WorkPermitStatusInput {
                id: wp.id,
                status: status.into(),
                expected_row_version: wp.row_version,
            },
            admin(&db).await,
        )
        .await
        .unwrap_or_else(|e| panic!("-> {status}: {e:?}"));
    }
    let iso = crate::permit::queries::upsert_permit_isolation(
        &db,
        crate::permit::domain::PermitIsolationUpsertInput {
            id: None,
            permit_id: wp.id,
            isolation_point: "Départ 4 — disjoncteur Q4".into(),
            energy_type: "electrical".into(),
            isolation_method: "cadenas + étiquette".into(),
            lock_number: None,
        },
    )
    .await
    .expect("isolation");
    let action = |rv: i64| crate::permit::domain::PermitIsolationActionInput {
        isolation_id: iso.id,
        expected_row_version: rv,
        lock_number: Some("C-12".into()),
    };

    let refused = crate::permit::queries::apply_permit_isolation(&db, action(iso.row_version), TECH_USER)
        .await
        .expect_err("no LOTO authority");
    assert_eq!(codes(refused)[0], "QUALIFICATION_MISSING");
    // A login not linked to a personnel record cannot be checked: refused (fail closed).
    let unknown = crate::permit::queries::apply_permit_isolation(&db, action(iso.row_version), admin(&db).await)
        .await
        .expect_err("unlinked login");
    assert_eq!(codes(unknown), vec!["LOTO_ROLE_PERSON_UNKNOWN"]);

    enforce_permit_role(
        &db,
        tech,
        wp.id,
        "isolation_apply",
        Some(&QualificationOverrideInput {
            reason: "Chargé de consignation absent, consignation sous contrôle du responsable".into(),
            approved_by_id: admin(&db).await,
        }),
    )
    .await
    .expect("override")
    .expect("override recorded");
    let applied = crate::permit::queries::apply_permit_isolation(&db, action(iso.row_version), TECH_USER)
        .await
        .expect("applied under override");
    assert!(applied.applied_at.is_some());
    assert_eq!(audit_overrides(&db).await, 1);
}

/// A login with no personnel card cannot be checked: on a work order with requirements, booking
/// its labour or making it responsible is refused (fails closed) and readiness names it; on a
/// work order without requirements it is accepted.
#[tokio::test]
async fn a_login_without_a_personnel_card_is_refused_where_qualifications_are_required() {
    let db = setup().await;
    let actor = admin(&db).await;
    let labour = |wo_id: i64| crate::wo::labor::AddLaborInput {
        wo_id,
        intervener_id: actor,
        skill_id: None,
        started_at: Some(yesterday(8)),
        ended_at: Some(yesterday(10)),
        hours_worked: None,
        hourly_rate: None,
        notes: None,
    };

    let free = work_order(&db).await;
    crate::wo::labor::add_labor_entry(&db, labour(free.id))
        .await
        .expect("no requirement: accepted");

    let wo = work_order(&db).await;
    require_on_wo(&db, wo.id, cert(&db, "HAB-T").await).await;
    let refused = crate::wo::labor::add_labor_entry(&db, labour(wo.id))
        .await
        .expect_err("unlinked login on a job with requirements");
    assert_eq!(codes(refused), vec!["QUALIFICATION_PERSON_UNKNOWN".to_string()]);

    let wo = crate::wo::workflow::actions::submit::submit_wo(
        &db,
        crate::wo::workflow::actions::submit::WoSubmitInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit");
    let refused = crate::wo::workflow::actions::assign::emergency_reassign(
        &db,
        crate::wo::workflow::actions::assign::WoEmergencyReassignInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            assigned_team_id: None,
            primary_responsible_id: Some(actor),
            override_reason: "Responsable de garde".into(),
        },
    )
    .await
    .expect_err("unlinked responsible");
    assert_eq!(codes(refused), vec!["QUALIFICATION_PERSON_UNKNOWN".to_string()]);

    // Labour recorded before the requirement existed: readiness names the unlinked login.
    exec(
        &db,
        &format!(
            "INSERT INTO work_order_interveners (work_order_id, intervener_id, hours_worked) VALUES ({}, {actor}, 1.0)",
            wo.id
        ),
    )
    .await;
    let assessment = crate::qualification::gate::assess_work_order(&db, wo.id)
        .await
        .expect("assessment");
    assert!(!assessment.ok);
    assert_eq!(assessment.unidentified.len(), 1);
    assert_eq!(assessment.unidentified[0].role, "labor");
    let report = crate::wo::workflow::readiness::evaluate_wo_readiness(&db, wo.id)
        .await
        .expect("readiness");
    let crew = report.checks.iter().find(|c| c.code == "crew_qualified").expect("rule");
    assert_eq!(crew.outcome, "fail");
    assert!(crew
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("fiche du personnel"));
}

/// Separation of duties: a supervisor cannot waive their own qualification gap.
#[tokio::test]
async fn nobody_overrides_their_own_qualification_gap() {
    let db = setup().await;
    let tech = person(&db, "H-1").await;
    let wo = work_order(&db).await;
    require_on_wo(&db, wo.id, cert(&db, "HAB-T").await).await;
    let own = QualificationOverrideInput {
        reason: "Je connais l'installation, pas de risque".into(),
        approved_by_id: TECH_USER,
    };
    let refused = enforce_work_order_assignment(&db, tech, wo.id, Some(&own))
        .await
        .expect_err("self override");
    assert_eq!(codes(refused), vec!["QUALIFICATION_OVERRIDE_SELF".to_string()]);
    assert_eq!(audit_overrides(&db).await, 0);
    override_on_wo(&db, tech, wo.id).await;
    assert_eq!(audit_overrides(&db).await, 1);
}
