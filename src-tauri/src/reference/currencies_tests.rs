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

async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'", vec![]).await
}

async fn currency_draft(db: &sea_orm::DatabaseConnection, actor: i64) -> crate::reference::sets::ReferenceSet {
    let domain = crate::reference::domains::get_reference_domain_by_code(db, CURRENCY_DOMAIN)
        .await
        .expect("domain");
    crate::reference::sets::create_draft_set(db, domain.id, actor)
        .await
        .expect("draft")
}

async fn publish(db: &sea_orm::DatabaseConnection, set_id: i64, actor: i64) {
    crate::reference::sets::validate_set(db, set_id, actor)
        .await
        .expect("validate");
    crate::reference::publish::preview_publish_impact(db, set_id)
        .await
        .expect("impact");
    crate::reference::publish::publish_reference_set(db, set_id, actor)
        .await
        .expect("publish");
}

fn currency_row(code: &str, meta: serde_json::Value) -> crate::reference::imports::ImportRowInput {
    crate::reference::imports::ImportRowInput {
        code: Some(code.into()),
        label: Some(code.into()),
        description: None,
        parent_code: None,
        sort_order: None,
        color_hex: None,
        icon_name: None,
        semantic_tag: None,
        external_code: None,
        metadata_json: Some(meta.to_string()),
    }
}

#[tokio::test]
async fn a_csv_import_applies_the_currency_rules() {
    use crate::reference::imports;
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let draft = currency_draft(&db, actor).await;
    let domain = crate::reference::domains::get_reference_domain_by_code(&db, CURRENCY_DOMAIN)
        .await
        .expect("domain");
    let batch = imports::create_import_batch(&db, domain.id, "devises.csv", "sha-currencies", Some(actor))
        .await
        .expect("batch");
    imports::stage_import_rows(
        &db,
        batch.id,
        vec![
            currency_row("XOF", serde_json::json!({ "minor_units": 0, "symbol": "FCFA" })),
            currency_row("XOFX", serde_json::json!({ "minor_units": 0 })),
            currency_row("ABC", serde_json::json!({ "minor_units": 7 })),
            currency_row("DEF", serde_json::json!({ "symbol": "D" })),
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
    let in_draft = |code: &'static str| {
        let db = db.clone();
        let set = draft.id;
        async move {
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM reference_values WHERE set_id = ? AND code = ?",
                vec![set.into(), code.into()],
            )
            .await
        }
    };
    assert_eq!(in_draft("XOF").await, 1);
    for refused in ["XOFX", "ABC", "DEF"] {
        assert_eq!(in_draft(refused).await, 0, "{refused} imported");
    }
}

#[tokio::test]
async fn a_currency_in_use_cannot_be_deactivated_and_plant_currencies_are_not_system() {
    let db = crate::db::test_db::seeded().await;
    let actor = admin_id(&db).await;
    let draft = currency_draft(&db, actor).await;
    crate::reference::values::create_value(
        &db,
        crate::reference::values::CreateReferenceValuePayload {
            set_id: draft.id,
            parent_id: None,
            code: "XOF".into(),
            label: "Franc CFA".into(),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: Some(r#"{"minor_units":0,"symbol":"FCFA","label_en":"CFA franc"}"#.into()),
        },
        actor,
    )
    .await
    .expect("plant currency");
    publish(&db, draft.id, actor).await;
    let xof = lookup_id(&db, "XOF").await.expect("mirrored");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_system FROM lookup_values WHERE id = ?",
            vec![xof.into()]
        )
        .await,
        0,
        "a plant currency is not a system row"
    );
    let eur = lookup_id(&db, "EUR").await.expect("EUR");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_system FROM lookup_values WHERE id = ?",
            vec![eur.into()]
        )
        .await,
        1
    );

    // XOF becomes the company currency: it cannot be retired, neither in the editor nor by a
    // published list.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE app_settings SET setting_value_json = '\"XOF\"' WHERE setting_key = 'locale.currency_code'".to_string(),
    ))
    .await
    .unwrap();
    assert!(!currency_uses(&db, "XOF").await.unwrap().is_empty());
    let next = currency_draft(&db, actor).await;
    let in_next = scalar_i64(
        &db,
        "SELECT id FROM reference_values WHERE set_id = ? AND code = 'XOF'",
        vec![next.id.into()],
    )
    .await;
    let refused = crate::reference::values::deactivate_value(&db, in_next, actor).await;
    match refused {
        Err(AppError::ValidationFailed(msgs)) => assert!(msgs[0].starts_with(GATE_CURRENCY_IN_USE), "{msgs:?}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'XOF' AND set_id IN \
         (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
           WHERE d.code = 'INVENTORY.CURRENCY' AND rs.status = 'published')"
            .to_string(),
    ))
    .await
    .unwrap();
    assert!(
        mirror_into_lookup(&db).await.is_err(),
        "the operational copy keeps it active"
    );
}
