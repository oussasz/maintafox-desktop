//! Migration 139 — DI lifecycle redesign: dispositions + status collapse.
//!
//! - Adds DI.DISPOSITION (Category B) reference domain + seed values
//! - Adds disposition / related_di / closed_by / deferred_from_status columns
//! - Backfills status rename map and dispositions from legacy terminal statuses
//! - Adds related_di_id on di_review_events for close snapshots

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260816_000139_di_lifecycle_disposition"
    }
}

async fn add_column_if_missing(
    db: &dyn ConnectionTrait,
    table: &str,
    column: &str,
    ddl_type: &str,
) -> Result<(), DbErr> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    let exists = rows
        .iter()
        .any(|r| r.try_get::<String>("", "name").map(|n| n == column).unwrap_or(false));
    if !exists {
        db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl_type}"))
            .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── DI.DISPOSITION reference domain (Category B) ─────────────────────
        db.execute_unprepared(
            r#"
            INSERT INTO reference_domains (
              code, name, structure_type, governance_level, governance_category,
              is_extendable, validation_rules_json, created_at, updated_at
            )
            SELECT
              'DI.DISPOSITION',
              'Dispositions de clôture DI',
              'flat',
              'tenant_managed',
              'operational_dictionary',
              1,
              NULL,
              strftime('%Y-%m-%dT%H:%M:%SZ','now'),
              strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE NOT EXISTS (
              SELECT 1 FROM reference_domains
              WHERE UPPER(TRIM(code)) = 'DI.DISPOSITION'
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at)
            SELECT rd.id, 1, 'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL,
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')
              FROM reference_domains rd
             WHERE UPPER(TRIM(rd.code)) = 'DI.DISPOSITION'
               AND NOT EXISTS (
                 SELECT 1 FROM reference_sets rs
                  WHERE rs.domain_id = rd.id AND rs.status = 'published'
               );
            "#,
        )
        .await?;

        let sys_meta = r#"{"origin":"system"}"#;
        let seed_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, seed.description, seed.sort_order, NULL, NULL, 'di_disposition', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'converted_to_wo' AS code, 'Convertie en OT' AS label, 'Demande convertie en ordre de travail.' AS description, 1 AS sort_order \
                    UNION ALL SELECT 'rejected_invalid', 'Rejetée — invalide', 'Demande invalide ou hors périmètre.', 2 \
                    UNION ALL SELECT 'duplicate', 'Doublon', 'Doublon d''une autre demande (related_di_id obligatoire).', 3 \
                    UNION ALL SELECT 'cancelled_by_requester', 'Annulée par le demandeur', 'Retrait par le demandeur.', 4 \
                    UNION ALL SELECT 'cancelled_by_planner', 'Annulée par le planificateur', 'Annulation opérationnelle après revue.', 5 \
                    UNION ALL SELECT 'no_work_required', 'Aucun travail requis', 'Aucune intervention nécessaire.', 6 \
                    UNION ALL SELECT 'solved_immediately', 'Résolue immédiatement', 'Résolue sans créer d''OT.', 7 \
                    UNION ALL SELECT 'information_only', 'Information seule', 'Demande informative — pas d''action.', 8 \
                    UNION ALL SELECT 'other', 'Autre', 'Autre disposition (notes obligatoires).', 9 \
               ) seed \
               JOIN reference_domains d ON UPPER(TRIM(d.code)) = 'DI.DISPOSITION' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, seed_sql)).await?;

        // ── IR columns ───────────────────────────────────────────────────────
        add_column_if_missing(db, "intervention_requests", "disposition_code", "TEXT NULL").await?;
        add_column_if_missing(db, "intervention_requests", "disposition_notes", "TEXT NULL").await?;
        add_column_if_missing(db, "intervention_requests", "related_di_id", "INTEGER NULL").await?;
        add_column_if_missing(db, "intervention_requests", "closed_by_id", "INTEGER NULL").await?;
        add_column_if_missing(db, "intervention_requests", "deferred_from_status", "TEXT NULL").await?;

        add_column_if_missing(db, "di_review_events", "related_di_id", "INTEGER NULL").await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ir_status_disposition \
             ON intervention_requests (status, disposition_code)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ir_related_di \
             ON intervention_requests (related_di_id)",
        )
        .await?;

        // ── Status + disposition backfill ────────────────────────────────────
        // Stage renames (non-terminal)
        db.execute_unprepared("UPDATE intervention_requests SET status = 'in_review' WHERE status = 'pending_review'")
            .await?;
        db.execute_unprepared(
            "UPDATE intervention_requests SET status = 'awaiting_approval' WHERE status = 'screened'",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE intervention_requests SET status = 'approved' WHERE status = 'approved_for_planning'",
        )
        .await?;

        // Terminal → closed + disposition
        db.execute_unprepared(
            "UPDATE intervention_requests SET \
               status = 'closed', \
               disposition_code = COALESCE(disposition_code, 'rejected_invalid'), \
               closed_at = COALESCE(closed_at, declined_at, updated_at), \
               disposition_notes = COALESCE(disposition_notes, reviewer_note) \
             WHERE status = 'rejected'",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE intervention_requests SET \
               status = 'closed', \
               disposition_code = COALESCE(disposition_code, 'no_work_required'), \
               closed_at = COALESCE(closed_at, updated_at), \
               disposition_notes = COALESCE(disposition_notes, reviewer_note) \
             WHERE status = 'closed_as_non_executable'",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE intervention_requests SET \
               status = 'closed', \
               disposition_code = COALESCE(disposition_code, 'converted_to_wo'), \
               closed_at = COALESCE(closed_at, converted_at, updated_at) \
             WHERE status = 'converted_to_work_order'",
        )
        .await?;

        // Archived → closed; preserve prior disposition if already set via history,
        // otherwise infer from converted / declined / default other.
        db.execute_unprepared(
            "UPDATE intervention_requests SET \
               status = 'closed', \
               disposition_code = COALESCE( \
                 disposition_code, \
                 CASE \
                   WHEN converted_to_wo_id IS NOT NULL THEN 'converted_to_wo' \
                   WHEN declined_at IS NOT NULL THEN 'rejected_invalid' \
                   ELSE 'other' \
                 END \
               ), \
               closed_at = COALESCE(closed_at, declined_at, converted_at, archived_at, updated_at), \
               archived_at = COALESCE(archived_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
               disposition_notes = COALESCE(disposition_notes, reviewer_note, 'Migrated from archived status') \
             WHERE status = 'archived'",
        )
        .await?;

        // Copy reject reason_code from latest transition log into notes when empty
        db.execute_unprepared(
            "UPDATE intervention_requests \
             SET disposition_notes = ( \
               SELECT t.reason_code FROM di_state_transition_log t \
                WHERE t.di_id = intervention_requests.id \
                  AND t.action = 'reject' \
                  AND t.reason_code IS NOT NULL AND TRIM(t.reason_code) != '' \
                ORDER BY t.id DESC LIMIT 1 \
             ) \
             WHERE status = 'closed' \
               AND disposition_code = 'rejected_invalid' \
               AND (disposition_notes IS NULL OR TRIM(disposition_notes) = '') \
               AND EXISTS ( \
                 SELECT 1 FROM di_state_transition_log t2 \
                  WHERE t2.di_id = intervention_requests.id AND t2.action = 'reject' \
               )",
        )
        .await?;

        // Lifecycle notification categories (emit going forward; no historical spam)
        db.execute_unprepared(
            "INSERT OR IGNORE INTO notification_categories
                (code, label, default_severity, default_requires_ack, is_user_configurable)
             VALUES
                ('di_returned',  'DI Returned for Clarification', 'warning', 0, 1),
                ('di_approved',  'DI Approved',                   'info',    0, 1),
                ('di_closed',    'DI Closed',                     'info',    0, 1),
                ('di_converted', 'DI Converted to WO',             'info',    0, 1),
                ('di_deferred',  'DI Deferred',                   'warning', 0, 1)",
        )
        .await?;

        for (cat, mode) in [
            ("di_returned", "submitter"),
            ("di_approved", "reviewer"),
            ("di_closed", "submitter"),
            ("di_converted", "submitter"),
            ("di_deferred", "reviewer"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO notification_rules
                    (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
                 SELECT ?, ?, 0, 120, NULL, 1
                 WHERE NOT EXISTS (
                    SELECT 1 FROM notification_rules WHERE category_code = ?
                 )",
                [cat.into(), mode.into(), cat.into()],
            ))
            .await?;
        }

        // ── Migrate audit/review event status strings (ported from safety stash) ──
        // Keep historical transition/review rows aligned with the Closed-only vocabulary.
        for (old, new) in [
            ("pending_review", "in_review"),
            ("screened", "awaiting_approval"),
            ("approved_for_planning", "approved"),
            ("converted_to_work_order", "closed"),
            ("closed_as_non_executable", "closed"),
            ("rejected", "closed"),
            ("archived", "closed"),
        ] {
            db.execute_unprepared(&format!(
                "UPDATE di_state_transition_log SET from_status = '{new}' \
                 WHERE from_status = '{old}'"
            ))
            .await?;
            db.execute_unprepared(&format!(
                "UPDATE di_state_transition_log SET to_status = '{new}' \
                 WHERE to_status = '{old}'"
            ))
            .await?;
        }

        for (old, new) in [
            ("pending_review", "in_review"),
            ("screened", "awaiting_approval"),
            ("approved_for_planning", "approved"),
            ("converted_to_work_order", "closed"),
            ("closed_as_non_executable", "closed"),
            ("rejected", "closed"),
            ("archived", "closed"),
        ] {
            db.execute_unprepared(&format!(
                "UPDATE di_review_events SET from_status = '{new}' \
                 WHERE from_status = '{old}'"
            ))
            .await?;
            db.execute_unprepared(&format!(
                "UPDATE di_review_events SET to_status = '{new}' \
                 WHERE to_status = '{old}'"
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
