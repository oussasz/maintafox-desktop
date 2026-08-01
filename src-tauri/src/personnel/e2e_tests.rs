//! Roadmap E2E checks — `01-personnel-master-data-and-readiness-model.md` (V1–V6).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::personnel::domain::{PersonnelCreateInput, PersonnelListFilter, PersonnelUpdateInput};
    use crate::personnel::queries::{
        create_external_company, create_personnel, deactivate_personnel, get_active_rate_card,
        get_personnel, list_personnel, update_personnel,
    };
    use crate::wo::domain::WoCreateInput;
    use crate::wo::queries as wo_queries;

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder");

        db
    }

    async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1".to_string(),
            ))
            .await
            .expect("query")
            .expect("admin");
        row.try_get::<i64>("", "id").expect("id")
    }

    /// Insert minimal prerequisites (org_structure_model, node_type, two org_nodes, schedule RV).
    /// Returns `(position_id, entity_id, team_id, sched_rv_id)`.
    async fn prepare_prerequisites(db: &sea_orm::DatabaseConnection) -> (i64, i64, i64, i64) {
        let now = "2026-01-01T00:00:00Z";

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "INSERT OR IGNORE INTO org_structure_models \
                 (sync_id, version_number, status, created_at, updated_at) \
                 VALUES ('test-model-001', 1, 'active', '{now}', '{now}')"
            ),
        ))
        .await
        .expect("insert org_structure_model");

        let model_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_structure_models WHERE sync_id = 'test-model-001'".to_string(),
            ))
            .await
            .expect("q")
            .expect("model row")
            .try_get("", "id")
            .unwrap();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "INSERT OR IGNORE INTO org_node_types \
                 (sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
                 VALUES ('test-type-001', {model_id}, 'TEST', 'Test Type', 1, '{now}', '{now}')"
            ),
        ))
        .await
        .expect("insert org_node_type");

        let type_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_node_types WHERE sync_id = 'test-type-001' LIMIT 1".to_string(),
            ))
            .await
            .expect("q")
            .expect("type row")
            .try_get("", "id")
            .unwrap();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "INSERT OR IGNORE INTO org_nodes \
                 (sync_id, code, name, node_type_id, structure_model_id, \
                  ancestor_path, depth, status, created_at, updated_at) \
                 VALUES ('test-entity-001', 'ENT-TEST', 'Test Entity', \
                  {type_id}, {model_id}, '/', 0, 'active', '{now}', '{now}')"
            ),
        ))
        .await
        .expect("insert entity node");

        let entity_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes WHERE sync_id = 'test-entity-001'".to_string(),
            ))
            .await
            .expect("q")
            .expect("entity row")
            .try_get("", "id")
            .unwrap();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "INSERT OR IGNORE INTO org_nodes \
                 (sync_id, code, name, node_type_id, structure_model_id, \
                  ancestor_path, depth, status, created_at, updated_at) \
                 VALUES ('test-team-001', 'TEAM-TEST', 'Test Team', \
                  {type_id}, {model_id}, '/', 0, 'active', '{now}', '{now}')"
            ),
        ))
        .await
        .expect("insert team node");

        let team_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes WHERE sync_id = 'test-team-001'".to_string(),
            ))
            .await
            .expect("q")
            .expect("team row")
            .try_get("", "id")
            .unwrap();

        let position_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM positions WHERE code = 'POS-TECH' LIMIT 1".to_string(),
            ))
            .await
            .expect("q")
            .expect("position row")
            .try_get("", "id")
            .unwrap();

        // Prefer seeded ORG.SCHEDULE_CLASS; insert a minimal active value if missing.
        let sched_rv_id: i64 = match db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT rv.id FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(TRIM(rd.code)) = 'ORG.SCHEDULE_CLASS' \
                   AND rv.is_active = 1 \
                 ORDER BY rv.id ASC LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("q")
        {
            Some(row) => row.try_get("", "id").unwrap(),
            None => {
                // Fallback: any active reference_value (schedule assert may still fail — seed should provide)
                panic!("ORG.SCHEDULE_CLASS reference value missing after migrations/seeder");
            }
        };

        (position_id, entity_id, team_id, sched_rv_id)
    }

    /// V1 — Create personnel; DB row; activity `personnel.created`.
    #[tokio::test]
    async fn v1_create_personnel_end_to_end() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (position_id, entity_id, team_id, sched_rv_id) = prepare_prerequisites(&db).await;

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Test Tech".to_string(),
                employee_code: "PER-0001".to_string(),
                employment_type: "employee".to_string(),
                employment_origin: "internal".to_string(),
                employment_status: None,
                position_id,
                primary_entity_id: entity_id,
                primary_team_id: team_id,
                supervisor_id: None,
                home_schedule_reference_value_id: sched_rv_id,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                contract_number: None,
                contract_start_date: None,
                contract_end_date: None,
                notes: None,
                blocked_override: None,
                assignment_reason: None,
                skills: vec![],
                certifications: vec![],
            },
            actor,
        )
        .await
        .expect("create");

        assert_eq!(p.employee_code, "PER-0001");
        assert_eq!(p.availability_status, "available");

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT employee_code, availability_status FROM personnel WHERE id = ?",
                [p.id.into()],
            ))
            .await
            .expect("select")
            .expect("row");
        let code: String = row.try_get("", "employee_code").unwrap();
        let st: String = row.try_get("", "availability_status").unwrap();
        assert_eq!(code, "PER-0001");
        assert_eq!(st, "available");

        let ev = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, event_code, source_module, source_record_id FROM activity_events \
                 WHERE event_code = 'personnel.created' AND source_record_id = ? \
                 ORDER BY id DESC LIMIT 1",
                [p.id.to_string().into()],
            ))
            .await
            .expect("activity")
            .expect("event row");
        assert_eq!(
            ev.try_get::<String>("", "event_code").unwrap(),
            "personnel.created"
        );
        assert_eq!(
            ev.try_get::<String>("", "source_module").unwrap(),
            "personnel"
        );
    }

    /// V2 — Contractor + `external_company_id`; `get_personnel` fills `company_name`.
    #[tokio::test]
    async fn v2_contractor_company_name_join() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (position_id, entity_id, team_id, sched_rv_id) = prepare_prerequisites(&db).await;

        let co = create_external_company(
            &db,
            "Acme Contractors".to_string(),
            Some("maintenance".into()),
            None,
            None,
            None,
        )
        .await
        .expect("company");

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Contractor User".to_string(),
                employee_code: "CTR-0001".to_string(),
                employment_type: "contractor".to_string(),
                employment_origin: "external".to_string(),
                employment_status: None,
                position_id,
                primary_entity_id: entity_id,
                primary_team_id: team_id,
                supervisor_id: None,
                home_schedule_reference_value_id: sched_rv_id,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: Some(co.id),
                contract_number: None,
                contract_start_date: None,
                contract_end_date: None,
                notes: None,
                blocked_override: None,
                assignment_reason: None,
                skills: vec![],
                certifications: vec![],
            },
            actor,
        )
        .await
        .expect("create contractor");

        let g = get_personnel(&db, p.id).await.expect("get").expect("found");
        assert_eq!(
            g.company_name.as_deref(),
            Some("Acme Contractors"),
            "company_name join"
        );
    }

    /// V3 — Stale `expected_row_version` → validation error.
    #[tokio::test]
    async fn v3_update_stale_row_version() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (position_id, entity_id, team_id, sched_rv_id) = prepare_prerequisites(&db).await;

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Versioned".to_string(),
                employee_code: "VER-0001".to_string(),
                employment_type: "employee".to_string(),
                employment_origin: "internal".to_string(),
                employment_status: None,
                position_id,
                primary_entity_id: entity_id,
                primary_team_id: team_id,
                supervisor_id: None,
                home_schedule_reference_value_id: sched_rv_id,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                contract_number: None,
                contract_start_date: None,
                contract_end_date: None,
                notes: None,
                blocked_override: None,
                assignment_reason: None,
                skills: vec![],
                certifications: vec![],
            },
            actor,
        )
        .await
        .expect("create");

        let err = update_personnel(
            &db,
            PersonnelUpdateInput {
                id: p.id,
                expected_row_version: 99,
                full_name: Some("Should Fail".into()),
                employment_type: None,
                employment_origin: None,
                employment_status: None,
                blocked_override: None,
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_reference_value_id: None,
                availability_status: None,
                hire_date: None,
                termination_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                contract_number: None,
                contract_start_date: None,
                contract_end_date: None,
                notes: None,
                assignment_reason: None,
                employee_code: None,
            },
            actor,
        )
        .await
        .expect_err("stale version");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    /// V4 — WO primary blocks deactivation; after cancel, deactivation succeeds.
    #[tokio::test]
    async fn v4_deactivate_blocked_by_wo_then_succeeds() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (position_id, entity_id, team_id, sched_rv_id) = prepare_prerequisites(&db).await;

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "WO Linked".to_string(),
                employee_code: "WOL-0001".to_string(),
                employment_type: "employee".to_string(),
                employment_origin: "internal".to_string(),
                employment_status: None,
                position_id,
                primary_entity_id: entity_id,
                primary_team_id: team_id,
                supervisor_id: None,
                home_schedule_reference_value_id: sched_rv_id,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                contract_number: None,
                contract_start_date: None,
                contract_end_date: None,
                notes: None,
                blocked_override: None,
                assignment_reason: None,
                skills: vec![],
                certifications: vec![],
            },
            actor,
        )
        .await
        .expect("personnel");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET personnel_id = ? WHERE id = ?",
            [p.id.into(), actor.into()],
        ))
        .await
        .expect("link user to personnel");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "INSERT INTO equipment (sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at) \
                 VALUES ('test-eq-per-v4', 'EQ-PER-V4', 'Personnel V4 Equipment', 'active_in_service', \
                 {entity_id}, datetime('now'), datetime('now'))"
            ),
        ))
        .await
        .expect("insert equipment");

        let equipment_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM equipment WHERE sync_id = 'test-eq-per-v4'".to_string(),
            ))
            .await
            .expect("q")
            .expect("equipment row")
            .try_get("", "id")
            .unwrap();

        let wo = wo_queries::create_work_order(
            &db,
            WoCreateInput {
                type_code: "corrective".into(),
                equipment_id: Some(equipment_id),
                location_id: None,
                source_di_id: None,
                source_inspection_anomaly_id: None,
                source_ram_ishikawa_diagram_id: None,
                source_ishikawa_flow_node_id: None,
                source_rca_cause_text: None,
                entity_id: None,
                planner_id: None,
                urgency_id: Some(3),
                title: "Block deactivate test".into(),
                description: Some("test".into()),
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: Some(1.0),
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("wo");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET primary_responsible_id = ? WHERE id = ?",
            [actor.into(), wo.id.into()],
        ))
        .await
        .expect("assign primary to admin");

        let err = deactivate_personnel(&db, p.id, p.row_version, actor)
            .await
            .expect_err("blocked");
        let msg = match err {
            AppError::ValidationFailed(v) => v.join(" "),
            e => panic!("expected ValidationFailed, got {e:?}"),
        };
        assert!(
            msg.contains("Ordres de travail actifs") && msg.contains(&wo.code),
            "msg={msg}"
        );

        let cancelled_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM work_order_statuses WHERE code = 'cancelled' LIMIT 1".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = ?, cancelled_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            [cancelled_id.into(), wo.id.into()],
        ))
        .await
        .expect("cancel wo");

        let p2 = get_personnel(&db, p.id).await.unwrap().unwrap();
        deactivate_personnel(&db, p.id, p2.row_version, actor)
            .await
            .expect("deactivate after cancel");
    }

    /// V5 — Two rate cards with different `effective_from`; active = latest ≤ today.
    #[tokio::test]
    async fn v5_active_rate_card_picks_latest_effective() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (position_id, entity_id, team_id, sched_rv_id) = prepare_prerequisites(&db).await;

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Rated".to_string(),
                employee_code: "RTD-0001".to_string(),
                employment_type: "employee".to_string(),
                employment_origin: "internal".to_string(),
                employment_status: None,
                position_id,
                primary_entity_id: entity_id,
                primary_team_id: team_id,
                supervisor_id: None,
                home_schedule_reference_value_id: sched_rv_id,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                contract_number: None,
                contract_start_date: None,
                contract_end_date: None,
                notes: None,
                blocked_override: None,
                assignment_reason: None,
                skills: vec![],
                certifications: vec![],
            },
            actor,
        )
        .await
        .expect("create");

        db.execute_unprepared(&format!(
            "INSERT INTO personnel_rate_cards \
             (personnel_id, effective_from, labor_rate, overtime_rate, source_type, created_at) \
             VALUES ({}, '2020-01-01', 10.0, 15.0, 'manual', strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            p.id
        ))
        .await
        .unwrap();
        db.execute_unprepared(&format!(
            "INSERT INTO personnel_rate_cards \
             (personnel_id, effective_from, labor_rate, overtime_rate, source_type, created_at) \
             VALUES ({}, '2025-06-01', 20.0, 30.0, 'manual', strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            p.id
        ))
        .await
        .unwrap();

        let active = get_active_rate_card(&db, p.id)
            .await
            .expect("query")
            .expect("has active");
        assert_eq!(active.effective_from, "2025-06-01");
        assert!((active.labor_rate - 20.0).abs() < f64::EPSILON);
    }

    /// V6 — `list_personnel` search narrows to matching names.
    #[tokio::test]
    async fn v6_list_search_filters_by_name() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (position_id, entity_id, team_id, sched_rv_id) = prepare_prerequisites(&db).await;

        for (i, name) in ["Alice Jones", "Bob unique Smith", "Charlie Brown"]
            .iter()
            .enumerate()
        {
            create_personnel(
                &db,
                PersonnelCreateInput {
                    full_name: name.to_string(),
                    employee_code: format!("SRH-{i:04}"),
                    employment_type: "employee".to_string(),
                    employment_origin: "internal".to_string(),
                    employment_status: None,
                    position_id,
                    primary_entity_id: entity_id,
                    primary_team_id: team_id,
                    supervisor_id: None,
                    home_schedule_reference_value_id: sched_rv_id,
                    hire_date: None,
                    email: None,
                    phone: None,
                    external_company_id: None,
                    contract_number: None,
                    contract_start_date: None,
                    contract_end_date: None,
                    notes: None,
                    blocked_override: None,
                    assignment_reason: None,
                    skills: vec![],
                    certifications: vec![],
                },
                actor,
            )
            .await
            .expect(name);
        }

        let page = list_personnel(
            &db,
            PersonnelListFilter {
                search: Some("unique".into()),
                ..Default::default()
            },
        )
        .await
        .expect("list");

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert!(
            page.items[0].full_name.contains("unique"),
            "{}",
            page.items[0].full_name
        );
    }
}
