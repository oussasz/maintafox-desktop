//! L2-09 — repeat failures: the flag follows the settings, 5 pourquoi and corrective actions are
//! recorded on the equipment.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::repeat_failures::{
        self, SaveCorrectiveActionPayload, SaveFiveWhyPayload, SETTING_COUNT, SETTING_WINDOW_DAYS,
    };

    async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    async fn exec(db: &sea_orm::DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn id_of(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .expect("row")
            .try_get_by_index::<i64>(0)
            .expect("id")
    }

    async fn equipment(db: &sea_orm::DatabaseConnection, code: &str) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
                 VALUES ('l209-{code}', '{code}', 'Equipment {code}', 'active_in_service', datetime('now'), datetime('now'))"
            ),
        )
        .await;
        id_of(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
    }

    /// An unplanned failure `days_ago` days back (dated by `failed_at`).
    async fn failure(db: &sea_orm::DatabaseConnection, equipment_id: i64, n: i64, days_ago: i64, planned: bool) {
        exec(
            db,
            &format!(
                "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, failed_at, is_planned, \
                 created_at, updated_at) VALUES ('l209-fe-{equipment_id}-{n}', 'work_order', {source}, {equipment_id}, \
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-{days_ago} days'), {planned}, datetime('now'), datetime('now'))",
                source = equipment_id * 1000 + n,
                planned = i64::from(planned),
            ),
        )
        .await;
    }

    async fn set(db: &sea_orm::DatabaseConnection, key: &str, value: &str) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE app_settings SET setting_value_json = ? WHERE setting_key = ? AND setting_scope = 'tenant'",
            [value.into(), key.into()],
        ))
        .await
        .expect("update setting");
    }

    #[tokio::test]
    async fn the_flag_follows_the_count_and_window_settings() {
        let db = setup().await;
        let rule = repeat_failures::repeat_failure_rule(&db).await.expect("shipped rule");
        assert_eq!((rule.min_failures, rule.window_days), (3, 90));

        let flagged = equipment(&db, "RF-1").await;
        let quiet = equipment(&db, "RF-2").await;
        for (n, days) in [(1, 5), (2, 30), (3, 80)] {
            failure(&db, flagged, n, days, false).await;
        }
        // Old, planned or too few: not flagged.
        failure(&db, quiet, 1, 10, false).await;
        failure(&db, quiet, 2, 200, false).await;
        failure(&db, quiet, 3, 20, true).await;

        let overview = repeat_failures::list_repeat_failure_assets(&db)
            .await
            .expect("overview");
        assert_eq!(overview.assets.len(), 1);
        assert_eq!(overview.assets[0].asset_id, flagged);
        assert_eq!(overview.assets[0].failure_count, 3);

        let status = repeat_failures::asset_repeat_failure_status(&db, quiet)
            .await
            .expect("status");
        assert!(!status.is_repeat_failure);
        assert_eq!(status.failure_count, 1);

        // A shorter window drops the oldest failure; a lower count flags the other equipment.
        set(&db, SETTING_WINDOW_DAYS, "60").await;
        assert!(repeat_failures::list_repeat_failure_assets(&db)
            .await
            .expect("overview")
            .assets
            .is_empty());
        set(&db, SETTING_COUNT, "2").await;
        let overview = repeat_failures::list_repeat_failure_assets(&db)
            .await
            .expect("overview");
        assert_eq!(
            overview.assets.iter().map(|a| a.asset_id).collect::<Vec<_>>(),
            vec![flagged]
        );
        set(&db, SETTING_WINDOW_DAYS, "365").await;
        let overview = repeat_failures::list_repeat_failure_assets(&db)
            .await
            .expect("overview");
        assert_eq!(overview.assets.len(), 2);

        // Never a guessed rule: a setting out of its bounds is an error.
        set(&db, SETTING_COUNT, "1").await;
        assert!(repeat_failures::list_repeat_failure_assets(&db).await.is_err());
    }

    #[tokio::test]
    async fn five_why_and_corrective_actions_are_tracked_on_the_equipment() {
        let db = setup().await;
        let asset = equipment(&db, "RF-3").await;
        exec(
            &db,
            "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('L209-P', 'Karim Responsable', 'available')",
        )
        .await;
        let owner = id_of(&db, "SELECT id FROM personnel WHERE employee_code = 'L209-P'").await;

        // 5 pourquoi: the problem is required, a change needs the current version.
        let payload = |id: Option<i64>, version: Option<i64>, problem: &str| SaveFiveWhyPayload {
            id,
            asset_id: asset,
            expected_row_version: version,
            problem_statement: problem.to_string(),
            why_1: Some("Roulement grippé".into()),
            why_2: Some("Graissage insuffisant".into()),
            why_3: None,
            why_4: None,
            why_5: None,
            root_cause: Some("Gamme de graissage absente".into()),
        };
        let err = repeat_failures::save_five_why(&db, payload(None, None, "  "), 1)
            .await
            .expect_err("problem required");
        assert!(format!("{err:?}").contains("GATE_ASSET_FIVE_WHY_PROBLEM_REQUIRED"));
        let created = repeat_failures::save_five_why(&db, payload(None, None, "Arrêts répétés du moteur"), 1)
            .await
            .expect("created");
        assert_eq!(created.row_version, 1);
        let updated = repeat_failures::save_five_why(&db, payload(Some(created.id), Some(1), "Arrêts répétés"), 1)
            .await
            .expect("updated");
        assert_eq!(updated.row_version, 2);
        let err = repeat_failures::save_five_why(&db, payload(Some(created.id), Some(1), "stale"), 1)
            .await
            .expect_err("stale version");
        assert!(format!("{err:?}").contains("GATE_ASSET_FIVE_WHY_CHANGED_ELSEWHERE"));
        assert_eq!(
            repeat_failures::list_five_whys(&db, asset).await.expect("list").len(),
            1
        );

        // Corrective action: text, owner, due date and status are required.
        let action =
            |id: Option<i64>, version: Option<i64>, status: &str, due: Option<&str>| SaveCorrectiveActionPayload {
                id,
                asset_id: asset,
                expected_row_version: version,
                action_text: "Créer la gamme de graissage".into(),
                owner_personnel_id: Some(owner),
                due_date: due.map(str::to_string),
                status: status.into(),
            };
        let err = repeat_failures::save_corrective_action(&db, action(None, None, "open", None), 1)
            .await
            .expect_err("due date required");
        assert!(format!("{err:?}").contains("GATE_ASSET_ACTION_DUE_DATE_REQUIRED"));
        let err = repeat_failures::save_corrective_action(&db, action(None, None, "maybe", Some("2026-12-01")), 1)
            .await
            .expect_err("status");
        assert!(format!("{err:?}").contains("GATE_ASSET_ACTION_STATUS_INVALID"));

        let overdue = repeat_failures::save_corrective_action(&db, action(None, None, "open", Some("2000-01-01")), 1)
            .await
            .expect("created");
        assert!(overdue.is_overdue);
        assert_eq!(overdue.owner_name.as_deref(), Some("Karim Responsable"));
        let status = repeat_failures::asset_repeat_failure_status(&db, asset)
            .await
            .expect("status");
        assert_eq!((status.open_action_count, status.overdue_action_count), (1, 1));

        let done = repeat_failures::save_corrective_action(
            &db,
            action(Some(overdue.id), Some(overdue.row_version), "done", Some("2000-01-01")),
            1,
        )
        .await
        .expect("done");
        assert_eq!(done.status, "done");
        assert!(done.completed_at.is_some() && !done.is_overdue);
        let reopened = repeat_failures::save_corrective_action(
            &db,
            action(Some(done.id), Some(done.row_version), "open", Some("2999-01-01")),
            1,
        )
        .await
        .expect("reopened");
        assert!(reopened.completed_at.is_none());
        assert_eq!(
            repeat_failures::list_corrective_actions(&db, asset)
                .await
                .expect("list")
                .len(),
            1
        );
    }
}
