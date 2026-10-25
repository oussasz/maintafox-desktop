//! Planning four-eyes (audit findings AU-06, AU-102, S4; owner decision D6; CMMS Advisor answers
//! Q1-Q4): a break-in that is urgent or forces a blocking check is signed by a second planner
//! holding plan.confirm at the crew's site, with their password; the unfreeze decider holds
//! plan.confirm at the site; received commitments, break-ins and reopened weeks are judged again.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::Value;

use super::domain::{CommitBreakIn, CreateScheduleBreakInInput, ScheduleCommitment};
use super::scheduling;
use super::tests::{
    commit_input, create_planning_wo, ensure_personnel_with_rate, exec_sql, insert_candidate, local, scalar, setup_db,
    signing_approver, week_ref, weekly_site, APPROVER_PASSWORD,
};
use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};

type Payload = serde_json::Map<String, Value>;

/// A user holding plan.confirm only at `site` (an organisation-node assignment), linked to a
/// personnel record, with [`APPROVER_PASSWORD`].
async fn site_approver(db: &DatabaseConnection, user_id: i64, code: &str, site: i64) -> i64 {
    let personnel = ensure_personnel_with_rate(db, code).await;
    let hash = crate::auth::password::hash_password(APPROVER_PASSWORD).expect("hash");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, is_active, is_admin, \
            force_password_change, personnel_id, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 1, 0, 0, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [
            user_id.into(),
            format!("site-approver-{user_id}").into(),
            format!("site-approver-{user_id}").into(),
            format!("Site approver {user_id}").into(),
            personnel.into(),
            hash.into(),
        ],
    ))
    .await
    .expect("site approver");
    exec_sql(
        db,
        "INSERT OR IGNORE INTO roles (id, entity_sync_id, name, created_at, updated_at, row_version)
         VALUES (778, 'planning-site-approver-role', 'planning-site-approver', '2026-01-01T00:00:00Z',
                 '2026-01-01T00:00:00Z', 1)",
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at, granted_by_id)
             SELECT 778, id, '2026-01-01T00:00:00Z', 1 FROM permissions WHERE name = '{}'",
            crate::rbac::permissions::PLAN_CONFIRM
        ),
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
                valid_from, created_at, updated_at)
             VALUES ('site-approver-usa-{user_id}', {user_id}, 778, 'org_node', '{site}', '2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    user_id
}

/// A user holding plan.windows only (plant-wide), linked and with [`APPROVER_PASSWORD`].
async fn windows_only_user(db: &DatabaseConnection, user_id: i64, code: &str) -> i64 {
    let personnel = ensure_personnel_with_rate(db, code).await;
    let hash = crate::auth::password::hash_password(APPROVER_PASSWORD).expect("hash");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, is_active, is_admin, \
            force_password_change, personnel_id, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 1, 0, 0, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [
            user_id.into(),
            format!("windows-{user_id}").into(),
            format!("windows-{user_id}").into(),
            format!("Windows {user_id}").into(),
            personnel.into(),
            hash.into(),
        ],
    ))
    .await
    .expect("windows user");
    exec_sql(
        db,
        "INSERT OR IGNORE INTO roles (id, entity_sync_id, name, created_at, updated_at, row_version)
         VALUES (779, 'planning-windows-role', 'planning-windows', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at, granted_by_id)
             SELECT 779, id, '2026-01-01T00:00:00Z', 1 FROM permissions WHERE name = '{}'",
            crate::rbac::permissions::PLAN_WINDOWS
        ),
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
                valid_from, created_at, updated_at)
             VALUES ('windows-usa-{user_id}', {user_id}, 779, 'tenant', NULL, '2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    user_id
}

/// A commitment of a work order (a source that travels by sync) of the crew `team`.
async fn wo_commitment(db: &DatabaseConnection, team: i64, title: &str, start: &str, end: &str) -> ScheduleCommitment {
    let (wo, _) = create_planning_wo(db, title).await;
    let candidate = insert_candidate(db, "work_order", wo, "ready", 100.0).await;
    scheduling::create_schedule_commitment(db, 1, commit_input(candidate, team, start, end))
        .await
        .expect("work order commitment")
}

/// A commitment of the crew `team` on a slot of the open week after 2026-05-03.
async fn open_commitment(
    db: &DatabaseConnection,
    team: i64,
    source_id: i64,
    start: &str,
    end: &str,
) -> ScheduleCommitment {
    let candidate = insert_candidate(db, "manual_test", source_id, "ready", 100.0).await;
    scheduling::create_schedule_commitment(db, 1, commit_input(candidate, team, start, end))
        .await
        .expect("commitment")
}

fn move_input(c: &ScheduleCommitment, reason: &str, start: &str, end: &str) -> CreateScheduleBreakInInput {
    CreateScheduleBreakInInput {
        schedule_commitment_id: c.id,
        expected_commitment_row_version: c.row_version,
        break_in_reason: reason.to_string(),
        approved_by_user_id: None,
        new_slot_start: local(start),
        new_slot_end: local(end),
        new_assigned_team_id: None,
        new_assigned_personnel_id: None,
        clear_assignee: None,
        bypass_availability: Some(false),
        override_reason: None,
        dangerous_override_reason: None,
        approver_password: None,
        acknowledged_assignee_issues: Vec::new(),
    }
}

fn signed(mut input: CreateScheduleBreakInInput, approver: i64, password: &str) -> CreateScheduleBreakInInput {
    input.approved_by_user_id = Some(approver);
    input.approver_password = Some(password.into());
    input
}

async fn failed_attempts(db: &DatabaseConnection, user: i64) -> i64 {
    scalar(
        db,
        &format!("SELECT COALESCE(failed_login_attempts, 0) FROM user_accounts WHERE id = {user}"),
    )
    .await
}

fn code_of(err: &crate::errors::AppError) -> String {
    format!("{err:?}")
}

#[tokio::test]
async fn an_urgent_move_is_signed_by_another_planner_of_the_crews_site_with_their_password() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "FE1").await;
    let (other_site, _) = weekly_site(&db, "FE1B").await;
    let c = open_commitment(&db, team, 8101, "2026-05-12T08:00", "2026-05-12T09:00").await;
    let urgent = || move_input(&c, "emergency", "2026-05-12T10:00", "2026-05-12T11:00");

    // No signer, even with a dangerous-override reason: refused (no fallback, scheduling.rs:1697).
    let mut dangerous = urgent();
    dangerous.dangerous_override_reason = Some("Line stopped".to_string());
    let err = scheduling::create_schedule_break_in(&db, 1, dangerous)
        .await
        .expect_err("needs a signer");
    assert!(code_of(&err).contains("PLANNING_BREAK_IN_APPROVAL_REQUIRED"), "{err:?}");

    // The requester never signs their own break-in.
    let me = signing_approver(&db, 120, "FE1-ME").await;
    let err = scheduling::create_schedule_break_in(&db, me, signed(urgent(), me, APPROVER_PASSWORD))
        .await
        .expect_err("self");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_SELF"), "{err:?}");

    // plan.confirm at another site, or plan.windows only: not allowed.
    let elsewhere = site_approver(&db, 121, "FE1-ELSE", other_site).await;
    let err = scheduling::create_schedule_break_in(&db, 1, signed(urgent(), elsewhere, APPROVER_PASSWORD))
        .await
        .expect_err("other site");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_NOT_ALLOWED"), "{err:?}");
    let windows = windows_only_user(&db, 122, "FE1-WIN").await;
    let err = scheduling::create_schedule_break_in(&db, 1, signed(urgent(), windows, APPROVER_PASSWORD))
        .await
        .expect_err("plan.windows is no approval right");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_NOT_ALLOWED"), "{err:?}");

    // At the crew's site: the password decides. Empty, wrong (counted, though refused), locked.
    let here = site_approver(&db, 123, "FE1-HERE", site).await;
    let err = scheduling::create_schedule_break_in(&db, 1, signed(urgent(), here, ""))
        .await
        .expect_err("no password");
    assert!(
        code_of(&err).contains("GATE_PLAN_APPROVER_PASSWORD_REQUIRED"),
        "{err:?}"
    );
    let before = failed_attempts(&db, here).await;
    let err = scheduling::create_schedule_break_in(&db, 1, signed(urgent(), here, "wrong"))
        .await
        .expect_err("wrong password");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_AUTH_FAILED"), "{err:?}");
    assert_eq!(
        failed_attempts(&db, here).await,
        before + 1,
        "counted outside the transaction"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_break_ins WHERE schedule_commitment_id = {}",
                c.id
            )
        )
        .await,
        0,
        "nothing recorded"
    );
    exec_sql(
        &db,
        &format!("UPDATE user_accounts SET locked_until = '2999-01-01T00:00:00Z' WHERE id = {here}"),
    )
    .await;
    let err = scheduling::create_schedule_break_in(&db, 1, signed(urgent(), here, APPROVER_PASSWORD))
        .await
        .expect_err("locked");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_LOCKED"), "{err:?}");
    exec_sql(
        &db,
        &format!("UPDATE user_accounts SET locked_until = NULL WHERE id = {here}"),
    )
    .await;

    // Signed: recorded with both names, the count cleared, one audit event.
    let created = scheduling::create_schedule_break_in(&db, 1, signed(urgent(), here, APPROVER_PASSWORD))
        .await
        .expect("signed break-in");
    assert_eq!(created.approved_by_user_id, Some(here));
    assert!(created.approver_signed_at.is_some());
    assert!(created.forced_checks.is_empty());
    assert_eq!(failed_attempts(&db, here).await, 0, "a right password clears the count");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_change_log WHERE commitment_id = {} AND action_type = 'break_in_override' \
                   AND json_extract(details_json, '$.requested_by_user_id') = 1 \
                   AND json_extract(details_json, '$.approved_by_user_id') = {here} \
                   AND json_extract(details_json, '$.approver_signed') = 1",
                c.id
            )
        )
        .await,
        1,
        "the change log names requester and approver"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = 'planning.break_in.signed' \
                   AND target_type = 'schedule_commitment' AND target_id = '{}'",
                c.id
            )
        )
        .await,
        1
    );
    // No break-in is ever "approved" by its own requester.
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM schedule_break_ins WHERE approved_by_user_id = created_by_id"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn an_urgent_insert_into_a_frozen_week_is_signed_the_same_way() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "FE2").await;
    super::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    let approver = site_approver(&db, 130, "FE2-HERE", site).await;
    let candidate = insert_candidate(&db, "manual_test", 8201, "ready", 100.0).await;
    let mut input = commit_input(candidate, team, "2026-05-05T08:00", "2026-05-05T09:00");
    let with = |approver: Option<i64>, password: Option<&str>| CommitBreakIn {
        reason: "safety".to_string(),
        approved_by_user_id: approver,
        approver_password: password.map(Into::into),
        dangerous_override_reason: Some("Guard missing".to_string()),
    };
    input.frozen_week_break_in = Some(with(None, None));
    let err = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("no signer");
    assert!(code_of(&err).contains("PLANNING_BREAK_IN_APPROVAL_REQUIRED"), "{err:?}");
    input.frozen_week_break_in = Some(with(Some(1), Some(APPROVER_PASSWORD)));
    let err = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("self");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_SELF"), "{err:?}");
    // Not linked to a personnel record.
    exec_sql(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = NULL WHERE id = {approver}"),
    )
    .await;
    input.frozen_week_break_in = Some(with(Some(approver), Some(APPROVER_PASSWORD)));
    let err = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("no personnel");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_NO_PERSONNEL"), "{err:?}");
    let personnel = ensure_personnel_with_rate(&db, "FE2-HERE").await;
    exec_sql(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = {personnel} WHERE id = {approver}"),
    )
    .await;
    let before = failed_attempts(&db, approver).await;
    input.frozen_week_break_in = Some(with(Some(approver), Some("wrong")));
    let err = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("wrong password");
    assert!(code_of(&err).contains("GATE_PLAN_APPROVER_AUTH_FAILED"), "{err:?}");
    assert_eq!(failed_attempts(&db, approver).await, before + 1);
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) FROM schedule_commitments WHERE schedule_candidate_id = {candidate}")
        )
        .await,
        0,
        "the commitment is rolled back with its break-in"
    );
    input.frozen_week_break_in = Some(with(Some(approver), Some(APPROVER_PASSWORD)));
    let inserted = scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("signed insert");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_break_ins WHERE schedule_commitment_id = {} AND kind = 'insert' \
                   AND approved_by_user_id = {approver} AND approver_signed_at IS NOT NULL",
                inserted.id
            )
        )
        .await,
        1
    );
}

#[tokio::test]
async fn the_approver_list_holds_plan_confirm_at_the_crews_site_linked_people_but_not_the_caller() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "FE3").await;
    let (other_site, _) = weekly_site(&db, "FE3B").await;
    let plant_wide = signing_approver(&db, 140, "FE3-PLANT").await;
    let here = site_approver(&db, 141, "FE3-HERE", site).await;
    let elsewhere = site_approver(&db, 142, "FE3-ELSE", other_site).await;
    let unlinked = site_approver(&db, 143, "FE3-UNLINKED", site).await;
    exec_sql(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = NULL WHERE id = {unlinked}"),
    )
    .await;
    let windows = windows_only_user(&db, 144, "FE3-WIN").await;

    let ids = |caller: i64| {
        let db = db.clone();
        async move {
            super::break_in_approval::list_break_in_approvers(&db, caller, team)
                .await
                .expect("list")
                .into_iter()
                .map(|o| o.user_id)
                .collect::<Vec<_>>()
        }
    };
    let listed = ids(here).await;
    assert!(listed.contains(&plant_wide), "{listed:?}");
    assert!(!listed.contains(&here), "never the caller");
    assert!(!listed.contains(&elsewhere), "another site");
    assert!(!listed.contains(&unlinked), "no personnel record");
    assert!(!listed.contains(&windows), "plan.windows is no approval right");
    assert!(ids(plant_wide).await.contains(&here), "held at the crew's site");
}

#[tokio::test]
async fn the_unfreeze_decider_holds_plan_confirm_at_the_site() {
    let db = setup_db().await;
    let (site, _team) = weekly_site(&db, "FE4").await;
    let (other_site, _) = weekly_site(&db, "FE4B").await;
    let requester = signing_approver(&db, 150, "FE4-REQ").await;
    let elsewhere = site_approver(&db, 151, "FE4-ELSE", other_site).await;
    let here = site_approver(&db, 152, "FE4-HERE", site).await;
    super::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    super::weekly_schedule::request_unfreeze(
        &db,
        requester,
        super::weekly_schedule::UnfreezeRequestInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: None,
            reason: "Shutdown moved".to_string(),
        },
    )
    .await
    .expect("requested");
    let decide = |actor: i64| {
        let db = db.clone();
        async move {
            super::weekly_schedule::decide_unfreeze(
                &db,
                actor,
                super::weekly_schedule::UnfreezeDecisionInput {
                    site_id: site,
                    day: "2026-05-04".to_string(),
                    expected_row_version: None,
                    approve: true,
                    comment: None,
                },
            )
            .await
        }
    };
    let err = decide(elsewhere).await.expect_err("another site");
    assert!(
        code_of(&err).contains("PLANNING_UNFREEZE_NOT_ALLOWED_AT_SITE"),
        "{err:?}"
    );
    let err = decide(requester).await.expect_err("requester");
    assert!(code_of(&err).contains("PLANNING_UNFREEZE_SAME_PERSON"), "{err:?}");
    decide(here).await.expect("decided at the site");
}

// ── Received rows (AU-102, S4) ─────────────────────────────────────────────────────────────

async fn sync_id(db: &DatabaseConnection, table: &str, id: i64) -> String {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT entity_sync_id FROM {table} WHERE id = {id}"),
    ))
    .await
    .expect("q")
    .expect("row")
    .try_get("", "entity_sync_id")
    .expect("sync id")
}

async fn payload(db: &DatabaseConnection, entity_type: &str, sid: &str) -> Payload {
    crate::sync::test_support::current_payload(db, entity_type, sid, &Default::default())
        .await
        .expect("payload")
        .expect("row")
}

async fn receive(db: &DatabaseConnection, entity_type: &str, sid: &str, mut p: Payload) -> ApplyOutcome {
    let rv = p.get("row_version").and_then(Value::as_i64).unwrap_or(1) + 10;
    p.insert("row_version".into(), rv.into());
    let outcome = apply_inbound_payload(db, entity_type, sid, "upsert", rv, &Value::Object(p).to_string())
        .await
        .expect("apply");
    crate::sync::row_codec::clear_ref_cache();
    outcome
}

/// A row waiting in the inbox, as a pull leaves it.
async fn inbox(db: &DatabaseConnection, entity_type: &str, sid: &str, p: &Payload) {
    let rv = p.get("row_version").and_then(Value::as_i64).unwrap_or(1) + 10;
    let mut p = p.clone();
    p.insert("row_version".into(), rv.into());
    exec_sql(
        db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
             row_version, payload_json, payload_hash, apply_status) VALUES ('b-{sid}', 'c1', '{entity_type}', \
             '{sid}', 'upsert', {rv}, '{}', 'h-{sid}-{rv}', 'pending')",
            serde_json::to_string(&p).expect("json").replace('\'', "''")
        ),
    )
    .await;
}

fn conflict_code(o: &ApplyOutcome) -> Option<&str> {
    match o {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

/// A frozen week (2026-05-03 … 09) of a fresh site, a commitment of the next (open) week, and a
/// real break-in payload (of another commitment) to build received break-ins from.
struct Received {
    db: DatabaseConnection,
    site: i64,
    team: i64,
    commitment: ScheduleCommitment,
    commitment_sid: String,
    break_in_template: Payload,
    approver: i64,
}

async fn received_setup(code: &str) -> Received {
    let db = setup_db().await;
    // Rows get their sync identity as on a device (capture installed, seeded identities given).
    crate::sync::capture::install_capture_in(&db).await.expect("capture");
    let (site, team) = weekly_site(&db, code).await;
    let approver = site_approver(&db, 160, &format!("{code}-APP"), site).await;
    let commitment = wo_commitment(
        &db,
        team,
        &format!("{code} received"),
        "2026-05-12T08:00",
        "2026-05-12T09:00",
    )
    .await;
    let template_of = wo_commitment(
        &db,
        team,
        &format!("{code} template"),
        "2026-05-13T08:00",
        "2026-05-13T09:00",
    )
    .await;
    let bi = scheduling::create_schedule_break_in(
        &db,
        1,
        signed(
            move_input(&template_of, "emergency", "2026-05-13T10:00", "2026-05-13T11:00"),
            approver,
            APPROVER_PASSWORD,
        ),
    )
    .await
    .expect("template break-in");
    let bi_sid = sync_id(&db, "schedule_break_ins", bi.id).await;
    let break_in_template = payload(&db, "schedule_break_ins", &bi_sid).await;
    super::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    let commitment_sid = sync_id(&db, "schedule_commitments", commitment.id).await;
    Received {
        db,
        site,
        team,
        commitment,
        commitment_sid,
        break_in_template,
        approver,
    }
}

impl Received {
    /// The commitment as another PC sends it, moved to `[start, end)` (local times).
    async fn moved(&self, start: &str, end: &str) -> Payload {
        let mut p = payload(&self.db, "schedule_commitments", &self.commitment_sid).await;
        p.insert("committed_start".into(), local(start).into());
        p.insert("committed_end".into(), local(end).into());
        p.insert("updated_at".into(), "2026-05-02T09:00:00Z".into());
        p
    }

    /// A break-in of the commitment to `[start, end)`.
    fn break_in(&self, start: &str, end: &str, edit: impl FnOnce(&mut Payload)) -> Payload {
        let mut p = self.break_in_template.clone();
        p.insert("schedule_commitment_sync_id".into(), self.commitment_sid.clone().into());
        p.insert("new_slot_start".into(), local(start).into());
        p.insert("new_slot_end".into(), local(end).into());
        p.insert("notification_dedupe_key".into(), format!("recv-{start}").into());
        edit(&mut p);
        p
    }

    async fn slot_here(&self) -> String {
        self.db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT committed_start FROM schedule_commitments WHERE id = {}",
                    self.commitment.id
                ),
            ))
            .await
            .expect("q")
            .expect("row")
            .try_get("", "committed_start")
            .expect("start")
    }
}

#[tokio::test]
async fn work_moved_into_a_frozen_week_by_sync_without_its_break_in_is_a_conflict_at_once() {
    let r = received_setup("FE5").await;
    let before = r.slot_here().await;
    let moved = r.moved("2026-05-06T08:00", "2026-05-06T09:00").await;
    let outcome = receive(&r.db, "schedule_commitments", &r.commitment_sid, moved).await;
    assert_eq!(conflict_code(&outcome), Some("PLANNING_WEEK_FROZEN"), "{outcome:?}");
    match &outcome {
        ApplyOutcome::Conflict { message, .. } => {
            assert!(
                message.contains("2026-05-02T09:00:00Z"),
                "the sender's change time: {message}"
            );
            assert!(message.contains("frozen here at"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.slot_here().await, before, "nothing applied");

    // Moved within the open week: applied. Released: applied (leaving is not entering).
    let open = r.moved("2026-05-12T10:00", "2026-05-12T11:00").await;
    let outcome = receive(&r.db, "schedule_commitments", &r.commitment_sid, open).await;
    assert!(conflict_code(&outcome).is_none(), "{outcome:?}");
    let _ = r.team;
}

#[tokio::test]
async fn a_commitment_received_with_its_signed_break_in_is_applied_with_it() {
    let r = received_setup("FE6").await;
    let bi = r.break_in("2026-05-06T08:00", "2026-05-06T09:00", |_| {});
    inbox(&r.db, "schedule_break_ins", "recv-bi-fe6", &bi).await;
    let moved = r.moved("2026-05-06T08:00", "2026-05-06T09:00").await;
    let outcome = receive(&r.db, "schedule_commitments", &r.commitment_sid, moved).await;
    assert!(conflict_code(&outcome).is_none(), "{outcome:?}");
    assert_eq!(r.slot_here().await, local("2026-05-06T08:00"));
    assert_eq!(
        scalar(
            &r.db,
            "SELECT COUNT(*) FROM schedule_break_ins WHERE entity_sync_id = 'recv-bi-fe6'"
        )
        .await,
        1,
        "the break-in is applied in the same transaction"
    );
}

#[tokio::test]
async fn a_self_approved_break_in_refuses_its_commitment_too() {
    let r = received_setup("FE7").await;
    let creator = r.break_in_template.get("created_by_sync_id").cloned().expect("creator");
    let bi = r.break_in("2026-05-06T08:00", "2026-05-06T09:00", |p| {
        p.insert("approved_by_user_sync_id".into(), creator);
    });
    inbox(&r.db, "schedule_break_ins", "recv-bi-fe7", &bi).await;
    let before = r.slot_here().await;
    let moved = r.moved("2026-05-06T08:00", "2026-05-06T09:00").await;
    let outcome = receive(&r.db, "schedule_commitments", &r.commitment_sid, moved).await;
    assert_eq!(conflict_code(&outcome), Some("SYNC_GROUP_REFUSED"), "{outcome:?}");
    match &outcome {
        ApplyOutcome::Conflict { message, .. } => {
            assert!(message.contains("PLANNING_BREAK_IN_SELF_APPROVAL"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    let _ = before;

    // Through the inbox pass, the refused group is rolled back: nothing of it is kept.
    let r = received_setup("FE7B").await;
    exec_sql(&r.db, "DELETE FROM sync_capture_journal").await;
    exec_sql(&r.db, "DELETE FROM sync_outbox").await;
    let creator = r.break_in_template.get("created_by_sync_id").cloned().expect("creator");
    let bi = r.break_in("2026-05-06T08:00", "2026-05-06T09:00", |p| {
        p.insert("approved_by_user_sync_id".into(), creator);
    });
    inbox(&r.db, "schedule_break_ins", "recv-bi-fe7b", &bi).await;
    let before = r.slot_here().await;
    let moved = r.moved("2026-05-06T08:00", "2026-05-06T09:00").await;
    inbox(&r.db, "schedule_commitments", &r.commitment_sid, &moved).await;
    crate::sync::materialize::materialize_inbox(&r.db, 200, true)
        .await
        .expect("pass");
    assert_eq!(r.slot_here().await, before, "nothing of the group is kept");
    assert_eq!(
        scalar(
            &r.db,
            "SELECT COUNT(*) FROM schedule_break_ins WHERE entity_sync_id = 'recv-bi-fe7b'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar(
            &r.db,
            "SELECT COUNT(*) FROM sync_conflicts WHERE conflict_type = 'SYNC_GROUP_REFUSED'                AND entity_type = 'schedule_commitments'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_received_break_in_is_judged_on_its_signature_and_its_approvers_right_here() {
    let r = received_setup("FE8").await;
    // Applied here first (a move of the commitment is not what is judged here).
    let check = |p: Payload| {
        let db = r.db.clone();
        async move {
            super::sync_check::inbound_break_in_check(&db, "recv-bi-direct", &p)
                .await
                .expect("check")
        }
    };
    let verdict_code = |v: Option<crate::settings::site::InboundVerdict>| match v {
        Some(crate::settings::site::InboundVerdict::Conflict { code, .. }) => Some(code.to_string()),
        Some(crate::settings::site::InboundVerdict::Defer(m)) => Some(format!("DEFER: {m}")),
        None => None,
    };
    let valid = r.break_in("2026-05-12T10:00", "2026-05-12T11:00", |_| {});
    assert_eq!(
        verdict_code(check(valid.clone()).await),
        None,
        "signed at the crew's site"
    );

    let unsigned = r.break_in("2026-05-12T10:00", "2026-05-12T11:00", |p| {
        p.insert("approver_signed_at".into(), Value::Null);
    });
    assert_eq!(
        verdict_code(check(unsigned).await).as_deref(),
        Some("PLANNING_BREAK_IN_UNSIGNED"),
        "named only (older version)"
    );
    let unapproved = r.break_in("2026-05-12T10:00", "2026-05-12T11:00", |p| {
        p.insert("approved_by_user_sync_id".into(), Value::Null);
        p.insert("approved_by_personnel_sync_id".into(), Value::Null);
    });
    assert_eq!(
        verdict_code(check(unapproved.clone()).await).as_deref(),
        Some("PLANNING_BREAK_IN_APPROVAL_REQUIRED"),
        "urgent without an approver"
    );
    let mut ordinary = unapproved.clone();
    ordinary.insert("break_in_reason".into(), "other".into());
    assert_eq!(
        verdict_code(check(ordinary.clone()).await),
        None,
        "an ordinary reason needs no signer"
    );
    ordinary.insert("forced_checks_json".into(), "[\"locked_window\"]".into());
    assert_eq!(
        verdict_code(check(ordinary).await).as_deref(),
        Some("PLANNING_BREAK_IN_APPROVAL_REQUIRED"),
        "a forced check needs the signer"
    );
    // The approver's right withdrawn here since: a conflict (rights judged as held now).
    exec_sql(
        &r.db,
        &format!("DELETE FROM user_scope_assignments WHERE user_id = {}", r.approver),
    )
    .await;
    assert_eq!(
        verdict_code(check(valid).await).as_deref(),
        Some("PLANNING_BREAK_IN_APPROVER_UNAUTHORISED")
    );
    let _ = r.site;
}

#[tokio::test]
async fn a_frozen_week_conflict_closes_when_its_signed_break_in_arrives() {
    let r = received_setup("FE9").await;
    // Everything made here has been sent: no unsent local change of the commitment.
    exec_sql(&r.db, "DELETE FROM sync_capture_journal").await;
    exec_sql(&r.db, "DELETE FROM sync_outbox").await;
    let moved = r.moved("2026-05-06T08:00", "2026-05-06T09:00").await;
    inbox(&r.db, "schedule_commitments", &r.commitment_sid, &moved).await;
    crate::sync::materialize::materialize_inbox(&r.db, 200, true)
        .await
        .expect("first pass");
    let open = "SELECT COUNT(*) FROM sync_conflicts WHERE conflict_type = 'PLANNING_WEEK_FROZEN' \
                AND status IN ('new', 'triaged', 'escalated')";
    assert_eq!(scalar(&r.db, open).await, 1, "a conflict at once, not a wait");

    // Its break-in arrives with the next synchronisation.
    let bi = r.break_in("2026-05-06T08:00", "2026-05-06T09:00", |_| {});
    inbox(&r.db, "schedule_break_ins", "recv-bi-fe9", &bi).await;
    crate::sync::materialize::materialize_inbox(&r.db, 200, true)
        .await
        .expect("second pass");
    assert_eq!(scalar(&r.db, open).await, 0, "closed by its break-in");
    assert_eq!(r.slot_here().await, local("2026-05-06T08:00"));
    assert_eq!(
        scalar(
            &r.db,
            "SELECT COUNT(*) FROM sync_conflicts WHERE conflict_type = 'PLANNING_WEEK_FROZEN' \
               AND status = 'resolved_remote' AND resolution_action = 'break_in_received'"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &r.db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'planning.week_frozen_conflict.closed_by_break_in'"
        )
        .await,
        1,
        "audited"
    );
}

#[tokio::test]
async fn a_week_reopened_by_sync_needs_its_four_eyes_unfreeze_decision() {
    let r = received_setup("FE10").await;
    let week_sid: String =
        r.db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT entity_sync_id FROM weekly_schedules WHERE entity_id = {}",
                r.site
            ),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "entity_sync_id")
        .expect("sid");
    let mut reopened = payload(&r.db, "weekly_schedules", &week_sid).await;
    reopened.insert("status".into(), "approved".into());
    reopened.insert("frozen_at".into(), Value::Null);
    let status = || {
        let db = r.db.clone();
        let site = r.site;
        async move {
            scalar_text(
                &db,
                &format!("SELECT status FROM weekly_schedules WHERE entity_id = {site}"),
            )
            .await
        }
    };

    // No decision here or received with it.
    let outcome = receive(&r.db, "weekly_schedules", &week_sid, reopened.clone()).await;
    assert_eq!(conflict_code(&outcome), Some("WEEK_UNFREEZE_UNDECIDED"), "{outcome:?}");
    assert_eq!(status().await, "frozen");

    let event = |requester: &str, decider: &str| {
        let mut p = Payload::new();
        p.insert("weekly_schedule_sync_id".into(), week_sid.clone().into());
        p.insert("action".into(), "unfrozen".into());
        p.insert("actor_sync_id".into(), requester.into());
        p.insert("approver_sync_id".into(), decider.into());
        p.insert("reason".into(), "Shutdown moved".into());
        p.insert("baseline_no".into(), 1.into());
        p.insert("created_at".into(), "2026-05-04T10:00:00Z".into());
        p.insert("row_version".into(), 1.into());
        p
    };
    let requester = signing_approver(&r.db, 170, "FE10-REQ").await;
    let requester_sid = sync_id(&r.db, "user_accounts", requester).await;
    let approver_sid = sync_id(&r.db, "user_accounts", r.approver).await;

    // Decided by its own requester: a conflict.
    inbox(
        &r.db,
        "weekly_schedule_events",
        "recv-ev-self",
        &event(&requester_sid, &requester_sid),
    )
    .await;
    let outcome = receive(&r.db, "weekly_schedules", &week_sid, reopened.clone()).await;
    assert_eq!(
        conflict_code(&outcome),
        Some("WEEK_UNFREEZE_SAME_PERSON"),
        "{outcome:?}"
    );
    exec_sql(&r.db, "DELETE FROM sync_inbox WHERE entity_sync_id = 'recv-ev-self'").await;

    // Decided by someone else holding plan.confirm at the site: applied.
    inbox(
        &r.db,
        "weekly_schedule_events",
        "recv-ev-ok",
        &event(&requester_sid, &approver_sid),
    )
    .await;
    let outcome = receive(&r.db, "weekly_schedules", &week_sid, reopened).await;
    assert!(conflict_code(&outcome).is_none(), "{outcome:?}");
    assert_eq!(status().await, "approved");
}

async fn scalar_text(db: &DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index::<String>(0)
        .expect("text")
}

#[test]
fn the_approver_password_is_never_serialized_or_printed() {
    let input: CommitBreakIn = serde_json::from_value(serde_json::json!({
        "reason": "emergency",
        "approved_by_user_id": 7,
        "approver_password": "Very-Secret-1",
        "dangerous_override_reason": null
    }))
    .expect("read from the command input");
    assert_eq!(
        input.approver_password.as_ref().map(super::domain::ApproverPassword::expose),
        Some("Very-Secret-1")
    );
    assert!(!format!("{input:?}").contains("Very-Secret-1"), "redacted in Debug");
    let sent = serde_json::to_string(&input).expect("serialize");
    assert!(!sent.contains("Very-Secret-1") && !sent.contains("approver_password"), "{sent}");
}
