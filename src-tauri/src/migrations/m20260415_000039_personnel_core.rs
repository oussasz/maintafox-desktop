//! Migration 039 — Personnel core (PRD §6.6)
//!
//! Positions, schedules, external companies, personnel master, rate cards, and authorizations.
//! `external_companies` is created before `personnel` because of `personnel.external_company_id`.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260415_000039_personnel_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS positions (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                code                    TEXT    NOT NULL UNIQUE,
                name                    TEXT    NOT NULL,
                category                TEXT    NOT NULL DEFAULT 'technician',
                requirement_profile_id  INTEGER NULL,
                is_active               INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_classes (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                name                    TEXT    NOT NULL,
                shift_pattern_code      TEXT    NOT NULL,
                is_continuous           INTEGER NOT NULL DEFAULT 0,
                nominal_hours_per_day   REAL    NOT NULL DEFAULT 8.0,
                is_active               INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_details (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                schedule_class_id       INTEGER NOT NULL REFERENCES schedule_classes(id),
                day_of_week             INTEGER NOT NULL,
                shift_start             TEXT    NOT NULL,
                shift_end               TEXT    NOT NULL,
                is_rest_day             INTEGER NOT NULL DEFAULT 0,
                UNIQUE(schedule_class_id, day_of_week)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS external_companies (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                name                    TEXT    NOT NULL,
                service_domain          TEXT    NULL,
                contract_start          TEXT    NULL,
                contract_end            TEXT    NULL,
                onboarding_status       TEXT    NOT NULL DEFAULT 'pending',
                insurance_status        TEXT    NOT NULL DEFAULT 'unknown',
                notes                   TEXT    NULL,
                is_active               INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS external_company_contacts (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                company_id              INTEGER NOT NULL REFERENCES external_companies(id),
                contact_name            TEXT    NOT NULL,
                contact_role            TEXT    NULL,
                phone                   TEXT    NULL,
                email                   TEXT    NULL,
                is_primary              INTEGER NOT NULL DEFAULT 0,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ecc_company ON external_company_contacts(company_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                employee_code           TEXT    NOT NULL UNIQUE,
                full_name               TEXT    NOT NULL,
                employment_type         TEXT    NOT NULL DEFAULT 'employee',
                position_id             INTEGER NULL REFERENCES positions(id),
                primary_entity_id       INTEGER NULL REFERENCES org_nodes(id),
                primary_team_id         INTEGER NULL REFERENCES org_nodes(id),
                supervisor_id           INTEGER NULL REFERENCES personnel(id),
                home_schedule_id        INTEGER NULL REFERENCES schedule_classes(id),
                availability_status     TEXT    NOT NULL DEFAULT 'available',
                hire_date               TEXT    NULL,
                termination_date        TEXT    NULL,
                email                   TEXT    NULL,
                phone                   TEXT    NULL,
                photo_path              TEXT    NULL,
                hr_external_id          TEXT    NULL,
                external_company_id     INTEGER NULL REFERENCES external_companies(id),
                notes                   TEXT    NULL,
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_code ON personnel(employee_code)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_entity ON personnel(primary_entity_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_team ON personnel(primary_team_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_employment ON personnel(employment_type)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_status ON personnel(availability_status)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_supervisor ON personnel(supervisor_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_per_company ON personnel(external_company_id)")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_rate_cards (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id            INTEGER NOT NULL REFERENCES personnel(id),
                effective_from          TEXT    NOT NULL,
                labor_rate              REAL    NOT NULL DEFAULT 0.0,
                overtime_rate           REAL    NOT NULL DEFAULT 0.0,
                cost_center_id          INTEGER NULL,
                source_type             TEXT    NOT NULL DEFAULT 'manual',
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_prc_personnel ON personnel_rate_cards(personnel_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_authorizations (
                id                          INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id                INTEGER NOT NULL REFERENCES personnel(id),
                authorization_type          TEXT    NOT NULL,
                valid_from                  TEXT    NOT NULL,
                valid_to                    TEXT    NULL,
                source_certification_type_id INTEGER NULL,
                is_active                   INTEGER NOT NULL DEFAULT 1,
                created_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pa_personnel ON personnel_authorizations(personnel_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pa_type ON personnel_authorizations(authorization_type)",
        )
        .await?;

        // ── Seed: default positions (PRD §6.6 categories) ─────────────────────
        db.execute_unprepared(
            "INSERT OR IGNORE INTO positions (code, name, category) VALUES \
             ('POS-TECH',  'Technicien de maintenance',      'technician'), \
             ('POS-SUPV',  'Chef d''équipe maintenance',     'supervisor'), \
             ('POS-ENG',   'Ingénieur maintenance',           'engineer'), \
             ('POS-OPR',   'Opérateur',                       'operator'), \
             ('POS-PLN',   'Planificateur maintenance',       'planner'), \
             ('POS-MAG',   'Magasinier',                      'storekeeper'), \
             ('POS-HSE',   'Responsable HSE',                 'hse')",
        )
        .await?;

        // ── Seed: default day-shift schedule class + Mon–Fri work, weekend rest ─
        db.execute_unprepared(
            "INSERT OR IGNORE INTO schedule_classes (name, shift_pattern_code, is_continuous, nominal_hours_per_day) \
             VALUES ('Journée normale', 'DAY_SHIFT', 0, 8.0)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO schedule_details (schedule_class_id, day_of_week, shift_start, shift_end, is_rest_day) \
             SELECT sc.id, d.day, '08:00', '16:00', d.rest \
             FROM schedule_classes sc \
             CROSS JOIN ( \
                 SELECT 1 AS day, 0 AS rest UNION ALL SELECT 2, 0 UNION ALL SELECT 3, 0 UNION ALL SELECT 4, 0 \
                 UNION ALL SELECT 5, 0 UNION ALL SELECT 6, 1 UNION ALL SELECT 7, 1 \
             ) d \
             WHERE sc.shift_pattern_code = 'DAY_SHIFT'",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP TABLE IF EXISTS personnel_authorizations")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS personnel_rate_cards")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS personnel").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS external_company_contacts")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS external_companies")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_details").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_classes").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS positions").await?;

        Ok(())
    }
}
