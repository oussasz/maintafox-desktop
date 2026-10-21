//! Content of the preventive work order (OT) created from a PM occurrence.
//!
//! The OT carries what the PM version prescribes, so the technician gets the job, not an empty
//! shell: readable title (`<plan title> — <asset code>`), task checklist, planned tools and parts,
//! expected duration and the permit flag. Text written into the OT (description section headings)
//! uses the tenant language, because the OT is shared data read by the whole plant.
//!
//! Mapping rules (PM version → OT):
//! - `task_package_json` (`{"steps": [...]}` or `[...]`, each step a string or
//!   `{"description", "is_mandatory"?, "estimated_minutes"?}`) → mandatory OT tasks, in order
//!   (a step written as plain text is a required step of the routine) + numbered list in the description.
//! - `required_tools_json` (array of labels) → planned OT tools + list in the description.
//! - `required_parts_json`: an entry `{"article_code", "quantity"}` whose code is an active article
//!   and whose quantity is > 0 → planned OT part; every other entry (plain code, unknown article,
//!   no quantity) is only listed in the description, because no quantity can be planned for it.
//! - `estimated_duration_hours` → `expected_duration_hours`; `planned_crew_size` → the OT's crew
//!   size; plan `requires_permit` → OT permit gate;
//!   plan `requires_shutdown` → the OT's own "needs shutdown" flag (`work_orders.requires_shutdown`,
//!   so planning checks the job against the shutdown windows as for any flagged OT) and a line in
//!   the description for the technician.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::Value;

use crate::errors::{AppError, AppResult};

/// One OT task derived from a PM step.
#[derive(Debug, Clone, PartialEq)]
pub struct PmWoTask {
    pub description: String,
    pub is_mandatory: bool,
    pub estimated_minutes: Option<i64>,
}

/// One entry of `required_parts_json`.
#[derive(Debug, Clone, PartialEq)]
pub struct PmRequiredPart {
    /// Article code, or the free text written on the PM version.
    pub code: String,
    pub quantity: Option<f64>,
    /// Required for the job (short: blocks the plan) or optional (short: warns). Migration 231
    /// set `"required": true` on every stored entry; an entry received from a device that has not
    /// run it is read as required, which is what it meant there.
    pub required: bool,
}

/// A required part resolved against the article master.
#[derive(Debug, Clone, PartialEq)]
pub struct PmWoPartLine {
    pub code: String,
    pub quantity: Option<f64>,
    pub article_id: Option<i64>,
    pub article_name: Option<String>,
    pub required: bool,
}

impl PmWoPartLine {
    /// The line becomes a planned OT part only with a known article and a quantity.
    pub fn plannable(&self) -> Option<(i64, f64)> {
        match (self.article_id, self.quantity) {
            (Some(article_id), Some(quantity)) if quantity > 0.0 => Some((article_id, quantity)),
            _ => None,
        }
    }
}

/// Everything the generated OT receives from the plan and its version.
#[derive(Debug, Clone)]
pub struct PmWoContent {
    pub title: String,
    pub description: Option<String>,
    pub expected_duration_hours: Option<f64>,
    pub planned_crew_size: Option<i64>,
    pub requires_permit: bool,
    /// The plan's "needs shutdown": the generated OT carries the same flag.
    pub requires_shutdown: bool,
    pub equipment_id: Option<i64>,
    pub tasks: Vec<PmWoTask>,
    pub tools: Vec<String>,
    pub parts: Vec<PmWoPartLine>,
}

struct Headings {
    tasks: &'static str,
    parts: &'static str,
    tools: &'static str,
    skills: &'static str,
    shutdown: &'static str,
}

fn headings(locale: &str) -> Headings {
    if locale.eq_ignore_ascii_case("en") {
        Headings {
            tasks: "Tasks",
            parts: "Required parts",
            tools: "Required tools",
            skills: "Required skills",
            shutdown: "Equipment shutdown required.",
        }
    } else {
        Headings {
            tasks: "Tâches",
            parts: "Pièces requises",
            tools: "Outillage requis",
            skills: "Compétences requises",
            shutdown: "Arrêt de l'équipement requis.",
        }
    }
}

fn step_text(obj: &serde_json::Map<String, Value>) -> Option<String> {
    ["description", "label", "text", "title"]
        .iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parses `task_package_json` into OT tasks. Text that is not JSON is read as one step per line.
pub fn parse_task_package(raw: Option<&str>) -> Vec<PmWoTask> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let parsed = match serde_json::from_str::<Value>(raw) {
        Ok(v) => v,
        Err(_) => {
            return raw
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| PmWoTask {
                    description: l.to_string(),
                    is_mandatory: true,
                    estimated_minutes: None,
                })
                .collect();
        }
    };
    let steps = match &parsed {
        Value::Array(items) => items.clone(),
        Value::Object(obj) => obj.get("steps").and_then(Value::as_array).cloned().unwrap_or_default(),
        Value::String(s) => vec![Value::String(s.clone())],
        _ => Vec::new(),
    };
    steps
        .iter()
        .filter_map(|step| match step {
            Value::String(s) => {
                let s = s.trim();
                (!s.is_empty()).then(|| PmWoTask {
                    description: s.to_string(),
                    is_mandatory: true,
                    estimated_minutes: None,
                })
            }
            Value::Object(obj) => step_text(obj).map(|description| PmWoTask {
                description,
                is_mandatory: obj
                    .get("is_mandatory")
                    .or_else(|| obj.get("mandatory"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                estimated_minutes: obj.get("estimated_minutes").and_then(Value::as_i64).filter(|m| *m > 0),
            }),
            _ => None,
        })
        .collect()
}

/// Parses a JSON array of labels (`required_tools_json`); text that is not JSON is one label.
pub fn parse_label_list(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Object(obj) => {
                    step_text(obj).or_else(|| obj.get("code").and_then(Value::as_str).map(|s| s.trim().to_string()))
                }
                _ => None,
            })
            .filter(|s| !s.is_empty())
            .collect(),
        Ok(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        Ok(_) => Vec::new(),
        Err(_) => vec![raw.to_string()],
    }
}

/// Parses `required_parts_json`: plain codes, or `{"article_code"|"code", "quantity"|"qty"}`.
pub fn parse_required_parts(raw: Option<&str>) -> Vec<PmRequiredPart> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let items = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(items)) => items,
        Ok(Value::String(s)) => vec![Value::String(s)],
        Ok(_) => Vec::new(),
        Err(_) => vec![Value::String(raw.to_string())],
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(s) => {
                let s = s.trim();
                (!s.is_empty()).then(|| PmRequiredPart {
                    code: s.to_string(),
                    quantity: None,
                    required: true,
                })
            }
            Value::Object(obj) => {
                let code = ["article_code", "code"]
                    .iter()
                    .find_map(|k| obj.get(*k).and_then(Value::as_str))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())?;
                let quantity = ["quantity", "qty"]
                    .iter()
                    .find_map(|k| obj.get(*k).and_then(Value::as_f64))
                    .filter(|q| *q > 0.0);
                let required = obj.get("required").and_then(Value::as_bool).unwrap_or(true);
                Some(PmRequiredPart {
                    code,
                    quantity,
                    required,
                })
            }
            _ => None,
        })
        .collect()
}

fn format_quantity(q: f64) -> String {
    if (q.fract()).abs() < f64::EPSILON {
        format!("{}", q as i64)
    } else {
        format!("{q}")
    }
}

/// Builds the OT description: plan description, task list, parts, tools, shutdown flag.
pub fn compose_description(
    locale: &str,
    plan_description: Option<&str>,
    tasks: &[PmWoTask],
    parts: &[PmWoPartLine],
    tools: &[String],
    skills: &[String],
    requires_shutdown: bool,
) -> Option<String> {
    let h = headings(locale);
    let mut sections: Vec<String> = Vec::new();
    if let Some(d) = plan_description.map(str::trim).filter(|d| !d.is_empty()) {
        sections.push(d.to_string());
    }
    if requires_shutdown {
        sections.push(h.shutdown.to_string());
    }
    if !tasks.is_empty() {
        let lines: Vec<String> = tasks
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}. {}", i + 1, t.description))
            .collect();
        sections.push(format!("{}{}\n{}", h.tasks, sep(locale), lines.join("\n")));
    }
    if !parts.is_empty() {
        let lines: Vec<String> = parts
            .iter()
            .map(|p| {
                let mut line = match &p.article_name {
                    Some(name) => format!("- {} — {}", p.code, name),
                    None => format!("- {}", p.code),
                };
                if let Some(q) = p.quantity {
                    line.push_str(&format!(" × {}", format_quantity(q)));
                }
                line
            })
            .collect();
        sections.push(format!("{}{}\n{}", h.parts, sep(locale), lines.join("\n")));
    }
    if !tools.is_empty() {
        let lines: Vec<String> = tools.iter().map(|t| format!("- {t}")).collect();
        sections.push(format!("{}{}\n{}", h.tools, sep(locale), lines.join("\n")));
    }
    if !skills.is_empty() {
        let lines: Vec<String> = skills.iter().map(|t| format!("- {t}")).collect();
        sections.push(format!("{}{}\n{}", h.skills, sep(locale), lines.join("\n")));
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

/// French typography puts a space before the colon; English does not.
fn sep(locale: &str) -> &'static str {
    if locale.eq_ignore_ascii_case("en") {
        ":"
    } else {
        " :"
    }
}

/// `<plan title> — <asset code>`; the plan title alone when the plan has no single asset.
pub fn compose_title(plan_title: &str, asset_code: Option<&str>) -> String {
    let title = plan_title.trim();
    match asset_code.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) => format!("{title} — {code}"),
        None => title.to_string(),
    }
}

/// Language of text written into shared records: the tenant language, else the active one.
pub async fn tenant_text_locale(db: &DatabaseConnection) -> AppResult<String> {
    let pref = crate::locale::resolve_locale_preference(db).await?;
    Ok(pref.tenant_locale.unwrap_or(pref.active_locale))
}

/// Labels of the required skill codes (PERSONNEL.SKILLS), the code when it has no label.
async fn skill_labels(db: &impl ConnectionTrait, codes: Vec<String>) -> AppResult<Vec<String>> {
    let mut out = Vec::with_capacity(codes.len());
    for code in codes {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rv.label FROM reference_values rv
                 JOIN reference_sets rs ON rs.id = rv.set_id
                 JOIN reference_domains rd ON rd.id = rs.domain_id
                 WHERE rd.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.code = ?
                 LIMIT 1",
                [code.clone().into()],
            ))
            .await?;
        out.push(match row {
            Some(r) => r.try_get::<String>("", "label")?,
            None => code,
        });
    }
    Ok(out)
}

/// Stock location holding enough available stock for the whole quantity (most stock first), so
/// the planned part is reserved there; `None` = no location covers it (no reservation; the
/// shortage stays visible on the OT and in planning readiness).
async fn reservable_location(db: &impl ConnectionTrait, article_id: i64, quantity: f64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT location_id FROM stock_balances
             WHERE article_id = ? AND on_hand_qty - reserved_qty >= ?
             ORDER BY on_hand_qty - reserved_qty DESC, location_id ASC LIMIT 1",
            [article_id.into(), quantity.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => Some(r.try_get("", "location_id")?),
        None => None,
    })
}

async fn resolve_part_lines(db: &impl ConnectionTrait, parts: Vec<PmRequiredPart>) -> AppResult<Vec<PmWoPartLine>> {
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, article_name FROM articles
                 WHERE UPPER(TRIM(article_code)) = UPPER(TRIM(?)) AND is_active = 1
                 LIMIT 1",
                [part.code.clone().into()],
            ))
            .await?;
        let (article_id, article_name) = match row {
            Some(r) => (
                Some(r.try_get::<i64>("", "id")?),
                Some(r.try_get::<String>("", "article_name")?),
            ),
            None => (None, None),
        };
        out.push(PmWoPartLine {
            code: part.code,
            quantity: part.quantity,
            article_id,
            article_name,
            required: part.required,
        });
    }
    Ok(out)
}

/// Reads the plan and the occurrence's version and builds the OT content.
/// `occurrence_equipment_id`: the occurrence's own asset (fanned-out plans); the plan's asset
/// is used otherwise.
pub async fn build_pm_work_order_content(
    db: &DatabaseConnection,
    pm_plan_id: i64,
    plan_version_id: i64,
    occurrence_equipment_id: Option<i64>,
) -> AppResult<PmWoContent> {
    let plan = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.title, p.description,
                    COALESCE(?, CASE WHEN p.asset_scope_type = 'equipment' THEN p.asset_scope_id END) AS asset_scope_id,
                    p.requires_permit, p.requires_shutdown, eq.asset_id_code AS asset_code
             FROM pm_plans p
             LEFT JOIN equipment eq ON eq.id =
                 COALESCE(?, CASE WHEN p.asset_scope_type = 'equipment' THEN p.asset_scope_id END)
             WHERE p.id = ?",
            [
                occurrence_equipment_id.into(),
                occurrence_equipment_id.into(),
                pm_plan_id.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan".to_string(),
            id: pm_plan_id.to_string(),
        })?;
    let version = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT task_package_json, required_parts_json, required_skills_json, required_tools_json,
                    estimated_duration_hours, planned_crew_size
             FROM pm_plan_versions WHERE id = ?",
            [plan_version_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan_version".to_string(),
            id: plan_version_id.to_string(),
        })?;

    let plan_title: String = plan.try_get("", "title")?;
    let plan_description: Option<String> = plan.try_get("", "description")?;
    let asset_scope_id: Option<i64> = plan.try_get("", "asset_scope_id")?;
    let requires_permit: i64 = plan.try_get("", "requires_permit")?;
    let requires_shutdown: i64 = plan.try_get("", "requires_shutdown")?;
    let asset_code: Option<String> = plan.try_get("", "asset_code")?;

    let task_package_json: Option<String> = version.try_get("", "task_package_json")?;
    let required_parts_json: Option<String> = version.try_get("", "required_parts_json")?;
    let required_tools_json: Option<String> = version.try_get("", "required_tools_json")?;
    let required_skills_json: Option<String> = version.try_get("", "required_skills_json")?;
    let estimated_duration_hours: Option<f64> = version.try_get("", "estimated_duration_hours")?;
    let planned_crew_size: Option<i64> = version.try_get("", "planned_crew_size")?;

    let tasks = parse_task_package(task_package_json.as_deref());
    let tools = parse_label_list(required_tools_json.as_deref());
    let skills = skill_labels(db, parse_label_list(required_skills_json.as_deref())).await?;
    let parts = resolve_part_lines(db, parse_required_parts(required_parts_json.as_deref())).await?;
    let locale = tenant_text_locale(db).await?;

    Ok(PmWoContent {
        title: compose_title(&plan_title, asset_code.as_deref()),
        description: compose_description(
            &locale,
            plan_description.as_deref(),
            &tasks,
            &parts,
            &tools,
            &skills,
            requires_shutdown == 1,
        ),
        expected_duration_hours: estimated_duration_hours.filter(|h| *h > 0.0),
        planned_crew_size: planned_crew_size.filter(|n| *n >= 1),
        requires_permit: requires_permit == 1,
        requires_shutdown: requires_shutdown == 1,
        equipment_id: asset_scope_id,
        tasks,
        tools,
        parts,
    })
}

/// Adds the crew size, the "needs shutdown" flag, the tasks, tools and plannable parts to the
/// freshly created (draft) OT, through the WO module's own functions so the OT execution log and
/// reservations behave as for a planner entry.
pub async fn apply_pm_content_to_work_order(
    db: &DatabaseConnection,
    wo_id: i64,
    content: &PmWoContent,
) -> AppResult<()> {
    if let Some(crew) = content.planned_crew_size {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET planned_crew_size = ?, row_version = row_version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            [crew.into(), wo_id.into()],
        ))
        .await?;
    }
    if content.requires_shutdown {
        // The OT was just created as a draft by this generation: the flag is part of its content,
        // as the crew size above (a planner changes it later through
        // `wo::workflow::actions::schedule::save_shutdown_requirement`).
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET requires_shutdown = 1, row_version = row_version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;
    }
    for (idx, task) in content.tasks.iter().enumerate() {
        crate::wo::tasks::add_task(
            db,
            crate::wo::tasks::AddTaskInput {
                wo_id,
                task_description: task.description.clone(),
                sequence_order: i64::try_from(idx).unwrap_or(i64::MAX) + 1,
                is_mandatory: task.is_mandatory,
                estimated_minutes: task.estimated_minutes,
                origin: None,
            },
        )
        .await?;
    }
    for tool in &content.tools {
        crate::wo::tools::add_tool(
            db,
            crate::wo::tools::AddToolInput {
                wo_id,
                tool_label: tool.clone(),
                tool_code: None,
                origin: None,
                notes: None,
            },
        )
        .await?;
    }
    for part in &content.parts {
        if let Some((article_id, quantity)) = part.plannable() {
            let stock_location_id = reservable_location(db, article_id, quantity).await?;
            crate::wo::parts::add_planned_part(
                db,
                crate::wo::parts::AddPartInput {
                    wo_id,
                    article_id: Some(article_id),
                    article_ref: Some(part.code.clone()),
                    quantity_planned: quantity,
                    unit_cost: None,
                    // With a location, `add_planned_part` reserves the stock (inventory reservation).
                    stock_location_id,
                    auto_reserve: Some(stock_location_id.is_some()),
                    notes: None,
                    origin: None,
                    is_required: part.required,
                },
            )
            .await?;
        }
    }
    Ok(())
}

/// Stable code of a PM part line without its required / optional flag.
pub const CODE_PART_REQUIRED_FLAG_MISSING: &str = "PM_PART_REQUIRED_FLAG_MISSING";

/// The code of an entry as `parse_required_parts` reads it (`article_code` or `code`).
fn entry_code(obj: &serde_json::Map<String, Value>) -> Option<String> {
    ["article_code", "code"]
        .iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A version's `required_parts_json` as the PM screens write it. It accepts exactly what
/// [`parse_required_parts`] reads, plus the flag rule: every part entry (an object with a code)
/// says whether it is `required` (boolean, no default). Free text that is not JSON (an old
/// version's note) is read as one part and accepted as stored; a JSON list may not hold bare
/// strings, which cannot carry the flag.
pub fn validate_required_parts_json(raw: Option<&str>) -> crate::errors::AppResult<()> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let refused = |code: String| {
        crate::errors::org_validation_failed(
            CODE_PART_REQUIRED_FLAG_MISSING,
            format!("Part line '{code}' must say whether it is required or optional."),
            std::collections::HashMap::from([("code".to_string(), code)]),
        )
    };
    let items = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(items)) => items,
        Ok(Value::String(s)) => return Err(refused(s)),
        // Not a list (an object, a number): the reader finds no part in it.
        Ok(_) => return Ok(()),
        // Free text: the reader keeps it as one part (stored by an old version).
        Err(_) => return Ok(()),
    };
    for item in items {
        match &item {
            Value::String(s) if !s.trim().is_empty() => return Err(refused(s.trim().to_string())),
            Value::Object(obj) => {
                // An object without a code is no part for the reader either.
                if let Some(code) = entry_code(obj) {
                    if !obj.get("required").is_some_and(Value::is_boolean) {
                        return Err(refused(code));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Copied part lines (a new draft from a published version): every entry gets the flag it is read
/// with — a flag-less entry (stored before migration 231 or received from a PC that has not run it)
/// is required, a bare code becomes `{"code", "required": true}`; free text stays as stored. The
/// result always passes [`validate_required_parts_json`] and reads the same parts.
pub fn normalized_required_parts_json(raw: Option<&str>) -> Option<String> {
    let text = raw?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(text.to_string());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::String(s)) => Some(serde_json::json!([{ "code": s.trim(), "required": true }]).to_string()),
        Ok(Value::Array(items)) => Some(
            Value::Array(
                items
                    .into_iter()
                    .map(|item| match item {
                        Value::String(s) => serde_json::json!({ "code": s.trim(), "required": true }),
                        Value::Object(mut obj) => {
                            if entry_code(&obj).is_some() && !obj.get("required").is_some_and(Value::is_boolean) {
                                obj.insert("required".into(), Value::Bool(true));
                            }
                            Value::Object(obj)
                        }
                        other => other,
                    })
                    .collect(),
            )
            .to_string(),
        ),
        _ => Some(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_package_accepts_steps_object_array_and_plain_text() {
        let from_steps = parse_task_package(Some(r#"{"steps":["Graisser palier","  ","Contrôler courroie"]}"#));
        assert_eq!(
            from_steps.iter().map(|t| t.description.as_str()).collect::<Vec<_>>(),
            vec!["Graisser palier", "Contrôler courroie"]
        );
        assert!(from_steps.iter().all(|t| t.is_mandatory));

        let objects = parse_task_package(Some(
            r#"[{"description":"Mesurer vibrations","is_mandatory":false,"estimated_minutes":15}]"#,
        ));
        assert_eq!(objects[0].estimated_minutes, Some(15));
        assert!(!objects[0].is_mandatory);

        let text = parse_task_package(Some("Étape 1\nÉtape 2\n"));
        assert_eq!(text.len(), 2);
        assert!(parse_task_package(None).is_empty());
    }

    #[test]
    fn required_parts_keep_quantity_only_when_given() {
        let parts = parse_required_parts(Some(
            r#"["FLT-01", {"article_code":"BRG-6204","quantity":2}, {"code":"X"}]"#,
        ));
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].quantity, None);
        assert_eq!(parts[1].code, "BRG-6204");
        assert_eq!(parts[1].quantity, Some(2.0));
        assert_eq!(parts[2].quantity, None);
    }

    #[test]
    fn title_has_no_raw_ids() {
        assert_eq!(
            compose_title("Graissage pompe", Some("P-101")),
            "Graissage pompe — P-101"
        );
        assert_eq!(compose_title("Graissage pompe", None), "Graissage pompe");
    }

    #[test]
    fn description_lists_tasks_parts_tools_in_tenant_language() {
        let tasks = parse_task_package(Some(r#"["Graisser","Contrôler"]"#));
        let parts = vec![PmWoPartLine {
            code: "BRG-6204".into(),
            quantity: Some(2.0),
            article_id: Some(1),
            article_name: Some("Roulement".into()),
            required: true,
        }];
        let fr = compose_description(
            "fr",
            Some("Routine mensuelle"),
            &tasks,
            &parts,
            &["Clé dynamométrique".into()],
            &["Mécanicien".into()],
            true,
        )
        .expect("description");
        assert!(fr.starts_with("Routine mensuelle"));
        assert!(fr.contains("Tâches :\n1. Graisser\n2. Contrôler"));
        assert!(fr.contains("- BRG-6204 — Roulement × 2"));
        assert!(fr.contains("Outillage requis :\n- Clé dynamométrique"));
        assert!(fr.contains("Arrêt de l'équipement requis."));
        let en = compose_description("en", None, &tasks, &[], &[], &[], false).expect("description");
        assert_eq!(en, "Tasks:\n1. Graisser\n2. Contrôler");
        assert!(compose_description("fr", None, &[], &[], &[], &[], false).is_none());
    }
}
