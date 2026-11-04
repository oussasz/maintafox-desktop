//! Organisation tree import (sites, zones, lines…), keyed by node code.
//!
//! Parents may come before or after their children in the file. A node that already exists is
//! only renamed or deactivated; moving it or changing its type is refused (the organisation
//! screen has its own checks for that). Nothing is ever deleted.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::org::audit::{self, OrgAuditEventInput};
use crate::org::nodes::{self, CreateOrgNodePayload, UpdateOrgNodeMetadataPayload};

pub const SPEC: EntitySpec = EntitySpec {
    code: "org_tree",
    label: "Organisation (sites, zones, lignes)",
    order: 1,
    needs: "Aucune. Le modèle d'organisation (types de nœuds et règles parent-enfant) doit exister.",
    permission: crate::rbac::permissions::ORG_MANAGE,
    columns: &[
        Column {
            key: "code",
            header: "code",
            required: true,
            aliases: &["code_noeud", "noeud"],
            hint: "Code unique du nœud, par exemple USINE-NORD.",
        },
        Column {
            key: "name",
            header: "libellé",
            required: true,
            aliases: &["libelle", "nom", "designation", "name"],
            hint: "Nom affiché du nœud.",
        },
        Column {
            key: "node_type",
            header: "type de nœud",
            required: true,
            aliases: &["type", "type_noeud", "node_type"],
            hint: "Code ou libellé d'un type du modèle d'organisation (site, zone, ligne…).",
        },
        Column {
            key: "parent_code",
            header: "code parent",
            required: true,
            aliases: &["parent", "code_parent", "parent_code"],
            hint: "Code du nœud parent. Vide seulement pour un site (racine).",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: "oui ou non. « non » désactive un nœud existant ; un nouveau nœud est toujours actif.",
        },
    ],
};

struct NodeType {
    id: i64,
    code: String,
}

struct ExistingNode {
    type_id: i64,
    parent_code: Option<String>,
    name: String,
    active: bool,
}

struct Line {
    code: String,
    name: String,
    type_id: Option<i64>,
    parent: Option<String>,
    active: Option<bool>,
    messages: Vec<RowMessage>,
}

fn key(s: &str) -> String {
    s.trim().to_lowercase()
}

fn parse_active(raw: Option<&String>) -> Result<Option<bool>, ()> {
    match raw.map(|s| key(s)) {
        None => Ok(None),
        Some(v) if matches!(v.as_str(), "oui" | "o" | "yes" | "y" | "1" | "true" | "vrai") => Ok(Some(true)),
        Some(v) if matches!(v.as_str(), "non" | "n" | "no" | "0" | "false" | "faux") => Ok(Some(false)),
        Some(_) => Err(()),
    }
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let model_id = crate::org::model_scope::get_active_model_id(db).await?;
    crate::org::model_scope::assert_structural_edit_allowed_for_model(db, model_id).await?;

    // Node types of the active model, found by code or by label.
    let mut types: HashMap<String, NodeType> = HashMap::new();
    let mut root_type_ids: HashSet<i64> = HashSet::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label, is_root_type FROM org_node_types WHERE structure_model_id = ? AND is_active = 1",
            [model_id.into()],
        ))
        .await?
    {
        let id: i64 = r.try_get("", "id")?;
        let code: String = r.try_get("", "code")?;
        let label: String = r.try_get("", "label")?;
        let is_root: i64 = r.try_get("", "is_root_type")?;
        if is_root != 0 {
            root_type_ids.insert(id);
        }
        for k in [key(&code), key(&label)] {
            types.entry(k).or_insert(NodeType { id, code: code.clone() });
        }
    }
    let known_types = {
        let mut known: Vec<&str> = types.values().map(|t| t.code.as_str()).collect();
        known.sort_unstable();
        known.dedup();
        known.join(", ")
    };

    let mut allowed: HashSet<(i64, i64)> = HashSet::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT parent_type_id, child_type_id FROM org_type_relationship_rules WHERE structure_model_id = ?",
            [model_id.into()],
        ))
        .await?
    {
        allowed.insert((r.try_get("", "parent_type_id")?, r.try_get("", "child_type_id")?));
    }

    let mut existing: HashMap<String, ExistingNode> = HashMap::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.code, n.name, n.node_type_id, n.status, p.code AS parent_code \
             FROM org_nodes n LEFT JOIN org_nodes p ON p.id = n.parent_id \
             WHERE n.structure_model_id = ? AND n.deleted_at IS NULL",
            [model_id.into()],
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let status: String = r.try_get("", "status")?;
        existing.insert(
            key(&code),
            ExistingNode {
                type_id: r.try_get("", "node_type_id")?,
                parent_code: r.try_get::<Option<String>>("", "parent_code")?.map(|p| key(&p)),
                name: r.try_get("", "name")?,
                active: status == "active",
            },
        );
    }

    // First pass: read each line on its own.
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let code = raw.get("code").map(|s| s.trim().to_string()).unwrap_or_default();
        let name = raw.get("name").map(|s| s.trim().to_string()).unwrap_or_default();
        if code.is_empty() {
            messages.push(RowMessage::error("ORG_IMPORT_CODE", "Le code du nœud est obligatoire."));
        } else if let Some(first) = first_row_of.get(&key(&code)) {
            messages.push(RowMessage::error(
                "ORG_IMPORT_DUPLICATE",
                format!("Le code « {code} » apparaît déjà à la ligne {} du fichier.", first + 2),
            ));
        } else {
            first_row_of.insert(key(&code), i);
        }
        if name.is_empty() {
            messages.push(RowMessage::error("ORG_IMPORT_NAME", "Le libellé est obligatoire."));
        }
        let type_id = match raw.get("node_type") {
            None => {
                messages.push(RowMessage::error("ORG_IMPORT_TYPE", "Le type de nœud est obligatoire."));
                None
            }
            Some(t) => {
                if let Some(nt) = types.get(&key(t)) {
                    Some(nt.id)
                } else {
                    messages.push(RowMessage::error(
                        "ORG_IMPORT_TYPE",
                        format!(
                            "Le type « {t} » n'existe pas dans le modèle d'organisation actif. Types connus : {known_types}."
                        ),
                    ));
                    None
                }
            }
        };
        let active = match parse_active(raw.get("active")) {
            Ok(a) => a,
            Err(()) => {
                messages.push(RowMessage::error(
                    "ORG_IMPORT_ACTIVE",
                    "La colonne « actif » doit contenir oui ou non.",
                ));
                None
            }
        };
        lines.push(Line {
            code,
            name,
            type_id,
            parent: raw.get("parent_code").map(|p| p.trim().to_string()),
            active,
            messages,
        });
    }

    // Who is the parent of each code once the file is applied? Existing nodes keep their parent.
    let mut effective_parent: HashMap<String, Option<String>> = HashMap::new();
    for (k, e) in &existing {
        effective_parent.insert(k.clone(), e.parent_code.clone());
    }
    for (i, l) in lines.iter().enumerate() {
        if l.code.is_empty() || first_row_of.get(&key(&l.code)) != Some(&i) {
            continue;
        }
        effective_parent
            .entry(key(&l.code))
            .or_insert_with(|| l.parent.as_ref().filter(|p| !p.is_empty()).map(|p| key(p)));
    }

    // Second pass: rules that need the whole file.
    for i in 0..lines.len() {
        if lines[i].code.is_empty() || first_row_of.get(&key(&lines[i].code)) != Some(&i) {
            continue;
        }
        let me = key(&lines[i].code);
        let parent = lines[i].parent.clone().filter(|p| !p.is_empty());
        let mut extra: Vec<RowMessage> = Vec::new();
        let ex = existing.get(&me);
        let my_type_is_root = lines[i].type_id.is_some_and(|t| root_type_ids.contains(&t));

        match (ex, &parent) {
            (Some(e), _) => {
                if lines[i].type_id.is_some_and(|t| t != e.type_id) {
                    extra.push(RowMessage::error(
                        "ORG_IMPORT_TYPE_CHANGE",
                        format!(
                            "Le nœud « {} » existe déjà avec un autre type : l'import ne change pas le type d'un nœud existant.",
                            lines[i].code
                        ),
                    ));
                }
                let wanted = parent.as_ref().map(|p| key(p));
                if wanted != e.parent_code {
                    extra.push(RowMessage::error(
                        "ORG_IMPORT_MOVE",
                        format!(
                            "Le nœud « {} » existe déjà sous un autre parent : le déplacement se fait depuis l'écran Organisation.",
                            lines[i].code
                        ),
                    ));
                }
            }
            (None, None) => {
                if lines[i].type_id.is_some() && !my_type_is_root {
                    extra.push(RowMessage::error(
                        "ORG_IMPORT_PARENT_REQUIRED",
                        "Seul un site (type racine) peut être créé sans parent : renseignez le code parent.",
                    ));
                }
            }
            (None, Some(p)) => {
                let pk = key(p);
                if pk == me {
                    extra.push(RowMessage::error(
                        "ORG_IMPORT_CYCLE",
                        "Un nœud ne peut pas être son propre parent.",
                    ));
                } else if !existing.contains_key(&pk) && !first_row_of.contains_key(&pk) {
                    extra.push(RowMessage::error(
                        "ORG_IMPORT_PARENT_UNKNOWN",
                        format!("Le parent « {p} » est introuvable : ni dans l'organisation, ni dans le fichier."),
                    ));
                } else {
                    // Loop through parents inside the file?
                    let mut cur = Some(pk.clone());
                    let mut steps = 0;
                    let mut in_loop = false;
                    while let Some(c) = cur {
                        if c == me {
                            in_loop = true;
                            extra.push(RowMessage::error(
                                "ORG_IMPORT_CYCLE",
                                format!("Le parent « {p} » crée une boucle dans la hiérarchie."),
                            ));
                            break;
                        }
                        steps += 1;
                        if steps > 500 {
                            break;
                        }
                        cur = effective_parent.get(&c).cloned().flatten();
                    }
                    let parent_type = existing
                        .get(&pk)
                        .map(|e| e.type_id)
                        .or_else(|| first_row_of.get(&pk).and_then(|pi| lines[*pi].type_id));
                    if my_type_is_root {
                        extra.push(RowMessage::error(
                            "ORG_IMPORT_ROOT_CHILD",
                            "Un type racine (site) ne peut pas avoir de parent.",
                        ));
                    } else if let (Some(child_t), Some(parent_t), false) = (lines[i].type_id, parent_type, in_loop) {
                        if !allowed.contains(&(parent_t, child_t)) {
                            extra.push(RowMessage::error(
                                "ORG_IMPORT_RULE",
                                format!(
                                    "Le modèle d'organisation n'autorise pas un nœud de ce type sous le parent « {p} »."
                                ),
                            ));
                        }
                    }
                }
            }
        }
        if ex.is_none() && lines[i].active == Some(false) {
            extra.push(RowMessage::error(
                "ORG_IMPORT_NEW_INACTIVE",
                "Un nouveau nœud est créé actif : importez-le d'abord, puis désactivez-le avec actif = non.",
            ));
        }
        if let (Some(e), Some(true)) = (ex, lines[i].active) {
            if !e.active {
                extra.push(RowMessage::warning(
                    "ORG_IMPORT_REACTIVATE",
                    "Le nœud est inactif : l'import ne le réactive pas.",
                ));
            }
        }
        lines[i].messages.extend(extra);
    }

    // A child whose new parent is itself refused cannot be created either.
    loop {
        let mut changed = false;
        for i in 0..lines.len() {
            let me = key(&lines[i].code);
            if lines[i].code.is_empty()
                || first_row_of.get(&me) != Some(&i)
                || existing.contains_key(&me)
                || lines[i].messages.iter().any(|m| m.severity == "error")
            {
                continue;
            }
            let Some(p) = lines[i].parent.clone().filter(|p| !p.is_empty()) else {
                continue;
            };
            let pk = key(&p);
            if existing.contains_key(&pk) {
                continue;
            }
            if let Some(pi) = first_row_of.get(&pk) {
                if lines[*pi].messages.iter().any(|m| m.severity == "error") {
                    lines[i].messages.push(RowMessage::error(
                        "ORG_IMPORT_PARENT_REFUSED",
                        format!("Le parent « {p} » est refusé dans ce fichier : corrigez-le d'abord."),
                    ));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    Ok(lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let me = key(&l.code);
            let is_first = !l.code.is_empty() && first_row_of.get(&me) == Some(&i);
            let ex = if is_first { existing.get(&me) } else { None };
            let has_error = l.messages.iter().any(|m| m.severity == "error");
            let deactivate = ex.is_some_and(|e| e.active) && l.active == Some(false);
            let action = if has_error || !is_first {
                "skip"
            } else if let Some(e) = ex {
                if (!l.name.is_empty() && l.name != e.name) || deactivate {
                    "update"
                } else {
                    "skip"
                }
            } else {
                "create"
            };
            RowOutcome {
                action,
                normalized: json!({
                    "code": l.code,
                    "name": l.name,
                    "type_id": l.type_id,
                    "parent_code": l.parent.filter(|p| !p.is_empty()),
                    "deactivate": deactivate,
                }),
                messages: l.messages,
            }
        })
        .collect())
}

fn fail(row: &StagedRow, message: impl Into<String>) -> RowApplied {
    RowApplied {
        row_id: row.id,
        status: "failed",
        message: message.into(),
    }
}

fn app_error_text(e: &AppError) -> String {
    match e {
        AppError::ValidationFailed(v) => v.join(" "),
        other => other.to_string(),
    }
}

async fn node_id_by_code(db: &DatabaseConnection, model_id: i64, code: &str) -> AppResult<Option<(i64, i64)>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_version FROM org_nodes \
             WHERE lower(code) = lower(?) AND structure_model_id = ? AND deleted_at IS NULL",
            [code.into(), model_id.into()],
        ))
        .await?
        .map(|r| {
            (
                r.try_get("", "id").unwrap_or_default(),
                r.try_get("", "row_version").unwrap_or_default(),
            )
        }))
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow], user_id: i32) -> AppResult<Vec<RowApplied>> {
    let model_id = crate::org::model_scope::get_active_model_id(db).await?;
    let mut out: Vec<RowApplied> = Vec::new();

    // Renames first: they never depend on another row.
    for row in rows.iter().filter(|r| r.action == "update") {
        let code = row.normalized["code"].as_str().unwrap_or_default();
        let Some((id, mut version)) = node_id_by_code(db, model_id, code).await? else {
            out.push(fail(row, format!("Le nœud « {code} » n'existe plus.")));
            continue;
        };
        let before = nodes::get_org_node_by_id(db, id).await.ok();
        let name = row.normalized["name"].as_str().unwrap_or_default().to_string();
        let mut result: AppResult<()> = Ok(());
        if before.as_ref().is_some_and(|b| b.name != name) && !name.is_empty() {
            match nodes::update_org_node_metadata(
                db,
                UpdateOrgNodeMetadataPayload {
                    node_id: id,
                    name: Some(name),
                    description: None,
                    cost_center_code: None,
                    external_reference: None,
                    erp_reference: None,
                    notes: None,
                    status: None,
                    expected_row_version: version,
                },
            )
            .await
            {
                Ok(n) => version = n.row_version,
                Err(e) => result = Err(e),
            }
        }
        if result.is_ok() && row.normalized["deactivate"].as_bool() == Some(true) {
            if let Err(e) = nodes::deactivate_org_node(db, id, version, user_id).await {
                result = Err(e);
            }
        }
        match result {
            Ok(()) => {
                if let Ok(after) = nodes::get_org_node_by_id(db, id).await {
                    let _ = audit::record_org_change(
                        db,
                        OrgAuditEventInput {
                            entity_kind: "org_node".to_string(),
                            entity_id: Some(id),
                            change_type: "update_metadata".to_string(),
                            before_json: before.and_then(|b| serde_json::to_string(&b).ok()),
                            after_json: serde_json::to_string(&after).ok(),
                            preview_summary_json: Some("{\"source\":\"bulk_import\"}".to_string()),
                            changed_by_id: Some(i64::from(user_id)),
                            requires_step_up: true,
                            apply_result: "applied".to_string(),
                        },
                    )
                    .await;
                }
                out.push(RowApplied {
                    row_id: row.id,
                    status: "updated",
                    message: String::new(),
                });
            }
            Err(e) => out.push(fail(row, app_error_text(&e))),
        }
    }

    // New nodes: a parent that is created in this file must exist before its children.
    let mut pending: Vec<&StagedRow> = rows.iter().filter(|r| r.action == "create").collect();
    loop {
        let before_len = pending.len();
        let mut still: Vec<&StagedRow> = Vec::new();
        for row in pending {
            let parent_id = match row.normalized["parent_code"].as_str() {
                None => None,
                Some(p) => {
                    if let Some((id, _)) = node_id_by_code(db, model_id, p).await? {
                        Some(id)
                    } else {
                        still.push(row);
                        continue;
                    }
                }
            };
            let created = nodes::create_org_node(
                db,
                CreateOrgNodePayload {
                    code: row.normalized["code"].as_str().unwrap_or_default().to_string(),
                    name: row.normalized["name"].as_str().unwrap_or_default().to_string(),
                    node_type_id: row.normalized["type_id"].as_i64().unwrap_or_default(),
                    parent_id,
                    description: None,
                    cost_center_code: None,
                    external_reference: None,
                    effective_from: None,
                    erp_reference: None,
                    notes: None,
                    structure_model_id: model_id,
                },
                user_id,
            )
            .await;
            match created {
                Ok(node) => {
                    let _ = audit::record_org_change(
                        db,
                        OrgAuditEventInput {
                            entity_kind: "org_node".to_string(),
                            entity_id: Some(node.id),
                            change_type: "create_node".to_string(),
                            before_json: None,
                            after_json: serde_json::to_string(&node).ok(),
                            preview_summary_json: Some("{\"source\":\"bulk_import\"}".to_string()),
                            changed_by_id: Some(i64::from(user_id)),
                            requires_step_up: true,
                            apply_result: "applied".to_string(),
                        },
                    )
                    .await;
                    out.push(RowApplied {
                        row_id: row.id,
                        status: "created",
                        message: String::new(),
                    });
                }
                Err(e) => out.push(fail(row, app_error_text(&e))),
            }
        }
        pending = still;
        if pending.is_empty() || pending.len() == before_len {
            break;
        }
    }
    for row in pending {
        out.push(fail(
            row,
            "Le parent n'a pas pu être créé : ce nœud n'a pas été importé.",
        ));
    }

    // Lines with nothing to do are reported as skipped.
    for row in rows.iter().filter(|r| r.action == "skip") {
        out.push(RowApplied {
            row_id: row.id,
            status: "skipped",
            message: "Déjà à jour.".to_string(),
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let model_id = crate::org::model_scope::get_active_model_id(db).await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.code, n.name, t.code AS type_code, p.code AS parent_code, n.status \
             FROM org_nodes n \
             JOIN org_node_types t ON t.id = n.node_type_id \
             LEFT JOIN org_nodes p ON p.id = n.parent_id \
             WHERE n.structure_model_id = ? AND n.deleted_at IS NULL \
             ORDER BY n.depth, n.code",
            [model_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let status: String = r.try_get("", "status")?;
            Ok(vec![
                r.try_get("", "code")?,
                r.try_get("", "name")?,
                r.try_get("", "type_code")?,
                r.try_get::<Option<String>>("", "parent_code")?.unwrap_or_default(),
                if status == "active" { "oui" } else { "non" }.to_string(),
            ])
        })
        .collect()
}
