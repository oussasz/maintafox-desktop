use anyhow::anyhow;
use sea_orm::DatabaseConnection;

use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reports::queries::{fetch_dashboard_summary_pairs, fetch_open_wo_by_status};

fn xlsx_e(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(anyhow!("xlsx: {e}"))
}

/// Two-column report (label, value) on the PDF engine.
fn key_value_pdf(title: &str, pairs: &[(String, String)], headers: [&str; 2]) -> AppResult<Vec<u8>> {
    let font = crate::reports::pdf::load_system_font()?;
    let mut doc = crate::reports::pdf::PdfDocument::new(&font, crate::reports::pdf::PaperSize::A4Portrait, title)?;
    doc.heading(title);
    doc.paragraph(&chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string());
    let rows: Vec<Vec<String>> = pairs.iter().map(|(k, v)| vec![k.clone(), v.clone()]).collect();
    doc.table(&headers, &rows, &[3.0, 2.0]);
    doc.finish()
}

pub async fn export_report_document(
    db: &DatabaseConnection,
    template_code: &str,
    export_format: &str,
) -> AppResult<ExportedBinaryDocument> {
    let fmt = export_format.to_ascii_lowercase();
    if fmt != "pdf" && fmt != "xlsx" {
        return Err(AppError::ValidationFailed(vec![
            "export_format must be pdf or xlsx.".into()
        ]));
    }

    match template_code {
        "dashboard_summary" => export_dashboard_summary(db, &fmt).await,
        "open_work_orders" => export_open_work_orders(db, &fmt).await,
        _ => Err(AppError::ValidationFailed(vec![format!(
            "Unknown template: {template_code}"
        )])),
    }
}

async fn export_dashboard_summary(db: &DatabaseConnection, fmt: &str) -> AppResult<ExportedBinaryDocument> {
    let pairs = fetch_dashboard_summary_pairs(db).await?;
    if fmt == "pdf" {
        let bytes = key_value_pdf("Maintafox — Dashboard summary", &pairs, ["Metric", "Value"])?;
        return Ok(ExportedBinaryDocument {
            file_name: "dashboard-summary.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            bytes,
        });
    }
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "Metric").map_err(xlsx_e)?;
    ws.write_string(0, 1, "Value").map_err(xlsx_e)?;
    for (i, (k, v)) in pairs.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write_string(r, 0, k).map_err(xlsx_e)?;
        ws.write_string(r, 1, v).map_err(xlsx_e)?;
    }
    let bytes = wb.save_to_buffer().map_err(xlsx_e)?;
    Ok(ExportedBinaryDocument {
        file_name: "dashboard-summary.xlsx".to_string(),
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        bytes,
    })
}

async fn export_open_work_orders(db: &DatabaseConnection, fmt: &str) -> AppResult<ExportedBinaryDocument> {
    let rows = fetch_open_wo_by_status(db).await?;
    if fmt == "pdf" {
        let pairs: Vec<(String, String)> = rows.iter().map(|(st, n)| (st.clone(), n.to_string())).collect();
        let bytes = key_value_pdf("Maintafox — Open work orders by status", &pairs, ["Status", "Count"])?;
        return Ok(ExportedBinaryDocument {
            file_name: "open-work-orders.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            bytes,
        });
    }
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "Status").map_err(xlsx_e)?;
    ws.write_string(0, 1, "Count").map_err(xlsx_e)?;
    for (i, (st, n)) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write_string(r, 0, st).map_err(xlsx_e)?;
        ws.write_number(r, 1, *n as f64).map_err(xlsx_e)?;
    }
    let bytes = wb.save_to_buffer().map_err(xlsx_e)?;
    Ok(ExportedBinaryDocument {
        file_name: "open-work-orders.xlsx".to_string(),
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        bytes,
    })
}
