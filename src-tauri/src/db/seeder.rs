use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

/// Current version of the system seed data set.
/// Increment this when adding new system domains or values in a release.
pub const SEED_SCHEMA_VERSION: i32 = 1;

/// Baseline settings seeded on first startup after settings migration is applied.
/// Tuple format: (`setting_key`, category, `setting_scope`, `setting_value_json`)
const DEFAULT_SETTINGS: &[(&str, &str, &str, &str)] = &[
    ("locale.primary_language", "localization", "tenant", r#""fr""#),
    ("locale.fallback_language", "localization", "tenant", r#""en""#),
    ("locale.date_format", "localization", "tenant", r#""DD/MM/YYYY""#),
    ("locale.number_format", "localization", "tenant", r#""fr-FR""#),
    ("locale.week_start_day", "localization", "tenant", r"1"),
    ("appearance.color_mode", "appearance", "tenant", r#""light""#),
    ("appearance.density", "appearance", "tenant", r#""standard""#),
    ("appearance.text_scale", "appearance", "tenant", r"1.0"),
    ("updater.release_channel", "system", "device", r#""stable""#),
    ("updater.auto_check", "system", "device", r"true"),
    ("backup.retention_daily", "backup", "tenant", r"7"),
    ("backup.retention_weekly", "backup", "tenant", r"4"),
    ("backup.retention_monthly", "backup", "tenant", r"12"),
    ("diagnostics.log_retention_days", "system", "device", r"30"),
];

/// Inserts all system-governed lookup domains and values idempotently.
///
/// Safe to call on every startup: uses INSERT OR IGNORE semantics.
/// On completion, records `seed_schema_version = SEED_SCHEMA_VERSION` in `system_config`.
pub async fn seed_system_data(db: &DatabaseConnection) -> AppResult<()> {
    tracing::info!("seeder::starting system seed (version {})", SEED_SCHEMA_VERSION);

    // ── 1. Insert all domain definitions ─────────────────────────────────
    seed_domain(
        db,
        "equipment.criticality",
        "Criticité équipement",
        "system",
        false,
        true,
    )
    .await?;
    seed_domain(
        db,
        "equipment.lifecycle_status",
        "Statut cycle de vie équipement",
        "system",
        false,
        false,
    )
    .await?;
    seed_domain(
        db,
        "equipment.hierarchy_relationship",
        "Type de relation hiérarchique équipement",
        "system",
        false,
        false,
    )
    .await?;
    seed_domain(
        db,
        "intervention_request.type",
        "Type de demande d'intervention",
        "tenant",
        false,
        true,
    )
    .await?;
    seed_domain(
        db,
        "intervention_request.urgency",
        "Urgence demande d'intervention",
        "system",
        true,
        true,
    )
    .await?;
    seed_domain(
        db,
        "intervention_request.status",
        "Statut demande d'intervention",
        "system",
        false,
        false,
    )
    .await?;
    seed_domain(db, "work_order.type", "Type d'ordre de travail", "tenant", false, true).await?;
    seed_domain(
        db,
        "work_order.status",
        "Statut ordre de travail",
        "system",
        false,
        false,
    )
    .await?;
    seed_domain(
        db,
        "work_order.priority",
        "Priorité ordre de travail",
        "system",
        true,
        true,
    )
    .await?;
    seed_domain(db, "failure.mode", "Mode de défaillance", "tenant", false, true).await?;
    seed_domain(db, "failure.cause", "Cause de défaillance", "tenant", false, true).await?;
    seed_domain(
        db,
        "work_order.closure_reason",
        "Motif de clôture OT",
        "tenant",
        false,
        true,
    )
    .await?;
    seed_domain(
        db,
        "personnel.skill_proficiency",
        "Niveau de compétence",
        "system",
        true,
        false,
    )
    .await?;
    seed_domain(db, "personnel.contract_type", "Type de contrat", "tenant", false, true).await?;
    seed_domain(
        db,
        "inventory.unit_of_measure",
        "Unité de mesure stock",
        "tenant",
        false,
        true,
    )
    .await?;
    seed_domain(
        db,
        "inventory.movement_type",
        "Type de mouvement stock",
        "system",
        false,
        false,
    )
    .await?;
    seed_domain(
        db,
        "org.responsibility_type",
        "Type de responsabilité organisationnelle",
        "system",
        false,
        true,
    )
    .await?;
    seed_domain(db, "permit.type", "Type de permis de travail", "tenant", false, true).await?;

    // ── 2. Resolve domain ids and insert values per domain ────────────────

    // equipment.criticality
    {
        let d = get_domain_id(db, "equipment.criticality").await?;
        seed_value(
            db,
            d,
            "CRITIQUE",
            "Critique",
            "Critique",
            "Critical",
            Some("#dc3545"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "IMPORTANT",
            "Important",
            "Important",
            "Important",
            Some("#ffc107"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "STANDARD",
            "Standard",
            "Standard",
            "Standard",
            Some("#0dcaf0"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NON_CRITIQUE",
            "Non-critique",
            "Non-critique",
            "Non-critical",
            Some("#198754"),
            4,
            true,
        )
        .await?;
    }

    // equipment.lifecycle_status
    {
        let d = get_domain_id(db, "equipment.lifecycle_status").await?;
        seed_value(
            db,
            d,
            "ACTIVE_IN_SERVICE",
            "En service",
            "En service",
            "In Service",
            Some("#198754"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "IN_STOCK",
            "En stock",
            "En stock",
            "In Stock",
            Some("#0dcaf0"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "UNDER_MAINTENANCE",
            "En maintenance",
            "En maintenance",
            "Under Maintenance",
            Some("#ffc107"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "DECOMMISSIONED",
            "Mis hors service",
            "Mis hors service",
            "Decommissioned",
            Some("#6c757d"),
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "SCRAPPED",
            "Mis au rebut",
            "Mis au rebut",
            "Scrapped",
            Some("#dc3545"),
            5,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "SPARE",
            "Pièce de rechange",
            "Pièce de rechange",
            "Spare",
            Some("#6c757d"),
            6,
            true,
        )
        .await?;
    }

    // equipment.hierarchy_relationship
    {
        let d = get_domain_id(db, "equipment.hierarchy_relationship").await?;
        seed_value(
            db,
            d,
            "PARENT_CHILD",
            "Parent \u{2014} Enfant",
            "Parent \u{2014} Enfant",
            "Parent \u{2014} Child",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "INSTALLED_IN",
            "Install\u{00e9} dans",
            "Install\u{00e9} dans",
            "Installed In",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "DRIVES",
            "Entra\u{00ee}ne",
            "Entra\u{00ee}ne",
            "Drives",
            None,
            3,
            true,
        )
        .await?;
        seed_value(db, d, "FEEDS", "Alimente", "Alimente", "Feeds", None, 4, true).await?;
    }

    // intervention_request.type (tenant-extensible examples)
    {
        let d = get_domain_id(db, "intervention_request.type").await?;
        seed_value(
            db,
            d,
            "CORRECTIVE",
            "Corrective",
            "Corrective",
            "Corrective",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "SIGNALEMENT",
            "Signalement",
            "Signalement",
            "Observation",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "AMELIORATION",
            "Am\u{00e9}lioration",
            "Am\u{00e9}lioration",
            "Improvement",
            None,
            3,
            false,
        )
        .await?;
    }

    // intervention_request.urgency
    {
        let d = get_domain_id(db, "intervention_request.urgency").await?;
        seed_value(
            db,
            d,
            "IMMEDIATE",
            "Imm\u{00e9}diate",
            "Imm\u{00e9}diate",
            "Immediate",
            Some("#dc3545"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "URGENT",
            "Urgente",
            "Urgente",
            "Urgent",
            Some("#ffc107"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NORMALE",
            "Normale",
            "Normale",
            "Normal",
            Some("#198754"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PLANIFIEE",
            "Planifi\u{00e9}e",
            "Planifi\u{00e9}e",
            "Planned",
            Some("#0dcaf0"),
            4,
            true,
        )
        .await?;
    }

    // intervention_request.status
    {
        let d = get_domain_id(db, "intervention_request.status").await?;
        seed_value(
            db,
            d,
            "DRAFT",
            "Brouillon",
            "Brouillon",
            "Draft",
            Some("#6c757d"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "SUBMITTED",
            "Soumise",
            "Soumise",
            "Submitted",
            Some("#0dcaf0"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "ACKNOWLEDGED",
            "Accus\u{00e9}e",
            "Accus\u{00e9}e",
            "Acknowledged",
            Some("#ffc107"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "IN_PROGRESS",
            "En cours",
            "En cours",
            "In Progress",
            Some("#003d8f"),
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "COMPLETED",
            "Cl\u{00f4}tur\u{00e9}e",
            "Cl\u{00f4}tur\u{00e9}e",
            "Completed",
            Some("#198754"),
            5,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "REJECTED",
            "Rejet\u{00e9}e",
            "Rejet\u{00e9}e",
            "Rejected",
            Some("#dc3545"),
            6,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "CANCELLED",
            "Annul\u{00e9}e",
            "Annul\u{00e9}e",
            "Cancelled",
            Some("#6c757d"),
            7,
            true,
        )
        .await?;
    }

    // work_order.type
    {
        let d = get_domain_id(db, "work_order.type").await?;
        seed_value(
            db,
            d,
            "CORRECTIVE",
            "Corrective",
            "Corrective",
            "Corrective",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PREVENTIVE",
            "Pr\u{00e9}ventive",
            "Pr\u{00e9}ventive",
            "Preventive",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PREDICTIVE",
            "Pr\u{00e9}dictive",
            "Pr\u{00e9}dictive",
            "Predictive",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "AMELIORATIVE",
            "Am\u{00e9}liorative",
            "Am\u{00e9}liorative",
            "Improvement",
            None,
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "INSPECTION",
            "Inspection",
            "Inspection",
            "Inspection",
            None,
            5,
            true,
        )
        .await?;
    }

    // work_order.status
    {
        let d = get_domain_id(db, "work_order.status").await?;
        seed_value(
            db,
            d,
            "DRAFT",
            "Brouillon",
            "Brouillon",
            "Draft",
            Some("#6c757d"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PLANNED",
            "Planifi\u{00e9}",
            "Planifi\u{00e9}",
            "Planned",
            Some("#0dcaf0"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "RELEASED",
            "Lanc\u{00e9}",
            "Lanc\u{00e9}",
            "Released",
            Some("#003d8f"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "IN_PROGRESS",
            "En cours",
            "En cours",
            "In Progress",
            Some("#ffc107"),
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "ON_HOLD",
            "En attente",
            "En attente",
            "On Hold",
            Some("#f0a500"),
            5,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "COMPLETED",
            "Termin\u{00e9}",
            "Termin\u{00e9}",
            "Completed",
            Some("#198754"),
            6,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "CLOSED",
            "Cl\u{00f4}tur\u{00e9}",
            "Cl\u{00f4}tur\u{00e9}",
            "Closed",
            Some("#6c757d"),
            7,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "CANCELLED",
            "Annul\u{00e9}",
            "Annul\u{00e9}",
            "Cancelled",
            Some("#dc3545"),
            8,
            true,
        )
        .await?;
    }

    // work_order.priority
    {
        let d = get_domain_id(db, "work_order.priority").await?;
        seed_value(
            db,
            d,
            "P1_CRITICAL",
            "P1 \u{2014} Critique",
            "P1 \u{2014} Critique",
            "P1 \u{2014} Critical",
            Some("#dc3545"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "P2_HIGH",
            "P2 \u{2014} Haute",
            "P2 \u{2014} Haute",
            "P2 \u{2014} High",
            Some("#ffc107"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "P3_MEDIUM",
            "P3 \u{2014} Moyenne",
            "P3 \u{2014} Moyenne",
            "P3 \u{2014} Medium",
            Some("#0dcaf0"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "P4_LOW",
            "P4 \u{2014} Basse",
            "P4 \u{2014} Basse",
            "P4 \u{2014} Low",
            Some("#198754"),
            4,
            true,
        )
        .await?;
    }

    // failure.mode (examples — tenant-extensible)
    {
        let d = get_domain_id(db, "failure.mode").await?;
        seed_value(db, d, "VIBRATION", "Vibration", "Vibration", "Vibration", None, 1, true).await?;
        seed_value(db, d, "CORROSION", "Corrosion", "Corrosion", "Corrosion", None, 2, true).await?;
        seed_value(
            db,
            d,
            "BRUIT",
            "Bruit anormal",
            "Bruit anormal",
            "Abnormal Noise",
            None,
            3,
            true,
        )
        .await?;
        seed_value(db, d, "FUITE", "Fuite", "Fuite", "Leak", None, 4, true).await?;
        seed_value(
            db,
            d,
            "SURCHAUFFE",
            "Surchauffe",
            "Surchauffe",
            "Overheating",
            None,
            5,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PANNE_ELEC",
            "Panne \u{00e9}lectrique",
            "Panne \u{00e9}lectrique",
            "Electrical Fault",
            None,
            6,
            true,
        )
        .await?;
        seed_value(db, d, "AUTRE", "Autre", "Autre", "Other", None, 99, true).await?;
    }

    // failure.cause
    {
        let d = get_domain_id(db, "failure.cause").await?;
        seed_value(
            db,
            d,
            "USURE_NORMALE",
            "Usure normale",
            "Usure normale",
            "Normal Wear",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "MAUVAIS_USAGE",
            "Mauvais usage",
            "Mauvais usage",
            "Misuse",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "DEFAUT_ENTRETIEN",
            "D\u{00e9}faut d'entretien",
            "D\u{00e9}faut d'entretien",
            "Maintenance Defect",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "DEFAUT_INSTALL",
            "D\u{00e9}faut d'installation",
            "D\u{00e9}faut d'installation",
            "Installation Defect",
            None,
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "DEFAUT_MATERIEL",
            "D\u{00e9}faut mat\u{00e9}riel",
            "D\u{00e9}faut mat\u{00e9}riel",
            "Material Defect",
            None,
            5,
            true,
        )
        .await?;
        seed_value(db, d, "INCONNU", "Inconnu", "Inconnu", "Unknown", None, 99, true).await?;
    }

    // work_order.closure_reason
    {
        let d = get_domain_id(db, "work_order.closure_reason").await?;
        seed_value(
            db,
            d,
            "REPARE",
            "R\u{00e9}par\u{00e9}",
            "R\u{00e9}par\u{00e9}",
            "Repaired",
            Some("#198754"),
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "REPORTE",
            "Report\u{00e9}",
            "Report\u{00e9}",
            "Deferred",
            Some("#ffc107"),
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NON_NECESSAIRE",
            "Non n\u{00e9}cessaire",
            "Non n\u{00e9}cessaire",
            "Not Required",
            Some("#6c757d"),
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "REMPLACE",
            "Remplac\u{00e9}",
            "Remplac\u{00e9}",
            "Replaced",
            Some("#0dcaf0"),
            4,
            true,
        )
        .await?;
    }

    // personnel.skill_proficiency
    {
        let d = get_domain_id(db, "personnel.skill_proficiency").await?;
        seed_value(
            db,
            d,
            "NIVEAU_1",
            "Niveau 1 \u{2014} Notions",
            "Niveau 1 \u{2014} Notions",
            "Level 1 \u{2014} Awareness",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NIVEAU_2",
            "Niveau 2 \u{2014} Appliqu\u{00e9}",
            "Niveau 2 \u{2014} Appliqu\u{00e9}",
            "Level 2 \u{2014} Applied",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NIVEAU_3",
            "Niveau 3 \u{2014} Ma\u{00ee}tris\u{00e9}",
            "Niveau 3 \u{2014} Ma\u{00ee}tris\u{00e9}",
            "Level 3 \u{2014} Proficient",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NIVEAU_4",
            "Niveau 4 \u{2014} Expert",
            "Niveau 4 \u{2014} Expert",
            "Level 4 \u{2014} Expert",
            None,
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "NIVEAU_5",
            "Niveau 5 \u{2014} Ma\u{00ee}tre formateur",
            "Niveau 5 \u{2014} Ma\u{00ee}tre formateur",
            "Level 5 \u{2014} Master Trainer",
            None,
            5,
            true,
        )
        .await?;
    }

    // personnel.contract_type
    {
        let d = get_domain_id(db, "personnel.contract_type").await?;
        seed_value(db, d, "CDI", "CDI", "CDI", "Permanent", None, 1, true).await?;
        seed_value(db, d, "CDD", "CDD", "CDD", "Fixed-term", None, 2, true).await?;
        seed_value(
            db,
            d,
            "INTERIMAIRE",
            "Int\u{00e9}rimaire",
            "Int\u{00e9}rimaire",
            "Temporary Agency",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PRESTATAIRE",
            "Prestataire externe",
            "Prestataire externe",
            "Contractor",
            None,
            4,
            true,
        )
        .await?;
        seed_value(db, d, "STAGIAIRE", "Stagiaire", "Stagiaire", "Intern", None, 5, false).await?;
    }

    // inventory.unit_of_measure
    {
        let d = get_domain_id(db, "inventory.unit_of_measure").await?;
        seed_value(db, d, "U", "Unit\u{00e9}", "Unit\u{00e9}", "Unit", None, 1, true).await?;
        seed_value(db, d, "KG", "kg", "kg", "kg", None, 2, true).await?;
        seed_value(db, d, "L", "L", "L", "L", None, 3, true).await?;
        seed_value(db, d, "M", "m", "m", "m", None, 4, true).await?;
        seed_value(db, d, "M2", "m\u{00b2}", "m\u{00b2}", "m\u{00b2}", None, 5, true).await?;
        seed_value(db, d, "BOX", "Bo\u{00ee}te", "Bo\u{00ee}te", "Box", None, 6, true).await?;
        seed_value(db, d, "ROUL", "Rouleau", "Rouleau", "Roll", None, 7, true).await?;
        seed_value(db, d, "PAIRE", "Paire", "Paire", "Pair", None, 8, true).await?;
    }

    // inventory.movement_type
    {
        let d = get_domain_id(db, "inventory.movement_type").await?;
        seed_value(
            db,
            d,
            "SORTIE_OT",
            "Sortie sur OT",
            "Sortie sur OT",
            "Issue to WO",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "ENTREE_ACHAT",
            "Entr\u{00e9}e achat",
            "Entr\u{00e9}e achat",
            "Purchase Receipt",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "RETOUR_OT",
            "Retour d'OT",
            "Retour d'OT",
            "Return from WO",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "AJUSTEMENT",
            "Ajustement inventaire",
            "Ajustement inventaire",
            "Inventory Adjustment",
            None,
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "INVENTAIRE",
            "Saisie inventaire",
            "Saisie inventaire",
            "Stock Count Entry",
            None,
            5,
            true,
        )
        .await?;
    }

    // org.responsibility_type
    {
        let d = get_domain_id(db, "org.responsibility_type").await?;
        seed_value(
            db,
            d,
            "MAINTENANCE_OWNER",
            "Responsable maintenance",
            "Responsable maintenance",
            "Maintenance Owner",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PRODUCTION_OWNER",
            "Responsable production",
            "Responsable production",
            "Production Owner",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "HSE_OWNER",
            "Responsable HSE",
            "Responsable HSE",
            "HSE Owner",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PLANNER",
            "Planificateur",
            "Planificateur",
            "Planner",
            None,
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "APPROVER",
            "Approbateur",
            "Approbateur",
            "Approver",
            None,
            5,
            true,
        )
        .await?;
    }

    // permit.type
    {
        let d = get_domain_id(db, "permit.type").await?;
        seed_value(
            db,
            d,
            "PERMIS_FEU",
            "Permis de feu",
            "Permis de feu",
            "Hot Work Permit",
            None,
            1,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PERMIS_ELECTRIQUE",
            "Permis \u{00e9}lectrique",
            "Permis \u{00e9}lectrique",
            "Electrical Permit",
            None,
            2,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PERMIS_HAUTEUR",
            "Travail en hauteur",
            "Travail en hauteur",
            "Work at Height",
            None,
            3,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PERMIS_ESPACE",
            "Espace confin\u{00e9}",
            "Espace confin\u{00e9}",
            "Confined Space",
            None,
            4,
            true,
        )
        .await?;
        seed_value(
            db,
            d,
            "PERMIS_GENERAL",
            "Permis g\u{00e9}n\u{00e9}ral",
            "Permis g\u{00e9}n\u{00e9}ral",
            "General Permit",
            None,
            5,
            true,
        )
        .await?;
    }

    // ── 3. Seed RBAC: permissions and system roles ───────────────────────
    seed_permissions(db).await?;
    seed_system_roles(db).await?;

    // ── 4. Record seed schema version in system_config ────────────────────
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO system_config (key, value, updated_at)
           VALUES ('seed_schema_version', ?, ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        [SEED_SCHEMA_VERSION.to_string().into(), now.into()],
    ))
    .await?;

    // ── 5. Seed bootstrap admin account ───────────────────────────────────
    seed_admin_account(db).await?;

    tracing::info!("seeder::complete — system seed version {} applied", SEED_SCHEMA_VERSION);
    Ok(())
}

/// Seeds baseline rows in `app_settings` if the table is still empty.
/// Safe to call on every startup after migrations.
pub async fn seed_default_settings(db: &DatabaseConnection) -> AppResult<()> {
    let count_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM app_settings;".to_string(),
        ))
        .await?;

    let count = count_row.and_then(|r| r.try_get::<i64>("", "cnt").ok()).unwrap_or(0);

    if count > 0 {
        tracing::debug!(
            count,
            "seed_default_settings: existing settings detected, skipping defaults"
        );
        return Ok(());
    }

    let now = Utc::now().to_rfc3339();

    for (setting_key, category, setting_scope, setting_value_json) in DEFAULT_SETTINGS {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO app_settings
                   (setting_key, category, setting_scope, setting_value_json,
                    setting_risk, validation_status, last_modified_at)
               VALUES (?, ?, ?, ?, 'low', 'valid', ?)",
            [
                (*setting_key).into(),
                (*category).into(),
                (*setting_scope).into(),
                (*setting_value_json).into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }

    tracing::info!(
        count = DEFAULT_SETTINGS.len(),
        "seed_default_settings: baseline settings inserted"
    );

    Ok(())
}

// ── Helper: insert domain if not exists ───────────────────────────────────
async fn seed_domain(
    db: &DatabaseConnection,
    domain_key: &str,
    display_name: &str,
    domain_type: &str,
    is_ordered: bool,
    is_extensible: bool,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT OR IGNORE INTO lookup_domains
               (sync_id, domain_key, display_name, domain_type,
                is_ordered, is_extensible, is_locked, schema_version,
                created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?, ?)",
        [
            sync_id.into(),
            domain_key.into(),
            display_name.into(),
            domain_type.into(),
            i32::from(is_ordered).into(),
            i32::from(is_extensible).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

// ── Helper: resolve domain id by key ─────────────────────────────────────
async fn get_domain_id(db: &DatabaseConnection, domain_key: &str) -> AppResult<i32> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL",
            [domain_key.into()],
        ))
        .await?;

    row.and_then(|r| r.try_get::<i32>("", "id").ok())
        .ok_or_else(|| AppError::NotFound {
            entity: "lookup_domain".into(),
            id: domain_key.to_string(),
        })
}

// ── Helper: insert value if not exists ───────────────────────────────────
#[allow(clippy::too_many_arguments)]
async fn seed_value(
    db: &DatabaseConnection,
    domain_id: i32,
    code: &str,
    label: &str,
    fr_label: &str,
    en_label: &str,
    color: Option<&str>,
    sort_order: i32,
    is_system: bool,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT OR IGNORE INTO lookup_values
               (sync_id, domain_id, code, label, fr_label, en_label,
                color, sort_order, is_active, is_system,
                created_at, updated_at, row_version)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 1)",
        [
            sync_id.into(),
            domain_id.into(),
            code.into(),
            label.into(),
            fr_label.into(),
            en_label.into(),
            color.map(std::string::ToString::to_string).into(),
            sort_order.into(),
            i32::from(is_system).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

// ── Helper: seed bootstrap admin account ──────────────────────────────────

// ── RBAC seeding: permissions and system roles ────────────────────────────

/// Seeds the full permission catalogue from PRD §6.7.
/// Uses INSERT OR IGNORE — safe to call multiple times.
/// Permissions are ordered by domain prefix then action suffix.
/// All seeded permissions have `is_system` = 1 and cannot be deleted.
async fn seed_permissions(db: &DatabaseConnection) -> AppResult<()> {
    // Permission rows: (name, description, category, is_dangerous, requires_step_up)
    // Format: dot-notation `domain.action`
    let permissions: &[(&str, &str, &str, bool, bool)] = &[
        // ── Equipment (eq) ──────────────────────────────────────────────────
        ("eq.view", "View equipment registry", "equipment", false, false),
        ("eq.manage", "Create/edit equipment records", "equipment", false, false),
        (
            "eq.import",
            "Import equipment from CSV / ERP",
            "equipment",
            false,
            false,
        ),
        ("eq.delete", "Soft-delete equipment records", "equipment", true, true),
        // ── Intervention Requests (di) ────────────────────────────────────
        ("di.view", "View intervention requests", "intervention", false, false),
        (
            "di.create",
            "Create intervention requests",
            "intervention",
            false,
            false,
        ),
        ("di.edit", "Edit intervention requests", "intervention", false, false),
        ("di.delete", "Delete intervention requests", "intervention", true, true),
        (
            "di.close",
            "Close/resolve intervention requests",
            "intervention",
            false,
            false,
        ),
        // ── Work Orders (ot) ──────────────────────────────────────────────
        ("ot.view", "View work orders", "work_order", false, false),
        ("ot.create", "Create work orders", "work_order", false, false),
        ("ot.edit", "Edit work orders", "work_order", false, false),
        ("ot.delete", "Delete work orders", "work_order", true, true),
        ("ot.close", "Close work orders", "work_order", false, false),
        ("ot.approve", "Approve work order execution", "work_order", false, false),
        // ── Organization (org) ───────────────────────────────────────────
        (
            "org.view",
            "View organizational structure",
            "organization",
            false,
            false,
        ),
        (
            "org.manage",
            "Create/edit org nodes and entities",
            "organization",
            false,
            false,
        ),
        (
            "org.admin",
            "Manage org structure model and types",
            "organization",
            true,
            true,
        ),
        // ── Personnel (per) ──────────────────────────────────────────────
        ("per.view", "View personnel records", "personnel", false, false),
        ("per.manage", "Create/edit personnel records", "personnel", false, false),
        (
            "per.sensitiveview",
            "View sensitive personnel fields",
            "personnel",
            false,
            false,
        ),
        // ── Reference Data (ref) ─────────────────────────────────────────
        ("ref.view", "View reference/lookup values", "reference", false, false),
        (
            "ref.manage",
            "Create/edit governed reference values",
            "reference",
            false,
            false,
        ),
        ("ref.publish", "Publish reference changes", "reference", true, true),
        // ── Inventory (inv) ──────────────────────────────────────────────
        ("inv.view", "View inventory and stock levels", "inventory", false, false),
        ("inv.manage", "Create/edit inventory records", "inventory", false, false),
        ("inv.adjust", "Post inventory adjustments", "inventory", true, true),
        (
            "inv.order",
            "Create purchase / replenishment orders",
            "inventory",
            false,
            false,
        ),
        // ── Preventive Maintenance (pm) ──────────────────────────────────
        ("pm.view", "View PM plans and schedules", "maintenance", false, false),
        ("pm.manage", "Create/edit PM plans", "maintenance", false, false),
        ("pm.approve", "Approve PM plan changes", "maintenance", false, false),
        // ── RAMS / Reliability (ram) ─────────────────────────────────────
        ("ram.view", "View RAMS / reliability data", "reliability", false, false),
        (
            "ram.manage",
            "Edit RAMS records and failure modes",
            "reliability",
            false,
            false,
        ),
        // ── Reports & Analytics (rep) ─────────────────────────────────────
        ("rep.view", "View standard reports", "reporting", false, false),
        ("rep.export", "Export report data", "reporting", false, false),
        ("rep.manage", "Create/edit custom reports", "reporting", false, false),
        // ── Archive Explorer (arc) ────────────────────────────────────────
        ("arc.view", "Browse archive entries", "archive", false, false),
        ("arc.export", "Export archived data", "archive", false, false),
        // ── Documentation (doc) ──────────────────────────────────────────
        (
            "doc.view",
            "View documentation and help content",
            "documentation",
            false,
            false,
        ),
        (
            "doc.manage",
            "Create/edit documentation articles",
            "documentation",
            false,
            false,
        ),
        // ── Administration (adm) ─────────────────────────────────────────
        ("adm.users", "Manage user accounts", "administration", true, true),
        (
            "adm.roles",
            "Manage roles and permissions",
            "administration",
            true,
            true,
        ),
        (
            "adm.permissions",
            "Assign permissions to roles",
            "administration",
            true,
            true,
        ),
        (
            "adm.settings",
            "Manage application settings",
            "administration",
            false,
            false,
        ),
        ("adm.audit", "View the full audit log", "administration", false, false),
        // ── Planning (plan) ──────────────────────────────────────────────
        (
            "plan.view",
            "View planning and scheduling data",
            "planning",
            false,
            false,
        ),
        ("plan.manage", "Manage planning schedules", "planning", false, false),
        // ── Audit Log (log) ──────────────────────────────────────────────
        ("log.view", "View activity feed", "audit", false, false),
        ("log.export", "Export audit log data", "audit", true, true),
        // ── Training (trn) ───────────────────────────────────────────────
        (
            "trn.view",
            "View training and certification records",
            "training",
            false,
            false,
        ),
        (
            "trn.manage",
            "Manage training records and plans",
            "training",
            false,
            false,
        ),
        ("trn.certify", "Issue or revoke certifications", "training", true, true),
        // ── IoT Integration (iot) ────────────────────────────────────────
        (
            "iot.view",
            "View IoT device data and readings",
            "integration",
            false,
            false,
        ),
        (
            "iot.manage",
            "Configure IoT gateways and devices",
            "integration",
            false,
            false,
        ),
        // ── ERP Connector (erp) ──────────────────────────────────────────
        (
            "erp.view",
            "View ERP sync status and mappings",
            "integration",
            false,
            false,
        ),
        (
            "erp.manage",
            "Configure ERP integration settings",
            "integration",
            true,
            true,
        ),
        (
            "erp.sync",
            "Trigger manual ERP synchronization",
            "integration",
            true,
            true,
        ),
        // ── Work Permits (ptw) ───────────────────────────────────────────
        ("ptw.view", "View work permits", "safety", false, false),
        ("ptw.create", "Create work permits", "safety", false, false),
        ("ptw.approve", "Approve or reject work permits", "safety", true, true),
        ("ptw.cancel", "Cancel active work permits", "safety", true, true),
        // ── Budget / Finance (fin) ───────────────────────────────────────
        ("fin.view", "View budgets and cost data", "finance", false, false),
        ("fin.manage", "Manage budgets and cost centers", "finance", false, false),
        ("fin.approve", "Approve budget changes", "finance", true, true),
        // ── Inspection (ins) ─────────────────────────────────────────────
        (
            "ins.view",
            "View inspection rounds and checklists",
            "inspection",
            false,
            false,
        ),
        (
            "ins.manage",
            "Create/edit inspection rounds",
            "inspection",
            false,
            false,
        ),
        (
            "ins.complete",
            "Complete inspection round executions",
            "inspection",
            false,
            false,
        ),
        // ── Configuration Engine (cfg) ───────────────────────────────────
        ("cfg.view", "View tenant configuration", "configuration", false, false),
        (
            "cfg.manage",
            "Manage tenant configuration rules",
            "configuration",
            true,
            true,
        ),
        (
            "cfg.publish",
            "Publish configuration changes",
            "configuration",
            true,
            true,
        ),
    ];

    let now = Utc::now().to_rfc3339();

    for (name, desc, category, is_dangerous, requires_step_up) in permissions {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permissions
                   (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
               VALUES (?, ?, ?, ?, ?, 1, ?)",
            [
                (*name).into(),
                (*desc).into(),
                (*category).into(),
                i32::from(*is_dangerous).into(),
                i32::from(*requires_step_up).into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }

    tracing::info!(count = permissions.len(), "seeder::permissions_seeded");
    Ok(())
}

/// Seeds the 4 system roles and assigns their initial permissions.
/// System roles are non-deletable and are the baseline for role templates.
async fn seed_system_roles(db: &DatabaseConnection) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();

    let system_roles: &[(&str, &str, &str)] = &[
        ("Administrator", "Full system access. Cannot be deleted.", "system"),
        (
            "Supervisor",
            "Full operational access. Can manage work, personnel, inventory.",
            "system",
        ),
        (
            "Operator",
            "Day-to-day CMMS use: view all, create and edit operational records.",
            "system",
        ),
        ("Readonly", "Read-only access to all operational modules.", "system"),
    ];

    for (name, desc, role_type) in system_roles {
        let sync_id = Uuid::new_v4().to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO roles
                   (sync_id, name, description, is_system, role_type, status, created_at, updated_at, row_version)
               VALUES (?, ?, ?, 1, ?, 'active', ?, ?, 1)",
            [
                sync_id.into(),
                (*name).into(),
                (*desc).into(),
                (*role_type).into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }

    // ── Assign permissions to roles using name-based resolution ───────────

    // Administrator -> all permissions
    if let Some(rid) = get_role_id_by_name(db, "Administrator").await? {
        assign_all_permissions_to_role(db, rid, &now).await?;
    }

    // Supervisor -> all operational permissions (excludes admin-only and config-only)
    let excluded_for_supervisor = [
        "adm.users",
        "adm.roles",
        "adm.permissions",
        "cfg.manage",
        "cfg.publish",
        "erp.manage",
        "erp.sync",
        "log.export",
    ];
    if let Some(rid) = get_role_id_by_name(db, "Supervisor").await? {
        assign_permissions_excluding(db, rid, &excluded_for_supervisor, &now).await?;
    }

    // Operator -> view + create/edit operational modules, no delete/approve/dangerous
    let operator_permissions = [
        "eq.view",
        "eq.manage",
        "di.view",
        "di.create",
        "di.edit",
        "di.close",
        "ot.view",
        "ot.create",
        "ot.edit",
        "org.view",
        "per.view",
        "ref.view",
        "inv.view",
        "inv.manage",
        "pm.view",
        "ram.view",
        "rep.view",
        "arc.view",
        "doc.view",
        "plan.view",
        "log.view",
        "trn.view",
        "iot.view",
        "erp.view",
        "ptw.view",
        "ptw.create",
        "fin.view",
        "ins.view",
        "ins.complete",
        "cfg.view",
        "adm.settings",
    ];
    if let Some(rid) = get_role_id_by_name(db, "Operator").await? {
        for perm_name in &operator_permissions {
            assign_permission_by_name(db, rid, perm_name, &now).await?;
        }
    }

    // Readonly -> only *.view permissions (subset of operator's list)
    if let Some(rid) = get_role_id_by_name(db, "Readonly").await? {
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        let view_perms: Vec<&str> = operator_permissions
            .iter()
            .copied()
            .filter(|p| p.ends_with(".view"))
            .collect();
        for perm_name in view_perms {
            assign_permission_by_name(db, rid, perm_name, &now).await?;
        }
    }

    tracing::info!("seeder::system_roles_seeded");
    Ok(())
}

// ── RBAC seeder helpers ───────────────────────────────────────────────────

async fn get_role_id_by_name(db: &DatabaseConnection, name: &str) -> AppResult<Option<i32>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE name = ? AND deleted_at IS NULL",
            [name.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i32>("", "id").ok()))
}

async fn assign_all_permissions_to_role(db: &DatabaseConnection, role_id: i32, now: &str) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
           SELECT ?, id, ? FROM permissions WHERE is_system = 1",
        [role_id.into(), now.into()],
    ))
    .await?;
    Ok(())
}

async fn assign_permissions_excluding(
    db: &DatabaseConnection,
    role_id: i32,
    excluded: &[&str],
    now: &str,
) -> AppResult<()> {
    let placeholders = excluded.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
           SELECT ?, id, ? FROM permissions
           WHERE is_system = 1 AND name NOT IN ({placeholders})",
    );
    let mut values: Vec<sea_orm::Value> = vec![role_id.into(), now.into()];
    for e in excluded {
        values.push((*e).into());
    }
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;
    Ok(())
}

async fn assign_permission_by_name(
    db: &DatabaseConnection,
    role_id: i32,
    permission_name: &str,
    now: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
           SELECT ?, id, ? FROM permissions WHERE name = ?",
        [role_id.into(), now.into(), permission_name.into()],
    ))
    .await?;
    Ok(())
}

// ── Helper: seed bootstrap admin account ──────────────────────────────────

/// Seeds the initial admin account for first launch.
/// Uses username "admin" with a known dev password "Admin#2026!" that forces
/// a password change on first login (`force_password_change = 1`).
///
/// This is only a development bootstrap credential. In production, the
/// first-launch wizard sets the administrator password interactively.
///
/// Safety: checks for existing admin and skips if present — will not
/// overwrite an existing admin account or its password.
async fn seed_admin_account(db: &DatabaseConnection) -> AppResult<()> {
    use crate::auth::password::hash_password;

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = ?",
            ["admin".into()],
        ))
        .await?;

    if existing.is_some() {
        tracing::debug!("seeder::admin_account already exists, skipping");
        return Ok(());
    }

    let password_hash = hash_password("Admin#2026!")?;

    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT OR IGNORE INTO user_accounts
               (sync_id, username, display_name, identity_mode, password_hash,
                is_active, is_admin, force_password_change,
                failed_login_attempts, created_at, updated_at, row_version)
           VALUES (?, ?, ?, ?, ?, 1, 1, 1, 0, ?, ?, 1)",
        [
            sync_id.into(),
            "admin".into(),
            "Administrateur Maintafox".into(),
            "local".into(),
            password_hash.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    tracing::info!("seeder::admin_account created (force_password_change=1)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_schema_version_is_positive() {
        assert!(
            SEED_SCHEMA_VERSION > 0,
            "Seed schema version must be a positive integer"
        );
    }

    #[test]
    fn default_settings_count_is_expected() {
        assert_eq!(
            DEFAULT_SETTINGS.len(),
            14,
            "Default settings count changed; update migration and verification checks"
        );
    }
}
