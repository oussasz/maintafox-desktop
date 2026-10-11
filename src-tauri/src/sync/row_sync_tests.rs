//! Round trips through the generic capture/apply engine (Phase 2.4): a row created, changed and
//! deleted on device A arrives identical on device B through the protocol v2 rules.

use sea_orm::DatabaseConnection;

use super::registry::{self, ConflictPolicy};
use super::row_layout;
use super::test_support::{
    current_payload, editable_text_column, exec, scalar_i64, started_db, CreatedRow, ExchangeReport, RowFactory,
    TestServer,
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
            let diff: Vec<String> = pa
                .iter()
                .filter(|(k, v)| pb.get(*k) != Some(*v))
                .map(|(k, v)| format!("{k}: A={v} B={:?}", pb.get(k)))
                .collect();
            problems.push(format!("{} {stage}: rows differ: {}", row.entity_type, diff.join("; ")));
        }
        (Ok(Some(_)), Ok(None)) => problems.push(format!("{} {stage}: missing on B", row.entity_type)),
        (on_a, on_b) => problems.push(format!("{} {stage}: unexpected A={on_a:?} B={on_b:?}", row.entity_type)),
    }
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
pub(crate) async fn round_trip(entity_types: &[&str]) {
    let a = started_db().await;
    let b = started_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let mut problems = Vec::new();

    let mut created = Vec::new();
    for entity_type in entity_types {
        match factory.create(&a, entity_type).await {
            Ok(row) => created.push(row),
            Err(e) => problems.push(format!("create {entity_type}: {e}")),
        }
    }
    let pushed = server.exchange("A", &a).await;
    check_exchange(&pushed, "create/push", &mut problems);
    problems.extend(stage_errors(&a).await.into_iter().map(|e| format!("stage: {e}")));
    let received = server.exchange("B", &b).await;
    check_exchange(&received, "create/receive", &mut problems);
    for row in &created {
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

    for row in created.iter().rev() {
        let table = row_layout::layout(&a, row.entity_type).await.expect("layout").table();
        exec(&a, &format!("DELETE FROM \"{table}\" WHERE rowid = ?"), vec![row.rowid.into()]).await;
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
    assert!(received.materialized.errors.is_empty(), "{:?}", received.materialized.errors);
    assert_eq!(received.materialized.conflicts, 0);
    assert_eq!(received.materialized.deferred, 0);

    let on_a = balance(&a, &article.entity_sync_id, &location.entity_sync_id).await;
    let on_b = balance(&b, &article.entity_sync_id, &location.entity_sync_id).await;
    assert_eq!(on_a, vec![7000, 1000, 6000], "A: 10 in, 4 reserved, 3 issued");
    assert_eq!(on_b, on_a, "B must hold the same balance as A");
}
