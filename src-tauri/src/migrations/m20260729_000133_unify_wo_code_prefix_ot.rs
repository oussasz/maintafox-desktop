//! Unify work order business codes on `OT-NNNN`.
//!
//! Historical generators used `WOR-*`, `WO-DEMO-*`, `GEN-WO-*`, and `RAMS-*`.
//! All non-canonical codes are remapped into the next free `OT-NNNN` slots
//! without colliding with existing `OT-*` rows.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260729_000133_unify_wo_code_prefix_ot"
    }
}

fn is_canonical_ot_code(code: &str) -> bool {
    let Some(rest) = code.strip_prefix("OT-") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

fn parse_ot_seq(code: &str) -> Option<i64> {
    if !is_canonical_ot_code(code) {
        return None;
    }
    code.strip_prefix("OT-")?.parse().ok()
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Preserve injector exclusion after remap (filters key off this marker).
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE work_orders
             SET description = CASE
               WHEN COALESCE(description, '') LIKE '%[rams_injector]%' THEN description
               ELSE TRIM(COALESCE(description, '') || ' [rams_injector]')
             END
             WHERE code LIKE 'RAMS-INJECT%'"
                .to_string(),
        ))
        .await?;

        let rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id, code FROM work_orders ORDER BY id ASC".to_string(),
            ))
            .await?;

        let mut max_seq: i64 = 0;
        let mut remap: Vec<(i64, String)> = Vec::new();

        for row in rows {
            let id: i64 = row.try_get("", "id")?;
            let code: String = row.try_get("", "code")?;
            if let Some(seq) = parse_ot_seq(&code) {
                max_seq = max_seq.max(seq);
            } else {
                remap.push((id, code));
            }
        }

        if remap.is_empty() {
            return Ok(());
        }

        // Phase 1: free UNIQUE(code) with per-id temps.
        for (id, _) in &remap {
            let temp = format!("__MIG_WO_{id}");
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE work_orders SET code = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
                [Value::from(temp), Value::from(*id)],
            ))
            .await?;
        }

        // Phase 2: assign canonical OT-NNNN and patch embedded JSON audit payloads.
        for (id, old_code) in remap {
            max_seq += 1;
            let new_code = format!("OT-{max_seq:04}");

            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE work_orders SET code = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
                [Value::from(new_code.clone()), Value::from(id)],
            ))
            .await?;

            // Best-effort: DI audit JSON / summaries that stored the old code.
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE di_change_events
                     SET details_json = REPLACE(details_json, ?, ?),
                         summary = REPLACE(COALESCE(summary, ''), ?, ?)
                     WHERE details_json LIKE ? OR summary LIKE ?",
                    [
                        Value::from(old_code.clone()),
                        Value::from(new_code.clone()),
                        Value::from(old_code.clone()),
                        Value::from(new_code.clone()),
                        Value::from(format!("%{old_code}%")),
                        Value::from(format!("%{old_code}%")),
                    ],
                ))
                .await;

            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE activity_events
                     SET summary_json = REPLACE(summary_json, ?, ?)
                     WHERE summary_json LIKE ?",
                    [
                        Value::from(old_code.clone()),
                        Value::from(new_code.clone()),
                        Value::from(format!("%{old_code}%")),
                    ],
                ))
                .await;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Irreversible data remap (lossy: original prefixes discarded).
        Ok(())
    }
}
