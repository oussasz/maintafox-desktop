//! Governed lists of the LOTO isolations (migration 225).
//!
//! - `PERMIT.ENERGY_TYPES` — the hazardous energy an isolation controls. A **SYSTEM** list (hidden
//!   from Données de référence, maintained by Maintafox): the sources named by ISO 14118 and
//!   OSHA 29 CFR 1910.147 (electrical, mechanical, hydraulic, pneumatic, thermal, chemical,
//!   gravitational / potential, radiation, stored / residual) plus "other".
//! - `PERMIT.ISOLATION_METHODS` — how the energy is isolated. A **COMPANY** list (category B): the
//!   starter methods below, each plant adds its own devices and procedures.
//!
//! An isolation links both (`permit_isolations.energy_type_id`, `isolation_method_id`). The texts
//! typed before migration 225 stay on the row as read-only history (`energy_type`,
//! `isolation_method` — "Saisie d'origine"); new and edited isolations write the links only and
//! leave those texts empty.
//!
//! Mapping of a text ([`map_isolation_texts`], run by migration 225 and by the startup pass for
//! rows that arrive later without links): a synonym table in French and English (accents, case,
//! punctuation ignored); an exact synonym wins, otherwise a single synonym found as whole words in
//! the text. An energy text no type matches is linked to `OTHER` and the row is flagged
//! `mapping_review_required`; a method text no method matches becomes a method of the plant
//! flagged `review_required` (never dropped). A matched spelling that differs from the value's
//! label is kept as a `legacy` alias of the value, so it stays searchable.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const ENERGY_TYPES_DOMAIN: &str = "PERMIT.ENERGY_TYPES";
pub const ISOLATION_METHODS_DOMAIN: &str = "PERMIT.ISOLATION_METHODS";
/// Energy type of a text no type matches (the row is flagged for review).
pub const ENERGY_OTHER: &str = "OTHER";

/// One shipped value with the spellings it replaces.
pub struct ListEntry {
    pub code: &'static str,
    pub label_fr: &'static str,
    pub label_en: &'static str,
    pub synonyms: &'static [&'static str],
}

/// ISO 14118 / OSHA 1910.147 energy sources.
pub const ENERGY_TYPES: &[ListEntry] = &[
    ListEntry {
        code: "ELECTRICAL",
        label_fr: "Électrique",
        label_en: "Electrical",
        synonyms: &[
            "electrique",
            "electrical",
            "electric",
            "electricite",
            "electricity",
            "elec",
            "courant",
            "tension",
            "basse tension",
            "haute tension",
            "bt",
            "ht",
            "voltage",
            "power",
        ],
    },
    ListEntry {
        code: "MECHANICAL",
        label_fr: "Mécanique",
        label_en: "Mechanical",
        synonyms: &[
            "mecanique",
            "mechanical",
            "cinetique",
            "kinetic",
            "rotation",
            "pieces en mouvement",
            "moving parts",
            "entrainement",
            "transmission",
        ],
    },
    ListEntry {
        code: "HYDRAULIC",
        label_fr: "Hydraulique",
        label_en: "Hydraulic",
        synonyms: &["hydraulique", "hydraulic", "huile sous pression", "hydraulic oil"],
    },
    ListEntry {
        code: "PNEUMATIC",
        label_fr: "Pneumatique",
        label_en: "Pneumatic",
        synonyms: &["pneumatique", "pneumatic", "air comprime", "compressed air", "air"],
    },
    ListEntry {
        code: "THERMAL",
        label_fr: "Thermique",
        label_en: "Thermal",
        synonyms: &[
            "thermique",
            "thermal",
            "chaleur",
            "heat",
            "vapeur",
            "steam",
            "chaud",
            "hot",
            "froid",
            "cold",
            "cryogenique",
            "cryogenic",
            "temperature",
        ],
    },
    ListEntry {
        code: "CHEMICAL",
        label_fr: "Chimique",
        label_en: "Chemical",
        synonyms: &[
            "chimique",
            "chemical",
            "produit chimique",
            "produits chimiques",
            "chemicals",
            "gaz",
            "gas",
            "toxique",
            "toxic",
            "corrosif",
            "corrosive",
        ],
    },
    ListEntry {
        code: "GRAVITATIONAL",
        label_fr: "Gravitationnelle / potentielle",
        label_en: "Gravitational / potential",
        synonyms: &[
            "gravitationnelle",
            "gravitationnel",
            "gravite",
            "gravity",
            "gravitational",
            "potentielle",
            "potential",
            "energie potentielle",
            "potential energy",
            "pesanteur",
            "charge suspendue",
            "suspended load",
            "chute",
        ],
    },
    ListEntry {
        code: "RADIATION",
        label_fr: "Rayonnement",
        label_en: "Radiation",
        synonyms: &[
            "rayonnement",
            "radiation",
            "radioactif",
            "radioactive",
            "ionisant",
            "ionizing",
            "laser",
            "rayons x",
            "x ray",
            "uv",
        ],
    },
    ListEntry {
        code: "STORED",
        label_fr: "Énergie stockée / résiduelle",
        label_en: "Stored / residual energy",
        synonyms: &[
            "energie stockee",
            "stored energy",
            "stored",
            "residuelle",
            "residual",
            "energie residuelle",
            "residual energy",
            "ressort",
            "spring",
            "condensateur",
            "capacitor",
            "accumulateur",
            "accumulator",
            "batterie",
            "battery",
        ],
    },
    ListEntry {
        code: ENERGY_OTHER,
        label_fr: "Autre (préciser)",
        label_en: "Other (specify)",
        synonyms: &["autre", "other", "divers", "misc"],
    },
];

/// Starter isolation methods (the plant extends the list).
pub const ISOLATION_METHODS: &[ListEntry] = &[
    ListEntry {
        code: "BREAKER_LOCKOUT",
        label_fr: "Disjoncteur / sectionneur consigné",
        label_en: "Breaker / disconnect locked out",
        synonyms: &[
            "disjoncteur",
            "sectionneur",
            "breaker",
            "circuit breaker",
            "disconnect",
            "disconnect switch",
            "interrupteur",
            "coupure electrique",
            "consignation electrique",
            "cadenas disjoncteur",
            "lockout breaker",
            "isolator",
        ],
    },
    ListEntry {
        code: "VALVE_LOCKOUT",
        label_fr: "Vanne fermée et cadenassée",
        label_en: "Valve closed and locked",
        synonyms: &[
            "vanne",
            "vanne fermee",
            "valve",
            "valve lockout",
            "robinet",
            "closed valve",
        ],
    },
    ListEntry {
        code: "BLIND",
        label_fr: "Joint plein / obturateur",
        label_en: "Blind / spade",
        synonyms: &[
            "joint plein",
            "obturateur",
            "blind",
            "spade",
            "platine",
            "blinding",
            "spectacle blind",
        ],
    },
    ListEntry {
        code: "DOUBLE_BLOCK_BLEED",
        label_fr: "Double barrière et purge",
        label_en: "Double block and bleed",
        synonyms: &[
            "double block and bleed",
            "double block bleed",
            "dbb",
            "double isolement",
            "double barriere",
        ],
    },
    ListEntry {
        code: "UNPLUG",
        label_fr: "Débranchement (prise retirée)",
        label_en: "Unplugged",
        synonyms: &[
            "debranchement",
            "debranche",
            "unplug",
            "unplugged",
            "prise retiree",
            "plug removed",
        ],
    },
    ListEntry {
        code: "MECHANICAL_BLOCK",
        label_fr: "Blocage mécanique / goupille",
        label_en: "Mechanical block / pin",
        synonyms: &[
            "blocage mecanique",
            "blocage",
            "cale",
            "calage",
            "goupille",
            "pin",
            "mechanical block",
            "block",
            "chock",
        ],
    },
    ListEntry {
        code: "BLEED_VENT",
        label_fr: "Purge / mise à l'air",
        label_en: "Bleed / vent",
        synonyms: &[
            "purge",
            "bleed",
            "vent",
            "venting",
            "mise a l air",
            "depressurisation",
            "depressurization",
            "vidange",
            "drain",
        ],
    },
    ListEntry {
        code: "OTHER",
        label_fr: "Autre (préciser)",
        label_en: "Other (specify)",
        synonyms: &["autre", "other"],
    },
];

fn strip_accent(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' => 'a',
        'ç' => 'c',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' | 'í' => 'i',
        'ô' | 'ö' | 'ó' => 'o',
        'ù' | 'û' | 'ü' | 'ú' => 'u',
        'œ' => 'o',
        _ => c,
    }
}

/// Lower case, no accents, every run of non-alphanumerics → one space.
pub fn normalize_text(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars().flat_map(char::to_lowercase).map(strip_accent) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// Code of the entry a text means: an exact synonym / code / label first, otherwise the one entry
/// whose synonym appears as whole words in the text (several candidates: no match, to review).
pub fn match_entry(entries: &[ListEntry], text: &str) -> Option<&'static str> {
    let norm = normalize_text(text);
    if norm.is_empty() {
        return None;
    }
    let spellings = |e: &ListEntry| {
        let mut all: Vec<String> = e.synonyms.iter().map(|s| normalize_text(s)).collect();
        all.push(normalize_text(e.code));
        all.push(normalize_text(e.label_fr));
        all.push(normalize_text(e.label_en));
        all
    };
    if let Some(e) = entries.iter().find(|e| spellings(e).contains(&norm)) {
        return Some(e.code);
    }
    let padded = format!(" {norm} ");
    let found: Vec<&'static str> = entries
        .iter()
        .filter(|e| e.code != ENERGY_OTHER)
        .filter(|e| {
            spellings(e)
                .iter()
                .any(|s| s.len() >= 3 && padded.contains(&format!(" {s} ")))
        })
        .map(|e| e.code)
        .collect();
    match found.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

fn seeded_value_sync_id(domain: &str, code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain}:{code}"))
}

async fn insert_value(
    db: &impl ConnectionTrait,
    domain: &str,
    code: &str,
    label: &str,
    sort_order: i64,
    metadata: &serde_json::Value,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, \
            sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, NULL, NULL, 1, ? \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?) \
          ORDER BY rs.version_no DESC \
          LIMIT 1",
        [
            seeded_value_sync_id(domain, code).into(),
            code.into(),
            label.into(),
            sort_order.into(),
            metadata.to_string().into(),
            domain.to_ascii_uppercase().into(),
            seeded_value_sync_id(domain, code).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Both lists with their shipped values. Idempotent (integrity pass and migration 225).
pub async fn ensure_permit_energy_catalogs(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        ENERGY_TYPES_DOMAIN,
        "Types d'énergie (consignation)",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    ensure_domain(
        db,
        ISOLATION_METHODS_DOMAIN,
        "Méthodes d'isolement (consignation)",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    for (domain, entries) in [
        (ENERGY_TYPES_DOMAIN, ENERGY_TYPES),
        (ISOLATION_METHODS_DOMAIN, ISOLATION_METHODS),
    ] {
        ensure_published_set(db, domain).await?;
        for (i, e) in entries.iter().enumerate() {
            let meta = serde_json::json!({ "origin": "system", "label_en": e.label_en });
            insert_value(db, domain, e.code, e.label_fr, i64::try_from(i).unwrap_or(0) + 1, &meta).await?;
        }
    }
    Ok(())
}

/// A value of `domain` (latest published set), by id: `(code, label, is_active)`.
pub async fn value_in_domain(
    db: &impl ConnectionTrait,
    domain: &str,
    id: i64,
) -> AppResult<Option<(String, String, bool)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.is_active FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE rv.id = ? AND UPPER(TRIM(d.code)) = ?",
            [id.into(), domain.to_ascii_uppercase().into()],
        ))
        .await?;
    row.map(|r| -> AppResult<(String, String, bool)> {
        let decode = |e: sea_orm::DbErr| AppError::Internal(anyhow::anyhow!("{domain} value decode: {e}"));
        Ok((
            r.try_get("", "code").map_err(decode)?,
            r.try_get("", "label").map_err(decode)?,
            r.try_get::<i64>("", "is_active").map_err(decode)? != 0,
        ))
    })
    .transpose()
}

/// The link of an isolation must be an active value of its list (server-side check of the form).
pub async fn require_active_value(
    db: &impl ConnectionTrait,
    domain: &str,
    field: &str,
    id: Option<i64>,
) -> AppResult<i64> {
    let (missing, inactive, unknown) = list_value_codes(domain);
    // Each code is bound to a variable named `gate` before the `{gate}` placeholder: gate_codes_tests
    // reads a `{gate}` placeholder as a code. Do not inline it.
    let Some(id) = id else {
        let gate = missing;
        return Err(AppError::ValidationFailed(vec![format!(
            "{gate}: {field} is required."
        )]));
    };
    match value_in_domain(db, domain, id).await? {
        Some((_, _, true)) => Ok(id),
        Some((_, _, false)) => {
            let gate = inactive;
            Err(AppError::ValidationFailed(vec![format!(
                "{gate}: {field}: this value is no longer active in {domain}."
            )]))
        }
        None => {
            let gate = unknown;
            Err(AppError::ValidationFailed(vec![format!(
                "{gate}: {field}: not a value of {domain}."
            )]))
        }
    }
}

/// The gate codes of an isolation link refused by [`require_active_value`]: missing, no longer
/// active, not a value of the list. The isolation method list has its own codes, the energy type
/// list (the only other list checked here) the others.
fn list_value_codes(domain: &str) -> (&'static str, &'static str, &'static str) {
    if domain == ISOLATION_METHODS_DOMAIN {
        (
            "GATE_PERMIT_ISOLATION_METHOD_REQUIRED",
            "GATE_PERMIT_ISOLATION_METHOD_INACTIVE",
            "GATE_PERMIT_ISOLATION_METHOD_UNKNOWN",
        )
    } else {
        (
            "GATE_PERMIT_ENERGY_TYPE_REQUIRED",
            "GATE_PERMIT_ENERGY_TYPE_INACTIVE",
            "GATE_PERMIT_ENERGY_TYPE_UNKNOWN",
        )
    }
}

async fn value_id_by_code(db: &impl ConnectionTrait, domain: &str, code: &str) -> AppResult<Option<i64>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id AS id FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [domain.to_ascii_uppercase().into(), code.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()?)
}

/// Methods of the plant whose label is the text (a value created by an earlier mapping run).
async fn method_by_label(db: &impl ConnectionTrait, text: &str) -> AppResult<Option<i64>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id AS id, rv.label AS label FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ?",
            [ISOLATION_METHODS_DOMAIN.into()],
        ))
        .await?;
    let norm = normalize_text(text);
    for r in rows {
        let label: String = r.try_get("", "label")?;
        if normalize_text(&label) == norm {
            return Ok(Some(r.try_get("", "id")?));
        }
    }
    Ok(None)
}

/// Code of a plant method created from a text: `M_<TEXT>` (ASCII), made unique with `_2`, `_3`…
/// Derived from the text only, so every device mapping the same text creates the same value.
fn method_code_for_text(text: &str) -> String {
    let mut code = String::from("M_");
    for c in normalize_text(text).chars() {
        if c.is_ascii_alphanumeric() {
            code.push(c.to_ascii_uppercase());
        } else if !code.ends_with('_') {
            code.push('_');
        }
    }
    code.truncate(60);
    code.trim_end_matches('_').to_string()
}

async fn create_review_method(db: &impl ConnectionTrait, text: &str) -> AppResult<i64> {
    let base = method_code_for_text(text);
    let mut code = base.clone();
    let mut n = 2;
    while value_id_by_code(db, ISOLATION_METHODS_DOMAIN, &code).await?.is_some() {
        code = format!("{base}_{n}");
        n += 1;
    }
    let next: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(rv.sort_order), 0) + 1 AS next FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id WHERE UPPER(TRIM(d.code)) = ?",
            [ISOLATION_METHODS_DOMAIN.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "next"))
        .transpose()?
        .unwrap_or(1);
    let meta = serde_json::json!({
        "origin": "migrated",
        "review_required": true,
        "source": "permit_isolations.isolation_method",
    });
    insert_value(db, ISOLATION_METHODS_DOMAIN, &code, text.trim(), next, &meta).await?;
    tracing::warn!(code = %code, "isolation method created from a typed text, to review in reference data");
    value_id_by_code(db, ISOLATION_METHODS_DOMAIN, &code)
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "{ISOLATION_METHODS_DOMAIN} value '{code}' missing after insert"
            ))
        })
}

/// Keeps a typed spelling as a `legacy` alias of the value it was mapped onto.
async fn keep_text_as_alias(db: &impl ConnectionTrait, domain: &str, value_id: i64, text: &str) -> AppResult<()> {
    let text = text.trim();
    let Some((code, label, _)) = value_in_domain(db, domain, value_id).await? else {
        return Ok(());
    };
    if normalize_text(text) == normalize_text(&label) || normalize_text(text) == normalize_text(&code) {
        return Ok(());
    }
    let sync_id =
        crate::sync::identity::seeded_entity_sync_id("reference_aliases", &format!("{domain}:{code}:legacy:fr:{text}"));
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_aliases \
         (entity_sync_id, reference_value_id, alias_label, locale, alias_type, is_preferred, created_at) \
         SELECT ?, ?, ?, 'fr', 'legacy', 0, strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE NOT EXISTS (SELECT 1 FROM reference_aliases \
                             WHERE reference_value_id = ? AND locale = 'fr' AND alias_type = 'legacy' AND alias_label = ?)",
        [sync_id.into(), value_id.into(), text.into(), value_id.into(), text.into()],
    ))
    .await?;
    Ok(())
}

/// What the mapping did (migration log, tests).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MappingReport {
    pub rows: u64,
    pub to_review: u64,
    pub methods_created: u64,
}

/// Links every isolation that has no energy / method link yet from its typed texts. The texts stay
/// on the row (history). Rows already linked are left alone, so a repeated run changes nothing.
pub async fn map_isolation_texts(db: &impl ConnectionTrait) -> AppResult<MappingReport> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, energy_type, isolation_method, energy_type_id, isolation_method_id \
               FROM permit_isolations \
              WHERE (energy_type_id IS NULL AND TRIM(COALESCE(energy_type, '')) <> '') \
                 OR (isolation_method_id IS NULL AND TRIM(COALESCE(isolation_method, '')) <> '') \
                 OR ((energy_type_id IS NULL OR isolation_method_id IS NULL) AND mapping_review_required = 0) \
              ORDER BY id"
                .to_string(),
        ))
        .await?;
    let mut report = MappingReport::default();
    if rows.is_empty() {
        return Ok(report);
    }
    let other_energy = value_id_by_code(db, ENERGY_TYPES_DOMAIN, ENERGY_OTHER)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("{ENERGY_TYPES_DOMAIN}: value {ENERGY_OTHER} missing")))?;
    let mut methods: HashMap<String, (i64, bool)> = HashMap::new();
    for row in rows {
        let id: i64 = row.try_get("", "id")?;
        let energy_text: String = row.try_get::<Option<String>>("", "energy_type")?.unwrap_or_default();
        let method_text: String = row
            .try_get::<Option<String>>("", "isolation_method")?
            .unwrap_or_default();
        let current_energy: Option<i64> = row.try_get("", "energy_type_id")?;
        let current_method: Option<i64> = row.try_get("", "isolation_method_id")?;
        let mut review = false;

        let energy_id = match current_energy {
            Some(e) => Some(e),
            None if energy_text.trim().is_empty() => {
                review = true;
                None
            }
            None => match match_entry(ENERGY_TYPES, &energy_text) {
                Some(code) => {
                    let vid = value_id_by_code(db, ENERGY_TYPES_DOMAIN, code).await?;
                    if let Some(v) = vid {
                        keep_text_as_alias(db, ENERGY_TYPES_DOMAIN, v, &energy_text).await?;
                    }
                    vid
                }
                None => {
                    review = true;
                    Some(other_energy)
                }
            },
        };

        let method_id = match current_method {
            Some(m) => Some(m),
            None if method_text.trim().is_empty() => {
                review = true;
                None
            }
            None => {
                let key = normalize_text(&method_text);
                let (vid, created) = if let Some(hit) = methods.get(&key) {
                    *hit
                } else if let Some(code) = match_entry(ISOLATION_METHODS, &method_text) {
                    let vid = value_id_by_code(db, ISOLATION_METHODS_DOMAIN, code)
                        .await?
                        .ok_or_else(|| {
                            AppError::Internal(anyhow::anyhow!("{ISOLATION_METHODS_DOMAIN}: {code} missing"))
                        })?;
                    (vid, false)
                } else if let Some(vid) = method_by_label(db, &method_text).await? {
                    (vid, false)
                } else {
                    report.methods_created += 1;
                    (create_review_method(db, &method_text).await?, true)
                };
                methods.insert(key, (vid, created));
                if created {
                    review = true;
                } else {
                    keep_text_as_alias(db, ISOLATION_METHODS_DOMAIN, vid, &method_text).await?;
                }
                Some(vid)
            }
        };

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_isolations SET energy_type_id = ?, isolation_method_id = ?, \
                    mapping_review_required = CASE WHEN ? = 1 THEN 1 ELSE mapping_review_required END \
              WHERE id = ?",
            [energy_id.into(), method_id.into(), i64::from(review).into(), id.into()],
        ))
        .await?;
        report.rows += 1;
        if review {
            report.to_review += 1;
        }
    }
    if report.rows > 0 {
        tracing::info!(?report, "permit isolations linked to the energy / method lists");
    }
    Ok(report)
}

/// One choice of an isolation form (active value of the latest published set).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct IsolationOption {
    pub id: i64,
    pub code: String,
    pub label: String,
    /// English label of a shipped value (`metadata_json.label_en`), when known.
    pub label_en: Option<String>,
    /// A value created from a typed text, still to be reviewed in Données de référence.
    pub review_required: bool,
}

/// Choices of the isolation form: the energy types and the plant's isolation methods.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IsolationOptions {
    pub energy_types: Vec<IsolationOption>,
    pub isolation_methods: Vec<IsolationOption>,
}

async fn active_options(db: &impl ConnectionTrait, domain: &str) -> AppResult<Vec<IsolationOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code, rv.label, rv.metadata_json FROM reference_values rv \
              WHERE rv.is_active = 1 AND rv.set_id = ( \
                    SELECT rs.id FROM reference_sets rs \
                      JOIN reference_domains d ON d.id = rs.domain_id \
                     WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' \
                     ORDER BY rs.version_no DESC LIMIT 1) \
              ORDER BY rv.sort_order, rv.label",
            [domain.to_ascii_uppercase().into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let meta: Option<String> = r.try_get("", "metadata_json")?;
            let meta: serde_json::Value = meta
                .as_deref()
                .and_then(|m| serde_json::from_str(m).ok())
                .unwrap_or(serde_json::Value::Null);
            Ok(IsolationOption {
                id: r.try_get("", "id")?,
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
                label_en: meta.get("label_en").and_then(|v| v.as_str()).map(str::to_string),
                review_required: meta.get("review_required").and_then(serde_json::Value::as_bool) == Some(true),
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

pub async fn list_isolation_options(db: &impl ConnectionTrait) -> AppResult<IsolationOptions> {
    Ok(IsolationOptions {
        energy_types: active_options(db, ENERGY_TYPES_DOMAIN).await?,
        isolation_methods: active_options(db, ISOLATION_METHODS_DOMAIN).await?,
    })
}

/// Id of a list value by code (tests building isolations).
#[cfg(test)]
pub async fn value_id_for_tests(db: &impl ConnectionTrait, domain: &str, code: &str) -> i64 {
    value_id_by_code(db, domain, code)
        .await
        .expect("list value query")
        .unwrap_or_else(|| panic!("{domain}:{code} must be seeded"))
}
