//! Supervisor verification tests for Phase 2 SP03 File 03 Sprint S2.
//!
//! V1 — Row-level diagnostics (malformed rows produce row-specific errors)
//! V2 — Protected-policy integration (protected domain imports show governance warnings)
//! V3 — Export completeness (export includes canonical values + alias data)

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::aliases::{self, CreateReferenceAliasPayload};
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::imports::{self, ImportRowInput, RefImportApplyPolicy};
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

    /// Creates a tenant_managed flat domain + draft set, returns (domain_id, set_id).
    async fn setup_tenant_domain(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let domain = domains::create_reference_domain(
            db,
            CreateReferenceDomainPayload {
                code: "IMPORT_TEST".to_string(),
                name: "Import Test Domain".to_string(),
                structure_type: "flat".to_string(),
                governance_level: "tenant_managed".to_string(),
                is_extendable: Some(true),
                validation_rules_json: None,
            },
            1,
        )
        .await
        .expect("create domain");

        let set = sets::create_draft_set(db, domain.id, 1)
            .await
            .expect("create draft set");

        (domain.id, set.id)
    }

    /// Creates a protected_analytical flat domain + draft set, returns (domain_id, set_id).
    async fn setup_protected_domain(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let domain = domains::create_reference_domain(
            db,
            CreateReferenceDomainPayload {
                code: "PROT_IMPORT".to_string(),
                name: "Protected Import Domain".to_string(),
                structure_type: "flat".to_string(),
                governance_level: "protected_analytical".to_string(),
                is_extendable: Some(false),
                validation_rules_json: None,
            },
            1,
        )
        .await
        .expect("create protected domain");

        let set = sets::create_draft_set(db, domain.id, 1)
            .await
            .expect("create draft set");

        (domain.id, set.id)
    }

    fn row(code: Option<&str>, label: Option<&str>) -> ImportRowInput {
        ImportRowInput {
            code: code.map(|c| c.to_string()),
            label: label.map(|l| l.to_string()),
            description: None,
            parent_code: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Row-level diagnostics
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_malformed_rows_produce_row_level_errors() {
        let db = setup().await;
        let (domain_id, _set_id) = setup_tenant_domain(&db).await;

        let batch = imports::create_import_batch(
            &db,
            domain_id,
            "test_v1.json",
            "abc123sha256",
            Some(1),
        )
        .await
        .expect("create batch");

        // Stage 5 rows: 2 valid, 3 with various errors
        let rows = vec![
            row(Some("VALID_A"), Some("Valid A")),             // valid
            row(Some("VALID_B"), Some("Valid B")),             // valid
            row(None, Some("No Code")),                        // missing code → error
            row(Some("lowercase"), Some("Bad Code Format")),   // invalid code → error
            row(Some("NO_LABEL"), None),                       // missing label → error
        ];

        imports::stage_import_rows(&db, batch.id, rows)
            .await
            .expect("stage rows");

        let validated = imports::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate batch");

        assert_eq!(validated.status, "validated");
        assert_eq!(validated.total_rows, 5);
        assert_eq!(validated.valid_rows, 2);
        assert_eq!(validated.error_rows, 3);

        // Verify row-level diagnostics in preview
        let preview = imports::get_import_preview(&db, batch.id)
            .await
            .expect("preview");

        assert_eq!(preview.rows.len(), 5);

        // Row 1: valid
        assert_eq!(preview.rows[0].validation_status, "valid");
        assert_eq!(preview.rows[0].proposed_action.as_deref(), Some("create"));

        // Row 3: missing code
        assert_eq!(preview.rows[2].validation_status, "error");
        assert!(preview.rows[2]
            .messages
            .iter()
            .any(|m| m.category == "MissingCode"));

        // Row 4: invalid code format
        assert_eq!(preview.rows[3].validation_status, "error");
        assert!(preview.rows[3]
            .messages
            .iter()
            .any(|m| m.category == "InvalidCodeFormat"));

        // Row 5: missing label
        assert_eq!(preview.rows[4].validation_status, "error");
        assert!(preview.rows[4]
            .messages
            .iter()
            .any(|m| m.category == "MissingLabel"));
    }

    #[tokio::test]
    async fn v1_duplicate_codes_within_batch_detected() {
        let db = setup().await;
        let (domain_id, _set_id) = setup_tenant_domain(&db).await;

        let batch = imports::create_import_batch(
            &db,
            domain_id,
            "dup_test.json",
            "dup_sha256",
            Some(1),
        )
        .await
        .expect("create batch");

        let rows = vec![
            row(Some("DUP_CODE"), Some("First occurrence")),
            row(Some("DUP_CODE"), Some("Second occurrence")),
        ];

        imports::stage_import_rows(&db, batch.id, rows)
            .await
            .expect("stage");

        let validated = imports::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        assert_eq!(validated.valid_rows, 1);
        assert_eq!(validated.error_rows, 1);

        let preview = imports::get_import_preview(&db, batch.id)
            .await
            .expect("preview");

        // Second row should have DuplicateInBatch error
        assert!(preview.rows[1]
            .messages
            .iter()
            .any(|m| m.category == "DuplicateInBatch"));
    }

    #[tokio::test]
    async fn v1_apply_rejects_non_validated_batch() {
        let db = setup().await;
        let (domain_id, set_id) = setup_tenant_domain(&db).await;

        let batch = imports::create_import_batch(
            &db,
            domain_id,
            "not_validated.json",
            "nv_sha256",
            Some(1),
        )
        .await
        .expect("create batch");

        let policy = RefImportApplyPolicy {
            include_warnings: false,
            target_set_id: set_id,
        };

        let err = imports::apply_import_batch(&db, batch.id, policy, 1)
            .await
            .expect_err("should reject uploaded batch");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_apply_idempotency_guard() {
        let db = setup().await;
        let (domain_id, set_id) = setup_tenant_domain(&db).await;

        let batch = imports::create_import_batch(
            &db,
            domain_id,
            "idempotent.json",
            "idem_sha256",
            Some(1),
        )
        .await
        .expect("create batch");

        imports::stage_import_rows(&db, batch.id, vec![row(Some("IDEM_A"), Some("A"))])
            .await
            .expect("stage");

        imports::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let policy = RefImportApplyPolicy {
            include_warnings: false,
            target_set_id: set_id,
        };

        imports::apply_import_batch(&db, batch.id, policy.clone(), 1)
            .await
            .expect("first apply");

        // Replay should fail
        let err = imports::apply_import_batch(&db, batch.id, policy, 1)
            .await
            .expect_err("replay rejected");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Protected-policy integration
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_protected_domain_import_shows_governance_warnings() {
        let db = setup().await;
        let (domain_id, set_id) = setup_protected_domain(&db).await;

        // Publish the set with an existing value to trigger collision.
        values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id,
                parent_id: None,
                code: "EXISTING_CODE".to_string(),
                label: "Existing Value".to_string(),
                description: None,
                sort_order: None,
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            },
            1,
        )
        .await
        .expect("create value");

        sets::validate_set(&db, set_id, 1).await.expect("validate set");
        sets::publish_set(&db, set_id, 1).await.expect("publish set");

        // Create new batch importing an existing code in the protected domain
        let batch = imports::create_import_batch(
            &db,
            domain_id,
            "protected_import.json",
            "prot_sha256",
            Some(1),
        )
        .await
        .expect("create batch");

        let rows = vec![
            row(Some("EXISTING_CODE"), Some("Updated Label")),
            row(Some("NEW_CODE"), Some("New Value")),
        ];

        imports::stage_import_rows(&db, batch.id, rows)
            .await
            .expect("stage");

        let validated = imports::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        assert_eq!(validated.valid_rows, 1); // NEW_CODE → valid
        assert_eq!(validated.warning_rows, 1); // EXISTING_CODE → warning

        let preview = imports::get_import_preview(&db, batch.id)
            .await
            .expect("preview");

        // First row: protected domain update → warning
        let prot_row = &preview.rows[0];
        assert_eq!(prot_row.validation_status, "warning");
        assert!(prot_row
            .messages
            .iter()
            .any(|m| m.category == "ProtectedDomainUpdate"));
        assert_eq!(prot_row.proposed_action.as_deref(), Some("update"));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Export completeness
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_export_includes_values_and_aliases() {
        let db = setup().await;
        let (domain_id, set_id) = setup_tenant_domain(&db).await;

        let val = values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id,
                parent_id: None,
                code: "EXPORT_A".to_string(),
                label: "Export Value A".to_string(),
                description: None,
                sort_order: None,
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            },
            1,
        )
        .await
        .expect("create value");

        // Add aliases to the value
        aliases::create_alias(
            &db,
            CreateReferenceAliasPayload {
                reference_value_id: val.id,
                alias_label: "Ancien nom A".to_string(),
                locale: "fr".to_string(),
                alias_type: "legacy".to_string(),
                is_preferred: Some(true),
            },
            1,
        )
        .await
        .expect("create alias");

        aliases::create_alias(
            &db,
            CreateReferenceAliasPayload {
                reference_value_id: val.id,
                alias_label: "Old name A".to_string(),
                locale: "en".to_string(),
                alias_type: "legacy".to_string(),
                is_preferred: Some(true),
            },
            1,
        )
        .await
        .expect("create alias en");

        // Export the set
        let export = imports::export_domain_set(&db, set_id)
            .await
            .expect("export");

        assert_eq!(export.domain.id, domain_id);
        assert_eq!(export.set.id, set_id);
        assert_eq!(export.rows.len(), 1);

        let export_row = &export.rows[0];
        assert_eq!(export_row.value.code, "EXPORT_A");
        assert_eq!(export_row.aliases.len(), 2);

        // Verify aliases are present
        let alias_labels: Vec<&str> = export_row
            .aliases
            .iter()
            .map(|a| a.alias_label.as_str())
            .collect();
        assert!(alias_labels.contains(&"Ancien nom A"));
        assert!(alias_labels.contains(&"Old name A"));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Additional coverage: full apply workflow
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn apply_creates_and_updates_deterministically() {
        let db = setup().await;
        let (domain_id, set_id) = setup_tenant_domain(&db).await;

        // Pre-populate a value in the draft set
        values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id,
                parent_id: None,
                code: "PRE_EXIST".to_string(),
                label: "Pre-existing".to_string(),
                description: None,
                sort_order: None,
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            },
            1,
        )
        .await
        .expect("create pre-existing");

        let batch = imports::create_import_batch(
            &db,
            domain_id,
            "apply_test.json",
            "apply_sha256",
            Some(1),
        )
        .await
        .expect("create batch");

        let rows = vec![
            row(Some("PRE_EXIST"), Some("Updated Label")),  // update
            row(Some("BRAND_NEW"), Some("Brand New Value")), // create
        ];

        imports::stage_import_rows(&db, batch.id, rows)
            .await
            .expect("stage");

        imports::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let policy = RefImportApplyPolicy {
            include_warnings: false,
            target_set_id: set_id,
        };

        let result = imports::apply_import_batch(&db, batch.id, policy, 1)
            .await
            .expect("apply");

        assert_eq!(result.created, 1);
        assert_eq!(result.updated, 1);
        assert_eq!(result.batch.status, "applied");

        // Verify the pre-existing value was updated
        let all_vals = values::list_values(&db, set_id).await.expect("list");
        let updated_val = all_vals.iter().find(|v| v.code == "PRE_EXIST").unwrap();
        assert_eq!(updated_val.label, "Updated Label");

        // Verify new value was created
        let new_val = all_vals.iter().find(|v| v.code == "BRAND_NEW").unwrap();
        assert_eq!(new_val.label, "Brand New Value");
    }

    #[tokio::test]
    async fn list_batches_filters_by_domain_and_status() {
        let db = setup().await;
        let (domain_id, _set_id) = setup_tenant_domain(&db).await;

        imports::create_import_batch(&db, domain_id, "a.json", "sha_a", Some(1))
            .await
            .expect("batch a");
        imports::create_import_batch(&db, domain_id, "b.json", "sha_b", Some(1))
            .await
            .expect("batch b");

        let all = imports::list_import_batches(&db, domain_id, None, None)
            .await
            .expect("list all");
        assert_eq!(all.len(), 2);

        let uploaded = imports::list_import_batches(
            &db,
            domain_id,
            Some("uploaded".to_string()),
            None,
        )
        .await
        .expect("list uploaded");
        assert_eq!(uploaded.len(), 2);
    }
}
