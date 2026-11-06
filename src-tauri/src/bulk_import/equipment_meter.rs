//! Meter import ("Compteurs", L7-32 §4b; Advisor decisions 2026-11-06): the meters of existing
//! equipment, keyed by equipment code + meter code.
//!
//! - A new meter gets its type (equipment.meter_type), its unit (MEASURE.UNITS) and, when the file
//!   gives one, its initial reading recorded as a real dated reading (source IMPORT), written in the
//!   same transaction as the meter.
//! - An existing meter is never edited (the screen has no meter edit yet; Advisor 2026-11-06): the
//!   import only records its first dated reading while it has none ("déjà relevé" otherwise).
//!   Values identical to the meter's are "inchangé"; a different label, type, unit or primary
//!   flag refuses the line. A line on an inactive meter is refused (the screen takes no reading
//!   on it).
//! - "principal = oui" makes a new meter the equipment's primary meter only when it has none: a
//!   second primary meter is refused, never switched silently.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::json;

use super::org_tree::{key, parse_active};
use super::{Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::AppResult;

pub const SPEC: EntitySpec = EntitySpec {
    code: "equipment_meter",
    label: "Compteurs des équipements",
    order: 4,
    needs: "Équipements, la liste de référence MEASURE.UNITS (unités) et les types de compteur.",
    permission: crate::rbac::permissions::EQ_MANAGE,
    columns: &[
        Column {
            key: "equipment_code",
            header: "code équipement",
            required: true,
            aliases: &["equipement", "code_equipement", "asset_code", "equipment_code"],
            hint: "Code d'un équipement existant.",
        },
        Column {
            key: "meter_code",
            header: "code compteur",
            required: true,
            aliases: &["compteur", "code_compteur", "meter_code"],
            hint: "Code du compteur, unique pour l'équipement (ex. HRS-001).",
        },
        Column {
            key: "name",
            header: "libellé",
            required: true,
            aliases: &["libelle", "nom", "name"],
            hint: "Nom du compteur (ex. Heures de marche).",
        },
        Column {
            key: "meter_type",
            header: "type de compteur",
            required: true,
            aliases: &["type", "type_compteur", "meter_type"],
            hint: "HOURS (Heures), CYCLES, DISTANCE, VOLUME ou CUSTOM (Personnalisé) : le code ou le libellé.",
        },
        Column {
            key: "unit",
            header: "unité",
            required: true,
            aliases: &["unite", "unit"],
            hint: "Code ou symbole d'une unité active de la liste de référence MEASURE.UNITS (h, km…).",
        },
        Column {
            key: "initial_reading",
            header: "relevé initial",
            required: false,
            aliases: &["releve_initial", "releve", "initial_reading"],
            hint: "Valeur du compteur à la date du relevé initial (nombre, virgule ou point). Ignoré si le compteur a déjà un relevé.",
        },
        Column {
            key: "reading_date",
            header: "date du relevé initial",
            required: false,
            aliases: &["date_releve", "date_releve_initial", "reading_date"],
            hint: "JJ/MM/AAAA. Obligatoire avec un relevé initial ; pas dans le futur.",
        },
        Column {
            key: "primary",
            header: "principal",
            required: false,
            aliases: &["compteur_principal", "is_primary", "primary"],
            hint: "oui / non. Un équipement n'a qu'un compteur principal : un second est refusé.",
        },
    ],
};

/// A meter already recorded, as the preview sees it.
#[derive(Clone)]
struct Stored {
    id: i64,
    meter_type: String,
    unit_id: Option<i64>,
    is_primary: bool,
    is_active: bool,
    has_reading: bool,
    name: String,
}

/// Live equipment by code key; codes held twice (letter case aside) are returned apart.
async fn equipment_by_code(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, i64>, HashSet<String>)> {
    let mut out = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, asset_id_code FROM equipment WHERE deleted_at IS NULL AND asset_id_code IS NOT NULL"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "asset_id_code")?;
        if out.insert(key(&code), r.try_get("", "id")?).is_some() {
            ambiguous.insert(key(&code));
        }
    }
    Ok((out, ambiguous))
}

/// Active meter types by code key and by label key, mapped to their code.
async fn meter_types(db: &impl ConnectionTrait) -> AppResult<HashMap<String, String>> {
    let mut out = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT lv.code, lv.label, lv.fr_label, lv.en_label FROM lookup_values lv \
             JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'equipment.meter_type' AND lv.is_active = 1 AND lv.deleted_at IS NULL \
             ORDER BY lv.sort_order, lv.id"
                .to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        out.insert(key(&code), code.clone());
        for col in ["label", "fr_label", "en_label"] {
            if let Some(label) = r.try_get::<Option<String>>("", col)? {
                out.entry(key(&label)).or_insert_with(|| code.clone());
            }
        }
    }
    Ok(out)
}

/// Units of the published MEASURE.UNITS list by code key and by symbol key: (id, active).
/// A key two different units answer to is returned apart.
pub(super) async fn units(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, (i64, bool)>, HashSet<String>)> {
    let mut out: HashMap<String, (i64, bool)> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT u.id, u.code, u.is_active, {} AS symbol FROM reference_values u \
                 JOIN reference_sets rs ON rs.id = u.set_id AND rs.status = 'published' \
                 JOIN reference_domains d ON d.id = rs.domain_id AND UPPER(TRIM(d.code)) = 'MEASURE.UNITS' \
                 ORDER BY rs.version_no DESC, u.id",
                crate::reference::units::symbol_sql("u")
            ),
        ))
        .await?
    {
        let id: i64 = r.try_get("", "id")?;
        let active = r.try_get::<i64>("", "is_active")? == 1;
        let code: String = r.try_get("", "code")?;
        let symbol: Option<String> = r.try_get("", "symbol")?;
        let mut keys = vec![key(&code)];
        if let Some(s) = symbol {
            if key(&s) != key(&code) {
                keys.push(key(&s));
            }
        }
        for k in keys {
            match out.get(&k) {
                Some((other, _)) if *other != id => {
                    ambiguous.insert(k);
                }
                Some(_) => {}
                None => {
                    out.insert(k, (id, active));
                }
            }
        }
    }
    Ok((out, ambiguous))
}

/// A decimal written with a comma or a dot.
pub(super) fn parse_number(raw: &str) -> Option<f64> {
    raw.trim()
        .replace(' ', "")
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// The reading time stored for a reading dated `day`: the start of that day, plant local time.
fn reading_at(day: chrono::NaiveDate) -> String {
    use chrono::TimeZone;
    let midnight = day.and_hms_opt(0, 0, 0).expect("midnight");
    chrono::Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|t| t.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc.from_utc_datetime(&midnight).to_rfc3339())
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let (equipment, ambiguous_equipment) = equipment_by_code(db).await?;
    let types = meter_types(db).await?;
    let (unit_keys, ambiguous_units) = units(db).await?;

    // Meters by (equipment, meter code key); a code held twice on one equipment cannot be updated.
    let mut stored: HashMap<(i64, String), Stored> = HashMap::new();
    let mut stored_twice: HashSet<(i64, String)> = HashSet::new();
    // Equipment that already has an active primary meter.
    let mut has_primary: HashSet<i64> = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT em.id, em.equipment_id, em.meter_code, em.name, em.meter_type, em.unit_value_id, \
                    em.is_primary, em.is_active, \
                    EXISTS (SELECT 1 FROM asset_meter_readings mr WHERE mr.meter_id = em.id) AS has_reading \
             FROM equipment_meters em"
                .to_string(),
        ))
        .await?
    {
        let equipment_id: i64 = r.try_get("", "equipment_id")?;
        let is_primary = r.try_get::<i64>("", "is_primary")? == 1;
        let is_active = r.try_get::<i64>("", "is_active")? == 1;
        if is_primary && is_active {
            has_primary.insert(equipment_id);
        }
        let Some(code) = r.try_get::<Option<String>>("", "meter_code")? else {
            continue;
        };
        let k = (equipment_id, key(&code));
        let m = Stored {
            id: r.try_get("", "id")?,
            meter_type: r.try_get("", "meter_type")?,
            unit_id: r.try_get("", "unit_value_id")?,
            is_primary,
            is_active,
            has_reading: r.try_get::<i64>("", "has_reading")? == 1,
            name: r.try_get("", "name")?,
        };
        if stored.insert(k.clone(), m).is_some() {
            stored_twice.insert(k);
        }
    }

    // The file's dates are plant-local days (see `reading_at`).
    let today = crate::clock::now_local().date_naive();
    let mut first_row_of: HashMap<(i64, String), usize> = HashMap::new();
    // Equipment made primary by an earlier line of the file.
    let mut primary_in_file: HashMap<i64, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let cell = |k: &str| raw.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());

        let equipment_id = match cell("equipment_code") {
            None => {
                messages.push(RowMessage::error(
                    "METER_IMPORT_EQUIPMENT",
                    "Le code équipement est obligatoire.",
                ));
                None
            }
            Some(c) if ambiguous_equipment.contains(&key(c)) => {
                messages.push(RowMessage::error(
                    "METER_IMPORT_EQUIPMENT_AMBIGUOUS",
                    format!(
                        "Plusieurs équipements ont le code « {c} » à la casse près : corrigez-les dans Équipements."
                    ),
                ));
                None
            }
            Some(c) => {
                let found = equipment.get(&key(c)).copied();
                if found.is_none() {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_EQUIPMENT",
                        format!("L'équipement « {c} » n'existe pas : importez d'abord les équipements."),
                    ));
                }
                found
            }
        };
        let meter_key = cell("meter_code").map(key);
        if meter_key.is_none() {
            messages.push(RowMessage::error(
                "METER_IMPORT_CODE",
                "Le code compteur est obligatoire.",
            ));
        }
        let name = cell("name").map(str::to_string);
        if name.is_none() {
            messages.push(RowMessage::error("METER_IMPORT_NAME", "Le libellé est obligatoire."));
        }
        let existing = equipment_id
            .zip(meter_key.clone())
            .and_then(|k| stored.get(&k).cloned());
        if let Some(k) = equipment_id.zip(meter_key.clone()) {
            if stored_twice.contains(&k) {
                messages.push(RowMessage::error(
                    "METER_IMPORT_CODE_TWICE",
                    "Cet équipement a deux compteurs avec ce code à la casse près : corrigez-les dans Équipements.",
                ));
            }
            if let Some(first) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "METER_IMPORT_DUPLICATE",
                    format!("Ce compteur apparaît déjà à la ligne {} du fichier.", first + 2),
                ));
            } else {
                first_row_of.insert(k, i);
            }
        }

        let meter_type = match cell("meter_type") {
            None => {
                messages.push(RowMessage::error(
                    "METER_IMPORT_TYPE",
                    "Le type de compteur est obligatoire.",
                ));
                None
            }
            Some(t) => {
                let found = types.get(&key(t)).cloned();
                if found.is_none() {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_TYPE",
                        format!(
                            "Le type de compteur « {t} » n'existe pas : HOURS, CYCLES, DISTANCE, VOLUME ou CUSTOM."
                        ),
                    ));
                }
                found
            }
        };
        let unit_id = match cell("unit") {
            None => {
                messages.push(RowMessage::error("METER_IMPORT_UNIT", "L'unité est obligatoire."));
                None
            }
            Some(u) if ambiguous_units.contains(&key(u)) => {
                messages.push(RowMessage::error(
                    "METER_IMPORT_UNIT_AMBIGUOUS",
                    format!("Plusieurs unités répondent à « {u} » : indiquez le code de l'unité."),
                ));
                None
            }
            Some(u) => match unit_keys.get(&key(u)).copied() {
                None => {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_UNIT",
                        format!("L'unité « {u} » n'existe pas dans la liste de référence MEASURE.UNITS : ajoutez-la d'abord dans Données de référence."),
                    ));
                    None
                }
                // A meter keeps the unit it already has, even if the plant retired it since.
                Some((id, active)) => {
                    if !active && existing.as_ref().and_then(|m| m.unit_id) != Some(id) {
                        messages.push(RowMessage::error(
                            "METER_IMPORT_UNIT_INACTIVE",
                            format!("L'unité « {u} » n'est plus active dans les Données de référence."),
                        ));
                    }
                    Some(id)
                }
            },
        };

        let reading = match cell("initial_reading") {
            None => None,
            Some(v) => {
                let parsed = parse_number(v).filter(|x| *x >= 0.0);
                if parsed.is_none() {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_READING",
                        format!("Le relevé initial « {v} » doit être un nombre positif ou nul."),
                    ));
                }
                parsed
            }
        };
        let reading_day = match super::parse_date(cell("reading_date")) {
            Ok(d) => d,
            Err(()) => {
                messages.push(RowMessage::error(
                    "METER_IMPORT_DATE",
                    "La date du relevé initial doit être au format JJ/MM/AAAA.",
                ));
                None
            }
        };
        if reading_day.is_some_and(|d| d > today) {
            messages.push(RowMessage::error(
                "METER_IMPORT_DATE_FUTURE",
                "La date du relevé initial ne peut pas être dans le futur.",
            ));
        }
        if cell("initial_reading").is_some() && cell("reading_date").is_none() {
            messages.push(RowMessage::error(
                "METER_IMPORT_DATE",
                "La date du relevé initial est obligatoire avec un relevé initial.",
            ));
        }
        if cell("initial_reading").is_none() && cell("reading_date").is_some() {
            messages.push(RowMessage::error(
                "METER_IMPORT_READING",
                "Une date de relevé est donnée sans relevé initial.",
            ));
        }

        let primary = match parse_active(raw.get("primary").filter(|s| !s.trim().is_empty())) {
            Ok(p) => p,
            Err(()) => {
                messages.push(RowMessage::error(
                    "METER_IMPORT_PRIMARY",
                    "La colonne principal doit valoir oui ou non.",
                ));
                None
            }
        };

        // An existing meter is not edited by an import: identical values are "inchangé", a
        // different one refuses the line; only its first reading may be recorded.
        let mut record_reading = reading.is_some();
        if let Some(m) = &existing {
            if !m.is_active {
                messages.push(RowMessage::error(
                    "METER_IMPORT_INACTIVE",
                    "Ce compteur est désactivé : il ne reçoit plus de relevé.",
                ));
            }
            let differs = name.as_deref().is_some_and(|n| n != m.name)
                || meter_type.as_ref().is_some_and(|t| *t != m.meter_type)
                || (unit_id.is_some() && unit_id != m.unit_id)
                || primary.is_some_and(|p| p != m.is_primary);
            if differs {
                messages.push(RowMessage::error(
                    "METER_IMPORT_EXISTING_CHANGE",
                    "Un compteur existant ne se modifie pas par import : il n'y a pas encore de modification de compteur à l'écran.",
                ));
            }
            if reading.is_some() && m.has_reading {
                record_reading = false;
                messages.push(RowMessage::warning(
                    "METER_IMPORT_ALREADY_READ",
                    "Déjà relevé : le relevé initial du fichier est ignoré.",
                ));
            }
        }
        let make_primary = existing.is_none() && primary == Some(true);
        if make_primary {
            if let Some(eq) = equipment_id {
                if has_primary.contains(&eq) {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_PRIMARY_TAKEN",
                        "Cet équipement a déjà un compteur principal : un second n'est pas possible.",
                    ));
                } else if let Some(first) = primary_in_file.get(&eq) {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_PRIMARY_TAKEN",
                        format!(
                            "Un autre compteur de cet équipement est déjà principal à la ligne {} du fichier.",
                            first + 2
                        ),
                    ));
                }
            }
        }

        let has_error = messages.iter().any(|m| m.severity == "error");
        // Only a line that will be written claims the equipment's primary flag.
        if make_primary && !has_error {
            if let Some(eq) = equipment_id {
                primary_in_file.insert(eq, i);
            }
        }
        let ex = existing.as_ref();
        let action = match (has_error, ex) {
            (true, _) => "skip",
            (false, None) => "create",
            (false, Some(_)) if record_reading => "update",
            _ => "skip",
        };
        out.push(RowOutcome {
            action,
            normalized: json!({
                "equipment_id": equipment_id,
                "meter_code": cell("meter_code"),
                "existing_id": ex.map(|m| m.id),
                "name": if ex.is_none() { name } else { None },
                "meter_type": if ex.is_none() { meter_type } else { None },
                "unit_id": if ex.is_none() { unit_id } else { None },
                "make_primary": make_primary,
                "reading": if record_reading { reading } else { None },
                "reading_at": if record_reading { reading_day.map(reading_at) } else { None },
            }),
            messages,
        });
    }
    Ok(out)
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow], user_id: i32) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        let n = &row.normalized;
        let (status, message) = match row.action.as_str() {
            "create" | "update" => match write_line(db, n, user_id).await? {
                Ok(equipment_id) => {
                    if n["reading"].as_f64().is_some() {
                        // As after a reading entered on the screen: never fails the line.
                        crate::pm::generation_control::try_generate_pm_for_equipment(db, equipment_id).await;
                    }
                    let done = if n["existing_id"].is_null() {
                        "created"
                    } else {
                        "updated"
                    };
                    (done, String::new())
                }
                Err(text) => ("failed", text.to_string()),
            },
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

/// Writes one line in one transaction (a new meter and its first reading together, or the first
/// reading of an existing meter). Each write re-checks what the preview saw, so a change made
/// elsewhere since fails this line only. `Ok(Err(text))`: refused, nothing written.
async fn write_line(
    db: &DatabaseConnection,
    n: &serde_json::Value,
    user_id: i32,
) -> AppResult<Result<i64, &'static str>> {
    let Some(equipment_id) = n["equipment_id"].as_i64() else {
        return Ok(Err("Ligne incomplète."));
    };
    let now = crate::clock::now().to_rfc3339();
    let txn = db.begin().await?;
    let meter_id = match n["existing_id"].as_i64() {
        None => match insert_meter(&txn, equipment_id, n, &now).await? {
            Ok(id) => id,
            Err(text) => {
                txn.rollback().await?;
                return Ok(Err(text));
            }
        },
        Some(id) => id,
    };

    if let (Some(value), Some(at)) = (n["reading"].as_f64(), n["reading_at"].as_str()) {
        // The meter's first reading, as the reading screen records one when there is no earlier
        // reading (`assets::meters`): a normal reading, its own lifetime value, the meter's current
        // value. Recorded only on an active meter that still has no reading.
        let inserted = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO asset_meter_readings                  (meter_id, reading_value, reading_at, source_type, source_reference, quality_flag, created_by_id,                   created_at, reading_kind, lifetime_value, entity_sync_id)                  SELECT m.id, ?, ?, 'IMPORT', 'Import', 'accepted', ?, ?, 'normal', ?, ?                  FROM equipment_meters m WHERE m.id = ? AND m.is_active = 1                    AND NOT EXISTS (SELECT 1 FROM asset_meter_readings r WHERE r.meter_id = m.id)",
                [
                    value.into(),
                    at.into(),
                    i64::from(user_id).into(),
                    now.clone().into(),
                    value.into(),
                    crate::sync::identity::new_entity_sync_id().into(),
                    meter_id.into(),
                ],
            ))
            .await?;
        if inserted.rows_affected() != 1 {
            txn.rollback().await?;
            return Ok(Err(
                "Ce compteur a reçu un relevé ou a été désactivé depuis la vérification du fichier : vérifiez le fichier à nouveau.",
            ));
        }
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_meters SET current_reading = ?, reading_offset = 0, last_read_at = ?, updated_at = ?              WHERE id = ?",
            [value.into(), at.into(), now.into(), meter_id.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    Ok(Ok(equipment_id))
}

/// Inserts a new meter inside the line's transaction, re-checking what the preview saw: the
/// equipment still live, the code still free on it (compared with `key()`, letter case aside),
/// the type and the unit still active, and, for a primary meter, no active primary meter yet.
async fn insert_meter(
    txn: &impl ConnectionTrait,
    equipment_id: i64,
    n: &serde_json::Value,
    now: &str,
) -> AppResult<Result<i64, &'static str>> {
    let (Some(code), Some(name), Some(meter_type), Some(unit_id)) = (
        n["meter_code"].as_str(),
        n["name"].as_str(),
        n["meter_type"].as_str(),
        n["unit_id"].as_i64(),
    ) else {
        return Ok(Err("Ligne incomplète."));
    };
    let changed = "L'équipement, ses compteurs, le type ou l'unité ont changé depuis la vérification du fichier : vérifiez le fichier à nouveau.";
    for r in txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT meter_code FROM equipment_meters WHERE equipment_id = ? AND meter_code IS NOT NULL",
            [equipment_id.into()],
        ))
        .await?
    {
        if key(&r.try_get::<String>("", "meter_code")?) == key(code) {
            return Ok(Err(changed));
        }
    }
    let make_primary = i64::from(n["make_primary"].as_bool() == Some(true));
    let inserted = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO equipment_meters              (entity_sync_id, equipment_id, name, meter_code, meter_type, unit_value_id, current_reading,               is_primary, is_active, created_at, updated_at)              SELECT ?, e.id, ?, ?, ?, ?, 0, ?, 1, ?, ? FROM equipment e              WHERE e.id = ? AND e.deleted_at IS NULL                AND EXISTS (SELECT 1 FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id                            WHERE ld.domain_key = 'equipment.meter_type' AND lv.code = ?                              AND lv.is_active = 1 AND lv.deleted_at IS NULL)                AND EXISTS (SELECT 1 FROM reference_values u JOIN reference_sets rs ON rs.id = u.set_id                            WHERE u.id = ? AND u.is_active = 1 AND rs.status = 'published')                AND (? = 0 OR NOT EXISTS (SELECT 1 FROM equipment_meters m WHERE m.equipment_id = e.id                                          AND m.is_primary = 1 AND m.is_active = 1))",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                name.into(),
                code.into(),
                meter_type.into(),
                unit_id.into(),
                make_primary.into(),
                now.into(),
                now.into(),
                equipment_id.into(),
                meter_type.into(),
                unit_id.into(),
                make_primary.into(),
            ],
        ))
        .await?;
    if inserted.rows_affected() != 1 {
        return Ok(Err(changed));
    }
    Ok(Ok(crate::db::inserted_id(&inserted)?))
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT e.asset_id_code, em.meter_code, em.name, em.meter_type, {} AS unit, em.current_reading, \
                        em.last_read_at, em.is_primary \
                 FROM equipment_meters em \
                 JOIN equipment e ON e.id = em.equipment_id AND e.deleted_at IS NULL \
                 LEFT JOIN reference_values mu ON mu.id = em.unit_value_id \
                 WHERE em.is_active = 1 \
                 ORDER BY e.asset_id_code, em.meter_code",
                crate::reference::units::symbol_sql("mu")
            ),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let read_at: Option<String> = r.try_get("", "last_read_at")?;
            let day = read_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string());
            let value = match day {
                Some(_) => r
                    .try_get::<Option<f64>>("", "current_reading")?
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                None => String::new(),
            };
            Ok(vec![
                r.try_get::<Option<String>>("", "asset_id_code")?.unwrap_or_default(),
                r.try_get::<Option<String>>("", "meter_code")?.unwrap_or_default(),
                r.try_get("", "name")?,
                r.try_get("", "meter_type")?,
                r.try_get::<Option<String>>("", "unit")?.unwrap_or_default(),
                value,
                day.unwrap_or_default(),
                if r.try_get::<i64>("", "is_primary")? == 1 {
                    "oui"
                } else {
                    "non"
                }
                .to_string(),
            ])
        })
        .collect()
}
