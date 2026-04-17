use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::errors::AppError;
use crate::finance::domain::{
    AcknowledgeBudgetAlertInput, BudgetActualFilter, BudgetAlertConfigFilter, BudgetAlertEventFilter,
    BudgetCommitmentFilter, BudgetDashboardFilter, BudgetForecastFilter, BudgetReportPackFilter,
    BudgetVarianceReviewFilter, CreateBudgetActualInput, CreateBudgetAlertConfigInput,
    CreateBudgetCommitmentInput, CreateBudgetLineInput, CreateBudgetSuccessorInput,
    CreateBudgetVarianceReviewInput, CreateBudgetVersionInput, CreateCostCenterInput,
    EvaluateBudgetAlertsInput, ExportBudgetReportPackInput, GenerateBudgetForecastInput,
    ImportErpCostCenterMasterInput, PostBudgetActualInput, ReverseBudgetActualInput,
    TransitionBudgetVarianceReviewInput, TransitionBudgetVersionLifecycleInput, UpdateBudgetLineInput,
    UpdateCostCenterInput,
};
use crate::finance::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
    crate::db::seeder::seed_system_data(&db)
        .await
        .expect("seed system data");
    seed_budget_fixture_org(&db).await;
    db
}

async fn seed_budget_fixture_org(db: &DatabaseConnection) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO org_structure_models
            (sync_id, version_number, status, description, created_at, updated_at)
         VALUES ('budget-structure', 1, 'active', 'budget test structure', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    ))
    .await
    .expect("insert org structure");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO org_node_types
            (sync_id, structure_model_id, code, label, can_host_assets, can_own_work, can_carry_cost_center,
             can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at)
         VALUES (
            'budget-node-type',
            (SELECT id FROM org_structure_models WHERE sync_id = 'budget-structure'),
            'SITE',
            'Site',
            1, 1, 1, 1, 1, 1, 1,
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z'
         )",
        [],
    ))
    .await
    .expect("insert org node type");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO org_nodes
            (sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, created_at, updated_at, row_version)
         VALUES (
            'budget-site',
            'SITE-BGT',
            'Budget Site',
            (SELECT id FROM org_node_types WHERE sync_id = 'budget-node-type'),
            NULL,
            '/',
            0,
            'active',
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z',
            1
         )",
        [],
    ))
    .await
    .expect("insert org node");
}

async fn site_node_id(db: &DatabaseConnection) -> i64 {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE sync_id = 'budget-site'",
            [],
        ))
        .await
        .expect("query org node")
        .expect("org node row");
    row.try_get("", "id").expect("org node id")
}

#[tokio::test]
async fn cost_center_hierarchy_rejects_cycles_and_self_parenting() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;

    let parent = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-PARENT".to_string(),
            name: "Parent".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create parent");

    let child = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-CHILD".to_string(),
            name: "Child".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: Some(parent.id),
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create child");

    let self_parent_err = queries::update_cost_center(
        &db,
        child.id,
        child.row_version,
        UpdateCostCenterInput {
            parent_cost_center_id: Some(child.id),
            ..UpdateCostCenterInput::default()
        },
    )
    .await
    .expect_err("self parent must fail");
    assert!(matches!(self_parent_err, AppError::ValidationFailed(_)));

    let cycle_err = queries::update_cost_center(
        &db,
        parent.id,
        parent.row_version,
        UpdateCostCenterInput {
            parent_cost_center_id: Some(child.id),
            ..UpdateCostCenterInput::default()
        },
    )
    .await
    .expect_err("cycle must fail");
    assert!(matches!(cycle_err, AppError::ValidationFailed(_)));
}

#[tokio::test]
async fn only_one_frozen_baseline_allowed_per_year_and_scenario() {
    let db = setup_db().await;

    let version_a = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2026,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("Baseline A".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version A");
    let version_a = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version_a.id,
            expected_row_version: version_a.row_version,
            next_status: "submitted".to_string(),
        },
    )
    .await
    .expect("submit version A");
    let version_a = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version_a.id,
            expected_row_version: version_a.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve version A");
    queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version_a.id,
            expected_row_version: version_a.row_version,
            next_status: "frozen".to_string(),
        },
    )
    .await
    .expect("freeze version A");

    let version_b = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2026,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("Baseline B".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version B");
    let version_b = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version_b.id,
            expected_row_version: version_b.row_version,
            next_status: "submitted".to_string(),
        },
    )
    .await
    .expect("submit version B");
    let version_b = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version_b.id,
            expected_row_version: version_b.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve version B");

    let duplicate_freeze = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version_b.id,
            expected_row_version: version_b.row_version,
            next_status: "frozen".to_string(),
        },
    )
    .await
    .expect_err("duplicate frozen baseline must fail");
    assert!(matches!(duplicate_freeze, AppError::ValidationFailed(_)));
}

#[tokio::test]
async fn successor_version_creation_copies_budget_lines() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-MAIN".to_string(),
            name: "Main".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: Some("ERP-CC-MAIN".to_string()),
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let base_version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2026,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("Original".to_string()),
            planning_basis: Some("Annual baseline".to_string()),
            source_basis_mix_json: Some("{\"pm\":0.6}".to_string()),
            labor_assumptions_json: Some("{\"headcount\":12}".to_string()),
            baseline_reference: Some("FY26-BASE".to_string()),
            erp_external_ref: Some("ERP-BUDGET-26".to_string()),
        },
    )
    .await
    .expect("create base version");

    let line = queries::create_budget_line(
        &db,
        CreateBudgetLineInput {
            budget_version_id: base_version.id,
            cost_center_id: center.id,
            period_month: Some(1),
            budget_bucket: "labor".to_string(),
            planned_amount: 12000.0,
            source_basis: Some("pm_forecast".to_string()),
            justification_note: Some("January labor".to_string()),
            asset_family: Some("rotating".to_string()),
            work_category: Some("preventive".to_string()),
            shutdown_package_ref: None,
            team_id: Some(entity_id),
            skill_pool_id: None,
            labor_lane: Some("regular".to_string()),
        },
    )
    .await
    .expect("create line");
    assert_eq!(line.period_month, Some(1));

    let submitted = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: base_version.id,
            expected_row_version: base_version.row_version,
            next_status: "submitted".to_string(),
        },
    )
    .await
    .expect("submit version");
    let approved = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: submitted.id,
            expected_row_version: submitted.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve version");

    let successor = queries::create_budget_successor_version(
        &db,
        1,
        CreateBudgetSuccessorInput {
            source_version_id: approved.id,
            fiscal_year: Some(2026),
            scenario_type: Some("reforecast".to_string()),
            title: Some("Reforecast Q1".to_string()),
            baseline_reference: Some("FY26-RF1".to_string()),
        },
    )
    .await
    .expect("create successor");

    let successor_lines = queries::list_budget_lines(
        &db,
        crate::finance::domain::BudgetLineFilter {
            budget_version_id: Some(successor.id),
            cost_center_id: None,
        },
    )
    .await
    .expect("list successor lines");
    assert_eq!(successor_lines.len(), 1);
    assert_eq!(successor_lines[0].planned_amount, 12000.0);
    assert_eq!(successor_lines[0].labor_lane.as_deref(), Some("regular"));
}

#[tokio::test]
async fn stale_row_version_rejections_are_enforced_for_budget_lines() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-STV".to_string(),
            name: "Stale Version".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2027,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: None,
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");
    let line = queries::create_budget_line(
        &db,
        CreateBudgetLineInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(2),
            budget_bucket: "parts".to_string(),
            planned_amount: 5000.0,
            source_basis: Some("manual".to_string()),
            justification_note: None,
            asset_family: None,
            work_category: None,
            shutdown_package_ref: None,
            team_id: None,
            skill_pool_id: None,
            labor_lane: None,
        },
    )
    .await
    .expect("create line");

    let updated = queries::update_budget_line(
        &db,
        line.id,
        line.row_version,
        UpdateBudgetLineInput {
            planned_amount: Some(5500.0),
            ..UpdateBudgetLineInput::default()
        },
    )
    .await
    .expect("first update");
    assert_eq!(updated.planned_amount, 5500.0);

    let stale = queries::update_budget_line(
        &db,
        line.id,
        line.row_version,
        UpdateBudgetLineInput {
            planned_amount: Some(6000.0),
            ..UpdateBudgetLineInput::default()
        },
    )
    .await
    .expect_err("stale update must fail");
    assert!(matches!(stale, AppError::ValidationFailed(_)));
}

#[tokio::test]
async fn posting_and_reversal_rules_preserve_traceability() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-ACT".to_string(),
            name: "Actuals".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2028,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("FY28".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");

    let provisional = queries::create_budget_actual(
        &db,
        1,
        CreateBudgetActualInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(1),
            budget_bucket: "labor".to_string(),
            amount_source: 3200.0,
            source_currency: "EUR".to_string(),
            amount_base: 3200.0,
            base_currency: "EUR".to_string(),
            source_type: "wo_labor".to_string(),
            source_id: "WO-1-LAB-1".to_string(),
            work_order_id: None,
            equipment_id: None,
            posting_status: Some("provisional".to_string()),
            provisional_reason: Some("waiting closeout".to_string()),
            personnel_id: None,
            team_id: Some(entity_id),
            rate_card_lane: Some("regular".to_string()),
            event_at: None,
        },
        false,
    )
    .await
    .expect("create provisional actual");
    assert_eq!(provisional.posting_status, "provisional");

    let posted = queries::post_budget_actual(
        &db,
        1,
        PostBudgetActualInput {
            actual_id: provisional.id,
            expected_row_version: provisional.row_version,
        },
    )
    .await
    .expect("post provisional");
    assert_eq!(posted.posting_status, "posted");

    let reversal = queries::reverse_budget_actual(
        &db,
        1,
        ReverseBudgetActualInput {
            actual_id: posted.id,
            expected_row_version: posted.row_version,
            reason: "wrong hour-rate mapping".to_string(),
        },
    )
    .await
    .expect("reverse posted");
    assert_eq!(reversal.posting_status, "reversed");
    assert_eq!(reversal.reversal_of_actual_id, Some(posted.id));
    assert_eq!(reversal.amount_base, -3200.0);

    let listed = queries::list_budget_actuals(
        &db,
        BudgetActualFilter {
            budget_version_id: Some(version.id),
            ..BudgetActualFilter::default()
        },
    )
    .await
    .expect("list actuals");
    assert_eq!(listed.len(), 2);
}

#[tokio::test]
async fn forecast_generation_is_idempotent_and_commitments_stay_separate() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-FRC".to_string(),
            name: "Forecast".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2029,
            scenario_type: "reforecast".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("FY29 RF".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");
    queries::create_budget_line(
        &db,
        CreateBudgetLineInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(2),
            budget_bucket: "services".to_string(),
            planned_amount: 9000.0,
            source_basis: Some("backlog_demand".to_string()),
            justification_note: None,
            asset_family: None,
            work_category: Some("corrective".to_string()),
            shutdown_package_ref: None,
            team_id: None,
            skill_pool_id: None,
            labor_lane: None,
        },
    )
    .await
    .expect("create seed line");
    queries::create_budget_commitment(
        &db,
        CreateBudgetCommitmentInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(2),
            budget_bucket: "services".to_string(),
            commitment_type: "po".to_string(),
            source_type: "purchase_order".to_string(),
            source_id: "PO-77-L1".to_string(),
            obligation_amount: 4000.0,
            source_currency: "EUR".to_string(),
            base_amount: 4000.0,
            base_currency: "EUR".to_string(),
            commitment_status: Some("open".to_string()),
            work_order_id: None,
            contract_id: None,
            purchase_order_id: Some(77),
            planning_commitment_ref: None,
            due_at: None,
            explainability_note: Some("Committed vendor scope".to_string()),
        },
    )
    .await
    .expect("create commitment");

    let first = queries::generate_budget_forecasts(
        &db,
        1,
        GenerateBudgetForecastInput {
            budget_version_id: version.id,
            idempotency_key: "fy29-full-v1".to_string(),
            scope_signature: "fy29:all".to_string(),
            period_month_start: None,
            period_month_end: None,
            include_pm_occurrence: Some(true),
            include_backlog_demand: Some(true),
            include_shutdown_demand: Some(true),
            include_planning_demand: Some(true),
            include_burn_rate: Some(false),
            confidence_policy_json: None,
        },
    )
    .await
    .expect("generate first run");
    assert!(!first.reused_existing_run);
    assert!(!first.forecasts.is_empty());

    let second = queries::generate_budget_forecasts(
        &db,
        1,
        GenerateBudgetForecastInput {
            budget_version_id: version.id,
            idempotency_key: "fy29-full-v1".to_string(),
            scope_signature: "fy29:all".to_string(),
            period_month_start: None,
            period_month_end: None,
            include_pm_occurrence: Some(true),
            include_backlog_demand: Some(true),
            include_shutdown_demand: Some(true),
            include_planning_demand: Some(true),
            include_burn_rate: Some(false),
            confidence_policy_json: None,
        },
    )
    .await
    .expect("generate second run idempotent");
    assert!(second.reused_existing_run);
    assert_eq!(first.run.id, second.run.id);

    let runs = queries::list_forecast_runs(&db, Some(version.id))
        .await
        .expect("list runs");
    assert_eq!(runs.len(), 1);

    let commitments = queries::list_budget_commitments(
        &db,
        BudgetCommitmentFilter {
            budget_version_id: Some(version.id),
            ..BudgetCommitmentFilter::default()
        },
    )
    .await
    .expect("list commitments");
    assert_eq!(commitments.len(), 1);

    let actuals = queries::list_budget_actuals(
        &db,
        BudgetActualFilter {
            budget_version_id: Some(version.id),
            ..BudgetActualFilter::default()
        },
    )
    .await
    .expect("list actuals");
    assert!(actuals.is_empty());

    let forecasts = queries::list_budget_forecasts(
        &db,
        BudgetForecastFilter {
            budget_version_id: Some(version.id),
            ..BudgetForecastFilter::default()
        },
    )
    .await
    .expect("list forecasts");
    assert!(!forecasts.is_empty());
}

#[tokio::test]
async fn variance_review_lifecycle_enforces_driver_taxonomy_and_duplicate_scope_rules() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-VAR".to_string(),
            name: "Variance".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2030,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("FY30".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");

    let open = queries::create_budget_variance_review(
        &db,
        CreateBudgetVarianceReviewInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(3),
            budget_bucket: "labor".to_string(),
            variance_amount: 1550.0,
            variance_pct: 11.2,
            driver_code: "labor_overrun".to_string(),
            action_owner_id: 1,
            review_commentary: "Overtime and contractor mix created gap".to_string(),
            snapshot_context_json: "{\"planned\":12000,\"actual\":13550}".to_string(),
        },
    )
    .await
    .expect("open review");
    assert_eq!(open.review_status, "open");

    let duplicate = queries::create_budget_variance_review(
        &db,
        CreateBudgetVarianceReviewInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(3),
            budget_bucket: "labor".to_string(),
            variance_amount: 1000.0,
            variance_pct: 8.0,
            driver_code: "labor_overrun".to_string(),
            action_owner_id: 1,
            review_commentary: "Duplicate".to_string(),
            snapshot_context_json: "{}".to_string(),
        },
    )
    .await
    .expect_err("duplicate open scope must fail");
    assert!(matches!(duplicate, AppError::ValidationFailed(_)));

    let in_review = queries::transition_budget_variance_review(
        &db,
        TransitionBudgetVarianceReviewInput {
            review_id: open.id,
            expected_row_version: open.row_version,
            next_status: "in_review".to_string(),
            review_commentary: Some("Investigating WO and PM evidence".to_string()),
            reopen_reason: None,
        },
    )
    .await
    .expect("to in_review");
    let actioned = queries::transition_budget_variance_review(
        &db,
        TransitionBudgetVarianceReviewInput {
            review_id: in_review.id,
            expected_row_version: in_review.row_version,
            next_status: "actioned".to_string(),
            review_commentary: Some("Rate-card correction action started".to_string()),
            reopen_reason: None,
        },
    )
    .await
    .expect("to actioned");
    let accepted = queries::transition_budget_variance_review(
        &db,
        TransitionBudgetVarianceReviewInput {
            review_id: actioned.id,
            expected_row_version: actioned.row_version,
            next_status: "accepted".to_string(),
            review_commentary: Some("Controller accepted mitigation plan".to_string()),
            reopen_reason: None,
        },
    )
    .await
    .expect("to accepted");
    let closed = queries::transition_budget_variance_review(
        &db,
        TransitionBudgetVarianceReviewInput {
            review_id: accepted.id,
            expected_row_version: accepted.row_version,
            next_status: "closed".to_string(),
            review_commentary: Some("Closed after validated period-end posting".to_string()),
            reopen_reason: None,
        },
    )
    .await
    .expect("to closed");
    assert_eq!(closed.review_status, "closed");

    let reopen_missing_reason = queries::transition_budget_variance_review(
        &db,
        TransitionBudgetVarianceReviewInput {
            review_id: closed.id,
            expected_row_version: closed.row_version,
            next_status: "open".to_string(),
            review_commentary: Some("Need reassessment".to_string()),
            reopen_reason: None,
        },
    )
    .await
    .expect_err("reopen without reason must fail");
    assert!(matches!(reopen_missing_reason, AppError::ValidationFailed(_)));

    let reopened = queries::transition_budget_variance_review(
        &db,
        TransitionBudgetVarianceReviewInput {
            review_id: closed.id,
            expected_row_version: closed.row_version,
            next_status: "open".to_string(),
            review_commentary: Some("Material late posting changed variance".to_string()),
            reopen_reason: Some("Late posting variance shift".to_string()),
        },
    )
    .await
    .expect("reopen with reason");
    assert_eq!(reopened.review_status, "open");
    assert_eq!(reopened.reopen_reason.as_deref(), Some("Late posting variance shift"));

    let listed = queries::list_budget_variance_reviews(
        &db,
        BudgetVarianceReviewFilter {
            budget_version_id: Some(version.id),
            ..BudgetVarianceReviewFilter::default()
        },
    )
    .await
    .expect("list variance reviews");
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn dashboard_drilldown_and_erp_exports_are_traceable_and_flagged() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-ERP".to_string(),
            name: "ERP Linked".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2031,
            scenario_type: "reforecast".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("FY31 RF".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");
    queries::create_budget_line(
        &db,
        CreateBudgetLineInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(4),
            budget_bucket: "labor".to_string(),
            planned_amount: 7000.0,
            source_basis: Some("pm_forecast".to_string()),
            justification_note: None,
            asset_family: None,
            work_category: Some("preventive".to_string()),
            shutdown_package_ref: None,
            team_id: Some(entity_id),
            skill_pool_id: None,
            labor_lane: Some("regular".to_string()),
        },
    )
    .await
    .expect("create line");
    queries::create_budget_commitment(
        &db,
        CreateBudgetCommitmentInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(4),
            budget_bucket: "labor".to_string(),
            commitment_type: "contract".to_string(),
            source_type: "shutdown_package".to_string(),
            source_id: "SD-44".to_string(),
            obligation_amount: 1200.0,
            source_currency: "EUR".to_string(),
            base_amount: 1200.0,
            base_currency: "EUR".to_string(),
            commitment_status: Some("open".to_string()),
            work_order_id: None,
            contract_id: None,
            purchase_order_id: None,
            planning_commitment_ref: None,
            due_at: None,
            explainability_note: None,
        },
    )
    .await
    .expect("create commitment");
    let posted_actual = queries::create_budget_actual(
        &db,
        1,
        CreateBudgetActualInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(4),
            budget_bucket: "labor".to_string(),
            amount_source: 9000.0,
            source_currency: "USD".to_string(),
            amount_base: 8200.0,
            base_currency: "EUR".to_string(),
            source_type: "wo_labor_repeat".to_string(),
            source_id: "WO-2031-LAB".to_string(),
            work_order_id: None,
            equipment_id: None,
            posting_status: Some("posted".to_string()),
            provisional_reason: None,
            personnel_id: None,
            team_id: Some(entity_id),
            rate_card_lane: Some("regular".to_string()),
            event_at: None,
        },
        true,
    )
    .await
    .expect("create posted actual");
    assert_eq!(posted_actual.posting_status, "posted");

    queries::generate_budget_forecasts(
        &db,
        1,
        GenerateBudgetForecastInput {
            budget_version_id: version.id,
            idempotency_key: "fy31-rf-v1".to_string(),
            scope_signature: "fy31:month4".to_string(),
            period_month_start: Some(4),
            period_month_end: Some(4),
            include_pm_occurrence: Some(true),
            include_backlog_demand: Some(true),
            include_shutdown_demand: Some(true),
            include_planning_demand: Some(true),
            include_burn_rate: Some(false),
            confidence_policy_json: None,
        },
    )
    .await
    .expect("generate forecasts");

    let submitted = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version.id,
            expected_row_version: version.row_version,
            next_status: "submitted".to_string(),
        },
    )
    .await
    .expect("submit version");
    queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: submitted.id,
            expected_row_version: submitted.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve version");

    queries::import_erp_cost_center_master(
        &db,
        ImportErpCostCenterMasterInput {
            import_batch_id: "batch-erp-1".to_string(),
            records: vec![crate::finance::domain::ErpCostCenterMasterRecordInput {
                external_code: "ERP-CC-ERP".to_string(),
                external_name: "ERP Linked".to_string(),
                local_cost_center_code: Some("CC-ERP".to_string()),
                is_active: Some(false),
            }],
        },
    )
    .await
    .expect("import erp master");

    let dashboard = queries::list_budget_dashboard_rows(
        &db,
        BudgetDashboardFilter {
            budget_version_id: Some(version.id),
            ..BudgetDashboardFilter::default()
        },
    )
    .await
    .expect("dashboard rows");
    assert!(!dashboard.is_empty());
    assert!(dashboard.iter().any(|row| row.source_links_json.contains("WO-2031-LAB")));

    let drilldown = queries::list_budget_dashboard_drilldown(
        &db,
        BudgetDashboardFilter {
            budget_version_id: Some(version.id),
            ..BudgetDashboardFilter::default()
        },
    )
    .await
    .expect("drilldown rows");
    assert!(!drilldown.is_empty());
    assert!(drilldown.iter().any(|row| row.layer_type == "actual" && row.source_id.as_deref() == Some("WO-2031-LAB")));

    let posted_payload = queries::export_posted_actuals_for_erp(&db)
        .await
        .expect("posted export");
    assert!(!posted_payload.is_empty());
    assert!(posted_payload[0].reconciliation_flags.contains(&"inactive_imported_cost_center".to_string()));
    assert!(posted_payload[0].reconciliation_flags.contains(&"base_currency_drift".to_string()));

    let reforecast_payload = queries::export_approved_reforecasts_for_erp(&db)
        .await
        .expect("reforecast export");
    assert!(!reforecast_payload.is_empty());
    assert_eq!(reforecast_payload[0].scenario_type, "reforecast");
}

#[tokio::test]
async fn budget_alert_threshold_dedupe_and_ack_flow_are_governed() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-ALT".to_string(),
            name: "Alerting".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2032,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("FY32 Frozen".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: None,
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");
    queries::create_budget_line(
        &db,
        CreateBudgetLineInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(5),
            budget_bucket: "labor".to_string(),
            planned_amount: 1000.0,
            source_basis: Some("manual".to_string()),
            justification_note: None,
            asset_family: None,
            work_category: Some("corrective".to_string()),
            shutdown_package_ref: None,
            team_id: Some(entity_id),
            skill_pool_id: None,
            labor_lane: Some("overtime".to_string()),
        },
    )
    .await
    .expect("line");
    queries::create_budget_actual(
        &db,
        1,
        CreateBudgetActualInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(5),
            budget_bucket: "labor".to_string(),
            amount_source: 950.0,
            source_currency: "EUR".to_string(),
            amount_base: 950.0,
            base_currency: "EUR".to_string(),
            source_type: "wo_labor_overtime".to_string(),
            source_id: "WO-ALT-1".to_string(),
            work_order_id: None,
            equipment_id: None,
            posting_status: Some("posted".to_string()),
            provisional_reason: None,
            personnel_id: None,
            team_id: Some(entity_id),
            rate_card_lane: Some("overtime".to_string()),
            event_at: None,
        },
        true,
    )
    .await
    .expect("posted actual");
    let submitted = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: version.id,
            expected_row_version: version.row_version,
            next_status: "submitted".to_string(),
        },
    )
    .await
    .expect("submit");
    let approved = queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: submitted.id,
            expected_row_version: submitted.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve");
    queries::transition_budget_version_lifecycle(
        &db,
        1,
        TransitionBudgetVersionLifecycleInput {
            version_id: approved.id,
            expected_row_version: approved.row_version,
            next_status: "frozen".to_string(),
        },
    )
    .await
    .expect("freeze");

    queries::create_budget_alert_config(
        &db,
        CreateBudgetAlertConfigInput {
            budget_version_id: Some(version.id),
            cost_center_id: Some(center.id),
            budget_bucket: Some("labor".to_string()),
            alert_type: "threshold_80".to_string(),
            threshold_pct: Some(80.0),
            threshold_amount: None,
            recipient_user_id: Some(1),
            recipient_role_id: None,
            labor_template: Some("overtime_spike".to_string()),
            dedupe_window_minutes: Some(240),
            requires_ack: Some(true),
            is_active: Some(true),
        },
    )
    .await
    .expect("create alert config");

    let first = queries::evaluate_budget_alerts(
        &db,
        1,
        EvaluateBudgetAlertsInput {
            budget_version_id: version.id,
            emit_notifications: Some(true),
        },
    )
    .await
    .expect("first evaluate");
    assert!(first.emitted_count >= 1);

    let second = queries::evaluate_budget_alerts(
        &db,
        1,
        EvaluateBudgetAlertsInput {
            budget_version_id: version.id,
            emit_notifications: Some(true),
        },
    )
    .await
    .expect("second evaluate deduped");
    assert_eq!(second.emitted_count, 0);
    assert!(second.deduped_count >= 1);

    let listed = queries::list_budget_alert_events(
        &db,
        BudgetAlertEventFilter {
            budget_version_id: Some(version.id),
            ..BudgetAlertEventFilter::default()
        },
    )
    .await
    .expect("list alert events");
    assert!(!listed.is_empty());
    let acknowledged = queries::acknowledge_budget_alert(
        &db,
        1,
        AcknowledgeBudgetAlertInput {
            alert_event_id: listed[0].id,
            note: Some("Controller acknowledged".to_string()),
        },
    )
    .await
    .expect("ack");
    assert!(acknowledged.acknowledged_at.is_some());
}

#[tokio::test]
async fn report_pack_export_matches_dashboard_and_handles_multi_currency() {
    let db = setup_db().await;
    let entity_id = site_node_id(&db).await;
    let center = queries::create_cost_center(
        &db,
        CreateCostCenterInput {
            code: "CC-RPT".to_string(),
            name: "Reporting".to_string(),
            entity_id: Some(entity_id),
            parent_cost_center_id: None,
            budget_owner_id: None,
            erp_external_id: None,
            is_active: Some(true),
        },
    )
    .await
    .expect("create center");
    let version = queries::create_budget_version(
        &db,
        1,
        CreateBudgetVersionInput {
            fiscal_year: 2033,
            scenario_type: "approved".to_string(),
            currency_code: "EUR".to_string(),
            title: Some("FY33".to_string()),
            planning_basis: None,
            source_basis_mix_json: None,
            labor_assumptions_json: None,
            baseline_reference: Some("FY33-BASE".to_string()),
            erp_external_ref: None,
        },
    )
    .await
    .expect("create version");
    queries::create_budget_line(
        &db,
        CreateBudgetLineInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(6),
            budget_bucket: "labor".to_string(),
            planned_amount: 2000.0,
            source_basis: Some("manual".to_string()),
            justification_note: None,
            asset_family: Some("compressor".to_string()),
            work_category: Some("preventive".to_string()),
            shutdown_package_ref: None,
            team_id: Some(entity_id),
            skill_pool_id: None,
            labor_lane: Some("contractor".to_string()),
        },
    )
    .await
    .expect("line");
    queries::create_budget_commitment(
        &db,
        CreateBudgetCommitmentInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(6),
            budget_bucket: "labor".to_string(),
            commitment_type: "po".to_string(),
            source_type: "purchase_order".to_string(),
            source_id: "PO-RPT-1".to_string(),
            obligation_amount: 700.0,
            source_currency: "EUR".to_string(),
            base_amount: 700.0,
            base_currency: "EUR".to_string(),
            commitment_status: Some("open".to_string()),
            work_order_id: None,
            contract_id: None,
            purchase_order_id: None,
            planning_commitment_ref: None,
            due_at: None,
            explainability_note: None,
        },
    )
    .await
    .expect("commitment");
    queries::create_budget_actual(
        &db,
        1,
        CreateBudgetActualInput {
            budget_version_id: version.id,
            cost_center_id: center.id,
            period_month: Some(6),
            budget_bucket: "labor".to_string(),
            amount_source: 1800.0,
            source_currency: "USD".to_string(),
            amount_base: 1650.0,
            base_currency: "EUR".to_string(),
            source_type: "wo_labor".to_string(),
            source_id: "WO-RPT-1".to_string(),
            work_order_id: None,
            equipment_id: None,
            posting_status: Some("posted".to_string()),
            provisional_reason: None,
            personnel_id: None,
            team_id: Some(entity_id),
            rate_card_lane: Some("contractor".to_string()),
            event_at: None,
        },
        true,
    )
    .await
    .expect("actual");
    queries::generate_budget_forecasts(
        &db,
        1,
        GenerateBudgetForecastInput {
            budget_version_id: version.id,
            idempotency_key: "fy33-v1".to_string(),
            scope_signature: "fy33:m6".to_string(),
            period_month_start: Some(6),
            period_month_end: Some(6),
            include_pm_occurrence: Some(true),
            include_backlog_demand: Some(true),
            include_shutdown_demand: Some(true),
            include_planning_demand: Some(true),
            include_burn_rate: Some(false),
            confidence_policy_json: None,
        },
    )
    .await
    .expect("forecast");
    let report = queries::build_budget_report_pack(
        &db,
        BudgetReportPackFilter {
            budget_version_id: version.id,
            cost_center_id: Some(center.id),
            period_month_start: Some(6),
            period_month_end: Some(6),
            budget_bucket: Some("labor".to_string()),
            spend_mix: None,
            team_id: Some(entity_id),
            assignee_id: None,
            labor_lane: Some("contractor".to_string()),
            variance_driver_code: None,
        },
    )
    .await
    .expect("build report");
    assert_eq!(report.budget_version_id, version.id);
    assert!(report.totals.baseline_amount > 0.0);
    assert!(report.explainability_json.contains("lineage"));
    assert!(!report.multi_currency_flags.is_empty());

    let exported = queries::export_budget_report_pack(
        &db,
        ExportBudgetReportPackInput {
            filter: BudgetReportPackFilter {
                budget_version_id: version.id,
                cost_center_id: None,
                period_month_start: None,
                period_month_end: None,
                budget_bucket: None,
                spend_mix: None,
                team_id: None,
                assignee_id: None,
                labor_lane: None,
                variance_driver_code: None,
            },
            format: "excel".to_string(),
        },
    )
    .await
    .expect("export report");
    assert_eq!(exported.format, "excel");
    assert!(exported.file_name.ends_with(".csv"));
    assert!(exported.content.contains("baseline"));

    let alert_configs = queries::list_budget_alert_configs(
        &db,
        BudgetAlertConfigFilter {
            budget_version_id: Some(version.id),
            ..BudgetAlertConfigFilter::default()
        },
    )
    .await
    .expect("list alert configs");
    assert!(alert_configs.is_empty());
}
