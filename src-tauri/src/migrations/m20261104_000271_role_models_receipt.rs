//! Role models after the purchase approval / goods receipt split (L3-03, CMMS Advisor
//! 2026-11-02): the Buyer model holds `inv.procure` only (no approval, no receipt), the Storekeeper
//! model receives goods (`inv.receive`), the Maintenance Manager model approves within its limit,
//! and the Planner model keeps no purchasing or stock movement (Advisor, Planner option a).
//!
//! Model rows stored by migration 000267 hold the registry lists of that time. This migration
//! stores the registry's current lists of the models whose rights changed since then. Only
//! template rows change: no role, no grant is touched (a role made earlier from a model keeps its
//! rights, which the administrator adapts in the role editor).
//!
//! Made by this migration on every PC with the same result: nothing is sent to the sync.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::rbac::permissions::ROLE_TEMPLATES;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000271_role_models_receipt"
    }
}

/// Model rows whose list is replaced by the registry's.
pub(crate) const RESTORED_TEMPLATES: &[&str] = &[
    "Buyer Template",
    "Storekeeper Template",
    "Maintenance Manager Template",
    "Planner Template",
];

/// The stored description of the Buyer model no longer mentions goods receipts (the screens show
/// the translated description, keyed by the name).
const BUYER_DESCRIPTION: &str = "Purchase requisitions and orders";

/// The registry's rights of a template, as stored in `module_set_json`.
fn stored_rights(name: &str) -> Result<String, DbErr> {
    let (_, rights) = ROLE_TEMPLATES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| DbErr::Custom(format!("role template {name} is not in the permission registry")))?;
    serde_json::to_string(rights).map_err(|e| DbErr::Custom(e.to_string()))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let capture = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if capture {
            crate::sync::capture::begin_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        for name in RESTORED_TEMPLATES {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE role_templates \
                 SET module_set_json = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                 WHERE name = ? AND is_system = 1",
                [stored_rights(name)?.into(), (*name).into()],
            ))
            .await?;
        }
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE role_templates SET description = ? WHERE name = 'Buyer Template' AND is_system = 1",
            [BUYER_DESCRIPTION.into()],
        ))
        .await?;
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The current lists stay: the earlier lists are not kept (templates only, never roles).
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::permissions::{INV_APPROVE, INV_MANAGE, INV_PROCURE, INV_RECEIVE, INV_VIEW};

    fn rights(name: &str) -> &'static [&'static str] {
        ROLE_TEMPLATES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{name} is not in the registry"))
    }

    #[test]
    fn every_restored_model_is_in_the_registry() {
        for name in RESTORED_TEMPLATES {
            assert!(stored_rights(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn buyer_buys_without_approving_or_receiving() {
        let buyer = rights("Buyer Template");
        assert!(buyer.contains(&INV_PROCURE));
        assert!(!buyer.contains(&INV_APPROVE), "the buyer does not approve");
        assert!(!buyer.contains(&INV_RECEIVE), "receiving is not the buyer's");
    }

    #[test]
    fn storekeeper_receives_without_buying() {
        let storekeeper = rights("Storekeeper Template");
        assert!(storekeeper.contains(&INV_RECEIVE));
        assert!(!storekeeper.contains(&INV_PROCURE));
        assert!(!storekeeper.contains(&INV_APPROVE));
    }

    #[test]
    fn planner_keeps_no_purchasing_or_stock_movement() {
        let planner = rights("Planner Template");
        for kept_out in [INV_PROCURE, INV_APPROVE, INV_RECEIVE, INV_MANAGE] {
            assert!(!planner.contains(&kept_out), "Planner holds {kept_out}");
        }
        assert!(planner.contains(&INV_VIEW));
    }

    #[test]
    fn maintenance_manager_approves_purchases() {
        let manager = rights("Maintenance Manager Template");
        assert!(manager.contains(&INV_APPROVE));
        assert!(manager.contains(&INV_PROCURE));
    }
}
