use chrono::{NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::permit::queries::get_work_permit_linked_to_work_order;
use crate::sync::domain::{PersonnelReadinessSnapshotSyncPayload, StageOutboxItemInput, SYNC_ENTITY_PERSONNEL_READINESS_SNAPSHOTS};
use crate::sync::queries::stage_outbox_item;
use crate::wo::queries::get_work_order;

use super::domain::{
    CrewPermitSkillGapInput, CrewPermitSkillGapResult, CrewPermitSkillGapRow, PersonnelCertification,
    PersonnelCertificationListFilter, PersonnelReadinessFilter, PersonnelReadinessRow, PersonnelReadinessSnapshot,
    PersonnelReadinessSnapshotUpsertInput, QualificationRequirementProfile,
};
use super::queries::{list_personnel_certifications, list_qualification_requirement_profiles};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("readiness decode '{field}': {err}"))
}

fn today_naive() -> NaiveDate {
    Utc::now().date_naive()
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    let p: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&p, "%Y-%m-%d").ok()
}

fn cert_acceptable_for_requirement(pc: &PersonnelCertification) -> bool {
    if pc.verification_status != "verified" {
        return false;
    }
    match &pc.expires_at {
        None => true,
        Some(exp) => parse_day(exp).map(|d| d >= today_naive()).unwrap_or(true),
    }
}

fn required_cert_type_ids_for_permit(
    profiles: &[QualificationRequirementProfile],
    permit_type_code: &str,
) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for p in profiles {
        let codes: Vec<String> = serde_json::from_str(&p.applies_to_permit_type_codes_json).unwrap_or_default();
        if !codes.iter().any(|c| c == permit_type_code) {
            continue;
        }
        let req: Vec<i64> = serde_json::from_str(&p.required_certification_type_ids_json).unwrap_or_default();
        out.extend(req);
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn distinct_permit_codes_from_profiles(profiles: &[QualificationRequirementProfile]) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for p in profiles {
        let c: Vec<String> = serde_json::from_str(&p.applies_to_permit_type_codes_json).unwrap_or_default();
        codes.extend(c);
    }
    codes.sort();
    codes.dedup();
    codes
}

fn evaluate_readiness(
    profiles: &[QualificationRequirementProfile],
    certs: &[PersonnelCertification],
    personnel_id: i64,
    permit_type_code: &str,
) -> PersonnelReadinessRow {
    let required = required_cert_type_ids_for_permit(profiles, permit_type_code);
    if required.is_empty() {
        return PersonnelReadinessRow {
            personnel_id,
            permit_type_code: permit_type_code.to_string(),
            is_qualified: true,
            blocking_reason: None,
            expires_at: None,
        };
    }

    let mut missing: Vec<i64> = Vec::new();
    let mut qualifying_expiries: Vec<String> = Vec::new();

    for ct_id in &required {
        let mut found = false;
        for pc in certs.iter().filter(|c| c.personnel_id == personnel_id && c.certification_type_id == *ct_id) {
            if cert_acceptable_for_requirement(pc) {
                found = true;
                if let Some(ref e) = pc.expires_at {
                    qualifying_expiries.push(e.clone());
                }
                break;
            }
        }
        if !found {
            missing.push(*ct_id);
        }
    }

    qualifying_expiries.sort();
    let earliest = qualifying_expiries.first().cloned();

    let (is_qualified, reason) = if !missing.is_empty() {
        (
            false,
            Some(format!("missing_required_certification_types:{missing:?}")),
        )
    } else {
        (true, None)
    };

    PersonnelReadinessRow {
        personnel_id,
        permit_type_code: permit_type_code.to_string(),
        is_qualified,
        blocking_reason: reason,
        expires_at: earliest,
    }
}

async fn permit_type_code_by_id(db: &DatabaseConnection, permit_type_id: i64) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM permit_types WHERE id = ?",
            [permit_type_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get("", "code").ok()))
}

async fn load_certs_for_personnel(
    db: &DatabaseConnection,
    personnel_id: i64,
) -> AppResult<Vec<PersonnelCertification>> {
    list_personnel_certifications(
        db,
        PersonnelCertificationListFilter {
            personnel_id: Some(personnel_id),
            limit: Some(2000),
        },
    )
    .await
}

pub async fn list_personnel_readiness(
    db: &DatabaseConnection,
    filter: PersonnelReadinessFilter,
) -> AppResult<Vec<PersonnelReadinessRow>> {
    let profiles = list_qualification_requirement_profiles(db).await?;
    if profiles.is_empty() {
        return Ok(vec![]);
    }

    match (filter.personnel_id, filter.permit_type_code.clone()) {
        (Some(pid), Some(code)) => {
            let certs = load_certs_for_personnel(db, pid).await?;
            Ok(vec![evaluate_readiness(&profiles, &certs, pid, &code)])
        }
        (Some(pid), None) => {
            let certs = load_certs_for_personnel(db, pid).await?;
            let codes = distinct_permit_codes_from_profiles(&profiles);
            let mut rows = Vec::with_capacity(codes.len());
            for code in codes {
                rows.push(evaluate_readiness(&profiles, &certs, pid, &code));
            }
            Ok(rows)
        }
        _ => Err(AppError::ValidationFailed(vec![
            "personnel_id is required (optionally with permit_type_code).".into(),
        ])),
    }
}

pub async fn evaluate_crew_permit_skill_gaps(
    db: &DatabaseConnection,
    input: CrewPermitSkillGapInput,
) -> AppResult<CrewPermitSkillGapResult> {
    if input.personnel_ids.is_empty() {
        return Err(AppError::ValidationFailed(vec!["personnel_ids must not be empty.".into()]));
    }

    let wo = get_work_order(db, input.work_order_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.work_order_id.to_string(),
        })?;

    let permit_code = if let Some(ref o) = input.permit_type_code {
        o.clone()
    } else if let Some(wp) = get_work_permit_linked_to_work_order(db, wo.id).await? {
        permit_type_code_by_id(db, wp.permit_type_id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PermitType".into(),
                id: wp.permit_type_id.to_string(),
            })?
    } else if !wo.requires_permit {
        return Ok(CrewPermitSkillGapResult {
            permit_type_code: String::new(),
            work_order_id: wo.id,
            rows: vec![],
        });
    } else {
        return Err(AppError::ValidationFailed(vec![
            "Work order requires a permit but none is linked; pass permit_type_code.".into(),
        ]));
    };

    let profiles = list_qualification_requirement_profiles(db).await?;
    let required = required_cert_type_ids_for_permit(&profiles, &permit_code);

    let mut rows: Vec<CrewPermitSkillGapRow> = Vec::new();
    for pid in &input.personnel_ids {
        let certs = load_certs_for_personnel(db, *pid).await?;
        let row = evaluate_readiness(&profiles, &certs, *pid, &permit_code);
        let missing: Vec<i64> = if row.is_qualified {
            vec![]
        } else {
            required
                .iter()
                .copied()
                .filter(|ct| {
                    !certs.iter().any(|pc| {
                        pc.personnel_id == *pid
                            && pc.certification_type_id == *ct
                            && cert_acceptable_for_requirement(pc)
                    })
                })
                .collect()
        };
        rows.push(CrewPermitSkillGapRow {
            personnel_id: *pid,
            is_qualified: row.is_qualified,
            blocking_reason: row.blocking_reason.clone(),
            missing_certification_type_ids: missing,
            expires_at: row.expires_at.clone(),
        });
    }

    Ok(CrewPermitSkillGapResult {
        permit_type_code: permit_code,
        work_order_id: wo.id,
        rows,
    })
}

async fn get_snapshot_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<PersonnelReadinessSnapshot>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, period, payload_json, row_version, created_at \
             FROM personnel_readiness_snapshots WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(
            PersonnelReadinessSnapshot {
                id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                entity_sync_id: r
                    .try_get("", "entity_sync_id")
                    .map_err(|e| decode_err("entity_sync_id", e))?,
                period: r.try_get("", "period").map_err(|e| decode_err("period", e))?,
                payload_json: r.try_get("", "payload_json").map_err(|e| decode_err("payload_json", e))?,
                row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
                created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
            },
        )),
        None => Ok(None),
    }
}

async fn stage_snapshot(db: &DatabaseConnection, row: &PersonnelReadinessSnapshot) -> AppResult<()> {
    let payload = PersonnelReadinessSnapshotSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        period: row.period.clone(),
        payload_json: row.payload_json.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "personnel_readiness_snapshots:{}:v{}",
                row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_PERSONNEL_READINESS_SNAPSHOTS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn list_personnel_readiness_snapshots(db: &DatabaseConnection) -> AppResult<Vec<PersonnelReadinessSnapshot>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, period, payload_json, row_version, created_at \
             FROM personnel_readiness_snapshots ORDER BY created_at DESC LIMIT 200"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(PersonnelReadinessSnapshot {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            entity_sync_id: r
                .try_get("", "entity_sync_id")
                .map_err(|e| decode_err("entity_sync_id", e))?,
            period: r.try_get("", "period").map_err(|e| decode_err("period", e))?,
            payload_json: r.try_get("", "payload_json").map_err(|e| decode_err("payload_json", e))?,
            row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
            created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        });
    }
    Ok(out)
}

pub async fn upsert_personnel_readiness_snapshot(
    db: &DatabaseConnection,
    input: PersonnelReadinessSnapshotUpsertInput,
) -> AppResult<PersonnelReadinessSnapshot> {
    if input.period.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["period is required.".into()]));
    }
    serde_json::from_str::<serde_json::Value>(&input.payload_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("payload_json must be valid JSON: {e}")]))?;

    if let Some(id) = input.id {
        let ev = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required for update.".into()]))?;
        let current = get_snapshot_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PersonnelReadinessSnapshot".into(),
                id: id.to_string(),
            })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec!["Row version mismatch.".into()]));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE personnel_readiness_snapshots SET period = ?, payload_json = ?, row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.period.into(),
                    input.payload_json.into(),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on personnel_readiness_snapshots.".into(),
            ]));
        }
        let updated = get_snapshot_by_id(db, id).await?.expect("row");
        stage_snapshot(db, &updated).await?;
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_readiness_snapshots (entity_sync_id, period, payload_json, row_version) \
         VALUES (?, ?, ?, 1)",
        [sync_id.into(), input.period.into(), input.payload_json.into()],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let created = get_snapshot_by_id(db, new_id).await?.expect("row");
    stage_snapshot(db, &created).await?;
    Ok(created)
}

pub async fn refresh_personnel_readiness_snapshot_payload(
    db: &DatabaseConnection,
    period: String,
) -> AppResult<PersonnelReadinessSnapshot> {
    let profiles = list_qualification_requirement_profiles(db).await?;
    let codes = distinct_permit_codes_from_profiles(&profiles);
    let personnel_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM personnel WHERE termination_date IS NULL ORDER BY id ASC LIMIT 500".to_string(),
        ))
        .await?;

    let mut matrix: Vec<serde_json::Value> = Vec::new();
    for pr in personnel_rows {
        let pid: i64 = pr.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let certs = load_certs_for_personnel(db, pid).await?;
        for code in &codes {
            let row = evaluate_readiness(&profiles, &certs, pid, code);
            matrix.push(serde_json::to_value(&row)?);
        }
    }

    let payload_json = serde_json::to_string(&serde_json::json!({
        "period": period,
        "generated_at": Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "profile_count": profiles.len(),
        "rows": matrix,
    }))?;

    upsert_personnel_readiness_snapshot(
        db,
        PersonnelReadinessSnapshotUpsertInput {
            id: None,
            period,
            payload_json,
            expected_row_version: None,
        },
    )
    .await
}
