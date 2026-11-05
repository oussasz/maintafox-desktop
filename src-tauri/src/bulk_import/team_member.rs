//! Crew membership import: who belongs to which crew, over which period.
//!
//! A line is matched to an existing membership of the same person and crew by its start date
//! (blank start: the membership without a start date, else the one still running). A matched
//! membership gets the line's end date; otherwise a new membership is created. A blank cell never
//! clears a value, and nothing is ever deleted. Two periods of the same person in the same crew
//! may not overlap.
//!
//! "équipe principale = oui" makes the crew the person's home crew, through the personnel rules
//! of the screen: the home crew needs a running period, and changing it ends the person's other
//! running memberships. "chef d'équipe" follows the crew screen: one lead per crew, on a running
//! membership; a new lead replaces the previous one.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::{key, parse_active};
use super::team::app_error_text;
use super::{parse_date, Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};

pub const SPEC: EntitySpec = EntitySpec {
    code: "team_member",
    label: "Membres d'équipe",
    order: 3,
    needs: "Personnel et Équipes.",
    permission: crate::rbac::permissions::PER_MANAGE,
    columns: &[
        Column {
            key: "employee_code",
            header: "matricule",
            required: true,
            aliases: &["employee_code", "matricule_personnel", "code_personnel"],
            hint: "Matricule d'une personne active du registre du personnel.",
        },
        Column {
            key: "team_code",
            header: "code équipe",
            required: true,
            aliases: &["equipe", "code_equipe", "team_code", "team"],
            hint: "Code d'une équipe active.",
        },
        Column {
            key: "valid_from",
            header: "date début",
            required: false,
            aliases: &["date_de_debut", "debut", "valid_from"],
            hint: "JJ/MM/AAAA. Sert aussi à retrouver une période existante.",
        },
        Column {
            key: "valid_to",
            header: "date fin",
            required: false,
            aliases: &["date_de_fin", "fin", "valid_to"],
            hint: "JJ/MM/AAAA. Vide : la période reste en cours (ou garde sa date de fin).",
        },
        Column {
            key: "home",
            header: "équipe principale",
            required: false,
            aliases: &["equipe_principale_oui_non", "principale", "home_crew"],
            hint: "oui : devient l'équipe principale de la personne (période en cours obligatoire). Ses autres équipes en cours sont alors clôturées.",
        },
        Column {
            key: "lead",
            header: "chef d'équipe",
            required: false,
            aliases: &["chef_d_equipe_oui_non", "chef", "chef_equipe", "lead"],
            hint: "oui ou non. Un seul chef par équipe, sur une période en cours ; le nouveau chef remplace l'ancien.",
        },
    ],
};

struct Membership {
    id: i64,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    is_lead: bool,
}

type Period = (Option<NaiveDate>, Option<NaiveDate>);

struct Line {
    person: Option<i64>,
    team: Option<i64>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    home: Option<bool>,
    lead: Option<bool>,
    /// Index, in the memberships of this person and crew, of the one the line updates.
    matched: Option<usize>,
    /// What the line changes on that membership (all true / set for a new one).
    to_change: bool,
    home_change: bool,
    lead_change: Option<bool>,
    /// Whether applying the line writes anything.
    writes: bool,
    messages: Vec<RowMessage>,
}

impl Line {
    fn has_error(&self) -> bool {
        self.messages.iter().any(|m| m.severity == "error")
    }
}

/// Stored dates are `YYYY-MM-DD`, sometimes followed by a time.
fn stored_date(raw: Option<String>) -> Option<NaiveDate> {
    raw.and_then(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok())
}

/// Whether two periods share a day (a missing start or end is open-ended).
fn overlaps(a: Period, b: Period) -> bool {
    let not_after = |from: Option<NaiveDate>, to: Option<NaiveDate>| match (from, to) {
        (Some(f), Some(t)) => f <= t,
        _ => true,
    };
    not_after(a.0, b.1) && not_after(b.0, a.1)
}

/// Running, as the personnel rules count it: no end date, or an end date not yet passed.
fn running(to: Option<NaiveDate>, today: NaiveDate) -> bool {
    to.is_none_or(|t| t >= today)
}

fn show(d: Option<NaiveDate>) -> String {
    d.map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_default()
}

fn show_or_open(d: Option<NaiveDate>) -> String {
    d.map_or_else(|| "sans date".to_string(), |d| d.format("%d/%m/%Y").to_string())
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let today = crate::clock::now().date_naive();

    // People by personnel number: (id, employment status), their home crew, the shown code, and
    // why the personnel update would refuse to change their home crew (it re-checks the stored
    // record: an external person needs a company, a contract cannot end before it starts).
    let mut people: HashMap<String, (i64, String)> = HashMap::new();
    let mut ambiguous_people: HashSet<String> = HashSet::new();
    let mut home_of: HashMap<i64, Option<i64>> = HashMap::new();
    let mut code_of_person: HashMap<i64, String> = HashMap::new();
    let mut home_blocked: HashMap<i64, &'static str> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, employee_code, employment_status, employment_origin, external_company_id, \
                    contract_start_date, contract_end_date, primary_team_id FROM personnel"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "employee_code")?;
        let id: i64 = r.try_get("", "id")?;
        if people.contains_key(&key(&code)) {
            ambiguous_people.insert(key(&code));
        }
        people.insert(key(&code), (id, r.try_get("", "employment_status")?));
        home_of.insert(id, r.try_get("", "primary_team_id")?);
        code_of_person.insert(id, code);
        let origin: Option<String> = r.try_get("", "employment_origin")?;
        let company: Option<i64> = r.try_get("", "external_company_id")?;
        let start: Option<String> = r.try_get("", "contract_start_date")?;
        let end: Option<String> = r.try_get("", "contract_end_date")?;
        if origin.as_deref() == Some("external") && company.is_none() {
            home_blocked.insert(
                id,
                "cette personne externe n'a pas d'entreprise : complétez sa fiche dans Personnel.",
            );
        } else if let (Some(s), Some(e)) = (start, end) {
            if s > e {
                home_blocked.insert(
                    id,
                    "son contrat se termine avant de commencer : corrigez sa fiche dans Personnel.",
                );
            }
        }
    }

    // Live crews by code: (id, active).
    let mut teams: HashMap<String, (i64, bool)> = HashMap::new();
    let mut ambiguous_teams: HashSet<String> = HashSet::new();
    let mut code_of_team: HashMap<i64, String> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, status FROM teams WHERE deleted_at IS NULL".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let id: i64 = r.try_get("", "id")?;
        if teams.contains_key(&key(&code)) {
            ambiguous_teams.insert(key(&code));
        }
        let status: String = r.try_get("", "status")?;
        teams.insert(key(&code), (id, status == "active"));
        code_of_team.insert(id, code);
    }

    // Every membership, by person and crew.
    let mut memberships: HashMap<(i64, i64), Vec<Membership>> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, team_id, valid_from, valid_to, is_lead \
             FROM personnel_team_assignments ORDER BY id"
                .to_string(),
        ))
        .await?
    {
        let lead: i64 = r.try_get("", "is_lead")?;
        memberships
            .entry((r.try_get("", "personnel_id")?, r.try_get("", "team_id")?))
            .or_default()
            .push(Membership {
                id: r.try_get("", "id")?,
                from: stored_date(r.try_get("", "valid_from")?),
                to: stored_date(r.try_get("", "valid_to")?),
                is_lead: lead != 0,
            });
    }

    // First pass: each line on its own.
    let mut first_row_of: HashMap<(i64, i64, Option<usize>, Option<NaiveDate>), usize> = HashMap::new();
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        // Refusals that only apply when the line changes something: an unchanged line of a person
        // who has left (or of a crew no longer active) is just reported as up to date.
        let mut if_writes: Vec<RowMessage> = Vec::new();
        let cell = |k: &str| raw.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
        let person = match cell("employee_code") {
            None => {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_PERSON",
                    "Le matricule est obligatoire.",
                ));
                None
            }
            Some(m) if ambiguous_people.contains(&key(m)) => {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_PERSON_AMBIGUOUS",
                    format!(
                        "Plusieurs personnes ont le matricule « {m} » à la casse près : corrigez-les dans Personnel."
                    ),
                ));
                None
            }
            Some(m) => match people.get(&key(m)) {
                None => {
                    messages.push(RowMessage::error(
                        "MEMBER_IMPORT_PERSON",
                        format!("Le matricule « {m} » n'existe pas dans le personnel : importez d'abord le personnel."),
                    ));
                    None
                }
                Some((id, status)) => {
                    if status != "active" {
                        if_writes.push(RowMessage::error(
                            "MEMBER_IMPORT_PERSON_INACTIVE",
                            format!("« {m} » n'est pas actif dans le personnel : réactivez-le d'abord."),
                        ));
                    }
                    Some(*id)
                }
            },
        };
        let team = match cell("team_code") {
            None => {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_TEAM",
                    "Le code équipe est obligatoire.",
                ));
                None
            }
            Some(t) if ambiguous_teams.contains(&key(t)) => {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_TEAM_AMBIGUOUS",
                    format!("Plusieurs équipes ont le code « {t} » à la casse près : renommez-les dans Personnel."),
                ));
                None
            }
            Some(t) => match teams.get(&key(t)) {
                None => {
                    messages.push(RowMessage::error(
                        "MEMBER_IMPORT_TEAM",
                        format!("L'équipe « {t} » n'existe pas : importez d'abord les équipes."),
                    ));
                    None
                }
                Some((id, active)) => {
                    if !active {
                        if_writes.push(RowMessage::error(
                            "MEMBER_IMPORT_TEAM_INACTIVE",
                            format!("L'équipe « {t} » n'est pas active."),
                        ));
                    }
                    Some(*id)
                }
            },
        };
        let mut date = |k: &str, label: &str| match parse_date(cell(k)) {
            Ok(d) => d,
            Err(()) => {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_DATE",
                    format!("La {label} doit être une date JJ/MM/AAAA."),
                ));
                None
            }
        };
        let from = date("valid_from", "date début");
        let to = date("valid_to", "date fin");
        let mut flag = |k: &str, label: &str| match parse_active(raw.get(k)) {
            Ok(v) => v,
            Err(()) => {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_FLAG",
                    format!("La colonne « {label} » doit contenir oui ou non."),
                ));
                None
            }
        };
        let home = flag("home", "équipe principale");
        let lead = flag("lead", "chef d'équipe");

        // The existing membership this line is about.
        let matched = match (person, team) {
            (Some(p), Some(t)) => memberships.get(&(p, t)).and_then(|list| match from {
                Some(f) => list.iter().position(|m| m.from == Some(f)),
                // Blank start: the membership without a start date (the one with the same end date
                // first, when there are several), else the one still running.
                None => list
                    .iter()
                    .position(|m| m.from.is_none() && to.is_some() && m.to == to)
                    .or_else(|| list.iter().position(|m| m.from.is_none()))
                    .or_else(|| list.iter().rposition(|m| running(m.to, today))),
            }),
            _ => None,
        };

        // What the line changes on the membership it matches (blank cells change nothing).
        let stored = matched.and_then(|m| memberships.get(&(person?, team?)).map(|list| &list[m]));
        let after_to = to.or(stored.and_then(|m| m.to));
        let to_change = to.is_some() && stored.is_none_or(|m| m.to != to);
        let home_now = person.and_then(|p| home_of.get(&p).copied().flatten());
        let home_change = home == Some(true) && team.is_some() && team != home_now;
        let was_lead = stored.is_some_and(|m| m.is_lead && running(m.to, today));
        // Ending the period ends the lead role with it: "non" on a line that closes the period
        // changes nothing more (the crew screen only changes the lead of a running membership).
        let lead_change = lead
            .filter(|v| *v != was_lead)
            .filter(|v| *v || running(after_to, today));
        let writes = stored.is_none() || to_change || home_change || lead_change.is_some();
        if writes {
            messages.append(&mut if_writes);
        }
        if let (true, Some(p)) = (home_change, person) {
            if let Some(reason) = home_blocked.get(&p) {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_HOME_PERSON",
                    format!("L'équipe principale ne peut pas être changée : {reason}"),
                ));
            }
        }
        if let (Some(p), Some(t)) = (person, team) {
            let k = (p, t, matched, if matched.is_some() { None } else { from });
            if let Some(first) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "MEMBER_IMPORT_DUPLICATE",
                    format!("Cette période apparaît déjà à la ligne {} du fichier.", first + 2),
                ));
            } else if messages.iter().all(|m| m.severity != "error") {
                first_row_of.insert(k, i);
            }
        }
        lines.push(Line {
            person,
            team,
            from,
            to,
            home,
            lead,
            matched,
            to_change,
            home_change,
            lead_change,
            writes,
            messages,
        });
    }

    // The membership a line updates, and its period once applied (a blank cell keeps the date).
    let stored_of = |l: &Line| -> Option<&Membership> {
        let list = memberships.get(&(l.person?, l.team?))?;
        l.matched.map(|m| &list[m])
    };
    let period = |l: &Line| -> Period {
        match stored_of(l) {
            Some(m) => (l.from.or(m.from), l.to.or(m.to)),
            None => (l.from, l.to),
        }
    };
    for l in &mut lines {
        if l.has_error() {
            continue;
        }
        if let (Some(f), Some(t)) = period(l) {
            if t < f {
                l.messages.push(RowMessage::error(
                    "MEMBER_IMPORT_PERIOD",
                    "La date de fin est avant la date de début.",
                ));
            }
        }
    }

    // Rules between lines, judged in file order exactly as apply writes the periods: each line is
    // checked against the memberships as the accepted lines before it leave them. A home crew line
    // anywhere in the file counts unless it is already refused (stricter, never looser, than apply).
    // Periods of each person and crew as the file goes: (stored membership, line that wrote it, period).
    let mut sim: HashMap<(i64, i64), Vec<(Option<usize>, Option<usize>, Period)>> = memberships
        .iter()
        .map(|(pt, list)| {
            (
                *pt,
                list.iter()
                    .enumerate()
                    .map(|(idx, m)| (Some(idx), None, (m.from, m.to)))
                    .collect(),
            )
        })
        .collect();
    let mut by_person: HashMap<i64, Vec<usize>> = HashMap::new();
    let mut by_team: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, l) in lines.iter().enumerate() {
        if let (false, Some(p), Some(t)) = (l.has_error(), l.person, l.team) {
            by_person.entry(p).or_default().push(i);
            by_team.entry(t).or_default().push(i);
        }
    }
    let mut refused_now: HashSet<usize> = HashSet::new();
    for i in 0..lines.len() {
        let l = &lines[i];
        let (false, Some(p), Some(t)) = (l.has_error(), l.person, l.team) else {
            continue;
        };
        // A line that changes nothing is reported as up to date and checks nothing (old data may
        // already hold overlapping periods); it still counts as what the file says for others.
        if !l.writes {
            continue;
        }
        let mine = period(l);
        let open = running(mine.1, today);
        let team_code = code_of_team.get(&t).cloned().unwrap_or_default();
        let live = |j: &&usize| !refused_now.contains(*j);
        let verdict: Option<RowMessage> = 'check: {
            let clash = sim.get(&(p, t)).and_then(|list| {
                list.iter()
                    .find(|(m, _, per)| (m.is_none() || *m != l.matched) && overlaps(mine, *per))
            });
            match clash {
                Some((_, Some(j), _)) => {
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_OVERLAP",
                        format!("Cette période chevauche celle de la ligne {} du fichier.", j + 2),
                    ));
                }
                Some((_, None, (f, to))) => {
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_OVERLAP",
                        format!(
                            "Cette période chevauche une période existante dans l'équipe « {team_code} » (du {} au {}).",
                            show_or_open(*f),
                            show_or_open(*to)
                        ),
                    ));
                }
                None => {}
            }

            if l.home == Some(true) {
                if !open {
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_HOME_CLOSED",
                        "Une équipe principale demande une période en cours (date de fin vide ou à venir).",
                    ));
                }
                if let Some(j) = by_person[&p]
                    .iter()
                    .filter(live)
                    .find(|j| **j < i && lines[**j].home == Some(true))
                {
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_HOME_TWICE",
                        format!("La ligne {} donne déjà une équipe principale à cette personne.", j + 2),
                    ));
                }
            }

            // A home crew change ends the person's other running memberships: a line that keeps one
            // running in another crew contradicts it.
            let home_now = home_of.get(&p).copied().flatten();
            let new_home = by_person[&p]
                .iter()
                .filter(live)
                .find(|j| lines[**j].home == Some(true) && lines[**j].team != home_now);
            if let Some(j) = new_home {
                let home_team = lines[*j].team;
                if home_team != Some(t) && open {
                    let home_code = home_team
                        .and_then(|h| code_of_team.get(&h))
                        .cloned()
                        .unwrap_or_default();
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_HOME_CONFLICT",
                        format!(
                            "La ligne {} fait de « {home_code} » l'équipe principale, ce qui clôture les autres équipes en cours de la personne : donnez une date de fin passée à cette ligne, ou retirez-la.",
                            j + 2
                        ),
                    ));
                }
            }

            if l.lead == Some(true) {
                if !open {
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_LEAD_CLOSED",
                        "Un chef d'équipe doit avoir une période en cours (date de fin vide ou à venir).",
                    ));
                }
                if let Some(j) = by_team[&t]
                    .iter()
                    .filter(live)
                    .find(|j| **j < i && lines[**j].lead == Some(true))
                {
                    break 'check Some(RowMessage::error(
                        "MEMBER_IMPORT_LEAD_TWICE",
                        format!("La ligne {} désigne déjà le chef de l'équipe « {team_code} ».", j + 2),
                    ));
                }
            }
            None
        };
        match verdict {
            Some(m) => {
                refused_now.insert(i);
                lines[i].messages.push(m);
            }
            None => {
                let matched = lines[i].matched;
                let list = sim.entry((p, t)).or_default();
                match matched.and_then(|m| list.iter_mut().find(|(s, _, _)| *s == Some(m))) {
                    Some(entry) => {
                        entry.1 = Some(i);
                        entry.2 = mine;
                    }
                    None => list.push((None, Some(i), mine)),
                }
            }
        }
    }

    // Current lead of each crew, for the replacement warning.
    let mut leads_now: HashMap<i64, i64> = HashMap::new();
    for ((p, t), list) in &memberships {
        if list.iter().any(|m| m.is_lead && running(m.to, today)) {
            leads_now.insert(*t, *p);
        }
    }

    // Crews in which each person has a running membership, for the home crew warning.
    let mut running_crews: HashMap<i64, Vec<i64>> = HashMap::new();
    for ((p, t), list) in &memberships {
        if list.iter().any(|m| running(m.to, today)) {
            running_crews.entry(*p).or_default().push(*t);
        }
    }

    let mut out = Vec::new();
    for l in &lines {
        let mut messages = l.messages.clone();
        let stored = stored_of(l);
        let home_now = l.person.and_then(|p| home_of.get(&p).copied().flatten());
        let (to_change, home_change, lead_change) = (l.to_change, l.home_change, l.lead_change);

        if let (false, Some(p), Some(t)) = (l.has_error(), l.person, l.team) {
            if home_change {
                let mut others: Vec<String> = running_crews
                    .get(&p)
                    .into_iter()
                    .flatten()
                    .filter(|mt| **mt != t)
                    .filter_map(|mt| code_of_team.get(mt).cloned())
                    .collect();
                others.sort();
                if !others.is_empty() {
                    messages.push(RowMessage::warning(
                        "MEMBER_IMPORT_HOME_CLOSES",
                        format!(
                            "Nouvelle équipe principale : les équipes en cours {} seront clôturées pour cette personne.",
                            others.join(", ")
                        ),
                    ));
                }
            }
            if lead_change == Some(true) {
                if let Some(prev) = leads_now.get(&t).filter(|prev| **prev != p) {
                    messages.push(RowMessage::warning(
                        "MEMBER_IMPORT_LEAD_REPLACED",
                        format!(
                            "« {} » n'est plus chef de cette équipe.",
                            code_of_person.get(prev).cloned().unwrap_or_default()
                        ),
                    ));
                }
            }
            if l.home == Some(false) && home_now == Some(t) {
                messages.push(RowMessage::warning(
                    "MEMBER_IMPORT_HOME_KEPT",
                    "« non » ne retire pas l'équipe principale : indiquez « oui » sur la ligne de la nouvelle équipe principale.",
                ));
            }
        }

        let action = if l.has_error() {
            "skip"
        } else if stored.is_none() {
            "create"
        } else if to_change || home_change || lead_change.is_some() {
            "update"
        } else {
            "skip"
        };
        out.push(RowOutcome {
            action,
            normalized: json!({
                "personnel_id": l.person,
                "team_id": l.team,
                "assignment_id": stored.map(|m| m.id),
                "valid_from": l.from.map(|d| d.to_string()),
                "valid_to": l.to.filter(|_| to_change).map(|d| d.to_string()),
                "home": home_change,
                "lead": lead_change,
            }),
            messages,
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

/// Writes the membership period of one line. `Ok(Err(reason))` when it can no longer be written.
async fn write_period(db: &DatabaseConnection, row: &StagedRow, p: i64, t: i64) -> AppResult<Result<(), String>> {
    let n = &row.normalized;
    let from = n["valid_from"]
        .as_str()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let to = n["valid_to"]
        .as_str()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let assignment = n["assignment_id"].as_i64();

    // The overlap rule again, on the memberships as they are now.
    let mut stored: Option<Period> = None;
    let mut others: Vec<Period> = Vec::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, valid_from, valid_to FROM personnel_team_assignments WHERE personnel_id = ? AND team_id = ?",
            [p.into(), t.into()],
        ))
        .await?
    {
        let id: i64 = r.try_get("", "id")?;
        let period = (
            stored_date(r.try_get("", "valid_from")?),
            stored_date(r.try_get("", "valid_to")?),
        );
        if Some(id) == assignment {
            stored = Some(period);
        } else {
            others.push(period);
        }
    }
    if assignment.is_some() && stored.is_none() {
        return Ok(Err("Cette période n'existe plus.".to_string()));
    }
    let after = match stored {
        Some((f, old_to)) => (from.or(f), to.or(old_to)),
        None => (from, to),
    };
    if others.iter().any(|o| overlaps(after, *o)) {
        return Ok(Err(
            "Cette période chevauche maintenant une autre période de la personne dans cette équipe.".to_string(),
        ));
    }

    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    match assignment {
        Some(id) => {
            if let Some(to) = to {
                // The lead role lives on a running membership: a period that this line ends, or
                // that it reopens after it had ended, no longer carries an old lead flag (a new
                // lead is set by the "chef d'équipe" column, after the periods).
                let today = crate::clock::now().date_naive();
                let was_running = stored.is_some_and(|(_, old_to)| running(old_to, today));
                let ends_or_reopens = !running(after.1, today) || !was_running;
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE personnel_team_assignments                      SET valid_to = ?, updated_at = ?,                          is_lead = CASE WHEN ? THEN 0 ELSE is_lead END,                          role_code = CASE WHEN ? AND is_lead = 1 THEN 'member' ELSE role_code END                      WHERE id = ?",
                    [
                        to.to_string().into(),
                        now.into(),
                        ends_or_reopens.into(),
                        ends_or_reopens.into(),
                        id.into(),
                    ],
                ))
                .await?;
            }
        }
        None => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO personnel_team_assignments \
                 (entity_sync_id, personnel_id, team_id, role_code, allocation_percent, valid_from, valid_to, \
                  is_lead, created_at, updated_at) \
                 VALUES (?, ?, ?, 'member', 100.0, ?, ?, 0, ?, ?)",
                [
                    crate::sync::identity::new_entity_sync_id().into(),
                    p.into(),
                    t.into(),
                    from.map(|d| d.to_string()).into(),
                    to.map(|d| d.to_string()).into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        }
    }
    Ok(Ok(()))
}

/// Makes the crew the person's home crew through the personnel update, as the screen does.
async fn set_home(db: &DatabaseConnection, p: i64, t: i64, user_id: i32) -> AppResult<Result<(), String>> {
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version FROM personnel WHERE id = ?",
            [p.into()],
        ))
        .await?
    else {
        return Ok(Err("Cette personne n'existe plus.".to_string()));
    };
    let input = crate::personnel::domain::PersonnelUpdateInput {
        id: p,
        expected_row_version: row.try_get("", "row_version")?,
        full_name: None,
        employment_type: None,
        employment_origin: None,
        employment_status: None,
        position_id: None,
        primary_entity_id: None,
        primary_team_id: Some(t),
        supervisor_id: None,
        home_schedule_reference_value_id: None,
        availability_status: None,
        blocked_override: None,
        hire_date: None,
        termination_date: None,
        email: None,
        phone: None,
        external_company_id: None,
        contract_number: None,
        contract_start_date: None,
        contract_end_date: None,
        notes: None,
        assignment_reason: Some("import".to_string()),
        employee_code: None,
    };
    match crate::personnel::queries::update_personnel(db, input, i64::from(user_id)).await {
        Ok(_) => Ok(Ok(())),
        Err(AppError::Database(e)) => Err(AppError::Database(e)),
        Err(e) => Ok(Err(app_error_text(&e))),
    }
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow], user_id: i32) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    // Periods first, then home crews (which end other running memberships), then crew leads
    // (which need the running membership in place).
    let mut written: Vec<(&StagedRow, i64, i64)> = Vec::new();
    for row in rows {
        if row.action != "create" && row.action != "update" {
            out.push(RowApplied {
                row_id: row.id,
                status: "skipped",
                message: "Déjà à jour.".to_string(),
            });
            continue;
        }
        let (Some(p), Some(t)) = (
            row.normalized["personnel_id"].as_i64(),
            row.normalized["team_id"].as_i64(),
        ) else {
            out.push(failed(row, "Ligne incomplète."));
            continue;
        };
        match write_period(db, row, p, t).await? {
            Ok(()) => written.push((row, p, t)),
            Err(reason) => out.push(failed(row, reason)),
        }
    }
    let mut refused: HashMap<i64, String> = HashMap::new();
    for (row, p, t) in &written {
        if row.normalized["home"].as_bool() == Some(true) {
            if let Err(reason) = set_home(db, *p, *t, user_id).await? {
                refused.insert(
                    row.id,
                    format!("Période enregistrée, mais équipe principale refusée : {reason}"),
                );
            }
        }
    }
    for (row, p, t) in &written {
        if refused.contains_key(&row.id) {
            continue;
        }
        if let Some(is_lead) = row.normalized["lead"].as_bool() {
            let set = crate::personnel::queries::set_team_lead(
                db,
                crate::personnel::domain::SetTeamLeadInput {
                    team_id: *t,
                    personnel_id: *p,
                    is_lead,
                },
            )
            .await;
            match set {
                Ok(_) => {}
                Err(AppError::Database(e)) => return Err(AppError::Database(e)),
                Err(e) => {
                    refused.insert(
                        row.id,
                        format!(
                            "Période enregistrée, mais chef d'équipe refusé : {}",
                            app_error_text(&e)
                        ),
                    );
                }
            }
        }
    }
    for (row, _, _) in written {
        out.push(match refused.remove(&row.id) {
            Some(reason) => failed(row, reason),
            None => RowApplied {
                row_id: row.id,
                status: if row.action == "create" { "created" } else { "updated" },
                message: String::new(),
            },
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let today = crate::clock::now().date_naive();
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.employee_code, t.code AS team_code, a.valid_from, a.valid_to, a.is_lead, \
                    CASE WHEN p.primary_team_id = t.id THEN 1 ELSE 0 END AS is_home \
             FROM personnel_team_assignments a \
             JOIN personnel p ON p.id = a.personnel_id \
             JOIN teams t ON t.id = a.team_id AND t.deleted_at IS NULL \
             ORDER BY t.code, p.employee_code, a.valid_from"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let to = stored_date(r.try_get("", "valid_to")?);
            let lead: i64 = r.try_get("", "is_lead")?;
            let home: i64 = r.try_get("", "is_home")?;
            // Home crew and lead only mean something on a running membership.
            let yes_no = |v: bool| match (running(to, today), v) {
                (false, _) => "",
                (true, true) => "oui",
                (true, false) => "non",
            };
            Ok(vec![
                r.try_get("", "employee_code")?,
                r.try_get("", "team_code")?,
                show(stored_date(r.try_get("", "valid_from")?)),
                show(to),
                yes_no(home == 1).to_string(),
                yes_no(lead != 0).to_string(),
            ])
        })
        .collect()
}
