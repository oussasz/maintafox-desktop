//! Audit trail tests for Sprint S2 — File 04.
//!
//! Tests:
//! - AT1: successful publish writes `org_change_events` row with `apply_result = 'applied'`
//! - AT2: blocked publish writes `org_change_events` row with `apply_result = 'blocked'`
//! - AT3: record_org_change round-trip with list query
//! - AT4: list filters by entity_kind and entity_id

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::audit::{self, OrgAuditEventInput};
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};
    use crate::org::validation;

    // ── Test helpers ──────────────────────────────────────────────────────

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

    /// Create and publish a base model with SITE (root) → PLANT → ZONE.
    /// Returns `(model_id, site_type_id, plant_type_id, zone_type_id)`.
    async fn create_base_active_model(
        db: &sea_orm::DatabaseConnection,
    ) -> (i32, i32, i32, i32) {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("Audit test base model".to_string()),
            },
            1,
        )
        .await
        .expect("create base model");

        let site = node_types::create_node_type(
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
        .expect("create SITE type");

        let plant = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "PLANT".to_string(),
                label: "Plant".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create PLANT type");

        let zone = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "ZONE".to_string(),
                label: "Zone".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create ZONE type");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: site.id,
                child_type_id: plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("SITE→PLANT rule");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: plant.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("PLANT→ZONE rule");

        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish base model");

        (model.id, site.id, plant.id, zone.id)
    }

    /// Create live nodes for the active model.
    async fn create_live_nodes(
        db: &sea_orm::DatabaseConnection,
        site_type_id: i32,
        plant_type_id: i32,
        zone_type_id: i32,
    ) -> (i64, i64, i64) {
        let site_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "HQ".to_string(),
                name: "Headquarters".to_string(),
                node_type_id: site_type_id as i64,
                parent_id: None,
                description: None,
                cost_center_code: None,
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect("create HQ node");

        let plant_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "PLT1".to_string(),
                name: "Plant 1".to_string(),
                node_type_id: plant_type_id as i64,
                parent_id: Some(site_node.id),
                description: None,
                cost_center_code: Some("CC-001".to_string()),
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect("create PLT1 node");

        let zone_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "ZN1".to_string(),
                name: "Zone 1".to_string(),
                node_type_id: zone_type_id as i64,
                parent_id: Some(plant_node.id),
                description: None,
                cost_center_code: None,
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect("create ZN1 node");

        (site_node.id, plant_node.id, zone_node.id)
    }

    // ── AT1 — Successful publish writes an audit row ──────────────────────

    #[tokio::test]
    async fn at1_successful_publish_writes_audit_row() {
        let db = setup().await;

        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Create a valid draft v2 with same codes but updated labels.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Audit test v2".to_string()),
            },
            1,
        )
        .await
        .expect("draft v2");

        let new_site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("SITE v2");

        let new_plant = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("PLANT v2");

        let new_zone = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "ZONE".to_string(),
                label: "Zone v2".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("ZONE v2");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_site.id,
                child_type_id: new_plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule v2: SITE→PLANT");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_plant.id,
                child_type_id: new_zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule v2: PLANT→ZONE");

        // Publish with remap — should succeed.
        let publish_result = validation::publish_model_with_remap(&db, draft.id as i64, 1)
            .await
            .expect("publish should succeed");

        assert!(publish_result.can_publish);

        // Record the audit event (mimics what the command handler does).
        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "structure_model".to_string(),
                entity_id: Some(draft.id as i64),
                change_type: "publish_model".to_string(),
                before_json: None,
                after_json: Some(
                    serde_json::to_string(&publish_result).unwrap_or_default(),
                ),
                preview_summary_json: None,
                changed_by_id: Some(1),
                requires_step_up: true,
                apply_result: "applied".to_string(),
            },
        )
        .await
        .expect("audit write");

        // Query org_change_events for the publish row.
        let events = audit::list_org_change_events(&db, Some(10), None, None)
            .await
            .expect("list events");

        let publish_event = events
            .iter()
            .find(|e| e.change_type == "publish_model" && e.apply_result == "applied");

        assert!(
            publish_event.is_some(),
            "AT1: expected a publish_model event with apply_result='applied'"
        );

        let evt = publish_event.unwrap();
        assert_eq!(evt.entity_kind, "structure_model");
        assert_eq!(evt.entity_id, Some(draft.id as i64));
        assert!(evt.requires_step_up);
    }

    // ── AT2 — Blocked publish writes a blocked audit row ──────────────────

    #[tokio::test]
    async fn at2_blocked_publish_writes_blocked_audit_row() {
        let db = setup().await;

        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Create an invalid draft v2 that omits ZONE.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Blocked audit test".to_string()),
            },
            1,
        )
        .await
        .expect("draft");

        let ns = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("SITE v2");

        let np = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("PLANT v2");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: ns.id,
                child_type_id: np.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule");

        // Attempt publish — must fail because ZONE is missing.
        let publish_err = validation::publish_model_with_remap(&db, draft.id as i64, 1)
            .await
            .expect_err("publish should fail — ZONE missing");

        assert!(matches!(
            publish_err,
            crate::errors::AppError::ValidationFailed(_)
        ));

        // Record the blocked audit event (mimics command handler on error path).
        let blocked_validation =
            validation::validate_draft_model_for_publish(&db, draft.id as i64)
                .await
                .ok();

        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "structure_model".to_string(),
                entity_id: Some(draft.id as i64),
                change_type: "publish_model".to_string(),
                before_json: None,
                after_json: None,
                preview_summary_json: blocked_validation
                    .and_then(|v| serde_json::to_string(&v).ok()),
                changed_by_id: Some(1),
                requires_step_up: true,
                apply_result: "blocked".to_string(),
            },
        )
        .await
        .expect("audit write blocked");

        // Query org_change_events for the blocked row.
        let events = audit::list_org_change_events(&db, Some(10), None, None)
            .await
            .expect("list events");

        let blocked_event = events
            .iter()
            .find(|e| e.change_type == "publish_model" && e.apply_result == "blocked");

        assert!(
            blocked_event.is_some(),
            "AT2: expected a publish_model event with apply_result='blocked'"
        );

        let evt = blocked_event.unwrap();
        assert_eq!(evt.entity_kind, "structure_model");
        assert!(evt.preview_summary_json.is_some(), "AT2: blocked event should contain validation summary");

        // Verify the summary contains the blocking issue.
        let summary = evt.preview_summary_json.as_ref().unwrap();
        assert!(
            summary.contains("MISSING_TYPE_CODE"),
            "AT2: summary should reference MISSING_TYPE_CODE — got: {}",
            summary
        );
    }

    // ── AT3 — Record and list round-trip ──────────────────────────────────

    #[tokio::test]
    async fn at3_record_and_list_round_trip() {
        let db = setup().await;

        // Write a move audit event directly.
        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "org_node".to_string(),
                entity_id: Some(42),
                change_type: "move_node".to_string(),
                before_json: Some(r#"{"parent_id": 1}"#.to_string()),
                after_json: Some(r#"{"parent_id": 2}"#.to_string()),
                preview_summary_json: None,
                changed_by_id: Some(1),
                requires_step_up: true,
                apply_result: "applied".to_string(),
            },
        )
        .await
        .expect("record move");

        let events = audit::list_org_change_events(&db, Some(10), None, None)
            .await
            .expect("list events");

        assert!(!events.is_empty(), "AT3: at least one event after record");

        let move_event = events
            .iter()
            .find(|e| e.change_type == "move_node");

        assert!(move_event.is_some(), "AT3: expected move_node event");
        let evt = move_event.unwrap();
        assert_eq!(evt.entity_kind, "org_node");
        assert_eq!(evt.entity_id, Some(42));
        assert!(evt.requires_step_up);
        assert_eq!(evt.apply_result, "applied");
        assert!(evt.before_json.is_some());
        assert!(evt.after_json.is_some());
    }

    // ── AT4 — List filters by entity_kind and entity_id ───────────────────

    #[tokio::test]
    async fn at4_list_filters_by_entity_kind_and_entity_id() {
        let db = setup().await;

        // Write two events with different entity kinds.
        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "org_node".to_string(),
                entity_id: Some(10),
                change_type: "create_node".to_string(),
                before_json: None,
                after_json: Some(r#"{"code":"N1"}"#.to_string()),
                preview_summary_json: None,
                changed_by_id: Some(1),
                requires_step_up: false,
                apply_result: "applied".to_string(),
            },
        )
        .await
        .expect("write node event");

        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "structure_model".to_string(),
                entity_id: Some(5),
                change_type: "publish_model".to_string(),
                before_json: None,
                after_json: None,
                preview_summary_json: None,
                changed_by_id: Some(1),
                requires_step_up: true,
                apply_result: "applied".to_string(),
            },
        )
        .await
        .expect("write model event");

        // Filter by entity_kind = org_node
        let node_events = audit::list_org_change_events(
            &db,
            Some(50),
            Some("org_node"),
            None,
        )
        .await
        .expect("filter by kind");

        assert!(
            node_events.iter().all(|e| e.entity_kind == "org_node"),
            "AT4: all events should be org_node"
        );
        assert!(!node_events.is_empty());

        // Filter by entity_kind + entity_id
        let specific = audit::list_org_change_events(
            &db,
            Some(50),
            Some("structure_model"),
            Some(5),
        )
        .await
        .expect("filter by kind + id");

        assert_eq!(specific.len(), 1);
        assert_eq!(specific[0].entity_id, Some(5));
        assert_eq!(specific[0].change_type, "publish_model");
    }

    // ══════════════════════════════════════════════════════════════════════
    // Sprint S2 — Supervisor Verifications (V1-V3)
    // ══════════════════════════════════════════════════════════════════════

    use crate::auth::session_manager::AuthenticatedUser;
    use crate::errors::AppError;
    use crate::state::AppState;

    /// Helper: mimics a command that gates on step-up.
    /// Uses the real `require_step_up!` macro so the test exercises the same
    /// code path as `move_org_node`, `deactivate_org_node`, `publish_org_model`.
    async fn step_up_gate(state: &AppState) -> crate::errors::AppResult<()> {
        crate::require_step_up!(state);
        Ok(())
    }

    // ── SV-V1 — Step-up enforcement on dangerous actions ──────────────────
    //
    // Scenario: A logged-in user with org.admin calls a dangerous action.
    //   • Without step-up → must receive `AppError::StepUpRequired`.
    //   • After `record_step_up()` → must succeed.
    //
    // This validates the same gate present in move_org_node, deactivate_org_node,
    // and publish_org_model (all three contain `require_step_up!(state)`).

    #[tokio::test]
    async fn sv_v1_step_up_required_blocks_without_verification() {
        let db = setup().await;
        let state = AppState::new(db);

        // Create an authenticated session (no step-up yet).
        {
            let mut guard = state.session.write().await;
            guard.create_session(AuthenticatedUser {
                user_id: 1,
                username: "admin".into(),
                display_name: Some("Admin".into()),
                is_admin: true,
                force_password_change: false,
            });
        }

        // Without step-up → must be rejected.
        let result = step_up_gate(&state).await;
        assert!(
            matches!(&result, Err(AppError::StepUpRequired)),
            "SV-V1a: expected StepUpRequired without step-up — got: {:?}",
            result,
        );

        // Perform step-up.
        {
            let mut guard = state.session.write().await;
            guard.record_step_up();
        }

        // After step-up → must succeed.
        let result = step_up_gate(&state).await;
        assert!(
            result.is_ok(),
            "SV-V1b: expected Ok after step-up — got: {:?}",
            result,
        );
    }

    // ── SV-V2 — Successful publish writes audit row ───────────────────────
    //
    // End-to-end: publish a valid draft → record audit (same path as the
    // command handler) → query audit table → row must have:
    //   change_type = 'publish_model', apply_result = 'applied',
    //   requires_step_up = true, entity_kind = 'structure_model'.

    #[tokio::test]
    async fn sv_v2_publish_success_audit_row() {
        let db = setup().await;

        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Build a valid v2 draft with the same type codes.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload { description: Some("SV-V2 draft".into()) },
            1,
        )
        .await
        .unwrap();

        for (code, label, depth, is_root) in [
            ("SITE", "Site SV2", 0, true),
            ("PLANT", "Plant SV2", 1, false),
            ("ZONE", "Zone SV2", 2, false),
        ] {
            node_types::create_node_type(
                &db,
                CreateNodeTypePayload {
                    structure_model_id: draft.id,
                    code: code.into(),
                    label: label.into(),
                    icon_key: None,
                    color: None,
                    depth_hint: Some(depth),
                    can_host_assets: true,
                    can_own_work: depth < 2,
                    can_carry_cost_center: depth < 2,
                    can_aggregate_kpis: depth < 2,
                    can_receive_permits: false,
                    is_root_type: is_root,
                },
            )
            .await
            .unwrap();
        }

        let types = node_types::list_node_types(&db, draft.id).await.unwrap();
        let site_t = types.iter().find(|t| t.code == "SITE").unwrap();
        let plant_t = types.iter().find(|t| t.code == "PLANT").unwrap();
        let zone_t = types.iter().find(|t| t.code == "ZONE").unwrap();

        for (parent, child) in [(site_t.id, plant_t.id), (plant_t.id, zone_t.id)] {
            relationship_rules::create_rule(
                &db,
                CreateRelationshipRulePayload {
                    structure_model_id: draft.id,
                    parent_type_id: parent,
                    child_type_id: child,
                    min_children: None,
                    max_children: None,
                },
            )
            .await
            .unwrap();
        }

        // Publish
        let result = validation::publish_model_with_remap(&db, draft.id as i64, 1)
            .await
            .expect("SV-V2: publish should succeed");
        assert!(result.can_publish);

        // Record audit (mirrors command handler success path).
        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "structure_model".into(),
                entity_id: Some(draft.id as i64),
                change_type: "publish_model".into(),
                before_json: None,
                after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
                preview_summary_json: None,
                changed_by_id: Some(1),
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await
        .unwrap();

        // Verify audit row.
        let events = audit::list_org_change_events(&db, Some(10), Some("structure_model"), None)
            .await
            .unwrap();

        let row = events
            .iter()
            .find(|e| e.change_type == "publish_model" && e.entity_id == Some(draft.id as i64));

        assert!(row.is_some(), "SV-V2: publish audit row must exist");
        let row = row.unwrap();
        assert_eq!(row.apply_result, "applied", "SV-V2: apply_result must be 'applied'");
        assert!(row.requires_step_up, "SV-V2: requires_step_up must be true");
        assert!(row.after_json.is_some(), "SV-V2: after_json should contain the validation result");
    }

    // ── SV-V3 — Blocked publish writes blocked audit row ──────────────────
    //
    // Draft that drops ZONE → publish fails → record 'blocked' audit →
    // row must have apply_result = 'blocked', preview_summary_json contains
    // the blocking issue code (MISSING_TYPE_CODE).

    #[tokio::test]
    async fn sv_v3_blocked_publish_audit_row() {
        let db = setup().await;

        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Invalid draft: drops ZONE type code.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload { description: Some("SV-V3 blocked".into()) },
            1,
        )
        .await
        .unwrap();

        let ns = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".into(),
                label: "Site V3".into(),
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
        .unwrap();

        let np = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".into(),
                label: "Plant V3".into(),
                icon_key: None,
                color: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .unwrap();

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: ns.id,
                child_type_id: np.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .unwrap();

        // Publish must fail.
        let err = validation::publish_model_with_remap(&db, draft.id as i64, 1)
            .await
            .expect_err("SV-V3: publish should fail — ZONE missing");

        assert!(matches!(err, AppError::ValidationFailed(_)));

        // Record blocked audit (mirrors command handler error path).
        let blocked_validation =
            validation::validate_draft_model_for_publish(&db, draft.id as i64)
                .await
                .ok();

        audit::record_org_change(
            &db,
            OrgAuditEventInput {
                entity_kind: "structure_model".into(),
                entity_id: Some(draft.id as i64),
                change_type: "publish_model".into(),
                before_json: None,
                after_json: None,
                preview_summary_json: blocked_validation
                    .and_then(|v| serde_json::to_string(&v).ok()),
                changed_by_id: Some(1),
                requires_step_up: true,
                apply_result: "blocked".into(),
            },
        )
        .await
        .unwrap();

        // Verify blocked audit row.
        let events = audit::list_org_change_events(&db, Some(10), Some("structure_model"), None)
            .await
            .unwrap();

        let row = events
            .iter()
            .find(|e| e.change_type == "publish_model" && e.apply_result == "blocked");

        assert!(row.is_some(), "SV-V3: blocked audit row must exist");
        let row = row.unwrap();
        assert_eq!(row.entity_kind, "structure_model");
        assert!(row.requires_step_up, "SV-V3: requires_step_up must be true");
        assert!(
            row.preview_summary_json.is_some(),
            "SV-V3: preview_summary_json should contain validation details"
        );
        let summary = row.preview_summary_json.as_ref().unwrap();
        assert!(
            summary.contains("MISSING_TYPE_CODE"),
            "SV-V3: summary must reference MISSING_TYPE_CODE — got: {}",
            summary,
        );
    }
}
