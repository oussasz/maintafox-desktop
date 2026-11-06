//! Supplier import (fournisseurs), keyed by supplier code.
//!
//! It does what the supplier form does: create a supplier with its status, rename it, change its
//! status or usual lead time, activate or deactivate it. A blank cell never clears a value. The
//! fields the form also holds (payment terms, currency, incoterms, buyer) are kept as they are.
//! Tax ids, address, phone and e-mail have no supplier field, so they are not imported (Advisor
//! 2026-11-06, stock §5 Q2).

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::{key, parse_active};
use super::{normalize_header, Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{InventorySupplier, InventorySupplierInput};
use crate::inventory::suppliers;

pub const SPEC: EntitySpec = EntitySpec {
    code: "stock_supplier",
    label: "Fournisseurs",
    order: 5,
    needs: "Aucune.",
    permission: crate::rbac::permissions::INV_MANAGE,
    columns: &[
        Column {
            key: "code",
            header: "code fournisseur",
            required: true,
            aliases: &["code", "fournisseur", "code_fournisseur", "supplier_code"],
            hint: "Code unique du fournisseur, par exemple FRN-001.",
        },
        Column {
            key: "name",
            header: "raison sociale",
            required: true,
            aliases: &["nom", "libelle", "raison_sociale", "name", "societe"],
            hint: "Nom du fournisseur. Obligatoire pour un nouveau fournisseur.",
        },
        Column {
            key: "status",
            header: "statut",
            required: false,
            aliases: &["status", "statut_fournisseur"],
            hint: "préféré, approuvé, bloqué ou en évaluation. Obligatoire pour un nouveau fournisseur.",
        },
        Column {
            key: "lead_time",
            header: "délai habituel (jours)",
            required: false,
            aliases: &[
                "delai",
                "delai_habituel",
                "delai_jours",
                "lead_time",
                "delai_de_livraison",
            ],
            hint: "Nombre entier de jours, 0 ou plus.",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: "oui ou non. Vide : un nouveau fournisseur est actif, un fournisseur existant ne change pas.",
        },
    ],
};

/// Status code from a code or a French label (case, accents and spaces ignored).
fn parse_status(raw: &str) -> Option<&'static str> {
    match normalize_header(raw).as_str() {
        "preferred" | "prefere" => Some("PREFERRED"),
        "approved" | "approuve" => Some("APPROVED"),
        "blocked" | "bloque" => Some("BLOCKED"),
        "under_evaluation" | "en_evaluation" | "evaluation" => Some("UNDER_EVALUATION"),
        _ => None,
    }
}

fn status_label(code: &str) -> &'static str {
    match code {
        "PREFERRED" => "préféré",
        "APPROVED" => "approuvé",
        "BLOCKED" => "bloqué",
        "UNDER_EVALUATION" => "en évaluation",
        _ => "",
    }
}

/// A whole number of days, 0 or more ("5" or "5,0").
fn parse_days(raw: &str) -> Option<i64> {
    let v: f64 = raw.trim().replace(',', ".").parse().ok()?;
    (v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= 3650.0).then_some(v as i64)
}

/// Every supplier by code key. Codes that differ only by letter case land in `ambiguous`.
async fn all_suppliers(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, InventorySupplier>, HashSet<String>)> {
    let mut map = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, external_company_id, status_code, payment_terms_code, \
                    currency_value_id, currency_code, incoterms_code, default_lead_time_days, \
                    default_buyer_person_id, is_active, row_version, created_at, updated_at \
             FROM inventory_suppliers"
                .to_string(),
        ))
        .await?
    {
        let s = InventorySupplier {
            id: r.try_get("", "id")?,
            code: r.try_get("", "code")?,
            name: r.try_get("", "name")?,
            external_company_id: r.try_get("", "external_company_id")?,
            status_code: r.try_get("", "status_code")?,
            payment_terms_code: r.try_get("", "payment_terms_code")?,
            currency_value_id: r.try_get("", "currency_value_id")?,
            currency_code: r.try_get("", "currency_code").ok().flatten(),
            incoterms_code: r.try_get("", "incoterms_code")?,
            default_lead_time_days: r.try_get("", "default_lead_time_days")?,
            default_buyer_person_id: r.try_get("", "default_buyer_person_id")?,
            is_active: r.try_get("", "is_active")?,
            row_version: r.try_get("", "row_version")?,
            created_at: r.try_get("", "created_at")?,
            updated_at: r.try_get("", "updated_at")?,
        };
        let k = key(&s.code);
        if map.contains_key(&k) {
            ambiguous.insert(k.clone());
        }
        map.insert(k, s);
    }
    Ok((map, ambiguous))
}

fn ambiguous_text(code: &str) -> String {
    format!("Plusieurs fournisseurs ont le code « {code} » à la casse près : renommez-les dans Stock avant d'importer.")
}

/// The values a line sets: `None` leaves the current value.
struct Wanted {
    name: Option<String>,
    status: Option<&'static str>,
    lead_time: Option<i64>,
    active: Option<bool>,
}

impl Wanted {
    fn from_normalized(v: &serde_json::Value) -> Self {
        Self {
            name: v["name"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            status: v["status"].as_str().and_then(parse_status),
            lead_time: v["lead_time"].as_i64(),
            active: v["active"].as_bool(),
        }
    }

    fn changes(&self, s: &InventorySupplier) -> bool {
        self.name.as_ref().is_some_and(|n| *n != s.name)
            || self.status.is_some_and(|st| st != s.status_code)
            || self.lead_time.is_some_and(|d| Some(d) != s.default_lead_time_days)
            || self.active.is_some_and(|a| a != (s.is_active != 0))
    }
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let (existing, ambiguous) = all_suppliers(db).await?;
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let get = |f: &str| raw.get(f).map(|s| s.trim().to_string()).unwrap_or_default();
        let code = get("code");
        let name = get("name");
        let mut first = false;
        if code.is_empty() {
            messages.push(RowMessage::error(
                "SUPPLIER_IMPORT_CODE",
                "Le code fournisseur est obligatoire.",
            ));
        } else if let Some(f) = first_row_of.get(&key(&code)) {
            messages.push(RowMessage::error(
                "SUPPLIER_IMPORT_DUPLICATE",
                format!(
                    "Le fournisseur « {code} » apparaît déjà à la ligne {} du fichier.",
                    f + 2
                ),
            ));
        } else {
            first_row_of.insert(key(&code), i);
            first = true;
        }
        let status_raw = get("status");
        let status = if status_raw.is_empty() {
            None
        } else if let Some(s) = parse_status(&status_raw) {
            Some(s)
        } else {
            messages.push(RowMessage::error(
                "SUPPLIER_IMPORT_STATUS",
                format!("Statut « {status_raw} » inconnu : mettez préféré, approuvé, bloqué ou en évaluation."),
            ));
            None
        };
        let lead_raw = get("lead_time");
        let lead_time = if lead_raw.is_empty() {
            None
        } else if let Some(d) = parse_days(&lead_raw) {
            Some(d)
        } else {
            messages.push(RowMessage::error(
                "SUPPLIER_IMPORT_LEAD_TIME",
                format!("Délai « {lead_raw} » invalide : un nombre entier de jours, 0 ou plus."),
            ));
            None
        };
        let active = parse_active(raw.get("active")).unwrap_or_else(|()| {
            messages.push(RowMessage::error(
                "SUPPLIER_IMPORT_ACTIVE",
                "La colonne « actif » doit contenir oui ou non.",
            ));
            None
        });
        let found = existing.get(&key(&code));
        if first && ambiguous.contains(&key(&code)) {
            messages.push(RowMessage::error(
                "SUPPLIER_IMPORT_CODE_AMBIGUOUS",
                ambiguous_text(&code),
            ));
        }
        if first && found.is_none() {
            if name.is_empty() {
                messages.push(RowMessage::error(
                    "SUPPLIER_IMPORT_NAME",
                    "La raison sociale est obligatoire pour un nouveau fournisseur.",
                ));
            }
            if status_raw.is_empty() {
                messages.push(RowMessage::error(
                    "SUPPLIER_IMPORT_STATUS",
                    "Le statut est obligatoire pour un nouveau fournisseur (préféré, approuvé, bloqué ou en évaluation).",
                ));
            }
        }
        let normalized = json!({
            "code": code,
            "name": name,
            "status": status,
            "lead_time": lead_time,
            "active": active,
        });
        let wanted = Wanted::from_normalized(&normalized);
        let action = if messages.iter().any(|m| m.severity == "error") || !first {
            "skip"
        } else {
            match found {
                None => "create",
                Some(s) if wanted.changes(s) => "update",
                Some(_) => "skip",
            }
        };
        out.push(RowOutcome {
            action,
            normalized,
            messages,
        });
    }
    Ok(out)
}

fn app_error_text(e: &AppError) -> String {
    match e {
        AppError::ValidationFailed(v) => v.join(" "),
        other => other.to_string(),
    }
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
        let wanted = Wanted::from_normalized(&row.normalized);
        // Read again at apply time: a supplier created, edited or duplicated elsewhere since the
        // preview fails this line only (or, if nothing is left to change, is skipped).
        let (now, ambiguous) = all_suppliers(db).await?;
        if ambiguous.contains(&key(&code)) {
            out.push(done("failed", ambiguous_text(&code)));
            continue;
        }
        let result = match (row.action.as_str(), now.get(&key(&code))) {
            ("create", None) => {
                let (Some(name), Some(status)) = (wanted.name.clone(), wanted.status) else {
                    out.push(done("failed", "Raison sociale ou statut manquant.".to_string()));
                    continue;
                };
                suppliers::upsert_supplier(
                    db,
                    None,
                    None,
                    InventorySupplierInput {
                        code,
                        name,
                        external_company_id: None,
                        status_code: status.to_string(),
                        payment_terms_code: None,
                        currency_value_id: None,
                        currency_code: None,
                        incoterms_code: None,
                        default_lead_time_days: wanted.lead_time,
                        default_buyer_person_id: None,
                        is_active: Some(wanted.active.unwrap_or(true)),
                    },
                )
                .await
                .map(|_| "created")
            }
            ("create", Some(_)) => {
                out.push(done(
                    "failed",
                    format!("Le fournisseur « {code} » a été créé entre-temps : relancez l'import."),
                ));
                continue;
            }
            (_, None) => {
                out.push(done("failed", format!("Le fournisseur « {code} » n'existe plus.")));
                continue;
            }
            (_, Some(s)) => {
                if !wanted.changes(s) {
                    out.push(done("skipped", "Déjà à jour.".to_string()));
                    continue;
                }
                // The form rewrites every field: the ones the file does not carry keep their value.
                suppliers::upsert_supplier(
                    db,
                    Some(s.id),
                    Some(s.row_version),
                    InventorySupplierInput {
                        code: s.code.clone(),
                        name: wanted.name.clone().unwrap_or_else(|| s.name.clone()),
                        external_company_id: s.external_company_id,
                        status_code: wanted.status.map_or_else(|| s.status_code.clone(), str::to_string),
                        payment_terms_code: s.payment_terms_code.clone(),
                        currency_value_id: s.currency_value_id,
                        currency_code: s.currency_code.clone(),
                        incoterms_code: s.incoterms_code.clone(),
                        default_lead_time_days: wanted.lead_time.or(s.default_lead_time_days),
                        default_buyer_person_id: s.default_buyer_person_id,
                        is_active: Some(wanted.active.unwrap_or(s.is_active != 0)),
                    },
                )
                .await
                .map(|_| "updated")
            }
        };
        out.push(match result {
            Ok(status) => done(status, String::new()),
            Err(e) => done("failed", app_error_text(&e)),
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let (map, _) = all_suppliers(db).await?;
    let mut list: Vec<&InventorySupplier> = map.values().collect();
    list.sort_by(|a, b| a.code.cmp(&b.code));
    Ok(list
        .into_iter()
        .map(|s| {
            vec![
                s.code.clone(),
                s.name.clone(),
                status_label(&s.status_code).to_string(),
                s.default_lead_time_days.map(|d| d.to_string()).unwrap_or_default(),
                if s.is_active != 0 { "oui" } else { "non" }.to_string(),
            ]
        })
        .collect())
}
