//! PM plan import: draft plan and version, create only, refusals, site rights, export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use serde_json::Value;

    use crate::auth::scope_tests::{assign, role, tree, user, Tree};
    use crate::bulk_import::{self, CreateInput};
    use crate::rbac::permissions::{PM_CREATE, PM_VIEW};

    const HEADER: &str = "code plan;libellé;code équipement;déclenchement;intervalle;unité;code compteur;\
                          tolérance (jours);date de dernière réalisation;code équipe;compétences requises;\
                          durée estimée (h);nb intervenants;permis requis;arrêt requis";

    fn input(csv: &str) -> CreateInput {
        CreateInput {
            entity: "pm_plan".to_string(),
            filename: "plans.csv".to_string(),
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

    async fn equipment(db: &DatabaseConnection, code: &str, node: i64) {
        exec(
            db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
                 created_at, updated_at) VALUES ('pmi-eq-{code}', '{code}', 'Équipement {code}', 'active_in_service', \
                 {node}, datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }

    /// Org tree (plant, site A, line A1, site B); equipment EQ-A on site A (with meter HM) and
    /// EQ-B on site B.
    async fn setup() -> (DatabaseConnection, Tree) {
        let db = crate::db::test_db::seeded().await;
        let t = tree(&db).await;
        equipment(&db, "EQ-A", t.site_a).await;
        equipment(&db, "EQ-B", t.site_b).await;
        exec(
            &db,
            "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_code, meter_type, current_reading, \
             is_primary, is_active, created_at, updated_at) \
             SELECT 'pmi-m-1', id, 'Heures de marche', 'HM', 'CUSTOM', 0, 1, 1, datetime('now'), datetime('now') \
             FROM equipment WHERE asset_id_code = 'EQ-A'",
        )
        .await;
        (db, t)
    }

    /// Message codes of each line, its action, and the apply result.
    async fn run(
        db: &DatabaseConnection,
        csv: &str,
        user_id: i32,
    ) -> (Vec<(String, Vec<String>)>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input(csv), Some(i64::from(user_id)))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(db, b.id).await.expect("preview");
        let rows = p
            .rows
            .iter()
            .map(|r| (r.action.clone(), r.messages.iter().map(|m| m.code.clone()).collect()))
            .collect();
        let res = bulk_import::apply_batch(db, b.id, user_id).await.expect("apply");
        (rows, res)
    }

    #[tokio::test]
    async fn creates_draft_plans_once_and_exports_them() {
        let (db, _) = setup().await;
        let file = format!(
            "{HEADER}\n\
             pm-a1;Graissage pompe A;EQ-A;calendrier;3;mois;;5;15/01/2026;;;1,5;2;oui;non\n\
             PM-A2;Vidange A;eq-a;compteur;500;;hm;;;;;;;;\n"
        );
        let (rows, res) = run(&db, &file, 1).await;
        assert_eq!(
            rows,
            vec![
                ("create".to_string(), vec![]),
                ("create".to_string(), vec!["PM_IMPORT_START_TODAY".to_string()]),
            ]
        );
        assert_eq!((res.created, res.failed), (2, 0));

        let r = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT p.strategy_type, p.lifecycle_status, p.requires_permit, p.requires_shutdown, v.status, \
                 v.effective_from, v.trigger_definition_json, v.estimated_duration_hours, v.planned_crew_size \
                 FROM pm_plans p JOIN pm_plan_versions v ON v.pm_plan_id = p.id WHERE p.code = 'PM-A1'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("plan PM-A1");
        assert_eq!(r.try_get::<String>("", "strategy_type").expect("s"), "fixed");
        assert_eq!(r.try_get::<String>("", "lifecycle_status").expect("l"), "draft");
        assert_eq!(r.try_get::<String>("", "status").expect("v"), "draft");
        assert_eq!(r.try_get::<i64>("", "requires_permit").expect("p"), 1);
        assert_eq!(r.try_get::<i64>("", "requires_shutdown").expect("a"), 0);
        assert!(r
            .try_get::<String>("", "effective_from")
            .expect("from")
            .starts_with("2026-01-15"));
        let trigger: Value =
            serde_json::from_str(&r.try_get::<String>("", "trigger_definition_json").expect("t")).expect("json");
        assert_eq!(trigger["interval_unit"], "month");
        assert_eq!(trigger["interval_value"].as_f64(), Some(3.0));
        assert_eq!(trigger["tolerance"]["late"].as_f64(), Some(5.0));
        assert_eq!(
            r.try_get::<Option<f64>>("", "estimated_duration_hours").expect("d"),
            Some(1.5)
        );
        assert_eq!(r.try_get::<Option<i64>>("", "planned_crew_size").expect("c"), Some(2));
        // No work order is generated by the import.
        let occurrences = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM pm_occurrences".to_string(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get::<i64>("", "n")
            .expect("n");
        assert_eq!(occurrences, 0);

        // The export, imported again: every plan exists, nothing changes.
        let table = bulk_import::export(&db, "pm_plan").await.expect("export");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(
            table.rows[0],
            vec![
                "PM-A1",
                "Graissage pompe A",
                "EQ-A",
                "calendrier",
                "3",
                "mois",
                "",
                "5",
                "15/01/2026",
                "",
                "",
                "1.5",
                "2",
                "oui",
                "non"
            ]
        );
        assert_eq!(table.rows[1][3], "compteur");
        assert_eq!(table.rows[1][6], "HM");
        let mut csv = table.header.join(";");
        for row in &table.rows {
            csv.push('\n');
            csv.push_str(&row.join(";"));
        }
        let (rows, res) = run(&db, &csv, 1).await;
        assert!(
            rows.iter()
                .all(|(a, m)| a == "skip" && m == &["PM_IMPORT_EXISTS".to_string()]),
            "{rows:?}"
        );
        assert_eq!((res.created, res.failed), (0, 0));
    }

    #[tokio::test]
    async fn refuses_what_the_plan_screen_refuses() {
        let (db, _) = setup().await;
        let file = format!(
            "{HEADER}\n\
             P1;Sans équipement;NOPE;calendrier;1;mois;;;;;;;;;\n\
             P2;Mauvais déclenchement;EQ-A;hebdo;1;;;;;;;;;;\n\
             P3;Mauvaise unité;EQ-A;calendrier;1;quinzaine;;;;;;;;;\n\
             P4;Intervalle nul;EQ-A;calendrier;0;mois;;;;;;;;;\n\
             P5;Compteur absent;EQ-A;compteur;100;;XX;;;;;;;;\n\
             P6;Date future;EQ-A;calendrier;1;mois;;;31/12/2099;;;;;;\n\
             P7;Équipe inconnue;EQ-A;calendrier;1;mois;;;;NOPE;;;;;\n\
             P8;Compétence inconnue;EQ-A;calendrier;1;mois;;;;;NOPE;;;;\n\
             P9;Trop d'intervenants;EQ-A;calendrier;1;mois;;;;;;;51;;\n\
             P10;Drapeau;EQ-A;calendrier;1;mois;;;;;;;;peut-être;\n\
             p1;Doublon;EQ-A;calendrier;1;mois;;;;;;;;;\n"
        );
        let (rows, res) = run(&db, &file, 1).await;
        let errors: Vec<Vec<String>> = rows
            .into_iter()
            .map(|(_, m)| m.into_iter().filter(|c| c != "PM_IMPORT_START_TODAY").collect())
            .collect();
        let expected: Vec<Vec<&str>> = vec![
            vec!["PM_IMPORT_EQUIPMENT_UNKNOWN"],
            vec!["PM_IMPORT_TRIGGER"],
            vec!["PM_IMPORT_UNIT"],
            vec!["PM_IMPORT_INTERVAL"],
            vec!["PM_IMPORT_METER_UNKNOWN"],
            vec!["PM_IMPORT_LAST_DONE_FUTURE"],
            vec!["PM_IMPORT_TEAM_UNKNOWN"],
            vec!["PM_IMPORT_SKILL_UNKNOWN"],
            vec!["PM_IMPORT_CREW"],
            vec!["PM_IMPORT_FLAG"],
            vec!["PM_IMPORT_DUPLICATE"],
        ];
        assert_eq!(errors, expected);
        assert_eq!(res.created, 0);
    }

    #[tokio::test]
    async fn a_site_planner_imports_plans_for_their_site_only() {
        let (db, t) = setup().await;
        let planner = user(&db, "pm_import_site_a").await;
        let site_role = role(&db, "PM import site A", &[PM_VIEW, PM_CREATE]).await;
        assign(&db, planner, site_role, "org_node", Some(t.site_a)).await;
        let file = format!(
            "{HEADER}\n\
             PA;Plan site A;EQ-A;calendrier;1;mois;;;;;;;;;\n\
             PB;Plan site B;EQ-B;calendrier;1;mois;;;;;;;;;\n"
        );
        let (rows, res) = run(&db, &file, planner).await;
        assert_eq!(rows[0].0, "create");
        assert_eq!(
            rows[1],
            (
                "skip".to_string(),
                vec!["PM_IMPORT_START_TODAY".to_string(), "PM_IMPORT_SITE_RIGHT".to_string()]
            )
        );
        assert_eq!((res.created, res.failed), (1, 0));
    }
}
