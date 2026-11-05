//! Crew and crew membership imports: preview rules and apply.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, parse_date, CreateInput};

    const TEAMS: &str = "code équipe;libellé;code site;type d'équipe;actif";
    const MEMBERS: &str = "matricule;code équipe;date début;date fin;équipe principale;chef d'équipe";

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
            .expect(sql);
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> Option<i64> {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql)
            .and_then(|r| r.try_get_by_index::<Option<i64>>(0).expect("value"))
    }

    async fn run(db: &DatabaseConnection, entity: &str, csv: &str) -> bulk_import::Preview {
        let b = bulk_import::create_batch(db, input(entity, csv), Some(1))
            .await
            .expect("batch");
        bulk_import::get_preview(db, b.id).await.expect("preview")
    }

    async fn apply(db: &DatabaseConnection, preview: &bulk_import::Preview) -> bulk_import::ApplyResult {
        bulk_import::apply_batch(db, preview.batch.id, 1).await.expect("apply")
    }

    /// Site S1 (loaded through the organisation import), crews IT-A and IT-B, people P1..P3.
    async fn db_with_crews() -> DatabaseConnection {
        let db = setup().await;
        let org = "code;libellé;type de nœud;code parent\nS1;Usine;SITE;\n";
        let p = run(&db, "org_tree", org).await;
        apply(&db, &p).await;
        let p = run(
            &db,
            "team",
            &format!("{TEAMS}\nIT-A;Mécanique;S1;maintenance;\nIT-B;Électricité;S1;Sous-traitant;\n"),
        )
        .await;
        assert_eq!(
            p.batch.error_rows,
            0,
            "{:?}",
            p.rows.iter().map(|r| &r.messages).collect::<Vec<_>>()
        );
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.failed), (2, 0));
        for code in ["P1", "P2", "P3"] {
            exec(
                &db,
                &format!(
                    "INSERT INTO personnel (employee_code, full_name, availability_status, employment_status) \
                     VALUES ('{code}', '{code}', 'available', 'active')"
                ),
            )
            .await;
        }
        db
    }

    async fn team_id(db: &DatabaseConnection, code: &str) -> i64 {
        scalar(db, &format!("SELECT id FROM teams WHERE code = '{code}'"))
            .await
            .expect("team")
    }

    async fn person_id(db: &DatabaseConnection, code: &str) -> i64 {
        scalar(db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'"))
            .await
            .expect("person")
    }

    async fn is_lead(db: &DatabaseConnection, person: &str, team: &str) -> bool {
        let p = person_id(db, person).await;
        let t = team_id(db, team).await;
        scalar(
            db,
            &format!(
                "SELECT MAX(is_lead) FROM personnel_team_assignments WHERE personnel_id = {p} AND team_id = {t} \
                 AND (valid_to IS NULL OR date(valid_to) >= date('now'))"
            ),
        )
        .await
            == Some(1)
    }

    fn errors(p: &bulk_import::Preview, row_no: i64) -> Vec<String> {
        p.rows
            .iter()
            .find(|r| r.row_no == row_no)
            .expect("row")
            .messages
            .iter()
            .filter(|m| m.severity == "error")
            .map(|m| m.code.clone())
            .collect()
    }

    fn action(p: &bulk_import::Preview, row_no: i64) -> String {
        p.rows.iter().find(|r| r.row_no == row_no).expect("row").action.clone()
    }

    /// The export of `entity`, kept to the lines whose column `col` starts with `IT-`, as a file.
    async fn exported(db: &DatabaseConnection, entity: &str, header: &str, col: usize) -> String {
        let export = bulk_import::export(db, entity).await.expect("export");
        let mut csv = format!("{header}\n");
        for row in export.rows.iter().filter(|r| r[col].starts_with("IT-")) {
            csv.push_str(&row.join(";"));
            csv.push('\n');
        }
        csv
    }

    #[test]
    fn dates_in_french_iso_and_excel_forms() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 3, 1);
        assert_eq!(parse_date(Some("01/03/2026")), Ok(d));
        assert_eq!(parse_date(Some("1/3/2026")), Ok(d));
        assert_eq!(parse_date(Some("2026-03-01")), Ok(d));
        assert_eq!(parse_date(Some("2026-03-01 08:30")), Ok(d));
        assert_eq!(parse_date(Some("46082")), Ok(d));
        assert_eq!(parse_date(Some("  ")), Ok(None));
        assert_eq!(parse_date(Some("mars 2026")), Err(()));
    }

    #[tokio::test]
    async fn crews_are_created_updated_and_round_trip() {
        let db = db_with_crews().await;
        let csv = format!(
            "{TEAMS}\nIT-A;Mécanique générale;;;non\nit-b;;;HSE;\nIT-C;Inspection;S1;contrôle;\nIT-D;;S1;inspection;\nIT-E;Inspection;NOPE;inspection;\n"
        );
        let p = run(&db, "team", &csv).await;
        assert_eq!(action(&p, 2), "update");
        assert_eq!(action(&p, 3), "update", "the letter case of the code does not matter");
        assert_eq!(errors(&p, 4), vec!["TEAM_IMPORT_TYPE"]);
        assert_eq!(errors(&p, 5), vec!["TEAM_IMPORT_NAME"]);
        assert_eq!(errors(&p, 6), vec!["TEAM_IMPORT_SITE"]);
        let res = apply(&db, &p).await;
        assert_eq!((res.updated, res.failed), (2, 0));
        let a = team_id(&db, "IT-A").await;
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT status, name FROM teams WHERE id = {a}"),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "status").unwrap(), "inactive");
        assert_eq!(row.try_get::<String>("", "name").unwrap(), "Mécanique générale");

        let again = run(&db, "team", &exported(&db, "team", TEAMS, 0).await).await;
        assert!(again.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));
    }

    #[tokio::test]
    async fn members_home_crew_and_lead_are_set() {
        let db = db_with_crews().await;
        let csv = format!("{MEMBERS}\nP1;IT-A;01/01/2026;;oui;oui\nP2;IT-A;01/01/2026;;;non\nP2;IT-B;;;;oui\n");
        let p = run(&db, "team_member", &csv).await;
        assert_eq!(
            p.batch.error_rows,
            0,
            "{:?}",
            p.rows.iter().map(|r| &r.messages).collect::<Vec<_>>()
        );
        assert!(p.rows.iter().all(|r| r.action == "create"));
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.failed), (3, 0));
        let a = team_id(&db, "IT-A").await;
        let p1 = person_id(&db, "P1").await;
        assert_eq!(
            scalar(&db, &format!("SELECT primary_team_id FROM personnel WHERE id = {p1}")).await,
            Some(a)
        );
        assert!(is_lead(&db, "P1", "IT-A").await);
        assert!(!is_lead(&db, "P2", "IT-A").await);
        assert!(is_lead(&db, "P2", "IT-B").await);

        // The export re-imports as "nothing to do".
        let again = run(&db, "team_member", &exported(&db, "team_member", MEMBERS, 1).await).await;
        assert!(
            again.rows.iter().all(|r| r.action == "skip" && r.status == "ok"),
            "{:?}",
            again.rows.iter().map(|r| (&r.action, &r.messages)).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn overlapping_periods_are_refused() {
        let db = db_with_crews().await;
        let p = run(
            &db,
            "team_member",
            &format!("{MEMBERS}\nP1;IT-A;01/01/2025;31/12/2025;;\n"),
        )
        .await;
        apply(&db, &p).await;
        let csv = format!(
            "{MEMBERS}\nP1;IT-A;01/06/2025;;;\nP1;IT-A;01/01/2026;30/06/2026;;\nP1;IT-A;01/03/2026;;;\nP1;IT-A;01/01/2025;30/06/2025;;\nP1;IT-A;01/07/2026;01/01/2026;;\n"
        );
        let p = run(&db, "team_member", &csv).await;
        assert_eq!(
            errors(&p, 2),
            vec!["MEMBER_IMPORT_OVERLAP"],
            "overlaps the stored 2025 period"
        );
        assert!(errors(&p, 3).is_empty());
        assert_eq!(errors(&p, 4), vec!["MEMBER_IMPORT_OVERLAP"], "overlaps line 3");
        assert!(errors(&p, 5).is_empty(), "shortens the stored period it matches");
        assert_eq!(action(&p, 5), "update");
        assert_eq!(errors(&p, 6), vec!["MEMBER_IMPORT_PERIOD"]);
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.updated, res.failed), (1, 1, 0));
    }

    #[tokio::test]
    async fn a_new_home_crew_conflicts_with_another_running_crew() {
        let db = db_with_crews().await;
        let csv = format!("{MEMBERS}\nP1;IT-A;;;oui;\nP1;IT-B;;;;\nP1;IT-B;01/01/2020;31/12/2020;;\n");
        let p = run(&db, "team_member", &csv).await;
        assert!(errors(&p, 2).is_empty());
        assert_eq!(errors(&p, 3), vec!["MEMBER_IMPORT_HOME_CONFLICT"]);
        assert!(errors(&p, 4).is_empty(), "a finished period does not conflict");
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.failed), (2, 0));
    }

    #[tokio::test]
    async fn a_new_lead_replaces_the_previous_one() {
        let db = db_with_crews().await;
        let p = run(&db, "team_member", &format!("{MEMBERS}\nP1;IT-A;;;;oui\nP2;IT-A;;;;\n")).await;
        apply(&db, &p).await;
        assert!(is_lead(&db, "P1", "IT-A").await);

        let csv = format!("{MEMBERS}\nP2;IT-A;;;;oui\nP3;IT-A;;;;oui\nP3;IT-B;01/01/2020;31/12/2020;;oui\n");
        let p = run(&db, "team_member", &csv).await;
        assert_eq!(action(&p, 2), "update");
        assert!(p
            .rows
            .iter()
            .find(|r| r.row_no == 2)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.code == "MEMBER_IMPORT_LEAD_REPLACED"));
        assert_eq!(errors(&p, 3), vec!["MEMBER_IMPORT_LEAD_TWICE"]);
        assert_eq!(errors(&p, 4), vec!["MEMBER_IMPORT_LEAD_CLOSED"]);
        let res = apply(&db, &p).await;
        assert_eq!((res.updated, res.failed), (1, 0));
        assert!(is_lead(&db, "P2", "IT-A").await);
        assert!(!is_lead(&db, "P1", "IT-A").await);
    }
}
