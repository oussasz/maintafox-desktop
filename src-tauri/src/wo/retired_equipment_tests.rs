//! AU-73 — no new work order on retired equipment (Déclassé / Rebuté), whatever creates it:
//! the shared creation function refuses it with a coded reason and writes nothing; equipment
//! returned to service takes work orders again.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::errors::AppError;
    use crate::wo::closeout_tests::tests::{admin_id, seed_test_equipment, setup};
    use crate::wo::domain::WoCreateInput;
    use crate::wo::queries;

    fn input(actor: i64) -> WoCreateInput {
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(1),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: "Retired equipment WO".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(1.0),
            creator_id: actor,
            requires_permit: None,
        }
    }

    async fn set_status(db: &sea_orm::DatabaseConnection, status: &str) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET lifecycle_status = ? WHERE id = 1",
            [status.into()],
        ))
        .await
        .expect("set equipment status");
    }

    async fn work_order_count(db: &sea_orm::DatabaseConnection) -> i64 {
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_orders".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "c")
        .expect("c")
    }

    #[tokio::test]
    async fn a_work_order_on_retired_equipment_is_refused_and_writes_nothing() {
        let db = setup().await;
        seed_test_equipment(&db).await;
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");
        let actor = admin_id(&db).await;

        for retired in ["DECOMMISSIONED", "SCRAPPED"] {
            set_status(&db, retired).await;
            let before = work_order_count(&db).await;
            let err = queries::create_work_order(&db, input(actor))
                .await
                .expect_err("retired equipment");
            assert!(
                matches!(&err, AppError::ValidationFailed(lines)
                    if lines.iter().any(|l| l.starts_with("GATE_WO_EQUIPMENT_RETIRED:asset=EQ-CO-001:"))),
                "{retired}: {err:?}"
            );
            assert_eq!(work_order_count(&db).await, before, "{retired}: nothing written");
        }

        // Back in service, the equipment takes work orders again.
        set_status(&db, "active_in_service").await;
        queries::create_work_order(&db, input(actor))
            .await
            .expect("equipment in service");
    }
}
