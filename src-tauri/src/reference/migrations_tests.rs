//! Supervisor verification tests for Phase 2 SP03 File 02 Sprint S3.
//!
//! V1 — Migration map presence: merge writes a mapping row.
//! V2 — Post-migration behavior: source becomes inactive, target stays active.
//! V3 — Dangerous action guard: merge without step-up fails.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::auth::rbac::PermissionScope;
    use crate::auth::session_manager::AuthenticatedUser;
    use crate::errors::{AppError, AppResult};
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::migrations as ref_migrations;
    use crate::reference::sets;
    use crate::reference::values::{self, CreateReferenceValuePayload};
    use crate::state::AppState;

    /// In-memory SQLite with all migrations + seed data.
    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations should apply cleanly");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run cleanly");

        db
    }

    /// Assigns a user to a role by name at tenant scope.
    async fn assign_role(db: &sea_orm::DatabaseConnection, user_id: i32, role_name: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES ('ref-s3-assign-' || ?, ?, \
               (SELECT id FROM roles WHERE name = ?), \
               'tenant', ?, ?)",
            [
                user_id.into(),
                user_id.into(),
                role_name.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert user_scope_assignment");
    }

    /// Creates a protected analytical equipment-classification domain.
    async fn setup_domain(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "EQUIPMENT_CLASSIFICATION".to_string(),
            name: "Classification equipement".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "protected_analytical".to_string(),
            is_extendable: Some(false),
            validation_rules_json: None,
        };

        domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create reference domain")
            .id
    }

    async fn setup_draft_set(db: &sea_orm::DatabaseConnection, domain_id: i64) -> i64 {
        sets::create_draft_set(db, domain_id, 1)
            .await
            .expect("create draft set")
            .id
    }

    fn value_payload(set_id: i64, code: &str, label: &str) -> CreateReferenceValuePayload {
        CreateReferenceValuePayload {
            set_id,
            parent_id: None,
            code: code.to_string(),
            label: label.to_string(),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        }
    }

    /// Seeds downstream usage by equipment_classes.code.
    async fn seed_equipment_class(db: &sea_orm::DatabaseConnection, code: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO equipment_classes \
                 (sync_id, code, name, level, is_active, created_at, updated_at) \
             VALUES (?, ?, ?, 'family', 1, ?, ?)",
            [
                format!("merge-test-{code}").into(),
                code.into(),
                format!("Classe {code}").into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("seed equipment class");
    }

    /// Mimics the IPC gate path for dangerous merge commands.
    async fn guarded_merge_action(
        state: &AppState,
        domain_id: i64,
        from_value_id: i64,
        to_value_id: i64,
    ) -> AppResult<ref_migrations::ReferenceUsageMigrationResult> {
        let user = crate::require_session!(state);
        crate::require_permission!(state, &user, "ref.publish", PermissionScope::Global);
        crate::require_step_up!(state);

        ref_migrations::merge_reference_values(
            &state.db,
            domain_id,
            from_value_id,
            to_value_id,
            i64::from(user.user_id),
        )
        .await
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Migration map presence
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_merge_persists_migration_map_row() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let from = values::create_value(&db, value_payload(set_id, "PUMP_OLD", "Pump old"), 1)
            .await
            .expect("create from value");
        let to = values::create_value(&db, value_payload(set_id, "PUMP_NEW", "Pump new"), 1)
            .await
            .expect("create to value");

        seed_equipment_class(&db, "PUMP_OLD").await;

        let result = ref_migrations::merge_reference_values(&db, domain_id, from.id, to.id, 1)
            .await
            .expect("merge should succeed");

        assert!(result.migration.id > 0, "migration row id should be set");
        assert_eq!(result.migration.domain_id, domain_id);
        assert_eq!(result.migration.from_value_id, from.id);
        assert_eq!(result.migration.to_value_id, to.id);
        assert_eq!(result.migration.reason_code.as_deref(), Some("merge"));
        assert!(
            result.remapped_references > 0,
            "usage remap should affect at least one row"
        );

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT domain_id, from_value_id, to_value_id \
                 FROM reference_value_migrations WHERE id = ?",
                [result.migration.id.into()],
            ))
            .await
            .expect("query migration row")
            .expect("migration row should exist");

        let row_domain: i64 = row.try_get("", "domain_id").expect("domain_id");
        let row_from: i64 = row.try_get("", "from_value_id").expect("from_value_id");
        let row_to: i64 = row.try_get("", "to_value_id").expect("to_value_id");

        assert_eq!(row_domain, domain_id);
        assert_eq!(row_from, from.id);
        assert_eq!(row_to, to.id);

        let listed = ref_migrations::list_reference_migrations(&db, domain_id, 10)
            .await
            .expect("list migrations");
        assert!(!listed.is_empty(), "list should return persisted rows");
        assert_eq!(listed[0].id, result.migration.id);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Post-migration behavior
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_migrate_inactivates_source_and_keeps_target_active() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let from = values::create_value(&db, value_payload(set_id, "OLD_CODE", "Old"), 1)
            .await
            .expect("create from value");
        let to = values::create_value(&db, value_payload(set_id, "NEW_CODE", "New"), 1)
            .await
            .expect("create to value");

        let result = ref_migrations::migrate_reference_usage(&db, domain_id, from.id, to.id, 1)
            .await
            .expect("migrate should succeed");

        assert!(
            result.source_deactivated,
            "source should be deactivated after migration"
        );
        assert!(
            !result.source_value.is_active,
            "source value should now be inactive"
        );
        assert!(result.target_value.is_active, "target must remain active");
        assert_eq!(
            result.migration.reason_code.as_deref(),
            Some("usage_migration")
        );

        let source = values::get_value(&db, from.id).await.expect("reload source");
        let target = values::get_value(&db, to.id).await.expect("reload target");

        assert!(!source.is_active, "persisted source must be inactive");
        assert!(target.is_active, "persisted target must stay active");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Dangerous action guard
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_merge_without_step_up_fails_then_succeeds_after_step_up() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let from = values::create_value(&db, value_payload(set_id, "SAFE_FROM", "From"), 1)
            .await
            .expect("create from value");
        let to = values::create_value(&db, value_payload(set_id, "SAFE_TO", "To"), 1)
            .await
            .expect("create to value");

        assign_role(&db, 20, "Administrator").await;

        let state = AppState::new(db.clone());
        {
            let mut sm = state.session.write().await;
            sm.create_session(AuthenticatedUser {
                user_id: 20,
                username: "admin-s3".to_string(),
                display_name: Some("Admin S3".to_string()),
                is_admin: true,
                force_password_change: false,
            });
        }

        // No step-up yet -> must fail.
        let err = guarded_merge_action(&state, domain_id, from.id, to.id)
            .await
            .expect_err("merge must require step-up");
        assert!(
            matches!(err, AppError::StepUpRequired),
            "expected StepUpRequired, got: {err:?}"
        );

        // With step-up -> succeeds.
        {
            let mut sm = state.session.write().await;
            sm.record_step_up();
        }

        let ok = guarded_merge_action(&state, domain_id, from.id, to.id)
            .await
            .expect("merge should pass after step-up");

        assert_eq!(ok.migration.reason_code.as_deref(), Some("merge"));
    }
}
