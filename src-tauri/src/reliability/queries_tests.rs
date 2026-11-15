#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use serde_json::Value;
    use uuid::Uuid;

    use crate::reliability::domain::{RefreshReliabilityKpiSnapshotInput, UpsertRuntimeExposureLogInput};
    use crate::reliability::queries;

    async fn setup() -> DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    async fn insert_minimal_equipment(db: &DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        let entity_sync_id = Uuid::new_v4().to_string();
        let code = format!("EQ-{}", &entity_sync_id[..8]);
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO equipment
              (entity_sync_id, asset_id_code, name, lifecycle_status, maintainable_boundary, rams_utilization_factor, created_at, updated_at, row_version)
             VALUES (?, ?, ?, 'IN_SERVICE', 1, 1.0, ?, ?, 1)",
            [
                entity_sync_id.clone().into(),
                code.into(),
                "KPI Test Asset".into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert equipment");
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM equipment WHERE entity_sync_id = ?",
                [entity_sync_id.into()],
            ))
            .await
            .expect("select equipment id")
            .expect("equipment row");
        row.try_get::<i64>("", "id").expect("equipment id")
    }

    #[tokio::test]
    async fn refresh_kpi_snapshot_is_idempotent_on_period_identity() {
        let db = setup().await;
        let equipment_id = insert_minimal_equipment(&db).await;

        let now = chrono::Utc::now();
        let p0 = (now - chrono::Duration::hours(24)).to_rfc3339();
        let p1 = now.to_rfc3339();

        let _ = queries::upsert_runtime_exposure_log(
            &db,
            UpsertRuntimeExposureLogInput {
                id: None,
                expected_row_version: None,
                equipment_id,
                exposure_type: "hours".into(),
                value: 8.0,
                recorded_at: (now - chrono::Duration::hours(2)).to_rfc3339(),
                source_type: "manual".into(),
            },
        )
        .await
        .expect("insert runtime exposure");

        let one = queries::refresh_reliability_kpi_snapshot(
            &db,
            RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: p0.clone(),
                period_end: p1.clone(),
                min_sample_n: Some(1),
                repeat_lookback_days: Some(30),
            },
        )
        .await
        .expect("first refresh");

        let two = queries::refresh_reliability_kpi_snapshot(
            &db,
            RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: p0,
                period_end: p1,
                min_sample_n: Some(1),
                repeat_lookback_days: Some(30),
            },
        )
        .await
        .expect("second refresh");

        assert_eq!(one.id, two.id, "same period must upsert same snapshot row");
        assert!(
            two.row_version > one.row_version,
            "row_version should increment on deterministic refresh"
        );
    }

    #[tokio::test]
    async fn refresh_kpi_snapshot_persists_exposure_provenance() {
        let db = setup().await;
        let equipment_id = insert_minimal_equipment(&db).await;

        let now = chrono::Utc::now();
        let out = queries::refresh_reliability_kpi_snapshot(
            &db,
            RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: (now - chrono::Duration::hours(12)).to_rfc3339(),
                period_end: now.to_rfc3339(),
                min_sample_n: Some(1),
                repeat_lookback_days: Some(7),
            },
        )
        .await
        .expect("refresh snapshot");

        let v: Value = serde_json::from_str(&out.analysis_input_spec_json).expect("valid analysis_input_spec_json");
        let exp = v
            .get("exposure_provenance")
            .and_then(|x| x.as_object())
            .expect("exposure_provenance object");

        assert_eq!(
            exp.get("source").and_then(|x| x.as_str()),
            Some("runtime_exposure_logs_fallback_no_completed_wo")
        );
        assert_eq!(exp.get("utilization_factor").and_then(|x| x.as_f64()), Some(1.0));
    }

    /// L2-10: a snapshot refresh counts every unplanned failure, decided live from `is_planned` —
    /// an unplanned record whose stored flag is false counts, a planned one does not.
    #[tokio::test]
    async fn refresh_kpi_snapshot_counts_unplanned_failures_whatever_the_stored_flag() {
        let db = setup().await;
        let equipment_id = insert_minimal_equipment(&db).await;
        let now = chrono::Utc::now();
        for (tag, hours_ago, planned) in [("a", 10, 0), ("b", 20, 0), ("p", 30, 1)] {
            let at = (now - chrono::Duration::hours(hours_ago)).to_rfc3339();
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, detected_at, \
                 failed_at, downtime_duration_hours, active_repair_hours, waiting_hours, is_planned, \
                 cause_not_determined, verification_status, eligible_flags_json, row_version, created_at, updated_at) \
                 VALUES (?, 'manual', (SELECT COALESCE(MAX(id), 0) + 100000 FROM failure_events), ?, ?, ?, 1.0, 1.0, \
                 0, ?, 0, 'recorded', '{\"eligible_unplanned_mtbf\":false}', 1, ?, ?)",
                [
                    format!("qt-fe-{tag}").into(),
                    equipment_id.into(),
                    at.clone().into(),
                    at.clone().into(),
                    planned.into(),
                    at.clone().into(),
                    at.into(),
                ],
            ))
            .await
            .expect("insert failure event");
        }
        let out = queries::refresh_reliability_kpi_snapshot(
            &db,
            RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: (now - chrono::Duration::hours(48)).to_rfc3339(),
                period_end: now.to_rfc3339(),
                min_sample_n: Some(1),
                repeat_lookback_days: Some(7),
            },
        )
        .await
        .expect("refresh snapshot");
        assert_eq!(out.event_count, 2, "the two unplanned records, not the planned one");
        assert_eq!(
            out.algorithm_version.as_deref(),
            Some(crate::reliability::compute::RELIABILITY_KPI_ALGORITHM_VERSION)
        );
        assert_eq!(out.algorithm_version.as_deref(), Some("reliability_kpi_v3"));
    }
}
