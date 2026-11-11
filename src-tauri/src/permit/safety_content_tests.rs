//! L4-03 / L4-04: a permit is issued only with its content, its HSE approval when its type requires
//! one, and a gas test within the limits of the settings when it needs one — on this PC (the issue
//! transition) and on a permit issued on another PC (sync re-check).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Map, Value};

use super::safety_content::{
    self, PermitGasTestInput, PermitHseApprovalInput, PermitSafetyContentInput, CODE_GAS_O2_OUT_OF_LIMITS,
    CODE_GAS_TEST_MISSING, CODE_HSE_APPROVAL_MISSING, CODE_PPE_MISSING, CODE_PREVENTIVE_MEASURES_MISSING,
    CODE_RISK_ASSESSMENT_MISSING, CODE_WORK_DESCRIPTION_MISSING,
};
use super::tests::{insert_equipment, set_status, setup, type_id, APPLIER, VERIFIER};
use crate::errors::AppError;
use crate::permit::domain::{WorkPermit, WorkPermitCreateInput, WorkPermitStatusInput};
use crate::permit::queries;
use crate::sync::materialize::ApplyOutcome;

fn codes(err: &AppError) -> Vec<String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.iter().map(|i| i.code.clone()).collect(),
        AppError::ValidationFailed(messages) => messages
            .iter()
            .map(|m| m.split(':').next().unwrap_or_default().to_string())
            .collect(),
        other => panic!("unexpected error {other:?}"),
    }
}

async fn reload(db: &DatabaseConnection, wp: &WorkPermit) -> WorkPermit {
    queries::get_work_permit(db, wp.id)
        .await
        .expect("read")
        .expect("permit")
}

/// A permit written by APPLIER, expiring within the shortest seeded duration (confined space: 4 h).
async fn create(db: &DatabaseConnection, type_code: &str, asset: i64) -> WorkPermit {
    queries::create_work_permit(
        db,
        WorkPermitCreateInput {
            permit_type_id: type_id(db, type_code).await,
            asset_id: Some(asset),
            entity_id: Some(1),
            linked_work_order_id: None,
            expires_at: Some((chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339()),
        },
        APPLIER,
    )
    .await
    .expect("create permit")
}

/// The issue as the screen asks for it (no test helper fills anything).
async fn issue(db: &DatabaseConnection, wp: &WorkPermit) -> Result<WorkPermit, AppError> {
    queries::set_work_permit_status(
        db,
        WorkPermitStatusInput {
            id: wp.id,
            status: "issued".into(),
            expected_row_version: wp.row_version,
            reason: None,
        },
        APPLIER,
    )
    .await
}

fn content(wp: &WorkPermit) -> PermitSafetyContentInput {
    PermitSafetyContentInput {
        permit_id: wp.id,
        expected_row_version: wp.row_version,
        work_description: "  Remplacement de la garniture mécanique de la pompe  ".into(),
        risk_assessment: "Fluide sous pression, produit chaud".into(),
        preventive_measures: "Vidange et purge, consignation électrique et hydraulique".into(),
        ppe_required: "Casque, lunettes, gants anti-chaleur".into(),
    }
}

async fn write_content(db: &DatabaseConnection, wp: &WorkPermit) -> WorkPermit {
    safety_content::save_permit_safety_content(db, content(wp), APPLIER)
        .await
        .expect("content");
    reload(db, wp).await
}

async fn approved(db: &DatabaseConnection, wp: WorkPermit) -> WorkPermit {
    let wp = set_status(db, &wp, "pending_review").await.expect("submit");
    set_status(db, &wp, "approved").await.expect("approve")
}

fn gas(wp: &WorkPermit, o2: f64, minutes_ago: i64) -> PermitGasTestInput {
    PermitGasTestInput {
        permit_id: wp.id,
        o2_percent: o2,
        lel_percent: 0.0,
        h2s_ppm: 0.0,
        co_ppm: Some(2.0),
        measured_at: (chrono::Utc::now() - chrono::Duration::minutes(minutes_ago)).to_rfc3339(),
        instrument: "Détecteur 4 gaz n° 12".into(),
    }
}

#[tokio::test]
async fn a_permit_without_its_content_is_not_issued_and_its_content_is_kept_trimmed() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-1").await;
    // Cold work: no HSE approval, no gas test (seeded type).
    let bare = approved(&db, create(&db, "cold_work", asset).await).await;
    let err = issue(&db, &bare).await.expect_err("no content");
    assert_eq!(
        codes(&err),
        vec![
            CODE_WORK_DESCRIPTION_MISSING,
            CODE_RISK_ASSESSMENT_MISSING,
            CODE_PREVENTIVE_MEASURES_MISSING,
            CODE_PPE_MISSING,
        ]
    );
    assert_eq!(reload(&db, &bare).await.status, "approved", "nothing written");
    // Approved: the content is frozen (it was reviewed as it is).
    let err = safety_content::save_permit_safety_content(&db, content(&bare), APPLIER)
        .await
        .expect_err("locked");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_CONTENT_LOCKED"]);

    // Written on the draft: issued.
    let wp = write_content(&db, &create(&db, "cold_work", asset).await).await;
    let shown = safety_content::get_permit_safety_content(&db, wp.id)
        .await
        .expect("read");
    assert_eq!(
        shown.work_description.as_deref(),
        Some("Remplacement de la garniture mécanique de la pompe")
    );
    assert!(shown.issue_blockers.is_empty(), "{:?}", shown.issue_blockers);
    let wp = approved(&db, wp).await;
    assert_eq!(issue(&db, &wp).await.expect("issue").status, "issued");
}

#[tokio::test]
async fn a_type_requiring_hse_approval_is_issued_only_once_it_is_recorded() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-2").await;
    // Hot work requires the HSE approval (seeded type).
    let incomplete = set_status(&db, &create(&db, "hot_work", asset).await, "pending_review")
        .await
        .expect("submit");
    let err = safety_content::record_permit_hse_approval(
        &db,
        PermitHseApprovalInput {
            permit_id: incomplete.id,
            expected_row_version: incomplete.row_version,
            note: None,
        },
        VERIFIER,
    )
    .await
    .expect_err("content first");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_HSE_CONTENT_INCOMPLETE"]);

    let wp = approved(&db, write_content(&db, &create(&db, "hot_work", asset).await).await).await;
    let err = issue(&db, &wp).await.expect_err("no HSE approval");
    assert_eq!(codes(&err), vec![CODE_HSE_APPROVAL_MISSING]);
    let shown = safety_content::record_permit_hse_approval(
        &db,
        PermitHseApprovalInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            note: Some("  Zone dégagée, extincteur sur place ".into()),
        },
        VERIFIER,
    )
    .await
    .expect("HSE approval");
    assert_eq!(shown.hse_approved_by_id, Some(VERIFIER));
    assert_eq!(
        shown.hse_approval_note.as_deref(),
        Some("Zone dégagée, extincteur sur place")
    );
    assert!(shown.issue_blockers.is_empty());
    let audited: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS c FROM audit_events WHERE action_code = 'permit.hse_approved' AND target_id = '{}'",
                wp.id
            ),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert_eq!(audited, 1);
    let wp = reload(&db, &wp).await;
    assert_eq!(issue(&db, &wp).await.expect("issue").status, "issued");
}

/// A change of content while the permit waits for review sends it back to draft: its submission
/// and any HSE approval of the former content are cleared.
#[tokio::test]
async fn a_content_change_in_review_returns_the_permit_to_draft_and_clears_the_hse_approval() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-3").await;
    let wp = write_content(&db, &create(&db, "hot_work", asset).await).await;
    let wp = set_status(&db, &wp, "pending_review").await.expect("submit");
    safety_content::record_permit_hse_approval(
        &db,
        PermitHseApprovalInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            note: None,
        },
        VERIFIER,
    )
    .await
    .expect("HSE approval");
    let wp = reload(&db, &wp).await;
    let mut changed = content(&wp);
    changed.risk_assessment = "Fluide sous pression, produit chaud, travail en hauteur".into();
    let shown = safety_content::save_permit_safety_content(&db, changed, APPLIER)
        .await
        .expect("change");
    assert_eq!(shown.permit_status, "draft");
    assert_eq!(shown.hse_approved_at, None);
    let wp = reload(&db, &wp).await;
    assert_eq!((wp.submitted_by_id, wp.submitted_at), (None, None));
    // A stale version is refused.
    let err = safety_content::save_permit_safety_content(
        &db,
        PermitSafetyContentInput {
            expected_row_version: wp.row_version - 1,
            ..content(&wp)
        },
        APPLIER,
    )
    .await
    .expect_err("stale");
    assert_eq!(codes(&err), vec!["GATE_RECORD_CHANGED_ELSEWHERE"]);
}

#[tokio::test]
async fn a_confined_space_permit_is_issued_only_on_a_latest_gas_test_within_the_limits() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-1").await;
    // Confined space: HSE approval and gas test (seeded type).
    let wp = approved(
        &db,
        write_content(&db, &create(&db, "confined_space", asset).await).await,
    )
    .await;
    safety_content::record_permit_hse_approval(
        &db,
        PermitHseApprovalInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            note: None,
        },
        VERIFIER,
    )
    .await
    .expect("HSE approval");
    let wp = reload(&db, &wp).await;
    assert_eq!(
        codes(&issue(&db, &wp).await.expect_err("no gas test")),
        vec![CODE_GAS_TEST_MISSING]
    );

    // Oxygen below the shipped 19.5 %: out of limits, refused.
    let shown = safety_content::record_permit_gas_test(&db, gas(&wp, 18.2, 10), APPLIER)
        .await
        .expect("gas test");
    assert!(!shown.gas_tests[0].within_limits);
    assert_eq!(shown.gas_tests[0].measured_by_id, APPLIER);
    let wp = reload(&db, &wp).await;
    assert_eq!(
        codes(&issue(&db, &wp).await.expect_err("low O2")),
        vec![CODE_GAS_O2_OUT_OF_LIMITS]
    );

    // A later reading within the limits: issued.
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 1), APPLIER)
        .await
        .expect("second gas test");
    let wp = reload(&db, &wp).await;
    assert_eq!(issue(&db, &wp).await.expect("issue").status, "issued");
}

/// The limits come from the settings: a stricter LEL limit set by the plant refuses a reading the
/// shipped limit accepts.
#[tokio::test]
async fn the_gas_limits_are_read_from_the_settings() {
    let db = setup().await;
    let limits = safety_content::gas_limits(&db).await.expect("limits");
    assert_eq!(
        (
            limits.o2_min_percent,
            limits.o2_max_percent,
            limits.lel_max_percent,
            limits.h2s_max_ppm,
            limits.co_max_ppm
        ),
        (19.5, 23.5, 10.0, 10.0, 25.0)
    );
    assert!(safety_content::reading_breaches(&limits, 20.9, 6.0, 0.0, None, "P").is_empty());
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "UPDATE app_settings SET setting_value_json = '5' WHERE setting_key = '{}'",
            safety_content::SETTING_GAS_LEL_MAX
        ),
    ))
    .await
    .expect("stricter LEL");
    let limits = safety_content::gas_limits(&db).await.expect("limits");
    let breaches = safety_content::reading_breaches(&limits, 20.9, 6.0, 0.0, Some(30.0), "P");
    let got: Vec<&str> = breaches.iter().map(|b| b.code.as_str()).collect();
    assert_eq!(
        got,
        vec![
            safety_content::CODE_GAS_LEL_TOO_HIGH,
            safety_content::CODE_GAS_CO_TOO_HIGH
        ]
    );
    assert_eq!(breaches[0].params.get("max").map(String::as_str), Some("5"));
}

/// The shipped confined-space type needs a gas test even with its "test atmosphérique" flag off;
/// a reading is refused when it cannot be one.
#[tokio::test]
async fn the_shipped_confined_space_type_always_needs_a_gas_test_and_readings_are_checked() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-2").await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE permit_types SET requires_atmospheric_test = 0 WHERE code = 'confined_space'".to_string(),
    ))
    .await
    .expect("flag off");
    let wp = write_content(&db, &create(&db, "confined_space", asset).await).await;
    let shown = safety_content::get_permit_safety_content(&db, wp.id)
        .await
        .expect("read");
    assert!(shown.requires_gas_test);
    assert!(shown.issue_blockers.iter().any(|b| b.code == CODE_GAS_TEST_MISSING));

    let mut negative = gas(&wp, 20.9, 1);
    negative.h2s_ppm = -1.0;
    let err = safety_content::record_permit_gas_test(&db, negative, APPLIER)
        .await
        .expect_err("negative");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_GAS_VALUE_INVALID"]);
    let mut blank = gas(&wp, 20.9, 1);
    blank.instrument = "  ".into();
    let err = safety_content::record_permit_gas_test(&db, blank, APPLIER)
        .await
        .expect_err("instrument");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_GAS_INSTRUMENT_REQUIRED"]);
    let future = gas(&wp, 20.9, -120);
    let err = safety_content::record_permit_gas_test(&db, future, APPLIER)
        .await
        .expect_err("future");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_GAS_TEST_TIME_INVALID"]);
}

// ── A permit issued on another PC ────────────────────────────────────────────────

type Payload = Map<String, Value>;

async fn wire(db: &DatabaseConnection, sync_id: &str) -> Payload {
    super::sync_check::local_wire(db, "work_permits", sync_id)
        .await
        .expect("payload")
        .expect("row")
}

async fn apply(db: &DatabaseConnection, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
    let layout = crate::sync::row_layout::layout(db, "work_permits")
        .await
        .expect("layout");
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        sync_id,
        "upsert",
        &serde_json::to_string(payload).expect("json"),
    )
    .await
    .expect("apply");
    if matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged) {
        tx.commit().await.expect("commit");
    } else {
        tx.rollback().await.expect("rollback");
    }
    crate::sync::row_codec::clear_ref_cache();
    outcome
}

fn conflict_code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

fn with(mut p: Payload, changes: &[(&str, Value)]) -> Payload {
    for (k, v) in changes {
        p.insert((*k).to_string(), v.clone());
    }
    p
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[tokio::test]
async fn a_permit_issued_elsewhere_is_judged_on_the_issue_gate() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-S1").await;
    // Held here approved, without its content: issued there without it, refused.
    let wp = approved(&db, create(&db, "cold_work", asset).await).await;
    let issued = with(
        wire(&db, &wp.entity_sync_id).await,
        &[("status", json!("issued")), ("issued_at", json!(now()))],
    );
    let outcome = apply(&db, &wp.entity_sync_id, &issued).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("PERMIT_ISSUE_CONTENT_MISSING"),
        "{outcome:?}"
    );
    assert_eq!(reload(&db, &wp).await.status, "approved", "nothing written");
    // With its content (written there before the review): applied.
    let complete = with(
        issued,
        &[
            ("work_description", json!("Nettoyage du filtre")),
            ("risk_assessment", json!("Coupures")),
            ("preventive_measures", json!("Outils adaptés")),
            ("ppe_required", json!("Gants")),
        ],
    );
    let outcome = apply(&db, &wp.entity_sync_id, &complete).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(reload(&db, &wp).await.status, "issued");

    // A confined-space permit with its content and HSE approval, issued there without a gas test.
    let cs = approved(
        &db,
        write_content(&db, &create(&db, "confined_space", asset).await).await,
    )
    .await;
    safety_content::record_permit_hse_approval(
        &db,
        PermitHseApprovalInput {
            permit_id: cs.id,
            expected_row_version: cs.row_version,
            note: None,
        },
        VERIFIER,
    )
    .await
    .expect("HSE approval");
    let issued = with(
        wire(&db, &cs.entity_sync_id).await,
        &[("status", json!("issued")), ("issued_at", json!(now()))],
    );
    let outcome = apply(&db, &cs.entity_sync_id, &issued).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("PERMIT_ISSUE_GAS_TEST_MISSING"),
        "{outcome:?}"
    );
    // Its gas test recorded here (in the limits): the issue received is applied.
    safety_content::record_permit_gas_test(&db, gas(&cs, 20.9, 1), APPLIER)
        .await
        .expect("gas test");
    let outcome = apply(&db, &cs.entity_sync_id, &issued).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}
