//! Article import (articles du stock), keyed by article code.
//!
//! It does what the article form does (Advisor rulings 2026-11-06, stock §5 Q3):
//! - a new article needs its designation, stock unit, stocking type and tax category; the other
//!   columns are optional;
//! - an existing article gets only the filled cells, and every field the file does not carry
//!   keeps its value (the form rewrites them all);
//! - the stock thresholds follow the form's rules, checked on the values the article will have
//!   once the line is applied;
//! - "code fournisseur principal" and "réf. fournisseur" write the supplier source of the
//!   article, marked preferred (the article's supplier list on screen); the preview says when
//!   this changes the preferred supplier;
//! - "actif = non" deactivates an article; a new article is created active.
//!
//! One rule is stricter than the form: the stock unit of an article that already has stock
//! movements is not changed by an import, because its quantities are counted in that unit.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Value};

use super::equipment_meter::{parse_number, units};
use super::org_tree::{key, parse_active};
use super::{normalize_header, Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    InventoryArticle, InventoryArticleFilter, InventoryArticleInput, SupplierArticleSourceInput,
};
use crate::inventory::{queries, suppliers};

pub const SPEC: EntitySpec = EntitySpec {
    code: "stock_article",
    label: "Articles",
    order: 5,
    needs: "Fournisseurs (si vous renseignez le fournisseur principal), les familles d'articles (Stock), \
            les unités MEASURE.UNITS, les catégories fiscales et les criticités (Données de référence).",
    permission: crate::rbac::permissions::INV_MANAGE,
    columns: &[
        Column {
            key: "code",
            header: "code article",
            required: true,
            aliases: &["code", "article", "code_article", "reference", "article_code"],
            hint: "Code unique de l'article (enregistré en majuscules), par exemple ROUL-6205.",
        },
        Column {
            key: "name",
            header: "désignation",
            required: true,
            aliases: &["designation", "libelle", "nom", "article_name", "name"],
            hint: "Libellé de l'article. Obligatoire pour un nouvel article.",
        },
        Column {
            key: "unit",
            header: "unité",
            required: false,
            aliases: &["unite", "unite_de_stock", "unit", "uom"],
            hint:
                "Code ou symbole d'une unité active de MEASURE.UNITS (U, kg, L…). Obligatoire pour un nouvel article.",
        },
        Column {
            key: "family",
            header: "famille",
            required: false,
            aliases: &["famille_article", "family"],
            hint: "Code d'une famille d'articles active (Stock > Familles).",
        },
        Column {
            key: "mpn",
            header: "réf. fabricant",
            required: false,
            aliases: &[
                "ref_fabricant",
                "reference_fabricant",
                "manufacturer_part_number",
                "mpn",
            ],
            hint: "Référence de la pièce chez le fabricant.",
        },
        Column {
            key: "manufacturer",
            header: "fabricant",
            required: false,
            aliases: &["marque", "manufacturer", "manufacturer_name"],
            hint: "Nom du fabricant.",
        },
        Column {
            key: "supplier",
            header: "code fournisseur principal",
            required: false,
            aliases: &["fournisseur", "fournisseur_principal", "code_fournisseur", "supplier"],
            hint: "Code d'un fournisseur actif. Il devient le fournisseur préféré de l'article.",
        },
        Column {
            key: "supplier_ref",
            header: "réf. fournisseur",
            required: false,
            aliases: &["ref_fournisseur", "reference_fournisseur", "supplier_article_code"],
            hint: "Référence de l'article chez le fournisseur principal (demande la colonne fournisseur).",
        },
        Column {
            key: "min_stock",
            header: "stock mini",
            required: false,
            aliases: &["mini", "stock_minimum", "min_stock"],
            hint: "Nombre, 0 ou plus.",
        },
        Column {
            key: "reorder_point",
            header: "point de commande",
            required: false,
            aliases: &["seuil_de_commande", "reorder_point"],
            hint: "Nombre, au moins égal au stock mini.",
        },
        Column {
            key: "max_stock",
            header: "stock maxi",
            required: false,
            aliases: &["maxi", "stock_maximum", "max_stock"],
            hint: "Nombre, au moins égal au stock mini et au point de commande.",
        },
        Column {
            key: "order_qty",
            header: "quantité économique",
            required: false,
            aliases: &[
                "quantite_de_commande",
                "qte_commande",
                "quantite_economique",
                "economic_order_qty",
            ],
            hint: "Quantité de commande habituelle (quantité économique), plus que 0.",
        },
        Column {
            key: "criticality",
            header: "criticité",
            required: false,
            aliases: &["criticite", "criticality"],
            hint: "Code ou libellé d'une criticité active (Données de référence).",
        },
        Column {
            key: "stocking_type",
            header: "type de stockage",
            required: false,
            aliases: &["stockage", "type_stockage", "stocking_type"],
            hint: "stocké ou hors stock. Obligatoire pour un nouvel article.",
        },
        Column {
            key: "tax_category",
            header: "catégorie fiscale",
            required: false,
            aliases: &["categorie_fiscale", "tva", "tax_category"],
            hint: "Code ou libellé d'une catégorie fiscale active (TVA_20…). Obligatoire pour un nouvel article.",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: "oui ou non. Vide : un nouvel article est actif, un article existant ne change pas.",
        },
    ],
};

// ─── Reference lists ──────────────────────────────────────────────────────────

/// Values of a lookup domain (deleted ones left out) by code key and label keys: (id, active).
/// A label shared with another value keeps the active one.
async fn lookup(db: &impl ConnectionTrait, domain: &str) -> AppResult<HashMap<String, (i64, bool)>> {
    let mut out = HashMap::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id, lv.code, lv.label, lv.fr_label, lv.en_label, lv.is_active FROM lookup_values lv \
             JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = ? AND lv.deleted_at IS NULL \
             ORDER BY lv.is_active DESC, lv.sort_order, lv.id",
            [domain.into()],
        ))
        .await?
    {
        let id: i64 = r.try_get("", "id")?;
        let active = r.try_get::<i64>("", "is_active")? != 0;
        let code: String = r.try_get("", "code")?;
        out.entry(key(&code)).or_insert((id, active));
        for col in ["label", "fr_label", "en_label"] {
            if let Some(label) = r.try_get::<Option<String>>("", col)? {
                out.entry(key(&label)).or_insert((id, active));
            }
        }
    }
    Ok(out)
}

/// Stocking type from a code, a label, or the short words "stocké" / "hors stock".
fn stocking_type(raw: &str, types: &HashMap<String, (i64, bool)>) -> Option<(i64, bool)> {
    let wanted = match normalize_header(raw).as_str() {
        "stocke" | "stock" => "stocked".to_string(),
        "hors_stock" | "non_stocke" | "non_stock" => "non_stock".to_string(),
        _ => key(raw),
    };
    types.get(&wanted).copied()
}

struct Lists {
    units: HashMap<String, (i64, bool)>,
    units_ambiguous: HashSet<String>,
    families: HashMap<String, (i64, bool)>,
    family_active: HashMap<i64, (String, bool)>,
    criticality: HashMap<String, (i64, bool)>,
    stocking: HashMap<String, (i64, bool)>,
    tax: HashMap<String, (i64, bool)>,
    /// Suppliers by code key: (id, active); codes held twice (letter case aside) are in `suppliers_ambiguous`.
    suppliers: HashMap<String, (i64, bool)>,
    suppliers_ambiguous: HashSet<String>,
    /// Stores and locations by id: (code, active), for the article's preferred store hint.
    stores: HashMap<i64, (String, bool)>,
    locations: HashMap<i64, (String, bool)>,
}

async fn active_by_id(db: &impl ConnectionTrait, sql: &str) -> AppResult<HashMap<i64, (String, bool)>> {
    let mut out = HashMap::new();
    for r in db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?
    {
        let active: i64 = r.try_get("", "is_active")?;
        out.insert(r.try_get("", "id")?, (r.try_get("", "code")?, active != 0));
    }
    Ok(out)
}

async fn lists(db: &impl ConnectionTrait) -> AppResult<Lists> {
    let (units, units_ambiguous) = units(db).await?;
    let family_active = active_by_id(db, "SELECT id, code, is_active FROM article_families").await?;
    let families = family_active
        .iter()
        .map(|(id, (code, active))| (key(code), (*id, *active)))
        .collect();
    let mut suppliers = HashMap::new();
    let mut suppliers_ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, is_active FROM inventory_suppliers".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let active: i64 = r.try_get("", "is_active")?;
        if suppliers
            .insert(key(&code), (r.try_get("", "id")?, active != 0))
            .is_some()
        {
            suppliers_ambiguous.insert(key(&code));
        }
    }
    Ok(Lists {
        units,
        units_ambiguous,
        families,
        family_active,
        criticality: lookup(db, "equipment.criticality").await?,
        stocking: lookup(db, "inventory.stocking_type").await?,
        tax: lookup(db, "inventory.tax_category").await?,
        suppliers,
        suppliers_ambiguous,
        stores: active_by_id(db, "SELECT id, code, is_active FROM warehouses").await?,
        locations: active_by_id(db, "SELECT id, code, is_active FROM stock_locations").await?,
    })
}

// ─── Current state ────────────────────────────────────────────────────────────

/// The preferred supplier source of an article: (supplier id, supplier code, supplier reference).
type Preferred = (i64, String, Option<String>);

struct Current {
    article: InventoryArticle,
    has_movement: bool,
    preferred: Option<Preferred>,
}

/// Every article by code key, with what the import compares against. Codes held twice (letter
/// case aside) are returned apart.
async fn current(db: &DatabaseConnection) -> AppResult<(HashMap<String, Current>, HashSet<String>)> {
    let mut moved: HashSet<i64> = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT article_id FROM inventory_transactions".to_string(),
        ))
        .await?
    {
        moved.insert(r.try_get("", "article_id")?);
    }
    let mut preferred: HashMap<i64, Preferred> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT sas.article_id, sas.supplier_id, s.code, sas.supplier_article_code \
             FROM inventory_supplier_article_sources sas JOIN inventory_suppliers s ON s.id = sas.supplier_id \
             WHERE sas.is_preferred = 1 ORDER BY sas.id"
                .to_string(),
        ))
        .await?
    {
        preferred.insert(
            r.try_get("", "article_id")?,
            (
                r.try_get("", "supplier_id")?,
                r.try_get("", "code")?,
                r.try_get("", "supplier_article_code")?,
            ),
        );
    }
    let mut out = HashMap::new();
    let mut ambiguous = HashSet::new();
    for a in queries::list_articles(db, InventoryArticleFilter { search: None }).await? {
        let k = key(&a.article_code);
        if out.contains_key(&k) {
            ambiguous.insert(k.clone());
        }
        out.insert(
            k,
            Current {
                has_movement: moved.contains(&a.id),
                preferred: preferred.remove(&a.id),
                article: a,
            },
        );
    }
    Ok((out, ambiguous))
}

fn ambiguous_text(code: &str) -> String {
    format!("Plusieurs articles ont le code « {code} » à la casse près : renommez-les dans Stock avant d'importer.")
}

// ─── Lines ────────────────────────────────────────────────────────────────────

/// What a line sets. `None` leaves the current value.
#[derive(Default)]
struct Wanted {
    name: Option<String>,
    unit_id: Option<i64>,
    family_id: Option<i64>,
    mpn: Option<String>,
    manufacturer: Option<String>,
    supplier_id: Option<i64>,
    supplier_code: Option<String>,
    supplier_ref: Option<String>,
    min_stock: Option<f64>,
    reorder_point: Option<f64>,
    max_stock: Option<f64>,
    order_qty: Option<f64>,
    criticality_id: Option<i64>,
    stocking_id: Option<i64>,
    tax_id: Option<i64>,
    active: Option<bool>,
}

impl Wanted {
    fn to_json(&self, code: &str) -> Value {
        json!({
            "code": code,
            "name": self.name,
            "unit_id": self.unit_id,
            "family_id": self.family_id,
            "mpn": self.mpn,
            "manufacturer": self.manufacturer,
            "supplier_id": self.supplier_id,
            "supplier_code": self.supplier_code,
            "supplier_ref": self.supplier_ref,
            "min_stock": self.min_stock,
            "reorder_point": self.reorder_point,
            "max_stock": self.max_stock,
            "order_qty": self.order_qty,
            "criticality_id": self.criticality_id,
            "stocking_id": self.stocking_id,
            "tax_id": self.tax_id,
            "active": self.active,
        })
    }

    fn from_json(v: &Value) -> Self {
        let s = |f: &str| v[f].as_str().map(str::to_string);
        Self {
            name: s("name"),
            unit_id: v["unit_id"].as_i64(),
            family_id: v["family_id"].as_i64(),
            mpn: s("mpn"),
            manufacturer: s("manufacturer"),
            supplier_id: v["supplier_id"].as_i64(),
            supplier_code: s("supplier_code"),
            supplier_ref: s("supplier_ref"),
            min_stock: v["min_stock"].as_f64(),
            reorder_point: v["reorder_point"].as_f64(),
            max_stock: v["max_stock"].as_f64(),
            order_qty: v["order_qty"].as_f64(),
            criticality_id: v["criticality_id"].as_i64(),
            stocking_id: v["stocking_id"].as_i64(),
            tax_id: v["tax_id"].as_i64(),
            active: v["active"].as_bool(),
        }
    }

    /// The form input once the line is applied to `before` (`None` for a new article). `None`
    /// when a field a new article needs is missing.
    fn input(&self, code: &str, before: Option<&InventoryArticle>) -> Option<InventoryArticleInput> {
        Some(match before {
            None => InventoryArticleInput {
                article_code: code.to_string(),
                article_name: self.name.clone()?,
                family_id: self.family_id,
                unit_value_id: self.unit_id?,
                criticality_value_id: self.criticality_id,
                stocking_type_value_id: self.stocking_id?,
                tax_category_value_id: self.tax_id?,
                procurement_category_value_id: None,
                preferred_warehouse_id: None,
                preferred_location_id: None,
                // The form starts these at 0 ("no threshold").
                min_stock: self.min_stock.unwrap_or(0.0),
                max_stock: self.max_stock,
                reorder_point: self.reorder_point.unwrap_or(0.0),
                safety_stock: 0.0,
                manufacturer_name: self.manufacturer.clone(),
                manufacturer_part_number: self.mpn.clone(),
                oem_part_number: None,
                replenishment_policy_code: None,
                economic_order_qty: self.order_qty,
                minimum_order_qty: None,
                maximum_order_qty: None,
                order_multiple_qty: None,
                lead_time_days: None,
                review_period_days: None,
                abc_class_code: None,
                xyz_class_code: None,
                is_critical_spare: None,
                requires_expiration: None,
                shelf_life_days: None,
                requires_batch_tracking: None,
                barcode: None,
                requires_serial_tracking: None,
                is_active: Some(true),
            },
            Some(a) => InventoryArticleInput {
                article_code: a.article_code.clone(),
                article_name: self.name.clone().unwrap_or_else(|| a.article_name.clone()),
                family_id: self.family_id.or(a.family_id),
                unit_value_id: self.unit_id.unwrap_or(a.unit_value_id),
                criticality_value_id: self.criticality_id.or(a.criticality_value_id),
                stocking_type_value_id: self.stocking_id.unwrap_or(a.stocking_type_value_id),
                tax_category_value_id: self.tax_id.unwrap_or(a.tax_category_value_id),
                procurement_category_value_id: a.procurement_category_value_id,
                preferred_warehouse_id: a.preferred_warehouse_id,
                preferred_location_id: a.preferred_location_id,
                min_stock: self.min_stock.unwrap_or(a.min_stock),
                max_stock: self.max_stock.or(a.max_stock),
                reorder_point: self.reorder_point.unwrap_or(a.reorder_point),
                safety_stock: a.safety_stock,
                manufacturer_name: self.manufacturer.clone().or_else(|| a.manufacturer_name.clone()),
                manufacturer_part_number: self.mpn.clone().or_else(|| a.manufacturer_part_number.clone()),
                oem_part_number: a.oem_part_number.clone(),
                replenishment_policy_code: a.replenishment_policy_code.clone(),
                economic_order_qty: self.order_qty.or(a.economic_order_qty),
                minimum_order_qty: a.minimum_order_qty,
                maximum_order_qty: a.maximum_order_qty,
                order_multiple_qty: a.order_multiple_qty,
                lead_time_days: a.lead_time_days,
                review_period_days: a.review_period_days,
                abc_class_code: a.abc_class_code.clone(),
                xyz_class_code: a.xyz_class_code.clone(),
                is_critical_spare: Some(a.is_critical_spare != 0),
                requires_expiration: Some(a.requires_expiration != 0),
                shelf_life_days: a.shelf_life_days,
                requires_batch_tracking: Some(a.requires_batch_tracking != 0),
                barcode: a.barcode.clone(),
                requires_serial_tracking: Some(a.requires_serial_tracking != 0),
                is_active: Some(self.active.unwrap_or(a.is_active != 0)),
            },
        })
    }

    /// Whether the article fields change (the supplier source is compared apart).
    fn article_changes(&self, a: &InventoryArticle) -> bool {
        let differs_f = |w: Option<f64>, cur: Option<f64>| w.is_some_and(|v| Some(v) != cur);
        self.name.as_ref().is_some_and(|n| *n != a.article_name)
            || self.unit_id.is_some_and(|u| u != a.unit_value_id)
            || self.family_id.is_some_and(|f| Some(f) != a.family_id)
            || self
                .mpn
                .as_ref()
                .is_some_and(|m| Some(m) != a.manufacturer_part_number.as_ref())
            || self
                .manufacturer
                .as_ref()
                .is_some_and(|m| Some(m) != a.manufacturer_name.as_ref())
            || differs_f(self.min_stock, Some(a.min_stock))
            || differs_f(self.reorder_point, Some(a.reorder_point))
            || differs_f(self.max_stock, a.max_stock)
            || differs_f(self.order_qty, a.economic_order_qty)
            || self.criticality_id.is_some_and(|c| Some(c) != a.criticality_value_id)
            || self.stocking_id.is_some_and(|s| s != a.stocking_type_value_id)
            || self.tax_id.is_some_and(|t| t != a.tax_category_value_id)
            || self.active.is_some_and(|v| v != (a.is_active != 0))
    }

    /// Whether the supplier source changes: a new preferred supplier, or a new reference.
    fn source_changes(&self, preferred: Option<&Preferred>) -> bool {
        let Some(sid) = self.supplier_id else {
            return false;
        };
        match preferred {
            Some((pid, _, pref)) if *pid == sid => self.supplier_ref.as_ref().is_some_and(|r| Some(r) != pref.as_ref()),
            _ => true,
        }
    }
}

/// The form's threshold rules, as French row messages.
fn threshold_messages(input: &InventoryArticleInput) -> Vec<RowMessage> {
    let mut texts: Vec<&str> = Vec::new();
    if let Some(max) = input.max_stock {
        if max < input.min_stock {
            texts.push("Le stock maxi doit être au moins égal au stock mini.");
        }
        if max < input.reorder_point {
            texts.push("Le stock maxi doit être au moins égal au point de commande.");
        }
    }
    if input.reorder_point < input.min_stock {
        texts.push("Le point de commande doit être au moins égal au stock mini.");
    }
    texts
        .into_iter()
        .map(|t| RowMessage::error("ARTICLE_IMPORT_THRESHOLDS", t))
        .collect()
}

/// How a reference cell resolves against the article's current value.
enum Picked {
    Unknown,
    /// Retired, and not the article's current value.
    Inactive,
    /// Retired, but the article already has it: nothing changes.
    Same,
    Set(i64),
}

fn pick(found: Option<(i64, bool)>, current: Option<i64>) -> Picked {
    match found {
        None => Picked::Unknown,
        Some((id, true)) => Picked::Set(id),
        Some((id, false)) if Some(id) == current => Picked::Same,
        Some(_) => Picked::Inactive,
    }
}

/// Values the line keeps from the article that the article form refuses on save once they are
/// retired: the family and the preferred store and location.
fn kept_value_messages(input: &InventoryArticleInput, l: &Lists) -> Vec<RowMessage> {
    let mut out = Vec::new();
    if let Some((code, false)) = input.family_id.and_then(|id| l.family_active.get(&id)) {
        out.push(RowMessage::error(
            "ARTICLE_IMPORT_FAMILY_INACTIVE",
            format!(
                "La famille « {code} » de cet article est inactive : indiquez une famille active dans la colonne famille, ou réactivez-la."
            ),
        ));
    }
    let store = input.preferred_warehouse_id.and_then(|id| l.stores.get(&id));
    let location = input.preferred_location_id.and_then(|id| l.locations.get(&id));
    for (what, found) in [("magasin", store), ("emplacement", location)] {
        if let Some((code, false)) = found {
            out.push(RowMessage::error(
                "ARTICLE_IMPORT_PREFERRED_STORE_INACTIVE",
                format!(
                    "Le {what} préféré « {code} » de cet article est inactif : corrigez-le sur la fiche article avant d'importer."
                ),
            ));
        }
    }
    out
}

/// A refusal of the article form, in plain French for the import report.
fn app_error_text(e: &AppError) -> String {
    let AppError::ValidationFailed(v) = e else {
        return format!("Enregistrement impossible : {e}");
    };
    v.iter()
        .map(|msg| {
            let code = msg.split(':').next().unwrap_or_default().trim();
            match code {
                "GATE_RECORD_CHANGED_ELSEWHERE" => {
                    "L'article a été modifié ailleurs entre-temps : relancez l'import.".to_string()
                }
                "GATE_INV_CODE_EXISTS" => "Ce code article existe déjà.".to_string(),
                "GATE_INV_THRESHOLDS" | "GATE_INV_THRESHOLD_NEGATIVE" => {
                    "Les seuils de stock sont incohérents (mini, point de commande, maxi).".to_string()
                }
                "GATE_INV_FAMILY_INACTIVE" | "GATE_INV_FAMILY_UNKNOWN" => {
                    "La famille de l'article est inactive ou n'existe plus.".to_string()
                }
                "GATE_INV_LOCATION_INACTIVE" | "GATE_INV_LOCATION_UNKNOWN" => {
                    "Le magasin ou l'emplacement préféré de l'article est inactif ou n'existe plus.".to_string()
                }
                "GATE_SUPPLIER_INACTIVE" => "Le fournisseur est inactif.".to_string(),
                c if c.starts_with("GATE_INV_LOOKUP") || c.starts_with("GATE_UNIT") => {
                    "Une valeur de référence de la ligne n'est plus valable (unité, criticité, type de stockage ou catégorie fiscale).".to_string()
                }
                _ => format!("Enregistrement refusé par la fiche article ({code})."),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_error(m: &[RowMessage]) -> bool {
    m.iter().any(|x| x.severity == "error")
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let l = lists(db).await?;
    let (existing, ambiguous) = current(db).await?;
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut m: Vec<RowMessage> = Vec::new();
        let get = |f: &str| raw.get(f).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let code = get("code").unwrap_or_default();
        let mut first = false;
        if code.is_empty() {
            m.push(RowMessage::error(
                "ARTICLE_IMPORT_CODE",
                "Le code article est obligatoire.",
            ));
        } else if let Some(f) = first_row_of.get(&key(&code)) {
            m.push(RowMessage::error(
                "ARTICLE_IMPORT_DUPLICATE",
                format!("L'article « {code} » apparaît déjà à la ligne {} du fichier.", f + 2),
            ));
        } else {
            first_row_of.insert(key(&code), i);
            first = true;
        }
        let cur = existing.get(&key(&code));
        let before = cur.map(|c| &c.article);
        let mut w = Wanted {
            name: get("name"),
            mpn: get("mpn"),
            manufacturer: get("manufacturer"),
            supplier_ref: get("supplier_ref"),
            ..Wanted::default()
        };

        if let Some(u) = get("unit") {
            if l.units_ambiguous.contains(&key(&u)) {
                m.push(RowMessage::error(
                    "ARTICLE_IMPORT_UNIT_AMBIGUOUS",
                    format!("« {u} » désigne plusieurs unités de MEASURE.UNITS : utilisez le code de l'unité."),
                ));
            } else {
                match l.units.get(&key(&u)) {
                    None => m.push(RowMessage::error(
                        "ARTICLE_IMPORT_UNIT",
                        format!(
                            "L'unité « {u} » n'existe pas dans la liste de référence MEASURE.UNITS : ajoutez-la d'abord dans Données de référence."
                        ),
                    )),
                    Some((id, active)) => {
                        let keeps = before.is_some_and(|a| a.unit_value_id == *id);
                        if !active && !keeps {
                            m.push(RowMessage::error(
                                "ARTICLE_IMPORT_UNIT_INACTIVE",
                                format!("L'unité « {u} » n'est plus active dans Données de référence."),
                            ));
                        } else if cur.is_some_and(|c| c.has_movement && c.article.unit_value_id != *id) {
                            m.push(RowMessage::error(
                                "ARTICLE_IMPORT_UNIT_LOCKED",
                                "Cet article a déjà des mouvements de stock : son unité ne change pas par import.",
                            ));
                        } else {
                            w.unit_id = Some(*id);
                        }
                    }
                }
            }
        }
        // A retired value is accepted only as the one the article already has (nothing changes).
        if let Some(f) = get("family") {
            match pick(l.families.get(&key(&f)).copied(), before.and_then(|a| a.family_id)) {
                Picked::Unknown => m.push(RowMessage::error(
                    "ARTICLE_IMPORT_FAMILY",
                    format!("La famille « {f} » n'existe pas : créez-la d'abord dans Stock > Familles d'articles."),
                )),
                Picked::Inactive => m.push(RowMessage::error(
                    "ARTICLE_IMPORT_FAMILY_INACTIVE",
                    format!("La famille « {f} » est inactive."),
                )),
                Picked::Same => {}
                Picked::Set(id) => w.family_id = Some(id),
            }
        }
        if let Some(v) = get("criticality") {
            match pick(
                l.criticality.get(&key(&v)).copied(),
                before.and_then(|a| a.criticality_value_id),
            ) {
                Picked::Unknown | Picked::Inactive => m.push(RowMessage::error(
                    "ARTICLE_IMPORT_CRITICALITY",
                    format!(
                        "La criticité « {v} » n'existe pas ou est inactive : ajoutez-la d'abord dans Données de référence."
                    ),
                )),
                Picked::Same => {}
                Picked::Set(id) => w.criticality_id = Some(id),
            }
        }
        if let Some(v) = get("tax_category") {
            match pick(
                l.tax.get(&key(&v)).copied(),
                before.map(|a| a.tax_category_value_id),
            ) {
                Picked::Unknown | Picked::Inactive => m.push(RowMessage::error(
                    "ARTICLE_IMPORT_TAX",
                    format!(
                        "La catégorie fiscale « {v} » n'existe pas ou est inactive : ajoutez-la d'abord dans Données de référence."
                    ),
                )),
                Picked::Same => {}
                Picked::Set(id) => w.tax_id = Some(id),
            }
        }
        if let Some(v) = get("stocking_type") {
            match pick(stocking_type(&v, &l.stocking), before.map(|a| a.stocking_type_value_id)) {
                Picked::Unknown | Picked::Inactive => m.push(RowMessage::error(
                    "ARTICLE_IMPORT_STOCKING",
                    format!("Type de stockage « {v} » inconnu : mettez stocké ou hors stock."),
                )),
                Picked::Same => {}
                Picked::Set(id) => w.stocking_id = Some(id),
            }
        }
        for (field, header, above_zero) in [
            ("min_stock", "stock mini", false),
            ("reorder_point", "point de commande", false),
            ("max_stock", "stock maxi", false),
            ("order_qty", "quantité économique", true),
        ] {
            let Some(v) = get(field) else { continue };
            match parse_number(&v).filter(|n| if above_zero { *n > 0.0 } else { *n >= 0.0 }) {
                None => m.push(RowMessage::error(
                    "ARTICLE_IMPORT_NUMBER",
                    format!(
                        "« {header} » : « {v} » n'est pas un nombre {}.",
                        if above_zero {
                            "plus grand que 0"
                        } else {
                            "positif ou nul"
                        }
                    ),
                )),
                Some(n) => match field {
                    "min_stock" => w.min_stock = Some(n),
                    "reorder_point" => w.reorder_point = Some(n),
                    "max_stock" => w.max_stock = Some(n),
                    _ => w.order_qty = Some(n),
                },
            }
        }
        match get("supplier") {
            Some(s) => {
                if l.suppliers_ambiguous.contains(&key(&s)) {
                    m.push(RowMessage::error(
                        "ARTICLE_IMPORT_SUPPLIER_AMBIGUOUS",
                        format!(
                            "Plusieurs fournisseurs ont le code « {s} » à la casse près : renommez-les avant d'importer."
                        ),
                    ));
                } else {
                    match l.suppliers.get(&key(&s)) {
                        None => m.push(RowMessage::error(
                            "ARTICLE_IMPORT_SUPPLIER",
                            format!("Le fournisseur « {s} » n'existe pas : importez d'abord les fournisseurs."),
                        )),
                        // A deactivated supplier that is already the preferred one, with the same
                        // reference (or none), changes nothing and is accepted.
                        Some((id, false))
                            if cur.and_then(|c| c.preferred.as_ref()).is_some_and(|(pid, _, pref)| {
                                pid == id && w.supplier_ref.as_ref().is_none_or(|r| Some(r) == pref.as_ref())
                            }) =>
                        {
                            w.supplier_ref = None;
                        }
                        Some((_, false)) => m.push(RowMessage::error(
                            "ARTICLE_IMPORT_SUPPLIER_INACTIVE",
                            format!("Le fournisseur « {s} » est inactif."),
                        )),
                        Some((id, true)) => {
                            w.supplier_id = Some(*id);
                            w.supplier_code = Some(s);
                        }
                    }
                }
            }
            None => {
                if w.supplier_ref.is_some() {
                    m.push(RowMessage::error(
                        "ARTICLE_IMPORT_SUPPLIER_REF",
                        "La réf. fournisseur demande le code fournisseur principal sur la même ligne.",
                    ));
                }
            }
        }
        w.active = parse_active(raw.get("active")).unwrap_or_else(|()| {
            m.push(RowMessage::error(
                "ARTICLE_IMPORT_ACTIVE",
                "La colonne « actif » doit contenir oui ou non.",
            ));
            None
        });

        if first && ambiguous.contains(&key(&code)) {
            m.push(RowMessage::error(
                "ARTICLE_IMPORT_CODE_AMBIGUOUS",
                ambiguous_text(&code),
            ));
        }
        if first && before.is_none() {
            for (missing, text) in [
                (
                    w.name.is_none(),
                    "La désignation est obligatoire pour un nouvel article.",
                ),
                (get("unit").is_none(), "L'unité est obligatoire pour un nouvel article."),
                (
                    get("stocking_type").is_none(),
                    "Le type de stockage (stocké ou hors stock) est obligatoire pour un nouvel article.",
                ),
                (
                    get("tax_category").is_none(),
                    "La catégorie fiscale est obligatoire pour un nouvel article.",
                ),
            ] {
                if missing {
                    m.push(RowMessage::error("ARTICLE_IMPORT_REQUIRED", text));
                }
            }
            if w.active == Some(false) {
                m.push(RowMessage::error(
                    "ARTICLE_IMPORT_NEW_INACTIVE",
                    "Un nouvel article est créé actif : laissez « actif » vide ou mettez oui.",
                ));
            }
        }
        // Thresholds, family and preferred store on the values the article will have once the line
        // is applied: the article form re-checks all of them on every save.
        if first && !has_error(&m) {
            if let Some(input) = w.input(&code, before) {
                m.extend(threshold_messages(&input));
                if before.is_none_or(|a| w.article_changes(a)) {
                    m.extend(kept_value_messages(&input, &l));
                }
            }
        }
        if first && !has_error(&m) {
            if let (Some(c), Some(new_code)) = (cur, w.supplier_code.as_ref()) {
                if let Some((pid, old_code, _)) = &c.preferred {
                    if Some(*pid) != w.supplier_id {
                        m.push(RowMessage::warning(
                            "ARTICLE_IMPORT_PREFERRED_CHANGE",
                            format!("Le fournisseur préféré passe de « {old_code} » à « {new_code} »."),
                        ));
                    }
                }
            }
        }
        let action = if has_error(&m) || !first {
            "skip"
        } else {
            match cur {
                None => "create",
                Some(c) if w.article_changes(&c.article) || w.source_changes(c.preferred.as_ref()) => "update",
                Some(_) => "skip",
            }
        };
        out.push(RowOutcome {
            action,
            normalized: w.to_json(&code),
            messages: m,
        });
    }
    Ok(out)
}

/// Writes the preferred supplier source when the line changes it; the other fields of an existing
/// source are kept.
async fn write_source(db: &DatabaseConnection, article_id: i64, w: &Wanted) -> AppResult<()> {
    let Some(supplier_id) = w.supplier_id else {
        return Ok(());
    };
    let preferred: Option<Preferred> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sas.supplier_id, s.code, sas.supplier_article_code \
             FROM inventory_supplier_article_sources sas JOIN inventory_suppliers s ON s.id = sas.supplier_id \
             WHERE sas.article_id = ? AND sas.is_preferred = 1 ORDER BY sas.id DESC LIMIT 1",
            [article_id.into()],
        ))
        .await?
        .map(|r| -> AppResult<Preferred> {
            Ok((
                r.try_get("", "supplier_id")?,
                r.try_get("", "code")?,
                r.try_get("", "supplier_article_code")?,
            ))
        })
        .transpose()?;
    if !w.source_changes(preferred.as_ref()) {
        return Ok(());
    }
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT priority, lead_time_days, unit_price_hint, min_order_qty, supplier_article_code \
             FROM inventory_supplier_article_sources WHERE supplier_id = ? AND article_id = ?",
            [supplier_id.into(), article_id.into()],
        ))
        .await?;
    let (priority, lead, price, moq, reference) = match existing {
        Some(r) => (
            r.try_get::<Option<i64>>("", "priority")?,
            r.try_get::<Option<i64>>("", "lead_time_days")?,
            r.try_get::<Option<f64>>("", "unit_price_hint")?,
            r.try_get::<Option<f64>>("", "min_order_qty")?,
            r.try_get::<Option<String>>("", "supplier_article_code")?,
        ),
        None => (None, None, None, None, None),
    };
    suppliers::upsert_supplier_article_source(
        db,
        SupplierArticleSourceInput {
            supplier_id,
            article_id,
            is_preferred: Some(true),
            priority,
            lead_time_days: lead,
            unit_price_hint: price,
            min_order_qty: moq,
            supplier_article_code: w.supplier_ref.clone().or(reference),
            is_active: Some(true),
        },
    )
    .await?;
    Ok(())
}

/// The article with this code (letter case aside), read again at apply time. `Err` when several
/// articles match.
async fn article_by_code(db: &DatabaseConnection, code: &str) -> AppResult<Result<Option<InventoryArticle>, String>> {
    // Matched in Rust (any letter case, accented letters included), then read in full by its
    // stored code: SQLite's LOWER only folds ASCII letters.
    let mut ids: Vec<(i64, String)> = Vec::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, article_code FROM articles".to_string(),
        ))
        .await?
    {
        let stored: String = r.try_get("", "article_code")?;
        if key(&stored) == key(code) {
            ids.push((r.try_get("", "id")?, stored));
        }
    }
    if ids.len() > 1 {
        return Ok(Err(ambiguous_text(code)));
    }
    let Some((id, stored)) = ids.pop() else {
        return Ok(Ok(None));
    };
    Ok(Ok(queries::list_articles(
        db,
        InventoryArticleFilter { search: Some(stored) },
    )
    .await?
    .into_iter()
    .find(|a| a.id == id)))
}

/// Writes the article fields of one line: `Ok((status, article id))` or the reason it was refused.
async fn write_article(
    db: &DatabaseConnection,
    row: &StagedRow,
    code: &str,
    w: &Wanted,
) -> AppResult<Result<(&'static str, i64), String>> {
    let before = match article_by_code(db, code).await? {
        Err(reason) => return Ok(Err(reason)),
        Ok(found) => found,
    };
    if let Some(supplier_id) = w.supplier_id {
        let active = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT is_active FROM inventory_suppliers WHERE id = ?",
                [supplier_id.into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "is_active"))
            .transpose()?;
        if active != Some(1) {
            return Ok(Err(
                "Le fournisseur principal a été désactivé ou supprimé entre-temps : relancez l'import.".to_string(),
            ));
        }
    }
    Ok(match (row.action.as_str(), before) {
        ("create", Some(_)) => Err(format!(
            "L'article « {code} » a été créé entre-temps : relancez l'import."
        )),
        ("create", None) => match w.input(code, None) {
            None => Err("Désignation, unité, type de stockage ou catégorie fiscale manquant.".to_string()),
            Some(input) => queries::create_article(db, input)
                .await
                .map(|a| ("created", a.id))
                .map_err(|e| app_error_text(&e)),
        },
        (_, None) => Err(format!("L'article « {code} » n'existe plus.")),
        (_, Some(a)) => {
            if w.unit_id.is_some_and(|u| u != a.unit_value_id) {
                let moved = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT 1 AS x FROM inventory_transactions WHERE article_id = ? LIMIT 1",
                        [a.id.into()],
                    ))
                    .await?
                    .is_some();
                if moved {
                    return Ok(Err(
                        "Cet article a reçu des mouvements de stock entre-temps : son unité ne change pas par import."
                            .to_string(),
                    ));
                }
            }
            if !w.article_changes(&a) {
                Ok(("updated", a.id))
            } else {
                match w.input(code, Some(&a)) {
                    None => Err("Article illisible.".to_string()),
                    Some(input) => queries::update_article(db, a.id, a.row_version, input)
                        .await
                        .map(|_| ("updated", a.id))
                        .map_err(|e| app_error_text(&e)),
                }
            }
        }
    })
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        let done = |status: &'static str, message: String| RowApplied {
            row_id: row.id,
            status,
            message,
        };
        if row.action != "create" && row.action != "update" {
            out.push(done("skipped", "Déjà à jour.".to_string()));
            continue;
        }
        let code = row.normalized["code"].as_str().unwrap_or_default().to_string();
        let w = Wanted::from_json(&row.normalized);
        out.push(match write_article(db, row, &code, &w).await? {
            Err(reason) => done("failed", reason),
            Ok((status, article_id)) => match write_source(db, article_id, &w).await {
                Ok(()) => done(status, String::new()),
                Err(e) => done(
                    "failed",
                    format!(
                        "Article enregistré, mais le fournisseur principal n'a pas été lié : {}",
                        app_error_text(&e)
                    ),
                ),
            },
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let (map, _) = current(db).await?;
    let mut list: Vec<&Current> = map.values().collect();
    list.sort_by(|a, b| a.article.article_code.cmp(&b.article.article_code));
    let num = |v: f64| {
        if v.fract() == 0.0 {
            format!("{v:.0}")
        } else {
            v.to_string()
        }
    };
    Ok(list
        .into_iter()
        .map(|c| {
            let a = &c.article;
            vec![
                a.article_code.clone(),
                a.article_name.clone(),
                a.unit_code.clone(),
                a.family_code.clone().unwrap_or_default(),
                a.manufacturer_part_number.clone().unwrap_or_default(),
                a.manufacturer_name.clone().unwrap_or_default(),
                c.preferred.as_ref().map(|p| p.1.clone()).unwrap_or_default(),
                c.preferred.as_ref().and_then(|p| p.2.clone()).unwrap_or_default(),
                num(a.min_stock),
                num(a.reorder_point),
                a.max_stock.map(num).unwrap_or_default(),
                a.economic_order_qty.map(num).unwrap_or_default(),
                a.criticality_code.clone().unwrap_or_default(),
                a.stocking_type_code.clone(),
                a.tax_category_code.clone(),
                if a.is_active != 0 { "oui" } else { "non" }.to_string(),
            ]
        })
        .collect())
}
