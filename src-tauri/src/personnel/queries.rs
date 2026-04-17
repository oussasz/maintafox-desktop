//! Personnel SQL layer (PRD §6.6) — SeaORM `Statement` + `DatabaseConnection`, same pattern as DI/WO.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::activity::emitter::{emit_activity_event, ActivityEventInput};
use crate::errors::{AppError, AppResult};

use super::domain::{
    generate_personnel_code, AuthorizationType, AvailabilityStatus, CompanyListFilter, EmploymentType,
    ExternalCompany, ExternalCompanyContact, Personnel, PersonnelAuthorization, PersonnelAvailabilityBlock,
    PersonnelCreateInput, PersonnelDetailPayload, PersonnelListFilter, PersonnelListPage, PersonnelRateCard,
    PersonnelSkillReferenceValue, PersonnelTeamAssignment, PersonnelUpdateInput, PersonnelWorkHistoryEntry,
    PersonnelWorkloadSummary, Position, PositionCategory, ScheduleClass, ScheduleClassWithDetails, ScheduleDetail,
    SuccessionRiskRow,
};

// ═══════════════════════════════════════════════════════════════════════════════
// SELECT list — personnel + joins
// ═══════════════════════════════════════════════════════════════════════════════

const P_JOINS: &str = "\
    FROM personnel p \
    LEFT JOIN positions pos ON pos.id = p.position_id \
    LEFT JOIN org_nodes e ON e.id = p.primary_entity_id \
    LEFT JOIN org_nodes t ON t.id = p.primary_team_id \
    LEFT JOIN personnel s ON s.id = p.supervisor_id \
    LEFT JOIN schedule_classes sc ON sc.id = p.home_schedule_id \
    LEFT JOIN external_companies ec ON ec.id = p.external_company_id";

const P_SELECT: &str = "\
    SELECT \
    p.id, p.employee_code, p.full_name, p.employment_type, p.position_id, p.primary_entity_id, \
    p.primary_team_id, p.supervisor_id, p.home_schedule_id, p.availability_status, p.hire_date, \
    p.termination_date, p.email, p.phone, p.photo_path, p.hr_external_id, p.external_company_id, \
    p.notes, p.row_version, p.created_at, p.updated_at, \
    pos.name AS position_name, pos.category AS position_category, \
    e.name AS entity_name, t.name AS team_name, s.full_name AS supervisor_name, \
    sc.name AS schedule_name, ec.name AS company_name ";

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
        home_schedule_id: row
            .try_get::<Option<i64>>("", "home_schedule_id")
            .map_err(|e| map_err("home_schedule_id", e))?,
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
        position_name: row
            .try_get::<Option<String>>("", "position_name")
            .unwrap_or(None),
        position_category: row
            .try_get::<Option<String>>("", "position_category")
            .unwrap_or(None),
        entity_name: row
            .try_get::<Option<String>>("", "entity_name")
            .unwrap_or(None),
        team_name: row
            .try_get::<Option<String>>("", "team_name")
            .unwrap_or(None),
        supervisor_name: row
            .try_get::<Option<String>>("", "supervisor_name")
            .unwrap_or(None),
        schedule_name: row
            .try_get::<Option<String>>("", "schedule_name")
            .unwrap_or(None),
        company_name: row
            .try_get::<Option<String>>("", "company_name")
            .unwrap_or(None),
    })
}

fn map_err(col: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("personnel row decode {col}: {e}"))
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
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM org_nodes WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Nœud d'organisation (id={id}) introuvable."
        )]));
    }
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
    let limit = if filter.limit > 0 {
        filter.limit
    } else {
        100
    };
    let offset = filter.offset.max(0);

    let count_sql = format!(
        "SELECT COUNT(*) AS c {P_JOINS} WHERE {where_sql}"
    );
    let total: i64 = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &count_sql, binds.clone()))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count personnel")))?
        .try_get::<i64>("", "c")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count c: {e}")))?;

    let mut list_binds = binds;
    list_binds.push(limit.into());
    list_binds.push(offset.into());

    let list_sql = format!(
        "{P_SELECT} {P_JOINS} WHERE {where_sql} ORDER BY p.full_name ASC LIMIT ? OFFSET ?"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &list_sql,
            list_binds,
        ))
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
    EmploymentType::try_from(input.employment_type.as_str()).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    if input.employment_type == "contractor" || input.employment_type == "vendor" {
        if input.external_company_id.is_none() {
            tracing::warn!(
                "personnel.create: employment_type={} without external_company_id",
                input.employment_type
            );
        }
    }

    let txn = db.begin().await?;

    let code = match &input.employee_code {
        None => generate_personnel_code(&txn).await?,
        Some(c) => {
            let trimmed = c.trim();
            if trimmed.is_empty() {
                return Err(AppError::ValidationFailed(vec![
                    "Le code employé ne peut pas être vide.".into(),
                ]));
            }
            let dup = txn
                .query_one(
                    Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT 1 AS x FROM personnel WHERE employee_code = ? LIMIT 1",
                        [trimmed.to_string().into()],
                    ),
                )
                .await?;
            if dup.is_some() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Le code employé « {trimmed} » est déjà utilisé."
                )]));
            }
            trimmed.to_string()
        }
    };

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel (\
            employee_code, full_name, employment_type, position_id, primary_entity_id, primary_team_id, \
            supervisor_id, home_schedule_id, availability_status, hire_date, termination_date, \
            email, phone, external_company_id, notes, row_version, created_at, updated_at\
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'available', ?, NULL, ?, ?, ?, ?, 1, ?, ?)",
        [
            code.clone().into(),
            input.full_name.clone().into(),
            input.employment_type.clone().into(),
            input.position_id.into(),
            input.primary_entity_id.into(),
            input.primary_team_id.into(),
            input.supervisor_id.into(),
            input.home_schedule_id.into(),
            input.hire_date.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.email.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.phone.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            input.external_company_id.into(),
            input.notes.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            now.clone().into(),
            now.into(),
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

    txn.commit().await?;

    emit_personnel_event(db, "personnel.created", new_id, Some(actor_id)).await?;

    get_personnel(db, new_id).await?.ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!(
            "personnel row missing after insert id={new_id}"
        ))
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) update_personnel
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn update_personnel(
    db: &DatabaseConnection,
    input: PersonnelUpdateInput,
    actor_id: i64,
) -> AppResult<Personnel> {
    let current = get_personnel(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: input.id.to_string(),
        })?;

    if current.availability_status == "inactive" {
        let reactivates = match &input.availability_status {
            Some(s) if s.as_str() != "inactive" => {
                AvailabilityStatus::try_from(s.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;
                true
            }
            _ => false,
        };
        if !reactivates {
            return Err(AppError::ValidationFailed(vec![
                "Impossible de modifier un collaborateur inactif sans le réactiver (availability_status)."
                    .into(),
            ]));
        }
    }

    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref n) = input.full_name {
        sets.push("full_name = ?".into());
        values.push(n.clone().into());
    }
    if let Some(ref e) = input.employment_type {
        EmploymentType::try_from(e.as_str()).map_err(|err| AppError::ValidationFailed(vec![err]))?;
        sets.push("employment_type = ?".into());
        values.push(e.clone().into());
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
    if let Some(v) = input.home_schedule_id {
        sets.push("home_schedule_id = ?".into());
        values.push(v.into());
    }
    if let Some(ref s) = input.availability_status {
        AvailabilityStatus::try_from(s.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;
        sets.push("availability_status = ?".into());
        values.push(s.clone().into());
    }
    if let Some(ref h) = input.hire_date {
        sets.push("hire_date = ?".into());
        values.push(h.clone().into());
    }
    if let Some(ref t) = input.termination_date {
        sets.push("termination_date = ?".into());
        values.push(t.clone().into());
    }
    if let Some(ref e) = input.email {
        sets.push("email = ?".into());
        values.push(e.clone().into());
    }
    if let Some(ref p) = input.phone {
        sets.push("phone = ?".into());
        values.push(p.clone().into());
    }
    if let Some(v) = input.external_company_id {
        sets.push("external_company_id = ?".into());
        values.push(v.into());
    }
    if let Some(ref n) = input.notes {
        sets.push("notes = ?".into());
        values.push(n.clone().into());
    }

    if sets.is_empty() {
        return Ok(current);
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    sets.push("row_version = row_version + 1".into());
    sets.push("updated_at = ?".into());
    values.push(now.into());

    values.push(input.id.into());
    values.push(input.expected_row_version.into());

    let sql = format!(
        "UPDATE personnel SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );

    let result = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

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

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel SET \
                availability_status = 'inactive', \
                termination_date = COALESCE(termination_date, ?), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                today.into(),
                now.clone().into(),
                id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }

    emit_personnel_event(db, "personnel.deactivated", id, Some(actor_id)).await?;

    get_personnel(db, id).await?.ok_or_else(|| AppError::NotFound {
        entity: "Personnel".into(),
        id: id.to_string(),
    })
}

/// WO assignments via `user_accounts.personnel_id` (WO stores user ids, not personnel ids).
async fn collect_deactivation_blockers(db: &DatabaseConnection, personnel_id: i64) -> AppResult<String> {
    let mut parts: Vec<String> = Vec::new();

    let wo_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT wo.id, wo.code \
             FROM user_accounts ua \
             JOIN work_orders wo ON ( \
                wo.primary_responsible_id = ua.id \
                OR EXISTS ( \
                    SELECT 1 FROM work_order_interveners woi \
                    WHERE woi.work_order_id = wo.id AND woi.intervener_id = ua.id \
                ) \
             ) \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE ua.personnel_id = ? \
               AND wos.code NOT IN ('closed', 'cancelled')",
            [personnel_id.into()],
        ))
        .await?;

    if !wo_rows.is_empty() {
        let codes: Vec<String> = wo_rows
            .iter()
            .filter_map(|r| r.try_get::<String>("", "code").ok())
            .collect();
        parts.push(format!(
            "Ordres de travail actifs : {}.",
            codes.join(", ")
        ));
    }

    // Work permit schema (6.23) not yet in local DB — no SQL guard here.

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(
            "Désactivation impossible : {}",
            parts.join(" ")
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// G/H) positions
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_positions(db: &DatabaseConnection) -> AppResult<Vec<Position>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, category, requirement_profile_id, is_active, created_at, updated_at \
             FROM positions WHERE is_active = 1 ORDER BY name ASC"
                .to_string(),
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_position_row(&r)?);
    }
    Ok(out)
}

fn map_position_row(row: &sea_orm::QueryResult) -> AppResult<Position> {
    Ok(Position {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        code: row.try_get("", "code").map_err(|e| map_err("code", e))?,
        name: row.try_get("", "name").map_err(|e| map_err("name", e))?,
        category: row
            .try_get("", "category")
            .map_err(|e| map_err("category", e))?,
        requirement_profile_id: row
            .try_get("", "requirement_profile_id")
            .map_err(|e| map_err("requirement_profile_id", e))?,
        is_active: row.try_get("", "is_active").map_err(|e| map_err("is_active", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
        updated_at: row
            .try_get("", "updated_at")
            .map_err(|e| map_err("updated_at", e))?,
    })
}

pub async fn create_position(
    db: &DatabaseConnection,
    code: String,
    name: String,
    category: String,
) -> AppResult<Position> {
    PositionCategory::try_from(category.as_str()).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let dup = db
        .query_one(
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM positions WHERE code = ? LIMIT 1",
                [code.clone().into()],
            ),
        )
        .await?;
    if dup.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le code position « {code} » existe déjà."
        )]));
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO positions (code, name, category, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, 1, ?, ?)",
        [
            code.into(),
            name.into(),
            category.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, category, requirement_profile_id, is_active, created_at, updated_at \
             FROM positions WHERE id = last_insert_rowid()"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("position after insert")))?;

    map_position_row(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// I) schedule classes + details
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_schedule_classes(db: &DatabaseConnection) -> AppResult<Vec<ScheduleClassWithDetails>> {
    let classes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, name, shift_pattern_code, is_continuous, nominal_hours_per_day, is_active, created_at \
             FROM schedule_classes WHERE is_active = 1 ORDER BY name ASC"
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
    Ok(ScheduleClass {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        name: row.try_get("", "name").map_err(|e| map_err("name", e))?,
        shift_pattern_code: row
            .try_get("", "shift_pattern_code")
            .map_err(|e| map_err("shift_pattern_code", e))?,
        is_continuous: row
            .try_get("", "is_continuous")
            .map_err(|e| map_err("is_continuous", e))?,
        nominal_hours_per_day: row
            .try_get("", "nominal_hours_per_day")
            .map_err(|e| map_err("nominal_hours_per_day", e))?,
        is_active: row.try_get("", "is_active").map_err(|e| map_err("is_active", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
    })
}

async fn load_schedule_details(db: &DatabaseConnection, schedule_class_id: i64) -> AppResult<Vec<ScheduleDetail>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, schedule_class_id, day_of_week, shift_start, shift_end, is_rest_day \
             FROM schedule_details WHERE schedule_class_id = ? ORDER BY day_of_week ASC",
            [schedule_class_id.into()],
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
        schedule_class_id: row
            .try_get("", "schedule_class_id")
            .map_err(|e| map_err("schedule_class_id", e))?,
        day_of_week: row
            .try_get("", "day_of_week")
            .map_err(|e| map_err("day_of_week", e))?,
        shift_start: row
            .try_get("", "shift_start")
            .map_err(|e| map_err("shift_start", e))?,
        shift_end: row
            .try_get("", "shift_end")
            .map_err(|e| map_err("shift_end", e))?,
        is_rest_day: row
            .try_get("", "is_rest_day")
            .map_err(|e| map_err("is_rest_day", e))?,
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
        labor_rate: row
            .try_get("", "labor_rate")
            .map_err(|e| map_err("labor_rate", e))?,
        overtime_rate: row
            .try_get("", "overtime_rate")
            .map_err(|e| map_err("overtime_rate", e))?,
        cost_center_id: row
            .try_get("", "cost_center_id")
            .map_err(|e| map_err("cost_center_id", e))?,
        source_type: row
            .try_get("", "source_type")
            .map_err(|e| map_err("source_type", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
    })
}

pub async fn get_active_rate_card(
    db: &DatabaseConnection,
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

pub async fn list_authorizations(
    db: &DatabaseConnection,
    personnel_id: i64,
) -> AppResult<Vec<PersonnelAuthorization>> {
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
        valid_from: row
            .try_get("", "valid_from")
            .map_err(|e| map_err("valid_from", e))?,
        valid_to: row
            .try_get("", "valid_to")
            .map_err(|e| map_err("valid_to", e))?,
        source_certification_type_id: row
            .try_get("", "source_certification_type_id")
            .map_err(|e| map_err("source_certification_type_id", e))?,
        is_active: row
            .try_get("", "is_active")
            .map_err(|e| map_err("is_active", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
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
    AuthorizationType::try_from(authorization_type.as_str())
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

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
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
        updated_at: row
            .try_get("", "updated_at")
            .map_err(|e| map_err("updated_at", e))?,
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

pub async fn list_company_contacts(
    db: &DatabaseConnection,
    company_id: i64,
) -> AppResult<Vec<ExternalCompanyContact>> {
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
        company_id: row
            .try_get("", "company_id")
            .map_err(|e| map_err("company_id", e))?,
        contact_name: row
            .try_get("", "contact_name")
            .map_err(|e| map_err("contact_name", e))?,
        contact_role: row
            .try_get("", "contact_role")
            .map_err(|e| map_err("contact_role", e))?,
        phone: row.try_get("", "phone").map_err(|e| map_err("phone", e))?,
        email: row.try_get("", "email").map_err(|e| map_err("email", e))?,
        is_primary: row
            .try_get("", "is_primary")
            .map_err(|e| map_err("is_primary", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| map_err("created_at", e))?,
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
                id,
                personnel_id,
                block_type,
                start_at,
                end_at,
                reason_note,
                is_critical,
                created_by_id,
                created_at
             FROM personnel_availability_blocks
             WHERE personnel_id = ?
             ORDER BY start_at DESC
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
            created_at: row.try_get("", "created_at").unwrap_or_default(),
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
            "SELECT * FROM (
                SELECT
                    'wo' AS source_module,
                    wo.id AS record_id,
                    wo.code AS record_code,
                    CASE
                        WHEN wo.primary_responsible_id = ua.id THEN 'primary_responsible'
                        ELSE 'intervener'
                    END AS role_code,
                    wos.code AS status_code,
                    wo.title AS title,
                    COALESCE(wo.actual_end, wo.updated_at, wo.created_at) AS happened_at
                FROM user_accounts ua
                JOIN work_orders wo ON (
                    wo.primary_responsible_id = ua.id OR EXISTS (
                        SELECT 1 FROM work_order_interveners woi
                        WHERE woi.work_order_id = wo.id AND woi.intervener_id = ua.id
                    )
                )
                LEFT JOIN work_order_statuses wos ON wos.id = wo.status_id
                WHERE ua.personnel_id = ?

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
            LIMIT ?",
            [personnel_id.into(), personnel_id.into(), row_limit.into()],
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
            "SELECT
                COALESCE((
                    SELECT COUNT(DISTINCT wo.id)
                    FROM user_accounts ua
                    JOIN work_orders wo ON (
                        wo.primary_responsible_id = ua.id OR EXISTS (
                            SELECT 1 FROM work_order_interveners woi
                            WHERE woi.work_order_id = wo.id AND woi.intervener_id = ua.id
                        )
                    )
                    JOIN work_order_statuses wos ON wos.id = wo.status_id
                    WHERE ua.personnel_id = ?
                      AND wos.code NOT IN ('closed', 'cancelled')
                ), 0) AS open_work_orders,
                COALESCE((
                    SELECT COUNT(DISTINCT wo.id)
                    FROM user_accounts ua
                    JOIN work_orders wo ON (
                        wo.primary_responsible_id = ua.id OR EXISTS (
                            SELECT 1 FROM work_order_interveners woi
                            WHERE woi.work_order_id = wo.id AND woi.intervener_id = ua.id
                        )
                    )
                    JOIN work_order_statuses wos ON wos.id = wo.status_id
                    WHERE ua.personnel_id = ?
                      AND wos.code IN ('in_progress', 'assigned', 'planned', 'ready_to_schedule')
                ), 0) AS in_progress_work_orders,
                COALESCE((
                    SELECT COUNT(DISTINCT di.id)
                    FROM user_accounts ua2
                    JOIN intervention_requests di ON (di.submitter_id = ua2.id OR di.reviewer_id = ua2.id)
                    WHERE ua2.personnel_id = ?
                      AND di.status IN ('submitted', 'pending_review', 'screened', 'awaiting_approval', 'approved_for_planning')
                ), 0) AS pending_interventions,
                COALESCE((
                    SELECT COUNT(*)
                    FROM intervention_requests di2
                    JOIN user_accounts ua3 ON ua3.id = di2.submitter_id
                    WHERE ua3.personnel_id = ?
                      AND date(di2.created_at) >= date('now', '-30 day')
                ), 0) AS interventions_last_30d",
            [
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
                personnel_id.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("workload summary query failed")))?;

    Ok(PersonnelWorkloadSummary {
        open_work_orders: row.try_get("", "open_work_orders").unwrap_or_default(),
        in_progress_work_orders: row
            .try_get("", "in_progress_work_orders")
            .unwrap_or_default(),
        pending_interventions: row.try_get("", "pending_interventions").unwrap_or_default(),
        interventions_last_30d: row.try_get("", "interventions_last_30d").unwrap_or_default(),
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
         LEFT JOIN org_nodes team ON team.id = p.primary_team_id
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
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
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
