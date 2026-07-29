//! Regression: WORK.FAILURE_MODES baseline after wipe (no failure_codes).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity;

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
}
