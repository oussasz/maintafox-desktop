//! Migration 235 — a permit hand-over records who signed it and its roles come from a list.
//!
//! - `permit_handover_logs.signed_by_id`: the person who recorded the hand-over, set by the server
//!   from the session (never from the screen). `received_by_id`: the receiving person, optional,
//!   picked from the user accounts. Both nullable: hand-overs recorded before keep NULL — nobody is
//!   invented after the fact. Both sync with the row as user references.
//! - `PERMIT.HANDOVER_ROLES` (COMPANY list, Données de référence) is created **empty**. The free
//!   texts already stored as "from" / "to" roles are adopted with their exact text and flagged
//!   "à revoir"; no stored hand-over is rewritten (`permit::handover_roles`, same pattern as 229).
//!   The integrity pass runs the same adoption for texts received later by sync.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000235_permit_handover_signer_roles"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("signed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("received_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('permit_handover_logs') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("ALTER TABLE permit_handover_logs ADD COLUMN {column} {ddl}"),
                ))
                .await?;
            }
        }
        crate::permit::handover_roles::adopt_stored_roles(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Signers and adopted roles describe stored data: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    async fn text(db: &DatabaseConnection, sql: &str) -> Option<String> {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
    }

    /// On a database from before 235, the free-text roles already stored are adopted into the list
    /// with their exact text and flagged "à revoir"; no stored hand-over is rewritten and none gets
    /// a signer or receiver.
    #[tokio::test]
    async fn stored_free_text_roles_are_adopted_flagged_and_nothing_is_rewritten() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == "m20261018_000235_permit_handover_signer_roles")
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 234");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
             confirmation_note, signed_at, row_version) VALUES \
             ('ho-before-235-a', 1, 'Équipe nuit', 'Exploitation', 'Zone propre', '2026-01-01T00:00:00Z', 1), \
             ('ho-before-235-b', 1, 'Exploitation', 'Équipe nuit', '', '2026-01-02T00:00:00Z', 2)"
                .to_string(),
        ))
        .await
        .expect("hand-overs recorded before 235");
        let stored = "SELECT group_concat(entity_sync_id || '|' || permit_id || '|' || handed_from_role || '|' || \
             handed_to_role || '|' || confirmation_note || '|' || signed_at || '|' || row_version, ';') \
             FROM (SELECT * FROM permit_handover_logs ORDER BY id)";
        let before = text(&db, stored).await;
        assert!(before.is_some());

        Migrator::up(&db, None).await.expect("235 and later");

        assert_eq!(text(&db, stored).await, before, "no stored hand-over is rewritten");
        assert_eq!(
            text(
                &db,
                "SELECT CAST(COUNT(*) AS TEXT) FROM permit_handover_logs \
                 WHERE signed_by_id IS NOT NULL OR received_by_id IS NOT NULL"
            )
            .await
            .as_deref(),
            Some("0"),
            "no signer or receiver invented"
        );
        let roles = crate::permit::handover_roles::list_handover_roles(&db)
            .await
            .expect("roles");
        let codes: Vec<_> = roles.iter().map(|r| (r.code.as_str(), r.review_required)).collect();
        // The starter roles of migration 239 join them ("Exploitation" is already there).
        assert_eq!(
            codes,
            [
                ("Maintenance", true),
                ("HSE", true),
                ("Exploitation", true),
                ("Équipe nuit", true)
            ]
        );
    }
}
