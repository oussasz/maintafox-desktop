//! Cost centre import: preview rules and apply.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, CreateInput};

    const HEADER: &str = "code;libellé;code site;code parent;matricule responsable;actif";

    fn input(entity: &str, csv: &str) -> CreateInput {
        CreateInput {
            entity: entity.to_string(),
            filename: "import.csv".to_string(),
            source_sha256: "0".repeat(64),
            source_kind: "csv".to_string(),
            file_content: csv.as_bytes().to_vec(),
        }
    }

    /// Organisation SITE S1 > ZONE Z1, loaded through the organisation import itself.
    async fn db_with_sites() -> DatabaseConnection {
        let db = setup().await;
        let org = "code;libellé;type de nœud;code parent\nS1;Usine;SITE;\nZ1;Zone A;ZONE;S1\n";
        let b = bulk_import::create_batch(&db, input("org_tree", org), Some(1))
            .await
            .expect("org batch");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("org apply");
        db
    }

    async fn parent_code_of(db: &DatabaseConnection, code: &str) -> Option<String> {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.code AS pc FROM cost_centers c LEFT JOIN cost_centers p ON p.id = c.parent_cost_center_id \
             WHERE c.code = ?",
            [code.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get::<Option<String>>("", "pc")
        .expect("pc")
    }

    #[tokio::test]
    async fn children_listed_before_their_parents_are_created_in_order() {
        let db = db_with_sites().await;
        let csv = format!("{HEADER}\nCC2;Mécanique;Z1;CC1;;\nCC1;Maintenance;S1;;;\n");
        let b = bulk_import::create_batch(&db, input("cost_centre", &csv), Some(1))
            .await
            .expect("batch");
        assert_eq!(b.error_rows, 0);
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (2, 0));
        assert_eq!(parent_code_of(&db, "CC2").await.as_deref(), Some("CC1"));
        assert_eq!(parent_code_of(&db, "CC1").await, None);
    }

    #[tokio::test]
    async fn bad_lines_are_refused_with_a_named_reason() {
        let db = db_with_sites().await;
        let csv = format!(
            "{HEADER}\n\
             OK1;Racine;S1;;;\n\
             BADSITE;X;NOPE;;;\n\
             NOSITE;X;;;;\n\
             BADPAR;X;S1;GHOST;;\n\
             LOOPA;A;S1;LOOPB;;\n\
             LOOPB;B;S1;LOOPA;;\n\
             BADOWN;X;S1;;M999;\n\
             OK1;Doublon;S1;;;\n\
             INACT;Arrêté;S1;;;non\n\
             UNDERINACT;Enfant;S1;INACT;;oui\n\
             UNDERBAD;Enfant;S1;BADSITE;;\n"
        );
        let b = bulk_import::create_batch(&db, input("cost_centre", &csv), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        let codes: Vec<Vec<String>> = p
            .rows
            .iter()
            .map(|r| r.messages.iter().map(|m| m.code.clone()).collect())
            .collect();
        assert!(codes[0].is_empty());
        assert_eq!(codes[1], vec!["CC_IMPORT_SITE"]);
        assert_eq!(codes[2], vec!["CC_IMPORT_SITE"]);
        assert_eq!(codes[3], vec!["CC_IMPORT_PARENT_UNKNOWN"]);
        assert_eq!(codes[4], vec!["CC_IMPORT_CYCLE"]);
        assert_eq!(codes[5], vec!["CC_IMPORT_CYCLE"]);
        assert_eq!(codes[6], vec!["CC_IMPORT_OWNER_UNKNOWN"]);
        assert_eq!(codes[7], vec!["CC_IMPORT_DUPLICATE"]);
        assert!(codes[8].is_empty());
        assert_eq!(codes[9], vec!["CC_IMPORT_PARENT_INACTIVE"]);
        assert_eq!(codes[10], vec!["CC_IMPORT_PARENT_REFUSED"]);
        assert_eq!(b.error_rows, 9);
        // Only the two good lines are written.
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (2, 0));
    }

    #[tokio::test]
    async fn existing_centre_is_updated_a_rerun_changes_nothing_and_export_round_trips() {
        let db = db_with_sites().await;
        let first = format!("{HEADER}\nCC1;Maintenance;S1;;;\nCC2;Mécanique;Z1;CC1;;\n");
        let b1 = bulk_import::create_batch(&db, input("cost_centre", &first), Some(1))
            .await
            .expect("batch 1");
        bulk_import::apply_batch(&db, b1.id, 1).await.expect("apply 1");

        // The same file again: nothing to do.
        let again = bulk_import::create_batch(&db, input("cost_centre", &first), Some(1))
            .await
            .expect("again");
        let p = bulk_import::get_preview(&db, again.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));

        // Rename one, deactivate the other; blank cells leave the other columns alone.
        let edit = format!("{HEADER}\nCC1;Maintenance générale;;;;\nCC2;;;;;non\n");
        let b2 = bulk_import::create_batch(&db, input("cost_centre", &edit), Some(1))
            .await
            .expect("batch 2");
        let p = bulk_import::get_preview(&db, b2.id).await.expect("preview 2");
        assert!(p.rows.iter().all(|r| r.action == "update" && r.status == "ok"));
        let res = bulk_import::apply_batch(&db, b2.id, 1).await.expect("apply 2");
        assert_eq!((res.updated, res.failed), (2, 0));
        assert_eq!(parent_code_of(&db, "CC2").await.as_deref(), Some("CC1"));
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name, is_active FROM cost_centers WHERE code = 'CC2'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "name").unwrap(), "Mécanique");
        assert_eq!(row.try_get::<i64>("", "is_active").unwrap(), 0);

        // Template and export share one header, and the exported rows load again unchanged.
        let template = bulk_import::template("cost_centre").expect("template");
        let exported = bulk_import::export(&db, "cost_centre").await.expect("export");
        assert_eq!(template.header, exported.header);
        assert_eq!(exported.rows.len(), 2);
        let mut csv = exported.header.join(";");
        for r in &exported.rows {
            csv.push('\n');
            csv.push_str(&r.join(";"));
        }
        let round = bulk_import::create_batch(&db, input("cost_centre", &csv), Some(1))
            .await
            .expect("re-import");
        let p = bulk_import::get_preview(&db, round.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));
    }
}
