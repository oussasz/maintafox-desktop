//! Meter import (L7-32 §4b): preview rules and apply.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::{self, CreateInput};

    const HEADER: &str =
        "code équipement;code compteur;libellé;type de compteur;unité;relevé initial;date du relevé initial;principal";

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql);
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql)
            .expect("row")
            .try_get_by_index::<i64>(0)
            .expect("i64")
    }

    async fn text(db: &DatabaseConnection, sql: &str) -> Option<String> {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql)
            .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("value"))
    }

    /// Two pieces of equipment, MT-P1 and MT-P2, on one site.
    async fn setup() -> DatabaseConnection {
        let db = crate::db::test_db::seeded().await;
        for sql in [
            "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'mt-model', 1, 'active', datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
             created_at, updated_at) VALUES (1, 'mt-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
             updated_at, structure_model_id) VALUES (1, 'mt-node', 'SITE-MT', 'Site', 1, 'active', \
             datetime('now'), datetime('now'), 1)",
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('mt-eq-1', 'MT-P1', 'Pompe 1', 'active_in_service', 1, \
             datetime('now'), datetime('now'))",
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('mt-eq-2', 'MT-P2', 'Pompe 2', 'active_in_service', 1, \
             datetime('now'), datetime('now'))",
        ] {
            exec(&db, sql).await;
        }
        db
    }

    fn input(csv: &str) -> CreateInput {
        CreateInput {
            entity: "equipment_meter".to_string(),
            filename: "compteurs.csv".to_string(),
            source_sha256: "0".repeat(64),
            source_kind: "csv".to_string(),
            file_content: csv.as_bytes().to_vec(),
        }
    }

    async fn run(db: &DatabaseConnection, lines: &[&str]) -> bulk_import::Preview {
        let csv = format!("{HEADER}\n{}\n", lines.join("\n"));
        let b = bulk_import::create_batch(db, input(&csv), Some(1))
            .await
            .expect("batch");
        bulk_import::get_preview(db, b.id).await.expect("preview")
    }

    async fn apply(db: &DatabaseConnection, p: &bulk_import::Preview) -> bulk_import::ApplyResult {
        bulk_import::apply_batch(db, p.batch.id, 1).await.expect("apply")
    }

    fn codes(p: &bulk_import::Preview, row_no: i64, severity: &str) -> Vec<String> {
        p.rows
            .iter()
            .find(|r| r.row_no == row_no)
            .expect("row")
            .messages
            .iter()
            .filter(|m| m.severity == severity)
            .map(|m| m.code.clone())
            .collect()
    }

    fn action(p: &bulk_import::Preview, row_no: i64) -> String {
        p.rows.iter().find(|r| r.row_no == row_no).expect("row").action.clone()
    }

    #[tokio::test]
    async fn creates_meters_with_a_dated_first_reading() {
        let db = setup().await;
        let p = run(
            &db,
            &[
                "MT-P1;HRS;Heures de marche;Heures;h;1234,5;01/03/2026;oui",
                "mt-p2;CYC;Démarrages;CYCLES;H;;;",
            ],
        )
        .await;
        assert_eq!(action(&p, 2), "create", "{:?}", p.rows);
        assert_eq!(action(&p, 3), "create", "{:?}", p.rows);
        let r = apply(&db, &p).await;
        assert_eq!((r.created, r.failed), (2, 0));

        let meter = scalar(&db, "SELECT id FROM equipment_meters WHERE meter_code = 'HRS'").await;
        let meter_sql = format!(
            "SELECT COUNT(*) FROM equipment_meters WHERE id = {meter} AND meter_type = 'HOURS' \
             AND is_primary = 1 AND current_reading = 1234.5 AND last_read_at IS NOT NULL \
             AND unit_value_id IS NOT NULL"
        );
        assert_eq!(scalar(&db, &meter_sql).await, 1);
        let reading_sql = format!(
            "SELECT COUNT(*) FROM asset_meter_readings WHERE meter_id = {meter} AND source_type = 'IMPORT' \
             AND reading_value = 1234.5 AND lifetime_value = 1234.5 AND reading_kind = 'normal' \
             AND entity_sync_id IS NOT NULL"
        );
        assert_eq!(scalar(&db, &reading_sql).await, 1);
        let at = text(
            &db,
            &format!("SELECT reading_at FROM asset_meter_readings WHERE meter_id = {meter}"),
        )
        .await
        .expect("reading_at");
        assert!(at.starts_with("2026-02-28") || at.starts_with("2026-03-01"), "{at}");
        // No reading given: none recorded, no date invented.
        let cyc_sql = "SELECT COUNT(*) FROM asset_meter_readings mr JOIN equipment_meters m ON m.id = mr.meter_id \
                       WHERE m.meter_code = 'CYC'";
        assert_eq!(scalar(&db, cyc_sql).await, 0);

        // The export reads back and re-imports as "nothing to do".
        let export = bulk_import::export(&db, "equipment_meter").await.expect("export");
        let lines: Vec<String> = export
            .rows
            .iter()
            .filter(|r| r[0].starts_with("MT-"))
            .map(|r| r.join(";"))
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        let again = run(&db, &lines.iter().map(String::as_str).collect::<Vec<_>>()).await;
        for row in &again.rows {
            assert_eq!(row.action, "skip", "{row:?}");
            assert!(row.messages.iter().all(|m| m.severity != "error"), "{row:?}");
        }
    }

    #[tokio::test]
    async fn refuses_bad_lines_and_never_invents_data() {
        let db = setup().await;
        let p = run(
            &db,
            &[
                "NOPE;A;A;HOURS;h;;;",              // 2 unknown equipment
                "MT-P1;A;A;BOGUS;h;;;",             // 3 unknown type
                "MT-P1;B;B;HOURS;furlong;;;",       // 4 unknown unit
                "MT-P1;C;C;HOURS;h;10;;",           // 5 reading without date
                "MT-P1;D;D;HOURS;h;;01/01/2026;",   // 6 date without reading
                "MT-P1;E;E;HOURS;h;-3;01/01/2026;", // 7 negative reading
                "MT-P1;F;F;HOURS;h;5;01/01/2999;",  // 8 future date
                "MT-P1;G;G;HOURS;h;;;peut-être",    // 9 bad yes/no
                "MT-P1;g;G bis;HOURS;h;;;",         // 10 same code as row 9 (case aside)
                "MT-P1;;Sans code;HOURS;h;;;",      // 11 no meter code
                "MT-P1;H;;;;;;",                    // 12 no label, type, unit
            ],
        )
        .await;
        assert_eq!(codes(&p, 2, "error"), ["METER_IMPORT_EQUIPMENT"]);
        assert_eq!(codes(&p, 3, "error"), ["METER_IMPORT_TYPE"]);
        assert_eq!(codes(&p, 4, "error"), ["METER_IMPORT_UNIT"]);
        assert_eq!(codes(&p, 5, "error"), ["METER_IMPORT_DATE"]);
        assert_eq!(codes(&p, 6, "error"), ["METER_IMPORT_READING"]);
        assert_eq!(codes(&p, 7, "error"), ["METER_IMPORT_READING"]);
        assert_eq!(codes(&p, 8, "error"), ["METER_IMPORT_DATE_FUTURE"]);
        assert_eq!(codes(&p, 9, "error"), ["METER_IMPORT_PRIMARY"]);
        assert_eq!(codes(&p, 10, "error"), ["METER_IMPORT_DUPLICATE"]);
        assert_eq!(codes(&p, 11, "error"), ["METER_IMPORT_CODE"]);
        assert_eq!(
            codes(&p, 12, "error"),
            ["METER_IMPORT_NAME", "METER_IMPORT_TYPE", "METER_IMPORT_UNIT"]
        );
    }

    #[tokio::test]
    async fn one_primary_meter_per_equipment_never_switched_silently() {
        let db = setup().await;
        let p = run(
            &db,
            &["MT-P1;HRS;Heures;HOURS;h;;;oui", "MT-P1;CYC;Cycles;CYCLES;h;;;oui"],
        )
        .await;
        assert!(codes(&p, 2, "error").is_empty());
        assert_eq!(codes(&p, 3, "error"), ["METER_IMPORT_PRIMARY_TAKEN"]);
        apply(&db, &p).await;

        let p = run(
            &db,
            &["MT-P1;CYC;Cycles;CYCLES;h;;;oui", "MT-P1;HRS;Heures;HOURS;h;;;non"],
        )
        .await;
        assert_eq!(codes(&p, 2, "error"), ["METER_IMPORT_PRIMARY_TAKEN"]);
        assert_eq!(codes(&p, 3, "error"), ["METER_IMPORT_PRIMARY_REMOVE"]);
    }

    #[tokio::test]
    async fn an_existing_meter_keeps_type_unit_and_readings_once_read() {
        let db = setup().await;
        let p = run(&db, &["MT-P1;HRS;Heures;HOURS;h;100;01/01/2026;"]).await;
        apply(&db, &p).await;

        // Label change: an update; the reading is ignored (already read).
        let p = run(&db, &["MT-P1;hrs;Heures de marche;HOURS;h;500;01/02/2026;"]).await;
        assert_eq!(action(&p, 2), "update", "{:?}", p.rows);
        assert_eq!(codes(&p, 2, "warning"), ["METER_IMPORT_ALREADY_READ"]);
        let r = apply(&db, &p).await;
        assert_eq!((r.updated, r.failed), (1, 0));
        assert_eq!(
            text(&db, "SELECT name FROM equipment_meters WHERE meter_code = 'HRS'")
                .await
                .as_deref(),
            Some("Heures de marche")
        );
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM asset_meter_readings").await,
            1,
            "the file's second reading is not recorded"
        );

        let p = run(&db, &["MT-P1;HRS;Heures de marche;CYCLES;h;;;"]).await;
        assert_eq!(codes(&p, 2, "error"), ["METER_IMPORT_TYPE_LOCKED"]);
    }

    #[tokio::test]
    async fn a_reading_recorded_elsewhere_since_the_check_fails_only_its_line() {
        let db = setup().await;
        let p = run(&db, &["MT-P1;HRS;Heures;HOURS;h;;;"]).await;
        apply(&db, &p).await;
        // Checked while the meter has no reading...
        let p = run(
            &db,
            &["MT-P1;HRS;Heures;HOURS;h;50;01/01/2026;", "MT-P2;HRS;Heures;HOURS;h;;;"],
        )
        .await;
        assert_eq!(action(&p, 2), "update");
        // ...then a reading is entered on the screen before the batch is applied.
        let meter = scalar(&db, "SELECT id FROM equipment_meters WHERE meter_code = 'HRS'").await;
        exec(
            &db,
            &format!(
                "INSERT INTO asset_meter_readings (meter_id, reading_value, reading_at, source_type, quality_flag, \
                 created_by_id, created_at, reading_kind, lifetime_value, entity_sync_id) \
                 VALUES ({meter}, 60, '2026-01-02T00:00:00+00:00', 'MANUAL', 'accepted', 1, datetime('now'), \
                 'normal', 60, 'mt-r-1')"
            ),
        )
        .await;
        let r = apply(&db, &p).await;
        assert_eq!((r.created, r.failed), (1, 1));
        let count_sql = format!("SELECT COUNT(*) FROM asset_meter_readings WHERE meter_id = {meter}");
        assert_eq!(scalar(&db, &count_sql).await, 1);
    }
}
