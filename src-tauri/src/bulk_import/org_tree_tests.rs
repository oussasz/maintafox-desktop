//! Organisation tree import: preview rules and apply.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::bulk_import::{self, CreateInput};
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};

    /// Published model: SITE (root) > ZONE > LINE.
    async fn setup() -> DatabaseConnection {
        let db = crate::db::test_db::seeded().await;
        let model = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("import test model".to_string()),
            },
            1,
        )
        .await
        .expect("create model");
        let mut ids = Vec::new();
        for (code, label, root, depth) in [
            ("SITE", "Site", true, 0),
            ("ZONE", "Zone", false, 1),
            ("LINE", "Ligne", false, 2),
        ] {
            let t = node_types::create_node_type(
                &db,
                CreateNodeTypePayload {
                    structure_model_id: model.id,
                    code: code.to_string(),
                    label: label.to_string(),
                    icon_key: None,
                    color: None,
                    depth_hint: Some(depth),
                    can_host_assets: true,
                    can_own_work: true,
                    can_carry_cost_center: false,
                    can_aggregate_kpis: false,
                    can_receive_permits: false,
                    is_root_type: root,
                },
            )
            .await
            .expect("create type");
            ids.push(t.id);
        }
        for pair in [(ids[0], ids[1]), (ids[1], ids[2])] {
            relationship_rules::create_rule(
                &db,
                CreateRelationshipRulePayload {
                    structure_model_id: model.id,
                    parent_type_id: pair.0,
                    child_type_id: pair.1,
                    min_children: None,
                    max_children: None,
                },
            )
            .await
            .expect("create rule");
        }
        structure_model::publish_model(&db, model.id, 1)
            .await
            .expect("publish model");
        db
    }

    fn input(csv: &str) -> CreateInput {
        CreateInput {
            entity: "org_tree".to_string(),
            filename: "organisation.csv".to_string(),
            source_sha256: "0".repeat(64),
            source_kind: "csv".to_string(),
            file_content: csv.as_bytes().to_vec(),
        }
    }

    async fn parent_code_of(db: &DatabaseConnection, code: &str) -> Option<String> {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.code AS pc FROM org_nodes n LEFT JOIN org_nodes p ON p.id = n.parent_id WHERE n.code = ?",
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
        let db = setup().await;
        let csv = "code;libellé;type de nœud;code parent\n\
                   L1;Ligne 1;Ligne;Z1\n\
                   Z1;Zone nord;ZONE;S1\n\
                   S1;Usine;site;\n";
        let batch = bulk_import::create_batch(&db, input(csv), Some(1))
            .await
            .expect("batch");
        assert_eq!(batch.error_rows, 0);
        let res = bulk_import::apply_batch(&db, batch.id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (3, 0));
        assert_eq!(parent_code_of(&db, "L1").await.as_deref(), Some("Z1"));
        assert_eq!(parent_code_of(&db, "Z1").await.as_deref(), Some("S1"));
        assert_eq!(parent_code_of(&db, "S1").await, None);
    }

    #[tokio::test]
    async fn bad_lines_are_refused_with_a_named_reason() {
        let db = setup().await;
        let csv = "code,libelle,type,parent\n\
                   S1,Usine,SITE,\n\
                   Z1,Zone,ZZZ,S1\n\
                   Z2,Zone sans parent connu,ZONE,NOPE\n\
                   L1,Ligne sous le site,LINE,S1\n\
                   A,Boucle A,ZONE,B\n\
                   B,Boucle B,ZONE,A\n\
                   Z3,Zone sans parent,ZONE,\n\
                   S1,Doublon,SITE,\n";
        let batch = bulk_import::create_batch(&db, input(csv), Some(1))
            .await
            .expect("batch");
        let preview = bulk_import::get_preview(&db, batch.id).await.expect("preview");
        let codes: Vec<Vec<String>> = preview
            .rows
            .iter()
            .map(|r| r.messages.iter().map(|m| m.code.clone()).collect())
            .collect();
        assert!(codes[0].is_empty());
        assert_eq!(codes[1], vec!["ORG_IMPORT_TYPE"]);
        assert_eq!(codes[2], vec!["ORG_IMPORT_PARENT_UNKNOWN"]);
        assert_eq!(codes[3], vec!["ORG_IMPORT_RULE"]);
        assert_eq!(codes[4], vec!["ORG_IMPORT_CYCLE"]);
        assert_eq!(codes[5], vec!["ORG_IMPORT_CYCLE"]);
        assert_eq!(codes[6], vec!["ORG_IMPORT_PARENT_REQUIRED"]);
        assert_eq!(codes[7], vec!["ORG_IMPORT_DUPLICATE"]);
        assert_eq!(batch.error_rows, 7);
        // Only the good line is written, and the refused ones are not.
        let res = bulk_import::apply_batch(&db, batch.id, 1).await.expect("apply");
        assert_eq!((res.created, res.failed), (1, 0));
    }

    #[tokio::test]
    async fn existing_node_is_renamed_never_moved_and_a_rerun_changes_nothing() {
        let db = setup().await;
        let first = "code;libellé;type de nœud;code parent\n\
                     S1;Usine;SITE;\n\
                     Z1;Zone A;ZONE;S1\n\
                     Z2;Zone B;ZONE;S1\n\
                     L1;Ligne;LINE;Z1\n";
        let b1 = bulk_import::create_batch(&db, input(first), Some(1))
            .await
            .expect("batch 1");
        bulk_import::apply_batch(&db, b1.id, 1).await.expect("apply 1");

        let again = bulk_import::create_batch(&db, input(first), Some(1))
            .await
            .expect("batch again");
        let p = bulk_import::get_preview(&db, again.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));

        let edit = "code;libellé;type de nœud;code parent\n\
                    Z1;Zone Nord;ZONE;S1\n\
                    L1;Ligne;LINE;Z2\n";
        let b2 = bulk_import::create_batch(&db, input(edit), Some(1))
            .await
            .expect("batch 2");
        let p = bulk_import::get_preview(&db, b2.id).await.expect("preview 2");
        assert_eq!(p.rows[0].action, "update");
        assert_eq!(p.rows[1].messages[0].code, "ORG_IMPORT_MOVE");
        let res = bulk_import::apply_batch(&db, b2.id, 1).await.expect("apply 2");
        assert_eq!((res.updated, res.created), (1, 0));
        assert_eq!(parent_code_of(&db, "L1").await.as_deref(), Some("Z1"));
        let name: String = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM org_nodes WHERE code = 'Z1'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "name")
            .unwrap();
        assert_eq!(name, "Zone Nord");
    }

    #[tokio::test]
    async fn missing_required_column_is_refused_and_template_matches_export() {
        let db = setup().await;
        let err = bulk_import::create_batch(&db, input("code;libellé\nS1;Usine\n"), Some(1))
            .await
            .expect_err("columns");
        assert!(format!("{err:?}").contains("GATE_BULK_IMPORT_COLUMNS"));

        let b = bulk_import::create_batch(
            &db,
            input("code;libellé;type de nœud;code parent\nS1;Usine;SITE;\nZ1;Zone;ZONE;S1\n"),
            Some(1),
        )
        .await
        .expect("batch");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        let template = bulk_import::template("org_tree").expect("template");
        let exported = bulk_import::export(&db, "org_tree").await.expect("export");
        assert_eq!(template.header, exported.header);
        assert_eq!(exported.rows.len(), 2);

        // The exported rows can be loaded again without any change.
        let mut csv = exported.header.join(";");
        for r in &exported.rows {
            csv.push('\n');
            csv.push_str(&r.join(";"));
        }
        let again = bulk_import::create_batch(&db, input(&csv), Some(1))
            .await
            .expect("re-import");
        let p = bulk_import::get_preview(&db, again.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.action == "skip" && r.status == "ok"));
    }

    const BASE: &str = "code;libellé;type de nœud;code parent;actif\n\
                        S1;Usine;SITE;;\n\
                        Z1;Zone A;ZONE;S1;\n\
                        Z2;Zone B;ZONE;S1;\n\
                        L1;Ligne;LINE;Z1;\n";

    #[tokio::test]
    async fn a_batch_cannot_be_applied_twice() {
        let db = setup().await;
        let b = bulk_import::create_batch(&db, input(BASE), Some(1))
            .await
            .expect("batch");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("first apply");
        let err = bulk_import::apply_batch(&db, b.id, 1).await.expect_err("second apply");
        assert!(format!("{err:?}").contains("GATE_BULK_IMPORT_STATUS"));
    }

    #[tokio::test]
    async fn deactivation_rules_follow_the_whole_file() {
        let db = setup().await;
        let b = bulk_import::create_batch(&db, input(BASE), Some(1))
            .await
            .expect("batch");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("apply base");

        // A parent cannot be deactivated while its child stays active.
        let alone = "code;libellé;type de nœud;code parent;actif\nZ1;Zone A;ZONE;S1;non\n";
        let b = bulk_import::create_batch(&db, input(alone), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows[0].messages[0].code, "ORG_IMPORT_ACTIVE_CHILDREN");

        // A new node under a parent that is being deactivated is refused.
        let under = "code;libellé;type de nœud;code parent;actif\n\
                     Z2;Zone B;ZONE;S1;non\n\
                     L9;Nouvelle ligne;LINE;Z2;\n";
        let b = bulk_import::create_batch(&db, input(under), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows[1].messages[0].code, "ORG_IMPORT_PARENT_INACTIVE");

        // Parent listed before its child: both go, the child first, and the extra right is needed.
        let both = "code;libellé;type de nœud;code parent;actif\n\
                    Z1;Zone A;ZONE;S1;non\n\
                    L1;Ligne;LINE;Z1;non\n";
        let b = bulk_import::create_batch(&db, input(both), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert!(p.rows.iter().all(|r| r.status == "ok" && r.action == "update"));
        assert_eq!(
            bulk_import::extra_permission(&db, b.id).await.expect("extra"),
            Some(crate::rbac::permissions::ORG_ADMIN)
        );
        let res = bulk_import::apply_batch(&db, b.id, 1).await.expect("apply");
        assert_eq!((res.updated, res.failed), (2, 0));

        // A plain rename needs nothing beyond the type's own permission.
        let rename = "code;libellé;type de nœud;code parent\nZ2;Zone Sud;ZONE;S1\n";
        let b = bulk_import::create_batch(&db, input(rename), Some(1))
            .await
            .expect("batch");
        assert_eq!(bulk_import::extra_permission(&db, b.id).await.expect("extra"), None);
    }

    #[tokio::test]
    async fn a_refused_deactivation_also_refuses_the_one_above_it() {
        let db = setup().await;
        let b = bulk_import::create_batch(&db, input(BASE), Some(1))
            .await
            .expect("batch");
        bulk_import::apply_batch(&db, b.id, 1).await.expect("apply base");

        // Z1 keeps its active child L1 (not in the file), so S1 cannot go either.
        let chain = "code;libellé;type de nœud;code parent;actif\n\
                     S1;Usine;SITE;;non\n\
                     Z1;Zone A;ZONE;S1;non\n\
                     Z2;Zone B;ZONE;S1;non\n";
        let b = bulk_import::create_batch(&db, input(chain), Some(1))
            .await
            .expect("batch");
        let p = bulk_import::get_preview(&db, b.id).await.expect("preview");
        assert_eq!(p.rows[0].messages[0].code, "ORG_IMPORT_ACTIVE_CHILDREN");
        assert_eq!(p.rows[1].messages[0].code, "ORG_IMPORT_ACTIVE_CHILDREN");
        assert_eq!((p.rows[2].status.as_str(), p.rows[2].action.as_str()), ("ok", "update"));
    }

    #[tokio::test]
    async fn a_file_with_too_many_lines_is_refused() {
        let db = setup().await;
        let mut csv = String::from("code;libellé;type de nœud;code parent\n");
        for i in 0..5001 {
            csv.push_str(&format!("S{i};Site {i};SITE;\n"));
        }
        let err = bulk_import::create_batch(&db, input(&csv), Some(1))
            .await
            .expect_err("too many lines");
        assert!(format!("{err:?}").contains("trop de lignes"));
    }
}
