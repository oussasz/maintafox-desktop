#[cfg(test)]
mod startup_tests {
    use crate::startup::{format_startup_message, validate_startup_duration, StartupEvent};
    use std::time::Instant;

    // ── StartupEvent serialization ──────────────────────────────────────

    #[test]
    fn db_ready_event_serializes_correctly() {
        let event = StartupEvent::DbReady;
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"db_ready""#));
    }

    #[test]
    fn migrations_complete_event_includes_applied_count() {
        let event = StartupEvent::MigrationsComplete { applied: 3 };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"migrations_complete""#));
        assert!(json.contains(r#""applied":3"#));
    }

    #[test]
    fn failed_event_includes_reason() {
        let event = StartupEvent::Failed {
            reason: "DB locked".to_string(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"failed""#));
        assert!(json.contains(r#""reason":"DB locked""#));
    }

    #[test]
    fn ready_event_serializes_as_snake_case_stage() {
        let event = StartupEvent::Ready;
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"ready""#));
    }

    // ── Startup timing helpers ──────────────────────────────────────────

    #[test]
    fn validate_startup_within_budget_returns_true() {
        let start = Instant::now();
        let (elapsed, within) = validate_startup_duration(start, 4_000);
        assert!(
            elapsed < 4_000,
            "Test itself took longer than the budget — CI machine is too slow"
        );
        assert!(within, "Instant startup should always be within 4000ms budget");
    }

    #[test]
    fn format_startup_message_within_budget() {
        let msg = format_startup_message(350, true, 4_000);
        assert!(msg.contains("350ms"), "Message must include elapsed time");
        assert!(msg.contains("within"), "Message must say 'within' for under-budget");
        assert!(msg.contains("4000ms"), "Message must include the budget value");
    }

    #[test]
    fn format_startup_message_over_budget() {
        let msg = format_startup_message(5_200, false, 4_000);
        assert!(msg.contains("5200ms"), "Message must include elapsed time");
        assert!(
            msg.to_lowercase().contains("warning"),
            "Message must contain a warning indicator"
        );
        assert!(msg.contains("4000ms"), "Message must include the budget value");
    }

    // ── Database work of a normal start, measured ──────────────────────

    /// The database work of a normal start, phase by phase: how long it took and how many
    /// transactions it committed (each commit of the encrypted WAL file is a disk sync).
    pub(super) mod normal_start {
        use std::future::Future;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;
        use std::time::Instant;

        use sea_orm::DatabaseConnection;

        pub const CAPTURE_SUSPENDED: &str = "capture suspended";
        pub const SYSTEM_SEED: &str = "system seed";
        pub const WORK_ORDER_CATALOGS: &str = "work-order types and urgency scale";
        pub const WORK_ORDER_STATUSES: &str = "work-order statuses";
        pub const REFERENCE_INTEGRITY: &str = "reference catalog integrity";

        /// Phases that only re-apply what is already there on a normal start: each one is a
        /// single transaction, so a single commit (one sync of the file to disk), however many
        /// statements it runs.
        pub const ONE_COMMIT_PHASES: &[&str] = &[
            CAPTURE_SUSPENDED,
            SYSTEM_SEED,
            WORK_ORDER_CATALOGS,
            WORK_ORDER_STATUSES,
            REFERENCE_INTEGRITY,
        ];

        /// Fails when a phase of [`ONE_COMMIT_PHASES`] committed more than once.
        pub fn assert_one_commit_phases(phases: &[Phase]) {
            for name in ONE_COMMIT_PHASES {
                let phase = phases
                    .iter()
                    .find(|p| p.name == *name)
                    .unwrap_or_else(|| panic!("phase {name} not run"));
                assert!(
                    phase.commits <= 1,
                    "{name}: {} commits, each one a sync of the database file to disk",
                    phase.commits
                );
            }
        }

        pub struct Phase {
            pub name: &'static str,
            pub ms: u128,
            pub commits: u64,
        }

        /// Counts the commits made on the connection of `db` (the app's pool has one connection).
        pub async fn count_commits(db: &DatabaseConnection) -> Arc<AtomicU64> {
            let commits = Arc::new(AtomicU64::new(0));
            let counter = Arc::clone(&commits);
            let mut conn = db
                .get_sqlite_connection_pool()
                .acquire()
                .await
                .expect("database connection");
            let mut handle = conn.lock_handle().await.expect("connection handle");
            handle.set_commit_hook(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            });
            commits
        }

        async fn timed<T>(
            phases: &mut Vec<Phase>,
            commits: &AtomicU64,
            name: &'static str,
            step: impl Future<Output = T>,
        ) -> T {
            let before = commits.load(Ordering::SeqCst);
            let started = Instant::now();
            let out = step.await;
            phases.push(Phase {
                name,
                ms: started.elapsed().as_millis(),
                commits: commits.load(Ordering::SeqCst) - before,
            });
            out
        }

        /// The database steps of `startup::run_startup_sequence` from Phase 2 to Phase 4a'', in its
        /// order, on a database already started once (no migration pending). The steps the
        /// startup treats as non-fatal are not asserted here either.
        pub async fn run(db: &DatabaseConnection, commits: &AtomicU64) -> Vec<Phase> {
            let mut phases = Vec::new();
            let p = &mut phases;
            timed(
                p,
                commits,
                CAPTURE_SUSPENDED,
                crate::sync::capture::suspend_capture_atomically(db),
            )
            .await
            .expect("capture suspended");
            timed(p, commits, "migrations (none pending)", crate::db::run_migrations(db))
                .await
                .expect("migrations");
            timed(p, commits, SYSTEM_SEED, crate::db::seeder::seed_system_data(db))
                .await
                .expect("system seed");
            timed(
                p,
                commits,
                WORK_ORDER_CATALOGS,
                crate::startup::enforce_work_order_catalogs(db),
            )
            .await
            .expect("work-order types and urgency scale");
            timed(
                p,
                commits,
                WORK_ORDER_STATUSES,
                crate::startup::enforce_work_order_statuses(db),
            )
            .await
            .expect("work-order statuses");
            timed(
                p,
                commits,
                REFERENCE_INTEGRITY,
                crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(db),
            )
            .await
            .expect("reference catalog integrity");
            let _ = timed(
                p,
                commits,
                "certification expiry scan",
                crate::qualification::expiry_alerts::scan_due_certification_alerts(db),
            )
            .await;
            timed(p, commits, "tenant bootstrap and organisation heal", async {
                let _ = crate::db::tenant_bootstrap::bootstrap_from_activation_claim(db, None).await;
                let _ = crate::org::model_scope::heal_null_structure_model_ids_onto_active(db).await;
            })
            .await;
            timed(p, commits, "licensing trust keys and entitlements", async {
                let _ = crate::license::security::ensure_default_licensing_trust_keys(db).await;
                let _ = crate::entitlements::queries::get_entitlement_summary(db).await;
            })
            .await;
            timed(p, commits, "sync capture installed", async {
                crate::sync::capture::install_capture(db).await?;
                crate::sync::capture::set_capture_actor(db, None).await
            })
            .await
            .expect("sync capture installed");
            timed(p, commits, "reference retirement, reservations, permits", async {
                let _ = crate::reference::retirement::retire_superseded_domains(db).await;
                let _ = crate::inventory::reservations::startup_reservation_upkeep(db).await;
                let _ = crate::permit::ended_hold::sweep(db).await;
                let _ = crate::permit::requirement_rules::catch_up(db).await;
            })
            .await;
            phases
        }

        pub fn report(title: &str, phases: &[Phase]) -> String {
            let mut out = format!("{title}\n");
            for phase in phases {
                out.push_str(&format!(
                    "  {:<45} {:>7} ms {:>6} commit(s)\n",
                    phase.name, phase.ms, phase.commits
                ));
            }
            let total: u128 = phases.iter().map(|p| p.ms).sum();
            let commits: u64 = phases.iter().map(|p| p.commits).sum();
            out.push_str(&format!("  {:<45} {total:>7} ms {commits:>6} commit(s)\n", "total"));
            out
        }
    }

    /// Synchronised rows of the site whose normal start was measured at 6.9 s (0.9.3, 2026-11-18).
    const SITE_SYNCED_ROWS: i64 = 34_000;

    /// An encrypted database file opened like the app opens it (`db::init_db`).
    async fn open_like_the_app(
        path: &std::path::Path,
        key: &crate::db::encryption::DatabaseKey,
    ) -> sea_orm::DatabaseConnection {
        let db = crate::db::connect_sqlcipher(path, Some(key), true)
            .await
            .expect("encrypted database file");
        crate::db::apply_connection_pragmas(&db).await.expect("pragmas");
        db
    }

    /// Rows of the synchronised tables.
    async fn synced_rows(db: &sea_orm::DatabaseConnection) -> i64 {
        let mut tables = std::collections::BTreeSet::new();
        for spec in crate::sync::row_spec::ROW_SPECS {
            let layout = crate::sync::row_layout::layout(db, spec.entity_type)
                .await
                .expect("layout");
            tables.insert(layout.table().to_string());
        }
        let mut total = 0;
        for table in tables {
            total +=
                crate::sync::test_support::scalar_i64(db, &format!("SELECT COUNT(*) FROM \"{table}\""), vec![]).await;
        }
        total
    }

    /// A site-size database started once by the app (migrations, seeders, capture), then filled
    /// with about [`SITE_SYNCED_ROWS`] synchronised rows of every entity type.
    async fn site_size_database(path: &std::path::Path, key: &crate::db::encryption::DatabaseKey) -> i64 {
        use sea_orm::ConnectionTrait;
        let db = open_like_the_app(path, key).await;
        crate::sync::test_support::prepare_started(&db).await;
        // Test setup only: the fill is not measured, so it does not wait for the disk per row.
        db.execute_unprepared("PRAGMA synchronous = OFF").await.expect("fill");
        let mut factory = crate::sync::test_support::RowFactory::new();
        let mut types: Vec<&str> = crate::sync::row_spec::ROW_SPECS.iter().map(|s| s.entity_type).collect();
        let mut rows = synced_rows(&db).await;
        while rows < SITE_SYNCED_ROWS && !types.is_empty() {
            let mut refused = Vec::new();
            for entity_type in &types {
                // A type that takes one row per parent or per plant refuses the next one: the
                // other types make up the volume.
                if factory.create(&db, entity_type).await.is_err() {
                    refused.push(*entity_type);
                }
            }
            types.retain(|t| !refused.contains(t));
            rows = synced_rows(&db).await;
        }
        // On a site the changes are staged for sending (drained) at every push and sign-out: a
        // normal start finds the capture journal empty, not holding the whole fill.
        crate::sync::capture::set_capture_actor(&db, None)
            .await
            .expect("capture journal drained");
        db.close().await.expect("close");
        rows
    }

    /// A normal start (second launch, nothing to migrate) of an encrypted database file the size
    /// of a real site: the time and the commits of each database phase of the startup, printed
    /// for the startup time gate (`--ignored --nocapture`). Minutes in a debug build: not part of
    /// the ordinary test run.
    #[tokio::test]
    #[ignore = "site-size measurement, run by the startup time gate"]
    async fn normal_start_of_a_site_size_database() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("maintafox.db");
        let key = crate::db::encryption::DatabaseKey::generate();
        let fill_started = std::time::Instant::now();
        let rows = site_size_database(&path, &key).await;
        eprintln!(
            "site-size database: {rows} synchronised rows, {} MB, built in {} s",
            std::fs::metadata(&path).map(|m| m.len() / 1_000_000).unwrap_or(0),
            fill_started.elapsed().as_secs()
        );
        assert!(rows >= SITE_SYNCED_ROWS, "only {rows} synchronised rows");

        let db = open_like_the_app(&path, &key).await;
        let commits = normal_start::count_commits(&db).await;
        let phases = normal_start::run(&db, &commits).await;
        eprintln!(
            "{}",
            normal_start::report("normal start of a site-size database (debug build)", &phases)
        );
        normal_start::assert_one_commit_phases(&phases);
        db.close().await.expect("close");
    }

    /// A normal start re-applies the system data without one commit per statement: the phases
    /// that only re-apply what is there each commit once.
    #[tokio::test]
    async fn a_normal_start_commits_each_system_data_phase_once() {
        let db = crate::sync::test_support::started_db().await;
        let commits = normal_start::count_commits(&db).await;
        let phases = normal_start::run(&db, &commits).await;
        eprintln!("{}", normal_start::report("normal start (test database)", &phases));
        normal_start::assert_one_commit_phases(&phases);
    }

    /// The seed is one transaction, but a failing step still keeps what the steps before it wrote,
    /// as when each statement was committed on its own (the startup goes on without the rest).
    #[tokio::test]
    async fn a_failing_seed_step_keeps_what_the_steps_before_it_wrote() {
        use crate::sync::test_support::{exec, scalar_opt_text};
        let db = crate::sync::test_support::started_db().await;
        let permission = &crate::rbac::permissions::CATALOG[0];
        // Step 3 (permissions) puts the catalog description back; step 4 (seed version) fails.
        exec(
            &db,
            "UPDATE permissions SET description = 'stale' WHERE name = ?",
            vec![permission.name.into()],
        )
        .await;
        exec(
            &db,
            "CREATE TEMP TRIGGER injected_seed_failure BEFORE INSERT ON system_config \
             WHEN NEW.key = 'seed_schema_version' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
            vec![],
        )
        .await;

        let seeded = crate::db::seeder::seed_system_data(&db).await;

        assert!(seeded.is_err(), "the injected failure stops the seed");
        let description = scalar_opt_text(
            &db,
            "SELECT description FROM permissions WHERE name = ?",
            vec![permission.name.into()],
        )
        .await;
        assert_eq!(description.as_deref(), Some(permission.description));
    }

    async fn class_audience(db: &sea_orm::DatabaseConnection) -> Option<String> {
        crate::sync::test_support::scalar_opt_text(
            db,
            "SELECT audience FROM reference_domains WHERE code = 'EQUIPMENT.CLASS'",
            vec![],
        )
        .await
    }

    /// The reference catalog integrity pass is one transaction: when a step fails, the startup
    /// stops and nothing the pass wrote before is kept; the next start runs it whole.
    #[tokio::test]
    async fn a_failing_reference_integrity_pass_changes_nothing() {
        use crate::sync::test_support::exec;
        let db = crate::sync::test_support::started_db().await;
        // The first step puts the audience of the equipment classes back; the last one fails.
        exec(
            &db,
            "UPDATE reference_domains SET audience = 'system' WHERE code = 'EQUIPMENT.CLASS'",
            vec![],
        )
        .await;
        exec(
            &db,
            &format!(
                "CREATE TEMP TRIGGER injected_integrity_failure BEFORE UPDATE ON reference_domains \
                 WHEN UPPER(TRIM(NEW.code)) = '{}' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
                crate::planning::break_in_reasons::BREAK_IN_REASONS_DOMAIN
            ),
            vec![],
        )
        .await;

        let failed = crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db).await;

        assert!(failed.is_err(), "the injected failure stops the pass");
        assert_eq!(class_audience(&db).await.as_deref(), Some("system"), "nothing kept");

        exec(&db, "DROP TRIGGER temp.injected_integrity_failure", vec![]).await;
        crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("integrity pass");
        assert_eq!(class_audience(&db).await.as_deref(), Some("company"), "repaired");
    }
}
