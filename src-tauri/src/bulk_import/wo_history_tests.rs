//! History import: read-only records, failure events for RAMS, refusals, site rights, export.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::auth::scope_tests::{assign, role, tree, user, Tree};
    use crate::bulk_import::{self, CreateInput};
    use crate::rbac::permissions::{OT_CREATE, OT_EDIT, OT_VIEW};

    const HEADER: &str = "n° OT d'origine;code équipement;nature;défaillance;date de début des travaux;\
                          date de fin des travaux;date de détection de la panne;date d'arrêt équipement;\
                          date de remise en service;durée d'arrêt (h);mode de défaillance;cause;mécanisme;\
                          méthode de détection;effet sur la fonction;description;travaux réalisés;\
                          heures MO totales;coût MO;coût pièces;coût externe;matricules intervenants";

    fn input(csv: &str) -> CreateInput {
        CreateInput {
            entity: "wo_history".to_string(),
            filename: "historique.csv".to_string(),
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

    async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "n")
            .expect("n")
    }

    async fn equipment(db: &DatabaseConnection, code: &str, node: i64) {
        exec(
            db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
                 created_at, updated_at) VALUES ('whi-eq-{code}', '{code}', 'Équipement {code}', 'active_in_service', \
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

    /// A published work order type code of the seed.
    async fn nature(db: &DatabaseConnection) -> String {
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.code FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rd.code = 'WORK.ORDER_TYPES' AND rs.status = 'published' AND rv.is_active = 1 \
             ORDER BY rv.id LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("query")
        .expect("a seeded work order type")
        .try_get("", "code")
        .expect("code")
    }

    /// Equipment EQ-A on site A and EQ-B on site B, failure mode BRG.
    async fn setup() -> (DatabaseConnection, Tree, String) {
        let db = crate::db::test_db::seeded().await;
        let t = tree(&db).await;
        equipment(&db, "EQ-A", t.site_a).await;
        equipment(&db, "EQ-B", t.site_b).await;
        crate::wo::closeout_reference_catalog::ensure_value(
            &db,
            "WORK.FAILURE_MODES",
            "BRG",
            "Roulement",
            None,
            None,
            90,
        )
        .await
        .expect("failure mode");
        trust_this_device(&db, 1).await;
        let n = nature(&db).await;
        (db, t, n)
    }

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

    const HISTORY_EVENTS: &str = "SELECT COUNT(*) AS n FROM failure_events WHERE source_type = 'history_record'";

    #[tokio::test]
    async fn history_lines_become_records_and_failures_feed_rams() {
        let (db, _, n) = setup().await;
        let file = format!(
            "{HEADER}\n\
             ot-1;EQ-A;{n};oui;10/03/2025 08:00;10/03/2025 12:00;10/03/2025 07:30;10/03/2025 07:00;\
             10/03/2025 13:00;6;brg;;;;;Bruit palier;Remplacement roulement;4;8000;12000;0;M01\n\
             OT-2;eq-a;{n};non;15/04/2025;15/04/2025;;;;;;;;;;;Graissage;1;2000;;;\n"
        );
        let (rows, res) = run(&db, &file, 1).await;
        // M01 is not in the personnel list: kept as text, warned, never an error.
        assert_eq!(
            rows[0],
            (
                "create".to_string(),
                vec!["HISTORY_IMPORT_INTERVENER_UNKNOWN".to_string()]
            )
        );
        assert_eq!(rows[1], ("create".to_string(), vec![]));
        assert_eq!((res.created, res.failed), (2, 0));
        assert_eq!(count(&db, "SELECT COUNT(*) AS n FROM wo_history_records").await, 2);

        // Only the failure has a failure event, dated as RAMS reads it.
        assert_eq!(count(&db, HISTORY_EVENTS).await, 1);
        let r = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT fe.failed_at, fe.restored_at, fe.downtime_duration_hours, fe.active_repair_hours, \
                 fe.failure_mode_id IS NOT NULL AS coded, h.import_ref \
                 FROM failure_events fe JOIN wo_history_records h ON h.id = fe.source_id \
                 WHERE fe.source_type = 'history_record'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("one failure event");
        assert_eq!(r.try_get::<String>("", "import_ref").expect("ref"), "OT-1");
        assert_eq!(r.try_get::<String>("", "failed_at").expect("f"), "2025-03-10T07:00:00Z");
        assert_eq!(
            r.try_get::<String>("", "restored_at").expect("r"),
            "2025-03-10T13:00:00Z"
        );
        assert_eq!(r.try_get::<f64>("", "downtime_duration_hours").expect("d"), 6.0);
        assert_eq!(r.try_get::<f64>("", "active_repair_hours").expect("h"), 4.0);
        assert_eq!(r.try_get::<i64>("", "coded").expect("c"), 1);

        // The export, imported again: everything is already there, nothing changes.
        let table = bulk_import::export(&db, "wo_history").await.expect("export");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0], "OT-1");
        assert_eq!(table.rows[0][4], "10/03/2025 08:00");
        assert_eq!(table.rows[1][4], "15/04/2025");
        let mut csv = table.header.join(";");
        for row in &table.rows {
            csv.push('\n');
            csv.push_str(&row.join(";"));
        }
        let (rows, res) = run(&db, &csv, 1).await;
        assert!(
            rows.iter()
                .all(|(a, m)| a == "skip" && m == &["HISTORY_IMPORT_EXISTS".to_string()]),
            "{rows:?}"
        );
        assert_eq!((res.created, res.failed), (0, 0));
        assert_eq!(count(&db, HISTORY_EVENTS).await, 1);
    }

    #[tokio::test]
    async fn refuses_wrong_lines_and_warns_on_gaps() {
        let (db, _, n) = setup().await;
        let file = format!(
            "{HEADER}\n\
             H1;NOPE;{n};non;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n\
             H2;EQ-A;NOPE;non;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n\
             H3;EQ-A;{n};peut-être;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n\
             H4;EQ-A;{n};non;02/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n\
             H5;EQ-A;{n};non;01/01/2099;01/01/2099;;;;;;;;;;;;;;;;\n\
             H6;EQ-A;{n};oui;01/01/2025;01/01/2025;;;;;XX;;;;;;;;;;;\n\
             H7;EQ-A;{n};oui;01/01/2025 08:00;01/01/2025 10:00;;01/01/2025 08:00;01/01/2025 10:00;5;BRG;;;;;;;;;;;\n\
             H8;EQ-A;{n};oui;01/01/2025;01/01/2025;;;;;;;;;;;;-3;;;;\n\
             h7;EQ-A;{n};non;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n"
        );
        let (rows, res) = run(&db, &file, 1).await;
        let codes: Vec<Vec<String>> = rows.into_iter().map(|(_, m)| m).collect();
        let expected: Vec<Vec<&str>> = vec![
            vec!["HISTORY_IMPORT_EQUIPMENT_UNKNOWN"],
            vec!["HISTORY_IMPORT_CODE_UNKNOWN"],
            vec!["HISTORY_IMPORT_FAILURE_FLAG"],
            vec!["HISTORY_IMPORT_DATE_ORDER"],
            vec!["HISTORY_IMPORT_DATE_FUTURE", "HISTORY_IMPORT_DATE_FUTURE"],
            vec!["HISTORY_IMPORT_CODE_UNKNOWN"],
            vec!["HISTORY_IMPORT_DOWNTIME_GAP"],
            vec!["HISTORY_IMPORT_CODING_INCOMPLETE", "HISTORY_IMPORT_NUMBER"],
            vec!["HISTORY_IMPORT_DUPLICATE"],
        ];
        assert_eq!(codes, expected);
        // Only H7 (a warning, no error) is created, with its failure event.
        assert_eq!((res.created, res.failed), (1, 0));
        assert_eq!(count(&db, HISTORY_EVENTS).await, 1);
    }

    #[tokio::test]
    async fn a_site_user_imports_history_for_their_site_only() {
        let (db, t, n) = setup().await;
        let tech = user(&db, "history_site_a").await;
        let site_role = role(&db, "History import site A", &[OT_VIEW, OT_CREATE]).await;
        assign(&db, tech, site_role, "org_node", Some(t.site_a)).await;
        trust_this_device(&db, tech).await;
        let file = format!(
            "{HEADER}\n\
             HA;EQ-A;{n};non;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n\
             HB;EQ-B;{n};non;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n"
        );
        let (rows, res) = run(&db, &file, tech).await;
        assert_eq!(rows[0], ("create".to_string(), vec![]));
        assert_eq!(
            rows[1],
            ("skip".to_string(), vec!["HISTORY_IMPORT_SITE_RIGHT".to_string()])
        );
        assert_eq!((res.created, res.failed), (1, 0));
    }

    async fn id_of(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "id")
            .expect("id")
    }

    #[tokio::test]
    async fn two_sites_may_reuse_a_number_and_a_line_has_one_identity_everywhere() {
        let (db, _, n) = setup().await;
        let file = format!(
            "{HEADER}\n\
             OT-1001;EQ-A;{n};non;01/01/2025;01/01/2025;;;;;;;;;;;;;;;;\n\
             OT-1001;EQ-B;{n};non;02/01/2025;02/01/2025;;;;;;;;;;;;;;;;\n"
        );
        let (rows, res) = run(&db, &file, 1).await;
        assert_eq!(
            rows,
            vec![("create".to_string(), vec![]), ("create".to_string(), vec![])]
        );
        assert_eq!((res.created, res.failed), (2, 0));
        // Imported again: both are there already.
        let (rows, _) = run(&db, &file, 1).await;
        assert!(rows
            .iter()
            .all(|(a, m)| a == "skip" && m == &["HISTORY_IMPORT_EXISTS".to_string()]));
        // Its sync id derives from the equipment and the n°: another PC importing the same line
        // makes the same row.
        let expected = {
            let eq: String = db
                .query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT entity_sync_id FROM equipment WHERE asset_id_code = 'EQ-A'".to_string(),
                ))
                .await
                .expect("query")
                .expect("row")
                .try_get("", "entity_sync_id")
                .expect("id");
            crate::sync::identity::derived_entity_sync_id("wo_history_records", &format!("{eq}|OT-1001"))
        };
        assert_eq!(
            count(
                &db,
                &format!(
                    "SELECT COUNT(*) AS n FROM wo_history_records h JOIN equipment e ON e.id = h.equipment_id \
                     WHERE e.asset_id_code = 'EQ-A' AND h.entity_sync_id = '{expected}'"
                ),
            )
            .await,
            1
        );
    }

    async fn access(db: &DatabaseConnection, user_id: i32) -> crate::auth::scope::AccessMap {
        let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
        crate::auth::scope::access_map_cached(db, &cache, user_id)
            .await
            .expect("access map")
    }

    /// One imported failure on EQ-A: labour 8 000 + parts 12 000, mode BRG. (record, equipment)
    async fn one_failure(db: &DatabaseConnection, n: &str) -> (i64, i64) {
        let file = format!(
            "{HEADER}\n\
             OT-9;EQ-A;{n};oui;10/03/2025 08:00;10/03/2025 12:00;;10/03/2025 07:00;10/03/2025 13:00;6;BRG;;;;;\
             Bruit palier;Remplacement roulement;4;8000;12000;0;\n"
        );
        let (_, res) = run(db, &file, 1).await;
        assert_eq!((res.created, res.failed), (1, 0));
        let record = id_of(db, "SELECT id FROM wo_history_records WHERE import_ref = 'OT-9'").await;
        let equipment = id_of(db, "SELECT id FROM equipment WHERE asset_id_code = 'EQ-A'").await;
        (record, equipment)
    }

    #[tokio::test]
    async fn imported_history_counts_in_the_cost_and_the_history_of_its_equipment() {
        let (db, _, n) = setup().await;
        let (record, equipment) = one_failure(&db, &n).await;

        let costs = crate::finance::asset_costs::list_asset_cost_history(
            &db,
            crate::finance::asset_costs::AssetCostFilter {
                search: Some("EQ-A".into()),
                ..Default::default()
            },
        )
        .await
        .expect("asset costs");
        let row = costs
            .rows
            .iter()
            .find(|r| r.equipment_id == equipment)
            .expect("EQ-A has a cost");
        assert_eq!(row.lifetime_cost, 20000.0);
        assert_eq!(row.lifetime_labor, 8000.0);
        assert_eq!(row.lifetime_parts, 12000.0);
        assert_eq!(row.closed_work_orders, 1);
        assert_eq!(row.last_closed_at.as_deref(), Some("2025-03-10T12:00:00Z"));

        let profile = crate::assets::maintenance_profile::get_asset_maintenance_profile(
            &db,
            equipment,
            false,
            crate::clock::now(),
        )
        .await
        .expect("profile");
        assert_eq!(profile.work_order_count, 1);
        assert_eq!(profile.failure_work_order_count, 1);
        assert_eq!(profile.downtime_hours, 6.0);
        assert_eq!(profile.cost_lifetime.total, 20000.0);
        assert_eq!(profile.last_failure_at.as_deref(), Some("2025-03-10T07:00:00Z"));

        let events = crate::assets::history::list_asset_history_events(
            &db,
            crate::assets::history::AssetHistoryQuery {
                asset_id: equipment,
                event_types: Some(vec!["history_imported".into()]),
                period: None,
                search: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .expect("history");
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev.occurred_at, "2025-03-10T12:00:00Z");
        let r = ev.r#ref.as_ref().expect("ref");
        assert_eq!((r.entity_type.as_str(), r.entity_id), ("history_record", Some(record)));
        assert_eq!(ev.metadata["row_version"], serde_json::json!(1));
        assert_eq!(ev.metadata["total_cost"], serde_json::json!(20000.0));
        assert!(ev.metadata["failure_mode_id"].is_number());
    }

    #[tokio::test]
    async fn failure_coding_of_imported_history_is_editable_at_the_equipment_site_only() {
        use crate::bulk_import::wo_history::{check_coding_right, save_coding, HistoryCodingInput};

        let (db, t, n) = setup().await;
        let (record, _) = one_failure(&db, &n).await;
        crate::wo::closeout_reference_catalog::ensure_value(
            &db,
            "WORK.FAILURE_MODES",
            "SEAL",
            "Étanchéité",
            None,
            None,
            91,
        )
        .await
        .expect("failure mode");
        let seal = id_of(
            &db,
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rd.code = 'WORK.FAILURE_MODES' AND rv.code = 'SEAL'",
        )
        .await;
        let nature_id = id_of(
            &db,
            "SELECT type_id AS id FROM wo_history_records WHERE import_ref = 'OT-9'",
        )
        .await;

        // The edit right on site B only: not on a record of site A.
        let edit_role = role(&db, "History coding", &[OT_VIEW, OT_EDIT]).await;
        let site_b = user(&db, "history_coder_b").await;
        assign(&db, site_b, edit_role, "org_node", Some(t.site_b)).await;
        // Refused with the site-scope code, naming the record's site (A).
        let denied = check_coding_right(&db, &access(&db, site_b).await, record).await;
        assert!(
            matches!(
                &denied,
                Err(crate::errors::AppError::OrgValidationFailed(issues))
                    if issues.iter().any(|i| i.code == "SCOPE_OUTSIDE_SITE"
                        && i.params.get("org_node_id") == Some(&t.site_a.to_string()))
            ),
            "{denied:?}"
        );
        // No edit right anywhere: refused.
        let viewer = user(&db, "history_viewer").await;
        let view_role = role(&db, "History view", &[OT_VIEW]).await;
        assign(&db, viewer, view_role, "org_node", Some(t.site_a)).await;
        assert!(check_coding_right(&db, &access(&db, viewer).await, record)
            .await
            .is_err());
        // The edit right on site A: allowed.
        let site_a = user(&db, "history_coder_a").await;
        assign(&db, site_a, edit_role, "org_node", Some(t.site_a)).await;
        check_coding_right(&db, &access(&db, site_a).await, record)
            .await
            .expect("site A may code");

        let coding = |mode: Option<i64>, version: i64| HistoryCodingInput {
            record_id: record,
            expected_row_version: version,
            failure_mode_id: mode,
            failure_cause_id: None,
            failure_mechanism_id: None,
            detection_method_id: None,
            failure_effect_id: None,
        };
        // A value of another list is refused.
        let wrong = save_coding(&db, coding(Some(nature_id), 1), site_a).await;
        assert!(
            matches!(wrong, Err(crate::errors::AppError::ValidationFailed(_))),
            "{wrong:?}"
        );
        // A stale version is refused.
        assert!(save_coding(&db, coding(Some(seal), 7), site_a).await.is_err());

        let saved = save_coding(&db, coding(Some(seal), 1), site_a)
            .await
            .expect("coding saved");
        assert_eq!(saved.row_version, 2);
        // The record and its failure event (RAMS) carry the new coding; the coder is recorded.
        let checks = [
            format!(
                "SELECT COUNT(*) AS n FROM wo_history_records WHERE id = {record} AND failure_mode_id = {seal} \
                 AND coded_by_id = {site_a} AND coded_at IS NOT NULL AND row_version = 2"
            ),
            format!(
                "SELECT COUNT(*) AS n FROM failure_events WHERE source_type = 'history_record' \
                 AND source_id = {record} AND failure_mode_id = {seal}"
            ),
            format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'wo_history.coding_saved' \
                 AND target_id = '{record}'"
            ),
        ];
        for sql in checks {
            assert_eq!(count(&db, &sql).await, 1, "{sql}");
        }
    }
}
