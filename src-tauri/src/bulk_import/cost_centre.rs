//! Cost centre import (finance), keyed by cost centre code.
//!
//! Parents may come before or after their children in the file. A cost centre that already exists
//! is updated (name, site, parent, owner, active flag); an empty cell never clears a value. Nothing
//! is ever deleted. The owner is given as a personnel number (matricule): it must exist in the
//! personnel register, and the owner is stored when that person has a user account.

use std::collections::{HashMap, HashSet};

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
    code: String,
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
    // Existing cost centres by code. Two codes that differ only by letter case cannot be told apart
    // by the file: lines naming them are refused.
    let mut existing: HashMap<String, Existing> = HashMap::new();
    let mut ambiguous: HashSet<String> = HashSet::new();
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
        if existing.contains_key(&key(&code)) {
            ambiguous.insert(key(&code));
        }
        existing.insert(
            key(&code),
            Existing {
                code: code.clone(),
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

    // Personnel numbers, with the live user account of the person when there is one (a disabled or
    // deleted account does not count).
    let mut people: HashMap<String, Option<i64>> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.employee_code AS code, MIN(ua.id) AS user_id \
             FROM personnel p LEFT JOIN user_accounts ua \
               ON ua.personnel_id = p.id AND ua.is_active = 1 AND ua.deleted_at IS NULL \
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

    // Rules that only need the line and what already exists.
    for (i, line) in lines.iter_mut().enumerate() {
        if line.code.is_empty() || first_row_of.get(&key(&line.code)) != Some(&i) {
            continue;
        }
        let me = key(&line.code);
        for c in std::iter::once(&line.code).chain(line.parent.as_ref()) {
            if ambiguous.contains(&key(c)) {
                line.messages.push(RowMessage::error(
                    "CC_IMPORT_CODE_AMBIGUOUS",
                    format!(
                        "Plusieurs centres de coût ont le code « {c} » à la casse près : renommez-les dans Finance avant d'importer."
                    ),
                ));
            }
        }
        let is_new = !existing.contains_key(&me);
        // Name and site are needed to create a cost centre; an existing one keeps what a blank
        // cell leaves out.
        if is_new && line.name.is_empty() {
            line.messages
                .push(RowMessage::error("CC_IMPORT_NAME", "Le libellé est obligatoire."));
        }
        if is_new && line.site.is_none() {
            line.messages.push(RowMessage::error(
                "CC_IMPORT_SITE",
                "Le code site est obligatoire pour un nouveau centre de coût.",
            ));
        }
        if let Some(p) = &line.parent {
            let pk = key(p);
            if pk == me {
                line.messages.push(RowMessage::error(
                    "CC_IMPORT_CYCLE",
                    "Un centre de coût ne peut pas être son propre parent.",
                ));
            } else if !existing.contains_key(&pk) && !first_row_of.contains_key(&pk) {
                line.messages.push(RowMessage::error(
                    "CC_IMPORT_PARENT_UNKNOWN",
                    format!("Le parent « {p} » est introuvable : ni parmi les centres de coût, ni dans le fichier."),
                ));
            }
        }
    }

    hierarchy_rules(&mut lines, &existing, &first_row_of);

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
            RowOutcome {
                action,
                normalized: json!({
                    "code": l.code,
                    "name": l.name,
                    "entity_id": l.entity_id,
                    "parent_code": l.parent,
                    "owner_id": l.owner_id(),
                    "active": l.active,
                }),
                messages: l.messages,
            }
        })
        .collect())
}

impl Line {
    fn owner_id(&self) -> Option<i64> {
        match self.owner {
            Owner::User(id) => Some(id),
            Owner::None | Owner::Unknown | Owner::NoAccount => None,
        }
    }
}

fn has_error(line: &Line) -> bool {
    line.messages.iter().any(|m| m.severity == "error")
}

/// Whether applying the line would change the existing cost centre (blank cells change nothing).
fn changes(line: &Line, e: &Existing) -> bool {
    (!line.name.is_empty() && line.name != e.name)
        || line.entity_id.is_some_and(|id| Some(id) != e.entity_id)
        || line.parent.as_ref().is_some_and(|p| Some(key(p)) != e.parent_code)
        || line.owner_id().is_some_and(|id| Some(id) != e.owner_id)
        || line.active.is_some_and(|a| a != e.active)
}

/// Parent rules, checked against the hierarchy as it will be once the file is applied: only lines
/// that are accepted count, so they are re-checked until no further line is refused. A refused line
/// changes nothing, which can in turn leave another line under an inactive parent or a refused one.
/// Finance re-checks the same rules when each line is written.
fn hierarchy_rules(lines: &mut [Line], existing: &HashMap<String, Existing>, first_row_of: &HashMap<String, usize>) {
    loop {
        let accepted: Vec<bool> = lines
            .iter()
            .enumerate()
            .map(|(i, l)| !l.code.is_empty() && first_row_of.get(&key(&l.code)) == Some(&i) && !has_error(l))
            .collect();

        // Parent, active flag and shown code of each cost centre once the accepted lines are applied.
        let mut parent_after: HashMap<String, Option<String>> = HashMap::new();
        let mut active_after: HashMap<String, bool> = HashMap::new();
        let mut shown: HashMap<String, String> = HashMap::new();
        for (k, e) in existing {
            parent_after.insert(k.clone(), e.parent_code.clone());
            active_after.insert(k.clone(), e.active);
            shown.insert(k.clone(), e.code.clone());
        }
        for (i, l) in lines.iter().enumerate() {
            if !accepted[i] {
                continue;
            }
            let me = key(&l.code);
            match &l.parent {
                Some(p) => {
                    parent_after.insert(me.clone(), Some(key(p)));
                }
                None => {
                    parent_after.entry(me.clone()).or_insert(None);
                }
            }
            let was = active_after.get(&me).copied().unwrap_or(true);
            active_after.insert(me.clone(), l.active.unwrap_or(was));
            shown.entry(me).or_insert_with(|| l.code.clone());
        }

        // Each failing line, with the cost centres above it (its parent path after the import).
        let mut failing: Vec<(usize, RowMessage, Vec<String>)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if !accepted[i] {
                continue;
            }
            let me = key(&l.code);
            // A line that changes nothing is skipped and checks nothing.
            if existing.get(&me).is_some_and(|e| !changes(l, e)) {
                continue;
            }
            // The parent after the import: the one in the line, or the current one when the cell is blank.
            let Some(pk) = parent_after.get(&me).cloned().flatten() else {
                continue;
            };
            let p = shown.get(&pk).cloned().unwrap_or_else(|| pk.clone());
            let mut path: Vec<String> = Vec::new();
            let mut cur = Some(pk.clone());
            let mut in_loop = false;
            while let Some(c) = cur {
                if c == me {
                    in_loop = true;
                    break;
                }
                if path.len() >= 500 {
                    break;
                }
                cur = parent_after.get(&c).cloned().flatten();
                path.push(c);
            }
            let message = if first_row_of.get(&pk).is_some_and(|pi| !accepted[*pi]) {
                RowMessage::error(
                    "CC_IMPORT_PARENT_REFUSED",
                    format!("Le parent « {p} » est refusé dans ce fichier : corrigez-le d'abord."),
                )
            } else if in_loop {
                RowMessage::error(
                    "CC_IMPORT_CYCLE",
                    format!("Le parent « {p} » crée une boucle dans la hiérarchie."),
                )
            } else if active_after.get(&me).copied().unwrap_or(true) && active_after.get(&pk) == Some(&false) {
                RowMessage::error(
                    "CC_IMPORT_PARENT_INACTIVE",
                    format!("Le parent « {p} » est inactif : un centre actif ne peut pas être rattaché dessous."),
                )
            } else {
                continue;
            };
            failing.push((i, message, path));
        }
        if failing.is_empty() {
            break;
        }
        // A line whose path holds another failing line waits for the next round: refusing that line
        // changes the hierarchy above it, so its own reason could no longer hold. When every failing
        // line waits (they form a loop), all of them are refused.
        let failing_codes: HashSet<String> = failing.iter().map(|(i, _, _)| key(&lines[*i].code)).collect();
        let ready = |path: &Vec<String>| !path.iter().any(|c| failing_codes.contains(c));
        let any_ready = failing.iter().any(|(_, _, path)| ready(path));
        for (i, m, path) in failing {
            if !any_ready || ready(&path) {
                lines[i].messages.push(m);
            }
        }
    }
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

fn text(row: &StagedRow, field: &str) -> String {
    row.normalized[field].as_str().unwrap_or_default().to_string()
}

/// Id and row version of a cost centre, matched on the code the same way as the preview (trimmed,
/// any letter case, accented letters included). `Err` when several cost centres match.
async fn cost_centre_by_code(db: &DatabaseConnection, code: &str) -> AppResult<Result<Option<(i64, i64)>, String>> {
    let wanted = key(code);
    let mut found = None;
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, row_version FROM cost_centers".to_string(),
        ))
        .await?
    {
        let c: String = r.try_get("", "code")?;
        if key(&c) == wanted {
            if found.is_some() {
                return Ok(Err(format!(
                    "Plusieurs centres de coût ont le code « {code} » à la casse près : renommez-les dans Finance avant d'importer."
                )));
            }
            found = Some((r.try_get("", "id")?, r.try_get("", "row_version")?));
        }
    }
    Ok(Ok(found))
}

/// Writes one line. `Ok(Err(reason))` means finance refused it for now: it is tried again while
/// other lines go through, since a parent created or reactivated later in the file may unblock it.
async fn apply_one(db: &DatabaseConnection, row: &StagedRow) -> AppResult<Result<&'static str, String>> {
    let code = text(row, "code");
    let parent_id = match row.normalized["parent_code"].as_str() {
        None => None,
        Some(p) => match cost_centre_by_code(db, p).await? {
            Err(reason) => return Ok(Err(reason)),
            Ok(Some((pid, _))) => Some(pid),
            Ok(None) => {
                return Ok(Err(format!(
                    "Le parent « {p} » n'existe pas : ce centre de coût n'a pas été importé."
                )))
            }
        },
    };
    let active = row.normalized["active"].as_bool();
    let owner = row.normalized["owner_id"].as_i64();
    let entity_id = row.normalized["entity_id"].as_i64();
    if row.action == "create" {
        let created = queries::create_cost_center(
            db,
            CreateCostCenterInput {
                code,
                name: text(row, "name"),
                entity_id,
                parent_cost_center_id: parent_id,
                budget_owner_id: owner,
                erp_external_id: None,
                is_active: active,
            },
        )
        .await;
        return Ok(created.map(|_| "created").map_err(|e| app_error_text(&e)));
    }
    let (id, version) = match cost_centre_by_code(db, &code).await? {
        Err(reason) => return Ok(Err(reason)),
        Ok(Some(found)) => found,
        Ok(None) => return Ok(Err(format!("Le centre de coût « {code} » n'existe plus."))),
    };
    let name = text(row, "name");
    let updated = queries::update_cost_center(
        db,
        id,
        version,
        UpdateCostCenterInput {
            code: None,
            name: (!name.is_empty()).then_some(name),
            entity_id,
            parent_cost_center_id: parent_id,
            budget_owner_id: owner,
            erp_external_id: None,
            is_active: active,
        },
    )
    .await;
    Ok(updated.map(|_| "updated").map_err(|e| app_error_text(&e)))
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out: Vec<RowApplied> = Vec::new();

    // New and existing cost centres in one queue, tried again as long as at least one line goes
    // through: a parent created, moved or reactivated anywhere in the file is then in place before
    // its children, whatever the depth.
    let mut pending: Vec<(&StagedRow, String)> = rows
        .iter()
        .filter(|r| r.action == "create" || r.action == "update")
        .map(|r| (r, String::new()))
        .collect();
    loop {
        let before_len = pending.len();
        let mut still: Vec<(&StagedRow, String)> = Vec::new();
        for (row, _) in pending {
            match apply_one(db, row).await? {
                Ok(status) => out.push(RowApplied {
                    row_id: row.id,
                    status,
                    message: String::new(),
                }),
                Err(reason) => still.push((row, reason)),
            }
        }
        pending = still;
        if pending.is_empty() || pending.len() == before_len {
            break;
        }
    }
    for (row, reason) in pending {
        out.push(fail(row, reason));
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
