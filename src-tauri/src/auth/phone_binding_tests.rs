//! Sign-in on a phone: only the enrolled person; an unbound phone resets under the policy.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::*;
use crate::commands::product_license::record_enrolled_claim_for_tests;

const TENANT: &str = "tenant-bound";

fn binding() -> MobileEnrolmentBinding {
    MobileEnrolmentBinding {
        enrolment_id: "enr-9".into(),
        person_sync_id: "person-amina".into(),
        person_label: "Amina Benali".into(),
        site_sync_id: "site-nord".into(),
        site_label: "Usine Nord".into(),
        enrolled_at: "2026-10-29T08:00:00Z".into(),
    }
}

fn code_of(err: &AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

async fn user(db: &DatabaseConnection, username: &str, sync_id: &str) -> i32 {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) VALUES (?, ?, ?, 'local', 1, 0, 0, datetime('now'), datetime('now'))",
        [sync_id.into(), username.into(), username.into()],
    ))
    .await
    .expect("user");
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM user_accounts WHERE username = ?",
        [username.into()],
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get("", "id")
    .expect("id")
}

#[test]
fn only_the_enrolled_person_signs_in() {
    let b = binding();
    assert_eq!(
        phone_sign_in(true, Some(&b), false, Some("person-amina")),
        PhoneSignIn::Allowed
    );
    for other in [Some("person-karim"), Some("PERSON-AMINA"), Some(""), None] {
        assert_eq!(
            phone_sign_in(true, Some(&b), false, other),
            PhoneSignIn::AssignedToOther {
                person_label: "Amina Benali".into()
            },
            "{other:?}"
        );
    }
}

#[test]
fn an_unbound_or_erasing_phone_signs_nobody_in() {
    assert_eq!(
        phone_sign_in(true, None, false, Some("person-amina")),
        PhoneSignIn::NotEnrolled
    );
    let b = binding();
    assert_eq!(
        phone_sign_in(true, Some(&b), true, Some("person-amina")),
        PhoneSignIn::RemovalInProgress
    );
    assert_eq!(phone_sign_in(false, None, true, None), PhoneSignIn::RemovalInProgress);
    // Not activated: sign-in is refused further on, as on any device.
    assert_eq!(phone_sign_in(false, None, false, None), PhoneSignIn::Allowed);
}

#[test]
fn the_unbound_policy_is_upload_then_reset() {
    assert_eq!(unbound_phone_policy(), UnboundPhonePolicy::UploadThenReset);
    assert_eq!(unbound_phone_removal(), Some(RemovalKind::Unbound));
}

#[tokio::test]
async fn the_gate_refuses_another_person_with_the_enrolled_name() {
    let db = crate::db::test_db::seeded().await;
    record_enrolled_claim_for_tests(&db, TENANT, "fp-1", "token-1", Some(&binding()))
        .await
        .expect("enrolled");
    let amina = user(&db, "amina", "person-amina").await;
    let karim = user(&db, "karim", "person-karim").await;

    enforce_phone_sign_in_on(&db, amina, true)
        .await
        .expect("the enrolled person");
    // The built-in administrator account is not the enrolled person either.
    let admin: i32 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'admin'".to_string(),
        ))
        .await
        .expect("query")
        .expect("built-in admin")
        .try_get("", "id")
        .expect("id");
    for other in [karim, admin] {
        let err = enforce_phone_sign_in_on(&db, other, true)
            .await
            .expect_err("someone else");
        assert_eq!(code_of(&err), PHONE_ASSIGNED_TO_OTHER_PERSON);
        let AppError::OrgValidationFailed(issues) = &err else {
            panic!("{err:?}");
        };
        assert_eq!(
            issues[0].params.get("person_label").map(String::as_str),
            Some("Amina Benali")
        );
    }
    // A desktop is never refused here.
    enforce_phone_sign_in_on(&db, karim, false).await.expect("desktop");
}

#[tokio::test]
async fn an_unbound_phone_refuses_sign_in_and_starts_its_reset() {
    let db = crate::db::test_db::seeded().await;
    record_enrolled_claim_for_tests(&db, TENANT, "fp-1", "token-1", None)
        .await
        .expect("activated with the key");
    let karim = user(&db, "karim", "person-karim").await;
    assert!(unbound_phone_needs_removal(&db).await.expect("unbound"));

    let err = enforce_phone_sign_in_on(&db, karim, true).await.expect_err("unbound");
    assert_eq!(code_of(&err), PHONE_NOT_ENROLLED);
    let pending = removal::pending(&db).await.expect("read").expect("recorded");
    assert_eq!(pending.kind, RemovalKind::Unbound);

    let err = enforce_phone_sign_in_on(&db, karim, true).await.expect_err("removal");
    assert_eq!(code_of(&err), PHONE_REMOVAL_IN_PROGRESS);
}

#[tokio::test]
async fn a_bound_phone_being_removed_signs_nobody_in() {
    let db = crate::db::test_db::seeded().await;
    record_enrolled_claim_for_tests(&db, TENANT, "fp-1", "token-1", Some(&binding()))
        .await
        .expect("enrolled");
    assert!(!unbound_phone_needs_removal(&db).await.expect("bound"));
    let amina = user(&db, "amina", "person-amina").await;
    removal::record(&db, RemovalKind::Removed).await.expect("removed");
    let err = enforce_phone_sign_in_on(&db, amina, true).await.expect_err("removal");
    assert_eq!(code_of(&err), PHONE_REMOVAL_IN_PROGRESS);
}
