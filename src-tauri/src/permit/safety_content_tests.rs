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

/// A permit written by APPLIER, expiring within the shortest seeded duration (confined space: 4 h),
/// raised an hour ago (so readings of the last hour count).
async fn create(db: &DatabaseConnection, type_code: &str, asset: i64) -> WorkPermit {
    let wp = queries::create_work_permit(
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
    .expect("create permit");
    raised_hours_ago(db, &wp, 1).await;
    wp
}

/// The permit was raised `hours` ago (its request time; the row version is kept).
async fn raised_hours_ago(db: &DatabaseConnection, wp: &WorkPermit, hours: i64) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET requested_at = ? WHERE id = ?",
        [
            (chrono::Utc::now() - chrono::Duration::hours(hours))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
                .into(),
            wp.id.into(),
        ],
    ))
    .await
    .expect("raised earlier");
}

/// A confined-space permit approved with its content and its HSE approval (by VERIFIER).
async fn confined_approved(db: &DatabaseConnection, asset: i64) -> WorkPermit {
    let wp = approved(db, write_content(db, &create(db, "confined_space", asset).await).await).await;
    safety_content::record_permit_hse_approval(
        db,
        PermitHseApprovalInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            note: None,
        },
        VERIFIER,
    )
    .await
    .expect("HSE approval");
    reload(db, &wp).await
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
    assert_eq!((wp.submitted_by_id, wp.submitted_at.as_deref()), (None, None));
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
// ── Review of L4-03 / L4-04 and CMMS Advisor rulings of 2026-11-11 ───────────────────────────────

/// BLOCKING 1: a reading taken before the permit was raised is refused, and a reading older than
/// the re-test interval (shipped 4 h) does not allow the issue.
#[tokio::test]
async fn a_gas_test_before_the_permit_or_older_than_the_interval_does_not_allow_the_issue() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-R1").await;
    let wp = confined_approved(&db, asset).await;
    // Raised an hour ago: a reading of five hours ago describes another atmosphere.
    let err = safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 300), APPLIER)
        .await
        .expect_err("before the permit");
    assert_eq!(codes(&err), vec![safety_content::CODE_GAS_TEST_BEFORE_PERMIT]);

    // Raised ten hours ago: the reading of five hours ago is recorded, in the limits, but too old.
    raised_hours_ago(&db, &wp, 10).await;
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 300), APPLIER)
        .await
        .expect("old reading recorded");
    let wp = reload(&db, &wp).await;
    let err = issue(&db, &wp).await.expect_err("out of date");
    assert_eq!(codes(&err), vec![safety_content::CODE_GAS_TEST_EXPIRED]);
    assert_eq!(reload(&db, &wp).await.status, "approved", "nothing written");

    // A reading synchronised from before the permit was raised is kept but never counts.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_gas_tests (entity_sync_id, permit_id, o2_percent, lel_percent, h2s_ppm, co_ppm, measured_at, measured_by_id, instrument, row_version, created_at, updated_at) VALUES (?, ?, 20.9, 0, 0, NULL, ?, ?, 'Ancien détecteur', 1, ?, ?)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            wp.id.into(),
            (chrono::Utc::now() - chrono::Duration::hours(20))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
                .into(),
            APPLIER.into(),
            now().into(),
            now().into(),
        ],
    ))
    .await
    .expect("old synced reading");
    let shown = safety_content::get_permit_safety_content(&db, wp.id)
        .await
        .expect("read");
    assert_eq!(shown.gas_validity_hours, 4.0);
    assert!(
        shown.gas_tests.iter().any(|g| !g.counts),
        "the reading before the permit does not count"
    );

    // A fresh reading: issued.
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 1), APPLIER)
        .await
        .expect("fresh reading");
    let wp = reload(&db, &wp).await;
    assert_eq!(issue(&db, &wp).await.expect("issue").status, "issued");
}

/// BLOCKING 2: a newer reading out of the limits refuses the activation (and the reinstatement,
/// and the start of the work: the same `activation_ready`).
#[tokio::test]
async fn a_newer_failing_gas_test_refuses_the_activation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-R2").await;
    let wp = confined_approved(&db, asset).await;
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 2), APPLIER)
        .await
        .expect("good reading");
    let wp = issue(&db, &reload(&db, &wp).await).await.expect("issue");
    // The atmosphere changed after the issue.
    safety_content::record_permit_gas_test(&db, gas(&wp, 18.2, 1), APPLIER)
        .await
        .expect("failing reading");
    let activate = |wp: WorkPermit| {
        let db = db.clone();
        async move {
            queries::set_work_permit_status(
                &db,
                WorkPermitStatusInput {
                    id: wp.id,
                    status: "active".into(),
                    expected_row_version: wp.row_version,
                    reason: None,
                },
                APPLIER,
            )
            .await
        }
    };
    let err = activate(reload(&db, &wp).await).await.expect_err("low O2 now");
    assert_eq!(codes(&err), vec![CODE_GAS_O2_OUT_OF_LIMITS]);
    assert_eq!(reload(&db, &wp).await.status, "issued");
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 0), APPLIER)
        .await
        .expect("ventilated");
    assert_eq!(activate(reload(&db, &wp).await).await.expect("active").status, "active");
}

/// CMMS Advisor ruling 1: the author (or submitter) does not record the HSE approval; another
/// person does, flagged as no self-approval.
#[tokio::test]
async fn the_author_does_not_record_the_hse_approval() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-R1").await;
    let wp = set_status(
        &db,
        &write_content(&db, &create(&db, "hot_work", asset).await).await,
        "pending_review",
    )
    .await
    .expect("submit");
    let input = |wp: &WorkPermit| PermitHseApprovalInput {
        permit_id: wp.id,
        expected_row_version: wp.row_version,
        note: None,
    };
    let err = safety_content::record_permit_hse_approval(&db, input(&wp), APPLIER)
        .await
        .expect_err("author");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_HSE_APPROVER_IS_AUTHOR"]);
    let shown = safety_content::record_permit_hse_approval(&db, input(&wp), VERIFIER)
        .await
        .expect("second person");
    assert_eq!(shown.hse_self_approved, Some(false));
}

/// Review should-fix 4: a return to draft (here a change of equipment while waiting for review)
/// clears the HSE approval, and the gas tests taken before stop counting.
#[tokio::test]
async fn a_change_of_equipment_clears_the_hse_approval_and_the_earlier_gas_tests() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-R3").await;
    let other = insert_equipment(&db, "EQ-L404-R3B").await;
    let wp = write_content(&db, &create(&db, "confined_space", asset).await).await;
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 5), APPLIER)
        .await
        .expect("reading");
    let wp = set_status(&db, &reload(&db, &wp).await, "pending_review")
        .await
        .expect("submit");
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
    .expect("HSE");
    let wp = reload(&db, &wp).await;
    queries::update_work_permit(
        &db,
        crate::permit::domain::WorkPermitUpdateInput {
            id: wp.id,
            linked_work_order_id: None,
            asset_id: Some(other),
            entity_id: Some(1),
            expires_at: wp.expires_at.clone(),
            expected_row_version: wp.row_version,
        },
        APPLIER,
    )
    .await
    .expect("equipment changed");
    let shown = safety_content::get_permit_safety_content(&db, wp.id)
        .await
        .expect("read");
    assert_eq!(shown.permit_status, "draft");
    assert_eq!(shown.hse_approved_at, None);
    assert!(shown.gas_tests.iter().all(|g| !g.counts));
    assert!(shown.issue_blockers.iter().any(|b| b.code == CODE_GAS_TEST_MISSING));
}

/// A permit issued on another PC on a reading older than the interval at its issue is a conflict.
#[tokio::test]
async fn a_permit_issued_elsewhere_on_an_out_of_date_gas_test_is_a_conflict() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-S2").await;
    let cs = confined_approved(&db, asset).await;
    raised_hours_ago(&db, &cs, 10).await;
    safety_content::record_permit_gas_test(&db, gas(&cs, 20.9, 300), APPLIER)
        .await
        .expect("old reading");
    let issued = with(
        wire(&db, &cs.entity_sync_id).await,
        &[("status", json!("issued")), ("issued_at", json!(now()))],
    );
    let outcome = apply(&db, &cs.entity_sync_id, &issued).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("PERMIT_ISSUE_GAS_TEST_EXPIRED"),
        "{outcome:?}"
    );
}

// ── Review of 1ed86d7e2: the gas gate's future bound, the order by instant, four-eyes ─────────────

/// A reading as received by sync (any stored form of its time), recorded by APPLIER.
async fn insert_reading(db: &DatabaseConnection, wp: &WorkPermit, o2: f64, measured_at: &str) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_gas_tests (entity_sync_id, permit_id, o2_percent, lel_percent, h2s_ppm, co_ppm, measured_at, measured_by_id, instrument, row_version, created_at, updated_at) VALUES (?, ?, ?, 0, 0, NULL, ?, ?, 'Détecteur reçu', 1, ?, ?)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            wp.id.into(),
            o2.into(),
            measured_at.into(),
            APPLIER.into(),
            now().into(),
            now().into(),
        ],
    ))
    .await
    .expect("received reading");
}

fn in_minutes(minutes: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::minutes(minutes))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// BLOCKING 2 of the review: a reading dated in the future (beyond the tolerated clock difference)
/// never counts locally: it is not the latest one (a failing one does not hide behind it, a good one
/// does not allow the issue) and it never has a negative age.
#[tokio::test]
async fn a_future_dated_reading_is_ignored_by_the_local_gate() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-F1").await;
    let wp = confined_approved(&db, asset).await;
    // Only a good reading dated three hours ahead: no reading counts.
    insert_reading(&db, &wp, 20.9, &in_minutes(180)).await;
    let err = issue(&db, &reload(&db, &wp).await).await.expect_err("future only");
    assert_eq!(codes(&err), vec![CODE_GAS_TEST_MISSING]);
    let shown = safety_content::get_permit_safety_content(&db, wp.id)
        .await
        .expect("read");
    assert!(
        shown.gas_tests.iter().all(|g| !g.counts),
        "the future reading does not count"
    );

    // A failing reading of now, and the future good one: the failing one is the latest that counts.
    insert_reading(&db, &wp, 17.5, &in_minutes(-1)).await;
    let err = issue(&db, &reload(&db, &wp).await).await.expect_err("low O2 now");
    assert_eq!(codes(&err), vec![CODE_GAS_O2_OUT_OF_LIMITS]);
    // A good reading of now: issued, the future one still ignored.
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 0), APPLIER)
        .await
        .expect("good reading");
    assert_eq!(
        issue(&db, &reload(&db, &wp).await).await.expect("issued").status,
        "issued"
    );
}

/// The latest reading is the latest instant, not the latest text: a reading stored with a +02:00
/// offset (written later as text) but taken earlier does not hide a newer failing one.
#[tokio::test]
async fn readings_are_ordered_by_instant_not_by_text() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-J1").await;
    let wp = confined_approved(&db, asset).await;
    let offset = chrono::FixedOffset::east_opt(2 * 3600).expect("offset");
    let earlier_good = (chrono::Utc::now() - chrono::Duration::minutes(20))
        .with_timezone(&offset)
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string();
    insert_reading(&db, &wp, 20.9, &earlier_good).await;
    insert_reading(&db, &wp, 17.5, &in_minutes(-5)).await;
    assert!(
        earlier_good.as_str() > in_minutes(-5).as_str(),
        "the text order differs"
    );
    let err = issue(&db, &reload(&db, &wp).await)
        .await
        .expect_err("latest is failing");
    assert_eq!(codes(&err), vec![CODE_GAS_O2_OUT_OF_LIMITS]);
    let shown = safety_content::get_permit_safety_content(&db, wp.id)
        .await
        .expect("read");
    assert!(
        (shown.gas_tests[0].o2_percent - 17.5).abs() < 1e-9,
        "latest first, by instant"
    );
}

/// SHOULD-FIX 3 of the review: four-eyes on the HSE approval is checked again at the issue (here the
/// approver became the permit's recorded author).
#[tokio::test]
async fn the_hse_four_eyes_is_checked_again_at_the_issue() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-F1").await;
    let wp = confined_approved(&db, asset).await;
    safety_content::record_permit_gas_test(&db, gas(&wp, 20.9, 1), APPLIER)
        .await
        .expect("reading");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET created_by_id = ? WHERE id = ?",
        [VERIFIER.into(), wp.id.into()],
    ))
    .await
    .expect("author");
    let err = issue(&db, &reload(&db, &wp).await).await.expect_err("self-approved");
    assert_eq!(codes(&err), vec![safety_content::CODE_HSE_SELF_APPROVAL]);
}

/// A permit received issued whose HSE approval was recorded by its author, not flagged a
/// self-approval: a conflict.
#[tokio::test]
async fn a_permit_issued_elsewhere_with_a_self_hse_approval_is_a_conflict() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L403-S3").await;
    let wp = approved(&db, write_content(&db, &create(&db, "cold_work", asset).await).await).await;
    let held = wire(&db, &wp.entity_sync_id).await;
    let author = held.get("created_by_sync_id").cloned().expect("author");
    let issued = with(
        held,
        &[
            ("status", json!("issued")),
            ("issued_at", json!(now())),
            ("hse_approved_by_sync_id", author),
            ("hse_approved_at", json!(now())),
            ("hse_self_approved", json!(0)),
        ],
    );
    let outcome = apply(&db, &wp.entity_sync_id, &issued).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("PERMIT_ISSUE_HSE_SELF_APPROVAL"),
        "{outcome:?}"
    );
    assert_eq!(reload(&db, &wp).await.status, "approved", "nothing written");
}

/// A permit received active whose latest counting reading was out of date when it was activated:
/// `PERMIT_ACTIVE_GAS_TEST_INVALID`.
#[tokio::test]
async fn a_permit_activated_elsewhere_on_an_out_of_date_gas_test_is_a_conflict() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L404-S3").await;
    // The activation is judged on its gas test only (no isolation asked of the type here).
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE permit_types SET requires_energy_isolation = 0 WHERE code = 'confined_space'".to_string(),
    ))
    .await
    .expect("type");
    let cs = confined_approved(&db, asset).await;
    safety_content::record_permit_gas_test(&db, gas(&cs, 20.9, 1), APPLIER)
        .await
        .expect("reading");
    let cs = issue(&db, &reload(&db, &cs).await).await.expect("issued");
    // That reading is now five hours old (the permit raised ten hours ago).
    raised_hours_ago(&db, &cs, 10).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE permit_gas_tests SET measured_at = ? WHERE permit_id = ?",
        [in_minutes(-300).into(), cs.id.into()],
    ))
    .await
    .expect("aged");
    let active = with(
        wire(&db, &cs.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    let outcome = apply(&db, &cs.entity_sync_id, &active).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("PERMIT_ACTIVE_GAS_TEST_INVALID"),
        "{outcome:?}"
    );
    assert_eq!(reload(&db, &cs).await.status, "issued", "nothing written");
}
