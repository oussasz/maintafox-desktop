//! Supervisor verification tests for Sprint S1 — Structure Model Service.
//!
//! V1 — Model creation and versioning
//! V2 — Publish transitions correctly
//! V3 — Archive guard

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::structure_model::{self, CreateStructureModelPayload};

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

    // ── V1 — Model creation and versioning ────────────────────────────────

    #[tokio::test]
    async fn v1_create_model_assigns_draft_status() {
        let db = setup().await;

        let model = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("First model".to_string()),
            },
            1,
        )
        .await
        .expect("create_model should succeed");

        assert_eq!(model.status, "draft");
        assert_eq!(model.version_number, 1);
        assert_eq!(model.description.as_deref(), Some("First model"));
        assert!(model.activated_at.is_none());
        assert!(model.activated_by_id.is_none());
    }

    #[tokio::test]
    async fn v1_second_model_gets_version_2() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("first model");

        let m2 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("second model");

        assert_eq!(m1.version_number, 1);
        assert_eq!(m1.status, "draft");
        assert_eq!(m2.version_number, 2);
        assert_eq!(m2.status, "draft");
    }

    #[tokio::test]
    async fn v1_list_models_returns_descending_by_version() {
        let db = setup().await;

        structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("first model");

        structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("second model");

        let models = structure_model::list_models(&db).await.expect("list");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].version_number, 2); // descending
        assert_eq!(models[1].version_number, 1);
    }

    #[tokio::test]
    async fn v1_get_active_model_returns_none_when_no_publish() {
        let db = setup().await;

        // Create a draft but don't publish it
        structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("create");

        let active = structure_model::get_active_model(&db)
            .await
            .expect("get_active_model should not error");
        assert!(active.is_none(), "no model should be active yet");
    }

    // ── V2 — Publish transitions correctly ────────────────────────────────

    #[tokio::test]
    async fn v2_publish_first_model_makes_it_active() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        let m2 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m2");

        // Publish model 1
        let published = structure_model::publish_model(&db, m1.id, 1).await.expect("publish m1");
        assert_eq!(published.status, "active");
        assert!(published.activated_at.is_some());
        assert_eq!(published.activated_by_id, Some(1));

        // Model 2 is still draft
        let m2_after = structure_model::get_model_by_id(&db, m2.id).await.expect("get m2");
        assert_eq!(m2_after.status, "draft");

        // Active model query returns m1
        let active = structure_model::get_active_model(&db)
            .await
            .expect("get active")
            .expect("should have an active model");
        assert_eq!(active.id, m1.id);
    }

    #[tokio::test]
    async fn v2_publish_second_model_supersedes_first() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        let m2 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m2");

        // Publish model 1, then model 2
        structure_model::publish_model(&db, m1.id, 1).await.expect("publish m1");
        structure_model::publish_model(&db, m2.id, 1).await.expect("publish m2");

        // After: m1 = superseded, m2 = active
        let m1_after = structure_model::get_model_by_id(&db, m1.id).await.expect("get m1");
        assert_eq!(m1_after.status, "superseded");
        assert!(m1_after.superseded_at.is_some());

        let m2_after = structure_model::get_model_by_id(&db, m2.id).await.expect("get m2");
        assert_eq!(m2_after.status, "active");

        // Only one active model at a time — verify via count
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM org_structure_models WHERE status = 'active'".to_string(),
            ))
            .await
            .expect("query")
            .expect("row");
        let active_count: i32 = row.try_get("", "cnt").expect("cnt");
        assert_eq!(active_count, 1, "exactly one active model must exist");
    }

    #[tokio::test]
    async fn v2_cannot_publish_non_draft_model() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        // Publish once
        structure_model::publish_model(&db, m1.id, 1).await.expect("publish");

        // Attempt to publish again — should fail (it's now "active", not "draft")
        let err = structure_model::publish_model(&db, m1.id, 1)
            .await
            .expect_err("should reject non-draft publish");

        let msg = err.to_string();
        assert!(
            msg.contains("not 'draft'"),
            "error message should mention draft requirement, got: {msg}"
        );
    }

    // ── V3 — Archive guard ────────────────────────────────────────────────

    #[tokio::test]
    async fn v3_archive_active_model_is_rejected() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        structure_model::publish_model(&db, m1.id, 1).await.expect("publish");

        let err = structure_model::archive_model(&db, m1.id)
            .await
            .expect_err("archiving active model should fail");

        let msg = err.to_string();
        assert!(
            msg.contains("cannot archive the active model"),
            "error should mention active model guard, got: {msg}"
        );
    }

    #[tokio::test]
    async fn v3_archive_draft_model_succeeds() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        let archived = structure_model::archive_model(&db, m1.id)
            .await
            .expect("archiving draft should succeed");
        assert_eq!(archived.status, "archived");
    }

    #[tokio::test]
    async fn v3_archive_superseded_model_succeeds() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        let m2 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m2");

        // Publish m1, then m2 → m1 becomes superseded
        structure_model::publish_model(&db, m1.id, 1).await.expect("publish m1");
        structure_model::publish_model(&db, m2.id, 1).await.expect("publish m2");

        let archived = structure_model::archive_model(&db, m1.id)
            .await
            .expect("archiving superseded should succeed");
        assert_eq!(archived.status, "archived");
    }

    // ── Bonus: update_model_description guard ─────────────────────────────

    #[tokio::test]
    async fn update_description_rejected_on_non_draft() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        structure_model::publish_model(&db, m1.id, 1).await.expect("publish");

        let err = structure_model::update_model_description(&db, m1.id, Some("new desc".to_string()))
            .await
            .expect_err("should reject update on active model");

        let msg = err.to_string();
        assert!(
            msg.contains("not a draft"),
            "error should mention draft-only editing, got: {msg}"
        );
    }

    #[tokio::test]
    async fn update_description_succeeds_on_draft() {
        let db = setup().await;

        let m1 = structure_model::create_model(&db, CreateStructureModelPayload { description: None }, 1)
            .await
            .expect("m1");

        let updated = structure_model::update_model_description(&db, m1.id, Some("Updated description".to_string()))
            .await
            .expect("update should succeed on draft");

        assert_eq!(updated.description.as_deref(), Some("Updated description"));
    }
}
