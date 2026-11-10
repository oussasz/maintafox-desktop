//! Tasks of the preventive plans (sheet "Tâches"). One line = one task: plan code, order number,
//! description. The lines of one plan replace the task list of that plan's latest version, which
//! must still be a DRAFT (a published version is never changed: the planner makes a new version
//! in Préventif). This is the list the version form shows, stored as `{"steps": [...]}`.
//!
//! Advisor ruling 2026-11-10 (Q1): plan code, order number and description only.
//! Rights: `pm.edit` on each plan (the rule of the version form), in the preview and again when
//! the batch is applied. A plan is replaced whole or not at all: one wrong line of a plan stops
//! every line of that plan.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Value};

use super::equipment_meter::parse_number;
use super::pm_plan::{access_of, cell, err, has_error};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::auth::scope::AccessMap;
use crate::errors::{AppError, AppResult};
use crate::pm::domain::UpdatePmPlanVersionInput;
use crate::rbac::permissions::PM_EDIT;

pub const SPEC: EntitySpec = EntitySpec {
    code: "pm_task",
    label: "Tâches des plans préventifs",
    order: 7,
    needs: "Plans préventifs (encore en brouillon).",
    permission: PM_EDIT,
    columns: &[
        Column {
            key: "plan_code",
            header: "code plan",
            required: true,
            aliases: &["plan", "code_plan", "plan_code"],
            hint: "Plan préventif dont la version brouillon reçoit la tâche.",
        },
        Column {
            key: "order",
            header: "n° d'ordre",
            required: true,
            aliases: &["n_d_ordre", "no_d_ordre", "numero_d_ordre", "ordre", "order", "etape"],
            hint: "Rang de la tâche dans le plan : 1, 2, 3…",
        },
        Column {
            key: "description",
            header: "description",
            required: true,
            aliases: &["tache", "taches", "libelle", "description"],
            hint: "Ce qu'il faut faire. Les lignes d'un plan remplacent toutes ses tâches actuelles.",
        },
    ],
};

/// Largest order number of a file line.
const MAX_ORDER: f64 = 999.0;
/// Longest task text.
const MAX_DESCRIPTION: usize = 1000;

/// A plan as the import sees it: its latest version, if any.
#[derive(Clone)]
struct Plan {
    version_id: Option<i64>,
    version_status: Option<String>,
    row_version: i64,
    task_package_json: Option<String>,
}

/// Plans by code (stored in upper case), with their latest version.
async fn plans(db: &impl ConnectionTrait) -> AppResult<HashMap<String, Plan>> {
    let mut out = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.code, v.id AS version_id, v.status, v.row_version, v.task_package_json \
             FROM pm_plans p \
             LEFT JOIN pm_plan_versions v \
               ON v.id = (SELECT MAX(v2.id) FROM pm_plan_versions v2 WHERE v2.pm_plan_id = p.id)"
                .to_string(),
        ))
        .await?
    {
        out.insert(
            crate::pm::queries::normalize_plan_code(&r.try_get::<String>("", "code")?),
            Plan {
                version_id: r.try_get("", "version_id")?,
                version_status: r.try_get("", "status")?,
                row_version: r.try_get::<Option<i64>>("", "row_version")?.unwrap_or(0),
                task_package_json: r.try_get("", "task_package_json")?,
            },
        );
    }
    Ok(out)
}

/// The task list stored on a version (`{"steps": [...]}`).
fn steps_of(task_package_json: Option<&str>) -> Vec<String> {
    task_package_json
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.get("steps").cloned())
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
}

/// The version's task package with its steps replaced (other keys, if any, are kept).
fn with_steps(task_package_json: Option<&str>, steps: &[String]) -> String {
    let mut package = task_package_json
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    package["steps"] = json!(steps);
    package.to_string()
}

/// Whether the user may edit this version (`pm.edit` on its plan).
async fn may_edit(db: &DatabaseConnection, access: &AccessMap, version_id: i64) -> AppResult<bool> {
    match crate::pm::access::check_on_version(db, access, PM_EDIT, version_id).await {
        Ok(_) => Ok(true),
        Err(
            AppError::PermissionDenied(_)
            | AppError::OrgValidationFailed(_)
            | AppError::ValidationFailed(_)
            | AppError::NotFound { .. },
        ) => Ok(false),
        Err(e) => Err(e),
    }
}

fn no_right(plan: &str) -> String {
    format!("Vous n'avez pas le droit de modifier le plan préventif « {plan} ».")
}

fn not_draft(plan: &str) -> String {
    format!(
        "La dernière version du plan « {plan} » n'est pas un brouillon : créez une nouvelle version dans Préventif, puis importez ses tâches."
    )
}

/// One line read: (order, description). Every problem is pushed to `messages`.
fn read_line(raw: &HashMap<String, String>, messages: &mut Vec<RowMessage>) -> Option<(i64, String)> {
    let order = parse_number(&cell(raw, "order"))
        .filter(|v| v.fract() == 0.0 && (1.0..=MAX_ORDER).contains(v))
        .map(|v| v as i64);
    if order.is_none() {
        err(
            messages,
            "PM_TASK_IMPORT_ORDER",
            format!("Le n° d'ordre doit être un nombre entier de 1 à {MAX_ORDER}."),
        );
    }
    // The version form keeps one task per line: a line break inside a cell joins the text.
    let description = cell(raw, "description")
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if description.is_empty() {
        err(
            messages,
            "PM_TASK_IMPORT_DESCRIPTION",
            "La description de la tâche est obligatoire.",
        );
    } else if description.chars().count() > MAX_DESCRIPTION {
        err(
            messages,
            "PM_TASK_IMPORT_DESCRIPTION_LONG",
            format!("La description dépasse {MAX_DESCRIPTION} caractères."),
        );
    }
    if has_error(messages) {
        return None;
    }
    Some((order?, description))
}

/// One line of a plan in the file: (line index, order, description).
type Line = (usize, Option<i64>, String);

pub async fn validate(
    db: &DatabaseConnection,
    rows: &[HashMap<String, String>],
    user_id: Option<i64>,
) -> AppResult<Vec<RowOutcome>> {
    let user_id =
        user_id.ok_or_else(|| AppError::Internal(anyhow::anyhow!("PM task import needs the importing user")))?;
    let access = access_of(db, user_id).await?;
    let plans = plans(db).await?;
    let mut right_of: HashMap<i64, bool> = HashMap::new();
    // Plan code → its lines, in file order (plans in order of first appearance).
    let mut groups: Vec<(String, Vec<Line>)> = Vec::new();
    let mut out: Vec<RowOutcome> = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let code = crate::pm::queries::normalize_plan_code(&cell(raw, "plan_code"));
        let read = read_line(raw, &mut messages);
        if code.is_empty() {
            err(&mut messages, "PM_TASK_IMPORT_PLAN", "Le code plan est obligatoire.");
        } else {
            match plans.get(&code) {
                None => err(
                    &mut messages,
                    "PM_TASK_IMPORT_PLAN_UNKNOWN",
                    format!("Le plan « {code} » n'existe pas : importez d'abord les plans préventifs."),
                ),
                Some(p) => match p.version_id {
                    Some(v) if p.version_status.as_deref() == Some("draft") => {
                        let allowed = match right_of.get(&v) {
                            Some(a) => *a,
                            None => {
                                let a = may_edit(db, &access, v).await?;
                                right_of.insert(v, a);
                                a
                            }
                        };
                        if !allowed {
                            err(&mut messages, "PM_TASK_IMPORT_RIGHT", no_right(&code));
                        }
                    }
                    _ => err(&mut messages, "PM_TASK_IMPORT_NOT_DRAFT", not_draft(&code)),
                },
            }
        }
        let (order, description) = match read {
            Some((o, d)) => (Some(o), d),
            None => (None, String::new()),
        };
        if !code.is_empty() {
            let pos = match groups.iter().position(|(c, _)| *c == code) {
                Some(p) => p,
                None => {
                    groups.push((code.clone(), Vec::new()));
                    groups.len() - 1
                }
            };
            if let Some(o) = order {
                if let Some((first, _, _)) = groups[pos].1.iter().find(|(_, other, _)| *other == Some(o)) {
                    err(
                        &mut messages,
                        "PM_TASK_IMPORT_ORDER_TWICE",
                        format!("Le plan « {code} » a déjà une tâche n° {o} à la ligne {}.", first + 2),
                    );
                }
            }
            groups[pos].1.push((i, order, description.clone()));
        }
        out.push(RowOutcome {
            action: "update",
            normalized: json!({ "plan": code, "order": order, "description": description }),
            messages,
        });
    }

    // A plan is replaced whole: one wrong line stops all its lines; an unchanged list is skipped.
    for (code, lines) in &groups {
        let first_wrong = lines.iter().map(|(i, _, _)| *i).find(|i| has_error(&out[*i].messages));
        if let Some(first_wrong) = first_wrong {
            for (i, _, _) in lines {
                if !has_error(&out[*i].messages) {
                    err(
                        &mut out[*i].messages,
                        "PM_TASK_IMPORT_PLAN_STOPPED",
                        format!(
                            "La ligne {} du plan « {code} » est en erreur : aucune tâche de ce plan n'est remplacée.",
                            first_wrong + 2
                        ),
                    );
                }
            }
            continue;
        }
        let mut sorted = lines.clone();
        sorted.sort_by_key(|(_, o, _)| *o);
        let steps: Vec<String> = sorted.into_iter().map(|(_, _, d)| d).collect();
        let current = steps_of(plans.get(code).and_then(|p| p.task_package_json.as_deref()));
        let first = lines[0].0;
        if current == steps {
            for (i, _, _) in lines {
                out[*i].action = "skip";
            }
            out[first].messages.push(RowMessage::warning(
                "PM_TASK_IMPORT_UNCHANGED",
                format!("Les tâches du plan « {code} » sont déjà celles du fichier : rien ne change."),
            ));
        } else if !current.is_empty() {
            out[first].messages.push(RowMessage::warning(
                "PM_TASK_IMPORT_REPLACES",
                format!(
                    "Les {} tâches actuelles de la version brouillon du plan « {code} » sont remplacées par les {} du fichier.",
                    current.len(),
                    steps.len()
                ),
            ));
        }
    }
    for o in &mut out {
        if has_error(&o.messages) {
            o.action = "skip";
        }
    }
    Ok(out)
}

/// A refusal of the version form, in plain French for the import report.
fn app_error_text(e: &AppError) -> String {
    let AppError::ValidationFailed(v) = e else {
        tracing::warn!("pm task import: plan refused: {e}");
        return "Enregistrement impossible : les tâches n'ont pas pu être enregistrées (détail dans le journal de l'application)."
            .to_string();
    };
    v.iter()
        .map(|msg| {
            let code = msg.split([':', ' ']).next().unwrap_or_default();
            match code {
                "GATE_PM_VERSION_NOT_DRAFT_EDIT" => "La version n'est plus un brouillon.".to_string(),
                "GATE_RECORD_CHANGED_ELSEWHERE" => {
                    "La version a été modifiée entre-temps : relancez l'import.".to_string()
                }
                "GATE_PM_METER_UNKNOWN" => "Le compteur du plan n'existe plus ou n'est plus actif.".to_string(),
                "GATE_PM_SKILL_UNKNOWN" => {
                    "Une compétence du plan n'est plus publiée dans les Données de référence.".to_string()
                }
                _ => format!("Tâches refusées par l'écran Préventif ({code})."),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn each(lines: &[(i64, i64, String)], status: &'static str, message: &str) -> Vec<RowApplied> {
    lines
        .iter()
        .map(|(id, _, _)| RowApplied {
            row_id: *id,
            status,
            message: message.to_string(),
        })
        .collect()
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow], user_id: i32) -> AppResult<Vec<RowApplied>> {
    // The same entitlement and licence checks as the version form, then the user's rights now.
    crate::entitlements::queries::enforce_capability_for_permission(db, PM_EDIT).await?;
    crate::license::queries::enforce_permission_matrix(db, user_id, PM_EDIT).await?;
    let access = access_of(db, i64::from(user_id)).await?;
    let mut out = Vec::new();
    // Plan code → its lines (row id, order, description), plans in order of first appearance.
    let mut groups: Vec<(String, Vec<(i64, i64, String)>)> = Vec::new();
    for row in rows {
        let plan = row.normalized["plan"].as_str().unwrap_or_default().to_string();
        let description = row.normalized["description"].as_str().unwrap_or_default().to_string();
        match row.normalized["order"].as_i64() {
            Some(order) if row.action == "update" && !plan.is_empty() && !description.is_empty() => {
                match groups.iter_mut().find(|(c, _)| *c == plan) {
                    Some((_, g)) => g.push((row.id, order, description)),
                    None => groups.push((plan, vec![(row.id, order, description)])),
                }
            }
            _ => out.push(RowApplied {
                row_id: row.id,
                status: "skipped",
                message: String::new(),
            }),
        }
    }
    for (code, mut lines) in groups {
        // Read the plan again: anything changed since the preview fails this plan only.
        let Some(plan) = plans(db).await?.remove(&code) else {
            out.extend(each(&lines, "failed", &format!("Le plan « {code} » n'existe plus.")));
            continue;
        };
        let version_id = match plan.version_id {
            Some(v) if plan.version_status.as_deref() == Some("draft") => v,
            _ => {
                out.extend(each(&lines, "failed", &not_draft(&code)));
                continue;
            }
        };
        if !may_edit(db, &access, version_id).await? {
            out.extend(each(&lines, "failed", &no_right(&code)));
            continue;
        }
        lines.sort_by_key(|(_, o, _)| *o);
        let steps: Vec<String> = lines.iter().map(|(_, _, d)| d.clone()).collect();
        if steps_of(plan.task_package_json.as_deref()) == steps {
            out.extend(each(&lines, "skipped", ""));
            continue;
        }
        let input = UpdatePmPlanVersionInput {
            effective_from: None,
            effective_to: None,
            trigger_definition_json: None,
            task_package_json: Some(with_steps(plan.task_package_json.as_deref(), &steps)),
            required_parts_json: None,
            required_skills_json: None,
            required_tools_json: None,
            estimated_duration_hours: None,
            planned_crew_size: None,
            clear_planned_crew_size: false,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        };
        match crate::pm::queries::update_pm_plan_version(db, version_id, plan.row_version, input).await {
            Ok(_) => out.extend(each(&lines, "updated", "")),
            Err(e) => out.extend(each(&lines, "failed", &format!("{code} : {}", app_error_text(&e)))),
        }
    }
    Ok(out)
}

/// The tasks of every plan's latest version, one line per task. Imported again, nothing changes.
pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let mut by_code: Vec<(String, Plan)> = plans(db).await?.into_iter().collect();
    by_code.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (code, plan) in by_code {
        for (i, step) in steps_of(plan.task_package_json.as_deref()).into_iter().enumerate() {
            out.push(vec![code.clone(), (i + 1).to_string(), step]);
        }
    }
    Ok(out)
}
