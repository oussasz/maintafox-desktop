//! Lists shared by requests (DI), work orders (OT), planning and RAMS (migrations 198 / 199):
//! the urgency scale `WORK.URGENCY`, the OT types with their EN 13306 nature
//! (`WORK.ORDER_TYPES` / `WORK.MAINTENANCE_NATURES`) and the failure symptoms `WORK.SYMPTOMS`.
//!
//! The migration tests build a database up to the migration under test, write rows the way the
//! application stored them before (legacy ids and codes), run the migration and check every link.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::prelude::MigratorTrait;

use crate::errors::AppError;
use crate::migrations::Migrator;
use crate::reference::urgency_scale::{self, UpdateUrgencyLevelInput};
use crate::wo::types::{self as wo_types, MaintenanceNature};

// ── helpers ────────────────────────────────────────────────────────────────────────────────

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn opt_i64(db: &DatabaseConnection, sql: &str) -> Option<i64> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<i64>>(0).expect("i64 column"))
}

async fn i64_of(db: &DatabaseConnection, sql: &str) -> i64 {
    opt_i64(db, sql).await.unwrap_or_else(|| panic!("{sql}: no value"))
}

async fn opt_str(db: &DatabaseConnection, sql: &str) -> Option<String> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("text column"))
}

async fn table_exists(db: &DatabaseConnection, name: &str) -> bool {
    opt_i64(
        db,
        &format!("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '{name}'"),
    )
    .await
    .is_some()
}

async fn domain_exists(db: &DatabaseConnection, code: &str) -> bool {
    opt_i64(db, &format!("SELECT id FROM reference_domains WHERE code = '{code}'"))
        .await
        .is_some()
}

/// `reference_values.id` of `code` in the published set of `domain`.
async fn value_id(db: &DatabaseConnection, domain: &str, code: &str) -> i64 {
    i64_of(
        db,
        &format!(
            "SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
             JOIN reference_domains d ON d.id = s.domain_id \
             WHERE d.code = '{domain}' AND s.status = 'published' AND v.code = '{code}'"
        ),
    )
    .await
}

/// A database with every migration applied up to, not including, migration `name`.
async fn migrated_before(name: &str) -> DatabaseConnection {
    let db = crate::db::test_db::empty_memory_db().await;
    let pos = Migrator::migrations()
        .iter()
        .position(|m| m.name() == name)
        .unwrap_or_else(|| panic!("migration {name} is not registered"));
    Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
        .await
        .expect("migrations before the one under test");
    db
}

/// No row points at a missing reference value, set or domain after the migration (the legacy
/// fixtures leave equipment / users out on purpose, so only the reference links are checked).
async fn assert_foreign_keys_hold(db: &DatabaseConnection) {
    let broken: Vec<String> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_key_check".to_string(),
        ))
        .await
        .expect("foreign_key_check")
        .iter()
        .filter_map(|r| {
            let table: String = r.try_get("", "table").ok()?;
            let parent: String = r.try_get("", "parent").ok()?;
            parent.starts_with("reference_").then(|| format!("{table} -> {parent}"))
        })
        .collect();
    assert!(broken.is_empty(), "rows break a reference foreign key: {broken:?}");
}

const NOW: &str = "'2026-10-01T08:00:00Z'";

async fn legacy_wo(db: &DatabaseConnection, code: &str, type_code: &str, urgency_code: Option<&str>) -> i64 {
    let urgency = urgency_code
        .map(|u| format!("(SELECT id FROM urgency_levels WHERE code = '{u}')"))
        .unwrap_or_else(|| "NULL".into());
    exec(
        db,
        &format!(
            "INSERT INTO work_orders (entity_sync_id, code, type_id, status_id, urgency_id, title, row_version, \
             created_at, updated_at) VALUES ('sync-{code}', '{code}', \
             (SELECT id FROM work_order_types WHERE code = '{type_code}'), \
             (SELECT id FROM work_order_statuses WHERE code = 'draft'), {urgency}, '{code}', 1, {NOW}, {NOW})"
        ),
    )
    .await;
    i64_of(db, &format!("SELECT id FROM work_orders WHERE code = '{code}'")).await
}

async fn legacy_di(
    db: &DatabaseConnection,
    code: &str,
    reported: &str,
    validated: Option<&str>,
    symptom: Option<i64>,
) -> i64 {
    let validated = validated.map(|v| format!("'{v}'")).unwrap_or_else(|| "NULL".into());
    let symptom = symptom.map(|s| s.to_string()).unwrap_or_else(|| "NULL".into());
    exec(
        db,
        &format!(
            "INSERT INTO intervention_requests (entity_sync_id, code, equipment_id, org_node_id, status, title, \
             description, origin_type, request_type, symptom_code_id, impact_level, production_impact, safety_flag, \
             environmental_flag, quality_flag, reported_urgency, validated_urgency, submitted_at, submitter_id, \
             row_version, created_at, updated_at) VALUES ('sync-{code}', '{code}', 1, 1, 'submitted', '{code}', 'd', \
             'operator', 'repair', {symptom}, 'unknown', 0, 0, 0, 0, '{reported}', {validated}, {NOW}, 1, 1, {NOW}, {NOW})"
        ),
    )
    .await;
    i64_of(
        db,
        &format!("SELECT id FROM intervention_requests WHERE code = '{code}'"),
    )
    .await
}

// ── Migration 198: urgency scale and OT types ───────────────────────────────────────────────

#[tokio::test]
async fn migration_198_maps_urgencies_types_and_sla_rules_by_code() {
    let db = migrated_before(M198).await;
    // Legacy rows reference equipment / users this test does not need.
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    exec(
        &db,
        "INSERT INTO work_order_types (code, label, is_system, is_active) VALUES ('lubrication', 'Graissage', 0, 1)",
    )
    .await;
    let wo_a = legacy_wo(&db, "OT-A", "corrective", Some("critical")).await;
    let wo_b = legacy_wo(&db, "OT-B", "preventive", Some("very_low")).await;
    let wo_c = legacy_wo(&db, "OT-C", "lubrication", Some("medium")).await;
    let wo_d = legacy_wo(&db, "OT-D", "emergency", None).await;
    let wo_e = legacy_wo(&db, "OT-E", "inspection", Some("high")).await;
    exec(
        &db,
        "INSERT INTO schedule_candidates (entity_sync_id, source_type, source_id, priority_id) \
         VALUES ('sync-sc-1', 'work_order', 1, (SELECT id FROM urgency_levels WHERE code = 'high'))",
    )
    .await;
    let di_1 = legacy_di(&db, "DI-1", "critical", Some("medium"), None).await;
    let di_2 = legacy_di(&db, "DI-2", "low", None, None).await;
    // The plant had changed the all-origins "high" targets; and kept one specific rule.
    exec(
        &db,
        "UPDATE di_sla_rules SET target_response_hours = 3, target_resolution_hours = 20, \
         escalation_threshold_hours = 6 WHERE urgency_level = 'high' AND origin_type IS NULL",
    )
    .await;
    exec(
        &db,
        "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, target_response_hours, \
         target_resolution_hours, escalation_threshold_hours, is_active) \
         VALUES ('Critical IoT', 'critical', 'iot', NULL, 1, 4, 2, 1)",
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;

    Migrator::up(&db, None).await.expect("migrations 198+");

    // Urgency of the work orders: by code on the shared scale.
    let level = |code: &'static str| {
        let db = &db;
        async move { value_id(db, "WORK.URGENCY", code).await }
    };
    let wo_urgency = |id: i64| {
        let db = &db;
        async move { opt_i64(db, &format!("SELECT urgency_id FROM work_orders WHERE id = {id}")).await }
    };
    assert_eq!(
        wo_urgency(wo_a).await,
        Some(level("emergency").await),
        "critical → emergency"
    );
    assert_eq!(wo_urgency(wo_b).await, Some(level("low").await), "very_low → low");
    assert_eq!(wo_urgency(wo_c).await, Some(level("normal").await), "medium → normal");
    assert_eq!(wo_urgency(wo_d).await, None, "no urgency stays empty");
    assert_eq!(wo_urgency(wo_e).await, Some(level("high").await), "high → high");
    assert_eq!(
        opt_i64(
            &db,
            "SELECT priority_id FROM schedule_candidates WHERE entity_sync_id = 'sync-sc-1'"
        )
        .await,
        Some(level("high").await)
    );
    let candidate_fk = opt_str(
        &db,
        "SELECT \"table\" FROM pragma_foreign_key_list('schedule_candidates') WHERE \"from\" = 'priority_id'",
    )
    .await;
    assert_eq!(candidate_fk.as_deref(), Some("reference_values"));

    // OT types: same code, nature by EN 13306; a plant's own type keeps code and label.
    let wo_type = |id: i64| {
        let db = &db;
        async move {
            (
                opt_str(
                    db,
                    &format!("SELECT t.code FROM work_orders w JOIN reference_values t ON t.id = w.type_id WHERE w.id = {id}"),
                )
                .await,
                wo_types::work_order_nature(db, id).await.expect("nature"),
            )
        }
    };
    assert_eq!(
        wo_type(wo_a).await,
        (Some("corrective".into()), Some(MaintenanceNature::CorrectiveDeferred))
    );
    assert_eq!(
        wo_type(wo_b).await,
        (
            Some("preventive".into()),
            Some(MaintenanceNature::PreventivePredetermined)
        )
    );
    assert_eq!(wo_type(wo_c).await, (Some("lubrication".into()), None));
    assert_eq!(
        wo_type(wo_d).await,
        (Some("emergency".into()), Some(MaintenanceNature::CorrectiveImmediate))
    );
    assert_eq!(
        wo_type(wo_e).await,
        (
            Some("inspection".into()),
            Some(MaintenanceNature::PreventiveConditionBased)
        )
    );
    let lubrication = wo_types::list_work_order_types(&db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == "lubrication")
        .expect("plant type carried over");
    assert_eq!(lubrication.label, "Graissage");
    assert!(!lubrication.is_system && lubrication.is_active && lubrication.nature.is_none());

    // Requests and SLA rules.
    let di_codes = |id: i64| {
        let db = &db;
        async move {
            (
                opt_str(
                    db,
                    &format!("SELECT reported_urgency FROM intervention_requests WHERE id = {id}"),
                )
                .await,
                opt_str(
                    db,
                    &format!("SELECT validated_urgency FROM intervention_requests WHERE id = {id}"),
                )
                .await,
            )
        }
    };
    assert_eq!(di_codes(di_1).await, (Some("emergency".into()), Some("normal".into())));
    assert_eq!(di_codes(di_2).await, (Some("low".into()), None));
    assert_eq!(
        opt_str(
            &db,
            "SELECT urgency_level FROM di_sla_rules WHERE name = 'Critical IoT'"
        )
        .await
        .as_deref(),
        Some("emergency")
    );
    assert_eq!(
        opt_i64(&db, "SELECT is_active FROM di_sla_rules WHERE name = 'Critical IoT'").await,
        Some(1),
        "specific rules stay"
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM di_sla_rules WHERE origin_type IS NULL AND asset_criticality_class IS NULL AND is_active = 1"
        )
        .await,
        Some(0),
        "all-origins rules moved onto the scale"
    );
    let high = urgency_scale::get_urgency_level(&db, "high").await.unwrap().unwrap();
    assert_eq!(
        (
            high.response_target_hours,
            high.completion_target_hours,
            high.escalation_threshold_hours
        ),
        (Some(3), Some(20), Some(6)),
        "the plant's own targets are kept"
    );
    assert_eq!(high.targets_source, urgency_scale::TARGETS_SOURCE_DI_SLA_RULE);
    let urgent = urgency_scale::get_urgency_level(&db, "urgent").await.unwrap().unwrap();
    assert_eq!(urgent.targets_source, urgency_scale::TARGETS_SOURCE_STARTER);

    // The copies are gone.
    assert!(!table_exists(&db, "urgency_levels").await);
    assert!(!table_exists(&db, "work_order_types").await);
    assert!(!domain_exists(&db, "DI.PRIORITY").await);

    // Owner decisions: OT sub-type column, request types tagged with the OT nature.
    assert!(opt_i64(
        &db,
        "SELECT 1 FROM pragma_table_info('work_orders') WHERE name = 'subtype_id'"
    )
    .await
    .is_some());
    assert_eq!(
        opt_str(
            &db,
            "SELECT v.semantic_tag FROM reference_values v JOIN reference_sets s ON s.id = v.set_id              JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'DI.REQUEST_TYPE' AND v.code = 'inspection'"
        )
        .await
        .as_deref(),
        Some("preventive_condition_based")
    );
    assert!(domain_exists(&db, wo_types::SUBTYPES_DOMAIN).await);
    assert_foreign_keys_hold(&db).await;
}

#[tokio::test]
async fn migration_198_classifies_a_work_order_of_unknown_type_as_deferred_corrective() {
    let db = migrated_before(M198).await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    exec(
        &db,
        &format!(
            "INSERT INTO work_orders (entity_sync_id, code, type_id, status_id, title, notes, row_version, created_at, \
             updated_at) VALUES ('sync-orphan', 'OT-ORPHAN', 987654, 1, 'orphan', 'Avant', 1, {NOW}, {NOW})"
        ),
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    Migrator::up(&db, None)
        .await
        .expect("an unknown type does not stop the start");

    let id = i64_of(&db, "SELECT id FROM work_orders WHERE code = 'OT-ORPHAN'").await;
    assert_eq!(
        wo_types::work_order_nature(&db, id).await.unwrap(),
        Some(MaintenanceNature::CorrectiveDeferred)
    );
    let notes = opt_str(&db, &format!("SELECT notes FROM work_orders WHERE id = {id}"))
        .await
        .unwrap();
    assert!(notes.starts_with("Avant\n[Migration 198]"), "{notes}");
    assert!(notes.contains("type_id=987654"), "{notes}");
}

const M198: &str = "m20261015_000198_shared_urgency_and_wo_natures";
const M199: &str = "m20261015_000199_shared_failure_symptoms";

/// Forget that `name` ran, as if a crash had happened before sea-orm recorded it.
async fn unrecord(db: &DatabaseConnection, name: &str) {
    exec(db, &format!("DELETE FROM seaql_migrations WHERE version = '{name}'")).await;
}

async fn wo_links(db: &DatabaseConnection, code: &str) -> (Option<i64>, Option<i64>) {
    let urgency = opt_i64(db, &format!("SELECT urgency_id FROM work_orders WHERE code = '{code}'")).await;
    let ty = opt_i64(db, &format!("SELECT type_id FROM work_orders WHERE code = '{code}'")).await;
    (urgency, ty)
}

#[tokio::test]
async fn migration_198_can_run_again_without_nulling_or_remapping_twice() {
    let db = migrated_before(M198).await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    legacy_wo(&db, "OT-R", "emergency", Some("critical")).await;
    exec(
        &db,
        "INSERT INTO schedule_candidates (entity_sync_id, source_type, source_id, priority_id) \
         VALUES ('sync-sc-r', 'work_order', 1, (SELECT id FROM urgency_levels WHERE code = 'medium'))",
    )
    .await;
    legacy_di(&db, "DI-R", "critical", Some("medium"), None).await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    Migrator::up(&db, Some(1)).await.expect("migration 198");
    let after_first = wo_links(&db, "OT-R").await;
    let candidate = opt_i64(
        &db,
        "SELECT priority_id FROM schedule_candidates WHERE entity_sync_id = 'sync-sc-r'",
    )
    .await;

    // The same step again (not recorded): nothing moves.
    unrecord(&db, M198).await;
    Migrator::up(&db, Some(1)).await.expect("migration 198 again");
    assert_eq!(wo_links(&db, "OT-R").await, after_first);
    assert_eq!(after_first.0, Some(value_id(&db, "WORK.URGENCY", "emergency").await));
    assert_eq!(
        opt_i64(
            &db,
            "SELECT priority_id FROM schedule_candidates WHERE entity_sync_id = 'sync-sc-r'"
        )
        .await,
        candidate
    );
    assert_eq!(candidate, Some(value_id(&db, "WORK.URGENCY", "normal").await));
    assert_eq!(
        opt_str(
            &db,
            "SELECT reported_urgency FROM intervention_requests WHERE code = 'DI-R'"
        )
        .await
        .as_deref(),
        Some("emergency")
    );
    assert_foreign_keys_hold(&db).await;
}

#[tokio::test]
async fn migration_198_resumes_a_partial_state_left_by_an_interrupted_run() {
    // A run interrupted after the work orders were remapped but before `urgency_levels` was
    // dropped (possible before the step ran in one transaction): resuming must not null or remap
    // the already remapped work order.
    let db = migrated_before(M198).await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    legacy_wo(&db, "OT-DONE", "corrective", Some("high")).await;
    legacy_wo(&db, "OT-TODO", "corrective", Some("low")).await;
    urgency_scale::ensure_urgency_scale(&db).await.expect("scale");
    let emergency = value_id(&db, "WORK.URGENCY", "emergency").await;
    exec(
        &db,
        &format!("UPDATE work_orders SET urgency_id = {emergency} WHERE code = 'OT-DONE'"),
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;

    Migrator::up(&db, None).await.expect("resume");
    assert_eq!(wo_links(&db, "OT-DONE").await.0, Some(emergency), "kept, not nulled");
    assert_eq!(
        wo_links(&db, "OT-TODO").await.0,
        Some(value_id(&db, "WORK.URGENCY", "low").await)
    );
}

#[tokio::test]
async fn migration_198_drops_pending_sync_entries_of_the_removed_entity_types() {
    let db = migrated_before(M198).await;
    exec(
        &db,
        "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, operation) VALUES \
           ('urgency_levels', 'u-1', 'upsert'), ('work_order_types', 't-1', 'upsert'), ('work_orders', 'w-1', 'upsert')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, payload_json, \
           payload_hash, status) VALUES \
           ('k1', 'urgency_levels', 'u-1', 'upsert', 1, '{}', 'h', 'pending'), \
           ('k2', 'work_order_types', 't-1', 'upsert', 1, '{}', 'h', 'acknowledged'), \
           ('k3', 'work_orders', 'w-1', 'upsert', 1, '{}', 'h', 'pending')",
    )
    .await;
    Migrator::up(&db, None).await.expect("migrations");
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type IN ('urgency_levels', 'work_order_types')"
        )
        .await,
        Some(0)
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'work_orders'"
        )
        .await,
        Some(1),
        "other entries stay"
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM sync_outbox WHERE status = 'pending' AND entity_type = 'urgency_levels'"
        )
        .await,
        Some(0)
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id IN ('t-1', 'w-1')"
        )
        .await,
        Some(2),
        "history (acknowledged) and other types stay"
    );
}

#[tokio::test]
async fn migration_199_can_run_again_without_touching_the_links() {
    let db = migrated_before(M199).await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    let di_vibration = value_id(&db, "DI.SYMPTOM", "vibration").await;
    legacy_di(&db, "DI-S", "high", None, Some(di_vibration)).await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    Migrator::up(&db, None).await.expect("migration 199");
    let linked = opt_i64(
        &db,
        "SELECT symptom_code_id FROM intervention_requests WHERE code = 'DI-S'",
    )
    .await;
    unrecord(&db, M199).await;
    Migrator::up(&db, None).await.expect("migration 199 again");
    assert_eq!(
        opt_i64(
            &db,
            "SELECT symptom_code_id FROM intervention_requests WHERE code = 'DI-S'"
        )
        .await,
        linked
    );
    assert_eq!(linked, Some(value_id(&db, "WORK.SYMPTOMS", "vibration").await));
}

// ── Startup retirement pass ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn startup_retirement_pass_recreates_nothing_on_a_migrated_database() {
    let db = crate::db::test_db::seeded().await;
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("integrity pass");
    let retired = crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("pass");
    assert!(retired.is_empty(), "{retired:?}");
    assert!(!domain_exists(&db, "DI.SYMPTOM").await);
    assert!(!domain_exists(&db, "DI.PRIORITY").await);
}

#[tokio::test]
async fn startup_retirement_pass_merges_a_list_brought_back_by_sync_and_sends_the_change() {
    let db = crate::db::test_db::seeded().await;
    crate::sync::capture::install_capture(&db).await.expect("capture on");
    // A replayed history recreates DI.SYMPTOM with one value and a request pointing at it.
    exec(
        &db,
        "INSERT INTO reference_domains (code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
         VALUES ('DI.SYMPTOM', 'Symptômes DI', 'flat', 'tenant_managed', 1, datetime('now'), datetime('now'))",
    )
    .await;
    exec(
        &db,
        "INSERT INTO reference_sets (domain_id, version_no, status, created_at) \
         SELECT id, 1, 'published', datetime('now') FROM reference_domains WHERE code = 'DI.SYMPTOM'",
    )
    .await;
    exec(
        &db,
        "INSERT INTO reference_values (set_id, code, label, is_active) \
         SELECT s.id, 'vibration', 'Vibration anormale', 1 FROM reference_sets s \
           JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'DI.SYMPTOM'",
    )
    .await;
    let ghost = value_id(&db, "DI.SYMPTOM", "vibration").await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    legacy_di(&db, "DI-GHOST", "normal", None, Some(ghost)).await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    exec(&db, "DELETE FROM sync_capture_journal").await;

    let retired = crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("pass");
    assert_eq!(retired, vec!["DI.SYMPTOM".to_string()]);
    assert!(!domain_exists(&db, "DI.SYMPTOM").await);
    assert_eq!(
        opt_i64(
            &db,
            "SELECT symptom_code_id FROM intervention_requests WHERE code = 'DI-GHOST'"
        )
        .await,
        Some(value_id(&db, "WORK.SYMPTOMS", "vibration").await)
    );
    // Capture was on: the remap and the deletes are queued for the other devices.
    for (entity, op) in [
        ("intervention_requests", "upsert"),
        ("reference_values", "delete"),
        ("reference_domains", "delete"),
    ] {
        assert!(
            opt_i64(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = '{entity}' AND operation = '{op}'"
                )
            )
            .await
            .unwrap_or(0)
                > 0,
            "{entity} {op} captured"
        );
    }
    // And a second start finds nothing to do.
    assert!(crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("pass")
        .is_empty());
}

// ── Migration 199: failure symptoms ─────────────────────────────────────────────────────────

#[tokio::test]
async fn migration_199_merges_request_symptoms_and_keeps_every_link() {
    let db = migrated_before(M199).await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    let di_vibration = value_id(&db, "DI.SYMPTOM", "vibration").await;
    let work_vibration = value_id(&db, "WORK.SYMPTOMS", "vibration").await;
    assert_ne!(di_vibration, work_vibration);
    exec(
        &db,
        "INSERT INTO reference_values (entity_sync_id, set_id, code, label, is_active) \
         SELECT 'sync-seal-weep', s.id, 'seal_weep', 'Suintement de garniture', 1 \
           FROM reference_sets s JOIN reference_domains d ON d.id = s.domain_id \
          WHERE d.code = 'DI.SYMPTOM' AND s.status = 'published'",
    )
    .await;
    let plant_symptom = value_id(&db, "DI.SYMPTOM", "seal_weep").await;
    let di_a = legacy_di(&db, "DI-A", "high", None, Some(di_vibration)).await;
    let di_b = legacy_di(&db, "DI-B", "low", None, Some(plant_symptom)).await;
    // Migration 198 already moved the OT types to `WORK.ORDER_TYPES`.
    let corrective = value_id(&db, "WORK.ORDER_TYPES", "corrective").await;
    exec(
        &db,
        &format!(
            "INSERT INTO work_orders (entity_sync_id, code, type_id, status_id, title, row_version, created_at,              updated_at) VALUES ('sync-OT-S', 'OT-S', {corrective},              (SELECT id FROM work_order_statuses WHERE code = 'draft'), 'OT-S', 1, {NOW}, {NOW})"
        ),
    )
    .await;
    let wo = i64_of(&db, "SELECT id FROM work_orders WHERE code = 'OT-S'").await;
    exec(
        &db,
        &format!(
            "INSERT INTO work_order_failure_details (work_order_id, symptom_id, is_temporary_repair, \
             is_permanent_repair, cause_not_determined) VALUES ({wo}, {di_vibration}, 0, 1, 1)"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, created_at, updated_at) \
             VALUES ('failure_event:test', 'work_order', {wo}, 1, {NOW}, {NOW})"
        ),
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;

    Migrator::up(&db, None).await.expect("migration 199");

    let di_symptom = |id: i64| {
        let db = &db;
        async move {
            opt_i64(
                db,
                &format!("SELECT symptom_code_id FROM intervention_requests WHERE id = {id}"),
            )
            .await
        }
    };
    assert_eq!(di_symptom(di_a).await, Some(work_vibration), "same code → shared value");
    assert_eq!(
        di_symptom(di_b).await,
        Some(plant_symptom),
        "a plant value keeps its id"
    );
    assert_eq!(
        value_id(&db, "WORK.SYMPTOMS", "seal_weep").await,
        plant_symptom,
        "…and now belongs to the shared list"
    );
    assert_eq!(
        opt_i64(
            &db,
            &format!("SELECT symptom_id FROM work_order_failure_details WHERE work_order_id = {wo}")
        )
        .await,
        Some(work_vibration)
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT symptom_id FROM failure_events WHERE entity_sync_id = 'failure_event:test'"
        )
        .await,
        Some(work_vibration),
        "RAMS failure event records the observed symptom"
    );
    assert!(!domain_exists(&db, "DI.SYMPTOM").await);
    // ISO 14224 codes added to the shared list.
    for code in [
        "breakdown",
        "fail_to_function",
        "fail_to_open",
        "fail_to_close",
        "unknown",
    ] {
        value_id(&db, "WORK.SYMPTOMS", code).await;
    }
    assert_foreign_keys_hold(&db).await;
}

// ── Urgency scale: company attributes ───────────────────────────────────────────────────────

fn level_input(fr: &str, en: &str, response: i64, completion: i64, escalation: i64) -> UpdateUrgencyLevelInput {
    UpdateUrgencyLevelInput {
        label_fr: fr.into(),
        label_en: en.into(),
        response_target_hours: response,
        completion_target_hours: completion,
        escalation_threshold_hours: escalation,
    }
}

fn is_scale_refusal(err: AppError) -> bool {
    matches!(err, AppError::ValidationFailed(ref m) if m.iter().any(|s| s.contains(urgency_scale::GATE_URGENCY_SCALE_INVALID)))
}

#[tokio::test]
async fn the_scale_has_five_fixed_levels_whose_labels_and_targets_the_plant_edits() {
    let db = crate::db::test_db::seeded().await;
    let levels = urgency_scale::list_urgency_levels(&db).await.expect("levels");
    let codes: Vec<&str> = levels.iter().map(|l| l.code.as_str()).collect();
    assert_eq!(codes, ["emergency", "urgent", "high", "normal", "low"]);
    assert!(
        levels.iter().all(|l| l.targets().is_some()),
        "starter targets on every level"
    );

    let saved = urgency_scale::update_urgency_level(&db, "urgent", level_input("Prioritaire", "Priority", 2, 12, 6))
        .await
        .expect("save company attributes");
    assert_eq!(
        (saved.label_fr.as_str(), saved.label_en.as_str()),
        ("Prioritaire", "Priority")
    );
    assert_eq!(saved.completion_target_hours, Some(12));
    assert_eq!(saved.targets_source, urgency_scale::TARGETS_SOURCE_COMPANY);

    // The startup seeder never rewrites what the plant saved.
    urgency_scale::ensure_urgency_scale(&db).await.expect("reseed");
    let again = urgency_scale::get_urgency_level(&db, "urgent").await.unwrap().unwrap();
    assert_eq!(again.label_fr, "Prioritaire");

    // A more urgent level cannot get longer targets than a less urgent one.
    let inverted = urgency_scale::update_urgency_level(&db, "urgent", level_input("U", "U", 100, 200, 50)).await;
    assert!(is_scale_refusal(inverted.expect_err("inverted targets")));
    // Response after completion, unknown level, empty label.
    assert!(is_scale_refusal(
        urgency_scale::update_urgency_level(&db, "low", level_input("B", "L", 200, 100, 50))
            .await
            .expect_err("response > completion")
    ));
    assert!(is_scale_refusal(
        urgency_scale::update_urgency_level(&db, "critical", level_input("C", "C", 1, 2, 1))
            .await
            .expect_err("not a level")
    ));
    assert!(is_scale_refusal(
        urgency_scale::update_urgency_level(&db, "low", level_input(" ", "Low", 72, 168, 120))
            .await
            .expect_err("empty label")
    ));
    // The generic reference editor cannot touch the SYSTEM domain.
    let refused = crate::reference::audience::assert_value_id_editable(&db, saved.id).await;
    assert!(refused.is_err(), "WORK.URGENCY is a SYSTEM list");
}

// ── OT types: every shipped type has its nature ─────────────────────────────────────────────

#[tokio::test]
async fn shipped_types_carry_their_en_13306_nature_and_a_default_per_nature() {
    let db = crate::db::test_db::seeded().await;
    let types = wo_types::list_work_order_types(&db).await.expect("types");
    let nature_of = |code: &str| {
        types
            .iter()
            .find(|t| t.code == code)
            .and_then(|t| t.nature.as_deref())
            .and_then(MaintenanceNature::parse)
    };
    assert_eq!(nature_of("emergency"), Some(MaintenanceNature::CorrectiveImmediate));
    assert_eq!(nature_of("corrective"), Some(MaintenanceNature::CorrectiveDeferred));
    assert_eq!(
        nature_of("preventive"),
        Some(MaintenanceNature::PreventivePredetermined)
    );
    assert_eq!(nature_of("overhaul"), Some(MaintenanceNature::PreventivePredetermined));
    assert_eq!(
        nature_of("condition_based"),
        Some(MaintenanceNature::PreventiveConditionBased)
    );
    assert_eq!(
        nature_of("inspection"),
        Some(MaintenanceNature::PreventiveConditionBased)
    );
    assert_eq!(nature_of("predictive"), Some(MaintenanceNature::PreventivePredictive));
    assert_eq!(nature_of("improvement"), Some(MaintenanceNature::Improvement));
    for nature in MaintenanceNature::ALL {
        let t = wo_types::default_type_for_nature(&db, nature)
            .await
            .expect("default type");
        assert_eq!(t.nature.as_deref(), Some(nature.as_str()));
    }
    assert_eq!(
        wo_types::default_type_for_nature(&db, MaintenanceNature::CorrectiveImmediate)
            .await
            .unwrap()
            .code,
        "emergency"
    );
    // Both lists are SYSTEM: hidden from the generic editor, read-only.
    for domain in [wo_types::WORK_ORDER_TYPES_DOMAIN, wo_types::NATURES_DOMAIN] {
        let audience = opt_str(
            &db,
            &format!("SELECT audience FROM reference_domains WHERE code = '{domain}'"),
        )
        .await;
        assert_eq!(audience.as_deref(), Some("system"), "{domain}");
    }
}

// ── Owner decisions 2026-10-15: request types and OT sub-types ──────────────────────────────

#[tokio::test]
async fn request_types_are_a_company_list_whose_tag_names_the_ot_nature() {
    let db = crate::db::test_db::seeded().await;
    let domain = crate::reference::domains::get_reference_domain_by_code(&db, "DI.REQUEST_TYPE")
        .await
        .expect("domain");
    assert_eq!(domain.audience, crate::reference::audience::DomainAudience::Company);
    let caps = crate::reference::governance::capabilities_for(&domain, None);
    assert_eq!(caps.category, "controlled_catalog");
    assert!(caps.can_create_draft_set && caps.can_publish, "{caps:?}");

    let nature = |code: &'static str| {
        let db = &db;
        async move { wo_types::nature_for_request_type(db, code).await.expect("nature") }
    };
    assert_eq!(
        nature("inspection").await,
        Some(MaintenanceNature::PreventiveConditionBased)
    );
    assert_eq!(
        nature("preventive").await,
        Some(MaintenanceNature::PreventivePredetermined)
    );
    assert_eq!(nature("improvement").await, Some(MaintenanceNature::Improvement));
    assert_eq!(nature("repair").await, None, "a repair becomes corrective work");
}

async fn seed_equipment(db: &DatabaseConnection) {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
         VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
         VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', datetime('now'), datetime('now'), 1)",
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Pompe', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
    ] {
        exec(db, sql).await;
    }
}

fn wo_input(
    type_code: &str,
    subtype: Option<&str>,
    urgency: Option<&str>,
    creator: i64,
) -> crate::wo::domain::WoCreateInput {
    crate::wo::domain::WoCreateInput {
        type_code: type_code.into(),
        equipment_id: Some(1),
        location_id: None,
        source_di_id: None,
        source_inspection_anomaly_id: None,
        source_ram_ishikawa_diagram_id: None,
        source_ishikawa_flow_node_id: None,
        source_rca_cause_text: None,
        entity_id: None,
        planner_id: None,
        urgency_code: urgency.map(str::to_string),
        subtype_code: subtype.map(str::to_string),
        title: "Graissage paliers".into(),
        description: None,
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        creator_id: creator,
        requires_permit: None,
    }
}

#[tokio::test]
async fn ot_subtypes_hang_under_one_nature_and_must_match_the_ot_type() {
    let db = crate::db::test_db::seeded().await;
    seed_equipment(&db).await;
    let admin = i64_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let predetermined = value_id(&db, wo_types::NATURES_DOMAIN, "preventive_predetermined").await;
    let create = |parent: Option<i64>, label: &'static str| {
        let db = &db;
        async move {
            crate::reference::values::create_operational_value(
                db,
                crate::reference::values::CreateOperationalReferenceValuePayload {
                    domain_code: wo_types::SUBTYPES_DOMAIN.into(),
                    label: label.into(),
                    description: None,
                    parent_id: parent,
                    code: None,
                },
                admin,
            )
            .await
        }
    };
    let missing = create(None, "Sans nature")
        .await
        .expect_err("a sub-type needs a nature");
    assert!(matches!(missing, AppError::ValidationFailed(ref m) if m[0].contains("GATE_REF_PARENT_REQUIRED")));
    let wrong_domain = value_id(&db, "WORK.URGENCY", "high").await;
    assert!(create(Some(wrong_domain), "Mauvais parent").await.is_err());

    let lubrication = create(Some(predetermined), "Lubrification")
        .await
        .expect("plant sub-type");
    let listed = wo_types::list_work_order_subtypes(&db).await.expect("subtypes");
    let row = listed.iter().find(|s| s.id == lubrication.id).expect("listed");
    assert_eq!(row.nature.as_deref(), Some("preventive_predetermined"));

    // Same nature as the OT type: accepted and shown on the OT, rules keep the type's nature.
    let wo = crate::wo::queries::create_work_order(
        &db,
        wo_input("preventive", Some(&lubrication.code), Some("normal"), admin),
    )
    .await
    .expect("preventive OT with its sub-type");
    let read = crate::wo::queries::get_work_order(&db, wo.id).await.unwrap().unwrap();
    assert_eq!(read.subtype_code.as_deref(), Some(lubrication.code.as_str()));
    assert_eq!(read.subtype_label.as_deref(), Some("Lubrification"));
    assert_eq!(read.nature_code.as_deref(), Some("preventive_predetermined"));
    assert_eq!(read.urgency_code.as_deref(), Some("normal"));

    // Another nature: refused.
    let err = crate::wo::queries::create_work_order(&db, wo_input("corrective", Some(&lubrication.code), None, admin))
        .await
        .expect_err("corrective OT cannot carry a preventive sub-type");
    assert!(matches!(err, AppError::ValidationFailed(ref m) if m[0].contains(wo_types::GATE_WO_SUBTYPE_INVALID)));
    // An unknown urgency level is refused too.
    let err = crate::wo::queries::create_work_order(&db, wo_input("corrective", None, Some("critical"), admin))
        .await
        .expect_err("legacy urgency code");
    assert!(
        matches!(err, AppError::ValidationFailed(ref m) if m[0].contains(urgency_scale::GATE_URGENCY_SCALE_INVALID))
    );
}
