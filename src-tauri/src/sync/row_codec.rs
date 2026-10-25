//! Generic row payloads (Phase 2.4): build the payload of a local row, and apply an inbound
//! payload to the local table.
//!
//! Payload (`row.v1`): a JSON object with every replicated column of the row, keys sorted.
//! - value columns keep their column name and SQLite value (integer, real, text, NULL; BLOB as
//!   base64);
//! - references are sent as the target's `entity_sync_id` under `<column without _id>_sync_id`
//!   (catalog references as the catalog key under `<column without _id>_key`);
//! - `_sync_format: "row.v1"`.
//!
//! Applying never invents a value: a column missing from the payload is left as it is (update) or
//! gets the table default (insert, which fails visibly when the column is required); a reference
//! whose target is not here yet defers the row.

use std::collections::BTreeMap;

use base64::Engine as _;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SqlValue};
use serde_json::{Map, Number, Value};

use crate::errors::{AppError, AppResult};
use crate::sync::identity::seeded_entity_sync_id;
use crate::sync::materialize::{ApplyOutcome, WaitsFor, DEFER_MISSING_PARENT};
use crate::sync::registry::ConflictPolicy;
use crate::sync::row_layout::{
    EntityLayout, JsonRef, JsonSeg, LayoutColumn, Role, Target, PAYLOAD_FORMAT, PAYLOAD_FORMAT_KEY,
};

fn codec_err(table: &str, msg: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("{table}: {msg}"))
}

/// Scalar read from SQLite with its storage class.
#[derive(Debug, Clone)]
enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// How to find one row of a replicated table.
#[derive(Debug, Clone)]
pub(crate) enum RowLocator {
    /// Tables keyed by `id`: the row's `entity_sync_id`.
    Identity(String),
    /// Tables keyed by a natural primary key: local values of the key columns, in key order.
    Natural(Vec<(String, SqlValue)>),
}

impl RowLocator {
    /// `WHERE` clause (columns qualified with `alias`, or bare when `alias` is empty) and values.
    fn where_sql(&self, alias: &str) -> (String, Vec<SqlValue>) {
        let q = |col: &str| {
            if alias.is_empty() {
                format!("\"{col}\"")
            } else {
                format!("{alias}.\"{col}\"")
            }
        };
        match self {
            RowLocator::Identity(id) => (format!("{} = ?", q("entity_sync_id")), vec![id.clone().into()]),
            RowLocator::Natural(parts) => (
                parts
                    .iter()
                    .map(|(col, _)| format!("{} = ?", q(col)))
                    .collect::<Vec<_>>()
                    .join(" AND "),
                parts.iter().map(|(_, v)| v.clone()).collect(),
            ),
        }
    }
}

/// A local row turned into a payload.
#[derive(Debug, Clone)]
pub(crate) struct RowPayload {
    pub entity_sync_id: String,
    pub payload: Map<String, Value>,
    /// Owner of a user-scoped row (`owner_user_sync_id`).
    pub owner_user_sync_id: Option<String>,
}

impl RowPayload {
    pub fn payload_json(&self) -> AppResult<String> {
        Ok(serde_json::to_string(&self.payload)?)
    }
}

async fn read_cells(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    locator: &RowLocator,
) -> AppResult<Option<Vec<Cell>>> {
    let table = layout.table();
    let select = layout
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("typeof(t.\"{0}\") AS \"t{i}\", t.\"{0}\" AS \"v{i}\"", c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let (where_sql, values) = locator.where_sql("t");
    let sql = format!(
        "SELECT {select} FROM \"{table}\" t WHERE {where_sql} AND {} LIMIT 1",
        layout.filter_sql("t")
    );
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, &sql, values))
        .await?
    else {
        return Ok(None);
    };
    let mut cells = Vec::with_capacity(layout.columns.len());
    for (i, c) in layout.columns.iter().enumerate() {
        let read = |e: sea_orm::DbErr| codec_err(table, format!("{}: {e}", c.name));
        let kind: String = row.try_get("", &format!("t{i}")).map_err(read)?;
        let v = format!("v{i}");
        let cell = match kind.as_str() {
            "null" => Cell::Null,
            "integer" => Cell::Int(row.try_get::<i64>("", &v).map_err(read)?),
            "real" => Cell::Real(row.try_get::<f64>("", &v).map_err(read)?),
            "text" => Cell::Text(row.try_get::<String>("", &v).map_err(read)?),
            "blob" => Cell::Blob(row.try_get::<Vec<u8>>("", &v).map_err(read)?),
            other => {
                return Err(codec_err(
                    table,
                    format!("{}: unexpected storage class {other}", c.name),
                ))
            }
        };
        cells.push(cell);
    }
    Ok(Some(cells))
}

/// Local id stored in a reference column (integer, or the text form used by text columns).
fn local_ref_id(table: &str, column: &LayoutColumn, cell: &Cell) -> AppResult<Option<i64>> {
    match cell {
        Cell::Null => Ok(None),
        Cell::Int(v) => Ok(Some(*v)),
        Cell::Text(t) if t.trim().is_empty() => Ok(None),
        Cell::Text(t) => t
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| codec_err(table, format!("{} holds '{t}', not a local id", column.name))),
        Cell::Real(_) | Cell::Blob(_) => Err(codec_err(table, format!("{} is not a local id", column.name))),
    }
}

async fn scalar_text(conn: &impl ConnectionTrait, sql: &str, values: Vec<SqlValue>) -> AppResult<Option<String>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => r
            .try_get::<Option<String>>("", "v")
            .map_err(|e| AppError::SyncError(e.to_string())),
    }
}

tokio::task_local! {
    /// Local ids found by sync id during one materialization pass (`materialize`): received rows
    /// share their parents (equipment, people, statuses), so each is looked up once per pass, not
    /// once per row. Only found ids are kept; a delete or a rolled-back row clears it.
    pub(crate) static REF_CACHE: std::cell::RefCell<std::collections::HashMap<(String, String), i64>>;
}

/// Forget the cached reference lookups of the current pass (no-op outside a pass).
pub(crate) fn clear_ref_cache() {
    let _ = REF_CACHE.try_with(|c| c.borrow_mut().clear());
}

/// [`scalar_id`] for a reference lookup `sql` by one key, through the pass cache.
async fn cached_ref_id(conn: &impl ConnectionTrait, sql: &str, key: &str) -> AppResult<Option<i64>> {
    let cache_key = (sql.to_string(), key.to_string());
    if let Ok(Some(id)) = REF_CACHE.try_with(|c| c.borrow().get(&cache_key).copied()) {
        return Ok(Some(id));
    }
    let found = scalar_id(conn, sql, vec![key.into()]).await?;
    if let Some(id) = found {
        let _ = REF_CACHE.try_with(|c| c.borrow_mut().insert(cache_key, id));
    }
    Ok(found)
}

pub(crate) async fn scalar_id(conn: &impl ConnectionTrait, sql: &str, values: Vec<SqlValue>) -> AppResult<Option<i64>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => r
            .try_get::<Option<i64>>("", "v")
            .map_err(|e| AppError::SyncError(e.to_string())),
    }
}

fn poly_target<'a>(
    targets: &'a [(&'static str, &'static str, &'static str)],
    type_value: Option<&str>,
) -> Option<&'a (&'static str, &'static str, &'static str)> {
    let type_value = type_value?;
    targets.iter().find(|(t, _, _)| *t == type_value)
}

fn cell_text(cell: Option<&Cell>) -> Option<String> {
    match cell {
        Some(Cell::Text(t)) => Some(t.clone()),
        Some(Cell::Int(i)) => Some(i.to_string()),
        _ => None,
    }
}

/// Value of a polymorphic reference whose type does not point to a row (`row_spec::NOT_A_ROW`).
fn verbatim_poly_value(layout: &EntityLayout, target: &Target, cell: &Cell, cells: &[Cell]) -> Option<Value> {
    let Target::Poly { type_column, targets } = target else {
        return None;
    };
    let type_index = layout.columns.iter().position(|c| c.name == *type_column)?;
    let (_, _, target_table) = poly_target(targets, cell_text(cells.get(type_index)).as_deref())?;
    match *target_table {
        crate::sync::row_spec::NOT_A_ROW => Some(cell_text(Some(cell)).map_or(Value::Null, Value::String)),
        crate::sync::row_spec::LOCAL_ROW => Some(Value::Null),
        _ => None,
    }
}

/// Sync id (or catalog key) of the row a reference cell points to.
async fn outbound_ref(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    column: &LayoutColumn,
    target: &Target,
    history: bool,
    cell: &Cell,
    cells: &[Cell],
) -> AppResult<Value> {
    let table = layout.table();
    if let Some(verbatim) = verbatim_poly_value(layout, target, cell, cells) {
        return Ok(verbatim);
    }
    let Some(local_id) = local_ref_id(table, column, cell)? else {
        return Ok(Value::Null);
    };
    let (sql, what) = match target {
        Target::Unmapped => {
            return Err(codec_err(
                table,
                format!(
                    "{} holds local id {local_id}, which has no synchronised meaning; it must be empty",
                    column.name
                ),
            ))
        }
        Target::Entity {
            table: target_table, ..
        } => (
            format!("SELECT entity_sync_id AS v FROM \"{target_table}\" WHERE id = ?"),
            (*target_table).to_string(),
        ),
        Target::Catalog {
            table: catalog,
            key_column,
        } => (
            format!("SELECT \"{key_column}\" AS v FROM \"{catalog}\" WHERE id = ?"),
            (*catalog).to_string(),
        ),
        Target::Poly { type_column, targets } => {
            let type_index = layout
                .columns
                .iter()
                .position(|c| c.name == *type_column)
                .ok_or_else(|| codec_err(table, format!("type column {type_column} missing")))?;
            let type_value = cell_text(cells.get(type_index));
            let Some((_, _, target_table)) = poly_target(targets, type_value.as_deref()) else {
                return Err(codec_err(
                    table,
                    format!(
                        "{} = {local_id} with {type_column} = {:?}: this kind of reference is not mapped for sync",
                        column.name, type_value
                    ),
                ));
            };
            (
                format!("SELECT entity_sync_id AS v FROM \"{target_table}\" WHERE id = ?"),
                (*target_table).to_string(),
            )
        }
    };
    match scalar_text(conn, &sql, vec![local_id.into()]).await? {
        Some(v) if !v.trim().is_empty() => Ok(Value::String(v)),
        // The event outlives the row it describes.
        _ if history => Ok(Value::Null),
        _ => Err(codec_err(
            table,
            format!(
                "{} points to {what} row {local_id}, which does not exist or has no sync identity",
                column.name
            ),
        )),
    }
}

fn outbound_value(table: &str, column: &LayoutColumn, cell: &Cell) -> AppResult<Value> {
    Ok(match cell {
        Cell::Null => Value::Null,
        Cell::Int(v) => Value::Number((*v).into()),
        Cell::Real(v) => Value::Number(
            Number::from_f64(*v).ok_or_else(|| codec_err(table, format!("{} is not a finite number", column.name)))?,
        ),
        Cell::Text(t) => Value::String(t.clone()),
        Cell::Blob(b) => {
            if !column.is_blob {
                return Err(codec_err(table, format!("{} holds binary data", column.name)));
            }
            Value::String(base64::engine::general_purpose::STANDARD.encode(b))
        }
    })
}

fn payload_key(layout: &EntityLayout, index: usize) -> AppResult<&str> {
    match &layout.columns[index].role {
        Role::Value { key } | Role::Ref { key, .. } => Ok(key),
        _ => Err(codec_err(layout.table(), "primary key column has no payload key")),
    }
}

/// JSON pointers of the values a reference path selects.
fn json_ref_pointers(value: &Value, path: &[JsonSeg], base: String, out: &mut Vec<String>) {
    match path.split_first() {
        None => out.push(base),
        Some((JsonSeg::Each, rest)) => {
            if let Value::Array(items) = value {
                for (i, item) in items.iter().enumerate() {
                    json_ref_pointers(item, rest, format!("{base}/{i}"), out);
                }
            }
        }
        Some((JsonSeg::Key(k), rest)) => {
            if let Some(inner) = value.get(k) {
                let escaped = k.replace('~', "~0").replace('/', "~1");
                json_ref_pointers(inner, rest, format!("{base}/{escaped}"), out);
            }
        }
        // URL references are translated by `translate_url_out` / `translate_url_in`.
        Some((JsonSeg::Query(_), _)) => {}
    }
}

fn parse_json_text(table: &str, column: &LayoutColumn, text: &Value) -> AppResult<Option<Value>> {
    match text {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => serde_json::from_str(s)
            .map(Some)
            .map_err(|e| codec_err(table, format!("{} is not valid JSON: {e}", column.name))),
        _ => Err(codec_err(table, format!("{} must hold JSON text", column.name))),
    }
}

/// Query parameter a URL reference reads (`JsonSeg::Query`), if the column holds a URL.
fn url_param(r: &JsonRef) -> Option<&str> {
    match r.path.as_slice() {
        [JsonSeg::Query(param)] => Some(param),
        _ => None,
    }
}

/// `/path?a=1&b=2` as its path and its `(name, value)` pairs.
fn split_url(url: &str) -> (&str, Vec<(String, String)>) {
    let Some((path, query)) = url.split_once('?') else {
        return (url, Vec::new());
    };
    let pairs = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (p.to_string(), String::new()),
        })
        .collect();
    (path, pairs)
}

fn join_url(path: &str, pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return path.to_string();
    }
    let query: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{path}?{}", query.join("&"))
}

/// Replace the local ids in the query of a URL column by sync ids (outbound). A link to a row
/// that no longer exists loses that parameter: it opens the list instead of another record.
async fn translate_url_out(
    conn: &impl ConnectionTrait,
    table: &str,
    column: &LayoutColumn,
    url: &str,
) -> AppResult<Value> {
    let (path, pairs) = split_url(url);
    let mut out = Vec::with_capacity(pairs.len());
    for (name, value) in pairs {
        let Some(r) = column.json_refs.iter().find(|r| url_param(r) == Some(name.as_str())) else {
            out.push((name, value));
            continue;
        };
        let local_id = value.trim().parse::<i64>().map_err(|_| {
            codec_err(
                table,
                format!("{}: parameter {name}='{value}' is not a local id", column.name),
            )
        })?;
        let sync_id = scalar_text(
            conn,
            &format!("SELECT entity_sync_id AS v FROM \"{}\" WHERE id = ?", r.table),
            vec![local_id.into()],
        )
        .await?
        .filter(|v| !v.trim().is_empty());
        if let Some(sync_id) = sync_id {
            out.push((name, sync_id));
        }
    }
    Ok(Value::String(join_url(path, &out)))
}

/// Replace the sync ids in the query of a URL column by local ids (inbound); a target not
/// received yet defers the row.
async fn translate_url_in(
    conn: &impl ConnectionTrait,
    column: &LayoutColumn,
    url: &str,
) -> AppResult<Result<Value, ApplyOutcome>> {
    let (path, mut pairs) = split_url(url);
    for (name, value) in pairs.iter_mut() {
        let Some(r) = column.json_refs.iter().find(|r| url_param(r) == Some(name.as_str())) else {
            continue;
        };
        match scalar_id(
            conn,
            &format!("SELECT id AS v FROM \"{}\" WHERE entity_sync_id = ?", r.table),
            vec![value.clone().into()],
        )
        .await?
        {
            Some(id) => *value = id.to_string(),
            None => {
                return Ok(Err(ApplyOutcome::Deferred {
                    code: DEFER_MISSING_PARENT.into(),
                    message: format!("{} needs {} {value}, not received yet", column.name, r.entity_type),
                    waits_for: Some(WaitsFor {
                        entity_type: r.entity_type.to_string(),
                        sync_id: value.clone(),
                    }),
                }))
            }
        }
    }
    Ok(Ok(Value::String(join_url(path, &pairs))))
}

/// Replace the local ids inside a JSON column by sync ids (outbound).
async fn translate_json_out(
    conn: &impl ConnectionTrait,
    table: &str,
    column: &LayoutColumn,
    text: Value,
) -> AppResult<Value> {
    if column.json_refs.iter().any(|r| url_param(r).is_some()) {
        return match &text {
            Value::String(url) => translate_url_out(conn, table, column, url).await,
            _ => Ok(text),
        };
    }
    let Some(mut doc) = parse_json_text(table, column, &text)? else {
        return Ok(text);
    };
    for r in &column.json_refs {
        let mut pointers = Vec::new();
        json_ref_pointers(&doc, &r.path, String::new(), &mut pointers);
        for pointer in pointers {
            let Some(slot) = doc.pointer_mut(&pointer) else {
                continue;
            };
            let local_id = match slot {
                Value::Null => continue,
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            }
            .ok_or_else(|| codec_err(table, format!("{}{pointer} is not a local id", column.name)))?;
            let sync_id = scalar_text(
                conn,
                &format!("SELECT entity_sync_id AS v FROM \"{}\" WHERE id = ?", r.table),
                vec![local_id.into()],
            )
            .await?
            .filter(|v| !v.trim().is_empty());
            *slot = match sync_id {
                Some(sync_id) => Value::String(sync_id),
                // A history value outlives the row it mentions.
                None if r.history => Value::Null,
                None => {
                    return Err(codec_err(
                        table,
                        format!(
                            "{}{pointer} points to {} row {local_id}, which does not exist",
                            column.name, r.table
                        ),
                    ))
                }
            };
        }
    }
    Ok(Value::String(serde_json::to_string(&doc)?))
}

/// Replace the sync ids inside a JSON column by local ids (inbound); a target not received yet
/// defers the row.
async fn translate_json_in(
    conn: &impl ConnectionTrait,
    table: &str,
    column: &LayoutColumn,
    text: &Value,
) -> AppResult<Result<Value, ApplyOutcome>> {
    if column.json_refs.iter().any(|r| url_param(r).is_some()) {
        return match text {
            Value::String(url) => translate_url_in(conn, column, url).await,
            _ => Ok(Ok(text.clone())),
        };
    }
    let Some(mut doc) = parse_json_text(table, column, text)? else {
        return Ok(Ok(text.clone()));
    };
    for r in &column.json_refs {
        let mut pointers = Vec::new();
        json_ref_pointers(&doc, &r.path, String::new(), &mut pointers);
        for pointer in pointers {
            let Some(slot) = doc.pointer_mut(&pointer) else {
                continue;
            };
            let sync_id = match slot {
                Value::Null => continue,
                Value::String(s) => s.clone(),
                _ => return Err(codec_err(table, format!("{}{pointer} must be a sync id", column.name))),
            };
            match scalar_id(
                conn,
                &format!("SELECT id AS v FROM \"{}\" WHERE entity_sync_id = ?", r.table),
                vec![sync_id.clone().into()],
            )
            .await?
            {
                Some(id) => *slot = Value::Number(id.into()),
                None => {
                    return Ok(Err(ApplyOutcome::Deferred {
                        code: DEFER_MISSING_PARENT.into(),
                        message: format!("{} needs {} {sync_id}, not received yet", column.name, r.entity_type),
                        waits_for: Some(WaitsFor {
                            entity_type: r.entity_type.to_string(),
                            sync_id,
                        }),
                    }))
                }
            }
        }
    }
    Ok(Ok(Value::String(serde_json::to_string(&doc)?)))
}

/// Canonical natural key of a natural-key row: `key=json` pairs in key order.
pub(crate) fn natural_key_string(layout: &EntityLayout, payload: &Map<String, Value>) -> AppResult<String> {
    let Some(pk) = &layout.natural_pk else {
        return Err(codec_err(layout.table(), "not keyed by a natural key"));
    };
    let mut parts = Vec::with_capacity(pk.len());
    for &i in pk {
        let key = payload_key(layout, i)?;
        let value = payload
            .get(key)
            .ok_or_else(|| codec_err(layout.table(), format!("natural key field {key} is missing")))?;
        if value.is_null() {
            return Err(codec_err(layout.table(), format!("natural key field {key} is empty")));
        }
        parts.push(format!("{key}={value}"));
    }
    Ok(parts.join("|"))
}

/// Identity of a natural-key row.
pub(crate) fn natural_entity_sync_id(layout: &EntityLayout, payload: &Map<String, Value>) -> AppResult<String> {
    Ok(seeded_entity_sync_id(
        layout.def.entity_type,
        &natural_key_string(layout, payload)?,
    ))
}

/// Payload of the current local row, or `None` when the row does not exist here (or is outside
/// the entity's row filter).
pub(crate) async fn load_row_payload(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    locator: &RowLocator,
) -> AppResult<Option<RowPayload>> {
    let Some(cells) = read_cells(conn, layout, locator).await? else {
        return Ok(None);
    };
    let table = layout.table();
    let mut payload: BTreeMap<String, Value> = BTreeMap::new();
    let mut identity = None;
    for (column, cell) in layout.columns.iter().zip(&cells) {
        match &column.role {
            Role::LocalKey | Role::LocalVersion | Role::DeviceLocal | Role::Derived { .. } => {}
            Role::Identity => {
                identity = match cell {
                    Cell::Text(t) if !t.trim().is_empty() => Some(t.clone()),
                    _ => return Err(codec_err(table, "row has no entity_sync_id")),
                };
            }
            Role::Value { key } => {
                let value = outbound_value(table, column, cell)?;
                let value = if column.json_refs.is_empty() {
                    value
                } else {
                    translate_json_out(conn, table, column, value).await?
                };
                payload.insert(key.clone(), value);
            }
            Role::Ref {
                key, target, history, ..
            } => {
                payload.insert(
                    key.clone(),
                    outbound_ref(conn, layout, column, target, *history, cell, &cells).await?,
                );
            }
        }
    }
    payload.insert(
        PAYLOAD_FORMAT_KEY.to_string(),
        Value::String(PAYLOAD_FORMAT.to_string()),
    );
    let mut payload: Map<String, Value> = payload.into_iter().collect();
    let entity_sync_id = match identity {
        Some(id) => id,
        None => natural_entity_sync_id(layout, &payload)?,
    };
    // A soft reference received before its target is empty here until the target arrives
    // (`sync_pending_refs`): the row sent again (a local change) still names the target, so the
    // other devices never lose the link.
    for column in &layout.columns {
        if let Role::Ref { key, soft: true, .. } = &column.role {
            if payload.get(key).is_none_or(Value::is_null) {
                let pending = conn
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "SELECT target_sync_id FROM sync_pending_refs \
                          WHERE entity_type = ? AND entity_sync_id = ? AND column_name = ?",
                        [
                            layout.def.entity_type.into(),
                            entity_sync_id.as_str().into(),
                            column.name.as_str().into(),
                        ],
                    ))
                    .await?
                    .and_then(|r| r.try_get::<String>("", "target_sync_id").ok());
                if let Some(target) = pending {
                    payload.insert(key.clone(), Value::String(target));
                }
            }
        }
    }
    let owner_user_sync_id = match layout.owner_payload_key()? {
        None => None,
        Some(key) => Some(
            payload
                .get(&key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| codec_err(table, "user-scoped row has no owner"))?,
        ),
    };
    Ok(Some(RowPayload {
        entity_sync_id,
        payload,
        owner_user_sync_id,
    }))
}

/// Payload of a delete: the format marker, plus the natural key for natural-key rows.
pub(crate) fn delete_payload(layout: &EntityLayout, natural_key: Option<&Map<String, Value>>) -> Map<String, Value> {
    let mut payload = Map::new();
    if let (Some(pk), Some(key)) = (&layout.natural_pk, natural_key) {
        for &i in pk {
            if let Ok(k) = payload_key(layout, i) {
                if let Some(v) = key.get(k) {
                    payload.insert(k.to_string(), v.clone());
                }
            }
        }
    }
    payload.insert(
        PAYLOAD_FORMAT_KEY.to_string(),
        Value::String(PAYLOAD_FORMAT.to_string()),
    );
    payload
}

/// The required (not soft) references of `payload` that are not applied here yet: their entity type
/// and sync id. The safety re-checks (`permit::sync_check`) count a row still waiting in the inbox
/// only when everything it refers to is here (its own parent excepted, applied with it).
pub(crate) async fn missing_refs(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    payload: &Map<String, Value>,
) -> AppResult<Vec<(&'static str, String)>> {
    let mut missing = Vec::new();
    for column in &layout.columns {
        let Role::Ref {
            key,
            target,
            soft: false,
            ..
        } = &column.role
        else {
            continue;
        };
        let Some(v) = payload.get(key) else {
            continue;
        };
        if let Resolved::Missing { target_entity, sync_id } =
            inbound_ref(conn, layout, column, target, v, payload).await?
        {
            missing.push((target_entity, sync_id));
        }
    }
    Ok(missing)
}

/// Outcome of resolving an inbound reference.
enum Resolved {
    Local(SqlValue),
    /// Target not here yet.
    Missing {
        target_entity: &'static str,
        sync_id: String,
    },
}

fn null_for(column: &LayoutColumn) -> SqlValue {
    if column.is_text {
        SqlValue::String(None)
    } else {
        SqlValue::BigInt(None)
    }
}

async fn inbound_ref(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    column: &LayoutColumn,
    target: &Target,
    value: &Value,
    payload: &Map<String, Value>,
) -> AppResult<Resolved> {
    let table = layout.table();
    let key = match value {
        Value::Null => return Ok(Resolved::Local(null_for(column))),
        Value::String(s) if !s.trim().is_empty() => s.clone(),
        _ => {
            return Err(codec_err(
                table,
                format!("{}: reference must be a sync id string", column.name),
            ))
        }
    };
    let (sql, target_entity, target_table) = match target {
        Target::Unmapped => {
            return Err(codec_err(
                table,
                format!("{}: a value was sent for a reference that must stay empty", column.name),
            ))
        }
        Target::Entity { entity_type, table } => (
            format!("SELECT id AS v FROM \"{table}\" WHERE entity_sync_id = ?"),
            Some(*entity_type),
            *table,
        ),
        Target::Catalog {
            table: catalog,
            key_column,
        } => (
            format!("SELECT id AS v FROM \"{catalog}\" WHERE \"{key_column}\" = ?"),
            None,
            *catalog,
        ),
        Target::Poly { type_column, targets } => {
            let type_value = payload.get(*type_column).and_then(Value::as_str);
            let Some((_, entity, target_table)) = poly_target(targets, type_value) else {
                return Err(codec_err(
                    table,
                    format!(
                        "{}: {type_column} {type_value:?} is not a mapped reference type",
                        column.name
                    ),
                ));
            };
            if *target_table == crate::sync::row_spec::NOT_A_ROW {
                // Not a row reference: stored as sent.
                return Ok(Resolved::Local(key.into()));
            }
            if *target_table == crate::sync::row_spec::LOCAL_ROW {
                return Err(codec_err(
                    table,
                    format!(
                        "{}: {type_column} {type_value:?} points to rows that are not synchronised",
                        column.name
                    ),
                ));
            }
            (
                format!("SELECT id AS v FROM \"{target_table}\" WHERE entity_sync_id = ?"),
                Some(*entity),
                *target_table,
            )
        }
    };
    let mut found = cached_ref_id(conn, &sql, &key).await?;
    if found.is_none()
        && target_table == "reference_values"
        && crate::reference::units::is_unit_ref_column(table, &column.name)
    {
        // A device older than migration 201 names a unit by the sync id of the retired lookup
        // `inventory.unit_of_measure`: the unit with that code (`reference::units`).
        found = crate::reference::units::unit_for_legacy_sync_id(conn, &key).await?;
    }
    match (found, target_entity) {
        (Some(id), _) if column.is_text => Ok(Resolved::Local(id.to_string().into())),
        (Some(id), _) => Ok(Resolved::Local(id.into())),
        (None, Some(target_entity)) => Ok(Resolved::Missing {
            target_entity,
            sync_id: key,
        }),
        // App catalogs ship with the application: a missing key means another app version.
        (None, None) => Err(codec_err(
            table,
            format!("{}: {target_table} '{key}' does not exist in this version", column.name),
        )),
    }
}

fn inbound_value(table: &str, column: &LayoutColumn, value: &Value) -> AppResult<SqlValue> {
    Ok(match value {
        Value::Null => SqlValue::String(None),
        Value::Bool(b) => SqlValue::BigInt(Some(i64::from(*b))),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::BigInt(Some(i))
            } else if let Some(f) = n.as_f64() {
                SqlValue::Double(Some(f))
            } else {
                return Err(codec_err(table, format!("{}: number out of range", column.name)));
            }
        }
        Value::String(s) if column.is_blob => SqlValue::Bytes(Some(Box::new(
            base64::engine::general_purpose::STANDARD
                .decode(s)
                .map_err(|e| codec_err(table, format!("{}: invalid base64: {e}", column.name)))?,
        ))),
        Value::String(s) => SqlValue::String(Some(Box::new(s.clone()))),
        Value::Array(_) | Value::Object(_) => {
            return Err(codec_err(
                table,
                format!("{}: payload values must be scalars", column.name),
            ))
        }
    })
}

fn parse_payload(table: &str, payload_json: &str) -> AppResult<Map<String, Value>> {
    let value: Value = serde_json::from_str(payload_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("{table}: invalid payload_json: {e}")]))?;
    let Value::Object(map) = value else {
        return Err(codec_err(table, "payload is not a JSON object"));
    };
    match map.get(PAYLOAD_FORMAT_KEY).and_then(Value::as_str) {
        Some(PAYLOAD_FORMAT) => Ok(map),
        Some(other) => Err(codec_err(
            table,
            format!("payload format '{other}' is not supported by this version"),
        )),
        None => Err(codec_err(
            table,
            "payload has no _sync_format: it was written by a version before Phase 2.4",
        )),
    }
}

/// Natural-key locator from a payload (or a journal key in payload form), or `None` when a key
/// target is not on this device (so the row cannot exist here either).
pub(crate) async fn natural_locator(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    payload: &Map<String, Value>,
) -> AppResult<Option<RowLocator>> {
    let pk = layout
        .natural_pk
        .as_ref()
        .ok_or_else(|| codec_err(layout.table(), "not keyed by a natural key"))?;
    let mut parts = Vec::with_capacity(pk.len());
    for &i in pk {
        let column = &layout.columns[i];
        let key = payload_key(layout, i)?;
        let v = payload
            .get(key)
            .ok_or_else(|| codec_err(layout.table(), format!("natural key field {key} is missing")))?;
        match &column.role {
            Role::Ref { target, .. } => match inbound_ref(conn, layout, column, target, v, payload).await? {
                Resolved::Local(local) => parts.push((column.name.clone(), local)),
                Resolved::Missing { .. } => return Ok(None),
            },
            _ => parts.push((column.name.clone(), inbound_value(layout.table(), column, v)?)),
        }
    }
    Ok(Some(RowLocator::Natural(parts)))
}

async fn row_exists(conn: &impl ConnectionTrait, layout: &EntityLayout, locator: &RowLocator) -> AppResult<bool> {
    let (where_sql, values) = locator.where_sql("");
    let sql = format!("SELECT 1 AS v FROM \"{}\" WHERE {where_sql} LIMIT 1", layout.table());
    Ok(conn
        .query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, &sql, values))
        .await?
        .is_some())
}

/// Whether a received row of an append-only entity, already held here, is skipped by the apply
/// (history is never rewritten) rather than adopted (`apply_hooks::adopt_existing_append_only`).
/// The apply and the hooks that skip judging such a row (`wo::closeout_sync`) share this test.
pub(crate) async fn append_only_replay_skipped(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<bool> {
    Ok(layout.def.conflict_policy == ConflictPolicy::AppendOnly
        && !crate::sync::apply_hooks::adopt_existing_append_only(conn, layout, entity_sync_id, payload).await?)
}

fn is_delete(operation: &str) -> bool {
    operation == "delete" || operation == "archive"
}

/// Apply an inbound row. Callers hold a transaction with capture suppressed.
pub(crate) async fn apply_row(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    operation: &str,
    payload_json: &str,
) -> AppResult<ApplyOutcome> {
    let table = layout.table();
    let mut payload = parse_payload(table, payload_json)?;
    // Unit texts of a device older than migration 201 become links to MEASURE.UNITS.
    crate::reference::units::upgrade_legacy_unit_payload(conn, table, &mut payload).await?;

    let locator = if layout.natural_pk.is_some() {
        if natural_entity_sync_id(layout, &payload)? != entity_sync_id {
            return Err(codec_err(
                table,
                "entity_sync_id does not match the natural key of the payload",
            ));
        }
        natural_locator(conn, layout, &payload).await?
    } else {
        Some(RowLocator::Identity(entity_sync_id.to_string()))
    };

    if is_delete(operation) {
        // History and audit rows are never erased by synchronisation (append-only). Media
        // attachments are the one append-only type whose removal is replicated (its file is
        // dropped with it, `apply_hooks::after_delete`).
        if layout.def.conflict_policy == ConflictPolicy::AppendOnly && layout.def.entity_type != "media_attachments" {
            return Ok(ApplyOutcome::Conflict {
                code: "APPEND_ONLY_DELETE".into(),
                message: format!("{table}: history rows are never deleted by synchronisation"),
            });
        }
        if let Some(outcome) = crate::sync::apply_hooks::before_delete_checked(conn, layout, entity_sync_id).await? {
            return Ok(outcome);
        }
        let Some(locator) = locator else {
            return Ok(ApplyOutcome::Unchanged);
        };
        let rights_before = crate::sync::grant_check::rights_before(conn, layout).await?;
        let (where_sql, values) = locator.where_sql("");
        let sql = format!("DELETE FROM \"{table}\" WHERE {where_sql}");
        let result = conn
            .execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, &sql, values))
            .await?;
        if result.rows_affected() == 0 {
            return Ok(ApplyOutcome::Unchanged);
        }
        clear_ref_cache();
        crate::sync::apply_hooks::after_delete(conn, layout, entity_sync_id).await?;
        crate::sync::grant_check::after_rights_change(conn, layout, entity_sync_id, None, rights_before).await?;
        return Ok(ApplyOutcome::Applied);
    }

    if let Some(outcome) = crate::sync::apply_hooks::before_upsert(layout, &payload) {
        return Ok(outcome);
    }
    if let Some(outcome) =
        crate::sync::apply_hooks::before_upsert_checked(conn, layout, entity_sync_id, &payload).await?
    {
        return Ok(outcome);
    }
    // `user_accounts`: a password change or a re-enable whose author could not make it here is held
    // back (its keys leave the payload; the rest of the row applies), after the checks above so a
    // row they refuse records nothing (`sync::credential_check`).
    if let Some(outcome) =
        crate::sync::credential_check::inbound_credential_check(conn, layout, entity_sync_id, &mut payload).await?
    {
        return Ok(outcome);
    }
    let group = crate::sync::apply_hooks::upsert_group(conn, layout, entity_sync_id, &payload).await?;
    let rights_before = crate::sync::grant_check::rights_before(conn, layout).await?;

    let mut assignments: Vec<(String, SqlValue)> = Vec::new();
    let mut unresolved_soft: Vec<(String, &'static str, String)> = Vec::new();
    let mut resolved_soft: Vec<String> = Vec::new();
    for column in &layout.columns {
        match &column.role {
            Role::LocalKey | Role::Identity | Role::LocalVersion | Role::DeviceLocal | Role::Derived { .. } => {}
            Role::Value { key } => {
                if let Some(v) = payload.get(key) {
                    if column.json_refs.is_empty() {
                        assignments.push((column.name.clone(), inbound_value(table, column, v)?));
                    } else {
                        match translate_json_in(conn, table, column, v).await? {
                            Ok(local) => assignments.push((column.name.clone(), inbound_value(table, column, &local)?)),
                            Err(deferred) => return Ok(deferred),
                        }
                    }
                }
            }
            Role::Ref { key, target, soft, .. } => {
                let Some(v) = payload.get(key) else {
                    continue;
                };
                match inbound_ref(conn, layout, column, target, v, &payload).await? {
                    Resolved::Local(local) => {
                        if *soft {
                            resolved_soft.push(column.name.clone());
                        }
                        assignments.push((column.name.clone(), local));
                    }
                    Resolved::Missing { target_entity, sync_id } if *soft => {
                        unresolved_soft.push((column.name.clone(), target_entity, sync_id));
                        assignments.push((column.name.clone(), null_for(column)));
                    }
                    Resolved::Missing { target_entity, sync_id } => {
                        return Ok(ApplyOutcome::Deferred {
                            code: DEFER_MISSING_PARENT.into(),
                            message: format!("{} needs {target_entity} {sync_id}, not received yet", column.name),
                            waits_for: Some(WaitsFor {
                                entity_type: target_entity.to_string(),
                                sync_id,
                            }),
                        });
                    }
                }
            }
        }
    }

    let existing = match &locator {
        Some(l) if row_exists(conn, layout, l).await? => Some(l.clone()),
        _ => None,
    };
    let (local_row, inserted) = if let Some(locator) = existing {
        if append_only_replay_skipped(conn, layout, entity_sync_id, &payload).await? {
            // History rows are never rewritten; a replayed row is a no-op.
            return Ok(ApplyOutcome::Unchanged);
        }
        let mut sets: Vec<String> = assignments.iter().map(|(c, _)| format!("\"{c}\" = ?")).collect();
        if layout.has_row_version {
            // Open editors holding the previous version get a stale-version error instead of
            // overwriting the change that just arrived.
            sets.push("row_version = row_version + 1".to_string());
        }
        if !sets.is_empty() {
            let (where_sql, where_values) = locator.where_sql("");
            let sql = format!("UPDATE \"{table}\" SET {} WHERE {where_sql}", sets.join(", "));
            let mut values: Vec<SqlValue> = assignments.into_iter().map(|(_, v)| v).collect();
            values.extend(where_values);
            conn.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, &sql, values))
                .await?;
        }
        (locator, false)
    } else {
        let mut cols: Vec<String> = assignments.iter().map(|(c, _)| format!("\"{c}\"")).collect();
        let mut placeholders: Vec<String> = vec!["?".to_string(); cols.len()];
        let mut values: Vec<SqlValue> = assignments.iter().map(|(_, v)| v.clone()).collect();
        for column in &layout.columns {
            if let Role::Derived { initial } = column.role {
                cols.push(format!("\"{}\"", column.name));
                placeholders.push(initial.to_string());
            }
        }
        if layout.natural_pk.is_none() {
            cols.push("entity_sync_id".into());
            placeholders.push("?".into());
            values.push(entity_sync_id.to_string().into());
        }
        if layout.has_row_version {
            // The local optimistic-concurrency counter starts at 1, as for a row created here.
            cols.push("row_version".into());
            placeholders.push("?".into());
            values.push(1_i64.into());
        }
        let sql = format!(
            "INSERT INTO \"{table}\" ({}) VALUES ({})",
            cols.join(", "),
            placeholders.join(", ")
        );
        conn.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, &sql, values))
            .await?;
        let located = match locator {
            Some(l) => l,
            None => natural_locator(conn, layout, &payload)
                .await?
                .ok_or_else(|| codec_err(table, "inserted row cannot be located"))?,
        };
        (located, true)
    };

    for column in resolved_soft {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "DELETE FROM sync_pending_refs WHERE entity_type = ? AND entity_sync_id = ? AND column_name = ?",
            [layout.def.entity_type.into(), entity_sync_id.into(), column.into()],
        ))
        .await?;
    }
    for (column, target_entity, target_sync_id) in unresolved_soft {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO sync_pending_refs (entity_type, entity_sync_id, column_name, target_entity_type, target_sync_id)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(entity_type, entity_sync_id, column_name) DO UPDATE SET
                target_entity_type = excluded.target_entity_type,
                target_sync_id = excluded.target_sync_id",
            [
                layout.def.entity_type.into(),
                entity_sync_id.into(),
                column.into(),
                target_entity.into(),
                target_sync_id.into(),
            ],
        ))
        .await?;
    }
    complete_pending_refs_to(conn, layout.def.entity_type, entity_sync_id).await?;
    crate::sync::apply_hooks::after_upsert(conn, layout, &local_row, inserted, &payload).await?;
    crate::sync::grant_check::after_rights_change(conn, layout, entity_sync_id, Some(&payload), rights_before).await?;
    if let Some(group) = group {
        if let Some(outcome) = crate::sync::apply_hooks::after_upsert_group(conn, group).await? {
            return Ok(outcome);
        }
    }
    Ok(ApplyOutcome::Applied)
}

/// Local primary key (`id`) of a located row, for hooks.
pub(crate) async fn located_row_id(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    locator: &RowLocator,
) -> AppResult<Option<i64>> {
    if layout.natural_pk.is_some() {
        return Ok(None);
    }
    let (where_sql, values) = locator.where_sql("");
    scalar_id(
        conn,
        &format!("SELECT id AS v FROM \"{}\" WHERE {where_sql}", layout.table()),
        values,
    )
    .await
}

/// Fill the soft references that were waiting for the row just applied.
async fn complete_pending_refs_to(
    conn: &impl ConnectionTrait,
    target_entity_type: &str,
    target_sync_id: &str,
) -> AppResult<()> {
    let waiting = conn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_type, entity_sync_id, column_name FROM sync_pending_refs
             WHERE target_entity_type = ? AND target_sync_id = ?",
            [target_entity_type.into(), target_sync_id.into()],
        ))
        .await?;
    if waiting.is_empty() {
        return Ok(());
    }
    let target = crate::sync::registry::require_entity(target_entity_type)?;
    let target_id = scalar_id(
        conn,
        &format!("SELECT id AS v FROM \"{}\" WHERE entity_sync_id = ?", target.table),
        vec![target_sync_id.into()],
    )
    .await?
    .ok_or_else(|| AppError::SyncError(format!("{target_entity_type} {target_sync_id} vanished")))?;
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("sync_pending_refs: {e}"));
    for row in waiting {
        let id: i64 = row.try_get("", "id").map_err(read)?;
        let entity_type: String = row.try_get("", "entity_type").map_err(read)?;
        let entity_sync_id: String = row.try_get("", "entity_sync_id").map_err(read)?;
        let column_name: String = row.try_get("", "column_name").map_err(read)?;
        let layout = crate::sync::row_layout::layout(conn, &entity_type).await?;
        let column = layout
            .column(&column_name)
            .ok_or_else(|| AppError::SyncError(format!("{entity_type}.{column_name} no longer exists")))?;
        let value: SqlValue = if column.is_text {
            target_id.to_string().into()
        } else {
            target_id.into()
        };
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "UPDATE \"{}\" SET \"{column_name}\" = ? WHERE entity_sync_id = ?",
                layout.table()
            ),
            [value, entity_sync_id.into()],
        ))
        .await?;
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "DELETE FROM sync_pending_refs WHERE id = ?",
            [id.into()],
        ))
        .await?;
    }
    Ok(())
}
