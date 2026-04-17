//! Supervisor verification tests for Phase 2 SP03 File 02 Sprint S2.
//!
//! V1 — Duplicate code detection: duplicate codes produce a blocking issue.
//! V2 — Cycle detection: hierarchy cycles produce a blocking issue.
//! V3 — Validation persistence: report rows are persisted and retrievable.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::sets;
    use crate::reference::validation::{self, IssueSeverity};
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

    /// Creates a hierarchical protected analytical domain and returns its id.
    async fn setup_protected_domain(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "FAILURE_CLASS".to_string(),
            name: "Classes de defaillance".to_string(),
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

    /// Creates a flat tenant-managed domain and returns its id.
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

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Duplicate code detection
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_duplicate_code_produces_blocking_issue() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // The DB unique index on (set_id, code) is case-sensitive, but the
        // validation engine normalises to uppercase.  Insert two case-variant
        // codes that the engine will flag as duplicates.
        values::create_value(&db, value_payload(set_id, "ALPHA", "Alpha"), 1)
            .await
            .expect("create first");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values \
                 (set_id, code, label, is_active) \
             VALUES (?, 'alpha', 'Alpha lowercase', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert case-variant duplicate");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate should succeed");

        assert_eq!(result.status, "failed", "should fail with duplicates");
        assert!(result.blocking_count > 0, "should have blocking issues");

        let dup_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "duplicate_code")
            .collect();

        assert!(
            !dup_issues.is_empty(),
            "should have at least one duplicate_code issue"
        );
        assert_eq!(dup_issues[0].severity, IssueSeverity::Blocking);
        assert!(
            dup_issues[0].message.to_ascii_lowercase().contains("alpha"),
            "issue message should name the code"
        );
    }

    #[tokio::test]
    async fn v1_clean_set_passes_validation() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        values::create_value(&db, value_payload(set_id, "ALPHA", "Alpha"), 1)
            .await
            .expect("create alpha");
        values::create_value(&db, value_payload(set_id, "BETA", "Beta"), 1)
            .await
            .expect("create beta");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        assert_eq!(result.status, "passed");
        assert_eq!(result.blocking_count, 0);
    }

    #[tokio::test]
    async fn v1_duplicate_code_blocks_lifecycle_transition() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        values::create_value(&db, value_payload(set_id, "CODE_A", "A"), 1)
            .await
            .expect("create");

        // Case-variant duplicate — passes DB unique index but fails validation
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, is_active) VALUES (?, 'code_a', 'A dup', 1)",
            [set_id.into()],
        ))
        .await
        .expect("inject case-variant duplicate");

        // Attempt to validate the set — should fail
        let err = sets::validate_set(&db, set_id, 1)
            .await
            .expect_err("validate_set should reject blocking issues");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("bloquant"),
                    "error should mention blocking issues, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }

        // Verify set is still in draft
        let set = sets::get_reference_set(&db, set_id)
            .await
            .expect("get set");
        assert_eq!(set.status, "draft", "set should remain draft after failed validation");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Cycle detection
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_hierarchy_cycle_produces_blocking_issue() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Create A → B chain
        let a = values::create_value(&db, value_payload(set_id, "NODE_A", "A"), 1)
            .await
            .expect("A");
        let mut bp = value_payload(set_id, "NODE_B", "B");
        bp.parent_id = Some(a.id);
        let b = values::create_value(&db, bp, 1).await.expect("B");

        // Inject a cycle: set A's parent to B via raw SQL
        // (the service layer's cycle detection blocks this, so bypass it)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET parent_id = ? WHERE id = ?",
            [b.id.into(), a.id.into()],
        ))
        .await
        .expect("inject cycle");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate should succeed even with issues");

        assert_eq!(result.status, "failed");

        let cycle_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "hierarchy_cycle")
            .collect();

        assert!(
            !cycle_issues.is_empty(),
            "should detect hierarchy cycle"
        );
        assert_eq!(cycle_issues[0].severity, IssueSeverity::Blocking);
    }

    #[tokio::test]
    async fn v2_deep_cycle_detection() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Create A → B → C chain
        let a = values::create_value(&db, value_payload(set_id, "X", "X"), 1)
            .await
            .expect("X");
        let mut bp = value_payload(set_id, "Y", "Y");
        bp.parent_id = Some(a.id);
        let b = values::create_value(&db, bp, 1).await.expect("Y");
        let mut cp = value_payload(set_id, "Z", "Z");
        cp.parent_id = Some(b.id);
        let c = values::create_value(&db, cp, 1).await.expect("Z");

        // Inject cycle: A → C (so A→B→C→A)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET parent_id = ? WHERE id = ?",
            [c.id.into(), a.id.into()],
        ))
        .await
        .expect("inject deep cycle");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let cycle_count = result
            .issues
            .iter()
            .filter(|i| i.check == "hierarchy_cycle")
            .count();

        assert!(cycle_count >= 2, "deep cycle should flag multiple members, got {cycle_count}");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Validation persistence
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_validation_report_persisted() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        values::create_value(&db, value_payload(set_id, "ITEM", "Item"), 1)
            .await
            .expect("create item");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        assert_eq!(result.status, "passed");
        assert!(result.report_id > 0, "report should have a valid id");

        // Verify persisted row via raw SQL
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT set_id, status, issue_count, blocking_count, report_json \
                 FROM reference_validation_reports WHERE id = ?",
                [result.report_id.into()],
            ))
            .await
            .expect("query report row")
            .expect("report row should exist");

        let status: String = row.try_get("", "status").expect("status");
        let issue_count: i64 = row.try_get("", "issue_count").expect("issue_count");
        let blocking_count: i64 = row.try_get("", "blocking_count").expect("blocking_count");
        let report_json: String = row.try_get("", "report_json").expect("report_json");

        assert_eq!(status, "passed");
        assert_eq!(issue_count, 0);
        assert_eq!(blocking_count, 0);
        assert!(!report_json.is_empty(), "report_json should not be empty");
    }

    #[tokio::test]
    async fn v3_latest_report_retrieved() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        values::create_value(&db, value_payload(set_id, "FIRST", "First"), 1)
            .await
            .expect("create");

        // Run validation twice
        let _r1 = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("first validation");
        let r2 = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("second validation");

        // get_latest should return the second report
        let latest = validation::get_latest_validation_report(&db, set_id)
            .await
            .expect("get latest");

        assert_eq!(latest.id, r2.report_id, "latest should be the most recent report");
    }

    #[tokio::test]
    async fn v3_failed_report_persisted_with_issues() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        values::create_value(&db, value_payload(set_id, "VALID", "Valid"), 1)
            .await
            .expect("create");

        // Case-variant duplicate
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, is_active) VALUES (?, 'valid', 'Dup', 1)",
            [set_id.into()],
        ))
        .await
        .expect("inject case-variant duplicate");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        assert_eq!(result.status, "failed");

        let report = validation::get_latest_validation_report(&db, set_id)
            .await
            .expect("get latest");

        assert_eq!(report.status, "failed");
        assert!(report.blocking_count > 0);
        assert!(report.issue_count > 0);

        // Verify report_json is valid and contains issue details
        let issues: Vec<serde_json::Value> =
            serde_json::from_str(&report.report_json).expect("parse report_json");
        assert!(!issues.is_empty(), "report_json should contain issues");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Supplementary — additional check coverage
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn missing_label_detected() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Insert value with blank label via raw SQL
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, is_active) VALUES (?, 'BLANK_LBL', '   ', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert blank label");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let label_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "missing_label")
            .collect();

        assert!(!label_issues.is_empty(), "should detect missing label");
        assert_eq!(label_issues[0].severity, IssueSeverity::Blocking);
    }

    #[tokio::test]
    async fn orphan_parent_detected() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Insert value with parent_id=99999 that does not exist
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, parent_id, code, label, is_active) \
             VALUES (?, 99999, 'ORPHAN', 'Orphan node', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert orphan");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let orphan_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "orphan_parent")
            .collect();

        assert!(!orphan_issues.is_empty(), "should detect orphan parent");
        assert_eq!(orphan_issues[0].severity, IssueSeverity::Blocking);
    }

    #[tokio::test]
    async fn invalid_color_detected_as_warning() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Insert value with bad color
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, color_hex, is_active) \
             VALUES (?, 'BAD_CLR', 'Bad color', 'ZZZZZZ', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert bad color");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let color_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "invalid_color_hex")
            .collect();

        assert!(!color_issues.is_empty(), "should detect invalid color");
        assert_eq!(
            color_issues[0].severity,
            IssueSeverity::Warning,
            "color issues should be warnings, not blocking"
        );

        // Color issue is not blocking so overall should pass (if no other blocking issues)
        assert_eq!(result.status, "passed");
    }

    #[tokio::test]
    async fn valid_colors_accepted() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Insert values with valid color formats
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, color_hex, is_active) \
             VALUES (?, 'HEX6', 'Six digit', 'FF00AA', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert hex6");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, color_hex, is_active) \
             VALUES (?, 'HEX3', 'Three digit', '#F0A', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert hex3");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let color_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "invalid_color_hex")
            .collect();

        assert!(color_issues.is_empty(), "valid colors should not produce issues");
    }

    #[tokio::test]
    async fn external_code_pattern_validated() {
        let db = setup().await;

        // Create domain with external_code_pattern validation rule.
        let payload = CreateReferenceDomainPayload {
            code: "ERP_CODES".to_string(),
            name: "Codes ERP".to_string(),
            structure_type: "external_code_set".to_string(),
            governance_level: "erp_synced".to_string(),
            is_extendable: Some(true),
            validation_rules_json: Some(
                r#"{"external_code_pattern": "^SAP-[0-9]{4}$"}"#.to_string(),
            ),
        };
        let domain = domains::create_reference_domain(&db, payload, 1)
            .await
            .expect("create erp domain");

        let set_id = setup_draft_set(&db, domain.id).await;

        // Valid external code
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, external_code, is_active) \
             VALUES (?, 'GOOD', 'Good', 'SAP-1234', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert good");

        // Invalid external code
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, code, label, external_code, is_active) \
             VALUES (?, 'BAD', 'Bad', 'ORACLE-9999', 1)",
            [set_id.into()],
        ))
        .await
        .expect("insert bad");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let ext_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "invalid_external_code")
            .collect();

        assert_eq!(ext_issues.len(), 1, "should detect one invalid external code");
        assert_eq!(ext_issues[0].severity, IssueSeverity::Blocking);
        assert!(ext_issues[0].message.contains("ORACLE-9999"));
    }

    #[tokio::test]
    async fn protected_deactivation_without_migration_map_warns() {
        let db = setup().await;
        let domain_id = setup_protected_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        // Create and deactivate a value
        let val = values::create_value(&db, value_payload(set_id, "OLD_CODE", "Old"), 1)
            .await
            .expect("create");
        values::deactivate_value(&db, val.id, 1)
            .await
            .expect("deactivate");

        let result = validation::validate_reference_set(&db, set_id, 1)
            .await
            .expect("validate");

        let prot_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.check == "protected_deactivation_no_migration")
            .collect();

        assert!(
            !prot_issues.is_empty(),
            "should warn about deactivated value without migration map"
        );
        assert_eq!(
            prot_issues[0].severity,
            IssueSeverity::Warning,
            "should be a warning, not blocking"
        );

        // Warning only, so overall should pass
        assert_eq!(result.status, "passed");
    }

    #[tokio::test]
    async fn clean_set_validates_and_transitions() {
        let db = setup().await;
        let domain_id = setup_tenant_domain(&db).await;
        let set_id = setup_draft_set(&db, domain_id).await;

        values::create_value(&db, value_payload(set_id, "OK_VAL", "Valid value"), 1)
            .await
            .expect("create");

        // validate_set should succeed and transition to validated
        let validated = sets::validate_set(&db, set_id, 1)
            .await
            .expect("validate_set should succeed");

        assert_eq!(validated.status, "validated");
    }
}
