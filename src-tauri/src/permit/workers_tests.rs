//! L4-05 / L4-06 / L4-18: the workers of a permit (induction of external staff, qualifications of
//! the permit's type) and their personal padlocks on the group lock box — on this PC and on a
//! permit issued on another PC.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Map, Value};

use super::tests::{
    advance_to_issued, create_permit, insert_equipment, scalar_i64, set_status, setup, APPLIER, VERIFIER,
};
use super::workers::{
    self, PermitWorkerAddInput, PermitWorkerInductionInput, PermitWorkerWithdrawInput, PersonalLockForceRemoveInput,
    PersonalLockPlaceInput, PersonalLockRemoveInput, CODE_INDUCTION_MISSING, CODE_LOCK_NOT_A_WORKER,
    CODE_LOCK_NOT_YOURS, CODE_PERSONAL_LOCKS_ON, CODE_QUALIFICATION_EXPIRED, CODE_QUALIFICATION_MISSING,
    CODE_QUALIFICATION_UNVERIFIABLE,
};
use crate::errors::AppError;
use crate::permit::domain::WorkPermit;
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

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect(sql);
}

async fn person(db: &DatabaseConnection, code: &str, name: &str, origin: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO personnel (employee_code, full_name, availability_status, employment_origin) VALUES ('{code}', '{name}', 'available', '{origin}')"
        ),
    )
    .await;
    scalar_i64(db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'")).await
}

async fn company(db: &DatabaseConnection, name: &str) -> i64 {
    exec(
        db,
        &format!("INSERT INTO external_companies (entity_sync_id, name, is_active) VALUES ('co-{name}', '{name}', 1)"),
    )
    .await;
    scalar_i64(db, &format!("SELECT id FROM external_companies WHERE name = '{name}'")).await
}

fn day(offset_days: i64) -> String {
    (chrono::Utc::now().date_naive() + chrono::Duration::days(offset_days))
        .format("%Y-%m-%d")
        .to_string()
}

/// The permit type `type_code` requires the certification "B1V" of every worker.
async fn require_b1v(db: &DatabaseConnection, type_code: &str) -> i64 {
    exec(
        db,
        "INSERT INTO certification_types (entity_sync_id, code, name, default_validity_months, renewal_lead_days, category, row_version) VALUES ('ct-b1v', 'HAB-B1V', 'Habilitation B1V', 36, 60, 'electrical', 1)",
    )
    .await;
    let ct = scalar_i64(db, "SELECT id FROM certification_types WHERE code = 'HAB-B1V'").await;
    exec(
        db,
        &format!(
            "INSERT INTO qualification_requirements (entity_sync_id, target_kind, permit_type_id, certification_type_id, coverage, is_active, row_version, created_at, updated_at) VALUES ('qr-b1v', 'permit_type', (SELECT id FROM permit_types WHERE code = '{type_code}'), {ct}, 'each', 1, 1, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    ct
}

async fn give_cert(db: &DatabaseConnection, personnel_id: i64, ct: i64, expires: &str) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_certifications (entity_sync_id, personnel_id, certification_type_id, issued_at, expires_at, verification_status, row_version) VALUES (?, ?, ?, ?, ?, 'verified', 1)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            personnel_id.into(),
            ct.into(),
            day(-400).into(),
            expires.into(),
        ],
    ))
    .await
    .expect("certification");
}

fn internal(permit_id: i64, personnel_id: i64) -> PermitWorkerAddInput {
    PermitWorkerAddInput {
        permit_id,
        personnel_id: Some(personnel_id),
        external_name: None,
        external_company_id: None,
        role_on_permit: "Électricien".into(),
        induction_at: None,
    }
}

fn external(permit_id: i64, company_id: i64, induction_at: Option<String>) -> PermitWorkerAddInput {
    PermitWorkerAddInput {
        permit_id,
        personnel_id: None,
        external_name: Some("  Yacine Prestataire ".into()),
        external_company_id: Some(company_id),
        role_on_permit: "Soudeur".into(),
        induction_at,
    }
}

async fn issue(db: &DatabaseConnection, wp: &WorkPermit) -> Result<WorkPermit, AppError> {
    set_status(db, &reload(db, wp).await, "issued").await
}

/// L4-05: an external worker without the safety induction refuses the issue; once inducted (by the
/// session user), the permit is issued. On a permit already issued, an external worker is added
/// only with the induction.
#[tokio::test]
async fn an_external_worker_without_induction_blocks_the_issue() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L405-1").await;
    let co = company(&db, "Soudure Services").await;
    let wp = create_permit(&db, "cold_work", asset).await;
    let view = workers::add_permit_worker(&db, external(wp.id, co, None), APPLIER)
        .await
        .expect("added before the issue without induction");
    let w = &view.workers[0];
    assert!(w.is_external);
    assert_eq!(w.external_name.as_deref(), Some("Yacine Prestataire"));
    assert_eq!(w.company_name.as_deref(), Some("Soudure Services"));
    assert_eq!(
        w.blockers.iter().map(|b| b.code.as_str()).collect::<Vec<_>>(),
        vec![CODE_INDUCTION_MISSING]
    );
    let wp = set_status(&db, &wp, "pending_review").await.expect("submit");
    let wp = set_status(&db, &wp, "approved").await.expect("approve");
    let err = issue(&db, &wp).await.expect_err("not inducted");
    assert_eq!(codes(&err), vec![CODE_INDUCTION_MISSING]);
    assert_eq!(reload(&db, &wp).await.status, "approved", "nothing written");

    let view = workers::record_permit_worker_induction(
        &db,
        PermitWorkerInductionInput {
            worker_id: w.id,
            expected_row_version: w.row_version,
            induction_at: None,
        },
        VERIFIER,
    )
    .await
    .expect("inducted");
    assert_eq!(view.workers[0].induction_by_id, Some(VERIFIER));
    let wp = issue(&db, &wp).await.expect("issued");
    assert_eq!(wp.status, "issued");

    // Issued: an external worker comes with their induction.
    let err = workers::add_permit_worker(&db, external(wp.id, co, None), APPLIER)
        .await
        .expect_err("no induction after the issue");
    assert_eq!(codes(&err), vec![CODE_INDUCTION_MISSING]);
    let at = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
    workers::add_permit_worker(&db, external(wp.id, co, Some(at)), APPLIER)
        .await
        .expect("inducted on arrival");
}

/// L4-06: a worker whose authorisation the permit's type requires is refused when it expired or is
/// missing, when added and at the issue (it may expire in between); an external worker without a
/// personnel record cannot be checked.
#[tokio::test]
async fn a_worker_with_an_expired_or_missing_authorisation_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L406-1").await;
    let ct = require_b1v(&db, "cold_work").await;
    let expired = person(&db, "W-1", "Karim Électricien", "internal").await;
    give_cert(&db, expired, ct, &day(-1)).await;
    let missing = person(&db, "W-2", "Lina Mécanicienne", "internal").await;
    let valid = person(&db, "W-3", "Nadia Électricienne", "internal").await;
    give_cert(&db, valid, ct, &day(365)).await;
    let co = company(&db, "Elec Plus").await;
    let wp = create_permit(&db, "cold_work", asset).await;

    let err = workers::add_permit_worker(&db, internal(wp.id, expired), APPLIER)
        .await
        .expect_err("expired");
    assert_eq!(codes(&err), vec![CODE_QUALIFICATION_EXPIRED]);
    let err = workers::add_permit_worker(&db, internal(wp.id, missing), APPLIER)
        .await
        .expect_err("missing");
    assert_eq!(codes(&err), vec![CODE_QUALIFICATION_MISSING]);
    let err = workers::add_permit_worker(&db, external(wp.id, co, None), APPLIER)
        .await
        .expect_err("cannot be checked");
    assert_eq!(codes(&err), vec![CODE_QUALIFICATION_UNVERIFIABLE]);
    let view = workers::add_permit_worker(&db, internal(wp.id, valid), APPLIER)
        .await
        .expect("valid");
    assert_eq!(view.workers.len(), 1, "the refused workers were not kept");
    let err = workers::add_permit_worker(&db, internal(wp.id, valid), APPLIER)
        .await
        .expect_err("listed twice");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_WORKER_ALREADY_LISTED"]);

    // The authorisation runs out before the issue: refused, naming the worker.
    exec(
        &db,
        &format!(
            "UPDATE personnel_certifications SET expires_at = '{}' WHERE personnel_id = {valid}",
            day(-1)
        ),
    )
    .await;
    let wp = set_status(&db, &wp, "pending_review").await.expect("submit");
    let wp = set_status(&db, &wp, "approved").await.expect("approve");
    let err = issue(&db, &wp).await.expect_err("expired since");
    assert_eq!(codes(&err), vec![CODE_QUALIFICATION_EXPIRED]);
    match err {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(
                issues[0].params.get("worker").map(String::as_str),
                Some("Nadia Électricienne")
            );
            assert_eq!(
                issues[0].params.get("qualification").map(String::as_str),
                Some("Habilitation B1V")
            );
        }
        other => panic!("{other:?}"),
    }
}

/// L4-18: a worker places and removes only their own padlock, on a permit they are listed on; no
/// isolation lock comes off and the permit is not closed while a padlock is on; a `ptw.close`
/// holder removes an absent worker's padlock by force, never their own.
#[tokio::test]
async fn a_worker_places_and_removes_only_their_own_padlock() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L418-1").await;
    let worker = person(&db, "W-L", "Omar Mécanicien", "internal").await;
    // VERIFIER's login is Omar's card.
    exec(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = {worker} WHERE id = {VERIFIER}"),
    )
    .await;
    let wp = advance_to_issued(&db, create_permit(&db, "cold_work", asset).await).await;

    // Not listed yet: refused.
    let place = |by: i64| {
        let db = db.clone();
        let permit_id = wp.id;
        async move {
            workers::place_my_personal_lock(
                &db,
                PersonalLockPlaceInput {
                    permit_id,
                    lock_number: " C-12 ".into(),
                },
                by,
            )
            .await
        }
    };
    assert_eq!(
        codes(&place(VERIFIER).await.expect_err("not listed")),
        vec![CODE_LOCK_NOT_A_WORKER]
    );
    workers::add_permit_worker(&db, internal(wp.id, worker), APPLIER)
        .await
        .expect("listed");
    // The admin is no worker of the permit: refused.
    assert_eq!(
        codes(&place(APPLIER).await.expect_err("admin")),
        vec![CODE_LOCK_NOT_A_WORKER]
    );
    let view = place(VERIFIER).await.expect("own padlock");
    let lock = view.workers[0].open_lock.clone().expect("on");
    assert_eq!(lock.lock_number, "C-12");
    assert_eq!(view.open_lock_count, 1);
    assert_eq!(
        codes(&place(VERIFIER).await.expect_err("twice")),
        vec!["GATE_PERMIT_PERSONAL_LOCK_ALREADY_ON"]
    );

    // The group lockout rule.
    let err = workers::ensure_no_personal_lock_on(&db, wp.id)
        .await
        .expect_err("padlock on");
    assert_eq!(codes(&err), vec![CODE_PERSONAL_LOCKS_ON]);
    let w = &view.workers[0];
    let err = workers::withdraw_permit_worker(
        &db,
        PermitWorkerWithdrawInput {
            worker_id: w.id,
            expected_row_version: w.row_version,
        },
        APPLIER,
    )
    .await
    .expect_err("padlock on");
    assert_eq!(codes(&err), vec!["GATE_PERMIT_WORKER_LOCK_ON"]);

    // Somebody else's padlock: refused.
    let remove = |by: i64, lock: workers::PermitPersonalLock| {
        let db = db.clone();
        async move {
            workers::remove_my_personal_lock(
                &db,
                PersonalLockRemoveInput {
                    lock_id: lock.id,
                    expected_row_version: lock.row_version,
                },
                by,
            )
            .await
        }
    };
    assert_eq!(
        codes(&remove(APPLIER, lock.clone()).await.expect_err("not yours")),
        vec![CODE_LOCK_NOT_YOURS]
    );
    let view = remove(VERIFIER, lock.clone()).await.expect("own padlock off");
    assert_eq!(view.open_lock_count, 0);
    assert_eq!(view.locks[0].removed_by_id, Some(VERIFIER));
    workers::ensure_no_personal_lock_on(&db, wp.id).await.expect("box free");

    // Forced removal of an absent worker's padlock: never by its owner, with a reason.
    let view = place(VERIFIER).await.expect("on again");
    let lock = view.workers[0].open_lock.clone().expect("on");
    let force = |by: i64, reason: &str| {
        let db = db.clone();
        let lock = lock.clone();
        let reason = reason.to_string();
        async move {
            workers::force_remove_personal_lock(
                &db,
                PersonalLockForceRemoveInput {
                    lock_id: lock.id,
                    expected_row_version: lock.row_version,
                    reason,
                    worker_absent_attested: true,
                },
                by,
            )
            .await
        }
    };
    let long = "Intervenant parti sans retirer son cadenas, injoignable par téléphone";
    assert_eq!(
        codes(&force(VERIFIER, long).await.expect_err("own")),
        vec!["GATE_PERMIT_PERSONAL_LOCK_FORCE_OWN"]
    );
    assert_eq!(
        codes(&force(APPLIER, "absent").await.expect_err("short")),
        vec!["GATE_PERMIT_PERSONAL_LOCK_FORCE_REASON_TOO_SHORT"]
    );
    let view = force(APPLIER, long).await.expect("forced");
    assert!(view
        .locks
        .iter()
        .any(|l| l.forced_removal && l.removed_by_id == Some(APPLIER)));
    assert_eq!(view.open_lock_count, 0);
}

// ── A permit issued on another PC ────────────────────────────────────────────────

type Payload = Map<String, Value>;

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

/// A permit held here approved, with an external worker not inducted here, received issued: the
/// issue is judged on its workers too.
#[tokio::test]
async fn a_permit_issued_elsewhere_with_a_worker_not_inducted_is_a_conflict() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-L405-S1").await;
    let co = company(&db, "Levage SARL").await;
    let wp = create_permit(&db, "cold_work", asset).await;
    workers::add_permit_worker(&db, external(wp.id, co, None), APPLIER)
        .await
        .expect("added");
    let wp = set_status(&db, &wp, "pending_review").await.expect("submit");
    let wp = set_status(&db, &wp, "approved").await.expect("approve");
    crate::permit::safety_content::fill_issue_requirements_for_tests(&db, wp.id).await;
    let mut issued = super::sync_check::local_wire(&db, "work_permits", &wp.entity_sync_id)
        .await
        .expect("payload")
        .expect("row");
    issued.insert("status".into(), json!("issued"));
    issued.insert(
        "issued_at".into(),
        json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    );
    let outcome = apply(&db, &wp.entity_sync_id, &issued).await;
    assert!(
        matches!(&outcome, ApplyOutcome::Conflict { code, .. } if code == "PERMIT_ISSUE_WORKER_INDUCTION_MISSING"),
        "{outcome:?}"
    );
    assert_eq!(reload(&db, &wp).await.status, "approved", "nothing written");
}
