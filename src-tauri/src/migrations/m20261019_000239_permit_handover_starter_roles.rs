//! Migration 239 — the plant's hand-over roles list starts with three roles to review (owner
//! decision).
//!
//! `PERMIT.HANDOVER_ROLES` (migration 235) was shipped empty, so no hand-over could be recorded
//! until an administrator filled it. It now receives three starter roles — Maintenance,
//! Exploitation, HSE — flagged "à revoir" (`review_required`, `origin = "starter"`), which the
//! administrator renames, deactivates or completes in Données de référence
//! (`permit::handover_roles::seed_starter_roles`). Seeded once, here: only into a list that holds no
//! value an administrator added (roles adopted from old free text do not count), each with the same
//! seeded sync identity on every PC, and never over an existing value — an administrator's edit is
//! never overwritten and a role removed later is not brought back.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000239_permit_handover_starter_roles"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        crate::permit::handover_roles::seed_starter_roles(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Reference data the administrator may have edited since: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;
    use crate::permit::handover_roles::{list_handover_roles, seed_starter_roles, DOMAIN};

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn roles(db: &DatabaseConnection) -> Vec<(String, String, bool)> {
        list_handover_roles(db)
            .await
            .expect("roles")
            .into_iter()
            .map(|r| (r.code, r.label, r.review_required))
            .collect()
    }

    /// A database migrated up to 238 (before this step).
    async fn before_239() -> DatabaseConnection {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == "m20261019_000239_permit_handover_starter_roles")
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 238");
        db
    }

    async fn add_value(db: &DatabaseConnection, code: &str, meta: Option<&str>) {
        exec(
            db,
            &format!(
                "INSERT INTO reference_values (set_id, code, label, sort_order, is_active, metadata_json) \
                 SELECT rs.id, '{code}', '{code}', 5, 1, {} FROM reference_sets rs \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = '{DOMAIN}' AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
                meta.map_or_else(|| "NULL".to_string(), |m| format!("'{m}'"))
            ),
        )
        .await;
    }

    /// The list starts with the three starter roles, flagged "à revoir", in their order.
    #[tokio::test]
    async fn the_list_starts_with_three_roles_to_review() {
        let db = before_239().await;
        Migrator::up(&db, None).await.expect("239");
        assert_eq!(
            roles(&db).await,
            [
                ("Maintenance".to_string(), "Maintenance".to_string(), true),
                ("Exploitation".to_string(), "Exploitation".to_string(), true),
                ("HSE".to_string(), "HSE".to_string(), true),
            ]
        );
    }

    /// Seeding again never overwrites what the administrator changed and never brings back a role
    /// that was taken out of the list.
    #[tokio::test]
    async fn seeding_again_keeps_the_administrators_edits() {
        let db = before_239().await;
        Migrator::up(&db, None).await.expect("239");
        exec(
            &db,
            "UPDATE reference_values SET label = 'Hygiène, sécurité, environnement', metadata_json = '{}' \
             WHERE code = 'HSE'",
        )
        .await;
        exec(
            &db,
            "UPDATE reference_values SET is_active = 0 WHERE code = 'Maintenance'",
        )
        .await;
        exec(
            &db,
            "UPDATE reference_values SET code = 'Exploitation (jour)', label = 'Exploitation (jour)' \
             WHERE code = 'Exploitation'",
        )
        .await;
        seed_starter_roles(&db).await.expect("again");
        assert_eq!(
            roles(&db).await,
            [
                (
                    "Exploitation (jour)".to_string(),
                    "Exploitation (jour)".to_string(),
                    true
                ),
                ("HSE".to_string(), "Hygiène, sécurité, environnement".to_string(), false),
            ]
        );
    }

    /// A list the plant already filled is left alone; roles adopted from old free text do not count
    /// as filled, and a starter already there under another case is not added twice.
    #[tokio::test]
    async fn only_a_list_without_the_plants_own_roles_is_seeded() {
        let db = before_239().await;
        add_value(&db, "Chef de poste", None).await;
        Migrator::up(&db, None).await.expect("239");
        assert_eq!(
            roles(&db).await,
            [("Chef de poste".to_string(), "Chef de poste".to_string(), false)]
        );

        let db = before_239().await;
        add_value(&db, "hse", Some(r#"{"origin":"migrated","review_required":true}"#)).await;
        Migrator::up(&db, None).await.expect("239");
        let codes: Vec<String> = roles(&db).await.into_iter().map(|r| r.0).collect();
        assert_eq!(codes, ["Maintenance", "Exploitation", "hse"]);
    }
}
