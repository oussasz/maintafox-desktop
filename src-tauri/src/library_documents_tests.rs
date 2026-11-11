//! Document control fields of the library (L7-18): number, revision, validity date, category.

use super::*;

async fn seeded_user(db: &DatabaseConnection) -> i64 {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts ORDER BY id LIMIT 1".to_string(),
        ))
        .await
        .expect("query")
        .expect("a seeded user");
    row.try_get::<i64>("", "id").expect("id")
}

fn upload(number: Option<&str>, valid_until: Option<&str>, user: i64) -> LibraryDocumentUploadInput {
    LibraryDocumentUploadInput {
        category: "sops".to_string(),
        equipment_id: None,
        title: "Consignation électrique".to_string(),
        file_name: "loto.pdf".to_string(),
        file_bytes: b"%PDF-1.7 test".to_vec(),
        mime_type: "application/pdf".to_string(),
        notes: None,
        uploaded_by_id: user,
        document_number: number.map(str::to_string),
        revision: Some("  B ".to_string()),
        valid_until: valid_until.map(str::to_string),
    }
}

fn refusal(err: &AppError) -> String {
    match err {
        AppError::ValidationFailed(messages) => messages.join(" "),
        other => panic!("expected a validation refusal, got {other:?}"),
    }
}

fn edit(id: i64) -> UpdateLibraryDocumentInput {
    UpdateLibraryDocumentInput {
        id,
        title: None,
        equipment_id: None,
        clear_equipment_link: false,
        category: None,
        document_number: None,
        revision: None,
        valid_until: None,
    }
}

#[tokio::test]
async fn a_document_keeps_its_number_revision_and_validity() {
    let db = crate::db::test_db::seeded().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let user = seeded_user(&db).await;

    let doc = save_library_document(&db, dir.path(), upload(Some(" PR-ELEC-012 "), Some("2027-03-31"), user))
        .await
        .expect("saved");
    assert_eq!(doc.document_number.as_deref(), Some("PR-ELEC-012"));
    assert_eq!(doc.revision.as_deref(), Some("B"));
    assert_eq!(doc.valid_until.as_deref(), Some("2027-03-31"));

    let listed = list_library_documents(&db, Some("sops".to_string()), None)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].valid_until.as_deref(), Some("2027-03-31"));
}

#[tokio::test]
async fn a_wrong_date_or_a_used_number_is_refused_with_its_code() {
    let db = crate::db::test_db::seeded().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let user = seeded_user(&db).await;

    for bad in ["31/03/2027", "2027-02-30", "demain"] {
        let err = save_library_document(&db, dir.path(), upload(None, Some(bad), user))
            .await
            .expect_err("not a date");
        assert!(refusal(&err).starts_with(GATE_VALID_UNTIL_INVALID), "{bad}");
    }

    save_library_document(&db, dir.path(), upload(Some("PR-ELEC-012"), None, user))
        .await
        .expect("first");
    let err = save_library_document(&db, dir.path(), upload(Some("pr-elec-012"), None, user))
        .await
        .expect_err("number used");
    assert!(refusal(&err).starts_with(GATE_NUMBER_TAKEN));
    // Nothing was written for the refused documents.
    assert_eq!(list_library_documents(&db, None, None).await.expect("list").len(), 1);
}

#[tokio::test]
async fn an_edit_changes_category_and_control_fields_and_keeps_what_is_not_sent() {
    let db = crate::db::test_db::seeded().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let user = seeded_user(&db).await;
    let first = save_library_document(&db, dir.path(), upload(Some("PR-1"), Some("2027-01-31"), user))
        .await
        .expect("first");
    let second = save_library_document(&db, dir.path(), upload(Some("PR-2"), None, user))
        .await
        .expect("second");

    // Keeping its own number is not a conflict; the category moves to another list value.
    let moved = update_library_document_metadata(
        &db,
        UpdateLibraryDocumentInput {
            category: Some("safety_protocols".to_string()),
            document_number: Some("PR-1".to_string()),
            revision: Some("C".to_string()),
            ..edit(first.id)
        },
    )
    .await
    .expect("edited");
    assert_eq!(moved.category, "safety_protocols");
    assert_eq!(moved.revision.as_deref(), Some("C"));
    assert_eq!(moved.valid_until.as_deref(), Some("2027-01-31"), "not sent: kept");

    // A blank value clears the field.
    let cleared = update_library_document_metadata(
        &db,
        UpdateLibraryDocumentInput {
            valid_until: Some(String::new()),
            ..edit(first.id)
        },
    )
    .await
    .expect("cleared");
    assert_eq!(cleared.valid_until, None);
    assert_eq!(cleared.document_number.as_deref(), Some("PR-1"));

    // Another document's number, a category outside the list: refused, nothing written.
    let err = update_library_document_metadata(
        &db,
        UpdateLibraryDocumentInput {
            document_number: Some("pr-1".to_string()),
            title: Some("Renamed".to_string()),
            ..edit(second.id)
        },
    )
    .await
    .expect_err("number used");
    assert!(refusal(&err).starts_with(GATE_NUMBER_TAKEN));
    update_library_document_metadata(
        &db,
        UpdateLibraryDocumentInput {
            category: Some("drawings".to_string()),
            title: Some("Renamed".to_string()),
            ..edit(second.id)
        },
    )
    .await
    .expect_err("not a category");
    let unchanged = list_library_documents(&db, Some("sops".to_string()), None)
        .await
        .expect("list");
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0].title, "Consignation électrique");
    assert_eq!(unchanged[0].document_number.as_deref(), Some("PR-2"));
}

/// Two devices may file the same number offline (sync does not enforce the uniqueness): each
/// document stays editable as long as its number is not changed.
#[tokio::test]
async fn a_number_duplicated_by_sync_does_not_block_other_edits() {
    let db = crate::db::test_db::seeded().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let user = seeded_user(&db).await;
    let mine = save_library_document(&db, dir.path(), upload(Some("PR-9"), None, user))
        .await
        .expect("mine");
    // The other device's row, as sync writes it (no check).
    db.execute_unprepared(
        "INSERT INTO library_documents (category, title, file_name, relative_path, entity_sync_id, \
         document_number) VALUES ('sops', 'Theirs', 't.pdf', 'library_documents/sops/u-t.pdf', 'ld-theirs', 'pr-9 ')",
    )
    .await
    .expect("synced duplicate");

    let renamed = update_library_document_metadata(
        &db,
        UpdateLibraryDocumentInput {
            title: Some("Renamed".to_string()),
            document_number: Some("PR-9".to_string()),
            ..edit(mine.id)
        },
    )
    .await
    .expect("own number kept");
    assert_eq!(renamed.title, "Renamed");
}

fn new_version(id: i64, revision: Option<&str>, user: i64) -> LibraryDocumentVersionInput {
    LibraryDocumentVersionInput {
        id,
        file_name: "loto-v2.pdf".to_string(),
        file_bytes: b"%PDF-1.7 v2".to_vec(),
        mime_type: "application/pdf".to_string(),
        notes: Some("Nouvelle étape de vérification".to_string()),
        uploaded_by_id: user,
        revision: revision.map(str::to_string),
        valid_until: Some("2028-06-30".to_string()),
    }
}

/// L7-17: a new version replaces the file; the earlier one stays in the history, read-only.
#[tokio::test]
async fn a_new_version_replaces_the_current_one_and_keeps_the_history() {
    let db = crate::db::test_db::seeded().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let user = seeded_user(&db).await;
    let first = save_library_document(&db, dir.path(), upload(Some("PR-ELEC-012"), Some("2027-01-31"), user))
        .await
        .expect("first");

    // A revision label is required and must change.
    let err = save_library_document_version(&db, dir.path(), new_version(first.id, Some("  "), user))
        .await
        .expect_err("no revision");
    assert!(refusal(&err).starts_with(GATE_REVISION_REQUIRED));
    let err = save_library_document_version(&db, dir.path(), new_version(first.id, Some("b"), user))
        .await
        .expect_err("same revision");
    assert!(refusal(&err).starts_with(GATE_REVISION_UNCHANGED));

    let second = save_library_document_version(&db, dir.path(), new_version(first.id, Some("C"), user))
        .await
        .expect("new version");
    assert_eq!(second.previous_version_id, Some(first.id));
    assert_eq!(second.document_number.as_deref(), Some("PR-ELEC-012"), "number kept");
    assert_eq!(second.title, first.title);
    assert_eq!(second.category, first.category);
    assert_eq!(second.valid_until.as_deref(), Some("2028-06-30"));
    assert_ne!(second.relative_path, first.relative_path, "a new file");

    // The library shows the current version only; the history shows both, newest first.
    let listed = list_library_documents(&db, None, None).await.expect("list");
    assert_eq!(listed.iter().map(|d| d.id).collect::<Vec<_>>(), vec![second.id]);
    let history = list_library_document_versions(&db, second.id).await.expect("history");
    assert_eq!(
        history.iter().map(|d| d.id).collect::<Vec<_>>(),
        vec![second.id, first.id]
    );
    assert!(history[1].superseded_at.is_some());
    // The earlier file is still readable.
    assert_eq!(
        read_library_document_file(&db, dir.path(), first.id)
            .await
            .expect("old file"),
        b"%PDF-1.7 test".to_vec()
    );

    // The earlier version is kept as it is: no edit, no new version from it, no deletion.
    let err = update_library_document_metadata(
        &db,
        UpdateLibraryDocumentInput {
            title: Some("Changed".to_string()),
            ..edit(first.id)
        },
    )
    .await
    .expect_err("earlier version");
    assert!(refusal(&err).starts_with(GATE_NOT_CURRENT));
    let err = save_library_document_version(&db, dir.path(), new_version(first.id, Some("D"), user))
        .await
        .expect_err("earlier version");
    assert!(refusal(&err).starts_with(GATE_NOT_CURRENT));
    let err = delete_library_document_record(&db, first.id)
        .await
        .expect_err("earlier version");
    assert!(refusal(&err).starts_with(GATE_NOT_CURRENT));

    // The number stays the document's own: a new document cannot take it.
    let err = save_library_document(&db, dir.path(), upload(Some("PR-ELEC-012"), None, user))
        .await
        .expect_err("number used");
    assert!(refusal(&err).starts_with(GATE_NUMBER_TAKEN));
}

/// Deleting a version uploaded by mistake makes the replaced one current again.
#[tokio::test]
async fn deleting_the_current_version_restores_the_previous_one() {
    let db = crate::db::test_db::seeded().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let user = seeded_user(&db).await;
    let first = save_library_document(&db, dir.path(), upload(Some("PR-7"), None, user))
        .await
        .expect("first");
    let second = save_library_document_version(&db, dir.path(), new_version(first.id, Some("C"), user))
        .await
        .expect("new version");

    delete_library_document_record(&db, second.id).await.expect("withdrawn");
    let listed = list_library_documents(&db, None, None).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, first.id);
    assert_eq!(listed[0].superseded_at, None);
    assert_eq!(listed[0].revision.as_deref(), Some("B"));
}
