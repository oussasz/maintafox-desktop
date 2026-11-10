//! Spare parts of equipment (pièces de rechange: article ↔ équipement), keyed by equipment code
//! plus article code.
//!
//! It does what the spare-parts list of the equipment sheet does (Advisor ruling 2026-11-06,
//! stock §5 Q5), under the same "Gérer les équipements" right:
//! - a new pair adds the article to the equipment's list (the article must be active);
//! - an existing pair gets its filled cells (quantity, position, critical spare, notes) and keeps
//!   the others;
//! - a pair is never removed: removing a part stays a choice made on the equipment sheet.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::equipment_meter::{equipment_by_code, parse_number};
use super::org_tree::{key, parse_active};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::assets::spare_parts::{
    add_asset_spare_part, update_asset_spare_part, AddSparePartPayload, SparePartInput, NOTES_MAX_CHARS,
    POSITION_LABEL_MAX_CHARS,
};
use crate::errors::{AppError, AppResult};

pub const SPEC: EntitySpec = EntitySpec {
    code: "equipment_spare_part",
    label: "Pièces de rechange des équipements",
    order: 6,
    needs: "Équipements et articles du stock.",
    permission: crate::rbac::permissions::EQ_MANAGE,
    columns: &[
        Column {
            key: "equipment_code",
            header: "code équipement",
            required: true,
            aliases: &[
                "equipement",
                "code_equipement",
                "equipment_code",
                "asset_code",
                "code_actif",
            ],
            hint: "Code de l'équipement.",
        },
        Column {
            key: "article_code",
            header: "code article",
            required: true,
            aliases: &["article", "code_article", "article_code", "reference"],
            hint: "Code de l'article du stock.",
        },
        Column {
            key: "quantity",
            header: "quantité",
            required: false,
            aliases: &["quantite", "qte", "quantity", "quantite_installee"],
            hint: "Quantité montée sur l'équipement, plus grande que 0. Obligatoire pour une nouvelle pièce.",
        },
        Column {
            key: "position",
            header: "position",
            required: false,
            aliases: &["emplacement_sur_equipement", "repere", "position_label"],
            hint: "Où se trouve la pièce, par exemple « roulement côté accouplement ».",
        },
        Column {
            key: "critical",
            header: "pièce critique",
            required: false,
            aliases: &["piece_critique", "critique", "critical", "is_critical_spare"],
            hint: "oui ou non. Vide : non pour une nouvelle pièce, inchangé pour une pièce existante.",
        },
        Column {
            key: "notes",
            header: "remarques",
            required: false,
            aliases: &["remarque", "notes", "commentaire"],
            hint: "Texte libre.",
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
}

/// Every article by code key. Codes that differ only by letter case land in `ambiguous`.
async fn articles(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, Article>, HashSet<String>)> {
    let mut map: HashMap<String, Article> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, article_code, is_active FROM articles".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "article_code")?;
        if map.contains_key(&key(&code)) {
            ambiguous.insert(key(&code));
        }
        map.insert(
            key(&code),
            Article {
                id: r.try_get("", "id")?,
                code,
                active: r.try_get::<i64>("", "is_active")? != 0,
            },
        );
    }
    Ok((map, ambiguous))
}

struct Line {
    id: i64,
    quantity: f64,
    position: Option<String>,
    critical: bool,
    notes: Option<String>,
    row_version: i64,
}

/// The active spare-parts lines by (equipment id, article id).
async fn lines(db: &impl ConnectionTrait) -> AppResult<HashMap<(i64, i64), Line>> {
    let mut out = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, equipment_id, article_id, quantity, position_label, is_critical_spare, notes, row_version \
             FROM equipment_spare_parts WHERE is_active = 1"
                .to_string(),
        ))
        .await?
    {
        out.insert(
            (r.try_get("", "equipment_id")?, r.try_get("", "article_id")?),
            Line {
                id: r.try_get("", "id")?,
                quantity: r.try_get("", "quantity")?,
                position: r.try_get("", "position_label")?,
                critical: r.try_get::<i64>("", "is_critical_spare")? != 0,
                notes: r.try_get("", "notes")?,
                row_version: r.try_get("", "row_version")?,
            },
        );
    }
    Ok(out)
}

/// The values the line will have once applied: the filled cells over the current line.
fn merged(row: &serde_json::Value, current: Option<&Line>) -> Option<SparePartInput> {
    let text = |f: &str| row[f].as_str().map(str::to_string).filter(|s| !s.is_empty());
    let quantity = row["quantity"].as_f64().or(current.map(|l| l.quantity))?;
    Some(SparePartInput {
        quantity,
        position_label: text("position").or_else(|| current.and_then(|l| l.position.clone())),
        is_critical_spare: row["critical"]
            .as_bool()
            .or(current.map(|l| l.critical))
            .unwrap_or(false),
        notes: text("notes").or_else(|| current.and_then(|l| l.notes.clone())),
    })
}

fn changes(input: &SparePartInput, line: &Line) -> bool {
    input.quantity != line.quantity
        || input.position_label != line.position
        || input.is_critical_spare != line.critical
        || input.notes != line.notes
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let (equipment, equipment_ambiguous) = equipment_by_code(db).await?;
    let (article_map, article_ambiguous) = articles(db).await?;
    let existing = lines(db).await?;
    let mut first_row_of: HashMap<(String, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let equipment_code = cell(raw, "equipment_code");
        let article_code = cell(raw, "article_code");
        if equipment_code.is_empty() {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_EQUIPMENT",
                "Le code équipement est obligatoire.",
            ));
        }
        if article_code.is_empty() {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_ARTICLE",
                "Le code article est obligatoire.",
            ));
        }
        let mut first = false;
        if !has_error(&messages) {
            let k = (key(&equipment_code), key(&article_code));
            if let Some(f) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "SPARE_IMPORT_DUPLICATE",
                    format!(
                        "L'article « {article_code} » de l'équipement « {equipment_code} » apparaît déjà à la ligne {} du fichier.",
                        f + 2
                    ),
                ));
            } else {
                first_row_of.insert(k, i);
                first = true;
            }
        }

        let quantity_cell = cell(raw, "quantity");
        let quantity = if quantity_cell.is_empty() {
            None
        } else {
            match parse_number(&quantity_cell) {
                Some(q) if q > 0.0 => Some(q),
                _ => {
                    messages.push(RowMessage::error(
                        "SPARE_IMPORT_QUANTITY",
                        "La quantité doit être un nombre plus grand que 0.",
                    ));
                    None
                }
            }
        };
        let position = cell(raw, "position");
        if position.chars().count() > POSITION_LABEL_MAX_CHARS {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_POSITION",
                format!("La position est limitée à {POSITION_LABEL_MAX_CHARS} caractères."),
            ));
        }
        let notes = cell(raw, "notes");
        if notes.chars().count() > NOTES_MAX_CHARS {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_NOTES",
                format!("Les remarques sont limitées à {NOTES_MAX_CHARS} caractères."),
            ));
        }
        let critical = parse_active(raw.get("critical")).unwrap_or_else(|()| {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_CRITICAL",
                "La colonne « pièce critique » doit contenir oui ou non.",
            ));
            None
        });

        let equipment_id = if equipment_code.is_empty() {
            None
        } else if equipment_ambiguous.contains(&key(&equipment_code)) {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_EQUIPMENT_AMBIGUOUS",
                format!(
                    "Plusieurs équipements ont le code « {equipment_code} » à la casse près : renommez-les avant d'importer."
                ),
            ));
            None
        } else if let Some(id) = equipment.get(&key(&equipment_code)) {
            Some(*id)
        } else {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_EQUIPMENT_UNKNOWN",
                format!("L'équipement « {equipment_code} » n'existe pas : importez d'abord les équipements."),
            ));
            None
        };
        let article = if article_code.is_empty() {
            None
        } else if article_ambiguous.contains(&key(&article_code)) {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_ARTICLE_AMBIGUOUS",
                format!(
                    "Plusieurs articles ont le code « {article_code} » à la casse près : renommez-les dans Stock avant d'importer."
                ),
            ));
            None
        } else if let Some(a) = article_map.get(&key(&article_code)) {
            Some(a)
        } else {
            messages.push(RowMessage::error(
                "SPARE_IMPORT_ARTICLE_UNKNOWN",
                format!("L'article « {article_code} » n'existe pas : importez d'abord les articles."),
            ));
            None
        };

        let normalized = json!({
            "equipment_code": equipment_code,
            "article_code": article_code,
            "quantity": quantity,
            "position": position,
            "critical": critical,
            "notes": notes,
        });
        let mut action = "skip";
        if let (Some(equipment_id), Some(article), true) = (equipment_id, article, first) {
            match existing.get(&(equipment_id, article.id)) {
                Some(line) => {
                    if let Some(input) = merged(&normalized, Some(line)) {
                        if changes(&input, line) {
                            action = "update";
                        }
                    }
                }
                None => {
                    if quantity.is_none() && !has_error(&messages) {
                        messages.push(RowMessage::error(
                            "SPARE_IMPORT_QUANTITY_REQUIRED",
                            "La quantité est obligatoire pour une nouvelle pièce de l'équipement.",
                        ));
                    }
                    if !article.active {
                        messages.push(RowMessage::error(
                            "SPARE_IMPORT_ARTICLE_INACTIVE",
                            format!(
                                "L'article « {} » est inactif : réactivez-le avant de l'ajouter à un équipement.",
                                article.code
                            ),
                        ));
                    }
                    action = "create";
                }
            }
        }
        if has_error(&messages) {
            action = "skip";
        }
        out.push(RowOutcome {
            action,
            normalized,
            messages,
        });
    }
    Ok(out)
}

/// A refusal of the spare-parts list, in plain French for the import report.
fn app_error_text(e: &AppError) -> String {
    let codes: Vec<String> = match e {
        AppError::OrgValidationFailed(issues) => issues.iter().map(|i| i.code.clone()).collect(),
        AppError::NotFound { .. } => vec!["NOT_FOUND".to_string()],
        other => return format!("Enregistrement impossible : {other}"),
    };
    codes
        .iter()
        .map(|code| match code.as_str() {
            "ASSET_SPARE_DUPLICATE" => {
                "Cet article a été ajouté à l'équipement entre-temps : relancez l'import.".to_string()
            }
            "ASSET_SPARE_ARTICLE_INACTIVE" => "L'article est devenu inactif.".to_string(),
            "ASSET_STALE_VERSION" => "La pièce a été modifiée ailleurs entre-temps : relancez l'import.".to_string(),
            "NOT_FOUND" => "L'équipement, l'article ou la pièce n'existe plus.".to_string(),
            other => format!("Enregistrement refusé par la liste des pièces ({other})."),
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
    let (equipment, equipment_ambiguous) = equipment_by_code(db).await?;
    let (article_map, article_ambiguous) = articles(db).await?;
    let mut out = Vec::new();
    for row in rows {
        if row.action != "create" && row.action != "update" {
            out.push(RowApplied {
                row_id: row.id,
                status: "skipped",
                message: "Déjà à jour.".to_string(),
            });
            continue;
        }
        let text = |f: &str| row.normalized[f].as_str().unwrap_or_default().to_string();
        let (equipment_code, article_code) = (text("equipment_code"), text("article_code"));
        let (Some(equipment_id), Some(article)) = (
            equipment
                .get(&key(&equipment_code))
                .filter(|_| !equipment_ambiguous.contains(&key(&equipment_code))),
            article_map
                .get(&key(&article_code))
                .filter(|_| !article_ambiguous.contains(&key(&article_code))),
        ) else {
            out.push(fail(
                row,
                "L'équipement ou l'article n'existe plus, ou son code est devenu ambigu.",
            ));
            continue;
        };
        // Read the line again: a change made on the equipment sheet since the preview is kept,
        // and only the filled cells of the file are written over it.
        let current = lines(db).await?;
        let line = current.get(&(*equipment_id, article.id));
        let Some(input) = merged(&row.normalized, line) else {
            out.push(fail(
                row,
                "La quantité est obligatoire pour une nouvelle pièce de l'équipement.",
            ));
            continue;
        };
        let result = match line {
            None => add_asset_spare_part(
                db,
                AddSparePartPayload {
                    asset_id: *equipment_id,
                    article_id: article.id,
                    line: input,
                },
                i64::from(user_id),
            )
            .await
            .map(|_| "created"),
            Some(l) if !changes(&input, l) => Ok("skipped"),
            Some(l) => update_asset_spare_part(db, l.id, input, l.row_version)
                .await
                .map(|_| "updated"),
        };
        out.push(match result {
            Ok(status) => RowApplied {
                row_id: row.id,
                status,
                message: if status == "skipped" {
                    "Déjà à jour.".to_string()
                } else {
                    String::new()
                },
            },
            Err(e) => fail(row, app_error_text(&e)),
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT e.asset_id_code, a.article_code, sp.quantity, sp.position_label, sp.is_critical_spare, sp.notes \
             FROM equipment_spare_parts sp \
             JOIN equipment e ON e.id = sp.equipment_id \
             JOIN articles a ON a.id = sp.article_id \
             WHERE sp.is_active = 1 AND e.deleted_at IS NULL AND e.asset_id_code IS NOT NULL \
             ORDER BY e.asset_id_code, a.article_code"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let qty: f64 = r.try_get("", "quantity")?;
            let critical: i64 = r.try_get("", "is_critical_spare")?;
            Ok(vec![
                r.try_get("", "asset_id_code")?,
                r.try_get("", "article_code")?,
                qty.to_string(),
                r.try_get::<Option<String>>("", "position_label")?.unwrap_or_default(),
                if critical != 0 { "oui" } else { "non" }.to_string(),
                r.try_get::<Option<String>>("", "notes")?.unwrap_or_default(),
            ])
        })
        .collect()
}
