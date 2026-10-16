//! Currency list: shipped ISO currencies, the operational copy stock costs link to, and checks.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use super::*;

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("decode")
}

async fn lookup_id(db: &impl ConnectionTrait, code: &str) -> Option<i64> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
         WHERE ld.domain_key = ? AND lv.code = ? AND lv.deleted_at IS NULL",
        [LOOKUP_DOMAIN_KEY.into(), code.into()],
    ))
    .await
    .expect("query")
    .map(|r| r.try_get_by_index::<i64>(0).expect("id"))
}

#[tokio::test]
async fn the_list_holds_the_settings_currencies_and_keeps_the_stock_links() {
    let db = crate::db::test_db::seeded().await;
    let currencies = published_currencies(&db).await.unwrap();
    for c in ISO_CURRENCIES {
        let found = currencies.iter().find(|o| o.code == c.code).expect("shipped currency");
        assert!(found.is_active);
        assert_eq!(found.minor_units, Some(c.minor_units));
        assert_eq!(found.symbol.as_deref(), Some(c.symbol));
        assert_eq!(found.label_en.as_deref(), Some(c.label_en));
        assert!(
            lookup_id(&db, c.code).await.is_some(),
            "{} mirrored for stock costs",
            c.code
        );
    }

    // The ids stock costs already hold do not change when the list is mirrored again.
    let eur = lookup_id(&db, "EUR").await.expect("EUR");
    ensure_currency_catalog(&db).await.unwrap();
    assert_eq!(lookup_id(&db, "EUR").await, Some(eur));
    assert_eq!(
        mirror_into_lookup(&db).await.unwrap(),
        0,
        "a second mirror changes nothing"
    );
}

#[tokio::test]
async fn a_currency_retired_from_the_list_is_deactivated_not_deleted() {
    let db = crate::db::test_db::seeded().await;
    let gbp = lookup_id(&db, "GBP").await.expect("GBP");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'GBP' AND set_id IN \
         (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
           WHERE d.code = 'INVENTORY.CURRENCY')"
            .to_string(),
    ))
    .await
    .unwrap();
    mirror_into_lookup(&db).await.unwrap();
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_active FROM lookup_values WHERE id = ?",
            vec![gbp.into()]
        )
        .await,
        0
    );
    assert!(ensure_active_currency(&db, "GBP").await.is_err());
    assert!(ensure_active_currency(&db, "dzd").await.is_ok());
}

#[test]
fn plant_currencies_are_checked() {
    assert!(checked_currency_metadata("XOF", Some(r#"{"minor_units":0,"symbol":"FCFA"}"#)).is_ok());
    assert!(
        checked_currency_metadata("XO", Some(r#"{"minor_units":0}"#)).is_err(),
        "three letters"
    );
    assert!(
        checked_currency_metadata("XOF", Some(r#"{"symbol":"FCFA"}"#)).is_err(),
        "minor units required"
    );
    assert!(checked_currency_metadata("XOF", Some(r#"{"minor_units":7}"#)).is_err());
    assert!(checked_currency_metadata("XOF", Some(r#"{"minor_units":2,"symbol":"TOO-LONG-SYMBOL"}"#)).is_err());
}
