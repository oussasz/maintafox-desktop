//! Supervisor verification tests for Phase 2 SP02 File 01 Sprint S1.
//!
//! V1 — Asset creation and uniqueness: duplicate `asset_code` rejected
//! V2 — Org linkage guard: inactive org node rejected
//! V3 — Lookup governance guard: unknown criticality_code rejected

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::errors::AppError;
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};

    /// In-memory SQLite with all migrations + seed data.
    async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    /// Publish an org model and create an active root org node.
    /// Returns the org node id for linking assets.
    async fn setup_org_node(db: &sea_orm::DatabaseConnection) -> i64 {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("test model".to_string()),
            },
            1,
        )
        .await
        .expect("create model");

        let root_type = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "SITE".to_string(),
                label: "Site".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(0),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: true,
            },
        )
        .await
        .expect("create root type");

        let child_type = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "WORKSHOP".to_string(),
                label: "Atelier".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create child type");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: root_type.id,
                child_type_id: child_type.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("create rule");

        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish model");

        let root = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Test Site".to_string(),
                node_type_id: root_type.id.into(),
                parent_id: None,
                description: None,
                cost_center_code: None,
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
                structure_model_id: model.id as i64,
            },
            1,
        )
        .await
        .expect("create root node");

        root.id
    }

    /// Insert a test equipment class so class_code can be resolved.
    /// Returns the class id.
    async fn setup_equipment_class(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        let entity_sync_id = uuid::Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO equipment_classes
              (entity_sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
              VALUES (?, 'PUMP', 'Pompes', NULL, 'class', 1, ?, ?)",
            [entity_sync_id.into(), now.clone().into(), now.into()],
        ))
        .await
        .expect("insert equipment class");

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM equipment_classes WHERE code = 'PUMP'".to_string(),
            ))
            .await
            .expect("query class")
            .expect("class row");

        row.try_get::<i64>("", "id").expect("class id")
    }

    /// Standard payload for a valid asset creation.
    fn valid_payload(org_node_id: i64) -> CreateAssetPayload {
        CreateAssetPayload {
            asset_code: "PMP-1001".to_string(),
            asset_name: "Pompe centrifuge P-101".to_string(),
            class_code: "PUMP".to_string(),
            family_code: None,
            subfamily_code: None,
            criticality_code: "STANDARD".to_string(),
            status_code: "ACTIVE_IN_SERVICE".to_string(),
            manufacturer: Some("KSB".to_string()),
            model: Some("Etanorm 50-200".to_string()),
            serial_number: Some("SN-2024-00123".to_string()),
            maintainable_boundary: true,
            rams_schedule_reference_value_id: None,
            rams_utilization_factor: Some(1.0),
            org_node_id,
            commissioned_at: Some("2024-03-15T00:00:00Z".to_string()),
            commercial: None,
        }
    }

    // ── V1 — Asset creation and uniqueness ────────────────────────────────

    #[tokio::test]
    async fn v1_create_asset_then_duplicate_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // First insert succeeds
        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("first create should succeed");

        assert_eq!(asset.asset_code, "PMP-1001");
        assert_eq!(asset.row_version, 1);
        assert!(asset.maintainable_boundary);
        assert_eq!(asset.status_code, "ACTIVE_IN_SERVICE");
        assert!(asset.org_node_id.is_some());

        // Second insert with same code must fail
        let err = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect_err("duplicate code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("PMP-1001"),
                    "error should name the duplicate code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_create_asset_row_version_is_1() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("create should succeed");

        assert_eq!(asset.row_version, 1, "new asset must start at row_version 1");
    }

    // ── V2 — Org linkage guard ────────────────────────────────────────────

    #[tokio::test]
    async fn v2_create_asset_with_nonexistent_org_node_rejected() {
        let db = setup().await;
        // An active structure model must exist, otherwise the guard fails earlier with
        // ORG_NO_ACTIVE_MODEL and the missing-node path is never exercised.
        setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Use a node id that does not exist
        let mut payload = valid_payload(999_999);
        payload.org_node_id = 999_999;

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("nonexistent org node should fail");

        match err {
            AppError::NotFound { entity, id } => {
                assert_eq!(entity, "org_node");
                assert_eq!(id, "999999");
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_create_asset_with_inactive_org_node_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Deactivate the org node directly
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET status = 'inactive' WHERE id = ?",
            [org_node_id.into()],
        ))
        .await
        .expect("deactivate node");

        let payload = valid_payload(org_node_id);

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("inactive org node should fail");

        match err {
            AppError::OrgValidationFailed(issues) => {
                assert!(
                    issues.iter().any(|i| i.code == "ORG_NODE_NOT_ACTIVE"),
                    "expected ORG_NODE_NOT_ACTIVE, got: {issues:?}"
                );
            }
            other => panic!("expected OrgValidationFailed(ORG_NODE_NOT_ACTIVE), got: {other:?}"),
        }
    }

    // ── V3 — Lookup governance guard ──────────────────────────────────────

    #[tokio::test]
    async fn v3_create_asset_with_unknown_criticality_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let mut payload = valid_payload(org_node_id);
        payload.criticality_code = "DOES_NOT_EXIST".to_string();

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("unknown criticality code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("DOES_NOT_EXIST") || joined.contains("non reconnu"),
                    "error should name the bad code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_create_asset_with_unknown_status_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let mut payload = valid_payload(org_node_id);
        payload.status_code = "FANTASY_STATUS".to_string();

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("unknown status code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("FANTASY_STATUS"),
                    "error should name the bad code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_create_asset_with_unknown_class_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        // NOTE: we do NOT call setup_equipment_class here

        let payload = valid_payload(org_node_id);

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("unknown class code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("PUMP"),
                    "error should name the bad class code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn retiring_through_the_identity_form_is_refused() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        // Tags the published statuses (retired: decommissioned, scrapped).
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");
        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("create");
        let payload = |status: Option<&str>, decom: Option<Option<&str>>| identity::UpdateAssetIdentityPayload {
            asset_name: None,
            class_code: None,
            family_code: None,
            subfamily_code: None,
            criticality_code: None,
            status_code: status.map(str::to_string),
            manufacturer: None,
            model: None,
            serial_number: None,
            maintainable_boundary: None,
            rams_schedule_reference_value_id: None,
            rams_utilization_factor: None,
            commissioned_at: None,
            decommissioned_at: decom.map(|d| d.map(str::to_string)),
            org_node_id: None,
        };
        let refused = |r: Result<_, AppError>| matches!(r, Err(AppError::ValidationFailed(m)) if m[0] == identity::RETIRE_THROUGH_DECOMMISSION);
        for status in ["DECOMMISSIONED", "SCRAPPED"] {
            let r =
                identity::update_asset_identity(&db, asset.id, payload(Some(status), None), asset.row_version, 1).await;
            assert!(refused(r), "{status} is set by Déclasser");
        }
        let r = identity::update_asset_identity(
            &db,
            asset.id,
            payload(None, Some(Some("2026-10-01T00:00:00Z"))),
            asset.row_version,
            1,
        )
        .await;
        assert!(refused(r), "the end-of-life date is set by Déclasser");
        // Another in-service status, and the unchanged (empty) end-of-life date, are accepted.
        let updated = identity::update_asset_identity(
            &db,
            asset.id,
            payload(Some("UNDER_MAINTENANCE"), Some(None)),
            asset.row_version,
            1,
        )
        .await
        .expect("in-service status change");
        assert_eq!(updated.status_code, "UNDER_MAINTENANCE");

        // Never created retired.
        let mut retired = valid_payload(org_node_id);
        retired.asset_code = "PMP-1002".into();
        retired.status_code = "SCRAPPED".into();
        assert!(refused(identity::create_asset(&db, retired, 1).await));

        // Leaving a retired status is a recommission event, not an identity edit.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED', decommissioned_at = '2026-10-01T00:00:00Z' \
             WHERE id = ?",
            [asset.id.into()],
        ))
        .await
        .expect("retire for the test");
        let back = identity::update_asset_identity(
            &db,
            asset.id,
            payload(Some("ACTIVE_IN_SERVICE"), None),
            updated.row_version,
            1,
        )
        .await;
        assert!(
            matches!(back, Err(AppError::ValidationFailed(ref m)) if m[0] == identity::RECOMMISSION_REQUIRED),
            "{back:?}"
        );
        let retired = identity::get_asset_by_id(&db, asset.id).await.expect("fetch");
        // Clearing the end-of-life date is a return to service: refused.
        let cleared =
            identity::update_asset_identity(&db, asset.id, payload(None, Some(None)), retired.row_version, 1).await;
        assert!(
            matches!(cleared, Err(AppError::ValidationFailed(ref m)) if m[0] == identity::RECOMMISSION_REQUIRED),
            "{cleared:?}"
        );
        // The same day in another format is no change; an edit of the name keeps the date as it is.
        let mut same_day = payload(Some("DECOMMISSIONED"), Some(Some("2026-10-01")));
        same_day.asset_name = Some("Pompe déclassée".into());
        let kept = identity::update_asset_identity(&db, asset.id, same_day, retired.row_version, 1)
            .await
            .expect("same day, other format");
        assert_eq!(kept.decommissioned_at.as_deref(), Some("2026-10-01T00:00:00Z"));
        // Another day: refused (the end-of-life date belongs to Déclasser).
        let moved = identity::update_asset_identity(
            &db,
            asset.id,
            payload(None, Some(Some("2026-11-01"))),
            kept.row_version,
            1,
        )
        .await;
        assert!(
            matches!(moved, Err(AppError::ValidationFailed(ref m)) if m[0] == identity::RETIRE_THROUGH_DECOMMISSION),
            "{moved:?}"
        );
    }

    #[tokio::test]
    async fn v3_valid_lookup_codes_succeed() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Canonical codes: C criticality (legacy STANDARD), ACTIVE_IN_SERVICE status, PUMP class
        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("valid lookup codes should succeed");

        assert!(asset.criticality_code.as_deref() == Some("C"));
        assert_eq!(asset.status_code, "ACTIVE_IN_SERVICE");
        assert!(asset.class_code.as_deref() == Some("PUMP"));
    }
}
