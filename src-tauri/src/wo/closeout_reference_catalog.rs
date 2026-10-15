//! Close-out failure coding catalogs (ISO 14224 starter sets).
//!
//! The WO close-out codes a failure with a symptom, failure mode, mechanism, cause, effect and
//! detection method, all `reference_values` of governed domains. `WORK.FAILURE_MODES` is seeded
//! by `reference::system_catalog_integrity`; this module ensures the other domains exist with a
//! published set and an ISO 14224-based starter list (Annex B tables), so the close-out
//! dropdowns are never empty on a new plant. Customers extend or retire values through the
//! reference manager (the domains are tenant-managed); rows already present are never touched.
//!
//! Labels are French (primary UI language of the catalogs, like the other system seeds);
//! `description` carries the English term and `external_code` the ISO 14224 code.
//!
//! Called from migration 178 (so a freshly migrated database is usable) and from the startup
//! catalog integrity pass (tenant wipe keeps migrations but deletes catalog rows).

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::AppResult;
use crate::reference::governance::derive_category_for_persist;

pub const DOMAIN_SYMPTOMS: &str = "WORK.SYMPTOMS";
pub const DOMAIN_FAILURE_CAUSES: &str = "WORK.FAILURE_CAUSES";
pub const DOMAIN_FAILURE_EFFECTS: &str = "WORK.FAILURE_EFFECTS";
pub const DOMAIN_FAILURE_MECHANISMS: &str = "WORK.FAILURE_MECHANISMS";
pub const DOMAIN_DETECTION_METHODS: &str = "WORK.DETECTION_METHODS";

const SYSTEM_META: &str = r#"{"origin":"system","standard":"ISO 14224"}"#;

/// (code, French label, English term, ISO 14224 code)
type Row = (&'static str, &'static str, &'static str, &'static str);

/// Observed symptoms. Codes shared with `DI.SYMPTOM` (vibration, leakage, overheating, noise,
/// performance_drop) let a converted request carry its symptom onto the WO close-out.
const SYMPTOMS: &[Row] = &[
    ("vibration", "Vibration anormale", "Vibration", "VIB"),
    ("noise", "Bruit anormal", "Noise", "NOI"),
    ("overheating", "Surchauffe", "Overheating", "OHE"),
    (
        "leakage",
        "Fuite externe (fluide de procédé)",
        "External leakage - process medium",
        "ELP",
    ),
    (
        "leakage_utility",
        "Fuite externe (fluide utilitaire)",
        "External leakage - utility medium",
        "ELU",
    ),
    ("internal_leakage", "Fuite interne", "Internal leakage", "INL"),
    ("performance_drop", "Débit / puissance insuffisant", "Low output", "LOO"),
    ("high_output", "Débit / puissance excessif", "High output", "HIO"),
    ("erratic_output", "Fonctionnement erratique", "Erratic output", "ERO"),
    ("fail_to_start", "Refus de démarrage", "Fail to start on demand", "FTS"),
    ("fail_to_stop", "Refus d'arrêt", "Fail to stop on demand", "STP"),
    ("spurious_stop", "Arrêt intempestif", "Spurious stop", "UST"),
    (
        "spurious_operation",
        "Fonctionnement intempestif",
        "Spurious operation",
        "SPO",
    ),
    (
        "parameter_deviation",
        "Dérive d'un paramètre",
        "Parameter deviation",
        "PDE",
    ),
    (
        "abnormal_reading",
        "Indication anormale d'instrument",
        "Abnormal instrument reading",
        "AIR",
    ),
    (
        "structural_deficiency",
        "Défaut structurel",
        "Structural deficiency",
        "STD",
    ),
    ("plugged", "Colmatage / obstruction", "Plugged / choked", "PLU"),
    (
        "minor_in_service",
        "Problème mineur en service",
        "Minor in-service problems",
        "SER",
    ),
    ("other", "Autre symptôme", "Other", "OTH"),
];

/// Failure causes (ISO 14224 Table B.3).
const CAUSES: &[Row] = &[
    (
        "design_capacity",
        "Capacité / dimensionnement inadapté",
        "Improper capacity",
        "1.1",
    ),
    ("design_material", "Matériau inadapté", "Improper material", "1.2"),
    ("design_other", "Défaut de conception", "Design-related, general", "1.0"),
    ("fabrication_error", "Erreur de fabrication", "Fabrication error", "2.1"),
    (
        "installation_error",
        "Erreur d'installation",
        "Installation error",
        "2.2",
    ),
    (
        "off_design_service",
        "Utilisation hors conditions de conception",
        "Off-design service",
        "3.1",
    ),
    ("operating_error", "Erreur d'exploitation", "Operating error", "3.2"),
    ("maintenance_error", "Erreur de maintenance", "Maintenance error", "3.3"),
    ("wear_and_tear", "Usure normale", "Expected wear and tear", "3.4"),
    (
        "documentation_error",
        "Erreur de documentation",
        "Documentation error",
        "4.1",
    ),
    ("management_error", "Erreur d'organisation", "Management error", "4.2"),
    ("no_cause_found", "Aucune cause trouvée", "No cause found", "5.1"),
    ("common_cause", "Cause commune", "Common cause", "5.2"),
    ("combined_causes", "Causes combinées", "Combined causes", "5.3"),
    (
        "other_unit",
        "Défaillance d'un autre équipement",
        "Other unit / cascading failure",
        "5.4",
    ),
    ("other", "Autre cause", "Other", "5.5"),
    ("unknown", "Cause inconnue", "Unknown", "5.6"),
];

/// Failure effects: ISO 14224 severity class and the operational consequence.
const EFFECTS: &[Row] = &[
    ("critical", "Perte de fonction critique", "Critical failure", "C"),
    ("degraded", "Fonction dégradée", "Degraded failure", "D"),
    ("incipient", "Défaillance naissante", "Incipient failure", "I"),
    (
        "production_stop",
        "Arrêt total de production",
        "Total production loss",
        "PROD-STOP",
    ),
    (
        "production_reduced",
        "Production réduite",
        "Reduced production",
        "PROD-RED",
    ),
    (
        "no_production_impact",
        "Sans impact production",
        "No production impact",
        "PROD-NONE",
    ),
    ("safety", "Impact sécurité", "Safety consequence", "HSE-S"),
    (
        "environment",
        "Impact environnement",
        "Environmental consequence",
        "HSE-E",
    ),
];

/// Failure mechanisms (ISO 14224 Table B.2).
const MECHANISMS: &[Row] = &[
    ("leakage", "Fuite", "Leakage", "1.1"),
    ("vibration", "Vibration", "Vibration", "1.2"),
    (
        "clearance_alignment",
        "Jeu / désalignement",
        "Clearance / alignment failure",
        "1.3",
    ),
    ("deformation", "Déformation", "Deformation", "1.4"),
    ("looseness", "Desserrage", "Looseness", "1.5"),
    ("sticking", "Grippage / coincement", "Sticking", "1.6"),
    ("cavitation", "Cavitation", "Cavitation", "2.1"),
    ("corrosion", "Corrosion", "Corrosion", "2.2"),
    ("erosion", "Érosion", "Erosion", "2.3"),
    ("wear", "Usure", "Wear", "2.4"),
    ("breakage", "Rupture", "Breakage", "2.5"),
    ("fatigue", "Fatigue", "Fatigue", "2.6"),
    ("overheating", "Échauffement", "Overheating", "2.7"),
    ("burst", "Éclatement", "Burst", "2.8"),
    ("control_failure", "Défaut de régulation", "Control failure", "3.1"),
    (
        "no_signal",
        "Absence de signal",
        "No signal / indication / alarm",
        "3.2",
    ),
    (
        "faulty_signal",
        "Signal erroné",
        "Faulty signal / indication / alarm",
        "3.3",
    ),
    ("out_of_adjustment", "Déréglage", "Out of adjustment", "3.4"),
    ("software_failure", "Défaut logiciel", "Software failure", "3.5"),
    ("short_circuit", "Court-circuit", "Short circuiting", "4.1"),
    ("open_circuit", "Circuit ouvert", "Open circuit", "4.2"),
    ("no_power", "Absence d'alimentation", "No power / voltage", "4.3"),
    (
        "faulty_power",
        "Alimentation défectueuse",
        "Faulty power / voltage",
        "4.4",
    ),
    (
        "earth_fault",
        "Défaut d'isolement / terre",
        "Earth / isolation fault",
        "4.5",
    ),
    ("blockage", "Bouchage / colmatage", "Blockage / plugged", "5.1"),
    ("contamination", "Contamination", "Contamination", "5.2"),
    (
        "external_influence",
        "Influence externe",
        "Miscellaneous external influences",
        "5.3",
    ),
    ("no_cause_found", "Mécanisme non identifié", "No cause found", "6.1"),
    ("combined_causes", "Mécanismes combinés", "Combined causes", "6.2"),
    ("other", "Autre mécanisme", "Other", "6.3"),
    ("unknown", "Mécanisme inconnu", "Unknown", "6.4"),
];

/// Detection methods (ISO 14224 Table B.4).
const DETECTION_METHODS: &[Row] = &[
    (
        "periodic_maintenance",
        "Maintenance préventive périodique",
        "Periodic maintenance",
        "1",
    ),
    ("functional_testing", "Essai fonctionnel", "Functional testing", "2"),
    ("inspection", "Inspection", "Inspection", "3"),
    (
        "periodic_condition_monitoring",
        "Surveillance conditionnelle périodique",
        "Periodic condition monitoring",
        "4",
    ),
    (
        "continuous_condition_monitoring",
        "Surveillance conditionnelle continue",
        "Continuous condition monitoring",
        "5",
    ),
    (
        "production_interference",
        "Perturbation de la production",
        "Production interference",
        "6",
    ),
    ("casual_observation", "Observation fortuite", "Casual observation", "7"),
    (
        "corrective_maintenance",
        "Maintenance corrective",
        "Corrective maintenance",
        "8",
    ),
    ("on_demand", "À la sollicitation", "On demand", "9"),
    ("other", "Autre", "Other", "10"),
];

async fn ensure_domain(conn: &impl ConnectionTrait, code: &str, name: &str) -> AppResult<()> {
    let category = derive_category_for_persist(code, "tenant_managed");
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, \
          is_extendable, validation_rules_json, created_at, updated_at) \
         SELECT ?, ?, 'flat', 'tenant_managed', ?, 1, NULL, \
                strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE NOT EXISTS (SELECT 1 FROM reference_domains WHERE UPPER(TRIM(code)) = ?)",
        [
            code.into(),
            name.into(),
            category.as_str().into(),
            code.to_ascii_uppercase().into(),
        ],
    ))
    .await?;
    // Failure coding differs per plant (ISO 14224 lists extended locally): a company list.
    crate::reference::audience::declare_domain_audience(
        conn,
        code,
        crate::reference::audience::DomainAudience::Company,
    )
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_sets \
         (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at) \
         SELECT d.id, COALESCE((SELECT MAX(rs.version_no) FROM reference_sets rs WHERE rs.domain_id = d.id), 0) + 1, \
                'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL, \
                strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
           FROM reference_domains d \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS (SELECT 1 FROM reference_sets rs WHERE rs.domain_id = d.id AND rs.status = 'published')",
        [code.to_ascii_uppercase().into()],
    ))
    .await?;
    Ok(())
}

/// Insert one value into the latest published set of `domain_code` unless a value with the same
/// code (or the same seeded sync identity) already exists in that domain.
pub(crate) async fn ensure_value(
    conn: &impl ConnectionTrait,
    domain_code: &str,
    code: &str,
    label: &str,
    description: Option<&str>,
    external_code: Option<&str>,
    sort_order: i64,
) -> AppResult<()> {
    let sync_id = crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain_code}:{code}"));
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
          semantic_tag, external_code, is_active, metadata_json) \
         SELECT ?, rs.id, NULL, ?, ?, ?, ?, NULL, NULL, NULL, ?, 1, ? \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS ( \
                SELECT 1 FROM reference_values rv \
                  JOIN reference_sets s2 ON s2.id = rv.set_id \
                 WHERE s2.domain_id = d.id AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
            ) \
          ORDER BY rs.version_no DESC \
          LIMIT 1",
        [
            sync_id.into(),
            code.into(),
            label.into(),
            description.map(str::to_string).into(),
            sort_order.into(),
            external_code.map(str::to_string).into(),
            SYSTEM_META.into(),
            domain_code.to_ascii_uppercase().into(),
            code.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn ensure_catalog(conn: &impl ConnectionTrait, domain_code: &str, name: &str, rows: &[Row]) -> AppResult<()> {
    ensure_domain(conn, domain_code, name).await?;
    for (i, (code, label, en, iso)) in rows.iter().enumerate() {
        ensure_value(conn, domain_code, code, label, Some(en), Some(iso), (i as i64) + 1).await?;
    }
    Ok(())
}

/// Ensure every close-out failure-coding domain exists with its starter values.
pub async fn ensure_closeout_reference_catalogs(conn: &impl ConnectionTrait) -> AppResult<()> {
    ensure_catalog(conn, DOMAIN_SYMPTOMS, "Symptômes de défaillance", SYMPTOMS).await?;
    ensure_catalog(conn, DOMAIN_FAILURE_CAUSES, "Causes de défaillance", CAUSES).await?;
    ensure_catalog(conn, DOMAIN_FAILURE_EFFECTS, "Effets de défaillance", EFFECTS).await?;
    ensure_catalog(conn, DOMAIN_FAILURE_MECHANISMS, "Mécanismes de défaillance", MECHANISMS).await?;
    ensure_catalog(
        conn,
        DOMAIN_DETECTION_METHODS,
        "Méthodes de détection",
        DETECTION_METHODS,
    )
    .await?;
    Ok(())
}

/// Id of the `WORK.SYMPTOMS` value whose code matches `code` (case-insensitive), if any.
pub async fn resolve_work_symptom_id_by_code(conn: &impl ConnectionTrait, code: &str) -> AppResult<Option<i64>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) AND rv.is_active = 1 \
              ORDER BY rs.version_no DESC LIMIT 1",
            [DOMAIN_SYMPTOMS.into(), code.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i64>("", "id").ok()))
}
