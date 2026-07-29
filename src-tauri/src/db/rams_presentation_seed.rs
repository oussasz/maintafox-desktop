//! DEMO-ONLY RAMS presentation seed (graduation / developer demos).
//!
//! **Production vs Demo separation**
//! - Production equipment create, startup, login, onboarding, and migrations must
//!   never call these functions.
//! - Demo data is reachable only through explicit developer IPC commands
//!   (`seed_rams_presentation_data`, `seed_rams_sql_demo_data`) or the manual
//!   "Generate Demo RAMS Data" UI action.
//!
//! Creates governed failure history via WO close-out (ingest + KPI refresh),
//! then runs Weibull fit and Markov evaluation. Idempotent per equipment.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::assets::identity::{update_asset_identity, UpdateAssetIdentityPayload};
use crate::errors::{AppError, AppResult};
use crate::reliability::advanced_rams::domain::WeibullFitRunInput;
use crate::reliability::advanced_rams::queries as advanced_rams_queries;
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::reliability::markov_mc::domain::CreateMarkovModelInput;
use crate::reliability::markov_mc::queries as markov_queries;
use crate::reliability::queries as reliability_queries;
use crate::wo::closeout::{
    self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput,
};
use crate::wo::domain::WoCreateInput;
use crate::wo::execution::{
    self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput,
};
use crate::wo::labor::{self, AddLaborInput};
use crate::wo::parts;
use crate::wo::queries;

const SEED_WO_TITLE_PREFIX: &str = "RAMS-PRESENTATION-";
const SEED_MARKOV_TITLE: &str = "RAMS-PRESENTATION-Markov";
const PRESENTATION_MARKOV_GRAPH: &str = r#"{"spec_version":1,"kind":"discrete","states":["Up","Degraded","Down"],"matrix":[[0.94,0.05,0.01],[0.10,0.75,0.15],[0.25,0.50,0.25]]}"#;

#[derive(Debug, Clone, Deserialize)]
pub struct RamsPresentationSeedInput {
    pub equipment_id: Option<i64>,
    #[serde(default)]
    pub months_back: Option<i64>,
    #[serde(default)]
    pub failure_count: Option<i64>,
    #[serde(default)]
    pub actor_id: Option<i64>,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RamsPresentationSeedReport {
    pub equipment_id: i64,
    pub skipped: bool,
    pub work_orders_created: i64,
    pub failure_events_count: i64,
    pub exposure_hours: Option<f64>,
    pub weibull_beta: Option<f64>,
    pub weibull_eta: Option<f64>,
    pub weibull_adequate: bool,
    pub markov_model_id: Option<i64>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Refresh KPI snapshot, Weibull fit, and Markov for an equipment (after SQL or WO seed).
pub async fn refresh_rams_analytics_for_equipment(
    db: &DatabaseConnection,
    equipment_id: i64,
    actor_id: i64,
    months_back: i64,
) -> AppResult<RamsPresentationSeedReport> {
    let report = RamsPresentationSeedReport {
        equipment_id,
        skipped: true,
        work_orders_created: 0,
        failure_events_count: count_failure_events(db, equipment_id).await.unwrap_or(0),
        exposure_hours: None,
        weibull_beta: None,
        weibull_eta: None,
        weibull_adequate: false,
        markov_model_id: None,
        warnings: Vec::new(),
        errors: Vec::new(),
    };
    finalize_analytics(db, actor_id, equipment_id, months_back, report).await
}

/// Idempotent startup hook: populate governed RAMS analytics on demo equipment without UI action.
/// Explicit DEMO orchestrator: SQL simulation backfill + presentation seed for
/// equipment that still lacks demo history.
///
/// **Must only be invoked from explicit developer diagnostics commands.**
/// Never call from startup, asset create, login, onboarding, or migrations.
pub async fn run_explicit_rams_demo_seed(db: &DatabaseConnection) -> AppResult<()> {
    ensure_demo_verifier_user(db).await?;

    if let Err(e) = crate::db::rams_sql_demo_seed::ensure_all_equipment_rams_simulation(db).await {
        tracing::warn!("rams SQL demo seed pass failed (non-fatal): {e}");
    }

    let targets = list_equipment_needing_rams_seed(db).await?;
    if targets.is_empty() {
        tracing::debug!("rams explicit demo seed: all target equipment already seeded");
        return Ok(());
    }
    for equipment_id in targets {
        let report = seed_rams_presentation_data(
            db,
            RamsPresentationSeedInput {
                equipment_id: Some(equipment_id),
                months_back: Some(12),
                failure_count: Some(8),
                actor_id: None,
                force: None,
            },
        )
        .await?;
        if report.exposure_hours.unwrap_or(0.0) <= 0.0 {
            tracing::warn!(
                equipment_id,
                exposure_hours = ?report.exposure_hours,
                warnings = ?report.warnings,
                "rams explicit demo seed: exposure_hours still zero after seed"
            );
        }
        if report.errors.is_empty() {
            tracing::info!(
                equipment_id,
                skipped = report.skipped,
                work_orders = report.work_orders_created,
                exposure_hours = ?report.exposure_hours,
                "rams explicit demo seed complete"
            );
        } else {
            tracing::warn!(
                equipment_id,
                errors = ?report.errors,
                "rams explicit demo seed finished with errors"
            );
        }
    }
    Ok(())
}

/// Explicit DEMO seed for one equipment (or a resolved default).
/// Invoked only via the `seed_rams_presentation_data` IPC command / manual UI.
pub async fn seed_rams_presentation_data(
    db: &DatabaseConnection,
    input: RamsPresentationSeedInput,
) -> AppResult<RamsPresentationSeedReport> {
    let months_back = input.months_back.unwrap_or(12).clamp(3, 36);
    let failure_count = input.failure_count.unwrap_or(8).clamp(6, 20);
    let force = input.force.unwrap_or(false);

    ensure_demo_verifier_user(db).await?;

    let actor_id = match input.actor_id {
        Some(id) if id > 0 => id,
        _ => resolve_actor_id(db).await?,
    };
    let verifier_id = resolve_verifier_id(db, actor_id).await?;

    let equipment_id = match input.equipment_id {
        Some(id) if id > 0 => id,
        _ => resolve_default_equipment_id(db).await?,
    };

    let mut report = RamsPresentationSeedReport {
        equipment_id,
        skipped: false,
        work_orders_created: 0,
        failure_events_count: 0,
        exposure_hours: None,
        weibull_beta: None,
        weibull_eta: None,
        weibull_adequate: false,
        markov_model_id: None,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    neutralize_injector_work_order_anchors(db, equipment_id).await?;

    if !force && presentation_already_seeded(db, equipment_id).await? {
        report.skipped = true;
        report.warnings.push(
            "Presentation seed already present for this equipment (use force=true to re-run analytics only)."
                .into(),
        );
        return finalize_analytics(db, actor_id, equipment_id, months_back, report).await;
    }

    if let Err(e) = ensure_failure_reference_prerequisites(db).await {
        report.errors.push(format!("failure reference prerequisites: {e}"));
        return Ok(report);
    }

    let failure_mode_id = resolve_active_failure_mode_id(db).await?;
    let failure_cause_id = resolve_failure_cause_id(db).await?;

    if let Err(e) = assign_rams_schedule_class(db, equipment_id, actor_id as i32).await {
        report.errors.push(format!("RAMS profile assignment: {e}"));
        return Ok(report);
    }

    let org_node_id = load_equipment_org_node_id(db, equipment_id).await?;
    let now = Utc::now();
    let period_end = now.to_rfc3339();
    let period_start = (now - Duration::days(months_back * 30)).to_rfc3339();

    let mut wo_ids: Vec<i64> = Vec::new();
    let mut wo_time_windows: Vec<(String, String)> = Vec::new();
    for i in 0..failure_count {
        let months_ago = failure_count - i;
        let event_anchor = now - Duration::days(months_ago * 28);
        let fail_start = event_anchor - Duration::hours(4);
        let fail_end = event_anchor + Duration::hours(2 + (i % 3) as i64);
        let fail_start_s = fail_start.to_rfc3339();
        let fail_end_s = fail_end.to_rfc3339();

        match create_and_close_corrective_wo(
            db,
            CreateWoSeedCtx {
                equipment_id,
                org_node_id,
                actor_id,
                verifier_id,
                failure_mode_id,
                failure_cause_id,
                title: format!("{SEED_WO_TITLE_PREFIX}{:03}", i + 1),
                fail_start: fail_start_s.clone(),
                fail_end: fail_end_s.clone(),
                repair_hours: 1.5 + (i % 4) as f64 * 0.5,
            },
        )
        .await
        {
            Ok(wo_id) => {
                wo_ids.push(wo_id);
                wo_time_windows.push((fail_start_s, fail_end_s));
                report.work_orders_created += 1;
            }
            Err(e) => report.errors.push(format!("WO seed index {}: {e}", i + 1)),
        }
    }

    if wo_ids.is_empty() {
        report.errors.push("No work orders were created; aborting analytics.".into());
        return Ok(report);
    }

    for (wo_id, (fail_start, fail_end)) in wo_ids.iter().zip(wo_time_windows.iter()) {
        if let Err(e) = backdate_closed_wo_and_failure_event(db, *wo_id, fail_start, fail_end).await {
            report.warnings.push(format!("backdate wo {wo_id}: {e}"));
        }
    }

    match reliability_queries::refresh_reliability_kpi_snapshot(
        db,
        RefreshReliabilityKpiSnapshotInput {
            equipment_id,
            period_start: period_start.clone(),
            period_end: period_end.clone(),
            min_sample_n: Some(5),
            repeat_lookback_days: Some(30),
        },
    )
    .await
    {
        Ok(snap) => {
            report.failure_events_count = snap.event_count;
            if let Ok(ev) = reliability_queries::evaluate_reliability_analysis_input(
                db,
                RefreshReliabilityKpiSnapshotInput {
                    equipment_id,
                    period_start: period_start.clone(),
                    period_end: period_end.clone(),
                    min_sample_n: Some(5),
                    repeat_lookback_days: Some(30),
                },
            )
            .await
            {
                report.exposure_hours = Some(ev.exposure_hours);
            }
        }
        Err(e) => report.errors.push(format!("KPI snapshot refresh: {e}")),
    }

    finalize_analytics(db, actor_id, equipment_id, months_back, report).await
}

async fn finalize_analytics(
    db: &DatabaseConnection,
    actor_id: i64,
    equipment_id: i64,
    months_back: i64,
    mut report: RamsPresentationSeedReport,
) -> AppResult<RamsPresentationSeedReport> {
    let now = Utc::now();
    let period_end = now.to_rfc3339();
    let period_start = (now - Duration::days(months_back * 30)).to_rfc3339();

    match advanced_rams_queries::run_and_store_weibull_fit(
        db,
        Some(actor_id as i32),
        WeibullFitRunInput {
            equipment_id,
            period_start: None,
            period_end: None,
            include_censored: Some(false),
        },
    )
    .await
    {
        Ok(fit) => {
            report.weibull_beta = fit.beta;
            report.weibull_eta = fit.eta;
            report.weibull_adequate = fit.adequate_sample;
            if !fit.adequate_sample {
                report.warnings.push(format!("Weibull inadequate sample: {}", fit.message));
            }
        }
        Err(e) => report.errors.push(format!("Weibull fit: {e}")),
    }

    match ensure_markov_model(db, equipment_id, actor_id).await {
        Ok(id) => report.markov_model_id = Some(id),
        Err(e) => report.errors.push(format!("Markov model: {e}")),
    }

    if report.exposure_hours.is_none() {
        if let Ok(ev) = reliability_queries::evaluate_reliability_analysis_input(
            db,
            RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: period_start.clone(),
                period_end: period_end.clone(),
                min_sample_n: Some(1),
                repeat_lookback_days: Some(30),
            },
        )
        .await
        {
            report.exposure_hours = Some(ev.exposure_hours);
        }
    }

    if report.failure_events_count == 0 {
        let cnt = count_failure_events(db, equipment_id).await.unwrap_or(0);
        report.failure_events_count = cnt;
    }

    if report.exposure_hours.unwrap_or(0.0) <= 0.0 {
        report.warnings.push(
            "Exposure hours remain zero after seed — verify RAMS schedule class and governed WO anchors."
                .into(),
        );
    }

    Ok(report)
}

async fn neutralize_injector_work_order_anchors(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<()> {
    let updated = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders
             SET closed_at = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE equipment_id = ?
               AND (code LIKE 'RAMS-INJECT%'
                    OR COALESCE(description, '') LIKE '%[rams_injector]%')",
            [equipment_id.into()],
        ))
        .await?;
    if updated.rows_affected() > 0 {
        tracing::info!(
            equipment_id,
            rows = updated.rows_affected(),
            "neutralized injector work order closed_at anchors"
        );
    }
    Ok(())
}

struct CreateWoSeedCtx {
    equipment_id: i64,
    org_node_id: i64,
    actor_id: i64,
    verifier_id: i64,
    failure_mode_id: i64,
    failure_cause_id: i64,
    title: String,
    fail_start: String,
    fail_end: String,
    repair_hours: f64,
}

async fn create_and_close_corrective_wo(
    db: &DatabaseConnection,
    ctx: CreateWoSeedCtx,
) -> AppResult<i64> {
    let wo = queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(ctx.equipment_id),
            location_id: Some(ctx.org_node_id),
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: Some(ctx.org_node_id),
            planner_id: Some(ctx.actor_id),
            urgency_id: Some(3),
            title: ctx.title,
            description: Some("RAMS presentation seed — governed corrective close-out.".into()),
            notes: None,
            planned_start: Some(ctx.fail_start.clone()),
            planned_end: Some(ctx.fail_end.clone()),
            shift: None,
            expected_duration_hours: Some(ctx.repair_hours),
            creator_id: ctx.actor_id,
            requires_permit: Some(false),
        },
    )
    .await?;

    let wo = execution::plan_wo(
        db,
        WoPlanInput {
            wo_id: wo.id,
            actor_id: ctx.actor_id,
            expected_row_version: wo.row_version,
            planner_id: ctx.actor_id,
            planned_start: ctx.fail_start.clone(),
            planned_end: ctx.fail_end.clone(),
            shift: None,
            expected_duration_hours: Some(ctx.repair_hours),
            urgency_id: None,
            planned_downtime_hours: None,
        },
    )
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET \
         status_id = (SELECT id FROM work_order_statuses WHERE code = 'ready_to_schedule'), \
         row_version = row_version + 1, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE id = ?",
        [wo.id.into()],
    ))
    .await?;

    let wo = execution::assign_wo(
        db,
        WoAssignInput {
            wo_id: wo.id,
            actor_id: ctx.actor_id,
            expected_row_version: wo.row_version + 1,
            assigned_group_id: None,
            primary_responsible_id: Some(ctx.actor_id),
            scheduled_at: Some(ctx.fail_start.clone()),
        },
    )
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET actual_start = ? WHERE id = ?",
        [ctx.fail_start.clone().into(), wo.id.into()],
    ))
    .await?;

    let wo = execution::start_wo(
        db,
        WoStartInput {
            wo_id: wo.id,
            actor_id: ctx.actor_id,
            expected_row_version: wo.row_version,
        },
    )
    .await?;

    labor::add_labor_entry(
        db,
        AddLaborInput {
            wo_id: wo.id,
            intervener_id: ctx.actor_id,
            skill_id: None,
            started_at: Some(ctx.fail_start.clone()),
            ended_at: Some(ctx.fail_end.clone()),
            hours_worked: Some(ctx.repair_hours),
            hourly_rate: Some(50.0),
            notes: None,
        },
    )
    .await?;

    parts::confirm_no_parts_used(db, wo.id, ctx.actor_id).await?;

    let wo = execution::complete_wo_mechanically(
        db,
        WoMechCompleteInput {
            wo_id: wo.id,
            actor_id: ctx.actor_id,
            expected_row_version: wo.row_version,
            actual_end: Some(ctx.fail_end.clone()),
            actual_duration_hours: Some(ctx.repair_hours),
            conclusion: Some("Presentation seed repair completed.".into()),
        },
    )
    .await?;

    closeout::save_failure_detail(
        db,
        SaveFailureDetailInput {
            wo_id: wo.id,
            symptom_id: None,
            failure_mode_id: Some(ctx.failure_mode_id),
            failure_cause_id: Some(ctx.failure_cause_id),
            failure_effect_id: None,
            is_temporary_repair: false,
            is_permanent_repair: true,
            cause_not_determined: false,
            notes: Some("ISO 14224 coded failure for RAMS presentation dataset.".into()),
        },
    )
    .await?;

    closeout::update_wo_rca(
        db,
        UpdateWoRcaInput {
            wo_id: wo.id,
            root_cause_summary: Some(
                "Progressive mechanical degradation identified during governed close-out."
                    .into(),
            ),
            corrective_action_summary: Some("Component replaced and alignment verified.".into()),
        },
    )
    .await?;

    let (_ver, wo) = closeout::save_verification(
        db,
        SaveVerificationInput {
            wo_id: wo.id,
            verified_by_id: ctx.verifier_id,
            result: "pass".into(),
            return_to_service_confirmed: true,
            recurrence_risk_level: Some("low".into()),
            notes: Some("Presentation seed verification.".into()),
            expected_row_version: wo.row_version,
        },
    )
    .await?;

    let closed = closeout::close_wo(
        db,
        WoCloseInput {
            wo_id: wo.id,
            actor_id: ctx.actor_id,
            expected_row_version: wo.row_version,
            ..Default::default()
        },
    )
    .await?;

    Ok(closed.id)
}

async fn backdate_closed_wo_and_failure_event(
    db: &DatabaseConnection,
    wo_id: i64,
    fail_start: &str,
    fail_end: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET actual_start = ?, closed_at = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
        [fail_start.into(), fail_end.into(), wo_id.into()],
    ))
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE failure_events SET detected_at = ?, failed_at = ?, restored_at = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE source_type = 'work_order' AND source_id = ?",
        [
            fail_start.into(),
            fail_start.into(),
            fail_end.into(),
            wo_id.into(),
        ],
    ))
    .await?;

    Ok(())
}

async fn assign_rams_schedule_class(
    db: &DatabaseConnection,
    equipment_id: i64,
    actor_id: i32,
) -> AppResult<()> {
    let schedule_ref_id = resolve_schedule_reference_value_id(db).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version, rams_schedule_reference_value_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: equipment_id.to_string(),
        })?;
    let rv: i64 = row.try_get("", "row_version").map_err(decode)?;
    let current: Option<i64> = row
        .try_get("", "rams_schedule_reference_value_id")
        .map_err(decode)?;
    if current == Some(schedule_ref_id) {
        return Ok(());
    }
    update_asset_identity(
        db,
        equipment_id,
        UpdateAssetIdentityPayload {
            rams_schedule_reference_value_id: Some(Some(schedule_ref_id)),
            rams_utilization_factor: Some(1.0),
            asset_name: None,
            class_code: None,
            family_code: None,
            subfamily_code: None,
            criticality_code: None,
            status_code: None,
            manufacturer: None,
            model: None,
            serial_number: None,
            maintainable_boundary: None,
            commissioned_at: None,
            decommissioned_at: None,
        },
        rv,
        actor_id,
    )
    .await?;
    Ok(())
}

async fn ensure_markov_model(db: &DatabaseConnection, equipment_id: i64, actor_id: i64) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM markov_models WHERE equipment_id = ? AND title = ? ORDER BY id DESC LIMIT 1",
            [equipment_id.into(), SEED_MARKOV_TITLE.into()],
        ))
        .await?;
    let model_id = if let Some(row) = existing {
        row.try_get("", "id").map_err(decode)?
    } else {
        let model = markov_queries::create_markov_model(
            db,
            Some(actor_id),
            CreateMarkovModelInput {
                equipment_id,
                title: SEED_MARKOV_TITLE.into(),
                graph_json: Some(PRESENTATION_MARKOV_GRAPH.into()),
                status: Some("active".into()),
            },
        )
        .await?;
        model.id
    };
    markov_queries::evaluate_markov_model(db, model_id).await?;
    Ok(model_id)
}

async fn ensure_failure_reference_prerequisites(db: &DatabaseConnection) -> AppResult<()> {
    if resolve_active_failure_mode_id(db).await.is_ok() {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    let domain_id = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_domains WHERE UPPER(TRIM(code)) = UPPER(TRIM('WORK.FAILURE_MODES')) LIMIT 1",
            [],
        ))
        .await?;
    let domain_id: i64 = if let Some(row) = domain_id {
        row.try_get("", "id").map_err(decode)?
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_domains (code, name, structure_type, governance_level, governance_category, is_extendable, created_at, updated_at) \
             VALUES ('WORK.FAILURE_MODES', 'Modes de défaillance', 'hierarchical', 'system_seeded', 'controlled_catalog', 0, ?, ?)",
            [now.clone().into(), now.clone().into()],
        ))
        .await?;
        last_insert_id(db).await?
    };

    let set_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets WHERE domain_id = ? AND status = 'published' ORDER BY version_no DESC LIMIT 1",
            [domain_id.into()],
        ))
        .await?;
    let set_id: i64 = if let Some(row) = set_row {
        row.try_get("", "id").map_err(decode)?
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_sets (domain_id, version_no, status, created_by_id, created_at) VALUES (?, 1, 'published', 1, ?)",
            [domain_id.into(), now.clone().into()],
        ))
        .await?;
        last_insert_id(db).await?
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values (set_id, parent_id, code, label, sort_order, is_active, created_at) \
         VALUES (?, NULL, 'MECH.BEARING', 'Roulement défectueux', 1, 1, ?)",
        [set_id.into(), now.into()],
    ))
    .await?;

    Ok(())
}

async fn resolve_active_failure_mode_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id AS id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES')) AND rv.is_active = 1 \
             ORDER BY rv.parent_id IS NOT NULL DESC, rv.sort_order ASC, rv.id ASC LIMIT 1",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "No published active WORK.FAILURE_MODES reference value. Publish failure modes in Reference Data.".into(),
            ])
        })?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_failure_cause_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id AS id FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'failure.cause' AND lv.is_active = 1 AND lv.deleted_at IS NULL \
             ORDER BY lv.sort_order ASC, lv.id ASC LIMIT 1",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "No active failure.cause lookup value found.".into(),
            ])
        })?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_schedule_reference_value_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id AS id \
             FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.is_active = 1 \
             ORDER BY CASE WHEN UPPER(TRIM(rv.code)) = 'DAY_SHIFT' THEN 0 ELSE 1 END, rv.id ASC \
             LIMIT 1",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "No active ORG.SCHEDULE_CLASS reference value found (required for exposure inference)."
                    .into(),
            ])
        })?;
    row.try_get("", "id").map_err(decode)
}

async fn presentation_already_seeded(db: &DatabaseConnection, equipment_id: i64) -> AppResult<bool> {
    let pattern = format!("{SEED_WO_TITLE_PREFIX}%");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_orders WHERE equipment_id = ? AND title LIKE ?",
            [equipment_id.into(), pattern.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("seed count missing".into()))?;
    let c: i64 = row.try_get("", "c").map_err(decode)?;
    Ok(c >= 6)
}

async fn count_failure_events(db: &DatabaseConnection, equipment_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM failure_events WHERE equipment_id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("failure event count missing".into()))?;
    row.try_get("", "c").map_err(decode)
}

async fn load_equipment_org_node_id(db: &DatabaseConnection, equipment_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: equipment_id.to_string(),
        })?;
    row.try_get::<Option<i64>>("", "installed_at_node_id")
        .map_err(decode)?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "Equipment has no installed_at_node_id; cannot create governed work orders.".into(),
            ])
        })
}

async fn list_equipment_needing_rams_seed(db: &DatabaseConnection) -> AppResult<Vec<i64>> {
    const MAX_EXPLICIT_DEMO_SEED_TARGETS: usize = 5;
    let all_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, asset_id_code FROM equipment WHERE deleted_at IS NULL ORDER BY id ASC"
                .to_string(),
        ))
        .await?;
    let mut demo_targets: Vec<i64> = Vec::new();
    let mut other_targets: Vec<i64> = Vec::new();
    for row in all_rows {
        let id: i64 = row.try_get("", "id").map_err(decode)?;
        if presentation_already_seeded(db, id).await? {
            continue;
        }
        let code: Option<String> = row.try_get("", "asset_id_code").ok();
        let is_demo = code
            .as_deref()
            .is_some_and(|c| c.starts_with("GEN-PUMP-") || c.starts_with("GEN-MOTOR-"));
        if is_demo {
            demo_targets.push(id);
        } else {
            other_targets.push(id);
        }
    }
    let mut targets: Vec<i64> = Vec::new();
    for id in demo_targets.into_iter().chain(other_targets) {
        if targets.len() >= MAX_EXPLICIT_DEMO_SEED_TARGETS {
            break;
        }
        targets.push(id);
    }
    Ok(targets)
}

async fn ensure_demo_verifier_user(db: &DatabaseConnection) -> AppResult<()> {
    let count_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM user_accounts WHERE is_active = 1".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("user count missing".into()))?;
    let active_count: i64 = count_row.try_get("", "c").map_err(decode)?;
    if active_count >= 2 {
        return Ok(());
    }

    use crate::auth::password::hash_password;
    use uuid::Uuid;

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = ?",
            ["demo.verifier".into()],
        ))
        .await?;
    if existing.is_some() {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET is_active = 1 WHERE username = ?",
            ["demo.verifier".into()],
        ))
        .await?;
        return Ok(());
    }

    let password_hash = hash_password("Demo#Verifier1!")?;
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO user_accounts
            (sync_id, username, display_name, identity_mode, password_hash,
             is_active, is_admin, force_password_change, failed_login_attempts, created_at, updated_at, row_version)
           VALUES (?, ?, ?, 'local', ?, 1, 0, 0, 0, ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            "demo.verifier".into(),
            "Demo Verifier (RAMS seed)".into(),
            password_hash.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    tracing::info!("rams explicit demo seed: created demo.verifier account for WO verification");
    Ok(())
}

async fn resolve_default_equipment_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE deleted_at IS NULL ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["No equipment found in database.".into()])
        })?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_actor_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT ua.id FROM user_accounts ua \
             INNER JOIN user_scope_assignments usa ON usa.user_id = ua.id AND usa.deleted_at IS NULL \
             INNER JOIN roles r ON r.id = usa.role_id \
             WHERE ua.is_active = 1 AND r.name IN ('Administrator', 'Superadmin') \
             ORDER BY ua.id ASC LIMIT 1"
                .to_string(),
        ))
        .await?;
    if let Some(r) = row {
        return r.try_get("", "id").map_err(decode);
    }
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE is_active = 1 ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["No active user account.".into()]))?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_verifier_id(db: &DatabaseConnection, actor_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE is_active = 1 AND id <> ? ORDER BY id ASC LIMIT 1",
            [actor_id.into()],
        ))
        .await?;
    if let Some(r) = row {
        return r.try_get("", "id").map_err(decode);
    }
    Err(AppError::ValidationFailed(vec![
        "Need at least two active users: actor and verifier must differ for WO verification.".into(),
    ]))
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("last_insert_rowid missing".into()))?;
    row.try_get("", "id").map_err(decode)
}

fn decode(err: sea_orm::DbErr) -> AppError {
    AppError::SyncError(format!("rams_presentation_seed decode: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markov_graph_is_valid_json() {
        let parsed: serde_json::Value =
            serde_json::from_str(PRESENTATION_MARKOV_GRAPH).expect("valid markov json");
        assert_eq!(parsed["kind"], "discrete");
        assert_eq!(parsed["states"].as_array().map(|a| a.len()), Some(3));
    }
}
