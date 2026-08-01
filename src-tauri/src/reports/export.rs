use anyhow::anyhow;
use sea_orm::DatabaseConnection;

use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reports::queries::{fetch_dashboard_summary_pairs, fetch_open_wo_by_status};

fn xlsx_e(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(anyhow!("xlsx: {e}"))
}

fn pdf_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn build_simple_pdf(lines: &[String], paper_size: &str) -> Vec<u8> {
    let (w, h) = if paper_size.eq_ignore_ascii_case("A3") {
        (1191, 842)
    } else {
        (842, 595)
    };
    let mut content = String::from("BT /F1 11 Tf 40 ");
    content.push_str(&(h - 50).to_string());
    content.push_str(" Td ");
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            content.push_str("T* ");
        }
        content.push('(');
        content.push_str(&pdf_escape(line));
        content.push_str(") Tj ");
    }
    content.push_str("ET");
    let stream = content.into_bytes();

    let mut out = Vec::<u8>::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = vec![0_usize];
    let push_obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\n");
    };

    push_obj(
        &mut out,
        &mut offsets,
        b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj",
    );
    push_obj(
        &mut out,
        &mut offsets,
        b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj",
    );
    let page_obj = format!(
        "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj"
    );
    push_obj(&mut out, &mut offsets, page_obj.as_bytes());
    let stream_head = format!("4 0 obj << /Length {} >> stream\n", stream.len());
    offsets.push(out.len());
    out.extend_from_slice(stream_head.as_bytes());
    out.extend_from_slice(&stream);
    out.extend_from_slice(b"\nendstream endobj\n");
    push_obj(
        &mut out,
        &mut offsets,
        b"5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj",
    );

    let xref_pos = out.len();
    let object_count = offsets.len();
    out.extend_from_slice(format!("xref\n0 {}\n", object_count).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            object_count, xref_pos
        )
        .as_bytes(),
    );
    out
}

pub async fn export_report_document(
    db: &DatabaseConnection,
    template_code: &str,
    export_format: &str,
) -> AppResult<ExportedBinaryDocument> {
    let fmt = export_format.to_ascii_lowercase();
    if fmt != "pdf" && fmt != "xlsx" {
        return Err(AppError::ValidationFailed(vec!["export_format must be pdf or xlsx.".into()]));
    }

    match template_code {
        "dashboard_summary" => export_dashboard_summary(db, &fmt).await,
        "open_work_orders" => export_open_work_orders(db, &fmt).await,
        _ => Err(AppError::ValidationFailed(vec![format!("Unknown template: {template_code}")]))
    }
}

async fn export_dashboard_summary(db: &DatabaseConnection, fmt: &str) -> AppResult<ExportedBinaryDocument> {
    let pairs = fetch_dashboard_summary_pairs(db).await?;
    if fmt == "pdf" {
        let mut lines = vec![
            "Maintafox — Dashboard summary".to_string(),
            chrono::Utc::now().to_rfc3339(),
        ];
        for (k, v) in &pairs {
            lines.push(format!("{k}: {v}"));
        }
        let bytes = build_simple_pdf(&lines, "A4");
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
        let mut lines = vec![
            "Maintafox — Open work orders by status".to_string(),
            chrono::Utc::now().to_rfc3339(),
        ];
        for (st, n) in &rows {
            lines.push(format!("{st}: {n}"));
        }
        let bytes = build_simple_pdf(&lines, "A4");
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
