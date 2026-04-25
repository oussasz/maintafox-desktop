//! Supervisor verification tests for Phase 2 SP03 File 03 Sprint S3.
//!
//! V1 — Legacy alias continuity (old term finds value via legacy alias)
//! V2 — Locale ranking (preferred alias in locale ranks above non-preferred)
//! V3 — Canonical precedence (exact code match always ranks first)

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::reference::aliases::{self, CreateReferenceAliasPayload};
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::search;
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

    /// Create a flat tenant_managed domain with a draft set, returns (domain_code, set_id).
    async fn setup_search_domain(db: &sea_orm::DatabaseConnection) -> (String, i64) {
        let domain = domains::create_reference_domain(
            db,
            CreateReferenceDomainPayload {
                code: "SEARCH_TEST".to_string(),
                name: "Search Test Domain".to_string(),
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

        (domain.code, set.id)
    }

    /// Helper: create a value and return its id.
    async fn add_value(
        db: &sea_orm::DatabaseConnection,
        set_id: i64,
        code: &str,
        label: &str,
    ) -> i64 {
        let v = values::create_value(
            db,
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
            },
            1,
        )
        .await
        .expect("create value");
        v.id
    }

    /// Helper: add an alias.
    async fn add_alias(
        db: &sea_orm::DatabaseConnection,
        value_id: i64,
        label: &str,
        locale: &str,
        alias_type: &str,
        is_preferred: bool,
    ) {
        aliases::create_alias(
            db,
            CreateReferenceAliasPayload {
                reference_value_id: value_id,
                alias_label: label.to_string(),
                locale: locale.to_string(),
                alias_type: alias_type.to_string(),
                is_preferred: Some(is_preferred),
            },
            1,
        )
        .await
        .expect("create alias");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Legacy alias continuity
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_legacy_alias_finds_renamed_value() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        // Value was renamed from "Pompe centrifuge" to "Pompe centrifuge haute pression"
        let val_id = add_value(&db, set_id, "PUMP_HP", "Pompe centrifuge haute pression").await;

        // Legacy alias preserves the old name
        add_alias(&db, val_id, "Pompe centrifuge", "fr", "legacy", true).await;

        // Search using the old name
        let results = search::search_reference_values(&db, &domain_code, "Pompe centrifuge", "fr", 10)
            .await
            .expect("search");

        assert!(!results.is_empty(), "should find at least one result");
        assert_eq!(results[0].value_id, val_id);
        assert_eq!(results[0].code, "PUMP_HP");
        // The match should come from the alias
        assert_eq!(results[0].match_source, "alias");
        assert_eq!(results[0].matched_text, "Pompe centrifuge");
    }

    #[tokio::test]
    async fn v1_legacy_alias_works_across_multiple_values() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        let val_a = add_value(&db, set_id, "VALVE_A", "Vanne papillon DN100").await;
        let val_b = add_value(&db, set_id, "VALVE_B", "Vanne a boisseau DN150").await;

        // Legacy alias for val_a
        add_alias(&db, val_a, "Vanne ancienne", "fr", "legacy", true).await;
        // Unrelated alias for val_b
        add_alias(&db, val_b, "Robinet a tournant", "fr", "legacy", true).await;

        // Search "Vanne ancienne" — only val_a should match via alias
        let results = search::search_reference_values(&db, &domain_code, "Vanne ancienne", "fr", 10)
            .await
            .expect("search");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].value_id, val_a);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Locale ranking
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_preferred_alias_ranks_above_non_preferred() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        let val_id = add_value(&db, set_id, "MOTOR_AC", "Moteur asynchrone").await;

        // Preferred alias in fr
        add_alias(&db, val_id, "Moteur electrique", "fr", "search", true).await;
        // Non-preferred alias in fr
        add_alias(&db, val_id, "Moteur electrique triphasé", "fr", "search", false).await;

        // Both contain "Moteur electrique" but preferred should win
        let results = search::search_reference_values(
            &db, &domain_code, "Moteur electrique", "fr", 10,
        )
        .await
        .expect("search");

        assert!(!results.is_empty());
        // The hit should reflect the preferred alias match (higher rank)
        assert_eq!(results[0].value_id, val_id);
        assert!(
            results[0].rank >= 65,
            "preferred alias rank ({}) should be >= 65 (RANK_PREFERRED_ALIAS_PREFIX)",
            results[0].rank
        );
    }

    #[tokio::test]
    async fn v2_locale_alias_ranks_above_foreign_locale() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        let val_fr = add_value(&db, set_id, "BEARING_A", "Roulement a billes").await;
        let val_en = add_value(&db, set_id, "BEARING_B", "Roulement a rouleaux").await;

        // fr alias on val_fr
        add_alias(&db, val_fr, "Palier", "fr", "search", true).await;
        // en alias using same term on val_en (unusual but valid)
        add_alias(&db, val_en, "Palier", "en", "search", true).await;

        // Search "Palier" in locale fr — val_fr should rank higher
        let results = search::search_reference_values(
            &db, &domain_code, "Palier", "fr", 10,
        )
        .await
        .expect("search");

        assert!(results.len() >= 2, "should find both values");
        assert_eq!(results[0].value_id, val_fr, "locale-matched alias should rank first");
        assert!(
            results[0].rank > results[1].rank,
            "locale rank ({}) > fallback rank ({})",
            results[0].rank, results[1].rank
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Canonical precedence
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_exact_code_ranks_first() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        let val_code = add_value(&db, set_id, "PUMP_HP", "Pompe haute pression").await;
        let val_alias = add_value(&db, set_id, "PUMP_LP", "Pompe basse pression").await;

        // Create an alias "PUMP_HP" on val_alias (label matches the CODE of val_code)
        add_alias(&db, val_alias, "PUMP_HP", "fr", "import", true).await;

        // Search "PUMP_HP" — canonical code should beat alias
        let results = search::search_reference_values(
            &db, &domain_code, "PUMP_HP", "fr", 10,
        )
        .await
        .expect("search");

        assert!(results.len() >= 2, "should find both values");
        assert_eq!(results[0].value_id, val_code, "canonical code match must rank first");
        assert_eq!(results[0].match_source, "canonical_code");
        assert_eq!(results[0].rank, 100); // RANK_EXACT_CODE
    }

    #[tokio::test]
    async fn v3_canonical_label_ranks_above_alias() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        let val_label = add_value(&db, set_id, "COMP_A", "Compresseur centrifuge").await;
        let val_alias = add_value(&db, set_id, "COMP_B", "Compresseur a piston").await;

        // Alias matches exact label of val_label
        add_alias(&db, val_alias, "Compresseur centrifuge", "fr", "search", true).await;

        // Search "Compresseur centrifuge" — canonical label should beat alias
        let results = search::search_reference_values(
            &db, &domain_code, "Compresseur centrifuge", "fr", 10,
        )
        .await
        .expect("search");

        assert!(results.len() >= 2);
        assert_eq!(results[0].value_id, val_label, "canonical label match must rank first");
        assert_eq!(results[0].match_source, "canonical_label");
        assert!(results[0].rank > results[1].rank);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Additional coverage
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn empty_query_returns_empty() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;
        add_value(&db, set_id, "ANY_CODE", "Any Label").await;

        let results = search::search_reference_values(&db, &domain_code, "", "fr", 10)
            .await
            .expect("search");
        assert!(results.is_empty());

        let results2 = search::search_reference_values(&db, &domain_code, "   ", "fr", 10)
            .await
            .expect("search");
        assert!(results2.is_empty());
    }

    #[tokio::test]
    async fn limit_is_respected() {
        let db = setup().await;
        let (domain_code, set_id) = setup_search_domain(&db).await;

        for i in 1..=5 {
            add_value(&db, set_id, &format!("VAL_{i:03}"), &format!("Value {i}")).await;
        }

        let results = search::search_reference_values(&db, &domain_code, "VAL", "fr", 3)
            .await
            .expect("search");

        assert_eq!(results.len(), 3, "should respect limit=3");
    }
}
