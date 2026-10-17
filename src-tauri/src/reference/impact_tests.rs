//! Publish impact: changed codes against the published set, usage found through every foreign
//! key to `reference_values` and `lookup_values`, grouped per module.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::reference::domains::{self, CreateReferenceDomainPayload};
use crate::reference::impact::{module_of, value_changes};
use crate::reference::values::{self, CreateReferenceValuePayload};
use crate::reference::{publish, sets};

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn setup() -> DatabaseConnection {
    crate::db::test_db::seeded().await
}

async fn add_value(db: &DatabaseConnection, set_id: i64, code: &str) {
    values::create_value(
        db,
        CreateReferenceValuePayload {
            set_id,
            parent_id: None,
            code: code.to_string(),
            label: format!("Valeur {code}"),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        },
        1,
    )
    .await
    .expect("create value");
}

/// Domain `IMPACT_DOM` published with KEEP, DROP and OFF, then a validated candidate that removes
/// DROP and deactivates OFF. Records use DROP and OFF through a work order, an inventory and a
/// reliability table (foreign keys to `reference_values`) and through the lookup copy of the
/// domain (`impact.dom`, foreign key to `lookup_values`). Returns `(published, candidate)`.
async fn published_and_candidate(db: &DatabaseConnection) -> (i64, i64) {
    let domain = domains::create_reference_domain(
        db,
        CreateReferenceDomainPayload {
            code: "IMPACT_DOM".to_string(),
            name: "Impact".to_string(),
            structure_type: "flat".to_string(),
            governance_level: "tenant_managed".to_string(),
            governance_category: Some("controlled_catalog".to_string()),
            is_extendable: Some(true),
            validation_rules_json: None,
        },
        1,
    )
    .await
    .expect("domain");
    let first = sets::create_draft_set(db, domain.id, 1).await.expect("draft");
    for code in ["KEEP", "DROP", "OFF"] {
        add_value(db, first.id, code).await;
    }
    sets::validate_set(db, first.id, 1).await.expect("validate");
    publish::publish_reference_set(db, first.id, 1).await.expect("publish");

    let candidate = sets::create_draft_set(db, domain.id, 1).await.expect("candidate");
    exec(
        db,
        &format!(
            "DELETE FROM reference_values WHERE set_id = {} AND code = 'DROP'",
            candidate.id
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "UPDATE reference_values SET is_active = 0 WHERE set_id = {} AND code = 'OFF'",
            candidate.id
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO reference_value_migrations (domain_id, from_value_id, to_value_id, migrated_at) \
             SELECT {}, o.id, k.id, datetime('now') FROM reference_values o, reference_values k \
             WHERE o.set_id = {c} AND o.code = 'OFF' AND k.set_id = {c} AND k.code = 'KEEP'",
            domain.id,
            c = candidate.id
        ),
    )
    .await;
    sets::validate_set(db, candidate.id, 1)
        .await
        .expect("validate candidate");

    let value = |code: &str| {
        format!(
            "(SELECT id FROM reference_values WHERE set_id = {} AND code = '{code}')",
            first.id
        )
    };
    for sql in [
        "CREATE TABLE work_order_probe (id INTEGER PRIMARY KEY, reason_id INTEGER REFERENCES reference_values(id))"
            .to_string(),
        "CREATE TABLE inventory_probe (id INTEGER PRIMARY KEY, unit_id INTEGER REFERENCES lookup_values(id))"
            .to_string(),
        "CREATE TABLE failure_probe (id INTEGER PRIMARY KEY, mode_id INTEGER REFERENCES reference_values(id))"
            .to_string(),
        format!(
            "INSERT INTO work_order_probe (reason_id) VALUES ({}), ({}), ({})",
            value("DROP"),
            value("DROP"),
            value("KEEP")
        ),
        format!("INSERT INTO failure_probe (mode_id) VALUES ({})", value("OFF")),
        "INSERT INTO lookup_domains (entity_sync_id, domain_key, display_name, domain_type, is_ordered, \
         is_extensible, is_locked, schema_version, created_at, updated_at) VALUES ('ld-impact', 'impact.dom', \
         'Impact', 'tenant', 0, 1, 0, 1, datetime('now'), datetime('now'))"
            .to_string(),
        "INSERT INTO lookup_values (entity_sync_id, domain_id, code, label, sort_order, is_active, is_system, \
         created_at, updated_at, row_version) SELECT 'lv-off', id, 'OFF', 'Off', 1, 1, 0, datetime('now'), \
         datetime('now'), 1 FROM lookup_domains WHERE domain_key = 'impact.dom'"
            .to_string(),
        "INSERT INTO inventory_probe (unit_id) SELECT id FROM lookup_values WHERE entity_sync_id = 'lv-off'"
            .to_string(),
    ] {
        exec(db, &sql).await;
    }
    (first.id, candidate.id)
}

#[test]
fn tables_are_grouped_by_module() {
    assert_eq!(module_of("work_order_delays"), "work_orders");
    assert_eq!(module_of("wo_plan_actuals"), "work_orders");
    assert_eq!(module_of("articles"), "inventory");
    assert_eq!(module_of("purchase_order_lines"), "inventory");
    assert_eq!(module_of("failure_events"), "reliability_events");
    assert_eq!(module_of("fmeca_items"), "reliability_events");
    assert_eq!(module_of("equipment"), "assets");
    assert_eq!(module_of("pm_plans"), "pm_plans");
    assert_eq!(module_of("intervention_requests"), "interventions");
    assert_eq!(module_of("personnel"), "personnel");
    assert_eq!(module_of("something_else"), "other");
}

#[tokio::test]
async fn changed_codes_compare_the_candidate_with_the_published_set() {
    let db = setup().await;
    let (published, candidate) = published_and_candidate(&db).await;
    let changes = value_changes(&db, Some(published), candidate).await.expect("changes");
    assert_eq!(changes.removed, vec!["DROP".to_string()]);
    assert_eq!(changes.deactivated, vec!["OFF".to_string()]);
}

#[tokio::test]
async fn preview_counts_usage_in_work_orders_inventory_and_reliability() {
    let db = setup().await;
    let (_, candidate) = published_and_candidate(&db).await;
    let impact = publish::preview_publish_impact(&db, candidate).await.expect("impact");
    assert_eq!(impact.removed_codes, vec!["DROP".to_string()]);
    assert_eq!(impact.deactivated_codes, vec!["OFF".to_string()]);
    let dim = |name: &str| impact.dimensions.iter().find(|d| d.module == name).expect(name).clone();

    let wo = dim("work_orders");
    assert_eq!(wo.status, "available");
    assert_eq!(wo.affected_count, 2, "the record that keeps KEEP is not affected");
    assert_eq!(wo.tables[0].table, "work_order_probe");
    assert_eq!(wo.tables[0].column, "reason_id");
    assert_eq!(dim("reliability_events").affected_count, 1);
    assert_eq!(dim("inventory").affected_count, 1, "lookup copy of the domain");
    assert_eq!(dim("assets").affected_count, 0);
    assert_eq!(impact.total_affected, 4);
    assert_eq!(dim("external_integrations").status, "unavailable");
}

#[tokio::test]
async fn unchanged_set_has_no_impact() {
    let db = setup().await;
    let (published, _) = published_and_candidate(&db).await;
    // The published set compared with itself changes nothing.
    let impact = publish::preview_publish_impact(&db, published).await.expect("impact");
    assert_eq!(impact.total_affected, 0);
    assert!(impact
        .dimensions
        .iter()
        .filter(|d| d.module != "external_integrations")
        .all(|d| d.status == "no_impact"));
}
