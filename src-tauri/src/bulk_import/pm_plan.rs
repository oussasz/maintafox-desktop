//! Preventive plans (plans préventifs), keyed by plan code. One line = one plan on one equipment
//! with its first version, created as a DRAFT that a planner checks and publishes.
//!
//! Advisor rulings 2026-11-10 (supersede spec §6):
//! - create only: a plan code that already exists is skipped ("modifiez-le dans Préventif");
//! - trigger: calendar (interval + unit) or meter (a meter code of that equipment + interval),
//!   with an optional tolerance in days (the same before and after);
//! - "date de dernière réalisation" is the version's start date; blank = the import day, with a
//!   warning to check it before publishing; the import never generates work orders;
//! - "permis requis" and "arrêt requis" flags; optional team, required skills, duration, crew;
//! - the user needs `pm.create` at each equipment's site, checked line by line (the same rule as
//!   the plan screen), in the preview and again when the batch is applied.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};

use super::equipment_meter::{equipment_by_code, parse_number};
use super::org_tree::{key, parse_active};
use super::{parse_date, Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::auth::scope::AccessMap;
use crate::errors::{AppError, AppResult};
use crate::pm::domain::{CreatePmPlanInput, CreatePmPlanVersionInput};
use crate::rbac::permissions::PM_CREATE;

pub const SPEC: EntitySpec = EntitySpec {
    code: "pm_plan",
    label: "Plans préventifs",
    order: 7,
    needs: "Équipements (et leurs compteurs), équipes, compétences.",
    permission: PM_CREATE,
    columns: &[
        Column {
            key: "code",
            header: "code plan",
            required: true,
            aliases: &["plan", "code_plan", "plan_code"],
            hint: "Code du plan, unique. Il est enregistré en majuscules.",
        },
        Column {
            key: "title",
            header: "libellé",
            required: true,
            aliases: &["libelle", "titre", "title", "designation"],
            hint: "Nom du plan.",
        },
        Column {
            key: "equipment_code",
            header: "code équipement",
            required: true,
            aliases: &["equipement", "code_equipement", "equipment_code", "asset_code"],
            hint: "Équipement entretenu par le plan.",
        },
        Column {
            key: "trigger",
            header: "déclenchement",
            required: true,
            aliases: &["type_de_declenchement", "declenchement", "trigger"],
            hint: "calendrier ou compteur.",
        },
        Column {
            key: "interval",
            header: "intervalle",
            required: true,
            aliases: &["periodicite", "frequence", "interval"],
            hint: "Nombre entier plus grand que 0 : tous les N jours, semaines, mois ou ans, ou tous les N de compteur.",
        },
        Column {
            key: "unit",
            header: "unité",
            required: false,
            aliases: &["unite", "unit", "unite_intervalle"],
            hint: "Pour un plan calendrier : jours, semaines, mois ou ans.",
        },
        Column {
            key: "meter_code",
            header: "code compteur",
            required: false,
            aliases: &["compteur", "code_compteur", "meter_code"],
            hint: "Pour un plan compteur : code d'un compteur actif de cet équipement.",
        },
        Column {
            key: "tolerance",
            header: "tolérance (jours)",
            required: false,
            aliases: &["tolerance", "tolerance_jours"],
            hint: "Nombre de jours accepté avant et après l'échéance. Vide : aucune.",
        },
        Column {
            key: "last_done",
            header: "date de dernière réalisation",
            required: false,
            aliases: &["derniere_realisation", "date_derniere_realisation", "last_done"],
            hint: "JJ/MM/AAAA, pas dans le futur. Le calendrier part de cette date. Vide : le jour de l'import, à vérifier avant publication.",
        },
        Column {
            key: "team_code",
            header: "code équipe",
            required: false,
            aliases: &["equipe", "code_equipe", "team_code"],
            hint: "Équipe active chargée du plan.",
        },
        Column {
            key: "skills",
            header: "compétences requises",
            required: false,
            aliases: &["competences", "competences_requises", "skills"],
            hint: "Codes de compétences (Données de référence), séparés par des virgules.",
        },
        Column {
            key: "duration",
            header: "durée estimée (h)",
            required: false,
            aliases: &["duree", "duree_estimee", "duree_estimee_h", "duration"],
            hint: "En heures, 0 ou plus.",
        },
        Column {
            key: "crew",
            header: "nb intervenants",
            required: false,
            aliases: &["intervenants", "nb_intervenants", "crew"],
            hint: "Nombre entier de 1 à 50.",
        },
        Column {
            key: "permit",
            header: "permis requis",
            required: false,
            aliases: &["permis", "permis_requis", "requires_permit"],
            hint: "oui ou non. Vide : non.",
        },
        Column {
            key: "shutdown",
            header: "arrêt requis",
            required: false,
            aliases: &["arret", "arret_requis", "requires_shutdown"],
            hint: "oui ou non. Vide : non.",
        },
    ],
};

/// Largest crew on one job (`planning::load::MAX_CREW_SIZE`).
const MAX_CREW: i64 = 50;

fn cell(raw: &HashMap<String, String>, field: &str) -> String {
    raw.get(field).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn has_error(messages: &[RowMessage]) -> bool {
    messages.iter().any(|m| m.severity == "error")
}

fn err(messages: &mut Vec<RowMessage>, code: &str, text: impl Into<String>) {
    messages.push(RowMessage::error(code, text));
}

/// The rights of the user now (a fresh cache, so a role changed a moment ago counts).
async fn access_of(db: &DatabaseConnection, user_id: i64) -> AppResult<AccessMap> {
    let user_id = i32::try_from(user_id).map_err(|_| AppError::Internal(anyhow::anyhow!("user id out of range")))?;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    crate::auth::scope::access_map_cached(db, &cache, user_id).await
}

/// Whether the user may create a plan on this equipment (`pm.create` at its site).
async fn may_create_on(db: &DatabaseConnection, access: &AccessMap, equipment_id: i64) -> AppResult<bool> {
    match crate::pm::access::check_create_at(db, access, "equipment", Some(equipment_id)).await {
        Ok(_) => Ok(true),
        Err(AppError::PermissionDenied(_) | AppError::OrgValidationFailed(_) | AppError::ValidationFailed(_)) => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

fn no_right(equipment: &str) -> String {
    format!("Vous n'avez pas le droit de créer un plan préventif sur le site de l'équipement « {equipment} ».")
}

/// Existing plan codes (stored in upper case).
async fn plan_codes(db: &impl ConnectionTrait) -> AppResult<HashSet<String>> {
    let mut out = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT code FROM pm_plans".to_string(),
        ))
        .await?
    {
        out.insert(crate::pm::queries::normalize_plan_code(
            &r.try_get::<String>("", "code")?,
        ));
    }
    Ok(out)
}

/// Active meters by (equipment id, meter code key); a code held twice on one equipment lands in
/// `twice`.
async fn meters(db: &impl ConnectionTrait) -> AppResult<(HashMap<(i64, String), i64>, HashSet<(i64, String)>)> {
    let mut out = HashMap::new();
    let mut twice = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, equipment_id, meter_code FROM equipment_meters WHERE is_active = 1 AND meter_code IS NOT NULL"
                .to_string(),
        ))
        .await?
    {
        let k = (
            r.try_get::<i64>("", "equipment_id")?,
            key(&r.try_get::<String>("", "meter_code")?),
        );
        if out.insert(k.clone(), r.try_get::<i64>("", "id")?).is_some() {
            twice.insert(k);
        }
    }
    Ok((out, twice))
}

/// Active crews by code key; codes that differ only by letter case land in `twice`.
async fn teams(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, i64>, HashSet<String>)> {
    let mut out = HashMap::new();
    let mut twice = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code FROM teams WHERE deleted_at IS NULL AND status = 'active'".to_string(),
        ))
        .await?
    {
        let k = key(&r.try_get::<String>("", "code")?);
        if out.insert(k.clone(), r.try_get::<i64>("", "id")?).is_some() {
            twice.insert(k);
        }
    }
    Ok((out, twice))
}

/// Published, active skill codes by code key → the stored code (the plan screen checks the exact
/// code against the published PERSONNEL.SKILLS list).
async fn skills(db: &impl ConnectionTrait) -> AppResult<HashMap<String, String>> {
    let mut out = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.code FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rd.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.is_active = 1"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        out.entry(key(&code)).or_insert(code);
    }
    Ok(out)
}

/// Calendar unit of the file, as the trigger stores it.
fn calendar_unit(raw: &str) -> Option<&'static str> {
    match super::normalize_header(raw).as_str() {
        "jour" | "jours" | "j" | "day" | "days" => Some("day"),
        "semaine" | "semaines" | "sem" | "week" | "weeks" => Some("week"),
        "mois" | "month" | "months" => Some("month"),
        "an" | "ans" | "annee" | "annees" | "year" | "years" => Some("year"),
        _ => None,
    }
}

type CodeMap = (HashMap<String, i64>, HashSet<String>);

struct Lists {
    equipment: CodeMap,
    meters: (HashMap<(i64, String), i64>, HashSet<(i64, String)>),
    teams: CodeMap,
    skills: HashMap<String, String>,
}

async fn lists(db: &DatabaseConnection) -> AppResult<Lists> {
    Ok(Lists {
        equipment: equipment_by_code(db).await?,
        meters: meters(db).await?,
        teams: teams(db).await?,
        skills: skills(db).await?,
    })
}

/// What one line asks for, as the plan screen would save it.
struct ReadLine {
    equipment_id: i64,
    plan: CreatePmPlanInput,
    version: CreatePmPlanVersionInput,
}

/// Reads one line. Every problem is pushed to `messages`; `None` when the line cannot be saved.
fn read_line(raw: &HashMap<String, String>, l: &Lists, messages: &mut Vec<RowMessage>) -> Option<ReadLine> {
    let code = crate::pm::queries::normalize_plan_code(&cell(raw, "code"));
    let title = cell(raw, "title");
    let equipment_code = cell(raw, "equipment_code");
    if code.is_empty() {
        err(messages, "PM_IMPORT_CODE", "Le code plan est obligatoire.");
    }
    if title.is_empty() {
        err(messages, "PM_IMPORT_TITLE", "Le libellé est obligatoire.");
    }
    let equipment_id = if equipment_code.is_empty() {
        err(messages, "PM_IMPORT_EQUIPMENT", "Le code équipement est obligatoire.");
        None
    } else if l.equipment.1.contains(&key(&equipment_code)) {
        err(
            messages,
            "PM_IMPORT_EQUIPMENT_AMBIGUOUS",
            format!(
                "Plusieurs équipements ont le code « {equipment_code} » à la casse près : renommez-les avant d'importer."
            ),
        );
        None
    } else if let Some(id) = l.equipment.0.get(&key(&equipment_code)) {
        Some(*id)
    } else {
        err(
            messages,
            "PM_IMPORT_EQUIPMENT_UNKNOWN",
            format!("L'équipement « {equipment_code} » n'existe pas : importez d'abord les équipements."),
        );
        None
    };

    // Trigger.
    let interval = parse_number(&cell(raw, "interval")).filter(|v| *v > 0.0 && v.fract() == 0.0);
    if interval.is_none() {
        err(
            messages,
            "PM_IMPORT_INTERVAL",
            "L'intervalle doit être un nombre entier plus grand que 0.",
        );
    }
    let tolerance_cell = cell(raw, "tolerance");
    let tolerance = if tolerance_cell.is_empty() {
        None
    } else {
        let t = parse_number(&tolerance_cell).filter(|v| *v >= 0.0);
        if t.is_none() {
            err(
                messages,
                "PM_IMPORT_TOLERANCE",
                "La tolérance doit être un nombre de jours, 0 ou plus.",
            );
        }
        t
    };
    let (strategy, mut trigger) = match super::normalize_header(&cell(raw, "trigger")).as_str() {
        "calendrier" | "calendaire" | "calendar" | "date" => {
            let unit = calendar_unit(&cell(raw, "unit"));
            if unit.is_none() {
                err(
                    messages,
                    "PM_IMPORT_UNIT",
                    "Pour un plan calendrier, l'unité doit être jours, semaines, mois ou ans.",
                );
            }
            ("fixed", json!({ "interval_unit": unit, "interval_value": interval }))
        }
        "compteur" | "meter" => {
            let meter_code = cell(raw, "meter_code");
            let meter_id = match equipment_id {
                _ if meter_code.is_empty() => {
                    err(
                        messages,
                        "PM_IMPORT_METER",
                        "Pour un plan compteur, le code compteur est obligatoire.",
                    );
                    None
                }
                Some(eq) if l.meters.1.contains(&(eq, key(&meter_code))) => {
                    err(
                        messages,
                        "PM_IMPORT_METER_AMBIGUOUS",
                        format!("L'équipement a deux compteurs actifs « {meter_code} » : corrigez-les sur sa fiche."),
                    );
                    None
                }
                Some(eq) => {
                    let found = l.meters.0.get(&(eq, key(&meter_code))).copied();
                    if found.is_none() {
                        err(
                            messages,
                            "PM_IMPORT_METER_UNKNOWN",
                            format!("L'équipement « {equipment_code} » n'a pas de compteur actif « {meter_code} »."),
                        );
                    }
                    found
                }
                None => None,
            };
            (
                "meter",
                json!({ "asset_meter_id": meter_id, "interval_value": interval }),
            )
        }
        _ => {
            err(
                messages,
                "PM_IMPORT_TRIGGER",
                "Le déclenchement doit être calendrier ou compteur.",
            );
            ("fixed", json!({}))
        }
    };
    if let (Some(t), Some(obj)) = (tolerance.filter(|t| *t > 0.0), trigger.as_object_mut()) {
        obj.insert("tolerance".into(), json!({ "unit": "days", "early": t, "late": t }));
    }

    // Start date.
    let today = crate::inventory::fx::today();
    let effective_from = match parse_date(raw.get("last_done").map(String::as_str)) {
        Ok(Some(d)) if d.format("%Y-%m-%d").to_string() > today => {
            err(
                messages,
                "PM_IMPORT_LAST_DONE_FUTURE",
                "La date de dernière réalisation ne peut pas être dans le futur.",
            );
            None
        }
        Ok(Some(d)) => Some(d.format("%Y-%m-%d").to_string()),
        Ok(None) => {
            messages.push(RowMessage::warning(
                "PM_IMPORT_START_TODAY",
                "Sans date de dernière réalisation, le calendrier part du jour de l'import : vérifiez-le avant de publier le plan.",
            ));
            Some(today)
        }
        Err(()) => {
            err(
                messages,
                "PM_IMPORT_LAST_DONE",
                "La date de dernière réalisation doit être au format JJ/MM/AAAA.",
            );
            None
        }
    };

    // Team and resources.
    let team_code = cell(raw, "team_code");
    let team_id = if team_code.is_empty() {
        None
    } else if l.teams.1.contains(&key(&team_code)) {
        err(
            messages,
            "PM_IMPORT_TEAM_AMBIGUOUS",
            format!("Plusieurs équipes ont le code « {team_code} » à la casse près : renommez-les avant d'importer."),
        );
        None
    } else {
        let found = l.teams.0.get(&key(&team_code)).copied();
        if found.is_none() {
            err(
                messages,
                "PM_IMPORT_TEAM_UNKNOWN",
                format!("L'équipe « {team_code} » n'existe pas ou n'est pas active."),
            );
        }
        found
    };
    let mut skill_codes: Vec<String> = Vec::new();
    for s in cell(raw, "skills")
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match l.skills.get(&key(s)) {
            Some(code) if !skill_codes.contains(code) => skill_codes.push(code.clone()),
            Some(_) => {}
            None => err(
                messages,
                "PM_IMPORT_SKILL_UNKNOWN",
                format!("La compétence « {s} » n'est pas publiée dans les Données de référence (compétences)."),
            ),
        }
    }
    let duration_cell = cell(raw, "duration");
    let duration = if duration_cell.is_empty() {
        None
    } else {
        let d = parse_number(&duration_cell).filter(|v| *v >= 0.0);
        if d.is_none() {
            err(
                messages,
                "PM_IMPORT_DURATION",
                "La durée estimée doit être un nombre d'heures, 0 ou plus.",
            );
        }
        d
    };
    let crew_cell = cell(raw, "crew");
    let crew = if crew_cell.is_empty() {
        None
    } else {
        let c = parse_number(&crew_cell)
            .filter(|v| v.fract() == 0.0 && (1.0..=MAX_CREW as f64).contains(v))
            .map(|v| v as i64);
        if c.is_none() {
            err(
                messages,
                "PM_IMPORT_CREW",
                format!("Le nombre d'intervenants doit être un entier de 1 à {MAX_CREW}."),
            );
        }
        c
    };
    let mut flag = |field: &str, label: &str| {
        parse_active(raw.get(field)).unwrap_or_else(|()| {
            err(
                messages,
                "PM_IMPORT_FLAG",
                format!("La colonne « {label} » doit contenir oui ou non."),
            );
            None
        })
    };
    let permit = flag("permit", "permis requis").unwrap_or(false);
    let shutdown = flag("shutdown", "arrêt requis").unwrap_or(false);

    if has_error(messages) {
        return None;
    }
    let (equipment_id, effective_from) = (equipment_id?, effective_from?);
    Some(ReadLine {
        equipment_id,
        plan: CreatePmPlanInput {
            code,
            title,
            description: None,
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(equipment_id),
            strategy_type: strategy.to_string(),
            criticality_value_id: None,
            assigned_team_id: team_id,
            requires_shutdown: shutdown,
            requires_permit: permit,
            is_active: Some(true),
        },
        version: CreatePmPlanVersionInput {
            effective_from: format!("{effective_from}T00:00:00Z"),
            effective_to: None,
            trigger_definition_json: trigger.to_string(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: (!skill_codes.is_empty()).then(|| Value::from(skill_codes).to_string()),
            required_tools_json: None,
            estimated_duration_hours: duration,
            planned_crew_size: crew,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: Some("Import de données".to_string()),
        },
    })
}

pub async fn validate(
    db: &DatabaseConnection,
    rows: &[HashMap<String, String>],
    user_id: Option<i64>,
) -> AppResult<Vec<RowOutcome>> {
    let user_id =
        user_id.ok_or_else(|| AppError::Internal(anyhow::anyhow!("PM plan import needs the importing user")))?;
    let access = access_of(db, user_id).await?;
    let l = lists(db).await?;
    let existing = plan_codes(db).await?;
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let code = crate::pm::queries::normalize_plan_code(&cell(raw, "code"));
        let mut duplicate = false;
        if !code.is_empty() {
            if let Some(f) = first_row_of.get(&code) {
                err(
                    &mut messages,
                    "PM_IMPORT_DUPLICATE",
                    format!("Le plan « {code} » apparaît déjà à la ligne {} du fichier.", f + 2),
                );
                duplicate = true;
            } else {
                first_row_of.insert(code.clone(), i);
            }
        }
        if !duplicate && existing.contains(&code) {
            messages.push(RowMessage::warning(
                "PM_IMPORT_EXISTS",
                format!("Le plan « {code} » existe déjà : l'import ne le modifie pas, modifiez-le dans Préventif."),
            ));
            out.push(RowOutcome {
                action: "skip",
                normalized: json!({ "code": code }),
                messages,
            });
            continue;
        }
        let read = read_line(raw, &l, &mut messages);
        if let Some(line) = &read {
            if !may_create_on(db, &access, line.equipment_id).await? {
                err(
                    &mut messages,
                    "PM_IMPORT_SITE_RIGHT",
                    no_right(&cell(raw, "equipment_code")),
                );
            }
        }
        let ok = read.is_some() && !has_error(&messages);
        out.push(RowOutcome {
            action: if ok { "create" } else { "skip" },
            normalized: json!({ "code": code, "raw": raw }),
            messages,
        });
    }
    Ok(out)
}

/// A refusal of the plan screen, in plain French for the import report.
fn app_error_text(e: &AppError) -> String {
    let AppError::ValidationFailed(v) = e else {
        return format!("Enregistrement impossible : {e}");
    };
    v.iter()
        .map(|msg| {
            let code = msg.split([':', ' ']).next().unwrap_or_default();
            match code {
                "GATE_REPO_TEAM_NOT_FOUND" => "L'équipe n'existe plus ou n'est plus active.".to_string(),
                "GATE_PM_METER_UNKNOWN" => "Le compteur n'existe plus ou n'est plus actif.".to_string(),
                "GATE_PM_SKILL_UNKNOWN" => {
                    "Une compétence n'est plus publiée dans les Données de référence.".to_string()
                }
                _ => format!("Plan refusé par l'écran Préventif ({code})."),
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
    // The same entitlement and licence checks as the plan screen, then the user's rights now.
    crate::entitlements::queries::enforce_capability_for_permission(db, PM_CREATE).await?;
    crate::license::queries::enforce_permission_matrix(db, user_id, PM_CREATE).await?;
    let access = access_of(db, i64::from(user_id)).await?;
    let l = lists(db).await?;
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
        let raw: HashMap<String, String> = serde_json::from_value(row.normalized["raw"].clone()).unwrap_or_default();
        // Read the line again against today's lists: anything changed since the preview fails
        // this line only.
        let mut messages = Vec::new();
        let Some(line) = read_line(&raw, &l, &mut messages) else {
            let why = messages
                .into_iter()
                .filter(|m| m.severity == "error")
                .map(|m| m.message)
                .collect::<Vec<_>>()
                .join(" ");
            out.push(fail(row, why));
            continue;
        };
        if !may_create_on(db, &access, line.equipment_id).await? {
            out.push(fail(row, no_right(&cell(&raw, "equipment_code"))));
            continue;
        }
        if plan_codes(db).await?.contains(&line.plan.code) {
            out.push(fail(
                row,
                format!("Le plan « {} » a été créé entre-temps.", line.plan.code),
            ));
            continue;
        }
        let code = line.plan.code.clone();
        let tx = db.begin().await?;
        let created = async {
            let plan_id = crate::pm::queries::create_pm_plan_in(&tx, line.plan).await?;
            crate::pm::queries::create_pm_plan_version_in(&tx, plan_id, line.version).await?;
            Ok::<(), AppError>(())
        }
        .await;
        match created {
            Ok(()) => {
                tx.commit().await?;
                out.push(RowApplied {
                    row_id: row.id,
                    status: "created",
                    message: String::new(),
                });
            }
            Err(e) => {
                drop(tx);
                out.push(fail(row, format!("{code} : {}", app_error_text(&e))));
            }
        }
    }
    Ok(out)
}

/// The calendar and meter plans on one equipment, with their latest version, in the columns of
/// the import. Imported again, every line is skipped (the plans exist).
pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.code, p.title, e.asset_id_code, p.strategy_type, p.requires_permit, p.requires_shutdown, \
                    t.code AS team_code, v.trigger_definition_json, v.effective_from, v.required_skills_json, \
                    v.estimated_duration_hours, v.planned_crew_size, m.meter_code \
             FROM pm_plans p \
             JOIN equipment e ON e.id = p.asset_scope_id AND p.asset_scope_type = 'equipment' \
             LEFT JOIN teams t ON t.id = p.assigned_team_id \
             LEFT JOIN pm_plan_versions v \
               ON v.id = (SELECT MAX(v2.id) FROM pm_plan_versions v2 WHERE v2.pm_plan_id = p.id) \
             LEFT JOIN equipment_meters m ON m.id = json_extract(v.trigger_definition_json, '$.asset_meter_id') \
             WHERE p.strategy_type IN ('fixed', 'meter') AND e.asset_id_code IS NOT NULL \
             ORDER BY p.code"
                .to_string(),
        ))
        .await?;
    let yes_no = |v: i64| if v != 0 { "oui" } else { "non" }.to_string();
    let num = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
    rows.iter()
        .map(|r| {
            let trigger: Value = serde_json::from_str(
                &r.try_get::<Option<String>>("", "trigger_definition_json")?
                    .unwrap_or_default(),
            )
            .unwrap_or(Value::Null);
            let strategy: String = r.try_get("", "strategy_type")?;
            let unit = match trigger["interval_unit"].as_str().unwrap_or_default() {
                "day" | "days" => "jours",
                "week" | "weeks" => "semaines",
                "month" | "months" => "mois",
                "year" | "years" => "ans",
                _ => "",
            };
            let skills = code_list(r.try_get::<Option<String>>("", "required_skills_json")?.as_deref());
            let effective: Option<String> = r.try_get("", "effective_from")?;
            Ok(vec![
                r.try_get("", "code")?,
                r.try_get("", "title")?,
                r.try_get("", "asset_id_code")?,
                if strategy == "meter" { "compteur" } else { "calendrier" }.to_string(),
                num(trigger["interval_value"].as_f64()),
                if strategy == "meter" {
                    String::new()
                } else {
                    unit.to_string()
                },
                r.try_get::<Option<String>>("", "meter_code")?.unwrap_or_default(),
                num(trigger["tolerance"]["late"].as_f64()),
                effective
                    .and_then(|d| chrono::NaiveDate::parse_from_str(d.get(..10).unwrap_or_default(), "%Y-%m-%d").ok())
                    .map(|d| d.format("%d/%m/%Y").to_string())
                    .unwrap_or_default(),
                r.try_get::<Option<String>>("", "team_code")?.unwrap_or_default(),
                skills,
                num(r.try_get("", "estimated_duration_hours")?),
                r.try_get::<Option<i64>>("", "planned_crew_size")?
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
                yes_no(r.try_get("", "requires_permit")?),
                yes_no(r.try_get("", "requires_shutdown")?),
            ])
        })
        .collect()
}

fn code_list(raw: Option<&str>) -> String {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .map(|v| v.join(", "))
        .unwrap_or_default()
}
