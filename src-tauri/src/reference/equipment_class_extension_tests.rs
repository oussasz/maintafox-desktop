//! A plant extends the equipment class catalogue (`EQUIPMENT.CLASS`) through draft → publish,
//! the product-seeded classes stay protected, and records keep their class after the publish.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::taxonomy_reference;
    use crate::errors::AppError;
    use crate::reference::domains;
    use crate::reference::governance::{self, GovernanceCategory};
    use crate::reference::publish;
    use crate::reference::sets;
    use crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity;
    use crate::reference::values::{self, CreateReferenceValuePayload, UpdateReferenceValuePayload};

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = crate::db::test_db::seeded().await;
        ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("initial integrity");
        db
    }

    async fn published_set_id(db: &sea_orm::DatabaseConnection, domain_code: &str) -> i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = ? AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
                [domain_code.into()],
            ))
            .await
            .expect("query")
            .expect("published set");
        row.try_get::<i64>("", "id").expect("id")
    }

    async fn value_id(db: &sea_orm::DatabaseConnection, set_id: i64, code: &str) -> i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM reference_values WHERE set_id = ? AND code = ?",
                [set_id.into(), code.into()],
            ))
            .await
            .expect("query")
            .expect("value row");
        row.try_get::<i64>("", "id").expect("id")
    }

    fn new_value(set_id: i64, code: &str, label: &str, parent_id: Option<i64>) -> CreateReferenceValuePayload {
        CreateReferenceValuePayload {
            set_id,
            parent_id,
            code: code.to_string(),
            label: label.to_string(),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: Some("equipment_class".to_string()),
            external_code: None,
            metadata_json: None,
        }
    }

    #[tokio::test]
    async fn plant_adds_equipment_classes_through_draft_and_publish() {
        let db = setup().await;

        // The class catalogue is a controlled catalogue whose seeded classes are locked.
        let class_domain = domains::get_reference_domain_by_code(&db, "EQUIPMENT.CLASS")
            .await
            .expect("class domain");
        assert_eq!(
            governance::category_of(&class_domain),
            GovernanceCategory::ControlledCatalog
        );
        assert_eq!(class_domain.governance_category, "controlled_catalog");
        let v1 = published_set_id(&db, "EQUIPMENT.CLASS").await;
        let v1_set = sets::get_reference_set(&db, v1).await.expect("v1");
        let caps = governance::capabilities_for(&class_domain, Some(&v1_set));
        assert!(caps.can_create_draft_set);
        assert!(caps.can_publish);
        assert!(caps.system_values_locked);
        assert!(!caps.can_create_value, "the published version itself stays read-only");

        // Records that use the v1 classes: an asset, and a tenant family under PUMP.
        let pump_v1 = value_id(&db, v1, "PUMP").await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, \
             equipment_class_ref_id, created_at, updated_at) \
             VALUES ('eq-class-ext-1', 'EQ-CLS-001', 'Pompe de reprise', 'active_in_service', ?, \
             datetime('now'), datetime('now'))",
            [pump_v1.into()],
        ))
        .await
        .expect("insert equipment");
        let family_set = published_set_id(&db, "EQUIPMENT.FAMILY").await;
        let tenant_family = values::create_value(
            &db,
            new_value(family_set, "PUMP_DOSING", "Pompes doseuses", Some(pump_v1)),
            1,
        )
        .await
        .expect("tenant family under PUMP");

        // Draft v2: add the plant's own classes.
        let draft = sets::create_draft_set(&db, class_domain.id, 1)
            .await
            .expect("create draft");
        for (code, label) in [
            ("PRESS", "Presse"),
            ("BOILER", "Chaudière"),
            ("TRANSFORMER", "Transformateur"),
            ("PACKAGING_LINE", "Ligne de conditionnement"),
        ] {
            values::create_value(&db, new_value(draft.id, code, label, None), 1)
                .await
                .unwrap_or_else(|e| panic!("add {code}: {e:?}"));
        }

        // Seeded classes cannot be relabelled or deactivated, even in the draft.
        let pump_draft = value_id(&db, draft.id, "PUMP").await;
        let relabel = values::update_value(
            &db,
            pump_draft,
            UpdateReferenceValuePayload {
                label: Some("Pompe locale".into()),
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
        .await;
        assert!(matches!(relabel, Err(AppError::ValidationFailed(_))));
        assert!(matches!(
            values::deactivate_value(&db, pump_draft, 1).await,
            Err(AppError::ValidationFailed(_))
        ));
        // A tenant cannot pass its own value off as a product value.
        let mut forged = new_value(draft.id, "FORGED", "Forged", None);
        forged.metadata_json = Some(r#"{"origin":"system"}"#.into());
        assert!(matches!(
            values::create_value(&db, forged, 1).await,
            Err(AppError::ValidationFailed(_))
        ));
        // The plant's own classes stay editable in the draft.
        let press = value_id(&db, draft.id, "PRESS").await;
        values::update_value(
            &db,
            press,
            UpdateReferenceValuePayload {
                label: Some("Presse hydraulique".into()),
                description: Some(Some("Presses de formage".into())),
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
        .expect("edit tenant class");

        // Validate → impact preview → governed publish.
        sets::validate_set(&db, draft.id, 1).await.expect("validate draft");
        publish::preview_publish_impact(&db, draft.id)
            .await
            .expect("impact preview");
        let result = publish::publish_reference_set(&db, draft.id, 1)
            .await
            .expect("publish v2");
        assert_eq!(result.superseded_set_id, Some(v1));
        let v2 = published_set_id(&db, "EQUIPMENT.CLASS").await;
        assert_eq!(v2, draft.id);

        // The asset and the tenant family follow PUMP into v2.
        let pump_v2 = value_id(&db, v2, "PUMP").await;
        assert_ne!(pump_v1, pump_v2);
        let eq_class: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT equipment_class_ref_id AS id FROM equipment WHERE asset_id_code = 'EQ-CLS-001'".to_string(),
            ))
            .await
            .expect("query")
            .expect("equipment row")
            .try_get("", "id")
            .expect("id");
        assert_eq!(eq_class, pump_v2, "asset class re-pointed to the published version");
        let family_after = values::get_value(&db, tenant_family.id).await.expect("family");
        assert_eq!(family_after.parent_id, Some(pump_v2), "family parent follows its class");

        // Startup integrity runs again against v2 without error and keeps the plant's classes.
        ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("integrity after publish");
        let catalog = taxonomy_reference::list_equipment_taxonomy_catalog(&db)
            .await
            .expect("taxonomy catalog");
        let class_codes: Vec<&str> = catalog.classes.iter().map(|c| c.code.as_str()).collect();
        for code in ["PUMP", "PRESS", "BOILER", "TRANSFORMER", "PACKAGING_LINE"] {
            assert!(class_codes.contains(&code), "{code} listed for asset forms");
        }
        assert!(catalog.classes.iter().any(|c| c.code == "PUMP" && c.is_system));
        assert!(catalog.classes.iter().any(|c| c.code == "PRESS" && !c.is_system));
        let synced: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM equipment_classes WHERE code = 'PRESS' AND is_active = 1".to_string(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "n")
            .expect("n");
        assert_eq!(synced, 1, "new class usable by asset create (equipment_classes)");
        let system_family = values::get_value(&db, value_id(&db, family_set, "PUMP_FAMILY").await)
            .await
            .expect("system family");
        assert_eq!(system_family.parent_id, Some(pump_v2));
    }

    #[tokio::test]
    async fn carried_over_inactive_values_do_not_block_publish() {
        let db = setup().await;
        let class_domain = domains::get_reference_domain_by_code(&db, "EQUIPMENT.CLASS")
            .await
            .expect("class domain");

        // v2 adds then retires a tenant class (with a migration map to a seeded class).
        let draft = sets::create_draft_set(&db, class_domain.id, 1).await.expect("draft v2");
        let old = values::create_value(&db, new_value(draft.id, "OLD_SKID", "Skid", None), 1)
            .await
            .expect("tenant class");
        values::deactivate_value(&db, old.id, 1).await.expect("deactivate");
        let pump = value_id(&db, draft.id, "PUMP").await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_value_migrations \
             (domain_id, from_value_id, to_value_id, reason_code, migrated_by_id, migrated_at) \
             VALUES (?, ?, ?, 'retired', 1, datetime('now'))",
            [class_domain.id.into(), old.id.into(), pump.into()],
        ))
        .await
        .expect("migration map");
        sets::validate_set(&db, draft.id, 1).await.expect("validate v2");
        publish::publish_reference_set(&db, draft.id, 1)
            .await
            .expect("publish v2");

        // v3 copies the inactive class; it is not a new deactivation.
        let draft3 = sets::create_draft_set(&db, class_domain.id, 1).await.expect("draft v3");
        values::create_value(&db, new_value(draft3.id, "CRANE", "Pont roulant", None), 1)
            .await
            .expect("add crane");
        sets::validate_set(&db, draft3.id, 1).await.expect("validate v3");
        let readiness = publish::compute_publish_readiness(&db, draft3.id)
            .await
            .expect("readiness");
        assert!(
            !readiness.issues.iter().any(|i| i.check == "unresolved_migrations"),
            "carried-over inactive value must not block: {:?}",
            readiness.issues
        );
        publish::publish_reference_set(&db, draft3.id, 1)
            .await
            .expect("publish v3");
    }
}
