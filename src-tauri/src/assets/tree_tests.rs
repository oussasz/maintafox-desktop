//! Equipment tree read level by level from the database (`assets::tree`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::assets::search::AssetSearchFilters;
use crate::assets::tree::list_asset_tree_level;

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn id_of(db: &DatabaseConnection, code: &str) -> i64 {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM equipment WHERE asset_id_code = ?",
        [code.into()],
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get("", "id")
    .expect("id")
}

/// Plant: ROOT → LINE → PUMP-1, PUMP-2 (each pump has a MOTOR) plus a lone root OTHER.
async fn seed(db: &DatabaseConnection) {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'tree-test-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
         VALUES (1, 'tree-test-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
         VALUES (1, 'tree-test-org', 'SITE-T', 'Tree site', 1, 'active', datetime('now'), datetime('now'), 1)",
    ] {
        exec(db, sql).await;
    }
    for code in [
        "T-ROOT",
        "T-LINE",
        "T-PUMP-1",
        "T-PUMP-2",
        "T-MOTOR-1",
        "T-MOTOR-2",
        "T-OTHER",
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
                 created_at, updated_at) VALUES ('sync-{code}', '{code}', 'Asset {code}', 'active_in_service', 1, \
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            ),
        )
        .await;
    }
    for (parent, child) in [
        ("T-ROOT", "T-LINE"),
        ("T-LINE", "T-PUMP-1"),
        ("T-LINE", "T-PUMP-2"),
        ("T-PUMP-1", "T-MOTOR-1"),
        ("T-PUMP-2", "T-MOTOR-2"),
    ] {
        let (p, c) = (id_of(db, parent).await, id_of(db, child).await);
        exec(
            db,
            &format!(
                "INSERT INTO equipment_hierarchy (parent_equipment_id, child_equipment_id, relationship_type, \
                 effective_from, created_at) VALUES ({p}, {c}, 'contains', \
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            ),
        )
        .await;
    }
}

fn filters(query: Option<&str>, limit: Option<u64>, offset: Option<u64>) -> AssetSearchFilters {
    AssetSearchFilters {
        query: query.map(str::to_string),
        class_codes: None,
        family_codes: None,
        status_codes: None,
        org_node_ids: None,
        include_decommissioned: None,
        limit,
        registry_focus: None,
        offset,
        criticality_codes: None,
        org_subtree: None,
    }
}

fn codes(level: &crate::assets::tree::AssetTreeLevel) -> Vec<&str> {
    level.items.iter().map(|r| r.asset.asset_code.as_str()).collect()
}

#[tokio::test]
async fn the_tree_is_read_from_the_hierarchy_one_level_at_a_time() {
    let db = crate::db::test_db::seeded().await;
    seed(&db).await;

    let roots = list_asset_tree_level(&db, filters(Some("T-"), None, None), None)
        .await
        .expect("roots");
    assert_eq!(codes(&roots), vec!["T-OTHER", "T-ROOT"]);
    let root = roots.items.iter().find(|r| r.asset.asset_code == "T-ROOT").unwrap();
    assert_eq!(root.child_count, 1);

    let line = list_asset_tree_level(&db, filters(Some("T-"), None, None), Some(root.asset.id))
        .await
        .expect("line");
    assert_eq!(codes(&line), vec!["T-LINE"]);
    let pumps = list_asset_tree_level(&db, filters(Some("T-"), None, None), Some(line.items[0].asset.id))
        .await
        .expect("pumps");
    assert_eq!(codes(&pumps), vec!["T-PUMP-1", "T-PUMP-2"]);
    assert!(pumps.items.iter().all(|r| r.child_count == 1 && r.is_match));

    // Paged level: the total says more exist.
    let page = list_asset_tree_level(&db, filters(Some("T-"), Some(1), Some(1)), Some(line.items[0].asset.id))
        .await
        .expect("page");
    assert_eq!(page.total, 2);
    assert_eq!(codes(&page), vec!["T-PUMP-2"]);
}

#[tokio::test]
async fn a_deep_match_is_reachable_through_its_ancestors() {
    let db = crate::db::test_db::seeded().await;
    seed(&db).await;
    let f = || filters(Some("T-MOTOR-2"), None, None);

    let roots = list_asset_tree_level(&db, f(), None).await.expect("roots");
    assert_eq!(codes(&roots), vec!["T-ROOT"], "only the branch leading to the match");
    assert!(!roots.items[0].is_match, "ancestor shown as context");
    let line = list_asset_tree_level(&db, f(), Some(roots.items[0].asset.id))
        .await
        .expect("line");
    let pumps = list_asset_tree_level(&db, f(), Some(line.items[0].asset.id))
        .await
        .expect("pumps");
    assert_eq!(codes(&pumps), vec!["T-PUMP-2"], "the other pump has no match below it");
    let motors = list_asset_tree_level(&db, f(), Some(pumps.items[0].asset.id))
        .await
        .expect("motors");
    assert_eq!(codes(&motors), vec!["T-MOTOR-2"]);
    assert!(motors.items[0].is_match);
    assert_eq!(motors.items[0].child_count, 0);
}
