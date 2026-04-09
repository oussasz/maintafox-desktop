//! Demo / development seed data.
//!
//! Inserts a complete organisational tree, equipment catalogue, and
//! intervention requests so that the desktop UI is usable out-of-the-box
//! without manual data entry.
//!
//! **Idempotent** — skips if demo data already exists (checks a sentinel row).
//! Safe to call on every dev startup; has no effect in a production database
//! that already has real tenant data.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

/// Entry point — call from a Tauri command or startup hook.
pub async fn seed_demo_data(db: &DatabaseConnection) -> AppResult<()> {
    // Guard: skip if demo data already seeded.
    let sentinel = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE code = 'DEMO-SITE' LIMIT 1".to_string(),
        ))
        .await?;
    if sentinel.is_some() {
        tracing::info!("demo_seeder: demo data already present — skipping");
        return Ok(());
    }

    tracing::info!("demo_seeder: inserting demo data …");
    let now = Utc::now().to_rfc3339();

    // ── 1. Org structure model (required for org_node_types) ──────────
    let model_sync = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_structure_models
            (sync_id, version_number, status, description, activated_at, created_at, updated_at)
          VALUES (?, 1, 'active', 'Modèle démo', ?, ?, ?)",
        [model_sync.clone().into(), now.clone().into(), now.clone().into(), now.clone().into()],
    ))
    .await?;

    let model_id = last_insert_id(db).await?;

    // ── 2. Org node types ─────────────────────────────────────────────
    let types = [
        ("SITE",       "Site industriel", 1, 0, 1),
        ("ZONE",       "Zone de production", 0, 1, 0),
        ("BUILDING",   "Bâtiment",        0, 0, 0),
        ("LINE",       "Ligne de production", 1, 1, 0),
    ];
    let mut type_ids: Vec<i64> = Vec::new();
    for (code, label, can_host, can_own, is_root) in &types {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO org_node_types
                (sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, created_at, updated_at)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                Uuid::new_v4().to_string().into(),
                model_id.into(),
                (*code).into(),
                (*label).into(),
                (*can_host).into(),
                (*can_own).into(),
                (*is_root).into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
        type_ids.push(last_insert_id(db).await?);
    }

    // ── 3. Org nodes (site → zones → buildings → lines) ──────────────
    // Root site
    let site_id = insert_org_node(db, "DEMO-SITE", "Site Casablanca", type_ids[0], None, "/", 0, &now).await?;

    // Zones
    let zone_prod = insert_org_node(db, "ZONE-PROD", "Zone Production", type_ids[1], Some(site_id), &format!("/{site_id}/"), 1, &now).await?;
    let zone_util = insert_org_node(db, "ZONE-UTIL", "Zone Utilités", type_ids[1], Some(site_id), &format!("/{site_id}/"), 1, &now).await?;

    // Buildings
    let bat_a = insert_org_node(db, "BAT-A", "Bâtiment A – Production", type_ids[2], Some(zone_prod), &format!("/{site_id}/{zone_prod}/"), 2, &now).await?;
    let _bat_b = insert_org_node(db, "BAT-B", "Bâtiment B – Conditionnement", type_ids[2], Some(zone_prod), &format!("/{site_id}/{zone_prod}/"), 2, &now).await?;
    let bat_util = insert_org_node(db, "BAT-UTIL", "Bâtiment Utilités", type_ids[2], Some(zone_util), &format!("/{site_id}/{zone_util}/"), 2, &now).await?;

    // Lines
    let line_1 = insert_org_node(db, "LIGNE-01", "Ligne 1 – Embouteillage", type_ids[3], Some(bat_a), &format!("/{site_id}/{zone_prod}/{bat_a}/"), 3, &now).await?;
    let line_2 = insert_org_node(db, "LIGNE-02", "Ligne 2 – Mélange", type_ids[3], Some(bat_a), &format!("/{site_id}/{zone_prod}/{bat_a}/"), 3, &now).await?;
    let _line_util = insert_org_node(db, "LIGNE-UTIL", "Circuit eau glacée", type_ids[3], Some(bat_util), &format!("/{site_id}/{zone_util}/{bat_util}/"), 3, &now).await?;

    // ── 4. Equipment classes ──────────────────────────────────────────
    let cls_pump = insert_eq_class(db, "POMPE", "Pompes", None, "class", &now).await?;
    let cls_motor = insert_eq_class(db, "MOTEUR", "Moteurs", None, "class", &now).await?;
    let cls_conv = insert_eq_class(db, "CONVOYEUR", "Convoyeurs", None, "class", &now).await?;
    let cls_comp = insert_eq_class(db, "COMPRESSEUR", "Compresseurs", None, "class", &now).await?;
    let cls_valve = insert_eq_class(db, "VANNE", "Vannes industrielles", None, "class", &now).await?;

    // Sub-classes (families)
    let _fam_centri = insert_eq_class(db, "POMPE-CENT", "Pompe centrifuge", Some(cls_pump), "family", &now).await?;
    let _fam_gear = insert_eq_class(db, "POMPE-ENG", "Pompe à engrenage", Some(cls_pump), "family", &now).await?;

    // ── 5. Equipment items ────────────────────────────────────────────
    let eq_data: &[(&str, &str, Option<i64>, i64, &str)] = &[
        ("POMPE-001", "Pompe centrifuge alimentation chaudière", Some(cls_pump), line_1, "Grundfos"),
        ("POMPE-002", "Pompe doseuse produit chimique", Some(cls_pump), line_2, "Grundfos"),
        ("MOT-001",   "Moteur principal convoyeur L1", Some(cls_motor), line_1, "ABB"),
        ("MOT-002",   "Moteur agitateur cuve T2", Some(cls_motor), line_2, "Siemens"),
        ("CONV-001",  "Convoyeur à bande entrée ligne 1", Some(cls_conv), line_1, "FlexLink"),
        ("CONV-002",  "Convoyeur à rouleaux sortie L2", Some(cls_conv), line_2, "Hytrol"),
        ("COMP-001",  "Compresseur air comprimé bâtiment A", Some(cls_comp), bat_a, "Atlas Copco"),
        ("COMP-002",  "Compresseur réfrigérant", Some(cls_comp), bat_util, "Bitzer"),
        ("VAN-001",   "Vanne de régulation vapeur", Some(cls_valve), line_1, "Fisher"),
        ("VAN-002",   "Vanne papillon circuit eau", Some(cls_valve), bat_util, "Butterfly"),
        ("POMPE-003", "Pompe circulation eau glacée", Some(cls_pump), bat_util, "Wilo"),
        ("MOT-003",   "Moteur pompe surpression", Some(cls_motor), bat_util, "WEG"),
    ];

    let mut eq_ids: Vec<i64> = Vec::new();
    for (code, name, class_id, node_id, mfr) in eq_data {
        let eid = insert_equipment(db, code, name, *class_id, *node_id, mfr, &now).await?;
        eq_ids.push(eid);
    }

    // ── 6. Get the admin user id (submitter) ──────────────────────────
    let admin_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1".to_string(),
        ))
        .await?;
    let admin_id: i64 = admin_row
        .map(|r| {
            use sea_orm::TryGetable;
            i64::try_get(&r, "", "id").unwrap_or(1)
        })
        .unwrap_or(1);

    // ── 7. Intervention requests ──────────────────────────────────────
    let di_data: &[(&str, &str, usize, i64, &str, &str, &str, bool)] = &[
        // (title, description, eq_index, org_node, urgency, origin, status, safety)
        (
            "Vibration anormale pompe P-001",
            "Vibrations détectées sur palier côté accouplement. Amplitude 8mm/s dépassant le seuil ISO 10816.",
            0, line_1, "high", "operator", "submitted", false,
        ),
        (
            "Fuite huile réducteur moteur M-001",
            "Fuite d'huile visible au niveau du joint SPI côté ventilateur. Tache au sol ~30cm diamètre.",
            2, line_1, "medium", "technician", "pending_review", false,
        ),
        (
            "Bande convoyeur décentrée CONV-001",
            "La bande du convoyeur a tendance à dériver vers la droite. Risque de détérioration bord de bande.",
            4, line_1, "high", "operator", "screened", false,
        ),
        (
            "Alarme surpression compresseur COMP-001",
            "Alarme surpression répétée 3 fois ce matin. Soupape de sécurité s'est déclenchée une fois.",
            6, bat_a, "critical", "operator", "approved_for_planning", true,
        ),
        (
            "Roulement bruyant moteur agitateur M-002",
            "Bruit de roulement type grincement sur moteur agitateur cuve T2. Détecté lors de la ronde hebdomadaire.",
            3, line_2, "medium", "inspection", "submitted", false,
        ),
        (
            "Vanne VAN-001 fuit en position fermée",
            "La vanne de régulation vapeur ne ferme plus complètement. Passage de vapeur détecté en aval.",
            8, line_1, "high", "technician", "pending_review", true,
        ),
        (
            "Pompe doseuse P-002 débit irrégulier",
            "Le débit de la pompe doseuse oscille entre 30% et 80% de la consigne. Impact sur la qualité produit.",
            1, line_2, "critical", "quality", "approved_for_planning", false,
        ),
        (
            "Convoyeur CONV-002 moteur en surcharge",
            "Courant moteur supérieur à 120% nominal depuis hier. Risque de déclenchement thermique.",
            5, line_2, "high", "operator", "submitted", false,
        ),
        (
            "Compresseur COMP-002 niveau huile bas",
            "Niveau huile compresseur réfrigérant sous le minimum. À compléter avant prochaine mise en route.",
            7, bat_util, "low", "inspection", "screened", false,
        ),
        (
            "Pompe eau glacée P-003 cavitation",
            "Bruit de cavitation détecté à la mise en route. Pression d'aspiration insuffisante ?",
            10, bat_util, "medium", "technician", "submitted", false,
        ),
        (
            "Moteur surpression M-003 échauffement",
            "Température carter moteur 95°C relevée par caméra thermique. Normal < 70°C.",
            11, bat_util, "high", "inspection", "pending_review", false,
        ),
        (
            "Remplacement joint pompe P-001 (planifié)",
            "Suite DI vibrations : le joint mécanique doit être remplacé lors de l'arrêt programmé.",
            0, line_1, "medium", "pm", "deferred", false,
        ),
    ];

    for (i, (title, desc, eq_idx, org_id, urgency, origin, status, safety)) in di_data.iter().enumerate() {
        let code = format!("DI-{:04}", i + 1);
        let submitted_offset = format!("2026-03-{:02}T08:00:00+00:00", (i % 28) + 1);
        insert_di(
            db,
            &code,
            title,
            desc,
            eq_ids[*eq_idx],
            *org_id,
            admin_id,
            urgency,
            origin,
            status,
            *safety,
            &submitted_offset,
            &now,
        )
        .await?;
    }

    tracing::info!("demo_seeder: complete — {} org nodes, {} equipment, {} DIs inserted",
        8, eq_data.len(), di_data.len());

    // Seed reference domain governance demo data (separate sentinel)
    seed_reference_demo_data(db).await?;

    // Seed a limited test user for permission testing
    seed_test_viewer_user(db).await?;

    Ok(())
}

// ═════════════════════════════════════════════════════════════════════════════
// Reference domain demo data
// ═════════════════════════════════════════════════════════════════════════════

/// Seeds reference domains, sets, and values for the governance UI demo.
/// Uses its own sentinel so it can run even when org/equipment demo data
/// was already present from a previous startup.
pub async fn seed_reference_demo_data(db: &DatabaseConnection) -> AppResult<()> {
    let sentinel = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM reference_domains WHERE code = 'EQUIPMENT.FAMILY' LIMIT 1".to_string(),
        ))
        .await?;
    if sentinel.is_some() {
        tracing::info!("demo_seeder: reference demo data already present — skipping");
        return Ok(());
    }

    tracing::info!("demo_seeder: inserting reference domain demo data …");
    let now = Utc::now().to_rfc3339();

    // ── Reference domains ─────────────────────────────────────────────
    let domains: &[(&str, &str, &str, &str, bool)] = &[
        ("EQUIPMENT.FAMILY",       "Familles d'équipements",          "hierarchical",       "tenant_managed",       true),
        ("EQUIPMENT.CLASS",        "Classes d'équipements",           "flat",                "tenant_managed",       true),
        ("EQUIPMENT.STATUS",       "Statuts équipement",              "flat",                "system_seeded",        false),
        ("WORK.PRIORITY",          "Priorités ordre de travail",      "flat",                "protected_analytical", false),
        ("WORK.FAILURE_MODES",     "Modes de défaillance",            "hierarchical",        "tenant_managed",       true),
        ("ORG.POSITIONS",          "Postes organisationnels",         "flat",                "tenant_managed",       true),
        ("ORG.SCHEDULES",          "Horaires de travail",             "flat",                "tenant_managed",       true),
        ("PERSONNEL.SKILLS",       "Compétences techniques",          "hierarchical",        "tenant_managed",       true),
        ("PERSONNEL.CERTIFICATIONS", "Certifications et habilitations", "flat",              "tenant_managed",       true),
    ];

    let mut domain_ids: Vec<(i64, &str)> = Vec::new();
    for (code, name, structure, governance, extendable) in domains {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO reference_domains
                (code, name, structure_type, governance_level, is_extendable, created_at, updated_at)
              VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                (*code).into(),
                (*name).into(),
                (*structure).into(),
                (*governance).into(),
                (*extendable as i32).into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
        let id = last_insert_id(db).await?;
        domain_ids.push((id, *code));
    }

    // ── Reference sets (each domain gets one published set + some get a draft) ──
    for (domain_id, code) in &domain_ids {
        // Published set v1
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO reference_sets
                (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at)
              VALUES (?, 1, 'published', ?, 1, ?, ?)",
            [
                (*domain_id).into(),
                now.clone().into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
        let set_id = last_insert_id(db).await?;

        // Seed values for key domains
        match *code {
            "EQUIPMENT.FAMILY" => {
                seed_ref_values(db, set_id, &[
                    ("POMPES",       "Pompes",             None,    1, None),
                    ("MOTEURS",      "Moteurs électriques", None,   2, None),
                    ("CONVOYEURS",   "Convoyeurs",         None,    3, None),
                    ("COMPRESSEURS", "Compresseurs",       None,    4, None),
                    ("VANNES",       "Vannes industrielles", None,  5, None),
                    ("INSTRUMENTS",  "Instrumentation",    None,    6, None),
                ], &now).await?;
                // Add some child values for hierarchical demo
                let parent = get_ref_value_id(db, set_id, "POMPES").await?;
                seed_ref_values(db, set_id, &[
                    ("POMPES.CENTRIFUGES", "Pompes centrifuges",   Some(parent), 1, None),
                    ("POMPES.ENGRENAGES",  "Pompes à engrenages",  Some(parent), 2, None),
                    ("POMPES.DOSEUSES",    "Pompes doseuses",      Some(parent), 3, None),
                ], &now).await?;
            }
            "EQUIPMENT.CLASS" => {
                seed_ref_values(db, set_id, &[
                    ("ROTATING",  "Machines tournantes",  None, 1, None),
                    ("STATIC",    "Équipements statiques", None, 2, None),
                    ("ELECTRICAL", "Équipements électriques", None, 3, None),
                    ("PIPING",    "Tuyauterie et robinetterie", None, 4, None),
                ], &now).await?;
            }
            "EQUIPMENT.STATUS" => {
                seed_ref_values(db, set_id, &[
                    ("IN_SERVICE",     "En service",        None, 1, Some("#198754")),
                    ("OUT_OF_SERVICE", "Hors service",      None, 2, Some("#dc3545")),
                    ("STANDBY",        "En attente",        None, 3, Some("#ffc107")),
                    ("DECOMMISSIONED", "Déclassé",          None, 4, Some("#6c757d")),
                ], &now).await?;
            }
            "WORK.PRIORITY" => {
                seed_ref_values(db, set_id, &[
                    ("P1_CRITICAL",  "P1 – Critique",       None, 1, Some("#dc3545")),
                    ("P2_HIGH",      "P2 – Haute",          None, 2, Some("#fd7e14")),
                    ("P3_MEDIUM",    "P3 – Moyenne",        None, 3, Some("#ffc107")),
                    ("P4_LOW",       "P4 – Basse",          None, 4, Some("#198754")),
                ], &now).await?;
            }
            "WORK.FAILURE_MODES" => {
                seed_ref_values(db, set_id, &[
                    ("MECHANICAL",  "Défaillance mécanique",  None, 1, None),
                    ("ELECTRICAL",  "Défaillance électrique",  None, 2, None),
                    ("PROCESS",     "Défaillance processus",   None, 3, None),
                    ("INSTRUMENT",  "Défaillance instrumentation", None, 4, None),
                ], &now).await?;
                // Hierarchical children
                let mech = get_ref_value_id(db, set_id, "MECHANICAL").await?;
                seed_ref_values(db, set_id, &[
                    ("MECH.BEARING",   "Roulement défectueux",   Some(mech), 1, None),
                    ("MECH.VIBRATION", "Vibration excessive",    Some(mech), 2, None),
                    ("MECH.SEAL",      "Fuite joint mécanique",  Some(mech), 3, None),
                ], &now).await?;
            }
            "PERSONNEL.SKILLS" => {
                seed_ref_values(db, set_id, &[
                    ("MECH_GENERAL",    "Mécanique générale",     None, 1, None),
                    ("ELEC_INDUSTRIAL", "Électricité industrielle", None, 2, None),
                    ("INSTRUMENTATION", "Instrumentation",        None, 3, None),
                    ("WELDING",         "Soudure",                None, 4, None),
                    ("HYDRAULICS",      "Hydraulique",            None, 5, None),
                ], &now).await?;
            }
            "PERSONNEL.CERTIFICATIONS" => {
                seed_ref_values(db, set_id, &[
                    ("ELEC_HAB",     "Habilitation électrique",     None, 1, None),
                    ("HEIGHT_CERT",  "Travail en hauteur",          None, 2, None),
                    ("CONFINED",     "Espace confiné",              None, 3, None),
                    ("HOT_WORK",     "Permis feu / travaux chauds", None, 4, None),
                ], &now).await?;
            }
            _ => {
                // ORG.POSITIONS, ORG.SCHEDULES — minimal seed
                seed_ref_values(db, set_id, &[
                    ("DEFAULT", "Valeur par défaut", None, 1, None),
                ], &now).await?;
            }
        }

        // Add a draft v2 set for EQUIPMENT.FAMILY and WORK.PRIORITY domains
        if *code == "EQUIPMENT.FAMILY" || *code == "WORK.PRIORITY" {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO reference_sets
                    (domain_id, version_no, status, created_by_id, created_at)
                  VALUES (?, 2, 'draft', 1, ?)",
                [(*domain_id).into(), now.clone().into()],
            ))
            .await?;
        }
    }

    tracing::info!("demo_seeder: reference domains seeded — {} domains", domain_ids.len());
    Ok(())
}

/// Seed reference values in bulk.
async fn seed_ref_values(
    db: &DatabaseConnection,
    set_id: i64,
    values: &[(&str, &str, Option<i64>, i32, Option<&str>)],
    now: &str,
) -> AppResult<()> {
    for (code, label, parent_id, sort_order, color) in values {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO reference_values
                (set_id, parent_id, code, label, sort_order, color_hex, is_active, created_at)
              VALUES (?, ?, ?, ?, ?, ?, 1, ?)",
            [
                set_id.into(),
                parent_id.map_or(sea_orm::Value::String(None), |v| v.into()),
                (*code).into(),
                (*label).into(),
                (*sort_order).into(),
                color.map_or(sea_orm::Value::String(None), |c| c.into()),
                now.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// Get the id of a reference value by set_id and code.
async fn get_ref_value_id(db: &DatabaseConnection, set_id: i64, code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_values WHERE set_id = ? AND code = ? LIMIT 1",
            [set_id.into(), code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("ref value not found: {code}")))?;
    use sea_orm::TryGetable;
    let id = i64::try_get(&row, "", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e:?}")))?;
    Ok(id)
}

// ═════════════════════════════════════════════════════════════════════════════
// Test user for permission verification
// ═════════════════════════════════════════════════════════════════════════════

/// Creates a test user with no role assignment (no permissions at all).
/// Used for V4 permission gate verification.
pub async fn seed_test_viewer_user(db: &DatabaseConnection) -> AppResult<()> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = ?",
            ["viewer_noref".into()],
        ))
        .await?;
    if existing.is_some() {
        tracing::info!("demo_seeder: test viewer user already exists — skipping");
        return Ok(());
    }

    let password_hash = crate::auth::password::hash_password("Test#2026!")?;
    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO user_accounts
            (sync_id, username, display_name, identity_mode, password_hash,
             is_active, is_admin, force_password_change, failed_login_attempts,
             created_at, updated_at, row_version)
          VALUES (?, ?, ?, 'local', ?, 1, 0, 0, 0, ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            "viewer_noref".into(),
            "Viewer (sans référence)".into(),
            password_hash.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    tracing::info!("demo_seeder: test user 'viewer_noref' created (no role, no permissions)");
    Ok(())
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("last_insert_rowid failed")))?;
    use sea_orm::TryGetable;
    let id = i64::try_get(&row, "", "id")
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e:?}")))?;
    Ok(id)
}

async fn insert_org_node(
    db: &DatabaseConnection,
    code: &str,
    name: &str,
    type_id: i64,
    parent_id: Option<i64>,
    ancestor_path: &str,
    depth: i32,
    now: &str,
) -> AppResult<i64> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_nodes
            (sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, ?, ?, ?, 'active', ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            code.into(),
            name.into(),
            type_id.into(),
            parent_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            ancestor_path.into(),
            depth.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn insert_eq_class(
    db: &DatabaseConnection,
    code: &str,
    name: &str,
    parent_id: Option<i64>,
    level: &str,
    now: &str,
) -> AppResult<i64> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO equipment_classes
            (sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
        [
            Uuid::new_v4().to_string().into(),
            code.into(),
            name.into(),
            parent_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            level.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn insert_equipment(
    db: &DatabaseConnection,
    asset_code: &str,
    name: &str,
    class_id: Option<i64>,
    node_id: i64,
    manufacturer: &str,
    now: &str,
) -> AppResult<i64> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO equipment
            (sync_id, asset_id_code, name, class_id, lifecycle_status, installed_at_node_id,
             manufacturer, created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, 'active_in_service', ?, ?, ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            asset_code.into(),
            name.into(),
            class_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            node_id.into(),
            manufacturer.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

#[allow(clippy::too_many_arguments)]
async fn insert_di(
    db: &DatabaseConnection,
    code: &str,
    title: &str,
    description: &str,
    asset_id: i64,
    org_node_id: i64,
    submitter_id: i64,
    urgency: &str,
    origin: &str,
    status: &str,
    safety: bool,
    submitted_at: &str,
    now: &str,
) -> AppResult<i64> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO intervention_requests
            (code, asset_id, org_node_id, status, title, description, origin_type,
             impact_level, production_impact, safety_flag, environmental_flag, quality_flag,
             reported_urgency, submitted_at, submitter_id, row_version, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, 'unknown', 0, ?, 0, 0, ?, ?, ?, 1, ?, ?)",
        [
            code.into(),
            asset_id.into(),
            org_node_id.into(),
            status.into(),
            title.into(),
            description.into(),
            origin.into(),
            (safety as i32).into(),
            urgency.into(),
            submitted_at.into(),
            submitter_id.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}
