//! Migration 029 — Authoritative Permission Catalog
//!
//! Phase 2 - Sub-phase 06 - File 02.
//!
//! Seeds the full PRD §6.7 permission catalog across all 21 domains.
//! Uses INSERT OR IGNORE so permissions already seeded by earlier migrations
//! (SP04 di.*, SP05 ot.*, seeder baseline) are NOT overwritten.
//! This makes the migration idempotent and order-independent.
//!
//! After this migration, `SELECT COUNT(*) FROM permissions` returns ≥ 70,
//! covering at least 21 distinct categories.
//!
//! Prerequisites: migration 002 (permissions table created).

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260412_000029_permission_catalog"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        // Full permission catalog: (name, description, category, is_dangerous, requires_step_up)
        //
        // 83 rows across 22 domains. INSERT OR IGNORE respects the UNIQUE
        // constraint on permissions.name — rows already present are skipped.
        let catalog: &[(&str, &str, &str, i32, i32)] = &[
            // ── Equipment (eq) ───────────────────────────────────────────
            ("eq.view",    "View equipment records",                   "eq", 0, 0),
            ("eq.create",  "Create new equipment",                     "eq", 0, 0),
            ("eq.edit",    "Edit equipment fields and lifecycle",      "eq", 0, 0),
            ("eq.delete",  "Delete draft or decommissioned equipment", "eq", 1, 0),
            ("eq.admin",   "Manage equipment import, merge, archive",  "eq", 1, 1),

            // ── Intervention Requests (di) ───────────────────────────────
            ("di.view",    "View intervention requests",               "di", 0, 0),
            ("di.submit",  "Submit new intervention requests",         "di", 0, 0),
            ("di.review",  "Review and enrich submitted DIs",          "di", 0, 0),
            ("di.approve", "Approve or reject DIs in review",          "di", 1, 1),
            ("di.convert", "Convert DI to work order",                 "di", 1, 0),
            ("di.delete",  "Delete draft DIs",                         "di", 1, 0),
            ("di.admin",   "Reopen, override, and manage DI settings", "di", 1, 1),

            // ── Work Orders (ot) ─────────────────────────────────────────
            ("ot.view",    "View work orders and details",             "ot", 0, 0),
            ("ot.create",  "Create new work orders",                   "ot", 0, 0),
            ("ot.edit",    "Edit, plan, assign, and execute WOs",      "ot", 0, 0),
            ("ot.approve", "Approve work orders from draft",           "ot", 0, 0),
            ("ot.close",   "Close technically verified work orders",   "ot", 1, 1),
            ("ot.reopen",  "Reopen recently closed work orders",       "ot", 1, 1),
            ("ot.admin",   "Override, archive, manage WO settings",    "ot", 1, 0),
            ("ot.delete",  "Delete draft work orders",                 "ot", 1, 0),

            // ── Organization (org) ───────────────────────────────────────
            ("org.view",   "View org structure",                       "org", 0, 0),
            ("org.edit",   "Edit org units and hierarchies",           "org", 0, 0),
            ("org.admin",  "Publish, archive, and merge org units",    "org", 1, 1),

            // ── Personnel (per) ──────────────────────────────────────────
            ("per.view",   "View personnel records",                   "per", 0, 0),
            ("per.create", "Create new personnel records",             "per", 0, 0),
            ("per.edit",   "Edit personnel details and availability",  "per", 0, 0),
            ("per.delete", "Deactivate personnel records",             "per", 1, 0),
            ("per.admin",  "Manage rate cards, onboarding, overrides", "per", 1, 1),

            // ── Reference Data (ref) ─────────────────────────────────────
            ("ref.view",    "View reference domains and values",         "ref", 0, 0),
            ("ref.edit",    "Edit and draft reference domain values",    "ref", 0, 0),
            ("ref.publish", "Publish reference domain versions",         "ref", 1, 1),
            ("ref.admin",   "Import, merge, retire reference domains",   "ref", 1, 1),

            // ── Inventory (inv) ──────────────────────────────────────────
            ("inv.view",    "View stock, articles, and transactions",    "inv", 0, 0),
            ("inv.manage",  "Issue, return, adjust, transfer stock",     "inv", 0, 0),
            ("inv.procure", "Create and approve purchase requisitions",  "inv", 1, 0),
            ("inv.count",   "Execute and post stock count sessions",     "inv", 1, 0),

            // ── Preventive Maintenance (pm) ──────────────────────────────
            ("pm.view",   "View PM plans and occurrences",             "pm", 0, 0),
            ("pm.create", "Create new PM plans",                       "pm", 0, 0),
            ("pm.edit",   "Edit PM plans and versions",                "pm", 0, 0),
            ("pm.delete", "Delete draft PM plans",                     "pm", 1, 0),

            // ── RAMS / Reliability (ram) ─────────────────────────────────
            ("ram.view",    "View reliability events and KPIs",          "ram", 0, 0),
            ("ram.analyze", "Perform FMECA and RCM analysis",            "ram", 0, 0),
            ("ram.export",  "Export reliability data and reports",        "ram", 0, 0),

            // ── Reports/Analytics (rep) ──────────────────────────────────
            ("rep.view",   "View operational reports and dashboards",   "rep", 0, 0),
            ("rep.export", "Export reports to PDF or Excel",            "rep", 0, 0),
            ("rep.admin",  "Manage report templates and sharing",       "rep", 1, 0),

            // ── Archive Explorer (arc) ───────────────────────────────────
            ("arc.view",    "Browse archived records",                   "arc", 0, 0),
            ("arc.restore", "Restore eligible archived records",         "arc", 1, 1),
            ("arc.purge",   "Purge records past retention policy",       "arc", 1, 1),

            // ── Documentation (doc) ──────────────────────────────────────
            ("doc.view",   "View documents and support articles",       "doc", 0, 0),
            ("doc.author", "Author and publish documentation",          "doc", 0, 0),
            ("doc.admin",  "Manage doc lifecycle and acknowledgements", "doc", 1, 0),

            // ── Planning & Scheduling (plan) ─────────────────────────────
            ("plan.view",  "View planning boards and schedule",         "plan", 0, 0),
            ("plan.edit",  "Schedule and commit work to calendar",      "plan", 0, 0),
            ("plan.admin", "Manage capacity limits and constraints",    "plan", 1, 0),

            // ── Activity Log (log) ───────────────────────────────────────
            ("log.view",   "View activity feed events",                 "log", 0, 0),
            ("log.export", "Export audit log",                          "log", 1, 0),
            ("log.admin",  "Manage activity feed settings",             "log", 1, 1),

            // ── Training & Habilitation (trn) ────────────────────────────
            ("trn.view",     "View training and certification records",   "trn", 0, 0),
            ("trn.manage",   "Manage certifications and training records","trn", 0, 0),
            ("trn.override", "Override qualification holds with reason",  "trn", 1, 1),

            // ── IoT Gateway (iot) ────────────────────────────────────────
            ("iot.view",      "View IoT device streams and alerts",        "iot", 0, 0),
            ("iot.configure", "Configure IoT devices, rules, and mapping", "iot", 1, 1),

            // ── ERP Connector (erp) ──────────────────────────────────────
            ("erp.view",      "View ERP connector status and logs",        "erp", 0, 0),
            ("erp.configure", "Configure ERP mappings and sync contracts", "erp", 1, 1),
            ("erp.reconcile", "Run and approve ERP reconciliation runs",   "erp", 1, 1),

            // ── Work Permits (ptw) ───────────────────────────────────────
            ("ptw.view",  "View work permits",                          "ptw", 0, 0),
            ("ptw.issue", "Issue and activate work permits",            "ptw", 1, 1),
            ("ptw.close", "Close and cancel work permits",              "ptw", 1, 1),

            // ── Budget/Finance (fin) ─────────────────────────────────────
            ("fin.view",    "View budgets and cost center reports",       "fin", 0, 0),
            ("fin.budget",  "Manage budget baselines and forecast drafts","fin", 1, 0),
            ("fin.report",  "Generate governed finance reports and exports","fin", 0, 0),
            ("fin.manage",  "Manage budget baselines and forecasts",      "fin", 1, 0),
            ("fin.approve", "Approve cost events and commitments",        "fin", 1, 1),
            ("fin.post",    "Post cost actuals to ERP",                   "fin", 1, 1),

            // ── Inspection Rounds (ins) ──────────────────────────────────
            ("ins.view",    "View inspection rounds and checklists",       "ins", 0, 0),
            ("ins.execute", "Execute inspection rounds and record results","ins", 0, 0),
            ("ins.admin",   "Manage inspection templates and schedules",   "ins", 1, 0),

            // ── Configuration Engine (cfg) ───────────────────────────────
            ("cfg.view",    "View configuration engine settings",         "cfg", 0, 0),
            ("cfg.edit",    "Edit and draft configuration changes",       "cfg", 1, 0),
            ("cfg.publish", "Publish configuration changes globally",     "cfg", 1, 1),
            ("cfg.admin",   "Manage tenant customization rules",          "cfg", 1, 1),

            // ── Administration (adm) ─────────────────────────────────────
            ("adm.users",       "Manage user accounts and scope assignments", "adm", 1, 1),
            ("adm.roles",       "Manage roles and role permissions",          "adm", 1, 1),
            ("adm.permissions", "View and govern the permission catalog",     "adm", 1, 1),
        ];

        for (name, description, category, is_dangerous, requires_step_up) in catalog {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO permissions
                       (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
                   VALUES (?, ?, ?, ?, ?, 1, ?)",
                [
                    (*name).into(),
                    (*description).into(),
                    (*category).into(),
                    (*is_dangerous).into(),
                    (*requires_step_up).into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        tracing::info!(
            count = catalog.len(),
            "migration_029::permission_catalog_seeded"
        );

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Down migration is intentionally a no-op.
        //
        // This migration only performs INSERT OR IGNORE — it adds rows that
        // did not previously exist. Removing them on rollback could break
        // role_permissions FK references created by later migrations or by
        // admin actions. The safe rollback boundary is at migration 028.
        Ok(())
    }
}
