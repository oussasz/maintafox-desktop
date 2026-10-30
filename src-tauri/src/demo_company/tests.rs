//! Demo-company generator tests. Each test builds an app-like empty database in a temporary
//! FILE (the simulated-clock VFS needs one), with the startup sequence of `sync::test_support`.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};

use super::guard::{ensure_demo_company_licence, ensure_no_business_data, ensure_no_received_sync_data};
use super::{generate, preflight, GenerationOptions, GenerationReport};
use crate::clock::simulated::{register_vfs, SimClock};

static VFS_SEQ: AtomicUsize = AtomicUsize::new(0);

/// A database file as the app leaves it on first start (migrations, system seed, catalogs).
async fn empty_company_file(path: &Path) {
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let db = Database::connect(&url).await.expect("create database file");
    crate::sync::test_support::prepare_started(&db).await;
    db.close().await.expect("close");
}

/// The licence of `name` activated on this PC, as a successful activation leaves it: the licence
/// record (`product.license_onboarding`, with the organisation's display name from the claim, the
/// field the vendor console fills), the licensing trust keys, then the startup's tenant bootstrap
/// (root organisation named after the licence, `db::tenant_bootstrap`).
async fn activate_licence(path: &Path, name: &str) {
    let url = format!("sqlite://{}?mode=rw", path.display());
    let db = Database::connect(&url).await.expect("open");
    crate::license::security::ensure_default_licensing_trust_keys(&db)
        .await
        .expect("trust keys");
    let record = serde_json::json!({
        "schema_version": 1,
        "key_sha256": "test",
        "submitted_at": "2025-01-01T00:00:00Z",
        "status": "active",
        "pending_online_validation": false,
        "activation_claim": {
            "tenant_id": "tenant-tpn",
            "license_id": "licence-tpn",
            "machine_fingerprint": "pc-tpn",
            "activation_token": "token-tpn",
            "tenant_display_name": name,
        },
        "reconciliation": { "retry_attempt": 0 },
        "diagnostics": [],
        "company_display_name": name,
    });
    crate::settings::set_setting(
        &db,
        "product.license_onboarding",
        "device",
        &record.to_string(),
        None,
        "test activation",
    )
    .await
    .expect("licence record");
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(&db, None)
        .await
        .expect("tenant bootstrap");
    crate::entitlements::queries::get_entitlement_summary(&db)
        .await
        .expect("entitlements");
    crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("retired lists");
    db.close().await.expect("close");
}

/// A database of the demo company as the app leaves it on first start after the activation.
async fn fresh_company_file(path: &Path) {
    empty_company_file(path).await;
    activate_licence(path, super::catalog::COMPANY_NAME).await;
}

/// Opens `path` under a fresh simulated clock, as the CLI does.
async fn open_simulated(path: &Path) -> (DatabaseConnection, SimClock) {
    let clock = SimClock::new();
    let name = format!("maintafox-demo-test-{}", VFS_SEQ.fetch_add(1, Ordering::SeqCst));
    register_vfs(&name, &clock).expect("vfs");
    let db = crate::db::open_existing_with_vfs(path, &name)
        .await
        .expect("open with vfs");
    (db, clock)
}

async fn run(path: &Path, opts: &GenerationOptions) -> (DatabaseConnection, GenerationReport) {
    let (db, clock) = open_simulated(path).await;
    let _guard = clock.install_on_this_thread();
    let report = generate(&db, &clock, opts).await.expect("generation");
    (db, report)
}

async fn rows(db: &DatabaseConnection, sql: &str) -> Vec<String> {
    db.query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .iter()
        .map(|r| r.try_get_by_index::<String>(0).unwrap_or_else(|e| panic!("{sql}: {e}")))
        .collect()
}

/// Business content without the per-device parts (sync ids, device code in document numbers).
async fn fingerprint(db: &DatabaseConnection, report: &GenerationReport) -> Vec<String> {
    let mut out: Vec<String> = report.counts.iter().map(|(k, v)| format!("count {k}={v}")).collect();
    for sql in [
        "SELECT 'wo ' || substr(w.code, length(w.code) - 3) || ' ' || COALESCE(w.title,'') || ' ' || s.code || ' ' \
           || COALESCE(w.created_at,'') || ' ' || COALESCE(w.closed_at,'') || ' ' || printf('%.2f', COALESCE(w.total_cost,0)) \
         FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id ORDER BY w.created_at, w.id",
        "SELECT 'di ' || substr(code, length(code) - 3) || ' ' || status || ' ' || COALESCE(submitted_at,'') FROM intervention_requests ORDER BY submitted_at, id",
        "SELECT 'pm ' || p.code || ' ' || o.due_at || ' ' || o.status FROM pm_occurrences o JOIN pm_plans p ON p.id = o.pm_plan_id ORDER BY o.due_at, p.code",
        "SELECT 'stock ' || a.article_code || ' ' || b.location_id || ' ' || printf('%.2f', b.on_hand_qty) FROM stock_balances b JOIN articles a ON a.id = b.article_id ORDER BY a.article_code, b.location_id",
        "SELECT 'permit ' || status || ' ' || COALESCE(activated_at,'') FROM work_permits ORDER BY requested_at",
        "SELECT 'failure ' || e.asset_id_code || ' ' || COALESCE(f.failed_at,'') || ' ' || printf('%.2f', COALESCE(f.active_repair_hours,0)) FROM failure_events f JOIN equipment e ON e.id = f.equipment_id ORDER BY f.failed_at, e.asset_id_code",
    ] {
        out.extend(rows(db, sql).await);
    }
    out
}

/// A fresh company database opened under a simulated clock, with the id of its administrator.
async fn fresh_opened() -> (tempfile::TempDir, DatabaseConnection) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("company.db");
    fresh_company_file(&path).await;
    let (db, _clock) = open_simulated(&path).await;
    preflight(&db).await.expect("a freshly activated demo company passes");
    (dir, db)
}

async fn first_id(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index(0)
        .expect("id")
}

fn refused_with(result: crate::errors::AppResult<()>, needle: &str) {
    let err = result.expect_err("refused");
    assert!(err.to_string().contains(needle), "{err}");
}

#[tokio::test]
async fn refuses_one_more_user_account() {
    let (_dir, db) = fresh_opened().await;
    let admin = first_id(&db, "SELECT id FROM user_accounts ORDER BY id LIMIT 1").await;
    let caller = crate::auth::session_manager::AuthenticatedUser {
        user_id: i32::try_from(admin).expect("id"),
        username: "admin".into(),
        display_name: None,
        is_admin: true,
        force_password_change: false,
        tenant_id: String::new(),
        token_tenant_id: String::new(),
    };
    let role_id = first_id(&db, "SELECT id FROM roles WHERE name = 'Administrator'").await;
    let state = crate::state::AppState::new(db.clone());
    for username in ["premier.admin", "second.compte"] {
        crate::commands::admin_users::create_user_impl(
            &state,
            &caller,
            crate::commands::admin_users::CreateUserInput {
                username: username.into(),
                identity_mode: "hybrid".into(),
                role_id,
                scope_node_id: None,
                personnel_id: None,
                initial_password: None,
                force_password_change: Some(true),
            },
        )
        .await
        .expect("account");
        if username == "premier.admin" {
            // The first administrator besides the built-in account is allowed.
            ensure_no_business_data(&db)
                .await
                .expect("one administrator is not company data");
        }
    }
    refused_with(ensure_no_business_data(&db).await, "user_accounts");
}

#[tokio::test]
async fn refuses_a_custom_role_or_an_organisation_node() {
    let (_dir, db) = fresh_opened().await;
    // A role created in the company (its identity is not derived from a seeded name).
    db.execute_unprepared(
        "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at, row_version) \
         VALUES ('6f0c1c52-2f4c-4f0e-9a51-6c3c1f1b2a10', 'Chef d''atelier', 'Rôle maison', 0, 'custom', 'active', \
         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
    )
    .await
    .expect("custom role");
    refused_with(ensure_no_business_data(&db).await, "roles");

    let (_dir2, db) = fresh_opened().await;
    // A draft organisation model (not the activation's) is company data.
    crate::org::structure_model::create_model(
        &db,
        crate::org::structure_model::CreateStructureModelPayload { description: None },
        1,
    )
    .await
    .expect("model");
    refused_with(ensure_no_business_data(&db).await, "org_structure_models");
}

#[tokio::test]
async fn refuses_an_edited_reference_list() {
    let (_dir, db) = fresh_opened().await;
    let domain = first_id(&db, "SELECT id FROM reference_domains WHERE code = 'EQUIPMENT.CLASS'").await;
    crate::reference::sets::create_draft_set(&db, domain, 1)
        .await
        .expect("new version of a list");
    refused_with(ensure_no_business_data(&db).await, "reference_sets");
}

#[tokio::test]
async fn refuses_the_licence_of_another_company() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("company.db");
    empty_company_file(&path).await;
    let (db, _clock) = open_simulated(&path).await;
    refused_with(ensure_demo_company_licence(&db).await, "Aucune licence");
    db.close().await.expect("close");

    activate_licence(&path, "Sidérurgie Réelle SPA").await;
    let (db, clock) = open_simulated(&path).await;
    refused_with(ensure_demo_company_licence(&db).await, "Sidérurgie Réelle SPA");
    let refused = generate(
        &db,
        &clock,
        &GenerationOptions::new(1, NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()),
    )
    .await;
    assert!(refused.is_err());
    assert!(super::guard::same_company(
        "  TELLMÉTAL profilés du nord ",
        super::catalog::COMPANY_NAME
    ));
}

/// The checkpoint of the complete pull that onboarding needs before the first administrator.
const ONBOARDING_PULL: &str = "INSERT INTO sync_checkpoint (id, checkpoint_token, last_sync_at) \
     VALUES (1, 'cp-1', '2026-01-01T00:00:00Z') \
     ON CONFLICT(id) DO UPDATE SET checkpoint_token = 'cp-1', last_sync_at = '2026-01-01T00:00:00Z'";

#[tokio::test]
async fn accepts_the_checkpoint_of_the_onboarding_pull() {
    let (_dir, db) = fresh_opened().await;
    db.execute_unprepared(ONBOARDING_PULL).await.expect("pull applied");
    preflight(&db)
        .await
        .expect("a complete pull that brought nothing is accepted");
}

#[tokio::test]
async fn refuses_a_database_that_received_sync_data() {
    let (_dir, db) = fresh_opened().await;
    db.execute_unprepared(ONBOARDING_PULL).await.expect("pull applied");
    db.execute_unprepared(
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) \
         VALUES ('b-1', 'cp-1', 'equipment', 'eq-1', 'upsert', 1, '{}', 'h-1', 'pending')",
    )
    .await
    .expect("row received");
    refused_with(ensure_no_received_sync_data(&db).await, "1 éléments reçus");
    refused_with(preflight(&db).await, "synchronisation");
}

#[tokio::test]
async fn refuses_an_account_received_with_the_checkpoint() {
    let (_dir, db) = fresh_opened().await;
    db.execute_unprepared(ONBOARDING_PULL).await.expect("pull applied");
    // The administrator created at onboarding, then an account of the plant that a pull brought
    // (applied, so it sits in `user_accounts` too).
    let columns = rows(&db, "SELECT name FROM pragma_table_info('user_accounts')").await;
    let copied: Vec<&str> = columns
        .iter()
        .map(String::as_str)
        .filter(|c| !matches!(*c, "id" | "username" | "entity_sync_id"))
        .collect();
    for (username, sync_id) in [
        ("premier.admin", "onboarding-admin"),
        ("compte.recu", "received-account"),
    ] {
        db.execute_unprepared(&format!(
            "INSERT INTO user_accounts (username, entity_sync_id, {cols}) \
             SELECT '{username}', '{sync_id}', {cols} FROM user_accounts ORDER BY id LIMIT 1",
            cols = copied.join(", ")
        ))
        .await
        .expect("account");
        if username == "premier.admin" {
            preflight(&db).await.expect("the onboarding administrator is accepted");
        }
    }
    refused_with(preflight(&db).await, "user_accounts");
}

/// The only steps allowed to stop are the ones that reached the end of the history.
fn assert_only_end_of_history_warnings(report: &GenerationReport) {
    let refusals: Vec<&String> = report
        .warnings
        .iter()
        .filter(|w| !w.contains("après la fin de l'historique"))
        .collect();
    assert!(refusals.is_empty(), "service refusals: {refusals:#?}");
}

/// No audit event and no company record is dated after the end of the history (the rows the
/// first start and the activation wrote before the generation are left out: system tables).
async fn assert_nothing_after_the_end(db: &DatabaseConnection, report: &GenerationReport) {
    use super::guard::{RULED_TABLES, SYSTEM_TABLES};
    let limit = report.limit.expect("limit").format("%Y-%m-%dT%H:%M:%S").to_string();
    let tables = rows(db, "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").await;
    let mut late = Vec::new();
    let mut checked = 0;
    for table in tables {
        let company_table = !SYSTEM_TABLES.contains(&table.as_str()) && !RULED_TABLES.contains(&table.as_str());
        if !(company_table || table == "audit_events") {
            continue;
        }
        let columns = rows(
            db,
            &format!(
                "SELECT name FROM pragma_table_info('{table}') WHERE name IN \
                 ('created_at', 'updated_at', 'happened_at', 'occurred_at', 'recorded_at', 'closed_at', 'completed_at')"
            ),
        )
        .await;
        for column in columns {
            checked += 1;
            let n = rows(
                db,
                &format!(
                    "SELECT CAST(COUNT(*) AS TEXT) FROM \"{table}\" \
                     WHERE {column} IS NOT NULL AND substr(replace({column}, ' ', 'T'), 1, 19) > '{limit}'"
                ),
            )
            .await;
            if n[0] != "0" {
                late.push(format!("{table}.{column}: {}", n[0]));
            }
        }
    }
    assert!(checked > 50, "only {checked} date columns checked");
    assert!(late.is_empty(), "dated after {limit}: {late:#?}");
}

#[tokio::test]
async fn same_seed_and_day_give_the_same_company() {
    let today = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
    let opts = GenerationOptions {
        seed: 7,
        today,
        history_days: 45,
    };
    let mut prints = Vec::new();
    for _ in 0..2 {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("company.db");
        fresh_company_file(&path).await;
        let (db, report) = run(&path, &opts).await;
        assert_only_end_of_history_warnings(&report);
        assert_nothing_after_the_end(&db, &report).await;
        prints.push(fingerprint(&db, &report).await);
        db.close().await.expect("close");
    }
    assert!(prints[0].len() > 100, "history too small: {}", prints[0].len());
    assert_eq!(prints[0], prints[1]);
}

#[tokio::test]
async fn a_full_year_gives_sensible_kpis() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("company.db");
    fresh_company_file(&path).await;
    let today = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
    let (db, report) = run(&path, &GenerationOptions::new(super::DEFAULT_SEED, today)).await;
    let k = &report.kpis;
    eprintln!("counts: {:#?}", report.counts);
    eprintln!(
        "PM {:?}% (due {}, on time {}, late {}, not done {}); backlog {:.0} h = {:?} weeks; open OT {}, in progress {}, open DI {}; closed OT {}; cost {:.0} DZD",
        k.pm_compliance_pct, k.pm_due, k.pm_on_time, k.pm_late, k.pm_not_done, k.backlog_hours, k.backlog_weeks,
        k.open_work_orders, k.in_progress_work_orders, k.open_requests, k.closed_work_orders, k.total_cost
    );
    for a in &k.assets {
        eprintln!(
            "{} failures={} mtbf={:?} mttr={:?} cost={:.0}",
            a.code, a.failures, a.mtbf_hours, a.mttr_hours, a.cost
        );
    }
    eprintln!("warnings: {:#?}", report.warnings);
    assert_only_end_of_history_warnings(&report);
    assert_nothing_after_the_end(&db, &report).await;

    let mtbf = k.mtbf_values();
    assert!(mtbf.len() >= 40, "MTBF computed for {} assets", mtbf.len());
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let mtbf_median = median(mtbf.clone());
    assert!((300.0..=3000.0).contains(&mtbf_median), "median MTBF {mtbf_median}");
    let in_range = mtbf.iter().filter(|m| (300.0..=3000.0).contains(*m)).count();
    assert!(
        in_range * 10 >= mtbf.len() * 8,
        "{in_range}/{} MTBF in 300–3000 h",
        mtbf.len()
    );
    let mttr = k.mttr_values();
    let mttr_median = median(mttr.clone());
    assert!((1.0..=8.0).contains(&mttr_median), "median MTTR {mttr_median}");
    let pm = k.pm_compliance_pct.expect("PM compliance");
    assert!((80.0..=95.0).contains(&pm), "PM compliance {pm}");
    let weeks = k.backlog_weeks.expect("backlog weeks");
    assert!((2.0..=5.0).contains(&weeks), "backlog {weeks} weeks");
    assert!(
        k.in_progress_work_orders + k.open_requests > 0,
        "nothing in progress at the end"
    );
    let machines_with_cost = k.assets.iter().filter(|a| a.cost > 0.0).count();
    assert!(machines_with_cost >= 40, "{machines_with_cost} assets with costs");
}
