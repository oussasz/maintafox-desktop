//! Meter import ("Compteurs", L7-32 §4b; Advisor decisions 2026-11-06): the meters of existing
//! equipment, keyed by equipment code + meter code.
//!
//! - A new meter gets its type (equipment.meter_type), its unit (MEASURE.UNITS) and, when the file
//!   gives one, its initial reading recorded as a real dated reading (source IMPORT), written in the
//!   same transaction as the meter.
//! - An existing meter may get a new label. Its type or unit changes only while it has no reading;
//!   its initial reading is recorded only while it has none ("déjà relevé" otherwise).
//! - "principal = oui" makes the meter the equipment's primary meter only when it has none: a
//!   second primary meter is refused, never switched silently. The import never removes a primary
//!   flag and never deactivates a meter (the screen has no such action).

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
async fn units(db: &impl ConnectionTrait) -> AppResult<(HashMap<String, (i64, bool)>, HashSet<String>)> {
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
fn parse_number(raw: &str) -> Option<f64> {
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
        if is_primary && r.try_get::<i64>("", "is_active")? == 1 {
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
            has_reading: r.try_get::<i64>("", "has_reading")? == 1,
            name: r.try_get("", "name")?,
        };
        if stored.insert(k.clone(), m).is_some() {
            stored_twice.insert(k);
        }
    }

    let today = crate::clock::now().date_naive();
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

        // Changes on an existing meter.
        let mut record_reading = reading.is_some();
        if let Some(m) = &existing {
            if meter_type.as_ref().is_some_and(|t| *t != m.meter_type) && m.has_reading {
                messages.push(RowMessage::error(
                    "METER_IMPORT_TYPE_LOCKED",
                    "Ce compteur a déjà des relevés : son type ne change plus.",
                ));
            }
            if unit_id.is_some() && unit_id != m.unit_id && m.has_reading {
                messages.push(RowMessage::error(
                    "METER_IMPORT_UNIT_LOCKED",
                    "Ce compteur a déjà des relevés : son unité ne change plus.",
                ));
            }
            if reading.is_some() && m.has_reading {
                record_reading = false;
                messages.push(RowMessage::warning(
                    "METER_IMPORT_ALREADY_READ",
                    "Déjà relevé : le relevé initial du fichier est ignoré.",
                ));
            }
            if primary == Some(false) && m.is_primary {
                messages.push(RowMessage::error(
                    "METER_IMPORT_PRIMARY_REMOVE",
                    "Ce compteur est le compteur principal de l'équipement : l'import ne retire pas ce choix.",
                ));
            }
        }
        let make_primary = primary == Some(true) && !existing.as_ref().is_some_and(|m| m.is_primary);
        if make_primary {
            if let Some(eq) = equipment_id {
                if has_primary.contains(&eq) {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_PRIMARY_TAKEN",
                        "Cet équipement a déjà un compteur principal : changez-le à l'écran.",
                    ));
                } else if let Some(first) = primary_in_file.get(&eq) {
                    messages.push(RowMessage::error(
                        "METER_IMPORT_PRIMARY_TAKEN",
                        format!(
                            "Un autre compteur de cet équipement est déjà principal à la ligne {} du fichier.",
                            first + 2
                        ),
                    ));
                } else {
                    primary_in_file.insert(eq, i);
                }
            }
        }

        let has_error = messages.iter().any(|m| m.severity == "error");
        let ex = existing.as_ref();
        // Only the fields that change are written.
        let new_name = name.filter(|n| ex.is_none_or(|m| *n != m.name));
        let new_type = meter_type.filter(|t| ex.is_none_or(|m| *t != m.meter_type));
        let new_unit = unit_id.filter(|u| ex.is_none_or(|m| Some(*u) != m.unit_id));
        let changes = new_name.is_some() || new_type.is_some() || new_unit.is_some() || make_primary || record_reading;
        let action = match (has_error, ex) {
            (true, _) => "skip",
            (false, None) => "create",
            (false, Some(_)) if changes => "update",
            _ => "skip",
        };
        out.push(RowOutcome {
            action,
            normalized: json!({
                "equipment_id": equipment_id,
                "meter_code": cell("meter_code"),
                "existing_id": ex.map(|m| m.id),
                "name": new_name,
                "meter_type": new_type,
                "unit_id": new_unit,
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

/// Writes one line in one transaction (meter, primary flag and first reading together). Each
/// write re-checks what the preview saw, so a change made elsewhere since fails this line only.
/// `Ok(Err(text))`: refused, nothing written.
async fn write_line(
    db: &DatabaseConnection,
    n: &serde_json::Value,
    user_id: i32,
) -> AppResult<Result<i64, &'static str>> {
    let Some(equipment_id) = n["equipment_id"].as_i64() else {
        return Ok(Err("Ligne incomplète."));
    };
    let make_primary = n["make_primary"].as_bool() == Some(true);
    let now = crate::clock::now().to_rfc3339();
    let txn = db.begin().await?;
    let meter_id = match n["existing_id"].as_i64() {
        None => {
            let (Some(code), Some(name), Some(meter_type)) =
                (n["meter_code"].as_str(), n["name"].as_str(), n["meter_type"].as_str())
            else {
                return Ok(Err("Ligne incomplète."));
            };
            // No meter with this code on the equipment (letter case aside) and, for a primary
            // meter, no active primary meter: both as the preview saw them.
            let inserted = txn
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO equipment_meters \
                     (entity_sync_id, equipment_id, name, meter_code, meter_type, unit_value_id, current_reading, \
                      is_primary, is_active, created_at, updated_at) \
                     SELECT ?, e.id, ?, ?, ?, ?, 0, ?, 1, ?, ? FROM equipment e \
                     WHERE e.id = ? AND e.deleted_at IS NULL \
                       AND NOT EXISTS (SELECT 1 FROM equipment_meters m WHERE m.equipment_id = e.id \
                                       AND LOWER(TRIM(m.meter_code)) = LOWER(TRIM(?))) \
                       AND (? = 0 OR NOT EXISTS (SELECT 1 FROM equipment_meters m WHERE m.equipment_id = e.id \
                                                 AND m.is_primary = 1 AND m.is_active = 1))",
                    [
                        crate::sync::identity::new_entity_sync_id().into(),
                        name.into(),
                        code.into(),
                        meter_type.into(),
                        n["unit_id"].as_i64().into(),
                        i64::from(make_primary).into(),
                        now.clone().into(),
                        now.clone().into(),
                        equipment_id.into(),
                        code.into(),
                        i64::from(make_primary).into(),
                    ],
                ))
                .await?;
            if inserted.rows_affected() != 1 {
                txn.rollback().await?;
                return Ok(Err(
                    "L'équipement ou ses compteurs ont changé depuis la vérification du fichier : vérifiez le fichier à nouveau.",
                ));
            }
            crate::db::inserted_id(&inserted)?
        }
        Some(id) => {
            let mut sets: Vec<&str> = Vec::new();
            let mut values: Vec<sea_orm::Value> = Vec::new();
            if let Some(name) = n["name"].as_str() {
                sets.push("name = ?");
                values.push(name.into());
            }
            if let Some(t) = n["meter_type"].as_str() {
                sets.push("meter_type = ?");
                values.push(t.into());
            }
            if let Some(u) = n["unit_id"].as_i64() {
                sets.push("unit_value_id = ?");
                values.push(u.into());
            }
            if make_primary {
                sets.push("is_primary = 1");
            }
            if !sets.is_empty() {
                sets.push("updated_at = ?");
                values.push(now.clone().into());
                values.push(id.into());
                values.push(equipment_id.into());
                // A type or unit changes only while the meter has no reading; the primary flag
                // only while the equipment has no other active primary meter.
                let type_or_unit = n["meter_type"].is_string() || n["unit_id"].is_i64();
                let sql = format!(
                    "UPDATE equipment_meters SET {} WHERE id = ? AND equipment_id = ? \
                     AND ({} OR NOT EXISTS (SELECT 1 FROM asset_meter_readings mr WHERE mr.meter_id = equipment_meters.id)) \
                     AND ({} OR NOT EXISTS (SELECT 1 FROM equipment_meters o WHERE o.equipment_id = equipment_meters.equipment_id \
                                            AND o.id <> equipment_meters.id AND o.is_primary = 1 AND o.is_active = 1))",
                    sets.join(", "),
                    if type_or_unit { "0" } else { "1" },
                    if make_primary { "0" } else { "1" },
                );
                let written = txn
                    .execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
                    .await?;
                if written.rows_affected() != 1 {
                    txn.rollback().await?;
                    return Ok(Err(
                        "Ce compteur a changé depuis la vérification du fichier (relevé ou compteur principal) : vérifiez le fichier à nouveau.",
                    ));
                }
            }
            id
        }
    };

    if let (Some(value), Some(at)) = (n["reading"].as_f64(), n["reading_at"].as_str()) {
        // The meter's first reading, as the reading screen records one when there is no earlier
        // reading (`assets::meters`): a normal reading, its own lifetime value, the meter's current
        // value. Recorded only while the meter still has no reading.
        let inserted = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO asset_meter_readings \
                 (meter_id, reading_value, reading_at, source_type, source_reference, quality_flag, created_by_id, \
                  created_at, reading_kind, lifetime_value, entity_sync_id) \
                 SELECT ?, ?, ?, 'IMPORT', 'Import', 'accepted', ?, ?, 'normal', ?, ? \
                 WHERE NOT EXISTS (SELECT 1 FROM asset_meter_readings WHERE meter_id = ?)",
                [
                    meter_id.into(),
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
                "Ce compteur a reçu un relevé depuis la vérification du fichier : vérifiez le fichier à nouveau.",
            ));
        }
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_meters SET current_reading = ?, reading_offset = 0, last_read_at = ?, updated_at = ? \
             WHERE id = ?",
            [value.into(), at.into(), now.into(), meter_id.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    Ok(Ok(equipment_id))
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
