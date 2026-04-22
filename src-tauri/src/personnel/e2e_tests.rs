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

    /// V1 — Create personnel; DB row; activity `personnel.created`.
    #[tokio::test]
    async fn v1_create_personnel_end_to_end() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Test Tech".to_string(),
                employee_code: None,
                employment_type: "employee".to_string(),
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_id: None,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                notes: None,
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
                employee_code: None,
                employment_type: "contractor".to_string(),
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_id: None,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: Some(co.id),
                notes: None,
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

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Versioned".to_string(),
                employee_code: None,
                employment_type: "employee".to_string(),
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_id: None,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                notes: None,
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
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_id: None,
                availability_status: None,
                hire_date: None,
                termination_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                notes: None,
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

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "WO Linked".to_string(),
                employee_code: None,
                employment_type: "employee".to_string(),
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_id: None,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                notes: None,
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

        let wo = wo_queries::create_work_order(
            &db,
            WoCreateInput {
                type_code: "corrective".into(),
                equipment_id: None,
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

        let p = create_personnel(
            &db,
            PersonnelCreateInput {
                full_name: "Rated".to_string(),
                employee_code: None,
                employment_type: "employee".to_string(),
                position_id: None,
                primary_entity_id: None,
                primary_team_id: None,
                supervisor_id: None,
                home_schedule_id: None,
                hire_date: None,
                email: None,
                phone: None,
                external_company_id: None,
                notes: None,
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

        for name in ["Alice Jones", "Bob unique Smith", "Charlie Brown"] {
            create_personnel(
                &db,
                PersonnelCreateInput {
                    full_name: name.to_string(),
                    employee_code: None,
                    employment_type: "employee".to_string(),
                    position_id: None,
                    primary_entity_id: None,
                    primary_team_id: None,
                    supervisor_id: None,
                    home_schedule_id: None,
                    hire_date: None,
                    email: None,
                    phone: None,
                    external_company_id: None,
                    notes: None,
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
