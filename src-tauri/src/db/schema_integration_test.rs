#[cfg(test)]
mod tests {
    /// Tests that all 7 Phase 1 migrations apply cleanly against a
    /// freshly created in-memory SQLite database.
    ///
    /// This test is the primary fast-feedback guard against:
    /// - Migration SQL syntax errors
    /// - Wrong column type declarations
    /// - Duplicate column names
    /// - Missing IF NOT EXISTS guards
    #[tokio::test]
    async fn all_migrations_apply_to_clean_database() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("Failed to open in-memory SQLite");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("Failed to enable foreign keys");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migration failed — check migration SQL for syntax errors");

        let expected_tables = [
            "system_config",
            "trusted_devices",
            "audit_events",
            "app_sessions",
            "user_accounts",
            "roles",
            "permissions",
            "role_permissions",
            "user_scope_assignments",
            "lookup_domains",
            "lookup_values",
            "lookup_value_aliases",
            "org_structure_models",
            "org_node_types",
            "org_type_relationship_rules",
            "org_nodes",
            "org_node_responsibilities",
            "org_entity_bindings",
            "equipment_classes",
            "equipment",
            "equipment_hierarchy",
            "equipment_meters",
            "equipment_lifecycle_events",
            "skill_categories",
            "skill_definitions",
            "teams",
            "team_skill_requirements",
            "app_settings",
            "secure_secret_refs",
            "connection_profiles",
            "policy_snapshots",
            "settings_change_events",
            // Migration 009 — org audit
            "org_change_events",
            // Migration 010 — asset identity
            "asset_external_ids",
        ];

        for table in &expected_tables {
            let sql = format!("SELECT COUNT(*) FROM {};", table);
            db.execute(Statement::from_string(DbBackend::Sqlite, sql))
                .await
                .unwrap_or_else(|e| panic!("Table '{}' is missing or inaccessible: {}", table, e));
        }
    }

    /// Verify critical column presence on the equipment table per PRD §7.1 principles
    #[tokio::test]
    async fn equipment_table_has_required_sync_columns() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("Migrations");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(equipment);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info failed");

        let columns: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap_or_default())
            .collect();

        for required in &[
            "id",
            "entity_sync_id",
            "asset_id_code",
            "created_at",
            "updated_at",
            "deleted_at",
            "row_version",
            "origin_machine_id",
            "last_synced_checkpoint",
        ] {
            assert!(
                columns.contains(&required.to_string()),
                "equipment table is missing required column: {}",
                required
            );
        }
    }

    /// Verify equipment_lifecycle_events does NOT have deleted_at (append-only rule)
    #[tokio::test]
    async fn lifecycle_events_table_is_append_only() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("Migrations");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(equipment_lifecycle_events);".to_string(),
            ))
            .await
            .expect("PRAGMA");

        let columns: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap_or_default())
            .collect();

        assert!(
            !columns.contains(&"deleted_at".to_string()),
            "equipment_lifecycle_events must NOT have deleted_at — it is append-only"
        );

        assert!(
            !columns.contains(&"updated_at".to_string()),
            "equipment_lifecycle_events must NOT have updated_at — it is append-only"
        );
    }

    #[tokio::test]
    async fn default_settings_seed_populates_baseline_and_keeps_settings_audit_empty() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("Migrations");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};

        // Migrations may write the change history of a setting they retire (migration 259 keeps the
        // last value of `inventory.count_critical_variance` there): seeding itself adds nothing.
        let events = |db: &sea_orm::DatabaseConnection| {
            let db = db.clone();
            async move {
                db.query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT COUNT(*) AS cnt FROM settings_change_events;".to_string(),
                ))
                .await
                .expect("Count query for settings_change_events should succeed")
                .expect("Count query for settings_change_events should return one row")
                .try_get::<i64>("", "cnt")
                .expect("Count column should be readable")
            }
        };
        let events_before_seed = events(&db).await;

        crate::db::seeder::seed_default_settings(&db)
            .await
            .expect("Default settings seeding should succeed");

        // Migrations may also INSERT OR IGNORE into app_settings; assert baseline keys, not a brittle total.
        for key in [
            "locale.primary_language",
            "locale.currency_code",
            "appearance.color_mode",
            "backup.retention_daily",
            "updater.release_channel",
        ] {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT setting_key FROM app_settings WHERE setting_key = ? LIMIT 1",
                    [key.into()],
                ))
                .await
                .expect("lookup")
                .unwrap_or_else(|| panic!("baseline setting `{key}` must exist after seed"));
            let found: String = row.try_get("", "setting_key").expect("setting_key");
            assert_eq!(found, key);
        }

        let currency_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT setting_value_json AS v FROM app_settings WHERE setting_key = 'locale.currency_code' LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("currency lookup")
            .expect("locale.currency_code must exist");
        let currency_json: String = currency_row.try_get("", "v").expect("v");
        assert_eq!(
            currency_json, r#""DZD""#,
            "tenant currency seed default must be DZD (customizable in Settings)"
        );

        assert_eq!(
            events(&db).await,
            events_before_seed,
            "seeding must not write to settings_change_events (read-only seeding)"
        );
    }

    /// When only device-scoped rows remain (e.g. after `DELETE ... WHERE setting_scope = 'tenant'`),
    /// baseline tenant settings must still be re-inserted — otherwise the Settings UI loses categories.
    #[tokio::test]
    async fn default_settings_seed_fills_missing_tenant_rows_when_table_not_empty() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("Migrations");

        let now = chrono::Utc::now().to_rfc3339();
        for (key, cat, scope, val) in [
            ("updater.release_channel", "system", "device", r#""stable""#),
            ("updater.auto_check", "system", "device", r"true"),
            ("diagnostics.log_retention_days", "system", "device", r"30"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO app_settings
                       (setting_key, category, setting_scope, setting_value_json,
                        setting_risk, validation_status, last_modified_at)
                   VALUES (?, ?, ?, ?, 'low', 'valid', ?)",
                [key.into(), cat.into(), scope.into(), val.into(), now.clone().into()],
            ))
            .await
            .expect("device-only seed insert");
        }

        crate::db::seeder::seed_default_settings(&db)
            .await
            .expect("Default settings seeding should succeed");

        for key in [
            "locale.currency_code",
            "locale.primary_language",
            "appearance.color_mode",
            "backup.retention_daily",
        ] {
            let exists = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS ok FROM app_settings WHERE setting_key = ? LIMIT 1",
                    [key.into()],
                ))
                .await
                .expect("lookup");
            assert!(exists.is_some(), "missing baseline setting `{key}` after backfill seed");
        }

        let cat_rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT DISTINCT category FROM app_settings ORDER BY category".to_string(),
            ))
            .await
            .expect("categories");

        let cats: Vec<String> = cat_rows
            .into_iter()
            .map(|r| r.try_get::<String>("", "category").expect("category"))
            .collect();

        for required in ["appearance", "backup", "localization", "system"] {
            assert!(
                cats.iter().any(|c| c == required),
                "tenant category `{required}` must be present alongside system (got {cats:?})"
            );
        }
    }
}
