//! PM task import: steps of the draft version, whole-plan replacement, refusals, rights, export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use serde_json::Value;

    use crate::auth::scope_tests::{assign, role, tree, user, Tree};
    use crate::bulk_import::{self, CreateInput};
    use crate::rbac::permissions::{PM_EDIT, PM_VIEW};

    const PLANS: &str = "code plan;libellé;code équipement;déclenchement;intervalle;unité\n\
                         PA;Plan A;EQ-A;calendrier;1;mois\n\
                         PA2;Plan A bis;EQ-A;calendrier;1;mois\n\
                         PB;Plan B;EQ-B;calendrier;1;mois\n";
    const HEADER: &str = "code plan;n° d'ordre;description";

    fn input(entity: &str, csv: &str) -> CreateInput {
        CreateInput {
            entity: entity.to_string(),
            filename: "taches.csv".to_string(),
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
                 created_at, updated_at) VALUES ('pti-eq-{code}', '{code}', 'Équipement {code}', 'active_in_service', \
                 {node}, datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }

    async fn trust_this_device(db: &DatabaseConnection, user_id: i32) {
        let fingerprint = crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());
        crate::auth::device::register_device_trust(db, user_id, &fingerprint, None)
            .await
            .expect("trust register");
    }

    /// Message codes of each line, its action, and the apply result.
    async fn run(
        db: &DatabaseConnection,
        entity: &str,
        csv: &str,
        user_id: i32,
    ) -> (Vec<(String, Vec<String>)>, bulk_import::ApplyResult) {
        let b = bulk_import::create_batch(db, input(entity, csv), Some(i64::from(user_id)))
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

    /// Plans PA and PA2 on EQ-A (site A) and PB on EQ-B (site B), each with a draft version.
    async fn setup() -> (DatabaseConnection, Tree) {
        let db = crate::db::test_db::seeded().await;
        let t = tree(&db).await;
        equipment(&db, "EQ-A", t.site_a).await;
        equipment(&db, "EQ-B", t.site_b).await;
        trust_this_device(&db, 1).await;
        let (_, res) = run(&db, "pm_plan", PLANS, 1).await;
        assert_eq!((res.created, res.failed), (3, 0));
        (db, t)
    }

    async fn steps(db: &DatabaseConnection, plan: &str) -> Vec<String> {
        let raw: Option<String> = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT v.task_package_json FROM pm_plan_versions v JOIN pm_plans p ON p.id = v.pm_plan_id \
                     WHERE p.code = '{plan}' ORDER BY v.id DESC LIMIT 1"
                ),
            ))
            .await
            .expect("query")
            .expect("version")
            .try_get("", "task_package_json")
            .expect("json");
        raw.map(|s| {
            let v: Value = serde_json::from_str(&s).expect("package");
            serde_json::from_value(v["steps"].clone()).expect("steps")
        })
        .unwrap_or_default()
    }

    #[tokio::test]
    async fn tasks_fill_the_draft_in_order_and_export_round_trips() {
        let (db, _) = setup().await;
        let file = format!(
            "{HEADER}\n\
             pa;2;Graisser les paliers\n\
             PA;1;Consigner la pompe\n\
             PA;3;Remettre en service\n\
             PB;1;Contrôler le niveau d'huile\n"
        );
        let (rows, res) = run(&db, "pm_task", &file, 1).await;
        assert!(rows.iter().all(|(a, m)| a == "update" && m.is_empty()), "{rows:?}");
        assert_eq!((res.updated, res.failed), (4, 0));
        assert_eq!(
            steps(&db, "PA").await,
            vec!["Consigner la pompe", "Graisser les paliers", "Remettre en service"]
        );
        assert_eq!(steps(&db, "PB").await, vec!["Contrôler le niveau d'huile"]);
        assert!(steps(&db, "PA2").await.is_empty());

        // The export, imported again: nothing changes.
        let table = bulk_import::export(&db, "pm_task").await.expect("export");
        assert_eq!(table.rows.len(), 4);
        assert_eq!(table.rows[0], vec!["PA", "1", "Consigner la pompe"]);
        let mut csv = table.header.join(";");
        for row in &table.rows {
            csv.push('\n');
            csv.push_str(&row.join(";"));
        }
        let (rows, res) = run(&db, "pm_task", &csv, 1).await;
        assert!(rows.iter().all(|(a, _)| a == "skip"), "{rows:?}");
        assert_eq!(rows[0].1, vec!["PM_TASK_IMPORT_UNCHANGED".to_string()]);
        assert_eq!((res.updated, res.failed), (0, 0));

        // A new list replaces the old one whole, with a warning in the preview.
        let (rows, res) = run(&db, "pm_task", &format!("{HEADER}\nPA;1;Nouvelle tâche unique\n"), 1).await;
        assert_eq!(
            rows,
            vec![("update".to_string(), vec!["PM_TASK_IMPORT_REPLACES".to_string()])]
        );
        assert_eq!(res.updated, 1);
        assert_eq!(steps(&db, "PA").await, vec!["Nouvelle tâche unique"]);
    }

    #[tokio::test]
    async fn a_wrong_line_stops_its_whole_plan_and_published_versions_stay_unchanged() {
        let (db, _) = setup().await;
        exec(
            &db,
            "UPDATE pm_plan_versions SET status = 'published' \
             WHERE pm_plan_id = (SELECT id FROM pm_plans WHERE code = 'PB')",
        )
        .await;
        let file = format!(
            "{HEADER}\n\
             PA;1;Première\n\
             PA;1;Même rang\n\
             PA2;x;Rang illisible\n\
             PA2;2;Bonne ligne d'un plan en erreur\n\
             PB;1;Version publiée\n\
             NOPE;1;Plan inconnu\n\
             PA2;3;\n"
        );
        let (rows, res) = run(&db, "pm_task", &file, 1).await;
        let codes: Vec<Vec<String>> = rows.iter().map(|(_, m)| m.clone()).collect();
        let expected: Vec<Vec<&str>> = vec![
            vec!["PM_TASK_IMPORT_PLAN_STOPPED"],
            vec!["PM_TASK_IMPORT_ORDER_TWICE"],
            vec!["PM_TASK_IMPORT_ORDER"],
            vec!["PM_TASK_IMPORT_PLAN_STOPPED"],
            vec!["PM_TASK_IMPORT_NOT_DRAFT"],
            vec!["PM_TASK_IMPORT_PLAN_UNKNOWN"],
            vec!["PM_TASK_IMPORT_DESCRIPTION"],
        ];
        assert_eq!(codes, expected);
        assert!(rows.iter().all(|(a, _)| a == "skip"));
        assert_eq!((res.updated, res.failed), (0, 0));
        assert!(steps(&db, "PA").await.is_empty());
        assert!(steps(&db, "PA2").await.is_empty());
    }

    #[tokio::test]
    async fn a_draft_edited_after_the_preview_is_not_overwritten() {
        let (db, _) = setup().await;
        let file = format!("{HEADER}\nPA;1;Tâche du fichier\nPB;1;Autre plan\n");
        let b = bulk_import::create_batch(&db, input("pm_task", &file), Some(1))
            .await
            .expect("batch");
        // Someone edits PA's draft in Préventif between the preview and the apply.
        exec(
            &db,
            "UPDATE pm_plan_versions SET task_package_json = '{\"steps\":[\"Saisie à l''écran\"]}', \
             row_version = row_version + 1 \
             WHERE pm_plan_id = (SELECT id FROM pm_plans WHERE code = 'PA')",
        )
        .await;
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.updated, res.failed), (1, 1));
        assert_eq!(steps(&db, "PA").await, vec!["Saisie à l'écran"]);
        assert_eq!(steps(&db, "PB").await, vec!["Autre plan"]);
    }

    #[tokio::test]
    async fn a_site_planner_imports_tasks_for_their_site_only() {
        let (db, t) = setup().await;
        let planner = user(&db, "pm_task_site_a").await;
        let site_role = role(&db, "PM tasks site A", &[PM_VIEW, PM_EDIT]).await;
        assign(&db, planner, site_role, "org_node", Some(t.site_a)).await;
        trust_this_device(&db, planner).await;
        let file = format!("{HEADER}\nPA;1;Tâche site A\nPB;1;Tâche site B\n");
        let (rows, res) = run(&db, "pm_task", &file, planner).await;
        assert_eq!(rows[0], ("update".to_string(), vec![]));
        assert_eq!(rows[1], ("skip".to_string(), vec!["PM_TASK_IMPORT_RIGHT".to_string()]));
        assert_eq!((res.updated, res.failed), (1, 0));
        assert!(steps(&db, "PB").await.is_empty());
    }
}
