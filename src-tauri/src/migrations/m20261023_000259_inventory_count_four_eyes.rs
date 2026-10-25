//! Migration 259 — four-eyes on stock count variances above the plant's tolerance (D11, CMMS
//! Advisor decision register 2026-10-22/23).
//!
//! - `inventory_count_lines.counted_by_id` (synced, a user reference): the person who entered the
//!   counted quantity, taken from the signed-in session. A variance above the tolerance is approved
//!   by someone else (`inventory::count_four_eyes`). Lines entered before this version have none.
//! - `inventory_count_lines.self_approved` (synced, 0/1): the counter approved a variance above the
//!   tolerance under the four-eyes opt-out of the plant's only site ("auto-approbation").
//! - The tolerance is now the pair of optional plant settings
//!   `inventory.count_four_eyes_value_threshold` / `inventory.count_four_eyes_percent_threshold`
//!   (not seeded: blank until the plant sets them). The former `inventory.count_critical_variance`
//!   (a quantity, seeded 5 by migration 232) is retired: its last value is written to the settings
//!   change history (`settings_change_events`, kept with its value as a retired plant parameter,
//!   `settings::catalog::RETIRED_PLANT_PARAMETERS`), then its row is deleted. Both on every PC with
//!   capture suppressed, as migration 232 seeded it: the history row has the same seeded identity
//!   on every PC and nothing is sent.
//! - `inventory_count_sessions.critical_abs_threshold` was `REAL NOT NULL DEFAULT 5`: existing
//!   sessions keep the value they were opened with (history only); new sessions record none. SQLite
//!   cannot drop NOT NULL: the table is rebuilt as migration 254 rebuilt `pm_plan_versions` (new
//!   table with the column nullable, every row copied as it is, ids included, old table dropped,
//!   new one renamed, its AUTOINCREMENT counter kept, its indexes and triggers — capture triggers
//!   included — created again from their own SQL; foreign keys off and `legacy_alter_table` on
//!   during the rebuild only, so the count lines still point to the same sessions). A table already
//!   nullable there is left alone; a table whose definition is not the one expected is refused
//!   (nothing is guessed).
//! - `inventory_count_lines.second_approved_by_id` (synced, a user reference),
//!   `second_approved_at`, `second_approval_note`: the second approval a different person gives,
//!   on a PC where the count is applied, to a variance above the tolerance its counter approved
//!   ("auto-approbation"), so that a PC where four-eyes applies accepts the count it holds back
//!   (`inventory::count_four_eyes`, CMMS Advisor Q8).
//! - `inventory_count_lines.entry_unit_cost`, `entry_variance_value` (synced): the location's unit
//!   cost read when the counted quantity was entered and the variance's value at that cost
//!   (variance quantity × unit cost; both blank when the article had no unit cost there). A PC
//!   receiving the line judges "above the tolerance" with that cost when it is consistent and close
//!   to its own, so a cost that drifted between PCs does not make a legitimate approval a conflict
//!   (`inventory::count_four_eyes`, CMMS Advisor Q10). Lines entered before have none.
//! - `inventory_count_lines.critique_raised_version` (device-local, never sent: `sync::row_spec`):
//!   the row version at which this PC raised the line's "above the tolerance" flag itself, from a
//!   received version that said "within" (`count_four_eyes::store_judged_critique`). Only such a
//!   flag, while the line has not changed here since (every change bumps `row_version`), may be
//!   cleared again by a later received version without a new entry; a flag entered here never.
//! - The notification category `stock_count_second_approval` and its rule (audience
//!   `permission:inv.manage`): the count-approval holders are told once per count held back here.
//! - The notification category `stock_count_recount` and its rule (audience `assignee`: the counter
//!   of the count line, `notifications::router`): a count from the stock-to-reconcile list whose
//!   approval is refused because the stock moved since it tells its counter to count again
//!   (`inventory::negative_stock`, CMMS Advisor Q7). Seeded identities, as migration 251.

use regex::Regex;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261023_000259_inventory_count_four_eyes"
    }
}

/// The retired setting (migration 232).
pub(crate) const RETIRED_SETTING: &str = "inventory.count_critical_variance";

const SESSIONS: &str = "inventory_count_sessions";

async fn has_column<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

async fn texts(conn: &impl ConnectionTrait, sql: &str) -> Result<Vec<String>, DbErr> {
    conn.query_all(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [SESSIONS.into()],
    ))
    .await?
    .iter()
    .map(|r| r.try_get_by_index::<String>(0))
    .collect()
}

async fn capture_present(conn: &impl ConnectionTrait) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'".to_string(),
        ))
        .await?
        .is_some())
}

/// The session table without NOT NULL on `critical_abs_threshold` (see the module documentation).
async fn threshold_nullable<C: ConnectionTrait + TransactionTrait>(db: &C) -> Result<(), DbErr> {
    let not_null = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT \"notnull\" AS nn FROM pragma_table_info(?) WHERE name = 'critical_abs_threshold'",
            [SESSIONS.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "nn"))
        .transpose()?;
    if not_null != Some(1) {
        return Ok(());
    }
    let Some(create_sql) = texts(db, "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?")
        .await?
        .into_iter()
        .next()
    else {
        return Ok(());
    };
    let column = Regex::new(r"(?i)critical_abs_threshold\s+REAL\s+NOT\s+NULL\s+DEFAULT\s+5\b").expect("regex");
    let name =
        Regex::new(r#"(?i)^\s*CREATE\s+TABLE\s+(IF\s+NOT\s+EXISTS\s+)?"?inventory_count_sessions"?"#).expect("regex");
    if column.find_iter(&create_sql).count() != 1 || !name.is_match(&create_sql) {
        return Err(DbErr::Custom(
            "migration 259: inventory_count_sessions is not defined as expected; its critical_abs_threshold \
             column is left as it is (nothing is guessed)"
                .into(),
        ));
    }
    let rebuilt_sql = name
        .replace(
            &column.replace(&create_sql, "critical_abs_threshold  REAL    NULL"),
            "CREATE TABLE inventory_count_sessions_rebuilt",
        )
        .into_owned();
    // Indexes and triggers of the table (the automatic index of a constraint has no SQL).
    let kept = texts(
        db,
        "SELECT sql FROM sqlite_master WHERE tbl_name = ? AND type IN ('index', 'trigger') AND sql IS NOT NULL
         ORDER BY type, name",
    )
    .await?;

    db.execute_unprepared("PRAGMA foreign_keys = OFF").await?;
    db.execute_unprepared("PRAGMA legacy_alter_table = ON").await?;
    let rebuilt = async {
        let txn = db.begin().await?;
        // The AUTOINCREMENT counter goes with the dropped table: kept, so the id of a deleted session
        // is never given again.
        let sequence: Option<i64> = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT seq FROM sqlite_sequence WHERE name = 'inventory_count_sessions'".to_string(),
            ))
            .await?
            .map(|r| r.try_get_by_index::<i64>(0))
            .transpose()?;
        txn.execute_unprepared(&rebuilt_sql).await?;
        txn.execute_unprepared("INSERT INTO inventory_count_sessions_rebuilt SELECT * FROM inventory_count_sessions")
            .await?;
        txn.execute_unprepared("DROP TABLE inventory_count_sessions").await?;
        txn.execute_unprepared("ALTER TABLE inventory_count_sessions_rebuilt RENAME TO inventory_count_sessions")
            .await?;
        for sql in &kept {
            txn.execute_unprepared(sql).await?;
        }
        if let Some(seq) = sequence {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sqlite_sequence SET seq = MAX(seq, ?) WHERE name = 'inventory_count_sessions'",
                [seq.into()],
            ))
            .await?;
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO sqlite_sequence (name, seq) SELECT 'inventory_count_sessions', ?
                 WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'inventory_count_sessions')",
                [seq.into()],
            ))
            .await?;
        }
        // Every count line still names an existing session.
        let orphans = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) FROM pragma_foreign_key_check('inventory_count_lines')".to_string(),
            ))
            .await?
            .map(|r| r.try_get_by_index::<i64>(0))
            .transpose()?
            .unwrap_or(0);
        if orphans > 0 {
            return Err(DbErr::Custom(
                "migration 259: count lines would lose their session; nothing was changed".into(),
            ));
        }
        txn.commit().await
    }
    .await;
    db.execute_unprepared("PRAGMA legacy_alter_table = OFF").await?;
    db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
    rebuilt
}

/// A notice category of this migration and its rule (see the module documentation).
async fn notice_category<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    category: &str,
    label: &str,
    audience: &str,
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    // Made by this migration on every PC with the same identities: nothing is sent.
    let capture = capture_present(&txn).await?;
    if capture {
        crate::sync::capture::begin_suppression(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
    }
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT OR IGNORE INTO notification_categories \
         (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
          module_code, is_offered) VALUES (?, ?, ?, 'warning', 0, 1, 'inv', 1)",
        [
            crate::sync::identity::seeded_entity_sync_id("notification_categories", category).into(),
            category.into(),
            label.into(),
        ],
    ))
    .await?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO notification_rules \
         (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
          escalation_policy_id, is_active, lead_time_minutes, audiences_json) \
         SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, ? \
         WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
        [
            crate::sync::identity::seeded_entity_sync_id("notification_rules", category).into(),
            category.into(),
            serde_json::json!([audience]).to_string().into(),
            category.into(),
        ],
    ))
    .await?;
    if capture {
        crate::sync::capture::end_suppression(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
    }
    txn.commit().await
}

/// The retired setting's last value goes to the change history, then its row is deleted.
async fn retire_critical_variance<C: ConnectionTrait + TransactionTrait>(db: &C) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let capture = capture_present(&txn).await?;
    if capture {
        crate::sync::capture::begin_suppression(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
    }
    let last = txn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = ? AND setting_scope = 'tenant'",
            [RETIRED_SETTING.into()],
        ))
        .await?
        .map(|r| r.try_get_by_index::<String>(0))
        .transpose()?;
    if let Some(value) = last {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO settings_change_events
                 (entity_sync_id, setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                  changed_by_id, changed_at, required_step_up, apply_result, old_value_json, new_value_json)
             VALUES (?, ?, ?, ?, NULL, NULL, ?, 0, 'applied', ?, NULL)",
            [
                crate::sync::identity::seeded_entity_sync_id("settings_retirement", RETIRED_SETTING).into(),
                RETIRED_SETTING.into(),
                "Retired (migration 259): a count variance is critical when it is above the four-eyes tolerance \
                 (inventory.count_four_eyes_value_threshold / inventory.count_four_eyes_percent_threshold)"
                    .into(),
                crate::settings::sha256_hex(&value).into(),
                now.into(),
                value.into(),
            ],
        ))
        .await?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "DELETE FROM app_settings WHERE setting_key = ? AND setting_scope = 'tenant'",
            [RETIRED_SETTING.into()],
        ))
        .await?;
    }
    if capture {
        crate::sync::capture::end_suppression(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
    }
    txn.commit().await
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        if !has_column(db, "inventory_count_lines", "counted_by_id").await? {
            db.execute_unprepared(
                "ALTER TABLE inventory_count_lines ADD COLUMN counted_by_id INTEGER NULL REFERENCES user_accounts(id)",
            )
            .await?;
        }
        if !has_column(db, "inventory_count_lines", "self_approved").await? {
            db.execute_unprepared(
                "ALTER TABLE inventory_count_lines ADD COLUMN self_approved INTEGER NOT NULL DEFAULT 0",
            )
            .await?;
        }
        threshold_nullable(db).await?;
        for (column, definition) in [
            ("second_approved_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
            ("second_approved_at", "TEXT NULL"),
            ("second_approval_note", "TEXT NULL"),
            ("entry_unit_cost", "REAL NULL"),
            ("entry_variance_value", "REAL NULL"),
            ("critique_raised_version", "INTEGER NULL"),
        ] {
            if !has_column(db, "inventory_count_lines", column).await? {
                db.execute_unprepared(&format!(
                    "ALTER TABLE inventory_count_lines ADD COLUMN {column} {definition}"
                ))
                .await?;
            }
        }
        retire_critical_variance(db).await?;
        notice_category(
            db,
            crate::inventory::negative_stock::RECOUNT_CATEGORY,
            "Stock count to do again",
            "assignee",
        )
        .await?;
        notice_category(
            db,
            crate::inventory::count_four_eyes::SECOND_APPROVAL_CATEGORY,
            "Stock count held for a second approval",
            "permission:inv.manage",
        )
        .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The columns are nullable or defaulted and harmless when left; the retired setting's value
        // is kept in the change history.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::sync::test_support::{exec, scalar_i64, started_db_before_migration, RowFactory};

    async fn strings(db: &DatabaseConnection, sql: &str) -> Vec<String> {
        db.query_all(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).expect("text"))
            .collect()
    }

    /// Every row of the session table, every column, in id order.
    async fn sessions_snapshot(db: &DatabaseConnection) -> Vec<String> {
        let columns = strings(
            db,
            "SELECT name FROM pragma_table_info('inventory_count_sessions') ORDER BY cid",
        )
        .await;
        let quoted: Vec<String> = columns.iter().map(|c| format!("quote(\"{c}\")")).collect();
        strings(
            db,
            &format!(
                "SELECT {} FROM inventory_count_sessions ORDER BY id",
                quoted.join(" || '|' || ")
            ),
        )
        .await
    }

    #[tokio::test]
    async fn an_upgraded_database_keeps_its_sessions_and_retires_the_critical_variance_with_its_value() {
        let db = started_db_before_migration("m20261023_000259_inventory_count_four_eyes").await;
        exec(
            &db,
            "UPDATE app_settings SET setting_value_json = '3' WHERE setting_key = ? AND setting_scope = 'tenant'",
            vec![super::RETIRED_SETTING.into()],
        )
        .await;
        // Rows made before 259 (the count line layout of this version names columns 259 adds, so
        // the line is written by hand).
        let mut factory = RowFactory::new();
        factory.create(&db, "stock_locations").await.expect("a location");
        let session = factory
            .create(&db, "inventory_count_sessions")
            .await
            .expect("a count session");
        let article = factory.create(&db, "articles").await.expect("article");
        exec(
            &db,
            "INSERT INTO inventory_count_lines (entity_sync_id, session_id, article_id, warehouse_id, location_id,
                                                system_qty, counted_qty, variance_qty)
             SELECT 'line-before-259', s.id, ?, s.warehouse_id, l.id, 10, 8, -2
             FROM inventory_count_sessions s JOIN stock_locations l ON l.warehouse_id = s.warehouse_id
             WHERE s.id = ? LIMIT 1",
            vec![article.rowid.into(), session.rowid.into()],
        )
        .await;
        assert_eq!(
            scalar_i64(&db, "SELECT COUNT(*) FROM inventory_count_lines", vec![]).await,
            1,
            "a line of that session"
        );
        let rows_before = sessions_snapshot(&db).await;
        assert!(!rows_before.is_empty());
        let lines_before = strings(
            &db,
            "SELECT entity_sync_id || '|' || session_id || '|' || variance_qty FROM inventory_count_lines ORDER BY id",
        )
        .await;
        // A deleted session leaves the AUTOINCREMENT counter above every live id: it must stay so.
        exec(
            &db,
            "UPDATE sqlite_sequence SET seq = seq + 40 WHERE name = 'inventory_count_sessions'",
            vec![],
        )
        .await;
        let sequence_before = scalar_i64(
            &db,
            "SELECT seq FROM sqlite_sequence WHERE name = 'inventory_count_sessions'",
            vec![],
        )
        .await;
        let triggers_before = strings(
            &db,
            "SELECT name FROM sqlite_master WHERE type IN ('trigger', 'index') AND tbl_name = 'inventory_count_sessions'
             AND sql IS NOT NULL ORDER BY name",
        )
        .await;
        assert!(
            triggers_before.iter().any(|t| t.starts_with("sync_cap_")),
            "capture is installed"
        );
        let journal_before = scalar_i64(&db, "SELECT COUNT(*) FROM sync_capture_journal", vec![]).await;

        crate::migrations::Migrator::up(&db, None).await.expect("migration 259");

        assert_eq!(
            sessions_snapshot(&db).await,
            rows_before,
            "every session kept as it was"
        );
        assert_eq!(
            strings(
                &db,
                "SELECT entity_sync_id || '|' || session_id || '|' || variance_qty FROM inventory_count_lines ORDER BY id",
            )
            .await,
            lines_before,
            "every count line kept, on its session"
        );
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT seq FROM sqlite_sequence WHERE name = 'inventory_count_sessions'",
                vec![],
            )
            .await,
            sequence_before,
            "the AUTOINCREMENT counter is kept"
        );
        assert_eq!(
            strings(
                &db,
                "SELECT name FROM sqlite_master WHERE type IN ('trigger', 'index') AND tbl_name = 'inventory_count_sessions'
                 AND sql IS NOT NULL ORDER BY name",
            )
            .await,
            triggers_before,
            "indexes and triggers created again"
        );
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT \"notnull\" FROM pragma_table_info('inventory_count_sessions') WHERE name = 'critical_abs_threshold'",
                vec![],
            )
            .await,
            0
        );
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM pragma_foreign_key_check('inventory_count_lines')",
                vec![]
            )
            .await,
            0,
            "the count lines still name their sessions"
        );
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM app_settings WHERE setting_key = ?",
                vec![super::RETIRED_SETTING.into()],
            )
            .await,
            0,
            "the retired setting is gone"
        );
        let history = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT old_value_json, new_value_json FROM settings_change_events WHERE setting_key_or_domain = ?",
                [super::RETIRED_SETTING.into()],
            ))
            .await
            .expect("history")
            .expect("its retirement is recorded");
        assert_eq!(history.try_get_by_index::<String>(0).expect("old"), "3");
        assert_eq!(history.try_get_by_index::<Option<String>>(1).expect("new"), None);
        assert_eq!(
            scalar_i64(&db, "SELECT COUNT(*) FROM sync_capture_journal", vec![]).await,
            journal_before,
            "nothing is sent"
        );
        // The history keeps the value at the next startup (retired plant parameter).
        crate::settings::scrub_hash_only_history(&db).await.expect("scrub");
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM settings_change_events WHERE setting_key_or_domain = ? AND old_value_json = '3'",
                vec![super::RETIRED_SETTING.into()],
            )
            .await,
            1
        );
        // A new session records no tolerance; the line columns are there.
        exec(
            &db,
            "INSERT INTO inventory_count_sessions (session_code, warehouse_id, status) \
             SELECT 'CC-259', warehouse_id, 'draft' FROM inventory_count_sessions LIMIT 1",
            vec![],
        )
        .await;
        exec(
            &db,
            "UPDATE inventory_count_lines SET counted_by_id = NULL, self_approved = 0, entry_unit_cost = NULL,
                    entry_variance_value = NULL",
            vec![],
        )
        .await;
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM inventory_count_sessions WHERE session_code = 'CC-259' AND critical_abs_threshold IS NULL",
                vec![],
            )
            .await,
            1
        );

        // A second run changes nothing.
        crate::migrations::Migrator::down(&db, Some(1)).await.expect("down 259");
        crate::migrations::Migrator::up(&db, None).await.expect("259 again");
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM settings_change_events WHERE setting_key_or_domain = ?",
                vec![super::RETIRED_SETTING.into()],
            )
            .await,
            1
        );
    }
}
