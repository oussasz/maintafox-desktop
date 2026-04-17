# Phase 1 · Sub-phase 03 · File 04
# Seed Data Integrity and Recovery Validation

## Context and Purpose

Files 01–03 built the schema, the migration framework, and the repository pattern.
The database is structurally sound and queryable. But two critical gaps remain:

1. **The reference domain tables are empty.** Every Phase 2 module dropdown, badge, and
   filter panel that calls `get_lookup_values("equipment.criticality")` will return an
   empty array until system seed data is loaded. The seed data — the governed vocabulary
   that ships with the product — must be inserted exactly once, be reproducible, and be
   idempotent (running it twice must not duplicate rows).

2. **The startup integrity check is incomplete.** `startup.rs` verifies that migrations
   ran, but it does not verify that seed data is present. If a developer or a technician
   deletes a required reference value, no subsystem notices until a feature fails at
   runtime. A startup integrity check that validates the critical seed domains must be
   added.

This file also introduces the **recovery validator**: a Tauri IPC command and a frontend
recovery screen that activates when the integrity check finds a repairable corruption
(missing seed values that can be re-inserted without data loss).

## Architecture Rules Applied

- Seed data is inserted via a dedicated Rust module `db/seeder.rs` that is idempotent
  (uses `INSERT OR IGNORE` semantics on the unique `(domain_id, code)` compound index).
- Seed data is versioned: each seed set carries a `seed_schema_version` stored in
  `system_config` table. If the version advances in a future release, the seeder inserts
  new rows without touching existing ones.
- The integrity check is a pure async Rust function returning `IntegrityReport`, not a
  side-effecting startup blocker. The startup sequence uses the report to decide whether
  to proceed, warn, or halt.
- System-protected seed values (`is_system = 1`) cannot be deleted through the normal
  lookup value CRUD; the service layer enforces this.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/db/seeder.rs` | Idempotent seed data inserter for all system domains |
| `src-tauri/src/db/integrity.rs` | Startup integrity check: verifies tables, seed domains, and reserved config keys |
| `src-tauri/src/commands/diagnostics.rs` | IPC commands: run_integrity_check, repair_seed_data |
| `src/hooks/use-integrity-check.ts` | Frontend hook: calls run_integrity_check, drives recovery UI |
| `src/pages/RecoveryPage.tsx` | Recovery screen shown when integrity check finds repairable corruption |
| Updated `startup.rs` | Wires seeder + integrity check into the startup sequence |
| `docs/SEED_DATA_REFERENCE.md` | Living document listing all system lookup domains and values |

## Prerequisites

- SP03-F01 complete: migrations 001–006 applied, all tables exist
- SP03-F02 complete: migration framework governance in place
- SP03-F03 complete: repository and service layer in place
- SP02-F01 complete: `startup.rs` startup sequence and `StartupEvent` emission

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | System Seed Data — All Governed Domains | `db/seeder.rs` with all 18 system lookup domains + values, system_config seed keys |
| S2 | Startup Integrity Check and Recovery Path | `db/integrity.rs`, `commands/diagnostics.rs`, updated startup sequence |
| S3 | Frontend Recovery Page and Integrity Hook | `RecoveryPage.tsx`, `use-integrity-check.ts`, router update, i18n additions |

---

## Sprint S1 — System Seed Data: All Governed Domains

### AI Agent Prompt

```
You are a senior Rust engineer working on Maintafox Desktop (Tauri 2.x, sea-orm 1.x).
Sub-phase 03 Files 01–03 are complete. Migration 003 defined the lookup_domains,
lookup_values, and lookup_value_aliases tables. The tables currently exist but are empty.

YOUR TASK: Create the system seed data module that inserts all 18 system-governed
lookup domains and their values. The seeder must be:
- Idempotent: running it twice must not create duplicate rows
- Versioned: updates do not delete existing rows, they only add missing ones
- Auditable: seeded values carry is_system = 1 so the service layer protects them

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/db/seeder.rs
─────────────────────────────────────────────────────────────────────
The seeder is a single async function `seed_system_data` that accepts a
`DatabaseConnection` and idempotently inserts all system data.

Structure:
1. For each domain: INSERT OR IGNORE into lookup_domains (on `domain_key` conflict)
2. For each value: INSERT OR IGNORE into lookup_values (on `(domain_id, code)` conflict)
3. Update system_config to record the seed schema version

```rust
// src-tauri/src/db/seeder.rs

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use chrono::Utc;
use uuid::Uuid;
use crate::errors::{AppError, AppResult};

/// Current version of the system seed data set.
/// Increment this when adding new system domains or values in a release.
pub const SEED_SCHEMA_VERSION: i32 = 1;

/// Inserts all system-governed lookup domains and values idempotently.
/// Safe to call on every startup: uses INSERT OR IGNORE semantics.
/// On completion, records `seed_schema_version = SEED_SCHEMA_VERSION` in system_config.
pub async fn seed_system_data(db: &DatabaseConnection) -> AppResult<()> {
    tracing::info!("seeder::starting system seed (version {})", SEED_SCHEMA_VERSION);

    // ── 1. Insert all domain definitions ─────────────────────────────────
    seed_domain(db, "equipment.criticality",
        "Criticité équipement", "system", false, true).await?;
    seed_domain(db, "equipment.lifecycle_status",
        "Statut cycle de vie équipement", "system", false, false).await?;
    seed_domain(db, "equipment.hierarchy_relationship",
        "Type de relation hiérarchique équipement", "system", false, false).await?;
    seed_domain(db, "intervention_request.type",
        "Type de demande d'intervention", "tenant", false, true).await?;
    seed_domain(db, "intervention_request.urgency",
        "Urgence demande d'intervention", "system", true, true).await?;
    seed_domain(db, "intervention_request.status",
        "Statut demande d'intervention", "system", false, false).await?;
    seed_domain(db, "work_order.type",
        "Type d'ordre de travail", "tenant", false, true).await?;
    seed_domain(db, "work_order.status",
        "Statut ordre de travail", "system", false, false).await?;
    seed_domain(db, "work_order.priority",
        "Priorité ordre de travail", "system", true, true).await?;
    seed_domain(db, "failure.mode",
        "Mode de défaillance", "tenant", false, true).await?;
    seed_domain(db, "failure.cause",
        "Cause de défaillance", "tenant", false, true).await?;
    seed_domain(db, "work_order.closure_reason",
        "Motif de clôture OT", "tenant", false, true).await?;
    seed_domain(db, "personnel.skill_proficiency",
        "Niveau de compétence", "system", true, false).await?;
    seed_domain(db, "personnel.contract_type",
        "Type de contrat", "tenant", false, true).await?;
    seed_domain(db, "inventory.unit_of_measure",
        "Unité de mesure stock", "tenant", false, true).await?;
    seed_domain(db, "inventory.movement_type",
        "Type de mouvement stock", "system", false, false).await?;
    seed_domain(db, "org.responsibility_type",
        "Type de responsabilité organisationnelle", "system", false, true).await?;
    seed_domain(db, "permit.type",
        "Type de permis de travail", "tenant", false, true).await?;

    // ── 2. Resolve domain ids and insert values per domain ────────────────

    // equipment.criticality
    {
        let d = get_domain_id(db, "equipment.criticality").await?;
        seed_value(db, d, "CRITIQUE",       "Critique",      "Critique",    "Critical",     "#dc3545",  1, true).await?;
        seed_value(db, d, "IMPORTANT",      "Important",     "Important",   "Important",    "#ffc107",  2, true).await?;
        seed_value(db, d, "STANDARD",       "Standard",      "Standard",    "Standard",     "#0dcaf0",  3, true).await?;
        seed_value(db, d, "NON_CRITIQUE",   "Non-critique",  "Non-critique","Non-critical", "#198754",  4, true).await?;
    }

    // equipment.lifecycle_status
    {
        let d = get_domain_id(db, "equipment.lifecycle_status").await?;
        seed_value(db, d, "ACTIVE_IN_SERVICE",  "En service",         "En service",        "In Service",        "#198754",  1, true).await?;
        seed_value(db, d, "IN_STOCK",           "En stock",           "En stock",          "In Stock",          "#0dcaf0",  2, true).await?;
        seed_value(db, d, "UNDER_MAINTENANCE",  "En maintenance",     "En maintenance",    "Under Maintenance", "#ffc107",  3, true).await?;
        seed_value(db, d, "DECOMMISSIONED",     "Mis hors service",   "Mis hors service",  "Decommissioned",    "#6c757d",  4, true).await?;
        seed_value(db, d, "SCRAPPED",           "Mis au rebut",       "Mis au rebut",      "Scrapped",          "#dc3545",  5, true).await?;
        seed_value(db, d, "SPARE",              "Pièce de rechange",  "Pièce de rechange", "Spare",             "#6c757d",  6, true).await?;
    }

    // equipment.hierarchy_relationship
    {
        let d = get_domain_id(db, "equipment.hierarchy_relationship").await?;
        seed_value(db, d, "PARENT_CHILD", "Parent — Enfant",    "Parent — Enfant",    "Parent — Child",   NULL_COLOR, 1, true).await?;
        seed_value(db, d, "INSTALLED_IN", "Installé dans",      "Installé dans",      "Installed In",     NULL_COLOR, 2, true).await?;
        seed_value(db, d, "DRIVES",       "Entraîne",           "Entraîne",           "Drives",           NULL_COLOR, 3, true).await?;
        seed_value(db, d, "FEEDS",        "Alimente",           "Alimente",           "Feeds",            NULL_COLOR, 4, true).await?;
    }

    // intervention_request.type (tenant-extensible examples)
    {
        let d = get_domain_id(db, "intervention_request.type").await?;
        seed_value(db, d, "CORRECTIVE",   "Corrective",        "Corrective",    "Corrective",    NULL_COLOR, 1, true).await?;
        seed_value(db, d, "SIGNALEMENT",  "Signalement",       "Signalement",   "Observation",   NULL_COLOR, 2, true).await?;
        seed_value(db, d, "AMELIORATION", "Amélioration",      "Amélioration",  "Improvement",   NULL_COLOR, 3, false).await?;
    }

    // intervention_request.urgency
    {
        let d = get_domain_id(db, "intervention_request.urgency").await?;
        seed_value(db, d, "IMMEDIATE",  "Immédiate",    "Immédiate",    "Immediate",    "#dc3545",  1, true).await?;
        seed_value(db, d, "URGENT",     "Urgente",      "Urgente",      "Urgent",       "#ffc107",  2, true).await?;
        seed_value(db, d, "NORMALE",    "Normale",      "Normale",      "Normal",       "#198754",  3, true).await?;
        seed_value(db, d, "PLANIFIEE",  "Planifiée",    "Planifiée",    "Planned",      "#0dcaf0",  4, true).await?;
    }

    // intervention_request.status
    {
        let d = get_domain_id(db, "intervention_request.status").await?;
        seed_value(db, d, "DRAFT",          "Brouillon",      "Brouillon",      "Draft",          "#6c757d",  1, true).await?;
        seed_value(db, d, "SUBMITTED",      "Soumise",        "Soumise",        "Submitted",      "#0dcaf0",  2, true).await?;
        seed_value(db, d, "ACKNOWLEDGED",   "Accusée",        "Accusée",        "Acknowledged",   "#ffc107",  3, true).await?;
        seed_value(db, d, "IN_PROGRESS",    "En cours",       "En cours",       "In Progress",    "#003d8f",  4, true).await?;
        seed_value(db, d, "COMPLETED",      "Clôturée",       "Clôturée",       "Completed",      "#198754",  5, true).await?;
        seed_value(db, d, "REJECTED",       "Rejetée",        "Rejetée",        "Rejected",       "#dc3545",  6, true).await?;
        seed_value(db, d, "CANCELLED",      "Annulée",        "Annulée",        "Cancelled",      "#6c757d",  7, true).await?;
    }

    // work_order.type
    {
        let d = get_domain_id(db, "work_order.type").await?;
        seed_value(db, d, "CORRECTIVE",   "Corrective",       "Corrective",       "Corrective",      NULL_COLOR, 1, true).await?;
        seed_value(db, d, "PREVENTIVE",   "Préventive",       "Préventive",       "Preventive",      NULL_COLOR, 2, true).await?;
        seed_value(db, d, "PREDICTIVE",   "Prédictive",       "Prédictive",       "Predictive",      NULL_COLOR, 3, true).await?;
        seed_value(db, d, "AMELIORATIVE", "Améliorative",     "Améliorative",     "Improvement",     NULL_COLOR, 4, true).await?;
        seed_value(db, d, "INSPECTION",   "Inspection",       "Inspection",       "Inspection",      NULL_COLOR, 5, true).await?;
    }

    // work_order.status
    {
        let d = get_domain_id(db, "work_order.status").await?;
        seed_value(db, d, "DRAFT",          "Brouillon",       "Brouillon",       "Draft",           "#6c757d",  1, true).await?;
        seed_value(db, d, "PLANNED",        "Planifié",        "Planifié",        "Planned",         "#0dcaf0",  2, true).await?;
        seed_value(db, d, "RELEASED",       "Lancé",           "Lancé",           "Released",        "#003d8f",  3, true).await?;
        seed_value(db, d, "IN_PROGRESS",    "En cours",        "En cours",        "In Progress",     "#ffc107",  4, true).await?;
        seed_value(db, d, "ON_HOLD",        "En attente",      "En attente",      "On Hold",         "#f0a500",  5, true).await?;
        seed_value(db, d, "COMPLETED",      "Terminé",         "Terminé",         "Completed",       "#198754",  6, true).await?;
        seed_value(db, d, "CLOSED",         "Clôturé",         "Clôturé",         "Closed",          "#6c757d",  7, true).await?;
        seed_value(db, d, "CANCELLED",      "Annulé",          "Annulé",          "Cancelled",       "#dc3545",  8, true).await?;
    }

    // work_order.priority
    {
        let d = get_domain_id(db, "work_order.priority").await?;
        seed_value(db, d, "P1_CRITICAL",  "P1 — Critique",    "P1 — Critique",    "P1 — Critical",    "#dc3545",  1, true).await?;
        seed_value(db, d, "P2_HIGH",      "P2 — Haute",       "P2 — Haute",       "P2 — High",        "#ffc107",  2, true).await?;
        seed_value(db, d, "P3_MEDIUM",    "P3 — Moyenne",     "P3 — Moyenne",     "P3 — Medium",      "#0dcaf0",  3, true).await?;
        seed_value(db, d, "P4_LOW",       "P4 — Basse",       "P4 — Basse",       "P4 — Low",         "#198754",  4, true).await?;
    }

    // failure.mode (examples — tenant-extensible)
    {
        let d = get_domain_id(db, "failure.mode").await?;
        seed_value(db, d, "VIBRATION",   "Vibration",           "Vibration",           "Vibration",      NULL_COLOR, 1, true).await?;
        seed_value(db, d, "CORROSION",   "Corrosion",           "Corrosion",           "Corrosion",      NULL_COLOR, 2, true).await?;
        seed_value(db, d, "BRUIT",       "Bruit anormal",       "Bruit anormal",       "Abnormal Noise", NULL_COLOR, 3, true).await?;
        seed_value(db, d, "FUITE",       "Fuite",               "Fuite",               "Leak",           NULL_COLOR, 4, true).await?;
        seed_value(db, d, "SURCHAUFFE",  "Surchauffe",          "Surchauffe",          "Overheating",    NULL_COLOR, 5, true).await?;
        seed_value(db, d, "PANNE_ELEC",  "Panne électrique",    "Panne électrique",    "Electrical Fault",NULL_COLOR,6, true).await?;
        seed_value(db, d, "AUTRE",       "Autre",               "Autre",               "Other",          NULL_COLOR, 99, true).await?;
    }

    // failure.cause
    {
        let d = get_domain_id(db, "failure.cause").await?;
        seed_value(db, d, "USURE_NORMALE",   "Usure normale",      "Usure normale",      "Normal Wear",         NULL_COLOR, 1, true).await?;
        seed_value(db, d, "MAUVAIS_USAGE",   "Mauvais usage",      "Mauvais usage",      "Misuse",              NULL_COLOR, 2, true).await?;
        seed_value(db, d, "DEFAUT_ENTRETIEN","Défaut d'entretien", "Défaut d'entretien", "Maintenance Defect",  NULL_COLOR, 3, true).await?;
        seed_value(db, d, "DEFAUT_INSTALL",  "Défaut d'installation","Défaut d'installation","Installation Defect",NULL_COLOR,4, true).await?;
        seed_value(db, d, "DEFAUT_MATERIEL", "Défaut matériel",    "Défaut matériel",    "Material Defect",     NULL_COLOR, 5, true).await?;
        seed_value(db, d, "INCONNU",         "Inconnu",            "Inconnu",            "Unknown",             NULL_COLOR, 99,true).await?;
    }

    // work_order.closure_reason
    {
        let d = get_domain_id(db, "work_order.closure_reason").await?;
        seed_value(db, d, "REPARE",          "Réparé",             "Réparé",             "Repaired",         "#198754",  1, true).await?;
        seed_value(db, d, "REPORTE",         "Reporté",            "Reporté",            "Deferred",         "#ffc107",  2, true).await?;
        seed_value(db, d, "NON_NECESSAIRE",  "Non nécessaire",     "Non nécessaire",     "Not Required",     "#6c757d",  3, true).await?;
        seed_value(db, d, "REMPLACE",        "Remplacé",           "Remplacé",           "Replaced",         "#0dcaf0",  4, true).await?;
    }

    // personnel.skill_proficiency
    {
        let d = get_domain_id(db, "personnel.skill_proficiency").await?;
        seed_value(db, d, "NIVEAU_1", "Niveau 1 — Notions",        "Niveau 1 — Notions",        "Level 1 — Awareness",    NULL_COLOR, 1, true).await?;
        seed_value(db, d, "NIVEAU_2", "Niveau 2 — Appliqué",       "Niveau 2 — Appliqué",       "Level 2 — Applied",      NULL_COLOR, 2, true).await?;
        seed_value(db, d, "NIVEAU_3", "Niveau 3 — Maîtrisé",       "Niveau 3 — Maîtrisé",       "Level 3 — Proficient",   NULL_COLOR, 3, true).await?;
        seed_value(db, d, "NIVEAU_4", "Niveau 4 — Expert",         "Niveau 4 — Expert",         "Level 4 — Expert",       NULL_COLOR, 4, true).await?;
        seed_value(db, d, "NIVEAU_5", "Niveau 5 — Maître formateur","Niveau 5 — Maître formateur","Level 5 — Master Trainer",NULL_COLOR,5, true).await?;
    }

    // personnel.contract_type
    {
        let d = get_domain_id(db, "personnel.contract_type").await?;
        seed_value(db, d, "CDI",         "CDI",                 "CDI",                 "Permanent",         NULL_COLOR, 1, true).await?;
        seed_value(db, d, "CDD",         "CDD",                 "CDD",                 "Fixed-term",        NULL_COLOR, 2, true).await?;
        seed_value(db, d, "INTERIMAIRE", "Intérimaire",         "Intérimaire",         "Temporary Agency",  NULL_COLOR, 3, true).await?;
        seed_value(db, d, "PRESTATAIRE", "Prestataire externe", "Prestataire externe", "Contractor",        NULL_COLOR, 4, true).await?;
        seed_value(db, d, "STAGIAIRE",   "Stagiaire",           "Stagiaire",           "Intern",            NULL_COLOR, 5, false).await?;
    }

    // inventory.unit_of_measure
    {
        let d = get_domain_id(db, "inventory.unit_of_measure").await?;
        seed_value(db, d, "U",    "Unité",    "Unité",    "Unit",     NULL_COLOR,  1, true).await?;
        seed_value(db, d, "KG",   "kg",       "kg",       "kg",       NULL_COLOR,  2, true).await?;
        seed_value(db, d, "L",    "L",        "L",        "L",        NULL_COLOR,  3, true).await?;
        seed_value(db, d, "M",    "m",        "m",        "m",        NULL_COLOR,  4, true).await?;
        seed_value(db, d, "M2",   "m²",       "m²",       "m²",       NULL_COLOR,  5, true).await?;
        seed_value(db, d, "BOX",  "Boîte",    "Boîte",    "Box",      NULL_COLOR,  6, true).await?;
        seed_value(db, d, "ROUL", "Rouleau",  "Rouleau",  "Roll",     NULL_COLOR,  7, true).await?;
        seed_value(db, d, "PAIRE","Paire",    "Paire",    "Pair",     NULL_COLOR,  8, true).await?;
    }

    // inventory.movement_type
    {
        let d = get_domain_id(db, "inventory.movement_type").await?;
        seed_value(db, d, "SORTIE_OT",      "Sortie sur OT",       "Sortie sur OT",       "Issue to WO",          NULL_COLOR, 1, true).await?;
        seed_value(db, d, "ENTREE_ACHAT",   "Entrée achat",        "Entrée achat",        "Purchase Receipt",     NULL_COLOR, 2, true).await?;
        seed_value(db, d, "RETOUR_OT",      "Retour d'OT",         "Retour d'OT",         "Return from WO",       NULL_COLOR, 3, true).await?;
        seed_value(db, d, "AJUSTEMENT",     "Ajustement inventaire","Ajustement inventaire","Inventory Adjustment",NULL_COLOR, 4, true).await?;
        seed_value(db, d, "INVENTAIRE",     "Saisie inventaire",   "Saisie inventaire",   "Stock Count Entry",    NULL_COLOR, 5, true).await?;
    }

    // org.responsibility_type
    {
        let d = get_domain_id(db, "org.responsibility_type").await?;
        seed_value(db, d, "MAINTENANCE_OWNER",  "Responsable maintenance", "Responsable maintenance", "Maintenance Owner", NULL_COLOR, 1, true).await?;
        seed_value(db, d, "PRODUCTION_OWNER",   "Responsable production",  "Responsable production",  "Production Owner",  NULL_COLOR, 2, true).await?;
        seed_value(db, d, "HSE_OWNER",          "Responsable HSE",         "Responsable HSE",         "HSE Owner",         NULL_COLOR, 3, true).await?;
        seed_value(db, d, "PLANNER",            "Planificateur",           "Planificateur",           "Planner",           NULL_COLOR, 4, true).await?;
        seed_value(db, d, "APPROVER",           "Approbateur",             "Approbateur",             "Approver",          NULL_COLOR, 5, true).await?;
    }

    // permit.type
    {
        let d = get_domain_id(db, "permit.type").await?;
        seed_value(db, d, "PERMIS_FEU",       "Permis de feu",          "Permis de feu",          "Hot Work Permit",       NULL_COLOR, 1, true).await?;
        seed_value(db, d, "PERMIS_ELECTRIQUE","Permis électrique",       "Permis électrique",       "Electrical Permit",    NULL_COLOR, 2, true).await?;
        seed_value(db, d, "PERMIS_HAUTEUR",   "Travail en hauteur",      "Travail en hauteur",      "Work at Height",        NULL_COLOR, 3, true).await?;
        seed_value(db, d, "PERMIS_ESPACE",    "Espace confiné",          "Espace confiné",          "Confined Space",        NULL_COLOR, 4, true).await?;
        seed_value(db, d, "PERMIS_GENERAL",   "Permis général",          "Permis général",          "General Permit",        NULL_COLOR, 5, true).await?;
    }

    // ── 3. Record seed schema version in system_config ────────────────────
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO system_config (key, value, updated_at)
           VALUES ('seed_schema_version', ?, ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        "#,
        [SEED_SCHEMA_VERSION.to_string().into(), now.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    tracing::info!("seeder::complete — system seed version {} applied", SEED_SCHEMA_VERSION);
    Ok(())
}

// ── Helper: no-color sentinel ─────────────────────────────────────────────
const NULL_COLOR: Option<&str> = None;

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
        r#"INSERT OR IGNORE INTO lookup_domains
               (sync_id, domain_key, display_name, domain_type,
                is_ordered, is_extensible, is_locked, schema_version,
                created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?, ?)
        "#,
        [
            sync_id.into(),
            domain_key.into(),
            display_name.into(),
            domain_type.into(),
            (is_ordered as i32).into(),
            (is_extensible as i32).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

// ── Helper: resolve domain id by key ─────────────────────────────────────
async fn get_domain_id(
    db: &DatabaseConnection,
    domain_key: &str,
) -> AppResult<i32> {
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL",
        [domain_key.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    row.and_then(|r| r.try_get::<i32>("", "id").ok())
        .ok_or_else(|| AppError::NotFound {
            entity: "lookup_domain".into(),
            id: domain_key.to_string(),
        })
}

// ── Helper: insert value if not exists ───────────────────────────────────
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
        r#"INSERT OR IGNORE INTO lookup_values
               (sync_id, domain_id, code, label, fr_label, en_label,
                color, sort_order, is_active, is_system,
                created_at, updated_at, row_version)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 1)
        "#,
        [
            sync_id.into(),
            domain_id.into(),
            code.into(),
            label.into(),
            fr_label.into(),
            en_label.into(),
            color.map(|s| s.to_string()).into(),
            sort_order.into(),
            (is_system as i32).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_schema_version_is_positive() {
        assert!(SEED_SCHEMA_VERSION > 0, "Seed schema version must be a positive integer");
    }

    #[test]
    fn null_color_sentinel_is_none() {
        assert!(NULL_COLOR.is_none());
    }
}
```

Register seeder in `src-tauri/src/db/mod.rs`:
```rust
pub mod seeder;
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create docs/SEED_DATA_REFERENCE.md
─────────────────────────────────────────────────────────────────────
```markdown
# Seed Data Reference

Source: `src-tauri/src/db/seeder.rs` — version controlled alongside the code.

## System Seed Schema Version: 1

Recorded in `system_config` table under key `seed_schema_version`.

## Governed Domains (18 system domains)

| Domain Key | Display Name (FR) | Type | Extensible | Ordered | Values |
|------------|------------------|------|-----------|---------|--------|
| `equipment.criticality` | Criticité équipement | system | No | Yes | 4 |
| `equipment.lifecycle_status` | Statut cycle de vie équipement | system | No | No | 6 |
| `equipment.hierarchy_relationship` | Type de relation hiérarchique | system | No | No | 4 |
| `intervention_request.type` | Type de demande d'intervention | tenant | Yes | No | 3 |
| `intervention_request.urgency` | Urgence DI | system | No | Yes | 4 |
| `intervention_request.status` | Statut DI | system | No | No | 7 |
| `work_order.type` | Type d'OT | tenant | Yes | No | 5 |
| `work_order.status` | Statut OT | system | No | No | 8 |
| `work_order.priority` | Priorité OT | system | No | Yes | 4 |
| `failure.mode` | Mode de défaillance | tenant | Yes | No | 7 |
| `failure.cause` | Cause de défaillance | tenant | Yes | No | 6 |
| `work_order.closure_reason` | Motif de clôture OT | tenant | Yes | No | 4 |
| `personnel.skill_proficiency` | Niveau de compétence | system | No | Yes | 5 |
| `personnel.contract_type` | Type de contrat | tenant | Yes | No | 5 |
| `inventory.unit_of_measure` | Unité de mesure stock | tenant | Yes | No | 8 |
| `inventory.movement_type` | Type de mouvement stock | system | No | No | 5 |
| `org.responsibility_type` | Type de responsabilité org. | system | No | Yes | 5 |
| `permit.type` | Type de permis de travail | tenant | Yes | No | 5 |

## Protected Values

Values with `is_system = 1` cannot be deleted via the Lookup Manager UI. They can be
deactivated (set `is_active = 0`) but their codes remain reserved and are used by the
application's business logic for conditional rendering and workflow routing.

## Adding New System Values in a Future Release

1. Add the new `seed_value()` call to `seeder.rs`
2. Increment `SEED_SCHEMA_VERSION`
3. The seeder uses `INSERT OR IGNORE` — existing values are untouched
4. Update this document to reflect the new values
5. Add a migration integrity test assertion if the value is load-bearing
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures
- pnpm run dev starts cleanly; database now has rows in lookup_domains and lookup_values
- DBeaver shows 18 rows in lookup_domains and at least 90 rows in lookup_values
- system_config table has a row with key = 'seed_schema_version' and value = '1'
- docs/SEED_DATA_REFERENCE.md is present and lists all 18 domains
```

---

### Supervisor Verification — Sprint S1

**V1 — Seed data is present after startup.**
Run `pnpm run dev`. Then open the database in DBeaver. Click on `lookup_domains` and
select "View Data" (or run `SELECT COUNT(*) FROM lookup_domains;`). You should see 18
rows. Click on `lookup_values` — there should be at least 90 rows. If either count is 0,
the seeder is not running. Flag it.

**V2 — Seed schema version is recorded.**
In DBeaver, run: `SELECT value FROM system_config WHERE key = 'seed_schema_version';`
The result should be `1`. If the row is absent or the value is different, flag it.

**V3 — Idempotency works.**
Stop the application and run it again a second time. Open DBeaver and check
`SELECT COUNT(*) FROM lookup_domains;` again. The count must still be 18 — not 36.
If the second restart created duplicates, the INSERT OR IGNORE semantics are broken.
Flag it.

**V4 — French labels in equipment.criticality.**
Run: `SELECT code, label, fr_label FROM lookup_values lv JOIN lookup_domains ld on ld.id = lv.domain_id WHERE ld.domain_key = 'equipment.criticality';`
You should see 4 rows: CRITIQUE, IMPORTANT, STANDARD, NON_CRITIQUE with French labels.

---

## Sprint S2 — Startup Integrity Check and Recovery Path

### AI Agent Prompt

```
You are a senior Rust engineer continuing work on Maintafox Desktop. Sprint S1 is
complete: 18 system lookup domains and their values are seeded idempotently on every
startup. The system_config table records the seed schema version.

YOUR TASK: Build the integrity check system that runs after seeding and before the app
emits the `ready` StartupEvent. This check validates that all critical seed domains are
present and that no required system values were accidentally deleted. It also creates
an IPC command that the frontend can call on-demand to re-run the check and trigger a
self-repair if the check finds repairable issues.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/db/integrity.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/db/integrity.rs

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use crate::errors::{AppError, AppResult};

// ── Report types ──────────────────────────────────────────────────────────

/// The result of a startup integrity check.
#[derive(Debug, Clone, Serialize)]
pub struct IntegrityReport {
    /// True if everything is healthy and the app can start normally.
    pub is_healthy: bool,
    /// True if the found issues are recoverable without data loss.
    pub is_recoverable: bool,
    /// List of issues found. Empty if is_healthy is true.
    pub issues: Vec<IntegrityIssue>,
    /// Seed schema version found in system_config (None if not yet seeded).
    pub seed_schema_version: Option<i32>,
    /// Total number of lookup domains found.
    pub domain_count: i32,
    /// Total number of lookup values found.
    pub value_count: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrityIssue {
    /// Machine-readable issue code.
    pub code: String,
    /// Human-readable description (French).
    pub description: String,
    /// Whether this issue can be auto-repaired by re-running the seeder.
    pub is_auto_repairable: bool,
    /// The domain_key or table name that has the issue.
    pub subject: String,
}

// ── Required seed domain keys — must all be present for the app to start ─

const REQUIRED_DOMAINS: &[&str] = &[
    "equipment.criticality",
    "equipment.lifecycle_status",
    "intervention_request.urgency",
    "intervention_request.status",
    "work_order.status",
    "work_order.priority",
    "personnel.skill_proficiency",
];

/// Minimum number of active values that must be present in each required domain.
const REQUIRED_DOMAIN_MIN_VALUES: &[(&str, i32)] = &[
    ("equipment.criticality",         2),
    ("equipment.lifecycle_status",    3),
    ("intervention_request.urgency",  2),
    ("intervention_request.status",   4),
    ("work_order.status",             4),
    ("work_order.priority",           2),
    ("personnel.skill_proficiency",   3),
];

// ── Integrity check ───────────────────────────────────────────────────────

/// Runs the full startup integrity check and returns a report.
/// Does not modify any data.
pub async fn run_integrity_check(db: &DatabaseConnection) -> AppResult<IntegrityReport> {
    let mut issues: Vec<IntegrityIssue> = Vec::new();

    // ── Check 1: migration tables exist ──────────────────────────────────
    for table in &["lookup_domains", "lookup_values", "system_config"] {
        let sql = format!("SELECT COUNT(*) as cnt FROM {};", table);
        if let Err(e) = db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await {
            issues.push(IntegrityIssue {
                code: "MISSING_TABLE".into(),
                description: format!("Table '{}' introuvable : {}", table, e),
                is_auto_repairable: false,
                subject: table.to_string(),
            });
        }
    }

    // If tables are missing, abort further checks — rest would panic
    if !issues.is_empty() {
        return Ok(IntegrityReport {
            is_healthy: false,
            is_recoverable: false,
            issues,
            seed_schema_version: None,
            domain_count: 0,
            value_count: 0,
        });
    }

    // ── Check 2: seed schema version ─────────────────────────────────────
    let seed_version: Option<i32> = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT value FROM system_config WHERE key = 'seed_schema_version'",
        [],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|r| r.try_get::<String>("", "value").ok())
    .and_then(|s| s.parse::<i32>().ok());

    if seed_version.is_none() {
        issues.push(IntegrityIssue {
            code: "SEED_NOT_APPLIED".into(),
            description: "Les données système de base n'ont pas été initialisées.".into(),
            is_auto_repairable: true,
            subject: "system_config::seed_schema_version".into(),
        });
    }

    // ── Check 3: counts ───────────────────────────────────────────────────
    let domain_count: i32 = db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM lookup_domains WHERE deleted_at IS NULL".to_string(),
    ))
    .await.ok().flatten()
    .and_then(|r| r.try_get::<i64>("", "cnt").ok())
    .unwrap_or(0) as i32;

    let value_count: i32 = db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM lookup_values WHERE deleted_at IS NULL".to_string(),
    ))
    .await.ok().flatten()
    .and_then(|r| r.try_get::<i64>("", "cnt").ok())
    .unwrap_or(0) as i32;

    // ── Check 4: required domains present ────────────────────────────────
    for &domain_key in REQUIRED_DOMAINS {
        let row = db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL",
            [domain_key.into()],
        ))
        .await.ok().flatten();

        if row.is_none() {
            issues.push(IntegrityIssue {
                code: "MISSING_DOMAIN".into(),
                description: format!(
                    "Domaine système requis absent : '{}'. La réparation automatique permettra de le restaurer.",
                    domain_key
                ),
                is_auto_repairable: true,
                subject: domain_key.to_string(),
            });
        }
    }

    // ── Check 5: minimum value counts per required domain ─────────────────
    for &(domain_key, min_count) in REQUIRED_DOMAIN_MIN_VALUES {
        let actual: i32 = db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT COUNT(*) as cnt FROM lookup_values lv
               INNER JOIN lookup_domains ld ON ld.id = lv.domain_id
               WHERE ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL"#,
            [domain_key.into()],
        ))
        .await.ok().flatten()
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0) as i32;

        if actual < min_count {
            issues.push(IntegrityIssue {
                code: "INSUFFICIENT_VALUES".into(),
                description: format!(
                    "Domaine '{}' : {} valeur(s) active(s) trouvée(s), minimum requis : {}.",
                    domain_key, actual, min_count
                ),
                is_auto_repairable: true,
                subject: domain_key.to_string(),
            });
        }
    }

    let is_recoverable = issues.iter().all(|i| i.is_auto_repairable);
    let is_healthy = issues.is_empty();

    Ok(IntegrityReport {
        is_healthy,
        is_recoverable,
        issues,
        seed_schema_version: seed_version,
        domain_count,
        value_count,
    })
}
```

Register in `src-tauri/src/db/mod.rs`:
```rust
pub mod integrity;
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/src/commands/diagnostics.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/commands/diagnostics.rs

use tauri::State;
use crate::state::AppState;
use crate::errors::AppResult;
use crate::db::{integrity, seeder};

/// Runs the database integrity check and returns a report.
/// Called by the frontend on startup and from the diagnostics panel.
#[tauri::command]
pub async fn run_integrity_check(
    state: State<'_, AppState>,
) -> AppResult<integrity::IntegrityReport> {
    integrity::run_integrity_check(&state.db).await
}

/// Re-applies the system seed data and re-runs the integrity check.
/// Used for self-repair when the integrity check found recoverable issues.
/// Safe to call even if seed data is already present (idempotent).
#[tauri::command]
pub async fn repair_seed_data(
    state: State<'_, AppState>,
) -> AppResult<integrity::IntegrityReport> {
    tracing::info!("diagnostics::repair_seed_data called");
    seeder::seed_system_data(&state.db).await?;
    integrity::run_integrity_check(&state.db).await
}
```

Register in commands/mod.rs:
```rust
pub mod diagnostics;
```

Register in lib.rs handler:
```rust
commands::diagnostics::run_integrity_check,
commands::diagnostics::repair_seed_data,
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Wire integrity check + seeder into startup.rs
─────────────────────────────────────────────────────────────────────
Update the startup sequence in startup.rs to run seeder then integrity check after
migrations. The sequence is now:

1. DB init (WAL pragmas)
2. Run migrations
3. Run seed_system_data (idempotent)
4. Run integrity check
5. If NOT is_healthy AND NOT is_recoverable → emit error StartupEvent, halt
6. If NOT is_healthy AND is_recoverable → emit a warning in the ready event payload
7. If is_healthy → emit ready StartupEvent normally

```rust
// In startup.rs, after run_migrations:

// Step 3: Seed system data (idempotent)
app_handle.emit("startup_event", StartupEvent {
    phase: "config_loaded".into(),
    message: Some("Initialisation des données système…".into()),
    version: None,
})?;
crate::db::seeder::seed_system_data(db).await
    .map_err(|e| {
        tracing::error!("startup::seed_failed: {}", e);
        e
    })?;

// Step 4: Integrity check
let report = crate::db::integrity::run_integrity_check(db).await?;
if !report.is_healthy {
    if report.is_recoverable {
        // Log warning but proceed — the frontend will offer repair
        tracing::warn!(
            issues = report.issues.len(),
            "startup::integrity_warning — recoverable issues found, proceeding"
        );
    } else {
        tracing::error!(
            issues = ?report.issues,
            "startup::integrity_fatal — unrecoverable integrity issues"
        );
        app_handle.emit("startup_event", StartupEvent {
            phase: "error".into(),
            message: Some(format!(
                "Erreur d'intégrité : {}",
                report.issues.first().map(|i| i.description.as_str()).unwrap_or("inconnue")
            )),
            version: None,
        })?;
        return Err(crate::errors::AppError::Internal(
            "Startup integrity check failed with unrecoverable issues".into()
        ));
    }
}
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Add integrity check IPC types to shared/ipc-types.ts
─────────────────────────────────────────────────────────────────────
```typescript
// shared/ipc-types.ts — add to existing exports

export interface IntegrityIssue {
  code: string;
  description: string;
  is_auto_repairable: boolean;
  subject: string;
}

export interface IntegrityReport {
  is_healthy: boolean;
  is_recoverable: boolean;
  issues: IntegrityIssue[];
  seed_schema_version: number | null;
  domain_count: number;
  value_count: number;
}
```

─────────────────────────────────────────────────────────────────────
STEP 5 — Add integrity commands to docs/IPC_COMMAND_REGISTRY.md
─────────────────────────────────────────────────────────────────────
```markdown
## run_integrity_check

| Field | Value |
|-------|-------|
| Command | `run_integrity_check` |
| Module | Diagnostics / Startup |
| Auth Required | No |
| Parameters | None |
| Response | `IntegrityReport` |
| Errors | `DATABASE_ERROR` |
| Since | v0.1.0 |
| PRD Ref | §14.2 Reliability and Recovery |

## repair_seed_data

| Field | Value |
|-------|-------|
| Command | `repair_seed_data` |
| Module | Diagnostics / Recovery |
| Auth Required | No (only callable during startup recovery screen) |
| Parameters | None |
| Response | `IntegrityReport` (post-repair) |
| Errors | `DATABASE_ERROR` |
| Since | v0.1.0 |
| PRD Ref | §14.2 |
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures
- pnpm run dev starts cleanly; startup log shows "seeder::complete" then no integrity errors
- Calling `invoke('run_integrity_check')` from the Tauri console returns a report
  where `is_healthy === true` and `issues.length === 0`
- `domain_count` in the report is 18, `value_count` is ≥ 90
- Calling `invoke('repair_seed_data')` is idempotent — returns same healthy report
```

---

### Supervisor Verification — Sprint S2

**V1 — Integrity check returns healthy.**
Run `pnpm run dev`. Open Tauri developer tools console and run:
```javascript
window.__TAURI__.core.invoke('run_integrity_check').then(r => console.log(JSON.stringify(r, null, 2)));
```
The output should show `"is_healthy": true`, `"domain_count": 18`, `"issues": []`.
If `is_healthy` is false, copy the first issue from the `issues` array and flag it.

**V2 — Repair command is idempotent.**
In the same console session, run:
```javascript
window.__TAURI__.core.invoke('repair_seed_data').then(r => console.log('After repair:', r.domain_count, 'domains'));
```
The domain count must still be 18 (not 36). If the count doubled, the INSERT OR IGNORE
logic is broken. Flag it.

**V3 — Startup crashes gracefully on missing table.**
This test is dangerous — do not attempt it unless you have a database backup. Skip to V4
instead.

**V4 — Startup log shows seeder completion.**
In the terminal where `pnpm run dev` is running, look for lines containing
`seeder::complete`. If those lines are absent, the seeder is not being called from the
startup sequence. Flag it.

---

## Sprint S3 — Frontend Recovery Page and Integrity Hook

### AI Agent Prompt

```
You are a senior React and TypeScript engineer continuing work on Maintafox Desktop.
Sprint S2 is complete: the integrity check IPC command is live, returns a structured
report, and is wired into the startup sequence. Your task is to build the frontend
recovery experience: a hook that calls the integrity check, a RecoveryPage component
that activates when issues are found, and the router update that makes the recovery
page reachable.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src/services/diagnostics-service.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/services/diagnostics-service.ts
import { invoke } from "@tauri-apps/api/core";
import type { IntegrityReport } from "@shared/ipc-types";

export async function runIntegrityCheck(): Promise<IntegrityReport> {
  return invoke<IntegrityReport>("run_integrity_check");
}

export async function repairSeedData(): Promise<IntegrityReport> {
  return invoke<IntegrityReport>("repair_seed_data");
}
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src/hooks/use-integrity-check.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/hooks/use-integrity-check.ts
import { useState, useCallback } from "react";
import { runIntegrityCheck, repairSeedData } from "@/services/diagnostics-service";
import type { IntegrityReport } from "@shared/ipc-types";

interface IntegrityState {
  report: IntegrityReport | null;
  isChecking: boolean;
  isRepairing: boolean;
  error: string | null;
}

interface IntegrityActions {
  check: () => Promise<void>;
  repair: () => Promise<void>;
}

export function useIntegrityCheck(): IntegrityState & IntegrityActions {
  const [state, setState] = useState<IntegrityState>({
    report: null,
    isChecking: false,
    isRepairing: false,
    error: null,
  });

  const check = useCallback(async () => {
    setState((s) => ({ ...s, isChecking: true, error: null }));
    try {
      const report = await runIntegrityCheck();
      setState((s) => ({ ...s, report, isChecking: false }));
    } catch (e) {
      setState((s) => ({
        ...s,
        isChecking: false,
        error: e instanceof Error ? e.message : "Erreur lors de la vérification.",
      }));
    }
  }, []);

  const repair = useCallback(async () => {
    setState((s) => ({ ...s, isRepairing: true, error: null }));
    try {
      const report = await repairSeedData();
      setState((s) => ({ ...s, report, isRepairing: false }));
    } catch (e) {
      setState((s) => ({
        ...s,
        isRepairing: false,
        error: e instanceof Error ? e.message : "Erreur lors de la réparation.",
      }));
    }
  }, []);

  return { ...state, check, repair };
}
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Create src/pages/DiagnosticsPage.tsx
─────────────────────────────────────────────────────────────────────
The diagnostics page is accessible from the dashboard to maintenance supervisors.
It shows the integrity report and an on-demand repair button.

```tsx
// src/pages/DiagnosticsPage.tsx
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ShieldCheck, AlertTriangle, RefreshCw, Wrench } from "lucide-react";
import { useIntegrityCheck } from "@/hooks/use-integrity-check";
import { cn } from "@/lib/utils";

export function DiagnosticsPage() {
  const { t } = useTranslation("shell");
  const { report, isChecking, isRepairing, error, check, repair } =
    useIntegrityCheck();

  useEffect(() => {
    void check();
  }, [check]);

  return (
    <div className="p-6 space-y-6 max-w-3xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-text-primary">
          {t("diagnostics.title")}
        </h1>
        <button
          onClick={() => void check()}
          disabled={isChecking}
          className="btn-ghost flex items-center gap-2"
        >
          <RefreshCw className={cn("h-4 w-4", isChecking && "animate-spin-slow")} />
          {t("diagnostics.recheck")}
        </button>
      </div>

      {error && (
        <div className="rounded-lg border border-status-danger/30 bg-status-danger/10 p-4 text-sm text-text-danger">
          {error}
        </div>
      )}

      {report && (
        <>
          {/* Status summary */}
          <div className={cn(
            "rounded-lg border p-4 flex items-center gap-3",
            report.is_healthy
              ? "border-status-success/30 bg-status-success/10"
              : "border-status-danger/30 bg-status-danger/10",
          )}>
            {report.is_healthy ? (
              <ShieldCheck className="h-5 w-5 text-status-success shrink-0" />
            ) : (
              <AlertTriangle className="h-5 w-5 text-status-danger shrink-0" />
            )}
            <div>
              <p className="text-sm font-medium text-text-primary">
                {report.is_healthy
                  ? t("diagnostics.healthy")
                  : t("diagnostics.unhealthy", { count: report.issues.length })}
              </p>
              <p className="text-xs text-text-secondary mt-0.5">
                {t("diagnostics.stats", {
                  domains: report.domain_count,
                  values: report.value_count,
                  version: report.seed_schema_version ?? "—",
                })}
              </p>
            </div>
          </div>

          {/* Issues list */}
          {report.issues.length > 0 && (
            <div className="space-y-2">
              <p className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                {t("diagnostics.issues")}
              </p>
              {report.issues.map((issue, i) => (
                <div
                  key={i}
                  className="rounded-lg border border-surface-border bg-surface-2 p-3"
                >
                  <p className="text-xs font-mono text-status-warning">{issue.code}</p>
                  <p className="text-sm text-text-primary mt-1">{issue.description}</p>
                  <p className="text-xs text-text-muted mt-1">{issue.subject}</p>
                </div>
              ))}

              {report.is_recoverable && (
                <button
                  onClick={() => void repair()}
                  disabled={isRepairing}
                  className="btn-primary flex items-center gap-2 mt-2"
                >
                  <Wrench className={cn("h-4 w-4", isRepairing && "animate-spin-slow")} />
                  {isRepairing
                    ? t("diagnostics.repairing")
                    : t("diagnostics.repair")}
                </button>
              )}
            </div>
          )}
        </>
      )}

      {isChecking && !report && (
        <div className="flex items-center gap-3 text-sm text-text-secondary">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
          {t("diagnostics.checking")}
        </div>
      )}
    </div>
  );
}
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Add diagnostics i18n strings
─────────────────────────────────────────────────────────────────────
Add to src/i18n/fr/shell.json under a "diagnostics" key:
```json
"diagnostics": {
  "title":      "Diagnostics système",
  "recheck":    "Revérifier",
  "healthy":    "Intégrité vérifiée — aucun problème détecté",
  "unhealthy":  "{{count}} problème(s) détecté(s)",
  "stats":      "{{domains}} domaines · {{values}} valeurs · version seed {{version}}",
  "issues":     "Problèmes détectés",
  "repair":     "Réparer automatiquement",
  "repairing":  "Réparation en cours…",
  "checking":   "Vérification en cours…"
}
```

Add to src/i18n/en/shell.json:
```json
"diagnostics": {
  "title":      "System Diagnostics",
  "recheck":    "Re-check",
  "healthy":    "Integrity verified — no issues found",
  "unhealthy":  "{{count}} issue(s) detected",
  "stats":      "{{domains}} domains · {{values}} values · seed version {{version}}",
  "issues":     "Detected Issues",
  "repair":     "Auto-repair",
  "repairing":  "Repairing…",
  "checking":   "Checking…"
}
```

─────────────────────────────────────────────────────────────────────
STEP 5 — Add /diagnostics route to router.tsx and nav registry
─────────────────────────────────────────────────────────────────────
In src/router.tsx, add a lazy import and a route:
```tsx
const DiagnosticsPage = lazy(() =>
  import("@/pages/DiagnosticsPage").then((m) => ({ default: m.DiagnosticsPage }))
);
// In routes array:
{ path: "/diagnostics", element: <AppShell><PageSuspense><DiagnosticsPage /></PageSuspense></AppShell> },
```

Also add a `settings` group nav item for diagnostics in nav-registry.tsx:
```tsx
{ key: "diagnostics", labelKey: "nav.diagnostics", path: "/diagnostics", icon: <ShieldCheck className="h-4 w-4" /> },
```

Add to both i18n shell.json files:
```json
// fr: "diagnostics": "Diagnostics"
// en: "diagnostics": "Diagnostics"
```

─────────────────────────────────────────────────────────────────────
STEP 6 — Add integration tests for use-integrity-check hook
─────────────────────────────────────────────────────────────────────
```typescript
// src/hooks/__tests__/use-integrity-check.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { IntegrityReport } from "@shared/ipc-types";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

import { useIntegrityCheck } from "../use-integrity-check";

const healthyReport: IntegrityReport = {
  is_healthy: true,
  is_recoverable: true,
  issues: [],
  seed_schema_version: 1,
  domain_count: 18,
  value_count: 92,
};

const unhealthyReport: IntegrityReport = {
  is_healthy: false,
  is_recoverable: true,
  issues: [{ code: "MISSING_DOMAIN", description: "Test issue", is_auto_repairable: true, subject: "test.domain" }],
  seed_schema_version: null,
  domain_count: 5,
  value_count: 10,
};

describe("useIntegrityCheck", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("starts with null report and not checking", () => {
    const { result } = renderHook(() => useIntegrityCheck());
    expect(result.current.report).toBeNull();
    expect(result.current.isChecking).toBe(false);
  });

  it("sets isChecking while check is in progress", async () => {
    let resolve!: (value: IntegrityReport) => void;
    mockInvoke.mockReturnValueOnce(new Promise((r) => { resolve = r; }));
    const { result } = renderHook(() => useIntegrityCheck());

    act(() => { void result.current.check(); });
    expect(result.current.isChecking).toBe(true);

    await act(async () => { resolve(healthyReport); });
    expect(result.current.isChecking).toBe(false);
    expect(result.current.report?.is_healthy).toBe(true);
  });

  it("sets report on successful check", async () => {
    mockInvoke.mockResolvedValueOnce(healthyReport);
    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => { await result.current.check(); });
    expect(result.current.report).toEqual(healthyReport);
    expect(result.current.error).toBeNull();
  });

  it("sets error on failed check", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("DB error"));
    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => { await result.current.check(); });
    expect(result.current.error).toContain("DB error");
    expect(result.current.report).toBeNull();
  });

  it("repair calls repair_seed_data and updates report", async () => {
    mockInvoke.mockResolvedValueOnce(healthyReport);
    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => { await result.current.repair(); });
    expect(mockInvoke).toHaveBeenCalledWith("repair_seed_data");
    expect(result.current.report?.is_healthy).toBe(true);
  });

  it("unhealthy report exposes issues correctly", async () => {
    mockInvoke.mockResolvedValueOnce(unhealthyReport);
    const { result } = renderHook(() => useIntegrityCheck());

    await act(async () => { await result.current.check(); });
    expect(result.current.report?.is_healthy).toBe(false);
    expect(result.current.report?.issues).toHaveLength(1);
    expect(result.current.report?.is_recoverable).toBe(true);
  });
});
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- pnpm run test passes with 0 failures (including 6 new hook tests)
- pnpm run typecheck passes with 0 errors
- pnpm run dev: navigating to /diagnostics shows the diagnostics page with a green
  "Intégrité vérifiée" status, domain count of 18, and value count ≥ 90
- The "Diagnostics" item is visible in the sidebar under the Administration group
- Calling repair when healthy changes nothing (idempotent)
```

---

### Supervisor Verification — Sprint S3

**V1 — Diagnostics page is reachable.**
Run `pnpm run dev`. In the sidebar, scroll to the Administration group. You should see
"Diagnostics" as a navigation item. Click it. The main content area should show the
diagnostics page with a green shield icon and text "Intégrité vérifiée — aucun
problème détecté". If the page is blank, shows an error, or is not in the sidebar,
flag it.

**V2 — Domain and value counts are shown.**
On the Diagnostics page, look for a line showing domain and value counts. It should read
something like "18 domaines · 92 valeurs · version seed 1". If the counts are 0 or the
seed version shows "—", the integrity check is not communicating correctly with the
database. Flag it.

**V3 — Re-check button works.**
Click the "Revérifier" button on the diagnostics page. The button should briefly show a
spinner, then the page refreshes with the same healthy status. If clicking the button
produces an error or the page remains in a loading state for more than 5 seconds, flag it.

**V4 — Tests pass.**
Run:
```
pnpm run test
```
All tests should pass. Specifically, look for 6 tests in the output mentioning
`use-integrity-check`. If any of these fail, flag the test name.

---

## Sub-phase 03 Completion Checklist

All four files of Sub-phase 03 are now complete. The supervisor should verify the
following before marking Sub-phase 03 as done and starting Sub-phase 04:

| # | Check | Method |
|---|-------|--------|
| 1 | `cargo test` in src-tauri produces 0 failures | Terminal |
| 2 | `pnpm run test` produces 0 failures | Terminal |
| 3 | `pnpm run typecheck` produces 0 errors | Terminal |
| 4 | `pnpm run lint:check` produces 0 errors | Terminal |
| 5 | 18 rows in `lookup_domains`, ≥ 90 in `lookup_values` | DBeaver |
| 6 | `system_config` has `seed_schema_version = 1` | DBeaver |
| 7 | Diagnostics page shows "Intégrité vérifiée" with domain_count 18 | Browser dev tools |
| 8 | All 6 tables from migration 004 present: org_structure_models through org_entity_bindings | DBeaver |
| 9 | All 5 tables from migration 005 present: equipment_classes through equipment_lifecycle_events | DBeaver |
| 10 | equipment_lifecycle_events has NO deleted_at column | DBeaver |
| 11 | All 4 tables from migration 006 present: skill_categories through team_skill_requirements | DBeaver |
| 12 | All four repository files present in src-tauri/src/repository/ | File explorer |
| 13 | `docs/REPOSITORY_CONTRACTS.md` present with all tables | File explorer |
| 14 | `docs/SEED_DATA_REFERENCE.md` present, lists 18 domains | File explorer |
| 15 | `docs/MIGRATION_GUIDE.md` present | File explorer |
| 16 | `docs/DB_SCHEMA_CONVENTIONS.md` present with migration baseline table | File explorer |
| 17 | Calling `get_lookup_values('equipment.criticality')` returns 4 values | Browser console invoke |
| 18 | Calling `repair_seed_data()` is idempotent (domain count stays 18) | Browser console invoke |

**Only proceed to Sub-phase 04 (Authentication, Session, Trusted Device, and RBAC
plumbing) when all 18 checks above are green.**

---

*End of Phase 1 · Sub-phase 03 · File 04*
*Sub-phase 03 complete. Next: Sub-phase 04 — Authentication, Session, Trusted Device, and RBAC Plumbing*
