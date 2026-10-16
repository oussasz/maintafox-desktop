//! Planning windows: when and where work may (not) be scheduled (SAP PM operating windows;
//! Maximo work windows / shutdowns).
//!
//! - A window belongs to a site of the organisation (`entity_id`, an org node) and optionally to one
//!   equipment (`equipment_id`, migration 211). A window without a site is plant-wide (the schedule
//!   freeze). A window applies to a job when its site is the job's location or one of the
//!   location's parents, and — when it names an equipment — when the job is on that equipment.
//! - Window types are the SYSTEM list `PLANNING.WINDOW_TYPES` (fixed codes, translated labels):
//!   `production` (the plant runs), `maintenance` (the equipment is released to maintenance),
//!   `shutdown` (planned stop) and `freeze` (the schedule is frozen). The types whose semantic tag is
//!   [`TAG_SHUTDOWN_WORK`] (`maintenance`, `shutdown`) can host work that needs the equipment
//!   stopped.
//! - A locked window refuses commitments on the jobs it applies to; a job that needs a shutdown is
//!   refused unless its slot lies inside an open window of a shutdown-work type that applies to it.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};
use crate::reference::audience::DomainAudience;

pub const WINDOW_TYPES_DOMAIN: &str = "PLANNING.WINDOW_TYPES";
/// Semantic tag of the window types that can host work needing the equipment stopped.
pub const TAG_SHUTDOWN_WORK: &str = "planning_window_shutdown_work";
/// Semantic tag of the other window types.
pub const TAG_WINDOW: &str = "planning_window";

/// (code, French label, English label, semantic tag)
const WINDOW_TYPES: &[(&str, &str, &str, &str)] = &[
    ("production", "Production", "Production", TAG_WINDOW),
    (
        "maintenance",
        "Fenêtre de maintenance",
        "Maintenance window",
        TAG_SHUTDOWN_WORK,
    ),
    ("shutdown", "Arrêt programmé", "Planned shutdown", TAG_SHUTDOWN_WORK),
    ("freeze", "Gel du planning", "Schedule freeze", TAG_WINDOW),
];

/// Ensures the SYSTEM list of window types (migration 211 and the startup catalog integrity pass).
pub async fn ensure_window_type_catalog(db: &impl ConnectionTrait) -> AppResult<()> {
    crate::reference::system_catalog_integrity::ensure_domain(
        db,
        WINDOW_TYPES_DOMAIN,
        "Types de fenêtres de planification",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    crate::reference::system_catalog_integrity::ensure_published_set(db, WINDOW_TYPES_DOMAIN).await?;
    for (i, (code, label, label_en, tag)) in WINDOW_TYPES.iter().enumerate() {
        let sync_id =
            crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{WINDOW_TYPES_DOMAIN}:{code}"));
        let meta = serde_json::json!({ "origin": "system", "label_en": label_en }).to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values \
             (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
              semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, ?, ?, NULL, NULL, ?, NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv JOIN reference_sets s2 ON s2.id = rv.set_id \
                                 WHERE s2.domain_id = d.id AND rv.code = ?) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [
                sync_id.into(),
                (*code).into(),
                (*label).into(),
                (*label_en).into(),
                ((i as i64) + 1).into(),
                (*tag).into(),
                meta.into(),
                WINDOW_TYPES_DOMAIN.into(),
                (*code).into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// The active window types and their semantic tag.
pub async fn window_types(db: &impl ConnectionTrait) -> AppResult<HashMap<String, String>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, COALESCE(rv.semantic_tag, '') AS tag FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1",
            [WINDOW_TYPES_DOMAIN.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String>("", "code").map_err(|e| decode_err("code", e))?,
                r.try_get::<String>("", "tag").map_err(|e| decode_err("tag", e))?,
            ))
        })
        .collect()
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("planning window decode '{field}': {err}"))
}

/// A window's type is one of the active values of `PLANNING.WINDOW_TYPES`.
pub async fn validate_window_type(db: &impl ConnectionTrait, code: &str) -> AppResult<()> {
    if window_types(db).await?.contains_key(code.trim()) {
        return Ok(());
    }
    Err(org_validation_failed(
        "PLANNING_WINDOW_TYPE_UNKNOWN",
        format!("'{code}' is not a planning window type (Données de référence, {WINDOW_TYPES_DOMAIN})."),
        HashMap::from([("code".to_string(), code.to_string())]),
    ))
}

/// A window's site is an org node and its equipment, when given, exists and is installed on that
/// site or one of its sub-nodes. Returns the site: the equipment's location when only the equipment
/// is given.
pub async fn validate_window_scope(
    db: &impl ConnectionTrait,
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
) -> AppResult<Option<i64>> {
    if let Some(node) = entity_id {
        let found = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
                [node.into()],
            ))
            .await?;
        if found.is_none() {
            return Err(org_validation_failed(
                "PLANNING_WINDOW_SITE_UNKNOWN",
                "The window's site is not an organisation node.",
                HashMap::from([("id".to_string(), node.to_string())]),
            ));
        }
    }
    let Some(equipment) = equipment_id else {
        return Ok(entity_id);
    };
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [equipment.into()],
        ))
        .await?
    else {
        return Err(org_validation_failed(
            "PLANNING_WINDOW_EQUIPMENT_UNKNOWN",
            "The window's equipment does not exist.",
            HashMap::from([("id".to_string(), equipment.to_string())]),
        ));
    };
    let installed_at: Option<i64> = row
        .try_get("", "installed_at_node_id")
        .map_err(|e| decode_err("installed_at_node_id", e))?;
    let Some(site) = entity_id else {
        return Ok(installed_at);
    };
    // The equipment is on the site when the site is its location or one of the location's parents.
    let on_site = match installed_at {
        Some(node) => db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "WITH RECURSIVE anc(id) AS ( \
                     SELECT ? \
                     UNION ALL \
                     SELECT n.parent_id FROM org_nodes n JOIN anc ON n.id = anc.id WHERE n.parent_id IS NOT NULL) \
                 SELECT 1 AS ok FROM anc WHERE id = ? LIMIT 1",
                [node.into(), site.into()],
            ))
            .await?
            .is_some(),
        None => false,
    };
    if !on_site {
        return Err(org_validation_failed(
            "PLANNING_WINDOW_EQUIPMENT_NOT_ON_SITE",
            "The window's equipment is not installed on its site.",
            HashMap::from([
                ("equipment_id".to_string(), equipment.to_string()),
                ("site_id".to_string(), site.to_string()),
            ]),
        ));
    }
    Ok(Some(site))
}

/// Where a job is: its equipment and its organisation node (the equipment's location first).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobLocation {
    pub equipment_id: Option<i64>,
    pub node_id: Option<i64>,
}

/// The location of a planning candidate's source (work order, PM occurrence or request). A PM plan
/// scoped to a family or a criticality group has no single location: only plant-wide windows apply.
pub async fn job_location(db: &impl ConnectionTrait, source_type: &str, source_id: i64) -> AppResult<JobLocation> {
    let sql = match source_type {
        "work_order" => {
            "SELECT wo.equipment_id AS equipment_id, \
                    COALESCE(e.installed_at_node_id, wo.entity_id) AS node_id \
               FROM work_orders wo LEFT JOIN equipment e ON e.id = wo.equipment_id WHERE wo.id = ?"
        }
        "pm_occurrence" => {
            "SELECT COALESCE(po.equipment_id, CASE WHEN p.asset_scope_type = 'equipment' THEN p.asset_scope_id END) \
                        AS equipment_id, \
                    COALESCE(e.installed_at_node_id, \
                             CASE WHEN p.asset_scope_type = 'location' THEN p.asset_scope_id END) AS node_id \
               FROM pm_occurrences po JOIN pm_plans p ON p.id = po.pm_plan_id \
               LEFT JOIN equipment e ON e.id = COALESCE(po.equipment_id, \
                    CASE WHEN p.asset_scope_type = 'equipment' THEN p.asset_scope_id END) \
              WHERE po.id = ?"
        }
        "inspection_follow_up" => {
            "SELECT ir.equipment_id AS equipment_id, COALESCE(e.installed_at_node_id, ir.org_node_id) AS node_id \
               FROM intervention_requests ir LEFT JOIN equipment e ON e.id = ir.equipment_id WHERE ir.id = ?"
        }
        _ => return Ok(JobLocation::default()),
    };
    match db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [source_id.into()],
        ))
        .await?
    {
        Some(row) => Ok(JobLocation {
            equipment_id: row
                .try_get("", "equipment_id")
                .map_err(|e| decode_err("equipment_id", e))?,
            node_id: row.try_get("", "node_id").map_err(|e| decode_err("node_id", e))?,
        }),
        None => Ok(JobLocation::default()),
    }
}

/// SQL predicate (alias `pw`): the window applies to a job at `loc` — plant-wide, or on the job's
/// node or one of its parents, and (when it names one) on the job's equipment. Binds: node, equipment.
const APPLIES_SQL: &str = "(pw.entity_id IS NULL OR pw.entity_id IN ( \
        WITH RECURSIVE anc(id) AS ( \
            SELECT ? \
            UNION ALL \
            SELECT n.parent_id FROM org_nodes n JOIN anc ON n.id = anc.id WHERE n.parent_id IS NOT NULL) \
        SELECT id FROM anc WHERE id IS NOT NULL)) \
    AND (pw.equipment_id IS NULL OR pw.equipment_id = ?)";

/// Columns of a window (alias `pw`), with the names of its site and equipment.
pub const WINDOW_COLS: &str = "pw.id,pw.entity_id,pw.equipment_id,pw.window_type,pw.start_datetime,pw.end_datetime,\
pw.is_locked,pw.lock_reason,pw.scope_review_required,pw.row_version,pw.created_at,pw.updated_at,\
(SELECT n.name FROM org_nodes n WHERE n.id = pw.entity_id) AS entity_name,\
(SELECT e.asset_id_code FROM equipment e WHERE e.id = pw.equipment_id) AS equipment_code,\
(SELECT e.name FROM equipment e WHERE e.id = pw.equipment_id) AS equipment_name";

/// The first locked window that overlaps `[start, end)` and applies to the job.
pub async fn find_locked_window_for(
    db: &impl ConnectionTrait,
    start: &str,
    end: &str,
    loc: JobLocation,
) -> AppResult<Option<sea_orm::QueryResult>> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "SELECT {WINDOW_COLS} FROM planning_windows pw \
              WHERE pw.is_locked = 1 AND julianday(pw.start_datetime) < julianday(?) \
                AND julianday(pw.end_datetime) > julianday(?) AND {APPLIES_SQL} \
              ORDER BY pw.start_datetime ASC LIMIT 1"
        ),
        [end.into(), start.into(), loc.node_id.into(), loc.equipment_id.into()],
    ))
    .await
    .map_err(Into::into)
}

/// Whether an open window of a shutdown-work type that applies to the job overlaps its due window
/// (any such window when the job has no due window): the backlog's hint before a slot is chosen.
pub async fn shutdown_window_overlaps(
    db: &impl ConnectionTrait,
    start: Option<&str>,
    end: Option<&str>,
    loc: JobLocation,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS ok FROM planning_windows pw \
                  WHERE pw.is_locked = 0 \
                    AND (? IS NULL OR julianday(pw.end_datetime) > julianday(?)) \
                    AND (? IS NULL OR julianday(pw.start_datetime) < julianday(?)) \
                    AND pw.window_type IN ( \
                        SELECT rv.code FROM reference_values rv \
                          JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                          JOIN reference_domains d ON d.id = rs.domain_id \
                         WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1 AND rv.semantic_tag = ?) \
                    AND {APPLIES_SQL} LIMIT 1"
            ),
            [
                start.into(),
                start.into(),
                end.into(),
                end.into(),
                WINDOW_TYPES_DOMAIN.into(),
                TAG_SHUTDOWN_WORK.into(),
                loc.node_id.into(),
                loc.equipment_id.into(),
            ],
        ))
        .await?;
    Ok(row.is_some())
}

/// Whether an open window of a shutdown-work type contains `[start, end]` and applies to the job.
pub async fn has_shutdown_window_for(
    db: &impl ConnectionTrait,
    start: &str,
    end: &str,
    loc: JobLocation,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS ok FROM planning_windows pw \
                  WHERE pw.is_locked = 0 AND julianday(pw.start_datetime) <= julianday(?) \
                    AND julianday(pw.end_datetime) >= julianday(?) \
                    AND pw.window_type IN ( \
                        SELECT rv.code FROM reference_values rv \
                          JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                          JOIN reference_domains d ON d.id = rs.domain_id \
                         WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1 AND rv.semantic_tag = ?) \
                    AND {APPLIES_SQL} LIMIT 1"
            ),
            [
                start.into(),
                end.into(),
                WINDOW_TYPES_DOMAIN.into(),
                TAG_SHUTDOWN_WORK.into(),
                loc.node_id.into(),
                loc.equipment_id.into(),
            ],
        ))
        .await?;
    Ok(row.is_some())
}
