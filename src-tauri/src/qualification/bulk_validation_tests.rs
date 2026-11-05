//! Validation list (L7-32): unvalidated skills and pending certificates, validated line by line
//! exactly as shown, each with its audit event.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::qualification::bulk_validation::{
    list_pending_qualifications, validate_qualifications, QualificationValidationItem, Validator,
};

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("exec");
}

struct Fixture {
    db: DatabaseConnection,
    karim: i64,
    validator: i64,
    skill_row: i64,
    cert_row: i64,
}

async fn setup() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('BV-1', 'Karim Elec', 'available')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('BV-2', 'Nadia Chef', 'available')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, availability_status, employment_status) \
         VALUES ('BV-3', 'Omar Parti', 'available', 'inactive')",
    )
    .await;
    let karim = scalar_i64(&db, "SELECT id FROM personnel WHERE employee_code = 'BV-1'").await;
    let validator = scalar_i64(&db, "SELECT id FROM personnel WHERE employee_code = 'BV-2'").await;
    let omar = scalar_i64(&db, "SELECT id FROM personnel WHERE employee_code = 'BV-3'").await;
    let skill = scalar_i64(
        &db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE rd.code = 'PERSONNEL.SKILLS' AND rv.code = 'HYDRAULICS'",
    )
    .await;
    for (pid, validated) in [(karim, 0), (omar, 0), (validator, 1)] {
        exec(
            &db,
            &format!(
                "INSERT INTO personnel_skills (entity_sync_id, personnel_id, reference_value_id, proficiency_level, \
                 source_type, is_validated, created_at, updated_at) \
                 VALUES ('bv-skill-{pid}', {pid}, {skill}, 3, 'import', {validated}, datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }
    exec(
        &db,
        "INSERT INTO certification_types (entity_sync_id, code, name, default_validity_months, renewal_lead_days, \
         category, row_version) VALUES ('ct-bv', 'BR', 'Habilitation BR', 36, 60, 'electrical', 1)",
    )
    .await;
    let ct = scalar_i64(&db, "SELECT id FROM certification_types WHERE code = 'BR'").await;
    for (pid, status) in [(karim, "pending"), (omar, "pending"), (validator, "verified")] {
        exec(
            &db,
            &format!(
                "INSERT INTO personnel_certifications (entity_sync_id, personnel_id, certification_type_id, issued_at, \
                 expires_at, verification_status, row_version) \
                 VALUES ('bv-cert-{pid}', {pid}, {ct}, '2026-01-10', '2029-01-10', '{status}', 1)"
            ),
        )
        .await;
    }
    let skill_row = scalar_i64(
        &db,
        &format!("SELECT id FROM personnel_skills WHERE personnel_id = {karim}"),
    )
    .await;
    let cert_row = scalar_i64(
        &db,
        &format!("SELECT id FROM personnel_certifications WHERE personnel_id = {karim}"),
    )
    .await;
    Fixture {
        db,
        karim,
        validator,
        skill_row,
        cert_row,
    }
}

fn skill_item(id: i64, level: i64, valid_to: Option<&str>) -> QualificationValidationItem {
    QualificationValidationItem {
        kind: "skill".into(),
        id,
        proficiency_level: Some(level),
        valid_to: valid_to.map(str::to_string),
        row_version: None,
    }
}

fn cert_item(id: i64, row_version: i64) -> QualificationValidationItem {
    QualificationValidationItem {
        kind: "certification".into(),
        id,
        proficiency_level: None,
        valid_to: None,
        row_version: Some(row_version),
    }
}

fn by(validator: Option<i64>) -> Validator {
    Validator {
        user_id: 1,
        personnel_id: validator,
    }
}

async fn audit_count(db: &DatabaseConnection, action: &str) -> i64 {
    scalar_i64(
        db,
        &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = '{action}'"),
    )
    .await
}

#[tokio::test]
async fn lists_only_waiting_lines_of_active_people() {
    let f = setup().await;
    let rows = list_pending_qualifications(&f.db).await.expect("list");
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().all(|r| r.personnel_id == f.karim));
    let skill = rows.iter().find(|r| r.kind == "skill").expect("skill");
    assert_eq!(skill.item_code, "HYDRAULICS");
    assert_eq!(skill.proficiency_level, Some(3));
    assert_eq!(skill.source_type.as_deref(), Some("import"));
    let cert = rows.iter().find(|r| r.kind == "certification").expect("cert");
    assert_eq!(cert.item_code, "BR");
    assert_eq!(cert.row_version, Some(1));
    assert_eq!(cert.valid_to.as_deref(), Some("2029-01-10"));
}

#[tokio::test]
async fn validates_each_line_with_its_audit() {
    let f = setup().await;
    let results = validate_qualifications(
        &f.db,
        &[skill_item(f.skill_row, 3, None), cert_item(f.cert_row, 1)],
        by(Some(f.validator)),
    )
    .await
    .expect("validate");
    assert!(results.iter().all(|r| r.validated), "{results:?}");
    let skill_sql = format!(
        "SELECT COUNT(*) FROM personnel_skills WHERE id = {} AND is_validated = 1 AND verified_by_id = {} \
         AND verified_at IS NOT NULL",
        f.skill_row, f.validator
    );
    assert_eq!(scalar_i64(&f.db, &skill_sql).await, 1);
    let cert_sql = format!(
        "SELECT COUNT(*) FROM personnel_certifications WHERE id = {} AND verification_status = 'verified' \
         AND row_version = 2",
        f.cert_row
    );
    assert_eq!(scalar_i64(&f.db, &cert_sql).await, 1);
    assert_eq!(audit_count(&f.db, "qualification.skill_validated").await, 1);
    assert_eq!(audit_count(&f.db, "qualification.certificate_verified").await, 1);
    assert!(list_pending_qualifications(&f.db).await.expect("list").is_empty());
}

#[tokio::test]
async fn a_line_changed_since_shown_is_refused_and_the_others_go_on() {
    let f = setup().await;
    // The level was raised on the sheet after the list was read: the shown claim is gone.
    exec(
        &f.db,
        &format!(
            "UPDATE personnel_skills SET proficiency_level = 4 WHERE id = {}",
            f.skill_row
        ),
    )
    .await;
    let results = validate_qualifications(
        &f.db,
        &[
            skill_item(f.skill_row, 3, None),
            cert_item(f.cert_row, 1),
            // Asked twice: counted once.
            cert_item(f.cert_row, 1),
        ],
        by(Some(f.validator)),
    )
    .await
    .expect("validate");
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(!results[0].validated);
    assert_eq!(results[0].code.as_deref(), Some("QUAL_VALIDATION_CHANGED"));
    assert!(results[1].validated);
    assert_eq!(audit_count(&f.db, "qualification.skill_validated").await, 0);
    let validated = format!("SELECT is_validated FROM personnel_skills WHERE id = {}", f.skill_row);
    assert_eq!(scalar_i64(&f.db, &validated).await, 0);
    // A stale certificate version is refused too.
    let again = validate_qualifications(&f.db, &[cert_item(f.cert_row, 1)], by(Some(f.validator)))
        .await
        .expect("validate");
    assert_eq!(again[0].code.as_deref(), Some("QUAL_VALIDATION_CHANGED"));
    assert_eq!(audit_count(&f.db, "qualification.certificate_verified").await, 1);
}

#[tokio::test]
async fn a_skill_needs_a_validator_with_a_personnel_record() {
    let f = setup().await;
    let results = validate_qualifications(
        &f.db,
        &[skill_item(f.skill_row, 3, None), cert_item(f.cert_row, 1)],
        by(None),
    )
    .await
    .expect("validate");
    assert_eq!(results[0].code.as_deref(), Some("SKILL_VALIDATOR_NOT_LINKED"));
    assert!(results[1].validated, "certificates name no validator record");
    assert_eq!(audit_count(&f.db, "qualification.skill_validated").await, 0);
}

#[tokio::test]
async fn lines_of_inactive_people_and_unknown_kinds_are_refused() {
    let f = setup().await;
    let omar_skill = scalar_i64(
        &f.db,
        "SELECT ps.id FROM personnel_skills ps JOIN personnel p ON p.id = ps.personnel_id WHERE p.employee_code = 'BV-3'",
    )
    .await;
    let unknown = QualificationValidationItem {
        kind: "badge".into(),
        id: f.skill_row,
        proficiency_level: Some(3),
        valid_to: None,
        row_version: None,
    };
    let results = validate_qualifications(
        &f.db,
        &[skill_item(omar_skill, 3, None), unknown],
        by(Some(f.validator)),
    )
    .await
    .expect("validate");
    assert_eq!(results[0].code.as_deref(), Some("QUAL_VALIDATION_CHANGED"));
    assert_eq!(results[1].code.as_deref(), Some("QUAL_VALIDATION_KIND_UNKNOWN"));
}
