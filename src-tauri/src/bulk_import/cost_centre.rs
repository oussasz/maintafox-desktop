//! Cost centre import (finance), keyed by cost centre code.
//!
//! Parents may come before or after their children in the file. A cost centre that already exists
//! is updated (name, site, parent, owner, active flag); an empty cell never clears a value. Nothing
//! is ever deleted. The owner is given as a personnel number (matricule): it must exist in the
//! personnel register, and the owner is stored when that person has a user account.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::{key, parse_active};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::finance::domain::{CreateCostCenterInput, UpdateCostCenterInput};
use crate::finance::queries;

pub const SPEC: EntitySpec = EntitySpec {
    code: "cost_centre",
    label: "Centres de coût",
    order: 2,
    needs: "Organisation (les codes site). Personnel, si vous renseignez un responsable.",
    permission: crate::rbac::permissions::FIN_BUDGET,
    columns: &[
        Column {
            key: "code",
            header: "code",
            required: true,
            aliases: &["code_centre", "centre_de_cout", "cc"],
            hint: "Code unique du centre de coût, par exemple CC-MAINT-NORD.",
        },
        Column {
            key: "name",
            header: "libellé",
            required: true,
            aliases: &["libelle", "nom", "designation", "name"],
            hint: "Nom affiché du centre de coût.",
        },
        Column {
            key: "site_code",
            header: "code site",
            required: true,
            aliases: &["site", "code_entite", "entite", "site_code"],
            hint: "Code d'un nœud de l'organisation (site, zone…). Obligatoire pour un nouveau centre.",
        },
        Column {
            key: "parent_code",
            header: "code parent",
            required: false,
            aliases: &["parent", "code_parent", "parent_code"],
            hint: "Code du centre de coût parent. Vide pour un centre de premier niveau.",
        },
        Column {
            key: "owner",
            header: "matricule responsable",
            required: false,
            aliases: &["responsable", "matricule", "matricule_responsable", "owner"],
            hint: "Matricule d'une personne du registre du personnel (elle doit avoir un compte utilisateur).",
        },
        Column {
            key: "active",
            header: "actif",
            required: false,
            aliases: &["actif_oui_non", "active"],
            hint: "oui ou non. Vide : un nouveau centre est actif, un centre existant ne change pas.",
        },
    ],
};

struct Existing {
    name: String,
    entity_id: Option<i64>,
    parent_code: Option<String>,
    owner_id: Option<i64>,
    active: bool,
}

enum Owner {
    None,
    Unknown,
    NoAccount,
    User(i64),
}

struct Line {
    code: String,
    name: String,
    site: Option<String>,
    entity_id: Option<i64>,
    parent: Option<String>,
    owner: Owner,
    active: Option<bool>,
    messages: Vec<RowMessage>,
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    // Existing cost centres by code.
    let mut existing: HashMap<String, Existing> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT cc.code, cc.name, cc.entity_id, cc.budget_owner_id, cc.is_active, p.code AS parent_code \
             FROM cost_centers cc LEFT JOIN cost_centers p ON p.id = cc.parent_cost_center_id"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let active: i64 = r.try_get("", "is_active")?;
        existing.insert(
            key(&code),
            Existing {
                name: r.try_get("", "name")?,
                entity_id: r.try_get("", "entity_id")?,
                parent_code: r.try_get::<Option<String>>("", "parent_code")?.map(|p| key(&p)),
                owner_id: r.try_get("", "budget_owner_id")?,
                active: active != 0,
            },
        );
    }

    // Organisation nodes of the active model, by code.
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

    // Personnel numbers, with the user account of the person when there is one.
    let mut people: HashMap<String, Option<i64>> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.employee_code AS code, MIN(ua.id) AS user_id \
             FROM personnel p LEFT JOIN user_accounts ua ON ua.personnel_id = p.id \
             GROUP BY p.id"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        people.insert(key(&code), r.try_get::<Option<i64>>("", "user_id")?);
    }

    // First pass: read each line on its own.
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let code = raw.get("code").map(|s| s.trim().to_string()).unwrap_or_default();
        let name = raw.get("name").map(|s| s.trim().to_string()).unwrap_or_default();
        if code.is_empty() {
            messages.push(RowMessage::error(
                "CC_IMPORT_CODE",
                "Le code du centre de coût est obligatoire.",
            ));
        } else if let Some(first) = first_row_of.get(&key(&code)) {
            messages.push(RowMessage::error(
                "CC_IMPORT_DUPLICATE",
                format!("Le code « {code} » apparaît déjà à la ligne {} du fichier.", first + 2),
            ));
        } else {
            first_row_of.insert(key(&code), i);
        }
        let site = raw
            .get("site_code")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let entity_id = match &site {
            None => None,
            Some(s) => {
                if let Some(id) = sites.get(&key(s)) {
                    Some(*id)
                } else {
                    messages.push(RowMessage::error(
                        "CC_IMPORT_SITE",
                        format!(
                            "Le code site « {s} » n'existe pas dans l'organisation : importez d'abord l'organisation."
                        ),
                    ));
                    None
                }
            }
        };
        let owner = match raw.get("owner").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            None => Owner::None,
            Some(m) => match people.get(&key(m)) {
                None => {
                    messages.push(RowMessage::error(
                        "CC_IMPORT_OWNER_UNKNOWN",
                        format!("Le matricule « {m} » n'existe pas dans le personnel : importez d'abord le personnel."),
                    ));
                    Owner::Unknown
                }
                Some(None) => {
                    messages.push(RowMessage::warning(
                        "CC_IMPORT_OWNER_NO_ACCOUNT",
                        format!("« {m} » n'a pas de compte utilisateur : le responsable ne sera pas renseigné."),
                    ));
                    Owner::NoAccount
                }
                Some(Some(uid)) => Owner::User(*uid),
            },
        };
        let active = match parse_active(raw.get("active")) {
            Ok(a) => a,
            Err(()) => {
                messages.push(RowMessage::error(
                    "CC_IMPORT_ACTIVE",
                    "La colonne « actif » doit contenir oui ou non.",
                ));
                None
            }
        };
        lines.push(Line {
            code,
            name,
            site,
            entity_id,
            parent: raw
                .get("parent_code")
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty()),
            owner,
            active,
            messages,
        });
    }

    // Who is the parent of each code once the file is applied?
    let mut effective_parent: HashMap<String, Option<String>> = HashMap::new();
    for (k, e) in &existing {
        effective_parent.insert(k.clone(), e.parent_code.clone());
    }
    for (i, l) in lines.iter().enumerate() {
        if l.code.is_empty() || first_row_of.get(&key(&l.code)) != Some(&i) {
            continue;
        }
        match &l.parent {
            Some(p) => {
                effective_parent.insert(key(&l.code), Some(key(p)));
            }
            None => {
                effective_parent.entry(key(&l.code)).or_insert(None);
            }
        }
    }
    // Is each code active once the file is applied?
    let mut active_after: HashMap<String, bool> = existing.iter().map(|(k, e)| (k.clone(), e.active)).collect();
    for (i, l) in lines.iter().enumerate() {
        if l.code.is_empty() || first_row_of.get(&key(&l.code)) != Some(&i) {
            continue;
        }
        let was = active_after.get(&key(&l.code)).copied().unwrap_or(true);
        active_after.insert(key(&l.code), l.active.unwrap_or(was));
    }

    // Second pass: rules that need the whole file.
    for (i, line) in lines.iter_mut().enumerate() {
        if line.code.is_empty() || first_row_of.get(&key(&line.code)) != Some(&i) {
            continue;
        }
        let me = key(&line.code);
        let is_new = !existing.contains_key(&me);
        let mut extra: Vec<RowMessage> = Vec::new();
        // Name and site are needed to create a cost centre; an existing one keeps what a blank
        // cell leaves out.
        if is_new && line.name.is_empty() {
            extra.push(RowMessage::error("CC_IMPORT_NAME", "Le libellé est obligatoire."));
        }
        if is_new && line.site.is_none() {
            extra.push(RowMessage::error(
                "CC_IMPORT_SITE",
                "Le code site est obligatoire pour un nouveau centre de coût.",
            ));
        }
        if let Some(p) = line.parent.clone() {
            let pk = key(&p);
            if pk == me {
                extra.push(RowMessage::error(
                    "CC_IMPORT_CYCLE",
                    "Un centre de coût ne peut pas être son propre parent.",
                ));
            } else if !existing.contains_key(&pk) && !first_row_of.contains_key(&pk) {
                extra.push(RowMessage::error(
                    "CC_IMPORT_PARENT_UNKNOWN",
                    format!("Le parent « {p} » est introuvable : ni parmi les centres de coût, ni dans le fichier."),
                ));
            } else {
                let mut cur = Some(pk.clone());
                let mut steps = 0;
                let mut in_loop = false;
                while let Some(c) = cur {
                    if c == me {
                        in_loop = true;
                        extra.push(RowMessage::error(
                            "CC_IMPORT_CYCLE",
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
                let child_active = active_after.get(&me).copied().unwrap_or(true);
                if !in_loop && child_active && active_after.get(&pk) == Some(&false) {
                    extra.push(RowMessage::error(
                        "CC_IMPORT_PARENT_INACTIVE",
                        format!("Le parent « {p} » est inactif : un centre actif ne peut pas être rattaché dessous."),
                    ));
                }
            }
        }
        line.messages.extend(extra);
    }

    // A child whose parent is created in this file and refused there cannot be imported either.
    loop {
        let mut changed = false;
        for i in 0..lines.len() {
            let me = key(&lines[i].code);
            if lines[i].code.is_empty()
                || first_row_of.get(&me) != Some(&i)
                || lines[i].messages.iter().any(|m| m.severity == "error")
            {
                continue;
            }
            let Some(p) = lines[i].parent.clone() else {
                continue;
            };
            let pk = key(&p);
            if existing.contains_key(&pk) {
                continue;
            }
            if let Some(pi) = first_row_of.get(&pk) {
                if lines[*pi].messages.iter().any(|m| m.severity == "error") {
                    lines[i].messages.push(RowMessage::error(
                        "CC_IMPORT_PARENT_REFUSED",
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
            let owner_id = match l.owner {
                Owner::User(id) => Some(id),
                Owner::None | Owner::Unknown | Owner::NoAccount => None,
            };
            let action = if has_error || !is_first {
                "skip"
            } else if let Some(e) = ex {
                let changed = (!l.name.is_empty() && l.name != e.name)
                    || l.entity_id.is_some_and(|id| Some(id) != e.entity_id)
                    || l.parent.as_ref().is_some_and(|p| Some(key(p)) != e.parent_code)
                    || owner_id.is_some_and(|id| Some(id) != e.owner_id)
                    || l.active.is_some_and(|a| a != e.active);
                if changed {
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
                    "entity_id": l.entity_id,
                    "parent_code": l.parent,
                    "owner_id": owner_id,
                    "active": l.active,
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

async fn cost_centre_by_code(db: &DatabaseConnection, code: &str) -> AppResult<Option<(i64, i64)>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_version FROM cost_centers WHERE lower(code) = lower(?)",
            [code.into()],
        ))
        .await?
        .map(|r| {
            (
                r.try_get("", "id").unwrap_or_default(),
                r.try_get("", "row_version").unwrap_or_default(),
            )
        }))
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out: Vec<RowApplied> = Vec::new();
    let text = |row: &StagedRow, field: &str| row.normalized[field].as_str().unwrap_or_default().to_string();

    // New cost centres: a parent created in this file must exist before its children.
    let mut pending: Vec<&StagedRow> = rows.iter().filter(|r| r.action == "create").collect();
    loop {
        let before_len = pending.len();
        let mut still: Vec<&StagedRow> = Vec::new();
        for row in pending {
            let parent_id = match row.normalized["parent_code"].as_str() {
                None => None,
                Some(p) => {
                    if let Some((id, _)) = cost_centre_by_code(db, p).await? {
                        Some(id)
                    } else {
                        still.push(row);
                        continue;
                    }
                }
            };
            let created = queries::create_cost_center(
                db,
                CreateCostCenterInput {
                    code: text(row, "code"),
                    name: text(row, "name"),
                    entity_id: row.normalized["entity_id"].as_i64(),
                    parent_cost_center_id: parent_id,
                    budget_owner_id: row.normalized["owner_id"].as_i64(),
                    erp_external_id: None,
                    is_active: row.normalized["active"].as_bool(),
                },
            )
            .await;
            out.push(match created {
                Ok(_) => RowApplied {
                    row_id: row.id,
                    status: "created",
                    message: String::new(),
                },
                Err(e) => fail(row, app_error_text(&e)),
            });
        }
        pending = still;
        if pending.is_empty() || pending.len() == before_len {
            break;
        }
    }
    for row in pending {
        out.push(fail(
            row,
            "Le parent n'a pas pu être créé : ce centre de coût n'a pas été importé.",
        ));
    }

    // Existing cost centres: two passes, so a parent reactivated later in the file does not block
    // its children.
    let mut todo: Vec<&StagedRow> = rows.iter().filter(|r| r.action == "update").collect();
    for pass in 0..2 {
        let mut retry: Vec<&StagedRow> = Vec::new();
        for row in todo {
            let code = text(row, "code");
            let Some((id, version)) = cost_centre_by_code(db, &code).await? else {
                out.push(fail(row, format!("Le centre de coût « {code} » n'existe plus.")));
                continue;
            };
            let parent_id = match row.normalized["parent_code"].as_str() {
                None => None,
                Some(p) => match cost_centre_by_code(db, p).await? {
                    Some((pid, _)) => Some(pid),
                    None => {
                        out.push(fail(row, format!("Le parent « {p} » n'existe pas.")));
                        continue;
                    }
                },
            };
            let name = text(row, "name");
            let result = queries::update_cost_center(
                db,
                id,
                version,
                UpdateCostCenterInput {
                    code: None,
                    name: (!name.is_empty()).then_some(name),
                    entity_id: row.normalized["entity_id"].as_i64(),
                    parent_cost_center_id: parent_id,
                    budget_owner_id: row.normalized["owner_id"].as_i64(),
                    erp_external_id: None,
                    is_active: row.normalized["active"].as_bool(),
                },
            )
            .await;
            match result {
                Ok(_) => out.push(RowApplied {
                    row_id: row.id,
                    status: "updated",
                    message: String::new(),
                }),
                Err(_) if pass == 0 => retry.push(row),
                Err(e) => out.push(fail(row, app_error_text(&e))),
            }
        }
        todo = retry;
        if todo.is_empty() {
            break;
        }
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
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT cc.code, cc.name, org.code AS site_code, p.code AS parent_code, \
                    per.employee_code AS owner_code, cc.is_active \
             FROM cost_centers cc \
             LEFT JOIN org_nodes org ON org.id = cc.entity_id \
             LEFT JOIN cost_centers p ON p.id = cc.parent_cost_center_id \
             LEFT JOIN user_accounts ua ON ua.id = cc.budget_owner_id \
             LEFT JOIN personnel per ON per.id = ua.personnel_id \
             ORDER BY cc.code"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let active: i64 = r.try_get("", "is_active")?;
            Ok(vec![
                r.try_get("", "code")?,
                r.try_get("", "name")?,
                r.try_get::<Option<String>>("", "site_code")?.unwrap_or_default(),
                r.try_get::<Option<String>>("", "parent_code")?.unwrap_or_default(),
                r.try_get::<Option<String>>("", "owner_code")?.unwrap_or_default(),
                if active != 0 { "oui" } else { "non" }.to_string(),
            ])
        })
        .collect()
}
