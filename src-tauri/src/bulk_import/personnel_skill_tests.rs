//! Skill and certificate imports: preview rules and apply.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::org_tree_tests::tests::setup;
    use crate::bulk_import::{self, CreateInput};

    const SKILLS: &str = "matricule;code compétence;niveau";
    const CERTS: &str = "matricule;type de certificat;date d'obtention;date d'expiration;organisme;n° de certificat";

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

    async fn text(db: &DatabaseConnection, sql: &str) -> Option<String> {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql)
            .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("value"))
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

    async fn exported(db: &DatabaseConnection, entity: &str, header: &str) -> String {
        let export = bulk_import::export(db, entity).await.expect("export");
        let mut csv = format!("{header}\n");
        for row in export.rows.iter().filter(|r| r[0].starts_with("IT-")) {
            csv.push_str(&row.join(";"));
            csv.push('\n');
        }
        csv
    }

    /// People IT-P1 and IT-P2; certificate types IT-HAB (36-month validity) and IT-CACES (none).
    async fn db_with_people() -> DatabaseConnection {
        let db = setup().await;
        for code in ["IT-P1", "IT-P2"] {
            exec(
                &db,
                &format!(
                    "INSERT INTO personnel (employee_code, full_name, availability_status, employment_status) \
                     VALUES ('{code}', '{code}', 'available', 'active')"
                ),
            )
            .await;
        }
        exec(
            &db,
            "INSERT INTO certification_types (entity_sync_id, code, name, default_validity_months, category, row_version) \
             VALUES ('ct-it-hab', 'IT-HAB', 'Habilitation électrique', 36, 'electrical', 1), \
                    ('ct-it-caces', 'IT-CACES', 'CACES', NULL, 'other', 1)",
        )
        .await;
        db
    }

    /// Two skill codes of the published PERSONNEL.SKILLS list.
    async fn two_skills(db: &DatabaseConnection) -> (String, String) {
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT rv.code FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = 'PERSONNEL.SKILLS' AND rv.is_active = 1 ORDER BY rv.code LIMIT 2"
                    .to_string(),
            ))
            .await
            .expect("skills");
        assert_eq!(rows.len(), 2, "the seeded skills list");
        (
            rows[0].try_get("", "code").unwrap(),
            rows[1].try_get("", "code").unwrap(),
        )
    }

    #[tokio::test]
    async fn skills_come_in_unvalidated_and_a_level_change_drops_the_validation() {
        let db = db_with_people().await;
        let (s1, s2) = two_skills(&db).await;
        let csv = format!(
            "{SKILLS}\nIT-P1;{s1};3 — Autonome\nIT-P1;{};4\nIT-P2;{s1};\nIT-P2;NOPE;2\nIT-P2;{s2};7\nIT-P1;{s1};2\n",
            s2.to_lowercase()
        );
        let p = run(&db, "personnel_skill", &csv).await;
        assert_eq!(action(&p, 2), "create");
        assert_eq!(action(&p, 3), "create", "the letter case of the code does not matter");
        assert_eq!(errors(&p, 4), vec!["SKILL_IMPORT_LEVEL"], "a new skill needs a level");
        assert_eq!(errors(&p, 5), vec!["SKILL_IMPORT_SKILL"]);
        assert_eq!(errors(&p, 6), vec!["SKILL_IMPORT_LEVEL"]);
        assert_eq!(errors(&p, 7), vec!["SKILL_IMPORT_DUPLICATE"]);
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.failed), (2, 0));
        let state = |code: &str| {
            format!(
                "SELECT ps.is_validated || ps.source_type || ps.proficiency_level FROM personnel_skills ps \
                 JOIN personnel p ON p.id = ps.personnel_id JOIN reference_values rv ON rv.id = ps.reference_value_id \
                 WHERE p.employee_code = 'IT-P1' AND rv.code = '{code}'"
            )
        };
        assert_eq!(text(&db, &state(&s1)).await.as_deref(), Some("0import3"));

        // Validated on the screen, then re-imported: unchanged keeps the validation, a new level drops it.
        exec(
            &db,
            "UPDATE personnel_skills SET is_validated = 1, source_type = 'manual'",
        )
        .await;
        let again = run(&db, "personnel_skill", &exported(&db, "personnel_skill", SKILLS).await).await;
        assert!(again.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));
        let p = run(
            &db,
            "personnel_skill",
            &format!("{SKILLS}\nIT-P1;{s1};5\nIT-P1;{s2};\n"),
        )
        .await;
        assert_eq!(action(&p, 2), "update");
        assert_eq!(action(&p, 3), "skip", "a blank level changes nothing");
        let res = apply(&db, &p).await;
        assert_eq!((res.updated, res.failed), (1, 0));
        assert_eq!(text(&db, &state(&s1)).await.as_deref(), Some("0import5"));
        assert_eq!(text(&db, &state(&s2)).await.as_deref(), Some("1manual4"));
    }

    #[tokio::test]
    async fn certificates_come_in_to_check_and_a_change_sends_them_back() {
        let db = db_with_people().await;
        let csv = format!(
            "{CERTS}\nIT-P1;IT-HAB;01/02/2024;01/02/2027;APAVE;H-1\nIT-P1;it-caces;;;;\nIT-P2;IT-HAB;01/02/2024;;;\n\
             IT-P2;IT-CACES;01/02/2025;01/01/2025;;\nIT-P1;IT-HAB;;01/02/2027;;h-1\n"
        );
        let p = run(&db, "personnel_certification", &csv).await;
        assert_eq!(action(&p, 2), "create");
        assert_eq!(action(&p, 3), "create");
        assert_eq!(errors(&p, 4), vec!["CERT_IMPORT_EXPIRY_REQUIRED"]);
        assert_eq!(errors(&p, 5), vec!["CERT_IMPORT_DATES"]);
        assert_eq!(
            errors(&p, 6),
            vec!["CERT_IMPORT_DUPLICATE"],
            "same number, any letter case"
        );
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.failed), (2, 0));
        let status = "SELECT pc.verification_status FROM personnel_certifications pc \
                      JOIN certification_types ct ON ct.id = pc.certification_type_id WHERE ct.code = 'IT-HAB'";
        assert_eq!(text(&db, status).await.as_deref(), Some("pending"));

        // Verified on the screen: the export re-imports as nothing to do and stays verified.
        exec(
            &db,
            "UPDATE personnel_certifications SET verification_status = 'verified'",
        )
        .await;
        let again = run(
            &db,
            "personnel_certification",
            &exported(&db, "personnel_certification", CERTS).await,
        )
        .await;
        assert!(
            again.rows.iter().all(|r| r.action == "skip" && r.status == "ok"),
            "{:?}",
            again.rows.iter().map(|r| (&r.action, &r.messages)).collect::<Vec<_>>()
        );

        // A changed certificate goes back to "to check". A CACES with an issue date the stored one
        // lacks is a second certificate; then a bare line cannot tell the two apart.
        let p = run(
            &db,
            "personnel_certification",
            &format!("{CERTS}\nIT-P1;IT-HAB;;;Bureau Veritas;H-1\nIT-P1;IT-CACES;01/03/2025;;;\n"),
        )
        .await;
        assert_eq!(action(&p, 2), "update");
        assert!(p.rows[0].messages.iter().any(|m| m.code == "CERT_IMPORT_REVERIFY"));
        assert_eq!(action(&p, 3), "create");
        let res = apply(&db, &p).await;
        assert_eq!((res.created, res.updated, res.failed), (1, 1, 0));
        assert_eq!(text(&db, status).await.as_deref(), Some("pending"));
        let p = run(
            &db,
            "personnel_certification",
            &format!("{CERTS}\nIT-P1;IT-CACES;;;;\n"),
        )
        .await;
        assert_eq!(errors(&p, 2), vec!["CERT_IMPORT_WHICH"]);
    }
}
