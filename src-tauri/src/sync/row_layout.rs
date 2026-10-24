//! Column map of a replicated table for the generic sync engine (Phase 2.4).
//!
//! Built once per entity type from the SQLite schema (`PRAGMA table_info`, `foreign_key_list`) and
//! the rules in `sync::row_spec`. Every column gets exactly one role; an integer `*_id` column that
//! is neither a declared foreign key, a listed reference nor a listed value column is an error, so
//! a new local-id column can never leak into a payload unnoticed.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::registry::{self, SyncEntityDef};
use crate::sync::row_spec::{self, RefTarget, RowSpec};

/// Payload key carrying the payload format; payloads without it are not applied.
pub(crate) const PAYLOAD_FORMAT_KEY: &str = "_sync_format";
/// Version of the generic row payload.
pub(crate) const PAYLOAD_FORMAT: &str = "row.v1";

#[derive(Debug, Clone)]
pub(crate) enum Target {
    Entity {
        entity_type: &'static str,
        table: &'static str,
    },
    Catalog {
        table: &'static str,
        key_column: &'static str,
    },
    Poly {
        type_column: &'static str,
        /// `(type value, entity type, table)`.
        targets: Vec<(&'static str, &'static str, &'static str)>,
    },
    /// Must stay NULL to be synchronised (`RefTarget::Unmapped`).
    Unmapped,
}

#[derive(Debug, Clone)]
pub(crate) enum Role {
    /// Local integer primary key `id`; never sent.
    LocalKey,
    /// `entity_sync_id`: carried by the envelope.
    Identity,
    /// Local optimistic-concurrency counter; bumped locally on every applied change.
    LocalVersion,
    /// Describes this device only; never sent, never overwritten.
    DeviceLocal,
    /// Recomputed on this device after a row is applied; `initial` (SQL literal) on insert.
    Derived {
        initial: &'static str,
    },
    Value {
        key: String,
    },
    Ref {
        key: String,
        target: Target,
        soft: bool,
        /// History link: sent as NULL when the target no longer exists here.
        history: bool,
    },
}

/// One step of a JSON reference path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonSeg {
    Each,
    Key(String),
    /// `?name`: the column holds a URL (`/path?a=1&b=2`); the id is query parameter `name`.
    Query(String),
}

/// Local ids inside a JSON text column (`RowSpec::json_refs`).
#[derive(Debug, Clone)]
pub(crate) struct JsonRef {
    pub path: Vec<JsonSeg>,
    pub entity_type: &'static str,
    pub table: &'static str,
    /// A target that no longer exists is sent empty (`row_spec::json_ref_hist`).
    pub history: bool,
}

fn parse_json_path(path: &str) -> AppResult<Vec<JsonSeg>> {
    if let Some(param) = path.strip_prefix('?') {
        if param.is_empty() || param.contains(['&', '=', '?']) {
            return Err(AppError::SyncError(format!("invalid URL reference path '{path}'")));
        }
        return Ok(vec![JsonSeg::Query(param.to_string())]);
    }
    let mut rest = path.strip_prefix('$').unwrap_or(path);
    let mut segs = Vec::new();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("[*]") {
            segs.push(JsonSeg::Each);
            rest = r;
        } else if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            if end == 0 {
                return Err(AppError::SyncError(format!("invalid JSON reference path '{path}'")));
            }
            segs.push(JsonSeg::Key(r[..end].to_string()));
            rest = &r[end..];
        } else {
            return Err(AppError::SyncError(format!("invalid JSON reference path '{path}'")));
        }
    }
    Ok(segs)
}

#[derive(Debug, Clone)]
pub(crate) struct LayoutColumn {
    pub name: String,
    /// Local ids inside this JSON text column (value columns only).
    pub json_refs: Vec<JsonRef>,
    /// Declared type is TEXT-like: a reference stored here keeps the local id as text.
    pub is_text: bool,
    pub is_blob: bool,
    pub role: Role,
}

#[derive(Debug)]
pub(crate) struct EntityLayout {
    pub def: &'static SyncEntityDef,
    pub spec: &'static RowSpec,
    pub columns: Vec<LayoutColumn>,
    /// Primary-key column positions when the table is not keyed by `id`: such rows are identified
    /// by their natural key (`seeded_entity_sync_id` of the key) and have no identity column.
    pub natural_pk: Option<Vec<usize>>,
    pub has_row_version: bool,
}

impl EntityLayout {
    pub fn table(&self) -> &'static str {
        self.def.table
    }

    pub fn column(&self, name: &str) -> Option<&LayoutColumn> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Payload key of the owner column of a user-scoped entity.
    pub fn owner_payload_key(&self) -> AppResult<Option<String>> {
        let Some(owner) = self.def.owner_user_column else {
            return Ok(None);
        };
        match self.column(owner).map(|c| &c.role) {
            Some(Role::Ref { key, .. }) => Ok(Some(key.clone())),
            _ => Err(AppError::SyncError(format!(
                "{}.{owner} (owner) is not a reference to user_accounts.",
                self.def.table
            ))),
        }
    }

    /// SQL row filter with `{row}` replaced by `alias`, or `1 = 1`.
    pub fn filter_sql(&self, alias: &str) -> String {
        self.spec
            .row_filter
            .map_or_else(|| "1 = 1".to_string(), |f| format!("({})", f.replace("{row}", alias)))
    }
}

/// `equipment_id` → `equipment_sync_id`, `scope_reference` → `scope_reference_sync_id`.
pub(crate) fn ref_payload_key(column: &str) -> String {
    match column.strip_suffix("_id") {
        Some(base) => format!("{base}_sync_id"),
        None => format!("{column}_sync_id"),
    }
}

/// `permission_id` → `permission_key`.
fn catalog_payload_key(column: &str) -> String {
    match column.strip_suffix("_id") {
        Some(base) => format!("{base}_key"),
        None => format!("{column}_key"),
    }
}

fn layouts() -> &'static Mutex<HashMap<&'static str, Arc<EntityLayout>>> {
    static LAYOUTS: OnceLock<Mutex<HashMap<&'static str, Arc<EntityLayout>>>> = OnceLock::new();
    LAYOUTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn replicated_entity_for_table(table: &str) -> Option<&'static SyncEntityDef> {
    registry::SYNC_ENTITIES.iter().find(|e| e.table == table)
}

fn entity_target(entity_type: &str) -> AppResult<Target> {
    let def = registry::require_entity(entity_type)?;
    Ok(Target::Entity {
        entity_type: def.entity_type,
        table: def.table,
    })
}

/// Layout of an entity synchronised by the generic engine. The schema is the same for every
/// database of one app version, so the layout is computed once per process.
pub(crate) async fn layout(conn: &impl ConnectionTrait, entity_type: &str) -> AppResult<Arc<EntityLayout>> {
    let cached = layouts()
        .lock()
        .map_err(|_| AppError::SyncError("sync layout cache poisoned".into()))?
        .get(entity_type)
        .cloned();
    if let Some(found) = cached {
        // Tests also build databases from before a migration (upgrade tests): a layout cached from
        // one of them is not used on a database of another schema (built again, not cached).
        #[cfg(test)]
        {
            let columns = conn
                .query_all(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("SELECT name FROM pragma_table_info('{}')", found.def.table),
                ))
                .await?;
            if columns.len() != found.columns.len() {
                return Ok(Arc::new(build_layout(conn, entity_type).await?));
            }
        }
        return Ok(found);
    }
    let built = Arc::new(build_layout(conn, entity_type).await?);
    layouts()
        .lock()
        .map_err(|_| AppError::SyncError("sync layout cache poisoned".into()))?
        .insert(built.def.entity_type, built.clone());
    Ok(built)
}

struct RawColumn {
    name: String,
    decl_type: String,
    pk: i64,
}

fn meta_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("schema metadata: {e}"))
}

async fn build_layout(conn: &impl ConnectionTrait, entity_type: &str) -> AppResult<EntityLayout> {
    let def = registry::require_entity(entity_type)?;
    let spec = row_spec::row_spec(entity_type).ok_or_else(|| {
        AppError::SyncError(format!(
            "'{entity_type}' is not synchronised by the generic engine (no row spec)."
        ))
    })?;
    let table = def.table;

    let raw: Vec<RawColumn> = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info('{table}')"),
        ))
        .await?
        .iter()
        .map(|r| -> AppResult<RawColumn> {
            Ok(RawColumn {
                name: r.try_get("", "name").map_err(meta_err)?,
                decl_type: r.try_get::<String>("", "type").map_err(meta_err)?.to_ascii_uppercase(),
                pk: r.try_get("", "pk").map_err(meta_err)?,
            })
        })
        .collect::<AppResult<_>>()?;
    if raw.is_empty() {
        return Err(AppError::SyncError(format!("table '{table}' does not exist.")));
    }

    let mut declared: HashMap<String, (String, Option<String>)> = HashMap::new();
    for fk in conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA foreign_key_list('{table}')"),
        ))
        .await?
    {
        let from: String = fk.try_get("", "from").map_err(meta_err)?;
        let target: String = fk.try_get("", "table").map_err(meta_err)?;
        let to: Option<String> = fk.try_get("", "to").map_err(meta_err)?;
        declared.insert(from, (target, to));
    }

    let mut pk_cols: Vec<(i64, usize)> = raw
        .iter()
        .enumerate()
        .filter(|(_, c)| c.pk > 0)
        .map(|(i, c)| (c.pk, i))
        .collect();
    pk_cols.sort_unstable();
    let keyed_by_id = pk_cols.len() == 1 && raw[pk_cols[0].1].name == "id";

    let spec_refs: HashMap<&str, _> = spec.refs.iter().map(|r| (r.column, r)).collect();
    let values: HashSet<&str> = spec.value_columns.iter().copied().collect();
    let device: HashSet<&str> = spec.device_local.iter().copied().collect();
    let derived: HashMap<&str, &'static str> = spec.derived.iter().copied().collect();
    for j in spec.json_refs {
        if !raw.iter().any(|c| c.name == j.column) {
            return Err(AppError::SyncError(format!(
                "row spec of '{entity_type}' names JSON column '{}' that {table} does not have.",
                j.column
            )));
        }
    }
    for listed in spec_refs
        .keys()
        .chain(values.iter())
        .chain(device.iter())
        .chain(derived.keys())
    {
        if !raw.iter().any(|c| c.name == *listed) {
            return Err(AppError::SyncError(format!(
                "row spec of '{entity_type}' names column '{listed}' that {table} does not have."
            )));
        }
    }

    let mut columns = Vec::with_capacity(raw.len());
    let mut has_row_version = false;
    for c in &raw {
        let name = c.name.as_str();
        let is_text = c.decl_type.contains("TEXT") || c.decl_type.contains("CHAR") || c.decl_type.contains("CLOB");
        let is_blob = c.decl_type.contains("BLOB");
        let role = if keyed_by_id && name == "id" {
            Role::LocalKey
        } else if keyed_by_id && name == "entity_sync_id" {
            Role::Identity
        } else if name == "row_version" {
            has_row_version = true;
            Role::LocalVersion
        } else if device.contains(name) {
            Role::DeviceLocal
        } else if let Some(initial) = derived.get(name) {
            Role::Derived { initial }
        } else if let Some(spec_ref) = spec_refs.get(name) {
            let target = match spec_ref.target {
                RefTarget::Entity(entity) => entity_target(entity)?,
                RefTarget::Catalog { table, key_column } => Target::Catalog { table, key_column },
                RefTarget::Poly { type_column, targets } => {
                    if !raw.iter().any(|x| x.name == type_column) {
                        return Err(AppError::SyncError(format!(
                            "row spec of '{entity_type}': type column '{type_column}' does not exist."
                        )));
                    }
                    let mut resolved = Vec::with_capacity(targets.len());
                    for (type_value, entity) in targets {
                        if *entity == super::row_spec::NOT_A_ROW || *entity == super::row_spec::LOCAL_ROW {
                            resolved.push((*type_value, *entity, *entity));
                            continue;
                        }
                        let def = registry::require_entity(entity)?;
                        resolved.push((*type_value, def.entity_type, def.table));
                    }
                    Target::Poly {
                        type_column,
                        targets: resolved,
                    }
                }
                RefTarget::Unmapped => Target::Unmapped,
            };
            let key = match target {
                Target::Catalog { .. } => catalog_payload_key(name),
                _ => ref_payload_key(name),
            };
            Role::Ref {
                key,
                target,
                soft: spec_ref.soft,
                history: spec_ref.history,
            }
        } else if values.contains(name) {
            Role::Value { key: name.to_string() }
        } else if let Some((target_table, to)) = declared.get(name) {
            let to_pk = to.as_deref().is_none_or(|t| t == "id");
            match replicated_entity_for_table(target_table) {
                Some(target) if to_pk => Role::Ref {
                    key: ref_payload_key(name),
                    target: Target::Entity {
                        entity_type: target.entity_type,
                        table: target.table,
                    },
                    soft: false,
                    history: false,
                },
                // A foreign key on a natural column (for example a category code) is a value.
                Some(_) => Role::Value { key: name.to_string() },
                None => {
                    return Err(AppError::SyncError(format!(
                        "{table}.{name} references {target_table}, which is not replicated; \
                         declare it in the row spec (catalog key or value)."
                    )))
                }
            }
        } else if name.ends_with("_id") && c.decl_type.contains("INT") {
            return Err(AppError::SyncError(format!(
                "{table}.{name} looks like a local reference; declare it in the row spec \
                 (reference or value column)."
            )));
        } else {
            Role::Value { key: name.to_string() }
        };
        let mut json_refs = Vec::new();
        for j in spec.json_refs.iter().filter(|j| j.column == name) {
            if !matches!(role, Role::Value { .. }) || !is_text {
                return Err(AppError::SyncError(format!(
                    "{table}.{name}: JSON references need a text value column"
                )));
            }
            let target = registry::require_entity(j.entity)?;
            let path = parse_json_path(j.path)?;
            let is_url = matches!(path.as_slice(), [JsonSeg::Query(_)]);
            if json_refs
                .iter()
                .any(|r: &JsonRef| matches!(r.path.as_slice(), [JsonSeg::Query(_)]) != is_url)
            {
                return Err(AppError::SyncError(format!(
                    "{table}.{name}: a column holds either JSON or a URL, not both"
                )));
            }
            json_refs.push(JsonRef {
                path,
                entity_type: target.entity_type,
                table: target.table,
                history: j.history,
            });
        }
        columns.push(LayoutColumn {
            name: c.name.clone(),
            json_refs,
            is_text,
            is_blob,
            role,
        });
    }

    let natural_pk = if keyed_by_id {
        if !columns.iter().any(|c| matches!(c.role, Role::Identity)) {
            return Err(AppError::SyncError(format!("{table} has no entity_sync_id column.")));
        }
        None
    } else {
        if pk_cols.is_empty() {
            return Err(AppError::SyncError(format!("{table} has no primary key.")));
        }
        if columns.iter().any(|c| matches!(c.role, Role::Ref { soft: true, .. })) {
            return Err(AppError::SyncError(format!(
                "{table}: soft references need an entity_sync_id column."
            )));
        }
        Some(pk_cols.iter().map(|(_, i)| *i).collect())
    };

    let mut keys = HashSet::new();
    for c in &columns {
        let key = match &c.role {
            Role::Value { key } | Role::Ref { key, .. } => key,
            _ => continue,
        };
        if key == PAYLOAD_FORMAT_KEY || !keys.insert(key.clone()) {
            return Err(AppError::SyncError(format!(
                "{table}: payload key '{key}' is used twice; rename the reference in the row spec."
            )));
        }
    }

    Ok(EntityLayout {
        def,
        spec,
        columns,
        natural_pk,
        has_row_version,
    })
}
