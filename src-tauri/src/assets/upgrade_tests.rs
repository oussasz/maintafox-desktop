//! Asset registry upgrade: commercial data, paged search, maintenance profile, spare-parts list
//! and criticality assessment, against a migrated in-memory database.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::assets::criticality::{self, AssessCriticalityPayload, CriticalityScores};
use crate::assets::hierarchy::{self, LinkAssetPayload};
use crate::assets::identity::{self, AssetCommercialInput, CreateAssetPayload};
use crate::assets::maintenance_profile;
use crate::assets::search::{self, AssetSearchFilters};
use crate::assets::spare_parts::{self, AddSparePartPayload, SparePartInput};
use crate::errors::AppError;
use crate::org::node_types::{self, CreateNodeTypePayload};
use crate::org::nodes::{self, CreateOrgNodePayload};
use crate::org::structure_model::{self, CreateStructureModelPayload};

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    let now = chrono::Utc::now().to_rfc3339();
    exec(
        &db,
        &format!(
            "INSERT INTO equipment_classes (entity_sync_id, code, name, parent_id, level, is_active, created_at, updated_at) \
             VALUES ('upg-class-pump', 'PUMP', 'Pompes', NULL, 'class', 1, '{now}', '{now}')"
        ),
    )
    .await;
    db
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

/// Published model with one site node carrying cost centre `CC-PRD`.
async fn site_with_cost_center(db: &DatabaseConnection) -> i64 {
    let model = structure_model::create_model(
        db,
        CreateStructureModelPayload {
            description: Some("upgrade tests".into()),
        },
        1,
    )
    .await
    .expect("model");
    let site_type = node_types::create_node_type(
        db,
        CreateNodeTypePayload {
            structure_model_id: model.id,
            code: "SITE".into(),
            label: "Site".into(),
            icon_key: None,
            color: None,
            depth_hint: Some(0),
            can_host_assets: true,
            can_own_work: true,
            can_carry_cost_center: true,
            can_aggregate_kpis: true,
            can_receive_permits: false,
            is_root_type: true,
        },
    )
    .await
    .expect("site type");
    structure_model::publish_model(db, model.id, 1).await.expect("publish");
    nodes::create_org_node(
        db,
        CreateOrgNodePayload {
            code: "SITE-UPG".into(),
            name: "Usine".into(),
            node_type_id: site_type.id.into(),
            parent_id: None,
            description: None,
            cost_center_code: Some("CC-PRD".into()),
            external_reference: None,
            effective_from: None,
            erp_reference: None,
            notes: None,
            structure_model_id: i64::from(model.id),
        },
        1,
    )
    .await
    .expect("site node")
    .id
}

fn payload(code: &str, org_node_id: i64, commercial: Option<AssetCommercialInput>) -> CreateAssetPayload {
    CreateAssetPayload {
        asset_code: code.into(),
        asset_name: format!("Pompe {code}"),
        class_code: "PUMP".into(),
        family_code: None,
        subfamily_code: None,
        criticality_code: "C".into(),
        status_code: "ACTIVE_IN_SERVICE".into(),
        manufacturer: None,
        model: None,
        serial_number: None,
        maintainable_boundary: true,
        rams_schedule_reference_value_id: None,
        rams_utilization_factor: None,
        org_node_id,
        commissioned_at: None,
        commercial,
    }
}

fn commercial(purchase: &str, warranty: &str, cost_center: Option<&str>) -> AssetCommercialInput {
    AssetCommercialInput {
        purchase_date: Some(purchase.into()),
        warranty_expiry_date: Some(warranty.into()),
        replacement_value: Some(125_000.0),
        cost_center_code: cost_center.map(Into::into),
        notes: Some("  Fournisseur: KSB  ".into()),
    }
}

fn issue_code(err: &AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

#[tokio::test]
async fn commercial_data_is_saved_and_location_cost_centre_is_inherited() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    exec(
        &db,
        "INSERT INTO cost_centers (code, name, is_active) VALUES ('CC-MNT', 'Maintenance', 1)",
    )
    .await;

    let asset = identity::create_asset(
        &db,
        payload(
            "PMP-001",
            site,
            Some(commercial("2024-01-10", "2026-01-10", Some("cc-mnt"))),
        ),
        1,
    )
    .await
    .expect("create");
    assert_eq!(asset.purchase_date.as_deref(), Some("2024-01-10"));
    assert_eq!(asset.warranty_expiry_date.as_deref(), Some("2026-01-10"));
    assert_eq!(asset.replacement_value, Some(125_000.0));
    assert_eq!(
        asset.cost_center_code.as_deref(),
        Some("CC-MNT"),
        "stored with the catalogue's code"
    );
    assert_eq!(asset.org_cost_center_code.as_deref(), Some("CC-PRD"));
    assert_eq!(asset.notes.as_deref(), Some("Fournisseur: KSB"));

    let cleared = identity::update_asset_commercial(&db, asset.id, AssetCommercialInput::default(), asset.row_version)
        .await
        .expect("clear");
    assert_eq!(cleared.cost_center_code, None);
    assert_eq!(cleared.purchase_date, None);
    assert_eq!(cleared.row_version, asset.row_version + 1);

    let stale = identity::update_asset_commercial(&db, asset.id, AssetCommercialInput::default(), asset.row_version)
        .await
        .expect_err("stale version");
    assert_eq!(issue_code(&stale), "ASSET_STALE_VERSION");
}

#[tokio::test]
async fn commercial_data_rules_are_enforced() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;

    let err = identity::create_asset(
        &db,
        payload("PMP-002", site, Some(commercial("2025-05-01", "2025-04-30", None))),
        1,
    )
    .await
    .expect_err("warranty before purchase");
    assert_eq!(issue_code(&err), "ASSET_WARRANTY_BEFORE_PURCHASE");

    let err = identity::create_asset(
        &db,
        payload(
            "PMP-003",
            site,
            Some(commercial("2025-05-01", "2027-05-01", Some("NOPE"))),
        ),
        1,
    )
    .await
    .expect_err("unknown cost centre");
    assert_eq!(issue_code(&err), "ASSET_COST_CENTER_UNKNOWN");

    let err = identity::create_asset(
        &db,
        payload("PMP-004", site, Some(commercial("01/05/2025", "2027-05-01", None))),
        1,
    )
    .await
    .expect_err("not an ISO date");
    assert_eq!(issue_code(&err), "ASSET_DATE_INVALID");

    let mut negative = commercial("2025-05-01", "2027-05-01", None);
    negative.replacement_value = Some(-1.0);
    let err = identity::create_asset(&db, payload("PMP-005", site, Some(negative)), 1)
        .await
        .expect_err("negative value");
    assert_eq!(issue_code(&err), "ASSET_REPLACEMENT_VALUE_INVALID");
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM equipment WHERE asset_id_code LIKE 'PMP-00%'").await,
        0
    );
}

fn filters(limit: u64, offset: u64) -> AssetSearchFilters {
    AssetSearchFilters {
        query: None,
        class_codes: None,
        family_codes: None,
        status_codes: None,
        org_node_ids: None,
        include_decommissioned: None,
        limit: Some(limit),
        registry_focus: None,
        offset: Some(offset),
        criticality_codes: None,
        org_subtree: None,
    }
}

#[tokio::test]
async fn search_page_reports_the_total_beyond_the_page() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    for code in ["PMP-101", "PMP-102", "PMP-103"] {
        identity::create_asset(&db, payload(code, site, None), 1)
            .await
            .expect("create");
    }
    let first = search::search_assets_page(&db, filters(2, 0)).await.expect("page 1");
    assert_eq!(first.total, 3);
    assert_eq!(
        first.items.iter().map(|a| a.asset_code.as_str()).collect::<Vec<_>>(),
        ["PMP-101", "PMP-102"]
    );
    let second = search::search_assets_page(&db, filters(2, 2)).await.expect("page 2");
    assert_eq!(second.total, 3);
    assert_eq!(second.items.len(), 1);

    let mut by_class = filters(10, 0);
    by_class.criticality_codes = Some(vec!["A".into()]);
    assert_eq!(
        search::search_assets_page(&db, by_class).await.expect("filter").total,
        0
    );
    let mut subtree = filters(10, 0);
    subtree.org_node_ids = Some(vec![site]);
    subtree.org_subtree = Some(true);
    assert_eq!(
        search::search_assets_page(&db, subtree).await.expect("subtree").total,
        3
    );
}

/// Text search folds case and accents in Rust: the previous SQL folding (dozens of nested
/// `REPLACE()`) overflowed the SQLCipher parser stack, so every search failed in the app.
#[tokio::test]
async fn text_search_is_accent_insensitive_and_ranks_the_code_first() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    let mut exchanger = payload("HX-12", site, None);
    exchanger.asset_name = "Échangeur à plaques HX-12".into();
    identity::create_asset(&db, exchanger, 1).await.expect("exchanger");
    for code in ["PMP-1020", "PMP-102", "PMP-1"] {
        identity::create_asset(&db, payload(code, site, None), 1)
            .await
            .expect("pump");
    }

    let mut by_name = filters(10, 0);
    by_name.query = Some("ECHANGEUR".into());
    let page = search::search_assets_page(&db, by_name).await.expect("name search");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].asset_code, "HX-12");

    let mut by_code = filters(10, 0);
    by_code.query = Some("pmp-102".into());
    let page = search::search_assets_page(&db, by_code).await.expect("code search");
    assert_eq!(
        page.items.iter().map(|a| a.asset_code.as_str()).collect::<Vec<_>>(),
        ["PMP-102", "PMP-1020"],
        "exact code first, then code prefix"
    );
    assert_eq!(page.total, 2);

    let names = search::suggest_asset_names(&db, "échang", Some(5))
        .await
        .expect("names");
    assert_eq!(names.len(), 1);
    let codes = search::suggest_asset_codes(&db, "pmp-10", Some(5))
        .await
        .expect("codes");
    assert_eq!(
        codes.iter().map(|a| a.asset_code.as_str()).collect::<Vec<_>>(),
        ["PMP-102", "PMP-1020"]
    );
}

/// A registry of a few thousand assets: one text-match scan per page request (count and page
/// share it), ranking by exact code / prefix / code order, and every match counted.
#[tokio::test]
async fn paged_text_search_scans_once_on_a_large_registry() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    identity::create_asset(&db, payload("PMP-0001", site, None), 1)
        .await
        .expect("template");
    exec(
        &db,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, class_id, lifecycle_status, \
            criticality_value_id, equipment_status_ref_id, equipment_criticality_ref_id, \
            equipment_class_ref_id, installed_at_node_id, maintainable_boundary, \
            rams_utilization_factor, created_at, updated_at, row_version) \
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 3000) \
         SELECT 'bulk-' || i, printf('BLK-%04d', i), 'Pompe ' || i, t.class_id, t.lifecycle_status, \
            t.criticality_value_id, t.equipment_status_ref_id, t.equipment_criticality_ref_id, \
            t.equipment_class_ref_id, t.installed_at_node_id, t.maintainable_boundary, \
            t.rams_utilization_factor, t.created_at, t.updated_at, 1 \
         FROM n, equipment t WHERE t.asset_id_code = 'PMP-0001'",
    )
    .await;

    let scans = || search::TEXT_MATCH_SCANS.with(std::cell::Cell::get);
    let before = scans();
    let mut f = filters(50, 0);
    f.query = Some("blk-2".into());
    let page = search::search_assets_page(&db, f).await.expect("prefix search");
    assert_eq!(scans() - before, 1, "count and page share one scan");
    assert_eq!(page.total, 1000, "BLK-2000 .. BLK-2999");
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.items[0].asset_code, "BLK-2000");
    assert_eq!(page.items[49].asset_code, "BLK-2049");

    let mut exact = filters(5, 0);
    exact.query = Some("blk-2500".into());
    let page = search::search_assets_page(&db, exact).await.expect("exact search");
    assert_eq!(page.items[0].asset_code, "BLK-2500");

    let mut second = filters(50, 50);
    second.query = Some("blk-2".into());
    let page = search::search_assets_page(&db, second).await.expect("second page");
    assert_eq!(page.items[0].asset_code, "BLK-2050");
}

async fn work_order(
    db: &DatabaseConnection,
    code: &str,
    asset_id: i64,
    type_code: &str,
    status: &str,
    total: f64,
    at: &str,
) {
    exec(
        db,
        &format!(
            "INSERT INTO work_orders (code, type_id, status_id, equipment_id, title, labor_cost, parts_cost, service_cost, \
               total_cost, downtime_hours, created_at, updated_at, closed_at) VALUES ('{code}', \
               (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = '{type_code}'), \
               (SELECT id FROM work_order_statuses WHERE code = '{status}'), {asset_id}, 'Test {code}', \
               {labor}, {parts}, 0, {total}, 2, '{at}', '{at}', '{at}')",
            labor = total * 0.6,
            parts = total * 0.4,
        ),
    )
    .await;
}

#[tokio::test]
async fn maintenance_profile_sums_costs_and_rolls_up_components() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    let mut parent_payload = payload("PMP-201", site, None);
    parent_payload.commercial = Some(AssetCommercialInput {
        replacement_value: Some(10_000.0),
        ..AssetCommercialInput::default()
    });
    let parent = identity::create_asset(&db, parent_payload, 1).await.expect("parent");
    let child = identity::create_asset(&db, payload("PMP-202", site, None), 1)
        .await
        .expect("child");
    hierarchy::link_asset_hierarchy(
        &db,
        LinkAssetPayload {
            parent_asset_id: parent.id,
            child_asset_id: child.id,
            relation_type: "PARENT_CHILD".into(),
            effective_from: None,
        },
        1,
    )
    .await
    .expect("link");

    let now = chrono::Utc::now();
    let recent = (now - chrono::Duration::days(30)).to_rfc3339();
    let old = (now - chrono::Duration::days(800)).to_rfc3339();
    work_order(&db, "OT-1", parent.id, "preventive", "closed", 1000.0, &old).await;
    work_order(&db, "OT-2", parent.id, "corrective", "closed", 500.0, &recent).await;
    work_order(&db, "OT-3", parent.id, "corrective", "cancelled", 9999.0, &recent).await;
    work_order(&db, "OT-4", child.id, "emergency", "closed", 250.0, &recent).await;

    let own = maintenance_profile::get_asset_maintenance_profile(&db, parent.id, false, now)
        .await
        .expect("own profile");
    assert_eq!(own.asset_count, 1);
    assert_eq!(own.work_order_count, 2, "the cancelled work order is ignored");
    assert_eq!(own.failure_work_order_count, 1);
    assert_eq!(own.planned_work_order_count, 1);
    assert!((own.cost_lifetime.total - 1500.0).abs() < 1e-9);
    assert!((own.cost_recent.total - 500.0).abs() < 1e-9);
    assert!((own.cost_lifetime.labor - 900.0).abs() < 1e-9);
    assert_eq!(own.cost_by_year.len(), 2);
    assert!((own.cost_to_replacement_ratio.expect("ratio") - 0.15).abs() < 1e-9);
    assert_eq!(own.last_failure_at.as_deref(), Some(recent.as_str()));

    let rolled = maintenance_profile::get_asset_maintenance_profile(&db, parent.id, true, now)
        .await
        .expect("rolled up profile");
    assert_eq!(rolled.asset_count, 2);
    assert_eq!(rolled.work_order_count, 3);
    assert!((rolled.cost_lifetime.total - 1750.0).abs() < 1e-9);
    assert_eq!(rolled.failure_work_order_count, 2);
}

async fn article(db: &DatabaseConnection, code: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) VALUES ('upg-{code}', '{code}', 'Roulement {code}', \
             (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
              JOIN reference_domains d ON d.id = s.domain_id \
              WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'U'))"
        ),
    )
    .await;
    scalar(db, &format!("SELECT id FROM articles WHERE article_code = '{code}'")).await
}

fn line(quantity: f64, critical: bool) -> SparePartInput {
    SparePartInput {
        quantity,
        position_label: Some(" Palier côté accouplement ".into()),
        is_critical_spare: critical,
        notes: None,
    }
}

#[tokio::test]
async fn spare_parts_list_add_update_remove() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    let asset = identity::create_asset(&db, payload("PMP-301", site, None), 1)
        .await
        .expect("asset");
    let bearing = article(&db, "BRG-6205").await;
    let seal = article(&db, "SEAL-40").await;

    let added = spare_parts::add_asset_spare_part(
        &db,
        AddSparePartPayload {
            asset_id: asset.id,
            article_id: bearing,
            line: line(2.0, false),
        },
        1,
    )
    .await
    .expect("add bearing");
    assert_eq!(added.article_code, "BRG-6205");
    assert_eq!(added.position_label.as_deref(), Some("Palier côté accouplement"));
    spare_parts::add_asset_spare_part(
        &db,
        AddSparePartPayload {
            asset_id: asset.id,
            article_id: seal,
            line: line(1.0, true),
        },
        1,
    )
    .await
    .expect("add seal");

    let dup = spare_parts::add_asset_spare_part(
        &db,
        AddSparePartPayload {
            asset_id: asset.id,
            article_id: bearing,
            line: line(1.0, false),
        },
        1,
    )
    .await
    .expect_err("duplicate");
    assert_eq!(issue_code(&dup), "ASSET_SPARE_DUPLICATE");
    let zero = spare_parts::add_asset_spare_part(
        &db,
        AddSparePartPayload {
            asset_id: asset.id,
            article_id: seal,
            line: line(0.0, false),
        },
        1,
    )
    .await
    .expect_err("zero quantity");
    assert_eq!(issue_code(&zero), "ASSET_SPARE_QUANTITY_INVALID");

    let listed = spare_parts::list_asset_spare_parts(&db, asset.id).await.expect("list");
    assert_eq!(
        listed.iter().map(|l| l.article_code.as_str()).collect::<Vec<_>>(),
        ["SEAL-40", "BRG-6205"],
        "critical spares first"
    );

    let updated = spare_parts::update_asset_spare_part(&db, added.id, line(4.0, true), added.row_version)
        .await
        .expect("update");
    assert!((updated.quantity - 4.0).abs() < 1e-9);
    assert!(updated.is_critical_spare);

    spare_parts::remove_asset_spare_part(&db, added.id, updated.row_version)
        .await
        .expect("remove");
    let usage = spare_parts::list_article_asset_usage(&db, bearing)
        .await
        .expect("usage");
    assert!(usage.is_empty());
    assert_eq!(
        spare_parts::list_article_asset_usage(&db, seal)
            .await
            .expect("usage")
            .len(),
        1
    );
    let gone = spare_parts::remove_asset_spare_part(&db, added.id, updated.row_version + 1)
        .await
        .expect_err("already removed");
    assert!(matches!(gone, AppError::NotFound { .. }));
}

#[tokio::test]
async fn criticality_assessment_sets_the_class_and_keeps_history() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    let asset = identity::create_asset(&db, payload("PMP-401", site, None), 1)
        .await
        .expect("asset");
    assert_eq!(asset.criticality_code.as_deref(), Some("C"));

    let (assessment, updated) = criticality::assess_asset_criticality(
        &db,
        AssessCriticalityPayload {
            asset_id: asset.id,
            scores: CriticalityScores {
                safety: 2,
                environment: 1,
                production: 5,
                quality: 2,
                cost: 3,
                likelihood: 4,
            },
            rationale: Some("Seule pompe d'alimentation de la chaudière".into()),
            expected_row_version: asset.row_version,
        },
        1,
    )
    .await
    .expect("assess");
    assert_eq!(assessment.risk_score, 20);
    assert_eq!(assessment.resulting_class, "A");
    assert_eq!(assessment.previous_class.as_deref(), Some("C"));
    assert_eq!(updated.criticality_code.as_deref(), Some("A"));
    assert_eq!(updated.row_version, asset.row_version + 1);

    let stale = criticality::assess_asset_criticality(
        &db,
        AssessCriticalityPayload {
            asset_id: asset.id,
            scores: CriticalityScores {
                safety: 1,
                environment: 1,
                production: 1,
                quality: 1,
                cost: 1,
                likelihood: 1,
            },
            rationale: None,
            expected_row_version: asset.row_version,
        },
        1,
    )
    .await
    .expect_err("stale version");
    assert_eq!(issue_code(&stale), "ASSET_STALE_VERSION");

    let history = criticality::list_assessments(&db, asset.id).await.expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].method_code, "RISK_5X5_V1");
}

// ── Registry strip: reference-data tags and the modules' open rules ─────────────────────────

async fn set_status_and_criticality(db: &DatabaseConnection, code: &str, status: &str, criticality: &str) {
    exec(
        db,
        &format!(
            "UPDATE equipment SET \
               equipment_status_ref_id = (SELECT rv.id FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = 'EQUIPMENT.STATUS' AND rv.code = '{status}'), \
               lifecycle_status = '{status}', \
               equipment_criticality_ref_id = (SELECT rv.id FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = 'EQUIPMENT.CRITICALITY' AND rv.code = '{criticality}') \
             WHERE asset_id_code = '{code}'"
        ),
    )
    .await;
}

async fn request_on(db: &DatabaseConnection, code: &str, asset: &str, site: i64, status: &str, converted: bool) {
    let closed_at = if status == "closed" { "datetime('now')" } else { "NULL" };
    let converted_at = if converted { "datetime('now')" } else { "NULL" };
    exec(
        db,
        &format!(
            "INSERT INTO intervention_requests (code, equipment_id, org_node_id, status, title, description, \
             origin_type, impact_level, reported_urgency, submitted_at, submitter_id, closed_at, converted_at, \
             created_at, updated_at) VALUES ('{code}', (SELECT id FROM equipment WHERE asset_id_code = '{asset}'), \
             {site}, '{status}', '{code}', 'd', 'operator', 'unknown', 'normal', datetime('now'), 1, {closed_at}, \
             {converted_at}, datetime('now'), datetime('now'))"
        ),
    )
    .await;
}

async fn work_order_on(db: &DatabaseConnection, code: &str, asset: &str, site: i64, status: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO work_orders (code, type_id, status_id, equipment_id, entity_id, title, row_version, \
             created_at, updated_at) VALUES ('{code}', \
             (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
               JOIN reference_domains d ON d.id = s.domain_id \
               WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'), \
             (SELECT id FROM work_order_statuses WHERE code = '{status}'), \
             (SELECT id FROM equipment WHERE asset_id_code = '{asset}'), {site}, '{code}', 1, \
             datetime('now'), datetime('now'))"
        ),
    )
    .await;
}

fn metric(p: &crate::assets::registry_metrics::EquipmentRegistryMetricsPayload, key: &str) -> f64 {
    p.cells
        .iter()
        .find(|c| c.semantic_key == key)
        .unwrap_or_else(|| panic!("cell {key}"))
        .value
}

#[tokio::test]
async fn registry_strip_reads_reference_tags_and_module_open_rules() {
    use crate::assets::registry_metrics::get_equipment_registry_metrics;
    use crate::kpi_definitions::equipment as k;

    let db = setup().await;
    // Startup integrity hook: tags the system EQUIPMENT.STATUS rows (up / down / retired …).
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
        .await
        .expect("taxonomy integrity");
    let site = site_with_cost_center(&db).await;
    for code in [
        "EQ-UP",
        "EQ-MAINT",
        "EQ-CLOSED-DI",
        "EQ-DONE-OT",
        "EQ-OPEN-DI",
        "EQ-SCRAP",
    ] {
        identity::create_asset(&db, payload(code, site, None), 1)
            .await
            .expect("create");
    }
    // Under maintenance carries the `down` tag in EQUIPMENT.STATUS; A is the most critical level.
    set_status_and_criticality(&db, "EQ-MAINT", "UNDER_MAINTENANCE", "A").await;
    set_status_and_criticality(&db, "EQ-CLOSED-DI", "ACTIVE_IN_SERVICE", "A").await;
    set_status_and_criticality(&db, "EQ-DONE-OT", "ACTIVE_IN_SERVICE", "A").await;
    set_status_and_criticality(&db, "EQ-OPEN-DI", "ACTIVE_IN_SERVICE", "B").await;
    set_status_and_criticality(&db, "EQ-SCRAP", "SCRAPPED", "A").await;
    request_on(&db, "DI-CLOSED", "EQ-CLOSED-DI", site, "closed", false).await;
    request_on(&db, "DI-CONV", "EQ-CLOSED-DI", site, "submitted", true).await;
    request_on(&db, "DI-OPEN", "EQ-OPEN-DI", site, "submitted", false).await;
    work_order_on(&db, "OT-DONE", "EQ-DONE-OT", site, "completed").await;
    work_order_on(&db, "OT-CANCEL", "EQ-DONE-OT", site, "cancelled").await;

    let p = get_equipment_registry_metrics(&db, filters(100, 0))
        .await
        .expect("metrics");
    assert_eq!(p.scoped_total, 5, "the scrapped (retired) asset is out of scope");
    let status = p
        .cells
        .iter()
        .find(|c| c.semantic_key == k::EQUIP_STATUS_SUMMARY)
        .expect("status cell");
    assert_eq!((status.active_count, status.down_count), (Some(4), Some(1)));
    // Closed / converted requests and completed / cancelled work orders are not open work.
    assert_eq!(metric(&p, k::EQUIP_OPEN_WORK), 1.0, "EQ-OPEN-DI only");
    assert_eq!(
        metric(&p, k::EQUIP_CRITICAL_AT_RISK),
        1.0,
        "EQ-MAINT (down); EQ-OPEN-DI is not critical"
    );

    // The list opened from a tile holds exactly what the tile counts.
    for (focus, expected) in [("status_down", 1), ("open_work", 1), ("critical_at_risk", 1)] {
        let mut f = filters(100, 0);
        f.registry_focus = Some(focus.into());
        let page = search::search_assets_page(&db, f).await.expect("focus");
        assert_eq!(page.total, expected, "{focus}");
    }

    let mut with_retired = filters(100, 0);
    with_retired.include_decommissioned = Some(true);
    assert_eq!(
        search::search_assets_page(&db, with_retired).await.expect("all").total,
        6
    );
}

#[tokio::test]
async fn life_summary_availability_is_the_latest_reliability_snapshot_not_the_health_score() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    let asset = identity::create_asset(&db, payload("PMP-AV1", site, None), 1)
        .await
        .expect("create");
    let none = crate::assets::history::get_asset_history_summary(&db, asset.id)
        .await
        .expect("summary");
    assert_eq!(
        (none.availability_percent, none.availability_period_end),
        (None, None),
        "no reliability analysis: no availability (never the health score)"
    );
    for (sid, end, availability) in [
        ("av-1", "2026-08-31T00:00:00Z", 0.91),
        ("av-2", "2026-09-30T00:00:00Z", 0.97),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO reliability_kpi_snapshots (entity_sync_id, equipment_id, period_start, period_end, \
                 availability, event_count, data_quality_score) \
                 VALUES ('{sid}', {id}, '2026-01-01T00:00:00Z', '{end}', {availability}, 1, 1.0)",
                id = asset.id
            ),
        )
        .await;
    }
    let s = crate::assets::history::get_asset_history_summary(&db, asset.id)
        .await
        .expect("summary");
    assert!((s.availability_percent.expect("availability") - 97.0).abs() < 1e-9);
    assert_eq!(s.availability_period_end.as_deref(), Some("2026-09-30T00:00:00Z"));

    // The status list carries its availability tags for the screens.
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
        .await
        .expect("taxonomy integrity");
    let catalog = crate::assets::taxonomy_reference::list_equipment_taxonomy_catalog(&db)
        .await
        .expect("catalog");
    let tag = |code: &str| {
        catalog
            .statuses
            .iter()
            .find(|o| o.code == code)
            .and_then(|o| o.semantic_tag.clone())
    };
    assert_eq!(tag("UNDER_MAINTENANCE").as_deref(), Some("down"));
    assert_eq!(tag("SCRAPPED").as_deref(), Some("retired"));
}

/// The maintenance profile's open count is "not yet closed" (`wo::backlog::NOT_CLOSED_SQL`), the
/// same rule and figure as the asset binding card.
#[tokio::test]
async fn maintenance_profile_open_count_is_the_not_yet_closed_rule() {
    let db = setup().await;
    let site = site_with_cost_center(&db).await;
    let asset = identity::create_asset(&db, payload("PMP-301", site, None), 1)
        .await
        .expect("asset");
    let at = chrono::Utc::now().to_rfc3339();
    work_order(&db, "NC-HOLD", asset.id, "corrective", "on_hold", 10.0, &at).await;
    work_order(&db, "NC-DONE", asset.id, "corrective", "completed", 10.0, &at).await;
    work_order(&db, "NC-CLOSED", asset.id, "corrective", "closed", 10.0, &at).await;
    work_order(&db, "NC-CANC", asset.id, "corrective", "cancelled", 10.0, &at).await;

    let profile = maintenance_profile::get_asset_maintenance_profile(&db, asset.id, false, chrono::Utc::now())
        .await
        .expect("profile");
    assert_eq!(profile.open_work_order_count, 2, "on hold + completed");
    assert_eq!(profile.open_work_order_rule, crate::wo::backlog::RULE_NOT_YET_CLOSED);
    assert_eq!(
        crate::assets::bindings::count_open_wo(&db, asset.id)
            .await
            .expect("binding"),
        profile.open_work_order_count
    );
}
