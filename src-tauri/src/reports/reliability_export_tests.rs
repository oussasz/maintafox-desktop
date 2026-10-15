//! Failure-mode Pareto on real failure events, and the reliability report export (PDF + Excel).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::reliability::pareto::{get_failure_mode_pareto, FailureModeParetoInput};
use crate::reports::reliability_export::export_reliability_report;

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

/// Equipment FAN-07 with failures: 3 x "Bearing seizure" (2 h downtime each), 1 x "Belt slip"
/// (10 h), 1 unclassified, 1 planned replacement (excluded); one FMECA item with criticality.
async fn setup() -> (DatabaseConnection, i64) {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 're-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 're-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 're-node', 'SITE-RE', 'Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('re-eq', 'FAN-07', 'Ventilateur de refroidissement', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
        "INSERT INTO failure_codes (entity_sync_id, hierarchy_id, parent_id, code, label, code_type, is_active, \
         row_version) VALUES ('fc:re:bear', (SELECT id FROM failure_hierarchies ORDER BY id LIMIT 1), NULL, \
         'RE_BEAR', 'Bearing seizure', 'mode', 1, 1)",
        "INSERT INTO failure_codes (entity_sync_id, hierarchy_id, parent_id, code, label, code_type, is_active, \
         row_version) VALUES ('fc:re:belt', (SELECT id FROM failure_hierarchies ORDER BY id LIMIT 1), NULL, \
         'RE_BELT', 'Belt slip', 'mode', 1, 1)",
    ] {
        exec(&db, sql).await;
    }
    let eq = scalar(&db, "SELECT id FROM equipment WHERE asset_id_code = 'FAN-07'").await;
    let events = [
        (1, "RE_BEAR", 2.0, 0, "2026-03-01T08:00:00Z"),
        (2, "RE_BEAR", 2.0, 0, "2026-04-01T08:00:00Z"),
        (3, "RE_BEAR", 2.0, 0, "2026-05-01T08:00:00Z"),
        (4, "RE_BELT", 10.0, 0, "2026-05-15T08:00:00Z"),
        (5, "", 1.0, 0, "2026-06-01T08:00:00Z"),
        (6, "RE_BELT", 50.0, 1, "2026-06-10T08:00:00Z"),
    ];
    for (n, mode, dt, planned, at) in events {
        let mode_sql = if mode.is_empty() {
            "NULL".to_string()
        } else {
            format!("(SELECT id FROM failure_codes WHERE code = '{mode}')")
        };
        exec(
            &db,
            &format!(
                "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, failed_at, \
                 downtime_duration_hours, is_planned, failure_mode_id, created_at, updated_at) \
                 VALUES ('re-fe-{n}', 'manual', {n}, {eq}, '{at}', {dt}, {planned}, {mode_sql}, '{at}', '{at}')"
            ),
        )
        .await;
    }
    exec(
        &db,
        &format!(
            "INSERT INTO fmeca_analyses (entity_sync_id, equipment_id, title, created_at, updated_at) \
             VALUES ('re-fa', {eq}, 'AMDEC ventilateur', datetime('now'), datetime('now'))"
        ),
    )
    .await;
    exec(
        &db,
        "INSERT INTO fmeca_items (entity_sync_id, analysis_id, functional_failure, failure_effect, severity, \
         occurrence, detectability, rpn, recommended_action, current_control, criticality_number, row_version, \
         updated_at) VALUES ('re-item', (SELECT id FROM fmeca_analyses), 'Grippage du roulement', \
         'Moteur en surchauffe', 8, 4, 5, 160, 'Surveillance vibratoire', '', 0.00042, 1, datetime('now'))",
    )
    .await;
    (db, eq)
}

#[tokio::test]
async fn pareto_by_count_excludes_planned_and_ranks_modes() {
    let (db, eq) = setup().await;
    let p = get_failure_mode_pareto(
        &db,
        FailureModeParetoInput {
            metric: "count".into(),
            equipment_id: Some(eq),
            ..Default::default()
        },
    )
    .await
    .expect("pareto");
    assert_eq!(p.total, 5.0, "planned replacement is not a failure");
    assert_eq!(p.rows[0].mode_code.as_deref(), Some("RE_BEAR"));
    assert_eq!(p.rows[0].failure_count, 3);
    assert!((p.rows[0].share - 0.6).abs() < 1e-12);
    assert!((p.rows.last().expect("rows").cumulative_share - 1.0).abs() < 1e-12);
    assert!(p
        .rows
        .iter()
        .any(|r| r.failure_mode_id.is_none() && r.failure_count == 1));
}

#[tokio::test]
async fn pareto_by_downtime_and_period_filter() {
    let (db, eq) = setup().await;
    let p = get_failure_mode_pareto(
        &db,
        FailureModeParetoInput {
            metric: "downtime".into(),
            equipment_id: Some(eq),
            ..Default::default()
        },
    )
    .await
    .expect("pareto");
    assert_eq!(p.rows[0].mode_code.as_deref(), Some("RE_BELT"), "10 h beats 3 x 2 h");
    assert!((p.rows[0].value - 10.0).abs() < 1e-12);

    let window = get_failure_mode_pareto(
        &db,
        FailureModeParetoInput {
            metric: "count".into(),
            period_start: Some("2026-03-15T00:00:00Z".into()),
            period_end: Some("2026-05-10T00:00:00Z".into()),
            equipment_id: Some(eq),
        },
    )
    .await
    .expect("pareto");
    assert_eq!(window.total, 2.0);
    assert_eq!(window.rows.len(), 1);
}

#[tokio::test]
async fn pareto_rejects_bad_input() {
    let (db, _) = setup().await;
    for input in [
        FailureModeParetoInput {
            metric: "mtbf".into(),
            ..Default::default()
        },
        FailureModeParetoInput {
            metric: "count".into(),
            period_start: Some("last month".into()),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            get_failure_mode_pareto(&db, input).await,
            Err(AppError::ValidationFailed(_))
        ));
    }
}

#[tokio::test]
async fn reliability_report_exports_excel_with_every_section() {
    use calamine::Reader;
    let (db, eq) = setup().await;
    let doc = export_reliability_report(&db, eq, "xlsx").await.expect("xlsx");
    assert_eq!(doc.file_name, "reliability-FAN-07.xlsx");
    let mut wb: calamine::Xlsx<_> =
        calamine::open_workbook_from_rs(std::io::Cursor::new(doc.bytes)).expect("readable workbook");
    let names = wb.sheet_names();
    assert_eq!(names.len(), 4, "{names:?}");
    let fmeca = wb.worksheet_range(&names[2]).expect("fmeca sheet");
    let cells: Vec<String> = fmeca.cells().map(|(_, _, c)| c.to_string()).collect();
    assert!(cells.iter().any(|c| c == "Grippage du roulement"), "{cells:?}");
    assert!(cells.iter().any(|c| c == "160"));
    let pareto = wb.worksheet_range(&names[3]).expect("pareto sheet");
    let cells: Vec<String> = pareto.cells().map(|(_, _, c)| c.to_string()).collect();
    assert!(cells.iter().any(|c| c == "Bearing seizure"), "{cells:?}");
}

#[tokio::test]
async fn reliability_report_pdf_or_clear_font_error() {
    let (db, eq) = setup().await;
    match export_reliability_report(&db, eq, "pdf").await {
        Ok(doc) => {
            assert!(doc.bytes.starts_with(b"%PDF-"));
            assert_eq!(doc.mime_type, "application/pdf");
        }
        Err(AppError::ValidationFailed(m)) => assert!(m[0].contains("No embeddable TrueType font"), "{m:?}"),
        Err(e) => panic!("unexpected {e:?}"),
    }
}

#[tokio::test]
async fn reliability_report_rejects_unknown_equipment_and_format() {
    let (db, eq) = setup().await;
    assert!(matches!(
        export_reliability_report(&db, eq, "docx").await,
        Err(AppError::ValidationFailed(_))
    ));
    assert!(matches!(
        export_reliability_report(&db, 99_999, "xlsx").await,
        Err(AppError::NotFound { .. })
    ));
}
