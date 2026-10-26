//! AU-76b: a reliability decision received approved or applied is checked again on arrival
//! (`reliability::study::decision_sync`, CMMS Advisor answers 2026-10-26).

use serde_json::{Map, Value};

use super::*;
use crate::reliability::study::decision_sync::{
    approval_seal, local_approval_seal, retry_decision_conflicts, CONFLICT_CLOSED_EVENT, REFUSED_EVENT,
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
