//! Units of measure under reference-data governance (`reference::units`, migration 201): the
//! relink of the fixed lookup and of the free texts, the protected standard units, plant units
//! with their base and factor through draft → publish, and the conversion rules.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::prelude::MigratorTrait;

use crate::errors::AppError;
use crate::inventory::unit_conversions::{self, CreateUnitConversionInput};
use crate::migrations::Migrator;
use crate::reference::units::{self, UNITS_DOMAIN};
use crate::reference::{publish, sets, validation, values};

const M201: &str = "m20261015_000201_governed_units_of_measure";

// ── helpers ────────────────────────────────────────────────────────────────────────────────

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn opt_i64(db: &DatabaseConnection, sql: &str) -> Option<i64> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<i64>>(0).expect("i64 column"))
}

async fn i64_of(db: &DatabaseConnection, sql: &str) -> i64 {
    opt_i64(db, sql).await.unwrap_or_else(|| panic!("{sql}: no value"))
}

async fn opt_str(db: &DatabaseConnection, sql: &str) -> Option<String> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("text column"))
}

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> bool {
    opt_i64(
        db,
        &format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}'"),
    )
    .await
    .is_some()
}

async fn fk_target(db: &DatabaseConnection, table: &str, column: &str) -> Option<String> {
    opt_str(
        db,
        &format!("SELECT \"table\" FROM pragma_foreign_key_list('{table}') WHERE \"from\" = '{column}'"),
    )
    .await
}

async fn unit(db: &DatabaseConnection, code: &str) -> i64 {
    units::unit_id_by_code(db, code).await.expect(code)
}

async fn unit_count(db: &DatabaseConnection) -> i64 {
    i64_of(
        db,
        "SELECT COUNT(*) FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
         JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS'",
    )
    .await
}

async fn migrated_before(name: &str) -> DatabaseConnection {
    let db = crate::db::test_db::empty_memory_db().await;
    let pos = Migrator::migrations()
        .iter()
        .position(|m| m.name() == name)
        .unwrap_or_else(|| panic!("migration {name} is not registered"));
    Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
        .await
        .expect("migrations before the one under test");
    db
}

/// No row points at a missing reference value after the migration.
async fn assert_reference_links_hold(db: &DatabaseConnection) {
    let broken: Vec<String> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_key_check".to_string(),
        ))
        .await
        .expect("foreign_key_check")
        .iter()
        .filter_map(|r| {
            let table: String = r.try_get("", "table").ok()?;
            let parent: String = r.try_get("", "parent").ok()?;
            parent.starts_with("reference_").then(|| format!("{table} -> {parent}"))
        })
        .collect();
    assert!(broken.is_empty(), "rows break a reference foreign key: {broken:?}");
}

fn gate_of<T: std::fmt::Debug>(result: Result<T, AppError>) -> String {
    match result {
        Err(AppError::ValidationFailed(m)) => m.join(" "),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

const NOW: &str = "'2026-10-01T08:00:00Z'";

/// The fixed lookup as start-up created it, plus a code Maintafox did not know (`SAC`) and a
/// second spelling of KG (`kg`). Returns the lookup id of `code`.
async fn legacy_lookup(db: &DatabaseConnection) {
    exec(
        db,
        &format!(
            "INSERT INTO lookup_domains (entity_sync_id, domain_key, display_name, domain_type, is_ordered, \
             is_extensible, is_locked, schema_version, created_at, updated_at) \
             VALUES ('sync-ld-uom', 'inventory.unit_of_measure', 'Unité de mesure stock', 'tenant', 0, 1, 0, 1, {NOW}, {NOW})"
        ),
    )
    .await;
    for (i, (code, label, en)) in [
        ("U", "Unité", "Unit"),
        ("KG", "kg", "kg"),
        ("BOX", "Boîte", "Box"),
        ("SAC", "Sac", "Bag"),
        ("kg", "kilo", "kilo"),
    ]
    .iter()
    .enumerate()
    {
        exec(
            db,
            &format!(
                "INSERT INTO lookup_values (entity_sync_id, domain_id, code, label, fr_label, en_label, sort_order, \
                 is_active, is_system, created_at, updated_at, row_version) \
                 SELECT 'sync-lv-{i}', id, '{code}', '{label}', '{label}', '{en}', {i}, 1, 1, {NOW}, {NOW}, 1 \
                   FROM lookup_domains WHERE domain_key = 'inventory.unit_of_measure'"
            ),
        )
        .await;
    }
}

async fn lookup_id(db: &DatabaseConnection, code: &str) -> i64 {
    i64_of(
        db,
        &format!(
            "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'inventory.unit_of_measure' AND lv.code = '{code}'"
        ),
    )
    .await
}

async fn legacy_article(db: &DatabaseConnection, code: &str, unit_code: &str) {
    let unit = lookup_id(db, unit_code).await;
    exec(
        db,
        &format!(
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) \
             VALUES ('sync-art-{code}', '{code}', 'Article {code}', {unit})"
        ),
    )
    .await;
}

async fn article_unit(db: &DatabaseConnection, code: &str) -> i64 {
    i64_of(
        db,
        &format!("SELECT unit_value_id FROM articles WHERE article_code = '{code}'"),
    )
    .await
}

async fn legacy_meter(db: &DatabaseConnection, code: &str, unit_text: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit, current_reading, \
             is_primary, is_active, created_at, updated_at) \
             VALUES ('sync-meter-{code}', 1, '{code}', 'HOURS', '{unit_text}', 0, 0, 1, {NOW}, {NOW})"
        ),
    )
    .await;
}

async fn meter_unit(db: &DatabaseConnection, code: &str) -> Option<i64> {
    opt_i64(
        db,
        &format!("SELECT unit_value_id FROM equipment_meters WHERE name = '{code}'"),
    )
    .await
}

async fn legacy_checkpoint(db: &DatabaseConnection, seq: i64, code: &str, unit_text: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO inspection_checkpoints (entity_sync_id, template_version_id, sequence_order, checkpoint_code, \
             check_type, measurement_unit, requires_photo, requires_comment_on_exception, row_version) \
             VALUES ('sync-cp-{code}', 1, {seq}, '{code}', 'numeric', '{unit_text}', 0, 0, 1)"
        ),
    )
    .await;
}

/// The legacy data of every kind migration 201 relinks.
async fn legacy_plant(db: &DatabaseConnection) {
    exec(db, "PRAGMA foreign_keys = OFF").await;
    legacy_lookup(db).await;
    legacy_article(db, "ART-U", "U").await;
    legacy_article(db, "ART-KG", "KG").await;
    legacy_article(db, "ART-SAC", "SAC").await;
    legacy_article(db, "ART-KG2", "kg").await;
    let (bx, u, kg) = (
        lookup_id(db, "BOX").await,
        lookup_id(db, "U").await,
        lookup_id(db, "KG").await,
    );
    exec(
        db,
        &format!(
            "INSERT INTO unit_conversions (entity_sync_id, from_unit_value_id, to_unit_value_id, factor, is_active, \
             row_version, created_at, updated_at) VALUES \
             ('sync-uc-box', {bx}, {u}, 12, 1, 1, {NOW}, {NOW}), \
             ('sync-uc-cross', {kg}, {u}, 3, 1, 1, {NOW}, {NOW})"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO inventory_supplier_prices (supplier_id, article_id, unit_price, price_unit_value_id, \
             valid_from, is_active, created_at, updated_at) \
             SELECT 1, id, 9.5, {kg}, {NOW}, 1, {NOW}, {NOW} FROM articles WHERE article_code = 'ART-KG'"
        ),
    )
    .await;
    legacy_meter(db, "MTR-HEURES", "Heures").await;
    legacy_meter(db, "MTR-H", "h").await;
    legacy_meter(db, "MTR-KM", " KM ").await;
    legacy_meter(db, "MTR-TR", "tr").await;
    legacy_meter(db, "MTR-EMPTY", "").await;
    legacy_checkpoint(db, 1, "CP-TEMP", "deg C").await;
    legacy_checkpoint(db, 2, "CP-PRESS", "bar ").await;
    exec(
        db,
        &format!(
            "INSERT INTO inspection_template_versions (entity_sync_id, template_id, version_no, effective_from, \
             checkpoint_package_json, requires_review, row_version) VALUES ('sync-itv', 1, 1, {NOW}, \
             '[{{\"checkpoint_code\":\"CP-TEMP\",\"measurement_unit\":\"deg C\"}},{{\"checkpoint_code\":\"CP-OK\",\"measurement_unit\":null}}]', 0, 1)"
        ),
    )
    .await;
    exec(db, "PRAGMA foreign_keys = ON").await;
}

// ── Migration 201 ─────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn migration_201_relinks_lookup_units_free_texts_and_unknown_codes() {
    let db = migrated_before(M201).await;
    legacy_plant(&db).await;
    Migrator::up(&db, Some(1)).await.expect("migration 201");

    // Articles: known codes by code, the unknown SAC and the second spelling kept as units to review.
    assert_eq!(article_unit(&db, "ART-U").await, unit(&db, "U").await);
    assert_eq!(article_unit(&db, "ART-KG").await, unit(&db, "KG").await);
    let sac = units::unit_by_id(&db, article_unit(&db, "ART-SAC").await)
        .await
        .expect("read")
        .expect("SAC unit");
    assert_eq!((sac.code.as_str(), sac.label.as_str()), ("SAC", "Sac"));
    assert_eq!(sac.label_en.as_deref(), Some("Bag"));
    assert!(sac.review_required && !sac.is_standard && sac.base_factor.is_none());
    let kg2 = units::unit_by_id(&db, article_unit(&db, "ART-KG2").await)
        .await
        .expect("read")
        .expect("kg unit");
    assert_eq!(kg2.code, "KG_2", "two lookup codes that differ by case keep two units");
    assert!(kg2.review_required);

    // Conversions, supplier price: remapped, foreign keys now on reference_values.
    assert_eq!(
        opt_i64(
            &db,
            "SELECT from_unit_value_id FROM unit_conversions WHERE entity_sync_id = 'sync-uc-box'"
        )
        .await,
        Some(unit(&db, "BOX").await)
    );
    assert_eq!(
        opt_i64(&db, "SELECT price_unit_value_id FROM inventory_supplier_prices").await,
        Some(unit(&db, "KG").await)
    );
    for (table, column) in units::UNIT_LINK_COLUMNS {
        assert_eq!(
            fk_target(&db, table, column).await.as_deref(),
            Some("reference_values"),
            "{table}.{column}"
        );
    }
    assert_eq!(
        fk_target(&db, "articles", "criticality_value_id").await.as_deref(),
        Some("lookup_values"),
        "only the unit column was retargeted"
    );
    // The kg → unit conversion measures two different things: kept, with a note to review it.
    assert!(opt_str(
        &db,
        "SELECT notes FROM unit_conversions WHERE entity_sync_id = 'sync-uc-cross'"
    )
    .await
    .unwrap_or_default()
    .contains("[Migration 201]"));

    // Free texts: synonyms and symbols map; an ambiguous text becomes a unit to review.
    assert_eq!(meter_unit(&db, "MTR-HEURES").await, Some(unit(&db, "H").await));
    assert_eq!(meter_unit(&db, "MTR-H").await, Some(unit(&db, "H").await));
    assert_eq!(meter_unit(&db, "MTR-KM").await, Some(unit(&db, "KM").await));
    assert_eq!(meter_unit(&db, "MTR-EMPTY").await, None);
    let tr = units::unit_by_id(&db, meter_unit(&db, "MTR-TR").await.expect("tr linked"))
        .await
        .expect("read")
        .expect("tr unit");
    assert_eq!((tr.code.as_str(), tr.symbol.as_str()), ("TR", "tr"));
    assert!(tr.review_required);
    assert_eq!(
        opt_i64(
            &db,
            "SELECT measurement_unit_id FROM inspection_checkpoints WHERE checkpoint_code = 'CP-TEMP'"
        )
        .await,
        Some(unit(&db, "DEGC").await)
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT measurement_unit_id FROM inspection_checkpoints WHERE checkpoint_code = 'CP-PRESS'"
        )
        .await,
        Some(unit(&db, "BAR").await)
    );
    assert!(!column_exists(&db, "equipment_meters", "unit").await);
    assert!(!column_exists(&db, "inspection_checkpoints", "measurement_unit").await);
    // The original spelling stays as a legacy alias of the unit it was mapped onto.
    assert_eq!(
        opt_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM reference_aliases WHERE reference_value_id = {} AND alias_label = 'Heures' \
                 AND alias_type = 'legacy'",
                unit(&db, "H").await
            )
        )
        .await,
        Some(1)
    );
    // The checkpoint package links the unit too.
    let package = opt_str(&db, "SELECT checkpoint_package_json FROM inspection_template_versions")
        .await
        .expect("package");
    let package: serde_json::Value = serde_json::from_str(&package).expect("json");
    assert_eq!(
        package[0]["measurement_unit_id"],
        serde_json::json!(unit(&db, "DEGC").await)
    );
    assert!(package[0].get("measurement_unit").is_none());
    assert!(package[1]["measurement_unit_id"].is_null());

    // The lookup is gone; the units to review are listed for the report.
    assert!(opt_i64(
        &db,
        "SELECT id FROM lookup_domains WHERE domain_key = 'inventory.unit_of_measure'"
    )
    .await
    .is_none());
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM lookup_values WHERE code IN ('U', 'KG', 'BOX', 'SAC', 'kg')"
        )
        .await,
        Some(0)
    );
    let to_review: Vec<String> = units::list_units_to_review(&db)
        .await
        .expect("list")
        .into_iter()
        .map(|u| u.code)
        .collect();
    assert_eq!(to_review, vec!["KG_2", "SAC", "TR"]);
    assert_reference_links_hold(&db).await;
}

#[tokio::test]
async fn migration_201_can_run_again_without_creating_or_moving_anything() {
    let db = migrated_before(M201).await;
    legacy_plant(&db).await;
    Migrator::up(&db, Some(1)).await.expect("migration 201");
    let units_before = unit_count(&db).await;
    let aliases_before = i64_of(&db, "SELECT COUNT(*) FROM reference_aliases").await;
    let links_before = (
        article_unit(&db, "ART-SAC").await,
        meter_unit(&db, "MTR-TR").await,
        opt_str(
            &db,
            "SELECT notes FROM unit_conversions WHERE entity_sync_id = 'sync-uc-cross'",
        )
        .await,
    );

    exec(&db, &format!("DELETE FROM seaql_migrations WHERE version = '{M201}'")).await;
    Migrator::up(&db, Some(1)).await.expect("migration 201 again");
    assert_eq!(unit_count(&db).await, units_before);
    assert_eq!(
        i64_of(&db, "SELECT COUNT(*) FROM reference_aliases").await,
        aliases_before
    );
    assert_eq!(
        (
            article_unit(&db, "ART-SAC").await,
            meter_unit(&db, "MTR-TR").await,
            opt_str(
                &db,
                "SELECT notes FROM unit_conversions WHERE entity_sync_id = 'sync-uc-cross'"
            )
            .await,
        ),
        links_before
    );
    assert_reference_links_hold(&db).await;
}

#[tokio::test]
async fn a_fresh_database_has_the_standard_units_and_no_unit_lookup() {
    let db = crate::db::test_db::seeded().await;
    assert!(opt_i64(
        &db,
        "SELECT id FROM lookup_domains WHERE domain_key = 'inventory.unit_of_measure'"
    )
    .await
    .is_none());
    let units_domain = crate::reference::domains::get_reference_domain_by_code(&db, UNITS_DOMAIN)
        .await
        .expect("units domain");
    assert_eq!(
        units_domain.audience,
        crate::reference::audience::DomainAudience::Company
    );
    assert_eq!(units_domain.governance_category, "controlled_catalog");
    let dims = crate::reference::domains::get_reference_domain_by_code(&db, units::DIMENSIONS_DOMAIN)
        .await
        .expect("dimensions");
    assert_eq!(dims.audience, crate::reference::audience::DomainAudience::System);
    let kg = units::unit_by_id(&db, unit(&db, "KG").await)
        .await
        .expect("read")
        .expect("kg");
    assert_eq!(
        (
            kg.symbol.as_str(),
            kg.external_code.as_deref(),
            kg.dimension,
            kg.is_standard
        ),
        ("kg", Some("KGM"), Some(units::UnitDimension::Mass), true)
    );
    // The integrity pass and the startup retirement pass recreate nothing.
    let before = unit_count(&db).await;
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("integrity");
    assert_eq!(unit_count(&db).await, before);
    assert!(crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("pass")
        .is_empty());
}

#[tokio::test]
async fn startup_retirement_pass_relinks_a_unit_lookup_brought_back_by_sync() {
    let db = crate::db::test_db::seeded().await;
    crate::sync::capture::install_capture(&db).await.expect("capture on");
    // A replayed history recreates the lookup and an article pointing at it.
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    legacy_lookup(&db).await;
    legacy_article(&db, "ART-GHOST", "BOX").await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    exec(&db, "DELETE FROM sync_capture_journal").await;

    let retired = crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("pass");
    assert_eq!(retired, vec![units::LEGACY_UNIT_LOOKUP_DOMAIN.to_string()]);
    assert_eq!(article_unit(&db, "ART-GHOST").await, unit(&db, "BOX").await);
    for (entity, op) in [
        ("articles", "upsert"),
        ("lookup_values", "delete"),
        ("lookup_domains", "delete"),
    ] {
        assert!(
            opt_i64(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = '{entity}' AND operation = '{op}'"
                )
            )
            .await
            .unwrap_or(0)
                > 0,
            "{entity} {op} captured"
        );
    }
    assert!(crate::reference::retirement::retire_superseded_domains(&db)
        .await
        .expect("pass")
        .is_empty());
}

// ── Governance: protected standard units, plant units through draft → publish ──────────────

async fn admin(db: &DatabaseConnection) -> i64 {
    i64_of(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

async fn units_draft(db: &DatabaseConnection, actor: i64) -> sets::ReferenceSet {
    let domain = crate::reference::domains::get_reference_domain_by_code(db, UNITS_DOMAIN)
        .await
        .expect("domain");
    sets::create_draft_set(db, domain.id, actor).await.expect("draft")
}

async fn in_set(db: &DatabaseConnection, set_id: i64, code: &str) -> i64 {
    i64_of(
        db,
        &format!("SELECT id FROM reference_values WHERE set_id = {set_id} AND code = '{code}'"),
    )
    .await
}

fn plant_unit(set_id: i64, code: &str, base: Option<i64>, factor: Option<f64>) -> values::CreateReferenceValuePayload {
    let mut meta = serde_json::json!({ "label_en": code, "symbol": code.to_lowercase() });
    if let Some(f) = factor {
        meta["base_factor"] = f.into();
    }
    values::CreateReferenceValuePayload {
        set_id,
        parent_id: base,
        code: code.into(),
        label: code.into(),
        description: None,
        sort_order: None,
        color_hex: None,
        icon_name: None,
        semantic_tag: None,
        external_code: None,
        metadata_json: Some(meta.to_string()),
    }
}

async fn publish_draft(db: &DatabaseConnection, set_id: i64, actor: i64) {
    sets::validate_set(db, set_id, actor).await.expect("validate draft");
    publish::preview_publish_impact(db, set_id)
        .await
        .expect("impact preview");
    publish::publish_reference_set(db, set_id, actor)
        .await
        .expect("publish draft");
}

#[tokio::test]
async fn standard_units_are_protected_and_plant_units_need_a_base_and_a_factor() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin(&db).await;
    let draft = units_draft(&db, actor).await;
    let kg = in_set(&db, draft.id, "KG").await;
    let metre = in_set(&db, draft.id, "M").await;

    // Standard units: no edit, no deactivation, no move.
    assert!(values::deactivate_value(&db, kg, actor).await.is_err());
    assert!(values::update_value(
        &db,
        kg,
        values::UpdateReferenceValuePayload {
            label: Some("Kilo".into()),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        },
        actor
    )
    .await
    .is_err());

    // Plant units: base and factor required and checked.
    assert!(
        gate_of(values::create_value(&db, plant_unit(draft.id, "SAC", None, Some(25.0)), actor).await)
            .contains(units::GATE_UNIT_BASE_REQUIRED)
    );
    assert!(
        gate_of(values::create_value(&db, plant_unit(draft.id, "SAC", Some(kg), None), actor).await)
            .contains(units::GATE_UNIT_BASE_REQUIRED)
    );
    assert!(
        gate_of(values::create_value(&db, plant_unit(draft.id, "SAC", Some(kg), Some(0.0)), actor).await)
            .contains(units::GATE_UNIT_BASE_INVALID)
    );
    let mut wrong_dimension = plant_unit(draft.id, "SAC", Some(kg), Some(25.0));
    wrong_dimension.semantic_tag = Some("length".into());
    assert!(gate_of(values::create_value(&db, wrong_dimension, actor).await).contains(units::GATE_UNIT_BASE_INVALID));
    let sac = values::create_value(&db, plant_unit(draft.id, "SAC", Some(kg), Some(25.0)), actor)
        .await
        .expect("1 bag = 25 kg");
    assert_eq!(
        sac.semantic_tag.as_deref(),
        Some("mass"),
        "the dimension is the base unit's"
    );
    // A plant unit is not a base.
    assert!(
        gate_of(values::create_value(&db, plant_unit(draft.id, "PALETTE", Some(sac.id), Some(40.0)), actor).await)
            .contains(units::GATE_UNIT_BASE_INVALID)
    );
    // Moving a plant unit under a unit of another dimension is refused.
    assert!(
        gate_of(values::move_value_parent(&db, sac.id, Some(metre), actor).await)
            .contains(units::GATE_UNIT_BASE_INVALID)
    );

    publish_draft(&db, draft.id, actor).await;
    let sac = unit(&db, "SAC").await;
    let published = units::unit_by_id(&db, sac).await.expect("read").expect("SAC");
    assert_eq!(
        (published.base_unit_id, published.base_factor),
        (Some(unit(&db, "KG").await), Some(25.0))
    );
}

#[tokio::test]
async fn conversions_use_the_article_row_then_the_unit_factor_and_never_assume_one() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin(&db).await;
    let draft = units_draft(&db, actor).await;
    let kg = in_set(&db, draft.id, "KG").await;
    values::create_value(&db, plant_unit(draft.id, "SAC", Some(kg), Some(25.0)), actor)
        .await
        .expect("bag");
    // A unit to review (as migration 201 creates them), without base or factor.
    exec(
        &db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active, metadata_json) \
             VALUES ('sync-review-fut', {}, 'FUT', 'Fût', 90, 1, '{{\"origin\":\"migrated\",\"symbol\":\"fût\",\"review_required\":true}}')",
            draft.id
        ),
    )
    .await;
    // Draft validation: the unit to review is a warning, not a block.
    let report = validation::validate_reference_set(&db, draft.id, actor)
        .await
        .expect("validation");
    assert!(report
        .issues
        .iter()
        .any(|i| i.check == "unit_to_review" && i.severity == validation::IssueSeverity::Warning));
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.severity == validation::IssueSeverity::Blocking),
        "{:?}",
        report.issues
    );
    publish_draft(&db, draft.id, actor).await;

    let (sac, kg, g, fut, u) = (
        unit(&db, "SAC").await,
        unit(&db, "KG").await,
        unit(&db, "G").await,
        unit(&db, "FUT").await,
        unit(&db, "U").await,
    );
    exec(
        &db,
        &format!(
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) \
             VALUES ('sync-art-cement', 'CIM-25', 'Ciment', {kg})"
        ),
    )
    .await;
    let cement = i64_of(&db, "SELECT id FROM articles WHERE article_code = 'CIM-25'").await;
    let input = |from: i64, to: i64, article_id: Option<i64>, factor: f64| CreateUnitConversionInput {
        from_unit_value_id: from,
        to_unit_value_id: to,
        article_id,
        factor,
        notes: None,
    };

    // The general factor of the plant unit: 2 bags = 50 kg; through kg → g for grams.
    let general = unit_conversions::convert_quantity(&db, 2.0, sac, kg, None)
        .await
        .expect("unit factor");
    assert_eq!((general.quantity, general.path.as_str()), (50.0, "unit_factor"));
    unit_conversions::create_unit_conversion(&db, actor, input(kg, g, None, 1000.0))
        .await
        .expect("kg -> g");
    let grams = unit_conversions::convert_quantity(&db, 1.0, sac, g, None)
        .await
        .expect("via kg");
    assert_eq!(grams.quantity, 25_000.0);
    // The general factor of a plant unit lives on the unit, not in a general row.
    assert!(
        gate_of(unit_conversions::create_unit_conversion(&db, actor, input(sac, kg, None, 20.0)).await)
            .contains(units::GATE_UNIT_GENERAL_FACTOR_ON_UNIT)
    );
    // The article's own conversion wins.
    unit_conversions::create_unit_conversion(&db, actor, input(sac, kg, Some(cement), 20.0))
        .await
        .expect("this supplier's bags hold 20 kg");
    let own = unit_conversions::convert_quantity(&db, 2.0, sac, kg, Some(cement))
        .await
        .expect("article");
    assert_eq!((own.quantity, own.path.as_str()), (40.0, "direct"));

    // A unit to review is never converted with an assumed factor of 1.
    assert!(
        gate_of(unit_conversions::convert_quantity(&db, 1.0, fut, u, None).await)
            .contains(units::GATE_UNIT_FACTOR_MISSING)
    );
    // Different dimensions never convert.
    assert!(
        gate_of(unit_conversions::convert_quantity(&db, 1.0, sac, u, None).await)
            .contains(units::GATE_UNIT_DIMENSION_MISMATCH)
    );
}

#[tokio::test]
async fn editing_a_unit_to_review_requires_its_base_and_factor_and_clears_the_flag() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin(&db).await;
    let draft = units_draft(&db, actor).await;
    exec(
        &db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active, metadata_json) \
             VALUES ('sync-review-tr', {}, 'TR', 'tr', 90, 1, '{{\"origin\":\"migrated\",\"symbol\":\"tr\",\"review_required\":true}}')",
            draft.id
        ),
    )
    .await;
    let tr = in_set(&db, draft.id, "TR").await;
    let relabel = |metadata: Option<String>| values::UpdateReferenceValuePayload {
        label: Some("Tour".into()),
        description: None,
        sort_order: None,
        color_hex: None,
        icon_name: None,
        semantic_tag: None,
        external_code: None,
        metadata_json: metadata.map(Some),
    };
    assert!(gate_of(values::update_value(&db, tr, relabel(None), actor).await).contains(units::GATE_UNIT_BASE_REQUIRED));
    let rpm = in_set(&db, draft.id, "RPM").await;
    values::move_value_parent(&db, tr, Some(rpm), actor)
        .await
        .expect("base rpm");
    let updated = values::update_value(
        &db,
        tr,
        relabel(Some(
            serde_json::json!({ "origin": "migrated", "symbol": "tr", "base_factor": 1.0, "review_required": true })
                .to_string(),
        )),
        actor,
    )
    .await
    .expect("reviewed");
    assert_eq!(updated.semantic_tag.as_deref(), Some("frequency"));
    let meta: serde_json::Value = serde_json::from_str(updated.metadata_json.as_deref().expect("meta")).expect("json");
    assert!(
        meta.get("review_required").is_none(),
        "the review flag is cleared: {meta}"
    );
}

// ── Review follow-ups: sync of older devices, merge, publish, import, outbox ───────────────

/// A unit straight in the published list (as a seeder, the migration or a published draft leave
/// it), returning its id.
async fn published_unit(
    db: &DatabaseConnection,
    code: &str,
    meta: serde_json::Value,
    base: Option<i64>,
    tag: Option<&str>,
) -> i64 {
    let base = base.map(|b| b.to_string()).unwrap_or_else(|| "NULL".into());
    let tag = tag.map(|t| format!("'{t}'")).unwrap_or_else(|| "NULL".into());
    exec(
        db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, parent_id, code, label, sort_order, semantic_tag, \
             is_active, metadata_json) \
             SELECT 'sync-unit-{code}', s.id, {base}, '{code}', '{code}', 500, {tag}, 1, '{meta}' \
               FROM reference_sets s JOIN reference_domains d ON d.id = s.domain_id \
              WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published'"
        ),
    )
    .await;
    unit(db, code).await
}

async fn plain_article(db: &DatabaseConnection, code: &str, unit_id: i64) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) \
             VALUES ('sync-art-{code}', '{code}', 'Article {code}', {unit_id})"
        ),
    )
    .await;
    i64_of(db, &format!("SELECT id FROM articles WHERE article_code = '{code}'")).await
}

#[tokio::test]
async fn rows_of_an_older_device_link_their_lookup_unit_and_unit_texts() {
    use crate::sync::row_codec::{self, RowLocator};
    let db = crate::db::test_db::seeded().await;
    let kg = unit(&db, "KG").await;
    plain_article(&db, "ART-LOCAL", kg).await;
    let layout = crate::sync::row_layout::layout(&db, "articles").await.expect("layout");
    let mut payload = row_codec::load_row_payload(&db, &layout, &RowLocator::Identity("sync-art-ART-LOCAL".into()))
        .await
        .expect("payload")
        .expect("row")
        .payload;
    // The older device sends the unit as the sync id of the retired lookup value "KG".
    payload.insert("unit_value_id".into(), units::legacy_lookup_sync_id("KG").into());
    payload.insert("article_code".into(), "ART-OLD".into());
    let applied = row_codec::apply_row(
        &db,
        &layout,
        "sync-art-old-device",
        "upsert",
        &serde_json::Value::Object(payload).to_string(),
    )
    .await
    .expect("apply");
    assert!(
        matches!(applied, crate::sync::materialize::ApplyOutcome::Applied),
        "{applied:?}"
    );
    assert_eq!(article_unit(&db, "ART-OLD").await, kg);

    // A meter of an older device names its unit by text: mapped with the synonym table, sent on
    // as the unit's sync id; a link the payload already carries wins over a text.
    let h_sync = opt_str(
        &db,
        &format!(
            "SELECT entity_sync_id FROM reference_values WHERE id = {}",
            unit(&db, "H").await
        ),
    )
    .await
    .expect("sync id");
    let mut meter = serde_json::Map::new();
    meter.insert("unit".into(), " Heures ".into());
    units::upgrade_legacy_unit_payload(&db, "equipment_meters", &mut meter)
        .await
        .expect("upgrade meter");
    assert!(meter.get("unit").is_none());
    assert_eq!(meter["unit_value_id"], serde_json::json!(h_sync));
    let mut linked = serde_json::Map::new();
    linked.insert("unit".into(), "km".into());
    linked.insert("unit_value_id".into(), h_sync.clone().into());
    units::upgrade_legacy_unit_payload(&db, "equipment_meters", &mut linked)
        .await
        .expect("upgrade linked meter");
    assert_eq!(linked["unit_value_id"], serde_json::json!(h_sync));

    // A checkpoint package of an older device: the text becomes the unit's sync id.
    let mut package = serde_json::Map::new();
    package.insert(
        "checkpoint_package_json".into(),
        r#"[{"checkpoint_code":"T","measurement_unit":"°C"},{"checkpoint_code":"X","measurement_unit":"tr"}]"#.into(),
    );
    units::upgrade_legacy_unit_payload(&db, "inspection_template_versions", &mut package)
        .await
        .expect("upgrade");
    let upgraded: serde_json::Value =
        serde_json::from_str(package["checkpoint_package_json"].as_str().expect("text")).expect("json");
    let degc_sync = opt_str(
        &db,
        &format!(
            "SELECT entity_sync_id FROM reference_values WHERE id = {}",
            unit(&db, "DEGC").await
        ),
    )
    .await
    .expect("sync id");
    assert_eq!(upgraded[0]["measurement_unit_id"], serde_json::json!(degc_sync));
    assert!(upgraded[0].get("measurement_unit").is_none());
    // "tr" is ambiguous: a unit to review was created, never an assumed one.
    assert!(units::list_units_to_review(&db)
        .await
        .expect("list")
        .iter()
        .any(|u| u.code == "TR"));
}

#[tokio::test]
async fn merging_units_moves_every_record_and_keeps_quantities() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin(&db).await;
    let domain = crate::reference::domains::get_reference_domain_by_code(&db, UNITS_DOMAIN)
        .await
        .expect("domain");
    let h = unit(&db, "H").await;
    let kg = unit(&db, "KG").await;
    let heures = published_unit(
        &db,
        "HEURES",
        serde_json::json!({ "origin": "migrated", "symbol": "heures", "review_required": true }),
        None,
        None,
    )
    .await;
    let sac = published_unit(
        &db,
        "SAC",
        serde_json::json!({ "symbol": "sac", "base_factor": 25.0 }),
        Some(kg),
        Some("mass"),
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    exec(
        &db,
        &format!(
            "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit_value_id, current_reading, \
             is_primary, is_active, created_at, updated_at) VALUES ('sync-meter-m', 1, 'MTR-M', 'HOURS', {heures}, 0, 0, 1, {NOW}, {NOW})"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO inspection_template_versions (entity_sync_id, template_id, version_no, effective_from, \
             checkpoint_package_json, requires_review, row_version) VALUES ('sync-itv-m', 1, 1, {NOW}, \
             '[{{\"checkpoint_code\":\"RUN\",\"measurement_unit_id\":{heures}}}]', 0, 1)"
        ),
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;

    // A protected standard unit is never merged away.
    assert!(
        crate::reference::migrations::merge_reference_values(&db, domain.id, kg, h, actor)
            .await
            .is_err()
    );
    // A bag of 25 kg is not a kilogram.
    assert!(
        gate_of(crate::reference::migrations::merge_reference_values(&db, domain.id, sac, kg, actor).await)
            .contains(units::GATE_UNIT_MERGE_CHANGES_QUANTITIES)
    );
    // A unit to review merged into h: meter and package follow, the unit is retired.
    let merged = crate::reference::migrations::merge_reference_values(&db, domain.id, heures, h, actor)
        .await
        .expect("merge");
    assert!(merged.source_deactivated);
    assert_eq!(meter_unit(&db, "MTR-M").await, Some(h));
    let package = opt_str(
        &db,
        "SELECT checkpoint_package_json FROM inspection_template_versions WHERE entity_sync_id = 'sync-itv-m'",
    )
    .await
    .expect("package");
    assert!(package.contains(&format!("\"measurement_unit_id\":{h}")), "{package}");
}

#[tokio::test]
async fn publishing_a_new_version_moves_the_units_inside_checkpoint_packages() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin(&db).await;
    let bar_v1 = unit(&db, "BAR").await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    exec(
        &db,
        &format!(
            "INSERT INTO inspection_template_versions (entity_sync_id, template_id, version_no, effective_from, \
             checkpoint_package_json, requires_review, row_version) VALUES ('sync-itv-p', 1, 1, {NOW}, \
             '[{{\"checkpoint_code\":\"PRESS\",\"measurement_unit_id\":{bar_v1}}}]', 0, 1)"
        ),
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    let draft = units_draft(&db, actor).await;
    publish_draft(&db, draft.id, actor).await;
    let bar_v2 = unit(&db, "BAR").await;
    assert_ne!(bar_v1, bar_v2, "the new version has its own ids");
    let package = opt_str(
        &db,
        "SELECT checkpoint_package_json FROM inspection_template_versions WHERE entity_sync_id = 'sync-itv-p'",
    )
    .await
    .expect("package");
    assert!(
        package.contains(&format!("\"measurement_unit_id\":{bar_v2}")),
        "{package}"
    );
}

#[tokio::test]
async fn csv_import_applies_the_plant_unit_rules_and_never_imports_a_review_flag() {
    use crate::reference::imports;
    let db = crate::db::test_db::seeded().await;
    let actor = admin(&db).await;
    let draft = units_draft(&db, actor).await;
    let domain = crate::reference::domains::get_reference_domain_by_code(&db, UNITS_DOMAIN)
        .await
        .expect("domain");
    let row = |code: &str, parent: Option<&str>, meta: serde_json::Value| imports::ImportRowInput {
        code: Some(code.into()),
        label: Some(code.into()),
        description: None,
        parent_code: parent.map(str::to_string),
        sort_order: None,
        color_hex: None,
        icon_name: None,
        semantic_tag: None,
        external_code: None,
        metadata_json: Some(meta.to_string()),
    };
    let batch = imports::create_import_batch(&db, domain.id, "units.csv", "sha-units", Some(actor))
        .await
        .expect("batch");
    imports::stage_import_rows(
        &db,
        batch.id,
        vec![
            row(
                "BIDON",
                Some("L"),
                serde_json::json!({ "symbol": "bidon", "base_factor": 20 }),
            ),
            row(
                "SANSBASE",
                None,
                serde_json::json!({ "symbol": "sb", "base_factor": 2 }),
            ),
            row(
                "REVUE",
                Some("KG"),
                serde_json::json!({ "symbol": "rv", "base_factor": 2, "review_required": true }),
            ),
            row(
                "ZERO",
                Some("KG"),
                serde_json::json!({ "symbol": "z", "base_factor": 0 }),
            ),
        ],
    )
    .await
    .expect("stage");
    let validated = imports::validate_import_batch(&db, batch.id, Some(actor))
        .await
        .expect("validate");
    assert_eq!(validated.error_rows, 3, "{validated:?}");
    imports::apply_import_batch(
        &db,
        batch.id,
        imports::RefImportApplyPolicy {
            include_warnings: true,
            target_set_id: draft.id,
        },
        actor,
    )
    .await
    .expect("apply");
    let bidon = in_set(&db, draft.id, "BIDON").await;
    assert_eq!(
        opt_str(
            &db,
            &format!("SELECT semantic_tag FROM reference_values WHERE id = {bidon}")
        )
        .await
        .as_deref(),
        Some("volume"),
        "the dimension of the base unit"
    );
    assert_eq!(
        opt_i64(
            &db,
            &format!("SELECT parent_id FROM reference_values WHERE id = {bidon}")
        )
        .await,
        Some(in_set(&db, draft.id, "L").await)
    );
    for refused in ["SANSBASE", "REVUE", "ZERO"] {
        assert!(
            opt_i64(
                &db,
                &format!(
                    "SELECT id FROM reference_values WHERE set_id = {} AND code = '{refused}'",
                    draft.id
                )
            )
            .await
            .is_none(),
            "{refused} imported"
        );
    }
}

#[tokio::test]
async fn migration_201_restages_pending_sync_rows_and_links_broken_unit_links() {
    let db = migrated_before(M201).await;
    legacy_plant(&db).await;
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    // An article whose unit pointed at nothing, and pending outbox rows built before the migration.
    exec(
        &db,
        "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) \
         VALUES ('sync-art-broken', 'ART-BROKEN', 'Broken', 987654)",
    )
    .await;
    exec(
        &db,
        "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, payload_json, \
         payload_hash, status) VALUES ('k-old', 'articles', 'sync-art-ART-KG', 'upsert', 1, '{\"unit_value_id\":\"old\"}', 'h', 'pending'), \
         ('k-del', 'articles', 'sync-art-gone', 'delete', 1, '{}', 'h2', 'pending')",
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    Migrator::up(&db, Some(1)).await.expect("migration 201");

    let broken = units::unit_by_id(&db, article_unit(&db, "ART-BROKEN").await)
        .await
        .expect("read")
        .expect("unknown unit");
    assert_eq!(broken.code, "INCONNUE");
    assert!(broken.review_required);
    assert_eq!(
        opt_i64(&db, "SELECT COUNT(*) FROM sync_outbox WHERE idempotency_key = 'k-old'").await,
        Some(0),
        "the stale payload is dropped"
    );
    assert_eq!(
        opt_i64(&db, "SELECT COUNT(*) FROM sync_outbox WHERE idempotency_key = 'k-del'").await,
        Some(1),
        "a delete carries no unit and stays"
    );
    assert_eq!(
        opt_i64(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'articles' \
             AND entity_sync_id = 'sync-art-ART-KG' AND operation = 'upsert'"
        )
        .await,
        Some(1),
        "the row is journaled again from its current state"
    );
    assert_reference_links_hold(&db).await;
}
