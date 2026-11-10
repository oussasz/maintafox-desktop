//! Supplier import: preview rules, apply, re-run and export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, CreateInput};

    const HEADER: &str = "code fournisseur;raison sociale;statut;délai habituel (jours);actif";

    fn input(csv: &str) -> CreateInput {
        CreateInput {
            entity: "stock_supplier".to_string(),
            filename: "import.csv".to_string(),
            source_sha256: "0".repeat(64),
            source_kind: "csv".to_string(),
            file_content: csv.as_bytes().to_vec(),
        }
    }

    async fn run(db: &DatabaseConnection, csv: &str) -> (Vec<Vec<String>>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input(csv), Some(1)).await.expect("batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        let codes = p
            .rows
            .iter()
            .map(|r| r.messages.iter().map(|m| m.code.clone()).collect())
            .collect();
        let res = bulk_import::apply_batch(db, b.id, 1).await.expect("apply");
        (codes, res)
    }

    /// (name, status, lead time, active, payment terms) of a supplier.
    async fn supplier(db: &DatabaseConnection, code: &str) -> (String, String, Option<i64>, i64, Option<String>) {
        let r = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT name, status_code, default_lead_time_days, is_active, payment_terms_code \
                 FROM inventory_suppliers WHERE code = ?",
                [code.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        (
            r.try_get("", "name").expect("name"),
            r.try_get("", "status_code").expect("status"),
            r.try_get("", "default_lead_time_days").expect("lead"),
            r.try_get("", "is_active").expect("active"),
            r.try_get("", "payment_terms_code").expect("terms"),
        )
    }

    #[tokio::test]
    async fn suppliers_are_created_updated_and_other_fields_are_kept() {
        let db = setup().await;
        let first = format!("{HEADER}\nF1;Roulements SA;approuvé;10;\nF2;Hydro SARL;EN ÉVALUATION;;non\n");
        let (codes, res) = run(&db, &first).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        assert_eq!((res.created, res.failed), (2, 0));
        assert_eq!(
            supplier(&db, "F1").await,
            ("Roulements SA".into(), "APPROVED".into(), Some(10), 1, None)
        );
        assert_eq!(
            supplier(&db, "F2").await,
            ("Hydro SARL".into(), "UNDER_EVALUATION".into(), None, 0, None)
        );

        // A field the file does not carry (payment terms, set on screen) survives an update.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE inventory_suppliers SET payment_terms_code = 'NET30' WHERE code = 'F1'".to_string(),
        ))
        .await
        .expect("terms");

        // Same file again: nothing to do.
        let b = bulk_import::create_batch(&db, input(&first), Some(1))
            .await
            .expect("again");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));

        // Letter case of the code does not matter; blank cells keep the current values.
        let (_, res) = run(&db, &format!("{HEADER}\nf1;;bloqué;;non\nF2;;;;oui\n")).await;
        assert_eq!((res.updated, res.failed), (2, 0));
        assert_eq!(
            supplier(&db, "F1").await,
            (
                "Roulements SA".into(),
                "BLOCKED".into(),
                Some(10),
                0,
                Some("NET30".into())
            )
        );
        assert_eq!(supplier(&db, "F2").await.3, 1);

        // Export re-imports as "nothing to do".
        let table = bulk_import::export(&db, "stock_supplier").await.expect("export");
        let mut csv = table.header.join(";");
        for row in &table.rows {
            csv.push('\n');
            csv.push_str(&row.join(";"));
        }
        let b = bulk_import::create_batch(&db, input(&csv), Some(1))
            .await
            .expect("export batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows.len(), 2);
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));
    }

    #[tokio::test]
    async fn bad_supplier_lines_are_refused_with_a_named_reason() {
        let db = setup().await;
        let csv = format!(
            "{HEADER}\n\
             OK;Bon;préféré;;\n\
             NONAME;;approuvé;;\n\
             NOSTATUS;Sans statut;;;\n\
             BADSTATUS;X;excellent;;\n\
             BADLEAD;X;approuvé;-2;\n\
             FRACLEAD;X;approuvé;2,5;\n\
             BADACT;X;approuvé;;peut-être\n\
             ok;Doublon;approuvé;;\n"
        );
        let (codes, res) = run(&db, &csv).await;
        assert!(codes[0].is_empty());
        assert_eq!(codes[1], vec!["SUPPLIER_IMPORT_NAME"]);
        assert_eq!(codes[2], vec!["SUPPLIER_IMPORT_STATUS"]);
        assert_eq!(codes[3], vec!["SUPPLIER_IMPORT_STATUS"]);
        assert_eq!(codes[4], vec!["SUPPLIER_IMPORT_LEAD_TIME"]);
        assert_eq!(codes[5], vec!["SUPPLIER_IMPORT_LEAD_TIME"]);
        assert_eq!(codes[6], vec!["SUPPLIER_IMPORT_ACTIVE"]);
        assert_eq!(codes[7], vec!["SUPPLIER_IMPORT_DUPLICATE"]);
        assert_eq!((res.created, res.failed), (1, 0));
    }

    #[tokio::test]
    async fn codes_differing_only_by_case_are_refused() {
        let db = setup().await;
        for code in ["DUP", "dup"] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO inventory_suppliers (code, name, status_code, is_active, row_version, created_at, updated_at) \
                 VALUES (?, 'X', 'APPROVED', 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [code.into()],
            ))
            .await
            .expect("insert");
        }
        let (codes, res) = run(&db, &format!("{HEADER}\nDup;Nouveau nom;;;\n")).await;
        assert_eq!(codes[0], vec!["SUPPLIER_IMPORT_CODE_AMBIGUOUS"]);
        assert_eq!((res.updated, res.failed), (0, 0));
    }
}
