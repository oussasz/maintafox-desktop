//! Roadmap verification — `01-personnel-master-data-and-readiness-model.md` (V1–V4).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::personnel::domain::{
        insert_personnel_with_auto_code, AvailabilityStatus, AuthorizationType, EmploymentType,
        InsuranceStatus, OnboardingStatus, PositionCategory,
    };

    async fn setup_migrated_seeded(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(db, None)
            .await
            .expect("migrations should apply");

        crate::db::seeder::seed_system_data(db)
            .await
            .expect("seeder should run");
    }

    /// V1 — Migration applies cleanly; 8 tables; seeds: 7 positions, 1 schedule class, 7 details.
    #[tokio::test]
    async fn v1_migration_tables_and_seed_counts() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect");
        setup_migrated_seeded(&db).await;

        let expected_tables = [
            "positions",
            "schedule_classes",
            "schedule_details",
            "external_companies",
            "external_company_contacts",
            "personnel",
            "personnel_rate_cards",
            "personnel_authorizations",
        ];

        for name in expected_tables {
            let row = db
                .query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT name FROM sqlite_master WHERE type='table' AND name='{name}';"
                    )
                    .to_string(),
                ))
                .await
                .expect("sqlite_master query");
            assert!(
                row.is_some(),
                "table `{name}` must exist after migration 039"
            );
        }

        let n: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM positions".to_string(),
            ))
            .await
            .expect("count positions")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("c");
        assert_eq!(n, 7, "seed positions");

        let active: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM positions WHERE is_active = 1".to_string(),
            ))
            .await
            .expect("count active positions")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("c");
        assert_eq!(active, 7, "all seeded positions active");

        let sc: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM schedule_classes".to_string(),
            ))
            .await
            .expect("count schedule_classes")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("c");
        assert_eq!(sc, 1, "seed schedule class");

        let sd: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM schedule_details".to_string(),
            ))
            .await
            .expect("count schedule_details")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("c");
        assert_eq!(sd, 7, "seed schedule details");

        // Key indexes from migration 039
        for (tbl, idx) in [
            ("personnel", "idx_per_code"),
            ("personnel", "idx_per_entity"),
            ("personnel_rate_cards", "idx_prc_personnel"),
            ("personnel_authorizations", "idx_pa_personnel"),
        ] {
            let q = format!(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND tbl_name='{tbl}' AND name='{idx}'"
            );
            let ok = db
                .query_one(Statement::from_string(DbBackend::Sqlite, q))
                .await
                .expect("index query")
                .is_some();
            assert!(ok, "index {idx} on {tbl} must exist");
        }
    }

    /// V2 — `TryFrom` for every personnel enum: valid + invalid strings.
    #[test]
    fn v2_enum_try_from_valid_and_invalid() {
        assert_eq!(
            EmploymentType::try_from("contractor").unwrap(),
            EmploymentType::Contractor
        );
        assert!(EmploymentType::try_from("invalid").is_err());

        assert!(AvailabilityStatus::try_from("available").is_ok());
        assert!(AvailabilityStatus::try_from("not_a_status").is_err());

        assert!(PositionCategory::try_from("technician").is_ok());
        assert!(PositionCategory::try_from("CEO").is_err());

        assert!(AuthorizationType::try_from("warehouse_signoff").is_ok());
        assert!(AuthorizationType::try_from("root").is_err());

        assert!(OnboardingStatus::try_from("pending").is_ok());
        assert!(OnboardingStatus::try_from("").is_err());

        assert!(InsuranceStatus::try_from("not_required").is_ok());
        assert!(InsuranceStatus::try_from("maybe").is_err());
    }

    /// V3 — Two personnel rows created concurrently get `PER-0001` and `PER-0002` (no duplicates).
    #[tokio::test]
    async fn v3_concurrent_auto_codes_unique() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("personnel_v3.sqlite");
        let url = format!("sqlite://{}?mode=rwc", path.display().to_string().replace('\\', "/"));

        let db_a = Database::connect(&url).await.expect("db_a");
        setup_migrated_seeded(&db_a).await;

        let db_b = Database::connect(&url).await.expect("db_b");

        let (r1, r2) = tokio::join!(
            insert_personnel_with_auto_code(&db_a, "Concurrent A"),
            insert_personnel_with_auto_code(&db_b, "Concurrent B"),
        );

        let c1 = r1.expect("insert A");
        let c2 = r2.expect("insert B");

        let mut codes = vec![c1, c2];
        codes.sort();
        assert_eq!(codes, vec!["PER-0001".to_string(), "PER-0002".to_string()]);

        let n: i64 = db_a
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM personnel".to_string(),
            ))
            .await
            .expect("count")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("c");
        assert_eq!(n, 2);
    }

    /// V4 — Seed integrity (also covered by V1; explicit rest-day count).
    #[tokio::test]
    async fn v4_seed_positions_and_rest_days() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect");
        setup_migrated_seeded(&db).await;

        let pos: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM positions WHERE is_active = 1".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        assert_eq!(pos, 7);

        let rest: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM schedule_details WHERE is_rest_day = 1".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        assert_eq!(rest, 2, "Sat/Sun rest days");
    }
}
