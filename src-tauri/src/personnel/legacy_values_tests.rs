//! Values stored before the personnel / document lists existed (migrations 228 and 229, review
//! B1 / S1 / S3): run through the real migration chain, nothing is rewritten, behaviour is kept,
//! every unknown value is listed and flagged "à revoir", and records stay editable.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use super::availability::compute_personnel_availability_state;
use super::reference_lists::{self as lists, PersonnelOption};
use crate::migrations::Migrator;

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

async fn texts(db: &DatabaseConnection, sql: &str) -> Vec<String> {
    db.query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .iter()
        .map(|r| r.try_get_by_index::<String>(0).expect("text"))
        .collect()
}

/// A database with every migration applied up to, not including, migration `name`.
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

fn find<'a>(options: &'a [PersonnelOption], code: &str) -> &'a PersonnelOption {
    options
        .iter()
        .find(|o| o.code == code)
        .unwrap_or_else(|| panic!("{code} is not listed"))
}

/// One person per legacy absence type, each with a block over today; one person with a legacy
/// type, position category and status.
async fn seed_legacy_people(db: &DatabaseConnection) {
    exec(
        db,
        "INSERT INTO positions (entity_sync_id, code, name, category, is_active, created_at, updated_at) \
         VALUES ('lg-pos', 'LG-POS', 'Apprenti', 'apprentice', 1, datetime('now'), datetime('now'))",
    )
    .await;
    for (code, block) in [
        ("LG-VAC", "vacation"),
        ("LG-SICK", "sick"),
        ("LG-LEAVE", "leave"),
        ("LG-OFF", "day_off"),
        ("LG-TRN", "training"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO personnel (employee_code, full_name, employment_type, availability_status) \
                 VALUES ('{code}', 'Person {code}', 'employee', 'available')"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO personnel_availability_blocks (personnel_id, block_type, start_at, end_at, is_critical) \
                 SELECT id, '{block}', '2000-01-01T00:00:00Z', '2100-01-01T00:00:00Z', 0 \
                   FROM personnel WHERE employee_code = '{code}'"
            ),
        )
        .await;
    }
    exec(
        db,
        "INSERT INTO personnel (employee_code, full_name, employment_type, employment_status, availability_status, \
         position_id, row_version) SELECT 'LG-ODD', 'Odd Person', 'intern', 'on_secondment', 'available', id, 1 \
         FROM positions WHERE code = 'LG-POS'",
    )
    .await;
}

async fn state_of(db: &DatabaseConnection, code: &str) -> String {
    let id = scalar(db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'")).await;
    compute_personnel_availability_state(db, id)
        .await
        .expect("availability")
        .status
}

/// The availability each legacy absence type gave before the lists (migration 228's parent code):
/// leave / vacation / sick → on leave, training → in training, anything else → blocked.
const FORMER_STATES: &[(&str, &str)] = &[
    ("LG-VAC", "on_leave"),
    ("LG-SICK", "on_leave"),
    ("LG-LEAVE", "on_leave"),
    ("LG-OFF", "blocked"),
    ("LG-TRN", "in_training"),
];

#[tokio::test]
async fn legacy_values_keep_their_behaviour_and_are_flagged_for_review() {
    let db = migrated_before("m20261017_000228_personnel_document_lists").await;
    seed_legacy_people(&db).await;
    let stored_before = texts(&db, "SELECT block_type FROM personnel_availability_blocks ORDER BY id").await;

    Migrator::up(&db, None).await.expect("228, 229 and the rest");

    for (code, former) in FORMER_STATES {
        assert_eq!(
            state_of(&db, code).await,
            *former,
            "{code}: same availability as before"
        );
    }
    assert_eq!(
        texts(&db, "SELECT block_type FROM personnel_availability_blocks ORDER BY id").await,
        stored_before,
        "no stored value rewritten"
    );

    let o = lists::list_personnel_reference_options(&db).await.expect("options");
    for (code, tag) in [("vacation", "leave"), ("sick", "leave"), ("day_off", "critical")] {
        let v = find(&o.absence_types, code);
        assert!(v.review_required, "{code} flagged à revoir");
        assert_eq!(v.tag.as_deref(), Some(tag), "{code} keeps its former behaviour");
    }
    assert!(
        !find(&o.absence_types, "leave").review_required,
        "a shipped value is not flagged"
    );
    assert!(find(&o.employment_types, "intern").review_required);
    assert!(find(&o.position_categories, "apprentice").review_required);

    // SYSTEM statuses: not added to the list, listed for review, read as before (not active).
    assert!(o.employment_statuses.iter().all(|s| s.code != "on_secondment"));
    let review = lists::list_personnel_status_review(&db).await.expect("review");
    assert_eq!(
        review
            .iter()
            .map(|r| (r.employee_code.as_str(), r.stored_status.as_str()))
            .collect::<Vec<_>>(),
        [("LG-ODD", "on_secondment")]
    );
    let (_, counted, without) = crate::analytics::smrp::weekly_capacity(&db, None)
        .await
        .expect("capacity");
    let active_people = scalar(&db, "SELECT COUNT(*) FROM personnel WHERE employment_status = 'active'").await;
    assert_eq!(
        counted + without,
        active_people,
        "the unknown status is not counted as active"
    );
}

/// S1: a record keeps a retired or flagged value while the field is left as it is; only a change
/// is checked against the list.
#[tokio::test]
async fn a_record_with_a_retired_or_flagged_value_stays_editable() {
    let db = migrated_before("m20261017_000228_personnel_document_lists").await;
    seed_legacy_people(&db).await;
    Migrator::up(&db, None).await.expect("migrations");
    // The plant retires its "contractor" type after people were given it.
    exec(
        &db,
        "UPDATE personnel SET employment_type = 'contractor' WHERE employee_code = 'LG-VAC'",
    )
    .await;
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'contractor' AND set_id IN \
         (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'PERSONNEL.EMPLOYMENT_TYPES')",
    )
    .await;

    for (code, payload) in [
        (
            "LG-VAC",
            serde_json::json!({ "employment_type": "contractor", "phone": "0601" }),
        ),
        (
            "LG-ODD",
            serde_json::json!({ "employment_type": "intern", "employment_status": "on_secondment", "phone": "0602" }),
        ),
    ] {
        let id = scalar(&db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'")).await;
        let rv = scalar(&db, &format!("SELECT row_version FROM personnel WHERE id = {id}")).await;
        let mut body = payload.as_object().cloned().expect("object");
        body.insert("id".into(), id.into());
        body.insert("expected_row_version".into(), rv.into());
        let input: super::domain::PersonnelUpdateInput =
            serde_json::from_value(serde_json::Value::Object(body)).expect("payload");
        super::queries::update_personnel(&db, input, 1)
            .await
            .unwrap_or_else(|e| panic!("{code}: an unchanged stored value must not be refused: {e:?}"));
    }

    // Changing TO the retired value is refused.
    let id = scalar(&db, "SELECT id FROM personnel WHERE employee_code = 'LG-SICK'").await;
    let rv = scalar(&db, &format!("SELECT row_version FROM personnel WHERE id = {id}")).await;
    let input: super::domain::PersonnelUpdateInput = serde_json::from_value(serde_json::json!({
        "id": id, "expected_row_version": rv, "employment_type": "contractor"
    }))
    .expect("payload");
    assert!(super::queries::update_personnel(&db, input, 1).await.is_err());

    // A block of a flagged type is edited without changing its type: kept, behaviour kept.
    let block_id = scalar(
        &db,
        "SELECT b.id FROM personnel_availability_blocks b JOIN personnel p ON p.id = b.personnel_id \
         WHERE p.employee_code = 'LG-OFF'",
    )
    .await;
    super::availability::update_availability_block(
        &db,
        super::availability::AvailabilityBlockUpdateInput {
            id: block_id,
            block_type: Some("day_off".into()),
            start_at: "2000-01-01T00:00:00Z".into(),
            end_at: "2100-01-01T00:00:00Z".into(),
            reason_note: Some("checked".into()),
            keep_reason_note: false,
            is_critical: None,
            confirm_overlap: None,
        },
        1,
    )
    .await
    .expect("unchanged flagged type is editable");
    assert_eq!(state_of(&db, "LG-OFF").await, "blocked");
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT is_critical FROM personnel_availability_blocks WHERE id = {block_id}")
        )
        .await,
        0,
        "a note edit keeps the stored critical flag of a legacy block"
    );
}

/// S3: the real chain up to 228 with documents in all four categories and sync capture installed:
/// the rebuild keeps every row and writes nothing to the capture journal; a document received by
/// sync with a category unknown here is stored as sent and its category is adopted, flagged.
#[tokio::test]
async fn the_document_rebuild_on_the_real_chain_and_an_unknown_category_by_sync() {
    let db = migrated_before("m20261017_000228_personnel_document_lists").await;
    // Capture triggers on the documents as a device has them (insert / update / delete into the
    // journal). Written here rather than by `sync::capture::install_capture_in`, whose layout
    // cache is shared by the whole test process and may describe a newer schema than this
    // database's (an installed device builds it after its migrations).
    for (event, row) in [("INSERT", "NEW"), ("UPDATE", "NEW"), ("DELETE", "OLD")] {
        exec(
            &db,
            &format!(
                "CREATE TRIGGER lg_cap_library_documents_{event} AFTER {event} ON library_documents \
                 BEGIN INSERT INTO sync_capture_journal (entity_type, entity_sync_id, operation) \
                 VALUES ('library_documents', {row}.entity_sync_id, \
                         CASE WHEN '{event}' = 'DELETE' THEN 'delete' ELSE 'upsert' END); END"
            ),
        )
        .await;
    }
    for (i, cat) in [
        "technical_manuals",
        "sops",
        "safety_protocols",
        "compliance_certificates",
    ]
    .iter()
    .enumerate()
    {
        exec(
            &db,
            &format!(
                "INSERT INTO library_documents (category, title, file_name, relative_path, entity_sync_id) \
                 VALUES ('{cat}', 'Doc {i}', 'f{i}.pdf', 'library_documents/{cat}/f{i}.pdf', 'lg-doc-{i}')"
            ),
        )
        .await;
    }
    let rows_sql = "SELECT id || '|' || category || '|' || entity_sync_id FROM library_documents ORDER BY id";
    let before = texts(&db, rows_sql).await;
    let journal_before = scalar(
        &db,
        "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'library_documents'",
    )
    .await;

    Migrator::up(&db, None).await.expect("228 and later");

    assert_eq!(
        texts(&db, rows_sql).await,
        before,
        "every document, id and category kept"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'library_documents'"
        )
        .await,
        journal_before,
        "the table copy writes nothing to the capture journal"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'library_documents' \
             AND sql LIKE '%CHECK (category IN%'"
        )
        .await,
        0
    );

    // A document arriving by sync from a device that has a plant category this one does not.
    let mut payload =
        crate::sync::test_support::current_payload(&db, "library_documents", "lg-doc-1", &Default::default())
            .await
            .expect("payload")
            .expect("row");
    payload.insert("category".into(), "DRAWINGS".into());
    payload.insert("relative_path".into(), "library_documents/DRAWINGS/plan.pdf".into());
    let outcome = crate::sync::materialize::apply_inbound_payload(
        &db,
        "library_documents",
        "lg-doc-sync",
        "upsert",
        1,
        &serde_json::Value::Object(payload).to_string(),
    )
    .await
    .expect("apply");
    assert!(
        matches!(outcome, crate::sync::materialize::ApplyOutcome::Applied),
        "{outcome:?}"
    );
    assert_eq!(
        texts(
            &db,
            "SELECT category FROM library_documents WHERE entity_sync_id = 'lg-doc-sync'"
        )
        .await,
        ["DRAWINGS"],
        "stored as sent, never lost"
    );
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("integrity pass");
    let categories = crate::library_documents::list_document_categories(&db)
        .await
        .expect("categories");
    let drawings = categories.iter().find(|c| c.code == "DRAWINGS").expect("adopted");
    assert!(drawings.review_required, "flagged à revoir");
    assert_eq!(
        crate::library_documents::list_library_documents(&db, Some("DRAWINGS".into()), None)
            .await
            .expect("list")
            .len(),
        1,
        "listed under its category"
    );
}

/// Codes were compared EXACTLY before the lists: `Leave`, `TRAINING`, `Vacation` showed the person
/// blocked. They keep that state, each case variant is its own flagged value labelled verbatim,
/// and a variant arriving later (by sync, after `vacation`) behaves the same once adopted.
#[tokio::test]
async fn case_variants_keep_their_former_state() {
    let db = migrated_before("m20261017_000228_personnel_document_lists").await;
    for (code, block) in [
        ("CV-L", "Leave"),
        ("CV-T", "TRAINING"),
        ("CV-V", "Vacation"),
        ("CV-v", "vacation"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO personnel (employee_code, full_name, employment_type, availability_status) \
                 VALUES ('{code}', 'Person {code}', 'employee', 'available')"
            ),
        )
        .await;
        exec(
            &db,
            &format!(
                "INSERT INTO personnel_availability_blocks (personnel_id, block_type, start_at, end_at, is_critical) \
                 SELECT id, '{block}', '2000-01-01T00:00:00Z', '2100-01-01T00:00:00Z', 0 \
                   FROM personnel WHERE employee_code = '{code}'"
            ),
        )
        .await;
    }
    Migrator::up(&db, None).await.expect("migrations");
    for (code, former) in [
        ("CV-L", "blocked"),
        ("CV-T", "blocked"),
        ("CV-V", "blocked"),
        ("CV-v", "on_leave"),
    ] {
        assert_eq!(state_of(&db, code).await, former, "{code}");
    }
    let o = lists::list_personnel_reference_options(&db).await.expect("options");
    for (code, tag) in [
        ("Leave", "critical"),
        ("TRAINING", "critical"),
        ("Vacation", "critical"),
        ("vacation", "leave"),
    ] {
        let v = find(&o.absence_types, code);
        assert!(v.review_required, "{code} flagged");
        assert_eq!(v.label, code, "labelled verbatim");
        assert_eq!(v.tag.as_deref(), Some(tag), "{code}");
    }
    // A variant received later by sync, after `vacation` was adopted.
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, employment_type, availability_status) \
         VALUES ('CV-S', 'Synced', 'employee', 'available')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO personnel_availability_blocks (personnel_id, block_type, start_at, end_at, is_critical) \
         SELECT id, 'VACATION', '2000-01-01T00:00:00Z', '2100-01-01T00:00:00Z', 0 FROM personnel WHERE employee_code = 'CV-S'",
    )
    .await;
    assert_eq!(state_of(&db, "CV-S").await, "blocked", "before adoption: as before");
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("integrity");
    assert_eq!(state_of(&db, "CV-S").await, "blocked", "after adoption: unchanged");
    let o = lists::list_personnel_reference_options(&db).await.expect("options");
    assert!(find(&o.absence_types, "VACATION").review_required);
}

/// Running the adoption again (migration 229 twice, the integrity pass on every start) writes
/// nothing: no new list value, nothing for sync to send.
#[tokio::test]
async fn a_second_run_writes_nothing() {
    let db = migrated_before("m20261017_000228_personnel_document_lists").await;
    seed_legacy_people(&db).await;
    Migrator::up(&db, None).await.expect("migrations");
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("first start");
    crate::sync::capture::install_capture_in(&db).await.expect("capture");
    let values = "SELECT COUNT(*) FROM reference_values";
    let journal = "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type IN ('reference_values', 'reference_domains', 'reference_sets')";
    let (values_before, journal_before) = (scalar(&db, values).await, scalar(&db, journal).await);

    // Migration 229's body again, then a second start.
    lists::ensure_personnel_reference_lists(&db).await.expect("lists");
    crate::library_documents::ensure_document_categories(&db)
        .await
        .expect("categories");
    lists::adopt_stored_personnel_values(&db).await.expect("adopt");
    crate::library_documents::adopt_stored_document_categories(&db)
        .await
        .expect("adopt docs");
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("second start");

    assert_eq!(scalar(&db, values).await, values_before, "no new list value");
    assert_eq!(scalar(&db, journal).await, journal_before, "nothing captured for sync");
}
