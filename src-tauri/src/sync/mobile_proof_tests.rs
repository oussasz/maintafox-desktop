//! Mobile sync proof (Android field app, step 0): the desktop side of a run where a real desktop
//! database and the real Android app on a phone sync one work order and one intervention request
//! through the real control API, on this machine only.
//!
//! Run it with `pnpm sync:harness --mobile-proof` (`scripts/sync-harness.mjs`): the script starts
//! the database and the API on 127.0.0.1 (fixed port, reached by the phone through
//! `adb reverse`), issues a licence with two desktops and one phone, and passes the connection
//! details in the file named by `MAINTAFOX_SYNC_HARNESS_CONFIG`. Without that file the test is
//! ignored by `cargo test`.
//!
//! What the desktop does, with the engine's own functions:
//! 1. activation: licence check, key-entry check, claim and key entry, tenant initialisation;
//! 2. plant join: the real sync worker (production access: entitlement + stored activation token)
//!    completes the first pull, so "the server confirmed no accounts" holds; then the first
//!    administrator is created;
//! 3. a technician account (Maintenance Technician, plant-wide) with a generated password, printed
//!    on stdout only;
//! 4. one equipment on the root site and one work order assigned to the technician (submitted,
//!    planned, assigned: it is in "planning" and would pass "mark ready");
//! 5. sync, `MOBILE_PROOF_READY`: what the phone operator enters;
//! 6. sync every 5 s until an intervention request made on another device arrives
//!    (`MAINTAFOX_MOBILE_PROOF_WAIT_SECS`, default 1200);
//! 7. the work order is marked ready on the desktop and synced: `MOBILE_PROOF_DOWNLINK`; the API
//!    then stays up `MAINTAFOX_MOBILE_PROOF_HOLD_SECS` (default 300) for the phone to pull it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Deserialize;

use super::harness_tests::require_local;
use super::transport::worker::{CycleOutcome, LicenseSyncAccess};
use super::transport::{SyncTiming, SyncWorker, SyncWorkerConfig, SyncWorkerHandle};
use crate::commands::product_license::{self as licence, ActivationCallResult, LicenceCaller};

const CONFIG_ENV: &str = "MAINTAFOX_SYNC_HARNESS_CONFIG";
const WAIT_ENV: &str = "MAINTAFOX_MOBILE_PROOF_WAIT_SECS";
const HOLD_ENV: &str = "MAINTAFOX_MOBILE_PROOF_HOLD_SECS";
const POLL: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct ProofConfig {
    api_base_url: String,
    tenant_id: String,
    license_key: String,
}

fn secs_from_env(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a number of seconds, not {v:?}")),
        _ => default,
    }
}

/// The desktop: its database, its worker status and its app data directory.
struct Desktop {
    db: DatabaseConnection,
    handle: SyncWorkerHandle,
    media_root: std::path::PathBuf,
    api: String,
}

impl Desktop {
    /// One cycle of the production worker (licence access, stored activation token).
    async fn cycle(&self) -> CycleOutcome {
        let worker = SyncWorker::new(
            self.db.clone(),
            self.handle.clone(),
            SyncWorkerConfig {
                api_base_url: self.api.clone(),
                media_root: self.media_root.clone(),
                timing: SyncTiming::production(),
            },
            Arc::new(LicenseSyncAccess),
        )
        .expect("sync worker");
        worker.cycle_once().await.0
    }

    /// Cycles until nothing is left to send or receive; returns the last outcome.
    async fn sync(&self) -> CycleOutcome {
        for _ in 0..30 {
            match self.cycle().await {
                CycleOutcome::Synced { more_work: true } => continue,
                other => return other,
            }
        }
        panic!("the desktop still had work after 30 cycles");
    }

    async fn synced(&self, stage: &str) {
        let outcome = self.sync().await;
        assert!(
            matches!(outcome, CycleOutcome::Synced { more_work: false }),
            "{stage}: sync did not complete: {outcome:?}"
        );
    }
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<i64>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn answered<T: std::fmt::Debug>(what: &str, result: ActivationCallResult<T>) -> T {
    match result {
        ActivationCallResult::Answered { answer } => answer,
        other => panic!("{what}: the control API did not answer: {other:?}"),
    }
}

/// A stored time (RFC 3339, or `YYYY-MM-DD HH:MM:SS` in UTC) as an instant.
fn parse_time(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|t| t.and_utc())
        })
}

#[tokio::test]
#[ignore = "needs the control API, Postgres and a phone: run `pnpm sync:harness --mobile-proof`"]
async fn mobile_proof() {
    let path = std::env::var(CONFIG_ENV)
        .unwrap_or_else(|_| panic!("{CONFIG_ENV} is not set: run `pnpm sync:harness --mobile-proof`"));
    let config: ProofConfig =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("proof config")).expect("proof config JSON");
    require_local(&config.api_base_url);
    // Warnings (an envelope not applied, a refused row) in the test output.
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_test_writer()
        .try_init();
    let wait_secs = secs_from_env(WAIT_ENV, 1200);
    let hold_secs = secs_from_env(HOLD_ENV, 300);
    let api = config.api_base_url.clone();

    // An encrypted database file opened like the app opens it, migrations and startup seeders run.
    let media_root = std::env::temp_dir().join(format!("mfx-mobile-proof-desktop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&media_root).expect("media root");
    let db = super::test_support::started_file_db(&media_root.join("maintafox.db")).await;
    let desktop = Desktop {
        db: db.clone(),
        handle: SyncWorkerHandle::new(),
        media_root,
        api: api.clone(),
    };

    // 1. Activation, as the licence gate runs it (signed out, first start).
    let signed_out = LicenceCaller::default();
    let fingerprint = format!("fp-mobile-proof-desktop-{}", uuid::Uuid::new_v4().simple());
    let version = licence::running_app_version();
    let preview = answered(
        "licence check",
        licence::preview_product_activation_in(
            &api,
            &config.license_key,
            &fingerprint,
            Some("Mobile proof desktop"),
            Some(version),
        )
        .await
        .expect("licence check"),
    );
    licence::check_product_license_key_entry_in(&db, signed_out, &preview.to_string())
        .await
        .expect("this PC accepts the licence");
    let slot = licence::PendingActivationClaimSlot::default();
    let claim = answered(
        "activation claim",
        licence::claim_product_activation_in(
            &slot,
            &api,
            &config.license_key,
            &fingerprint,
            Some("Mobile proof desktop"),
            Some(version),
        )
        .await
        .expect("activation claim"),
    );
    let tenant_id = claim["tenant_id"].as_str().expect("claimed tenant").to_string();
    assert_eq!(tenant_id, config.tenant_id, "the licence belongs to the proof tenant");
    licence::submit_pending_product_license_key_in(
        &db,
        signed_out,
        &slot,
        &config.license_key,
        &tenant_id,
        Some(version.to_string()),
    )
    .await
    .expect("key entry");
    if claim["is_initialized"].as_bool() != Some(true) {
        let status = answered(
            "tenant initialisation",
            licence::mark_activation_tenant_initialized_in(&db, &api)
                .await
                .expect("tenant initialisation"),
        );
        assert!(status.is_initialized, "the server marks the tenant initialised");
    }
    println!("[mobile-proof] desktop activated for tenant {tenant_id} (machine {fingerprint})");
    let sync_right = crate::entitlements::queries::check_entitlement_capability(&db, "sync".into())
        .await
        .expect("sync entitlement");
    assert!(
        sync_right.allowed,
        "the licence lets this desktop sync after its activation: {sync_right:?}"
    );

    // 2. Plant join: the first complete pull confirms the organisation has no account yet.
    desktop.synced("first pull").await;
    assert!(
        crate::sync::plant_join::server_confirmed_no_accounts(&db, &tenant_id)
            .await
            .expect("plant join fact"),
        "the first complete pull confirms that the server has no account"
    );
    let state = licence::get_activation_bootstrap_state_in(&db, &desktop.handle.status())
        .await
        .expect("first-start state");
    assert_eq!(
        state.plant_join,
        Some(crate::sync::plant_join::PlantJoinState::CreateFirstAdmin),
        "the first-start screen offers the first administrator"
    );
    let policy = crate::auth::password_policy::PasswordPolicy::load(&db).await;
    let password_length = policy.min_length.max(16);
    let admin_username = "proof.admin";
    licence::bootstrap_initial_tenant_admin_in(
        &db,
        licence::BootstrapInitialAdminInput {
            username: admin_username.into(),
            email: "proof.admin@mobile-proof.test".into(),
            password: crate::commands::admin_users::generate_temporary_password(password_length),
            display_name: Some("Proof administrator".into()),
        },
    )
    .await
    .expect("first administrator");
    let admin_id = scalar_i64(
        &db,
        "SELECT id FROM user_accounts WHERE username = ? AND deleted_at IS NULL",
        vec![admin_username.into()],
    )
    .await;
    let admin = crate::auth::session_manager::AuthenticatedUser {
        user_id: i32::try_from(admin_id).expect("user id"),
        username: admin_username.into(),
        display_name: Some("Proof administrator".into()),
        is_admin: true,
        force_password_change: false,
        tenant_id: tenant_id.clone(),
        token_tenant_id: tenant_id.clone(),
    };
    println!("[mobile-proof] plant joined; first administrator created");

    // 3. The technician: seeded role "Maintenance Technician", plant-wide.
    let app_state = crate::state::AppState::new(db.clone());
    let technician_role = scalar_i64(
        &db,
        "SELECT id FROM roles WHERE name = 'Maintenance Technician' AND deleted_at IS NULL",
        vec![],
    )
    .await;
    let technician_username = "tech.mobile";
    let technician_password = crate::commands::admin_users::generate_temporary_password(password_length);
    let technician_id = crate::commands::admin_users::create_user_then_retire_builtin_admin(
        &app_state,
        &admin,
        crate::commands::admin_users::CreateUserInput {
            username: technician_username.into(),
            identity_mode: "local".into(),
            role_id: technician_role,
            scope_node_id: None,
            personnel_id: None,
            initial_password: Some(technician_password.clone()),
            // The field app has no password-change screen yet: the operator signs in with it.
            force_password_change: Some(false),
        },
    )
    .await
    .expect("technician account")
    .id;
    let site_id = scalar_i64(
        &db,
        "SELECT id FROM org_nodes WHERE parent_id IS NULL AND deleted_at IS NULL ORDER BY id LIMIT 1",
        vec![],
    )
    .await;
    let technician_user = i32::try_from(technician_id).expect("user id");
    for permission in [
        crate::rbac::permissions::OT_VIEW,
        crate::rbac::permissions::OT_EDIT,
        crate::rbac::permissions::DI_CREATE,
    ] {
        assert!(
            crate::auth::rbac::check_permission(
                &db,
                technician_user,
                permission,
                &crate::auth::rbac::PermissionScope::OrgNode(site_id.to_string()),
            )
            .await
            .expect("permission check"),
            "the technician holds {permission} at the site"
        );
    }

    // 4. Equipment on the site, from the published equipment catalogues.
    let catalog = crate::assets::taxonomy_reference::list_equipment_taxonomy_catalog(&db)
        .await
        .expect("equipment catalogues");
    let class = catalog.classes.first().expect("a published equipment class");
    let criticality = catalog.criticalities.first().expect("a published criticality");
    let status = catalog
        .statuses
        .iter()
        .find(|s| s.semantic_tag.as_deref() == Some("up"))
        .expect("a published in-service equipment status");
    let suffix = uuid::Uuid::new_v4().simple().to_string()[..4].to_ascii_uppercase();
    let equipment = crate::assets::identity::create_asset(
        &db,
        crate::assets::identity::CreateAssetPayload {
            asset_code: format!("MPROOF-{suffix}"),
            asset_name: "Pompe de preuve mobile".into(),
            class_code: class.code.clone(),
            family_code: None,
            subfamily_code: None,
            criticality_code: criticality.code.clone(),
            status_code: status.code.clone(),
            manufacturer: None,
            model: None,
            serial_number: None,
            maintainable_boundary: true,
            rams_schedule_reference_value_id: None,
            rams_utilization_factor: None,
            org_node_id: site_id,
            commissioned_at: None,
            commercial: None,
        },
        admin.user_id,
    )
    .await
    .expect("equipment");

    // The work order: created, submitted, planned and assigned to the technician.
    let work_type = crate::wo::types::list_work_order_types(&db)
        .await
        .expect("work order types")
        .into_iter()
        .find(|t| t.is_active && t.code == "corrective")
        .expect("the corrective work order type");
    let wo = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            type_code: work_type.code,
            equipment_id: Some(equipment.id),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_code: None,
            subtype_code: None,
            title: "Preuve mobile : contrôler la pompe".into(),
            description: Some("Ordre de travail de la preuve de synchronisation mobile.".into()),
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: admin_id,
            requires_permit: None,
        },
    )
    .await
    .expect("work order");
    let wo = crate::wo::workflow::actions::submit_wo(
        &db,
        crate::wo::workflow::actions::WoSubmitInput {
            wo_id: wo.id,
            actor_id: admin_id,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit the work order");
    let tomorrow = chrono::Utc::now().date_naive() + chrono::Days::new(1);
    let wo = crate::wo::execution::plan_wo(
        &db,
        crate::wo::execution::WoPlanInput {
            wo_id: wo.id,
            actor_id: admin_id,
            expected_row_version: wo.row_version,
            planner_id: admin_id,
            planned_start: Some(format!("{tomorrow}T08:00:00Z")),
            planned_end: Some(format!("{tomorrow}T10:00:00Z")),
            shift: None,
            shift_class_version_id: None,
            clear_shift: false,
            expected_duration_hours: Some(2.0),
            planned_crew_size: None,
            clear_crew_size: false,
            urgency_code: None,
            planned_downtime_hours: None,
        },
    )
    .await
    .expect("plan the work order");
    let wo = crate::wo::execution::assign_wo(
        &db,
        crate::wo::execution::WoAssignInput {
            wo_id: wo.id,
            actor_id: admin_id,
            expected_row_version: wo.row_version,
            assigned_team_id: None,
            primary_responsible_id: Some(technician_id),
            scheduled_at: None,
            override_reason: "Preuve mobile : affecté au technicien de terrain".into(),
        },
    )
    .await
    .expect("assign the work order");
    assert_eq!(wo.primary_responsible_id, Some(technician_id));
    // The change sent later ("mark ready") must not be refused after the phone operator's work.
    let readiness = crate::wo::workflow::readiness::evaluate_wo_readiness(&db, wo.id)
        .await
        .expect("readiness");
    assert!(
        readiness.can_mark_ready,
        "the work order can be marked ready later: {readiness:?}"
    );

    // 5. Everything reaches the server; the phone may now be activated.
    desktop.synced("desktop data sent").await;
    println!(
        "MOBILE_PROOF_READY api={api} licence_key={} username={technician_username} password={technician_password} work_order={} work_order_status={} \
         equipment={}",
        config.license_key,
        wo.code,
        wo.status_code.as_deref().unwrap_or("?"),
        equipment.asset_code,
    );
    println!(
        "[mobile-proof] on the phone: activate with the licence key, wait for the plant join, sign in as \
         {technician_username}, check that {} is in \"My work orders\", then report a fault on {} (waiting up to \
         {wait_secs} s)",
        wo.code, equipment.asset_code
    );

    if std::env::var("MAINTAFOX_MOBILE_PROOF_JOIN_PROBE").is_ok_and(|v| v == "1") {
        // Its own thread and runtime with a large stack: a second device's futures nested inside
        // this test overflow the default test-thread stack in a debug build.
        let (probe_api, probe_key, probe_user) =
            (api.clone(), config.license_key.clone(), technician_username.to_string());
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("probe runtime")
                    .block_on(join_probe(&probe_api, &probe_key, &probe_user));
            })
            .expect("probe thread")
            .join()
            .expect("join probe");
        return;
    }

    // 6. Wait for an intervention request made elsewhere.
    let known: Vec<i64> = crate::di::queries::list_intervention_requests(
        &db,
        crate::di::queries::DiListFilter {
            limit: 500,
            ..Default::default()
        },
    )
    .await
    .expect("requests")
    .items
    .into_iter()
    .map(|di| di.id)
    .collect();
    let waiting_since = Instant::now();
    let mut last_problem: Option<String> = None;
    let arrived = loop {
        let outcome = desktop.sync().await;
        if !matches!(outcome, CycleOutcome::Synced { more_work: false }) {
            let problem = format!("{outcome:?}");
            if last_problem.as_deref() != Some(problem.as_str()) {
                println!("[mobile-proof] sync while waiting: {problem}");
                last_problem = Some(problem);
            }
        }
        let found = crate::di::queries::list_intervention_requests(
            &db,
            crate::di::queries::DiListFilter {
                limit: 500,
                ..Default::default()
            },
        )
        .await
        .expect("requests")
        .items
        .into_iter()
        .find(|di| !known.contains(&di.id));
        if let Some(di) = found {
            break di;
        }
        assert!(
            waiting_since.elapsed() < Duration::from_secs(wait_secs),
            "no intervention request from another device arrived within {wait_secs} s"
        );
        tokio::time::sleep(POLL).await;
    };
    let arrived_at = chrono::Utc::now();
    let created_at = parse_time(&arrived.created_at)
        .unwrap_or_else(|| panic!("unreadable created_at of {}: {}", arrived.code, arrived.created_at));
    let uplink = arrived_at - created_at;
    println!(
        "MOBILE_PROOF_DI code={} subject={:?} equipment={} submitter_id={} created_at={} arrived_at={} \
         uplink_seconds={}",
        arrived.code,
        arrived.title,
        arrived.asset_code.as_deref().unwrap_or("?"),
        arrived.submitter_id,
        arrived.created_at,
        arrived_at.to_rfc3339(),
        uplink.num_seconds()
    );
    assert_eq!(
        arrived.equipment_id, equipment.id,
        "the request is on the proof equipment"
    );
    assert_eq!(
        arrived.submitter_id, technician_id,
        "the request was made by the technician"
    );

    // 7. Downlink: the work order is marked ready here and sent.
    let wo = crate::wo::workflow::actions::mark_wo_ready(
        &db,
        crate::wo::workflow::actions::WoMarkReadyInput {
            wo_id: wo.id,
            actor_id: admin_id,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("mark the work order ready");
    let sent = Instant::now();
    desktop.synced("downlink sent").await;
    println!(
        "MOBILE_PROOF_DOWNLINK work_order={} status={} sent_at={} push_ms={}",
        wo.code,
        wo.status_code.as_deref().unwrap_or("?"),
        chrono::Utc::now().to_rfc3339(),
        sent.elapsed().as_millis()
    );
    println!(
        "MOBILE_PROOF_SUMMARY uplink_di={} uplink_seconds={} downlink_work_order={} downlink_status={} \
         (the downlink arrival is checked on the phone: refresh \"My work orders\")",
        arrived.code,
        uplink.num_seconds(),
        wo.code,
        wo.status_code.as_deref().unwrap_or("?")
    );

    // The API stays up for the phone to pull the change.
    println!("[mobile-proof] the API stays up {hold_secs} s for the phone (Ctrl+C ends earlier)");
    let holding = Instant::now();
    while holding.elapsed() < Duration::from_secs(hold_secs) {
        tokio::time::sleep(POLL).await;
        let _ = desktop.sync().await;
    }
}

/// Diagnostic (`MAINTAFOX_MOBILE_PROOF_JOIN_PROBE=1`): a second device joins the proof plant exactly
/// as the activation screens run it (licence check, claim, the local data reset a first activation
/// makes, key entry, first pull) and prints what waits in its inbox and whether the technician
/// gets a tenant scope there. Takes the licence's second desktop slot.
async fn join_probe(api: &str, key: &str, technician_username: &str) {
    let media_root = std::env::temp_dir().join(format!("mfx-join-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&media_root).expect("media root");
    let db = super::test_support::started_file_db(&media_root.join("maintafox.db")).await;
    let device = Desktop {
        db: db.clone(),
        handle: SyncWorkerHandle::new(),
        media_root,
        api: api.to_string(),
    };
    let signed_out = LicenceCaller::default();
    let fingerprint = format!("fp-join-probe-{}", uuid::Uuid::new_v4().simple());
    let version = licence::running_app_version();
    let preview = answered(
        "probe licence check",
        licence::preview_product_activation_in(api, key, &fingerprint, Some("Join probe"), Some(version))
            .await
            .expect("licence check"),
    );
    licence::check_product_license_key_entry_in(&db, signed_out, &preview.to_string())
        .await
        .expect("this PC accepts the licence");
    let slot = licence::PendingActivationClaimSlot::default();
    let claim = answered(
        "probe activation claim",
        licence::claim_product_activation_in(&slot, api, key, &fingerprint, Some("Join probe"), Some(version))
            .await
            .expect("activation claim"),
    );
    let tenant_id = claim["tenant_id"].as_str().expect("claimed tenant").to_string();
    // As `ProductLicenseGate` / the phone's activation screen: no bound tenant yet → local reset.
    let wiped = licence::reset_local_tenant_runtime_data_impl(&db)
        .await
        .expect("local reset");
    println!("[join-probe] local reset wiped {wiped} rows");
    licence::submit_pending_product_license_key_in(&db, signed_out, &slot, key, &tenant_id, Some(version.to_string()))
        .await
        .expect("key entry");
    let outcome = device.sync().await;
    println!("[join-probe] first pull: {outcome:?}");
    let report = crate::sync::waiting::list_waiting_inbox_rows(&db, Some(50))
        .await
        .expect("waiting rows");
    println!("[join-probe] waiting rows: {report:#?}");
    let user_id = scalar_i64(
        &db,
        "SELECT coalesce(max(id), 0) FROM user_accounts WHERE username = ?",
        vec![technician_username.into()],
    )
    .await;
    let scope = crate::auth::session_manager::resolve_tenant_scope_for_user(
        &db,
        i32::try_from(user_id).expect("user id"),
        &tenant_id,
    )
    .await
    .expect("tenant scope");
    println!(
        "[join-probe] {technician_username} (local id {user_id}) tenant scope here: {:?}",
        scope.is_some()
    );
}
