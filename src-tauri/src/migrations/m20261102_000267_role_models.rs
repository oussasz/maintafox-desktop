//! Role models (CMMS Advisor decision L5-04): the templates offered under "Créer à partir d'un
//! modèle" when a role is created. A model is never a role: creating a role from it copies its
//! rights into the new role, which the administrator then adapts; no grant is linked to it.
//!
//! The rights of each model are those of the permission registry (`role_templates`, generated into
//! [`ROLE_TEMPLATES`]), the one list the screens also read. This migration:
//! - adds the rows of the models not stored yet (the six new models and the Finance Manager
//!   template, which was in the registry only), with the same identity on every PC;
//! - stores the trimmed lists of the Supervisor, Technician (least privilege, Advisor answer 8)
//!   and Planner templates (Advisor, option a: no emergency reassignment, purchasing or stock
//!   movements). Only template rows change: no role, no grant is touched.
//!
//! Made by this migration on every PC with the same result: nothing is sent to the sync.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::rbac::permissions::ROLE_TEMPLATES;
use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261102_000267_role_models"
    }
}

/// Template rows this migration adds: (name, stored description). The screens show the translated
/// name and description (`admin:roles.templates`), keyed by the name.
pub(crate) const ADDED_TEMPLATES: &[(&str, &str)] = &[
    ("Finance Manager Template", "Budget preparation, reporting and approval"),
    (
        "Storekeeper Template",
        "Store work: stock issues, returns, transfers and counts",
    ),
    ("Buyer Template", "Purchase requisitions and orders, goods receipts"),
    ("HSE Manager Template", "Work permits and lockout, safety follow-up"),
    (
        "Maintenance Manager Template",
        "Runs and signs off maintenance work and its programme",
    ),
    (
        "Cost Controller Template",
        "Reads and exports maintenance costs and budgets",
    ),
    (
        "Production Operator Template",
        "Reports problems on the machines of his area",
    ),
];

/// Stored templates whose list is replaced by the registry's (trimmed to least privilege).
pub(crate) const TRIMMED_TEMPLATES: &[&str] = &["Supervisor Template", "Technician Template", "Planner Template"];

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
        for (name, description) in ADDED_TEMPLATES {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT OR IGNORE INTO role_templates \
                 (entity_sync_id, name, description, module_set_json, is_system) \
                 VALUES (?, ?, ?, ?, 1)",
                [
                    seeded_entity_sync_id("role_templates", name).into(),
                    (*name).into(),
                    (*description).into(),
                    stored_rights(name)?.into(),
                ],
            ))
            .await?;
        }
        for name in TRIMMED_TEMPLATES {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE role_templates \
                 SET module_set_json = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                 WHERE name = ? AND is_system = 1",
                [stored_rights(name)?.into(), (*name).into()],
            ))
            .await?;
        }
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The trimmed lists stay: the earlier lists are not kept (templates only, never roles).
        let db = manager.get_connection();
        for (name, _) in ADDED_TEMPLATES {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "DELETE FROM role_templates WHERE name = ? AND is_system = 1",
                [(*name).into()],
            ))
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::permissions::{
        DI_CREATE_OWN, DI_VIEW, EQ_VIEW, INV_MANAGE, INV_PROCURE, INV_VIEW, OT_EDIT, OT_EMERGENCY_ASSIGN, PLAN_EDIT,
        PTW_CLOSE, PTW_ISSUE, PTW_VIEW,
    };

    fn rights(name: &str) -> &'static [&'static str] {
        ROLE_TEMPLATES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{name} missing from the registry"))
    }

    #[test]
    fn every_template_written_here_is_in_the_registry() {
        for (name, _) in ADDED_TEMPLATES {
            assert!(stored_rights(name).is_ok(), "{name}");
        }
        for name in TRIMMED_TEMPLATES {
            assert!(stored_rights(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn all_eleven_models_are_offered() {
        // 4 rows of migration 028 + the 7 added here (Advisor answer 7: keep all 11).
        assert_eq!(ROLE_TEMPLATES.len(), 11);
    }

    #[test]
    fn production_operator_only_reports_problems() {
        let mut r = rights("Production Operator Template").to_vec();
        r.sort_unstable();
        let mut expected = vec![DI_CREATE_OWN, DI_VIEW, EQ_VIEW];
        expected.sort_unstable();
        assert_eq!(r, expected);
    }

    #[test]
    fn permit_rights_beyond_view_only_where_the_advisor_allowed_them() {
        for (name, r) in ROLE_TEMPLATES {
            if *name == "HSE Manager Template" {
                assert!(r.contains(&PTW_VIEW) && r.contains(&PTW_ISSUE) && r.contains(&PTW_CLOSE));
                continue;
            }
            for permit_right in [PTW_ISSUE, PTW_CLOSE] {
                assert!(!r.contains(&permit_right), "{name} holds {permit_right}");
            }
        }
    }

    #[test]
    fn trimmed_and_least_privilege_lists() {
        assert!(!rights("Technician Template").contains(&OT_EMERGENCY_ASSIGN));
        assert!(!rights("Supervisor Template").contains(&OT_EMERGENCY_ASSIGN));
        assert!(rights("Maintenance Manager Template").contains(&OT_EMERGENCY_ASSIGN));
        // The storekeeper receives goods (`inv.receive`) but holds no purchasing right (Advisor 1).
        assert!(!rights("Storekeeper Template").contains(&INV_PROCURE));
        assert!(rights("Buyer Template").contains(&INV_PROCURE));
    }

    #[test]
    fn planner_plans_without_purchasing_or_stock_movements() {
        // CMMS Advisor, option a. Reserving a work order's parts goes through the work order
        // (`add_part`, ot.edit), so it needs no stock right.
        let planner = rights("Planner Template");
        for kept_out in [OT_EMERGENCY_ASSIGN, INV_PROCURE, INV_MANAGE] {
            assert!(!planner.contains(&kept_out), "Planner holds {kept_out}");
        }
        for kept in [PLAN_EDIT, OT_EDIT, INV_VIEW] {
            assert!(planner.contains(&kept), "Planner lacks {kept}");
        }
        assert!(TRIMMED_TEMPLATES.contains(&"Planner Template"));
    }
}
