//! Crew (équipe) import, keyed by crew code.
//!
//! A crew that already exists is updated (name, site, type, active flag); an empty cell never
//! clears a value. Nothing is ever deleted. The crew type is one of the fixed crew types of the
//! personnel screen, given by its code or its French label. Members are a separate import
//! (`team_member`), loaded after the crews.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::{key, parse_active};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::repository::team_repository::{self, TeamCreateInput, TeamUpdateInput};

pub const SPEC: EntitySpec = EntitySpec {
    code: "team",
    label: "Équipes",
    order: 3,
    needs: "Organisation (les codes site).",
    permission: crate::rbac::permissions::PER_MANAGE,
    columns: &[
        Column {
            key: "code",
            header: "code équipe",
            required: true,
            aliases: &["code", "equipe", "code_equipe", "team_code"],
            hint: "Code unique de l'équipe, par exemple EQ-MECA-NORD.",
        },
        Column {
            key: "name",
            header: "libellé",
            required: true,
            aliases: &["libelle", "nom", "designation", "name"],
            hint: "Nom affiché de l'équipe.",
        },
        Column {
            key: "site_code",
            header: "code site",
            required: true,
            aliases: &["site", "code_entite", "entite", "site_code"],
            hint: "Code d'un nœud actif de l'organisation (site, zone…). Obligatoire pour une nouvelle équipe.",
        },
        Column {
            key: "team_type",
            header: "type d'équipe",
            required: true,
            aliases: &["type", "type_equipe", "team_type"],
            hint: "maintenance, inspection, planning, sous-traitant (contractor) ou HSE. Obligatoire pour une nouvelle équipe.",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: "oui ou non. Vide : une nouvelle équipe est active, une équipe existante ne change pas.",
        },
    ],
};

/// Crew type code from the cell: the code itself or the French label of the personnel screen.
fn team_type(raw: &str) -> Option<&'static str> {
    match key(raw).as_str() {
        "maintenance" => Some("maintenance"),
        "inspection" => Some("inspection"),
        "planning" | "planification" => Some("planning"),
        "contractor" | "sous-traitant" | "sous traitant" | "soustraitant" | "prestataire" => Some("contractor"),
        "hse" => Some("hse"),
        _ => None,
    }
}

struct Existing {
    id: i64,
    name: String,
    node_id: Option<i64>,
    team_type: String,
    status: String,
}

struct Line {
    code: String,
    name: String,
    node_id: Option<i64>,
    team_type: Option<&'static str>,
    active: Option<bool>,
    messages: Vec<RowMessage>,
}

fn has_error(line: &Line) -> bool {
    line.messages.iter().any(|m| m.severity == "error")
}

/// The status the line writes: `None` when the active flag is blank or already right.
fn new_status(line: &Line, current: Option<&str>) -> Option<&'static str> {
    match (line.active, current) {
        (None, _) => None,
        (Some(true), Some("active")) | (Some(false), Some("inactive" | "disbanded")) => None,
        (Some(true), _) => Some("active"),
        (Some(false), _) => Some("inactive"),
    }
}

fn changes(line: &Line, e: &Existing) -> bool {
    (!line.name.is_empty() && line.name != e.name)
        || line.node_id.is_some_and(|id| Some(id) != e.node_id)
        || line.team_type.is_some_and(|t| t != e.team_type)
        || new_status(line, Some(&e.status)).is_some()
}

pub(super) fn app_error_text(e: &AppError) -> String {
    match e {
        AppError::ValidationFailed(v) => v.join(" "),
        other => other.to_string(),
    }
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    // Live crews by code; two codes that differ only by letter case cannot be told apart by the
    // file. A deleted crew still holds its code (the code is unique), so it cannot be reused.
    let mut existing: HashMap<String, Existing> = HashMap::new();
    let mut ambiguous: HashSet<String> = HashSet::new();
    let mut deleted: HashSet<String> = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, team_type, status, primary_node_id, deleted_at FROM teams".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let k = key(&code);
        if r.try_get::<Option<String>>("", "deleted_at")?.is_some() {
            deleted.insert(k);
            continue;
        }
        if existing.contains_key(&k) {
            ambiguous.insert(k.clone());
        }
        existing.insert(
            k,
            Existing {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                node_id: r.try_get("", "primary_node_id")?,
                team_type: r.try_get("", "team_type")?,
                status: r.try_get("", "status")?,
            },
        );
    }

    // Organisation nodes of the active model, by code; whether each one can hold a crew is asked
    // to the organisation rules themselves (the same check the crew screen runs).
    let model_id = crate::org::model_scope::get_active_model_id(db).await?;
    let mut sites: HashMap<String, i64> = HashMap::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code FROM org_nodes WHERE structure_model_id = ? AND deleted_at IS NULL",
            [model_id.into()],
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        sites.entry(key(&code)).or_insert(r.try_get("", "id")?);
    }
    let mut node_refusal: HashMap<i64, Option<String>> = HashMap::new();

    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let code = raw.get("code").map(|s| s.trim().to_string()).unwrap_or_default();
        let name = raw.get("name").map(|s| s.trim().to_string()).unwrap_or_default();
        let mut is_first = false;
        if code.is_empty() {
            messages.push(RowMessage::error(
                "TEAM_IMPORT_CODE",
                "Le code de l'équipe est obligatoire.",
            ));
        } else if let Some(first) = first_row_of.get(&key(&code)) {
            messages.push(RowMessage::error(
                "TEAM_IMPORT_DUPLICATE",
                format!("Le code « {code} » apparaît déjà à la ligne {} du fichier.", first + 2),
            ));
        } else {
            first_row_of.insert(key(&code), i);
            is_first = true;
        }
        let me = key(&code);
        let current = existing.get(&me);
        if is_first && ambiguous.contains(&me) {
            messages.push(RowMessage::error(
                "TEAM_IMPORT_CODE_AMBIGUOUS",
                format!(
                    "Plusieurs équipes ont le code « {code} » à la casse près : renommez-les dans Personnel avant d'importer."
                ),
            ));
        }
        if is_first && current.is_none() && deleted.contains(&me) {
            messages.push(RowMessage::error(
                "TEAM_IMPORT_CODE_DELETED",
                format!("Le code « {code} » appartient à une équipe supprimée : choisissez un autre code."),
            ));
        }

        let node_id = match raw.get("site_code").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            None => {
                if current.is_none() {
                    messages.push(RowMessage::error(
                        "TEAM_IMPORT_SITE",
                        "Le code site est obligatoire pour une nouvelle équipe.",
                    ));
                }
                None
            }
            Some(s) => {
                match sites.get(&key(s)) {
                    None => {
                        messages.push(RowMessage::error(
                        "TEAM_IMPORT_SITE",
                        format!("Le code site « {s} » n'existe pas dans l'organisation : importez d'abord l'organisation."),
                    ));
                        None
                    }
                    Some(id) => Some(*id),
                }
            }
        };
        // A site that does not change is not re-checked (the crew keeps it, as on the screen).
        if let Some(id) = node_id.filter(|id| current.is_none_or(|e| e.node_id != Some(*id))) {
            if let std::collections::hash_map::Entry::Vacant(slot) = node_refusal.entry(id) {
                let refusal = match crate::org::model_scope::assert_org_node_active(db, id).await {
                    Ok(()) => None,
                    Err(AppError::Database(e)) => return Err(AppError::Database(e)),
                    Err(e) => Some(app_error_text(&e)),
                };
                slot.insert(refusal);
            }
            if let Some(Some(reason)) = node_refusal.get(&id) {
                messages.push(RowMessage::error(
                    "TEAM_IMPORT_SITE_INACTIVE",
                    format!("Ce nœud de l'organisation ne peut pas recevoir d'équipe : {reason}"),
                ));
            }
        }

        let team_type = match raw.get("team_type").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            None => {
                if current.is_none() {
                    messages.push(RowMessage::error(
                        "TEAM_IMPORT_TYPE",
                        "Le type d'équipe est obligatoire pour une nouvelle équipe.",
                    ));
                }
                None
            }
            Some(t) => {
                let found = team_type(t);
                if found.is_none() {
                    messages.push(RowMessage::error(
                        "TEAM_IMPORT_TYPE",
                        format!(
                            "Le type d'équipe « {t} » n'existe pas. Valeurs admises : maintenance, inspection, planning, sous-traitant, HSE."
                        ),
                    ));
                }
                found
            }
        };
        if current.is_none() && name.is_empty() {
            messages.push(RowMessage::error("TEAM_IMPORT_NAME", "Le libellé est obligatoire."));
        }
        let active = match parse_active(raw.get("active")) {
            Ok(a) => a,
            Err(()) => {
                messages.push(RowMessage::error(
                    "TEAM_IMPORT_ACTIVE",
                    "La colonne « actif » doit contenir oui ou non.",
                ));
                None
            }
        };
        let mut line = Line {
            code,
            name,
            node_id,
            team_type,
            active,
            messages,
        };

        // Moving a crew to another site is refused while it has committed jobs in a frozen week
        // (the same rule as the crew screen, which apply runs again).
        if let (Some(e), Some(new_node)) = (current, line.node_id) {
            if is_first && !has_error(&line) && e.node_id != Some(new_node) {
                if let Some(block) =
                    crate::planning::crew_site::crew_site_change_block(db, e.id, Some(new_node), crate::clock::now())
                        .await?
                {
                    line.messages.push(RowMessage::error(
                        "TEAM_IMPORT_SITE_FROZEN",
                        format!(
                            "L'équipe ne peut pas changer de site : {} travail(s) engagé(s) dans la ou les semaines gelées {}.",
                            block.jobs,
                            block.weeks.join(", ")
                        ),
                    ));
                }
            }
        }
        lines.push(line);
    }

    Ok(lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let me = key(&l.code);
            let is_first = !l.code.is_empty() && first_row_of.get(&me) == Some(&i);
            let action = if has_error(&l) || !is_first {
                "skip"
            } else if let Some(e) = existing.get(&me) {
                if changes(&l, e) {
                    "update"
                } else {
                    "skip"
                }
            } else {
                "create"
            };
            let status = new_status(&l, existing.get(&me).map(|e| e.status.as_str()));
            RowOutcome {
                action,
                normalized: json!({
                    "code": l.code,
                    "name": l.name,
                    "node_id": l.node_id,
                    "team_type": l.team_type,
                    "status": status,
                }),
                messages: l.messages,
            }
        })
        .collect())
}

/// Id of the live crew with this code, matched the same way as the preview. `Err` when several
/// crews match or none does.
pub(super) async fn team_by_code(db: &impl ConnectionTrait, code: &str) -> AppResult<Result<i64, String>> {
    let wanted = key(code);
    let mut found = None;
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code FROM teams WHERE deleted_at IS NULL".to_string(),
        ))
        .await?
    {
        let c: String = r.try_get("", "code")?;
        if key(&c) == wanted {
            if found.is_some() {
                return Ok(Err(format!(
                    "Plusieurs équipes ont le code « {code} » à la casse près : renommez-les dans Personnel avant d'importer."
                )));
            }
            found = Some(r.try_get("", "id")?);
        }
    }
    Ok(found.ok_or_else(|| format!("L'équipe « {code} » n'existe plus.")))
}

fn text(row: &StagedRow, field: &str) -> Option<String> {
    row.normalized[field]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        let code = text(row, "code").unwrap_or_default();
        let node_id = row.normalized["node_id"].as_i64();
        let written = match row.action.as_str() {
            "create" => {
                let Some(node) = node_id else {
                    out.push(failed(row, "Le code site est obligatoire pour une nouvelle équipe."));
                    continue;
                };
                team_repository::create_team(
                    db,
                    TeamCreateInput {
                        code,
                        name: text(row, "name").unwrap_or_default(),
                        team_type: text(row, "team_type").unwrap_or_default(),
                        primary_node_id: node,
                        description: None,
                        status: text(row, "status"),
                        icon_key: None,
                        accent_color: None,
                    },
                )
                .await
                .map(|_| "created")
            }
            "update" => match team_by_code(db, &code).await? {
                Err(reason) => {
                    out.push(failed(row, reason));
                    continue;
                }
                Ok(id) => team_repository::update_team(
                    db,
                    TeamUpdateInput {
                        id,
                        code: None,
                        name: text(row, "name"),
                        team_type: text(row, "team_type"),
                        primary_node_id: node_id,
                        description: None,
                        status: text(row, "status"),
                        icon_key: None,
                        accent_color: None,
                    },
                )
                .await
                .map(|_| "updated"),
            },
            _ => Ok("skipped"),
        };
        out.push(match written {
            Ok("skipped") => RowApplied {
                row_id: row.id,
                status: "skipped",
                message: "Déjà à jour.".to_string(),
            },
            Ok(status) => RowApplied {
                row_id: row.id,
                status,
                message: String::new(),
            },
            Err(AppError::Database(e)) => return Err(AppError::Database(e)),
            Err(e) => failed(row, app_error_text(&e)),
        });
    }
    Ok(out)
}

fn failed(row: &StagedRow, message: impl Into<String>) -> RowApplied {
    RowApplied {
        row_id: row.id,
        status: "failed",
        message: message.into(),
    }
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT t.code, t.name, org.code AS site_code, t.team_type, t.status \
             FROM teams t LEFT JOIN org_nodes org ON org.id = t.primary_node_id \
             WHERE t.deleted_at IS NULL ORDER BY t.code"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let status: String = r.try_get("", "status")?;
            Ok(vec![
                r.try_get("", "code")?,
                r.try_get("", "name")?,
                r.try_get::<Option<String>>("", "site_code")?.unwrap_or_default(),
                r.try_get("", "team_type")?,
                if status == "active" { "oui" } else { "non" }.to_string(),
            ])
        })
        .collect()
}
