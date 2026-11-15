//! L2-07 — technical characteristics per family: defined in Données de référence
//! (`EQUIPMENT.CHARACTERISTICS`), filled in on the equipment.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::characteristics::{
        self, checked_definition_metadata, normalized_value, parse_definition_meta, CharacteristicType,
        SaveAssetCharacteristicsPayload,
    };
    use crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity;

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = crate::db::test_db::seeded().await;
        ensure_system_reference_catalog_integrity(&db).await.expect("integrity");
        db
    }

    async fn exec(db: &sea_orm::DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    /// An equipment of the shipped family `family` (no class needed for the sheet).
    async fn equipment_of_family(db: &sea_orm::DatabaseConnection, code: &str, family: &str) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at, \
                 equipment_family_ref_id) VALUES ('l207-{code}', '{code}', 'Equipment {code}', 'active_in_service', \
                 datetime('now'), datetime('now'), \
                 (SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                   JOIN reference_domains d ON d.id = rs.domain_id WHERE d.code = 'EQUIPMENT.FAMILY' AND rv.code = '{family}'))"
            ),
        )
        .await;
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'"),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "id")
        .expect("id")
    }

    #[test]
    fn definitions_say_what_they_hold() {
        let meta =
            parse_definition_meta(Some(r#"{"data_type":"number","unit_code":"KW","required":true}"#)).expect("reads");
        assert_eq!(meta.data_type, CharacteristicType::Number);
        assert_eq!(meta.unit_code.as_deref(), Some("KW"));
        assert!(meta.required);
        assert!(parse_definition_meta(None).is_err(), "a type is required");
        assert!(parse_definition_meta(Some(r#"{"data_type":"colour"}"#)).is_err());
        assert!(
            parse_definition_meta(Some(r#"{"data_type":"text","unit_code":"KW"}"#)).is_err(),
            "only a number has a unit"
        );
        let err = checked_definition_metadata("X", Some("{}")).expect_err("no type");
        assert!(format!("{err:?}").contains("GATE_ASSET_CHARACTERISTIC_DEFINITION_INVALID"));
        // Canonical: the three keys always written, other keys kept.
        let canonical: serde_json::Value = serde_json::from_str(
            &checked_definition_metadata("X", Some(r#"{"data_type":"text","note":"n"}"#)).expect("ok"),
        )
        .expect("json");
        assert_eq!(canonical["data_type"], "text");
        assert_eq!(canonical["required"], false);
        assert!(canonical["unit_code"].is_null());
        assert_eq!(canonical["note"], "n");
    }

    #[test]
    fn values_are_stored_for_their_type() {
        assert_eq!(
            normalized_value("P", CharacteristicType::Number, " 7,5 ").expect("n"),
            "7.5"
        );
        assert!(normalized_value("P", CharacteristicType::Number, "sept")
            .expect_err("not a number")
            .contains("GATE_ASSET_CHARACTERISTIC_NOT_NUMBER"));
        assert_eq!(
            normalized_value("B", CharacteristicType::Boolean, "true").expect("b"),
            "true"
        );
        assert!(normalized_value("B", CharacteristicType::Boolean, "oui")
            .expect_err("not a boolean")
            .contains("GATE_ASSET_CHARACTERISTIC_NOT_BOOLEAN"));
        assert_eq!(
            normalized_value("T", CharacteristicType::Text, " 4-20 mA ").expect("t"),
            "4-20 mA"
        );
    }

    #[tokio::test]
    async fn shipped_families_have_their_characteristics_in_reference_data() {
        let db = setup().await;
        // Seeding twice keeps one row per characteristic.
        characteristics::ensure_characteristic_catalog(&db)
            .await
            .expect("again");
        let n: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = 'EQUIPMENT.CHARACTERISTICS' AND rv.code = 'MOTOR_POWER'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "n")
            .expect("n");
        assert_eq!(n, 1);
        let asset = equipment_of_family(&db, "MOT-1", "MOTOR_FAMILY").await;
        let sheet = characteristics::get_asset_characteristics(&db, asset)
            .await
            .expect("sheet");
        assert_eq!(sheet.family_code.as_deref(), Some("MOTOR_FAMILY"));
        let power = sheet.items.iter().find(|i| i.code == "MOTOR_POWER").expect("power");
        assert_eq!(power.data_type.as_deref(), Some("number"));
        assert_eq!(power.unit_symbol.as_deref(), Some("kW"));
        assert!(power.required);
        assert!(
            sheet.items.iter().all(|i| !i.code.starts_with("PUMP_")),
            "only the family's own characteristics"
        );
    }

    #[tokio::test]
    async fn values_are_checked_against_the_family_definitions() {
        let db = setup().await;
        let asset = equipment_of_family(&db, "MOT-2", "MOTOR_FAMILY").await;
        let save = |values: &[(&str, Option<&str>)]| SaveAssetCharacteristicsPayload {
            asset_id: asset,
            values: values
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.map(str::to_string)))
                .collect::<BTreeMap<_, _>>(),
        };

        // The required power is missing.
        let err = characteristics::save_asset_characteristics(&db, save(&[("MOTOR_VOLTAGE", Some("400"))]), 1)
            .await
            .expect_err("required");
        assert!(format!("{err:?}").contains("GATE_ASSET_CHARACTERISTIC_REQUIRED"));

        // A characteristic of another family is refused.
        let err = characteristics::save_asset_characteristics(
            &db,
            save(&[("MOTOR_POWER", Some("15")), ("PUMP_FLOW", Some("3"))]),
            1,
        )
        .await
        .expect_err("unknown");
        assert!(format!("{err:?}").contains("GATE_ASSET_CHARACTERISTIC_UNKNOWN"));

        let sheet = characteristics::save_asset_characteristics(
            &db,
            save(&[("MOTOR_POWER", Some("15,5")), ("MOTOR_VOLTAGE", Some("400"))]),
            1,
        )
        .await
        .expect("saved");
        let value = |code: &str| {
            sheet
                .items
                .iter()
                .find(|i| i.code == code)
                .and_then(|i| i.value.clone())
        };
        assert_eq!(value("MOTOR_POWER").as_deref(), Some("15.5"));
        assert_eq!(value("MOTOR_VOLTAGE").as_deref(), Some("400"));

        // Clearing one keeps the others.
        let sheet = characteristics::save_asset_characteristics(&db, save(&[("MOTOR_VOLTAGE", None)]), 1)
            .await
            .expect("cleared");
        assert!(sheet.items.iter().any(|i| i.code == "MOTOR_POWER" && i.value.is_some()));
        assert!(sheet
            .items
            .iter()
            .all(|i| i.code != "MOTOR_VOLTAGE" || i.value.is_none()));

        // A value whose characteristic left the list stays visible.
        exec(
            &db,
            "UPDATE reference_values SET is_active = 0 WHERE code = 'MOTOR_POWER' AND set_id IN \
             (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
               WHERE d.code = 'EQUIPMENT.CHARACTERISTICS')",
        )
        .await;
        let sheet = characteristics::get_asset_characteristics(&db, asset)
            .await
            .expect("sheet");
        assert!(sheet
            .unlisted
            .iter()
            .any(|u| u.code == "MOTOR_POWER" && u.value == "15.5"));
    }

    #[test]
    fn a_characteristic_hangs_under_a_family() {
        assert_eq!(
            crate::reference::values::required_parent_domain("EQUIPMENT.CHARACTERISTICS"),
            Some("EQUIPMENT.FAMILY")
        );
    }
}
