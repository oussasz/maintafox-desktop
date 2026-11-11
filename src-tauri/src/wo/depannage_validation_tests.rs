//! After-the-fact validation of a dépannage (`wo::depannage_validation`, CMMS Advisor decision
//! 2026-11-04): never by its starter; the close waits for it unless the site opted out (flagged
//! automatic approval); a received validation holds only for an approver of the site here; the
//! times of a dépannage (EN 13306).

use chrono::Duration;
use sea_orm::DatabaseConnection;
use serde_json::{json, Map, Value};

use crate::auth::scope_tests::{assign, exec, role, scalar, tree, user, Tree};
use crate::errors::AppError;
use crate::rbac::permissions::{OT_APPROVE, OT_CREATE, OT_EDIT, OT_VIEW};
use crate::settings::site::InboundVerdict;
use crate::wo::depannage::{self, WoDepannageInput};
use crate::wo::depannage_validation::{
    self as validation, CloseValidation, WoDepannageValidateInput, AUTO_VALIDATION_SETTING, CODE_ALREADY_VALIDATED,
    CODE_NOT_VALIDATED, CODE_SYNC_VALIDATION_INVALID, CODE_VALIDATOR_IS_STARTER,
};
use crate::wo::time::stamp;

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// ot.create + ot.edit + ot.approve on site A: starts dépannages.
    tech: i32,
    /// ot.approve on site A.
    approver: i32,
    /// ot.view only on site A.
    viewer: i32,
    eq_a: i64,
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let tech = user(&db, "val_tech").await;
    let approver = user(&db, "val_approver").await;
    let viewer = user(&db, "val_viewer").await;
    let tech_role = role(&db, "Validation tech A", &[OT_VIEW, OT_CREATE, OT_EDIT, OT_APPROVE]).await;
    let approver_role = role(&db, "Validation approver A", &[OT_VIEW, OT_APPROVE]).await;
    let viewer_role = role(&db, "Validation viewer A", &[OT_VIEW]).await;
    assign(&db, tech, tech_role, "org_node", Some(t.site_a)).await;
    assign(&db, approver, approver_role, "org_node", Some(t.site_a)).await;
    assign(&db, viewer, viewer_role, "org_node", Some(t.site_a)).await;
    exec(
        &db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('val-eq-a', 'VAL-A', 'Equipment A', 'active_in_service', {},
                     datetime('now'), datetime('now'))",
            t.site_a
        ),
    )
    .await;
    let eq_a = scalar(&db, "SELECT id FROM equipment WHERE entity_sync_id = 'val-eq-a'").await;
    Fixture {
        db,
        t,
        tech,
        approver,
        viewer,
        eq_a,
    }
}

fn before(d: Duration) -> String {
    stamp(crate::wo::time::now_utc_second() - d)
}

async fn start(f: &Fixture, machine_stopped: bool) -> crate::wo::domain::WorkOrder {
    depannage::start_depannage(
        &f.db,
        WoDepannageInput {
            equipment_id: f.eq_a,
            type_code: None,
            urgency_code: Some("emergency".into()),
            subtype_code: None,
            title: "Pompe en défaut".into(),
            description: None,
            breakdown_at: Some(before(Duration::hours(3))),
            machine_stopped: Some(machine_stopped),
            assignee_id: None,
            requires_permit: None,
            actor_id: i64::from(f.tech),
            permit_drafting: None,
        },
    )
    .await
    .expect("dépannage")
    .wo
}

fn validate_input(wo: &crate::wo::domain::WorkOrder, actor: i32) -> WoDepannageValidateInput {
    WoDepannageValidateInput {
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        actor_id: i64::from(actor),
    }
}

fn refused_with(err: &AppError, code: &str) {
    assert!(format!("{err:?}").contains(code), "expected {code}, got {err:?}");
}

#[tokio::test]
async fn a_depannage_is_validated_once_by_an_approver_who_did_not_start_it() {
    let f = fixture().await;
    let wo = start(&f, true).await;
    assert!(wo.validated_by_id.is_none(), "à valider");

    let err = validation::validate_depannage(&f.db, validate_input(&wo, f.tech))
        .await
        .expect_err("the starter never validates their own dépannage");
    refused_with(&err, CODE_VALIDATOR_IS_STARTER);

    let done = validation::validate_depannage(&f.db, validate_input(&wo, f.approver))
        .await
        .expect("another approver validates it");
    assert_eq!(done.validated_by_id, Some(i64::from(f.approver)));
    assert!(done.validated_at.is_some());
    assert_eq!(done.row_version, wo.row_version + 1);

    let err = validation::validate_depannage(&f.db, validate_input(&done, f.viewer))
        .await
        .expect_err("validated once");
    refused_with(&err, CODE_ALREADY_VALIDATED);
}

#[tokio::test]
async fn the_close_waits_for_the_validation_unless_the_site_opted_out() {
    let f = fixture().await;
    let wo = start(&f, true).await;
    match validation::close_validation(&f.db, wo.id).await.expect("state") {
        CloseValidation::Refused(msg) => assert!(msg.starts_with(CODE_NOT_VALIDATED), "{msg}"),
        other => panic!("not validated: refused, got {other:?}"),
    }
    let (errors, _, _) = crate::wo::closeout::close_preflight(
        &f.db,
        wo.id,
        &crate::wo::closeout::CloseEvidence {
            closer_user_id: Some(i64::from(f.tech)),
            no_downtime_attestation: None,
            no_downtime_attestation_reason: None,
            fmeca_override_reason: None,
            fmeca_override_signer_id: None,
            fmeca_override_signer_password: crate::wo::closeout::SignerPassword::Given(None),
        },
        1000,
    )
    .await
    .expect("preflight");
    assert!(
        errors.iter().any(|e| e.starts_with(CODE_NOT_VALIDATED)),
        "the close checks list the missing validation: {errors:?}"
    );

    exec(
        &f.db,
        &format!(
            "INSERT INTO site_settings (entity_sync_id, site_node_id, setting_key, value_json) \
             VALUES ('val-site-setting', {}, '{AUTO_VALIDATION_SETTING}', 'true')",
            f.t.site_a
        ),
    )
    .await;
    assert_eq!(
        validation::close_validation(&f.db, wo.id).await.expect("state"),
        CloseValidation::AutoApproved
    );
    validation::mark_auto_approved(&f.db, wo.id).await.expect("flag");
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COALESCE(validation_auto_approved, 0) FROM work_orders WHERE id = {}",
                wo.id
            )
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM work_orders WHERE id = {} AND validated_by_id IS NULL",
                wo.id
            )
        )
        .await,
        1,
        "an automatic approval names no validator"
    );

    let other = start(&f, false).await;
    validation::validate_depannage(&f.db, validate_input(&other, f.approver))
        .await
        .expect("validated");
    assert_eq!(
        validation::close_validation(&f.db, other.id).await.expect("state"),
        CloseValidation::Validated
    );
}

#[tokio::test]
async fn the_times_of_a_depannage_run_from_the_breakdown() {
    let f = fixture().await;
    let wo = start(&f, true).await;
    exec(
        &f.db,
        &format!(
            "UPDATE work_orders SET breakdown_at = '2026-11-01T08:00:00Z', actual_start = '2026-11-01T09:30:00Z', \
                    actual_end = '2026-11-01T11:30:00Z', closed_at = '2026-11-01T12:00:00Z' WHERE id = {}",
            wo.id
        ),
    )
    .await;
    let times = validation::depannage_times(&f.db, wo.id)
        .await
        .expect("times")
        .expect("a dépannage");
    assert_eq!(times.response_hours, Some(1.5), "breakdown → actual start");
    assert_eq!(times.active_repair_hours, Some(2.0), "actual start → end");
    assert_eq!(
        times.restore_hours,
        Some(4.0),
        "breakdown → back in service (the close)"
    );
    assert_eq!(times.stopped_hours, Some(4.0), "machine stopped from the breakdown");
    assert_eq!(times.downtime_start.as_deref(), Some("2026-11-01T08:00:00Z"));

    exec(
        &f.db,
        &format!("UPDATE work_orders SET machine_stopped = 0 WHERE id = {}", wo.id),
    )
    .await;
    let times = validation::depannage_times(&f.db, wo.id)
        .await
        .expect("times")
        .expect("a dépannage");
    assert_eq!(
        times.stopped_hours, None,
        "machine running: the downtime segments tell it"
    );
    assert_eq!(times.downtime_start, None, "no machine stop recorded");

    exec(
        &f.db,
        &format!("UPDATE work_orders SET is_depannage = 0 WHERE id = {}", wo.id),
    )
    .await;
    assert!(validation::depannage_times(&f.db, wo.id)
        .await
        .expect("times")
        .is_none());
}

fn payload(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn is_conflict(v: &Option<InboundVerdict>) -> bool {
    matches!(v, Some(InboundVerdict::Conflict { code, .. }) if *code == CODE_SYNC_VALIDATION_INVALID)
}

#[tokio::test]
async fn a_received_validation_holds_only_for_an_approver_of_the_site_who_did_not_start_it() {
    let f = fixture().await;
    let at = before(Duration::minutes(5));
    let base = |validator: &str| {
        payload(&[
            ("requester_sync_id", json!("scope-user-val_tech")),
            ("entity_sync_id", json!("scope-A")),
            ("validated_by_sync_id", json!(validator)),
            ("validated_at", json!(at)),
        ])
    };

    let ok = validation::inbound_validation_check(&f.db, "OT-1", None, &base("scope-user-val_approver"))
        .await
        .expect("check");
    assert!(ok.is_none(), "an approver of site A: accepted, got {ok:?}");

    let own = validation::inbound_validation_check(&f.db, "OT-1", None, &base("scope-user-val_tech"))
        .await
        .expect("check");
    assert!(is_conflict(&own), "validated by its starter: conflict, got {own:?}");

    let no_right = validation::inbound_validation_check(&f.db, "OT-1", None, &base("scope-user-val_viewer"))
        .await
        .expect("check");
    assert!(
        is_conflict(&no_right),
        "no approval right at the site: conflict, got {no_right:?}"
    );

    let unknown = validation::inbound_validation_check(&f.db, "OT-1", None, &base("scope-user-nobody"))
        .await
        .expect("check");
    assert!(
        matches!(unknown, Some(InboundVerdict::Defer(_))),
        "a validator not here yet: waits, got {unknown:?}"
    );

    let mut both = base("scope-user-val_approver");
    both.insert("validation_auto_approved".into(), json!(1));
    let both = validation::inbound_validation_check(&f.db, "OT-1", None, &both)
        .await
        .expect("check");
    assert!(
        is_conflict(&both),
        "validated and auto-approved at once: conflict, got {both:?}"
    );

    let local = base("scope-user-val_approver");
    let rewritten = validation::inbound_validation_check(&f.db, "OT-1", Some(&local), &base("scope-user-val_viewer"))
        .await
        .expect("check");
    assert!(
        is_conflict(&rewritten),
        "a validation here is never rewritten, got {rewritten:?}"
    );

    let mut removed = local.clone();
    removed.insert("validated_by_sync_id".into(), Value::Null);
    removed.insert("validated_at".into(), Value::Null);
    let removed = validation::inbound_validation_check(&f.db, "OT-1", Some(&local), &removed)
        .await
        .expect("check");
    assert!(
        is_conflict(&removed),
        "a validation here is never removed, got {removed:?}"
    );
}
