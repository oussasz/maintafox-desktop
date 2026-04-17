//! Supervisor verification tests for Phase 2 SP03 File 03 Sprint S1.
//!
//! V1 — Preferred alias uniqueness
//! V2 — Duplicate alias guard
//! V3 — Delete behavior (auto-promote on preferred deletion)

#[cfg(test)]
mod tests {
    use sea_orm::{Database, DbBackend, Statement, ConnectionTrait};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::reference::aliases::{
        self, CreateReferenceAliasPayload, UpdateReferenceAliasPayload,
    };
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
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

    /// Creates a flat domain + draft set + one value, returns (domain_id, set_id, value_id).
    async fn setup_value(db: &sea_orm::DatabaseConnection) -> (i64, i64, i64) {
        let domain = domains::create_reference_domain(
            db,
            CreateReferenceDomainPayload {
                code: "ALIAS_TEST_DOM".to_string(),
                name: "Alias Test Domain".to_string(),
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

        let value = values::create_value(
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

        (domain.id, set.id, value.id)
    }

    fn alias_payload(
        value_id: i64,
        label: &str,
        locale: &str,
        alias_type: &str,
        preferred: bool,
    ) -> CreateReferenceAliasPayload {
        CreateReferenceAliasPayload {
            reference_value_id: value_id,
            alias_label: label.to_string(),
            locale: locale.to_string(),
            alias_type: alias_type.to_string(),
            is_preferred: Some(preferred),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Preferred alias uniqueness
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_only_one_preferred_alias_per_scope() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        // Create first preferred alias
        let a1 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Ancien nom", "fr", "legacy", true),
            1,
        )
        .await
        .expect("first preferred");
        assert!(a1.is_preferred);

        // Create second preferred alias in same scope — should demote a1
        let a2 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Autre ancien nom", "fr", "legacy", true),
            1,
        )
        .await
        .expect("second preferred");
        assert!(a2.is_preferred);

        // Verify a1 was demoted
        let a1_refreshed = aliases::get_alias(&db, a1.id).await.expect("get a1");
        assert!(
            !a1_refreshed.is_preferred,
            "First alias should have been demoted when second was marked preferred"
        );
    }

    #[tokio::test]
    async fn v1_preferred_uniqueness_scoped_by_locale_and_type() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        // Preferred in fr/legacy
        let a_fr = aliases::create_alias(
            &db,
            alias_payload(val_id, "Ancien en FR", "fr", "legacy", true),
            1,
        )
        .await
        .expect("fr preferred");

        // Preferred in en/legacy — different locale, should NOT demote fr one
        let a_en = aliases::create_alias(
            &db,
            alias_payload(val_id, "Old name EN", "en", "legacy", true),
            1,
        )
        .await
        .expect("en preferred");

        // Preferred in fr/search — different type, should NOT demote fr/legacy one
        let a_search = aliases::create_alias(
            &db,
            alias_payload(val_id, "Recherche FR", "fr", "search", true),
            1,
        )
        .await
        .expect("fr search preferred");

        // All three should still be preferred
        let a_fr_r = aliases::get_alias(&db, a_fr.id).await.expect("get fr");
        let a_en_r = aliases::get_alias(&db, a_en.id).await.expect("get en");
        let a_search_r = aliases::get_alias(&db, a_search.id).await.expect("get search");
        assert!(a_fr_r.is_preferred);
        assert!(a_en_r.is_preferred);
        assert!(a_search_r.is_preferred);
    }

    #[tokio::test]
    async fn v1_update_to_preferred_demotes_existing() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        let a1 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Nom A", "fr", "legacy", true),
            1,
        )
        .await
        .expect("a1");

        let a2 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Nom B", "fr", "legacy", false),
            1,
        )
        .await
        .expect("a2");

        // Update a2 to preferred — should demote a1
        let a2_updated = aliases::update_alias(
            &db,
            a2.id,
            UpdateReferenceAliasPayload {
                alias_label: None,
                locale: None,
                alias_type: None,
                is_preferred: Some(true),
            },
            1,
        )
        .await
        .expect("update a2");
        assert!(a2_updated.is_preferred);

        let a1_r = aliases::get_alias(&db, a1.id).await.expect("get a1");
        assert!(!a1_r.is_preferred, "a1 should be demoted after a2 became preferred");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Duplicate alias guard
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_duplicate_alias_label_rejected() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        aliases::create_alias(
            &db,
            alias_payload(val_id, "Duplicate Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect("first alias");

        let err = aliases::create_alias(
            &db,
            alias_payload(val_id, "Duplicate Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect_err("duplicate should fail");

        assert!(
            matches!(err, AppError::ValidationFailed(_)),
            "Expected ValidationFailed, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn v2_same_label_different_type_allowed() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        aliases::create_alias(
            &db,
            alias_payload(val_id, "Same Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect("legacy alias");

        // Same label but different type — should succeed
        aliases::create_alias(
            &db,
            alias_payload(val_id, "Same Label", "fr", "search", false),
            1,
        )
        .await
        .expect("search alias with same label should be allowed");
    }

    #[tokio::test]
    async fn v2_same_label_different_locale_allowed() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        aliases::create_alias(
            &db,
            alias_payload(val_id, "Same Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect("fr alias");

        // Same label but different locale — should succeed
        aliases::create_alias(
            &db,
            alias_payload(val_id, "Same Label", "en", "legacy", false),
            1,
        )
        .await
        .expect("en alias with same label should be allowed");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Delete behavior (auto-promote on preferred deletion)
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_delete_preferred_auto_promotes_oldest() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        let a1 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Preferred Alias", "fr", "legacy", true),
            1,
        )
        .await
        .expect("preferred");

        let a2 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Second Alias", "fr", "legacy", false),
            1,
        )
        .await
        .expect("second");

        let _a3 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Third Alias", "fr", "legacy", false),
            1,
        )
        .await
        .expect("third");

        // Delete the preferred alias
        aliases::delete_alias(&db, a1.id, 1)
            .await
            .expect("delete preferred");

        // a2 should have been auto-promoted (oldest by id)
        let a2_r = aliases::get_alias(&db, a2.id).await.expect("get a2");
        assert!(
            a2_r.is_preferred,
            "Oldest remaining alias should be auto-promoted to preferred"
        );
    }

    #[tokio::test]
    async fn v3_delete_non_preferred_no_promotion() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        let a1 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Preferred", "fr", "legacy", true),
            1,
        )
        .await
        .expect("preferred");

        let a2 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Non-preferred", "fr", "legacy", false),
            1,
        )
        .await
        .expect("non-preferred");

        // Delete the non-preferred alias
        aliases::delete_alias(&db, a2.id, 1)
            .await
            .expect("delete non-preferred");

        // a1 should still be preferred
        let a1_r = aliases::get_alias(&db, a1.id).await.expect("get a1");
        assert!(a1_r.is_preferred, "Original preferred should remain");
    }

    #[tokio::test]
    async fn v3_delete_last_alias_no_panic() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        let a1 = aliases::create_alias(
            &db,
            alias_payload(val_id, "Only Alias", "fr", "legacy", true),
            1,
        )
        .await
        .expect("only alias");

        // Delete the only alias — should not panic (nothing to promote)
        aliases::delete_alias(&db, a1.id, 1)
            .await
            .expect("delete last alias should succeed");

        // List should now be empty
        let list = aliases::list_aliases(&db, val_id).await.expect("list");
        assert!(list.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Additional coverage: list and basic CRUD
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn list_aliases_returns_ordered_results() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        aliases::create_alias(
            &db,
            alias_payload(val_id, "B Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect("b");

        aliases::create_alias(
            &db,
            alias_payload(val_id, "A Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect("a");

        aliases::create_alias(
            &db,
            alias_payload(val_id, "C Label", "en", "search", true),
            1,
        )
        .await
        .expect("c");

        let all = aliases::list_aliases(&db, val_id).await.expect("list");
        assert_eq!(all.len(), 3);
        // Ordered by locale ASC, alias_type ASC, alias_label ASC
        assert_eq!(all[0].locale, "en");
        assert_eq!(all[1].alias_label, "A Label");
        assert_eq!(all[2].alias_label, "B Label");
    }

    #[tokio::test]
    async fn create_alias_validates_empty_label() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        let err = aliases::create_alias(
            &db,
            alias_payload(val_id, "   ", "fr", "legacy", false),
            1,
        )
        .await
        .expect_err("empty label");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn create_alias_validates_invalid_type() {
        let db = setup().await;
        let (_dom, _set, val_id) = setup_value(&db).await;

        let err = aliases::create_alias(
            &db,
            alias_payload(val_id, "Valid Label", "fr", "unknown_type", false),
            1,
        )
        .await
        .expect_err("invalid type");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn create_alias_rejects_nonexistent_value() {
        let db = setup().await;

        let err = aliases::create_alias(
            &db,
            alias_payload(99999, "Some Label", "fr", "legacy", false),
            1,
        )
        .await
        .expect_err("nonexistent value");

        assert!(matches!(err, AppError::NotFound { .. }));
    }
}
