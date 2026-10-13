//! Unit conversions: validation, article scope over general scope, inverse, optimistic updates,
//! disagreeing duplicates from two devices.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use super::*;

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

struct Units {
    u: i64,
    bx: i64,
    kg: i64,
    article: i64,
}

async fn setup() -> (DatabaseConnection, Units) {
    let db = crate::db::test_db::connect_in_memory().await.expect("in-memory SQLite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db).await.expect("seeder");
    let unit = |code: &str| {
        format!(
            "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'inventory.unit_of_measure' AND lv.code = '{code}'"
        )
    };
    let u = scalar(&db, &unit("U")).await;
    let bx = scalar(&db, &unit("BOX")).await;
    let kg = scalar(&db, &unit("KG")).await;
    exec(
        &db,
        &format!(
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id) \
             VALUES ('uc-art', 'FLT-10', 'Filtre à huile', {u})"
        ),
    )
    .await;
    let article = scalar(&db, "SELECT id FROM articles WHERE article_code = 'FLT-10'").await;
    (db, Units { u, bx, kg, article })
}

fn input(from: i64, to: i64, article_id: Option<i64>, factor: f64) -> CreateUnitConversionInput {
    CreateUnitConversionInput {
        from_unit_value_id: from,
        to_unit_value_id: to,
        article_id,
        factor,
        notes: Some("  carton fournisseur  ".into()),
    }
}

#[tokio::test]
async fn general_and_article_conversions_with_inverse() {
    let (db, u) = setup().await;
    let general = create_unit_conversion(&db, 1, input(u.bx, u.u, None, 10.0))
        .await
        .expect("general");
    assert_eq!(general.from_unit_code, "BOX");
    assert_eq!(general.notes.as_deref(), Some("carton fournisseur"));
    create_unit_conversion(&db, 1, input(u.bx, u.u, Some(u.article), 12.0))
        .await
        .expect("article");

    let any = convert_quantity(&db, 3.0, u.bx, u.u, None).await.expect("general");
    assert_eq!((any.quantity, any.path.as_str()), (30.0, "direct"));
    let filters = convert_quantity(&db, 3.0, u.bx, u.u, Some(u.article))
        .await
        .expect("article");
    assert_eq!(filters.quantity, 36.0, "the article's own conversion wins");
    let back = convert_quantity(&db, 24.0, u.u, u.bx, Some(u.article))
        .await
        .expect("inverse");
    assert_eq!((back.quantity, back.path.as_str()), (2.0, "inverse"));
    let same = convert_quantity(&db, 5.0, u.kg, u.kg, None).await.expect("identity");
    assert_eq!(same.path, "identity");
    assert!(convert_quantity(&db, 1.0, u.kg, u.u, None).await.is_err());

    let listed = list_unit_conversions(&db, Some(u.article), false).await.expect("list");
    assert_eq!(listed.len(), 2, "general + this article's");
}

#[tokio::test]
async fn invalid_conversions_are_refused() {
    let (db, u) = setup().await;
    for bad in [
        input(u.bx, u.bx, None, 2.0),
        input(u.bx, u.u, None, 0.0),
        input(u.bx, u.u, None, f64::NAN),
        input(u.bx, u.u, Some(9_999), 2.0),
    ] {
        assert!(create_unit_conversion(&db, 1, bad).await.is_err());
    }
    let not_a_unit = scalar(
        &db,
        "SELECT id FROM lookup_values WHERE id NOT IN (SELECT lv.id FROM lookup_values lv \
         JOIN lookup_domains ld ON ld.id = lv.domain_id WHERE ld.domain_key = 'inventory.unit_of_measure') LIMIT 1",
    )
    .await;
    assert!(
        create_unit_conversion(&db, 1, input(not_a_unit, u.u, None, 2.0))
            .await
            .is_err(),
        "only stock units"
    );
    create_unit_conversion(&db, 1, input(u.bx, u.u, None, 10.0))
        .await
        .expect("first");
    assert!(
        create_unit_conversion(&db, 1, input(u.u, u.bx, None, 0.1))
            .await
            .is_err(),
        "the inverse is derived, not stored twice"
    );
}

#[tokio::test]
async fn updates_are_optimistic_and_deactivation_hides_the_conversion() {
    let (db, u) = setup().await;
    let c = create_unit_conversion(&db, 1, input(u.bx, u.u, None, 10.0))
        .await
        .expect("create");
    let updated = update_unit_conversion(
        &db,
        UpdateUnitConversionInput {
            id: c.id,
            expected_row_version: c.row_version,
            factor: 12.0,
            notes: None,
        },
    )
    .await
    .expect("update");
    assert_eq!((updated.factor, updated.row_version), (12.0, c.row_version + 1));
    assert!(update_unit_conversion(
        &db,
        UpdateUnitConversionInput {
            id: c.id,
            expected_row_version: c.row_version,
            factor: 6.0,
            notes: None,
        },
    )
    .await
    .is_err());
    let off = deactivate_unit_conversion(&db, c.id, updated.row_version)
        .await
        .expect("deactivate");
    assert!(!off.is_active);
    assert!(convert_quantity(&db, 1.0, u.bx, u.u, None).await.is_err());
    assert_eq!(list_unit_conversions(&db, None, true).await.expect("all").len(), 1);
}

#[tokio::test]
async fn disagreeing_duplicates_from_two_devices_are_reported() {
    let (db, u) = setup().await;
    create_unit_conversion(&db, 1, input(u.bx, u.u, None, 10.0))
        .await
        .expect("here");
    // The same pair defined offline on another device arrives through sync.
    exec(
        &db,
        &format!(
            "INSERT INTO unit_conversions (entity_sync_id, from_unit_value_id, to_unit_value_id, factor, \
             created_at, updated_at) VALUES ('uc-other', {}, {}, 12, datetime('now'), datetime('now'))",
            u.bx, u.u
        ),
    )
    .await;
    assert!(convert_quantity(&db, 1.0, u.bx, u.u, None).await.is_err());
}
