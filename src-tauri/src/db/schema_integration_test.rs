#[cfg(test)]
mod tests {
    /// Tests that all 6 Phase 1 migrations apply cleanly against a
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
        ];

        for table in &expected_tables {
            let sql = format!("SELECT COUNT(*) FROM {};", table);
            db.execute(Statement::from_string(DbBackend::Sqlite, sql))
                .await
                .unwrap_or_else(|e| {
                    panic!("Table '{}' is missing or inaccessible: {}", table, e)
                });
        }
    }

    /// Verify critical column presence on the equipment table per PRD §7.1 principles
    #[tokio::test]
    async fn equipment_table_has_required_sync_columns() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations");

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
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations");

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
}
