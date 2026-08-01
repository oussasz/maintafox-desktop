//! Tests for ORG.SCHEDULE_CLASS schedule pattern extension.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::reference::schedule_patterns::{
        get_schedule_pattern, upsert_schedule_pattern, ScheduleDayPattern, UpsertSchedulePatternPayload,
        SCHEDULE_CLASS_DOMAIN_CODE,
    };
    use crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity;
    use crate::reference::values::{create_operational_value, CreateOperationalReferenceValuePayload};

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.expect("connect");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("pragma");
        crate::migrations::Migrator::up(&db, None).await.expect("migrate");
        ensure_system_reference_catalog_integrity(&db).await.expect("integrity");
        db
    }

    #[tokio::test]
    async fn org_schedule_class_domain_and_day_shift_exist_after_migration() {
        let db = setup().await;

        let domain = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT id FROM reference_domains WHERE UPPER(TRIM(code)) = '{}'",
                    SCHEDULE_CLASS_DOMAIN_CODE
                ),
            ))
            .await
            .expect("query")
            .expect("ORG.SCHEDULE_CLASS domain");

        let domain_id: i64 = domain.try_get("", "id").expect("id");
        assert!(domain_id > 0);

        let day_shift = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT rv.id FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                   AND UPPER(TRIM(rv.code)) = 'DAY_SHIFT'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("DAY_SHIFT value");

        let value_id: i64 = day_shift.try_get("", "id").expect("id");
        let details: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM schedule_details WHERE reference_value_id = ?",
                [value_id.into()],
            ))
            .await
            .expect("count")
            .expect("row")
            .try_get("", "c")
            .expect("c");
        assert_eq!(details, 7, "DAY_SHIFT must have 7 weekday rows");

        let legacy = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='schedule_classes'".to_string(),
            ))
            .await
            .expect("sqlite_master");
        assert!(legacy.is_none(), "schedule_classes must be dropped");

        let orphan = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM reference_domains WHERE UPPER(TRIM(code)) = 'ORG.SCHEDULES'".to_string(),
            ))
            .await
            .expect("query");
        assert!(orphan.is_none(), "ORG.SCHEDULES placeholder must be retired");
    }

    #[tokio::test]
    async fn create_operational_value_auto_seeds_weekday_details() {
        let db = setup().await;
        let created = create_operational_value(
            &db,
            CreateOperationalReferenceValuePayload {
                domain_code: SCHEDULE_CLASS_DOMAIN_CODE.into(),
                label: "Équipe weekend".into(),
                description: None,
                parent_id: None,
                code: Some("WEEKEND_SHIFT".into()),
            },
            1,
        )
        .await
        .expect("create operational");

        let pattern = get_schedule_pattern(&db, created.id).await.expect("get pattern");
        assert_eq!(pattern.details.len(), 7);
        assert_eq!(pattern.shift_pattern_code, "WEEKEND_SHIFT");
    }

    #[tokio::test]
    async fn upsert_schedule_pattern_updates_metadata_and_details() {
        let db = setup().await;
        let day_shift = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT rv.id FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                   AND UPPER(TRIM(rv.code)) = 'DAY_SHIFT' LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("DAY_SHIFT");
        let value_id: i64 = day_shift.try_get("", "id").expect("id");

        let details: Vec<ScheduleDayPattern> = (1..=7)
            .map(|day| ScheduleDayPattern {
                day_of_week: day,
                shift_start: "06:00".into(),
                shift_end: "14:00".into(),
                is_rest_day: day == 7,
            })
            .collect();

        let updated = upsert_schedule_pattern(
            &db,
            UpsertSchedulePatternPayload {
                reference_value_id: value_id,
                shift_pattern_code: Some("EARLY_SHIFT".into()),
                is_continuous: Some(false),
                nominal_hours_per_day: Some(8.0),
                details: Some(details),
            },
            1,
        )
        .await
        .expect("upsert");

        assert_eq!(updated.shift_pattern_code, "EARLY_SHIFT");
        assert_eq!(updated.details.len(), 7);
        assert_eq!(updated.details[0].shift_start, "06:00");
        assert!(updated.details[6].is_rest_day);
    }
}
