//! Skill import ("Compétences"): a person's skills from the PERSONNEL.SKILLS list, with a level.
//!
//! Skills come in unvalidated, with source "import": they count for no qualification requirement
//! until a trn.certify holder validates them. An existing skill (same person, same skill code)
//! only gets its level changed; a changed level drops an earlier validation, an unchanged one keeps
//! it. Nothing is ever deleted.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::key;
use super::team::app_error_text;
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};

pub const SPEC: EntitySpec = EntitySpec {
    code: "personnel_skill",
    label: "Compétences du personnel",
    order: 3,
    needs: "Personnel, et la liste de référence PERSONNEL.SKILLS (compétences).",
    permission: crate::rbac::permissions::PER_MANAGE,
    columns: &[
        Column {
            key: "employee_code",
            header: "matricule",
            required: true,
            aliases: &["employee_code", "matricule_personnel", "code_personnel"],
            hint: "Matricule d'une personne du registre du personnel.",
        },
        Column {
            key: "skill_code",
            header: "code compétence",
            required: true,
            aliases: &["competence", "code_competence", "skill", "skill_code"],
            hint: "Code d'une valeur publiée de la liste de référence PERSONNEL.SKILLS.",
        },
        Column {
            key: "level",
            header: "niveau",
            required: false,
            aliases: &["niveau_1_5", "level"],
            hint: "1 Notions, 2 Avec assistance, 3 Autonome, 4 Confirmé, 5 Expert / formateur. Obligatoire pour une nouvelle compétence.",
        },
    ],
};

/// Published, active skills of the PERSONNEL.SKILLS list by code key (newest list version first).
/// Codes given twice in the same list version, letter case aside, are returned apart: the file
/// cannot tell them apart.
async fn skill_catalogue(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, i64>, HashSet<String>)> {
    let mut out: HashMap<String, i64> = HashMap::new();
    let mut version_of: HashMap<String, i64> = HashMap::new();
    let mut ambiguous: HashSet<String> = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code, rs.version_no FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = 'PERSONNEL.SKILLS' AND rv.is_active = 1 \
             ORDER BY rs.version_no DESC, rv.id"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let version: i64 = r.try_get("", "version_no")?;
        if version_of.get(&key(&code)) == Some(&version) {
            ambiguous.insert(key(&code));
        }
        version_of.entry(key(&code)).or_insert(version);
        out.entry(key(&code)).or_insert(r.try_get("", "id")?);
    }
    Ok((out, ambiguous))
}

/// People by personnel number key; the keys held by several people are returned apart.
pub(super) async fn people_by_code(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, i64>, HashSet<String>)> {
    let mut people = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, employee_code FROM personnel".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "employee_code")?;
        if people.insert(key(&code), r.try_get("", "id")?).is_some() {
            ambiguous.insert(key(&code));
        }
    }
    Ok((people, ambiguous))
}

/// The level: a number from 1 to 5, alone or followed by its label ("3 — Autonome").
fn parse_level(raw: &str) -> Option<i64> {
    let digits: String = raw.trim().chars().take_while(char::is_ascii_digit).collect();
    let rest = raw.trim()[digits.len()..].trim_start();
    if !rest.is_empty() && !rest.starts_with(['—', '-', '–', ' ']) {
        return None;
    }
    digits.parse::<i64>().ok().filter(|l| (1..=5).contains(l))
}

/// The person cell, resolved; a refusal is added to `messages`.
pub(super) fn resolve_person(
    cell: Option<&str>,
    people: &HashMap<String, i64>,
    ambiguous: &HashSet<String>,
    prefix: &str,
    messages: &mut Vec<RowMessage>,
) -> Option<i64> {
    match cell {
        None => {
            messages.push(RowMessage::error(
                &format!("{prefix}_PERSON"),
                "Le matricule est obligatoire.",
            ));
            None
        }
        Some(m) if ambiguous.contains(&key(m)) => {
            messages.push(RowMessage::error(
                &format!("{prefix}_PERSON_AMBIGUOUS"),
                format!("Plusieurs personnes ont le matricule « {m} » à la casse près : corrigez-les dans Personnel."),
            ));
            None
        }
        Some(m) => {
            let found = people.get(&key(m)).copied();
            if found.is_none() {
                messages.push(RowMessage::error(
                    &format!("{prefix}_PERSON"),
                    format!("Le matricule « {m} » n'existe pas dans le personnel : importez d'abord le personnel."),
                ));
            }
            found
        }
    }
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let (people, ambiguous_people) = people_by_code(db).await?;
    let (catalogue, ambiguous_skills) = skill_catalogue(db).await?;

    // A person's existing skills by skill code key (whatever list version they point to):
    // (row id, level).
    // A person holding the same code twice (two list versions) cannot be updated from a file line.
    let mut held: HashMap<(i64, String), (i64, i64)> = HashMap::new();
    let mut held_twice: HashSet<(i64, String)> = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT ps.id, ps.personnel_id, ps.proficiency_level, rv.code \
             FROM personnel_skills ps JOIN reference_values rv ON rv.id = ps.reference_value_id"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let k = (r.try_get("", "personnel_id")?, key(&code));
        if held
            .insert(k.clone(), (r.try_get("", "id")?, r.try_get("", "proficiency_level")?))
            .is_some()
        {
            held_twice.insert(k);
        }
    }

    let mut first_row_of: HashMap<(i64, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let cell = |k: &str| raw.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
        let person = resolve_person(
            cell("employee_code"),
            &people,
            &ambiguous_people,
            "SKILL_IMPORT",
            &mut messages,
        );
        let skill_key = cell("skill_code").map(key);
        let catalogue_id = skill_key.as_ref().and_then(|k| catalogue.get(k).copied());
        let stored = person
            .zip(skill_key.clone())
            .and_then(|(p, k)| held.get(&(p, k)).copied());
        if let (Some(p), Some(k)) = (person, skill_key.clone()) {
            if held_twice.contains(&(p, k.clone())) {
                messages.push(RowMessage::error(
                    "SKILL_IMPORT_HELD_TWICE",
                    "Cette personne a cette compétence deux fois (deux versions de la liste) : corrigez sa fiche dans Personnel.",
                ));
            } else if stored.is_none() && ambiguous_skills.contains(&k) {
                messages.push(RowMessage::error(
                    "SKILL_IMPORT_SKILL_AMBIGUOUS",
                    "Plusieurs compétences de la liste ont ce code à la casse près : corrigez la liste dans Données de référence.",
                ));
            }
        }
        match (cell("skill_code"), catalogue_id, stored) {
            (None, _, _) => messages.push(RowMessage::error(
                "SKILL_IMPORT_SKILL",
                "Le code compétence est obligatoire.",
            )),
            // A skill the person already holds stays readable even if the list no longer offers it.
            (Some(_), _, Some(_)) | (Some(_), Some(_), None) => {}
            (Some(s), None, None) => messages.push(RowMessage::error(
                "SKILL_IMPORT_SKILL",
                format!(
                    "La compétence « {s} » n'existe pas dans la liste de référence PERSONNEL.SKILLS : ajoutez-la d'abord dans Données de référence."
                ),
            )),
        }
        let level = match cell("level") {
            None => None,
            Some(l) => {
                let parsed = parse_level(l);
                if parsed.is_none() {
                    messages.push(RowMessage::error(
                        "SKILL_IMPORT_LEVEL",
                        format!("Le niveau « {l} » doit être un nombre de 1 à 5."),
                    ));
                }
                parsed
            }
        };
        if let (Some(p), Some(k)) = (person, skill_key) {
            let k = (p, k);
            if let Some(first) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "SKILL_IMPORT_DUPLICATE",
                    format!(
                        "Cette compétence apparaît déjà pour cette personne à la ligne {} du fichier.",
                        first + 2
                    ),
                ));
            } else {
                first_row_of.insert(k, i);
            }
        }
        if stored.is_none() && level.is_none() && !messages.iter().any(|m| m.severity == "error") {
            messages.push(RowMessage::error(
                "SKILL_IMPORT_LEVEL",
                "Le niveau est obligatoire pour une nouvelle compétence.",
            ));
        }

        let has_error = messages.iter().any(|m| m.severity == "error");
        let action = match (has_error, stored, level) {
            (true, _, _) => "skip",
            (false, None, _) => "create",
            (false, Some((_, old)), Some(new)) if new != old => "update",
            _ => "skip",
        };
        if action == "update" {
            messages.push(RowMessage::warning(
                "SKILL_IMPORT_REVALIDATE",
                "Le niveau change : la compétence devra être validée à nouveau.",
            ));
        }
        out.push(RowOutcome {
            action,
            normalized: json!({
                "personnel_id": person,
                "reference_value_id": catalogue_id,
                "existing_id": stored.map(|(id, _)| id),
                "level": level,
            }),
            messages,
        });
    }
    Ok(out)
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        let n = &row.normalized;
        let existing = n["existing_id"].as_i64();
        let (status, message) = match (row.action.as_str(), n["personnel_id"].as_i64(), n["level"].as_i64()) {
            ("create" | "update", Some(p), Some(level)) => {
                let rv = n["reference_value_id"].as_i64();
                let written = match (existing, rv) {
                    (None, None) => Err(AppError::ValidationFailed(vec![
                        "GATE_BULK_IMPORT_SKILL_GONE: Cette compétence n'est plus dans la liste de référence."
                            .to_string(),
                    ])),
                    _ => {
                        crate::personnel::skills::record_imported_skill(db, p, rv.unwrap_or_default(), level, existing)
                            .await
                    }
                };
                match written {
                    Ok(()) => (if existing.is_some() { "updated" } else { "created" }, String::new()),
                    Err(AppError::Database(e)) => return Err(AppError::Database(e)),
                    Err(e) => ("failed", app_error_text(&e)),
                }
            }
            ("create" | "update", _, _) => ("failed", "Ligne incomplète.".to_string()),
            _ => ("skipped", "Déjà à jour.".to_string()),
        };
        out.push(RowApplied {
            row_id: row.id,
            status,
            message,
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.employee_code, rv.code, ps.proficiency_level \
             FROM personnel_skills ps \
             JOIN personnel p ON p.id = ps.personnel_id \
             JOIN reference_values rv ON rv.id = ps.reference_value_id \
             ORDER BY p.employee_code, rv.code"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let level: i64 = r.try_get("", "proficiency_level")?;
            Ok(vec![
                r.try_get("", "employee_code")?,
                r.try_get("", "code")?,
                level.to_string(),
            ])
        })
        .collect()
}
