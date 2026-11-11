//! Maintenance history of the former system ("OT historiques" sheet, L7-30). One line = one past
//! intervention, stored as a read-only history record (`wo_history_records`), never as a work
//! order: no workflow, permits, stock movements or budget postings (CMMS Advisor option C,
//! 2026-11-10). A line with "défaillance = oui" also writes one failure event, whatever the
//! nature (ISO 14224: failures found during preventive work count), so the RAMS indicators count
//! it with the live work orders.
//!
//! Rules (Advisor rulings Q1–Q6):
//! - the n° OT d'origine is the key: one already loaded is skipped, never changed;
//! - début and fin des travaux are required, fin not before début, no date in the future;
//! - an unknown nature or failure code is an error (reference data rule); an unknown intervener
//!   matricule is kept as text with a warning (leavers);
//! - durée d'arrêt and the arrêt / remise en service dates differing by more than 10 %: warning,
//!   the dates win;
//! - the user needs `ot.create` at each equipment's site (the rule of a new work order), checked
//!   line by line in the preview and again when the batch is applied.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDateTime;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::json;

use super::equipment_meter::{equipment_by_code, parse_number};
use super::org_tree::{key, parse_active};
use super::pm_plan::{access_of, cell, err, has_error};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::auth::scope::AccessMap;
use crate::errors::{AppError, AppResult};
use crate::rbac::permissions::{OT_CREATE, OT_EDIT};

/// `failure_events.source_type` of an imported history record.
pub const HISTORY_FAILURE_SOURCE: &str = "history_record";

const fn col(
    key: &'static str,
    header: &'static str,
    required: bool,
    aliases: &'static [&'static str],
    hint: &'static str,
) -> Column {
    Column {
        key,
        header,
        required,
        aliases,
        hint,
    }
}

pub const SPEC: EntitySpec = EntitySpec {
    code: "wo_history",
    label: "Historique des interventions (OT historiques)",
    order: 8,
    needs: "Équipements, natures d'OT et codes de défaillance (Données de référence). L'historique importé doit être complet pour la période couverte : les indicateurs RAMS (MTBF, disponibilité) le supposent.",
    permission: OT_CREATE,
    columns: &[
        col("import_ref", "n° OT d'origine", true, &["n_ot", "no_ot", "numero_ot", "ot_origine", "n_ot_d_origine"], "Numéro de l'intervention dans l'ancien système. Une intervention déjà importée est ignorée."),
        col("equipment_code", "code équipement", true, &["equipement", "code_equipement", "equipment_code", "asset_code"], "Équipement concerné."),
        col("nature", "nature", true, &["type", "type_ot", "nature_ot", "code_nature"], "Code d'un type d'OT des Données de référence (correctif, préventif…)."),
        col("is_failure", "défaillance", true, &["defaillance", "panne", "is_failure"], "oui si l'équipement était en panne (compté dans le MTBF), sinon non."),
        col("started", "date de début des travaux", true, &["debut", "date_debut", "debut_travaux"], "JJ/MM/AAAA, avec l'heure si connue (JJ/MM/AAAA HH:MM)."),
        col("ended", "date de fin des travaux", true, &["fin", "date_fin", "fin_travaux"], "JJ/MM/AAAA [HH:MM], pas avant le début."),
        col("detected", "date de détection de la panne", false, &["detection", "date_detection"], "JJ/MM/AAAA [HH:MM]."),
        col("stopped", "date d'arrêt équipement", false, &["arret", "date_arret"], "JJ/MM/AAAA [HH:MM]."),
        col("restored", "date de remise en service", false, &["remise_en_service", "date_remise_en_service"], "JJ/MM/AAAA [HH:MM]."),
        col("downtime", "durée d'arrêt (h)", false, &["duree_arret", "duree_d_arret", "arret_h"], "En heures. Si les dates d'arrêt et de remise en service sont données, elles priment."),
        col("mode", "mode de défaillance", false, &["mode", "code_mode"], "Code des Données de référence (modes de défaillance)."),
        col("cause", "cause", false, &["cause_defaillance", "code_cause"], "Code des Données de référence (causes)."),
        col("mechanism", "mécanisme", false, &["mecanisme", "mecanisme_defaillance"], "Code des Données de référence (mécanismes)."),
        col("detection_method", "méthode de détection", false, &["methode_detection", "detection_methode"], "Code des Données de référence (méthodes de détection)."),
        col("effect", "effet sur la fonction", false, &["effet", "effet_fonction"], "Code des Données de référence (effets)."),
        col("description", "description", false, &["symptome", "libelle"], "Ce qui a été constaté."),
        col("work_done", "travaux réalisés", false, &["travaux", "travaux_realises", "compte_rendu"], "Ce qui a été fait."),
        col("labor_hours", "heures MO totales", false, &["heures_mo", "heures", "main_d_oeuvre_h"], "Heures de main-d'œuvre, toutes personnes comprises."),
        col("labor_cost", "coût MO", false, &["cout_mo", "cout_main_d_oeuvre"], "Montant historique."),
        col("parts_cost", "coût pièces", false, &["cout_pieces"], "Montant historique (aucun mouvement de stock)."),
        col("service_cost", "coût externe", false, &["cout_externe", "sous_traitance", "cout_sous_traitance"], "Montant historique."),
        col("interveners", "matricules intervenants", false, &["intervenants", "matricules"], "Matricules séparés par des virgules. Un matricule inconnu est gardé tel quel."),
    ],
};

const MAX_HOURS: f64 = 100_000.0;
const MAX_AMOUNT: f64 = 1e12;
const MAX_TEXT: usize = 4000;

type Codes = (HashMap<String, i64>, HashSet<String>);

/// Codes of one reference domain (published set, active values) by key → id; codes that differ
/// only by letter case land in `twice`.
async fn reference_codes(db: &impl ConnectionTrait, domain: &str) -> AppResult<Codes> {
    let mut out = HashMap::new();
    let mut twice = HashSet::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rd.code = ? AND rs.status = 'published' AND rv.is_active = 1",
            [domain.into()],
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

struct Lists {
    equipment: Codes,
    natures: Codes,
    modes: Codes,
    causes: Codes,
    mechanisms: Codes,
    detection: Codes,
    effects: Codes,
    matricules: HashSet<String>,
}

async fn lists(db: &DatabaseConnection) -> AppResult<Lists> {
    let mut matricules = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT employee_code FROM personnel WHERE employee_code IS NOT NULL".to_string(),
        ))
        .await?
    {
        matricules.insert(key(&r.try_get::<String>("", "employee_code")?));
    }
    Ok(Lists {
        equipment: equipment_by_code(db).await?,
        natures: reference_codes(db, crate::wo::types::WORK_ORDER_TYPES_DOMAIN).await?,
        modes: reference_codes(db, "WORK.FAILURE_MODES").await?,
        causes: reference_codes(db, "WORK.FAILURE_CAUSES").await?,
        mechanisms: reference_codes(db, "WORK.FAILURE_MECHANISMS").await?,
        detection: reference_codes(db, "WORK.DETECTION_METHODS").await?,
        effects: reference_codes(db, "WORK.FAILURE_EFFECTS").await?,
        matricules,
    })
}

/// History n° already loaded (upper case).
async fn loaded_refs(db: &impl ConnectionTrait) -> AppResult<HashSet<String>> {
    let mut out = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT import_ref FROM wo_history_records".to_string(),
        ))
        .await?
    {
        out.insert(r.try_get::<String>("", "import_ref")?);
    }
    Ok(out)
}

fn normalize_ref(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// A date of the file, with its time if given (JJ/MM/AAAA HH:MM, or AAAA-MM-JJ HH:MM).
fn parse_datetime(raw: &str) -> Result<Option<NaiveDateTime>, ()> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    for f in [
        "%d/%m/%Y %H:%M",
        "%d/%m/%Y %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(d) = NaiveDateTime::parse_from_str(raw, f) {
            return Ok(Some(d));
        }
    }
    match super::parse_date(Some(raw))? {
        Some(d) => Ok(Some(d.and_hms_opt(0, 0, 0).ok_or(())?)),
        None => Ok(None),
    }
}

fn rfc3339(d: NaiveDateTime) -> String {
    d.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn hours_between(a: NaiveDateTime, b: NaiveDateTime) -> f64 {
    (b - a).num_minutes() as f64 / 60.0
}

/// One line as it will be stored.
struct Record {
    import_ref: String,
    equipment_id: i64,
    type_id: i64,
    is_failure: bool,
    started: NaiveDateTime,
    ended: NaiveDateTime,
    detected: Option<NaiveDateTime>,
    stopped: Option<NaiveDateTime>,
    restored: Option<NaiveDateTime>,
    downtime_hours: Option<f64>,
    mode: Option<i64>,
    cause: Option<i64>,
    mechanism: Option<i64>,
    detection_method: Option<i64>,
    effect: Option<i64>,
    description: Option<String>,
    work_done: Option<String>,
    labor_hours: Option<f64>,
    labor_cost: Option<f64>,
    parts_cost: Option<f64>,
    service_cost: Option<f64>,
    interveners: Option<String>,
}

/// A code of one reference domain; an unknown code is an error naming the list.
fn code_of(
    raw: &HashMap<String, String>,
    field: &str,
    label: &str,
    codes: &Codes,
    messages: &mut Vec<RowMessage>,
) -> Option<i64> {
    let value = cell(raw, field);
    if value.is_empty() {
        return None;
    }
    if codes.1.contains(&key(&value)) {
        err(
            messages,
            "HISTORY_IMPORT_CODE_AMBIGUOUS",
            format!("Plusieurs codes « {value} » ({label}) à la casse près : corrigez les Données de référence."),
        );
        return None;
    }
    let found = codes.0.get(&key(&value)).copied();
    if found.is_none() {
        err(
            messages,
            "HISTORY_IMPORT_CODE_UNKNOWN",
            format!("Le code « {value} » n'existe pas dans les Données de référence ({label})."),
        );
    }
    found
}

/// A number of the file, 0 or more and at most `max`; an error naming the column otherwise.
fn number(
    raw: &HashMap<String, String>,
    field: &str,
    label: &str,
    max: f64,
    messages: &mut Vec<RowMessage>,
) -> Option<f64> {
    let value = cell(raw, field);
    if value.is_empty() {
        return None;
    }
    let n = parse_number(&value).filter(|v| (0.0..=max).contains(v));
    if n.is_none() {
        err(
            messages,
            "HISTORY_IMPORT_NUMBER",
            format!("« {label} » doit être un nombre, 0 ou plus."),
        );
    }
    n
}

fn text(raw: &HashMap<String, String>, field: &str, label: &str, messages: &mut Vec<RowMessage>) -> Option<String> {
    let value = cell(raw, field);
    if value.chars().count() > MAX_TEXT {
        err(
            messages,
            "HISTORY_IMPORT_TEXT_LONG",
            format!("« {label} » dépasse {MAX_TEXT} caractères."),
        );
    }
    (!value.is_empty()).then_some(value)
}

/// A date column: `None` and a message when it is missing (if required), unreadable or in the
/// future.
fn date(
    raw: &HashMap<String, String>,
    field: &str,
    label: &str,
    required: bool,
    now: NaiveDateTime,
    messages: &mut Vec<RowMessage>,
) -> Option<NaiveDateTime> {
    match parse_datetime(&cell(raw, field)) {
        Ok(Some(d)) if d > now => {
            err(
                messages,
                "HISTORY_IMPORT_DATE_FUTURE",
                format!("La {label} ne peut pas être dans le futur."),
            );
            None
        }
        Ok(Some(d)) => Some(d),
        Ok(None) => {
            if required {
                err(
                    messages,
                    "HISTORY_IMPORT_DATE_REQUIRED",
                    format!("La {label} est obligatoire."),
                );
            }
            None
        }
        Err(()) => {
            err(
                messages,
                "HISTORY_IMPORT_DATE",
                format!("La {label} doit être au format JJ/MM/AAAA (avec l'heure HH:MM si connue)."),
            );
            None
        }
    }
}

/// Reads one line. Every problem is pushed to `messages`; `None` when it cannot be stored.
fn read_line(
    raw: &HashMap<String, String>,
    l: &Lists,
    now: NaiveDateTime,
    messages: &mut Vec<RowMessage>,
) -> Option<Record> {
    let import_ref = normalize_ref(&cell(raw, "import_ref"));
    if import_ref.is_empty() {
        err(messages, "HISTORY_IMPORT_REF", "Le n° OT d'origine est obligatoire.");
    }
    let equipment_code = cell(raw, "equipment_code");
    let equipment_id = if equipment_code.is_empty() {
        err(
            messages,
            "HISTORY_IMPORT_EQUIPMENT",
            "Le code équipement est obligatoire.",
        );
        None
    } else if l.equipment.1.contains(&key(&equipment_code)) {
        err(
            messages,
            "HISTORY_IMPORT_EQUIPMENT_AMBIGUOUS",
            format!(
                "Plusieurs équipements ont le code « {equipment_code} » à la casse près : renommez-les avant d'importer."
            ),
        );
        None
    } else {
        let found = l.equipment.0.get(&key(&equipment_code)).copied();
        if found.is_none() {
            err(
                messages,
                "HISTORY_IMPORT_EQUIPMENT_UNKNOWN",
                format!("L'équipement « {equipment_code} » n'existe pas : importez d'abord les équipements."),
            );
        }
        found
    };
    let type_id = if cell(raw, "nature").is_empty() {
        err(messages, "HISTORY_IMPORT_NATURE", "La nature est obligatoire.");
        None
    } else {
        code_of(raw, "nature", "types d'OT", &l.natures, messages)
    };
    let is_failure = match parse_active(raw.get("is_failure")) {
        Ok(Some(v)) => Some(v),
        Ok(None) => {
            err(
                messages,
                "HISTORY_IMPORT_FAILURE_FLAG",
                "La colonne « défaillance » est obligatoire : oui ou non.",
            );
            None
        }
        Err(()) => {
            err(
                messages,
                "HISTORY_IMPORT_FAILURE_FLAG",
                "La colonne « défaillance » doit contenir oui ou non.",
            );
            None
        }
    };

    let started = date(raw, "started", "date de début des travaux", true, now, messages);
    let ended = date(raw, "ended", "date de fin des travaux", true, now, messages);
    let detected = date(raw, "detected", "date de détection de la panne", false, now, messages);
    let stopped = date(raw, "stopped", "date d'arrêt équipement", false, now, messages);
    let restored = date(raw, "restored", "date de remise en service", false, now, messages);
    if let (Some(s), Some(e)) = (started, ended) {
        if e < s {
            err(
                messages,
                "HISTORY_IMPORT_DATE_ORDER",
                "La date de fin des travaux est avant la date de début.",
            );
        }
    }
    let mut downtime_hours = number(raw, "downtime", "durée d'arrêt (h)", MAX_HOURS, messages);
    if let (Some(s), Some(r)) = (stopped, restored) {
        if r < s {
            err(
                messages,
                "HISTORY_IMPORT_DATE_ORDER",
                "La date de remise en service est avant la date d'arrêt.",
            );
        } else {
            let from_dates = hours_between(s, r);
            if let Some(given) = downtime_hours {
                if (given - from_dates).abs() > 0.1 * from_dates.max(given) {
                    messages.push(RowMessage::warning(
                        "HISTORY_IMPORT_DOWNTIME_GAP",
                        format!(
                            "La durée d'arrêt ({given} h) diffère de plus de 10 % des dates d'arrêt et de remise en service ({from_dates:.1} h) : les dates sont retenues."
                        ),
                    ));
                }
            }
            downtime_hours = Some(from_dates);
        }
    }

    let mode = code_of(raw, "mode", "modes de défaillance", &l.modes, messages);
    let cause = code_of(raw, "cause", "causes", &l.causes, messages);
    let mechanism = code_of(raw, "mechanism", "mécanismes", &l.mechanisms, messages);
    let detection_method = code_of(raw, "detection_method", "méthodes de détection", &l.detection, messages);
    let effect = code_of(raw, "effect", "effets", &l.effects, messages);
    if is_failure == Some(true) && cell(raw, "mode").is_empty() {
        messages.push(RowMessage::warning(
            "HISTORY_IMPORT_CODING_INCOMPLETE",
            "Défaillance sans mode : comptée dans le MTBF mais signalée « codage incomplet ». Le codage pourra être complété ensuite.",
        ));
    }

    let description = text(raw, "description", "description", messages);
    let work_done = text(raw, "work_done", "travaux réalisés", messages);
    let labor_hours = number(raw, "labor_hours", "heures MO totales", MAX_HOURS, messages);
    let labor_cost = number(raw, "labor_cost", "coût MO", MAX_AMOUNT, messages);
    let parts_cost = number(raw, "parts_cost", "coût pièces", MAX_AMOUNT, messages);
    let service_cost = number(raw, "service_cost", "coût externe", MAX_AMOUNT, messages);
    let given: Vec<String> = cell(raw, "interveners")
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let unknown: Vec<&str> = given
        .iter()
        .filter(|m| !l.matricules.contains(&key(m)))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        messages.push(RowMessage::warning(
            "HISTORY_IMPORT_INTERVENER_UNKNOWN",
            format!(
                "Matricule(s) inconnu(s), gardé(s) tel(s) quel(s) : {}.",
                unknown.join(", ")
            ),
        ));
    }
    let interveners = (!given.is_empty()).then(|| given.join(", "));

    if has_error(messages) {
        return None;
    }
    Some(Record {
        import_ref,
        equipment_id: equipment_id?,
        type_id: type_id?,
        is_failure: is_failure?,
        started: started?,
        ended: ended?,
        detected,
        stopped,
        restored,
        downtime_hours,
        mode,
        cause,
        mechanism,
        detection_method,
        effect,
        description,
        work_done,
        labor_hours,
        labor_cost,
        parts_cost,
        service_cost,
        interveners,
    })
}

/// Whether the user may record work on this equipment (`ot.create` at its site).
async fn may_record_on(db: &DatabaseConnection, access: &AccessMap, equipment_id: i64) -> AppResult<bool> {
    match crate::wo::access::check_create_at(db, access, None, Some(equipment_id)).await {
        Ok(()) => Ok(true),
        Err(AppError::PermissionDenied(_) | AppError::OrgValidationFailed(_) | AppError::ValidationFailed(_)) => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

fn no_right(equipment: &str) -> String {
    format!("Vous n'avez pas le droit d'enregistrer des interventions sur le site de l'équipement « {equipment} ».")
}

fn now() -> NaiveDateTime {
    crate::clock::now().naive_utc()
}

pub async fn validate(
    db: &DatabaseConnection,
    rows: &[HashMap<String, String>],
    user_id: Option<i64>,
) -> AppResult<Vec<RowOutcome>> {
    let user_id =
        user_id.ok_or_else(|| AppError::Internal(anyhow::anyhow!("history import needs the importing user")))?;
    let access = access_of(db, user_id).await?;
    let l = lists(db).await?;
    let loaded = loaded_refs(db).await?;
    let now = now();
    let mut right_of: HashMap<i64, bool> = HashMap::new();
    let mut first_row_of: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let import_ref = normalize_ref(&cell(raw, "import_ref"));
        if !import_ref.is_empty() {
            if let Some(f) = first_row_of.get(&import_ref) {
                err(
                    &mut messages,
                    "HISTORY_IMPORT_DUPLICATE",
                    format!(
                        "L'intervention « {import_ref} » apparaît déjà à la ligne {} du fichier.",
                        f + 2
                    ),
                );
            } else {
                first_row_of.insert(import_ref.clone(), i);
                if loaded.contains(&import_ref) {
                    messages.push(RowMessage::warning(
                        "HISTORY_IMPORT_EXISTS",
                        format!("L'intervention « {import_ref} » est déjà importée : elle n'est pas modifiée."),
                    ));
                    out.push(RowOutcome {
                        action: "skip",
                        normalized: json!({ "import_ref": import_ref }),
                        messages,
                    });
                    continue;
                }
            }
        }
        let read = read_line(raw, &l, now, &mut messages);
        if let Some(r) = &read {
            let allowed = match right_of.get(&r.equipment_id) {
                Some(a) => *a,
                None => {
                    let a = may_record_on(db, &access, r.equipment_id).await?;
                    right_of.insert(r.equipment_id, a);
                    a
                }
            };
            if !allowed {
                err(
                    &mut messages,
                    "HISTORY_IMPORT_SITE_RIGHT",
                    no_right(&cell(raw, "equipment_code")),
                );
            }
        }
        let ok = read.is_some() && !has_error(&messages);
        out.push(RowOutcome {
            action: if ok { "create" } else { "skip" },
            normalized: json!({ "import_ref": import_ref, "raw": raw }),
            messages,
        });
    }
    Ok(out)
}

fn fail(row: &StagedRow, message: impl Into<String>) -> RowApplied {
    RowApplied {
        row_id: row.id,
        status: "failed",
        message: message.into(),
    }
}

/// Stores one record and, for a failure, its failure event, in one transaction.
async fn store(db: &DatabaseConnection, r: &Record, user_id: i32) -> AppResult<()> {
    let ts = crate::clock::now().to_rfc3339();
    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_history_records (entity_sync_id, import_ref, equipment_id, type_id, is_failure, \
         started_at, ended_at, detected_at, stopped_at, restored_at, downtime_hours, failure_mode_id, \
         failure_cause_id, failure_mechanism_id, detection_method_id, failure_effect_id, description, work_done, \
         labor_hours, labor_cost, parts_cost, service_cost, interveners_text, imported_by_id, row_version, \
         created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            r.import_ref.clone().into(),
            r.equipment_id.into(),
            r.type_id.into(),
            i64::from(r.is_failure).into(),
            rfc3339(r.started).into(),
            rfc3339(r.ended).into(),
            r.detected.map(rfc3339).into(),
            r.stopped.map(rfc3339).into(),
            r.restored.map(rfc3339).into(),
            r.downtime_hours.into(),
            r.mode.into(),
            r.cause.into(),
            r.mechanism.into(),
            r.detection_method.into(),
            r.effect.into(),
            r.description.clone().into(),
            r.work_done.clone().into(),
            r.labor_hours.into(),
            r.labor_cost.into(),
            r.parts_cost.into(),
            r.service_cost.into(),
            r.interveners.clone().into(),
            i64::from(user_id).into(),
            ts.clone().into(),
            ts.clone().into(),
        ],
    ))
    .await?;
    if r.is_failure {
        let id: i64 = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM wo_history_records WHERE import_ref = ?",
                [r.import_ref.clone().into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("history record missing after insert")))?
            .try_get("", "id")?;
        // The failure as RAMS reads it: failed when the equipment stopped (else detected, else
        // work started), restored when back in service (else work ended).
        let failed = r.stopped.or(r.detected).unwrap_or(r.started);
        let detected = r.detected.or(r.stopped).unwrap_or(r.started);
        let restored = r.restored.unwrap_or(r.ended);
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, component_id, \
             detected_at, failed_at, restored_at, downtime_duration_hours, active_repair_hours, waiting_hours, \
             is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, \
             failure_mechanism_id, symptom_id, cause_not_determined, production_impact_level, \
             safety_impact_level, recorded_by_id, verification_status, eligible_flags_json, row_version, \
             created_at, updated_at) \
             VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, 0, 0, NULL, ?, ?, ?, ?, NULL, 0, NULL, NULL, ?, \
             'recorded', '{}', 1, ?, ?)",
            [
                format!("failure_event:{}", uuid::Uuid::new_v4()).into(),
                HISTORY_FAILURE_SOURCE.into(),
                id.into(),
                r.equipment_id.into(),
                rfc3339(detected).into(),
                rfc3339(failed).into(),
                rfc3339(restored).into(),
                r.downtime_hours.unwrap_or(0.0).into(),
                r.labor_hours.unwrap_or(0.0).into(),
                r.mode.into(),
                r.cause.into(),
                r.effect.into(),
                r.mechanism.into(),
                i64::from(user_id).into(),
                ts.clone().into(),
                ts.into(),
            ],
        ))
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow], user_id: i32) -> AppResult<Vec<RowApplied>> {
    // The entitlement and licence checks of a new work order, then the user's rights now.
    crate::entitlements::queries::enforce_capability_for_permission(db, OT_CREATE).await?;
    crate::license::queries::enforce_permission_matrix(db, user_id, OT_CREATE).await?;
    let access = access_of(db, i64::from(user_id)).await?;
    let l = lists(db).await?;
    let now = now();
    let mut loaded = loaded_refs(db).await?;
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
        let Some(record) = read_line(&raw, &l, now, &mut messages) else {
            let why = messages
                .into_iter()
                .filter(|m| m.severity == "error")
                .map(|m| m.message)
                .collect::<Vec<_>>()
                .join(" ");
            out.push(fail(row, why));
            continue;
        };
        match may_record_on(db, &access, record.equipment_id).await {
            Ok(true) => {}
            Ok(false) => {
                out.push(fail(row, no_right(&cell(&raw, "equipment_code"))));
                continue;
            }
            Err(e) => {
                tracing::warn!("history import: rights check failed: {e}");
                out.push(fail(
                    row,
                    "Vérification des droits impossible (détail dans le journal de l'application).",
                ));
                continue;
            }
        }
        if loaded.contains(&record.import_ref) {
            out.push(fail(
                row,
                format!("L'intervention « {} » a été importée entre-temps.", record.import_ref),
            ));
            continue;
        }
        match store(db, &record, user_id).await {
            Ok(()) => {
                loaded.insert(record.import_ref.clone());
                out.push(RowApplied {
                    row_id: row.id,
                    status: "created",
                    message: String::new(),
                });
            }
            Err(e) => {
                tracing::warn!("history import: line refused: {e}");
                out.push(fail(
                    row,
                    format!(
                        "{} : enregistrement impossible (détail dans le journal de l'application).",
                        record.import_ref
                    ),
                ));
            }
        }
    }
    Ok(out)
}

/// A stored date as the file writes it: JJ/MM/AAAA, with HH:MM when not midnight.
fn file_date(v: Option<String>) -> String {
    v.and_then(|s| NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S").ok())
        .map(|d| {
            if d.time() == chrono::NaiveTime::MIN {
                d.format("%d/%m/%Y").to_string()
            } else {
                d.format("%d/%m/%Y %H:%M").to_string()
            }
        })
        .unwrap_or_default()
}

/// The imported history in the columns of the file. Imported again, every line is skipped.
pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT h.import_ref, h.is_failure, h.started_at, h.ended_at, h.detected_at, h.stopped_at, \
                    h.restored_at, h.downtime_hours, h.description, h.work_done, h.labor_hours, h.labor_cost, \
                    h.parts_cost, h.service_cost, h.interveners_text, e.asset_id_code, t.code AS nature, \
                    m.code AS mode, c.code AS cause, k.code AS mechanism, d.code AS detection, f.code AS effect \
             FROM wo_history_records h \
             JOIN equipment e ON e.id = h.equipment_id \
             JOIN reference_values t ON t.id = h.type_id \
             LEFT JOIN reference_values m ON m.id = h.failure_mode_id \
             LEFT JOIN reference_values c ON c.id = h.failure_cause_id \
             LEFT JOIN reference_values k ON k.id = h.failure_mechanism_id \
             LEFT JOIN reference_values d ON d.id = h.detection_method_id \
             LEFT JOIN reference_values f ON f.id = h.failure_effect_id \
             ORDER BY h.started_at, h.import_ref"
                .to_string(),
        ))
        .await?;
    let num = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
    rows.iter()
        .map(|r| {
            let s = |c: &str| -> AppResult<String> { Ok(r.try_get::<Option<String>>("", c)?.unwrap_or_default()) };
            Ok(vec![
                s("import_ref")?,
                s("asset_id_code")?,
                s("nature")?,
                if r.try_get::<i64>("", "is_failure")? != 0 {
                    "oui"
                } else {
                    "non"
                }
                .to_string(),
                file_date(r.try_get("", "started_at")?),
                file_date(r.try_get("", "ended_at")?),
                file_date(r.try_get("", "detected_at")?),
                file_date(r.try_get("", "stopped_at")?),
                file_date(r.try_get("", "restored_at")?),
                num(r.try_get("", "downtime_hours")?),
                s("mode")?,
                s("cause")?,
                s("mechanism")?,
                s("detection")?,
                s("effect")?,
                s("description")?,
                s("work_done")?,
                num(r.try_get("", "labor_hours")?),
                num(r.try_get("", "labor_cost")?),
                num(r.try_get("", "parts_cost")?),
                num(r.try_get("", "service_cost")?),
                s("interveners_text")?,
            ])
        })
        .collect()
}

// ── Failure coding of an imported record (Advisor Q3) ───────────────────────────────────────
//
// An imported record is read-only except its failure coding (mode, cause, mechanism, detection
// method, effect): editable with the failure-coding right of a work order (`ot.edit`) at the
// equipment's site, at the version the screen loaded, audited. Its failure event (RAMS) gets the
// same coding. Both rows are synchronised like the import wrote them.

/// The coding to store on one history record (`None`: not coded).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HistoryCodingInput {
    pub record_id: i64,
    pub expected_row_version: i64,
    pub failure_mode_id: Option<i64>,
    pub failure_cause_id: Option<i64>,
    pub failure_mechanism_id: Option<i64>,
    pub detection_method_id: Option<i64>,
    pub failure_effect_id: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryCodingSaved {
    pub record_id: i64,
    pub row_version: i64,
}

/// `ot.edit` at the site of the record's equipment (refused when held nowhere, not found when the
/// record does not exist; equipment installed nowhere: plant-wide users only).
pub async fn check_coding_right(db: &impl ConnectionTrait, access: &AccessMap, record_id: i64) -> AppResult<()> {
    let scope = crate::wo::access::check_anywhere(access, OT_EDIT)?;
    let equipment_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id FROM wo_history_records WHERE id = ?",
            [record_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "wo_history_record".into(),
            id: record_id.to_string(),
        })?
        .try_get("", "equipment_id")?;
    let node = crate::wo::access::equipment_node(db, equipment_id).await?;
    crate::auth::scope::check_anchor(db, &scope, OT_EDIT, crate::auth::scope::Anchor::of(node)).await
}

/// `value` is an active value of the published list `domain`, or the value the record already has
/// (a value retired since stays as it was).
async fn coding_value_ok(
    db: &impl ConnectionTrait,
    domain: &str,
    value: Option<i64>,
    current: Option<i64>,
) -> AppResult<bool> {
    let Some(id) = value else { return Ok(true) };
    if Some(id) == current {
        return Ok(true);
    }
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rv.id = ? AND rd.code = ? AND rs.status = 'published' AND rv.is_active = 1",
            [id.into(), domain.into()],
        ))
        .await?
        .is_some())
}

/// Stores the coding of one record and of its failure event, in one transaction with its audit
/// event. The caller checked [`check_coding_right`].
pub async fn save_coding(
    db: &DatabaseConnection,
    input: HistoryCodingInput,
    user_id: i32,
) -> AppResult<HistoryCodingSaved> {
    let tx = db.begin().await?;
    let row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT import_ref, failure_mode_id, failure_cause_id, failure_mechanism_id, detection_method_id, \
             failure_effect_id FROM wo_history_records WHERE id = ?",
            [input.record_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "wo_history_record".into(),
            id: input.record_id.to_string(),
        })?;
    let import_ref: String = row.try_get("", "import_ref")?;
    let checks = [
        (
            "WORK.FAILURE_MODES",
            input.failure_mode_id,
            "failure_mode_id",
            "Mode de défaillance",
        ),
        (
            "WORK.FAILURE_CAUSES",
            input.failure_cause_id,
            "failure_cause_id",
            "Cause",
        ),
        (
            "WORK.FAILURE_MECHANISMS",
            input.failure_mechanism_id,
            "failure_mechanism_id",
            "Mécanisme",
        ),
        (
            "WORK.DETECTION_METHODS",
            input.detection_method_id,
            "detection_method_id",
            "Méthode de détection",
        ),
        (
            "WORK.FAILURE_EFFECTS",
            input.failure_effect_id,
            "failure_effect_id",
            "Effet",
        ),
    ];
    let mut errors = Vec::new();
    for (domain, value, column, label) in checks {
        let current: Option<i64> = row.try_get("", column)?;
        if !coding_value_ok(&tx, domain, value, current).await? {
            errors.push(format!("{label} : valeur absente de la liste active « {domain} »."));
        }
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    let ts = crate::clock::now().to_rfc3339();
    let updated = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE wo_history_records SET failure_mode_id = ?, failure_cause_id = ?, failure_mechanism_id = ?, \
             detection_method_id = ?, failure_effect_id = ?, coded_by_id = ?, coded_at = ?, \
             row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
            [
                input.failure_mode_id.into(),
                input.failure_cause_id.into(),
                input.failure_mechanism_id.into(),
                input.detection_method_id.into(),
                input.failure_effect_id.into(),
                i64::from(user_id).into(),
                ts.clone().into(),
                ts.clone().into(),
                input.record_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    crate::wo::closeout::check_concurrency(updated.rows_affected())?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE failure_events SET failure_mode_id = ?, failure_cause_id = ?, failure_effect_id = ?, \
         failure_mechanism_id = ?, row_version = row_version + 1, updated_at = ? \
         WHERE source_type = ? AND source_id = ?",
        [
            input.failure_mode_id.into(),
            input.failure_cause_id.into(),
            input.failure_effect_id.into(),
            input.failure_mechanism_id.into(),
            ts.into(),
            HISTORY_FAILURE_SOURCE.into(),
            input.record_id.into(),
        ],
    ))
    .await?;
    let record_id = input.record_id.to_string();
    let detail = json!({
        "import_ref": import_ref,
        "failure_mode_id": input.failure_mode_id,
        "failure_cause_id": input.failure_cause_id,
        "failure_mechanism_id": input.failure_mechanism_id,
        "detection_method_id": input.detection_method_id,
        "failure_effect_id": input.failure_effect_id,
    })
    .to_string();
    crate::audit::emit_in(
        &tx,
        crate::audit::AuditEvent {
            event_type: "wo_history.coding_saved",
            actor_id: Some(user_id),
            entity_type: Some("wo_history_record"),
            entity_id: Some(&record_id),
            summary: "Codification de défaillance d'un historique importé",
            detail_json: Some(detail),
            ..Default::default()
        },
    )
    .await?;
    tx.commit().await?;
    Ok(HistoryCodingSaved {
        record_id: input.record_id,
        row_version: input.expected_row_version + 1,
    })
}
