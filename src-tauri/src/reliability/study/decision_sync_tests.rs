//! AU-76b: a reliability decision received approved or applied is checked again on arrival
//! (`reliability::study::decision_sync`, CMMS Advisor answers 2026-10-26).

use serde_json::{Map, Value};

use super::*;
use crate::reliability::study::decision_sync::{
    approval_seal, local_approval_seal, resend_sealed_after_complete_pull, retry_decision_conflicts,
    CONFLICT_CLOSED_EVENT, REFUSED_EVENT, SEAL_RESEND_KEY,
};
use crate::sync::materialize::ApplyOutcome;

type Payload = Map<String, Value>;

const ENTITY: &str = "reliability_study_decisions";

/// An approved one-off work-order decision (proposed by the admin, approved by the reviewer).
async fn approved(f: &Fixture, study_id: i64) -> (crate::reliability::study::domain::StudyDecision, String) {
    let d = wo_decision(f, study_id, None).await;
    let a = approve_decision(&f.db, decide(&d, Some("OK pour moi")), f.reviewer)
        .await
        .expect("approve");
    let sid = decision_sync_id(f, a.id).await;
    (a, sid)
}

/// The same payload under a new sync id: a decision new here.
fn as_new(wire: &Payload, tag: &str) -> (String, Payload) {
    (format!("pcb-{tag}"), wire.clone())
}

/// Re-seals a payload changed on purpose (an upgraded sender computing its own seal).
fn resealed(mut p: Payload) -> Payload {
    let seal = approval_seal(&p);
    p.insert("approval_seal".into(), seal.into());
    p
}

fn deferred_code(o: &ApplyOutcome) -> Option<&str> {
    match o {
        ApplyOutcome::Deferred { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

async fn user_sync(f: &Fixture, id: i64) -> String {
    text(f, &format!("SELECT entity_sync_id FROM user_accounts WHERE id = {id}")).await
}

/// A received row waiting in the inbox, as a pull leaves it.
async fn inbox(f: &Fixture, sid: &str, p: &Payload, rv: i64) {
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
             row_version, payload_json, payload_hash, apply_status) VALUES ('b-{sid}-{rv}', 'c1', '{ENTITY}', \
             '{sid}', 'upsert', {rv}, '{}', 'h-{sid}-{rv}', 'pending')",
            serde_json::to_string(p).expect("json").replace('\'', "''")
        ),
    )
    .await;
}

async fn materialize(f: &Fixture) {
    crate::sync::materialize::materialize_inbox(&f.db, 200, true)
        .await
        .expect("materialize");
    crate::sync::row_codec::clear_ref_cache();
}

/// The open conflict of a decision: id, row version, code.
async fn open_conflict(f: &Fixture, sid: &str) -> (i64, i64, String) {
    let r =
        f.db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT id, row_version, conflict_type FROM sync_conflicts WHERE entity_sync_id = '{sid}' \
                   AND status IN ('new', 'triaged', 'escalated') ORDER BY id DESC LIMIT 1"
            ),
        ))
        .await
        .expect("q")
        .expect("open conflict");
    (
        r.try_get("", "id").expect("id"),
        r.try_get("", "row_version").expect("rv"),
        r.try_get("", "conflict_type").expect("type"),
    )
}

async fn resolve(f: &Fixture, user: i64, sid: &str, action: &str) -> AppResult<()> {
    let (id, rv, _) = open_conflict(f, sid).await;
    crate::sync::queries::resolve_sync_conflict(
        &f.db,
        user,
        crate::sync::domain::ResolveSyncConflictInput {
            conflict_id: id,
            expected_row_version: rv,
            action: action.to_string(),
            resolution_note: Some("Approbation non valable sur ce poste".to_string()),
        },
    )
    .await
    .map(|_| ())
}

/// Test 1 and 12: approving seals the decision and audits the seal; the seal computed on the sent
/// payload and on the row applied on another PC (re-encoded) is the same, a meter reference in the
/// trigger included.
#[tokio::test]
async fn approving_seals_the_decision_and_the_seal_survives_the_round_trip() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (a, sid) = approved(&f, study_id).await;
    let stored = text(&f, &format!("SELECT approval_seal FROM {ENTITY} WHERE id = {}", a.id)).await;
    assert!(stored.starts_with("v1:") && stored.len() == 67, "{stored}");
    assert_eq!(
        local_approval_seal(&f.db, a.id).await.expect("seal").as_deref(),
        Some(stored.as_str())
    );
    let wire = wire_of(&f, ENTITY, &sid).await;
    assert_eq!(approval_seal(&wire), stored, "the sent form gives the same seal");
    assert_eq!(wire.get("approval_seal").and_then(Value::as_str), Some(stored.as_str()));

    // The approval audit event carries the seal and sync ids only.
    let details = text(
        &f,
        &format!(
            "SELECT details_json FROM audit_events WHERE action_code = 'reliability.decision.approved' \
               AND target_id = '{sid}'"
        ),
    )
    .await;
    let details: Value = serde_json::from_str(&details).expect("json");
    assert_eq!(details["approval_seal"], Value::String(stored.clone()));
    assert_eq!(details["decision_sync_id"], Value::String(sid.clone()));
    assert_eq!(
        details["proposed_by_sync_id"],
        Value::String(user_sync(&f, f.admin).await)
    );
    assert!(details.get("decision_id").is_none() && details.get("proposed_by_id").is_none());

    // A trigger naming a meter: the meter travels as its sync id, and the seal is the same.
    exec(
        &f.db,
        &format!(
            "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit_value_id, \
             current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at) \
             VALUES ('st-meter-1', {}, 'Heures', 'HOURS', (SELECT v.id FROM reference_values v \
             JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id \
             WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'H'), 50, 0, 0, 1, \
             datetime('now'), datetime('now'))",
            f.p101
        ),
    )
    .await;
    let meter = scalar(
        &f.db,
        "SELECT id FROM equipment_meters WHERE entity_sync_id = 'st-meter-1'",
    )
    .await;
    exec(
        &f.db,
        &format!(
            "UPDATE {ENTITY} SET proposed_trigger_definition_json = \
             '{{\"interval_value\":2688.0,\"asset_meter_id\":{meter},\"interval_unit\":\"operating_hours\"}}' \
             WHERE id = {}",
            a.id
        ),
    )
    .await;
    let local = local_approval_seal(&f.db, a.id).await.expect("seal").expect("row");
    let wire = wire_of(&f, ENTITY, &sid).await;
    assert!(
        wire["proposed_trigger_definition_json"]
            .as_str()
            .unwrap_or_default()
            .contains("st-meter-1"),
        "{wire:?}"
    );
    assert_eq!(approval_seal(&wire), local);

    // Applied on "PC B" as a decision new there (resealed with the meter), then re-encoded there.
    let (new_sid, mut b) = as_new(&wire, "roundtrip");
    b.insert("approval_seal".into(), local.clone().into());
    let outcome = apply_wire(&f, ENTITY, &new_sid, "upsert", &b).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let b_id = scalar(
        &f.db,
        &format!("SELECT id FROM {ENTITY} WHERE entity_sync_id = '{new_sid}'"),
    )
    .await;
    assert_eq!(
        local_approval_seal(&f.db, b_id).await.expect("seal").as_deref(),
        Some(local.as_str())
    );
    assert_eq!(approval_seal(&wire_of(&f, ENTITY, &new_sid).await), local);
}

/// Tests 2, 3, 7: content compared with the decision held here, the seal of a decision new here,
/// and the status never going backwards.
#[tokio::test]
async fn a_received_decision_keeps_its_content_its_seal_and_its_status() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let reviewer_sync = user_sync(&f, f.reviewer).await;

    // Held proposed, received approved with a changed field: refused, whichever field.
    let d = wo_decision(&f, study_id, None).await;
    let sid = decision_sync_id(&f, d.id).await;
    for (key, value) in [
        ("proposed_interval_hours", Value::from(100.0)),
        ("justification", Value::from("autre justification")),
        ("evidence_json", Value::from("{\"n_failures\":99}")),
        ("wo_type_code", Value::from("preventive")),
    ] {
        let mut w = wire_of(&f, ENTITY, &sid).await;
        w.insert("status".into(), "approved".into());
        w.insert("decided_by_sync_id".into(), reviewer_sync.clone().into());
        w.insert(key.into(), value);
        let w = resealed(w);
        let o = apply_wire(&f, ENTITY, &sid, "upsert", &w).await;
        assert_eq!(conflict_code(&o), Some("DECISION_CONTENT_CHANGED"), "{key}: {o:?}");
    }
    assert_eq!(decision_status(&f, d.id).await, "proposed");

    // Held approved, received applied with a changed title: refused.
    let (a, sid) = approved(&f, study_id).await;
    let mut w = wire_of(&f, ENTITY, &sid).await;
    w.insert("status".into(), "applied".into());
    w.insert("applied_by_sync_id".into(), user_sync(&f, f.admin).await.into());
    w.insert("title".into(), "Titre changé".into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &resealed(w)).await;
    assert_eq!(conflict_code(&o), Some("DECISION_CONTENT_CHANGED"), "{o:?}");

    // Held here without the seal key (a sender older than the seal): judged by comparison only.
    let mut old = wire_of(&f, ENTITY, &sid).await;
    old.remove("approval_seal");
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &old).await;
    assert_eq!(conflict_code(&o), None, "{o:?}");
    assert_eq!(deferred_code(&o), None, "{o:?}");

    // New here: title changed after sealing, seal empty, seal missing.
    let wire = wire_of(&f, ENTITY, &sid).await;
    let (n1, mut changed) = as_new(&wire, "changed");
    changed.insert("title".into(), "Changé après approbation".into());
    let o = apply_wire(&f, ENTITY, &n1, "upsert", &changed).await;
    assert_eq!(conflict_code(&o), Some("DECISION_CONTENT_CHANGED"), "{o:?}");
    let (n2, mut unsealed) = as_new(&wire, "unsealed");
    unsealed.insert("approval_seal".into(), Value::Null);
    let o = apply_wire(&f, ENTITY, &n2, "upsert", &unsealed).await;
    assert_eq!(conflict_code(&o), Some("DECISION_APPROVAL_UNSEALED"), "{o:?}");
    let (n3, mut legacy) = as_new(&wire, "legacy");
    legacy.remove("approval_seal");
    let o = apply_wire(&f, ENTITY, &n3, "upsert", &legacy).await;
    assert_eq!(deferred_code(&o), Some("SENDER_OUTDATED"), "{o:?}");

    // Regression: held approved, received proposed; held applied, received approved.
    let mut back = wire_of(&f, ENTITY, &sid).await;
    back.insert("status".into(), "proposed".into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &back).await;
    assert_eq!(conflict_code(&o), Some("DECISION_STATUS_REGRESSION"), "{o:?}");
    let current = crate::reliability::study::decisions::get_decision(&f.db, a.id)
        .await
        .expect("get");
    apply_decision(&f.db, decide(&current, None), f.admin)
        .await
        .expect("apply");
    let mut back = wire_of(&f, ENTITY, &sid).await;
    back.insert("status".into(), "approved".into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &back).await;
    assert_eq!(conflict_code(&o), Some("DECISION_STATUS_REGRESSION"), "{o:?}");
    assert_eq!(decision_status(&f, a.id).await, "applied");
}

/// Tests 4, 5, 6: the designated approver, the approver's right here (waiting while it arrives,
/// closing by itself once granted), an approval accepted here not judged again.
#[tokio::test]
async fn the_approver_is_the_designated_one_and_holds_the_right_here() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let other = third_user(&f).await;
    let other_sync = user_sync(&f, other).await;
    let (a, sid) = approved(&f, study_id).await;
    let wire = wire_of(&f, ENTITY, &sid).await;

    // Designated: the other user; approved by the reviewer.
    let (n, mut p) = as_new(&wire, "designated");
    p.insert("approver_sync_id".into(), other_sync.clone().into());
    let o = apply_wire(&f, ENTITY, &n, "upsert", &resealed(p)).await;
    assert_eq!(conflict_code(&o), Some("DECISION_NOT_DESIGNATED_APPROVER"), "{o:?}");

    // Approved by someone not applied here: waits.
    let (n, mut p) = as_new(&wire, "ghost");
    p.insert("decided_by_sync_id".into(), "ghost-user".into());
    let o = apply_wire(&f, ENTITY, &n, "upsert", &resealed(p)).await;
    assert_eq!(deferred_code(&o), Some("MISSING_PARENT"), "{o:?}");

    // Approved by the other user, who has no ram.manage here: conflict.
    let mut by_other = wire.clone();
    by_other.insert("decided_by_sync_id".into(), other_sync.clone().into());
    let by_other = resealed(by_other);
    let (n, p) = as_new(&by_other, "rights");
    let o = apply_wire(&f, ENTITY, &n, "upsert", &p).await;
    assert_eq!(conflict_code(&o), Some("DECISION_APPROVER_UNAUTHORISED"), "{o:?}");

    // An emergency plant-wide grant does not count.
    let role = scalar(
        &f.db,
        &format!(
            "SELECT rp.role_id FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id \
              WHERE p.name = '{}' ORDER BY rp.role_id LIMIT 1",
            crate::rbac::permissions::RAM_MANAGE
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, is_emergency, \
             created_at, updated_at, row_version) VALUES ('rs-usa-emergency', {other}, {role}, 'tenant', 1, \
             strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)"
        ),
    )
    .await;
    let o = apply_wire(&f, ENTITY, &n, "upsert", &p).await;
    assert_eq!(conflict_code(&o), Some("DECISION_APPROVER_UNAUTHORISED"), "{o:?}");

    // A plant-wide grant still in the inbox: waits.
    let role_sync = text(&f, &format!("SELECT entity_sync_id FROM roles WHERE id = {role}")).await;
    let grant: Payload = serde_json::from_value(serde_json::json!({
        "user_sync_id": other_sync, "role_sync_id": role_sync, "scope_type": "tenant", "is_emergency": 0
    }))
    .expect("grant");
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
             row_version, payload_json, payload_hash, apply_status) VALUES ('b-grant', 'c1', \
             'user_scope_assignments', 'pending-grant', 'upsert', 1, '{}', 'h-grant', 'deferred')",
            serde_json::to_string(&grant).expect("json")
        ),
    )
    .await;
    let o = apply_wire(&f, ENTITY, &n, "upsert", &p).await;
    assert_eq!(deferred_code(&o), Some("MISSING_PARENT"), "{o:?}");
    exec(&f.db, "DELETE FROM sync_inbox WHERE entity_sync_id = 'pending-grant'").await;

    // Through the inbox: a conflict; the right granted here later closes it by itself, audited.
    let (n, p) = as_new(&by_other, "retry");
    inbox(&f, &n, &p, 1).await;
    materialize(&f).await;
    assert_eq!(open_conflict(&f, &n).await.2, "DECISION_APPROVER_UNAUTHORISED");
    assert_eq!(
        retry_decision_conflicts(&f.db).await.expect("retry"),
        0,
        "still refused"
    );
    // The plant-wide grant made a regular one here.
    exec(
        &f.db,
        "UPDATE user_scope_assignments SET is_emergency = 0 WHERE entity_sync_id = 'rs-usa-emergency'",
    )
    .await;
    assert_eq!(retry_decision_conflicts(&f.db).await.expect("retry"), 1);
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM sync_conflicts WHERE entity_sync_id = '{n}' AND status = 'resolved_remote' \
                   AND resolution_action = 'judged_valid'"
            ),
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM {ENTITY} WHERE entity_sync_id = '{n}'")
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = '{CONFLICT_CLOSED_EVENT}' AND target_id = '{n}'"
            ),
        )
        .await,
        1
    );

    // Held approved (accepted here), the reviewer's right withdrawn since, received applied by the
    // admin: the approval is not judged again; the applier is.
    exec(
        &f.db,
        &format!(
            "UPDATE user_scope_assignments SET deleted_at = datetime('now') WHERE user_id = {}",
            f.reviewer
        ),
    )
    .await;
    let mut applied = wire_of(&f, ENTITY, &sid).await;
    applied.insert("status".into(), "applied".into());
    applied.insert("applied_by_sync_id".into(), user_sync(&f, f.reviewer).await.into());
    applied.insert("applied_at".into(), "2026-10-26T09:00:00Z".into());
    // Applied by the reviewer, who holds no right here any more: the applier is refused.
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &applied).await;
    assert_eq!(conflict_code(&o), Some("DECISION_APPLIER_UNAUTHORISED"), "{o:?}");
    applied.insert("applied_by_sync_id".into(), user_sync(&f, f.admin).await.into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &applied).await;
    assert_eq!(conflict_code(&o), None, "{o:?}");
    assert_eq!(decision_status(&f, a.id).await, "applied");
}

/// Tests 9, 10 and Q6: only "keep this PC's decision" and "escalate"; keeping marks the decision
/// refused here (or stores it refused when new here), sends it, audits it; the marks arriving on
/// another PC block apply and publish there; a work order created from it is kept and told of.
#[tokio::test]
async fn keeping_this_pc_decision_refuses_it_everywhere_without_deleting_anything() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (a, sid) = approved(&f, study_id).await;
    let wire = wire_of(&f, ENTITY, &sid).await;

    // New here, content changed after sealing: through the inbox, a conflict.
    let (n, mut p) = as_new(&wire, "keep-new");
    p.insert("title".into(), "Changé".into());
    p.insert("linked_work_order_sync_id".into(), Value::Null);
    inbox(&f, &n, &p, 3).await;
    materialize(&f).await;
    assert_eq!(open_conflict(&f, &n).await.2, "DECISION_CONTENT_CHANGED");
    for action in ["accept_remote", "dismiss"] {
        assert_eq!(
            refusal_code(resolve(&f, f.reviewer, &n, action).await),
            "GATE_SYNC_CONFLICT_DECISION_KEEP_ONLY",
            "{action}"
        );
    }
    // Keeping needs ram.manage.
    let other = third_user(&f).await;
    assert_eq!(
        refusal_code(resolve(&f, other, &n, "accept_local").await),
        "GATE_SYNC_KEEP_DECISION_NOT_ALLOWED"
    );
    resolve(&f, f.reviewer, &n, "accept_local").await.expect("keep");
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM {ENTITY} WHERE entity_sync_id = '{n}' AND sync_refused_at IS NOT NULL \
                   AND sync_refused_reason LIKE 'refusé (sync) : contenu modifié%' AND linked_work_order_id IS NULL"
            ),
        )
        .await,
        1,
        "stored refused, without links"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = '{n}' \
                   AND json_extract(payload_json, '$.sync_refused_at') IS NOT NULL"
            ),
        )
        .await,
        1,
        "sent with its mark"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = '{REFUSED_EVENT}' AND target_id = '{n}'"),
        )
        .await,
        1
    );
    let refused_id = scalar(&f.db, &format!("SELECT id FROM {ENTITY} WHERE entity_sync_id = '{n}'")).await;
    let refused = crate::reliability::study::decisions::get_decision(&f.db, refused_id)
        .await
        .expect("get");
    assert_eq!(
        refusal_code(apply_decision(&f.db, decide(&refused, None), f.admin).await),
        "GATE_REL_DECISION_SYNC_REFUSED"
    );

    // Held here and applied (its work order created here); the refusal marks arrive from another
    // PC: taken, the status and links held here kept, the work order kept, its planner and the
    // decision's proposer told.
    let current = crate::reliability::study::decisions::get_decision(&f.db, a.id)
        .await
        .expect("get");
    let applied = apply_decision(&f.db, decide(&current, None), f.admin)
        .await
        .expect("apply");
    let wo = applied.linked_work_order_id.expect("work order");
    let mut marked = wire.clone();
    marked.insert("sync_refused_at".into(), "2026-10-26T10:00:00Z".into());
    marked.insert(
        "sync_refused_reason".into(),
        "refusé (sync) : contenu modifié après approbation — test".into(),
    );
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &marked).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    assert_eq!(
        decision_status(&f, a.id).await,
        "applied",
        "the status held here is kept"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM {ENTITY} WHERE id = {} AND sync_refused_at IS NOT NULL AND linked_work_order_id = {wo}", a.id),
        )
        .await,
        1
    );
    assert_eq!(
        scalar(&f.db, &format!("SELECT COUNT(*) FROM work_orders WHERE id = {wo}")).await,
        1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
                  WHERE e.category_code = 'rel_decision_refused_wo_kept' AND n.recipient_user_id = {}",
                f.admin
            ),
        )
        .await,
        1,
        "the proposer is told"
    );

    // A received version clearing the mark: refused.
    let unmarked = wire_of(&f, ENTITY, &sid).await;
    let mut cleared = unmarked.clone();
    cleared.insert("sync_refused_at".into(), Value::Null);
    cleared.insert("sync_refused_reason".into(), Value::Null);
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &cleared).await;
    assert_eq!(conflict_code(&o), Some("DECISION_STATUS_REGRESSION"), "{o:?}");

    // Held approved, the marks arrive: it can no longer be applied here.
    let (b, bsid) = approved(&f, study_id).await;
    let mut marked = wire_of(&f, ENTITY, &bsid).await;
    marked.insert("sync_refused_at".into(), "2026-10-26T10:00:00Z".into());
    marked.insert("sync_refused_reason".into(), "refusé (sync) : test".into());
    let o = apply_wire(&f, ENTITY, &bsid, "upsert", &marked).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let b = crate::reliability::study::decisions::get_decision(&f.db, b.id)
        .await
        .expect("get");
    assert!(b.sync_refused_at.is_some());
    assert_eq!(
        refusal_code(apply_decision(&f.db, decide(&b, None), f.admin).await),
        "GATE_REL_DECISION_SYNC_REFUSED"
    );

    // Held here, a received version with a changed content kept as this PC's: the held decision is
    // marked refused (not deleted) and sent.
    let (c, csid) = approved(&f, study_id).await;
    let mut changed = wire_of(&f, ENTITY, &csid).await;
    changed.insert("justification".into(), "autre".into());
    inbox(&f, &csid, &resealed(changed), 9).await;
    materialize(&f).await;
    assert_eq!(open_conflict(&f, &csid).await.2, "DECISION_CONTENT_CHANGED");
    resolve(&f, f.reviewer, &csid, "accept_local").await.expect("keep held");
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM {ENTITY} WHERE id = {} AND sync_refused_at IS NOT NULL AND status = 'approved'",
                c.id
            ),
        )
        .await,
        1
    );
}

/// Test 10: a PM version from a decision refused on another PC (its marks received here) is never
/// published here.
#[tokio::test]
async fn a_pm_version_of_a_decision_refused_elsewhere_is_never_published() {
    use crate::pm::domain::PublishPmPlanVersionInput;
    use crate::pm::queries::{create_pm_plan, create_pm_plan_version, publish_pm_plan_version};
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let plan = create_pm_plan(
        &f.db,
        CreatePmPlanInput {
            code: "PM-ST-76B".into(),
            title: "Roulement pompe".into(),
            description: None,
            asset_scope_type: "equipment".into(),
            asset_scope_id: Some(f.p101),
            strategy_type: "fixed".into(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("plan");
    create_pm_plan_version(
        &f.db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: "2025-01-01T00:00:00Z".into(),
            effective_to: None,
            trigger_definition_json: r#"{"interval_unit":"week","interval_value":36}"#.into(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".into()),
            required_tools_json: None,
            estimated_duration_hours: Some(3.0),
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("version 1");
    exec(
        &f.db,
        &format!(
            "UPDATE pm_plans SET lifecycle_status = 'approved' WHERE id = {}",
            plan.id
        ),
    )
    .await;
    let d = pm_decision(&f, study_id, plan.id).await;
    let a = approve_decision(&f.db, decide(&d, None), f.reviewer)
        .await
        .expect("approve");
    let applied = apply_decision(&f.db, decide(&a, None), f.admin).await.expect("apply");
    let version = applied.linked_pm_plan_version_id.expect("draft");
    let sid = decision_sync_id(&f, d.id).await;
    let mut marked = wire_of(&f, ENTITY, &sid).await;
    marked.insert("sync_refused_at".into(), "2026-10-26T10:00:00Z".into());
    marked.insert("sync_refused_reason".into(), "refusé (sync) : test".into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &marked).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let rv = scalar(
        &f.db,
        &format!("SELECT row_version FROM pm_plan_versions WHERE id = {version}"),
    )
    .await;
    assert_eq!(
        refusal_code(
            publish_pm_plan_version(
                &f.db,
                PublishPmPlanVersionInput {
                    version_id: version,
                    expected_row_version: rv,
                },
            )
            .await
        ),
        "GATE_PM_SOURCE_DECISION_REFUSED"
    );
}

/// Q2: a decision approved by a PC that does not seal yet waits, listed in the sync centre with its
/// own reason (the sending PC's update).
#[tokio::test]
async fn an_unsealed_approval_from_an_older_pc_waits_visibly() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (_, sid) = approved(&f, study_id).await;
    let (n, mut p) = as_new(&wire_of(&f, ENTITY, &sid).await, "older-pc");
    p.remove("approval_seal");
    p.remove("sync_refused_at");
    p.remove("sync_refused_reason");
    inbox(&f, &n, &p, 1).await;
    materialize(&f).await;
    let report = crate::sync::waiting::list_waiting_inbox_rows(&f.db, None)
        .await
        .expect("waiting");
    let row = report
        .rows
        .iter()
        .find(|r| r.entity_sync_id == n)
        .expect("listed as waiting");
    assert_eq!(row.reason_code.as_deref(), Some("SENDER_OUTDATED"));
    assert_eq!(row.target.state, crate::sync::waiting::WAIT_OWN_REASON);
    assert_eq!(row.target.reason_code.as_deref(), Some("SENDER_OUTDATED"));
}

/// Test 8 and review of #179: the backfill of migration 264 seals approved and applied decisions
/// only, with the seal approval computes; withdraw and reject audits name sync ids.
#[tokio::test]
async fn the_backfill_seals_decided_decisions_and_audits_name_sync_ids() {
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (a, _) = approved(&f, study_id).await;
    let sealed = text(&f, &format!("SELECT approval_seal FROM {ENTITY} WHERE id = {}", a.id)).await;
    let proposed = wo_decision(&f, study_id, None).await;
    let rejected = wo_decision(&f, study_id, None).await;
    reject_decision(&f.db, decide(&rejected, Some("Pas maintenant, voir arrêt")), f.reviewer)
        .await
        .expect("reject");
    exec(&f.db, &format!("UPDATE {ENTITY} SET approval_seal = NULL")).await;
    let manager = SchemaManager::new(&f.db);
    crate::migrations::m20261026_000264_reliability_decision_seal::Migration
        .up(&manager)
        .await
        .expect("migration 264");
    assert_eq!(
        text(&f, &format!("SELECT approval_seal FROM {ENTITY} WHERE id = {}", a.id)).await,
        sealed
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM {ENTITY} WHERE id IN ({}, {}) AND approval_seal IS NULL",
                proposed.id, rejected.id
            ),
        )
        .await,
        2
    );
    let details = text(
        &f,
        "SELECT details_json FROM audit_events WHERE action_code = 'reliability.decision.rejected' ORDER BY id DESC LIMIT 1",
    )
    .await;
    let details: Value = serde_json::from_str(&details).expect("json");
    assert_eq!(
        details["decision_sync_id"],
        Value::String(decision_sync_id(&f, rejected.id).await)
    );
    assert!(details["study_sync_id"].is_string());
    for local in ["decision_id", "study_id", "proposed_by_id", "designated_approver_id"] {
        assert!(details.get(local).is_none(), "{local} is a local id");
    }
}

// ── Review of AU-76b (B1, S1-S4, S7) ──────────────────────────────────────────────────────────

/// A payload as another PC receives it: written to JSON text and parsed back (the real wire path).
fn through_text(p: &Payload) -> Payload {
    serde_json::from_str(&serde_json::to_string(p).expect("json")).expect("parse")
}

/// A one-off work-order decision proposed by the admin with these study baselines (as computed by
/// a real study: full-precision hours), approved by the reviewer.
async fn approved_with_baselines(f: &Fixture, study_id: i64, mtbf: f64, exposure: f64) -> (i64, String) {
    let d = wo_decision(f, study_id, None).await;
    exec(
        &f.db,
        &format!(
            "UPDATE {ENTITY} SET baseline_mtbf_hours = {mtbf:?}, baseline_exposure_hours = {exposure:?} \
             WHERE id = {}",
            d.id
        ),
    )
    .await;
    let current = crate::reliability::study::decisions::get_decision(&f.db, d.id)
        .await
        .expect("get");
    let a = approve_decision(&f.db, decide(&current, None), f.reviewer)
        .await
        .expect("approve");
    (a.id, decision_sync_id(f, a.id).await)
}

/// B1: full-precision baselines survive the JSON text of sync: a decision new here and a decision
/// held here are both accepted, never refused as changed.
#[tokio::test]
async fn full_precision_baselines_keep_the_seal_through_json_text() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let values = [
        (181_716.114_897_673_02, 2_725_741.723_464_595_3),
        (8_760.0 / 3.0, 26_280.0 / 7.0),
        (1_234.567_890_123_456_7, 98_765.432_109_876_54),
        (100.1 * 3.0, 0.1 + 0.2),
        (54_321.987_654_321_01 / 11.0, 7.0 / 9.0),
    ];
    for (n, (mtbf, exposure)) in values.into_iter().enumerate() {
        let (_, sid) = approved_with_baselines(&f, study_id, mtbf, exposure).await;
        let wire = wire_of(&f, ENTITY, &sid).await;
        // Held here: received again through its text form.
        let o = apply_wire(&f, ENTITY, &sid, "upsert", &through_text(&wire)).await;
        assert_eq!(conflict_code(&o), None, "held, {mtbf}: {o:?}");
        // New here: the same decision as another PC first receives it.
        let (new_sid, p) = as_new(&wire, &format!("text-{n}"));
        let o = apply_wire(&f, ENTITY, &new_sid, "upsert", &through_text(&p)).await;
        assert!(matches!(o, ApplyOutcome::Applied), "new here, {mtbf}: {o:?}");
        // The seal recomputed on the receiving side is the one sent.
        let id = scalar(
            &f.db,
            &format!("SELECT id FROM {ENTITY} WHERE entity_sync_id = '{new_sid}'"),
        )
        .await;
        assert_eq!(
            local_approval_seal(&f.db, id).await.expect("seal").as_deref(),
            wire.get("approval_seal").and_then(Value::as_str)
        );
    }
}

/// S1 and the empty mark: an unknown status never overwrites the status held here; an empty refusal
/// mark counts as a mark, as the gates read it.
#[tokio::test]
async fn an_unknown_status_is_refused_and_an_empty_mark_is_a_mark() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (a, sid) = approved(&f, study_id).await;
    let current = crate::reliability::study::decisions::get_decision(&f.db, a.id)
        .await
        .expect("get");
    apply_decision(&f.db, decide(&current, None), f.admin)
        .await
        .expect("apply");
    for status in [
        Value::from("Applied"),
        Value::from("applied "),
        Value::from("x"),
        Value::from(2),
    ] {
        let mut w = wire_of(&f, ENTITY, &sid).await;
        w.insert("status".into(), status.clone());
        let o = apply_wire(&f, ENTITY, &sid, "upsert", &w).await;
        assert_eq!(conflict_code(&o), Some("DECISION_STATUS_REGRESSION"), "{status}: {o:?}");
    }
    assert_eq!(decision_status(&f, a.id).await, "applied");

    let (b, bsid) = approved(&f, study_id).await;
    let mut w = wire_of(&f, ENTITY, &bsid).await;
    w.insert("sync_refused_at".into(), "".into());
    let o = apply_wire(&f, ENTITY, &bsid, "upsert", &w).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let b = crate::reliability::study::decisions::get_decision(&f.db, b.id)
        .await
        .expect("get");
    assert_eq!(
        refusal_code(apply_decision(&f.db, decide(&b, None), f.admin).await),
        "GATE_REL_DECISION_SYNC_REFUSED"
    );
    // Received without it afterwards: the mark is never cleared.
    let mut cleared = wire_of(&f, ENTITY, &bsid).await;
    cleared.insert("sync_refused_at".into(), Value::Null);
    let o = apply_wire(&f, ENTITY, &bsid, "upsert", &cleared).await;
    assert_eq!(conflict_code(&o), Some("DECISION_STATUS_REGRESSION"), "{o:?}");
}

/// S2: an approval from a PC that does not seal, accepted here over the proposal held here, is
/// sealed here, so this PC never sends it unsealed.
#[tokio::test]
async fn an_unsealed_approval_accepted_over_the_proposal_is_sealed_here() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let d = wo_decision(&f, study_id, None).await;
    let sid = decision_sync_id(&f, d.id).await;
    let mut w = wire_of(&f, ENTITY, &sid).await;
    w.insert("status".into(), "approved".into());
    w.insert("decided_by_sync_id".into(), user_sync(&f, f.reviewer).await.into());
    w.insert("decided_at".into(), "2026-10-26T08:00:00+00:00".into());
    w.remove("approval_seal");
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &through_text(&w)).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let stored = text(&f, &format!("SELECT approval_seal FROM {ENTITY} WHERE id = {}", d.id)).await;
    assert_eq!(
        local_approval_seal(&f.db, d.id).await.expect("seal").as_deref(),
        Some(stored.as_str())
    );
    // Sent from here, it carries the seal another upgraded PC accepts.
    let sent = wire_of(&f, ENTITY, &sid).await;
    assert_eq!(sent.get("approval_seal").and_then(Value::as_str), Some(stored.as_str()));
    let (n, p) = as_new(&sent, "resealed");
    let o = apply_wire(&f, ENTITY, &n, "upsert", &through_text(&p)).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
}

/// S3: the work order is held here but the decision is new here: a refused decision arriving, or
/// one refused here by "keep this PC's decision", tells the work order's people.
#[tokio::test]
async fn a_refused_decision_new_here_tells_of_the_work_order_held_here() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (a, sid) = approved(&f, study_id).await;
    let current = crate::reliability::study::decisions::get_decision(&f.db, a.id)
        .await
        .expect("get");
    let applied = apply_decision(&f.db, decide(&current, None), f.admin)
        .await
        .expect("apply");
    let wo = applied.linked_work_order_id.expect("work order");
    let wire = wire_of(&f, ENTITY, &sid).await;
    let notices = |decision_sid: String| {
        let db = &f.db;
        async move {
            let event = crate::sync::identity::seeded_entity_sync_id(
                "notification_events",
                &format!("rel_decision_refused_wo_kept|{decision_sid}"),
            );
            scalar(
                db,
                &format!(
                    "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
                      WHERE e.entity_sync_id = '{event}'"
                ),
            )
            .await
        }
    };

    // Arriving already refused, new here, naming the work order held here.
    let (n, mut p) = as_new(&wire, "marked-new");
    p.insert("sync_refused_at".into(), "2026-10-26T10:00:00Z".into());
    p.insert("sync_refused_reason".into(), "refusé (sync) : test".into());
    let o = apply_wire(&f, ENTITY, &n, "upsert", &p).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    assert!(notices(n.clone()).await >= 1, "the proposer is told");

    // Refused here by "keep this PC's decision" while new here: links cleared, the people told.
    let (k, mut q) = as_new(&wire, "keep-new-wo");
    q.insert("title".into(), "Changé".into());
    inbox(&f, &k, &q, 2).await;
    materialize(&f).await;
    assert_eq!(open_conflict(&f, &k).await.2, "DECISION_CONTENT_CHANGED");
    resolve(&f, f.reviewer, &k, "accept_local").await.expect("keep");
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM {ENTITY} WHERE entity_sync_id = '{k}' AND linked_work_order_id IS NULL")
        )
        .await,
        1
    );
    assert!(notices(k).await >= 1, "the proposer is told of the work order kept");
    assert_eq!(
        scalar(&f.db, &format!("SELECT COUNT(*) FROM work_orders WHERE id = {wo}")).await,
        1
    );
}

/// A plan with one published version of P-101, approved: ready for a decision's PM revision.
async fn approved_plan(f: &Fixture, code: &str) -> i64 {
    use crate::pm::queries::{create_pm_plan, create_pm_plan_version};
    let plan = create_pm_plan(
        &f.db,
        CreatePmPlanInput {
            code: code.into(),
            title: "Roulement pompe".into(),
            description: None,
            asset_scope_type: "equipment".into(),
            asset_scope_id: Some(f.p101),
            strategy_type: "fixed".into(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("plan");
    create_pm_plan_version(&f.db, plan.id, pm_version_input())
        .await
        .expect("version 1");
    exec(
        &f.db,
        &format!(
            "UPDATE pm_plans SET lifecycle_status = 'approved' WHERE id = {}",
            plan.id
        ),
    )
    .await;
    plan.id
}

fn pm_version_input() -> CreatePmPlanVersionInput {
    CreatePmPlanVersionInput {
        effective_from: "2025-01-01T00:00:00Z".into(),
        effective_to: None,
        trigger_definition_json: r#"{"interval_unit":"week","interval_value":36}"#.into(),
        task_package_json: None,
        required_parts_json: None,
        required_skills_json: Some("[]".into()),
        required_tools_json: None,
        estimated_duration_hours: Some(3.0),
        planned_crew_size: None,
        estimated_labor_cost: None,
        estimated_parts_cost: None,
        estimated_service_cost: None,
        change_reason: None,
    }
}

async fn publish(f: &Fixture, version: i64) -> AppResult<crate::pm::domain::PmPlanVersion> {
    let rv = scalar(
        &f.db,
        &format!("SELECT row_version FROM pm_plan_versions WHERE id = {version}"),
    )
    .await;
    crate::pm::queries::publish_pm_plan_version(
        &f.db,
        crate::pm::domain::PublishPmPlanVersionInput {
            version_id: version,
            expected_row_version: rv,
        },
    )
    .await
}

/// S4 (CMMS Advisor, option A): a PM version already published from a decision refused since keeps
/// running; its planners and the decision's proposer are told, naming the version; the plan shows
/// "to review" while its current version comes from a decision refused or not accepted here, until
/// another version is published.
#[tokio::test]
async fn a_published_version_of_a_refused_decision_runs_flagged_and_told() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let plan = approved_plan(&f, "PM-ST-76B-S4").await;
    let flag = |plan: i64| {
        let db = &f.db;
        async move {
            crate::pm::queries::get_pm_plan(db, plan)
                .await
                .expect("plan")
                .source_decision_to_review
        }
    };
    let d = pm_decision(&f, study_id, plan).await;
    let a = approve_decision(&f.db, decide(&d, None), f.reviewer)
        .await
        .expect("approve");
    let applied = apply_decision(&f.db, decide(&a, None), f.admin).await.expect("apply");
    let version = applied.linked_pm_plan_version_id.expect("draft");
    publish(&f, version).await.expect("published while accepted");
    assert!(!flag(plan).await, "accepted here: nothing to review");

    // The refusal arrives from another PC: the version stays published, flagged, and told of.
    let sid = decision_sync_id(&f, d.id).await;
    let mut marked = wire_of(&f, ENTITY, &sid).await;
    marked.insert("sync_refused_at".into(), "2026-10-26T10:00:00Z".into());
    marked.insert("sync_refused_reason".into(), "refusé (sync) : test".into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &marked).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    assert_eq!(
        text(&f, &format!("SELECT status FROM pm_plan_versions WHERE id = {version}")).await,
        "published"
    );
    assert!(flag(plan).await, "refused: to review");
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
                  WHERE e.category_code = 'rel_decision_refused_wo_kept' AND e.message_key = 'rel_decision_refused_pm_kept' \
                    AND n.recipient_user_id = {}",
                f.admin
            ),
        )
        .await,
        1,
        "the proposer is told, the version named"
    );

    // A planner publishes another version (normal PM versioning): the flag clears.
    let next = crate::pm::queries::create_pm_plan_version(&f.db, plan, pm_version_input())
        .await
        .expect("next version");
    publish(&f, next.id).await.expect("publish another version");
    assert!(!flag(plan).await, "another version in force");

    // A version received published from a decision not here (waiting): flagged too.
    let other = approved_plan(&f, "PM-ST-76B-WAIT").await;
    let foreign = crate::pm::queries::create_pm_plan_version(&f.db, other, pm_version_input())
        .await
        .expect("draft");
    exec(
        &f.db,
        &format!(
            "UPDATE pm_plan_versions SET status = 'published', source_type = 'reliability_study_decision', \
             source_id = NULL WHERE id = {}",
            foreign.id
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "UPDATE pm_plans SET current_version_id = {} WHERE id = {other}",
            foreign.id
        ),
    )
    .await;
    assert!(flag(other).await, "decision not accepted here: to review");
}

/// S7: the backfill re-sends a sealed decision once from the PC whose version the server holds (or
/// that has not sent it yet), not from the others; a sealed version applied here closes the
/// unsealed version that waited for it.
#[tokio::test]
async fn the_backfill_resends_from_the_last_writer_and_the_wait_closes() {
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (_, mine) = approved(&f, study_id).await;
    let (_, theirs) = approved(&f, study_id).await;
    let (_, unsent) = approved(&f, study_id).await;
    exec(&f.db, &format!("UPDATE {ENTITY} SET approval_seal = NULL")).await;
    exec(&f.db, "DELETE FROM sync_capture_journal").await;
    exec(&f.db, "DELETE FROM sync_outbox").await;
    for (sid, source) in [(&mine, "outbox_ack"), (&theirs, "inbound_apply")] {
        exec(
            &f.db,
            &format!(
                "INSERT OR REPLACE INTO sync_entity_state (entity_type, entity_sync_id, synced_row_version, \
                 synced_payload_hash, source) VALUES ('{ENTITY}', '{sid}', 3, 'h', '{source}')"
            ),
        )
        .await;
    }
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, \
             payload_json, payload_hash, status) VALUES ('k-unsent', '{ENTITY}', '{unsent}', 'upsert', 1, '{{}}', \
             'h', 'pending')"
        ),
    )
    .await;
    crate::migrations::m20261026_000264_reliability_decision_seal::Migration
        .up(&SchemaManager::new(&f.db))
        .await
        .expect("migration 264");
    let journaled = |sid: String| {
        let db = &f.db;
        async move {
            scalar(
                db,
                &format!(
                    "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = '{ENTITY}' \
                       AND entity_sync_id = '{sid}' AND operation = 'upsert'"
                ),
            )
            .await
        }
    };
    // Nothing is sent at the upgrade: before a pull, "my version is the server's" may be stale.
    assert_eq!(
        journaled(mine.clone()).await,
        0,
        "nothing sent before the first complete pull"
    );
    // A change not sent yet is kept, the sealed row journaled on top of it (review R3-B1).
    assert_eq!(
        journaled(unsent.clone()).await,
        1,
        "journaled on top of its unsent change"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = '{unsent}' AND status = 'pending'")
        )
        .await,
        1,
        "its unsent change is kept"
    );
    crate::sync::capture::drain_capture_journal(&f.db)
        .await
        .expect("drain at start-up");
    // After the first complete pull: the last writer re-sends it sealed.
    assert_eq!(resend_sealed_after_complete_pull(&f.db).await.expect("resend"), 1);
    let sent_sealed = |sid: String| {
        let db = &f.db;
        async move {
            scalar(
                db,
                &format!(
                    "SELECT COUNT(*) FROM sync_outbox WHERE entity_type = '{ENTITY}' AND entity_sync_id = '{sid}' AND json_extract(payload_json, '$.approval_seal') LIKE 'v1:%'"
                ),
            )
            .await
        }
    };
    assert_eq!(sent_sealed(mine.clone()).await, 1, "the last writer re-sends it");
    assert_eq!(sent_sealed(theirs.clone()).await, 0, "a receiver does not");
    assert_eq!(
        sent_sealed(unsent.clone()).await,
        1,
        "the unsent change is followed by its sealed version"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM system_config WHERE key = '{SEAL_RESEND_KEY}'")
        )
        .await,
        0,
        "judged once"
    );
    assert_eq!(resend_sealed_after_complete_pull(&f.db).await.expect("again"), 0);

    // On a receiver: the unsealed version waits, the sealed one arrives and closes the wait.
    let wire = wire_of(&f, ENTITY, &mine).await;
    let (n, mut legacy) = as_new(&wire, "legacy-wait");
    legacy.remove("approval_seal");
    inbox(&f, &n, &legacy, 1).await;
    materialize(&f).await;
    assert_eq!(
        text(
            &f,
            &format!("SELECT rejection_code FROM sync_inbox WHERE entity_sync_id = '{n}'")
        )
        .await,
        "SENDER_OUTDATED"
    );
    inbox(&f, &n, &wire, 2).await;
    materialize(&f).await;
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM {ENTITY} WHERE entity_sync_id = '{n}'")
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM sync_inbox WHERE entity_sync_id = '{n}' AND apply_status IN ('deferred', 'conflict')"
            ),
        )
        .await,
        0,
        "the wait closed, no stale-version conflict"
    );
}

// ── Review round 2 (R2-S8, R2-N3, R2-N4) ──────────────────────────────────────────────────────

/// R2-S8: PC B approved the decision (v2, acknowledged), PC A applied it since (v3); B is upgraded
/// before it pulls v3. B sends nothing at the upgrade; its first complete pull brings v3, B ends
/// applied, and B re-sends nothing: no conflict, and B cannot apply it a second time.
#[tokio::test]
async fn a_stale_last_writer_upgraded_before_its_pull_resends_nothing() {
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (b, sid) = approved(&f, study_id).await;
    // As on PC B before the upgrade: approved v2, acknowledged by the server, nothing unsent.
    exec(&f.db, &format!("UPDATE {ENTITY} SET approval_seal = NULL")).await;
    exec(&f.db, "DELETE FROM sync_capture_journal").await;
    exec(&f.db, "DELETE FROM sync_outbox").await;
    exec(
        &f.db,
        &format!(
            "INSERT OR REPLACE INTO sync_entity_state (entity_type, entity_sync_id, synced_row_version, synced_payload_hash, source) VALUES ('{ENTITY}', '{sid}', 2, 'h-v2', 'outbox_ack')"
        ),
    )
    .await;
    crate::migrations::m20261026_000264_reliability_decision_seal::Migration
        .up(&SchemaManager::new(&f.db))
        .await
        .expect("migration 264");
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = '{sid}'")
        )
        .await,
        0,
        "nothing sent at the upgrade"
    );
    // The first pull brings PC A's v3: applied by the admin (sealed by A's upgrade).
    let mut v3 = wire_of(&f, ENTITY, &sid).await;
    v3.insert("status".into(), "applied".into());
    v3.insert("applied_by_sync_id".into(), user_sync(&f, f.admin).await.into());
    v3.insert("applied_at".into(), "2026-10-26T09:00:00Z".into());
    inbox(&f, &sid, &through_text(&v3), 3).await;
    materialize(&f).await;
    assert_eq!(decision_status(&f, b.id).await, "applied");
    // After the complete pull: B is no longer the last writer, it re-sends nothing.
    assert_eq!(resend_sealed_after_complete_pull(&f.db).await.expect("resend"), 0);
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = '{sid}'")
        )
        .await,
        0,
        "no re-send, so no conflict"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM sync_conflicts WHERE entity_sync_id = '{sid}'")
        )
        .await,
        0
    );
    let held = crate::reliability::study::decisions::get_decision(&f.db, b.id)
        .await
        .expect("get");
    assert_eq!(
        refusal_code(apply_decision(&f.db, decide(&held, None), f.admin).await),
        "DECISION_NOT_APPROVED",
        "no second work order"
    );
}

/// R2-N3: "keep this PC's decision" on a decision new here with an unknown status says to escalate.
#[tokio::test]
async fn keeping_a_new_decision_with_an_unknown_status_says_to_escalate() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (_, sid) = approved(&f, study_id).await;
    let (n, mut p) = as_new(&wire_of(&f, ENTITY, &sid).await, "unknown-status");
    p.insert("status".into(), "Applied".into());
    inbox(&f, &n, &p, 1).await;
    materialize(&f).await;
    assert_eq!(open_conflict(&f, &n).await.2, "DECISION_STATUS_REGRESSION");
    assert_eq!(
        refusal_code(resolve(&f, f.reviewer, &n, "accept_local").await),
        "GATE_SYNC_KEEP_DECISION_UNKNOWN_STATUS"
    );
    resolve(&f, f.reviewer, &n, "escalate").await.expect("escalate");
}

/// R2-N4: a PM version received published after its decision was already refused here is told of.
#[tokio::test]
async fn a_version_published_after_the_refusal_arrived_is_told_of() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let plan = approved_plan(&f, "PM-ST-76B-N4").await;
    let d = pm_decision(&f, study_id, plan).await;
    let a = approve_decision(&f.db, decide(&d, None), f.reviewer)
        .await
        .expect("approve");
    let applied = apply_decision(&f.db, decide(&a, None), f.admin).await.expect("apply");
    let version = applied.linked_pm_plan_version_id.expect("draft");
    // The refusal arrives first, the version still a draft here: no notice yet.
    let sid = decision_sync_id(&f, d.id).await;
    let mut marked = wire_of(&f, ENTITY, &sid).await;
    marked.insert("sync_refused_at".into(), "2026-10-26T10:00:00Z".into());
    marked.insert("sync_refused_reason".into(), "refusé (sync) : test".into());
    let o = apply_wire(&f, ENTITY, &sid, "upsert", &marked).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let told = || {
        let db = &f.db;
        let admin = f.admin;
        async move {
            scalar(
                db,
                &format!(
                    "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id WHERE e.message_key = 'rel_decision_refused_pm_kept' AND n.recipient_user_id = {admin}"
                ),
            )
            .await
        }
    };
    assert_eq!(told().await, 0, "a draft runs nothing");
    // Then the version arrives published (published on the PC that applied it before the refusal).
    let version_sid = text(
        &f,
        &format!("SELECT entity_sync_id FROM pm_plan_versions WHERE id = {version}"),
    )
    .await;
    let mut published = wire_of(&f, "pm_plan_versions", &version_sid).await;
    published.insert("status".into(), "published".into());
    let o = apply_wire(&f, "pm_plan_versions", &version_sid, "upsert", &published).await;
    assert_eq!(conflict_code(&o), None, "{o:?}");
    assert_eq!(told().await, 1, "the proposer is told, the version named");
}

// ── Fix check of round 2 (R3-B1, R3-N1) ───────────────────────────────────────────────────────

/// R3-B1: a decision received from another PC (inbound_apply), then applied here and not sent yet
/// at the upgrade: its change is kept and followed by its sealed version, which another PC accepts
/// applied, so no PC can apply it a second time. R3-N1: a listed decision with a newer received
/// version not settled here is not re-sent.
#[tokio::test]
async fn an_unsent_local_change_is_kept_and_sent_sealed_after_the_upgrade() {
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (a, sid) = approved(&f, study_id).await;
    let current = crate::reliability::study::decisions::get_decision(&f.db, a.id)
        .await
        .expect("get");
    apply_decision(&f.db, decide(&current, None), f.admin)
        .await
        .expect("apply here");
    let (_, other) = approved(&f, study_id).await;
    // As before the upgrade: the decision was received (v2), applied here, that change not sent.
    exec(&f.db, &format!("UPDATE {ENTITY} SET approval_seal = NULL")).await;
    exec(&f.db, "DELETE FROM sync_capture_journal").await;
    exec(&f.db, "DELETE FROM sync_outbox").await;
    for (s, source) in [(&sid, "inbound_apply"), (&other, "outbox_ack")] {
        exec(
            &f.db,
            &format!(
                "INSERT OR REPLACE INTO sync_entity_state (entity_type, entity_sync_id, synced_row_version, synced_payload_hash, source) VALUES ('{ENTITY}', '{s}', 2, 'h-v2', '{source}')"
            ),
        )
        .await;
    }
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, base_row_version, payload_json, payload_hash, status) VALUES ('k-applied', '{ENTITY}', '{sid}', 'upsert', 3, 2, '{{\"status\":\"applied\"}}', 'h-v3', 'pending')"
        ),
    )
    .await;
    crate::migrations::m20261026_000264_reliability_decision_seal::Migration
        .up(&SchemaManager::new(&f.db))
        .await
        .expect("migration 264");
    crate::sync::capture::drain_capture_journal(&f.db)
        .await
        .expect("drain at start-up");
    let sends: Vec<(i64, Option<i64>, String)> = f
        .db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT row_version, base_row_version, payload_json FROM sync_outbox WHERE entity_sync_id = '{sid}' ORDER BY row_version"
            ),
        ))
        .await
        .expect("q")
        .iter()
        .map(|r| {
            (
                r.try_get("", "row_version").expect("rv"),
                r.try_get("", "base_row_version").expect("base"),
                r.try_get("", "payload_json").expect("payload"),
            )
        })
        .collect();
    assert_eq!(
        sends.len(),
        2,
        "the local change is kept, its sealed version follows: {sends:?}"
    );
    assert_eq!((sends[1].0, sends[1].1), (4, Some(3)), "stacked on the unsent change");
    let sealed: Payload = serde_json::from_str(&sends[1].2).expect("payload");
    assert_eq!(sealed.get("status").and_then(Value::as_str), Some("applied"));
    assert!(sealed
        .get("approval_seal")
        .and_then(Value::as_str)
        .is_some_and(|s| s.starts_with("v1:")));
    // Another PC (new there) accepts it applied: it can never apply it a second time.
    let (n, p) = as_new(&sealed, "other-pc");
    let o = apply_wire(&f, ENTITY, &n, "upsert", &through_text(&p)).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let there = scalar(&f.db, &format!("SELECT id FROM {ENTITY} WHERE entity_sync_id = '{n}'")).await;
    let there = crate::reliability::study::decisions::get_decision(&f.db, there)
        .await
        .expect("get");
    assert_eq!(there.status, "applied");
    assert_eq!(
        refusal_code(apply_decision(&f.db, decide(&there, None), f.admin).await),
        "DECISION_NOT_APPROVED"
    );

    // R3-N1: the other listed decision has a newer version waiting in the inbox: not re-sent.
    let mut newer = wire_of(&f, ENTITY, &other).await;
    newer.insert("decision_note".into(), "autre".into());
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, row_version, payload_json, payload_hash, apply_status) VALUES ('b-newer', 'c1', '{ENTITY}', '{other}', 'upsert', 3, '{}', 'h-newer', 'deferred')",
            serde_json::to_string(&newer).expect("json").replace('\'', "''")
        ),
    )
    .await;
    assert_eq!(resend_sealed_after_complete_pull(&f.db).await.expect("resend"), 0);
}

// ── Review round 3 (R3-S9, R3-N6) ─────────────────────────────────────────────────────────────

/// The unsent sends of a decision: (row version, base, status, payload).
async fn sends_of(f: &Fixture, sid: &str) -> Vec<(i64, Option<i64>, String, Payload)> {
    f.db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT row_version, base_row_version, status, payload_json FROM sync_outbox WHERE entity_sync_id = '{sid}' ORDER BY id"
        ),
    ))
    .await
    .expect("q")
    .iter()
    .map(|r| {
        (
            r.try_get("", "row_version").expect("rv"),
            r.try_get("", "base_row_version").expect("base"),
            r.try_get("", "status").expect("status"),
            serde_json::from_str(&r.try_get::<String>("", "payload_json").expect("payload")).expect("json"),
        )
    })
    .collect()
}

/// PC A as in review R3-S9: PC B approved the decision (v2, received here), A applied it offline
/// (v3, unsent, base v2; its work order created) and was upgraded (its sealed row journaled on top,
/// v4). Returns the decision's id, its sync id, and B's seal-only re-send of v2.
async fn pc_a_with_unsent_apply(f: &Fixture, study_id: i64) -> (i64, String, Payload) {
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    let (a, sid) = approved(f, study_id).await;
    let mut v2 = wire_of(f, ENTITY, &sid).await;
    for key in ["approval_seal", "sync_refused_at", "sync_refused_reason"] {
        v2.remove(key);
    }
    let v2_text = serde_json::to_string(&v2).expect("json");
    let current = crate::reliability::study::decisions::get_decision(&f.db, a.id)
        .await
        .expect("get");
    apply_decision(&f.db, decide(&current, None), f.admin)
        .await
        .expect("apply offline");
    exec(&f.db, &format!("UPDATE {ENTITY} SET approval_seal = NULL")).await;
    exec(&f.db, "DELETE FROM sync_capture_journal").await;
    exec(
        &f.db,
        &format!("DELETE FROM sync_outbox WHERE entity_sync_id = '{sid}'"),
    )
    .await;
    // v2 as received from B (kept in the inbox), the server version known here.
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, row_version, payload_json, payload_hash, apply_status) VALUES ('b-v2', 'c1', '{ENTITY}', '{sid}', 'upsert', 2, '{}', 'h-v2', 'applied')",
            v2_text.replace('\'', "''")
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "INSERT OR REPLACE INTO sync_entity_state (entity_type, entity_sync_id, synced_row_version, synced_payload_hash, source) VALUES ('{ENTITY}', '{sid}', 2, 'h-v2', 'inbound_apply')"
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, base_row_version, payload_json, payload_hash, status) VALUES ('k-v3', '{ENTITY}', '{sid}', 'upsert', 3, 2, '{{\"status\":\"applied\"}}', 'h-v3', 'pending')"
        ),
    )
    .await;
    crate::migrations::m20261026_000264_reliability_decision_seal::Migration
        .up(&SchemaManager::new(&f.db))
        .await
        .expect("migration 264");
    crate::sync::capture::drain_capture_journal(&f.db)
        .await
        .expect("drain at start-up");
    // B's re-send of v2 after its upgrade: the same content, sealed, with the new empty columns.
    let mut resend = v2.clone();
    resend.insert(
        "approval_seal".into(),
        local_approval_seal(&f.db, a.id)
            .await
            .expect("seal")
            .expect("row")
            .into(),
    );
    resend.insert("sync_refused_at".into(), Value::Null);
    resend.insert("sync_refused_reason".into(), Value::Null);
    (a.id, sid, through_text(&resend))
}

/// R3-S9, push path: B re-sent first; A's push gets a server conflict. It is settled with no
/// person: A's change is staged again on top of B's version, sealed, applied; another PC accepts it
/// and no PC can apply it a second time. A version that differs in anything else is not settled.
#[tokio::test]
async fn a_seal_only_resend_crossing_an_unsent_apply_settles_on_push() {
    use crate::reliability::study::decision_sync::settle_seal_resend_server_conflict;
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (id, sid, resend) = pc_a_with_unsent_apply(&f, study_id).await;
    let conflict = |payload: &Payload| crate::sync::domain::SyncServerConflictInput {
        idempotency_key: "k-v3".into(),
        entity_type: ENTITY.into(),
        entity_sync_id: sid.clone(),
        local_row_version: 3,
        local_base_row_version: Some(2),
        server_row_version: 3,
        server_operation: "upsert".into(),
        server_payload_json: Some(serde_json::to_string(payload).expect("json")),
        server_payload_hash: Some("h-b-v3".into()),
        server_owner_user_sync_id: None,
        server_deleted: false,
    };
    // Anything else changed: not a seal-only re-send, a person decides.
    let mut changed = resend.clone();
    changed.insert("decision_note".into(), "autre".into());
    assert!(!settle_seal_resend_server_conflict(&f.db, &conflict(&changed))
        .await
        .expect("judge"));
    // The seal-only re-send: settled.
    assert!(settle_seal_resend_server_conflict(&f.db, &conflict(&resend))
        .await
        .expect("settle"));
    let sends = sends_of(&f, &sid).await;
    let open: Vec<_> = sends.iter().filter(|s| s.2 == "pending").collect();
    assert_eq!(open.len(), 1, "{sends:?}");
    assert_eq!((open[0].0, open[0].1), (4, Some(3)), "staged on top of B's version");
    assert_eq!(open[0].3.get("status").and_then(Value::as_str), Some("applied"));
    assert!(open[0]
        .3
        .get("approval_seal")
        .and_then(Value::as_str)
        .is_some_and(|s| s.starts_with("v1:")));
    assert_eq!(sends.iter().filter(|s| s.2 == "superseded").count(), 2, "v3 and v4");
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM sync_conflicts WHERE entity_sync_id = '{sid}'")
        )
        .await,
        0,
        "nobody asked"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = 'reliability.decision.seal_resend_settled' AND target_id = '{sid}'")
        )
        .await,
        1
    );
    assert_eq!(decision_status(&f, id).await, "applied");
    // B (and any PC) accepts A's version applied: no second apply anywhere.
    let (n, p) = as_new(&open[0].3, "pc-b");
    let o = apply_wire(&f, ENTITY, &n, "upsert", &through_text(&p)).await;
    assert!(matches!(o, ApplyOutcome::Applied), "{o:?}");
    let there = scalar(&f.db, &format!("SELECT id FROM {ENTITY} WHERE entity_sync_id = '{n}'")).await;
    let there = crate::reliability::study::decisions::get_decision(&f.db, there)
        .await
        .expect("get");
    assert_eq!(
        refusal_code(apply_decision(&f.db, decide(&there, None), f.admin).await),
        "DECISION_NOT_APPROVED"
    );
}

/// R3-S9, pull path: A pulls B's re-send before pushing: settled with no person, A's row stays
/// applied, its change staged on top of B's version.
#[tokio::test]
async fn a_seal_only_resend_crossing_an_unsent_apply_settles_on_pull() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (id, sid, resend) = pc_a_with_unsent_apply(&f, study_id).await;
    inbox(&f, &sid, &resend, 3).await;
    materialize(&f).await;
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM sync_conflicts WHERE entity_sync_id = '{sid}'")
        )
        .await,
        0,
        "no CONCURRENT_EDIT"
    );
    assert_eq!(decision_status(&f, id).await, "applied", "not written over");
    let open: Vec<_> = sends_of(&f, &sid)
        .await
        .into_iter()
        .filter(|s| s.2 == "pending")
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!((open[0].0, open[0].1), (4, Some(3)));
    assert_eq!(open[0].3.get("status").and_then(Value::as_str), Some("applied"));
    // The received row is done with (review R4-N5): marked applied, and the settlement audited once.
    assert_eq!(
        text(
            &f,
            &format!("SELECT apply_status FROM sync_inbox WHERE entity_sync_id = '{sid}' AND row_version = 3")
        )
        .await,
        "applied"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = 'reliability.decision.seal_resend_settled' \
                   AND target_id = '{sid}' AND json_extract(details_json, '$.path') = 'pull' \
                   AND json_extract(details_json, '$.server_row_version') = 3"
            )
        )
        .await,
        1
    );
    assert_eq!(
        text(
            &f,
            &format!(
                "SELECT synced_row_version || ':' || synced_payload_hash FROM sync_entity_state \
                  WHERE entity_type = '{ENTITY}' AND entity_sync_id = '{sid}'"
            )
        )
        .await,
        format!("3:h-{sid}-3")
    );
}

/// Review R4-N2: a seal-only re-send received with a version not newer than the one this PC knows
/// the server holds is never settled (it runs before the concurrency check): the known version is
/// not moved back, this PC's change stays to be sent and a person decides.
#[tokio::test]
async fn a_seal_only_resend_not_newer_than_the_known_version_is_not_settled() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (id, sid, resend) = pc_a_with_unsent_apply(&f, study_id).await;
    let pending_before: Vec<_> = sends_of(&f, &sid)
        .await
        .into_iter()
        .filter(|s| s.2 == "pending")
        .map(|s| (s.0, s.1))
        .collect();
    for rv in [2, 1] {
        inbox(&f, &sid, &resend, rv).await;
        materialize(&f).await;
        assert_eq!(
            text(
                &f,
                &format!(
                    "SELECT synced_row_version || ':' || synced_payload_hash FROM sync_entity_state \
                      WHERE entity_type = '{ENTITY}' AND entity_sync_id = '{sid}'"
                )
            )
            .await,
            "2:h-v2",
            "v{rv}: the known version is not moved"
        );
        assert_eq!(
            text(
                &f,
                &format!("SELECT apply_status FROM sync_inbox WHERE entity_sync_id = '{sid}' AND row_version = {rv} AND server_batch_id = 'b-{sid}-{rv}'")
            )
            .await,
            "conflict",
            "v{rv}: a person decides"
        );
    }
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = 'reliability.decision.seal_resend_settled' AND target_id = '{sid}'")
        )
        .await,
        0,
        "nothing settled"
    );
    let pending_after: Vec<_> = sends_of(&f, &sid)
        .await
        .into_iter()
        .filter(|s| s.2 == "pending")
        .map(|s| (s.0, s.1))
        .collect();
    assert_eq!(pending_after, pending_before, "this PC's change is still to be sent");
    assert_eq!(decision_status(&f, id).await, "applied");
}

/// Review R4-N3: on the pull path, this PC's item already in `conflict` (with its server conflict
/// open for a person) is superseded by the settlement and that conflict is closed with it, noted
/// and counted in the audit; the change is staged again on top of the received version.
#[tokio::test]
async fn a_pull_settlement_closes_the_open_conflict_of_the_superseded_item() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (id, sid, resend) = pc_a_with_unsent_apply(&f, study_id).await;
    let v3 = scalar(
        &f.db,
        &format!("SELECT id FROM sync_outbox WHERE entity_sync_id = '{sid}' AND idempotency_key = 'k-v3'"),
    )
    .await;
    exec(
        &f.db,
        &format!("UPDATE sync_outbox SET status = 'conflict' WHERE id = {v3}"),
    )
    .await;
    crate::sync::queries::record_sync_conflict(
        &f.db,
        crate::sync::queries::NewSyncConflict {
            source_scope: "outbox",
            source_batch_id: Some("push-1".into()),
            linked_outbox_id: Some(v3),
            linked_inbox_id: None,
            entity_type: ENTITY.into(),
            entity_sync_id: sid.clone(),
            operation: "upsert".into(),
            conflict_type: "SERVER_CONFLICT".into(),
            local_payload_json: Some("{\"status\":\"applied\"}".into()),
            inbound_payload_json: None,
            checkpoint_token: None,
            local_row_version: Some(3),
            inbound_row_version: Some(3),
            detail_message: None,
        },
    )
    .await
    .expect("conflict");
    let (conflict_id, _, _) = open_conflict(&f, &sid).await;
    inbox(&f, &sid, &resend, 3).await;
    materialize(&f).await;
    assert_eq!(
        text(
            &f,
            &format!("SELECT status || ':' || resolution_action FROM sync_conflicts WHERE id = {conflict_id}")
        )
        .await,
        "resolved_remote:seal_resend_settled"
    );
    assert!(text(
        &f,
        &format!("SELECT resolution_note FROM sync_conflicts WHERE id = {conflict_id}")
    )
    .await
    .contains("server version 3"));
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM sync_conflicts WHERE entity_sync_id = '{sid}' \
                   AND status IN ('new', 'triaged', 'escalated')"
            )
        )
        .await,
        0,
        "no stale item left for a person"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = 'reliability.decision.seal_resend_settled' \
                   AND target_id = '{sid}' AND json_extract(details_json, '$.conflicts_closed') = 1"
            )
        )
        .await,
        1
    );
    let sends = sends_of(&f, &sid).await;
    let open: Vec<_> = sends.iter().filter(|s| s.2 == "pending").collect();
    assert_eq!(open.len(), 1, "{sends:?}");
    assert_eq!(open[0].1, Some(3), "staged again on top of the received version");
    assert_eq!(open[0].3.get("status").and_then(Value::as_str), Some("applied"));
    assert_eq!(
        text(&f, &format!("SELECT status FROM sync_outbox WHERE id = {v3}")).await,
        "superseded"
    );
    assert_eq!(decision_status(&f, id).await, "applied");
}

/// Review R4-N5: a version that differs from the base in anything a person can change is not a
/// seal-only re-send, whatever its seal: a status, the decider, an extra key, a null where the base
/// has no key, a refusal mark, or another seal. Nothing is settled, the known version is unchanged.
#[tokio::test]
async fn a_resend_differing_in_anything_else_is_not_settled() {
    use crate::reliability::study::decision_sync::settle_seal_resend_server_conflict;
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (_id, sid, resend) = pc_a_with_unsent_apply(&f, study_id).await;
    let with = |key: &str, value: Value| {
        let mut p = resend.clone();
        p.insert(key.into(), value);
        (key.to_string(), p)
    };
    let variants = [
        with("status", "applied".into()),
        with("decided_by_sync_id", "pc-x-another-person".into()),
        with("extra_key", "x".into()),
        with("extra_null", Value::Null),
        with("sync_refused_at", "2026-10-27T08:00:00Z".into()),
        with("sync_refused_reason", "DECISION_APPROVER_UNAUTHORISED".into()),
        with("approval_seal", "v1:another-seal".into()),
    ];
    for (key, payload) in variants {
        let conflict = crate::sync::domain::SyncServerConflictInput {
            idempotency_key: "k-v3".into(),
            entity_type: ENTITY.into(),
            entity_sync_id: sid.clone(),
            local_row_version: 3,
            local_base_row_version: Some(2),
            server_row_version: 3,
            server_operation: "upsert".into(),
            server_payload_json: Some(serde_json::to_string(&payload).expect("json")),
            server_payload_hash: Some("h-b-v3".into()),
            server_owner_user_sync_id: None,
            server_deleted: false,
        };
        assert!(
            !settle_seal_resend_server_conflict(&f.db, &conflict)
                .await
                .expect("judge"),
            "{key}: not a seal-only re-send"
        );
    }
    // A key the base had, missing from the re-send (missing vs a value): not settled either.
    let mut missing = resend.clone();
    missing.remove("decision_note");
    let conflict = crate::sync::domain::SyncServerConflictInput {
        idempotency_key: "k-v3".into(),
        entity_type: ENTITY.into(),
        entity_sync_id: sid.clone(),
        local_row_version: 3,
        local_base_row_version: Some(2),
        server_row_version: 3,
        server_operation: "upsert".into(),
        server_payload_json: Some(serde_json::to_string(&missing).expect("json")),
        server_payload_hash: Some("h-b-v3".into()),
        server_owner_user_sync_id: None,
        server_deleted: false,
    };
    assert!(!settle_seal_resend_server_conflict(&f.db, &conflict)
        .await
        .expect("judge"));
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = 'reliability.decision.seal_resend_settled' AND target_id = '{sid}'")
        )
        .await,
        0
    );
    assert_eq!(
        text(
            &f,
            &format!(
                "SELECT synced_row_version || ':' || synced_payload_hash FROM sync_entity_state \
                  WHERE entity_type = '{ENTITY}' AND entity_sync_id = '{sid}'"
            )
        )
        .await,
        "2:h-v2"
    );
    // The unchanged re-send is still settled: the cases above fail on their difference only.
    let conflict = crate::sync::domain::SyncServerConflictInput {
        server_payload_json: Some(serde_json::to_string(&resend).expect("json")),
        ..conflict
    };
    assert!(settle_seal_resend_server_conflict(&f.db, &conflict)
        .await
        .expect("settle"));
}

/// R3-N6: a change sent but not acknowledged at the upgrade (in flight) is kept, and the sealed row
/// is staged on top of it.
#[tokio::test]
async fn an_in_flight_change_is_kept_and_followed_by_its_sealed_version() {
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.reviewer).await;
    let (_, sid) = approved(&f, study_id).await;
    exec(&f.db, &format!("UPDATE {ENTITY} SET approval_seal = NULL")).await;
    exec(&f.db, "DELETE FROM sync_capture_journal").await;
    exec(&f.db, "DELETE FROM sync_outbox").await;
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, base_row_version, payload_json, payload_hash, status) VALUES ('k-flight', '{ENTITY}', '{sid}', 'upsert', 5, 4, '{{}}', 'h-5', 'in_flight')"
        ),
    )
    .await;
    crate::migrations::m20261026_000264_reliability_decision_seal::Migration
        .up(&SchemaManager::new(&f.db))
        .await
        .expect("migration 264");
    crate::sync::capture::drain_capture_journal(&f.db).await.expect("drain");
    let sends = sends_of(&f, &sid).await;
    assert_eq!(sends.len(), 2, "{sends:?}");
    assert_eq!(sends[0].2, "in_flight", "kept");
    assert_eq!(
        (sends[1].0, sends[1].1),
        (6, Some(5)),
        "on top of the in-flight version"
    );
    assert!(sends[1]
        .3
        .get("approval_seal")
        .and_then(Value::as_str)
        .is_some_and(|s| s.starts_with("v1:")));
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM system_config WHERE key = '{SEAL_RESEND_KEY}' AND value LIKE '%{sid}%'")
        )
        .await,
        0,
        "not listed for the post-pull re-send"
    );
}
