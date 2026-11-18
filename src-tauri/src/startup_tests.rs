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
                "capture suspended",
                crate::sync::capture::suspend_capture(db),
            )
            .await
            .expect("capture suspended");
            timed(p, commits, "migrations (none pending)", crate::db::run_migrations(db))
                .await
                .expect("migrations");
            timed(p, commits, "system seed", crate::db::seeder::seed_system_data(db))
                .await
                .expect("system seed");
            timed(p, commits, "work-order types and urgency scale", async {
                crate::wo::types::ensure_work_order_type_catalog(db).await?;
                crate::reference::urgency_scale::ensure_urgency_scale(db).await
            })
            .await
            .expect("work-order types and urgency scale");
            timed(
                p,
                commits,
                "work-order statuses",
                crate::wo::statuses::ensure_required_work_order_statuses(db),
            )
            .await
            .expect("work-order statuses");
            timed(
                p,
                commits,
                "reference catalog integrity",
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
        db.close().await.expect("close");
    }
}
