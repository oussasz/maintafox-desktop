//! Starter catalogue of certifications / authorisations and maintenance skills, written in the
//! tenant's language (French plants see « Habilitation électrique B2V », English ones
//! "Electrical authorisation B2V").
//!
//! Electrical habilitations (NF C 18-510) are a company reference list, `PERSONNEL.HABILITATIONS`
//! (controlled catalogue: draft → publish), seeded once with the standard symbols. Each published
//! symbol has a certification type `HAB-<symbol>` so it can be required by a job and recorded on a
//! person's sheet with its expiry (renewal every 3 years by default, editable). They are separate
//! from the four rights the program grants (`AuthorizationType`: permit issuer, isolation
//! authority, inspector, warehouse sign-off).
//!
//! Runs with the system reference catalogue at startup and on licence activation. Only adds
//! what is missing (by code); a type or skill the plant renamed, deactivated or removed is never
//! put back the way it was. Skills seeded earlier with their English label are relabelled in a
//! French tenant only while they still carry that untouched seed label.
//!
//! Validity periods follow common French practice (NF C18-510 electrical habilitation and
//! CATEC / work-at-height / LOTO refreshers every 3 years, SST every 2 years, CACES every 5 years,
//! ISO 9606-1 welder qualification confirmed every 2 years) and stay editable per tenant.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;

struct CertSeed {
    code: &'static str,
    category: &'static str,
    validity_months: i64,
    renewal_lead_days: i64,
    fr: &'static str,
    en: &'static str,
}

const fn cert(
    code: &'static str,
    category: &'static str,
    validity_months: i64,
    renewal_lead_days: i64,
    fr: &'static str,
    en: &'static str,
) -> CertSeed {
    CertSeed {
        code,
        category,
        validity_months,
        renewal_lead_days,
        fr,
        en,
    }
}

const CERTIFICATIONS: &[CertSeed] = &[
    cert(
        "LOTO-AUTH",
        "loto",
        36,
        60,
        "Chargé de consignation (LOTO) — énergies mécaniques et fluides",
        "LOTO authorised person — mechanical and fluid energies",
    ),
    cert(
        "LOTO-WORKER",
        "loto",
        36,
        60,
        "Consignation — intervenant (cadenas personnel)",
        "LOTO affected worker (personal lock)",
    ),
    cert(
        "CATEC",
        "confined_space",
        36,
        60,
        "Intervention en espace confiné (CATEC)",
        "Confined space entry (CATEC)",
    ),
    cert(
        "WORK-HEIGHT",
        "work_at_height",
        36,
        60,
        "Travail en hauteur et port du harnais",
        "Work at height and fall-arrest harness",
    ),
    cert(
        "WELD-ISO9606",
        "welding",
        24,
        60,
        "Soudeur qualifié ISO 9606-1",
        "Qualified welder ISO 9606-1",
    ),
    cert(
        "HOT-WORK",
        "hot_work",
        36,
        60,
        "Travaux par point chaud (permis feu)",
        "Hot work (fire permit)",
    ),
    cert(
        "CACES-R484",
        "lifting",
        60,
        90,
        "CACES R484 — ponts roulants et portiques",
        "Overhead and gantry crane operator (CACES R484)",
    ),
    cert(
        "CACES-R486",
        "lifting",
        60,
        90,
        "CACES R486 — plateformes élévatrices mobiles (PEMP)",
        "Mobile elevating work platform operator (CACES R486)",
    ),
    cert(
        "CACES-R489",
        "lifting",
        60,
        90,
        "CACES R489 — chariots élévateurs",
        "Forklift operator (CACES R489)",
    ),
    cert(
        "SST",
        "first_aid",
        24,
        60,
        "Sauveteur secouriste du travail (SST)",
        "First aid at work (SST)",
    ),
    cert(
        "ATEX-1",
        "other",
        36,
        60,
        "Intervention en zone ATEX — niveau 1",
        "Work in explosive atmospheres (ATEX) — level 1",
    ),
];

struct SkillSeed {
    code: &'static str,
    fr: &'static str,
    en: &'static str,
}

/// The first six codes are the ones `reference::system_catalog_integrity` seeds with their
/// English label; the others complete the usual maintenance trades.
const SKILLS: &[SkillSeed] = &[
    SkillSeed {
        code: "MECH_GENERAL",
        fr: "Mécanique générale",
        en: "General Mechanics",
    },
    SkillSeed {
        code: "ELEC_INDUSTRIAL",
        fr: "Électricité industrielle",
        en: "Industrial Electrical",
    },
    SkillSeed {
        code: "INSTRUMENTATION",
        fr: "Instrumentation",
        en: "Instrumentation",
    },
    SkillSeed {
        code: "WELDING",
        fr: "Soudage",
        en: "Welding",
    },
    SkillSeed {
        code: "HYDRAULICS",
        fr: "Hydraulique",
        en: "Hydraulics",
    },
    SkillSeed {
        code: "CONDITION_MONITOR",
        fr: "Surveillance conditionnelle",
        en: "Condition Monitoring",
    },
    SkillSeed {
        code: "PNEUMATICS",
        fr: "Pneumatique",
        en: "Pneumatics",
    },
    SkillSeed {
        code: "AUTOMATION_PLC",
        fr: "Automatisme et automates programmables",
        en: "Automation and PLCs",
    },
    SkillSeed {
        code: "REFRIGERATION",
        fr: "Froid et climatisation",
        en: "Refrigeration and HVAC",
    },
    SkillSeed {
        code: "LUBRICATION",
        fr: "Lubrification",
        en: "Lubrication",
    },
    SkillSeed {
        code: "MACHINING",
        fr: "Usinage",
        en: "Machining",
    },
    SkillSeed {
        code: "PIPEFITTING",
        fr: "Tuyauterie",
        en: "Pipefitting",
    },
    SkillSeed {
        code: "ALIGNMENT_BALANCING",
        fr: "Lignage et équilibrage",
        en: "Shaft alignment and balancing",
    },
];

const SKILLS_DOMAIN: &str = "PERSONNEL.SKILLS";
const SKILLS_DOMAIN_FR: &str = "Compétences techniques";
const SKILLS_DOMAIN_EN: &str = "Technical Skills";

fn is_french(locale: &str) -> bool {
    locale.to_ascii_lowercase().starts_with("fr")
}

async fn ensure_certification_types(db: &impl ConnectionTrait, french: bool) -> AppResult<()> {
    for seed in CERTIFICATIONS {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO certification_types \
             (entity_sync_id, code, name, default_validity_months, renewal_lead_days, category, is_active, row_version) \
             SELECT ?, ?, ?, ?, ?, ?, 1, 1 \
             WHERE NOT EXISTS (SELECT 1 FROM certification_types WHERE UPPER(code) = ?) \
               AND NOT EXISTS (SELECT 1 FROM certification_types WHERE entity_sync_id = ?)",
            [
                crate::sync::identity::seeded_entity_sync_id("certification_types", seed.code).into(),
                seed.code.into(),
                (if french { seed.fr } else { seed.en }).into(),
                seed.validity_months.into(),
                seed.renewal_lead_days.into(),
                seed.category.into(),
                seed.code.into(),
                crate::sync::identity::seeded_entity_sync_id("certification_types", seed.code).into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

fn seeded_skill_sync_id(code: &str) -> String {
    // Same identity as `reference::system_catalog_integrity` gives the skills it seeds.
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{SKILLS_DOMAIN}:{code}"))
}

async fn ensure_skills(db: &impl ConnectionTrait, french: bool) -> AppResult<()> {
    let set_id: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rs.id FROM reference_sets rs \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' \
             ORDER BY rs.version_no DESC LIMIT 1",
            [SKILLS_DOMAIN.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "id").ok());
    // The skills domain is created by the system catalogue; nothing to add to without it.
    let Some(set_id) = set_id else {
        return Ok(());
    };

    for (index, seed) in SKILLS.iter().enumerate() {
        let (label, other) = if french { (seed.fr, seed.en) } else { (seed.en, seed.fr) };
        // Untouched seed label in the other language → the tenant's language.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET label = ? \
             WHERE code = ? AND label = ? AND set_id IN ( \
                SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
                WHERE UPPER(TRIM(d.code)) = ?)",
            [label.into(), seed.code.into(), other.into(), SKILLS_DOMAIN.into()],
        ))
        .await?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values \
             (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
              semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, ?, NULL, ?, ?, NULL, ?, NULL, NULL, 'personnel_skill', NULL, 1, NULL \
             WHERE NOT EXISTS (SELECT 1 FROM reference_values WHERE entity_sync_id = ?)",
            [
                seeded_skill_sync_id(seed.code).into(),
                set_id.into(),
                seed.code.into(),
                label.into(),
                i64::try_from(index + 1).unwrap_or(i64::MAX).into(),
                seeded_skill_sync_id(seed.code).into(),
            ],
        ))
        .await?;
    }
    let (domain_name, other_name) = if french {
        (SKILLS_DOMAIN_FR, SKILLS_DOMAIN_EN)
    } else {
        (SKILLS_DOMAIN_EN, SKILLS_DOMAIN_FR)
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET name = ? WHERE UPPER(TRIM(code)) = ? AND name = ?",
        [domain_name.into(), SKILLS_DOMAIN.into(), other_name.into()],
    ))
    .await?;
    Ok(())
}

/// Reference list of electrical habilitations (NF C 18-510).
pub const HABILITATIONS_DOMAIN: &str = "PERSONNEL.HABILITATIONS";
const HABILITATIONS_DOMAIN_FR: &str = "Habilitations électriques (NF C 18-510)";
const HABILITATIONS_DOMAIN_EN: &str = "Electrical authorisations (NF C 18-510)";
/// Code prefix of the certification type of a habilitation symbol.
pub const HABILITATION_CERT_PREFIX: &str = "HAB-";
/// Renewal every 3 years (common practice for the NF C 18-510 refresher), reminded 60 days ahead.
const HABILITATION_VALIDITY_MONTHS: i64 = 36;
const HABILITATION_RENEWAL_LEAD_DAYS: i64 = 60;
/// Starter values the company may change (not product-locked).
const HABILITATION_META: &str = r#"{"origin":"starter","standard":"NF C 18-510"}"#;

/// `(symbol, French label, English description)`.
const HABILITATIONS: &[(&str, &str, &str)] = &[
    (
        "B0",
        "B0 — exécutant non électricien, basse tension",
        "B0 — non-electrician, low voltage",
    ),
    (
        "B0V",
        "B0V — non électricien au voisinage, basse tension",
        "B0V — non-electrician near live low-voltage parts",
    ),
    (
        "H0",
        "H0 — exécutant non électricien, haute tension",
        "H0 — non-electrician, high voltage",
    ),
    (
        "H0V",
        "H0V — non électricien au voisinage, haute tension",
        "H0V — non-electrician near live high-voltage parts",
    ),
    (
        "B1",
        "B1 — exécutant électricien, basse tension",
        "B1 — electrical worker, low voltage",
    ),
    (
        "B1V",
        "B1V — exécutant électricien au voisinage, basse tension",
        "B1V — electrical worker near live low-voltage parts",
    ),
    (
        "B2",
        "B2 — chargé de travaux, basse tension",
        "B2 — work supervisor, low voltage",
    ),
    (
        "B2V",
        "B2V — chargé de travaux au voisinage, basse tension",
        "B2V — work supervisor near live low-voltage parts",
    ),
    (
        "BR",
        "BR — chargé d'intervention générale, basse tension",
        "BR — general troubleshooting, low voltage",
    ),
    (
        "BS",
        "BS — chargé d'intervention élémentaire, basse tension",
        "BS — basic troubleshooting, low voltage",
    ),
    (
        "BC",
        "BC — chargé de consignation, basse tension",
        "BC — isolation (lock-out), low voltage",
    ),
    (
        "BE",
        "BE — chargé d'opérations spécifiques (essai, mesurage, vérification, manœuvre), basse tension",
        "BE — specific operations (testing, measuring, checking, switching), low voltage",
    ),
    (
        "H1",
        "H1 — exécutant électricien, haute tension",
        "H1 — electrical worker, high voltage",
    ),
    (
        "H1V",
        "H1V — exécutant électricien au voisinage, haute tension",
        "H1V — electrical worker near live high-voltage parts",
    ),
    (
        "H2",
        "H2 — chargé de travaux, haute tension",
        "H2 — work supervisor, high voltage",
    ),
    (
        "H2V",
        "H2V — chargé de travaux au voisinage, haute tension",
        "H2V — work supervisor near live high-voltage parts",
    ),
    (
        "HC",
        "HC — chargé de consignation, haute tension",
        "HC — isolation (lock-out), high voltage",
    ),
    (
        "HE",
        "HE — chargé d'opérations spécifiques (essai, mesurage, vérification, manœuvre), haute tension",
        "HE — specific operations (testing, measuring, checking, switching), high voltage",
    ),
];

/// Seeds the habilitation list once — when the domain does not exist yet — so a symbol the
/// company removed or renamed is never put back. The list is a company list (audience
/// `company`): shown and managed in Données de référence.
async fn ensure_habilitation_list(db: &impl ConnectionTrait, french: bool) -> AppResult<()> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM reference_domains WHERE UPPER(TRIM(code)) = ?",
            [HABILITATIONS_DOMAIN.into()],
        ))
        .await?
        .is_some();
    if exists {
        return Ok(());
    }
    let category = crate::reference::governance::derive_category_for_persist(HABILITATIONS_DOMAIN, "tenant_managed");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, is_extendable, validation_rules_json, \
          created_at, updated_at, entity_sync_id) \
         VALUES (?, ?, 'flat', 'tenant_managed', ?, 1, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
        [
            HABILITATIONS_DOMAIN.into(),
            (if french {
                HABILITATIONS_DOMAIN_FR
            } else {
                HABILITATIONS_DOMAIN_EN
            })
            .into(),
            category.as_str().into(),
            crate::sync::identity::seeded_entity_sync_id("reference_domains", HABILITATIONS_DOMAIN).into(),
        ],
    ))
    .await?;
    crate::reference::audience::declare_domain_audience(
        db,
        HABILITATIONS_DOMAIN,
        crate::reference::audience::DomainAudience::Company,
    )
    .await?;
    db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_domains SET audience = 'company' WHERE UPPER(TRIM(code)) = ?",
            [HABILITATIONS_DOMAIN.into()],
        ))
        .await?;
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_by_id, created_at, \
          published_at, entity_sync_id) \
         SELECT d.id, 1, 'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL, \
                strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), ? \
           FROM reference_domains d WHERE UPPER(TRIM(d.code)) = ?",
        [
            crate::sync::identity::seeded_entity_sync_id("reference_sets", &format!("{HABILITATIONS_DOMAIN}:1")).into(),
            HABILITATIONS_DOMAIN.into(),
        ],
    ))
    .await?;
    for (i, (code, fr, en)) in HABILITATIONS.iter().enumerate() {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, \
              external_code, is_active, metadata_json, entity_sync_id) \
             SELECT rs.id, NULL, ?, ?, ?, ?, NULL, NULL, 'habilitation', NULL, 1, ?, ? \
               FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published'",
            [
                (*code).into(),
                (if french { *fr } else { *en }).into(),
                (if french { *en } else { *fr }).into(),
                i64::try_from(i + 1).unwrap_or(0).into(),
                HABILITATION_META.into(),
                crate::sync::identity::seeded_entity_sync_id(
                    "reference_values",
                    &format!("{HABILITATIONS_DOMAIN}:{code}"),
                )
                .into(),
                HABILITATIONS_DOMAIN.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// Gives every active published habilitation its certification type `HAB-<symbol>` (only the
/// missing ones: a type the plant renamed or deactivated is left as it is). Runs at startup and
/// after the habilitation list is published.
pub async fn ensure_habilitation_certification_types(db: &impl ConnectionTrait, french: bool) -> AppResult<()> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' AND rv.is_active = 1 \
             ORDER BY rv.sort_order, rv.code",
            [HABILITATIONS_DOMAIN.into()],
        ))
        .await?;
    for r in rows {
        let symbol: String = r.try_get("", "code")?;
        let label: String = r.try_get("", "label")?;
        let code = format!("{HABILITATION_CERT_PREFIX}{}", symbol.trim().to_ascii_uppercase());
        let name = if french {
            format!("Habilitation électrique {label}")
        } else {
            format!("Electrical authorisation {label}")
        };
        let sync_id = crate::sync::identity::seeded_entity_sync_id("certification_types", &code);
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO certification_types \
             (entity_sync_id, code, name, default_validity_months, renewal_lead_days, category, is_active, row_version) \
             SELECT ?, ?, ?, ?, ?, 'electrical', 1, 1 \
             WHERE NOT EXISTS (SELECT 1 FROM certification_types WHERE UPPER(code) = ?) \
               AND NOT EXISTS (SELECT 1 FROM certification_types WHERE entity_sync_id = ?)",
            [
                sync_id.clone().into(),
                code.clone().into(),
                name.into(),
                HABILITATION_VALIDITY_MONTHS.into(),
                HABILITATION_RENEWAL_LEAD_DAYS.into(),
                code.into(),
                sync_id.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// Adds the starter certification types and skills missing for this tenant, in its language.
pub async fn ensure_qualification_catalog(db: &DatabaseConnection) -> AppResult<()> {
    let locale = crate::pm::wo_content::tenant_text_locale(db).await?;
    let french = is_french(&locale);
    ensure_certification_types(db, french).await?;
    ensure_habilitation_list(db, french).await?;
    ensure_habilitation_certification_types(db, french).await?;
    ensure_skills(db, french).await?;
    Ok(())
}

/// Same, with an explicit language (tests and callers that already resolved it).
pub async fn ensure_qualification_catalog_in(db: &impl ConnectionTrait, locale: &str) -> AppResult<()> {
    let french = is_french(locale);
    ensure_certification_types(db, french).await?;
    ensure_habilitation_list(db, french).await?;
    ensure_habilitation_certification_types(db, french).await?;
    ensure_skills(db, french).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CERTIFICATIONS, SKILLS};
    use crate::qualification::domain::CERTIFICATION_CATEGORIES;

    #[test]
    fn seed_codes_are_unique_and_categories_known() {
        let mut codes: Vec<&str> = CERTIFICATIONS.iter().map(|c| c.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), CERTIFICATIONS.len());
        for c in CERTIFICATIONS {
            assert!(CERTIFICATION_CATEGORIES.contains(&c.category), "{}", c.code);
            assert!(c.validity_months > 0 && c.renewal_lead_days > 0);
            assert!(!c.fr.is_empty() && !c.en.is_empty());
        }
        let mut skills: Vec<&str> = SKILLS.iter().map(|s| s.code).collect();
        skills.sort_unstable();
        skills.dedup();
        assert_eq!(skills.len(), SKILLS.len());
    }
}
