//! L4-05 / L4-06 / L4-18: the workers of a permit (induction of external staff, qualifications of
//! the permit's type) and their personal padlocks on the group lock box — on this PC and on a
//! permit issued on another PC.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Map, Value};

use super::tests::{
    action, add_isolation, advance_to_issued, apply_and_verify, create_permit, insert_equipment, scalar_i64,
    set_status, setup, user_with, APPLIER, VERIFIER,
};
use super::workers::{
    self, PermitWorkerAddInput, PermitWorkerInductionInput, PermitWorkerWithdrawInput, PersonalLockForceRemoveInput,
    PersonalLockPlaceInput, PersonalLockRemoveInput, CODE_INDUCTION_MISSING, CODE_LOCK_NOT_A_WORKER,
    CODE_LOCK_NOT_YOURS, CODE_PERSONAL_LOCKS_ON, CODE_QUALIFICATION_EXPIRED, CODE_QUALIFICATION_MISSING,
    CODE_QUALIFICATION_UNVERIFIABLE,
};
use crate::errors::AppError;
use crate::permit::domain::{ContactAttempt, PermitForcedRemovalInput, PermitIsolation, WorkPermit};
use crate::permit::queries;
use crate::rbac::permissions::PTW_CLOSE;
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

/// A personnel record with its sync identity, as the personnel commands create one (a worker line
/// naming a record without one cannot be written on the wire).
async fn person(db: &DatabaseConnection, code: &str, name: &str, origin: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO personnel (entity_sync_id, employee_code, full_name, availability_status, employment_origin) VALUES ('per-{code}', '{code}', '{name}', 'available', '{origin}')"
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

/// The first method of the plant's contact list (seeded "à revoir" by migration 249).
async fn contact_attempt(db: &DatabaseConnection) -> ContactAttempt {
    ContactAttempt {
        at: (chrono::Utc::now() - chrono::Duration::minutes(30)).to_rfc3339(),
        method: crate::permit::contact_methods::list_contact_methods(db)
            .await
            .expect("methods")
            .first()
            .expect("a starter method")
            .code
            .clone(),
        result: "Pas de réponse, messagerie".into(),
    }
}

async fn place(db: &DatabaseConnection, permit_id: i64, by: i64) -> Result<workers::PermitWorkersView, AppError> {
    workers::place_my_personal_lock(
        db,
        PersonalLockPlaceInput {
            permit_id,
            lock_number: " C-12 ".into(),
        },
        by,
    )
    .await
}

async fn remove(
    db: &DatabaseConnection,
    lock: &workers::PermitPersonalLock,
    by: i64,
) -> Result<workers::PermitWorkersView, AppError> {
    workers::remove_my_personal_lock(
        db,
        PersonalLockRemoveInput {
            lock_id: lock.id,
            expected_row_version: lock.row_version,
        },
        by,
    )
    .await
}

/// An issued cold-work permit with Omar (VERIFIER's card) listed as its worker and one isolation
/// added, not applied yet.
async fn permit_with_worker(db: &DatabaseConnection, asset_code: &str) -> (WorkPermit, PermitIsolation) {
    let asset = insert_equipment(db, asset_code).await;
    let worker = person(db, &format!("W-{asset_code}"), "Omar Mécanicien", "internal").await;
    // VERIFIER's login is Omar's card.
    exec(
        db,
        &format!("UPDATE user_accounts SET personnel_id = {worker} WHERE id = {VERIFIER}"),
    )
    .await;
    let wp = advance_to_issued(db, create_permit(db, "cold_work", asset).await).await;
    workers::add_permit_worker(db, internal(wp.id, worker), APPLIER)
        .await
        .expect("listed");
    let iso = add_isolation(db, wp.id, &format!("{asset_code} breaker")).await;
    (wp, iso)
}

/// The same permit, its isolation fitted and verified, activated, Omar's padlock on the box.
async fn padlock_on(
    db: &DatabaseConnection,
    asset_code: &str,
) -> (WorkPermit, PermitIsolation, workers::PermitPersonalLock) {
    let (wp, iso) = permit_with_worker(db, asset_code).await;
    let iso = apply_and_verify(db, &iso).await;
    let wp = set_status(db, &reload(db, &wp).await, "active").await.expect("active");
    let view = place(db, wp.id, VERIFIER).await.expect("own padlock");
    let lock = view.workers[0].open_lock.clone().expect("on");
    (wp, iso, lock)
}

/// L4-18 (review 6): a padlock is placed only when the group lock box protects something: every
/// counted isolation verified and none taken off.
#[tokio::test]
async fn a_padlock_is_placed_only_when_the_box_protects_something() {
    let db = setup().await;
    let (wp, iso) = permit_with_worker(&db, "EQ-L418-P").await;
    assert_eq!(
        codes(&place(&db, wp.id, VERIFIER).await.expect_err("not applied")),
        vec![workers::CODE_LOCK_NOTHING_PROTECTED]
    );
    let applied = queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("apply");
    assert_eq!(
        codes(&place(&db, wp.id, VERIFIER).await.expect_err("not verified")),
        vec![workers::CODE_LOCK_NOTHING_PROTECTED]
    );
    queries::verify_permit_isolation(&db, action(&applied), VERIFIER)
        .await
        .expect("verify");
    place(&db, wp.id, VERIFIER).await.expect("the box protects the work");
}

/// L4-18: a worker places and removes only their own padlock, on a permit they are listed on; no
/// isolation lock comes off and the permit is not closed while a padlock is on (the real paths:
/// "J'ai retiré mon cadenas", the forced removal of an isolation lock, the close).
#[tokio::test]
async fn a_worker_places_and_removes_only_their_own_padlock() {
    let db = setup().await;
    let (wp, _) = permit_with_worker(&db, "EQ-L418-1").await;
    // The admin is no worker of the permit: refused.
    assert_eq!(
        codes(&place(&db, wp.id, APPLIER).await.expect_err("admin")),
        vec![CODE_LOCK_NOT_A_WORKER]
    );
    let (wp, iso, lock) = padlock_on(&db, "EQ-L418-2").await;
    assert_eq!(lock.lock_number, "C-12");
    assert_eq!(
        codes(&place(&db, wp.id, VERIFIER).await.expect_err("twice")),
        vec!["GATE_PERMIT_PERSONAL_LOCK_ALREADY_ON"]
    );

    // The group lockout, on the real paths.
    let err = queries::record_permit_isolation_removed(&db, action(&iso), APPLIER)
        .await
        .expect_err("padlock on: the isolation lock stays");
    assert_eq!(codes(&err), vec![CODE_PERSONAL_LOCKS_ON]);
    let authoriser = user_with(&db, "l418-chef", &[PTW_CLOSE]).await;
    let remover = user_with(&db, "l418-remover", &[]).await;
    let err = queries::force_permit_isolation_removal(
        &db,
        PermitForcedRemovalInput {
            isolation_id: iso.id,
            expected_row_version: iso.row_version,
            removed_by_id: remover,
            reason: "Fin de poste, travailleur parti sans retirer son cadenas".into(),
            worker_absent_attested: true,
            contact_attempts: vec![contact_attempt(&db).await],
        },
        authoriser,
    )
    .await
    .expect_err("padlock on: no forced removal of the isolation lock either");
    assert_eq!(codes(&err), vec![CODE_PERSONAL_LOCKS_ON]);
    let err = set_status(&db, &reload(&db, &wp).await, "closed")
        .await
        .expect_err("padlock on: not closed");
    assert_eq!(codes(&err), vec![CODE_PERSONAL_LOCKS_ON]);
    assert_eq!(reload(&db, &wp).await.status, "active");

    let view = workers::list_permit_workers(&db, wp.id, VERIFIER).await.expect("view");
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

    // Somebody else's padlock: refused; the owner's: off.
    assert_eq!(
        codes(&remove(&db, &lock, APPLIER).await.expect_err("not yours")),
        vec![CODE_LOCK_NOT_YOURS]
    );
    let view = remove(&db, &lock, VERIFIER).await.expect("own padlock off");
    assert_eq!(view.open_lock_count, 0);
    assert_eq!(view.locks[0].removed_by_id, Some(VERIFIER));
    workers::ensure_no_personal_lock_on(&db, wp.id).await.expect("box free");
}

/// L4-18 (review 5): an absent worker's padlock comes off by the isolation locks' forced-removal
/// process: authorised by a close-right holder, taken off by a named person, with the reason, the
/// attestation and the contact attempts; the worker is told before placing a padlock again.
#[tokio::test]
async fn an_absent_workers_padlock_comes_off_by_the_forced_removal_process() {
    let db = setup().await;
    let (wp, _, lock) = padlock_on(&db, "EQ-L418-F").await;
    let authoriser = user_with(&db, "l418f-chef", &[PTW_CLOSE]).await;
    let remover = user_with(&db, "l418f-remover", &[]).await;
    let attempt = contact_attempt(&db).await;
    let long = "Intervenant parti sans retirer son cadenas, injoignable par téléphone";
    let input = |removed_by_id: i64, reason: &str, attempts: Vec<ContactAttempt>| PersonalLockForceRemoveInput {
        lock_id: lock.id,
        expected_row_version: lock.row_version,
        removed_by_id,
        reason: reason.to_string(),
        worker_absent_attested: true,
        contact_attempts: attempts,
    };
    let refused = |input: PersonalLockForceRemoveInput, by: i64| {
        let db = db.clone();
        async move {
            codes(
                &workers::force_remove_personal_lock(&db, input, by)
                    .await
                    .expect_err("refused"),
            )
        }
    };
    assert_eq!(
        refused(input(remover, long, vec![attempt.clone()]), VERIFIER).await,
        vec!["GATE_PERMIT_PERSONAL_LOCK_FORCE_OWN"]
    );
    assert_eq!(
        refused(input(VERIFIER, long, vec![attempt.clone()]), authoriser).await,
        vec![workers::CODE_FORCE_REMOVER_IS_OWNER]
    );
    assert_eq!(
        refused(input(authoriser, long, vec![attempt.clone()]), authoriser).await,
        vec![workers::CODE_FORCE_AUTHORISER_IS_REMOVER]
    );
    assert_eq!(
        refused(input(remover, "absent", vec![attempt.clone()]), authoriser).await,
        vec!["GATE_PERMIT_PERSONAL_LOCK_FORCE_REASON_TOO_SHORT"]
    );
    assert_eq!(
        refused(input(remover, long, vec![]), authoriser).await,
        vec![queries::CODE_FORCED_ATTEMPTS_REQUIRED]
    );
    let view = workers::force_remove_personal_lock(&db, input(remover, long, vec![attempt]), authoriser)
        .await
        .expect("forced");
    let off = view.locks.iter().find(|l| l.id == lock.id).expect("kept").clone();
    assert!(off.forced_removal && off.forced_worker_absent);
    assert_eq!(off.removed_by_id, Some(remover));
    assert_eq!(off.forced_authorized_by_id, Some(authoriser));
    assert_eq!(off.forced_contact_attempts.len(), 1);
    assert_eq!(view.open_lock_count, 0);

    // Not told yet: no new padlock; the owner acknowledges (nobody else does it for them).
    assert_eq!(
        codes(&place(&db, wp.id, VERIFIER).await.expect_err("not told")),
        vec![queries::CODE_WORKER_NOT_INFORMED]
    );
    assert_eq!(
        codes(
            &workers::acknowledge_forced_padlock_removal(&db, lock.id, APPLIER)
                .await
                .expect_err("not the owner")
        ),
        vec![CODE_LOCK_NOT_YOURS]
    );
    let view = workers::acknowledge_forced_padlock_removal(&db, lock.id, VERIFIER)
        .await
        .expect("told");
    assert!(view.locks.iter().any(|l| l.id == lock.id && l.informed_in_app));
    place(&db, wp.id, VERIFIER).await.expect("placed again once told");
}

/// L4-18 (review 7): a permit closed while a padlock was still on (received by sync) keeps no padlock
/// stuck: its owner removes it.
#[tokio::test]
async fn a_padlock_left_on_a_closed_permit_is_removed_by_its_owner() {
    let db = setup().await;
    let (wp, _, lock) = padlock_on(&db, "EQ-L418-C").await;
    exec(
        &db,
        &format!("UPDATE work_permits SET status = 'closed' WHERE id = {}", wp.id),
    )
    .await;
    let view = remove(&db, &lock, VERIFIER).await.expect("removed on a closed permit");
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

// ── The group lockout on rows received by sync (review blocking 1) ───────────────────────────────

async fn apply_entity(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
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

async fn user_sync_id(db: &DatabaseConnection, user_id: i64) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        [user_id.into()],
    ))
    .await
    .expect("read")
    .expect("user")
    .try_get::<String>("", "entity_sync_id")
    .expect("sync id")
}

async fn wire_of(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    super::sync_check::local_wire(db, entity_type, sync_id)
        .await
        .expect("payload")
        .expect("row")
}

fn stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn conflict_of(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

/// A padlock received as taken off by someone other than its owner, with no forced-removal record,
/// is a conflict; so is a padlock removal record held here then rewritten.
#[tokio::test]
async fn a_received_padlock_removal_by_a_non_owner_is_a_conflict() {
    let db = setup().await;
    let (_, _, lock) = padlock_on(&db, "EQ-L418-S1").await;
    let mut removed = wire_of(&db, "permit_personal_locks", &lock.entity_sync_id).await;
    removed.insert("removed_at".into(), json!(stamp()));
    removed.insert("removed_by_sync_id".into(), json!(user_sync_id(&db, APPLIER).await));
    let outcome = apply_entity(&db, "permit_personal_locks", &lock.entity_sync_id, &removed).await;
    assert_eq!(
        conflict_of(&outcome),
        Some("PERMIT_LOCK_REMOVAL_NOT_APPLIER"),
        "{outcome:?}"
    );
    let open = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM permit_personal_locks WHERE id = {} AND removed_at IS NULL",
            lock.id
        ),
    )
    .await;
    assert_eq!(open, 1, "the padlock stays on here");
    // By its owner: applied.
    removed.insert("removed_by_sync_id".into(), json!(user_sync_id(&db, VERIFIER).await));
    let outcome = apply_entity(&db, "permit_personal_locks", &lock.entity_sync_id, &removed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // Once held here, the removal is never rewritten.
    let mut rewritten = wire_of(&db, "permit_personal_locks", &lock.entity_sync_id).await;
    rewritten.insert("removed_at".into(), json!(null));
    rewritten.insert("removed_by_sync_id".into(), json!(null));
    let outcome = apply_entity(&db, "permit_personal_locks", &lock.entity_sync_id, &rewritten).await;
    assert_eq!(
        conflict_of(&outcome),
        Some(super::workers_sync::PERSONAL_LOCK_RECORD_CHANGED),
        "{outcome:?}"
    );
}

/// A permit received closed, an isolation lock received taken off, or a worker received withdrawn,
/// while a personal padlock is still on the box here: conflicts (the group lockout).
#[tokio::test]
async fn a_received_close_while_a_padlock_is_on_is_a_conflict() {
    let db = setup().await;
    let (wp, iso, lock) = padlock_on(&db, "EQ-L418-S2").await;
    let closed = with_keys(
        wire_of(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(stamp()))],
    );
    let outcome = apply_entity(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert_eq!(
        conflict_of(&outcome),
        Some(super::workers_sync::CLOSED_PERSONAL_LOCKS_ON),
        "{outcome:?}"
    );
    assert_eq!(reload(&db, &wp).await.status, "active", "nothing written");

    let taken_off = with_keys(
        wire_of(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[
            ("removed_at", json!(stamp())),
            ("removed_by_sync_id", json!(user_sync_id(&db, APPLIER).await)),
        ],
    );
    let outcome = apply_entity(&db, "permit_isolations", &iso.entity_sync_id, &taken_off).await;
    assert_eq!(
        conflict_of(&outcome),
        Some(super::workers_sync::ISOLATION_PERSONAL_LOCKS_ON),
        "{outcome:?}"
    );

    let worker_sync = scalar_text(
        &db,
        &format!(
            "SELECT pw.entity_sync_id AS v FROM permit_workers pw WHERE pw.id = {}",
            lock.worker_id
        ),
    )
    .await;
    let withdrawn = with_keys(
        wire_of(&db, "permit_workers", &worker_sync).await,
        &[
            ("withdrawn_at", json!(stamp())),
            ("withdrawn_by_sync_id", json!(user_sync_id(&db, APPLIER).await)),
        ],
    );
    let outcome = apply_entity(&db, "permit_workers", &worker_sync, &withdrawn).await;
    assert_eq!(
        conflict_of(&outcome),
        Some(super::workers_sync::WORKER_WITHDRAWN_LOCK_ON),
        "{outcome:?}"
    );
}

fn with_keys(mut p: Payload, changes: &[(&str, Value)]) -> Payload {
    for (k, v) in changes {
        p.insert((*k).to_string(), v.clone());
    }
    p
}

async fn scalar_text(db: &DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect(sql)
        .expect("row")
        .try_get::<String>("", "v")
        .expect("text")
}

// ── Review of 976237463: deletes, received placements, forced removals of padlocks ───────────────

async fn delete_entity(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let payload = crate::sync::row_codec::delete_payload(&layout, None);
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        sync_id,
        "delete",
        &serde_json::to_string(&payload).expect("json"),
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

async fn worker_sync_of(db: &DatabaseConnection, lock: &workers::PermitPersonalLock) -> String {
    scalar_text(
        db,
        &format!(
            "SELECT entity_sync_id AS v FROM permit_workers WHERE id = {}",
            lock.worker_id
        ),
    )
    .await
}

/// Omar's padlock taken off by the forced-removal process; Omar not told yet.
async fn force_off(db: &DatabaseConnection, lock: &workers::PermitPersonalLock, tag: &str) {
    let authoriser = user_with(db, &format!("{tag}-chef"), &[PTW_CLOSE]).await;
    let remover = user_with(db, &format!("{tag}-remover"), &[]).await;
    workers::force_remove_personal_lock(
        db,
        PersonalLockForceRemoveInput {
            lock_id: lock.id,
            expected_row_version: lock.row_version,
            removed_by_id: remover,
            reason: "Intervenant parti sans retirer son cadenas, injoignable par téléphone".into(),
            worker_absent_attested: true,
            contact_attempts: vec![contact_attempt(db).await],
        },
        authoriser,
    )
    .await
    .expect("forced");
}

/// A delete received by sync never takes a personal padlock off the box nor a worker off the permit:
/// both are refused (`PERMIT_DELETE_REFUSED`) and the rows stay, the group lockout still on.
#[tokio::test]
async fn a_received_delete_of_a_padlock_or_a_worker_is_refused() {
    let db = setup().await;
    let (_, _, lock) = padlock_on(&db, "EQ-L4R-D").await;
    let worker_sync = worker_sync_of(&db, &lock).await;
    for (entity_type, sync_id) in [
        ("permit_personal_locks", lock.entity_sync_id.clone()),
        ("permit_workers", worker_sync.clone()),
    ] {
        let outcome = delete_entity(&db, entity_type, &sync_id).await;
        assert_eq!(
            conflict_of(&outcome),
            Some("PERMIT_DELETE_REFUSED"),
            "{entity_type}: {outcome:?}"
        );
        let kept = scalar_i64(
            &db,
            &format!("SELECT COUNT(*) FROM \"{entity_type}\" WHERE entity_sync_id = '{sync_id}'"),
        )
        .await;
        assert_eq!(kept, 1, "{entity_type}: the row stays");
    }
    let open = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM permit_personal_locks WHERE id = {} AND removed_at IS NULL",
            lock.id
        ),
    )
    .await;
    assert_eq!(open, 1, "the padlock stays on the box");
}

/// A padlock received still on, new here, is accepted whatever its worker line, its group lock box
/// or its permit's status here: it only adds protection, and the group lockout here then counts it.
/// A padlock received already taken off is still placed on a permit issued here.
#[tokio::test]
async fn a_received_padlock_placement_is_judged_as_the_local_one() {
    let db = setup().await;
    let (wp, iso, lock) = padlock_on(&db, "EQ-L4R-P").await;
    let received = wire_of(&db, "permit_personal_locks", &lock.entity_sync_id).await;
    let not_here = format!(
        "DELETE FROM permit_personal_locks WHERE entity_sync_id = '{}'",
        lock.entity_sync_id
    );
    let held_sql = format!(
        "SELECT COUNT(*) FROM permit_personal_locks WHERE entity_sync_id = '{}' AND removed_at IS NULL",
        lock.entity_sync_id
    );
    // Not held here yet: the padlock arrives from another PC.
    exec(&db, &not_here).await;
    let verified_at = scalar_text(
        &db,
        &format!("SELECT verified_at AS v FROM permit_isolations WHERE id = {}", iso.id),
    )
    .await;
    let approved = format!("UPDATE work_permits SET status = 'approved' WHERE id = {}", wp.id);
    let active = format!("UPDATE work_permits SET status = 'active' WHERE id = {}", wp.id);
    let cases: [(String, String); 4] = [
        (approved.clone(), active.clone()),
        (
            format!(
                "UPDATE permit_workers SET withdrawn_at = '{}', withdrawn_by_id = {APPLIER} WHERE id = {}",
                stamp(),
                lock.worker_id
            ),
            format!(
                "UPDATE permit_workers SET withdrawn_at = NULL, withdrawn_by_id = NULL WHERE id = {}",
                lock.worker_id
            ),
        ),
        (
            format!("UPDATE permit_isolations SET verified_at = NULL WHERE id = {}", iso.id),
            format!(
                "UPDATE permit_isolations SET verified_at = '{verified_at}' WHERE id = {}",
                iso.id
            ),
        ),
        (
            format!(
                "UPDATE permit_isolations SET removed_at = '{}' WHERE id = {}",
                stamp(),
                iso.id
            ),
            format!("UPDATE permit_isolations SET removed_at = NULL WHERE id = {}", iso.id),
        ),
    ];
    for (break_it, restore) in &cases {
        exec(&db, break_it).await;
        let outcome = apply_entity(&db, "permit_personal_locks", &lock.entity_sync_id, &received).await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{break_it}: {outcome:?}");
        assert_eq!(
            scalar_i64(&db, &held_sql).await,
            1,
            "{break_it}: the padlock is on the box here"
        );
        // The group lockout here counts it: no isolation lock off, no close while it is on.
        let err = workers::ensure_no_personal_lock_on(&db, wp.id)
            .await
            .expect_err("a padlock on the box");
        assert_eq!(codes(&err), vec![CODE_PERSONAL_LOCKS_ON], "{break_it}");
        exec(&db, &not_here).await;
        exec(&db, restore).await;
    }
    // Received already taken off (by its owner) on a permit not issued here: refused.
    let placer = received.get("placed_by_sync_id").cloned().expect("placer");
    let taken_off = with_keys(
        received.clone(),
        &[("removed_at", json!(stamp())), ("removed_by_sync_id", placer)],
    );
    exec(&db, &approved).await;
    let outcome = apply_entity(&db, "permit_personal_locks", &lock.entity_sync_id, &taken_off).await;
    assert_eq!(
        conflict_of(&outcome),
        Some(super::workers_sync::PERSONAL_LOCK_PERMIT_NOT_ISSUED),
        "{outcome:?}"
    );
    exec(&db, &active).await;
    let written = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM permit_personal_locks WHERE entity_sync_id = '{}'",
            lock.entity_sync_id
        ),
    )
    .await;
    assert_eq!(written, 0, "nothing written");
}

/// An isolation lock received as fitted by a worker with a forced removal of their padlock they were
/// not told of (taken off before) is refused, as the local fitting is
/// (`permit::queries::open_forced_removal_of`).
#[tokio::test]
async fn a_received_lock_of_a_worker_not_told_of_a_padlock_forced_removal_is_refused() {
    let db = setup().await;
    let (_, _, lock) = padlock_on(&db, "EQ-L4R-F").await;
    force_off(&db, &lock, "l4rf").await;
    // The padlock taken off by force is APPLIER's, not told yet.
    exec(
        &db,
        &format!(
            "UPDATE permit_personal_locks SET placed_by_id = {APPLIER} WHERE id = {}",
            lock.id
        ),
    )
    .await;
    let asset = insert_equipment(&db, "EQ-L4R-F2").await;
    let other = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let line = add_isolation(&db, other.id, "Valve V-L4RF").await;
    let applier = user_sync_id(&db, APPLIER).await;
    let fitted_now = with_keys(
        wire_of(&db, "permit_isolations", &line.entity_sync_id).await,
        &[("applied_by_sync_id", json!(applier)), ("applied_at", json!(stamp()))],
    );
    let outcome = apply_entity(&db, "permit_isolations", &line.entity_sync_id, &fitted_now).await;
    assert_eq!(conflict_of(&outcome), Some("PERMIT_WORKER_NOT_INFORMED"), "{outcome:?}");
    // Fitted before the forced removal: the worker could not have been told yet, accepted.
    let earlier = (chrono::Utc::now() - chrono::Duration::minutes(90))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let fitted_before = with_keys(fitted_now, &[("applied_at", json!(earlier))]);
    let outcome = apply_entity(&db, "permit_isolations", &line.entity_sync_id, &fitted_before).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A worker withdrawn after their padlock was taken off by force, not told yet, still finds the
/// permit in "Mes cadenas" to acknowledge it; once told, a withdrawn line lists it no more.
#[tokio::test]
async fn a_withdrawn_worker_still_sees_an_untold_forced_removal_in_my_padlocks() {
    use crate::auth::scope::AccessScope;
    let db = setup().await;
    let (wp, _, lock) = padlock_on(&db, "EQ-L4R-W").await;
    force_off(&db, &lock, "l4rw").await;
    exec(
        &db,
        &format!(
            "UPDATE permit_workers SET withdrawn_at = '{}', withdrawn_by_id = {APPLIER} WHERE id = {}",
            stamp(),
            lock.worker_id
        ),
    )
    .await;
    let listed = workers::list_my_padlock_permits(&db, &AccessScope::Everywhere, VERIFIER)
        .await
        .expect("listed");
    assert!(
        listed.iter().any(|p| p.permit.id == wp.id),
        "the untold forced removal keeps the permit in Mes cadenas"
    );
    workers::acknowledge_forced_padlock_removal(&db, lock.id, VERIFIER)
        .await
        .expect("told");
    let listed = workers::list_my_padlock_permits(&db, &AccessScope::Everywhere, VERIFIER)
        .await
        .expect("listed");
    assert!(
        listed.iter().all(|p| p.permit.id != wp.id),
        "withdrawn and told: nothing left to do on it"
    );
}
