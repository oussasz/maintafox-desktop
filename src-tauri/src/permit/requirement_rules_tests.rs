//! Permit rules applied at request → work order conversion (owner decision "flag at conversion").

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::di::conversion::{convert_di_to_work_order, WoConversionInput};
use crate::di::queries::{create_intervention_request, DiCreateInput};
use crate::di::review::{approve_di, screen_di, DiApproveInput, DiScreenInput};
use crate::errors::AppError;
use crate::permit::requirement_rules::{self as rules, reasons};
use crate::wo::workflow::actions::permit_requirement::{self as req, WoPermitRequirementInput, WoPermitRuleClearInput};

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<i64>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn code_of(err: &AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

/// Seeded database with a pump (class PUMP, installed on a site) and a classification for requests.
async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
        .await
        .expect("taxonomy");
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'pr-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'pr-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'pr-node', 'PR-SITE', 'Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         equipment_class_ref_id, created_at, updated_at) \
         SELECT 1, 'pr-eq', 'PR-PUMP', 'Pump', 'active_in_service', 1, rv.id, datetime('now'), datetime('now') \
           FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
           JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'EQUIPMENT.CLASS' AND rv.code = 'PUMP'",
        "INSERT INTO reference_domains (id, code, name, structure_type, governance_level, is_extendable, \
         created_at, updated_at) VALUES (900101, 'PR_DI_CLASSIFICATION', 'DI Classification', 'flat', \
         'tenant_managed', 1, datetime('now'), datetime('now'))",
        "INSERT INTO reference_sets (id, domain_id, version_no, status, created_at) \
         VALUES (900101, 900101, 1, 'published', datetime('now'))",
        "INSERT INTO reference_values (id, set_id, code, label, is_active) VALUES (900101, 900101, 'MECH', 'Méca', 1)",
    ] {
        exec(&db, sql).await;
    }
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM equipment WHERE id = 1").await,
        1,
        "the pump has its class"
    );
    db
}

async fn admin(db: &DatabaseConnection) -> i64 {
    scalar(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

/// A rule of the plant's list (published set), as the editor would have saved it.
async fn add_rule(db: &DatabaseConnection, code: &str, metadata: serde_json::Value) {
    rules::add_published_rule(db, code, metadata).await;
}

/// An approved request on the pump, converted to a work order.
async fn convert(db: &DatabaseConnection, actor: i64) -> crate::di::conversion::WoConversionResult {
    convert_request(db, actor, false).await
}

/// An approved request on the pump (flagged "sécurité" when `safety`), converted to a work order.
async fn convert_request(
    db: &DatabaseConnection,
    actor: i64,
    safety: bool,
) -> crate::di::conversion::WoConversionResult {
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("symptom")
        .expect("seeded symptom");
    let di = create_intervention_request(
        db,
        DiCreateInput {
            equipment_id: 1,
            org_node_id: 1,
            title: "Pump leak".into(),
            description: "Leak on the pump".into(),
            origin_type: "operator".into(),
            request_type: "repair".into(),
            symptom_code_id: Some(symptom),
            impact_level: "unknown".into(),
            production_impact: false,
            safety_flag: safety,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "normal".into(),
            observed_at: None,
            source_inspection_anomaly_id: None,
            submitter_id: actor,
        },
    )
    .await
    .expect("request");
    exec(
        db,
        &format!(
            "UPDATE intervention_requests SET status = 'in_review', row_version = row_version + 1 WHERE id = {}",
            di.id
        ),
    )
    .await;
    let screened = screen_di(
        db,
        DiScreenInput {
            di_id: di.id,
            actor_id: actor,
            expected_row_version: 2,
            validated_urgency: "high".into(),
            classification_code_id: Some(900101),
            reviewer_note: None,
        },
    )
    .await
    .expect("screen");
    let approved = approve_di(
        db,
        DiApproveInput {
            di_id: di.id,
            actor_id: actor,
            expected_row_version: screened.row_version,
            notes: None,
        },
    )
    .await
    .expect("approve");
    convert_di_to_work_order(
        db,
        WoConversionInput {
            di_id: di.id,
            actor_id: actor,
            expected_row_version: approved.row_version,
            conversion_notes: None,
        },
    )
    .await
    .expect("convert")
}

#[tokio::test]
async fn with_no_rule_nothing_is_flagged_and_the_conversion_says_so() {
    let db = setup().await;
    let actor = admin(&db).await;
    let result = convert(&db, actor).await;
    assert!(!result.permit_flag.requires_permit);
    assert_eq!(result.permit_flag.reason_code, reasons::NONE_CONFIGURED);
    assert_eq!(result.permit_flag.rule_code, None);
    let wo = crate::wo::queries::get_work_order(&db, result.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert!(!wo.requires_permit);
    assert_eq!(wo.permit_rule_code, None);
    // An unflagged work order starts as today (no permit needed).
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, result.wo_id)
        .await
        .expect("no permit needed");
}

#[tokio::test]
async fn a_rule_that_does_not_match_flags_nothing() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "VALVES_HOT_WORK",
        serde_json::json!({ "equipment_class_code": "VALVE", "permit_type_code": "hot_work" }),
    )
    .await;
    let result = convert(&db, actor).await;
    assert!(!result.permit_flag.requires_permit);
    assert_eq!(result.permit_flag.reason_code, reasons::NO_RULE_MATCHED);
}

#[tokio::test]
async fn a_matching_rule_flags_the_work_order_and_only_the_permit_right_clears_it() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let result = convert(&db, actor).await;
    assert!(result.permit_flag.requires_permit);
    assert_eq!(result.permit_flag.reason_code, reasons::RULE_MATCHED);
    assert_eq!(result.permit_flag.rule_code.as_deref(), Some("PUMPS_LOTO"));
    assert_eq!(result.permit_flag.permit_type_code.as_deref(), Some("loto"));
    let wo = crate::wo::queries::get_work_order(&db, result.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert!(wo.requires_permit);
    assert_eq!(wo.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
    assert_eq!(wo.permit_rule_permit_type.as_deref(), Some("loto"));

    // The existing start gate: no permit, no start.
    let err = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, result.wo_id)
        .await
        .expect_err("needs a permit");
    assert_eq!(code_of(&err), "PERMIT_REQUIRED_TO_START");

    // The general edit cannot clear a rule's flag.
    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("edit cannot clear");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_NEEDS_CLEAR);

    // Someone without the permit-issue right cannot clear it.
    exec(
        &db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) VALUES ('pr-planner', 'planner', 'Planner', 'local', 1, 0, 0, \
         datetime('now'), datetime('now'))",
    )
    .await;
    let planner = scalar(&db, "SELECT id FROM user_accounts WHERE username = 'planner'").await;
    let clear = |reason: &str| WoPermitRuleClearInput {
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        reason: reason.into(),
    };
    let err = req::clear_permit_rule_flag(&db, clear("No hot work on this pump"), planner)
        .await
        .expect_err("no right");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);
    let err = req::clear_permit_rule_flag(&db, clear("short"), actor)
        .await
        .expect_err("reason");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_REASON);

    // With the right and a reason: cleared, and who / when / why is kept.
    let cleared = req::clear_permit_rule_flag(&db, clear("Visual check only, no intervention"), actor)
        .await
        .expect("cleared");
    assert!(!cleared.requires_permit);
    assert_eq!(
        cleared.permit_rule_code.as_deref(),
        Some("PUMPS_LOTO"),
        "the rule stays on record"
    );
    assert_eq!(cleared.permit_rule_cleared_by_id, Some(actor));
    assert!(cleared.permit_rule_cleared_at.is_some());
    assert_eq!(
        cleared.permit_rule_clear_reason.as_deref(),
        Some("Visual check only, no intervention")
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {} AND action_code = 'permit_rule_flag_cleared'",
                wo.id
            ),
        )
        .await,
        1,
        "audited"
    );
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo.id)
        .await
        .expect("no permit needed once cleared");
}

#[tokio::test]
async fn a_rule_must_name_what_it_matches_and_an_existing_permit_type() {
    let db = setup().await;
    rules::ensure_permit_requirement_rules(&db).await.expect("list");
    for bad in [
        serde_json::json!({ "permit_type_code": "loto" }),
        serde_json::json!({ "equipment_class_code": "PUMP" }),
        serde_json::json!({ "equipment_class_code": "NOT_A_CLASS", "permit_type_code": "loto" }),
        serde_json::json!({ "work_type_code": "NOT_A_TYPE", "permit_type_code": "loto" }),
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "no_such_permit" }),
    ] {
        let err = rules::checked_rule_metadata(&db, "R", Some(&bad.to_string()))
            .await
            .expect_err("refused");
        assert_eq!(code_of(&err), rules::CODE_RULE_INVALID, "{bad}");
    }
}

/// A work order flagged by a rule starts only under a permit of the rule's type: an active, ready
/// permit of another type does not protect the hazard the rule is about.
#[tokio::test]
async fn a_rule_flag_needs_a_permit_of_the_rule_type_to_start() {
    use crate::permit::tests::{advance_to_issued, create_from_wo, set_status};
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "PUMPS_HOT_WORK",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "hot_work" }),
    )
    .await;

    // Another type, active and ready: refused, naming the type the rule asks for.
    let wrong = convert(&db, actor).await;
    let wp = create_from_wo(&db, "cold_work", wrong.wo_id)
        .await
        .expect("cold work permit");
    set_status(&db, &advance_to_issued(&db, wp).await, "active")
        .await
        .expect("activate");
    let err = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wrong.wo_id)
        .await
        .expect_err("a cold work permit does not cover a hot work rule");
    assert_eq!(code_of(&err), "PERMIT_TYPE_REQUIRED_TO_START");
    match &err {
        AppError::OrgValidationFailed(issues) => assert_eq!(
            issues[0].params.get("permit_type").map(String::as_str),
            Some("hot_work")
        ),
        other => panic!("{other:?}"),
    }

    // The rule's type, active and ready: the work can start.
    let right = convert(&db, actor).await;
    let wp = create_from_wo(&db, "hot_work", right.wo_id)
        .await
        .expect("hot work permit");
    set_status(&db, &advance_to_issued(&db, wp).await, "active")
        .await
        .expect("activate");
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, right.wo_id)
        .await
        .expect("a permit of the rule's type lets the work start");
}

// ── A request flagged "sécurité" (audit finding AU-01, CMMS Advisor decision D1) ──────────────

/// An account holding `permission` on the org node `site` only (a permanent role).
async fn site_holder(db: &DatabaseConnection, username: &str, site: i64, permission: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
             force_password_change, created_at, updated_at) VALUES ('pr-{username}', '{username}', '{username}', \
             'local', 1, 0, 0, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    let user = scalar(
        db,
        &format!("SELECT id FROM user_accounts WHERE username = '{username}'"),
    )
    .await;
    let role = crate::auth::scope_tests::role(db, &format!("PR {username}"), &[permission]).await;
    crate::auth::scope_tests::assign(db, i32::try_from(user).expect("id"), role, "org_node", Some(site)).await;
    user
}

async fn add_second_site(db: &DatabaseConnection) {
    exec(
        db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (2, 'pr-node-2', 'PR-SITE-2', 'Site 2', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
    )
    .await;
}

async fn cleared_events(db: &DatabaseConnection, wo_id: i64) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {wo_id} AND action_code = 'permit_rule_flag_cleared'"
        ),
    )
    .await
}

#[tokio::test]
async fn a_safety_request_records_its_origin_on_the_work_order() {
    let db = setup().await;
    let actor = admin(&db).await;
    let safety = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, safety.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert!(wo.requires_permit);
    assert!(wo.permit_from_safety_di, "origin recorded at conversion");
    assert_eq!(wo.permit_rule_code, None, "no rule matched");

    let plain = convert_request(&db, actor, false).await;
    let wo = crate::wo::queries::get_work_order(&db, plain.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert!(!wo.requires_permit);
    assert!(!wo.permit_from_safety_di);
}

/// The general edit (the permit box, and the draft form) cannot untick a requirement carried from
/// a safety request.
#[tokio::test]
async fn the_general_edit_cannot_clear_a_safety_request_requirement() {
    let db = setup().await;
    let actor = admin(&db).await;
    let result = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, result.wo_id)
        .await
        .expect("q")
        .expect("wo");

    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("the permit box cannot clear it");
    assert_eq!(code_of(&err), req::CODE_SAFETY_FLAG_NEEDS_CLEAR);

    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'draft') \
             WHERE id = {}",
            wo.id
        ),
    )
    .await;
    let err = crate::wo::queries::update_wo_draft_fields(
        &db,
        crate::wo::domain::WoDraftUpdateInput {
            id: wo.id,
            expected_row_version: wo.row_version,
            title: None,
            type_code: None,
            equipment_id: None,
            location_id: None,
            description: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            notes: None,
            urgency_code: None,
            subtype_code: None,
            requires_permit: Some(false),
        },
        None,
    )
    .await
    .expect_err("the draft form cannot clear it");
    assert_eq!(code_of(&err), req::CODE_SAFETY_FLAG_NEEDS_CLEAR);
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT requires_permit FROM work_orders WHERE id = {}", wo.id)
        )
        .await,
        1,
        "still required"
    );
}

/// Clearing it needs `ptw.issue` at the work order's site and a reason of 10 characters or more;
/// who, when and why are kept and the clearing is in the action history.
#[tokio::test]
async fn a_safety_request_requirement_is_cleared_with_the_permit_right_at_its_site_and_a_reason() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_second_site(&db).await;
    let result = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, result.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert_eq!(wo.entity_id, Some(1), "the work order is on site 1");
    let clear = |reason: &str| WoPermitRuleClearInput {
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        reason: reason.into(),
    };

    // No permit-issue right at all.
    let editor = site_holder(&db, "editor", 1, crate::rbac::permissions::OT_EDIT).await;
    let err = req::clear_permit_rule_flag(&db, clear("Inspection visuelle seulement"), editor)
        .await
        .expect_err("no right");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);
    // The permit-issue right, at another site.
    let elsewhere = site_holder(&db, "issuer2", 2, crate::rbac::permissions::PTW_ISSUE).await;
    let err = req::clear_permit_rule_flag(&db, clear("Inspection visuelle seulement"), elsewhere)
        .await
        .expect_err("wrong site");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);
    // The right at the site, a reason too short.
    let issuer = site_holder(&db, "issuer1", 1, crate::rbac::permissions::PTW_ISSUE).await;
    let err = req::clear_permit_rule_flag(&db, clear("  court  "), issuer)
        .await
        .expect_err("reason");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_REASON);
    assert_eq!(cleared_events(&db, wo.id).await, 0);

    let cleared = req::clear_permit_rule_flag(&db, clear("Inspection visuelle seulement"), issuer)
        .await
        .expect("cleared at the site");
    assert!(!cleared.requires_permit);
    assert!(cleared.permit_from_safety_di, "the origin stays on record");
    assert_eq!(cleared.permit_rule_cleared_by_id, Some(issuer));
    assert!(cleared.permit_rule_cleared_at.is_some());
    assert_eq!(
        cleared.permit_rule_clear_reason.as_deref(),
        Some("Inspection visuelle seulement")
    );
    assert_eq!(cleared_events(&db, wo.id).await, 1, "audited");
    let payload: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT payload_json FROM wo_action_events WHERE wo_id = {} AND action_code = 'permit_rule_flag_cleared'",
                wo.id
            ),
        ))
        .await
        .expect("q")
        .expect("event")
        .try_get_by_index(0)
        .expect("payload");
    assert!(payload.contains("safety_request"), "{payload}");
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo.id)
        .await
        .expect("no permit needed once cleared");
    // Once cleared it is no longer protected: the general edit may require a permit again.
    req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: cleared.row_version,
            requires_permit: true,
        },
        actor,
    )
    .await
    .expect("set again");
}

/// A work order without a site: the permit-issue right plant-wide only. A plant rule keeps its
/// plant-wide clearing (a site role does not lift a plant rule), even on a safety request.
#[tokio::test]
async fn without_a_site_or_with_a_plant_rule_a_site_right_does_not_clear() {
    let db = setup().await;
    let actor = admin(&db).await;
    let issuer = site_holder(&db, "issuer1", 1, crate::rbac::permissions::PTW_ISSUE).await;

    let no_site = convert_request(&db, actor, true).await;
    exec(
        &db,
        &format!("UPDATE work_orders SET entity_id = NULL WHERE id = {}", no_site.wo_id),
    )
    .await;
    let wo = crate::wo::queries::get_work_order(&db, no_site.wo_id)
        .await
        .expect("q")
        .expect("wo");
    let err = req::clear_permit_rule_flag(
        &db,
        WoPermitRuleClearInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: "Inspection visuelle seulement".into(),
        },
        issuer,
    )
    .await
    .expect_err("no site: plant-wide only");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);

    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let ruled = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, ruled.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert_eq!(wo.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
    assert!(wo.permit_from_safety_di);
    let input = WoPermitRuleClearInput {
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        reason: "Inspection visuelle seulement".into(),
    };
    let err = req::clear_permit_rule_flag(&db, input.clone(), issuer)
        .await
        .expect_err("a plant rule stays plant-wide");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);
    let cleared = req::clear_permit_rule_flag(&db, input, actor)
        .await
        .expect("plant-wide right");
    assert!(!cleared.requires_permit);
}

// ── Review of AU-01: manual create, site move, scope cases ────────────────────────────────────

/// A submitted request on the pump flagged "sécurité" (not converted).
async fn safety_request(db: &DatabaseConnection, actor: i64) -> i64 {
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("symptom")
        .expect("seeded symptom");
    create_intervention_request(
        db,
        DiCreateInput {
            equipment_id: 1,
            org_node_id: 1,
            title: "Fuite".into(),
            description: "Fuite sur la pompe".into(),
            origin_type: "operator".into(),
            request_type: "repair".into(),
            symptom_code_id: Some(symptom),
            impact_level: "unknown".into(),
            production_impact: false,
            safety_flag: true,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "normal".into(),
            observed_at: None,
            source_inspection_anomaly_id: None,
            submitter_id: actor,
        },
    )
    .await
    .expect("request")
    .id
}

/// A work order created by hand from a request flagged "sécurité" requires a permit and carries that
/// origin, whatever the form sent: the general edit cannot untick it.
#[tokio::test]
async fn a_manual_work_order_from_a_safety_request_is_protected() {
    let db = setup().await;
    let actor = admin(&db).await;
    let di = safety_request(&db, actor).await;
    let wo = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(1),
            location_id: None,
            source_di_id: Some(di),
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: Some(1),
            planner_id: None,
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: "Fuite pompe".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor,
            requires_permit: Some(false),
        },
    )
    .await
    .expect("create");
    assert!(wo.requires_permit, "forced by the safety request");
    assert!(wo.permit_from_safety_di);
    assert_eq!(wo.permit_rule_code, None, "no plant rule configured");
    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("the general edit cannot clear it");
    assert_eq!(code_of(&err), req::CODE_SAFETY_FLAG_NEEDS_CLEAR);
}

/// A safety requirement cleared at its site no longer holds when the work order moves to another
/// site: the clearing is withdrawn in the same change and the move is in the history and the audit
/// trail with both sites.
#[tokio::test]
async fn a_cleared_safety_requirement_is_reinstated_when_the_work_order_changes_site() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_second_site(&db).await;
    exec(
        &db,
        "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         equipment_class_ref_id, created_at, updated_at) \
         SELECT 2, 'pr-eq-2', 'PR-PUMP-2', 'Pump 2', 'active_in_service', 2, equipment_class_ref_id, \
                datetime('now'), datetime('now') FROM equipment WHERE id = 1",
    )
    .await;
    let result = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, result.wo_id)
        .await
        .expect("q")
        .expect("wo");
    let issuer = site_holder(&db, "issuer1", 1, crate::rbac::permissions::PTW_ISSUE).await;
    let cleared = req::clear_permit_rule_flag(
        &db,
        WoPermitRuleClearInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: "Inspection visuelle seulement".into(),
        },
        issuer,
    )
    .await
    .expect("cleared at site 1");
    assert!(!cleared.requires_permit);
    // The clearing's audit entry names the site it was decided at.
    let details = text_of(
        &db,
        &format!(
            "SELECT details_json FROM wo_change_events WHERE wo_id = {} AND action = 'permit_rule_flag_cleared'",
            wo.id
        ),
    )
    .await;
    assert!(details.contains("\"entity_id\":1"), "{details}");

    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'draft') WHERE id = {}",
            wo.id
        ),
    )
    .await;
    let moved = crate::wo::queries::update_wo_draft_fields(
        &db,
        crate::wo::domain::WoDraftUpdateInput {
            id: wo.id,
            expected_row_version: cleared.row_version,
            title: None,
            type_code: None,
            equipment_id: Some(2),
            location_id: None,
            description: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            notes: None,
            urgency_code: None,
            subtype_code: None,
            requires_permit: None,
        },
        Some(actor),
    )
    .await
    .expect("moved to site 2");
    assert_eq!(moved.entity_id, Some(2));
    assert!(moved.requires_permit, "the requirement is in force again");
    assert_eq!(moved.permit_rule_cleared_at, None);
    assert_eq!(moved.permit_rule_cleared_by_id, None);
    assert_eq!(moved.permit_rule_clear_reason, None);
    let voided = text_of(
        &db,
        &format!(
            "SELECT payload_json FROM wo_action_events WHERE wo_id = {} AND action_code = 'permit_clearing_voided'",
            wo.id
        ),
    )
    .await;
    assert!(
        voided.contains("\"old_site\":1") && voided.contains("\"new_site\":2"),
        "{voided}"
    );
    // Who moved it, and the withdrawn clearer by their sync id too (another PC does not share local ids).
    assert!(
        voided.contains("\"voided_cleared_by_sync_id\":\"pr-issuer1\""),
        "{voided}"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {} AND action_code = 'permit_clearing_voided' \
                 AND actor_id = {actor}",
                wo.id
            ),
        )
        .await,
        1,
        "the actor is recorded"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {} AND action = 'permit_clearing_voided'",
                wo.id
            ),
        )
        .await,
        1,
        "audited"
    );
    // Protected again: the general edit cannot untick it.
    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: moved.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("protected again");
    assert_eq!(code_of(&err), req::CODE_SAFETY_FLAG_NEEDS_CLEAR);
}

async fn text_of(db: &DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<String>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// The right at the work order's site is a permanent role on its node or a parent: an emergency
/// grant there does not count, nor a role on a node below it.
#[tokio::test]
async fn the_site_right_is_permanent_and_held_on_the_node_or_a_parent() {
    let db = setup().await;
    let actor = admin(&db).await;
    for sql in [
        // Site 1 sits under a region (3); a workshop (4) sits under site 1.
        "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, \
         structure_model_id) VALUES (3, 'pr-node-3', 'PR-REGION', 'Region', 1, 'active', datetime('now'), \
         datetime('now'), 1)",
        "UPDATE org_nodes SET parent_id = 3 WHERE id = 1",
        "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, parent_id, created_at, \
         updated_at, structure_model_id) VALUES (4, 'pr-node-4', 'PR-WORKSHOP', 'Workshop', 1, 'active', 1, \
         datetime('now'), datetime('now'), 1)",
    ] {
        exec(&db, sql).await;
    }
    let result = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, result.wo_id)
        .await
        .expect("q")
        .expect("wo");
    let clear = || WoPermitRuleClearInput {
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        reason: "Inspection visuelle seulement".into(),
    };

    let emergency = site_holder(&db, "emergency1", 1, crate::rbac::permissions::PTW_ISSUE).await;
    exec(
        &db,
        &format!(
            "UPDATE user_scope_assignments SET is_emergency = 1, emergency_expires_at = '2099-01-01T00:00:00Z' \
             WHERE user_id = {emergency}"
        ),
    )
    .await;
    let err = req::clear_permit_rule_flag(&db, clear(), emergency)
        .await
        .expect_err("an emergency grant does not clear it");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);

    let below = site_holder(&db, "workshop4", 4, crate::rbac::permissions::PTW_ISSUE).await;
    let err = req::clear_permit_rule_flag(&db, clear(), below)
        .await
        .expect_err("a role below the site does not clear it");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_RIGHT);

    let above = site_holder(&db, "region3", 3, crate::rbac::permissions::PTW_ISSUE).await;
    let cleared = req::clear_permit_rule_flag(&db, clear(), above)
        .await
        .expect("a role on a parent of the site clears it");
    assert!(!cleared.requires_permit);
}

/// The withdrawn clearings of a work order: action events × 10 + audit trail entries.
async fn withdrawn_count(db: &DatabaseConnection, wo_id: i64) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT (SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {wo_id} \
                     AND action_code = 'permit_clearing_withdrawn') * 10 + \
                    (SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {wo_id} \
                     AND action = 'permit_clearing_withdrawn')"
        ),
    )
    .await
}

/// A cleared protected requirement required again by the general edit (the permit box, the draft
/// form) is protected again: its clearing is withdrawn and audited, and unticking it again needs the
/// permit-issue clearing. Both origins.
#[tokio::test]
async fn a_cleared_requirement_required_again_is_protected_again() {
    let db = setup().await;
    let actor = admin(&db).await;

    // A plant rule's flag, required again with the permit box.
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let ruled = convert(&db, actor).await;
    let wo = crate::wo::queries::get_work_order(&db, ruled.wo_id)
        .await
        .expect("q")
        .expect("wo");
    let cleared = req::clear_permit_rule_flag(
        &db,
        WoPermitRuleClearInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: "Inspection visuelle seulement".into(),
        },
        actor,
    )
    .await
    .expect("cleared");
    let again = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: cleared.row_version,
            requires_permit: true,
        },
        actor,
    )
    .await
    .expect("required again");
    assert!(again.requires_permit);
    assert_eq!(again.permit_rule_cleared_at, None, "clearing withdrawn");
    assert_eq!(again.permit_rule_cleared_by_id, None);
    assert_eq!(again.permit_rule_clear_reason, None);
    assert_eq!(
        withdrawn_count(&db, wo.id).await,
        11,
        "in the history and the audit trail"
    );
    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: again.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("protected again");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_NEEDS_CLEAR);

    // A safety request's requirement (no plant rule: the rule is switched off), required again with
    // the draft form.
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'PUMPS_LOTO'",
    )
    .await;
    let safety = convert_request(&db, actor, true).await;
    let wo = crate::wo::queries::get_work_order(&db, safety.wo_id)
        .await
        .expect("q")
        .expect("wo");
    assert_eq!(wo.permit_rule_code, None, "safety origin only");
    let wo = crate::wo::queries::get_work_order(&db, safety.wo_id)
        .await
        .expect("q")
        .expect("wo");
    let cleared = req::clear_permit_rule_flag(
        &db,
        WoPermitRuleClearInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: "Inspection visuelle seulement".into(),
        },
        actor,
    )
    .await
    .expect("cleared");
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'draft') WHERE id = {}",
            wo.id
        ),
    )
    .await;
    let draft = |rv: i64, requires: bool| crate::wo::domain::WoDraftUpdateInput {
        id: wo.id,
        expected_row_version: rv,
        title: None,
        type_code: None,
        equipment_id: None,
        location_id: None,
        description: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        notes: None,
        urgency_code: None,
        subtype_code: None,
        requires_permit: Some(requires),
    };
    let again = crate::wo::queries::update_wo_draft_fields(&db, draft(cleared.row_version, true), Some(actor))
        .await
        .expect("required again");
    assert!(again.requires_permit);
    assert_eq!(again.permit_rule_cleared_at, None, "clearing withdrawn");
    assert_eq!(withdrawn_count(&db, wo.id).await, 11);
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {} AND action = 'permit_clearing_withdrawn' \
                 AND actor_id = {actor}",
                wo.id
            ),
        )
        .await,
        1,
        "the actor of the draft form is recorded"
    );
    let err = crate::wo::queries::update_wo_draft_fields(&db, draft(again.row_version, false), None)
        .await
        .expect_err("protected again");
    assert_eq!(code_of(&err), req::CODE_SAFETY_FLAG_NEEDS_CLEAR);
}

/// A request flagged "sécurité" after a work order was linked to it: its work orders not started yet
/// are protected in the same change (origin, permit required, recorded with the actor); a work order
/// already past "ready" is left as it is.
#[tokio::test]
async fn a_request_flagged_after_its_work_order_was_linked_protects_it() {
    let db = setup().await;
    let actor = admin(&db).await;
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(&db, "vibration")
        .await
        .expect("symptom")
        .expect("seeded symptom");
    let request = |title: &'static str| DiCreateInput {
        equipment_id: 1,
        org_node_id: 1,
        title: title.into(),
        description: "Fuite sur la pompe".into(),
        origin_type: "operator".into(),
        request_type: "repair".into(),
        symptom_code_id: Some(symptom),
        impact_level: "unknown".into(),
        production_impact: false,
        safety_flag: false,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: "normal".into(),
        observed_at: None,
        source_inspection_anomaly_id: None,
        submitter_id: actor,
    };
    let linked = |di: i64| crate::wo::domain::WoCreateInput {
        type_code: "corrective".into(),
        equipment_id: Some(1),
        location_id: None,
        source_di_id: Some(di),
        source_inspection_anomaly_id: None,
        source_ram_ishikawa_diagram_id: None,
        source_ishikawa_flow_node_id: None,
        source_rca_cause_text: None,
        entity_id: Some(1),
        planner_id: None,
        urgency_code: Some("normal".into()),
        subtype_code: None,
        title: "Fuite pompe".into(),
        description: None,
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        creator_id: actor,
        requires_permit: Some(false),
    };
    let flag = |di: &crate::di::domain::InterventionRequest| crate::di::queries::DiDraftUpdateInput {
        id: di.id,
        expected_row_version: di.row_version,
        title: None,
        description: None,
        request_type: None,
        symptom_code_id: None,
        impact_level: None,
        production_impact: None,
        safety_flag: Some(true),
        environmental_flag: None,
        quality_flag: None,
        reported_urgency: None,
        observed_at: None,
    };

    // Linked while the request was not flagged: no permit, no origin.
    let di = create_intervention_request(&db, request("Fuite"))
        .await
        .expect("request");
    let wo = crate::wo::queries::create_work_order(&db, linked(di.id))
        .await
        .expect("create");
    assert!(!wo.requires_permit && !wo.permit_from_safety_di);
    // The request is flagged: the draft work order is protected in the same change.
    crate::di::queries::update_di_draft_fields(&db, flag(&di), Some(actor))
        .await
        .expect("flagged");
    let raised = crate::wo::queries::get_work_order(&db, wo.id)
        .await
        .expect("q")
        .expect("wo");
    assert!(raised.requires_permit, "a permit is required");
    assert!(raised.permit_from_safety_di, "the origin is recorded");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT (SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {id} \
                         AND action_code = 'permit_safety_origin_raised' AND actor_id = {actor}) * 10 + \
                        (SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {id} \
                         AND action = 'permit_safety_origin_raised' AND actor_id = {actor})",
                id = wo.id
            ),
        )
        .await,
        11,
        "in the history and the audit trail, with the actor"
    );
    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: raised.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("the general edit cannot clear it");
    assert_eq!(code_of(&err), req::CODE_SAFETY_FLAG_NEEDS_CLEAR);

    // A work order already past "ready" is not changed (its start was judged when it started).
    let di = create_intervention_request(&db, request("Fuite 2"))
        .await
        .expect("request");
    let started = crate::wo::queries::create_work_order(&db, linked(di.id))
        .await
        .expect("create");
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress') \
             WHERE id = {}",
            started.id
        ),
    )
    .await;
    crate::di::queries::update_di_draft_fields(&db, flag(&di), Some(actor))
        .await
        .expect("flagged");
    let kept = crate::wo::queries::get_work_order(&db, started.id)
        .await
        .expect("q")
        .expect("wo");
    assert!(
        !kept.requires_permit && !kept.permit_from_safety_di,
        "started: left as it is"
    );
}

/// A work order on the pump created directly (the form), with the explicit tick given.
fn direct_input(actor: i64, requires_permit: Option<bool>) -> crate::wo::domain::WoCreateInput {
    crate::wo::domain::WoCreateInput {
        type_code: "corrective".into(),
        equipment_id: Some(1),
        location_id: None,
        source_di_id: None,
        source_inspection_anomaly_id: None,
        source_ram_ishikawa_diagram_id: None,
        source_ishikawa_flow_node_id: None,
        source_rca_cause_text: None,
        entity_id: Some(1),
        planner_id: None,
        urgency_code: Some("normal".into()),
        subtype_code: None,
        title: "Fuite pompe".into(),
        description: None,
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        creator_id: actor,
        requires_permit,
    }
}

/// Owner decision D2 (a), audit finding AU-04: a work order created directly gets the plant's rule
/// like a converted request, whatever the form's tick says, and the rule's protection applies.
#[tokio::test]
async fn a_work_order_created_directly_gets_the_plant_rule() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let wo = crate::wo::queries::create_work_order(&db, direct_input(actor, Some(false)))
        .await
        .expect("create");
    assert!(wo.requires_permit, "the rule requires it, the form left it unticked");
    assert_eq!(wo.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
    assert_eq!(wo.permit_rule_permit_type.as_deref(), Some("loto"));
    assert!(!wo.permit_from_safety_di);

    // The protection of a rule flag: no start without a permit, no clearing by the general edit.
    let err = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo.id)
        .await
        .expect_err("needs a permit");
    assert_eq!(code_of(&err), "PERMIT_REQUIRED_TO_START");
    let err = req::set_permit_requirement(
        &db,
        WoPermitRequirementInput {
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            requires_permit: false,
        },
        actor,
    )
    .await
    .expect_err("edit cannot clear");
    assert_eq!(code_of(&err), req::CODE_RULE_FLAG_NEEDS_CLEAR);
}

/// A rule only adds a requirement: an explicit tick stands when no rule matches, and the work is
/// not flagged when nothing asks for a permit.
#[tokio::test]
async fn a_rule_never_removes_an_explicit_requirement() {
    let db = setup().await;
    let actor = admin(&db).await;
    let plain = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    assert!(!plain.requires_permit, "no rule, no tick: no permit");
    assert_eq!(plain.permit_rule_code, None);

    add_rule(
        &db,
        "VALVES_HOT_WORK",
        serde_json::json!({ "equipment_class_code": "VALVE", "permit_type_code": "hot_work" }),
    )
    .await;
    let ticked = crate::wo::queries::create_work_order(&db, direct_input(actor, Some(true)))
        .await
        .expect("create");
    assert!(ticked.requires_permit, "the tick stands when no rule matches");
    assert_eq!(ticked.permit_rule_code, None);

    add_rule(
        &db,
        "CORRECTIVE_LOTO",
        serde_json::json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    let both = crate::wo::queries::create_work_order(&db, direct_input(actor, Some(true)))
        .await
        .expect("create");
    assert!(both.requires_permit);
    assert_eq!(
        both.permit_rule_code.as_deref(),
        Some("CORRECTIVE_LOTO"),
        "matched on the work type"
    );
}

/// A work order created from a request flagged "sécurité" gets both origins: the plant's rule and
/// the request's safety flag (AU-01).
#[tokio::test]
async fn a_manual_work_order_from_a_safety_request_gets_the_rule_too() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let di = safety_request(&db, actor).await;
    let wo = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            source_di_id: Some(di),
            ..direct_input(actor, Some(false))
        },
    )
    .await
    .expect("create");
    assert!(wo.requires_permit);
    assert!(wo.permit_from_safety_di);
    assert_eq!(wo.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
}

// ── Review of AU-04: draft edits (B1) and unreadable rules (S1) ───────────────────────────────

/// A draft edit that changes nothing but what `change` sets.
fn draft_edit(wo: &crate::wo::domain::WorkOrder) -> crate::wo::domain::WoDraftUpdateInput {
    crate::wo::domain::WoDraftUpdateInput {
        id: wo.id,
        expected_row_version: wo.row_version,
        title: None,
        type_code: None,
        equipment_id: None,
        location_id: None,
        description: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        notes: None,
        urgency_code: None,
        subtype_code: None,
        requires_permit: None,
    }
}

/// A second equipment on the site, without a class (no rule on a class matches it).
async fn plain_equipment(db: &DatabaseConnection) -> i64 {
    exec(
        db,
        "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES (2, 'pr-eq-2', 'PR-TANK', 'Tank', 'active_in_service', 1, datetime('now'), \
         datetime('now'))",
    )
    .await;
    2
}

async fn action_events(db: &DatabaseConnection, wo_id: i64, action: &str) -> i64 {
    scalar(
        db,
        &format!("SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {wo_id} AND action_code = '{action}'"),
    )
    .await
}

/// B1: a draft moved to an equipment a rule covers gets the rule, even when the same save sends the
/// permit unticked; the start gate then asks for the permit. Moved back, the rule stays (adds only).
#[tokio::test]
async fn a_draft_moved_to_a_ruled_equipment_gets_the_rule_and_keeps_it() {
    let db = setup().await;
    let actor = admin(&db).await;
    let tank = plain_equipment(&db).await;
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let wo = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            equipment_id: Some(tank),
            ..direct_input(actor, None)
        },
    )
    .await
    .expect("create");
    assert_eq!(wo.permit_rule_code, None, "no rule on the tank");

    let moved = crate::wo::queries::update_wo_draft_fields(
        &db,
        crate::wo::domain::WoDraftUpdateInput {
            equipment_id: Some(1),
            requires_permit: Some(false),
            ..draft_edit(&wo)
        },
        Some(actor),
    )
    .await
    .expect("move to the pump");
    assert!(moved.requires_permit);
    assert_eq!(moved.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
    assert_eq!(moved.permit_rule_permit_type.as_deref(), Some("loto"));
    assert_eq!(
        action_events(&db, wo.id, "permit_rule_applied").await,
        1,
        "in its history"
    );
    let err = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo.id)
        .await
        .expect_err("needs a permit");
    assert_eq!(code_of(&err), "PERMIT_REQUIRED_TO_START");

    let back = crate::wo::queries::update_wo_draft_fields(
        &db,
        crate::wo::domain::WoDraftUpdateInput {
            equipment_id: Some(tank),
            ..draft_edit(&moved)
        },
        Some(actor),
    )
    .await
    .expect("move back");
    assert!(back.requires_permit, "a rule only adds");
    assert_eq!(back.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
}

/// B1: a draft whose work type changes to one a rule covers gets the rule.
#[tokio::test]
async fn a_draft_whose_work_type_changes_gets_the_rule() {
    let db = setup().await;
    let actor = admin(&db).await;
    let tank = plain_equipment(&db).await;
    add_rule(
        &db,
        "CORRECTIVE_LOTO",
        serde_json::json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    let wo = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            type_code: "preventive".into(),
            equipment_id: Some(tank),
            ..direct_input(actor, None)
        },
    )
    .await
    .expect("create");
    assert_eq!(wo.permit_rule_code, None);
    let changed = crate::wo::queries::update_wo_draft_fields(
        &db,
        crate::wo::domain::WoDraftUpdateInput {
            type_code: Some("corrective".into()),
            ..draft_edit(&wo)
        },
        Some(actor),
    )
    .await
    .expect("type change");
    assert!(changed.requires_permit);
    assert_eq!(changed.permit_rule_code.as_deref(), Some("CORRECTIVE_LOTO"));
}

/// S1: a published rule that matches but names a permit type not held here still flags the work
/// order (fail closed, its permit type as written); one whose settings do not read is not applied.
/// Both are recorded in the work order's history and audit trail.
#[tokio::test]
async fn rules_that_do_not_resolve_flag_and_unreadable_ones_are_recorded() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "PUMPS_NEW_PERMIT",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    add_rule(
        &db,
        "BROKEN",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    // As received from a PC where the permit type exists, and a rule damaged since.
    exec(
        &db,
        "UPDATE reference_values SET metadata_json = '{\"equipment_class_code\":\"PUMP\",\
         \"permit_type_code\":\"confined_space_new\"}' WHERE code = 'PUMPS_NEW_PERMIT'",
    )
    .await;
    exec(
        &db,
        "UPDATE reference_values SET metadata_json = 'not json' WHERE code = 'BROKEN'",
    )
    .await;

    let wo = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    assert!(wo.requires_permit, "fail closed");
    assert_eq!(wo.permit_rule_code.as_deref(), Some("PUMPS_NEW_PERMIT"));
    assert_eq!(wo.permit_rule_permit_type.as_deref(), Some("confined_space_new"));
    assert_eq!(action_events(&db, wo.id, "permit_rules_unreadable").await, 1);
    let details = text_of(
        &db,
        &format!(
            "SELECT payload_json FROM wo_action_events WHERE wo_id = {} AND action_code = 'permit_rules_unreadable'",
            wo.id
        ),
    )
    .await;
    assert!(
        details.contains("BROKEN") && details.contains("PUMPS_NEW_PERMIT"),
        "{details}"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {} AND action = 'permit_rules_unreadable'",
                wo.id
            ),
        )
        .await,
        1,
        "audited"
    );
}

/// B2: a rule published here flags the work orders not started yet that it matches (written as local
/// changes, sent to the other PCs); started ones are left as they are.
#[tokio::test]
async fn a_rule_published_here_flags_the_open_work_orders_it_matches() {
    let db = setup().await;
    let actor = admin(&db).await;
    let open = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    let started = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), \
             actual_start = '2026-10-01T08:00:00Z' WHERE id = {}",
            started.id
        ),
    )
    .await;
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    let set: i64 = scalar(
        &db,
        &format!(
            "SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = '{}' AND rs.status = 'published'",
            rules::DOMAIN
        ),
    )
    .await;
    // Capture on (as on a PC that synchronises); only the stamp's capture is counted below.
    crate::sync::capture::install_capture(&db).await.expect("capture");
    exec(
        &db,
        "DELETE FROM sync_capture_journal WHERE entity_type = 'work_orders'",
    )
    .await;
    rules::after_rule_list_change(&db, set, Some(actor))
        .await
        .expect("stamp");
    let open = crate::wo::queries::get_work_order(&db, open.id)
        .await
        .expect("q")
        .expect("wo");
    assert!(open.requires_permit);
    assert_eq!(open.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
    let started = crate::wo::queries::get_work_order(&db, started.id)
        .await
        .expect("q")
        .expect("wo");
    assert_eq!(started.permit_rule_code, None, "a started work order is left as it is");
    // Written as a local change: captured, to be sent to the other PCs.
    let open_sync = open.entity_sync_id.clone().expect("sync id");
    assert!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'work_orders' \
                 AND entity_sync_id = '{open_sync}'"
            ),
        )
        .await
            >= 1,
        "captured to be sent"
    );
}

/// R2-S2: the catch-up run at start-up and after each synchronisation pass flags the work orders not
/// started yet that a rule matches but that missed it (here a rule written to the list without
/// its stamp, as a failed or older run would leave it), derived here (never sent), once; a started
/// work order is left as it is.
#[tokio::test]
async fn the_catch_up_after_a_restart_flags_the_open_work_orders_a_rule_matches() {
    let db = setup().await;
    let actor = admin(&db).await;
    let open = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    let started = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), \
             actual_start = '2026-10-01T08:00:00Z' WHERE id = {}",
            started.id
        ),
    )
    .await;
    // The rule is in the published list, its stamp missed (no rule-list hook ran).
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    crate::sync::capture::install_capture(&db).await.expect("capture");
    exec(
        &db,
        "DELETE FROM sync_capture_journal WHERE entity_type = 'work_orders'",
    )
    .await;
    // The start-up run (`startup.rs`, and the end of each pass, `sync::materialize`).
    assert_eq!(rules::catch_up(&db).await, 1);
    let open = crate::wo::queries::get_work_order(&db, open.id)
        .await
        .expect("q")
        .expect("wo");
    assert!(open.requires_permit);
    assert_eq!(open.permit_rule_code.as_deref(), Some("PUMPS_LOTO"));
    assert_eq!(open.permit_rule_permit_type.as_deref(), Some("loto"));
    let cause = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {} AND action_code = 'permit_rule_applied' \
             AND json_extract(payload_json, '$.cause') = 'catch_up'",
            open.id
        ),
    )
    .await;
    assert_eq!(cause, 1, "recorded with its cause");
    let started = crate::wo::queries::get_work_order(&db, started.id)
        .await
        .expect("q")
        .expect("wo");
    assert_eq!(started.permit_rule_code, None, "a started work order is left as it is");
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'work_orders'",
        )
        .await,
        0,
        "derived on every PC: never sent"
    );
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM sync_capture_suppression").await,
        0,
        "capture back on"
    );
    // Run again: nothing more.
    assert_eq!(rules::catch_up(&db).await, 0);
}

/// R3-N3: one work order the catch-up cannot write does not hold back the others; it is flagged by
/// the next run once it can be written.
#[tokio::test]
async fn a_failing_work_order_does_not_stop_the_catch_up_and_is_retried() {
    let db = setup().await;
    let actor = admin(&db).await;
    let blocked = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    let other = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    add_rule(
        &db,
        "PUMPS_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    exec(
        &db,
        &format!(
            "CREATE TRIGGER test_catch_up_fails BEFORE UPDATE OF permit_rule_code ON work_orders \
             WHEN OLD.id = {} BEGIN SELECT RAISE(ABORT, 'test: cannot write'); END",
            blocked.id
        ),
    )
    .await;
    assert_eq!(rules::catch_up(&db).await, 1, "the other work order is flagged");
    let rule_of = |id: i64| {
        let db = db.clone();
        async move {
            crate::wo::queries::get_work_order(&db, id)
                .await
                .expect("q")
                .expect("wo")
                .permit_rule_code
        }
    };
    assert_eq!(rule_of(other.id).await.as_deref(), Some("PUMPS_LOTO"));
    assert_eq!(rule_of(blocked.id).await, None);
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM sync_capture_suppression").await,
        0,
        "capture back on after the failure"
    );
    exec(&db, "DROP TRIGGER test_catch_up_fails").await;
    assert_eq!(rules::catch_up(&db).await, 1, "retried by the next run");
    assert_eq!(rule_of(blocked.id).await.as_deref(), Some("PUMPS_LOTO"));
}

/// CMMS Advisor decision on AU-04 R5-S3 (option A): a new work order that a hot-work rule (first in
/// the list) and a LOTO rule both match gets the LOTO rule from its first stamp: a permit type that
/// requires an energy isolation outranks one that does not. Both types are shown on the work order.
#[tokio::test]
async fn the_first_stamp_prefers_the_isolation_type_and_shows_several_rules() {
    let db = setup().await;
    let actor = admin(&db).await;
    add_rule(
        &db,
        "A_PUMP_HOT",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "hot_work" }),
    )
    .await;
    add_rule(
        &db,
        "B_PUMP_LOTO",
        serde_json::json!({ "equipment_class_code": "PUMP", "permit_type_code": "loto" }),
    )
    .await;
    exec(
        &db,
        "UPDATE reference_values SET sort_order = 0 WHERE code = 'A_PUMP_HOT'",
    )
    .await;
    let wo = crate::wo::queries::create_work_order(&db, direct_input(actor, None))
        .await
        .expect("create");
    assert_eq!(wo.permit_rule_code.as_deref(), Some("B_PUMP_LOTO"));
    assert_eq!(wo.permit_rule_permit_type.as_deref(), Some("loto"));
    assert_eq!(
        rules::matching_permit_types(&db, wo.equipment_id, wo.type_id, wo.permit_rule_permit_type.as_deref())
            .await
            .expect("types"),
        vec!["hot_work".to_string(), "loto".to_string()],
        "several permit rules shown"
    );
    // One type only: nothing shown.
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'A_PUMP_HOT'",
    )
    .await;
    assert!(rules::matching_permit_types(&db, wo.equipment_id, wo.type_id, wo.permit_rule_permit_type.as_deref())
        .await
        .expect("types")
        .is_empty());
}
