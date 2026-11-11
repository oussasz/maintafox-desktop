//! Regression: WORK.FAILURE_MODES baseline after wipe (no failure_codes).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity;

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = crate::db::test_db::seeded().await;

        ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("initial integrity");

        db
    }

    async fn published_failure_mode_count(db: &sea_orm::DatabaseConnection) -> i64 {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c \
                 FROM reference_values rv \
                 INNER JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                 INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES')) \
                   AND rv.is_active = 1"
                    .to_string(),
            ))
            .await
            .expect("count query")
            .expect("count row");
        row.try_get::<i64>("", "c").expect("c")
    }

    #[tokio::test]
    async fn work_failure_modes_restored_without_failure_codes() {
        let db = setup().await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM failure_codes".to_string(),
        ))
        .await
        .expect("clear failure_codes");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM reference_values \
             WHERE set_id IN ( \
               SELECT rs.id FROM reference_sets rs \
               INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
               WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES')) \
             )"
            .to_string(),
        ))
        .await
        .expect("clear FAILURE_MODES values");

        assert_eq!(
            published_failure_mode_count(&db).await,
            0,
            "precondition: catalog emptied"
        );

        ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("integrity after wipe simulation");

        let count = published_failure_mode_count(&db).await;
        assert!(
            count >= 7,
            "expected baseline FAILURE_MODES values without failure_codes, got {count}"
        );

        let vibration = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 1 AS ok \
                 FROM reference_values rv \
                 INNER JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                 INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES')) \
                   AND UPPER(TRIM(rv.code)) = 'VIBRATION' \
                   AND rv.is_active = 1"
                    .to_string(),
            ))
            .await
            .expect("vibration query");
        assert!(vibration.is_some(), "VIBRATION baseline missing");

        // Idempotent
        ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("second integrity pass");
        assert_eq!(
            published_failure_mode_count(&db).await,
            count,
            "re-run must not duplicate baseline rows"
        );
    }

    async fn domain_value_count(db: &sea_orm::DatabaseConnection, domain: &str, prefix: &str) -> i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM reference_values rv \
                 INNER JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                 INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(TRIM(rd.code)) = ? AND substr(rv.code, 1, length(?)) = ? AND rv.is_active = 1",
                [domain.into(), prefix.into(), prefix.into()],
            ))
            .await
            .expect("count query")
            .expect("count row");
        row.try_get::<i64>("", "c").expect("c")
    }

    #[tokio::test]
    async fn failure_catalogue_family_modes_and_remedies_seed_idempotently() {
        let db = setup().await;
        let pump = domain_value_count(&db, "WORK.FAILURE_MODES", "POMPE_").await;
        let motor = domain_value_count(&db, "WORK.FAILURE_MODES", "MOTEUR_").await;
        let remedies = domain_value_count(&db, "WORK.FAILURE_REMEDIES", "REM_").await;
        assert!(
            pump >= 6 && motor >= 6,
            "family modes missing: pump {pump}, motor {motor}"
        );
        assert!(remedies >= 20, "remedies missing: {remedies}");

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT label, description FROM reference_values WHERE code = 'POMPE_CAVITATION'".to_string(),
            ))
            .await
            .expect("query")
            .expect("cavitation row");
        assert_eq!(row.try_get::<String>("", "label").expect("label"), "Pompe — Cavitation");
        assert_eq!(
            row.try_get::<String>("", "description").expect("description"),
            "Pump cavitation"
        );

        ensure_system_reference_catalog_integrity(&db)
            .await
            .expect("second pass");
        assert_eq!(domain_value_count(&db, "WORK.FAILURE_MODES", "POMPE_").await, pump);
        assert_eq!(domain_value_count(&db, "WORK.FAILURE_REMEDIES", "REM_").await, remedies);
    }
}
