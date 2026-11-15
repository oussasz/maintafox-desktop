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

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql);
    }

    /// A person with the given personnel number; `account` adds a user account (live or disabled).
    async fn person(db: &DatabaseConnection, code: &str, account: Option<bool>) -> Option<i64> {
        exec(
            db,
            &format!("INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('{code}', '{code}', 'available')"),
        )
        .await;
        let live = account?;
        exec(
            db,
            &format!(
                "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
                 force_password_change, personnel_id, created_at, updated_at) \
                 SELECT 'cc-{code}', 'u-{code}', '{code}', 'local', {}, 0, 0, id, datetime('now'), datetime('now') \
                 FROM personnel WHERE employee_code = '{code}'",
                i64::from(live)
            ),
        )
        .await;
        let r = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT id FROM user_accounts WHERE username = 'u-{code}'"),
            ))
            .await
            .unwrap()
            .unwrap();
        Some(r.try_get("", "id").unwrap())
    }

    async fn owner_of(db: &DatabaseConnection, code: &str) -> Option<i64> {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT budget_owner_id FROM cost_centers WHERE code = ?",
            [code.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "budget_owner_id")
        .unwrap()
    }

    async fn active_of(db: &DatabaseConnection, code: &str) -> bool {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active FROM cost_centers WHERE code = ?",
            [code.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "is_active")
        .unwrap()
            != 0
    }

    async fn message_codes(db: &DatabaseConnection, csv: &str) -> (i64, Vec<Vec<String>>) {
        let b = bulk_import::create_batch(db, input("cost_centre", csv), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        (
            b.id,
            p.rows
                .iter()
                .map(|r| r.messages.iter().map(|m| m.code.clone()).collect())
                .collect(),
        )
    }

    async fn load(db: &DatabaseConnection, csv: &str) {
        let (id, codes) = message_codes(db, csv).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        let res = bulk_import::apply_batch(db, id, 1).await.expect("apply");
        assert_eq!(res.failed, 0);
    }

    #[tokio::test]
    async fn owner_is_the_live_account_of_the_person_and_round_trips_through_export() {
        let db = db_with_sites().await;
        let m1 = person(&db, "M1", Some(true)).await;
        person(&db, "M2", None).await;
        person(&db, "M3", Some(false)).await;
        let m4 = person(&db, "M4", Some(true)).await;

        let csv = format!("{HEADER}\nCC1;Maintenance;S1;;M1;\nCC2;Méthodes;S1;;M2;\nCC3;Achats;S1;;M3;\n");
        let (id, codes) = message_codes(&db, &csv).await;
        assert!(codes[0].is_empty());
        // No account, or only a disabled one: a warning, and the owner stays empty.
        assert_eq!(codes[1], vec!["CC_IMPORT_OWNER_NO_ACCOUNT"]);
        assert_eq!(codes[2], vec!["CC_IMPORT_OWNER_NO_ACCOUNT"]);
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (3, 0));
        assert_eq!(owner_of(&db, "CC1").await, m1);
        assert_eq!(owner_of(&db, "CC2").await, None);
        assert_eq!(owner_of(&db, "CC3").await, None);

        // A new owner is seen as a change.
        let b = bulk_import::create_batch(&db, input("cost_centre", &format!("{HEADER}\nCC1;;;;M4;\n")), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows[0].action, "update");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!(owner_of(&db, "CC1").await, m4);

        // The export gives the personnel number back, and loads again with nothing to do.
        let exported = bulk_import::export(&db, "cost_centre").await.expect("export");
        let cc1 = exported.rows.iter().find(|r| r[0] == "CC1").expect("CC1 row");
        assert_eq!(cc1[4], "M4");
        let mut round = exported.header.join(";");
        for r in &exported.rows {
            round.push('\n');
            round.push_str(&r.join(";"));
        }
        let b = bulk_import::create_batch(&db, input("cost_centre", &round), Some(1))
            .await
            .expect("re-import");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip"));
    }

    #[tokio::test]
    async fn new_child_under_a_parent_the_file_reactivates_later_is_written() {
        let db = db_with_sites().await;
        load(&db, &format!("{HEADER}\nP;Parent;S1;;;non\n")).await;
        let (id, codes) = message_codes(&db, &format!("{HEADER}\nN;Nouveau;S1;P;;\nP;;;;;oui\n")).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.created, res.updated, res.failed), (1, 1, 0));
        assert_eq!(parent_code_of(&db, "N").await.as_deref(), Some("P"));
        assert!(active_of(&db, "P").await);
    }

    #[tokio::test]
    async fn a_chain_reactivated_from_the_bottom_up_is_written_at_any_depth() {
        let db = db_with_sites().await;
        load(
            &db,
            &format!("{HEADER}\nP;Racine;S1;;;non\nC;Milieu;S1;P;;non\nG;Bas;S1;C;;non\n"),
        )
        .await;
        let (id, codes) = message_codes(&db, &format!("{HEADER}\nG;;;;;oui\nC;;;;;oui\nP;;;;;oui\n")).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.updated, res.failed), (3, 0));
        for code in ["P", "C", "G"] {
            assert!(active_of(&db, code).await, "{code}");
        }
    }

    #[tokio::test]
    async fn a_blank_parent_cell_is_checked_against_the_current_parent() {
        let db = db_with_sites().await;
        load(&db, &format!("{HEADER}\nP;Racine;S1;;;non\nC;Enfant;S1;P;;non\n")).await;
        // Reactivating only the child: its current parent stays inactive.
        let (_, codes) = message_codes(&db, &format!("{HEADER}\nC;;;;;oui\n")).await;
        assert_eq!(codes[0], vec!["CC_IMPORT_PARENT_INACTIVE"]);
        // Reactivating both is fine.
        let (id, codes) = message_codes(&db, &format!("{HEADER}\nC;;;;;oui\nP;;;;;oui\n")).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.updated, res.failed), (2, 0));
    }

    #[tokio::test]
    async fn a_refused_line_counts_as_not_applied_for_the_lines_under_it() {
        let db = db_with_sites().await;
        load(&db, &format!("{HEADER}\nP;Inactif;S1;;;non\nX;Actif;S1;;;\n")).await;
        // P's reactivation is refused (bad site), so N cannot rely on it; X's line is refused, so a
        // child under X is refused too even though X exists.
        let csv = format!("{HEADER}\nP;;NOPE;;;oui\nN;Nouveau;S1;P;;\nX;;NOPE;;;\nY;Sous X;S1;X;;\n");
        let (id, codes) = message_codes(&db, &csv).await;
        assert_eq!(codes[0], vec!["CC_IMPORT_SITE"]);
        assert_eq!(codes[1], vec!["CC_IMPORT_PARENT_REFUSED"]);
        assert_eq!(codes[2], vec!["CC_IMPORT_SITE"]);
        assert_eq!(codes[3], vec!["CC_IMPORT_PARENT_REFUSED"]);
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.created, res.updated, res.failed), (0, 0, 0));
    }

    #[tokio::test]
    async fn a_refusal_reason_never_relies_on_a_change_that_is_itself_refused() {
        let db = db_with_sites().await;
        load(&db, &format!("{HEADER}\nP;Parent;S1;;;\nQ;Autre;S1;;;\n")).await;
        // Q is refused, so P's move under Q and its deactivation are refused: P stays active. N is
        // refused because its parent's line is refused, not because P would be inactive.
        let csv = format!("{HEADER}\nQ;;NOPE;;;\nP;;;Q;;non\nN;Nouveau;S1;P;;\n");
        let (id, codes) = message_codes(&db, &csv).await;
        assert_eq!(codes[0], vec!["CC_IMPORT_SITE"]);
        assert_eq!(codes[1], vec!["CC_IMPORT_PARENT_REFUSED"]);
        assert_eq!(codes[2], vec!["CC_IMPORT_PARENT_REFUSED"]);
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.created, res.updated), (0, 0));
        assert!(active_of(&db, "P").await);
    }

    #[tokio::test]
    async fn codes_that_differ_only_by_letter_case_are_refused() {
        let db = db_with_sites().await;
        exec(
            &db,
            "INSERT INTO cost_centers (code, name) VALUES ('CC-A', 'A'), ('cc-a', 'a')",
        )
        .await;
        let csv = format!("{HEADER}\nCC-A;Nouveau nom;;;;\nN;Nouveau;S1;Cc-A;;\n");
        let (id, codes) = message_codes(&db, &csv).await;
        assert_eq!(codes[0], vec!["CC_IMPORT_CODE_AMBIGUOUS"]);
        assert_eq!(codes[1], vec!["CC_IMPORT_CODE_AMBIGUOUS"]);
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.created, res.updated), (0, 0));
    }

    #[tokio::test]
    async fn a_reversed_branch_is_written_once_the_line_it_waits_for_is_in() {
        let db = db_with_sites().await;
        load(
            &db,
            &format!("{HEADER}\nR;Racine;S1;;;\nX;Milieu;S1;R;;\nY;Bas;S1;X;;\n"),
        )
        .await;
        // R > X > Y becomes R > Y > X: X can only move under Y once Y has left X.
        let (id, codes) = message_codes(&db, &format!("{HEADER}\nX;;;Y;;\nY;;;R;;\n")).await;
        assert!(codes.iter().all(Vec::is_empty), "{codes:?}");
        let res = bulk_import::apply_batch(&db, id, 1).await.expect("apply");
        assert_eq!((res.updated, res.failed), (2, 0));
        assert_eq!(parent_code_of(&db, "X").await.as_deref(), Some("Y"));
        assert_eq!(parent_code_of(&db, "Y").await.as_deref(), Some("R"));
    }

    #[tokio::test]
    async fn a_loop_across_two_lines_refuses_both() {
        let db = db_with_sites().await;
        let (_, codes) = message_codes(&db, &format!("{HEADER}\nA;A;S1;B;;\nB;B;S1;A;;\n")).await;
        assert_eq!(codes[0], vec!["CC_IMPORT_CYCLE"]);
        assert_eq!(codes[1], vec!["CC_IMPORT_CYCLE"]);
    }
}
