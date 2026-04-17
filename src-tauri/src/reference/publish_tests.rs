//! Supervisor verification tests for Phase 2 SP03 File 04 Sprint S1.
//!
//! V1 — Blocked publish (validation blockers prevent publish)
//! V2 — Impact preview requirement (protected domain without preview → blocked)
//! V3 — Successful publish transition (validated set publishes and supersedes prior)

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::publish;
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

        // Clear impact cache between tests.
        publish::clear_impact_cache();

        db
    }

    /// Creates a flat tenant_managed domain + draft set with one valid value,
    /// then validates the set. Returns (domain_id, set_id).
    async fn setup_validated_tenant_set(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let domain = domains::create_reference_domain(
            db,
            CreateReferenceDomainPayload {
                code: "PUB_TENANT".to_string(),
                name: "Publish Tenant Domain".to_string(),
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

        values::create_value(
            db,
            CreateReferenceValuePayload {
                set_id: set.id,
                parent_id: None,
                code: "VAL_A".to_string(),
                label: "Value A".to_string(),
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

        sets::validate_set(db, set.id, 1)
            .await
            .expect("validate set");

        (domain.id, set.id)
    }

    /// Creates a protected_analytical domain + validated set.
    async fn setup_validated_protected_set(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let domain = domains::create_reference_domain(
            db,
            CreateReferenceDomainPayload {
                code: "PUB_PROT".to_string(),
                name: "Publish Protected Domain".to_string(),
                structure_type: "flat".to_string(),
                governance_level: "protected_analytical".to_string(),
                is_extendable: Some(false),
                validation_rules_json: None,
            },
            1,
        )
        .await
        .expect("create domain");

        let set = sets::create_draft_set(db, domain.id, 1)
            .await
            .expect("create draft set");

        values::create_value(
            db,
            CreateReferenceValuePayload {
                set_id: set.id,
                parent_id: None,
                code: "PROT_A".to_string(),
                label: "Protected Value A".to_string(),
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

        sets::validate_set(db, set.id, 1)
            .await
            .expect("validate set");

        (domain.id, set.id)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Blocked publish
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_publish_blocked_when_set_not_validated() {
        let db = setup().await;

        let domain = domains::create_reference_domain(
            &db,
            CreateReferenceDomainPayload {
                code: "PUB_DRAFT".to_string(),
                name: "Draft Domain".to_string(),
                structure_type: "flat".to_string(),
                governance_level: "tenant_managed".to_string(),
                is_extendable: Some(true),
                validation_rules_json: None,
            },
            1,
        )
        .await
        .expect("create domain");

        let set = sets::create_draft_set(&db, domain.id, 1)
            .await
            .expect("create draft set");

        // Try publishing a draft set → must fail.
        let readiness = publish::compute_publish_readiness(&db, set.id)
            .await
            .expect("readiness");

        assert!(!readiness.is_ready, "draft set should not be ready");
        assert!(
            readiness.issues.iter().any(|i| i.check == "set_status"),
            "should have set_status blocker"
        );

        let err = publish::publish_reference_set(&db, set.id, 1)
            .await
            .expect_err("publish should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_publish_blocked_when_unresolved_deactivations() {
        let db = setup().await;

        let domain = domains::create_reference_domain(
            &db,
            CreateReferenceDomainPayload {
                code: "PUB_DEACT".to_string(),
                name: "Deactivation Domain".to_string(),
                structure_type: "flat".to_string(),
                governance_level: "tenant_managed".to_string(),
                is_extendable: Some(true),
                validation_rules_json: None,
            },
            1,
        )
        .await
        .expect("create domain");

        let set = sets::create_draft_set(&db, domain.id, 1)
            .await
            .expect("create draft set");

        let val = values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: set.id,
                parent_id: None,
                code: "DEACT_VAL".to_string(),
                label: "Will Be Deactivated".to_string(),
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

        // Deactivate the value (no migration map).
        values::deactivate_value(&db, val.id, 1)
            .await
            .expect("deactivate");

        // Validate the set (deactivated values don't block validation).
        sets::validate_set(&db, set.id, 1)
            .await
            .expect("validate");

        let readiness = publish::compute_publish_readiness(&db, set.id)
            .await
            .expect("readiness");

        assert!(!readiness.is_ready, "unresolved deactivations should block");
        assert!(
            readiness.issues.iter().any(|i| i.check == "unresolved_migrations"),
            "should have unresolved_migrations blocker"
        );
    }

    #[tokio::test]
    async fn v1_readiness_detects_missing_validation_report() {
        let db = setup().await;

        let domain = domains::create_reference_domain(
            &db,
            CreateReferenceDomainPayload {
                code: "PUB_NOREP".to_string(),
                name: "No Report Domain".to_string(),
                structure_type: "flat".to_string(),
                governance_level: "tenant_managed".to_string(),
                is_extendable: Some(true),
                validation_rules_json: None,
            },
            1,
        )
        .await
        .expect("create domain");

        let set = sets::create_draft_set(&db, domain.id, 1)
            .await
            .expect("create draft set");

        // Force set to 'validated' without running validation engine.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_sets SET status = 'validated' WHERE id = ?",
            [set.id.into()],
        ))
        .await
        .expect("force validated");

        let readiness = publish::compute_publish_readiness(&db, set.id)
            .await
            .expect("readiness");

        assert!(!readiness.is_ready);
        assert!(
            readiness.issues.iter().any(|i| i.check == "validation_report_missing"),
            "should flag missing validation report"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Impact preview requirement
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_protected_domain_requires_impact_preview() {
        let db = setup().await;
        let (_domain_id, set_id) = setup_validated_protected_set(&db).await;

        // Without impact preview → not ready.
        let readiness = publish::compute_publish_readiness(&db, set_id)
            .await
            .expect("readiness");

        assert!(readiness.is_protected);
        assert!(readiness.impact_preview_required);
        assert!(!readiness.impact_preview_available);
        assert!(!readiness.is_ready, "protected domain without preview should not be ready");
        assert!(
            readiness.issues.iter().any(|i| i.check == "impact_preview_required"),
            "should have impact_preview_required blocker"
        );

        // Now compute impact preview.
        let impact = publish::preview_publish_impact(&db, set_id)
            .await
            .expect("impact preview");

        assert_eq!(impact.set_id, set_id);
        assert_eq!(impact.dimensions.len(), 6, "all 6 dimensions should be present");

        // After preview, readiness should pass.
        let readiness2 = publish::compute_publish_readiness(&db, set_id)
            .await
            .expect("readiness after preview");

        assert!(readiness2.impact_preview_available);
        assert!(readiness2.is_ready, "should be ready after impact preview");
    }

    #[tokio::test]
    async fn v2_tenant_domain_does_not_require_impact_preview() {
        let db = setup().await;
        let (_domain_id, set_id) = setup_validated_tenant_set(&db).await;

        let readiness = publish::compute_publish_readiness(&db, set_id)
            .await
            .expect("readiness");

        assert!(!readiness.is_protected);
        assert!(!readiness.impact_preview_required);
        assert!(readiness.is_ready, "tenant domain should be ready without preview");
    }

    #[tokio::test]
    async fn v2_impact_preview_returns_all_dimensions_with_status() {
        let db = setup().await;
        let (_domain_id, set_id) = setup_validated_protected_set(&db).await;

        let impact = publish::preview_publish_impact(&db, set_id)
            .await
            .expect("impact");

        assert_eq!(impact.dimensions.len(), 6);

        let dim_names: Vec<&str> = impact.dimensions.iter().map(|d| d.module.as_str()).collect();
        assert!(dim_names.contains(&"assets"));
        assert!(dim_names.contains(&"work_orders"));
        assert!(dim_names.contains(&"pm_plans"));
        assert!(dim_names.contains(&"inventory"));
        assert!(dim_names.contains(&"reliability_events"));
        assert!(dim_names.contains(&"external_integrations"));

        // Assets and PM plans are wired. Depending on changed codes they can
        // report either "available" (impacts evaluated) or "no_impact".
        for dim in &impact.dimensions {
            if dim.module == "assets" || dim.module == "pm_plans" {
                assert!(
                    dim.status == "available" || dim.status == "no_impact",
                    "module {} should be available or no_impact, got {}",
                    dim.module,
                    dim.status
                );
            } else {
                assert_eq!(dim.status, "unavailable",
                    "module {} should be unavailable", dim.module);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Successful publish transition
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_publish_succeeds_and_supersedes_prior_set() {
        let db = setup().await;
        let (domain_id, set_id_v1) = setup_validated_tenant_set(&db).await;

        // Publish V1.
        let result_v1 = publish::publish_reference_set(&db, set_id_v1, 1)
            .await
            .expect("publish V1");

        assert_eq!(result_v1.set.status, "published");
        assert!(result_v1.set.published_at.is_some());
        assert!(result_v1.superseded_set_id.is_none(), "no prior set to supersede");

        // Create V2, validate, and publish.
        let set_v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create V2");

        values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: set_v2.id,
                parent_id: None,
                code: "VAL_B".to_string(),
                label: "Value B".to_string(),
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
        .expect("create value v2");

        sets::validate_set(&db, set_v2.id, 1)
            .await
            .expect("validate V2");

        let result_v2 = publish::publish_reference_set(&db, set_v2.id, 1)
            .await
            .expect("publish V2");

        assert_eq!(result_v2.set.status, "published");
        assert_eq!(
            result_v2.superseded_set_id,
            Some(set_id_v1),
            "V1 should be superseded"
        );

        // Verify V1 is now superseded.
        let v1_after = sets::get_reference_set(&db, set_id_v1)
            .await
            .expect("get V1 after");
        assert_eq!(v1_after.status, "superseded");
    }

    #[tokio::test]
    async fn v3_publish_sets_effective_from_timestamp() {
        let db = setup().await;
        let (_domain_id, set_id) = setup_validated_tenant_set(&db).await;

        let result = publish::publish_reference_set(&db, set_id, 1)
            .await
            .expect("publish");

        assert!(result.set.effective_from.is_some(), "effective_from should be set");
        assert!(result.set.published_at.is_some(), "published_at should be set");
    }
}
