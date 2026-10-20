//! Test kit for sync (Phase 2.4 / 2.5): databases as the startup leaves them, an in-process
//! server that follows the protocol v2 rules (`SYNC_PROTOCOL_V2.md` §3–4), and a factory that
//! creates a valid row of any replicated entity type (parents included).
//!
//! The in-process server is the reference behaviour the desktop relies on (conflict on a stale
//! base version, duplicate acknowledgement, tombstones, "never echo a device's own change",
//! per-user delivery), used to exercise the desktop engine quickly. The real API is exercised by
//! the multi-device harness (Phase 2.5).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as SqlValue};
use sea_orm_migration::MigratorTrait;

use super::domain::{
    ApplySyncBatchInput, SyncAckInput, SyncInboundItemInput, SyncServerConflictInput, SYNC_PROTOCOL_VERSION_V2,
};
use super::materialize::{materialize_inbox_batch, MaterializeInboxResult};
use super::queries::{apply_sync_batch, get_sync_push_payload};
use super::registry::{self, SyncScope};
use super::row_codec::{self, RowLocator};
use super::row_layout::{self, Role, Target};
use crate::db::test_db::Template;

static STARTED: Template = Template::new(|db| Box::pin(prepare_started(db)));

/// A database as the startup leaves it: migrations, every startup seeder, capture installed.
/// A private in-memory copy of a database built that way once per test process (`db::test_db`).
/// Every copy has the same replica identity (device code) and random ids: for two or more devices
/// in one test use [`started_device_db`].
pub(crate) async fn started_db() -> DatabaseConnection {
    STARTED.copy().await
}

/// Like [`started_db`], but built from scratch like a separately installed device: its own replica
/// identity, device code and random ids. For tests that put several devices side by side.
pub(crate) async fn started_device_db() -> DatabaseConnection {
    STARTED.fresh().await
}

/// Like [`started_db`], but an encrypted database file opened like the app opens it
/// (`db::connect_sqlcipher` with a fresh key, then the pragmas of `db::init_db`): disk writes and
/// encryption cost what they cost on a device. Used by measurements.
pub(crate) async fn started_file_db(path: &std::path::Path) -> DatabaseConnection {
    let key = crate::db::encryption::DatabaseKey::generate();
    let db = crate::db::connect_sqlcipher(path, Some(&key), true)
        .await
        .expect("encrypted database file");
    for pragma in [
        "PRAGMA journal_mode=WAL;",
        "PRAGMA foreign_keys=ON;",
        "PRAGMA busy_timeout=5000;",
        "PRAGMA cache_size=-20000;",
        "PRAGMA temp_store=MEMORY;",
    ] {
        db.execute_unprepared(pragma).await.expect("pragma");
    }
    prepare_started(&db).await;
    db
}

async fn prepare_started(db: &DatabaseConnection) {
    crate::migrations::Migrator::up(db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(db).await.expect("system seed");
    crate::wo::types::ensure_work_order_type_catalog(db)
        .await
        .expect("work order types");
    crate::reference::urgency_scale::ensure_urgency_scale(db)
        .await
        .expect("urgency scale");
    crate::wo::statuses::ensure_required_work_order_statuses(db)
        .await
        .expect("work order statuses");
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(db)
        .await
        .expect("reference catalog");
    super::capture::install_capture(db).await.expect("install capture");
}

pub(crate) async fn exec(db: &impl ConnectionTrait, sql: &str, values: Vec<SqlValue>) {
    db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

pub(crate) async fn scalar_i64(db: &impl ConnectionTrait, sql: &str, values: Vec<SqlValue>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<i64>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

pub(crate) async fn scalar_opt_text(db: &impl ConnectionTrait, sql: &str, values: Vec<SqlValue>) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
}

// ── In-process server (protocol v2 rules) ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ServerRow {
    pub row_version: i64,
    pub operation: String,
    pub payload_json: String,
    pub payload_hash: String,
    pub owner_user_sync_id: Option<String>,
    pub deleted: bool,
    pub origin_device: String,
    pub seq: i64,
}

#[derive(Debug, Default)]
pub(crate) struct TestServer {
    seq: i64,
    pub state: BTreeMap<(String, String), ServerRow>,
    cursors: HashMap<String, i64>,
}

#[derive(Debug)]
pub(crate) struct ExchangeReport {
    pub sent: usize,
    pub acknowledged: usize,
    pub conflicts: usize,
    pub received: usize,
    pub materialized: MaterializeInboxResult,
}

impl TestServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// One exchange of `device` (push its outbox, receive what other devices changed), then the
    /// device applies the answer and materializes its inbox, as the worker does.
    pub async fn exchange(&mut self, device: &str, db: &DatabaseConnection) -> ExchangeReport {
        let push = get_sync_push_payload(db, Some(1000)).await.expect("push payload");
        let mut acknowledged = Vec::new();
        let mut conflicts = Vec::new();
        for item in &push.outbox_batch {
            let key = (item.entity_type.clone(), item.entity_sync_id.clone());
            if let Some(cur) = self.state.get(&key).cloned() {
                if cur.row_version == item.row_version && cur.payload_hash == item.payload_hash {
                    acknowledged.push(SyncAckInput {
                        idempotency_key: item.idempotency_key.clone(),
                        entity_sync_id: item.entity_sync_id.clone(),
                        operation: item.operation.clone(),
                        server_seq: Some(cur.seq),
                        duplicate: true,
                    });
                    continue;
                }
                if item.base_row_version != Some(cur.row_version) {
                    conflicts.push(SyncServerConflictInput {
                        idempotency_key: item.idempotency_key.clone(),
                        entity_type: item.entity_type.clone(),
                        entity_sync_id: item.entity_sync_id.clone(),
                        local_row_version: item.row_version,
                        local_base_row_version: item.base_row_version,
                        server_row_version: cur.row_version,
                        server_operation: cur.operation.clone(),
                        server_payload_json: Some(cur.payload_json.clone()),
                        server_payload_hash: Some(cur.payload_hash.clone()),
                        server_owner_user_sync_id: cur.owner_user_sync_id.clone(),
                        server_deleted: cur.deleted,
                    });
                    continue;
                }
            }
            assert_eq!(
                item.row_version,
                item.base_row_version.map_or(1, |b| b + 1),
                "row_version must be base + 1 ({}/{})",
                item.entity_type,
                item.entity_sync_id
            );
            if registry::entity(&item.entity_type).map(|e| e.scope) == Some(SyncScope::User) {
                assert!(
                    item.owner_user_sync_id.is_some(),
                    "user-scoped item without owner: {}/{}",
                    item.entity_type,
                    item.entity_sync_id
                );
            }
            self.seq += 1;
            self.state.insert(
                key,
                ServerRow {
                    row_version: item.row_version,
                    operation: item.operation.clone(),
                    payload_json: item.payload_json.clone(),
                    payload_hash: item.payload_hash.clone(),
                    owner_user_sync_id: item.owner_user_sync_id.clone(),
                    deleted: item.operation == "delete",
                    origin_device: device.to_string(),
                    seq: self.seq,
                },
            );
            acknowledged.push(SyncAckInput {
                idempotency_key: item.idempotency_key.clone(),
                entity_sync_id: item.entity_sync_id.clone(),
                operation: item.operation.clone(),
                server_seq: Some(self.seq),
                duplicate: false,
            });
        }

        let cursor = self.cursors.get(device).copied().unwrap_or(0);
        let mut changed: Vec<(&(String, String), &ServerRow)> = self
            .state
            .iter()
            .filter(|(_, r)| r.seq > cursor && r.origin_device != device)
            .filter(
                |((entity_type, _), r)| match registry::entity(entity_type).map(|e| e.scope) {
                    Some(SyncScope::User) => r
                        .owner_user_sync_id
                        .as_ref()
                        .is_some_and(|o| push.current_user_sync_ids.contains(o)),
                    _ => true,
                },
            )
            .collect();
        changed.sort_by_key(|(_, r)| r.seq);
        let inbound: Vec<SyncInboundItemInput> = changed
            .iter()
            .map(|((entity_type, entity_sync_id), r)| SyncInboundItemInput {
                entity_type: entity_type.clone(),
                entity_sync_id: entity_sync_id.clone(),
                operation: if r.deleted {
                    "delete".into()
                } else {
                    r.operation.clone()
                },
                row_version: r.row_version,
                payload_json: r.payload_json.clone(),
                server_seq: Some(r.seq),
                payload_hash: Some(r.payload_hash.clone()),
                owner_user_sync_id: r.owner_user_sync_id.clone(),
                deleted: r.deleted,
            })
            .collect();
        self.cursors.insert(device.to_string(), self.seq);

        let sent = push.outbox_batch.len();
        let acknowledged_count = acknowledged.len();
        let conflict_count = conflicts.len();
        let received = inbound.len();
        apply_sync_batch(
            db,
            ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
                server_batch_id: format!("batch_{device}_{}", self.seq),
                checkpoint_token: format!("v2.test-{device}.{}", self.seq),
                acknowledged_items: acknowledged,
                rejected_items: vec![],
                inbound_items: inbound,
                conflict_items: conflicts,
                policy_metadata_json: None,
            },
        )
        .await
        .expect("apply exchange answer");
        let materialized = materialize_inbox_batch(db, Some(2000)).await.expect("materialize");
        ExchangeReport {
            sent,
            acknowledged: acknowledged_count,
            conflicts: conflict_count,
            received,
            materialized,
        }
    }
}

// ── Row factory ─────────────────────────────────────────────────────────────────────────────

/// Literal values for columns the generic generator cannot guess (enumerations checked by code,
/// cross-column constraints). `(entity type, column, SQL literal)`.
const VALUE_OVERRIDES: &[(&str, &str, &str)] = &[
    // Movement types are an enumeration of the inventory commands (`inventory::ledger`).
    ("inventory_transactions", "movement_type", "'ADJUST_IN'"),
    // The next run is computed from it on every device.
    ("report_schedules", "cron_expr", "'0 0 8 * * *'"),
    // Only tenant settings are synchronised (device settings stay on the device).
    ("app_settings", "setting_scope", "'tenant'"),
    // Per-site rules: a known rule; relaxing a site needs its audited change, so the generic
    // row turns the rule off (the strict side), and its event carries a reason.
    (
        "site_settings",
        "setting_key",
        "'reliability.decisions.allow_self_approval'",
    ),
    ("site_settings", "value_json", "'false'"),
    (
        "site_setting_events",
        "setting_key",
        "'reliability.decisions.allow_self_approval'",
    ),
    ("site_setting_events", "old_value_json", "'true'"),
    ("site_setting_events", "new_value_json", "'false'"),
    (
        "site_setting_events",
        "reason",
        "'Single reliability engineer on this site'",
    ),
    ("site_setting_events", "required_step_up", "1"),
    // Notification categories are referenced by code (seeded on every device).
    (
        "notification_preferences",
        "category_code",
        "(SELECT code FROM notification_categories ORDER BY id LIMIT 1)",
    ),
    (
        "notification_rules",
        "category_code",
        "(SELECT code FROM notification_categories ORDER BY id LIMIT 1)",
    ),
    // Labour booked to a person without a login (migration 190): personnel only.
    ("work_order_interveners", "intervener_id", "NULL"),
    // A received notice must name its created account as that account is
    // (`auth::recovery_code::inbound_notice_check`; a generic row has no redemption audit record):
    // the seeded administrator (same identity on every device), with its names, and a device code.
    (
        "admin_recovery_notices",
        "created_user_id",
        "(SELECT MIN(id) FROM user_accounts)",
    ),
    (
        "admin_recovery_notices",
        "created_username",
        "(SELECT username FROM user_accounts WHERE id = (SELECT MIN(id) FROM user_accounts))",
    ),
    (
        "admin_recovery_notices",
        "created_display_name",
        "(SELECT COALESCE(display_name, '') FROM user_accounts WHERE id = (SELECT MIN(id) FROM user_accounts))",
    ),
    ("admin_recovery_notices", "redeemed_device_code", "'7KQ2'"),
    // Only a notice's recipient acknowledges it (`auth::recovery_code::inbound_acknowledgement_check`):
    // the acknowledgement names the recipient of the notice just created as its parent.
    (
        "admin_recovery_acknowledgements",
        "acknowledged_by_id",
        "(SELECT recipient_user_id FROM admin_recovery_notices ORDER BY id DESC LIMIT 1)",
    ),
    // A requirement asks for one certification or one skill, never both.
    ("qualification_requirements", "skill_reference_value_id", "NULL"),
    ("qualification_requirements", "min_proficiency_level", "NULL"),
    // An override states its reason (at least 10 characters).
    (
        "qualification_overrides",
        "reason",
        "'Supervised on site by the shift lead'",
    ),
    ("qualification_overrides", "role_code", "NULL"),
    // A personnel record or a login without one, never both.
    ("qualification_overrides", "user_id", "NULL"),
    ("qualification_overrides", "permit_id", "NULL"),
    // The approver of an override must hold the certification right on the receiving device
    // (`qualification::sync_check`): generic accounts (administrators, `is_*` = 1) are not deleted.
    ("user_accounts", "deleted_at", "NULL"),
    // A received permit is judged on its status (`permit::sync_check`): the generic permit is
    // active (its generic times are in order: fitted, verified, activated, lock removed, expiry),
    // so its generic isolations arrive with it as a valid group.
    ("work_permits", "status", "'active'"),
    // The generic isolation is fitted, verified and removed under that active permit: a line in
    // use, never cancelled (a cancelled line is one never applied, on a draft or a permit waiting
    // for review, `permit::sync_check::cancellation_check`).
    ("permit_isolations", "cancelled_at", "NULL"),
    ("permit_isolations", "cancelled_by_id", "NULL"),
    // A generic work order carries no plant-rule permit flag (that trace is written at conversion and
    // judged on arrival, `permit::sync_check::inbound_permit_rule_check`).
    ("work_orders", "permit_rule_code", "NULL"),
    ("work_orders", "permit_rule_permit_type", "NULL"),
    ("work_orders", "permit_rule_cleared_at", "NULL"),
    // A generic record carries no assigned-site trace (written once by an administrator on a record
    // without a site and judged on arrival, `org::site_assignment::inbound_site_assignment_check`).
    ("work_orders", "site_assigned_by_id", "NULL"),
    ("work_orders", "site_assigned_at", "NULL"),
    ("work_orders", "site_assign_reason", "NULL"),
    ("intervention_requests", "site_assigned_by_id", "NULL"),
    ("intervention_requests", "site_assigned_at", "NULL"),
    ("intervention_requests", "site_assign_reason", "NULL"),
    ("planning_windows", "site_assigned_by_id", "NULL"),
    ("planning_windows", "site_assigned_at", "NULL"),
    ("planning_windows", "site_assign_reason", "NULL"),
    // A calendar day, not a timestamp.
    ("work_calendar_holidays", "holiday_date", "'2026-12-25'"),
    // A planned crew is 1–50 people (migration 207 triggers).
    ("work_orders", "planned_crew_size", "2"),
    ("pm_plan_versions", "planned_crew_size", "2"),
    ("schedule_candidates", "planned_crew_size", "2"),
    // Timetables (migrations 192, 197): calendar days, a weekly cycle, percentages in range.
    ("site_work_weeks", "effective_from", "'2026-12-01'"),
    ("site_work_weeks", "weekend_days", "'5,6'"),
    ("schedule_class_versions", "effective_from", "'2026-12-01'"),
    ("schedule_class_versions", "cycle_length_days", "7"),
    ("schedule_class_versions", "rotation_start_date", "NULL"),
    ("schedule_class_versions", "utilisation_percent", "85.0"),
    ("schedule_class_versions", "review_status", "NULL"),
    ("team_schedule_assignments", "effective_from", "'2026-12-01'"),
    ("team_schedule_assignments", "review_status", "NULL"),
    ("personnel_schedule_exceptions", "valid_from", "'2026-12-01'"),
    ("personnel_schedule_exceptions", "valid_to", "NULL"),
    ("personnel_schedule_exceptions", "work_percent", "50.0"),
];

#[derive(Debug, Clone)]
struct RawColumn {
    name: String,
    decl_type: String,
    notnull: bool,
    has_default: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CreatedRow {
    pub entity_type: &'static str,
    pub entity_sync_id: String,
    pub locator: RowLocator,
    pub rowid: i64,
    /// Payload as built on the creating device (natural key fields included).
    pub payload: serde_json::Map<String, serde_json::Value>,
}

#[derive(Default)]
pub(crate) struct RowFactory {
    counter: i64,
    /// One shared parent row per entity type (local rowid).
    parents: HashMap<String, i64>,
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// First allowed value of `<col> IN (...)` and `<col> BETWEEN a AND ...` checks of a table.
fn check_values(table_sql: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let in_list = regex::Regex::new(r"(?i)\b([a-z_][a-z0-9_]*)\s+IN\s*\(([^()]*)\)").expect("regex");
    for caps in in_list.captures_iter(table_sql) {
        let column = caps[1].to_ascii_lowercase();
        if let Some(first) = caps[2].split(',').next() {
            out.entry(column).or_insert_with(|| first.trim().to_string());
        }
    }
    let between = regex::Regex::new(r"(?i)\b([a-z_][a-z0-9_]*)\s+BETWEEN\s+(-?[0-9.]+)\s+AND").expect("regex");
    for caps in between.captures_iter(table_sql) {
        out.entry(caps[1].to_ascii_lowercase())
            .or_insert_with(|| caps[2].to_string());
    }
    out
}

fn looks_like_time(name: &str) -> bool {
    name.ends_with("_at")
        || name.ends_with("_date")
        || name.ends_with("_from")
        || name.ends_with("_to")
        || name.ends_with("_until")
        || name.ends_with("_start")
        || name.ends_with("_end")
        || name.contains("deadline")
        || name.starts_with("period_")
        || name.ends_with("_datetime")
}

impl RowFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// A factory for one of several devices that sync together: its generated values (codes,
    /// names) never repeat those of the other devices' factories, as distinct people's would not.
    pub fn for_device(device_index: i64) -> Self {
        Self {
            counter: device_index * 1_000_000,
            ..Self::default()
        }
    }

    /// Create a new row of `entity_type` with every replicated column filled (references point to
    /// shared parent rows created on demand). Capture must be installed on `db`.
    pub async fn create(&mut self, db: &DatabaseConnection, entity_type: &str) -> Result<CreatedRow, String> {
        let rowid = self.insert(db, entity_type, 0).await?;
        created_row(db, entity_type, rowid).await
    }

    fn parent<'a>(
        &'a mut self,
        db: &'a DatabaseConnection,
        entity_type: &'a str,
        ordinal: usize,
        depth: u32,
    ) -> BoxFuture<'a, Result<i64, String>> {
        Box::pin(async move {
            // Parents of parents are separate rows: rows of one table that share all their
            // parents could break a unique key.
            let key = format!("{entity_type}#{ordinal}@{depth}");
            if let Some(id) = self.parents.get(&key) {
                return Ok(*id);
            }
            let rowid = self.insert(db, entity_type, depth).await?;
            self.parents.insert(key, rowid);
            Ok(rowid)
        })
    }

    fn insert<'a>(
        &'a mut self,
        db: &'a DatabaseConnection,
        entity_type: &'a str,
        depth: u32,
    ) -> BoxFuture<'a, Result<i64, String>> {
        Box::pin(async move {
            if depth > 12 {
                return Err(format!("{entity_type}: reference chain too deep"));
            }
            let def = registry::require_entity(entity_type).map_err(|e| e.to_string())?;
            let table = def.table.to_string();
            let raw: Vec<RawColumn> = db
                .query_all(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("PRAGMA table_info('{table}')"),
                ))
                .await
                .map_err(|e| e.to_string())?
                .iter()
                .map(|r| RawColumn {
                    name: r.try_get("", "name").unwrap_or_default(),
                    decl_type: r.try_get::<String>("", "type").unwrap_or_default().to_ascii_uppercase(),
                    notnull: r.try_get::<i64>("", "notnull").unwrap_or(0) == 1,
                    has_default: r.try_get::<Option<String>>("", "dflt_value").ok().flatten().is_some(),
                })
                .collect();
            let table_sql = scalar_opt_text(
                db,
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
                vec![table.clone().into()],
            )
            .await
            .unwrap_or_default();
            let checks = check_values(&table_sql);
            let columns = factory_columns(db, entity_type, &raw).await?;

            // column -> SQL expression, with bound values in order of appearance.
            let mut assigned: Vec<(String, String, Option<SqlValue>)> = Vec::new();
            let mut forced_types: HashMap<String, &'static str> = HashMap::new();
            // Two references to the same entity type in one row get two parent rows (some tables
            // require them to differ, e.g. an article and its equivalent).
            let mut ordinals: HashMap<&'static str, usize> = HashMap::new();
            for (position, column) in columns.iter().enumerate() {
                let raw_col = raw.iter().find(|c| c.name == column.name).ok_or("column vanished")?;
                self.counter += 1;
                let n = self.counter;
                if let Some((_, _, literal)) = VALUE_OVERRIDES
                    .iter()
                    .find(|(e, c, _)| *e == entity_type && *c == column.name)
                {
                    assigned.push((column.name.clone(), (*literal).to_string(), None));
                    continue;
                }
                if let FactoryKind::Derived(initial) = &column.kind {
                    assigned.push((column.name.clone(), (*initial).to_string(), None));
                    continue;
                }
                let value: Option<SqlValue> = match &column.kind {
                    FactoryKind::Skip | FactoryKind::Derived(_) => None,
                    FactoryKind::Identity if raw_col.notnull && !raw_col.has_default => {
                        Some(uuid::Uuid::new_v4().to_string().into())
                    }
                    FactoryKind::Identity => None,
                    FactoryKind::Value => Some(value_for(raw_col, &checks, n, position)),
                    FactoryKind::Catalog(catalog) => Some(
                        scalar_i64(db, &format!("SELECT id FROM \"{catalog}\" ORDER BY id LIMIT 1"), vec![])
                            .await
                            .into(),
                    ),
                    FactoryKind::Entity(target) => {
                        // An optional link to an entity type not on the generic engine yet would
                        // never arrive on the other device: left empty.
                        let not_generic = super::row_spec::row_spec(target).is_none();
                        if (*target == entity_type && depth > 0) || (!raw_col.notnull && (depth >= 6 || not_generic)) {
                            None
                        } else {
                            let ordinal = *ordinals.entry(*target).and_modify(|o| *o += 1).or_insert(0);
                            let id = self.parent(db, target, ordinal, depth + 1).await?;
                            Some(if column.is_text {
                                id.to_string().into()
                            } else {
                                id.into()
                            })
                        }
                    }
                    FactoryKind::Poly { target, .. }
                        if !raw_col.notnull && super::row_spec::row_spec(target).is_none() =>
                    {
                        None
                    }
                    FactoryKind::Poly {
                        type_column,
                        type_value,
                        target,
                    } => {
                        forced_types.insert(type_column.clone(), type_value);
                        let ordinal = *ordinals.entry(*target).and_modify(|o| *o += 1).or_insert(0);
                        let id = self.parent(db, target, ordinal, depth + 1).await?;
                        Some(if column.is_text {
                            id.to_string().into()
                        } else {
                            id.into()
                        })
                    }
                };
                if let Some(v) = value {
                    assigned.push((column.name.clone(), "?".into(), Some(v)));
                }
            }
            for (type_column, type_value) in forced_types {
                assigned.retain(|(c, _, _)| *c != type_column);
                assigned.push((type_column, "?".into(), Some(type_value.to_string().into())));
            }
            let cols: Vec<String> = assigned.iter().map(|(c, _, _)| format!("\"{c}\"")).collect();
            let exprs: Vec<String> = assigned.iter().map(|(_, e, _)| e.clone()).collect();
            let values: Vec<SqlValue> = assigned.into_iter().filter_map(|(_, _, v)| v).collect();
            let sql = format!(
                "INSERT INTO \"{table}\" ({}) VALUES ({})",
                cols.join(", "),
                exprs.join(", ")
            );
            db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, &sql, values))
                .await
                .map_err(|e| format!("{table}: {e}\n  {sql}"))?;
            Ok(scalar_i64(db, "SELECT last_insert_rowid()", vec![]).await)
        })
    }
}

fn is_row_target(entity: &str) -> bool {
    entity != super::row_spec::NOT_A_ROW && entity != super::row_spec::LOCAL_ROW
}

#[derive(Debug, Clone)]
enum FactoryKind {
    Skip,
    /// Required derived column: the SQL initial value the materializer would write.
    Derived(&'static str),
    Identity,
    Value,
    Entity(&'static str),
    Catalog(&'static str),
    Poly {
        type_column: String,
        type_value: &'static str,
        target: &'static str,
    },
}

#[derive(Debug, Clone)]
struct FactoryColumn {
    name: String,
    is_text: bool,
    kind: FactoryKind,
}

/// How to fill each column: from the sync layout for entities on the generic engine, else from
/// the declared foreign keys (parents of entities not converted yet).
async fn factory_columns(
    db: &DatabaseConnection,
    entity_type: &str,
    raw: &[RawColumn],
) -> Result<Vec<FactoryColumn>, String> {
    if super::row_spec::row_spec(entity_type).is_some() {
        let layout = row_layout::layout(db, entity_type).await.map_err(|e| e.to_string())?;
        return Ok(layout
            .columns
            .iter()
            .map(|c| {
                let raw_col = raw.iter().find(|r| r.name == c.name);
                let kind = match &c.role {
                    Role::Identity => FactoryKind::Identity,
                    Role::Derived { initial } if raw_col.is_some_and(|r| r.notnull && !r.has_default) => {
                        FactoryKind::Derived(initial)
                    }
                    Role::LocalKey | Role::LocalVersion | Role::Derived { .. } => FactoryKind::Skip,
                    Role::DeviceLocal if raw_col.is_some_and(|r| !r.notnull || r.has_default) => FactoryKind::Skip,
                    Role::DeviceLocal | Role::Value { .. } => FactoryKind::Value,
                    Role::Ref { target, .. } => match target {
                        Target::Unmapped => FactoryKind::Skip,
                        Target::Catalog { table, .. } => FactoryKind::Catalog(table),
                        Target::Entity { entity_type, .. } => FactoryKind::Entity(entity_type),
                        Target::Poly { type_column, targets } => {
                            // Prefer a target on the generic engine, so the parent travels too.
                            let (type_value, target, _) = targets
                                .iter()
                                .find(|(_, t, _)| is_row_target(t) && super::row_spec::row_spec(t).is_some())
                                .or_else(|| targets.iter().find(|(_, t, _)| is_row_target(t)))
                                .unwrap_or(&targets[0]);
                            FactoryKind::Poly {
                                type_column: (*type_column).to_string(),
                                type_value,
                                target,
                            }
                        }
                    },
                };
                FactoryColumn {
                    name: c.name.clone(),
                    is_text: c.is_text,
                    kind,
                }
            })
            .collect());
    }
    let table = registry::require_entity(entity_type).map_err(|e| e.to_string())?.table;
    let mut declared: HashMap<String, String> = HashMap::new();
    for fk in db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA foreign_key_list('{table}')"),
        ))
        .await
        .map_err(|e| e.to_string())?
    {
        let from: String = fk.try_get("", "from").unwrap_or_default();
        let target: String = fk.try_get("", "table").unwrap_or_default();
        let to: Option<String> = fk.try_get("", "to").ok().flatten();
        if to.as_deref().is_none_or(|t| t == "id") {
            declared.insert(from, target);
        }
    }
    Ok(raw
        .iter()
        .map(|c| {
            let kind = if c.name == "id" || c.name == "row_version" {
                FactoryKind::Skip
            } else if c.name == "entity_sync_id" {
                FactoryKind::Identity
            } else if let Some(target) = declared.get(&c.name) {
                match registry::SYNC_ENTITIES.iter().find(|e| e.table == target) {
                    Some(e) => FactoryKind::Entity(e.entity_type),
                    None => FactoryKind::Skip,
                }
            } else if c.name.ends_with("_id") && c.decl_type.contains("INT") {
                // An undeclared reference of an entity not converted yet: left empty.
                FactoryKind::Skip
            } else {
                FactoryKind::Value
            };
            FactoryColumn {
                name: c.name.clone(),
                is_text: c.decl_type.contains("TEXT") || c.decl_type.contains("CHAR"),
                kind,
            }
        })
        .collect())
}

fn value_for(column: &RawColumn, checks: &HashMap<String, String>, n: i64, position: usize) -> SqlValue {
    let name = column.name.to_ascii_lowercase();
    if let Some(allowed) = checks.get(&name) {
        let trimmed = allowed.trim();
        if let Some(text) = trimmed.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
            return text.to_string().into();
        }
        if let Ok(i) = trimmed.parse::<i64>() {
            return i.into();
        }
        if let Ok(f) = trimmed.parse::<f64>() {
            return f.into();
        }
    }
    let t = &column.decl_type;
    if t.contains("INT") {
        if name == "is_default" || name == "is_primary" {
            // Usually unique per parent (one default location per warehouse).
            return 0_i64.into();
        }
        if name.starts_with("is_")
            || name.starts_with("has_")
            || name.starts_with("can_")
            || name.starts_with("requires_")
        {
            return 1_i64.into();
        }
        return n.into();
    }
    if t.contains("REAL")
        || t.contains("FLOA")
        || t.contains("DOUB")
        || t.contains("NUM")
        || t.contains("DEC")
        || t == "D"
    {
        #[allow(clippy::cast_precision_loss)]
        return (n as f64 + 0.5).into();
    }
    if t.contains("BLOB") {
        return SqlValue::Bytes(Some(Box::new(vec![1, 2, 3, u8::try_from(n % 250).unwrap_or(0)])));
    }
    if looks_like_time(&name) {
        // Later columns get later times, so "start before end" constraints hold.
        let minute = 10 + position;
        return format!("2026-03-01T08:{minute:02}:00Z").into();
    }
    if name.ends_with("_json") {
        return "{}".to_string().into();
    }
    format!("{name}-{n}").into()
}

/// Locator and payload of the row with local `rowid`.
pub(crate) async fn created_row(db: &DatabaseConnection, entity_type: &str, rowid: i64) -> Result<CreatedRow, String> {
    let layout = row_layout::layout(db, entity_type).await.map_err(|e| e.to_string())?;
    let table = layout.table();
    let locator = match &layout.natural_pk {
        None => RowLocator::Identity(
            scalar_opt_text(
                db,
                &format!("SELECT entity_sync_id FROM \"{table}\" WHERE rowid = ?"),
                vec![rowid.into()],
            )
            .await
            .ok_or_else(|| format!("{table}: row {rowid} has no entity_sync_id"))?,
        ),
        Some(pk) => {
            let mut parts = Vec::new();
            for &i in pk {
                let name = layout.columns[i].name.clone();
                let row = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        &format!("SELECT \"{name}\" AS v FROM \"{table}\" WHERE rowid = ?"),
                        [rowid.into()],
                    ))
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("row vanished")?;
                let value: SqlValue = match row.try_get::<i64>("", "v") {
                    Ok(i) => i.into(),
                    Err(_) => row.try_get::<String>("", "v").map_err(|e| e.to_string())?.into(),
                };
                parts.push((name, value));
            }
            RowLocator::Natural(parts)
        }
    };
    let payload = row_codec::load_row_payload(db, &layout, &locator)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{table}: created row is not readable"))?;
    Ok(CreatedRow {
        entity_type: layout.def.entity_type,
        entity_sync_id: payload.entity_sync_id,
        locator,
        rowid,
        payload: payload.payload,
    })
}

/// Payload of an entity as stored on `db`, or `None` when the row is not there. Natural-key rows
/// are found through `natural_key` (a payload holding the key fields).
pub(crate) async fn current_payload(
    db: &DatabaseConnection,
    entity_type: &str,
    entity_sync_id: &str,
    natural_key: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let layout = row_layout::layout(db, entity_type).await.map_err(|e| e.to_string())?;
    let locator = match &layout.natural_pk {
        None => Some(RowLocator::Identity(entity_sync_id.to_string())),
        Some(_) => row_codec::natural_locator(db, &layout, natural_key)
            .await
            .map_err(|e| e.to_string())?,
    };
    let Some(locator) = locator else {
        return Ok(None);
    };
    Ok(row_codec::load_row_payload(db, &layout, &locator)
        .await
        .map_err(|e| e.to_string())?
        .map(|p| p.payload))
}

/// Value columns that a test may change to produce an update (text, no check constraint, not
/// part of a natural key).
pub(crate) async fn editable_text_column(db: &DatabaseConnection, entity_type: &str) -> Option<String> {
    let layout = row_layout::layout(db, entity_type).await.ok()?;
    let table_sql = scalar_opt_text(
        db,
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
        vec![layout.table().to_string().into()],
    )
    .await
    .unwrap_or_default();
    let checks = check_values(&table_sql);
    let unique_cols: HashSet<String> = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT ii.name AS name FROM pragma_index_list('{t}') il, pragma_index_info(il.name) ii WHERE il.\"unique\" = 1",
                t = layout.table()
            ),
        ))
        .await
        .ok()?
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect();
    let pk: HashSet<usize> = layout.natural_pk.clone().unwrap_or_default().into_iter().collect();
    let type_columns: HashSet<&str> = layout
        .columns
        .iter()
        .filter_map(|c| match &c.role {
            Role::Ref {
                target: Target::Poly { type_column, .. },
                ..
            } => Some(*type_column),
            _ => None,
        })
        .collect();
    layout
        .columns
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            matches!(c.role, Role::Value { .. })
                && c.is_text
                && !pk.contains(i)
                && !checks.contains_key(&c.name.to_ascii_lowercase())
                && !VALUE_OVERRIDES
                    .iter()
                    .any(|(e, col, _)| *e == entity_type && *col == c.name)
                && !unique_cols.contains(&c.name)
                && !looks_like_time(&c.name)
                && !c.name.ends_with("_json")
                && !type_columns.contains(c.name.as_str())
                && !c.name.ends_with("_type")
                && !c.name.ends_with("status")
        })
        .map(|(_, c)| c.name.clone())
        .next()
}
