//! Forced removal of a lock whose applier is absent, and telling the worker (migration 249; CMMS
//! Advisor register 2026-10-20, OSHA 1910.147(e)(3)(ii) A–C): each refusal, the forced path, the
//! worker's acknowledgement at sign-in, the path without an account, the block on new locks, a
//! permit closing with the item still open, and the three-person rule kept.

use sea_orm::DatabaseConnection;

use super::tests::{
    action, add_isolation, advance_to_issued, apply_and_verify, create_permit, insert_equipment, scalar_i64,
    set_status, setup, take_off, user_with, APPLIER, VERIFIER,
};
use crate::auth::scope::AccessScope;
use crate::errors::AppError;
use crate::permit::domain::{
    ContactAttempt, PermitForcedRemovalInput, PermitIsolation, PermitWorkerInformedInput, WorkPermit,
};
use crate::permit::queries;
use crate::rbac::permissions::PTW_CLOSE;

fn code(err: AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

fn ago(minutes: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

/// The first method of the plant's contact list (seeded "à revoir" by migration 249).
async fn method(db: &DatabaseConnection) -> String {
    crate::permit::contact_methods::list_contact_methods(db)
        .await
        .expect("methods")
        .first()
        .expect("a starter method")
        .code
        .clone()
}

async fn attempt(db: &DatabaseConnection) -> ContactAttempt {
    ContactAttempt {
        at: ago(30),
        method: method(db).await,
        result: "Pas de réponse, messagerie".into(),
    }
}

fn forced(iso: &PermitIsolation, remover: i64, attempts: Vec<ContactAttempt>) -> PermitForcedRemovalInput {
    PermitForcedRemovalInput {
        isolation_id: iso.id,
        expected_row_version: iso.row_version,
        removed_by_id: remover,
        reason: "Fin de poste, travailleur parti sans retirer son cadenas".into(),
        worker_absent_attested: true,
        contact_attempts: attempts,
    }
}

/// An active LOTO permit whose lock was fitted by APPLIER and verified by VERIFIER, an authoriser
/// holding the close right plant-wide and a person to take the lock off.
async fn active_lock(db: &DatabaseConnection, asset_code: &str) -> (WorkPermit, PermitIsolation, i64, i64) {
    let asset = insert_equipment(db, asset_code).await;
    let wp = advance_to_issued(db, create_permit(db, "loto", asset).await).await;
    let iso = apply_and_verify(db, &add_isolation(db, wp.id, &format!("{asset_code} breaker")).await).await;
    let wp = set_status(db, &wp, "active").await.expect("activate");
    let authoriser = user_with(db, &format!("{asset_code}-chef"), &[PTW_CLOSE]).await;
    let remover = user_with(db, &format!("{asset_code}-remover"), &[]).await;
    (wp, iso, authoriser, remover)
}

async fn audits(db: &DatabaseConnection, action_code: &str) -> i64 {
    scalar_i64(
        db,
        &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = '{action_code}'"),
    )
    .await
}

#[tokio::test]
async fn a_forced_removal_names_who_why_and_the_attempts_or_is_refused() {
    let db = setup().await;
    let (_, iso, authoriser, remover) = active_lock(&db, "EQ-FR-1").await;
    let a = attempt(&db).await;

    let refusal = |input: PermitForcedRemovalInput, actor: i64| {
        let db = &db;
        async move {
            code(
                queries::force_permit_isolation_removal(db, input, actor)
                    .await
                    .expect_err("refused"),
            )
        }
    };
    assert_eq!(
        refusal(forced(&iso, remover, vec![a.clone()]), APPLIER).await,
        "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_APPLIER"
    );
    assert_eq!(
        refusal(forced(&iso, APPLIER, vec![a.clone()]), authoriser).await,
        "PERMIT_FORCED_REMOVAL_REMOVER_IS_APPLIER"
    );
    assert_eq!(
        refusal(forced(&iso, authoriser, vec![a.clone()]), authoriser).await,
        "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_REMOVER"
    );
    let mut short = forced(&iso, remover, vec![a.clone()]);
    short.reason = "   Absent            ".into();
    assert_eq!(
        refusal(short, authoriser).await,
        "PERMIT_FORCED_REMOVAL_REASON_TOO_SHORT"
    );
    let mut unattested = forced(&iso, remover, vec![a.clone()]);
    unattested.worker_absent_attested = false;
    assert_eq!(
        refusal(unattested, authoriser).await,
        "PERMIT_FORCED_REMOVAL_ATTESTATION_REQUIRED"
    );
    assert_eq!(
        refusal(forced(&iso, remover, vec![]), authoriser).await,
        "PERMIT_FORCED_REMOVAL_CONTACT_ATTEMPTS_REQUIRED"
    );
    let unknown_method = ContactAttempt {
        method: "Pigeon voyageur".into(),
        ..a.clone()
    };
    assert_eq!(
        refusal(forced(&iso, remover, vec![unknown_method]), authoriser).await,
        "PERMIT_CONTACT_METHOD_NOT_IN_LIST"
    );
    let future = ContactAttempt {
        at: (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339(),
        ..a.clone()
    };
    assert_eq!(
        refusal(forced(&iso, remover, vec![future]), authoriser).await,
        "PERMIT_FORCED_REMOVAL_ATTEMPT_INVALID"
    );
    let no_result = ContactAttempt {
        result: "  ".into(),
        ..a.clone()
    };
    assert_eq!(
        refusal(forced(&iso, remover, vec![no_result]), authoriser).await,
        "PERMIT_FORCED_REMOVAL_ATTEMPT_INVALID"
    );
    // Someone without the close right at the permit's site.
    let outsider = user_with(&db, "fr1-outsider", &[]).await;
    assert_eq!(
        refusal(forced(&iso, remover, vec![a.clone()]), outsider).await,
        "PERMIT_FORCED_REMOVAL_UNAUTHORISED"
    );
    assert_eq!(audits(&db, queries::AUDIT_FORCED_REMOVAL).await, 0, "nothing recorded");

    let out = queries::force_permit_isolation_removal(&db, forced(&iso, remover, vec![a.clone()]), authoriser)
        .await
        .expect("forced removal");
    assert!(out.forced_removal);
    assert!(out.forced_worker_absent);
    assert_eq!(out.removed_by_id, Some(remover));
    assert_eq!(out.forced_authorized_by_id, Some(authoriser));
    assert!(out.removed_at.is_some());
    assert_eq!(
        out.forced_reason.as_deref(),
        Some("Fin de poste, travailleur parti sans retirer son cadenas")
    );
    assert_eq!(out.forced_contact_attempts.len(), 1);
    assert_eq!(out.forced_contact_attempts[0].method, a.method);
    assert!(out.informed_at.is_none(), "the worker is not told yet");
    assert_eq!(audits(&db, queries::AUDIT_FORCED_REMOVAL).await, 1);
    // The ptw.close holders of the site have the persistent notice (the authoriser included).
    let notified = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.category_code = 'loto_forced_removal' AND n.recipient_user_id = {authoriser}"
        ),
    )
    .await;
    assert_eq!(notified, 1);
    let notices = crate::settings::received_notice::notices_for(&db, authoriser)
        .await
        .expect("notices");
    assert_eq!(notices.len(), 1, "shown after each sign-in until acknowledged");
    assert_eq!(
        notices[0].message_key.as_deref(),
        Some(crate::settings::received_notice::FORCED_REMOVAL_CATEGORY)
    );
    // Once only.
    assert_eq!(
        code(
            queries::force_permit_isolation_removal(&db, forced(&out, remover, vec![a]), authoriser)
                .await
                .expect_err("twice")
        ),
        "PERMIT_LOCK_ALREADY_REMOVED"
    );
    // The LOTO card says "Retrait forcé" with who, when and why.
    let card = queries::get_loto_card_view(&db, out.permit_id, out.id)
        .await
        .expect("card");
    assert!(card.forced_removal);
    assert_eq!(card.forced_removed_at, out.removed_at);
    assert!(card.forced_authorized_by.is_some());
    assert_eq!(card.forced_reason, out.forced_reason);
}

/// The removal of a forced removal is verified by a second person who is neither the applier nor
/// the remover; the permit then closes with the worker not told yet, and the item stays on the
/// review list; the worker fits no new lock until they acknowledge it at sign-in.
#[tokio::test]
async fn the_worker_acknowledges_at_sign_in_before_fitting_a_new_lock() {
    let db = setup().await;
    let (wp, iso, authoriser, remover) = active_lock(&db, "EQ-FR-2").await;
    let a = attempt(&db).await;
    let out = queries::force_permit_isolation_removal(&db, forced(&iso, remover, vec![a]), authoriser)
        .await
        .expect("forced removal");

    let err = queries::verify_permit_isolation_removal(&db, action(&out), remover)
        .await
        .expect_err("remover");
    assert_eq!(code(err), "PERMIT_REMOVAL_VERIFIER_IS_REMOVER");
    let err = queries::verify_permit_isolation_removal(&db, action(&out), APPLIER)
        .await
        .expect_err("applier");
    assert_eq!(code(err), "PERMIT_TWO_PERSON_REMOVAL");
    let verified = queries::verify_permit_isolation_removal(&db, action(&out), VERIFIER)
        .await
        .expect("verified");
    let closed = set_status(&db, &wp, "closed")
        .await
        .expect("the permit closes before the worker is told");
    assert_eq!(closed.status, "closed");

    // The open item stays on the review list of the authoriser and of the site's holders.
    let review = queries::open_forced_removals_in_scope(&db, &AccessScope::Everywhere, authoriser)
        .await
        .expect("review list");
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].isolation.id, verified.id);
    assert_eq!(review[0].permit_status, "closed");

    // The worker cannot fit a new lock.
    let asset = insert_equipment(&db, "EQ-FR-2b").await;
    let other = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let new_line = add_isolation(&db, other.id, "Valve V-2b").await;
    let err = queries::apply_permit_isolation(&db, action(&new_line), APPLIER)
        .await
        .expect_err("worker not told");
    assert_eq!(code(err), "PERMIT_WORKER_NOT_INFORMED");

    // The blocking screen at sign-in lists it for the worker only.
    let mine = queries::forced_removals_to_acknowledge(&db, APPLIER)
        .await
        .expect("mine");
    assert_eq!(mine.len(), 1);
    assert!(queries::forced_removals_to_acknowledge(&db, VERIFIER)
        .await
        .expect("not theirs")
        .is_empty());
    let err = queries::acknowledge_forced_removal(&db, verified.id, VERIFIER)
        .await
        .expect_err("not the worker");
    assert_eq!(code(err), "PERMIT_WORKER_ACK_NOT_YOURS");
    let told = queries::acknowledge_forced_removal(&db, verified.id, APPLIER)
        .await
        .expect("acknowledged");
    assert!(told.informed_in_app);
    assert_eq!(told.informed_by_id, Some(APPLIER));
    assert!(told.informed_at.is_some());
    assert!(told.informed_method.is_none());
    assert_eq!(audits(&db, queries::AUDIT_WORKER_ACKNOWLEDGED).await, 1);
    assert!(queries::forced_removals_to_acknowledge(&db, APPLIER)
        .await
        .expect("mine")
        .is_empty());
    assert!(
        queries::open_forced_removals_in_scope(&db, &AccessScope::Everywhere, authoriser)
            .await
            .expect("review list")
            .is_empty()
    );
    let err = queries::acknowledge_forced_removal(&db, verified.id, APPLIER)
        .await
        .expect_err("twice");
    assert_eq!(code(err), "PERMIT_WORKER_ALREADY_INFORMED");

    // Told: the worker fits locks again.
    queries::apply_permit_isolation(&db, action(&new_line), APPLIER)
        .await
        .expect("applied once told");
}

/// A worker without an account (or told in person): a holder of the close right at the site
/// records who told them, when and how; never the worker themselves, never before the removal.
#[tokio::test]
async fn a_close_right_holder_records_that_the_worker_was_told() {
    let db = setup().await;
    let (_, iso, authoriser, remover) = active_lock(&db, "EQ-FR-3").await;
    // A lock of the same worker taken off by themselves (fitted before the forced removal below).
    let (_, plain, _, _) = active_lock(&db, "EQ-FR-3b").await;
    let plain = take_off(&db, &plain).await;
    let m = method(&db).await;
    let out = queries::force_permit_isolation_removal(&db, forced(&iso, remover, vec![attempt(&db).await]), authoriser)
        .await
        .expect("forced removal");
    let input = |iso: &PermitIsolation, method: &str, at: Option<String>| PermitWorkerInformedInput {
        isolation_id: iso.id,
        expected_row_version: iso.row_version,
        method: method.into(),
        informed_at: at,
    };
    let refusal = |input: PermitWorkerInformedInput, actor: i64| {
        let db = &db;
        async move {
            code(
                queries::record_worker_informed(db, input, actor)
                    .await
                    .expect_err("refused"),
            )
        }
    };
    assert_eq!(
        refusal(input(&out, &m, None), APPLIER).await,
        "PERMIT_WORKER_INFORMED_SELF"
    );
    assert_eq!(
        refusal(input(&out, &m, None), remover).await,
        "PERMIT_WORKER_INFORMED_UNAUTHORISED"
    );
    assert_eq!(
        refusal(input(&out, "Pigeon voyageur", None), authoriser).await,
        "PERMIT_CONTACT_METHOD_NOT_IN_LIST"
    );
    assert_eq!(
        refusal(input(&out, &m, Some(ago(24 * 60))), authoriser).await,
        "PERMIT_WORKER_INFORMED_TIME_INVALID"
    );
    // A lock not taken off by force has nobody to tell.
    assert_eq!(
        refusal(input(&plain, &m, None), authoriser).await,
        "PERMIT_WORKER_INFORMED_NOT_FORCED"
    );

    let told = queries::record_worker_informed(&db, input(&out, &m, None), authoriser)
        .await
        .expect("recorded");
    assert!(!told.informed_in_app);
    assert_eq!(told.informed_by_id, Some(authoriser));
    assert_eq!(told.informed_method.as_deref(), Some(m.as_str()));
    assert_eq!(audits(&db, queries::AUDIT_WORKER_INFORMED).await, 1);
    assert_eq!(
        refusal(input(&told, &m, None), authoriser).await,
        "PERMIT_WORKER_ALREADY_INFORMED"
    );
    assert!(queries::forced_removals_to_acknowledge(&db, APPLIER)
        .await
        .expect("mine")
        .is_empty());
}

/// With the three-person chain on the permit's type, the isolation's verifier does not confirm the
/// removal of a forced removal either.
#[tokio::test]
async fn the_three_person_rule_applies_to_a_forced_removal() {
    let db = setup().await;
    super::tests::loto_with_three_person_chain(&db).await;
    let (_, iso, authoriser, remover) = active_lock(&db, "EQ-FR-4").await;
    let out = queries::force_permit_isolation_removal(&db, forced(&iso, remover, vec![attempt(&db).await]), authoriser)
        .await
        .expect("forced removal");
    let err = queries::verify_permit_isolation_removal(&db, action(&out), VERIFIER)
        .await
        .expect_err("isolation verifier");
    assert_eq!(code(err), "PERMIT_THREE_PERSON_REMOVAL");
    let third = user_with(&db, "fr4-third", &[]).await;
    queries::verify_permit_isolation_removal(&db, action(&out), third)
        .await
        .expect("a third person");
}

/// A lock taken off no longer protects work: a suspended permit is not reinstated with it.
#[tokio::test]
async fn a_lock_taken_off_blocks_reactivation() {
    let db = setup().await;
    let (wp, iso, _, _) = active_lock(&db, "EQ-FR-5").await;
    take_off(&db, &iso).await;
    let (wp, _) = queries::suspend_work_permit(
        &db,
        crate::permit::domain::PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Weather".into(),
            reactivation_conditions: None,
        },
        VERIFIER,
    )
    .await
    .expect("suspend");
    let wp = set_status(&db, &wp, "revalidation_required")
        .await
        .expect("revalidation");
    let err = set_status(&db, &wp, "active").await.expect_err("lock off");
    assert_eq!(code(err), "LOTO_ISOLATION_REMOVED");
}
