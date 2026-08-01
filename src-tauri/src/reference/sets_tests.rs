//! Supervisor verification tests for Phase 2 SP03 File 01 Sprint S2.
//!
//! V1 â€” Transition ordering: draft â†’ validated â†’ published; skip not allowed
//! V2 â€” Single published set: publish v2 supersedes v1
//! V3 â€” Published edit block: published set cannot be directly edited

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::sets::{self, SET_STATUS_DRAFT, SET_STATUS_PUBLISHED, SET_STATUS_SUPERSEDED, SET_STATUS_VALIDATED};

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

    /// Creates a reference domain and returns its id.
    async fn setup_domain(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "FAILURE_CLASS".to_string(),
            name: "Classes de dÃ©faillance".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "protected_analytical".to_string(),
            governance_category: Some("controlled_catalog".to_string()),
            is_extendable: Some(false),
            validation_rules_json: None,
        };
        let domain = domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create domain");
        domain.id
    }

    /// Helper: creates a second domain with a different code.
    async fn setup_domain_2(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "EQUIPMENT_FAMILY".to_string(),
            name: "Familles d'Ã©quipements".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "tenant_managed".to_string(),
                governance_category: Some("controlled_catalog".to_string()),
            is_extendable: Some(true),
            validation_rules_json: None,
        };
        let domain = domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create domain 2");
        domain.id
    }

    // â”€â”€ V1 â€” Transition ordering â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[tokio::test]
    async fn v1_publish_draft_directly_must_fail() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        assert_eq!(draft.status, SET_STATUS_DRAFT);

        // Attempt to publish a draft directly â€” must fail
        let err = sets::publish_set(&db, draft.id, 1)
            .await
            .expect_err("publishing draft directly should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("draft"),
                    "error should mention 'draft', got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_superseded_to_validated_must_fail() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        // Create, validate, publish v1 â†’ becomes published
        let v1 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v1 draft");
        let v1 = sets::validate_set(&db, v1.id, 1)
            .await
            .expect("validate v1");
        let v1 = sets::publish_set(&db, v1.id, 1)
            .await
            .expect("publish v1");
        assert_eq!(v1.status, SET_STATUS_PUBLISHED);

        // Create, validate, publish v2 â†’ v1 becomes superseded
        let v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v2 draft");
        let v2 = sets::validate_set(&db, v2.id, 1)
            .await
            .expect("validate v2");
        let _v2 = sets::publish_set(&db, v2.id, 1)
            .await
            .expect("publish v2");

        // Confirm v1 is superseded
        let v1_after = sets::get_reference_set(&db, v1.id)
            .await
            .expect("get v1");
        assert_eq!(v1_after.status, SET_STATUS_SUPERSEDED);

        // Try to validate superseded v1 â€” must fail
        let err = sets::validate_set(&db, v1.id, 1)
            .await
            .expect_err("validate superseded should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_validated_cannot_be_revalidated() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        let validated = sets::validate_set(&db, draft.id, 1)
            .await
            .expect("validate");
        assert_eq!(validated.status, SET_STATUS_VALIDATED);

        // Try to validate again â€” must fail (already validated, not draft)
        let err = sets::validate_set(&db, validated.id, 1)
            .await
            .expect_err("revalidate should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_correct_lifecycle_draft_validated_published() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        assert_eq!(draft.status, SET_STATUS_DRAFT);
        assert_eq!(draft.version_no, 1);
        assert!(draft.published_at.is_none());

        let validated = sets::validate_set(&db, draft.id, 1)
            .await
            .expect("validate");
        assert_eq!(validated.status, SET_STATUS_VALIDATED);

        let published = sets::publish_set(&db, validated.id, 1)
            .await
            .expect("publish");
        assert_eq!(published.status, SET_STATUS_PUBLISHED);
        assert!(published.published_at.is_some());
    }

    #[tokio::test]
    async fn v1_published_cannot_be_validated() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        let validated = sets::validate_set(&db, draft.id, 1)
            .await
            .expect("validate");
        let published = sets::publish_set(&db, validated.id, 1)
            .await
            .expect("publish");

        let err = sets::validate_set(&db, published.id, 1)
            .await
            .expect_err("validate published should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // â”€â”€ V2 â€” Single published set â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[tokio::test]
    async fn v2_publish_v2_supersedes_v1() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        // Create and publish v1
        let v1 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v1 draft");
        let v1 = sets::validate_set(&db, v1.id, 1).await.expect("validate v1");
        let v1 = sets::publish_set(&db, v1.id, 1).await.expect("publish v1");
        assert_eq!(v1.status, SET_STATUS_PUBLISHED);

        // Create and publish v2
        let v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v2 draft");
        assert_eq!(v2.version_no, 2);

        let v2 = sets::validate_set(&db, v2.id, 1).await.expect("validate v2");
        let v2 = sets::publish_set(&db, v2.id, 1).await.expect("publish v2");
        assert_eq!(v2.status, SET_STATUS_PUBLISHED);

        // v1 must now be superseded
        let v1_after = sets::get_reference_set(&db, v1.id)
            .await
            .expect("get v1");
        assert_eq!(
            v1_after.status, SET_STATUS_SUPERSEDED,
            "v1 should be superseded after v2 publish"
        );

        // Only one published set for this domain
        let all = sets::list_sets_for_domain(&db, domain_id)
            .await
            .expect("list sets");
        let published_count = all
            .iter()
            .filter(|s| s.status == SET_STATUS_PUBLISHED)
            .count();
        assert_eq!(
            published_count, 1,
            "exactly one published set per domain"
        );
    }

    #[tokio::test]
    async fn v2_publish_across_domains_independent() {
        let db = setup().await;
        let domain_a = setup_domain(&db).await;
        let domain_b = setup_domain_2(&db).await;

        // Publish v1 in domain A
        let a1 = sets::create_draft_set(&db, domain_a, 1)
            .await
            .expect("create A1");
        let a1 = sets::validate_set(&db, a1.id, 1).await.expect("validate A1");
        let a1 = sets::publish_set(&db, a1.id, 1).await.expect("publish A1");

        // Publish v1 in domain B
        let b1 = sets::create_draft_set(&db, domain_b, 1)
            .await
            .expect("create B1");
        let b1 = sets::validate_set(&db, b1.id, 1).await.expect("validate B1");
        let b1 = sets::publish_set(&db, b1.id, 1).await.expect("publish B1");

        // Both should be published â€” domains are independent
        let a1_r = sets::get_reference_set(&db, a1.id).await.expect("get A1");
        let b1_r = sets::get_reference_set(&db, b1.id).await.expect("get B1");
        assert_eq!(a1_r.status, SET_STATUS_PUBLISHED);
        assert_eq!(b1_r.status, SET_STATUS_PUBLISHED);

        // Publish v2 in domain A â€” should NOT affect domain B
        let a2 = sets::create_draft_set(&db, domain_a, 1)
            .await
            .expect("create A2");
        let a2 = sets::validate_set(&db, a2.id, 1).await.expect("validate A2");
        let _a2 = sets::publish_set(&db, a2.id, 1).await.expect("publish A2");

        let b1_still = sets::get_reference_set(&db, b1.id).await.expect("get B1");
        assert_eq!(
            b1_still.status, SET_STATUS_PUBLISHED,
            "domain B should not be affected by domain A publish"
        );
    }

    #[tokio::test]
    async fn v2_version_numbers_auto_increment() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        // v1
        let v1 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v1");
        assert_eq!(v1.version_no, 1);
        let v1 = sets::validate_set(&db, v1.id, 1).await.expect("validate v1");
        let _v1 = sets::publish_set(&db, v1.id, 1).await.expect("publish v1");

        // v2
        let v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v2");
        assert_eq!(v2.version_no, 2);
        let v2 = sets::validate_set(&db, v2.id, 1).await.expect("validate v2");
        let _v2 = sets::publish_set(&db, v2.id, 1).await.expect("publish v2");

        // v3
        let v3 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v3");
        assert_eq!(v3.version_no, 3);
    }

    // â”€â”€ V3 â€” Published edit block â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[tokio::test]
    async fn v3_published_set_immutable_via_guard() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        let validated = sets::validate_set(&db, draft.id, 1)
            .await
            .expect("validate");
        let published = sets::publish_set(&db, validated.id, 1)
            .await
            .expect("publish");

        // The guard function used by value operations must block edits
        let err = sets::assert_set_is_draft(&published);
        assert!(err.is_err(), "published set must not pass draft guard");

        let err = sets::assert_set_is_editable(&published);
        assert!(err.is_err(), "published set must not pass editable guard");
    }

    #[tokio::test]
    async fn v3_superseded_set_immutable_via_guard() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        // Publish v1 then v2 to supersede v1
        let v1 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v1");
        let v1 = sets::validate_set(&db, v1.id, 1).await.expect("validate v1");
        let v1 = sets::publish_set(&db, v1.id, 1).await.expect("publish v1");

        let v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create v2");
        let v2 = sets::validate_set(&db, v2.id, 1).await.expect("validate v2");
        let _v2 = sets::publish_set(&db, v2.id, 1).await.expect("publish v2");

        let v1_superseded = sets::get_reference_set(&db, v1.id)
            .await
            .expect("get v1");

        let err = sets::assert_set_is_draft(&v1_superseded);
        assert!(err.is_err(), "superseded set must not pass draft guard");

        let err = sets::assert_set_is_editable(&v1_superseded);
        assert!(err.is_err(), "superseded set must not pass editable guard");
    }

    #[tokio::test]
    async fn v3_publish_already_published_set_rejected() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        let validated = sets::validate_set(&db, draft.id, 1)
            .await
            .expect("validate");
        let published = sets::publish_set(&db, validated.id, 1)
            .await
            .expect("publish");

        // Attempt to publish again â€” must fail
        let err = sets::publish_set(&db, published.id, 1)
            .await
            .expect_err("re-publish should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // â”€â”€ Additional edge-case coverage â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[tokio::test]
    async fn only_one_draft_per_domain() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let _draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create first draft");

        // Creating a second draft for the same domain must fail
        let err = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect_err("second draft should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn create_draft_for_nonexistent_domain_rejected() {
        let db = setup().await;

        let err = sets::create_draft_set(&db, 999_999, 1)
            .await
            .expect_err("nonexistent domain should fail");

        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn list_sets_ordered_by_version_desc() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        // Create and publish v1
        let v1 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("v1");
        let v1 = sets::validate_set(&db, v1.id, 1).await.expect("v1");
        let _v1 = sets::publish_set(&db, v1.id, 1).await.expect("v1");

        // Create v2 as draft
        let _v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("v2");

        let list = sets::list_sets_for_domain(&db, domain_id)
            .await
            .expect("list");

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].version_no, 2, "first item should be newest");
        assert_eq!(list[1].version_no, 1, "second item should be oldest");
    }

    #[tokio::test]
    async fn get_nonexistent_set_returns_not_found() {
        let db = setup().await;

        let err = sets::get_reference_set(&db, 999_999)
            .await
            .expect_err("nonexistent set should fail");

        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn validate_nonexistent_set_returns_not_found() {
        let db = setup().await;

        let err = sets::validate_set(&db, 999_999, 1)
            .await
            .expect_err("nonexistent set should fail");

        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn publish_nonexistent_set_returns_not_found() {
        let db = setup().await;

        let err = sets::publish_set(&db, 999_999, 1)
            .await
            .expect_err("nonexistent set should fail");

        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn draft_set_records_created_by() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 42)
            .await
            .expect("create draft");

        assert_eq!(draft.created_by_id, Some(42));
    }

    // ── Clone-from-published + discard ─────────────────────────────────────

    #[tokio::test]
    async fn bootstrap_draft_is_empty_when_no_published() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("bootstrap draft");
        let vals = crate::reference::values::list_values(&db, draft.id)
            .await
            .expect("list");
        assert!(vals.is_empty(), "bootstrap draft must be empty");
    }

    #[tokio::test]
    async fn clone_from_published_preserves_hierarchy_metadata_and_aliases() {
        use crate::reference::aliases::{self, CreateReferenceAliasPayload};
        use crate::reference::values::{self, CreateReferenceValuePayload};

        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("v1 draft");

        let root = values::create_value(
            &db,
            CreateReferenceValuePayload {
            set_id: draft.id,
            parent_id: None,
            code: "ROOT".into(),
            label: "Root".into(),
            description: Some("root desc".into()),
            sort_order: Some(10),
            color_hex: Some("#112233".into()),
            icon_name: None,
            semantic_tag: None,
            external_code: Some("EXT-R".into()),
            metadata_json: Some(r#"{"k":"v"}"#.into()),
            },
            1,
        )
        .await
        .expect("root");

        let child = values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: draft.id,
                parent_id: Some(root.id),
                code: "CHILD".into(),
                label: "Child".into(),
                description: None,
                sort_order: Some(20),
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            },
            1,
        )
        .await
        .expect("child");

        // Inactive leaf
        let inactive = values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: draft.id,
                parent_id: None,
                code: "OLD".into(),
                label: "Old".into(),
                description: None,
                sort_order: Some(30),
                color_hex: None,
                icon_name: None,
                semantic_tag: None,
                external_code: None,
                metadata_json: None,
            },
            1,
        )
        .await
        .expect("inactive");
        values::deactivate_value(&db, inactive.id, 1)
            .await
            .expect("deactivate");

        aliases::create_alias(
            &db,
            CreateReferenceAliasPayload {
                reference_value_id: root.id,
                alias_label: "Racine".into(),
                locale: "fr".into(),
                alias_type: "search".into(),
                is_preferred: Some(true),
            },
            1,
        )
        .await
        .expect("alias");

        sets::validate_set(&db, draft.id, 1)
            .await
            .expect("validate");
        let published = sets::publish_set(&db, draft.id, 1)
            .await
            .expect("publish");

        let v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("clone draft");

        let cloned = values::list_values(&db, v2.id).await.expect("list clone");
        assert_eq!(cloned.len(), 3);

        let c_root = cloned.iter().find(|v| v.code == "ROOT").expect("ROOT");
        let c_child = cloned.iter().find(|v| v.code == "CHILD").expect("CHILD");
        let c_old = cloned.iter().find(|v| v.code == "OLD").expect("OLD");

        assert_ne!(c_root.id, root.id);
        assert_ne!(c_child.id, child.id);
        assert_eq!(c_child.parent_id, Some(c_root.id), "parent remapped");
        assert_eq!(c_root.description.as_deref(), Some("root desc"));
        assert_eq!(c_root.sort_order, Some(10));
        assert_eq!(c_root.color_hex.as_deref(), Some("#112233"));
        assert_eq!(c_root.external_code.as_deref(), Some("EXT-R"));
        assert_eq!(c_root.metadata_json.as_deref(), Some(r#"{"k":"v"}"#));
        assert!(!c_old.is_active, "inactive preserved");

        let aliases = aliases::list_aliases(&db, c_root.id)
            .await
            .expect("aliases");
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].alias_label, "Racine");
        assert!(aliases[0].is_preferred);

        // Published untouched
        let pub_vals = values::list_values(&db, published.id)
            .await
            .expect("pub vals");
        assert_eq!(pub_vals.len(), 3);
        assert_eq!(pub_vals.iter().find(|v| v.code == "ROOT").unwrap().id, root.id);
    }

    #[tokio::test]
    async fn discard_draft_removes_set_keeps_published() {
        use crate::reference::values::{self, CreateReferenceValuePayload};

        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let v1 = sets::create_draft_set(&db, domain_id, 1).await.expect("v1");
        values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: v1.id,
                parent_id: None,
                code: "A".into(),
                label: "A".into(),
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
        .expect("val");
        sets::validate_set(&db, v1.id, 1).await.expect("validate");
        let published = sets::publish_set(&db, v1.id, 1).await.expect("publish");

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("draft");
        assert!(!values::list_values(&db, draft.id).await.unwrap().is_empty());

        sets::discard_draft_set(&db, draft.id)
            .await
            .expect("discard");

        let err = sets::get_reference_set(&db, draft.id)
            .await
            .expect_err("draft gone");
        assert!(matches!(err, AppError::NotFound { .. }));

        let still = sets::get_reference_set(&db, published.id)
            .await
            .expect("published remains");
        assert_eq!(still.status, SET_STATUS_PUBLISHED);

        // Can create a new draft again
        let again = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("new draft after discard");
        assert_eq!(again.status, SET_STATUS_DRAFT);
    }

    #[tokio::test]
    async fn discard_rejects_published_set() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;
        let v1 = sets::create_draft_set(&db, domain_id, 1).await.expect("draft");
        sets::validate_set(&db, v1.id, 1).await.expect("validate");
        let published = sets::publish_set(&db, v1.id, 1).await.expect("publish");

        let err = sets::discard_draft_set(&db, published.id)
            .await
            .expect_err("cannot discard published");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn publish_after_clone_supersedes_prior() {
        use crate::reference::values::{self, CreateReferenceValuePayload};

        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let v1 = sets::create_draft_set(&db, domain_id, 1).await.expect("v1");
        values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: v1.id,
                parent_id: None,
                code: "KEEP".into(),
                label: "Keep".into(),
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
        .expect("val");
        sets::validate_set(&db, v1.id, 1).await.expect("validate");
        sets::publish_set(&db, v1.id, 1).await.expect("publish v1");

        let v2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("clone");
        sets::validate_set(&db, v2.id, 1).await.expect("validate v2");
        let published = sets::publish_set(&db, v2.id, 1).await.expect("publish v2");
        assert_eq!(published.status, SET_STATUS_PUBLISHED);

        let prior = sets::get_reference_set(&db, v1.id).await.expect("v1");
        assert_eq!(prior.status, SET_STATUS_SUPERSEDED);

        let live = values::list_values(&db, published.id).await.expect("live");
        assert!(live.iter().any(|v| v.code == "KEEP"));
    }

    #[tokio::test]
    async fn clone_rolls_back_on_orphan_hierarchy() {
        use crate::reference::values::{self, CreateReferenceValuePayload};

        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let v1 = sets::create_draft_set(&db, domain_id, 1).await.expect("v1");
        values::create_value(
            &db,
            CreateReferenceValuePayload {
                set_id: v1.id,
                parent_id: None,
                code: "OK".into(),
                label: "Ok".into(),
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
        .expect("val");
        sets::validate_set(&db, v1.id, 1).await.expect("validate");
        sets::publish_set(&db, v1.id, 1).await.expect("publish");

        // Corrupt published set with orphan parent_id (raw insert)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (set_id, parent_id, code, label, is_active) \
             VALUES (?, 999999, 'ORPH', 'Orphan', 1)",
            [v1.id.into()],
        ))
        .await
        .expect("corrupt");

        let err = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect_err("clone must fail on orphan");
        assert!(matches!(err, AppError::ValidationFailed(_)));

        let sets = sets::list_sets_for_domain(&db, domain_id)
            .await
            .expect("list");
        assert!(
            !sets.iter().any(|s| s.status == SET_STATUS_DRAFT),
            "failed clone must leave no draft"
        );
    }
}
