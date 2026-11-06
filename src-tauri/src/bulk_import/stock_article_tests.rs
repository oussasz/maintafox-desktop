//! Article import: preview rules, apply, supplier source, re-run and export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, CreateInput};

    // Columns: 1 code, 2 désignation, 3 unité, 4 famille, 5 réf. fabricant, 6 fabricant,
    // 7 fournisseur, 8 réf. fournisseur, 9 mini, 10 point de commande, 11 maxi, 12 quantité
    // économique, 13 criticité, 14 type de stockage, 15 catégorie fiscale, 16 actif.
    const HEADER: &str = "code article;désignation;unité;famille;réf. fabricant;fabricant;code fournisseur principal;\
                          réf. fournisseur;stock mini;point de commande;stock maxi;quantité économique;criticité;\
                          type de stockage;catégorie fiscale;actif";

    fn input(entity: &str, csv: &str) -> CreateInput {
        CreateInput {
            entity: entity.to_string(),
            filename: "import.csv".to_string(),
            source_sha256: "0".repeat(64),
            source_kind: "csv".to_string(),
            file_content: csv.as_bytes().to_vec(),
        }
    }

    /// One file line: `cells` holds columns 2 to 16 (fifteen cells).
    fn line(code: &str, cells: &str) -> String {
        assert_eq!(
            cells.matches(';').count(),
            14,
            "line {code} must have 15 cells after the code"
        );
        format!("{code};{cells}")
    }

    async fn run(db: &DatabaseConnection, csv: &str) -> (Vec<Vec<String>>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input("stock_article", csv), Some(1))
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

    async fn all_skip(db: &DatabaseConnection, csv: &str) -> usize {
        let b = bulk_import::create_batch(db, input("stock_article", csv), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        let not_skipped: Vec<_> = p
            .rows
            .iter()
            .filter(|r| r.action != "skip" || r.status != "ok")
            .map(|r| (r.row_no, r.action.clone(), r.messages.clone()))
            .collect();
        assert!(not_skipped.is_empty(), "{not_skipped:?}");
        p.rows.len()
    }

    /// Suppliers F1, F2 (active), FOFF (inactive), and families ROUL (active), OLD (inactive).
    async fn db_with_lists() -> DatabaseConnection {
        let db = setup().await;
        let suppliers = "code fournisseur;raison sociale;statut;actif\n\
                         F1;Roulements SA;approuvé;\nF2;Hydro SARL;approuvé;\nFOFF;Fermé;approuvé;non\n";
        let b = bulk_import::create_batch(&db, input("stock_supplier", suppliers), Some(1))
            .await
            .expect("suppliers");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("suppliers apply");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO article_families (code, name, is_active) VALUES ('ROUL', 'Roulements', 1), ('OLD', 'Ancienne', 0)"
                .to_string(),
        ))
        .await
        .expect("families");
        db
    }

    type ArticleRow = (String, f64, f64, Option<f64>, Option<f64>, i64, Option<String>);

    /// (name, min, reorder point, max, economic qty, active, manufacturer) of an article.
    async fn article(db: &DatabaseConnection, code: &str) -> ArticleRow {
        let r = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT article_name, min_stock, reorder_point, max_stock, economic_order_qty, is_active, manufacturer_name \
                 FROM articles WHERE article_code = ?",
                [code.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        (
            r.try_get("", "article_name").expect("name"),
            r.try_get("", "min_stock").expect("min"),
            r.try_get("", "reorder_point").expect("rop"),
            r.try_get("", "max_stock").expect("max"),
            r.try_get("", "economic_order_qty").expect("eoq"),
            r.try_get("", "is_active").expect("active"),
            r.try_get("", "manufacturer_name").expect("manufacturer"),
        )
    }

    /// Preferred supplier code and reference of an article.
    async fn preferred(db: &DatabaseConnection, code: &str) -> Vec<(String, Option<String>)> {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT s.code, sas.supplier_article_code FROM inventory_supplier_article_sources sas \
             JOIN inventory_suppliers s ON s.id = sas.supplier_id JOIN articles a ON a.id = sas.article_id \
             WHERE a.article_code = ? AND sas.is_preferred = 1",
            [code.into()],
        ))
        .await
        .expect("query")
        .iter()
        .map(|r| {
            (
                r.try_get("", "code").expect("code"),
                r.try_get("", "supplier_article_code").expect("ref"),
            )
        })
        .collect()
    }

    #[tokio::test]
    async fn articles_are_created_updated_and_rerun_is_a_no_op() {
        let db = db_with_lists().await;
        let first = format!(
            "{HEADER}\n{}\n{}\n",
            line(
                "roul-6205",
                "Roulement 6205;U;ROUL;6205-2RS;SKF;F1;SKF-6205;2;4;10;5;;stocké;TVA_20;"
            ),
            line("GRAISSE", "Graisse EP2;kg;;;;;;;;;;;hors stock;TVA 20%;"),
        );
        let (codes, res) = run(&db, &first).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        assert_eq!((res.created, res.failed), (2, 0));
        // The code is stored in capitals, as on the form.
        assert_eq!(
            article(&db, "ROUL-6205").await,
            (
                "Roulement 6205".into(),
                2.0,
                4.0,
                Some(10.0),
                Some(5.0),
                1,
                Some("SKF".into())
            )
        );
        assert_eq!(
            preferred(&db, "ROUL-6205").await,
            vec![("F1".into(), Some("SKF-6205".into()))]
        );
        assert_eq!(article(&db, "GRAISSE").await.1, 0.0);

        // Same file again: nothing to do.
        all_skip(&db, &first).await;

        // Blank cells keep the current values; thresholds are checked on the final values.
        let (codes, res) = run(
            &db,
            &format!(
                "{HEADER}\n{}\n",
                line("ROUL-6205", "Roulement 6205 2RS;;;;;;;;6;;;;;;non")
            ),
        )
        .await;
        assert!(codes[0].is_empty(), "{codes:?}");
        assert_eq!((res.updated, res.failed), (1, 0));
        assert_eq!(
            article(&db, "ROUL-6205").await,
            (
                "Roulement 6205 2RS".into(),
                2.0,
                6.0,
                Some(10.0),
                Some(5.0),
                0,
                Some("SKF".into())
            )
        );
        assert_eq!(
            preferred(&db, "ROUL-6205").await,
            vec![("F1".into(), Some("SKF-6205".into()))]
        );

        // Export re-imports as "nothing to do".
        let table = bulk_import::export(&db, "stock_article").await.expect("export");
        let mut csv = table.header.join(";");
        for row in &table.rows {
            csv.push('\n');
            csv.push_str(&row.join(";"));
        }
        assert_eq!(all_skip(&db, &csv).await, table.rows.len());
    }

    #[tokio::test]
    async fn bad_article_lines_are_refused_with_a_named_reason() {
        let db = db_with_lists().await;
        let lines = [
            line("OK", "Bon;U;;;;;;;;;;;stocké;TVA_20;"),
            line("NOUNIT", "Sans unité;;;;;;;;;;;;stocké;TVA_20;"),
            line("BADUNIT", "X;bidule;;;;;;;;;;;stocké;TVA_20;"),
            line("BADFAM", "X;U;NOPE;;;;;;;;;;stocké;TVA_20;"),
            line("OLDFAM", "X;U;OLD;;;;;;;;;;stocké;TVA_20;"),
            line("BADROP", "X;U;;;;;;5;2;;;;stocké;TVA_20;"),
            line("BADMAX", "X;U;;;;;;1;2;1;;;stocké;TVA_20;"),
            line("REFONLY", "X;U;;;;;REF-1;;;;;;stocké;TVA_20;"),
            line("SUPOFF", "X;U;;;;FOFF;;;;;;;stocké;TVA_20;"),
            line("BADTYPE", "X;U;;;;;;;;;;;parfois;TVA_20;"),
            line("NEWOFF", "X;U;;;;;;;;;;;stocké;TVA_20;non"),
            line("ok", "Doublon;U;;;;;;;;;;;stocké;TVA_20;"),
        ];
        let csv = format!("{HEADER}\n{}\n", lines.join("\n"));
        let (codes, res) = run(&db, &csv).await;
        assert!(codes[0].is_empty(), "{codes:?}");
        assert_eq!(codes[1], vec!["ARTICLE_IMPORT_REQUIRED"]);
        assert_eq!(codes[2], vec!["ARTICLE_IMPORT_UNIT"]);
        assert_eq!(codes[3], vec!["ARTICLE_IMPORT_FAMILY"]);
        assert_eq!(codes[4], vec!["ARTICLE_IMPORT_FAMILY_INACTIVE"]);
        assert_eq!(codes[5], vec!["ARTICLE_IMPORT_THRESHOLDS"]);
        assert_eq!(codes[6], vec!["ARTICLE_IMPORT_THRESHOLDS"]);
        assert_eq!(codes[7], vec!["ARTICLE_IMPORT_SUPPLIER_REF"]);
        assert_eq!(codes[8], vec!["ARTICLE_IMPORT_SUPPLIER_INACTIVE"]);
        assert_eq!(codes[9], vec!["ARTICLE_IMPORT_STOCKING"]);
        assert_eq!(codes[10], vec!["ARTICLE_IMPORT_NEW_INACTIVE"]);
        assert_eq!(codes[11], vec!["ARTICLE_IMPORT_DUPLICATE"]);
        assert_eq!((res.created, res.failed), (1, 0));
    }

    #[tokio::test]
    async fn a_new_preferred_supplier_is_shown_and_replaces_the_old_one() {
        let db = db_with_lists().await;
        run(
            &db,
            &format!("{HEADER}\n{}\n", line("A1", "Article;U;;;;F1;R-1;;;;;;stocké;TVA_20;")),
        )
        .await;
        let b = bulk_import::create_batch(
            &db,
            input(
                "stock_article",
                &format!("{HEADER}\n{}\n", line("A1", ";;;;;F2;;;;;;;;;")),
            ),
            Some(1),
        )
        .await
        .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows[0].action, "update");
        assert_eq!(p.rows[0].status, "warning");
        assert_eq!(p.rows[0].messages[0].code, "ARTICLE_IMPORT_PREFERRED_CHANGE");
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.updated, res.failed), (1, 0));
        assert_eq!(preferred(&db, "A1").await, vec![("F2".into(), None)]);
    }

    #[tokio::test]
    async fn the_unit_of_an_article_with_movements_is_not_changed() {
        let db = db_with_lists().await;
        run(
            &db,
            &format!("{HEADER}\n{}\n", line("A1", "Article;U;;;;;;;;;;;stocké;TVA_20;")),
        )
        .await;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO inventory_transactions \
               (entity_sync_id, article_id, warehouse_id, location_id, movement_type, quantity, source_type) \
             SELECT 'tx-unit-test', a.id, l.warehouse_id, l.id, 'ADJUST_IN', 1, 'MANUAL_ADJUSTMENT' \
             FROM articles a, stock_locations l WHERE a.article_code = 'A1' LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("movement");
        let (codes, res) = run(&db, &format!("{HEADER}\n{}\n", line("A1", ";kg;;;;;;;;;;;;;"))).await;
        assert_eq!(codes[0], vec!["ARTICLE_IMPORT_UNIT_LOCKED"]);
        assert_eq!((res.updated, res.failed), (0, 0));
    }
}
