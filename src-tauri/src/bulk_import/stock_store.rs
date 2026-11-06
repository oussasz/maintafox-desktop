//! Stock stores (magasins) and their locations (emplacements), two import sheets keyed by codes.
//!
//! They do what the store screen does and nothing more: create a store or a location, rename it,
//! activate or deactivate it. A location is keyed by its store code plus its own code, because
//! location codes are unique only inside one store. The default location of a store stays a
//! screen choice. Nothing is ever deleted, and a line missing from the file changes nothing.
//! Stores have no site yet: the store-to-site link will add a site column here too.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::{key, parse_active};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    CreateStockLocationInput, CreateWarehouseInput, UpdateStockLocationInput, UpdateWarehouseInput,
};
use crate::inventory::queries;

const STORE_COLUMN: Column = Column {
    key: "store_code",
    header: "code magasin",
    required: true,
    aliases: &["magasin", "code_magasin", "store_code", "warehouse", "code_entrepot"],
    hint: "Code du magasin, par exemple MAG-CENTRAL.",
};

const ACTIVE_HINT: &str = "oui ou non. Vide : une nouvelle ligne est active, une ligne existante ne change pas.";

pub const STORE_SPEC: EntitySpec = EntitySpec {
    code: "stock_store",
    label: "Magasins",
    order: 5,
    needs: "Aucune.",
    permission: crate::rbac::permissions::INV_MANAGE,
    columns: &[
        STORE_COLUMN,
        Column {
            key: "name",
            header: "libellé",
            required: false,
            aliases: &["libelle", "nom", "designation", "name"],
            hint: "Nom affiché du magasin. Obligatoire pour un nouveau magasin.",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: ACTIVE_HINT,
        },
    ],
};

pub const LOCATION_SPEC: EntitySpec = EntitySpec {
    code: "stock_location",
    label: "Emplacements de stock",
    order: 5,
    needs: "Magasins.",
    permission: crate::rbac::permissions::INV_MANAGE,
    columns: &[
        STORE_COLUMN,
        Column {
            key: "code",
            header: "code emplacement",
            required: true,
            aliases: &["emplacement", "code_emplacement", "location_code", "casier"],
            hint: "Code de l'emplacement, unique dans son magasin, par exemple A-01-02.",
        },
        Column {
            key: "name",
            header: "libellé",
            required: false,
            aliases: &["libelle", "nom", "designation", "name"],
            hint: "Nom affiché de l'emplacement. Obligatoire pour un nouvel emplacement.",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: ACTIVE_HINT,
        },
    ],
};

fn has_error(messages: &[RowMessage]) -> bool {
    messages.iter().any(|m| m.severity == "error")
}

fn cell(raw: &HashMap<String, String>, field: &str) -> String {
    raw.get(field).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn text(row: &StagedRow, field: &str) -> String {
    row.normalized[field].as_str().unwrap_or_default().to_string()
}

fn app_error_text(e: &AppError) -> String {
    match e {
        AppError::ValidationFailed(v) => v.join(" "),
        other => other.to_string(),
    }
}

fn fail(row: &StagedRow, message: impl Into<String>) -> RowApplied {
    RowApplied {
        row_id: row.id,
        status: "failed",
        message: message.into(),
    }
}

fn skipped(row: &StagedRow) -> RowApplied {
    RowApplied {
        row_id: row.id,
        status: "skipped",
        message: "Déjà à jour.".to_string(),
    }
}

/// The action of a line once its messages are known: refused lines and repeated keys are skipped.
/// `existing` is `None` for a new line, else whether the line changes the existing one.
fn action_for(messages: &[RowMessage], first: bool, existing: Option<bool>) -> &'static str {
    if has_error(messages) || !first {
        "skip"
    } else {
        match existing {
            None => "create",
            Some(true) => "update",
            Some(false) => "skip",
        }
    }
}

// ─── Stores ───────────────────────────────────────────────────────────────────

struct Store {
    id: i64,
    code: String,
    name: String,
    active: bool,
}

/// Every store by code key. Codes that differ only by letter case land in `ambiguous`.
async fn stores(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, Store>, HashSet<String>)> {
    let mut map: HashMap<String, Store> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, is_active FROM warehouses".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let active: i64 = r.try_get("", "is_active")?;
        if map.contains_key(&key(&code)) {
            ambiguous.insert(key(&code));
        }
        map.insert(
            key(&code),
            Store {
                id: r.try_get("", "id")?,
                code,
                name: r.try_get("", "name")?,
                active: active != 0,
            },
        );
    }
    Ok((map, ambiguous))
}

fn ambiguous_store(code: &str) -> String {
    format!("Plusieurs magasins ont le code « {code} » à la casse près : renommez-les dans Stock avant d'importer.")
}

pub async fn validate_stores(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let (existing, ambiguous) = stores(db).await?;
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let code = cell(raw, "store_code");
        let name = cell(raw, "name");
        let mut first = false;
        if code.is_empty() {
            messages.push(RowMessage::error(
                "STORE_IMPORT_CODE",
                "Le code magasin est obligatoire.",
            ));
        } else if let Some(f) = first_row_of.get(&key(&code)) {
            messages.push(RowMessage::error(
                "STORE_IMPORT_DUPLICATE",
                format!("Le magasin « {code} » apparaît déjà à la ligne {} du fichier.", f + 2),
            ));
        } else {
            first_row_of.insert(key(&code), i);
            first = true;
        }
        let active = parse_active(raw.get("active")).unwrap_or_else(|()| {
            messages.push(RowMessage::error(
                "STORE_IMPORT_ACTIVE",
                "La colonne « actif » doit contenir oui ou non.",
            ));
            None
        });
        let found = existing.get(&key(&code));
        if first && ambiguous.contains(&key(&code)) {
            messages.push(RowMessage::error("STORE_IMPORT_CODE_AMBIGUOUS", ambiguous_store(&code)));
        }
        if first && found.is_none() {
            if name.is_empty() {
                messages.push(RowMessage::error(
                    "STORE_IMPORT_NAME",
                    "Le libellé est obligatoire pour un nouveau magasin.",
                ));
            }
            if active == Some(false) {
                messages.push(RowMessage::error(
                    "STORE_IMPORT_NEW_INACTIVE",
                    "Un nouveau magasin est créé actif : laissez « actif » vide ou mettez oui.",
                ));
            }
        }
        let changes = found.map(|s| (!name.is_empty() && name != s.name) || active.is_some_and(|a| a != s.active));
        out.push(RowOutcome {
            action: action_for(&messages, first, changes),
            normalized: json!({ "code": code, "name": name, "active": active }),
            messages,
        });
    }
    Ok(out)
}

pub async fn apply_stores(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        if row.action != "create" && row.action != "update" {
            out.push(skipped(row));
            continue;
        }
        let code = text(row, "code");
        let name = text(row, "name");
        // Read again at apply time: a store created, renamed or duplicated elsewhere since the
        // preview fails this line only.
        let (now, ambiguous) = stores(db).await?;
        if ambiguous.contains(&key(&code)) {
            out.push(fail(row, ambiguous_store(&code)));
            continue;
        }
        let result = match (row.action.as_str(), now.get(&key(&code))) {
            ("create", None) => queries::create_warehouse(db, CreateWarehouseInput { code, name })
                .await
                .map(|_| "created"),
            ("create", Some(_)) => {
                out.push(fail(
                    row,
                    format!("Le magasin « {code} » a été créé entre-temps : relancez l'import."),
                ));
                continue;
            }
            (_, None) => {
                out.push(fail(row, format!("Le magasin « {code} » n'existe plus.")));
                continue;
            }
            (_, Some(store)) => {
                let new_name = (!name.is_empty() && name != store.name).then_some(name);
                let new_active = row.normalized["active"].as_bool().filter(|a| *a != store.active);
                if new_name.is_none() && new_active.is_none() {
                    out.push(skipped(row));
                    continue;
                }
                queries::update_warehouse(
                    db,
                    store.id,
                    UpdateWarehouseInput {
                        name: new_name,
                        is_active: new_active,
                    },
                )
                .await
                .map(|_| "updated")
            }
        };
        out.push(match result {
            Ok(status) => RowApplied {
                row_id: row.id,
                status,
                message: String::new(),
            },
            Err(e) => fail(row, app_error_text(&e)),
        });
    }
    Ok(out)
}

pub async fn export_stores(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let (map, _) = stores(db).await?;
    let mut list: Vec<&Store> = map.values().collect();
    list.sort_by(|a, b| a.code.cmp(&b.code));
    Ok(list
        .into_iter()
        .map(|s| {
            vec![
                s.code.clone(),
                s.name.clone(),
                if s.active { "oui" } else { "non" }.to_string(),
            ]
        })
        .collect())
}

// ─── Locations ────────────────────────────────────────────────────────────────

struct Location {
    id: i64,
    name: String,
    active: bool,
    row_version: i64,
}

/// Every location by (store id, location code key). Codes that differ only by letter case inside
/// one store land in `ambiguous`.
async fn locations(db: &impl ConnectionTrait) -> AppResult<(HashMap<(i64, String), Location>, HashSet<(i64, String)>)> {
    let mut map: HashMap<(i64, String), Location> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, warehouse_id, code, name, is_active, row_version FROM stock_locations".to_string(),
        ))
        .await?
    {
        let store: i64 = r.try_get("", "warehouse_id")?;
        let code: String = r.try_get("", "code")?;
        let active: i64 = r.try_get("", "is_active")?;
        let k = (store, key(&code));
        if map.contains_key(&k) {
            ambiguous.insert(k.clone());
        }
        map.insert(
            k,
            Location {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                active: active != 0,
                row_version: r.try_get("", "row_version")?,
            },
        );
    }
    Ok((map, ambiguous))
}

fn ambiguous_location(store: &str, code: &str) -> String {
    format!(
        "Plusieurs emplacements du magasin « {store} » ont le code « {code} » à la casse près : renommez-les dans Stock avant d'importer."
    )
}

pub async fn validate_locations(
    db: &DatabaseConnection,
    rows: &[HashMap<String, String>],
) -> AppResult<Vec<RowOutcome>> {
    let (store_map, store_ambiguous) = stores(db).await?;
    let (existing, ambiguous) = locations(db).await?;
    let mut first_row_of: HashMap<(String, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let store_code = cell(raw, "store_code");
        let code = cell(raw, "code");
        let name = cell(raw, "name");
        let active = parse_active(raw.get("active")).unwrap_or_else(|()| {
            messages.push(RowMessage::error(
                "LOCATION_IMPORT_ACTIVE",
                "La colonne « actif » doit contenir oui ou non.",
            ));
            None
        });
        if store_code.is_empty() {
            messages.push(RowMessage::error(
                "LOCATION_IMPORT_STORE",
                "Le code magasin est obligatoire.",
            ));
        }
        if code.is_empty() {
            messages.push(RowMessage::error(
                "LOCATION_IMPORT_CODE",
                "Le code emplacement est obligatoire.",
            ));
        }
        let mut first = false;
        if !store_code.is_empty() && !code.is_empty() {
            let k = (key(&store_code), key(&code));
            if let Some(f) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "LOCATION_IMPORT_DUPLICATE",
                    format!(
                        "L'emplacement « {code} » du magasin « {store_code} » apparaît déjà à la ligne {} du fichier.",
                        f + 2
                    ),
                ));
            } else {
                first_row_of.insert(k, i);
                first = true;
            }
        }
        let store = if store_code.is_empty() {
            None
        } else if store_ambiguous.contains(&key(&store_code)) {
            messages.push(RowMessage::error(
                "LOCATION_IMPORT_STORE_AMBIGUOUS",
                ambiguous_store(&store_code),
            ));
            None
        } else if let Some(s) = store_map.get(&key(&store_code)) {
            Some(s)
        } else {
            messages.push(RowMessage::error(
                "LOCATION_IMPORT_STORE_UNKNOWN",
                format!("Le magasin « {store_code} » n'existe pas : importez d'abord les magasins."),
            ));
            None
        };
        let mut changes = None;
        if let (Some(store), true) = (store, first) {
            let k = (store.id, key(&code));
            if ambiguous.contains(&k) {
                messages.push(RowMessage::error(
                    "LOCATION_IMPORT_CODE_AMBIGUOUS",
                    ambiguous_location(&store.code, &code),
                ));
            }
            match existing.get(&k) {
                Some(l) => {
                    changes = Some((!name.is_empty() && name != l.name) || active.is_some_and(|a| a != l.active));
                }
                None => {
                    if name.is_empty() {
                        messages.push(RowMessage::error(
                            "LOCATION_IMPORT_NAME",
                            "Le libellé est obligatoire pour un nouvel emplacement.",
                        ));
                    }
                    if active == Some(false) {
                        messages.push(RowMessage::error(
                            "LOCATION_IMPORT_NEW_INACTIVE",
                            "Un nouvel emplacement est créé actif : laissez « actif » vide ou mettez oui.",
                        ));
                    }
                    if !store.active {
                        messages.push(RowMessage::error(
                            "LOCATION_IMPORT_STORE_INACTIVE",
                            format!(
                                "Le magasin « {} » est inactif : réactivez-le avant d'y ajouter un emplacement.",
                                store.code
                            ),
                        ));
                    }
                }
            }
        }
        out.push(RowOutcome {
            action: action_for(&messages, first, changes),
            normalized: json!({ "store_code": store_code, "code": code, "name": name, "active": active }),
            messages,
        });
    }
    Ok(out)
}

pub async fn apply_locations(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        if row.action != "create" && row.action != "update" {
            out.push(skipped(row));
            continue;
        }
        let store_code = text(row, "store_code");
        let code = text(row, "code");
        let name = text(row, "name");
        // Read again at apply time: anything changed elsewhere since the preview fails this line only.
        let (store_map, store_ambiguous) = stores(db).await?;
        if store_ambiguous.contains(&key(&store_code)) {
            out.push(fail(row, ambiguous_store(&store_code)));
            continue;
        }
        let Some(store) = store_map.get(&key(&store_code)) else {
            out.push(fail(row, format!("Le magasin « {store_code} » n'existe plus.")));
            continue;
        };
        let (now, ambiguous) = locations(db).await?;
        let k = (store.id, key(&code));
        if ambiguous.contains(&k) {
            out.push(fail(row, ambiguous_location(&store.code, &code)));
            continue;
        }
        let result = match (row.action.as_str(), now.get(&k)) {
            ("create", None) => queries::create_stock_location(
                db,
                CreateStockLocationInput {
                    warehouse_id: store.id,
                    code,
                    name,
                    is_default: Some(false),
                },
            )
            .await
            .map(|_| "created"),
            ("create", Some(_)) => {
                out.push(fail(
                    row,
                    format!("L'emplacement « {code} » a été créé entre-temps : relancez l'import."),
                ));
                continue;
            }
            (_, None) => {
                out.push(fail(row, format!("L'emplacement « {code} » n'existe plus.")));
                continue;
            }
            (_, Some(loc)) => {
                let new_name = (!name.is_empty() && name != loc.name).then_some(name);
                let new_active = row.normalized["active"].as_bool().filter(|a| *a != loc.active);
                if new_name.is_none() && new_active.is_none() {
                    out.push(skipped(row));
                    continue;
                }
                queries::update_stock_location(
                    db,
                    loc.id,
                    loc.row_version,
                    UpdateStockLocationInput {
                        code: None,
                        name: new_name,
                        is_default: None,
                        is_active: new_active,
                    },
                )
                .await
                .map(|_| "updated")
            }
        };
        out.push(match result {
            Ok(status) => RowApplied {
                row_id: row.id,
                status,
                message: String::new(),
            },
            Err(e) => fail(row, app_error_text(&e)),
        });
    }
    Ok(out)
}

pub async fn export_locations(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT w.code AS store_code, l.code, l.name, l.is_active \
             FROM stock_locations l JOIN warehouses w ON w.id = l.warehouse_id \
             ORDER BY w.code, l.code"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let active: i64 = r.try_get("", "is_active")?;
            Ok(vec![
                r.try_get("", "store_code")?,
                r.try_get("", "code")?,
                r.try_get("", "name")?,
                if active != 0 { "oui" } else { "non" }.to_string(),
            ])
        })
        .collect()
}
