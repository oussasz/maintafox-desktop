//! Supervisor verification tests for Phase 2 SP03 File 02 Sprint S1.
//!
//! V1 — Protected delete block: in-use value in protected domain cannot be deleted.
//! V2 — Allowed non-protected delete: unused value in tenant domain can be deleted.
//! V3 — Deactivate fallback: protected in-use value can be deactivated.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::protected;
    use crate::reference::sets;
    use crate::reference::values::{self, CreateReferenceValuePayload};

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

    /// Creates a protected analytical hierarchical domain and returns its id.
    async fn setup_protected_domain(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "EQUIPMENT_FAMILY".to_string(),
            name: "Familles d'equipement".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "protected_analytical".to_string(),
            is_extendable: Some(false),
            validation_rules_json: None,
        };
        domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create protected domain")
            .id
    }

    /// Creates a tenant-managed flat domain and returns its id.
    async fn setup_tenant_domain(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "CUSTOM_TAG".to_string(),
            name: "Tags personnalises".to_string(),
            structure_type: "flat".to_string(),
            governance_level: "tenant_managed".to_string(),
            is_extendable: Some(true),
            validation_rules_json: None,
        };
        domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create tenant domain")
            .id
    }

    /// Creates a domain + draft set, returns (domain_id, set_id).
    async fn setup_draft_set(
        db: &sea_orm::DatabaseConnection,
        domain_id: i64,
    ) -> i64 {
        let set = sets::create_draft_set(db, domain_id, 1)
            .await
            .expect("create draft set");
        set.id
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

    /// Inserts a row into `equipment_classes` that uses a given code,
    /// simulating downstream usage of a reference value.
    async fn seed_equipment_class(db: &sea_orm::DatabaseConnection, code: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO equipment_classes \
                 (sync_id, code, name, level, is_active, created_at, updated_at) \
             VALUES (?, ?, ?, 'family', 1, ?, ?)",
            [
                format!("test-sync-{code}").into(),
                code.into(),
                format!("Famille {code}").into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("seed equipment_classes row");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Protected delete block
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_protected_domain_blocks_delete_of_in_use_value() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Create a reference value with code PUMP_FAMILY
        let val = values::create_value(
            &db,
            value_payload(set_id, "PUMP_FAMILY", "Pompes"),
            1,
        )
        .await
        .expect("create value");

        // Simulate downstream usage: insert an equipment class with the same code
        seed_equipment_class(&db, "PUMP_FAMILY").await;

        // Deletion must be blocked
        let err = protected::assert_can_delete_value(&db, val.id)
            .await
            .expect_err("protected in-use value should not be deletable");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("PUMP_FAMILY"),
                    "error should name the value code, got: {joined}"
                );
                assert!(
                    joined.contains("equipment_classes"),
                    "error should name the consuming table, got: {joined}"
                );
                assert!(
                    joined.contains("desactivation") || joined.contains("migration"),
                    "error should suggest deactivation or migration, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_protected_domain_allows_delete_of_unused_value() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Create value but do NOT seed any downstream usage
        let val = values::create_value(
            &db,
            value_payload(set_id, "UNUSED_FAMILY", "Famille inutilisee"),
            1,
        )
        .await
        .expect("create value");

        // Deletion should pass — no usage found
        protected::assert_can_delete_value(&db, val.id)
            .await
            .expect("unused value in protected domain should be deletable");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Allowed non-protected delete
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_tenant_domain_allows_delete_of_unused_value() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let val = values::create_value(
            &db,
            value_payload(set_id, "CUSTOM_A", "Etiquette A"),
            1,
        )
        .await
        .expect("create value");

        // No downstream usage — delete should pass
        protected::assert_can_delete_value(&db, val.id)
            .await
            .expect("unused tenant value should be deletable");
    }

    #[tokio::test]
    async fn v2_tenant_domain_blocks_delete_of_in_use_value() {
        let db = setup().await;

        // Create a tenant domain whose code pattern matches equipment classification
        // to trigger the equipment_classes probe.
        let payload = CreateReferenceDomainPayload {
            code: "EQUIPMENT_CLASSIFICATION".to_string(),
            name: "Classification tenant".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "tenant_managed".to_string(),
            is_extendable: Some(true),
            validation_rules_json: None,
        };
        let domain = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect("create domain");

        let set_id = setup_draft_set(&db, domain.id).await;

        let val = values::create_value(
            &db,
            value_payload(set_id, "MOTOR_CLASS", "Moteurs"),
            1,
        )
        .await
        .expect("create value");

        // Seed downstream usage
        seed_equipment_class(&db, "MOTOR_CLASS").await;

        // Even non-protected: in-use value is blocked
        let err = protected::assert_can_delete_value(&db, val.id)
            .await
            .expect_err("in-use tenant value should not be deletable");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Deactivate fallback
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_protected_in_use_value_can_be_deactivated() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let val = values::create_value(
            &db,
            value_payload(set_id, "PUMP_FAMILY", "Pompes"),
            1,
        )
        .await
        .expect("create value");

        // Seed downstream usage
        seed_equipment_class(&db, "PUMP_FAMILY").await;

        // Deactivation policy check should pass (governed alternative to deletion)
        protected::assert_can_deactivate_value(&db, val.id)
            .await
            .expect("protected in-use value should be deactivatable");

        // Also verify the actual deactivation via values service succeeds
        let deactivated = values::deactivate_value(&db, val.id, 1)
            .await
            .expect("deactivate value");

        assert!(!deactivated.is_active, "value should now be inactive");
        assert_eq!(deactivated.code, "PUMP_FAMILY");
    }

    #[tokio::test]
    async fn v3_deactivation_of_non_protected_value_also_allowed() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let val = values::create_value(
            &db,
            value_payload(set_id, "TAG_TEMP", "Tag temporaire"),
            1,
        )
        .await
        .expect("create value");

        // Policy check passes for non-protected domains
        protected::assert_can_deactivate_value(&db, val.id)
            .await
            .expect("tenant value deactivation should be allowed");

        let deactivated = values::deactivate_value(&db, val.id, 1)
            .await
            .expect("deactivate");

        assert!(!deactivated.is_active);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Supplementary — is_protected_domain check
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn is_protected_domain_returns_correct_flag() {
        let db = setup().await;
        let protected_id = setup_protected_domain(&db).await;
        let tenant_id = setup_tenant_domain(&db).await;

        assert!(
            protected::is_protected_domain(&db, protected_id)
                .await
                .expect("check protected"),
            "protected_analytical should return true"
        );

        assert!(
            !protected::is_protected_domain(&db, tenant_id)
                .await
                .expect("check tenant"),
            "tenant_managed should return false"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Supplementary — has_migration_map
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn has_migration_map_false_when_no_mapping() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let val = values::create_value(
            &db,
            value_payload(set_id, "NO_MAP", "Pas de mapping"),
            1,
        )
        .await
        .expect("create value");

        assert!(
            !protected::has_migration_map(&db, val.id)
                .await
                .expect("check migration map"),
            "no migration map should exist yet"
        );
    }

    #[tokio::test]
    async fn has_migration_map_true_after_insert() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        let from = values::create_value(
            &db,
            value_payload(set_id, "OLD_VAL", "Ancienne valeur"),
            1,
        )
        .await
        .expect("create from");

        let to = values::create_value(
            &db,
            value_payload(set_id, "NEW_VAL", "Nouvelle valeur"),
            1,
        )
        .await
        .expect("create to");

        // Insert a migration map row directly
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_value_migrations \
                 (domain_id, from_value_id, to_value_id, reason_code, migrated_by_id, migrated_at) \
             VALUES (?, ?, ?, 'CONSOLIDATION', 1, ?)",
            [domain_id.into(), from.id.into(), to.id.into(), now.into()],
        ))
        .await
        .expect("insert migration map");

        assert!(
            protected::has_migration_map(&db, from.id)
                .await
                .expect("check migration map"),
            "migration map should now exist"
        );
    }
}
