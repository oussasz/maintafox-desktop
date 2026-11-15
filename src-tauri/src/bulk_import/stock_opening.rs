//! Opening stock (stock d'ouverture): the quantities counted when Maintafox is set up, one line per
//! article and stock location, valued at a unit cost.
//!
//! Advisor rulings 2026-11-06 (stock §5 Q4):
//! - each line posts the same stock entry as the stock screen's manual adjustment (ADJUST_IN with
//!   its unit cost, reason "Saisie inventaire", comment "Inventaire d'ouverture (import)"), so the
//!   weighted-average cost starts from the imported cost, and writes one audit event;
//! - a line is refused when the article already has any stock movement at that location: an
//!   opening stock is posted once, later changes go through the stock screen (adjustment or count);
//! - lot- or serial-tracked articles and non-stock articles are refused (a tracked article needs
//!   its lots entered one by one on the stock screen);
//! - a blank date is the day of the import; a date in the future or inside a closed stock period
//!   is refused.
//!
//! Nothing is ever deleted or reversed by this import.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::equipment_meter::parse_number;
use super::org_tree::key;
use super::stock_store::{ambiguous_location, ambiguous_store, locations, stores, Location, Store};
use super::{parse_date, Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::inventory::domain::InventoryStockAdjustInput;
use crate::inventory::queries::{adjust_stock_full, stock_history_exists, AdjustStockOptions};

/// Movement reason of the entry (`inventory.movement_type` lookup, "Saisie inventaire").
const REASON_CODE: &str = "INVENTAIRE";
/// Comment written on every movement posted by this import.
pub const OPENING_NOTE: &str = "Inventaire d'ouverture (import)";
/// Audit action of one imported line.
pub const AUDIT_ACTION: &str = "inventory.opening_stock.imported";

pub const SPEC: EntitySpec = EntitySpec {
    code: "stock_opening",
    label: "Stock d'ouverture",
    order: 6,
    needs: "Articles, magasins et emplacements.",
    permission: crate::rbac::permissions::INV_MANAGE,
    columns: &[
        Column {
            key: "article_code",
            header: "code article",
            required: true,
            aliases: &["article", "code_article", "article_code", "reference"],
            hint: "Code de l'article, tel qu'il existe dans Stock.",
        },
        Column {
            key: "store_code",
            header: "code magasin",
            required: true,
            aliases: &["magasin", "code_magasin", "store_code", "warehouse", "code_entrepot"],
            hint: "Code du magasin.",
        },
        Column {
            key: "location_code",
            header: "code emplacement",
            required: true,
            aliases: &["emplacement", "code_emplacement", "location_code", "casier"],
            hint: "Code de l'emplacement dans ce magasin.",
        },
        Column {
            key: "quantity",
            header: "quantité",
            required: true,
            aliases: &["quantite", "qte", "quantity", "stock"],
            hint: "Quantité comptée, dans l'unité de stock de l'article. Plus grande que 0.",
        },
        Column {
            key: "unit_cost",
            header: "coût unitaire",
            required: true,
            aliases: &["cout_unitaire", "cout", "prix_unitaire", "unit_cost", "pump"],
            hint: "Coût d'une unité, dans la devise de l'entreprise. 0 ou plus.",
        },
        Column {
            key: "opening_date",
            header: "date d'ouverture",
            required: false,
            aliases: &["date_d_ouverture", "date_ouverture", "date", "opening_date"],
            hint: "JJ/MM/AAAA. Vide : le jour de l'import. Ni dans le futur, ni dans une période de stock clôturée.",
        },
    ],
};

fn cell(raw: &HashMap<String, String>, field: &str) -> String {
    raw.get(field).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn has_error(messages: &[RowMessage]) -> bool {
    messages.iter().any(|m| m.severity == "error")
}

struct Article {
    id: i64,
    code: String,
    active: bool,
    tracked: bool,
    non_stock: bool,
}

type ArticleMap = (HashMap<String, Article>, HashSet<String>);
type StoreMap = (HashMap<String, Store>, HashSet<String>);
type LocationMap = (HashMap<(i64, String), Location>, HashSet<(i64, String)>);

/// Every article by code key. Codes that differ only by letter case land in `ambiguous`.
async fn articles(db: &impl ConnectionTrait) -> AppResult<ArticleMap> {
    let mut map: HashMap<String, Article> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT a.id, a.article_code AS code, a.is_active, \
                    COALESCE(a.requires_batch_tracking, 0) AS batch, \
                    COALESCE(a.requires_serial_tracking, 0) AS serial, \
                    lv.code AS stocking_code \
             FROM articles a LEFT JOIN lookup_values lv ON lv.id = a.stocking_type_value_id"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let stocking: Option<String> = r.try_get("", "stocking_code")?;
        let k = key(&code);
        if map.contains_key(&k) {
            ambiguous.insert(k.clone());
        }
        map.insert(
            k,
            Article {
                id: r.try_get("", "id")?,
                code,
                active: r.try_get::<i64>("", "is_active")? != 0,
                tracked: r.try_get::<i64>("", "batch")? != 0 || r.try_get::<i64>("", "serial")? != 0,
                non_stock: stocking.is_some_and(|s| s.eq_ignore_ascii_case("NON_STOCK")),
            },
        );
    }
    Ok((map, ambiguous))
}

fn history_message(article: &str, store: &str, location: &str) -> String {
    format!(
        "L'article « {article} » a déjà des mouvements à l'emplacement « {location} » du magasin « {store} » : \
         le stock d'ouverture n'est saisi qu'une fois, corrigez-le avec un ajustement ou un inventaire dans Stock."
    )
}

/// The article (if it can take an opening stock) and the location of a line; a message is pushed
/// for every problem found.
fn resolve<'a>(
    messages: &mut Vec<RowMessage>,
    codes: (&str, &str, &str),
    article_map: &'a ArticleMap,
    store_map: &StoreMap,
    location_map: &LocationMap,
) -> (Option<&'a Article>, Option<i64>) {
    let (article_code, store_code, location_code) = codes;
    let mut article = None;
    if !article_code.is_empty() {
        let k = key(article_code);
        if article_map.1.contains(&k) {
            messages.push(RowMessage::error(
                "OPENING_IMPORT_ARTICLE_AMBIGUOUS",
                format!(
                    "Plusieurs articles ont le code « {article_code} » à la casse près : renommez-les dans Stock avant d'importer."
                ),
            ));
        } else if let Some(a) = article_map.0.get(&k) {
            if !a.active {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_ARTICLE_INACTIVE",
                    format!(
                        "L'article « {} » est inactif : réactivez-le avant d'y saisir du stock.",
                        a.code
                    ),
                ));
            } else if a.non_stock {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_ARTICLE_NON_STOCK",
                    format!(
                        "L'article « {} » est hors stock : il n'a pas de stock à saisir.",
                        a.code
                    ),
                ));
            } else if a.tracked {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_ARTICLE_TRACKED",
                    format!(
                        "L'article « {} » est suivi par lot ou numéro de série : saisissez son stock lot par lot dans Stock.",
                        a.code
                    ),
                ));
            } else {
                article = Some(a);
            }
        } else {
            messages.push(RowMessage::error(
                "OPENING_IMPORT_ARTICLE_UNKNOWN",
                format!("L'article « {article_code} » n'existe pas : importez d'abord les articles."),
            ));
        }
    }
    let mut location_id = None;
    if !store_code.is_empty() {
        let k = key(store_code);
        if store_map.1.contains(&k) {
            messages.push(RowMessage::error(
                "OPENING_IMPORT_STORE_AMBIGUOUS",
                ambiguous_store(store_code),
            ));
        } else if let Some(store) = store_map.0.get(&k) {
            if !store.active {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_STORE_INACTIVE",
                    format!(
                        "Le magasin « {} » est inactif : réactivez-le avant d'y saisir du stock.",
                        store.code
                    ),
                ));
            } else if !location_code.is_empty() {
                let lk = (store.id, key(location_code));
                if location_map.1.contains(&lk) {
                    messages.push(RowMessage::error(
                        "OPENING_IMPORT_LOCATION_AMBIGUOUS",
                        ambiguous_location(&store.code, location_code),
                    ));
                } else if let Some(loc) = location_map.0.get(&lk) {
                    if loc.active {
                        location_id = Some(loc.id);
                    } else {
                        messages.push(RowMessage::error(
                            "OPENING_IMPORT_LOCATION_INACTIVE",
                            format!(
                                "L'emplacement « {location_code} » du magasin « {} » est inactif : réactivez-le avant d'y saisir du stock.",
                                store.code
                            ),
                        ));
                    }
                } else {
                    messages.push(RowMessage::error(
                        "OPENING_IMPORT_LOCATION_UNKNOWN",
                        format!(
                            "L'emplacement « {location_code} » n'existe pas dans le magasin « {} » : importez d'abord les emplacements.",
                            store.code
                        ),
                    ));
                }
            }
        } else {
            messages.push(RowMessage::error(
                "OPENING_IMPORT_STORE_UNKNOWN",
                format!("Le magasin « {store_code} » n'existe pas : importez d'abord les magasins."),
            ));
        }
    }
    (article, location_id)
}

/// The day the posting is dated, checked the way the stock screen checks a back-dated movement.
/// The inner `Err` is the message for the line.
async fn check_date(db: &impl ConnectionTrait, date: Option<chrono::NaiveDate>) -> AppResult<Result<(), RowMessage>> {
    let today = crate::inventory::fx::today();
    let day = date
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| today.clone());
    if day > today {
        return Ok(Err(RowMessage::error(
            "OPENING_IMPORT_DATE_FUTURE",
            "La date d'ouverture ne peut pas être dans le futur.",
        )));
    }
    match crate::inventory::periods::ensure_posting_date_open(db, &day).await {
        Ok(()) => Ok(Ok(())),
        Err(AppError::ValidationFailed(_)) => Ok(Err(RowMessage::error(
            "OPENING_IMPORT_DATE_CLOSED",
            format!(
                "Le {} est dans une période de stock clôturée : choisissez une date après la dernière clôture.",
                date.map(|d| d.format("%d/%m/%Y").to_string())
                    .unwrap_or_else(|| "jour de l'import".to_string())
            ),
        ))),
        Err(e) => Err(e),
    }
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let article_map = articles(db).await?;
    let store_map = stores(db).await?;
    let location_map = locations(db).await?;
    let mut first_row_of: HashMap<(String, String, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let article_code = cell(raw, "article_code");
        let store_code = cell(raw, "store_code");
        let location_code = cell(raw, "location_code");
        for (value, code, text) in [
            (
                &article_code,
                "OPENING_IMPORT_ARTICLE",
                "Le code article est obligatoire.",
            ),
            (&store_code, "OPENING_IMPORT_STORE", "Le code magasin est obligatoire."),
            (
                &location_code,
                "OPENING_IMPORT_LOCATION",
                "Le code emplacement est obligatoire.",
            ),
        ] {
            if value.is_empty() {
                messages.push(RowMessage::error(code, text));
            }
        }
        if !has_error(&messages) {
            let k = (key(&article_code), key(&store_code), key(&location_code));
            if let Some(f) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_DUPLICATE",
                    format!(
                        "L'article « {article_code} » à l'emplacement « {location_code} » du magasin « {store_code} » apparaît déjà à la ligne {} du fichier.",
                        f + 2
                    ),
                ));
            } else {
                first_row_of.insert(k, i);
            }
        }

        let quantity = parse_number(&cell(raw, "quantity"));
        if !quantity.is_some_and(|q| q > 0.0) {
            messages.push(RowMessage::error(
                "OPENING_IMPORT_QUANTITY",
                "La quantité doit être un nombre plus grand que 0.",
            ));
        }
        let unit_cost = parse_number(&cell(raw, "unit_cost"));
        if !unit_cost.is_some_and(|c| c >= 0.0) {
            messages.push(RowMessage::error(
                "OPENING_IMPORT_COST",
                "Le coût unitaire doit être un nombre égal ou supérieur à 0.",
            ));
        }
        let date = match parse_date(raw.get("opening_date").map(String::as_str)) {
            Ok(d) => {
                if let Err(m) = check_date(db, d).await? {
                    messages.push(m);
                }
                d
            }
            Err(()) => {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_DATE",
                    "La date d'ouverture doit être au format JJ/MM/AAAA.",
                ));
                None
            }
        };

        let (article, location_id) = resolve(
            &mut messages,
            (&article_code, &store_code, &location_code),
            &article_map,
            &store_map,
            &location_map,
        );
        if let (Some(article), Some(location_id)) = (article, location_id) {
            if stock_history_exists(db, article.id, location_id).await? {
                messages.push(RowMessage::error(
                    "OPENING_IMPORT_HAS_MOVEMENTS",
                    history_message(&article.code, &store_code, &location_code),
                ));
            }
        }

        out.push(RowOutcome {
            action: if has_error(&messages) { "skip" } else { "create" },
            normalized: json!({
                "article_code": article_code,
                "store_code": store_code,
                "location_code": location_code,
                "quantity": quantity,
                "unit_cost": unit_cost,
                "opening_date": date.map(|d| d.format("%Y-%m-%d").to_string()),
            }),
            messages,
        });
    }
    Ok(out)
}

/// A refusal of the stock entry, in plain French for the import report.
fn app_error_text(e: &AppError) -> String {
    let AppError::ValidationFailed(v) = e else {
        return format!("Enregistrement impossible : {e}");
    };
    v.iter()
        .map(|msg| {
            let code = msg.split(':').next().unwrap_or_default().trim();
            match code {
                "GATE_INV_PERIOD_CLOSED" => {
                    "La date d'ouverture est désormais dans une période de stock clôturée.".to_string()
                }
                "GATE_INV_DATE_FUTURE" => "La date d'ouverture ne peut pas être dans le futur.".to_string(),
                "GATE_INV_ARTICLE_INACTIVE" | "GATE_INV_ARTICLE_UNKNOWN" => {
                    "L'article est devenu inactif ou n'existe plus.".to_string()
                }
                "GATE_INV_LOCATION_INACTIVE" | "GATE_INV_LOCATION_UNKNOWN" => {
                    "Le magasin ou l'emplacement est devenu inactif ou n'existe plus.".to_string()
                }
                "GATE_INV_LOT_OR_SERIAL_REQUIRED" | "GATE_INV_SERIAL_ONE_UNIT" | "GATE_INV_SERIAL_WHOLE_UNITS" => {
                    "L'article est désormais suivi par lot ou numéro de série : saisissez son stock dans Stock."
                        .to_string()
                }
                _ => format!("Entrée de stock refusée ({code})."),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn fail(row: &StagedRow, message: impl Into<String>) -> RowApplied {
    RowApplied {
        row_id: row.id,
        status: "failed",
        message: message.into(),
    }
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow], user_id: i32) -> AppResult<Vec<RowApplied>> {
    // Read once, at the start of the apply step. The stock entry itself checks again, inside its
    // own transaction, that the article and the location are still active, the date still open,
    // the article untracked and without history at the location (`first_entry_only`); it writes
    // the line's audit event in that same transaction.
    let article_map = articles(db).await?;
    let store_map = stores(db).await?;
    let location_map = locations(db).await?;
    let mut out = Vec::new();
    for row in rows {
        if row.action != "create" {
            out.push(RowApplied {
                row_id: row.id,
                status: "skipped",
                message: String::new(),
            });
            continue;
        }
        let text = |field: &str| row.normalized[field].as_str().unwrap_or_default().to_string();
        let (article_code, store_code, location_code) =
            (text("article_code"), text("store_code"), text("location_code"));
        let (Some(quantity), Some(unit_cost)) = (
            row.normalized["quantity"].as_f64(),
            row.normalized["unit_cost"].as_f64(),
        ) else {
            out.push(fail(row, "Quantité ou coût unitaire illisible."));
            continue;
        };
        let opening_date = row.normalized["opening_date"].as_str().map(str::to_string);

        let mut messages = Vec::new();
        let (article, location_id) = resolve(
            &mut messages,
            (&article_code, &store_code, &location_code),
            &article_map,
            &store_map,
            &location_map,
        );
        let (Some(article), Some(location_id)) = (article, location_id) else {
            let why = messages.into_iter().map(|m| m.message).collect::<Vec<_>>().join(" ");
            out.push(fail(row, why));
            continue;
        };

        let audit = crate::audit::writer::AuditEventInput {
            action_code: AUDIT_ACTION.to_string(),
            target_type: Some("stock_location_article".to_string()),
            target_id: Some(format!("{}:{location_id}", article.id)),
            actor_id: Some(i64::from(user_id)),
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(json!({
                "summary": OPENING_NOTE,
                "article_code": article.code,
                "store_code": store_code,
                "location_code": location_code,
                "quantity": quantity,
                "unit_cost": unit_cost,
                "posting_date": opening_date.clone().unwrap_or_else(crate::inventory::fx::today),
            })),
        };
        let posted = adjust_stock_full(
            db,
            InventoryStockAdjustInput {
                article_id: article.id,
                location_id,
                delta_qty: quantity,
                reason_code: Some(REASON_CODE.to_string()),
                notes: Some(OPENING_NOTE.to_string()),
                source_ref: None,
            },
            AdjustStockOptions {
                unit_cost: Some(unit_cost),
                posting_date: opening_date,
                actor_id: Some(i64::from(user_id)),
                first_entry_only: true,
                audit: Some(audit),
                ..Default::default()
            },
        )
        .await;
        if let Err(e) = posted {
            let refused_for_history = matches!(&e, AppError::ValidationFailed(v)
                if v.iter().any(|m| m.starts_with("GATE_INV_FIRST_ENTRY_HAS_HISTORY")));
            out.push(fail(
                row,
                if refused_for_history {
                    history_message(&article.code, &store_code, &location_code)
                } else {
                    app_error_text(&e)
                },
            ));
            continue;
        }
        out.push(RowApplied {
            row_id: row.id,
            status: "created",
            message: String::new(),
        });
    }
    Ok(out)
}

/// The stock on hand today, in the columns of the import (quantity and average cost per article
/// and location). It is a picture of the stock: imported again, each of its lines is refused,
/// because that stock already has movements.
pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT a.article_code, w.code AS store_code, l.code AS location_code, \
                    b.on_hand_qty, b.moving_avg_unit_cost \
             FROM stock_balances b \
             JOIN articles a ON a.id = b.article_id \
             JOIN stock_locations l ON l.id = b.location_id \
             JOIN warehouses w ON w.id = l.warehouse_id \
             WHERE b.on_hand_qty <> 0 \
             ORDER BY a.article_code, w.code, l.code"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let qty: f64 = r.try_get("", "on_hand_qty")?;
            let cost: Option<f64> = r.try_get("", "moving_avg_unit_cost")?;
            Ok(vec![
                r.try_get("", "article_code")?,
                r.try_get("", "store_code")?,
                r.try_get("", "location_code")?,
                qty.to_string(),
                cost.map(|c| c.to_string()).unwrap_or_default(),
                String::new(),
            ])
        })
        .collect()
}
