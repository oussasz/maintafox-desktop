//! "Transférer" (L2-04): an asset moves to another org node with reason, effective date and
//! author; refusals; the counts shown before confirming.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::history::{self, AssetHistoryQuery};
    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::transfer::{self, AssetTransferImpact, TransferAssetPayload};
    use crate::auth::scope::AccessScope;
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

    /// Create a second active org node under the same model as the first.
    async fn setup_second_org_node(db: &sea_orm::DatabaseConnection) -> i64 {
        // Re-use the existing child type for the second node.
        // The root already exists from setup_org_node — create a sibling root.
        let now = chrono::Utc::now().to_rfc3339();
        let entity_sync_id = uuid::Uuid::new_v4().to_string();
        let node_type_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_node_types WHERE code = 'SITE' LIMIT 1".to_string(),
            ))
            .await
            .expect("query node type")
            .expect("SITE type row")
            .try_get("", "id")
            .expect("node type id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_nodes \
             (entity_sync_id, code, name, node_type_id, parent_id, status, \
              created_at, updated_at, row_version, structure_model_id) \
             VALUES (?, 'SITE-002', 'Second Site', ?, NULL, 'active', ?, ?, 1, \
                     (SELECT id FROM org_structure_models WHERE status = 'active' ORDER BY id DESC LIMIT 1))",
            [
                entity_sync_id.into(),
                node_type_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert second org node");

        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE code = 'SITE-002'".to_string(),
        ))
        .await
        .expect("query node")
        .expect("node row")
        .try_get::<i64>("", "id")
        .expect("node id")
    }

    /// Insert a test equipment class.
    async fn setup_equipment_class(db: &sea_orm::DatabaseConnection) {
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
    }

    /// Create an asset with a unique code and return its full record.
    async fn create_test_asset(db: &sea_orm::DatabaseConnection, code: &str, org_node_id: i64) -> identity::Asset {
        identity::create_asset(
            db,
            CreateAssetPayload {
                asset_code: code.to_string(),
                asset_name: format!("Test asset {code}"),
                class_code: "PUMP".to_string(),
                family_code: None,
                subfamily_code: None,
                criticality_code: "STANDARD".to_string(),
                status_code: "ACTIVE_IN_SERVICE".to_string(),
                manufacturer: None,
                model: None,
                serial_number: None,
                maintainable_boundary: true,
                rams_schedule_reference_value_id: None,
                rams_utilization_factor: Some(1.0),
                org_node_id,
                commissioned_at: None,
                commercial: None,
            },
            1,
        )
        .await
        .expect(&format!("create asset {code}"))
    }

    fn today() -> String {
        crate::clock::now_local().date_naive().format("%Y-%m-%d").to_string()
    }

    fn payload(asset: &identity::Asset, to: i64, date: Option<String>, reason: &str) -> TransferAssetPayload {
        TransferAssetPayload {
            asset_id: asset.id,
            to_org_node_id: to,
            effective_date: date,
            reason: reason.to_string(),
            expected_row_version: asset.row_version,
        }
    }

    fn only(nodes: &[i64]) -> AccessScope {
        AccessScope::Subtrees(nodes.iter().copied().collect::<BTreeSet<i64>>())
    }

    /// The refusal code of an error (`GATE_…` message prefix or org-validation code).
    fn code_of(err: AppError) -> String {
        match err {
            AppError::ValidationFailed(msgs) => msgs
                .first()
                .map(|m| m.split([':', ' ']).next().unwrap_or_default().to_string())
                .unwrap_or_default(),
            AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
            other => format!("{other:?}"),
        }
    }

    async fn fixture() -> (sea_orm::DatabaseConnection, i64, i64, identity::Asset) {
        let db = setup().await;
        let from = setup_org_node(&db).await;
        let to = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "TRF-1", from).await;
        (db, from, to, asset)
    }

    async fn exec(db: &sea_orm::DatabaseConnection, sql: String) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .expect("fixture");
    }

    #[tokio::test]
    async fn transfer_records_reason_date_from_to_and_author() {
        let (db, from, to, asset) = fixture().await;

        let result = transfer::transfer_asset(
            &db,
            payload(&asset, to, Some(today()), "  Réaffectation à la ligne 2  "),
            &only(&[from, to]),
            1,
        )
        .await
        .expect("transfer");

        assert_eq!(result.asset.org_node_id, Some(to));
        assert_eq!(result.asset.row_version, asset.row_version + 1);
        assert_eq!(result.event.event_type, "MOVED");
        assert_eq!(result.event.from_org_node_id, Some(from));
        assert_eq!(result.event.to_org_node_id, Some(to));
        assert_eq!(result.event.notes.as_deref(), Some("Réaffectation à la ligne 2"));
        assert_eq!(result.event.created_by_id, Some(1));
        assert!(!result.event.event_at.is_empty());
        assert_eq!(result.impact, AssetTransferImpact::default());

        let audits: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = ? AND target_id = ?",
                [
                    crate::assets::lifecycle::ASSET_MOVED_EVENT.into(),
                    asset.id.to_string().into(),
                ],
            ))
            .await
            .expect("audit count")
            .expect("row")
            .try_get("", "n")
            .expect("n");
        assert_eq!(audits, 1, "the transfer is audited with the move");

        let events = history::list_asset_history_events(
            &db,
            AssetHistoryQuery {
                asset_id: asset.id,
                event_types: None,
                period: None,
                search: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .expect("history");
        let moved = events
            .iter()
            .find(|e| e.event_type == "org_changed")
            .expect("transfer in the history");
        assert_eq!(moved.description.as_deref(), Some("Réaffectation à la ligne 2"));
        let meta = |key: &str| moved.metadata.get(key).and_then(|v| v.as_str()).map(str::to_string);
        assert_eq!(meta("from_node_code").as_deref(), Some("SITE-001"));
        assert_eq!(meta("to_node_code").as_deref(), Some("SITE-002"));
        assert_eq!(meta("to_node_name").as_deref(), Some("Second Site"));
        let author: Option<String> = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COALESCE(display_name, username) AS label FROM user_accounts WHERE id = 1".to_string(),
            ))
            .await
            .expect("author")
            .map(|r| r.try_get::<String>("", "label").expect("label"));
        assert_eq!(moved.actor_label, author, "the author is shown");
    }

    #[tokio::test]
    async fn transfer_to_the_same_node_is_refused() {
        let (db, from, _to, asset) = fixture().await;
        let err = transfer::transfer_asset(&db, payload(&asset, from, None, "motif"), &AccessScope::Everywhere, 1)
            .await
            .expect_err("same node");
        assert_eq!(code_of(err), "GATE_ASSET_TRANSFER_SAME_NODE");
    }

    #[tokio::test]
    async fn transfer_without_reason_is_refused() {
        let (db, _from, to, asset) = fixture().await;
        let err = transfer::transfer_asset(&db, payload(&asset, to, None, "   "), &AccessScope::Everywhere, 1)
            .await
            .expect_err("no reason");
        assert_eq!(code_of(err), "GATE_ASSET_TRANSFER_REASON_REQUIRED");
    }

    #[tokio::test]
    async fn transfer_with_a_future_or_unreadable_date_is_refused() {
        let (db, _from, to, asset) = fixture().await;
        let tomorrow = (crate::clock::now_local().date_naive() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let err = transfer::transfer_asset(
            &db,
            payload(&asset, to, Some(tomorrow), "motif"),
            &AccessScope::Everywhere,
            1,
        )
        .await
        .expect_err("future date");
        assert_eq!(code_of(err), "GATE_ASSET_TRANSFER_DATE_FUTURE");

        let err = transfer::transfer_asset(
            &db,
            payload(&asset, to, Some("31/12/2025".into()), "motif"),
            &AccessScope::Everywhere,
            1,
        )
        .await
        .expect_err("unreadable date");
        assert_eq!(code_of(err), "GATE_ASSET_TRANSFER_DATE_INVALID");
    }

    #[tokio::test]
    async fn transfer_before_the_last_move_is_refused() {
        let (db, from, to, asset) = fixture().await;
        let moved = transfer::transfer_asset(&db, payload(&asset, to, None, "aller"), &AccessScope::Everywhere, 1)
            .await
            .expect("first transfer");
        let yesterday = (crate::clock::now_local().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let err = transfer::transfer_asset(
            &db,
            payload(&moved.asset, from, Some(yesterday), "retour"),
            &AccessScope::Everywhere,
            1,
        )
        .await
        .expect_err("before the last move");
        assert_eq!(code_of(err), "GATE_ASSET_TRANSFER_DATE_BEFORE_LAST_MOVE");
    }

    #[tokio::test]
    async fn transfer_on_a_stale_version_is_refused() {
        let (db, _from, to, asset) = fixture().await;
        let mut stale = payload(&asset, to, None, "motif");
        stale.expected_row_version -= 1;
        let err = transfer::transfer_asset(&db, stale, &AccessScope::Everywhere, 1)
            .await
            .expect_err("stale version");
        assert_eq!(code_of(err), "GATE_RECORD_CHANGED_ELSEWHERE");
    }

    #[tokio::test]
    async fn transfer_of_a_retired_asset_is_refused() {
        let (db, _from, to, asset) = fixture().await;
        crate::assets::decommission::decommission_asset(
            &db,
            crate::assets::decommission::DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".into(),
                reason: "Fin de vie".into(),
                notes: None,
            },
            1,
        )
        .await
        .expect("decommission");
        let retired = identity::get_asset_by_id(&db, asset.id).await.expect("asset");
        let err = transfer::transfer_asset(&db, payload(&retired, to, None, "motif"), &AccessScope::Everywhere, 1)
            .await
            .expect_err("retired");
        assert_eq!(code_of(err), "GATE_ASSET_TRANSFER_RETIRED");
    }

    #[tokio::test]
    async fn transfer_needs_the_right_on_both_nodes() {
        let (db, from, to, asset) = fixture().await;

        // Destination outside the person's sites.
        let err = transfer::transfer_asset(&db, payload(&asset, to, None, "motif"), &only(&[from]), 1)
            .await
            .expect_err("destination outside");
        assert_eq!(code_of(err), "SCOPE_OUTSIDE_SITE");

        // Source outside the person's sites.
        let err = transfer::transfer_asset(&db, payload(&asset, to, None, "motif"), &only(&[to]), 1)
            .await
            .expect_err("source outside");
        assert_eq!(code_of(err), "SCOPE_OUTSIDE_SITE");

        let stored = identity::get_asset_by_id(&db, asset.id).await.expect("asset");
        assert_eq!(stored.org_node_id, Some(from), "nothing moved");

        // The destinations are only nodes in scope, never the current node.
        let preview = transfer::preview_asset_transfer(&db, asset.id, &only(&[from]))
            .await
            .expect("preview");
        assert!(preview.destinations.iter().all(|d| d.id != to && d.id != from));
        let preview = transfer::preview_asset_transfer(&db, asset.id, &AccessScope::Everywhere)
            .await
            .expect("preview");
        assert!(preview.destinations.iter().any(|d| d.id == to));
        assert!(preview.destinations.iter().all(|d| d.id != from));
        let err = transfer::preview_asset_transfer(&db, asset.id, &only(&[to]))
            .await
            .expect_err("preview outside");
        assert_eq!(code_of(err), "SCOPE_OUTSIDE_SITE");
    }

    #[tokio::test]
    async fn preview_counts_what_stays_attached() {
        let (db, from, _to, asset) = fixture().await;
        let other = create_test_asset(&db, "TRF-2", from).await;

        // Preventive plans: one active on the asset; a draft one and one on another asset are not counted.
        for (code, asset_id, status) in [
            ("PM-T1", asset.id, "active"),
            ("PM-T2", asset.id, "draft"),
            ("PM-T3", other.id, "active"),
        ] {
            exec(
                &db,
                format!(
                    "INSERT INTO pm_plans (code, title, asset_scope_type, asset_scope_id, strategy_type, lifecycle_status) VALUES ('{code}', 'Plan {code}', 'equipment', {asset_id}, 'fixed', '{status}')"
                ),
            )
            .await;
        }
        // Spare parts: two active lines (one critical) and one removed line.
        exec(
            &db,
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) VALUES ('trf-art', 'BRG-TRF', 'Roulement', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'U'))".to_string(),
        )
        .await;
        for (n, critical, active) in [(1, 1, 1), (2, 0, 1), (3, 1, 0)] {
            exec(
                &db,
                format!(
                    "INSERT INTO equipment_spare_parts (entity_sync_id, equipment_id, article_id, quantity, is_critical_spare, is_active, created_at, updated_at) VALUES ('trf-sp-{n}', {}, (SELECT id FROM articles WHERE article_code = 'BRG-TRF'), 1, {critical}, {active}, datetime('now'), datetime('now'))",
                    asset.id
                ),
            )
            .await;
        }
        // Work orders: one not yet closed, one closed.
        for (code, status) in [("WO-TRF-1", "draft"), ("WO-TRF-2", "closed")] {
            exec(
                &db,
                format!(
                    "INSERT INTO work_orders (code, type_id, status_id, equipment_id, title, row_version, created_at, updated_at) VALUES ('{code}', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'), (SELECT id FROM work_order_statuses WHERE code = '{status}' LIMIT 1), {}, 'OT {code}', 1, datetime('now'), datetime('now'))",
                    asset.id
                ),
            )
            .await;
        }

        let preview = transfer::preview_asset_transfer(&db, asset.id, &AccessScope::Everywhere)
            .await
            .expect("preview");
        assert_eq!(preview.from_org_node_id, Some(from));
        assert_eq!(
            preview.impact,
            AssetTransferImpact {
                active_pm_plan_count: 1,
                spare_part_line_count: 2,
                critical_spare_part_count: 1,
                open_wo_count: 1,
            }
        );
    }
}
