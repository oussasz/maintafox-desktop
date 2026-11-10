//! Spare-parts import: add, update the filled cells, never remove, refusals, export round trip.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, CreateInput};

    const HEADER: &str = "code équipement;code article;quantité;position;pièce critique;remarques";
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

    /// Equipment EQ1, EQ2; articles A1, A2 (active) and AOFF (inactive).
    async fn db_with_data() -> DatabaseConnection {
        let db = setup().await;
        for sql in [
            "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'sp-model', 1, 'active', datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
             created_at, updated_at) VALUES (1, 'sp-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
             updated_at, structure_model_id) VALUES (1, 'sp-node', 'SITE-SP', 'Site', 1, 'active', \
             datetime('now'), datetime('now'), 1)",
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('sp-eq-1', 'EQ1', 'Pompe 1', 'active_in_service', 1, \
             datetime('now'), datetime('now'))",
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('sp-eq-2', 'EQ2', 'Pompe 2', 'active_in_service', 1, \
             datetime('now'), datetime('now'))",
        ] {
            exec(&db, sql).await;
        }
        let mut csv = ARTICLES.to_string();
        for code in ["A1", "A2", "AOFF"] {
            csv.push_str(&format!("\n{code};Article {code};U;;;;;;;;;;;stocké;TVA_20;"));
        }
        let b = bulk_import::create_batch(&db, input("stock_article", &csv), Some(1))
            .await
            .expect("articles");
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("articles apply");
        assert_eq!((res.created, res.failed), (3, 0));
        exec(&db, "UPDATE articles SET is_active = 0 WHERE article_code = 'AOFF'").await;
        db
    }

    async fn run(db: &DatabaseConnection, csv: &str) -> (Vec<(String, Vec<String>)>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input("equipment_spare_part", csv), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        let rows = p
            .rows
            .iter()
            .map(|r| (r.action.clone(), r.messages.iter().map(|m| m.code.clone()).collect()))
            .collect();
        let res = bulk_import::apply_batch(db, b.id, 1).await.expect("apply");
        (rows, res)
    }

    /// (quantity, position, critical, active) of the line of an equipment and article.
    async fn line(db: &DatabaseConnection, eq: &str, article: &str) -> (f64, Option<String>, i64, i64) {
        let r = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT sp.quantity, sp.position_label, sp.is_critical_spare, sp.is_active FROM equipment_spare_parts sp \
                 JOIN equipment e ON e.id = sp.equipment_id JOIN articles a ON a.id = sp.article_id \
                 WHERE e.asset_id_code = ? AND a.article_code = ?",
                [eq.into(), article.into()],
            ))
            .await
            .expect("query")
            .expect("line");
        (
            r.try_get("", "quantity").expect("qty"),
            r.try_get("", "position_label").expect("position"),
            r.try_get("", "is_critical_spare").expect("critical"),
            r.try_get("", "is_active").expect("active"),
        )
    }

    #[tokio::test]
    async fn adds_updates_filled_cells_and_never_removes() {
        let db = db_with_data().await;
        let (rows, res) = run(&db, &format!("{HEADER}\nEQ1;A1;2;côté moteur;oui;\neq1;a2;1;;;\n")).await;
        assert!(rows.iter().all(|(a, m)| a == "create" && m.is_empty()), "{rows:?}");
        assert_eq!((res.created, res.failed), (2, 0));
        assert_eq!(
            line(&db, "EQ1", "A1").await,
            (2.0, Some("côté moteur".to_string()), 1, 1)
        );
        assert_eq!(line(&db, "EQ1", "A2").await, (1.0, None, 0, 1));

        // Only the quantity is filled: position and critical flag keep their values; A2, absent
        // from the file, stays on the equipment.
        let (rows, res) = run(&db, &format!("{HEADER}\nEQ1;A1;4;;;\n")).await;
        assert_eq!(rows, vec![("update".to_string(), vec![])]);
        assert_eq!((res.updated, res.failed), (1, 0));
        assert_eq!(
            line(&db, "EQ1", "A1").await,
            (4.0, Some("côté moteur".to_string()), 1, 1)
        );
        assert_eq!(line(&db, "EQ1", "A2").await.3, 1);

        // The export, imported again, changes nothing.
        let table = bulk_import::export(&db, "equipment_spare_part").await.expect("export");
        assert_eq!(
            table.rows,
            vec![
                vec!["EQ1", "A1", "4", "côté moteur", "oui", ""],
                vec!["EQ1", "A2", "1", "", "non", ""],
            ]
        );
        let mut csv = table.header.join(";");
        for r in &table.rows {
            csv.push('\n');
            csv.push_str(&r.join(";"));
        }
        let (rows, res) = run(&db, &csv).await;
        assert!(rows.iter().all(|(a, m)| a == "skip" && m.is_empty()), "{rows:?}");
        assert_eq!((res.created, res.updated, res.failed), (0, 0, 0));
    }

    #[tokio::test]
    async fn refuses_what_the_equipment_sheet_refuses() {
        let db = db_with_data().await;
        let long = "x".repeat(121);
        let file = format!(
            "{HEADER}\n\
             NOPE;A1;1;;;\n\
             EQ1;NOPE;1;;;\n\
             EQ1;AOFF;1;;;\n\
             EQ1;A1;0;;;\n\
             EQ2;A1;;;;\n\
             EQ2;A2;1;;peut-être;\n\
             EQ2;A2;1;;;\n\
             ;A1;1;;;\n\
             EQ1;A2;1;{long};;\n"
        );
        let (rows, res) = run(&db, &file).await;
        let codes: Vec<Vec<String>> = rows.into_iter().map(|(_, m)| m).collect();
        let expected: Vec<Vec<&str>> = vec![
            vec!["SPARE_IMPORT_EQUIPMENT_UNKNOWN"],
            vec!["SPARE_IMPORT_ARTICLE_UNKNOWN"],
            vec!["SPARE_IMPORT_ARTICLE_INACTIVE"],
            vec!["SPARE_IMPORT_QUANTITY"],
            vec!["SPARE_IMPORT_QUANTITY_REQUIRED"],
            vec!["SPARE_IMPORT_CRITICAL"],
            vec!["SPARE_IMPORT_DUPLICATE"],
            vec!["SPARE_IMPORT_EQUIPMENT"],
            vec!["SPARE_IMPORT_POSITION"],
        ];
        assert_eq!(codes, expected);
        assert_eq!((res.created, res.updated), (0, 0));
    }
}
