//! Received password changes and re-enables re-checked against their author
//! (`sync::credential_check`), through the real inbound path (`apply_inbound_payload` →
//! `row_codec::apply_row`), with payloads read from the database as the codec sends them.
//! Numbers in the test names refer to the design's test list.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::rbac::permissions::{ADM_ROLES, ADM_USERS, OT_VIEW};
use crate::sync::credential_check::{ACCOUNT_REENABLE_UNAUTHORISED, PASSWORD_CHANGE_UNAUTHORISED};
use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};

type Payload = Map<String, Value>;

const AUTHOR: &str = "password_changed_by_sync_id";
const STATUS_AUTHOR: &str = "status_changed_by_sync_id";
const SET_BY: &str = "password_set_by_sync_id";
const SET_AT: &str = "password_set_at";
const REMOTE_HASH: &str = "hash-set-on-pc-a";

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<i64> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<i64>>(0).expect("integer"))
}

async fn text(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("text"))
}

fn stamp(offset: chrono::Duration) -> String {
    (chrono::Utc::now() + offset).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn now() -> String {
    stamp(chrono::Duration::zero())
}

async fn user(db: &DatabaseConnection, username: &str) -> i64 {
    let at = stamp(chrono::Duration::days(-30));
    exec(
        db,
        "INSERT INTO user_accounts \
         (entity_sync_id, username, display_name, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES (?, ?, ?, 'local', 'x', 1, 0, 0, ?, 0, ?, ?, 1)",
        vec![
            format!("{username}-sync").into(),
            username.into(),
            username.into(),
            at.clone().into(),
            at.clone().into(),
            at.into(),
        ],
    )
    .await;
    scalar(
        db,
        "SELECT id FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await
    .expect("user")
}

async fn role_id(db: &DatabaseConnection, name: &str) -> i64 {
    scalar(db, "SELECT id FROM roles WHERE name = ?", vec![name.into()])
        .await
        .unwrap_or_else(|| panic!("role {name}"))
}

async fn custom_role(db: &DatabaseConnection, name: &str, permissions: &[&str]) -> i64 {
    let at = now();
    exec(
        db,
        "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
         VALUES (?, ?, 'custom', 'active', 0, ?, ?, 1)",
        vec![
            format!("{name}-sync").into(),
            name.into(),
            at.clone().into(),
            at.clone().into(),
        ],
    )
    .await;
    let role = role_id(db, name).await;
    for p in permissions {
        exec(
            db,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) SELECT ?, id, ? FROM permissions WHERE name = ?",
            vec![role.into(), at.clone().into(), (*p).into()],
        )
        .await;
    }
    role
}

/// A plant-wide assignment of `role` to `user` given by `by` (a real author, as the S3 check needs).
async fn assign(db: &DatabaseConnection, sync_id: &str, user: i64, role: i64, by: Option<i64>, at: &str) {
    exec(
        db,
        "INSERT INTO user_scope_assignments \
         (entity_sync_id, user_id, role_id, scope_type, scope_reference, assigned_by_id, created_at, updated_at, \
          row_version) VALUES (?, ?, ?, 'tenant', NULL, ?, ?, ?, 1)",
        vec![
            sync_id.into(),
            user.into(),
            role.into(),
            by.map_or(sea_orm::Value::BigInt(None), Into::into),
            at.into(),
            at.into(),
        ],
    )
    .await;
}

async fn payload(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    crate::sync::test_support::current_payload(db, entity_type, sync_id, &Map::default())
        .await
        .expect("payload")
        .expect("row")
}

async fn receive(db: &DatabaseConnection, entity_type: &str, sync_id: &str, row: Payload) -> ApplyOutcome {
    apply_inbound_payload(db, entity_type, sync_id, "upsert", 2, &Value::Object(row).to_string())
        .await
        .expect("apply")
}

/// A flag in the wire form the codec uses for it.
fn flag(row: &Payload, key: &str, on: bool) -> Value {
    match row.get(key) {
        Some(Value::Bool(_)) => Value::Bool(on),
        _ => Value::from(i64::from(on)),
    }
}

/// The account row as PC A sends it after a password reset by `author` at `at`.
async fn remote_reset(db: &DatabaseConnection, account: &str, author: Option<&str>, at: &str) -> Payload {
    let mut row = payload(db, "user_accounts", &format!("{account}-sync")).await;
    row.insert("password_hash".into(), REMOTE_HASH.into());
    row.insert("password_changed_at".into(), at.into());
    let named = author.map_or(Value::Null, |a| Value::String(a.to_string()));
    row.insert(AUTHOR.into(), named.clone());
    // A reset names its administrator as who set the password too (`admin_set_user_password`).
    row.insert(SET_BY.into(), named);
    row.insert(SET_AT.into(), at.into());
    let on = flag(&row, "force_password_change", true);
    row.insert("force_password_change".into(), on);
    row
}

/// The account row as PC A sends it after the account changed its own password at `at`: who set
/// the password it replaces (`password_set_*`) is kept, as `change_password_and_rotate` does.
fn own_change(mut row: Payload, account: &str, hash: &str, at: &str) -> Payload {
    row.insert("password_hash".into(), hash.into());
    row.insert("password_changed_at".into(), at.into());
    row.insert(AUTHOR.into(), format!("{account}-sync").into());
    let off = flag(&row, "force_password_change", false);
    row.insert("force_password_change".into(), off);
    row
}

async fn hash(db: &DatabaseConnection, account: &str) -> Option<String> {
    text(
        db,
        "SELECT password_hash FROM user_accounts WHERE username = ?",
        vec![account.into()],
    )
    .await
}

async fn audits(db: &DatabaseConnection, code: &str) -> i64 {
    scalar(
        db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = ?",
        vec![code.into()],
    )
    .await
    .unwrap_or(0)
}

async fn notices(db: &DatabaseConnection, key: &str) -> Vec<String> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT COALESCE(message_params_json, '') AS p FROM notification_events WHERE message_key = ? ORDER BY id",
        [key.into()],
    ))
    .await
    .expect("events")
    .iter()
    .map(|r| r.try_get::<String>("", "p").expect("params"))
    .collect()
}

fn applied(outcome: &ApplyOutcome) -> bool {
    matches!(outcome, ApplyOutcome::Applied)
}

/// A plant administrator ("chef"), a user manager ("manager": `adm.users` and `adm.roles`, given by
/// the chef), a plain account ("worker") and a plain author who never held the right ("clerk").
struct Plant {
    db: DatabaseConnection,
    chef: i64,
    manager: i64,
    worker: i64,
    operator: i64,
}

async fn plant() -> Plant {
    let db = crate::db::test_db::seeded().await;
    crate::sync::capture::install_capture_in(&db)
        .await
        .expect("capture + seeded identities");
    let chef = user(&db, "chef").await;
    let admin_role = role_id(&db, "Administrator").await;
    assign(&db, "chef-admin", chef, admin_role, None, &now()).await;
    let manager = user(&db, "manager").await;
    let managers = custom_role(&db, "user-managers", &[ADM_USERS, ADM_ROLES]).await;
    assign(
        &db,
        "manager-role",
        manager,
        managers,
        Some(chef),
        &stamp(chrono::Duration::days(-10)),
    )
    .await;
    let worker = user(&db, "worker").await;
    user(&db, "clerk").await;
    let operator = custom_role(&db, "field-operators", &[OT_VIEW]).await;
    Plant {
        db,
        chef,
        manager,
        worker,
        operator,
    }
}

/// The manager's right withdrawn here at `at` (as received earlier from another PC).
async fn demote_manager(p: &Plant, at: &str) {
    exec(
        &p.db,
        "UPDATE user_scope_assignments SET deleted_at = ? WHERE entity_sync_id = 'manager-role'",
        vec![at.into()],
    )
    .await;
}

// ── Accepted ───────────────────────────────────────────────────────────────────────────────────

/// 1. One's own change, 2. a reset by a plant administrator, 3. a reset by a current user manager
/// holding every permission of the account: applied.
#[tokio::test]
async fn t01_03_own_change_and_authorised_resets_are_applied() {
    let p = plant().await;
    let at = now();
    let own = own_change(
        payload(&p.db, "user_accounts", "worker-sync").await,
        "worker",
        REMOTE_HASH,
        &at,
    );
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", own).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some(REMOTE_HASH));
    assert_eq!(
        scalar(
            &p.db,
            "SELECT password_changed_by_id FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        Some(p.worker)
    );

    let mut by_chef = remote_reset(&p.db, "worker", Some("chef-sync"), &now()).await;
    by_chef.insert("password_hash".into(), "hash-by-chef".into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", by_chef).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("hash-by-chef"));

    let mut by_manager = remote_reset(&p.db, "worker", Some("manager-sync"), &now()).await;
    by_manager.insert("password_hash".into(), "hash-by-manager".into());
    assert!(applied(
        &receive(&p.db, "user_accounts", "worker-sync", by_manager).await
    ));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("hash-by-manager"));
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 0);
}

/// 4. A user manager resetting an account that can reach a permission the manager lacks, 5. an
/// administrator account reset by a user manager who is not a plant administrator: held back.
#[tokio::test]
async fn t04_05_a_stronger_account_needs_a_stronger_author() {
    let p = plant().await;
    assign(&p.db, "worker-op", p.worker, p.operator, Some(p.chef), &now()).await;
    let row = remote_reset(&p.db, "worker", Some("manager-sync"), &now()).await;
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"), "kept");

    let row = remote_reset(&p.db, "chef", Some("manager-sync"), &now()).await;
    assert!(applied(&receive(&p.db, "user_accounts", "chef-sync", row).await));
    assert_eq!(hash(&p.db, "chef").await.as_deref(), Some("x"), "administrator kept");
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 2);
    assert_eq!(notices(&p.db, "password_change_refused").await.len(), 2);
}

/// 6. The finding: the manager's right was withdrawn here at T; a reset dated after T (minus the
/// clock skew) keeps this PC's password, date and flag (no session ends here), applies the rest of
/// the row, and is audited and told to the administrators. 7. A reset dated well before T applies.
#[tokio::test]
async fn t06_07_a_reset_by_a_demoted_administrator_is_held_back() {
    let p = plant().await;
    let withdrawn = now();
    demote_manager(&p, &withdrawn).await;
    let before_date = text(
        &p.db,
        "SELECT password_changed_at FROM user_accounts WHERE id = ?",
        vec![p.worker.into()],
    )
    .await;
    let mut row = remote_reset(
        &p.db,
        "worker",
        Some("manager-sync"),
        &stamp(chrono::Duration::minutes(1)),
    )
    .await;
    row.insert("display_name".into(), "Worker renamed on A".into());
    let outcome = receive(&p.db, "user_accounts", "worker-sync", row).await;
    assert!(applied(&outcome), "{outcome:?}");
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));
    assert_eq!(
        text(
            &p.db,
            "SELECT password_changed_at FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        before_date,
        "no session ends here"
    );
    assert_eq!(
        scalar(
            &p.db,
            "SELECT force_password_change FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        Some(0)
    );
    assert_eq!(
        text(
            &p.db,
            "SELECT display_name FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await
        .as_deref(),
        Some("Worker renamed on A"),
        "the rest of the row applies"
    );
    assert_eq!(
        scalar(
            &p.db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_id = ? AND actor_id = ? \
               AND json_extract(details_json, '$.reason') = 'unauthorised'",
            vec![
                PASSWORD_CHANGE_UNAUTHORISED.into(),
                p.worker.to_string().into(),
                p.manager.into()
            ],
        )
        .await,
        Some(1)
    );
    let refused = notices(&p.db, "password_change_refused").await;
    assert_eq!(refused.len(), 1);
    assert!(refused[0].contains("manager"), "{}", refused[0]);
    assert_eq!(
        scalar(
            &p.db,
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
              WHERE e.message_key = 'password_change_refused' AND n.recipient_user_id = ?",
            vec![p.chef.into()],
        )
        .await,
        Some(1),
        "the plant administrator is told"
    );

    // Made two hours before the withdrawal: applied.
    let row = remote_reset(
        &p.db,
        "worker",
        Some("manager-sync"),
        &stamp(chrono::Duration::hours(-2)),
    )
    .await;
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some(REMOTE_HASH));
}

/// 8. An author whose role lost the right at a time not known here, 9. an author who never held the
/// right: held back (review SHOULD-FIX: no "applied for review" for a password).
#[tokio::test]
async fn t08_09_loss_unknown_and_never_held_are_held_back() {
    let p = plant().await;
    let legacy = user(&p.db, "legacy").await;
    let old = custom_role(&p.db, "old-managers", &[OT_VIEW]).await;
    assign(&p.db, "legacy-role", legacy, old, Some(p.chef), &now()).await;
    let row = remote_reset(&p.db, "worker", Some("legacy-sync"), &now()).await;
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    // Held back, not applied for review: a password costs nothing to keep.
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));
    assert!(notices(&p.db, "grants_from_former_authors").await.is_empty());
    let refused = notices(&p.db, "password_change_refused").await;
    assert_eq!(refused.len(), 1);
    assert!(refused[0].contains("legacy"), "{}", refused[0]);

    let mut row = remote_reset(&p.db, "worker", Some("clerk-sync"), &now()).await;
    row.insert("password_hash".into(), "hash-by-clerk".into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"), "kept");
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 2);
}

// ── Waits ──────────────────────────────────────────────────────────────────────────────────────

/// Everything this PC wrote so far counts as sent (the fixtures are not local edits racing the
/// received rows).
async fn settle_local_changes(db: &DatabaseConnection) {
    exec(db, "DELETE FROM sync_capture_journal", vec![]).await;
    exec(db, "DELETE FROM sync_outbox", vec![]).await;
}

async fn inbox(db: &DatabaseConnection, entity_type: &str, sync_id: &str, row: &Payload) {
    exec(
        db,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES (?, 'c1', ?, ?, 'upsert', 1, ?, ?, 'pending')",
        vec![
            format!("b-{sync_id}").into(),
            entity_type.into(),
            sync_id.into(),
            Value::Object(row.clone()).to_string().into(),
            format!("h-{sync_id}-{}", uuid::Uuid::new_v4()).into(),
        ],
    )
    .await;
}

async fn inbox_status(db: &DatabaseConnection, sync_id: &str) -> String {
    text(
        db,
        "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = ? ORDER BY id DESC",
        vec![sync_id.into()],
    )
    .await
    .expect("inbox row")
}

/// 10. An author not applied here yet: the row waits for that account. 11. It never arrives: after a
/// complete pull the row ends, the password held back.
#[tokio::test]
async fn t10_11_an_author_not_here_makes_the_row_wait_until_a_complete_pull() {
    let p = plant().await;
    let row = remote_reset(&p.db, "worker", Some("ghost-sync"), &now()).await;
    match receive(&p.db, "user_accounts", "worker-sync", row.clone()).await {
        ApplyOutcome::Deferred { waits_for, .. } => {
            let w = waits_for.expect("waits for the author");
            assert_eq!(
                (w.entity_type.as_str(), w.sync_id.as_str()),
                ("user_accounts", "ghost-sync")
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));

    settle_local_changes(&p.db).await;
    inbox(&p.db, "user_accounts", "worker-sync", &row).await;
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    assert_eq!(inbox_status(&p.db, "worker-sync").await, "deferred");
    crate::sync::grant_check::release_after_complete_pull(&p.db)
        .await
        .expect("release");
    // The author never arrived: the password is held back (not applied for review), the row ends.
    assert_eq!(inbox_status(&p.db, "worker-sync").await, "applied");
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));
    assert!(notices(&p.db, "grants_from_former_authors").await.is_empty());
    assert_eq!(notices(&p.db, "password_change_refused").await.len(), 1);
}

/// The wire form of an account created on another PC, removed here so it arrives as new.
async fn remote_new_account(db: &DatabaseConnection, username: &str, author: Option<i64>, at: &str) -> Payload {
    let id = user(db, username).await;
    exec(
        db,
        "UPDATE user_accounts SET password_hash = ?, password_changed_at = ?, password_changed_by_id = ?, \
          password_set_at = ?, password_set_by_id = ?, created_at = ?, force_password_change = 1 WHERE id = ?",
        vec![
            REMOTE_HASH.into(),
            at.into(),
            author.map_or(sea_orm::Value::BigInt(None), Into::into),
            at.into(),
            author.map_or(sea_orm::Value::BigInt(None), Into::into),
            at.into(),
            id.into(),
        ],
    )
    .await;
    let row = payload(db, "user_accounts", &format!("{username}-sync")).await;
    exec(db, "DELETE FROM user_accounts WHERE id = ?", vec![id.into()]).await;
    row
}

/// 12. Two accounts whose passwords name each other, neither here (first pull of a new PC): both
/// wait, and the complete pull ends both waits: the first is held back (its author never arrived),
/// the second is then judged against the first, who never held the right here. Both are created
/// without a password; nothing waits any longer.
#[tokio::test]
async fn t12_a_cycle_of_authors_ends_after_a_complete_pull() {
    let p = plant().await;
    let a = user(&p.db, "cycle-a").await;
    let b = user(&p.db, "cycle-b").await;
    let at = now();
    for (id, by) in [(a, b), (b, a)] {
        exec(
            &p.db,
            "UPDATE user_accounts SET password_hash = ?, password_changed_at = ?, password_changed_by_id = ? \
              WHERE id = ?",
            vec![REMOTE_HASH.into(), at.clone().into(), by.into(), id.into()],
        )
        .await;
    }
    let row_a = payload(&p.db, "user_accounts", "cycle-a-sync").await;
    let row_b = payload(&p.db, "user_accounts", "cycle-b-sync").await;
    exec(
        &p.db,
        "DELETE FROM user_accounts WHERE id IN (?, ?)",
        vec![a.into(), b.into()],
    )
    .await;
    settle_local_changes(&p.db).await;
    inbox(&p.db, "user_accounts", "cycle-a-sync", &row_a).await;
    inbox(&p.db, "user_accounts", "cycle-b-sync", &row_b).await;
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    for s in ["cycle-a-sync", "cycle-b-sync"] {
        assert_eq!(inbox_status(&p.db, s).await, "deferred", "{s}");
    }
    crate::sync::grant_check::release_after_complete_pull(&p.db)
        .await
        .expect("release");
    for s in ["cycle-a-sync", "cycle-b-sync"] {
        assert_eq!(inbox_status(&p.db, s).await, "applied", "{s}");
    }
    let with_password = scalar(
        &p.db,
        "SELECT COUNT(*) FROM user_accounts WHERE username IN ('cycle-a', 'cycle-b') AND password_hash IS NOT NULL",
        vec![],
    )
    .await;
    assert_eq!(
        with_password,
        Some(0),
        "an author who never arrived, and one who never held the right: both without a password"
    );
    assert!(notices(&p.db, "grants_from_former_authors").await.is_empty());
    assert_eq!(notices(&p.db, "password_change_refused").await.len(), 2);
}

// ── New accounts, older versions, unauthored ────────────────────────────────────────────────────

/// 13. A new PC receives an account whose password a demoted administrator set: the account is
/// created without a password (its role assignment still applies) and sign-in says why.
#[tokio::test]
async fn t13_a_new_account_with_a_refused_password_arrives_without_one() {
    let p = plant().await;
    let hire_row = remote_new_account(&p.db, "hire", Some(p.manager), &now()).await;
    demote_manager(&p, &stamp(chrono::Duration::hours(-1))).await;
    let outcome = receive(&p.db, "user_accounts", "hire-sync", hire_row).await;
    assert!(applied(&outcome), "{outcome:?}");
    let hire = scalar(&p.db, "SELECT id FROM user_accounts WHERE username = 'hire'", vec![])
        .await
        .expect("account created");
    assert_eq!(hash(&p.db, "hire").await, None, "no password here");
    assert_eq!(
        scalar(
            &p.db,
            "SELECT force_password_change FROM user_accounts WHERE id = ?",
            vec![hire.into()]
        )
        .await,
        Some(1)
    );
    assert!(crate::sync::credential_check::password_withheld_on_arrival(&p.db, hire)
        .await
        .expect("check"));
    assert!(
        !crate::sync::credential_check::password_withheld_on_arrival(&p.db, p.worker)
            .await
            .expect("check"),
        "an account with its password is not concerned"
    );
    let refused = notices(&p.db, "password_change_refused").await;
    assert_eq!(refused.len(), 1);
    assert!(refused[0].contains("\"new_account\":true"), "{}", refused[0]);

    // Its role, given by the plant administrator, applies.
    assign(&p.db, "hire-op", hire, p.operator, Some(p.chef), &now()).await;
    let role_row = payload(&p.db, "user_scope_assignments", "hire-op").await;
    exec(
        &p.db,
        "DELETE FROM user_scope_assignments WHERE entity_sync_id = 'hire-op'",
        vec![],
    )
    .await;
    let outcome = receive(&p.db, "user_scope_assignments", "hire-op", role_row).await;
    assert!(applied(&outcome), "{outcome:?}");
}

/// 14. Author key absent (a PC with an older version): a new account is accepted (first pull), a
/// change of an account held here is held back with the "older version" notice. 15. Key present but
/// naming nobody: held back.
#[tokio::test]
async fn t14_15_older_versions_and_unauthored_changes() {
    let p = plant().await;
    let mut old_row = remote_new_account(&p.db, "old-hire", None, &now()).await;
    for key in [AUTHOR, STATUS_AUTHOR, SET_BY, SET_AT] {
        old_row.remove(key);
    }
    assert!(applied(
        &receive(&p.db, "user_accounts", "old-hire-sync", old_row).await
    ));
    assert_eq!(hash(&p.db, "old-hire").await.as_deref(), Some(REMOTE_HASH));

    let mut row = remote_reset(&p.db, "worker", None, &now()).await;
    row.remove(AUTHOR);
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));
    assert_eq!(notices(&p.db, "password_change_from_older_version").await.len(), 1);

    let row = remote_reset(&p.db, "worker", None, &now()).await;
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));
    assert_eq!(
        scalar(
            &p.db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = ? \
               AND json_extract(details_json, '$.reason') = 'no_author'",
            vec![PASSWORD_CHANGE_UNAUTHORISED.into()],
        )
        .await,
        Some(1)
    );
}

/// 16. `force_password_change` alone: applied unchecked. 17. `password_changed_at` moved with the
/// same hash by a demoted administrator: held back (no session ends here).
#[tokio::test]
async fn t16_17_flag_alone_applies_a_moved_date_is_checked() {
    let p = plant().await;
    let mut row = payload(&p.db, "user_accounts", "worker-sync").await;
    let on = flag(&row, "force_password_change", true);
    row.insert("force_password_change".into(), on);
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(
        scalar(
            &p.db,
            "SELECT force_password_change FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        Some(1)
    );
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 0);

    demote_manager(&p, &stamp(chrono::Duration::hours(-1))).await;
    let before = text(
        &p.db,
        "SELECT password_changed_at FROM user_accounts WHERE id = ?",
        vec![p.worker.into()],
    )
    .await;
    let mut row = payload(&p.db, "user_accounts", "worker-sync").await;
    row.insert("password_changed_at".into(), now().into());
    row.insert(AUTHOR.into(), "manager-sync".into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(
        text(
            &p.db,
            "SELECT password_changed_at FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        before
    );
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 1);
}

/// 18. The refusal cannot be recorded (notice refused): the whole row waits, nothing is written.
/// 19. A row refused by another check (a username already used here) records no credential refusal.
#[tokio::test]
async fn t18_19_unrecorded_refusals_wait_and_other_refusals_record_nothing() {
    let p = plant().await;
    demote_manager(&p, &stamp(chrono::Duration::hours(-1))).await;
    exec(
        &p.db,
        "CREATE TRIGGER refuse_credential_notice BEFORE INSERT ON notification_events \
         WHEN NEW.message_key = 'password_change_refused' BEGIN SELECT RAISE(ABORT, 'disk full'); END",
        vec![],
    )
    .await;
    let mut row = remote_reset(&p.db, "worker", Some("manager-sync"), &now()).await;
    row.insert("display_name".into(), "Renamed".into());
    let outcome = receive(&p.db, "user_accounts", "worker-sync", row.clone()).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(
        text(
            &p.db,
            "SELECT display_name FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await
        .as_deref(),
        Some("worker"),
        "nothing applied"
    );
    assert_eq!(
        audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await,
        0,
        "audit rolled back"
    );
    exec(&p.db, "DROP TRIGGER refuse_credential_notice", vec![]).await;
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 1);

    let mut clash = remote_reset(&p.db, "worker", Some("manager-sync"), &now()).await;
    clash.insert("username".into(), "chef".into());
    let outcome = receive(&p.db, "user_accounts", "worker-sync", clash).await;
    assert!(
        matches!(&outcome, ApplyOutcome::Conflict { code, .. } if code == crate::sync::username_clash::USERNAME_CLASH),
        "{outcome:?}"
    );
    assert_eq!(
        audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await,
        1,
        "nothing more recorded"
    );
}

/// 20. On the PC where the manager reset a password after the withdrawal made elsewhere: the
/// withdrawal arrives and the reset is listed in the "grants to review" notice (nothing undone).
#[tokio::test]
async fn t20_the_withdrawal_lists_the_passwords_set_since() {
    let p = plant().await;
    exec(
        &p.db,
        "UPDATE user_accounts SET password_hash = 'temp', password_changed_by_id = ?, password_changed_at = ? \
          WHERE id = ?",
        vec![
            p.manager.into(),
            stamp(chrono::Duration::hours(-1)).into(),
            p.worker.into(),
        ],
    )
    .await;
    let mut withdrawal = payload(&p.db, "user_scope_assignments", "manager-role").await;
    withdrawal.insert("deleted_at".into(), stamp(chrono::Duration::hours(-2)).into());
    assert!(applied(
        &receive(&p.db, "user_scope_assignments", "manager-role", withdrawal).await
    ));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("temp"), "nothing undone");
    let listed = notices(&p.db, "admin_grants_to_review").await;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].contains("password of worker"), "{}", listed[0]);
}

/// 23. A new account whose own password names itself: applied, the link to itself completed after
/// the insert (soft self-reference), nothing left pending.
#[tokio::test]
async fn t23_a_self_authored_new_account_links_to_itself() {
    let p = plant().await;
    let id = user(&p.db, "self-made").await;
    exec(
        &p.db,
        "UPDATE user_accounts SET password_changed_by_id = id, password_hash = ? WHERE id = ?",
        vec![REMOTE_HASH.into(), id.into()],
    )
    .await;
    let row = payload(&p.db, "user_accounts", "self-made-sync").await;
    assert_eq!(row.get(AUTHOR).and_then(Value::as_str), Some("self-made-sync"));
    exec(&p.db, "DELETE FROM user_accounts WHERE id = ?", vec![id.into()]).await;
    assert!(applied(&receive(&p.db, "user_accounts", "self-made-sync", row).await));
    assert_eq!(
        scalar(
            &p.db,
            "SELECT COUNT(*) FROM user_accounts WHERE username = 'self-made' AND password_changed_by_id = id",
            vec![]
        )
        .await,
        Some(1)
    );
    assert_eq!(
        scalar(
            &p.db,
            "SELECT COUNT(*) FROM sync_pending_refs WHERE entity_sync_id = 'self-made-sync'",
            vec![]
        )
        .await,
        Some(0)
    );
    assert_eq!(hash(&p.db, "self-made").await.as_deref(), Some(REMOTE_HASH));
}

// ── Re-enable and restore (lead decision O6) ────────────────────────────────────────────────────

async fn disable(p: &Plant, user: i64) {
    exec(
        &p.db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![user.into()],
    )
    .await;
}

async fn active(p: &Plant, user: i64) -> Option<i64> {
    scalar(
        &p.db,
        "SELECT is_active FROM user_accounts WHERE id = ?",
        vec![user.into()],
    )
    .await
}

/// A plain account re-enabled by a demoted manager (or with no author) stays disabled here, the rest
/// of the row applies, audited and told; re-enabled by the plant administrator it applies; a disable
/// by anyone applies.
#[tokio::test]
async fn re_enabling_is_checked_disabling_never_is() {
    let p = plant().await;
    let mut enabled = payload(&p.db, "user_accounts", "worker-sync").await;
    disable(&p, p.worker).await;
    demote_manager(&p, &stamp(chrono::Duration::hours(-1))).await;
    enabled.insert(STATUS_AUTHOR.into(), "manager-sync".into());
    enabled.insert("updated_at".into(), now().into());
    enabled.insert("display_name".into(), "Back on A".into());
    assert!(applied(
        &receive(&p.db, "user_accounts", "worker-sync", enabled.clone()).await
    ));
    assert_eq!(active(&p, p.worker).await, Some(0), "stays disabled");
    assert_eq!(
        text(
            &p.db,
            "SELECT display_name FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await
        .as_deref(),
        Some("Back on A")
    );
    assert_eq!(audits(&p.db, ACCOUNT_REENABLE_UNAUTHORISED).await, 1);
    assert_eq!(notices(&p.db, "account_reenable_refused").await.len(), 1);

    let mut unauthored = enabled.clone();
    unauthored.insert(STATUS_AUTHOR.into(), Value::Null);
    assert!(applied(
        &receive(&p.db, "user_accounts", "worker-sync", unauthored).await
    ));
    assert_eq!(active(&p, p.worker).await, Some(0), "no author: stays disabled");

    let mut by_chef = enabled;
    by_chef.insert(STATUS_AUTHOR.into(), "chef-sync".into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", by_chef).await));
    assert_eq!(
        active(&p, p.worker).await,
        Some(1),
        "re-enabled by the plant administrator"
    );

    // Disabled by the demoted manager: applied (it only takes access away).
    let mut disabled = payload(&p.db, "user_accounts", "worker-sync").await;
    let off = flag(&disabled, "is_active", false);
    disabled.insert("is_active".into(), off);
    disabled.insert(STATUS_AUTHOR.into(), "manager-sync".into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", disabled).await));
    assert_eq!(active(&p, p.worker).await, Some(0));
    assert_eq!(audits(&p.db, ACCOUNT_REENABLE_UNAUTHORISED).await, 2);
}

/// A deleted account restored by someone who never held the right stays deleted here.
#[tokio::test]
async fn restoring_is_checked() {
    let p = plant().await;
    let mut restored = payload(&p.db, "user_accounts", "worker-sync").await;
    exec(
        &p.db,
        "UPDATE user_accounts SET deleted_at = ? WHERE id = ?",
        vec![now().into(), p.worker.into()],
    )
    .await;
    restored.insert("deleted_at".into(), Value::Null);
    restored.insert(STATUS_AUTHOR.into(), "clerk-sync".into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", restored).await));
    assert!(
        text(
            &p.db,
            "SELECT deleted_at FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await
        .is_some(),
        "stays deleted"
    );
    assert_eq!(audits(&p.db, ACCOUNT_REENABLE_UNAUTHORISED).await, 1);
}

// ── Who set the password (review BLOCKING: a takeover through the account's own change) ─────────

/// The attack chain: the manager, demoted on B at T, resets the worker's password on a stale PC A
/// after T, signs in as the worker on A with the temporary password and makes the forced change
/// there (author: the worker itself; who set the password: still the manager). PC B receives both
/// rows and keeps the worker's password each time, with a notice; a new PC C's first pull creates
/// the account without the password.
#[tokio::test]
async fn the_takeover_through_a_forced_change_is_held_back() {
    let b = plant().await;
    demote_manager(&b, &stamp(chrono::Duration::hours(-1))).await;
    let reset_at = now();
    // On A: the reset, then the forced change made "as the worker".
    let reset = remote_reset(&b.db, "worker", Some("manager-sync"), &reset_at).await;
    let forced = own_change(
        reset.clone(),
        "worker",
        "hash-chosen-by-the-attacker",
        &stamp(chrono::Duration::minutes(2)),
    );
    assert_eq!(forced.get(SET_BY).and_then(Value::as_str), Some("manager-sync"));

    assert!(applied(&receive(&b.db, "user_accounts", "worker-sync", reset).await));
    assert_eq!(
        hash(&b.db, "worker").await.as_deref(),
        Some("x"),
        "the reset is held back"
    );
    let outcome = receive(&b.db, "user_accounts", "worker-sync", forced.clone()).await;
    assert!(applied(&outcome), "{outcome:?}");
    assert_eq!(
        hash(&b.db, "worker").await.as_deref(),
        Some("x"),
        "the change made with the temporary password is held back too"
    );
    assert_eq!(audits(&b.db, PASSWORD_CHANGE_UNAUTHORISED).await, 2);
    let refused = notices(&b.db, "password_change_refused").await;
    assert_eq!(refused.len(), 2);
    assert!(
        refused[1].contains("manager"),
        "named: who set the password: {}",
        refused[1]
    );
    assert_eq!(
        scalar(
            &b.db,
            "SELECT password_set_by_id FROM user_accounts WHERE id = ?",
            vec![b.worker.into()]
        )
        .await,
        None,
        "B keeps who set its password"
    );

    // A new PC C: its first pull brings the account after the manager's withdrawal.
    let c = plant().await;
    demote_manager(&c, &stamp(chrono::Duration::hours(-1))).await;
    exec(&c.db, "DELETE FROM user_accounts WHERE id = ?", vec![c.worker.into()]).await;
    let outcome = receive(&c.db, "user_accounts", "worker-sync", forced).await;
    assert!(applied(&outcome), "{outcome:?}");
    assert_eq!(hash(&c.db, "worker").await, None, "created without the password");
    let id = scalar(&c.db, "SELECT id FROM user_accounts WHERE username = 'worker'", vec![])
        .await
        .expect("created");
    assert!(crate::sync::credential_check::password_withheld_on_arrival(&c.db, id)
        .await
        .expect("check"));
}

/// A legitimate reset by the plant administrator, then the account's own change in its profile:
/// accepted on a PC that received the reset first, and on one that receives only the change.
#[tokio::test]
async fn an_own_change_after_a_legitimate_reset_is_accepted() {
    let p = plant().await;
    let reset = remote_reset(&p.db, "worker", Some("chef-sync"), &now()).await;
    let changed = own_change(
        reset.clone(),
        "worker",
        "hash-chosen-by-the-worker",
        &stamp(chrono::Duration::minutes(5)),
    );
    // Only the change (the reset row was superseded on the server).
    assert!(applied(
        &receive(&p.db, "user_accounts", "worker-sync", changed.clone()).await
    ));
    assert_eq!(
        hash(&p.db, "worker").await.as_deref(),
        Some("hash-chosen-by-the-worker")
    );

    // The reset first, then the change.
    let q = plant().await;
    assert!(applied(&receive(&q.db, "user_accounts", "worker-sync", reset).await));
    assert_eq!(hash(&q.db, "worker").await.as_deref(), Some(REMOTE_HASH));
    assert!(applied(&receive(&q.db, "user_accounts", "worker-sync", changed).await));
    assert_eq!(
        hash(&q.db, "worker").await.as_deref(),
        Some("hash-chosen-by-the-worker")
    );
    assert_eq!(audits(&q.db, PASSWORD_CHANGE_UNAUTHORISED).await, 0);
}

/// Who set the password changed alone (same hash): judged like a password change; the key missing
/// on an account held here (an older version): refused.
#[tokio::test]
async fn a_changed_or_missing_password_root_is_judged() {
    let p = plant().await;
    let mut row = payload(&p.db, "user_accounts", "worker-sync").await;
    row.insert(SET_BY.into(), "clerk-sync".into());
    row.insert(SET_AT.into(), now().into());
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(
        scalar(
            &p.db,
            "SELECT password_set_by_id FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        None,
        "not taken"
    );
    assert_eq!(audits(&p.db, PASSWORD_CHANGE_UNAUTHORISED).await, 1);

    let mut row = own_change(
        payload(&p.db, "user_accounts", "worker-sync").await,
        "worker",
        "hash-from-old-pc",
        &now(),
    );
    row.remove(SET_BY);
    assert!(applied(&receive(&p.db, "user_accounts", "worker-sync", row).await));
    assert_eq!(hash(&p.db, "worker").await.as_deref(), Some("x"));
    assert_eq!(notices(&p.db, "password_change_from_older_version").await.len(), 1);
}
