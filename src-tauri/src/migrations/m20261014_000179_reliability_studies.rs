//! Migration 179 — reliability studies (RAMS module redesign, part A).
//!
//! A study groups equipment of the same kind over a period and one failure mode, so their
//! failure and suspension records can be reviewed, fitted (Weibull) and turned into decisions.
//!
//! - `reliability_studies`: scope (period, failure mode, time basis), the cost inputs of the
//!   age-replacement model and the PM plan it is compared with.
//! - `reliability_study_assets`: equipment in the study.
//! - `reliability_study_record_events`: append-only history of the analyst's choices on the
//!   records (exclude / include, failure instant taken from the request), each with its reason.
//! - `reliability_study_decisions`: decision log (proposed → approved → applied) with the evidence
//!   snapshot and the PM plan version or work order it created.
//! - `reliability_cost_profiles`: planned-replacement and breakdown cost per equipment class,
//!   the defaults a new study starts from.
//!
//! Additive only. All five tables are tenant-synchronised (generic capture, `sync::row_spec`).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000179_reliability_studies"
    }
}

const STATEMENTS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS reliability_studies (
        id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        entity_sync_id TEXT NOT NULL UNIQUE,
        title TEXT NOT NULL,
        period_start TEXT NOT NULL,
        period_end TEXT NOT NULL,
        failure_mode_id INTEGER NULL REFERENCES reference_values(id),
        time_basis TEXT NOT NULL DEFAULT 'auto',
        planned_cost REAL NULL,
        failure_cost REAL NULL,
        cost_currency_code TEXT NULL,
        pm_plan_id INTEGER NULL REFERENCES pm_plans(id),
        status TEXT NOT NULL DEFAULT 'active',
        created_by_id INTEGER NULL,
        row_version INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_reliability_studies_status ON reliability_studies(status, updated_at)",
    "CREATE TABLE IF NOT EXISTS reliability_study_assets (
        id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        entity_sync_id TEXT NOT NULL UNIQUE,
        study_id INTEGER NOT NULL REFERENCES reliability_studies(id),
        equipment_id INTEGER NOT NULL REFERENCES equipment(id),
        row_version INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE (study_id, equipment_id)
    )",
    "CREATE INDEX IF NOT EXISTS idx_reliability_study_assets_equipment ON reliability_study_assets(equipment_id)",
    "CREATE TABLE IF NOT EXISTS reliability_study_record_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        entity_sync_id TEXT NOT NULL UNIQUE,
        study_id INTEGER NOT NULL REFERENCES reliability_studies(id),
        record_kind TEXT NOT NULL,
        failure_event_id INTEGER NULL REFERENCES failure_events(id),
        work_order_id INTEGER NULL REFERENCES work_orders(id),
        equipment_id INTEGER NULL REFERENCES equipment(id),
        action TEXT NOT NULL,
        reason TEXT NOT NULL DEFAULT '',
        actor_id INTEGER NULL,
        row_version INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_reliability_study_record_events_study ON reliability_study_record_events(study_id, created_at)",
    "CREATE TABLE IF NOT EXISTS reliability_study_decisions (
        id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        entity_sync_id TEXT NOT NULL UNIQUE,
        study_id INTEGER NOT NULL REFERENCES reliability_studies(id),
        title TEXT NOT NULL,
        action_kind TEXT NOT NULL,
        equipment_id INTEGER NULL REFERENCES equipment(id),
        pm_plan_id INTEGER NULL REFERENCES pm_plans(id),
        proposed_interval_hours REAL NULL,
        proposed_trigger_definition_json TEXT NULL,
        wo_type_code TEXT NULL,
        justification TEXT NOT NULL DEFAULT '',
        evidence_json TEXT NOT NULL DEFAULT '{}',
        baseline_mtbf_hours REAL NULL,
        baseline_failures INTEGER NULL,
        baseline_exposure_hours REAL NULL,
        status TEXT NOT NULL DEFAULT 'proposed',
        approver_id INTEGER NULL,
        proposed_by_id INTEGER NULL,
        proposed_at TEXT NOT NULL,
        decided_by_id INTEGER NULL,
        decided_at TEXT NULL,
        decision_note TEXT NULL,
        applied_by_id INTEGER NULL,
        applied_at TEXT NULL,
        linked_pm_plan_id INTEGER NULL REFERENCES pm_plans(id),
        linked_pm_plan_version_id INTEGER NULL REFERENCES pm_plan_versions(id),
        linked_work_order_id INTEGER NULL REFERENCES work_orders(id),
        row_version INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_reliability_study_decisions_study ON reliability_study_decisions(study_id, created_at)",
    "CREATE TABLE IF NOT EXISTS reliability_cost_profiles (
        id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        entity_sync_id TEXT NOT NULL UNIQUE,
        equipment_class_id INTEGER NOT NULL UNIQUE REFERENCES equipment_classes(id),
        planned_cost REAL NOT NULL,
        failure_cost REAL NOT NULL,
        currency_code TEXT NULL,
        updated_by_id INTEGER NULL,
        row_version INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in STATEMENTS {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive tables; kept on rollback so synchronised rows are never lost.
        Ok(())
    }
}
