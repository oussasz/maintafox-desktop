use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::permit::queries::get_work_permit_linked_to_work_order;
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
    crate::clock::now().date_naive()
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
        // An unreadable expiry never qualifies anyone (fail closed).
        Some(exp) => parse_day(exp).is_some_and(|d| d >= today_naive()),
    }
}

/// A malformed stored profile is an error: reading it as "no requirement" would qualify everyone.
fn profile_codes(p: &QualificationRequirementProfile) -> AppResult<Vec<String>> {
    serde_json::from_str(&p.applies_to_permit_type_codes_json).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_QUAL_PROFILE_PERMIT_CODES_UNREADABLE:profile_id={}: Qualification profile '{}' has unreadable permit type codes: {e}",
            p.id, p.profile_name
        )])
    })
}

fn profile_required_ids(p: &QualificationRequirementProfile) -> AppResult<Vec<i64>> {
    serde_json::from_str(&p.required_certification_type_ids_json).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_QUAL_PROFILE_CERT_TYPES_UNREADABLE:profile_id={}: Qualification profile '{}' has unreadable certification types: {e}",
            p.id, p.profile_name
        )])
    })
}

fn required_cert_type_ids_for_permit(
    profiles: &[QualificationRequirementProfile],
    permit_type_code: &str,
) -> AppResult<Vec<i64>> {
    let mut out: Vec<i64> = Vec::new();
    for p in profiles {
        if !profile_codes(p)?.iter().any(|c| c == permit_type_code) {
            continue;
        }
        out.extend(profile_required_ids(p)?);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn distinct_permit_codes_from_profiles(profiles: &[QualificationRequirementProfile]) -> AppResult<Vec<String>> {
    let mut codes: Vec<String> = Vec::new();
    for p in profiles {
        codes.extend(profile_codes(p)?);
    }
    codes.sort();
    codes.dedup();
    Ok(codes)
}

fn evaluate_readiness(
    profiles: &[QualificationRequirementProfile],
    certs: &[PersonnelCertification],
    personnel_id: i64,
    permit_type_code: &str,
) -> AppResult<PersonnelReadinessRow> {
    let required = required_cert_type_ids_for_permit(profiles, permit_type_code)?;
    if required.is_empty() {
        return Ok(PersonnelReadinessRow {
            personnel_id,
            permit_type_code: permit_type_code.to_string(),
            is_qualified: true,
            blocking_reason: None,
            expires_at: None,
        });
    }

    let mut missing: Vec<i64> = Vec::new();
    let mut qualifying_expiries: Vec<String> = Vec::new();

    for ct_id in &required {
        let mut found = false;
        for pc in certs
            .iter()
            .filter(|c| c.personnel_id == personnel_id && c.certification_type_id == *ct_id)
        {
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
        (false, Some(format!("missing_required_certification_types:{missing:?}")))
    } else {
        (true, None)
    };

    Ok(PersonnelReadinessRow {
        personnel_id,
        permit_type_code: permit_type_code.to_string(),
        is_qualified,
        blocking_reason: reason,
        expires_at: earliest,
    })
}

async fn permit_type_code_by_id(db: &DatabaseConnection, permit_type_id: i64) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM permit_types WHERE id = ?",
            [permit_type_id.into()],
        ))
        .await?;
    row.map(|r| r.try_get::<String>("", "code"))
        .transpose()
        .map_err(|e| decode_err("code", e))
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
            Ok(vec![evaluate_readiness(&profiles, &certs, pid, &code)?])
        }
        (Some(pid), None) => {
            let certs = load_certs_for_personnel(db, pid).await?;
            let codes = distinct_permit_codes_from_profiles(&profiles)?;
            let mut rows = Vec::with_capacity(codes.len());
            for code in codes {
                rows.push(evaluate_readiness(&profiles, &certs, pid, &code)?);
            }
            Ok(rows)
        }
        _ => Err(AppError::ValidationFailed(vec![
            "GATE_QUAL_READINESS_TARGET_REQUIRED: personnel_id is required (optionally with permit_type_code).".into(),
        ])),
    }
}

pub async fn evaluate_crew_permit_skill_gaps(
    db: &DatabaseConnection,
    input: CrewPermitSkillGapInput,
) -> AppResult<CrewPermitSkillGapResult> {
    if input.personnel_ids.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_QUAL_CREW_EMPTY: personnel_ids must not be empty.".into(),
        ]));
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
            "GATE_QUAL_CREW_PERMIT_TYPE_REQUIRED: Work order requires a permit but none is linked; pass permit_type_code."
                .into(),
        ]));
    };

    let profiles = list_qualification_requirement_profiles(db).await?;
    let required = required_cert_type_ids_for_permit(&profiles, &permit_code)?;

    let mut rows: Vec<CrewPermitSkillGapRow> = Vec::new();
    for pid in &input.personnel_ids {
        let certs = load_certs_for_personnel(db, *pid).await?;
        let row = evaluate_readiness(&profiles, &certs, *pid, &permit_code)?;
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

async fn get_snapshot_by_id(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<PersonnelReadinessSnapshot>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, period, payload_json, row_version, created_at \
             FROM personnel_readiness_snapshots WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(PersonnelReadinessSnapshot {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            entity_sync_id: r
                .try_get("", "entity_sync_id")
                .map_err(|e| decode_err("entity_sync_id", e))?,
            period: r.try_get("", "period").map_err(|e| decode_err("period", e))?,
            payload_json: r
                .try_get("", "payload_json")
                .map_err(|e| decode_err("payload_json", e))?,
            row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
            created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        })),
        None => Ok(None),
    }
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
            payload_json: r
                .try_get("", "payload_json")
                .map_err(|e| decode_err("payload_json", e))?,
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
        return Err(AppError::ValidationFailed(vec![
            "GATE_QUAL_SNAPSHOT_PERIOD_REQUIRED: period is required.".into(),
        ]));
    }
    serde_json::from_str::<serde_json::Value>(&input.payload_json).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_QUAL_SNAPSHOT_PAYLOAD_INVALID: payload_json must be valid JSON: {e}"
        )])
    })?;
    let txn = db.begin().await?;

    if let Some(id) = input.id {
        let ev = input.expected_row_version.ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INV_ROW_VERSION_REQUIRED: expected_row_version required for update.".into(),
            ])
        })?;
        let current = get_snapshot_by_id(&txn, id).await?.ok_or_else(|| AppError::NotFound {
            entity: "PersonnelReadinessSnapshot".into(),
            id: id.to_string(),
        })?;
        if current.row_version != ev {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: Row version mismatch.".into(),
            ]));
        }
        let new_rv = current.row_version + 1;
        let affected = txn
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
                "GATE_RECORD_CHANGED_ELSEWHERE: Concurrent update on personnel_readiness_snapshots.".into(),
            ]));
        }
        let updated = get_snapshot_by_id(&txn, id).await?.expect("row");
        txn.commit().await?;
        return Ok(updated);
    }

    let entity_sync_id = Uuid::new_v4().to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_readiness_snapshots (entity_sync_id, period, payload_json, row_version) \
         VALUES (?, ?, ?, 1)",
        [entity_sync_id.into(), input.period.into(), input.payload_json.into()],
    ))
    .await?;
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let created = get_snapshot_by_id(&txn, new_id).await?.expect("row");
    txn.commit().await?;
    Ok(created)
}

pub async fn refresh_personnel_readiness_snapshot_payload(
    db: &DatabaseConnection,
    period: String,
) -> AppResult<PersonnelReadinessSnapshot> {
    let profiles = list_qualification_requirement_profiles(db).await?;
    let codes = distinct_permit_codes_from_profiles(&profiles)?;
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
            let row = evaluate_readiness(&profiles, &certs, pid, code)?;
            matrix.push(serde_json::to_value(&row)?);
        }
    }

    let payload_json = serde_json::to_string(&serde_json::json!({
        "period": period,
        "generated_at": crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
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
