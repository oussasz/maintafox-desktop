//! Round trips through the generic capture/apply engine (Phase 2.4): a row created, changed and
//! deleted on device A arrives identical on device B through the protocol v2 rules.

use sea_orm::DatabaseConnection;

use super::registry::{self, ConflictPolicy, SyncScope};
use super::row_layout;
use super::test_support::{
    created_row, current_payload, editable_text_column, exec, scalar_i64, started_db, CreatedRow, ExchangeReport,
    RowFactory, TestServer,
};

/// Identity, access, organisation, assets and reference catalogs.
pub(crate) const FOUNDATION_TYPES: &[&str] = &[
    "roles",
    "role_templates",
    "role_permissions",
    "rbac_settings",
    "user_accounts",
    "user_scope_assignments",
    "delegated_admin_policies",
    "org_structure_models",
    "org_node_types",
    "org_type_relationship_rules",
    "org_nodes",
    "org_node_responsibilities",
    "org_entity_bindings",
    "org_change_events",
    "teams",
    "personnel",
    "positions",
    "position_change_events",
    "external_companies",
    "external_company_contacts",
    "equipment",
    "equipment_classes",
    "equipment_hierarchy",
    "equipment_meters",
    "asset_meter_readings",
    "equipment_lifecycle_events",
    "asset_document_links",
    "asset_external_ids",
    "asset_photos",
    "equipment_spare_parts",
    "equipment_criticality_assessments",
    "library_documents",
    "media_attachments",
    "lookup_domains",
    "lookup_values",
    "lookup_value_aliases",
    "reference_domains",
    "reference_sets",
    "reference_values",
    "reference_aliases",
    "reference_validation_reports",
    "reference_value_migrations",
];

pub(crate) const INVENTORY_TYPES: &[&str] = &[
    "article_families",
    "articles",
    "unit_conversions",
    "article_cost_profiles",
    "warehouses",
    "stock_locations",
    "stock_reservations",
    "inventory_transactions",
    "inventory_article_equivalents",
    "inventory_count_sessions",
    "inventory_count_lines",
    "inventory_document_links",
    "inventory_mutation_audit_links",
    "inventory_reconciliation_runs",
    "inventory_reconciliation_findings",
    "inventory_state_events",
    "inventory_supplier_article_sources",
    "inventory_supplier_contacts",
    "inventory_supplier_prices",
    "inventory_suppliers",
    "inventory_valuation_policies",
    "procurement_requisitions",
    "procurement_requisition_lines",
    "purchase_orders",
    "purchase_order_lines",
    "goods_receipts",
    "goods_receipt_lines",
    "repairable_orders",
];

/// Intervention requests and work orders with their details and history.
pub(crate) const WORK_ORDER_TYPES: &[&str] = &[
    "intervention_requests",
    "di_attachments",
    "di_change_events",
    "di_review_events",
    "di_sla_rules",
    "di_state_transition_log",
    "work_order_types",
    "work_order_statuses",
    "urgency_levels",
    "delay_reason_codes",
    "closeout_validation_policies",
    "work_orders",
    "work_order_tasks",
    "work_order_interveners",
    "work_order_parts",
    "work_order_tools",
    "work_order_verifications",
    "work_order_delay_segments",
    "work_order_downtime_segments",
    "work_order_failure_details",
    "work_order_execution_events",
    "work_order_attachments",
    "wo_action_events",
    "wo_change_events",
    "wo_state_transition_log",
];

/// Preventive maintenance and planning.
pub(crate) const PM_PLANNING_TYPES: &[&str] = &[
    "pm_plans",
    "pm_plan_versions",
    "pm_occurrences",
    "pm_executions",
    "pm_findings",
    "pm_occurrence_transitions",
    "pm_trigger_events",
    "planning_windows",
    "capacity_rules",
    "schedule_candidates",
    "schedule_commitments",
    "schedule_break_ins",
    "schedule_change_log",
    "schedule_details",
];

/// Budgets, costs, forecasts and ERP exports.
pub(crate) const FINANCE_TYPES: &[&str] = &[
    "cost_centers",
    "budget_versions",
    "budget_lines",
    "budget_actuals",
    "budget_commitments",
    "budget_erp_cost_center_master",
    "budget_forecast_runs",
    "budget_forecasts",
    "budget_variance_reviews",
    "budget_alert_configs",
    "budget_alert_events",
    "posted_export_batches",
    "integration_exceptions",
];

/// Personnel records, skills and qualifications.
pub(crate) const PERSONNEL_TYPES: &[&str] = &[
    "personnel_assignment_history",
    "personnel_authorizations",
    "personnel_availability_blocks",
    "personnel_rate_cards",
    "personnel_skills",
    "personnel_team_assignments",
    "skill_categories",
    "skill_definitions",
    "team_skill_requirements",
    "certification_types",
    "personnel_certifications",
    "qualification_requirement_profiles",
    "qualification_profile_skills",
    "training_sessions",
    "training_attendance",
    "document_acknowledgements",
];

/// Inspections and work permits.
pub(crate) const INSPECTION_PERMIT_TYPES: &[&str] = &[
    "inspection_templates",
    "inspection_template_versions",
    "inspection_checkpoints",
    "inspection_rounds",
    "inspection_results",
    "inspection_evidence",
    "inspection_anomalies",
    "permit_types",
    "work_permits",
    "permit_handover_logs",
    "permit_isolations",
    "permit_suspensions",
    "loto_card_print_jobs",
];

/// Reliability models, failure data, data integrity and the analytics contract.
pub(crate) const RELIABILITY_TYPES: &[&str] = &[
    "failure_hierarchies",
    "failure_codes",
    "failure_events",
    "runtime_exposure_logs",
    "ram_advanced_guardrails",
    "reliability_beta_benchmarks",
    "reliability_dashboard_policies",
    "fmeca_analyses",
    "fmeca_items",
    "fmeca_item_parts",
    "rcm_studies",
    "rcm_decisions",
    "fta_models",
    "rbd_models",
    "event_tree_models",
    "markov_models",
    "mc_models",
    "weibull_fit_results",
    "ram_expert_sign_offs",
    "ram_ishikawa_diagrams",
    "user_dismissals",
    "data_integrity_findings",
    "data_integrity_repair_actions",
    "analytics_contract_versions",
];

/// Settings, history logs, notifications and reports.
pub(crate) const CONFIG_NOTIFICATION_TYPES: &[&str] = &[
    "app_settings",
    "policy_snapshots",
    "retention_policies",
    "activity_events",
    "audit_events",
    "admin_change_events",
    "settings_change_events",
    "notification_categories",
    "notification_escalation_policies",
    "notification_rules",
    "notification_escalation_steps",
    "notification_events",
    "notifications",
    "notification_acknowledgements",
    "report_templates",
    "report_schedules",
];

/// Personal items of a user.
pub(crate) const PERSONAL_TYPES: &[&str] = &[
    "notification_preferences",
    "saved_activity_filters",
    "user_dashboard_layouts",
    "user_analytics_dashboard_layouts",
];

async fn compare(
    a: &DatabaseConnection,
    b: &DatabaseConnection,
    row: &CreatedRow,
    stage: &str,
    problems: &mut Vec<String>,
) {
    let on_a = current_payload(a, row.entity_type, &row.entity_sync_id, &row.payload).await;
    let on_b = current_payload(b, row.entity_type, &row.entity_sync_id, &row.payload).await;
    match (on_a, on_b) {
        (Ok(Some(pa)), Ok(Some(pb))) if pa == pb => {}
        (Ok(Some(pa)), Ok(Some(pb))) => {
            let unreceived = personal_links_not_received(b, row.entity_type, &pa, &pb).await;
            let diff: Vec<String> = pa
                .iter()
                .filter(|(k, v)| pb.get(*k) != Some(*v) && !unreceived.contains(*k))
                .map(|(k, v)| format!("{k}: A={v} B={:?}", pb.get(k)))
                .collect();
            if !diff.is_empty() {
                problems.push(format!("{} {stage}: rows differ: {}", row.entity_type, diff.join("; ")));
            }
        }
        (Ok(Some(_)), Ok(None)) => problems.push(format!("{} {stage}: missing on B", row.entity_type)),
        (on_a, on_b) => problems.push(format!("{} {stage}: unexpected A={on_a:?} B={on_b:?}", row.entity_type)),
    }
}

/// Payload keys of soft links to another person's row (a user-scoped entity) that `b` has not
/// received: such a link stays empty on devices of other users (`row_spec::soft`).
async fn personal_links_not_received(
    b: &DatabaseConnection,
    entity_type: &str,
    pa: &serde_json::Map<String, serde_json::Value>,
    pb: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    let layout = row_layout::layout(b, entity_type).await.expect("layout");
    let mut keys = Vec::new();
    for column in &layout.columns {
        let row_layout::Role::Ref {
            key,
            target:
                row_layout::Target::Entity {
                    entity_type: target,
                    table,
                },
            soft: true,
            ..
        } = &column.role
        else {
            continue;
        };
        let personal = registry::entity(target).is_some_and(|d| d.scope == SyncScope::User);
        let (Some(serde_json::Value::String(sync_id)), Some(serde_json::Value::Null)) = (pa.get(key), pb.get(key))
        else {
            continue;
        };
        let present = scalar_i64(
            b,
            &format!("SELECT COUNT(*) FROM \"{table}\" WHERE entity_sync_id = ?"),
            vec![sync_id.clone().into()],
        )
        .await;
        if personal && present == 0 {
            keys.push(key.clone());
        }
    }
    keys
}

fn check_exchange(report: &ExchangeReport, stage: &str, problems: &mut Vec<String>) {
    let m = &report.materialized;
    if m.conflicts > 0 || !m.errors.is_empty() || m.deferred > 0 {
        problems.push(format!(
            "{stage}: materialize conflicts={} deferred={} errors={:?}",
            m.conflicts, m.deferred, m.errors
        ));
    }
    if report.conflicts > 0 {
        problems.push(format!("{stage}: {} server conflicts", report.conflicts));
    }
}

async fn stage_errors(db: &DatabaseConnection) -> Vec<String> {
    let rows = super::queries::list_sync_conflicts(
        db,
        super::domain::SyncConflictFilter {
            conflict_type: Some("STAGE_ERROR".into()),
            ..Default::default()
        },
    )
    .await
    .expect("conflicts");
    rows.iter()
        .map(|c| format!("{} {}: {:?}", c.entity_type, c.entity_sync_id, c.local_payload_json))
        .collect()
}

/// Create, update and delete one row of every listed entity type on A; after each step A and B
/// hold the same rows.
/// Tables that hold exactly one row (`CHECK (id = 1)`), seeded on every device with the same
/// identity: the round trip edits the seeded row instead of creating and deleting one.
pub(crate) const SINGLETON_TYPES: &[&str] = &["ram_advanced_guardrails"];

pub(crate) async fn round_trip(entity_types: &[&str]) {
    let a = started_db().await;
    let b = started_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let mut problems = Vec::new();

    let mut created = Vec::new();
    for entity_type in entity_types {
        if SINGLETON_TYPES.contains(entity_type) {
            let table = row_layout::layout(&a, entity_type).await.expect("layout").table();
            let rowid = scalar_i64(&a, &format!("SELECT rowid FROM \"{table}\""), vec![]).await;
            match created_row(&a, entity_type, rowid).await {
                Ok(row) => created.push(row),
                Err(e) => problems.push(format!("seeded {entity_type}: {e}")),
            }
            continue;
        }
        match factory.create(&a, entity_type).await {
            Ok(row) => created.push(row),
            Err(e) => problems.push(format!("create {entity_type}: {e}")),
        }
    }
    // Personal rows travel only to devices where their owner is signed in or trusted: they are
    // given to the seeded administrator (same identity on A and B), who is trusted on B. All rows
    // of personal tables move (a personal row's personal parent belongs to the same person); a
    // table keyed by its owner moves only the created row.
    let admin = "SELECT id FROM user_accounts ORDER BY id LIMIT 1";
    let mut personal = false;
    for row in created.iter_mut() {
        let def = registry::entity(row.entity_type).expect("registered");
        let (SyncScope::User, Some(owner)) = (def.scope, def.owner_user_column) else {
            continue;
        };
        personal = true;
        let layout = row_layout::layout(&a, row.entity_type).await.expect("layout");
        if layout.natural_pk.is_some() {
            exec(
                &a,
                &format!("UPDATE \"{}\" SET \"{owner}\" = ({admin}) WHERE rowid = ?", def.table),
                vec![row.rowid.into()],
            )
            .await;
            let rowid = scalar_i64(
                &a,
                &format!("SELECT rowid FROM \"{}\" WHERE \"{owner}\" = ({admin})", def.table),
                vec![],
            )
            .await;
            *row = created_row(&a, row.entity_type, rowid).await.expect("personal row");
        } else {
            exec(
                &a,
                &format!("UPDATE \"{}\" SET \"{owner}\" = ({admin})", def.table),
                vec![],
            )
            .await;
            *row = created_row(&a, row.entity_type, row.rowid).await.expect("personal row");
        }
    }
    if personal {
        exec(
            &b,
            &format!(
                "INSERT INTO trusted_devices (id, device_fingerprint, user_id, trusted_at, is_revoked)
                 VALUES ('round-trip', 'round-trip-b', ({admin}), '2026-10-11T00:00:00Z', 0)"
            ),
            vec![],
        )
        .await;
    }
    let pushed = server.exchange("A", &a).await;
    check_exchange(&pushed, "create/push", &mut problems);
    problems.extend(stage_errors(&a).await.into_iter().map(|e| format!("stage: {e}")));
    let received = server.exchange("B", &b).await;
    check_exchange(&received, "create/receive", &mut problems);
    // A seeded singleton is not sent until it changes: each device keeps its own seeding time.
    for row in created.iter().filter(|row| !SINGLETON_TYPES.contains(&row.entity_type)) {
        compare(&a, &b, row, "after create", &mut problems).await;
    }

    let mut updated = Vec::new();
    for row in &created {
        let def = registry::entity(row.entity_type).expect("registered");
        if def.conflict_policy == ConflictPolicy::AppendOnly {
            continue;
        }
        let Some(column) = editable_text_column(&a, row.entity_type).await else {
            continue;
        };
        exec(
            &a,
            &format!("UPDATE \"{}\" SET \"{column}\" = ? WHERE rowid = ?", def.table),
            vec![format!("edited {}", row.entity_type).into(), row.rowid.into()],
        )
        .await;
        updated.push(row);
    }
    let pushed = server.exchange("A", &a).await;
    check_exchange(&pushed, "update/push", &mut problems);
    let received = server.exchange("B", &b).await;
    check_exchange(&received, "update/receive", &mut problems);
    for row in updated {
        compare(&a, &b, row, "after update", &mut problems).await;
    }

    let created: Vec<_> = created
        .into_iter()
        .filter(|row| !SINGLETON_TYPES.contains(&row.entity_type))
        .collect();
    for row in created.iter().rev() {
        let table = row_layout::layout(&a, row.entity_type).await.expect("layout").table();
        exec(
            &a,
            &format!("DELETE FROM \"{table}\" WHERE rowid = ?"),
            vec![row.rowid.into()],
        )
        .await;
    }
    let pushed = server.exchange("A", &a).await;
    check_exchange(&pushed, "delete/push", &mut problems);
    let received = server.exchange("B", &b).await;
    check_exchange(&received, "delete/receive", &mut problems);
    for row in &created {
        match current_payload(&b, row.entity_type, &row.entity_sync_id, &row.payload).await {
            Ok(None) => {}
            other => problems.push(format!("{} still on B after delete: {other:?}", row.entity_type)),
        }
    }

    assert!(problems.is_empty(), "round trip problems:\n{}", problems.join("\n"));
}

#[tokio::test]
async fn foundation_entities_round_trip() {
    round_trip(FOUNDATION_TYPES).await;
}

#[tokio::test]
async fn work_orders_and_requests_round_trip() {
    round_trip(WORK_ORDER_TYPES).await;
}

#[tokio::test]
async fn pm_and_planning_round_trip() {
    round_trip(PM_PLANNING_TYPES).await;
}

#[tokio::test]
async fn finance_round_trip() {
    round_trip(FINANCE_TYPES).await;
}

#[tokio::test]
async fn personnel_and_qualification_round_trip() {
    round_trip(PERSONNEL_TYPES).await;
}

#[tokio::test]
async fn inspections_and_permits_round_trip() {
    round_trip(INSPECTION_PERMIT_TYPES).await;
}

#[tokio::test]
async fn config_and_notifications_round_trip() {
    round_trip(CONFIG_NOTIFICATION_TYPES).await;
}

#[tokio::test]
async fn personal_items_round_trip() {
    round_trip(PERSONAL_TYPES).await;
}

#[tokio::test]
async fn reliability_round_trip() {
    round_trip(RELIABILITY_TYPES).await;
}

#[tokio::test]
async fn inventory_and_procurement_round_trip() {
    round_trip(INVENTORY_TYPES).await;
}

async fn balance(db: &DatabaseConnection, article_sync: &str, location_sync: &str) -> Vec<i64> {
    let mut out = Vec::new();
    for col in ["on_hand_qty", "reserved_qty", "available_qty"] {
        out.push(
            scalar_i64(
                db,
                &format!(
                    "SELECT CAST({col} * 1000 AS INTEGER) FROM stock_balances
                     WHERE article_id = (SELECT id FROM articles WHERE entity_sync_id = ?)
                       AND location_id = (SELECT id FROM stock_locations WHERE entity_sync_id = ?)"
                ),
                vec![article_sync.into(), location_sync.into()],
            )
            .await,
        );
    }
    out
}

/// Stock movements made on A change the stock balances of B exactly as they changed on A.
#[tokio::test]
async fn received_stock_movements_update_stock_balances() {
    use crate::inventory::domain::{InventoryIssueInput, InventoryReserveInput, InventoryStockAdjustInput};
    use crate::inventory::queries::{adjust_stock, issue_reserved_stock, reserve_stock};

    let a = started_db().await;
    let b = started_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let article = factory.create(&a, "articles").await.expect("article");
    let location = factory.create(&a, "stock_locations").await.expect("location");

    adjust_stock(
        &a,
        InventoryStockAdjustInput {
            article_id: article.rowid,
            location_id: location.rowid,
            delta_qty: 10.0,
            reason_code: None,
            notes: Some("initial".into()),
            source_ref: None,
        },
    )
    .await
    .expect("adjust in");
    let reservation = reserve_stock(
        &a,
        InventoryReserveInput {
            article_id: article.rowid,
            location_id: location.rowid,
            quantity: 4.0,
            source_type: "MANUAL".into(),
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("reserve");
    issue_reserved_stock(
        &a,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity: 3.0,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("issue");

    let pushed = server.exchange("A", &a).await;
    assert_eq!(pushed.conflicts, 0);
    let received = server.exchange("B", &b).await;
    assert!(
        received.materialized.errors.is_empty(),
        "{:?}",
        received.materialized.errors
    );
    assert_eq!(received.materialized.conflicts, 0);
    assert_eq!(received.materialized.deferred, 0);

    let on_a = balance(&a, &article.entity_sync_id, &location.entity_sync_id).await;
    let on_b = balance(&b, &article.entity_sync_id, &location.entity_sync_id).await;
    assert_eq!(on_a, vec![7000, 1000, 6000], "A: 10 in, 4 reserved, 3 issued");
    assert_eq!(on_b, on_a, "B must hold the same balance as A");
}

/// Every domain list above, in domain order.
const DOMAIN_TYPES: &[&[&str]] = &[
    FOUNDATION_TYPES,
    INVENTORY_TYPES,
    WORK_ORDER_TYPES,
    PM_PLANNING_TYPES,
    FINANCE_TYPES,
    PERSONNEL_TYPES,
    INSPECTION_PERMIT_TYPES,
    RELIABILITY_TYPES,
    CONFIG_NOTIFICATION_TYPES,
    PERSONAL_TYPES,
];

/// Phase 2.5: generated from the registry. Every synchronised type is in a domain round trip (a
/// new type cannot be registered without one), and all of them travel together between two
/// devices: created, edited and deleted on A, identical on B. A type without capture rules or
/// without a materializer fails here (`row_spec`, `registry_flags_match_stager_and_materializer_dispatch`).
#[tokio::test]
async fn every_registered_type_round_trips() {
    let listed: std::collections::HashSet<&str> = DOMAIN_TYPES.iter().flat_map(|d| d.iter().copied()).collect();
    let unlisted: Vec<&str> = registry::SYNC_ENTITIES
        .iter()
        .map(|e| e.entity_type)
        .filter(|t| !listed.contains(t))
        .collect();
    assert!(
        unlisted.is_empty(),
        "registered types in no domain round trip: {unlisted:?}"
    );
    let unregistered: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|t| registry::entity(t).is_none())
        .collect();
    assert!(
        unregistered.is_empty(),
        "round-trip types that are not registered: {unregistered:?}"
    );
    let all: Vec<&str> = registry::SYNC_ENTITIES.iter().map(|e| e.entity_type).collect();
    round_trip(&all).await;
}
