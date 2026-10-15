//! Migration 198 — one urgency scale for requests and work orders; OT types with an EN 13306
//! maintenance nature, both as SYSTEM reference lists.
//!
//! **Urgency.** Before: OT stored `work_orders.urgency_id` → `urgency_levels` (very_low, low,
//! medium, high, critical) and planning `schedule_candidates.priority_id` → the same table; DI
//! stored the `DI.PRIORITY` code (low, medium, high, critical) as text in `reported_urgency` /
//! `validated_urgency`, and the request SLA rules keyed `di_sla_rules.urgency_level` by it.
//! After: one SYSTEM domain `WORK.URGENCY` (emergency, urgent, high, normal, low;
//! `reference::urgency_scale`). Mapping by code (`urgency_scale::legacy_urgency_code`):
//!
//! | before                                   | after       |
//! |------------------------------------------|-------------|
//! | `critical` (OT « Urgente », DI « Critique ») | `emergency` |
//! | `high`                                   | `high`      |
//! | `medium`                                 | `normal`    |
//! | `low`                                    | `low`       |
//! | `very_low` (OT only)                     | `low`       |
//!
//! An `urgency_levels` row whose code is not one of these maps by its level number
//! (5 → emergency, 4 → high, 3 → normal, 1–2 → low, the same correspondence).
//! - `work_orders.urgency_id` and `schedule_candidates.priority_id` now hold the
//!   `reference_values.id` of the level (ids of the OT rows are unchanged). The declared foreign
//!   key of `schedule_candidates.priority_id` is retargeted to `reference_values(id)` (stored
//!   definition, as migration 164 did). An id that pointed at no `urgency_levels` row (it could not
//!   be shown) becomes NULL.
//! - DI codes and SLA rule codes are rewritten with the table above.
//! - The request SLA rules that applied to every origin and every criticality held the per-level
//!   target times: they are copied onto the levels (`targets_source = "di_sla_rule"`) and
//!   deactivated, so the scale is the single place of the per-level targets and the remaining
//!   rules are the specific ones (origin and/or criticality).
//! - `DI.PRIORITY` (nothing links to it by id) and `urgency_levels` are removed.
//!
//! **OT types.** Before: module table `work_order_types` (7 shipped codes, plus any type a plant
//! created). After: SYSTEM domains `WORK.MAINTENANCE_NATURES` and `WORK.ORDER_TYPES`
//! (`wo::types`). Shipped codes keep their code; a plant's own type is carried over with its
//! code, label and active flag, without a nature (`metadata_json.nature_review_required`), since
//! no rule can guess it. `work_orders.type_id` is remapped by code; a work order whose type id
//! matches no `work_order_types` row stops the migration (nothing can be guessed for it).
//! `v_ram_data_quality_issues` now selects corrective work by nature. `work_order_types` is
//! removed.
//!
//! **Owner decisions of 2026-10-15** (also in this migration):
//! - OT sub-types: COMPANY list `WORK.ORDER_SUBTYPES` (each value under one nature, see
//!   `wo::types`) and the new nullable `work_orders.subtype_id` (references `reference_values(id)`).
//!   Rules keep reading the nature of the OT type.
//! - Request types `DI.REQUEST_TYPE` become a COMPANY list extended through draft → publish
//!   (category C, extendable). The shipped request types that name a kind of work carry the
//!   nature of the OT they become as semantic tag (`wo::types::DI_REQUEST_TYPE_NATURES`), so the
//!   conversion reads reference data instead of request-type codes.
//!
//! The tables touched are synchronised; every device runs this migration on its own copy, and
//! the new values carry seeded identities (`domain:code`), so the links agree across devices.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::reference::urgency_scale::{
    self, legacy_urgency_code, LevelTargets, UrgencyCode, TARGETS_SOURCE_DI_SLA_RULE,
};
use crate::wo::types::{self as wo_types, MaintenanceNature, DI_REQUEST_TYPE_NATURES, WORK_ORDER_TYPES_DOMAIN};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000198_shared_urgency_and_wo_natures"
    }
}

fn db_err(e: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(e.to_string())
}

async fn table_exists<C: ConnectionTrait>(db: &C, table: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ))
        .await?
        .is_some())
}

/// Codes stored before this migration, with their level on the shared scale.
const LEGACY_URGENCY_CODES: &[&str] = &["critical", "high", "medium", "low", "very_low"];

/// `CASE` rewriting a column of legacy urgency codes to the shared scale (unknown codes kept).
fn urgency_case(column: &str) -> String {
    let mut sql = format!("CASE LOWER(TRIM({column}))");
    for old in LEGACY_URGENCY_CODES {
        let new = legacy_urgency_code(old).expect("legacy code maps");
        sql.push_str(&format!(" WHEN '{old}' THEN '{}'", new.as_str()));
    }
    sql.push_str(&format!(" ELSE {column} END"));
    sql
}

fn level_from_number(level: i64) -> UrgencyCode {
    match level {
        i64::MIN..=2 => UrgencyCode::Low,
        3 => UrgencyCode::Normal,
        4 => UrgencyCode::High,
        _ => UrgencyCode::Emergency,
    }
}

/// Retargets the declared foreign key of `schedule_candidates.priority_id` from
/// `urgency_levels(id)` to `reference_values(id)` in the stored table definition.
async fn retarget_schedule_candidates_fk<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    if !table_exists(db, "schedule_candidates").await? {
        return Ok(());
    }
    let version: i64 = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA schema_version".to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("schema_version unavailable".into()))?
        .try_get_by_index(0)?;
    db.execute_unprepared("PRAGMA writable_schema = ON").await?;
    db.execute_unprepared(
        "UPDATE sqlite_master \
            SET sql = replace(replace(sql, 'REFERENCES urgency_levels(id)', 'REFERENCES reference_values(id)'), \
                              'REFERENCES \"urgency_levels\"(id)', 'REFERENCES reference_values(id)') \
          WHERE type = 'table' AND name = 'schedule_candidates'",
    )
    .await?;
    db.execute_unprepared(&format!("PRAGMA schema_version = {}", version + 1))
        .await?;
    db.execute_unprepared("PRAGMA writable_schema = OFF").await?;

    let still_legacy = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA foreign_key_list('schedule_candidates')".to_string(),
        ))
        .await?
        .iter()
        .any(|r| r.try_get::<String>("", "table").is_ok_and(|t| t == "urgency_levels"));
    if still_legacy {
        return Err(DbErr::Custom(
            "schedule_candidates.priority_id: the foreign key to urgency_levels could not be retargeted".into(),
        ));
    }
    Ok(())
}

async fn migrate_urgency<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    urgency_scale::ensure_urgency_scale(db).await.map_err(db_err)?;
    retarget_schedule_candidates_fk(db).await?;

    db.execute_unprepared(
        "CREATE TEMP TABLE IF NOT EXISTS m198_urgency_map (old_id INTEGER PRIMARY KEY, new_id INTEGER NOT NULL)",
    )
    .await?;
    db.execute_unprepared("DELETE FROM m198_urgency_map").await?;

    if table_exists(db, "urgency_levels").await? {
        let rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id, code, level FROM urgency_levels".to_string(),
            ))
            .await?;
        for r in rows {
            let old_id: i64 = r.try_get("", "id")?;
            let code: Option<String> = r.try_get("", "code")?;
            let level: i64 = r.try_get("", "level")?;
            let new_code = code
                .as_deref()
                .and_then(legacy_urgency_code)
                .unwrap_or_else(|| level_from_number(level));
            let new_id = urgency_scale::urgency_value_id(db, new_code.as_str())
                .await
                .map_err(db_err)?;
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT OR REPLACE INTO m198_urgency_map (old_id, new_id) VALUES (?, ?)",
                [old_id.into(), new_id.into()],
            ))
            .await?;
        }
    }

    db.execute_unprepared(
        "UPDATE work_orders \
            SET urgency_id = (SELECT m.new_id FROM m198_urgency_map m WHERE m.old_id = work_orders.urgency_id) \
          WHERE urgency_id IS NOT NULL",
    )
    .await?;
    if table_exists(db, "schedule_candidates").await? {
        db.execute_unprepared(
            "UPDATE schedule_candidates \
                SET priority_id = (SELECT m.new_id FROM m198_urgency_map m WHERE m.old_id = schedule_candidates.priority_id) \
              WHERE priority_id IS NOT NULL",
        )
        .await?;
    }
    db.execute_unprepared("DROP TABLE IF EXISTS m198_urgency_map").await?;

    // DI codes and SLA rule codes.
    db.execute_unprepared(&format!(
        "UPDATE intervention_requests SET reported_urgency = {}",
        urgency_case("reported_urgency")
    ))
    .await?;
    db.execute_unprepared(&format!(
        "UPDATE intervention_requests SET validated_urgency = {} WHERE validated_urgency IS NOT NULL",
        urgency_case("validated_urgency")
    ))
    .await?;
    db.execute_unprepared(&format!(
        "UPDATE di_sla_rules SET urgency_level = {}",
        urgency_case("urgency_level")
    ))
    .await?;

    // Per-level targets: the rules covering every origin and every criticality move to the scale.
    let broad = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, urgency_level, target_response_hours, target_resolution_hours, escalation_threshold_hours \
               FROM di_sla_rules \
              WHERE is_active = 1 AND origin_type IS NULL AND asset_criticality_class IS NULL \
              ORDER BY id ASC"
                .to_string(),
        ))
        .await?;
    let mut done: Vec<UrgencyCode> = Vec::new();
    for r in broad {
        let id: i64 = r.try_get("", "id")?;
        let code: String = r.try_get("", "urgency_level")?;
        let Some(level) = UrgencyCode::parse(&code) else {
            continue;
        };
        if !done.contains(&level) {
            let targets = LevelTargets {
                response_hours: r.try_get("", "target_response_hours")?,
                completion_hours: r.try_get("", "target_resolution_hours")?,
                escalation_hours: r.try_get("", "escalation_threshold_hours")?,
            };
            urgency_scale::set_level_targets_from_migration(db, level, targets, TARGETS_SOURCE_DI_SLA_RULE)
                .await
                .map_err(db_err)?;
            done.push(level);
        }
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE di_sla_rules SET is_active = 0 WHERE id = ?",
            [id.into()],
        ))
        .await?;
    }

    crate::reference::retirement::delete_domain(db, "DI.PRIORITY")
        .await
        .map_err(db_err)?;
    db.execute_unprepared("DROP TABLE IF EXISTS urgency_levels").await?;
    Ok(())
}

async fn migrate_work_order_types<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    wo_types::ensure_work_order_type_catalog(db).await.map_err(db_err)?;
    if !table_exists(db, "work_order_types").await? {
        return Ok(());
    }

    db.execute_unprepared(
        "CREATE TEMP TABLE IF NOT EXISTS m198_type_map (old_id INTEGER PRIMARY KEY, new_id INTEGER NOT NULL)",
    )
    .await?;
    db.execute_unprepared("DELETE FROM m198_type_map").await?;

    let shipped: Vec<&str> = wo_types::shipped_type_codes().collect();
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, code, label, is_active FROM work_order_types ORDER BY id".to_string(),
        ))
        .await?;
    for (i, r) in rows.iter().enumerate() {
        let old_id: i64 = r.try_get("", "id")?;
        let code: String = r.try_get::<String>("", "code")?.trim().to_ascii_lowercase();
        let label: String = r.try_get("", "label")?;
        let is_active: Option<i64> = r.try_get("", "is_active")?;
        if !shipped.contains(&code.as_str()) {
            // A plant's own type: carried over without a nature, flagged for review.
            let meta = serde_json::json!({
                "origin": "migrated",
                "label_en": label,
                "nature_review_required": true,
            });
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT OR IGNORE INTO reference_values \
                 (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
                  semantic_tag, external_code, is_active, metadata_json) \
                 SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, NULL, NULL, ?, ? \
                   FROM reference_domains d \
                   JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
                  WHERE UPPER(TRIM(d.code)) = ? \
                  ORDER BY rs.version_no DESC LIMIT 1",
                [
                    crate::sync::identity::seeded_entity_sync_id(
                        "reference_values",
                        &format!("{WORK_ORDER_TYPES_DOMAIN}:{code}"),
                    )
                    .into(),
                    code.clone().into(),
                    label.into(),
                    (100 + i as i64).into(),
                    is_active.unwrap_or(1).into(),
                    meta.to_string().into(),
                    WORK_ORDER_TYPES_DOMAIN.into(),
                ],
            ))
            .await?;
        }
        let new_id: i64 = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT rv.id FROM reference_values rv \
                   JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                   JOIN reference_domains d ON d.id = rs.domain_id \
                  WHERE UPPER(TRIM(d.code)) = ? AND LOWER(TRIM(rv.code)) = ?",
                [WORK_ORDER_TYPES_DOMAIN.into(), code.clone().into()],
            ))
            .await?
            .ok_or_else(|| DbErr::Custom(format!("WORK.ORDER_TYPES value '{code}' missing after seeding")))?
            .try_get("", "id")?;
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR REPLACE INTO m198_type_map (old_id, new_id) VALUES (?, ?)",
            [old_id.into(), new_id.into()],
        ))
        .await?;
    }

    let orphans = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT code FROM work_orders WHERE type_id NOT IN (SELECT old_id FROM m198_type_map) ORDER BY id LIMIT 20"
                .to_string(),
        ))
        .await?;
    if !orphans.is_empty() {
        let codes: Vec<String> = orphans
            .iter()
            .filter_map(|r| r.try_get::<String>("", "code").ok())
            .collect();
        return Err(DbErr::Custom(format!(
            "work orders whose type matches no work_order_types row (type cannot be migrated): {}",
            codes.join(", ")
        )));
    }

    db.execute_unprepared(
        "UPDATE work_orders SET type_id = (SELECT m.new_id FROM m198_type_map m WHERE m.old_id = work_orders.type_id)",
    )
    .await?;
    db.execute_unprepared("DROP TABLE IF EXISTS m198_type_map").await?;
    Ok(())
}

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// OT sub-types (column) and request types as a company list tagged with the OT nature.
async fn owner_decisions_2026_10_15<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    if !column_exists(db, "work_orders", "subtype_id").await? {
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN subtype_id INTEGER NULL REFERENCES reference_values(id)",
        )
        .await?;
    }
    db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_wo_subtype ON work_orders(subtype_id)")
        .await?;

    db.execute_unprepared(
        "UPDATE reference_domains SET is_extendable = 1, governance_category = 'controlled_catalog' \
          WHERE UPPER(TRIM(code)) = 'DI.REQUEST_TYPE'",
    )
    .await?;
    for (code, nature) in DI_REQUEST_TYPE_NATURES {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE reference_values SET semantic_tag = ? \
              WHERE LOWER(TRIM(code)) = ? AND (semantic_tag IS NULL OR semantic_tag = 'di_request_type') \
                AND set_id IN (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
                                WHERE UPPER(TRIM(d.code)) = 'DI.REQUEST_TYPE')",
            [nature.as_str().into(), (*code).into()],
        ))
        .await?;
    }
    Ok(())
}

/// `v_ram_data_quality_issues` of migration 084, selecting corrective work by nature.
async fn recreate_ram_quality_view<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let corrective = wo_types::nature_sql_list(&MaintenanceNature::CORRECTIVE);
    db.execute_unprepared("DROP VIEW IF EXISTS v_ram_data_quality_issues")
        .await?;
    db.execute_unprepared(&format!(
        "CREATE VIEW v_ram_data_quality_issues AS
         SELECT DISTINCT wo.equipment_id AS equipment_id,
                'MISSING_FAILURE_MODE' AS issue_code,
                'blocking' AS severity,
                'maintafox://ram/equipment/' || wo.equipment_id || '/failure-mode' AS remediation_url
         FROM work_orders wo
         INNER JOIN work_order_failure_details wfd ON wfd.work_order_id = wo.id
         INNER JOIN reference_values wot ON wot.id = wo.type_id
         WHERE wo.equipment_id IS NOT NULL
           AND wo.closed_at IS NOT NULL
           AND wfd.failure_mode_id IS NULL
           AND wot.semantic_tag IN ({corrective})

         UNION ALL

         SELECT e.id AS equipment_id,
                'MISSING_EXPOSURE' AS issue_code,
                'blocking' AS severity,
                'maintafox://ram/equipment/' || e.id || '/exposure' AS remediation_url
         FROM equipment e
         WHERE NOT EXISTS (
           SELECT 1 FROM runtime_exposure_logs rel
           WHERE rel.equipment_id = e.id
             AND rel.recorded_at >= datetime('now', '-90 days')
         )

         UNION ALL

         SELECT DISTINCT k.equipment_id AS equipment_id,
                'LOW_SAMPLE' AS issue_code,
                'warning' AS severity,
                'maintafox://ram/equipment/' || k.equipment_id || '/kpi' AS remediation_url
         FROM reliability_kpi_snapshots k
         WHERE k.equipment_id IS NOT NULL
           AND (k.data_quality_score < 0.85 OR k.event_count < 5)

         UNION ALL

         SELECT DISTINCT s.equipment_id AS equipment_id,
                'MISSING_INSPECTION_COVERAGE' AS issue_code,
                'warning' AS severity,
                'maintafox://ram/equipment/' || s.equipment_id || '/inspection' AS remediation_url
         FROM inspection_reliability_signals s
         WHERE s.checkpoint_coverage_ratio < 0.85"
    ))
    .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        migrate_urgency(db).await?;
        migrate_work_order_types(db).await?;
        recreate_ram_quality_view(db).await?;
        db.execute_unprepared("DROP TABLE IF EXISTS work_order_types").await?;
        owner_decisions_2026_10_15(db).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Data remap and table removal: not reversible.
        Ok(())
    }
}
