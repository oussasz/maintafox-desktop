//! Reliability report of one equipment (Phase 6.6): latest KPI snapshot, latest Weibull fit,
//! FMECA items with RPN and criticality, and the Pareto of failure modes, as PDF (with charts)
//! or Excel (one sheet per section).

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reliability::pareto::{get_failure_mode_pareto, FailureModeParetoInput};
use crate::reports::pdf::{format_number, load_system_font, PaperSize, PdfDocument};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("reliability export decode '{field}': {err}"))
}

fn xlsx_e(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(anyhow::anyhow!("xlsx: {e}"))
}

fn opt(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_string(), format_number)
}

struct Section {
    title: &'static str,
    headers: Vec<&'static str>,
    rows: Vec<Vec<String>>,
    weights: Vec<f32>,
}

async fn load_sections(db: &impl ConnectionTrait, equipment_id: i64) -> AppResult<(String, Vec<Section>)> {
    let eq = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT asset_id_code, name FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Equipment".into(),
            id: equipment_id.to_string(),
        })?;
    let code: String = eq
        .try_get("", "asset_id_code")
        .map_err(|e| decode_err("asset_id_code", e))?;
    let name: String = eq.try_get("", "name").map_err(|e| decode_err("name", e))?;

    let mut sections = Vec::new();
    let kpi = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT period_start, period_end, mtbf, mttr, mttf, availability_inherent, availability_operational, \
             failure_rate, event_count, data_quality_score, algorithm_version FROM reliability_kpi_snapshots \
             WHERE equipment_id = ? ORDER BY period_end DESC, id DESC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;
    let mut kpi_rows = Vec::new();
    if let Some(r) = kpi {
        let f = |c: &str| -> AppResult<Option<f64>> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        let s = |c: &str| -> AppResult<Option<String>> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        let events: i64 = r.try_get("", "event_count").map_err(|e| decode_err("event_count", e))?;
        kpi_rows.push(vec![
            "Period".into(),
            format!(
                "{} → {}",
                s("period_start")?.unwrap_or_default(),
                s("period_end")?.unwrap_or_default()
            ),
        ]);
        kpi_rows.push(vec!["MTBF (h)".into(), opt(f("mtbf")?)]);
        kpi_rows.push(vec!["MTTF (h)".into(), opt(f("mttf")?)]);
        kpi_rows.push(vec!["MTTR (h)".into(), opt(f("mttr")?)]);
        kpi_rows.push(vec!["A_i".into(), opt(f("availability_inherent")?)]);
        kpi_rows.push(vec!["A_o".into(), opt(f("availability_operational")?)]);
        kpi_rows.push(vec!["λ (failures / h)".into(), opt(f("failure_rate")?)]);
        kpi_rows.push(vec!["Eligible failures".into(), events.to_string()]);
        kpi_rows.push(vec!["Data quality".into(), opt(f("data_quality_score")?)]);
        kpi_rows.push(vec![
            "Algorithm".into(),
            s("algorithm_version")?.unwrap_or_else(|| "—".into()),
        ]);
    }
    sections.push(Section {
        title: "Reliability KPIs (latest snapshot)",
        headers: vec!["Indicator", "Value"],
        rows: kpi_rows,
        weights: vec![2.0, 3.0],
    });

    let wb = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT period_start, period_end, beta, eta, beta_ci_low, beta_ci_high, eta_ci_low, eta_ci_high, \
             adequate_sample, n_failures, n_censored, algorithm_version, time_base, message \
             FROM weibull_fit_results WHERE equipment_id = ? ORDER BY created_at DESC, id DESC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;
    let mut wb_rows = Vec::new();
    if let Some(r) = wb {
        let f = |c: &str| -> AppResult<Option<f64>> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        let i = |c: &str| -> AppResult<Option<i64>> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        let s = |c: &str| -> AppResult<Option<String>> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        wb_rows.push(vec![
            "β (shape)".into(),
            format!(
                "{} [{} ; {}]",
                opt(f("beta")?),
                opt(f("beta_ci_low")?),
                opt(f("beta_ci_high")?)
            ),
        ]);
        wb_rows.push(vec![
            "η (scale, h)".into(),
            format!(
                "{} [{} ; {}]",
                opt(f("eta")?),
                opt(f("eta_ci_low")?),
                opt(f("eta_ci_high")?)
            ),
        ]);
        wb_rows.push(vec![
            "Failures / censored".into(),
            format!(
                "{} / {}",
                i("n_failures")?.map_or("—".into(), |v| v.to_string()),
                i("n_censored")?.map_or("—".into(), |v| v.to_string())
            ),
        ]);
        wb_rows.push(vec![
            "Adequate sample".into(),
            if i("adequate_sample")?.unwrap_or(0) != 0 {
                "yes".into()
            } else {
                "no".into()
            },
        ]);
        wb_rows.push(vec!["Time base".into(), s("time_base")?.unwrap_or_else(|| "—".into())]);
        wb_rows.push(vec![
            "Algorithm".into(),
            s("algorithm_version")?.unwrap_or_else(|| "—".into()),
        ]);
        if let Some(m) = s("message")?.filter(|m| !m.is_empty()) {
            wb_rows.push(vec!["Note".into(), m]);
        }
    }
    sections.push(Section {
        title: "Weibull fit (latest, 95 % confidence)",
        headers: vec!["Parameter", "Value"],
        rows: wb_rows,
        weights: vec![2.0, 3.0],
    });

    let items = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.title, i.functional_failure, i.failure_effect, i.severity, i.occurrence, i.detectability, \
             i.rpn, i.criticality_number, i.recommended_action FROM fmeca_items i \
             JOIN fmeca_analyses a ON a.id = i.analysis_id WHERE a.equipment_id = ? \
             AND i.review_required = 0 ORDER BY i.rpn DESC, i.id",
            [equipment_id.into()],
        ))
        .await?;
    let mut fm_rows = Vec::with_capacity(items.len());
    for r in &items {
        let s = |c: &str| -> AppResult<String> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        let n = |c: &str| -> AppResult<i64> { r.try_get("", c).map_err(|e| decode_err(c, e)) };
        let cm: Option<f64> = r
            .try_get("", "criticality_number")
            .map_err(|e| decode_err("criticality_number", e))?;
        fm_rows.push(vec![
            s("title")?,
            s("functional_failure")?,
            s("failure_effect")?,
            format!("{}/{}/{}", n("severity")?, n("occurrence")?, n("detectability")?),
            n("rpn")?.to_string(),
            opt(cm),
            s("recommended_action")?,
        ]);
    }
    // Rows whose ratings nobody chose (migration 221) are not ranked: say how many were left out.
    let to_review: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM fmeca_items i JOIN fmeca_analyses a ON a.id = i.analysis_id \
             WHERE a.equipment_id = ? AND i.review_required = 1",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("fmeca rows to review count returned no row")))?
        .try_get("", "n")
        .map_err(|e| decode_err("n", e))?;
    if to_review > 0 {
        fm_rows.push(vec![
            format!("{to_review} row(s) to review not counted (ratings not yet chosen)"),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
    }
    sections.push(Section {
        title: "FMECA (by RPN)",
        headers: vec![
            "Analysis",
            "Functional failure",
            "Effect",
            "S/O/D",
            "RPN",
            "Cm",
            "Action",
        ],
        rows: fm_rows,
        weights: vec![1.4, 2.0, 2.0, 0.8, 0.6, 0.8, 2.2],
    });
    Ok((format!("{code} — {name}"), sections))
}

pub async fn export_reliability_report(
    db: &impl ConnectionTrait,
    equipment_id: i64,
    export_format: &str,
) -> AppResult<ExportedBinaryDocument> {
    let fmt = export_format.to_ascii_lowercase();
    if fmt != "pdf" && fmt != "xlsx" {
        return Err(AppError::ValidationFailed(vec![
            "export_format must be pdf or xlsx.".into()
        ]));
    }
    let (equipment_label, sections) = load_sections(db, equipment_id).await?;
    let pareto = get_failure_mode_pareto(
        db,
        FailureModeParetoInput {
            metric: "count".into(),
            equipment_id: Some(equipment_id),
            ..Default::default()
        },
    )
    .await?;
    let pareto_rows: Vec<Vec<String>> = pareto
        .rows
        .iter()
        .map(|r| {
            vec![
                r.mode_label.clone().unwrap_or_else(|| "(unclassified)".into()),
                r.failure_count.to_string(),
                format_number(r.total_cost),
                format_number(r.downtime_hours),
                format!("{} %", format_number(r.cumulative_share * 100.0)),
            ]
        })
        .collect();
    let slug: String = equipment_label
        .chars()
        .take_while(|c| *c != ' ')
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();

    if fmt == "pdf" {
        let font = load_system_font()?;
        let mut doc = PdfDocument::new(&font, PaperSize::A4Portrait, "Maintafox — Reliability report")?;
        doc.heading(&format!("Reliability report — {equipment_label}"));
        doc.paragraph(&chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string());
        for s in &sections {
            doc.subheading(s.title);
            if s.rows.is_empty() {
                doc.paragraph("No data.");
            } else {
                doc.table(&s.headers, &s.rows, &s.weights);
            }
        }
        if pareto.rows.is_empty() {
            doc.subheading("Pareto of failure modes");
            doc.paragraph("No unplanned failure recorded.");
        } else {
            let items: Vec<(String, f64)> = pareto
                .rows
                .iter()
                .map(|r| (r.mode_label.clone().unwrap_or_else(|| "(unclassified)".into()), r.value))
                .collect();
            doc.bar_chart("Pareto of failure modes (count, cumulative line)", &items, "", true);
            doc.table(
                &["Mode", "Failures", "Cost", "Downtime (h)", "Cumulative"],
                &pareto_rows,
                &[3.0, 1.0, 1.2, 1.2, 1.0],
            );
        }
        return Ok(ExportedBinaryDocument {
            file_name: format!("reliability-{slug}.pdf"),
            mime_type: "application/pdf".into(),
            bytes: doc.finish()?,
        });
    }

    let mut wb = rust_xlsxwriter::Workbook::new();
    let mut all = sections;
    all.push(Section {
        title: "Pareto",
        headers: vec!["Mode", "Failures", "Cost", "Downtime (h)", "Cumulative"],
        rows: pareto_rows,
        weights: vec![],
    });
    for s in &all {
        let ws = wb.add_worksheet();
        let name: String = s
            .title
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
            .take(28)
            .collect();
        ws.set_name(name.trim()).map_err(xlsx_e)?;
        for (c, h) in s.headers.iter().enumerate() {
            ws.write_string(0, c as u16, *h).map_err(xlsx_e)?;
        }
        for (r, row) in s.rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string((r + 1) as u32, c as u16, v).map_err(xlsx_e)?;
            }
        }
    }
    Ok(ExportedBinaryDocument {
        file_name: format!("reliability-{slug}.xlsx"),
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        bytes: wb.save_to_buffer().map_err(xlsx_e)?,
    })
}

#[cfg(test)]
#[path = "reliability_export_tests.rs"]
mod tests;
