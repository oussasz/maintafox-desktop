//! CI guards for the sync entity registry (Phase 2.2).

use std::collections::{BTreeSet, HashMap, HashSet};

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

use super::registry::{self, SyncScope, LOCAL_ONLY_TABLES, SYNC_ENTITIES};

async fn migrated_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    db
}

async fn table_names(db: &DatabaseConnection) -> BTreeSet<String> {
    db.query_all(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'".to_string(),
    ))
    .await
    .expect("list tables")
    .iter()
    .map(|r| r.try_get::<String>("", "name").expect("table name"))
    .collect()
}

async fn columns(db: &DatabaseConnection, table: &str) -> Vec<String> {
    db.query_all(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("PRAGMA table_info('{table}')"),
    ))
    .await
    .expect("table_info")
    .iter()
    .map(|r| r.try_get::<String>("", "name").expect("column name"))
    .collect()
}

/// Fails when a migration adds a table that is neither replicated (registry entry) nor marked
/// local-only, and when either list names a table that no longer exists.
#[tokio::test]
async fn registry_covers_every_table() {
    let db = migrated_db().await;
    let tables = table_names(&db).await;

    let mut seen: HashMap<&str, &str> = HashMap::new();
    for e in SYNC_ENTITIES {
        assert!(
            seen.insert(e.table, "registry").is_none(),
            "table {} registered twice",
            e.table
        );
    }
    for t in LOCAL_ONLY_TABLES {
        assert!(
            seen.insert(t.table, "local_only").is_none(),
            "table {} is both registered and local-only (or listed twice)",
            t.table
        );
    }

    let missing: Vec<&String> = tables.iter().filter(|t| !seen.contains_key(t.as_str())).collect();
    assert!(
        missing.is_empty(),
        "Tables without a sync registry entry or a local-only mark (add them to sync/registry.rs): {missing:?}"
    );
    let stale: Vec<&&str> = seen.keys().filter(|t| !tables.contains(**t)).collect();
    assert!(stale.is_empty(), "Registry names tables that do not exist: {stale:?}");
}

#[tokio::test]
async fn registered_tables_with_stagers_have_sync_identity_columns() {
    let db = migrated_db().await;
    for e in SYNC_ENTITIES.iter().filter(|e| e.has_stager || e.has_materializer) {
        let cols = columns(&db, e.table).await;
        if super::row_spec::row_spec(e.entity_type).is_some() && !cols.iter().any(|c| c == "id") {
            // Keyed by a natural primary key: identified by `seeded_entity_sync_id` of that key.
            continue;
        }
        assert!(
            cols.iter().any(|c| c == "entity_sync_id"),
            "{} is synchronised but has no entity_sync_id column",
            e.table
        );
    }
}

#[tokio::test]
async fn user_scoped_entities_name_an_existing_owner_column() {
    let db = migrated_db().await;
    for e in SYNC_ENTITIES {
        match e.scope {
            SyncScope::User => {
                let column = e
                    .owner_user_column
                    .unwrap_or_else(|| panic!("{} is user-scoped without owner column", e.entity_type));
                let cols = columns(&db, e.table).await;
                assert!(
                    cols.iter().any(|c| c == column),
                    "{}.{column} (owner) does not exist",
                    e.table
                );
            }
            SyncScope::Tenant => assert!(
                e.owner_user_column.is_none(),
                "{} is tenant-scoped but names an owner column",
                e.entity_type
            ),
        }
    }
}

#[test]
fn registry_parent_graph_is_acyclic_and_closed() {
    let known: HashSet<&str> = SYNC_ENTITIES.iter().map(|e| e.entity_type).collect();
    for e in SYNC_ENTITIES {
        for p in e.parents {
            assert!(known.contains(p), "{} lists unknown parent {p}", e.entity_type);
            assert_ne!(*p, e.entity_type, "{} lists itself as parent", e.entity_type);
        }
    }
    fn visit<'a>(node: &'a str, stack: &mut Vec<&'a str>, done: &mut HashSet<&'a str>) {
        if done.contains(node) {
            return;
        }
        assert!(!stack.contains(&node), "registry parent cycle: {stack:?} -> {node}");
        stack.push(node);
        for p in registry::entity(node).map(|e| e.parents).unwrap_or(&[]) {
            visit(p, stack, done);
        }
        stack.pop();
        done.insert(node);
    }
    let mut done = HashSet::new();
    for e in SYNC_ENTITIES {
        visit(e.entity_type, &mut Vec::new(), &mut done);
    }
    for e in SYNC_ENTITIES {
        for p in e.parents {
            assert!(
                registry::apply_priority(p) < registry::apply_priority(e.entity_type),
                "{p} must be applied before {}",
                e.entity_type
            );
        }
    }
}

/// Declared SQLite foreign keys between registered tables must be reflected as parents, so a
/// child is never applied before the row it points to.
#[tokio::test]
async fn registry_parents_cover_declared_foreign_keys() {
    let db = migrated_db().await;
    let registered: HashSet<&str> = SYNC_ENTITIES.iter().map(|e| e.table).collect();
    for e in SYNC_ENTITIES {
        let fks = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("PRAGMA foreign_key_list('{}')", e.table),
            ))
            .await
            .expect("foreign_key_list");
        for fk in fks {
            let target: String = fk.try_get("", "table").expect("fk table");
            if target == e.table || !registered.contains(target.as_str()) {
                continue;
            }
            assert!(
                e.parents.contains(&target.as_str()),
                "{} has a foreign key to {target} but does not list it as parent",
                e.entity_type
            );
        }
    }
}

/// Every synchronised entity is captured and applied by the generic engine (`sync::row_spec`);
/// an entity is never staged without being applied, or the other way round.
#[test]
fn registry_flags_match_stager_and_materializer_dispatch() {
    // Every synchronised entity is captured and applied by the generic engine (Phase 2.4): a
    // registered type without a row spec would be neither sent nor applied.
    let missing: Vec<&str> = SYNC_ENTITIES
        .iter()
        .filter(|e| super::row_spec::row_spec(e.entity_type).is_none())
        .map(|e| e.entity_type)
        .collect();
    assert!(missing.is_empty(), "registered types without a row spec: {missing:?}");
    for e in SYNC_ENTITIES {
        assert!(
            e.has_stager && e.has_materializer,
            "{} has a row spec: mark has_stager and has_materializer",
            e.entity_type
        );
    }
}

fn shared_path(file: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("shared")
        .join("sync")
        .join(file)
}

/// `shared/sync/entity-registry.json` is generated from the registry. Regenerate it with
/// `MAINTAFOX_WRITE_SYNC_REGISTRY=1 cargo test --lib -- registry_export_matches_shared_file`.
#[test]
fn registry_export_matches_shared_file() {
    let generated = registry::registry_export_json().expect("export registry");
    let path = shared_path("entity-registry.json");
    if std::env::var("MAINTAFOX_WRITE_SYNC_REGISTRY").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("create shared/sync");
        std::fs::write(&path, &generated).expect("write registry export");
    }
    let committed =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is missing ({e}); regenerate it", path.display()));
    // Compared as JSON values: formatting (line endings, array wrapping) does not matter.
    let committed: serde_json::Value = serde_json::from_str(&committed).expect("committed registry json");
    let generated: serde_json::Value = serde_json::from_str(&generated).expect("generated registry json");
    assert_eq!(
        committed, generated,
        "shared/sync/entity-registry.json is out of date; regenerate it"
    );
}

/// Every entity this desktop stages must be accepted by the control-plane allowlist (a copy of
/// `maintafox-vendor-console/shared/sync-entity-registry.v2.json`) with the same scope.
#[test]
fn staged_entities_are_in_the_server_allowlist() {
    let path = shared_path("server-entity-allowlist.v2.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is missing: {e}", path.display()));
    let doc: serde_json::Value = serde_json::from_str(&raw).expect("allowlist json");
    let allowed: HashMap<String, String> = doc["entity_types"]
        .as_array()
        .expect("entity_types array")
        .iter()
        .map(|e| {
            (
                e["entity_type"].as_str().expect("entity_type").to_string(),
                e["scope"].as_str().expect("scope").to_string(),
            )
        })
        .collect();
    for e in SYNC_ENTITIES.iter().filter(|e| e.has_stager) {
        let scope = allowed
            .get(e.entity_type)
            .unwrap_or_else(|| panic!("{} is staged but not in the server allowlist", e.entity_type));
        let expected = match e.scope {
            SyncScope::Tenant => "tenant",
            SyncScope::User => "user",
        };
        assert_eq!(scope, expected, "scope mismatch for {}", e.entity_type);
    }
}

fn rust_sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "migrations") {
                continue;
            }
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let is_test_file = name == "tests.rs" || name.ends_with("_tests.rs") || name.contains("_test");
            if !is_test_file {
                out.push(path);
            }
        }
    }
}

/// Every raw-SQL insert into a table staged by a hand-written stager must set `entity_sync_id` at
/// creation, so the row has its merge key from the first write (migrations and test fixtures are
/// excluded). Tables on the generic capture get their identity from the capture insert trigger in
/// the same statement when the insert does not set one (`sync::capture`).
#[test]
fn inserts_into_synchronised_tables_set_entity_sync_id() {
    let synchronised: HashSet<&str> = SYNC_ENTITIES
        .iter()
        .filter(|e| e.has_stager || e.has_materializer)
        .filter(|e| super::row_spec::row_spec(e.entity_type).is_none())
        .map(|e| e.table)
        .collect();
    // `[\s\\]*` also skips Rust string line continuations between the table name and `(`.
    let insert =
        regex::Regex::new(r"(?is)INSERT\s+(?:OR\s+\w+\s+)?INTO\s+(\w+)[\s\\]*(\(([^)]*)\))?").expect("insert regex");
    let mut files = Vec::new();
    rust_sources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut violations = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("read source");
        for caps in insert.captures_iter(&source) {
            let table = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            if !synchronised.contains(table) {
                continue;
            }
            let columns = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
            if !columns.contains("entity_sync_id") {
                let line = source[..caps.get(0).map(|m| m.start()).unwrap_or_default()]
                    .matches('\n')
                    .count()
                    + 1;
                violations.push(format!("{}:{line} ({table})", file.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Inserts into synchronised tables without entity_sync_id:\n{}",
        violations.join("\n")
    );
}

/// Rows every device seeds itself (migrations and startup seeders) must have the same identity on
/// every database, or two devices would hold two copies of the same catalog row; and none of them
/// is queued for sync.
#[tokio::test]
async fn seeded_rows_have_the_same_identity_on_every_database() {
    let a = super::test_support::started_db().await;
    let b = super::test_support::started_db().await;
    let mut mismatches = Vec::new();
    for e in SYNC_ENTITIES {
        if !columns(&a, e.table).await.iter().any(|c| c == "entity_sync_id") {
            continue;
        }
        let ids = |db: &DatabaseConnection| {
            let sql = format!("SELECT entity_sync_id FROM {} ORDER BY entity_sync_id", e.table);
            let db = db.clone();
            async move {
                db.query_all(Statement::from_string(DatabaseBackend::Sqlite, sql))
                    .await
                    .expect("read ids")
                    .iter()
                    .map(|r| r.try_get::<Option<String>>("", "entity_sync_id").expect("id"))
                    .collect::<Vec<_>>()
            }
        };
        let (ids_a, ids_b) = (ids(&a).await, ids(&b).await);
        if ids_a != ids_b {
            mismatches.push(format!("{}: {} vs {} rows", e.table, ids_a.len(), ids_b.len()));
            continue;
        }
        for id in ids_a {
            match id {
                Some(id) if id.as_bytes().get(14) == Some(&b'8') => {}
                other => mismatches.push(format!("{}: seeded row without seeded identity ({other:?})", e.table)),
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "Seeded rows must use sync::seeded_identity / seeded_entity_sync_id:\n{}",
        mismatches.join("\n")
    );
    let queued = super::capture::pending_capture_count(&a).await.expect("journal count");
    assert_eq!(queued, 0, "seeded rows must not be queued for sync");
}

/// Every entity type on the generic engine has a valid column map (no unmapped local id column,
/// no unknown table or column in its rules), and after installation every one of them has its
/// three capture triggers.
#[tokio::test]
async fn every_row_spec_has_a_valid_layout_and_capture_triggers() {
    let db = super::test_support::started_db().await;
    let mut problems = Vec::new();
    for spec in super::row_spec::ROW_SPECS {
        if registry::entity(spec.entity_type).is_none() {
            problems.push(format!(
                "{}: row spec for an unregistered entity type",
                spec.entity_type
            ));
            continue;
        }
        if let Err(e) = super::row_layout::layout(&db, spec.entity_type).await {
            problems.push(format!("{}: {e}", spec.entity_type));
            continue;
        }
        let table = registry::entity(spec.entity_type).map(|e| e.table).unwrap_or_default();
        let triggers = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'trigger' AND tbl_name = '{table}'
                       AND name IN ('sync_cap_{table}_ai', 'sync_cap_{table}_au', 'sync_cap_{table}_ad')"
                ),
            ))
            .await
            .expect("triggers")
            .and_then(|r| r.try_get::<i64>("", "n").ok())
            .unwrap_or_default();
        if triggers != 3 {
            problems.push(format!("{}: {triggers} capture triggers installed", spec.entity_type));
        }
    }
    assert!(problems.is_empty(), "row spec problems:\n{}", problems.join("\n"));
}

/// Undeclared references must be parents too, so the apply order puts their targets first (back
/// edges of a cycle are declared `soft`; history links may point to rows applied later).
#[test]
fn row_spec_references_are_registry_parents() {
    let mut missing = Vec::new();
    for spec in super::row_spec::ROW_SPECS {
        let Some(def) = registry::entity(spec.entity_type) else {
            continue;
        };
        for r in spec.refs {
            if r.soft || r.history {
                continue;
            }
            let targets: Vec<&str> = match r.target {
                super::row_spec::RefTarget::Entity(t) => vec![t],
                super::row_spec::RefTarget::Poly { targets, .. } => targets
                    .iter()
                    .map(|(_, t)| *t)
                    .filter(|t| *t != super::row_spec::NOT_A_ROW && *t != super::row_spec::LOCAL_ROW)
                    .collect(),
                _ => vec![],
            };
            for t in targets {
                if t != def.entity_type && !def.parents.contains(&t) {
                    missing.push(format!("{} -> {t} ({})", def.entity_type, r.column));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "add these targets to the registry parents (or mark the reference soft):\n{}",
        missing.join("\n")
    );
}
