//! Opening stock import: costed entry, audit, refusals, posted once, export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::stock_opening::{AUDIT_ACTION, OPENING_NOTE};
    use crate::bulk_import::{self, CreateInput};

    const HEADER: &str = "code article;code magasin;code emplacement;quantité;coût unitaire;date d'ouverture";
    const ARTICLES: &str =
        "code article;désignation;unité;famille;réf. fabricant;fabricant;code fournisseur principal;\
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

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("exec");
    }

    async fn int(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "n")
            .expect("n")
    }

    /// Articles P1, P2 (stocked), HS (non-stock), LOT (lot-tracked), OFF (inactive); the seeded
    /// store MAIN has the location MAIN-BIN, and BIN2 is added to it.
    async fn db_with_articles() -> DatabaseConnection {
        let db = setup().await;
        let mut csv = ARTICLES.to_string();
        for (code, stocking) in [
            ("P1", "stocké"),
            ("P2", "stocké"),
            ("HS", "hors stock"),
            ("LOT", "stocké"),
            ("OFF", "stocké"),
        ] {
            csv.push_str(&format!("\n{code};Article {code};U;;;;;;;;;;;{stocking};TVA_20;"));
        }
        let b = bulk_import::create_batch(&db, input("stock_article", &csv), Some(1))
            .await
            .expect("articles");
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("articles apply");
        assert_eq!((res.created, res.failed), (5, 0));
        exec(
            &db,
            "UPDATE articles SET requires_batch_tracking = 1 WHERE article_code = 'LOT'",
        )
        .await;
        exec(&db, "UPDATE articles SET is_active = 0 WHERE article_code = 'OFF'").await;
        let b = bulk_import::create_batch(
            &db,
            input(
                "stock_location",
                "code magasin;code emplacement;libellé\nMAIN;BIN2;Casier 2",
            ),
            Some(1),
        )
        .await
        .expect("location");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("location apply");
        db
    }

    /// Message codes of each line of the file, and the apply result.
    async fn run(db: &DatabaseConnection, csv: &str) -> (Vec<Vec<String>>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input("stock_opening", csv), Some(1))
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

    async fn on_hand(db: &DatabaseConnection, article: &str, location: &str) -> (f64, Option<f64>) {
        let r = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT b.on_hand_qty, b.moving_avg_unit_cost FROM stock_balances b \
                 JOIN articles a ON a.id = b.article_id JOIN stock_locations l ON l.id = b.location_id \
                 WHERE a.article_code = ? AND l.code = ?",
                [article.into(), location.into()],
            ))
            .await
            .expect("query")
            .expect("balance");
        (
            r.try_get("", "on_hand_qty").expect("qty"),
            r.try_get("", "moving_avg_unit_cost").expect("cost"),
        )
    }

    #[tokio::test]
    async fn posts_a_costed_entry_with_its_audit_and_only_once() {
        let db = db_with_articles().await;
        let file = format!("{HEADER}\nP1;MAIN;MAIN-BIN;10;2,5;\np2;main;bin2;4;0;15/01/2026\n");
        let (codes, res) = run(&db, &file).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        assert_eq!((res.created, res.failed), (2, 0));

        assert_eq!(on_hand(&db, "P1", "MAIN-BIN").await, (10.0, Some(2.5)));
        assert_eq!(on_hand(&db, "P2", "BIN2").await, (4.0, Some(0.0)));
        // One costed ADJUST_IN per line, with the reason and comment of an opening count.
        let movements = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT t.movement_type, t.unit_cost, t.reason_code, t.reason, t.posting_date \
                 FROM inventory_transactions t JOIN articles a ON a.id = t.article_id ORDER BY a.article_code"
                    .to_string(),
            ))
            .await
            .expect("movements");
        assert_eq!(movements.len(), 2);
        for m in &movements {
            assert_eq!(m.try_get::<String>("", "movement_type").expect("type"), "ADJUST_IN");
            assert_eq!(
                m.try_get::<Option<String>>("", "reason_code")
                    .expect("reason")
                    .as_deref(),
                Some("INVENTAIRE")
            );
            assert_eq!(
                m.try_get::<Option<String>>("", "reason")
                    .expect("reason text")
                    .as_deref(),
                Some(OPENING_NOTE)
            );
        }
        assert_eq!(
            movements[0].try_get::<Option<f64>>("", "unit_cost").expect("cost"),
            Some(2.5)
        );
        assert_eq!(
            movements[1]
                .try_get::<Option<String>>("", "posting_date")
                .expect("date")
                .as_deref(),
            Some("2026-01-15")
        );
        assert_eq!(
            int(
                &db,
                &format!(
                    "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_ACTION}' AND actor_id = 1"
                )
            )
            .await,
            2
        );

        // The same file again: every line is refused, the stock does not move.
        let (codes, res) = run(&db, &file).await;
        assert!(
            codes.iter().all(|c| c == &["OPENING_IMPORT_HAS_MOVEMENTS".to_string()]),
            "{codes:?}"
        );
        assert_eq!((res.created, res.failed), (0, 0));
        assert_eq!(on_hand(&db, "P1", "MAIN-BIN").await.0, 10.0);

        // The export is the stock on hand.
        let table = bulk_import::export(&db, "stock_opening").await.expect("export");
        assert_eq!(
            table.rows,
            vec![
                vec!["P1", "MAIN", "MAIN-BIN", "10", "2.5", ""],
                vec!["P2", "MAIN", "BIN2", "4", "0", ""],
            ]
        );
    }

    #[tokio::test]
    async fn refuses_lines_the_stock_screen_would_refuse() {
        let db = db_with_articles().await;
        exec(
            &db,
            "INSERT INTO inventory_valuation_periods \
             (entity_sync_id, period_code, period_start, period_end, currency_code, closed_at, created_at, updated_at) \
             VALUES ('vp-open-test', '2026-01', '2026-01-01', '2026-01-31', 'DZD', '2026-02-01', '2026-02-01', '2026-02-01')",
        )
        .await;
        let file = format!(
            "{HEADER}\n\
             NOPE;MAIN;MAIN-BIN;1;1;\n\
             P1;NOSTORE;MAIN-BIN;1;1;\n\
             P1;MAIN;NOBIN;1;1;\n\
             HS;MAIN;MAIN-BIN;1;1;\n\
             LOT;MAIN;MAIN-BIN;1;1;\n\
             OFF;MAIN;MAIN-BIN;1;1;\n\
             P1;MAIN;MAIN-BIN;0;1;\n\
             P1;MAIN;BIN2;2;-1;\n\
             P2;MAIN;MAIN-BIN;1;1;31/12/2099\n\
             P2;MAIN;BIN2;1;1;15/01/2026\n\
             P1;MAIN;MAIN-BIN;1;1;demain\n\
             P2;MAIN;MAIN-BIN;1;1;\n\
             p2;main;main-bin;1;1;\n\
             ;MAIN;MAIN-BIN;1;1;\n"
        );
        let (codes, res) = run(&db, &file).await;
        let expected: Vec<Vec<&str>> = vec![
            vec!["OPENING_IMPORT_ARTICLE_UNKNOWN"],
            vec!["OPENING_IMPORT_STORE_UNKNOWN"],
            vec!["OPENING_IMPORT_LOCATION_UNKNOWN"],
            vec!["OPENING_IMPORT_ARTICLE_NON_STOCK"],
            vec!["OPENING_IMPORT_ARTICLE_TRACKED"],
            vec!["OPENING_IMPORT_ARTICLE_INACTIVE"],
            vec!["OPENING_IMPORT_QUANTITY"],
            vec!["OPENING_IMPORT_COST"],
            vec!["OPENING_IMPORT_DATE_FUTURE"],
            vec!["OPENING_IMPORT_DATE_CLOSED"],
            vec!["OPENING_IMPORT_DATE"],
            vec![],
            vec!["OPENING_IMPORT_DUPLICATE"],
            vec!["OPENING_IMPORT_ARTICLE"],
        ];
        assert_eq!(codes, expected);
        // Only the one clean line is posted.
        assert_eq!((res.created, res.failed), (1, 0));
        assert_eq!(int(&db, "SELECT COUNT(*) AS n FROM inventory_transactions").await, 1);
    }

    #[tokio::test]
    async fn a_movement_made_after_the_preview_fails_the_line_at_apply() {
        let db = db_with_articles().await;
        let b = bulk_import::create_batch(
            &db,
            input("stock_opening", &format!("{HEADER}\nP1;MAIN;MAIN-BIN;10;3;\n")),
            Some(1),
        )
        .await
        .expect("batch");
        // Someone posts an entry from the stock screen before the import is applied.
        let article = int(&db, "SELECT id AS n FROM articles WHERE article_code = 'P1'").await;
        let location = int(&db, "SELECT id AS n FROM stock_locations WHERE code = 'MAIN-BIN'").await;
        crate::inventory::queries::adjust_stock(
            &db,
            crate::inventory::domain::InventoryStockAdjustInput {
                article_id: article,
                location_id: location,
                delta_qty: 2.0,
                reason_code: None,
                notes: None,
                source_ref: None,
            },
        )
        .await
        .expect("screen entry");
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (0, 1));
        assert_eq!(on_hand(&db, "P1", "MAIN-BIN").await.0, 2.0);
    }
}
