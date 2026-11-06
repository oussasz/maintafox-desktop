//! Store and location imports: preview rules, apply, re-run and export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, CreateInput};

    const STORES: &str = "code magasin;libellé;actif";
    const LOCS: &str = "code magasin;code emplacement;libellé;actif";

    fn input(entity: &str, csv: &str) -> CreateInput {
        CreateInput {
            entity: entity.to_string(),
            filename: "import.csv".to_string(),
            source_sha256: "0".repeat(64),
            source_kind: "csv".to_string(),
            file_content: csv.as_bytes().to_vec(),
        }
    }

    async fn run(db: &DatabaseConnection, entity: &str, csv: &str) -> (Vec<Vec<String>>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input(entity, csv), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        let codes = p
            .rows
            .iter()
            .map(|r| r.messages.iter().map(|m| m.code.clone()).collect())
            .collect();
        let res = bulk_import::apply_batch(db, b.id, 1).await.expect("apply");
        (codes, res)
    }

    async fn one(db: &DatabaseConnection, sql: &str) -> (String, i64) {
        let r = db
            .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .expect("row");
        (
            r.try_get("", "name").expect("name"),
            r.try_get("", "is_active").expect("active"),
        )
    }

    async fn export_reimports_as_nothing_to_do(db: &DatabaseConnection, entity: &str, expected_rows: usize) {
        let table = bulk_import::export(db, entity).await.expect("export");
        let mut csv = table.header.join(";");
        for row in &table.rows {
            csv.push('\n');
            csv.push_str(&row.join(";"));
        }
        let b = bulk_import::create_batch(db, input(entity, &csv), Some(1))
            .await
            .expect("export batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        assert_eq!(p.rows.len(), expected_rows);
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));
    }

    #[tokio::test]
    async fn stores_are_created_renamed_deactivated_and_rerun_is_a_no_op() {
        let db = setup().await;
        let first = format!("{STORES}\nMAG1;Magasin central;\nMAG2;Annexe;oui\n");
        let (codes, res) = run(&db, "stock_store", &first).await;
        assert!(codes.iter().all(Vec::is_empty));
        assert_eq!((res.created, res.failed), (2, 0));

        // Same file again: nothing to do.
        let again = bulk_import::create_batch(&db, input("stock_store", &first), Some(1))
            .await
            .expect("again");
        let p = bulk_import::get_preview(&db, again.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));

        // Letter case of the code does not matter; a blank name keeps the current one.
        let (_, res) = run(&db, "stock_store", &format!("{STORES}\nmag1;Central;\nMAG2;;non\n")).await;
        assert_eq!((res.updated, res.failed), (2, 0));
        assert_eq!(
            one(&db, "SELECT name, is_active FROM warehouses WHERE code = 'MAG1'").await,
            ("Central".to_string(), 1)
        );
        assert_eq!(
            one(&db, "SELECT name, is_active FROM warehouses WHERE code = 'MAG2'").await,
            ("Annexe".to_string(), 0)
        );

        export_reimports_as_nothing_to_do(&db, "stock_store", 2).await;
    }

    #[tokio::test]
    async fn bad_store_lines_are_refused_with_a_named_reason() {
        let db = setup().await;
        let csv = format!(
            "{STORES}\n\
             OK;Bon;\n\
             NONAME;;\n\
             NEWOFF;Arrêté;non\n\
             BADACT;X;peut-être\n\
             ok;Doublon;\n"
        );
        let (codes, res) = run(&db, "stock_store", &csv).await;
        assert!(codes[0].is_empty());
        assert_eq!(codes[1], vec!["STORE_IMPORT_NAME"]);
        assert_eq!(codes[2], vec!["STORE_IMPORT_NEW_INACTIVE"]);
        assert_eq!(codes[3], vec!["STORE_IMPORT_ACTIVE"]);
        assert_eq!(codes[4], vec!["STORE_IMPORT_DUPLICATE"]);
        assert_eq!((res.created, res.failed), (1, 0));
    }

    #[tokio::test]
    async fn locations_are_keyed_by_store_and_code() {
        let db = setup().await;
        run(
            &db,
            "stock_store",
            &format!("{STORES}\nMAG1;Central;\nMAG2;Annexe;\nOFF;Fermé;\n"),
        )
        .await;
        run(&db, "stock_store", &format!("{STORES}\nOFF;;non\n")).await;

        // The same location code in two stores is two locations.
        let csv = format!(
            "{LOCS}\n\
             MAG1;A-01;Allée A;\n\
             MAG2;A-01;Allée A annexe;\n\
             GHOST;A-01;X;\n\
             OFF;A-01;X;\n\
             MAG1;B-01;;\n\
             MAG1;C-01;Fermé;non\n\
             mag1;a-01;Doublon;\n"
        );
        let (codes, res) = run(&db, "stock_location", &csv).await;
        assert!(codes[0].is_empty());
        assert!(codes[1].is_empty());
        assert_eq!(codes[2], vec!["LOCATION_IMPORT_STORE_UNKNOWN"]);
        assert_eq!(codes[3], vec!["LOCATION_IMPORT_STORE_INACTIVE"]);
        assert_eq!(codes[4], vec!["LOCATION_IMPORT_NAME"]);
        assert_eq!(codes[5], vec!["LOCATION_IMPORT_NEW_INACTIVE"]);
        assert_eq!(codes[6], vec!["LOCATION_IMPORT_DUPLICATE"]);
        assert_eq!((res.created, res.failed), (2, 0));

        // Rename and deactivate one; the other store's location of the same code is untouched.
        let (_, res) = run(&db, "stock_location", &format!("{LOCS}\nMAG1;a-01;Allée A1;non\n")).await;
        assert_eq!((res.updated, res.failed), (1, 0));
        assert_eq!(
            one(
                &db,
                "SELECT l.name, l.is_active FROM stock_locations l JOIN warehouses w ON w.id = l.warehouse_id \
                 WHERE w.code = 'MAG1' AND l.code = 'A-01'"
            )
            .await,
            ("Allée A1".to_string(), 0)
        );
        assert_eq!(
            one(
                &db,
                "SELECT l.name, l.is_active FROM stock_locations l JOIN warehouses w ON w.id = l.warehouse_id \
                 WHERE w.code = 'MAG2' AND l.code = 'A-01'"
            )
            .await,
            ("Allée A annexe".to_string(), 1)
        );

        export_reimports_as_nothing_to_do(&db, "stock_location", 2).await;
    }

    #[tokio::test]
    async fn a_store_created_elsewhere_after_the_preview_fails_that_line_only() {
        let db = setup().await;
        let b = bulk_import::create_batch(
            &db,
            input("stock_store", &format!("{STORES}\nMAG1;Central;\nMAG2;Annexe;\n")),
            Some(1),
        )
        .await
        .expect("batch");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO warehouses (entity_sync_id, code, name, is_active, row_version) \
             VALUES ('ws-test', 'MAG1', 'Ailleurs', 1, 1)"
                .to_string(),
        ))
        .await
        .expect("insert");
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (1, 1));
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows[0].apply_status.as_deref(), Some("failed"));
    }
}
