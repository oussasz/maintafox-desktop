//! Supervisor verification tests for Phase 2 SP03 File 01 Sprint S1.
//!
//! V1 — Domain uniqueness: duplicate domain code rejected
//! V2 — PRD type enforcement: unknown structure_type rejected
//! V3 — Governance-level enforcement: unknown governance_level rejected

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::domains::{
        self, CreateReferenceDomainPayload, UpdateReferenceDomainPayload,
        GOVERNANCE_LEVELS, STRUCTURE_TYPES,
    };

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

    /// Standard valid payload for reference domain creation.
    fn valid_payload() -> CreateReferenceDomainPayload {
        CreateReferenceDomainPayload {
            code: "FAILURE_CLASS".to_string(),
            name: "Classes de défaillance".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "protected_analytical".to_string(),
            is_extendable: Some(false),
            validation_rules_json: None,
        }
    }

    // ── V1 — Domain uniqueness ────────────────────────────────────────────

    #[tokio::test]
    async fn v1_create_domain_then_duplicate_code_rejected() {
        let db = setup().await;

        // First insert succeeds
        let domain = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("first create should succeed");

        assert_eq!(domain.code, "FAILURE_CLASS");
        assert_eq!(domain.structure_type, "hierarchical");
        assert_eq!(domain.governance_level, "protected_analytical");
        assert!(!domain.is_extendable);

        // Second insert with same code must fail
        let err = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect_err("duplicate code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("FAILURE_CLASS"),
                    "error should name the duplicate code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_code_normalized_to_uppercase() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.code = "  failure.class  ".to_string();

        let domain = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect("mixed-case code should normalize and succeed");

        assert_eq!(domain.code, "FAILURE.CLASS");
    }

    #[tokio::test]
    async fn v1_list_returns_created_domains() {
        let db = setup().await;

        domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        let mut payload2 = valid_payload();
        payload2.code = "EQUIPMENT_FAMILY".to_string();
        payload2.name = "Familles d'équipements".to_string();
        payload2.structure_type = "hierarchical".to_string();
        payload2.governance_level = "tenant_managed".to_string();

        domains::create_reference_domain(&db, payload2, 1)
            .await
            .expect("create second domain");

        let list = domains::list_reference_domains(&db)
            .await
            .expect("list domains");

        assert!(
            list.len() >= 2,
            "list should include at least the two newly created domains"
        );
        let codes: Vec<&str> = list.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"FAILURE_CLASS"));
        assert!(codes.contains(&"EQUIPMENT_FAMILY"));
    }

    #[tokio::test]
    async fn v1_get_by_id_returns_correct_domain() {
        let db = setup().await;

        let created = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        let fetched = domains::get_reference_domain(&db, created.id)
            .await
            .expect("get domain by id");

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.code, "FAILURE_CLASS");
    }

    #[tokio::test]
    async fn v1_get_nonexistent_id_returns_not_found() {
        let db = setup().await;

        let err = domains::get_reference_domain(&db, 999_999)
            .await
            .expect_err("nonexistent id should fail");

        match err {
            AppError::NotFound { entity, id } => {
                assert_eq!(entity, "ReferenceDomain");
                assert_eq!(id, "999999");
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    // ── V2 — PRD type enforcement ─────────────────────────────────────────

    #[tokio::test]
    async fn v2_unknown_structure_type_rejected() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.structure_type = "tree".to_string();

        let err = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect_err("unknown structure_type should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("tree"),
                    "error should name the bad type, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_all_prd_structure_types_accepted() {
        let db = setup().await;

        for (i, st) in STRUCTURE_TYPES.iter().enumerate() {
            let payload = CreateReferenceDomainPayload {
                code: format!("DOMAIN_{i}"),
                name: format!("Test domain {i}"),
                structure_type: (*st).to_string(),
                governance_level: "tenant_managed".to_string(),
                is_extendable: Some(true),
                validation_rules_json: None,
            };

            let domain = domains::create_reference_domain(&db, payload, 1)
                .await
                .unwrap_or_else(|e| panic!("structure_type '{st}' should be accepted: {e}"));

            assert_eq!(domain.structure_type, *st);
        }
    }

    #[tokio::test]
    async fn v2_structure_type_case_sensitive() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.structure_type = "FLAT".to_string(); // wrong case — must be lowercase

        let err = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect_err("uppercase structure_type should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v2_update_to_invalid_structure_type_rejected() {
        let db = setup().await;

        let domain = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        let update = UpdateReferenceDomainPayload {
            name: None,
            structure_type: Some("graph".to_string()),
            governance_level: None,
            is_extendable: None,
            validation_rules_json: None,
        };

        let err = domains::update_reference_domain(&db, domain.id, update, 1)
            .await
            .expect_err("update to invalid type should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ── V3 — Governance-level enforcement ─────────────────────────────────

    #[tokio::test]
    async fn v3_unknown_governance_level_rejected() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.governance_level = "public".to_string();

        let err = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect_err("unknown governance_level should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("public"),
                    "error should name the bad level, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_all_prd_governance_levels_accepted() {
        let db = setup().await;

        for (i, gl) in GOVERNANCE_LEVELS.iter().enumerate() {
            let payload = CreateReferenceDomainPayload {
                code: format!("GOV_DOMAIN_{i}"),
                name: format!("Test gov domain {i}"),
                structure_type: "flat".to_string(),
                governance_level: (*gl).to_string(),
                is_extendable: Some(true),
                validation_rules_json: None,
            };

            let domain = domains::create_reference_domain(&db, payload, 1)
                .await
                .unwrap_or_else(|e| panic!("governance_level '{gl}' should be accepted: {e}"));

            assert_eq!(domain.governance_level, *gl);
        }
    }

    #[tokio::test]
    async fn v3_governance_level_case_sensitive() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.governance_level = "PROTECTED_ANALYTICAL".to_string();

        let err = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect_err("uppercase governance_level should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v3_update_to_invalid_governance_level_rejected() {
        let db = setup().await;

        let domain = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        let update = UpdateReferenceDomainPayload {
            name: None,
            structure_type: None,
            governance_level: Some("admin_only".to_string()),
            is_extendable: None,
            validation_rules_json: None,
        };

        let err = domains::update_reference_domain(&db, domain.id, update, 1)
            .await
            .expect_err("update to invalid level should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ── Additional edge-case coverage ─────────────────────────────────────

    #[tokio::test]
    async fn update_domain_partial_fields() {
        let db = setup().await;

        let domain = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        let update = UpdateReferenceDomainPayload {
            name: Some("Updated Name".to_string()),
            structure_type: None,
            governance_level: None,
            is_extendable: Some(true),
            validation_rules_json: None,
        };

        let updated = domains::update_reference_domain(&db, domain.id, update, 1)
            .await
            .expect("partial update should succeed");

        assert_eq!(updated.name, "Updated Name");
        assert!(updated.is_extendable);
        // Unchanged fields preserved
        assert_eq!(updated.structure_type, "hierarchical");
        assert_eq!(updated.governance_level, "protected_analytical");
    }

    #[tokio::test]
    async fn update_nonexistent_domain_returns_not_found() {
        let db = setup().await;

        let update = UpdateReferenceDomainPayload {
            name: Some("Ghost".to_string()),
            structure_type: None,
            governance_level: None,
            is_extendable: None,
            validation_rules_json: None,
        };

        let err = domains::update_reference_domain(&db, 999_999, update, 1)
            .await
            .expect_err("update nonexistent should fail");

        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn empty_code_rejected() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.code = "".to_string();

        let err = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect_err("empty code should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn invalid_json_in_validation_rules_rejected() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.validation_rules_json = Some("not json at all".to_string());

        let err = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect_err("invalid json should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn valid_json_in_validation_rules_accepted() {
        let db = setup().await;

        let mut payload = valid_payload();
        payload.code = "WITH_RULES".to_string();
        payload.validation_rules_json = Some(r#"{"max_depth": 3, "required_fields": ["label"]}"#.to_string());

        let domain = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect("valid json should be accepted");

        assert!(domain.validation_rules_json.is_some());
    }

    #[tokio::test]
    async fn migration_013_creates_all_three_tables() {
        let db = setup().await;

        // Verify each table exists by running a simple SELECT
        for table in &["reference_domains", "reference_sets", "reference_values"] {
            let result = db
                .query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("SELECT COUNT(*) AS cnt FROM {table}"),
                ))
                .await;

            assert!(
                result.is_ok(),
                "table '{table}' should exist after migration 013"
            );
        }
    }

    #[tokio::test]
    async fn migration_013_unique_index_on_sets_domain_version() {
        let db = setup().await;
        let now = chrono::Utc::now().to_rfc3339();

        // Create a domain first
        let domain = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        // Insert a reference set version 1
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_sets (domain_id, version_no, status, created_at) \
             VALUES (?, 1, 'draft', ?)",
            [domain.id.into(), now.clone().into()],
        ))
        .await
        .expect("insert set v1");

        // Duplicate (domain_id, version_no) must fail at DB level
        let err = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO reference_sets (domain_id, version_no, status, created_at) \
                 VALUES (?, 1, 'draft', ?)",
                [domain.id.into(), now.into()],
            ))
            .await;

        assert!(err.is_err(), "duplicate (domain_id, version_no) should be rejected by unique index");
    }

    #[tokio::test]
    async fn migration_013_unique_index_on_values_set_code() {
        let db = setup().await;
        let now = chrono::Utc::now().to_rfc3339();

        let domain = domains::create_reference_domain(&db, valid_payload(), 1)
            .await
            .expect("create domain");

        // Create a set
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_sets (domain_id, version_no, status, created_at) \
             VALUES (?, 1, 'draft', ?)",
            [domain.id.into(), now.clone().into()],
        ))
        .await
        .expect("insert set");

        let set_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM reference_sets WHERE domain_id = 1 AND version_no = 1".to_string(),
            ))
            .await
            .expect("query set")
            .expect("set row");
        let set_id: i64 = set_row.try_get("", "id").expect("set id");

        // Insert a value
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label) VALUES (?, 'MECH', 'Mécanique')",
            [set_id.into()],
        ))
        .await
        .expect("insert value");

        // Duplicate (set_id, code) must fail at DB level
        let err = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO reference_values (set_id, code, label) VALUES (?, 'MECH', 'Duplicate')",
                [set_id.into()],
            ))
            .await;

        assert!(err.is_err(), "duplicate (set_id, code) should be rejected by unique index");
    }
}
