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
            "sync_id",
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
    async fn default_settings_seed_populates_14_rows_and_keeps_settings_audit_empty() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("Migrations");

        crate::db::seeder::seed_default_settings(&db)
            .await
            .expect("Default settings seeding should succeed");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};

        let settings_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM app_settings;".to_string(),
            ))
            .await
            .expect("Count query for app_settings should succeed")
            .expect("Count query for app_settings should return one row");
        let settings_count = settings_row
            .try_get::<i64>("", "cnt")
            .expect("Count column should be readable");

        let events_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM settings_change_events;".to_string(),
            ))
            .await
            .expect("Count query for settings_change_events should succeed")
            .expect("Count query for settings_change_events should return one row");
        let events_count = events_row
            .try_get::<i64>("", "cnt")
            .expect("Count column should be readable");

        assert_eq!(
            settings_count, 14,
            "Default settings seed should insert exactly 14 rows"
        );
        assert_eq!(
            events_count, 0,
            "settings_change_events must remain empty during read-only seeding"
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
                [
                    key.into(),
                    cat.into(),
                    scope.into(),
                    val.into(),
                    now.clone().into(),
                ],
            ))
            .await
            .expect("device-only seed insert");
        }

        crate::db::seeder::seed_default_settings(&db)
            .await
            .expect("Default settings seeding should succeed");

        let settings_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM app_settings;".to_string(),
            ))
            .await
            .expect("count")
            .expect("row")
            .try_get::<i64>("", "cnt")
            .expect("cnt");

        assert_eq!(settings_count, 14, "all baseline rows should exist");

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

        assert_eq!(
            cats,
            vec!["appearance", "backup", "localization", "system"],
            "tenant categories must be present alongside system"
        );
    }
}
