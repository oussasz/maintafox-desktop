//! Personnel SQL layer (PRD §6.6) — SeaORM `Statement` + `DatabaseConnection`, same pattern as DI/WO.

use crate::activity::emitter::{emit_activity_event, ActivityEventInput};
use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use super::domain::{
    AuthorizationType, AvailabilityStatus, CompanyListFilter, EmploymentOrigin, ExternalCompany,
    ExternalCompanyContact, Personnel, PersonnelAuthorization, PersonnelAvailabilityBlock, PersonnelCertDraft,
    PersonnelCreateInput, PersonnelDetailPayload, PersonnelListFilter, PersonnelListPage, PersonnelRateCard,
    PersonnelSkillDraft, PersonnelSkillReferenceValue, PersonnelTeamAssignment, PersonnelUpdateInput,
    PersonnelWorkHistoryEntry, PersonnelWorkloadSummary, Position, PositionChangeEvent, PositionDetailPayload,
    PositionListFilter, PositionRequirementProfileInput, PositionRequirementSeed, PositionUpsertInput, ScheduleClass,
    ScheduleClassWithDetails, ScheduleDetail, SuccessionRiskRow,
};

// ═══════════════════════════════════════════════════════════════════════════════
// SELECT list — personnel + joins
// ═══════════════════════════════════════════════════════════════════════════════

const P_JOINS: &str = "\
    FROM personnel p \
    LEFT JOIN positions pos ON pos.id = p.position_id \
    LEFT JOIN org_nodes e ON e.id = p.primary_entity_id \
    LEFT JOIN teams t ON t.id = p.primary_team_id \
    LEFT JOIN personnel s ON s.id = p.supervisor_id \
    LEFT JOIN reference_values sc ON sc.id = p.home_schedule_reference_value_id \
    LEFT JOIN external_companies ec ON ec.id = p.external_company_id";

const P_SELECT: &str = "\
    SELECT \
    p.id, p.employee_code, p.full_name, p.employment_type, \
    p.employment_origin, p.employment_status, p.blocked_override, \
    p.position_id, p.primary_entity_id, \
    p.primary_team_id, p.supervisor_id, p.home_schedule_reference_value_id, p.availability_status, p.hire_date, \
    p.termination_date, p.email, p.phone, p.photo_path, p.hr_external_id, p.external_company_id, \
    p.contract_number, p.contract_start_date, p.contract_end_date, \
    p.notes, p.row_version, p.created_at, p.updated_at, \
    pos.name AS position_name, pos.code AS position_code, pos.category AS position_category, \
    e.name AS entity_name, t.name AS team_name, s.full_name AS supervisor_name, \
    sc.label AS schedule_name, ec.name AS company_name ";

fn map_personnel_row(row: &sea_orm::QueryResult) -> AppResult<Personnel> {
    Ok(Personnel {
        id: row.try_get::<i64>("", "id").map_err(|e| map_err("id", e))?,
        employee_code: row
            .try_get::<String>("", "employee_code")
            .map_err(|e| map_err("employee_code", e))?,
        full_name: row
            .try_get::<String>("", "full_name")
            .map_err(|e| map_err("full_name", e))?,
        employment_type: row
            .try_get::<String>("", "employment_type")
            .map_err(|e| map_err("employment_type", e))?,
        employment_origin: row
            .try_get::<String>("", "employment_origin")
            .unwrap_or_else(|_| "internal".to_string()),
        employment_status: row
            .try_get::<String>("", "employment_status")
            .unwrap_or_else(|_| "active".to_string()),
        blocked_override: row.try_get::<i64>("", "blocked_override").unwrap_or(0) != 0,
        position_id: row
            .try_get::<Option<i64>>("", "position_id")
            .map_err(|e| map_err("position_id", e))?,
        primary_entity_id: row
            .try_get::<Option<i64>>("", "primary_entity_id")
            .map_err(|e| map_err("primary_entity_id", e))?,
        primary_team_id: row
            .try_get::<Option<i64>>("", "primary_team_id")
            .map_err(|e| map_err("primary_team_id", e))?,
        supervisor_id: row
            .try_get::<Option<i64>>("", "supervisor_id")
            .map_err(|e| map_err("supervisor_id", e))?,
        home_schedule_reference_value_id: row
            .try_get::<Option<i64>>("", "home_schedule_reference_value_id")
            .map_err(|e| map_err("home_schedule_reference_value_id", e))?,
        availability_status: row
            .try_get::<String>("", "availability_status")
            .map_err(|e| map_err("availability_status", e))?,
        hire_date: row
            .try_get::<Option<String>>("", "hire_date")
            .map_err(|e| map_err("hire_date", e))?,
        termination_date: row
            .try_get::<Option<String>>("", "termination_date")
            .map_err(|e| map_err("termination_date", e))?,
        email: row
            .try_get::<Option<String>>("", "email")
            .map_err(|e| map_err("email", e))?,
        phone: row
            .try_get::<Option<String>>("", "phone")
            .map_err(|e| map_err("phone", e))?,
        photo_path: row
            .try_get::<Option<String>>("", "photo_path")
            .map_err(|e| map_err("photo_path", e))?,
        hr_external_id: row
            .try_get::<Option<String>>("", "hr_external_id")
            .map_err(|e| map_err("hr_external_id", e))?,
        external_company_id: row
            .try_get::<Option<i64>>("", "external_company_id")
            .map_err(|e| map_err("external_company_id", e))?,
        contract_number: row.try_get::<Option<String>>("", "contract_number").unwrap_or(None),
        contract_start_date: row.try_get::<Option<String>>("", "contract_start_date").unwrap_or(None),
        contract_end_date: row.try_get::<Option<String>>("", "contract_end_date").unwrap_or(None),
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| map_err("notes", e))?,
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| map_err("row_version", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| map_err("updated_at", e))?,
        position_name: row.try_get::<Option<String>>("", "position_name").unwrap_or(None),
        position_code: row.try_get::<Option<String>>("", "position_code").unwrap_or(None),
        position_category: row.try_get::<Option<String>>("", "position_category").unwrap_or(None),
        entity_name: row.try_get::<Option<String>>("", "entity_name").unwrap_or(None),
        team_name: row.try_get::<Option<String>>("", "team_name").unwrap_or(None),
        supervisor_name: row.try_get::<Option<String>>("", "supervisor_name").unwrap_or(None),
        schedule_name: row.try_get::<Option<String>>("", "schedule_name").unwrap_or(None),
        company_name: row.try_get::<Option<String>>("", "company_name").unwrap_or(None),
        sensitive_masked: false,
    })
}

fn map_err(col: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("personnel row decode {col}: {e}"))
}

fn opt_string_val(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

async fn emit_personnel_event(
    db: &DatabaseConnection,
    event_code: &str,
    personnel_id: i64,
    actor_id: Option<i64>,
) -> AppResult<()> {
    let _ = emit_activity_event(
        db,
        ActivityEventInput {
            event_class: "operational".to_string(),
            event_code: event_code.to_string(),
            source_module: "personnel".to_string(),
            source_record_type: Some("personnel".to_string()),
            source_record_id: Some(personnel_id.to_string()),
            entity_scope_id: None,
            actor_id,
            severity: "info".to_string(),
            summary_json: None,
            correlation_id: None,
            visibility_scope: "entity".to_string(),
        },
    )
    .await;
    Ok(())
}

/// Prevent circular supervisor chains.
pub async fn assert_no_supervisor_cycle(
    db: &DatabaseConnection,
    personnel_id: i64,
    supervisor_id: i64,
) -> AppResult<()> {
    if supervisor_id == personnel_id {
        return Err(AppError::ValidationFailed(vec![
            "Un collaborateur ne peut pas être son propre superviseur.".into(),
        ]));
    }
    // Walk the supervisor chain from `supervisor_id` up; if we encounter `personnel_id`, there is a cycle.
    let mut current = supervisor_id;
    for _ in 0..50 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT supervisor_id FROM personnel WHERE id = ? LIMIT 1",
                [current.into()],
            ))
            .await?;
        let Some(r) = row else { break };
        let next: Option<i64> = r.try_get("", "supervisor_id").unwrap_or(None);
        match next {
            None => break,
            Some(sup) if sup == personnel_id => {
                return Err(AppError::ValidationFailed(vec![
                    "La relation de supervision crée un cycle.".into(),
                ]));
            }
            Some(sup) => {
                current = sup;
            }
        }
    }
    Ok(())
}

/// Close the current open assignment history segment for a personnel record.
pub async fn close_open_assignment_history_segment(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    ended_at: &str,
    reason: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel_assignment_history \
         SET ended_at = ?, reason = CASE WHEN reason IS NULL THEN ? ELSE reason || ' / ' || ? END \
         WHERE personnel_id = ? AND ended_at IS NULL",
        [ended_at.into(), reason.into(), reason.into(), personnel_id.into()],
    ))
    .await?;
    Ok(())
}

/// Open a new assignment history segment.
#[allow(clippy::too_many_arguments)]
pub async fn open_assignment_history_segment(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    entity_id: Option<i64>,
    team_id: Option<i64>,
    position_id: Option<i64>,
    manager_id: Option<i64>,
    schedule_reference_value_id: Option<i64>,
    started_at: &str,
    reason: &str,
    changed_by_id: Option<i64>,
    created_at: &str,
) -> AppResult<()> {
    if let Some(crew_id) = team_id {
        crate::repository::team_repository::assert_active_crew_exists(db, crew_id).await?;
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_assignment_history \
         (personnel_id, entity_id, team_id, position_id, manager_id, \
          schedule_reference_value_id, started_at, ended_at, reason, changed_by_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?)",
        [
            personnel_id.into(),
            entity_id.into(),
            team_id.into(),
            position_id.into(),
            manager_id.into(),
            schedule_reference_value_id.into(),
            started_at.into(),
            reason.into(),
            changed_by_id.into(),
            created_at.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Write an initial personnel_skills draft row.
async fn write_skill_draft(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    skill: &PersonnelSkillDraft,
    now: &str,
) -> AppResult<()> {
    let _ = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO personnel_skills \
             (personnel_id, reference_value_id, proficiency_level, source_type, \
              valid_to, is_primary, is_validated, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
            [
                personnel_id.into(),
                skill.reference_value_id.into(),
                skill.proficiency_level.into(),
                skill.source_type.clone().into(),
                opt_string_val(skill.valid_to.clone()),
                (if skill.is_primary.unwrap_or(false) {
                    1_i64
                } else {
                    0_i64
                })
                .into(),
                now.into(),
                now.into(),
            ],
        ))
        .await;
    Ok(())
}

/// Write an initial personnel_certifications draft row.
async fn write_cert_draft(db: &impl ConnectionTrait, personnel_id: i64, cert: &PersonnelCertDraft) -> AppResult<()> {
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    let _ = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO personnel_certifications (entity_sync_id, personnel_id, certification_type_id, \
              issued_at, expires_at, issuing_body, certificate_ref, \
              verification_status, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', 1)",
            [
                entity_sync_id.into(),
                personnel_id.into(),
                cert.certification_type_id.into(),
                opt_string_val(cert.issued_at.clone()),
                opt_string_val(cert.expires_at.clone()),
                opt_string_val(cert.issuing_body.clone()),
                opt_string_val(cert.certificate_ref.clone()),
            ],
        ))
        .await;
    Ok(())
}

pub async fn assert_position_exists(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM positions WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Poste (position_id={id}) introuvable."
        )]));
    }
    Ok(())
}

pub async fn assert_org_node_exists(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    crate::org::model_scope::assert_org_node_active(db, id)
        .await
        .map_err(|e| match e {
            AppError::NotFound { .. } => {
                AppError::ValidationFailed(vec![format!("Nœud d'organisation (id={id}) introuvable.")])
            }
            AppError::ValidationFailed(_) => AppError::ValidationFailed(vec![format!(
                "Nœud d'organisation (id={id}) introuvable ou non publié."
            )]),
            other => other,
        })
}

/// Sync home-crew membership in `personnel_team_assignments`.
/// Closes any open assignment for this person, then opens one for `team_id`.
pub async fn sync_home_crew_assignment(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    team_id: i64,
    now: &str,
) -> AppResult<()> {
    crate::repository::team_repository::assert_active_crew_exists(db, team_id).await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel_team_assignments
         SET valid_to = ?, updated_at = ?
         WHERE personnel_id = ?
           AND (valid_to IS NULL OR date(valid_to) >= date(?))
           AND team_id <> ?",
        [now.into(), now.into(), personnel_id.into(), now.into(), team_id.into()],
    ))
    .await?;

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, is_lead FROM personnel_team_assignments
             WHERE personnel_id = ?
               AND team_id = ?
               AND (valid_to IS NULL OR date(valid_to) >= date(?))
             ORDER BY is_lead DESC, id DESC
             LIMIT 1",
            [personnel_id.into(), team_id.into(), now.into()],
        ))
        .await?;

    if existing.is_some() {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_team_assignments
             SET allocation_percent = 100.0,
                 role_code = CASE WHEN is_lead = 1 THEN role_code ELSE 'member' END,
                 updated_at = ?
             WHERE personnel_id = ?
               AND team_id = ?
               AND (valid_to IS NULL OR date(valid_to) >= date(?))",
            [now.into(), personnel_id.into(), team_id.into(), now.into()],
        ))
        .await?;
        return Ok(());
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_team_assignments (
            personnel_id, team_id, role_code, allocation_percent,
            valid_from, valid_to, is_lead, created_at, updated_at
         ) VALUES (?, ?, 'member', 100.0, ?, NULL, 0, ?, ?)",
        [personnel_id.into(), team_id.into(), now.into(), now.into(), now.into()],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_personnel
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_personnel(db: &DatabaseConnection, filter: PersonnelListFilter) -> AppResult<PersonnelListPage> {
    let mut where_clauses: Vec<String> = vec!["1 = 1".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref types) = filter.employment_type {
        if !types.is_empty() {
            let ph = types.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("p.employment_type IN ({ph})"));
            for t in types {
                binds.push(t.clone().into());
            }
        }
    }
    if let Some(ref origins) = filter.employment_origin {
        if !origins.is_empty() {
            let ph = origins.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("p.employment_origin IN ({ph})"));
            for o in origins {
                binds.push(o.clone().into());
            }
        }
    }
    if let Some(ref emp_statuses) = filter.employment_status {
        if !emp_statuses.is_empty() {
            let ph = emp_statuses.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("p.employment_status IN ({ph})"));
            for s in emp_statuses {
                binds.push(s.clone().into());
            }
        }
    }
    if let Some(ref statuses) = filter.availability_status {
        if !statuses.is_empty() {
            let ph = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("p.availability_status IN ({ph})"));
            for s in statuses {
                binds.push(s.clone().into());
            }
        }
    }
    if let Some(pid) = filter.position_id {
        where_clauses.push("p.position_id = ?".into());
        binds.push(pid.into());
    }
    if let Some(eid) = filter.entity_id {
        where_clauses.push("p.primary_entity_id = ?".into());
        binds.push(eid.into());
    }
    if let Some(tid) = filter.team_id {
        where_clauses.push("p.primary_team_id = ?".into());
        binds.push(tid.into());
    }
    if let Some(cid) = filter.company_id {
        where_clauses.push("p.external_company_id = ?".into());
        binds.push(cid.into());
    }
    if let Some(ref q) = filter.search {
        let term = q.trim();
        if !term.is_empty() {
            let like = format!("%{term}%");
            where_clauses.push(
                "(p.employee_code LIKE ? OR p.full_name LIKE ? OR IFNULL(p.email,'') LIKE ? OR IFNULL(p.phone,'') LIKE ?)"
                    .into(),
            );
            for _ in 0..4 {
                binds.push(like.clone().into());
            }
        }
    }

    let where_sql = where_clauses.join(" AND ");
    let limit = if filter.limit > 0 { filter.limit } else { 100 };
    let offset = filter.offset.max(0);

    let count_sql = format!("SELECT COUNT(*) AS c {P_JOINS} WHERE {where_sql}");
    let total: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &count_sql,
            binds.clone(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count personnel")))?
        .try_get::<i64>("", "c")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count c: {e}")))?;

    let mut list_binds = binds;
    list_binds.push(limit.into());
    list_binds.push(offset.into());

    let list_sql = format!("{P_SELECT} {P_JOINS} WHERE {where_sql} ORDER BY p.full_name ASC LIMIT ? OFFSET ?");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &list_sql, list_binds))
        .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(map_personnel_row(&row)?);
    }

    Ok(PersonnelListPage { items, total })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B/C) get_personnel / get_personnel_by_code
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_personnel(db: &DatabaseConnection, id: i64) -> AppResult<Option<Personnel>> {
    let sql = format!("{P_SELECT} {P_JOINS} WHERE p.id = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, [id.into()]))
        .await?;
    row.map(|r| map_personnel_row(&r)).transpose()
}

pub async fn get_personnel_by_code(db: &DatabaseConnection, code: &str) -> AppResult<Option<Personnel>> {
    let sql = format!("{P_SELECT} {P_JOINS} WHERE p.employee_code = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, [code.into()]))
        .await?;
    row.map(|r| map_personnel_row(&r)).transpose()
}

pub async fn get_personnel_detail(db: &DatabaseConnection, id: i64) -> AppResult<Option<PersonnelDetailPayload>> {
    let Some(p) = get_personnel(db, id).await? else {
        return Ok(None);
    };
    let rate_cards = list_rate_cards(db, id).await?;
    let authorizations = list_authorizations(db, id).await?;
    Ok(Some(PersonnelDetailPayload {
        personnel: p,
        rate_cards,
        authorizations,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) create_personnel
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn create_personnel(
    db: &DatabaseConnection,
    input: PersonnelCreateInput,
    actor_id: i64,
) -> AppResult<Personnel> {
    // Employment type and status: values of their reference lists (`personnel::reference_lists`).
    let employment_type = super::reference_lists::checked_employment_type(db, &input.employment_type).await?;
    EmploymentOrigin::try_from(input.employment_origin.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let emp_status =
        super::reference_lists::checked_employment_status(db, input.employment_status.as_deref().unwrap_or("active"))
            .await?;

    if input.employment_origin == "external" && input.external_company_id.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "external_company_id est requis pour un collaborateur externe.".into(),
        ]));
    }

    if let (Some(ref start), Some(ref end)) = (&input.contract_start_date, &input.contract_end_date) {
        if start > end {
            return Err(AppError::ValidationFailed(vec![
                "contract_start_date ne peut pas être après contract_end_date.".into(),
            ]));
        }
    }

    let txn = db.begin().await?;

    let code = {
        let trimmed = input.employee_code.trim();
        if trimmed.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Le code employé ne peut pas être vide.".into(),
            ]));
        }
        let dup = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM personnel WHERE employee_code = ? LIMIT 1",
                [trimmed.to_string().into()],
            ))
            .await?;
        if dup.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Le code employé « {trimmed} » est déjà utilisé."
            )]));
        }
        trimmed.to_string()
    };

    crate::reference::schedule_patterns::assert_schedule_reference_value_active(
        &txn,
        input.home_schedule_reference_value_id,
    )
    .await?;

    if let Some(sup_id) = input.supervisor_id {
        // New person not yet inserted — no cycle possible. Validate supervisor exists.
        let exists = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM personnel WHERE id = ? LIMIT 1",
                [sup_id.into()],
            ))
            .await?;
        if exists.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Superviseur introuvable (id={sup_id})."
            )]));
        }
    }

    let blocked_override = input.blocked_override.unwrap_or(false);
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let started_at = input.hire_date.as_deref().unwrap_or(now.as_str()).to_string();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel (\
            entity_sync_id, employee_code, full_name, employment_type, employment_origin, employment_status, \
            blocked_override, position_id, primary_entity_id, primary_team_id, supervisor_id, \
            home_schedule_reference_value_id, availability_status, hire_date, termination_date, \
            email, phone, external_company_id, contract_number, contract_start_date, contract_end_date, \
            notes, row_version, created_at, updated_at\
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'available', ?, NULL, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            entity_sync_id.into(),
            code.clone().into(),
            input.full_name.clone().into(),
            employment_type.into(),
            input.employment_origin.clone().into(),
            emp_status.clone().into(),
            (if blocked_override { 1_i64 } else { 0_i64 }).into(),
            input.position_id.into(),
            input.primary_entity_id.into(),
            input.primary_team_id.into(),
            input.supervisor_id.into(),
            input.home_schedule_reference_value_id.into(),
            opt_string_val(input.hire_date.clone()),
            opt_string_val(input.email.clone()),
            opt_string_val(input.phone.clone()),
            input.external_company_id.into(),
            opt_string_val(input.contract_number.clone()),
            opt_string_val(input.contract_start_date.clone()),
            opt_string_val(input.contract_end_date.clone()),
            opt_string_val(input.notes.clone()),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec!["Code employé en doublon.".into()])
        } else if e.to_string().contains("FOREIGN KEY") {
            AppError::ValidationFailed(vec!["Référence invalide (FK).".into()])
        } else {
            AppError::Database(e)
        }
    })?;

    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row
        .try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("id: {e}")))?;

    // Write initial skills drafts
    for skill in &input.skills {
        write_skill_draft(&txn, new_id, skill, &now).await?;
    }

    // Write initial cert drafts
    for cert in &input.certifications {
        write_cert_draft(&txn, new_id, cert).await?;
    }

    sync_home_crew_assignment(&txn, new_id, input.primary_team_id, &now).await?;

    // Open initial assignment history segment
    open_assignment_history_segment(
        &txn,
        new_id,
        Some(input.primary_entity_id),
        Some(input.primary_team_id),
        Some(input.position_id),
        input.supervisor_id,
        Some(input.home_schedule_reference_value_id),
        &started_at,
        input.assignment_reason.as_deref().unwrap_or("initial_create"),
        Some(actor_id),
        &now,
    )
    .await?;

    txn.commit().await?;

    emit_personnel_event(db, "personnel.created", new_id, Some(actor_id)).await?;

    get_personnel(db, new_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("personnel row missing after insert id={new_id}")))
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) update_personnel
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn update_personnel(
    db: &DatabaseConnection,
    input: PersonnelUpdateInput,
    actor_id: i64,
) -> AppResult<Personnel> {
    let current = get_personnel(db, input.id).await?.ok_or_else(|| AppError::NotFound {
        entity: "Personnel".into(),
        id: input.id.to_string(),
    })?;

    // Gate: terminated personnel cannot be edited
    if current.employment_status == "terminated" {
        return Err(AppError::ValidationFailed(vec![
            "Impossible de modifier un collaborateur dont le statut est terminé.".into(),
        ]));
    }
    // Gate: inactive requires reactivation first
    if current.employment_status == "inactive" {
        let reactivates = input.employment_status.as_deref().is_some_and(|s| s != "inactive");
        if !reactivates {
            return Err(AppError::ValidationFailed(vec![
                "Impossible de modifier un collaborateur inactif sans le réactiver (employment_status).".into(),
            ]));
        }
    }

    // Validate enums
    let employment_type = match input.employment_type.as_deref() {
        Some(t) => Some(super::reference_lists::checked_employment_type(db, t).await?),
        None => None,
    };
    if let Some(ref o) = input.employment_origin {
        EmploymentOrigin::try_from(o.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    }
    let employment_status = match input.employment_status.as_deref() {
        Some(s) => Some(super::reference_lists::checked_employment_status(db, s).await?),
        None => None,
    };
    if let Some(ref s) = input.availability_status {
        AvailabilityStatus::try_from(s.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    }

    // Optional texts: `null` or an empty text clears the field (`PersonnelUpdateInput`).
    let text = |v: &Option<Option<String>>| -> Option<Option<String>> {
        v.as_ref().map(|inner| {
            inner
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        })
    };
    let hire_date = text(&input.hire_date);
    let termination_date = text(&input.termination_date);
    let email = text(&input.email);
    let phone = text(&input.phone);
    let contract_number = text(&input.contract_number);
    let contract_start_date = text(&input.contract_start_date);
    let contract_end_date = text(&input.contract_end_date);
    let notes = text(&input.notes);

    // An external person keeps a contractor company: it cannot be cleared.
    let eff_origin = input
        .employment_origin
        .as_deref()
        .unwrap_or(current.employment_origin.as_str());
    let eff_company = match input.external_company_id {
        Some(v) => v,
        None => current.external_company_id,
    };
    if eff_origin == "external" && eff_company.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "external_company_id est requis pour un collaborateur externe.".into(),
        ]));
    }

    // Contract date validation (on the values after the update, cleared ones included)
    let eff_start = match &contract_start_date {
        Some(v) => v.as_deref(),
        None => current.contract_start_date.as_deref(),
    };
    let eff_end = match &contract_end_date {
        Some(v) => v.as_deref(),
        None => current.contract_end_date.as_deref(),
    };
    if let (Some(start), Some(end)) = (eff_start, eff_end) {
        if start > end {
            return Err(AppError::ValidationFailed(vec![
                "contract_start_date ne peut pas être après contract_end_date.".into(),
            ]));
        }
    }

    // Supervisor cycle check
    if let Some(Some(sup_id)) = input.supervisor_id {
        assert_no_supervisor_cycle(db, input.id, sup_id).await?;
    }

    // Schedule validation
    if let Some(sched_id) = input.home_schedule_reference_value_id {
        crate::reference::schedule_patterns::assert_schedule_reference_value_active(db, sched_id).await?;
    }

    // Determine if any assignment-tracked fields change
    let new_position_id = input.position_id.or(current.position_id);
    let new_entity_id = input.primary_entity_id.or(current.primary_entity_id);
    let new_team_id = input.primary_team_id.or(current.primary_team_id);
    let new_supervisor_id = match input.supervisor_id {
        Some(v) => v,
        None => current.supervisor_id,
    };
    let new_schedule_id = input
        .home_schedule_reference_value_id
        .or(current.home_schedule_reference_value_id);

    let assignment_changed = input.position_id.is_some_and(|v| Some(v) != current.position_id)
        || input
            .primary_entity_id
            .is_some_and(|v| Some(v) != current.primary_entity_id)
        || input
            .primary_team_id
            .is_some_and(|v| Some(v) != current.primary_team_id)
        || input.supervisor_id.is_some_and(|v| v != current.supervisor_id)
        || input
            .home_schedule_reference_value_id
            .is_some_and(|v| Some(v) != current.home_schedule_reference_value_id);

    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref n) = input.full_name {
        sets.push("full_name = ?".into());
        values.push(n.clone().into());
    }
    if let Some(ref e) = employment_type {
        sets.push("employment_type = ?".into());
        values.push(e.clone().into());
    }
    if let Some(ref o) = input.employment_origin {
        sets.push("employment_origin = ?".into());
        values.push(o.clone().into());
    }
    if let Some(ref s) = employment_status {
        sets.push("employment_status = ?".into());
        values.push(s.clone().into());
    }
    if let Some(b) = input.blocked_override {
        sets.push("blocked_override = ?".into());
        values.push((if b { 1_i64 } else { 0_i64 }).into());
    }
    if let Some(v) = input.position_id {
        sets.push("position_id = ?".into());
        values.push(v.into());
    }
    if let Some(v) = input.primary_entity_id {
        sets.push("primary_entity_id = ?".into());
        values.push(v.into());
    }
    if let Some(v) = input.primary_team_id {
        sets.push("primary_team_id = ?".into());
        values.push(v.into());
    }
    if let Some(v) = input.supervisor_id {
        sets.push("supervisor_id = ?".into());
        values.push(v.into());
    }
    if let Some(v) = input.home_schedule_reference_value_id {
        sets.push("home_schedule_reference_value_id = ?".into());
        values.push(v.into());
    }
    if let Some(ref s) = input.availability_status {
        sets.push("availability_status = ?".into());
        values.push(s.clone().into());
    }
    // Optional fields: present = set to the value, or cleared (NULL) when `null` / empty.
    for (column, value) in [
        ("hire_date", hire_date),
        ("termination_date", termination_date),
        ("email", email),
        ("phone", phone),
        ("contract_number", contract_number),
        ("contract_start_date", contract_start_date),
        ("contract_end_date", contract_end_date),
        ("notes", notes),
    ] {
        if let Some(v) = value {
            sets.push(format!("{column} = ?"));
            values.push(v.into());
        }
    }
    if let Some(v) = input.external_company_id {
        sets.push("external_company_id = ?".into());
        values.push(v.into());
    }
    // employee_code handled separately below

    if sets.is_empty() && input.employee_code.is_none() {
        return Ok(current);
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let txn = db.begin().await?;

    // Handle assignment history if tracked fields changed
    if assignment_changed {
        let reason = input.assignment_reason.as_deref().unwrap_or("assignment_update");
        close_open_assignment_history_segment(&txn, input.id, &now, reason).await?;
        open_assignment_history_segment(
            &txn,
            input.id,
            new_entity_id,
            new_team_id,
            new_position_id,
            new_supervisor_id,
            new_schedule_id,
            &now,
            reason,
            Some(actor_id),
            &now,
        )
        .await?;
    }

    if let Some(team_id) = input.primary_team_id {
        if Some(team_id) != current.primary_team_id {
            sync_home_crew_assignment(&txn, input.id, team_id, &now).await?;
        }
    }

    // Apply employee_code change (checked before SET list)
    if let Some(ref new_code) = input.employee_code {
        let trimmed = new_code.trim();
        if !trimmed.is_empty() {
            let dup = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 FROM personnel WHERE employee_code = ? AND id <> ? LIMIT 1",
                    [trimmed.to_string().into(), input.id.into()],
                ))
                .await?;
            if dup.is_some() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Le code employé « {trimmed} » est déjà utilisé."
                )]));
            }
            sets.push("employee_code = ?".into());
            values.push(trimmed.to_string().into());
        }
    }

    if sets.is_empty() {
        txn.commit().await?;
        return Ok(current);
    }

    sets.push("row_version = row_version + 1".into());
    sets.push("updated_at = ?".into());
    values.push(now.into());
    values.push(input.id.into());
    values.push(input.expected_row_version.into());

    let sql = format!(
        "UPDATE personnel SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let result = txn
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    txn.commit().await?;

    emit_personnel_event(db, "personnel.updated", input.id, Some(actor_id)).await?;

    get_personnel(db, input.id).await?.ok_or_else(|| AppError::NotFound {
        entity: "Personnel".into(),
        id: input.id.to_string(),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) deactivate_personnel
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn deactivate_personnel(
    db: &DatabaseConnection,
    id: i64,
    expected_row_version: i64,
    actor_id: i64,
) -> AppResult<Personnel> {
    let blockers = collect_deactivation_blockers(db, id).await?;
    if !blockers.is_empty() {
        return Err(AppError::ValidationFailed(vec![blockers]));
    }

    let today = Utc::now().format("%Y-%m-%d").to_string();
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let txn = db.begin().await?;
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel SET \
                availability_status = 'inactive', \
                termination_date = COALESCE(termination_date, ?), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [today.into(), now.clone().into(), id.into(), expected_row_version.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    txn.commit().await?;

    emit_personnel_event(db, "personnel.deactivated", id, Some(actor_id)).await?;

    get_personnel(db, id).await?.ok_or_else(|| AppError::NotFound {
        entity: "Personnel".into(),
        id: id.to_string(),
    })
}

/// A work order the person `?` is on: responsible (through their login), or labour booked for
/// them — directly (`personnel_id`, people without a login) or through their login. Binds the
/// personnel id three times.
const PERSON_ON_WO: &str = "( \
    wo.primary_responsible_id IN (SELECT ua.id FROM user_accounts ua WHERE ua.personnel_id = ?) \
    OR EXISTS ( \
        SELECT 1 FROM work_order_interveners woi \
        LEFT JOIN user_accounts uai ON uai.id = woi.intervener_id \
        WHERE woi.work_order_id = wo.id AND (woi.personnel_id = ? OR uai.personnel_id = ?) \
    ) \
)";

/// Open work orders the person is on (responsible or labour).
async fn collect_deactivation_blockers(db: &DatabaseConnection, personnel_id: i64) -> AppResult<String> {
    let mut parts: Vec<String> = Vec::new();

    let wo_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT DISTINCT wo.id, wo.code \
                 FROM work_orders wo \
                 JOIN work_order_statuses wos ON wos.id = wo.status_id \
                 WHERE {PERSON_ON_WO} \
                   AND wos.code NOT IN {closed}",
                closed = crate::wo::backlog::CLOSED_STATUS_SQL
            ),
            [personnel_id.into(), personnel_id.into(), personnel_id.into()],
        ))
        .await?;

    if !wo_rows.is_empty() {
        let codes: Vec<String> = wo_rows
            .iter()
            .filter_map(|r| r.try_get::<String>("", "code").ok())
            .collect();
        parts.push(format!("Ordres de travail actifs : {}.", codes.join(", ")));
    }

    // Work permit schema (6.23) not yet in local DB — no SQL guard here.

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("Désactivation impossible : {}", parts.join(" ")))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// G/H) positions
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_positions(db: &DatabaseConnection) -> AppResult<Vec<Position>> {
    list_positions_filtered(db, PositionListFilter::default()).await
}

pub async fn list_positions_filtered(db: &DatabaseConnection, filter: PositionListFilter) -> AppResult<Vec<Position>> {
    let mut where_clauses = vec!["1 = 1".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if !filter.include_inactive.unwrap_or(false) {
        where_clauses.push("is_active = 1".into());
    }
    if let Some(ref q) = filter.search {
        let term = q.trim();
        if !term.is_empty() {
            let like = format!("%{term}%");
            where_clauses.push("(code LIKE ? OR name LIKE ?)".into());
            binds.push(like.clone().into());
            binds.push(like.into());
        }
    }

    let limit = filter.limit.clamp(1, 500);
    let offset = filter.offset.max(0);
    let where_sql = where_clauses.join(" AND ");

    let sql = format!(
        "SELECT id, code, name, category, requirement_profile_id, is_active, created_at, updated_at \
         FROM positions WHERE {where_sql} ORDER BY name ASC LIMIT {limit} OFFSET {offset}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_position_row(&r)?);
    }
    Ok(out)
}

pub async fn get_position(db: &DatabaseConnection, id: i64) -> AppResult<Option<PositionDetailPayload>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, category, requirement_profile_id, is_active, created_at, updated_at \
             FROM positions WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    let pos = map_position_row(&row)?;

    let skill_ids = load_position_skill_ids(db, pos.requirement_profile_id).await?;
    let cert_ids = load_position_cert_ids(db, pos.requirement_profile_id).await?;
    let change_events = load_position_change_events(db, id).await?;

    Ok(Some(PositionDetailPayload {
        position: pos,
        skill_reference_value_ids: skill_ids,
        certification_type_ids: cert_ids,
        change_events,
    }))
}

/// Create or update a position (upsert by id). Code + name must be unique.
/// Optionally links/creates a qualification requirement profile with skills and certs.
pub async fn upsert_position(
    db: &DatabaseConnection,
    input: PositionUpsertInput,
    actor_id: Option<i64>,
) -> AppResult<Position> {
    let category = super::reference_lists::checked_position_category(db, &input.category).await?;

    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code et le nom du poste sont obligatoires.".into(),
        ]));
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let txn = db.begin().await?;

    let (pos_id, old_code, old_name): (i64, String, String) = if let Some(id) = input.id {
        // Update path
        let existing = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, code, name FROM positions WHERE id = ? LIMIT 1",
                [id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "Position".into(),
                id: id.to_string(),
            })?;
        let ex_code: String = existing.try_get("", "code").unwrap_or_default();
        let ex_name: String = existing.try_get("", "name").unwrap_or_default();

        // Uniqueness check (exclude self)
        let dup = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM positions WHERE code = ? AND id <> ? LIMIT 1",
                [input.code.clone().into(), id.into()],
            ))
            .await?;
        if dup.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Le code position « {} » est déjà utilisé.",
                input.code
            )]));
        }

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE positions SET code = ?, name = ?, category = ?, is_active = ?, updated_at = ? WHERE id = ?",
            [
                input.code.clone().into(),
                input.name.clone().into(),
                category.clone().into(),
                (if input.is_active.unwrap_or(true) { 1_i64 } else { 0_i64 }).into(),
                now.clone().into(),
                id.into(),
            ],
        ))
        .await?;

        (id, ex_code, ex_name)
    } else {
        // Create path
        let dup = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM positions WHERE code = ? LIMIT 1",
                [input.code.clone().into()],
            ))
            .await?;
        if dup.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Le code position « {} » est déjà utilisé.",
                input.code
            )]));
        }

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO positions (code, name, category, is_active, created_at, updated_at) VALUES (?, ?, ?, 1, ?, ?)",
            [
                input.code.clone().into(),
                input.name.clone().into(),
                category.clone().into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;

        let id_row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("position insert rowid")))?;
        let new_id: i64 = id_row.try_get("", "id").unwrap();
        (new_id, input.code.clone(), input.name.clone())
    };

    // Sync requirement profile if provided
    if let Some(ref rp) = input.requirement_profile {
        sync_position_requirement_profile(&txn, pos_id, rp, &now).await?;
    }

    // Emit position change event
    let event_type = if input.id.is_some() { "updated" } else { "created" };
    let summary = if input.id.is_some() {
        format!("Position updated: {} → {}", old_code, input.code)
    } else {
        format!("Position created: {} — {}", input.code, input.name)
    };
    emit_position_change_event(&txn, pos_id, event_type, &summary, None, actor_id, &now).await?;

    let _ = (old_code, old_name); // suppress unused warnings

    txn.commit().await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, category, requirement_profile_id, is_active, created_at, updated_at \
             FROM positions WHERE id = ?",
            [pos_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("position after upsert")))?;
    map_position_row(&row)
}

pub async fn archive_position(db: &DatabaseConnection, id: i64, actor_id: Option<i64>) -> AppResult<Position> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let txn = db.begin().await?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE positions SET is_active = 0, updated_at = ? WHERE id = ?",
            [now.clone().into(), id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "Position".into(),
            id: id.to_string(),
        });
    }

    emit_position_change_event(&txn, id, "archived", "Position archived", None, actor_id, &now).await?;

    txn.commit().await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, category, requirement_profile_id, is_active, created_at, updated_at \
             FROM positions WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Position".into(),
            id: id.to_string(),
        })?;
    map_position_row(&row)
}

fn map_position_row(row: &sea_orm::QueryResult) -> AppResult<Position> {
    Ok(Position {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        code: row.try_get("", "code").map_err(|e| map_err("code", e))?,
        name: row.try_get("", "name").map_err(|e| map_err("name", e))?,
        category: row.try_get("", "category").map_err(|e| map_err("category", e))?,
        requirement_profile_id: row
            .try_get("", "requirement_profile_id")
            .map_err(|e| map_err("requirement_profile_id", e))?,
        is_active: row.try_get("", "is_active").map_err(|e| map_err("is_active", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| map_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| map_err("updated_at", e))?,
    })
}

pub async fn create_position(
    db: &DatabaseConnection,
    code: String,
    name: String,
    category: String,
) -> AppResult<Position> {
    upsert_position(
        db,
        PositionUpsertInput {
            id: None,
            code,
            name,
            category,
            is_active: Some(true),
            requirement_profile: None,
        },
        None,
    )
    .await
}

// ── Internal position helpers ─────────────────────────────────────────────────

async fn sync_position_requirement_profile(
    db: &impl ConnectionTrait,
    pos_id: i64,
    rp: &PositionRequirementProfileInput,
    now: &str,
) -> AppResult<()> {
    // Upsert qualification_requirement_profiles row
    let existing_profile_id: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM qualification_requirement_profiles WHERE id = \
             (SELECT requirement_profile_id FROM positions WHERE id = ?) LIMIT 1",
            [pos_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "id").ok());

    let profile_id: i64 = if let Some(pid) = existing_profile_id {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE qualification_requirement_profiles SET profile_name = ?, \
             required_certification_type_ids_json = ?, row_version = row_version + 1 WHERE id = ?",
            [
                rp.profile_name.clone().into(),
                serde_json::to_string(&rp.certification_type_ids)
                    .unwrap_or_default()
                    .into(),
                pid.into(),
            ],
        ))
        .await?;
        pid
    } else {
        let entity_sync_id = crate::sync::identity::new_entity_sync_id();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO qualification_requirement_profiles (entity_sync_id, profile_name, required_certification_type_ids_json, \
              applies_to_permit_type_codes_json, row_version) \
             VALUES (?, ?, ?, '[]', 1)",
            [
                entity_sync_id.into(),
                rp.profile_name.clone().into(),
                serde_json::to_string(&rp.certification_type_ids)
                    .unwrap_or_default()
                    .into(),
            ],
        ))
        .await?;
        let id_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("profile insert rowid")))?;
        let new_id: i64 = id_row.try_get("", "id").unwrap();
        // Link profile to position
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE positions SET requirement_profile_id = ?, updated_at = ? WHERE id = ?",
            [new_id.into(), now.into(), pos_id.into()],
        ))
        .await?;
        new_id
    };

    // Replace qualification_profile_skills rows
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM qualification_profile_skills WHERE profile_id = ?",
        [profile_id.into()],
    ))
    .await?;
    for rv_id in &rp.skill_reference_value_ids {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO qualification_profile_skills \
             (profile_id, reference_value_id, min_proficiency_level, is_required, created_at) \
             VALUES (?, ?, 1, 1, ?)",
            [profile_id.into(), (*rv_id).into(), now.into()],
        ))
        .await?;
    }

    Ok(())
}

async fn emit_position_change_event(
    db: &impl ConnectionTrait,
    position_id: i64,
    event_type: &str,
    summary: &str,
    detail_json: Option<&str>,
    changed_by_id: Option<i64>,
    now: &str,
) -> AppResult<()> {
    let _ = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO position_change_events \
             (position_id, event_type, summary, detail_json, changed_by_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
            [
                position_id.into(),
                event_type.to_string().into(),
                summary.to_string().into(),
                detail_json
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                changed_by_id.into(),
                now.to_string().into(),
            ],
        ))
        .await;
    Ok(())
}

async fn load_position_skill_ids(db: &DatabaseConnection, profile_id: Option<i64>) -> AppResult<Vec<i64>> {
    let Some(pid) = profile_id else { return Ok(vec![]) };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reference_value_id FROM qualification_profile_skills WHERE profile_id = ?",
            [pid.into()],
        ))
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<i64>("", "reference_value_id").ok())
        .collect())
}

async fn load_position_cert_ids(db: &DatabaseConnection, profile_id: Option<i64>) -> AppResult<Vec<i64>> {
    let Some(pid) = profile_id else { return Ok(vec![]) };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT required_certification_type_ids_json FROM qualification_requirement_profiles WHERE id = ?",
            [pid.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(vec![]) };
    let json: String = row
        .try_get("", "required_certification_type_ids_json")
        .unwrap_or_default();
    Ok(serde_json::from_str::<Vec<i64>>(&json).unwrap_or_default())
}

async fn load_position_change_events(db: &DatabaseConnection, position_id: i64) -> AppResult<Vec<PositionChangeEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, position_id, event_type, summary, detail_json, changed_by_id, created_at \
             FROM position_change_events WHERE position_id = ? ORDER BY created_at DESC LIMIT 50",
            [position_id.into()],
        ))
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| PositionChangeEvent {
            id: r.try_get("", "id").unwrap_or_default(),
            position_id: r.try_get("", "position_id").unwrap_or_default(),
            event_type: r.try_get("", "event_type").unwrap_or_default(),
            summary: r.try_get("", "summary").unwrap_or_default(),
            detail_json: r.try_get("", "detail_json").unwrap_or(None),
            changed_by_id: r.try_get("", "changed_by_id").unwrap_or(None),
            created_at: r.try_get("", "created_at").unwrap_or_default(),
        })
        .collect())
}

/// Snapshot of skills + certs for a position's requirement profile (for create dialog seed).
pub async fn get_position_requirement_seed(
    db: &DatabaseConnection,
    position_id: i64,
) -> AppResult<PositionRequirementSeed> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requirement_profile_id FROM positions WHERE id = ? LIMIT 1",
            [position_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "Position".into(),
            id: position_id.to_string(),
        });
    };
    let profile_id = row.try_get::<Option<i64>>("", "requirement_profile_id").unwrap_or(None);
    let skill_ids = load_position_skill_ids(db, profile_id).await?;
    let cert_ids = load_position_cert_ids(db, profile_id).await?;
    Ok(PositionRequirementSeed {
        skill_reference_value_ids: skill_ids,
        certification_type_ids: cert_ids,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// I) schedule classes + details
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_schedule_classes(db: &DatabaseConnection) -> AppResult<Vec<ScheduleClassWithDetails>> {
    // Thin adapter: ORG.SCHEDULE_CLASS reference values + schedule_details.
    let classes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.label AS name, rv.code, rv.is_active, rv.metadata_json \
             FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.is_active = 1 \
             ORDER BY rv.label ASC"
                .to_string(),
        ))
        .await?;

    let mut out: Vec<ScheduleClassWithDetails> = Vec::new();
    for crow in classes {
        let id: i64 = crow.try_get("", "id").map_err(|e| map_err("sc id", e))?;
        let details = load_schedule_details(db, id).await?;
        out.push(ScheduleClassWithDetails {
            class: map_schedule_class_row(&crow)?,
            details,
        });
    }
    Ok(out)
}

fn map_schedule_class_row(row: &sea_orm::QueryResult) -> AppResult<ScheduleClass> {
    let code: String = row.try_get("", "code").map_err(|e| map_err("code", e))?;
    let metadata_json: Option<String> = row
        .try_get("", "metadata_json")
        .map_err(|e| map_err("metadata_json", e))?;
    let (shift_pattern_code, is_continuous, nominal_hours_per_day) =
        parse_schedule_class_metadata(metadata_json.as_deref(), &code);

    Ok(ScheduleClass {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        name: row.try_get("", "name").map_err(|e| map_err("name", e))?,
        shift_pattern_code,
        is_continuous,
        nominal_hours_per_day,
        is_active: row.try_get("", "is_active").map_err(|e| map_err("is_active", e))?,
        // reference_values has no created_at; keep IPC field for ScheduleClass compatibility.
        created_at: String::new(),
    })
}

fn parse_schedule_class_metadata(raw: Option<&str>, fallback_code: &str) -> (String, i64, Option<f64>) {
    #[derive(serde::Deserialize)]
    struct Meta {
        #[serde(default)]
        shift_pattern_code: Option<String>,
        #[serde(default)]
        is_continuous: Option<bool>,
        #[serde(default)]
        nominal_hours_per_day: Option<f64>,
    }
    let parsed = raw.and_then(|s| serde_json::from_str::<Meta>(s).ok()).unwrap_or(Meta {
        shift_pattern_code: None,
        is_continuous: None,
        nominal_hours_per_day: None,
    });
    (
        parsed
            .shift_pattern_code
            .filter(|c| !c.trim().is_empty())
            .unwrap_or_else(|| fallback_code.to_string()),
        i64::from(parsed.is_continuous.unwrap_or(false)),
        parsed.nominal_hours_per_day,
    )
}

async fn load_schedule_details(db: &DatabaseConnection, reference_value_id: i64) -> AppResult<Vec<ScheduleDetail>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, reference_value_id, day_of_week, shift_start, shift_end, is_rest_day \
             FROM schedule_details WHERE reference_value_id = ? ORDER BY day_of_week ASC",
            [reference_value_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_schedule_detail_row(&r)?);
    }
    Ok(out)
}

fn map_schedule_detail_row(row: &sea_orm::QueryResult) -> AppResult<ScheduleDetail> {
    Ok(ScheduleDetail {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        reference_value_id: row
            .try_get("", "reference_value_id")
            .map_err(|e| map_err("reference_value_id", e))?,
        day_of_week: row.try_get("", "day_of_week").map_err(|e| map_err("day_of_week", e))?,
        shift_start: row.try_get("", "shift_start").map_err(|e| map_err("shift_start", e))?,
        shift_end: row.try_get("", "shift_end").map_err(|e| map_err("shift_end", e))?,
        is_rest_day: row.try_get("", "is_rest_day").map_err(|e| map_err("is_rest_day", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// J/K/L) rate cards
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_rate_cards(db: &DatabaseConnection, personnel_id: i64) -> AppResult<Vec<PersonnelRateCard>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, effective_from, labor_rate, overtime_rate, cost_center_id, source_type, created_at \
             FROM personnel_rate_cards WHERE personnel_id = ? ORDER BY effective_from DESC",
            [personnel_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_rate_card_row(&r)?);
    }
    Ok(out)
}

fn map_rate_card_row(row: &sea_orm::QueryResult) -> AppResult<PersonnelRateCard> {
    Ok(PersonnelRateCard {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        personnel_id: row
            .try_get("", "personnel_id")
            .map_err(|e| map_err("personnel_id", e))?,
        effective_from: row
            .try_get("", "effective_from")
            .map_err(|e| map_err("effective_from", e))?,
        labor_rate: row.try_get("", "labor_rate").map_err(|e| map_err("labor_rate", e))?,
        overtime_rate: row
            .try_get("", "overtime_rate")
            .map_err(|e| map_err("overtime_rate", e))?,
        cost_center_id: row
            .try_get("", "cost_center_id")
            .map_err(|e| map_err("cost_center_id", e))?,
        source_type: row.try_get("", "source_type").map_err(|e| map_err("source_type", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| map_err("created_at", e))?,
    })
}

pub async fn get_active_rate_card(
    db: &impl ConnectionTrait,
    personnel_id: i64,
) -> AppResult<Option<PersonnelRateCard>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, effective_from, labor_rate, overtime_rate, cost_center_id, source_type, created_at \
             FROM personnel_rate_cards \
             WHERE personnel_id = ? AND date(effective_from) <= date('now') \
             ORDER BY effective_from DESC LIMIT 1",
            [personnel_id.into()],
        ))
        .await?;
    row.map(|r| map_rate_card_row(&r)).transpose()
}

/// Regular labor rate from the active rate card (effective_from ≤ today), if any.
pub async fn resolve_active_labor_rate(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<Option<f64>> {
    Ok(get_active_rate_card(db, personnel_id).await?.map(|c| c.labor_rate))
}

/// Labour and overtime rates of the rate card valid on `day` (`YYYY-MM-DD`): the latest card
/// effective on or before that day. Labour booked for a past day is priced at that day's card.
pub async fn resolve_rate_pair_on(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    day: &str,
) -> AppResult<Option<(f64, f64)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT labor_rate, overtime_rate FROM personnel_rate_cards WHERE personnel_id = ? AND date(effective_from) <= date(?) ORDER BY effective_from DESC, id DESC LIMIT 1",
            [personnel_id.into(), day.into()],
        ))
        .await?;
    row.map(|r| -> AppResult<(f64, f64)> {
        Ok((
            r.try_get("", "labor_rate").map_err(|e| map_err("labor_rate", e))?,
            r.try_get("", "overtime_rate")
                .map_err(|e| map_err("overtime_rate", e))?,
        ))
    })
    .transpose()
}

/// Active labor + overtime rates for cost range estimates.
pub async fn resolve_active_rate_pair(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<Option<(f64, f64)>> {
    Ok(get_active_rate_card(db, personnel_id)
        .await?
        .map(|c| (c.labor_rate, c.overtime_rate)))
}

pub async fn create_rate_card(
    db: &DatabaseConnection,
    personnel_id: i64,
    labor_rate: f64,
    overtime_rate: f64,
    cost_center_id: Option<i64>,
    source_type: String,
    actor_id: i64,
) -> AppResult<PersonnelRateCard> {
    let effective_from = Utc::now().format("%Y-%m-%d").to_string();
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_rate_cards \
            (personnel_id, effective_from, labor_rate, overtime_rate, cost_center_id, source_type, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            personnel_id.into(),
            effective_from.into(),
            labor_rate.into(),
            overtime_rate.into(),
            cost_center_id.into(),
            source_type.into(),
            now.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, effective_from, labor_rate, overtime_rate, cost_center_id, source_type, created_at \
             FROM personnel_rate_cards WHERE id = last_insert_rowid()"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("rate card insert")))?;

    let rc = map_rate_card_row(&row)?;
    emit_personnel_event(db, "personnel.rate_card.created", personnel_id, Some(actor_id)).await?;
    Ok(rc)
}

// ═══════════════════════════════════════════════════════════════════════════════
// M/N) authorizations
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_authorizations(db: &DatabaseConnection, personnel_id: i64) -> AppResult<Vec<PersonnelAuthorization>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, authorization_type, valid_from, valid_to, \
                    source_certification_type_id, is_active, created_at \
             FROM personnel_authorizations WHERE personnel_id = ? ORDER BY valid_from DESC",
            [personnel_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_auth_row(&r)?);
    }
    Ok(out)
}

fn map_auth_row(row: &sea_orm::QueryResult) -> AppResult<PersonnelAuthorization> {
    Ok(PersonnelAuthorization {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        personnel_id: row
            .try_get("", "personnel_id")
            .map_err(|e| map_err("personnel_id", e))?,
        authorization_type: row
            .try_get("", "authorization_type")
            .map_err(|e| map_err("authorization_type", e))?,
        valid_from: row.try_get("", "valid_from").map_err(|e| map_err("valid_from", e))?,
        valid_to: row.try_get("", "valid_to").map_err(|e| map_err("valid_to", e))?,
        source_certification_type_id: row
            .try_get("", "source_certification_type_id")
            .map_err(|e| map_err("source_certification_type_id", e))?,
        is_active: row.try_get("", "is_active").map_err(|e| map_err("is_active", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| map_err("created_at", e))?,
    })
}

pub async fn create_authorization(
    db: &DatabaseConnection,
    personnel_id: i64,
    authorization_type: String,
    valid_from: String,
    valid_to: Option<String>,
    source_certification_type_id: Option<i64>,
    actor_id: i64,
) -> AppResult<PersonnelAuthorization> {
    AuthorizationType::try_from(authorization_type.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_authorizations \
            (personnel_id, authorization_type, valid_from, valid_to, source_certification_type_id, is_active, created_at) \
         VALUES (?, ?, ?, ?, ?, 1, ?)",
        [
            personnel_id.into(),
            authorization_type.into(),
            valid_from.into(),
            valid_to.into(),
            source_certification_type_id.into(),
            now.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, authorization_type, valid_from, valid_to, \
                    source_certification_type_id, is_active, created_at \
             FROM personnel_authorizations WHERE id = last_insert_rowid()"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("authorization insert")))?;

    let a = map_auth_row(&row)?;
    emit_personnel_event(db, "personnel.authorization.created", personnel_id, Some(actor_id)).await?;
    Ok(a)
}

// ═══════════════════════════════════════════════════════════════════════════════
// O/P/Q) external companies
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_external_companies(
    db: &DatabaseConnection,
    filter: CompanyListFilter,
) -> AppResult<Vec<ExternalCompany>> {
    let mut where_clauses = vec!["1 = 1".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref s) = filter.onboarding_status {
        where_clauses.push("onboarding_status = ?".into());
        binds.push(s.clone().into());
    }
    if let Some(ref q) = filter.search {
        let term = q.trim();
        if !term.is_empty() {
            let like = format!("%{term}%");
            where_clauses.push("(name LIKE ? OR IFNULL(service_domain,'') LIKE ?)".into());
            binds.push(like.clone().into());
            binds.push(like.into());
        }
    }

    let sql = format!(
        "SELECT id, name, service_domain, contract_start, contract_end, onboarding_status, insurance_status, \
                notes, is_active, created_at, updated_at \
         FROM external_companies WHERE {} ORDER BY name ASC",
        where_clauses.join(" AND ")
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_external_company_row(&r)?);
    }
    Ok(out)
}

fn map_external_company_row(row: &sea_orm::QueryResult) -> AppResult<ExternalCompany> {
    Ok(ExternalCompany {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        name: row.try_get("", "name").map_err(|e| map_err("name", e))?,
        service_domain: row
            .try_get("", "service_domain")
            .map_err(|e| map_err("service_domain", e))?,
        contract_start: row
            .try_get("", "contract_start")
            .map_err(|e| map_err("contract_start", e))?,
        contract_end: row
            .try_get("", "contract_end")
            .map_err(|e| map_err("contract_end", e))?,
        onboarding_status: row
            .try_get("", "onboarding_status")
            .map_err(|e| map_err("onboarding_status", e))?,
        insurance_status: row
            .try_get("", "insurance_status")
            .map_err(|e| map_err("insurance_status", e))?,
        notes: row.try_get("", "notes").map_err(|e| map_err("notes", e))?,
        is_active: row.try_get("", "is_active").map_err(|e| map_err("is_active", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| map_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| map_err("updated_at", e))?,
    })
}

pub async fn create_external_company(
    db: &DatabaseConnection,
    name: String,
    service_domain: Option<String>,
    contract_start: Option<String>,
    contract_end: Option<String>,
    notes: Option<String>,
) -> AppResult<ExternalCompany> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO external_companies \
            (name, service_domain, contract_start, contract_end, notes, onboarding_status, insurance_status, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 'pending', 'unknown', 1, ?, ?)",
        [
            name.into(),
            service_domain.into(),
            contract_start.into(),
            contract_end.into(),
            notes.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, name, service_domain, contract_start, contract_end, onboarding_status, insurance_status, \
                    notes, is_active, created_at, updated_at \
             FROM external_companies WHERE id = last_insert_rowid()"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("external company insert")))?;

    map_external_company_row(&row)
}

/// Contractor company edit: contract dates, onboarding and insurance status (an expired
/// insurance or a suspended contractor is visible on the company and its people).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExternalCompanyUpdateInput {
    pub id: i64,
    pub name: String,
    pub service_domain: Option<String>,
    pub contract_start: Option<String>,
    pub contract_end: Option<String>,
    pub onboarding_status: String,
    pub insurance_status: String,
    pub notes: Option<String>,
    pub is_active: bool,
}

pub async fn update_external_company(
    db: &DatabaseConnection,
    input: ExternalCompanyUpdateInput,
) -> AppResult<ExternalCompany> {
    let issue = |code: &str, message: &str| {
        crate::errors::org_validation_failed(code, message, std::collections::HashMap::new())
    };
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(issue("COMPANY_NAME_REQUIRED", "The company name is required."));
    }
    super::domain::OnboardingStatus::try_from(input.onboarding_status.as_str())
        .map_err(|_| issue("COMPANY_STATUS_UNKNOWN", "Unknown onboarding status."))?;
    super::domain::InsuranceStatus::try_from(input.insurance_status.as_str())
        .map_err(|_| issue("COMPANY_STATUS_UNKNOWN", "Unknown insurance status."))?;
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let (start, end) = (clean(input.contract_start), clean(input.contract_end));
    if let (Some(s), Some(e)) = (&start, &end) {
        if e < s {
            return Err(issue("COMPANY_CONTRACT_DATES", "The contract end is before its start."));
        }
    }
    let res = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE external_companies SET name = ?, service_domain = ?, contract_start = ?, contract_end = ?, \
             onboarding_status = ?, insurance_status = ?, notes = ?, is_active = ?, updated_at = ? WHERE id = ?",
            [
                name.into(),
                clean(input.service_domain).into(),
                start.into(),
                end.into(),
                input.onboarding_status.into(),
                input.insurance_status.into(),
                clean(input.notes).into(),
                i64::from(input.is_active).into(),
                Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string().into(),
                input.id.into(),
            ],
        ))
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "ExternalCompany".into(),
            id: input.id.to_string(),
        });
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, service_domain, contract_start, contract_end, onboarding_status, insurance_status, \
                    notes, is_active, created_at, updated_at \
             FROM external_companies WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ExternalCompany".into(),
            id: input.id.to_string(),
        })?;
    map_external_company_row(&row)
}

pub async fn list_company_contacts(db: &DatabaseConnection, company_id: i64) -> AppResult<Vec<ExternalCompanyContact>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, company_id, contact_name, contact_role, phone, email, is_primary, created_at \
             FROM external_company_contacts WHERE company_id = ? ORDER BY contact_name ASC",
            [company_id.into()],
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_contact_row(&r)?);
    }
    Ok(out)
}

fn map_contact_row(row: &sea_orm::QueryResult) -> AppResult<ExternalCompanyContact> {
    Ok(ExternalCompanyContact {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        company_id: row.try_get("", "company_id").map_err(|e| map_err("company_id", e))?,
        contact_name: row
            .try_get("", "contact_name")
            .map_err(|e| map_err("contact_name", e))?,
        contact_role: row
            .try_get("", "contact_role")
            .map_err(|e| map_err("contact_role", e))?,
        phone: row.try_get("", "phone").map_err(|e| map_err("phone", e))?,
        email: row.try_get("", "email").map_err(|e| map_err("email", e))?,
        is_primary: row.try_get("", "is_primary").map_err(|e| map_err("is_primary", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| map_err("created_at", e))?,
    })
}

pub async fn list_personnel_team_assignments(
    db: &DatabaseConnection,
    personnel_id: i64,
) -> AppResult<Vec<PersonnelTeamAssignment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                pta.id,
                pta.personnel_id,
                pta.team_id,
                t.code AS team_code,
                t.name AS team_name,
                pta.role_code,
                pta.allocation_percent,
                pta.valid_from,
                pta.valid_to,
                pta.is_lead,
                pta.created_at,
                pta.updated_at
             FROM personnel_team_assignments pta
             LEFT JOIN teams t ON t.id = pta.team_id
             WHERE pta.personnel_id = ?
             ORDER BY COALESCE(pta.valid_to, '9999-12-31') DESC, pta.id DESC",
            [personnel_id.into()],
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| PersonnelTeamAssignment {
            id: row.try_get("", "id").unwrap_or_default(),
            personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
            team_id: row.try_get("", "team_id").unwrap_or_default(),
            team_code: row.try_get("", "team_code").unwrap_or(None),
            team_name: row.try_get("", "team_name").unwrap_or(None),
            role_code: row.try_get("", "role_code").unwrap_or_else(|_| "member".to_string()),
            allocation_percent: row.try_get("", "allocation_percent").unwrap_or(100.0),
            valid_from: row.try_get("", "valid_from").unwrap_or(None),
            valid_to: row.try_get("", "valid_to").unwrap_or(None),
            is_lead: row.try_get("", "is_lead").unwrap_or_default(),
            created_at: row.try_get("", "created_at").unwrap_or_default(),
            updated_at: row.try_get("", "updated_at").unwrap_or_default(),
        })
        .collect())
}

/// Current (open) members of an operational crew.
pub async fn list_team_members(
    db: &DatabaseConnection,
    team_id: i64,
) -> AppResult<Vec<crate::personnel::domain::TeamMemberRow>> {
    // Ensure the crew exists (may be inactive — still allow roster inspection).
    let _ = crate::repository::team_repository::get_team_by_id(db, team_id).await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                pta.id AS assignment_id,
                p.id AS personnel_id,
                p.employee_code,
                p.full_name,
                pos.name AS position_name,
                pta.role_code,
                pta.allocation_percent,
                pta.valid_from,
                pta.valid_to,
                pta.is_lead,
                p.availability_status,
                (
                  SELECT ua.id
                  FROM user_accounts ua
                  WHERE ua.personnel_id = p.id
                    AND ua.deleted_at IS NULL
                  ORDER BY ua.id
                  LIMIT 1
                ) AS user_account_id
             FROM personnel_team_assignments pta
             INNER JOIN personnel p ON p.id = pta.personnel_id
             LEFT JOIN positions pos ON pos.id = p.position_id
             WHERE pta.team_id = ?
               AND (pta.valid_to IS NULL OR pta.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
               AND p.employment_status = 'active'
             ORDER BY pta.is_lead DESC, p.full_name ASC, pta.id ASC",
            [team_id.into()],
        ))
        .await?;

    Ok(rows.into_iter().map(|row| map_team_member_row(&row)).collect())
}

fn map_team_member_row(row: &sea_orm::QueryResult) -> crate::personnel::domain::TeamMemberRow {
    crate::personnel::domain::TeamMemberRow {
        assignment_id: row.try_get("", "assignment_id").unwrap_or_default(),
        personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
        employee_code: row.try_get("", "employee_code").unwrap_or_default(),
        full_name: row.try_get("", "full_name").unwrap_or_default(),
        position_name: row.try_get("", "position_name").unwrap_or(None),
        role_code: row.try_get("", "role_code").unwrap_or_else(|_| "member".to_string()),
        allocation_percent: row.try_get("", "allocation_percent").unwrap_or(100.0),
        valid_from: row.try_get("", "valid_from").unwrap_or(None),
        valid_to: row.try_get("", "valid_to").unwrap_or(None),
        is_lead: row.try_get("", "is_lead").unwrap_or_default(),
        availability_status: row
            .try_get("", "availability_status")
            .unwrap_or_else(|_| "available".to_string()),
        user_account_id: row.try_get("", "user_account_id").unwrap_or(None),
    }
}

/// Promote or clear crew lead on an open assignment.
/// Does **not** touch `personnel.primary_team_id` or call `sync_home_crew_assignment`.
/// Enforces a single lead per crew when promoting.
pub async fn set_team_lead(
    db: &DatabaseConnection,
    input: crate::personnel::domain::SetTeamLeadInput,
) -> AppResult<crate::personnel::domain::TeamMemberRow> {
    let _ = crate::repository::team_repository::get_team_by_id(db, input.team_id).await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let txn = db.begin().await?;

    let open = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM personnel_team_assignments
             WHERE personnel_id = ?
               AND team_id = ?
               AND (valid_to IS NULL OR date(valid_to) >= date(?))
             ORDER BY id DESC
             LIMIT 1",
            [input.personnel_id.into(), input.team_id.into(), now.clone().into()],
        ))
        .await?;

    if open.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Cette personne n'est pas membre actif de cette équipe.".into(),
        ]));
    }

    if input.is_lead {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_team_assignments
             SET is_lead = 0,
                 role_code = 'member',
                 updated_at = ?
             WHERE team_id = ?
               AND personnel_id <> ?
               AND is_lead = 1
               AND (valid_to IS NULL OR date(valid_to) >= date(?))",
            [
                now.clone().into(),
                input.team_id.into(),
                input.personnel_id.into(),
                now.clone().into(),
            ],
        ))
        .await?;

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_team_assignments
             SET is_lead = 1,
                 role_code = 'lead',
                 updated_at = ?
             WHERE personnel_id = ?
               AND team_id = ?
               AND (valid_to IS NULL OR date(valid_to) >= date(?))",
            [
                now.clone().into(),
                input.personnel_id.into(),
                input.team_id.into(),
                now.clone().into(),
            ],
        ))
        .await?;
    } else {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_team_assignments
             SET is_lead = 0,
                 role_code = 'member',
                 updated_at = ?
             WHERE personnel_id = ?
               AND team_id = ?
               AND (valid_to IS NULL OR date(valid_to) >= date(?))",
            [
                now.clone().into(),
                input.personnel_id.into(),
                input.team_id.into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }

    txn.commit().await?;

    let members = list_team_members(db, input.team_id).await?;
    members
        .into_iter()
        .find(|m| m.personnel_id == input.personnel_id)
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["Cette personne n'est pas membre actif de cette équipe.".into()])
        })
}

pub async fn list_personnel_availability_blocks(
    db: &DatabaseConnection,
    personnel_id: i64,
    limit: i64,
) -> AppResult<Vec<PersonnelAvailabilityBlock>> {
    let row_limit = limit.clamp(1, 200);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                b.id,
                b.personnel_id,
                b.block_type,
                b.start_at,
                b.end_at,
                b.reason_note,
                b.is_critical,
                b.created_by_id,
                COALESCE(NULLIF(TRIM(ua.display_name), ''), ua.username) AS created_by_label,
                b.created_at
             FROM personnel_availability_blocks b
             LEFT JOIN user_accounts ua ON ua.id = b.created_by_id
             WHERE b.personnel_id = ?
             ORDER BY b.start_at DESC
             LIMIT ?",
            [personnel_id.into(), row_limit.into()],
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| PersonnelAvailabilityBlock {
            id: row.try_get("", "id").unwrap_or_default(),
            personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
            block_type: row.try_get("", "block_type").unwrap_or_default(),
            start_at: row.try_get("", "start_at").unwrap_or_default(),
            end_at: row.try_get("", "end_at").unwrap_or_default(),
            reason_note: row.try_get("", "reason_note").unwrap_or(None),
            is_critical: row.try_get::<i64>("", "is_critical").unwrap_or(0) == 1,
            created_by_id: row.try_get("", "created_by_id").unwrap_or(None),
            created_by_label: row.try_get("", "created_by_label").unwrap_or(None),
            created_at: row.try_get("", "created_at").unwrap_or_default(),
            reason_masked: false,
        })
        .collect())
}

pub async fn list_personnel_work_history(
    db: &DatabaseConnection,
    personnel_id: i64,
    limit: i64,
) -> AppResult<Vec<PersonnelWorkHistoryEntry>> {
    let row_limit = limit.clamp(1, 300);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT * FROM (
                SELECT
                    'wo' AS source_module,
                    wo.id AS record_id,
                    wo.code AS record_code,
                    CASE
                        WHEN wo.primary_responsible_id IN
                            (SELECT u.id FROM user_accounts u WHERE u.personnel_id = ?) THEN 'primary_responsible'
                        ELSE 'intervener'
                    END AS role_code,
                    wos.code AS status_code,
                    wo.title AS title,
                    COALESCE(wo.actual_end, wo.updated_at, wo.created_at) AS happened_at
                FROM work_orders wo
                LEFT JOIN work_order_statuses wos ON wos.id = wo.status_id
                WHERE {PERSON_ON_WO}

                UNION ALL

                SELECT
                    'di' AS source_module,
                    di.id AS record_id,
                    di.code AS record_code,
                    CASE
                        WHEN di.submitter_id = ua2.id THEN 'submitter'
                        WHEN di.reviewer_id = ua2.id THEN 'reviewer'
                        ELSE 'participant'
                    END AS role_code,
                    di.status AS status_code,
                    di.title AS title,
                    COALESCE(di.closed_at, di.updated_at, di.created_at) AS happened_at
                FROM user_accounts ua2
                JOIN intervention_requests di ON (
                    di.submitter_id = ua2.id OR di.reviewer_id = ua2.id
                )
                WHERE ua2.personnel_id = ?
            ) h
            ORDER BY h.happened_at DESC
            LIMIT ?"
            ),
            [
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                row_limit.into(),
            ],
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| PersonnelWorkHistoryEntry {
            source_module: row.try_get("", "source_module").unwrap_or_default(),
            record_id: row.try_get("", "record_id").unwrap_or_default(),
            record_code: row.try_get("", "record_code").unwrap_or(None),
            role_code: row.try_get("", "role_code").unwrap_or_default(),
            status_code: row.try_get("", "status_code").unwrap_or(None),
            title: row.try_get("", "title").unwrap_or_default(),
            happened_at: row.try_get("", "happened_at").unwrap_or_default(),
        })
        .collect())
}

pub async fn get_personnel_workload_summary(
    db: &DatabaseConnection,
    personnel_id: i64,
) -> AppResult<PersonnelWorkloadSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT
                COALESCE((
                    SELECT COUNT(DISTINCT wo.id)
                    FROM work_orders wo
                    JOIN work_order_statuses wos ON wos.id = wo.status_id
                    WHERE {PERSON_ON_WO}
                      AND wos.code NOT IN {closed}
                ), 0) AS open_work_orders,
                COALESCE((
                    SELECT COUNT(DISTINCT wo.id)
                    FROM work_orders wo
                    JOIN work_order_statuses wos ON wos.id = wo.status_id
                    WHERE {PERSON_ON_WO}
                      AND wos.code IN {open}
                ), 0) AS in_progress_work_orders,
                COALESCE((
                    SELECT COUNT(DISTINCT ir.id)
                    FROM user_accounts ua2
                    JOIN intervention_requests ir ON (ir.submitter_id = ua2.id OR ir.reviewer_id = ua2.id)
                    WHERE ua2.personnel_id = ?
                      AND {di_open}
                ), 0) AS pending_interventions,
                COALESCE((
                    SELECT COUNT(*)
                    FROM intervention_requests di2
                    JOIN user_accounts ua3 ON ua3.id = di2.submitter_id
                    WHERE ua3.personnel_id = ?
                      AND date(di2.created_at) >= date('now', '-30 day')
                ), 0) AS interventions_last_30d",
                closed = crate::wo::backlog::CLOSED_STATUS_SQL,
                open = crate::wo::backlog::OPEN_STATUS_SQL,
                di_open = crate::di::stats::OPEN_SQL,
            ),
            [
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("workload summary query failed")))?;

    Ok(PersonnelWorkloadSummary {
        open_work_orders_rule: crate::wo::backlog::RULE_NOT_YET_CLOSED.to_string(),
        in_progress_work_orders_rule: crate::wo::backlog::RULE_OPEN_WORK_TO_DO.to_string(),
        open_work_orders: row.try_get("", "open_work_orders")?,
        in_progress_work_orders: row.try_get("", "in_progress_work_orders")?,
        pending_interventions: row.try_get("", "pending_interventions")?,
        interventions_last_30d: row.try_get("", "interventions_last_30d")?,
    })
}

pub async fn scan_succession_risk(
    db: &DatabaseConnection,
    entity_id: Option<i64>,
    team_id: Option<i64>,
) -> AppResult<Vec<SuccessionRiskRow>> {
    let mut where_parts = vec!["p.availability_status <> 'inactive'".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(eid) = entity_id {
        where_parts.push("p.primary_entity_id = ?".to_string());
        binds.push(eid.into());
    }
    if let Some(tid) = team_id {
        where_parts.push("p.primary_team_id = ?".to_string());
        binds.push(tid.into());
    }

    let sql = format!(
        "SELECT
            p.id AS personnel_id,
            p.full_name,
            p.employee_code,
            pos.name AS position_name,
            team.name AS team_name,
            COUNT(ps.id) AS coverage_count,
            CASE
                WHEN COUNT(ps.id) = 0 THEN 'high'
                WHEN COUNT(ps.id) = 1 THEN 'medium'
                ELSE 'low'
            END AS risk_level,
            CASE
                WHEN COUNT(ps.id) = 0 THEN 'No validated skills mapped.'
                WHEN COUNT(ps.id) = 1 THEN 'Single skill coverage only.'
                ELSE 'Multiple skills available.'
            END AS reason
         FROM personnel p
         LEFT JOIN positions pos ON pos.id = p.position_id
         LEFT JOIN teams team ON team.id = p.primary_team_id
         LEFT JOIN personnel_skills ps
           ON ps.personnel_id = p.id
          AND (ps.valid_to IS NULL OR date(ps.valid_to) >= date('now'))
         WHERE {}
         GROUP BY p.id, p.full_name, p.employee_code, pos.name, team.name
         ORDER BY
            CASE
                WHEN COUNT(ps.id) = 0 THEN 0
                WHEN COUNT(ps.id) = 1 THEN 1
                ELSE 2
            END ASC,
            p.full_name ASC",
        where_parts.join(" AND ")
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| SuccessionRiskRow {
            personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
            full_name: row.try_get("", "full_name").unwrap_or_default(),
            employee_code: row.try_get("", "employee_code").unwrap_or_default(),
            position_name: row.try_get("", "position_name").unwrap_or(None),
            team_name: row.try_get("", "team_name").unwrap_or(None),
            coverage_count: row.try_get("", "coverage_count").unwrap_or_default(),
            risk_level: row.try_get("", "risk_level").unwrap_or_else(|_| "high".to_string()),
            reason: row.try_get("", "reason").unwrap_or_default(),
        })
        .collect())
}

pub async fn list_personnel_skill_reference_values(
    db: &DatabaseConnection,
) -> AppResult<Vec<PersonnelSkillReferenceValue>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code, rv.label
             FROM reference_values rv
             JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE rd.code = 'PERSONNEL.SKILLS'
               AND rv.is_active = 1
             ORDER BY rv.sort_order ASC, rv.label ASC"
                .to_string(),
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| PersonnelSkillReferenceValue {
            id: row.try_get("", "id").unwrap_or_default(),
            code: row.try_get("", "code").unwrap_or_default(),
            label: row.try_get("", "label").unwrap_or_default(),
        })
        .collect())
}
