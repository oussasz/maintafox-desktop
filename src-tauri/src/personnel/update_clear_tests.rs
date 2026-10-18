//! Person update: an absent field is left unchanged, an explicit `null` (or an emptied text)
//! clears it.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use super::domain::PersonnelUpdateInput;
use super::queries::{get_personnel, update_personnel};

#[test]
fn absent_and_null_are_told_apart_in_the_payload() {
    let input: PersonnelUpdateInput = serde_json::from_value(serde_json::json!({
        "id": 1,
        "expected_row_version": 1,
        "email": null,
        "notes": "kept",
        "supervisor_id": null,
    }))
    .expect("payload");
    assert_eq!(input.email, Some(None), "null = clear");
    assert_eq!(input.notes, Some(Some("kept".to_string())));
    assert_eq!(input.supervisor_id, Some(None));
    assert_eq!(input.phone, None, "absent = unchanged");
    assert_eq!(input.contract_end_date, None);
}

#[tokio::test]
async fn a_cleared_field_is_saved_as_cleared_and_an_absent_one_is_kept() {
    let db = crate::db::test_db::seeded().await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO personnel (employee_code, full_name, employment_type, employment_origin, employment_status, \
         availability_status, email, phone, notes, contract_number, contract_start_date, contract_end_date, \
         row_version) VALUES ('CLR-1', 'Clear Me', 'employee', 'internal', 'active', 'available', \
         'old@example.test', '0600', 'a note', 'C-1', '2026-01-01', '2026-12-31', 1)"
            .to_string(),
    ))
    .await
    .expect("person");
    let id = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM personnel WHERE employee_code = 'CLR-1'".to_string(),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("id");
    let input: PersonnelUpdateInput = serde_json::from_value(serde_json::json!({
        "id": id,
        "expected_row_version": 1,
        "email": null,
        "notes": "   ",
        "contract_end_date": null,
        "contract_number": "C-2",
    }))
    .expect("payload");
    update_personnel(&db, input, 1).await.expect("update");

    let p = get_personnel(&db, id).await.expect("read").expect("person");
    assert_eq!(p.email, None, "null clears");
    assert_eq!(p.notes, None, "an emptied text clears");
    assert_eq!(p.contract_end_date, None);
    assert_eq!(p.contract_number.as_deref(), Some("C-2"), "a value sets");
    assert_eq!(p.phone.as_deref(), Some("0600"), "absent = unchanged");
    assert_eq!(p.contract_start_date.as_deref(), Some("2026-01-01"));
}
