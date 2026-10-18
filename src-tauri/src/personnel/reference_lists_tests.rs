//! Personnel and document lists as reference data (migration 228): seeded with the codes the
//! program used to fix, checked on every write, absence types carry a required behaviour tag.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::availability::{
    compute_personnel_availability_state, create_availability_block, AvailabilityBlockCreateInput,
};
use super::reference_lists::{self as lists, PersonnelOption};
use crate::errors::AppError;

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<i64>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// Adds a value to the published set of a list, as the reference editor would after publishing.
async fn add_value(db: &DatabaseConnection, domain: &str, code: &str, tag: Option<&str>) {
    let tag = tag.map_or("NULL".to_string(), |t| format!("'{t}'"));
    exec(
        db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, semantic_tag, is_active) \
             SELECT 'rl-{code}', rs.id, '{code}', 'Label {code}', 99, {tag}, 1 \
               FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE d.code = '{domain}' AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1"
        ),
    )
    .await;
}

fn codes(options: &[PersonnelOption]) -> Vec<&str> {
    options.iter().map(|o| o.code.as_str()).collect()
}

fn coded(err: &AppError) -> Option<&str> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.as_str()),
        _ => None,
    }
}

#[tokio::test]
async fn the_lists_are_seeded_with_the_codes_already_stored() {
    let db = crate::db::test_db::migrated().await;
    let o = lists::list_personnel_reference_options(&db).await.expect("options");
    assert_eq!(
        codes(&o.employment_statuses),
        ["active", "inactive", "suspended", "terminated"]
    );
    assert_eq!(codes(&o.employment_types), ["employee", "contractor", "temp", "vendor"]);
    assert_eq!(codes(&o.absence_types), ["leave", "medical", "restriction", "training"]);
    assert_eq!(
        o.absence_types.iter().map(|a| a.tag.as_deref()).collect::<Vec<_>>(),
        [Some("leave"), Some("critical"), Some("critical"), Some("training")]
    );
    assert_eq!(o.position_categories.len(), 8);
    let audience = |code: &str| {
        let code = code.to_string();
        let db = &db;
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT audience FROM reference_domains WHERE code = ?",
                [code.into()],
            ))
            .await
            .expect("q")
            .expect("row")
            .try_get_by_index::<String>(0)
            .expect("audience")
        }
    };
    assert_eq!(audience(lists::EMPLOYMENT_STATUSES_DOMAIN).await, "system");
    assert_eq!(audience(lists::ABSENCE_TYPES_DOMAIN).await, "company");
    assert_eq!(
        audience(crate::library_documents::DOCUMENT_CATEGORIES_DOMAIN).await,
        "company"
    );
    let docs = crate::library_documents::list_document_categories(&db)
        .await
        .expect("doc categories");
    assert_eq!(
        docs.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        [
            "technical_manuals",
            "sops",
            "safety_protocols",
            "compliance_certificates"
        ]
    );
}

#[tokio::test]
async fn codes_are_checked_against_the_lists_and_a_plant_value_is_accepted() {
    let db = crate::db::test_db::migrated().await;
    let err = lists::checked_employment_type(&db, "intern")
        .await
        .expect_err("not in the list");
    assert_eq!(coded(&err), Some(lists::CODE_NOT_IN_LIST));
    add_value(&db, lists::EMPLOYMENT_TYPES_DOMAIN, "INTERN", None).await;
    assert_eq!(
        lists::checked_employment_type(&db, "intern")
            .await
            .expect("plant value"),
        "INTERN"
    );
    assert!(lists::checked_employment_status(&db, "retired").await.is_err());
    assert_eq!(
        lists::checked_position_category(&db, "Planner").await.expect("seeded"),
        "planner"
    );

    let err = crate::library_documents::checked_category(&db, "drawings")
        .await
        .expect_err("not a category");
    assert_eq!(coded(&err), Some(crate::library_documents::CODE_CATEGORY_NOT_IN_LIST));
    add_value(
        &db,
        crate::library_documents::DOCUMENT_CATEGORIES_DOMAIN,
        "DRAWINGS",
        None,
    )
    .await;
    assert_eq!(
        crate::library_documents::checked_category(&db, "drawings")
            .await
            .expect("plant category"),
        "DRAWINGS"
    );
}

/// Every absence type needs a behaviour tag (no default): the reference engine refuses a value
/// without one, on every path (editor create / update, import).
#[tokio::test]
async fn an_absence_type_without_a_behaviour_tag_is_refused() {
    let db = crate::db::test_db::migrated().await;
    for missing in [None, Some(""), Some("holiday")] {
        let err = crate::reference::values::checked_semantic_tag(&db, lists::ABSENCE_TYPES_DOMAIN, missing)
            .await
            .expect_err("tag required");
        assert_eq!(coded(&err), Some(lists::CODE_ABSENCE_TAG_REQUIRED), "{missing:?}");
    }
    assert_eq!(
        crate::reference::values::checked_semantic_tag(&db, lists::ABSENCE_TYPES_DOMAIN, Some(" training "))
            .await
            .expect("valid tag"),
        Some("training".to_string())
    );
}

/// The program reads the behaviour tag, not the code: a plant's own critical type is always
/// critical, its own training type shows the person as in training.
#[tokio::test]
async fn a_plant_absence_type_behaves_by_its_tag() {
    let db = crate::db::test_db::seeded().await;
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('RL-1', 'Absent Person', 'available')",
    )
    .await;
    let person = scalar(&db, "SELECT id FROM personnel WHERE employee_code = 'RL-1'").await;
    let actor = scalar(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    add_value(&db, lists::ABSENCE_TYPES_DOMAIN, "ACCIDENT_TRAVAIL", Some("critical")).await;
    add_value(&db, lists::ABSENCE_TYPES_DOMAIN, "STAGE_EXTERNE", Some("training")).await;

    let block = |code: &str, start: &str, end: &str| AvailabilityBlockCreateInput {
        personnel_id: person,
        block_type: code.to_string(),
        start_at: start.to_string(),
        end_at: end.to_string(),
        reason_note: None,
        is_critical: None,
        confirm_overlap: None,
    };
    let critical = create_availability_block(
        &db,
        block("accident_travail", "2030-01-01T00:00:00Z", "2030-01-02T00:00:00Z"),
        actor,
    )
    .await
    .expect("critical block");
    assert!(critical.is_critical, "the tag makes it critical");
    assert_eq!(critical.block_type, "ACCIDENT_TRAVAIL");

    let err = create_availability_block(
        &db,
        block("holiday", "2030-02-01T00:00:00Z", "2030-02-02T00:00:00Z"),
        actor,
    )
    .await
    .expect_err("not an absence type");
    assert_eq!(coded(&err), Some(lists::CODE_NOT_IN_LIST));

    create_availability_block(
        &db,
        block("STAGE_EXTERNE", "2000-01-01T00:00:00Z", "2029-12-31T00:00:00Z"),
        actor,
    )
    .await
    .expect("training block");
    let state = compute_personnel_availability_state(&db, person)
        .await
        .expect("availability");
    assert_eq!(state.status, "in_training");
}
