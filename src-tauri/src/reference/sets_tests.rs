//! Supervisor verification tests for Phase 2 SP03 File 01 Sprint S2.
//!
//! V1 — Transition ordering: draft → validated → published; skip not allowed
//! V2 — Single published set: publish v2 supersedes v1
//! V3 — Published edit block: published set cannot be directly edited

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
            name: "Classes de défaillance".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "protected_analytical".to_string(),
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
            name: "Familles d'équipements".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "tenant_managed".to_string(),
            is_extendable: Some(true),
            validation_rules_json: None,
        };
        let domain = domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create domain 2");
        domain.id
    }

    // ── V1 — Transition ordering ──────────────────────────────────────────

    #[tokio::test]
    async fn v1_publish_draft_directly_must_fail() {
        let db = setup().await;
        let domain_id = setup_domain(&db).await;

        let draft = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("create draft");
        assert_eq!(draft.status, SET_STATUS_DRAFT);

        // Attempt to publish a draft directly — must fail
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

        // Create, validate, publish v1 → becomes published
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

        // Create, validate, publish v2 → v1 becomes superseded
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

        // Try to validate superseded v1 — must fail
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

        // Try to validate again — must fail (already validated, not draft)
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

    // ── V2 — Single published set ─────────────────────────────────────────

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

        // Both should be published — domains are independent
        let a1_r = sets::get_reference_set(&db, a1.id).await.expect("get A1");
        let b1_r = sets::get_reference_set(&db, b1.id).await.expect("get B1");
        assert_eq!(a1_r.status, SET_STATUS_PUBLISHED);
        assert_eq!(b1_r.status, SET_STATUS_PUBLISHED);

        // Publish v2 in domain A — should NOT affect domain B
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

    // ── V3 — Published edit block ─────────────────────────────────────────

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

        // Attempt to publish again — must fail
        let err = sets::publish_set(&db, published.id, 1)
            .await
            .expect_err("re-publish should fail");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ── Additional edge-case coverage ─────────────────────────────────────

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
}
