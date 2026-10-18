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
    rules::ensure_permit_requirement_rules(db).await.expect("list");
    let checked = rules::checked_rule_metadata(db, code, Some(&metadata.to_string()))
        .await
        .expect("valid rule");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active, metadata_json) \
         SELECT rs.id, ?, ?, 1, 1, ? FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = ? AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
        [code.into(), code.into(), checked.into(), rules::DOMAIN.into()],
    ))
    .await
    .expect("rule");
}

/// An approved request on the pump, converted to a work order.
async fn convert(db: &DatabaseConnection, actor: i64) -> crate::di::conversion::WoConversionResult {
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
            safety_flag: false,
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
    let wp = create_from_wo(&db, "cold_work", wrong.wo_id).await.expect("cold work permit");
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
    let wp = create_from_wo(&db, "hot_work", right.wo_id).await.expect("hot work permit");
    set_status(&db, &advance_to_issued(&db, wp).await, "active")
        .await
        .expect("activate");
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, right.wo_id)
        .await
        .expect("a permit of the rule's type lets the work start");
}
