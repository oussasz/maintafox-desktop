use anyhow::anyhow;
use sea_orm::DatabaseConnection;

use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reports::queries::{fetch_dashboard_summary, fetch_open_wo_by_status, DashboardSummaryFigures};

fn xlsx_e(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(anyhow!("xlsx: {e}"))
}

/// Language of a generated report (titles, column headers, labels, number format).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportLocale {
    Fr,
    En,
}

impl ReportLocale {
    /// `"en"` (any case, region suffix allowed) is English; everything else is French, the
    /// product default (`crate::locale::DEFAULT_LOCALE`).
    pub fn from_code(code: &str) -> Self {
        let base = code.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        if base == "en" {
            Self::En
        } else {
            Self::Fr
        }
    }

    /// The app language when the caller does not pass one (scheduled runs): user preference,
    /// then tenant default, then OS (`crate::locale::resolve_locale_preference`).
    pub async fn resolve(db: &DatabaseConnection, requested: Option<&str>) -> Self {
        if let Some(code) = requested.filter(|c| !c.trim().is_empty()) {
            return Self::from_code(code);
        }
        match crate::locale::resolve_locale_preference(db).await {
            Ok(pref) => Self::from_code(&pref.active_locale),
            Err(_) => Self::from_code(crate::locale::DEFAULT_LOCALE),
        }
    }

    pub(crate) fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Fr => fr,
            Self::En => en,
        }
    }

    /// Same as [`Self::pick`] for labels read from data (reference lists).
    pub(crate) fn pick_owned(self, fr: &str, en: &str) -> String {
        match self {
            Self::Fr => fr.to_string(),
            Self::En => en.to_string(),
        }
    }

    pub(crate) fn generated_at(self, now: chrono::DateTime<chrono::Utc>) -> String {
        match self {
            Self::Fr => format!("Généré le {} (UTC)", now.format("%d/%m/%Y à %H:%M")),
            Self::En => format!("Generated on {} (UTC)", now.format("%Y-%m-%d at %H:%M")),
        }
    }

    /// Decimal with `decimals` places; French uses a comma separator.
    pub(crate) fn decimal(self, value: f64, decimals: usize) -> String {
        let s = format!("{value:.decimals$}");
        match self {
            Self::Fr => s.replace('.', ","),
            Self::En => s,
        }
    }

    pub(crate) fn percent(self, ratio: f64) -> String {
        match self {
            Self::Fr => format!("{} %", self.decimal(ratio * 100.0, 1)),
            Self::En => format!("{}%", self.decimal(ratio * 100.0, 1)),
        }
    }
}

/// Work-order status code → label in the report language (unknown codes stay readable).
fn wo_status_label(locale: ReportLocale, code: &str) -> String {
    let label = match code {
        "draft" => locale.pick("Brouillon", "Draft"),
        "awaiting_approval" => locale.pick("En attente d'approbation", "Awaiting approval"),
        "planned" => locale.pick("Planifié", "Planned"),
        "ready_to_schedule" => locale.pick("Prêt à programmer", "Ready to schedule"),
        "assigned" => locale.pick("Assigné", "Assigned"),
        "waiting_for_prerequisite" => locale.pick("En attente de prérequis", "Waiting for prerequisite"),
        "in_progress" => locale.pick("En cours", "In progress"),
        "paused" => locale.pick("En pause", "Paused"),
        "mechanically_complete" => locale.pick("Mécaniquement complet", "Mechanically complete"),
        "technically_verified" => locale.pick("Techniquement vérifié", "Technically verified"),
        "closed" => locale.pick("Clôturé", "Closed"),
        "cancelled" => locale.pick("Annulé", "Cancelled"),
        other => {
            let spaced = other.replace('_', " ");
            let mut chars = spaced.chars();
            return match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            };
        }
    };
    label.to_string()
}

/// Rows of the dashboard summary report in the report language.
fn dashboard_summary_rows(locale: ReportLocale, f: &DashboardSummaryFigures) -> Vec<(String, String)> {
    let none = "—".to_string();
    vec![
        (
            locale
                .pick("Demandes d'intervention ouvertes", "Open intervention requests")
                .to_string(),
            f.open_di.to_string(),
        ),
        (
            locale.pick("Ordres de travail ouverts", "Open work orders").to_string(),
            f.open_wo.to_string(),
        ),
        (locale.pick("Équipements", "Assets").to_string(), f.assets.to_string()),
        (
            locale
                .pick(
                    "Équipements avec un instantané de fiabilité (365 j)",
                    "Assets with a reliability snapshot (365 days)",
                )
                .to_string(),
            f.assets_with_snapshots.to_string(),
        ),
        (
            locale
                .pick("Qualité moyenne des données (365 j)", "Average data quality (365 days)")
                .to_string(),
            f.data_quality
                .map(|v| locale.percent(v))
                .unwrap_or_else(|| none.clone()),
        ),
        (
            locale
                .pick("MTBF du parc, regroupé (365 j)", "Fleet MTBF, pooled (365 days)")
                .to_string(),
            f.mtbf_hours
                .map(|v| format!("{} h", locale.decimal(v, 1)))
                .unwrap_or(none),
        ),
    ]
}

/// Two-column report (label, value) on the PDF engine.
fn key_value_pdf(
    locale: ReportLocale,
    title: &str,
    pairs: &[(String, String)],
    headers: [&str; 2],
) -> AppResult<Vec<u8>> {
    let font = crate::reports::pdf::load_system_font()?;
    let mut doc = crate::reports::pdf::PdfDocument::new(&font, crate::reports::pdf::PaperSize::A4Portrait, title)?;
    doc.heading(title);
    doc.paragraph(&locale.generated_at(chrono::Utc::now()));
    let rows: Vec<Vec<String>> = pairs.iter().map(|(k, v)| vec![k.clone(), v.clone()]).collect();
    doc.table(&headers, &rows, &[3.0, 2.0]);
    doc.finish()
}

pub async fn export_report_document(
    db: &DatabaseConnection,
    template_code: &str,
    export_format: &str,
    locale: ReportLocale,
) -> AppResult<ExportedBinaryDocument> {
    let fmt = export_format.to_ascii_lowercase();
    if fmt != "pdf" && fmt != "xlsx" {
        return Err(AppError::ValidationFailed(vec![
            "export_format must be pdf or xlsx.".into()
        ]));
    }

    match template_code {
        "dashboard_summary" => export_dashboard_summary(db, &fmt, locale).await,
        "open_work_orders" => export_open_work_orders(db, &fmt, locale).await,
        crate::reports::monthly::TEMPLATE_CODE => {
            crate::reports::monthly::export_monthly_management(db, &fmt, locale).await
        }
        _ => Err(AppError::ValidationFailed(vec![format!(
            "Unknown template: {template_code}"
        )])),
    }
}

async fn export_dashboard_summary(
    db: &DatabaseConnection,
    fmt: &str,
    locale: ReportLocale,
) -> AppResult<ExportedBinaryDocument> {
    let figures = fetch_dashboard_summary(db).await?;
    let pairs = dashboard_summary_rows(locale, &figures);
    let headers = [locale.pick("Indicateur", "Metric"), locale.pick("Valeur", "Value")];
    let base_name = locale.pick("synthese-tableau-de-bord", "dashboard-summary");
    if fmt == "pdf" {
        let title = locale.pick(
            "Maintafox — Synthèse du tableau de bord",
            "Maintafox — Dashboard summary",
        );
        let bytes = key_value_pdf(locale, title, &pairs, headers)?;
        return Ok(ExportedBinaryDocument {
            file_name: format!("{base_name}.pdf"),
            mime_type: "application/pdf".to_string(),
            bytes,
        });
    }
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, headers[0]).map_err(xlsx_e)?;
    ws.write_string(0, 1, headers[1]).map_err(xlsx_e)?;
    for (i, (k, v)) in pairs.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write_string(r, 0, k).map_err(xlsx_e)?;
        ws.write_string(r, 1, v).map_err(xlsx_e)?;
    }
    let bytes = wb.save_to_buffer().map_err(xlsx_e)?;
    Ok(ExportedBinaryDocument {
        file_name: format!("{base_name}.xlsx"),
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        bytes,
    })
}

async fn export_open_work_orders(
    db: &DatabaseConnection,
    fmt: &str,
    locale: ReportLocale,
) -> AppResult<ExportedBinaryDocument> {
    let rows = fetch_open_wo_by_status(db).await?;
    let headers = [locale.pick("Statut", "Status"), locale.pick("Nombre", "Count")];
    let base_name = locale.pick("ot-ouverts", "open-work-orders");
    if fmt == "pdf" {
        let pairs: Vec<(String, String)> = rows
            .iter()
            .map(|(st, n)| (wo_status_label(locale, st), n.to_string()))
            .collect();
        let title = locale.pick(
            "Maintafox — Ordres de travail ouverts par statut",
            "Maintafox — Open work orders by status",
        );
        let bytes = key_value_pdf(locale, title, &pairs, headers)?;
        return Ok(ExportedBinaryDocument {
            file_name: format!("{base_name}.pdf"),
            mime_type: "application/pdf".to_string(),
            bytes,
        });
    }
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, headers[0]).map_err(xlsx_e)?;
    ws.write_string(0, 1, headers[1]).map_err(xlsx_e)?;
    for (i, (st, n)) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write_string(r, 0, wo_status_label(locale, st)).map_err(xlsx_e)?;
        ws.write_number(r, 1, *n as f64).map_err(xlsx_e)?;
    }
    let bytes = wb.save_to_buffer().map_err(xlsx_e)?;
    Ok(ExportedBinaryDocument {
        file_name: format!("{base_name}.xlsx"),
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_code_parsing_defaults_to_french() {
        assert_eq!(ReportLocale::from_code("en"), ReportLocale::En);
        assert_eq!(ReportLocale::from_code("EN-us"), ReportLocale::En);
        assert_eq!(ReportLocale::from_code("fr"), ReportLocale::Fr);
        assert_eq!(ReportLocale::from_code(""), ReportLocale::Fr);
        assert_eq!(ReportLocale::from_code("ar"), ReportLocale::Fr);
    }

    #[test]
    fn dashboard_summary_rows_are_localised() {
        let figures = DashboardSummaryFigures {
            open_di: 4,
            open_wo: 7,
            assets: 120,
            assets_with_snapshots: 30,
            data_quality: Some(0.875),
            mtbf_hours: Some(412.26),
        };
        let fr = dashboard_summary_rows(ReportLocale::Fr, &figures);
        assert_eq!(fr[0], ("Demandes d'intervention ouvertes".to_string(), "4".to_string()));
        assert_eq!(fr[4].1, "87,5 %");
        assert_eq!(fr[5].1, "412,3 h");
        let en = dashboard_summary_rows(ReportLocale::En, &figures);
        assert_eq!(en[1], ("Open work orders".to_string(), "7".to_string()));
        assert_eq!(en[4].1, "87.5%");
        assert_eq!(en[5].1, "412.3 h");

        let empty = DashboardSummaryFigures {
            data_quality: None,
            mtbf_hours: None,
            ..figures
        };
        assert_eq!(dashboard_summary_rows(ReportLocale::En, &empty)[4].1, "—");
    }

    #[test]
    fn wo_status_codes_never_reach_the_report_raw() {
        assert_eq!(wo_status_label(ReportLocale::Fr, "in_progress"), "En cours");
        assert_eq!(
            wo_status_label(ReportLocale::En, "awaiting_approval"),
            "Awaiting approval"
        );
        assert_eq!(
            wo_status_label(ReportLocale::En, "custom_site_state"),
            "Custom site state"
        );
    }
}
