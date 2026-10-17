//! Monthly management report (`monthly_management`): fleet reliability, SMRP indicators, top bad
//! actors and maintenance cost for the last complete calendar month, under the company document
//! header configured for prints (`documents.default_header`, text only — the PDF engine draws no
//! images).

use anyhow::anyhow;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TryGetable};

use crate::analytics::as_of::instant;
use crate::analytics::projection::{load_bad_actors, AnalyticsBadActorAsset};
use crate::analytics::smrp::{build_smrp_kpis, SmrpKpis, SmrpRatio, SmrpWindow};
use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reliability::fleet::{pooled_fleet_kpis, FleetPooledKpis, FleetWindow};
use crate::reliability::readiness::DQ_GREEN_THRESHOLD;
use crate::reports::export::ReportLocale;

pub const TEMPLATE_CODE: &str = "monthly_management";

fn xlsx_e(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(anyhow!("xlsx: {e}"))
}

/// The last complete calendar month (UTC) before `now`: `[first day 00:00, first day of next 00:00)`.
pub fn previous_month(now: DateTime<Utc>) -> (SmrpWindow, i32, u32) {
    let this_month = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).expect("first of month");
    let (y, m) = if now.month() == 1 {
        (now.year() - 1, 12)
    } else {
        (now.year(), now.month() - 1)
    };
    let first = NaiveDate::from_ymd_opt(y, m, 1).expect("first of previous month");
    let at = |d: NaiveDate| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"));
    (
        SmrpWindow {
            from: at(first),
            to: at(this_month),
        },
        y,
        m,
    )
}

fn month_name(locale: ReportLocale, year: i32, month: u32) -> String {
    const FR: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    const EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let i = (month.clamp(1, 12) - 1) as usize;
    match locale {
        ReportLocale::Fr => format!("{} {year}", FR[i]),
        ReportLocale::En => format!("{} {year}", EN[i]),
    }
}

/// Plain text lines of the company document header (HTML stored by the header editor).
pub fn header_lines(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(bytes.len());
            let tag = &lower[i..end];
            let breaks = ["<br", "</p", "</div", "</h1", "</h2", "</h3", "</h4", "</li", "</tr"];
            if breaks.iter().any(|b| tag.starts_with(b)) {
                text.push('\n');
            } else if tag.starts_with("</td") || tag.starts_with("</th") {
                text.push_str("  ");
            }
            i = end;
        } else {
            let next = html[i..].find('<').map(|e| i + e).unwrap_or(bytes.len());
            text.push_str(&html[i..next]);
            i = next;
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    decoded
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

async fn setting_json(db: &DatabaseConnection, key: &str) -> Option<serde_json::Value> {
    let s = crate::settings::get_setting(db, key, "tenant").await.ok().flatten()?;
    serde_json::from_str(&s.setting_value_json).ok()
}

async fn company_header(db: &DatabaseConnection) -> Vec<String> {
    setting_json(db, "documents.default_header")
        .await
        .and_then(|v| v.get("html").and_then(|h| h.as_str()).map(header_lines))
        .unwrap_or_default()
}

async fn tenant_currency(db: &DatabaseConnection) -> String {
    setting_json(db, "locale.currency_code")
        .await
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Thousands-grouped amount with two decimals and the currency code.
fn money(locale: ReportLocale, value: f64, currency: &str) -> String {
    let neg = value < 0.0;
    let cents = (value.abs() * 100.0).round() as u64;
    let (int, frac) = (cents / 100, cents % 100);
    let digits = int.to_string();
    let sep = match locale {
        ReportLocale::Fr => ' ',
        ReportLocale::En => ',',
    };
    let mut grouped = String::new();
    for (k, ch) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            grouped.push(sep);
        }
        grouped.push(ch);
    }
    let dec = match locale {
        ReportLocale::Fr => ',',
        ReportLocale::En => '.',
    };
    let sign = if neg { "-" } else { "" };
    format!("{sign}{grouped}{dec}{frac:02} {currency}")
        .trim_end()
        .to_string()
}

pub struct CostLine {
    pub label: String,
    pub amount: f64,
    pub currency: String,
}

/// Posted budget actuals of the month by cost centre (`budget_actuals`, `posting_status = posted`).
async fn cost_by_cost_centre(db: &DatabaseConnection, w: SmrpWindow) -> AppResult<Vec<CostLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cc.code AS code, cc.name AS name, SUM(ba.amount_base) AS total, MAX(ba.base_currency) AS cur \
             FROM budget_actuals ba JOIN cost_centers cc ON cc.id = ba.cost_center_id \
             WHERE ba.posting_status = 'posted' \
               AND datetime(ba.event_at) >= datetime(?) AND datetime(ba.event_at) < datetime(?) \
             GROUP BY cc.id ORDER BY total DESC",
            vec![instant(w.from).into(), instant(w.to).into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .map(|r| CostLine {
            label: format!(
                "{} — {}",
                String::try_get(r, "", "code").unwrap_or_default(),
                String::try_get(r, "", "name").unwrap_or_default()
            ),
            amount: f64::try_get(r, "", "total").unwrap_or(0.0),
            currency: String::try_get(r, "", "cur").unwrap_or_default(),
        })
        .collect())
}

/// Cost of work orders closed in the month, by work-order type: `(label FR, label EN, total, count)`,
/// the labels read from the OT type list (`WORK.ORDER_TYPES`).
async fn cost_by_wo_type(db: &DatabaseConnection, w: SmrpWindow) -> AppResult<Vec<(String, String, f64, i64)>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.label AS label_fr, \
                    COALESCE(json_extract(t.metadata_json, '$.label_en'), t.label) AS label_en, \
                    COALESCE(SUM(wo.total_cost), 0) AS total, COUNT(*) AS n \
             FROM work_orders wo JOIN reference_values t ON t.id = wo.type_id \
             WHERE wo.closed_at IS NOT NULL \
               AND datetime(wo.closed_at) >= datetime(?) AND datetime(wo.closed_at) < datetime(?) \
             GROUP BY t.id ORDER BY total DESC",
            vec![instant(w.from).into(), instant(w.to).into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                String::try_get(r, "", "label_fr").unwrap_or_default(),
                String::try_get(r, "", "label_en").unwrap_or_default(),
                f64::try_get(r, "", "total").unwrap_or(0.0),
                i64::try_get(r, "", "n").unwrap_or(0),
            )
        })
        .collect())
}

/// Everything the report shows, gathered once for PDF and XLSX.
pub struct MonthlyData {
    pub title: String,
    pub month: String,
    pub header: Vec<String>,
    pub currency: String,
    pub fleet: FleetPooledKpis,
    pub smrp: SmrpKpis,
    pub bad_actors: Vec<AnalyticsBadActorAsset>,
    pub cost_centres: Vec<CostLine>,
    pub cost_by_type: Vec<(String, String, f64, i64)>,
}

pub async fn gather(db: &DatabaseConnection, locale: ReportLocale, now: DateTime<Utc>) -> AppResult<MonthlyData> {
    let (window, year, month) = previous_month(now);
    let days_ago = |d: DateTime<Utc>| ((now - d).num_seconds() as f64 / 86_400.0).ceil() as i64;
    let fleet = pooled_fleet_kpis(
        db,
        FleetWindow {
            since_days: days_ago(window.from),
            until_days: Some(days_ago(window.to).max(0)),
        },
        None,
        DQ_GREEN_THRESHOLD,
    )
    .await?;
    let smrp = build_smrp_kpis(db, window, now, None, true, true).await?;
    let bad_actors = load_bad_actors(db, window.from, window.to, None, true, 5).await?;
    Ok(MonthlyData {
        title: locale
            .pick(
                "Maintafox — Rapport mensuel de maintenance",
                "Maintafox — Monthly maintenance report",
            )
            .to_string(),
        month: month_name(locale, year, month),
        header: company_header(db).await,
        currency: tenant_currency(db).await,
        fleet,
        smrp,
        bad_actors,
        cost_centres: cost_by_cost_centre(db, window).await?,
        cost_by_type: cost_by_wo_type(db, window).await?,
    })
}

fn pct(locale: ReportLocale, r: Option<f64>) -> String {
    r.map(|v| locale.percent(v / 100.0)).unwrap_or_else(|| "—".into())
}

fn ratio_detail(locale: ReportLocale, r: &SmrpRatio, unit_fr: &'static str, unit_en: &'static str) -> String {
    let unit = locale.pick(unit_fr, unit_en);
    format!(
        "{} / {} {unit}",
        locale.decimal(r.numerator, 1),
        locale.decimal(r.denominator, 1)
    )
}

/// Rows (indicator, value, how it was obtained) of the reliability + SMRP tables.
pub fn indicator_rows(locale: ReportLocale, d: &MonthlyData) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let none = "—".to_string();
    let hours = |v: Option<f64>| {
        v.map(|h| format!("{} h", locale.decimal(h, 1)))
            .unwrap_or_else(|| none.clone())
    };
    let reliability = vec![
        vec![
            locale
                .pick("Disponibilité opérationnelle (A_o)", "Operational availability (A_o)")
                .into(),
            d.fleet
                .availability_operational
                .map(|v| locale.percent(v))
                .unwrap_or_else(|| none.clone()),
        ],
        vec![
            locale.pick("MTBF du parc", "Fleet MTBF").into(),
            hours(d.fleet.mtbf_hours),
        ],
        vec![
            locale.pick("MTTR du parc", "Fleet MTTR").into(),
            hours(d.fleet.mttr_hours),
        ],
        vec![
            locale.pick("Défaillances", "Failure events").into(),
            d.fleet.total_failure_events.to_string(),
        ],
        vec![
            locale
                .pick("Équipements suivis", "Assets with a reliability snapshot")
                .into(),
            d.fleet.equipment_count.to_string(),
        ],
    ];

    let mut smrp = Vec::new();
    if let Some(m) = &d.smrp.schedule_compliance {
        // Measured against the frozen weekly baseline (`planning::kpis`): without a frozen week
        // that ended in the month, the reason is given, never a number.
        let (value, detail) = match m.current.value_pct {
            Some(_) => (
                pct(locale, m.current.value_pct),
                ratio_detail(locale, &m.current, "h", "h"),
            ),
            None => (
                locale.pick("non mesuré", "not measured").into(),
                locale
                    .pick(
                        "aucune semaine gelée terminée sur la période",
                        "no frozen week ended in the period",
                    )
                    .into(),
            ),
        };
        smrp.push(vec![
            locale.pick("Respect du planning", "Schedule compliance").into(),
            value,
            detail,
        ]);
    }
    if let Some(mix) = &d.smrp.work_mix {
        let (fr_u, en_u) = if mix.basis == "hours" { ("h", "h") } else { ("OT", "WO") };
        for (fr, en, m) in [
            ("Travail planifié", "Planned work", &mix.planned),
            ("Travail réactif", "Reactive work", &mix.reactive),
            ("Travail d'urgence", "Emergency work", &mix.emergency),
        ] {
            smrp.push(vec![
                locale.pick(fr, en).to_string(),
                pct(locale, m.current.value_pct),
                ratio_detail(locale, &m.current, fr_u, en_u),
            ]);
        }
    }
    if let Some(m) = &d.smrp.pm_compliance {
        smrp.push(vec![
            locale.pick("Respect des PM", "PM compliance").into(),
            pct(locale, m.current.value_pct),
            ratio_detail(locale, &m.current, "PM", "PM"),
        ]);
    }
    if let Some(b) = &d.smrp.backlog {
        smrp.push(vec![
            locale
                .pick("Carnet de travaux (à la date du rapport)", "Backlog (at report date)")
                .into(),
            b.weeks
                .map(|w| format!("{} {}", locale.decimal(w, 1), locale.pick("semaines", "weeks")))
                .unwrap_or_else(|| none.clone()),
            format!(
                "{} h / {} h {}",
                locale.decimal(b.backlog_hours, 1),
                locale.decimal(b.weekly_capacity_hours, 1),
                locale.pick("par semaine", "per week")
            ),
        ]);
    }
    if let Some(c) = &d.smrp.cost_to_rav {
        smrp.push(vec![
            locale
                .pick("Coût / valeur de remplacement", "Cost / replacement value")
                .into(),
            c.value_pct.map(|v| locale.percent(v / 100.0)).unwrap_or_else(|| {
                locale
                    .pick("valeur de remplacement manquante", "replacement value missing")
                    .into()
            }),
            format!(
                "{} / {} ({} {} {})",
                money(locale, c.annualized_cost, &d.currency),
                money(locale, c.replacement_value_total, &d.currency),
                c.assets_with_value,
                locale.pick("sur", "of"),
                c.assets_total
            ),
        ]);
    }
    smrp.push(vec![
        locale.pick("Temps outil (wrench time)", "Wrench time").into(),
        locale.pick("non mesuré", "not measured").into(),
        locale
            .pick("nécessite un échantillonnage du travail", "requires work sampling")
            .into(),
    ]);
    (reliability, smrp)
}

fn bad_actor_rows(locale: ReportLocale, d: &MonthlyData) -> Vec<Vec<String>> {
    d.bad_actors
        .iter()
        .map(|a| {
            vec![
                format!("{} — {}", a.asset_code, a.asset_label),
                a.event_count.to_string(),
                a.mtbf_hours
                    .map(|h| format!("{} h", locale.decimal(h, 1)))
                    .unwrap_or_else(|| "—".into()),
                a.availability.map(|v| locale.percent(v)).unwrap_or_else(|| "—".into()),
                money(locale, a.maintenance_cost, &d.currency),
            ]
        })
        .collect()
}

pub async fn export_monthly_management(
    db: &DatabaseConnection,
    fmt: &str,
    locale: ReportLocale,
) -> AppResult<ExportedBinaryDocument> {
    let now = Utc::now();
    let d = gather(db, locale, now).await?;
    let (reliability, smrp) = indicator_rows(locale, &d);
    let bad = bad_actor_rows(locale, &d);
    let cc: Vec<Vec<String>> = d
        .cost_centres
        .iter()
        .map(|c| vec![c.label.clone(), money(locale, c.amount, &c.currency)])
        .collect();
    let by_type: Vec<Vec<String>> = d
        .cost_by_type
        .iter()
        .map(|(label_fr, label_en, total, n)| {
            vec![
                locale.pick_owned(label_fr, label_en),
                n.to_string(),
                money(locale, *total, &d.currency),
            ]
        })
        .collect();
    let h_ind = [locale.pick("Indicateur", "Indicator"), locale.pick("Valeur", "Value")];
    let h_smrp = [
        locale.pick("Indicateur", "Indicator"),
        locale.pick("Valeur", "Value"),
        locale.pick("Calcul", "Computed from"),
    ];
    let h_bad = [
        locale.pick("Équipement", "Asset"),
        locale.pick("Défaillances", "Failures"),
        "MTBF",
        locale.pick("Disponibilité", "Availability"),
        locale.pick("Coût", "Cost"),
    ];
    let h_cc = [
        locale.pick("Centre de coût", "Cost centre"),
        locale.pick("Montant", "Amount"),
    ];
    let h_type = [
        locale.pick("Type d'OT", "Work-order type"),
        locale.pick("OT clôturés", "Closed WOs"),
        locale.pick("Coût", "Cost"),
    ];
    let s_rel = locale.pick("Fiabilité du parc", "Fleet reliability");
    let s_smrp = locale.pick("Indicateurs SMRP", "SMRP indicators");
    let s_bad = locale.pick("Équipements les plus défaillants", "Top bad actors");
    let s_cc = locale.pick("Coût par centre de coût", "Cost by cost centre");
    let s_type = locale.pick("Coût par type d'OT", "Cost by work-order type");
    let no_cc = locale.pick(
        "Aucun coût comptabilisé sur un centre de coût ce mois-ci.",
        "No cost was posted to a cost centre this month.",
    );
    let base = format!(
        "{}-{}",
        locale.pick("rapport-mensuel", "monthly-report"),
        d.month.replace(' ', "-")
    );

    if fmt == "pdf" {
        let font = crate::reports::pdf::load_system_font()?;
        let mut doc =
            crate::reports::pdf::PdfDocument::new(&font, crate::reports::pdf::PaperSize::A4Portrait, &d.title)?;
        for line in &d.header {
            doc.paragraph(line);
        }
        doc.heading(&d.title);
        doc.subheading(&d.month);
        doc.paragraph(&locale.generated_at(now));
        doc.subheading(s_rel);
        doc.table(&h_ind, &reliability, &[3.0, 2.0]);
        doc.subheading(s_smrp);
        doc.table(&h_smrp, &smrp, &[2.5, 1.5, 2.5]);
        doc.subheading(s_bad);
        if bad.is_empty() {
            doc.paragraph(locale.pick(
                "Aucun instantané de fiabilité sur le mois.",
                "No reliability snapshot this month.",
            ));
        } else {
            doc.table(&h_bad, &bad, &[3.0, 1.0, 1.2, 1.2, 1.6]);
        }
        doc.subheading(s_cc);
        if cc.is_empty() {
            doc.paragraph(no_cc);
        } else {
            doc.table(&h_cc, &cc, &[3.0, 2.0]);
        }
        doc.subheading(s_type);
        if by_type.is_empty() {
            doc.paragraph(locale.pick("Aucun OT clôturé sur le mois.", "No work order closed this month."));
        } else {
            doc.table(&h_type, &by_type, &[2.5, 1.0, 2.0]);
        }
        doc.paragraph(locale.pick(
            "Définitions : voir Analytique → Tableaux de bord → Indicateurs SMRP (bouton d'information de chaque indicateur).",
            "Definitions: see Analytics → Dashboards → SMRP indicators (information button of each indicator).",
        ));
        let bytes = doc.finish()?;
        return Ok(ExportedBinaryDocument {
            file_name: format!("{base}.pdf"),
            mime_type: "application/pdf".to_string(),
            bytes,
        });
    }

    let mut wb = rust_xlsxwriter::Workbook::new();
    let bold = rust_xlsxwriter::Format::new().set_bold();
    let mut write_sheet = |name: &str, headers: &[&str], rows: &[Vec<String>], empty: &str| -> AppResult<()> {
        let ws = wb.add_worksheet();
        ws.set_name(name).map_err(xlsx_e)?;
        ws.write_string_with_format(0, 0, format!("{} — {}", d.title, d.month), &bold)
            .map_err(xlsx_e)?;
        for (c, h) in headers.iter().enumerate() {
            ws.write_string_with_format(2, c as u16, *h, &bold).map_err(xlsx_e)?;
        }
        if rows.is_empty() {
            ws.write_string(3, 0, empty).map_err(xlsx_e)?;
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(3 + r as u32, c as u16, v).map_err(xlsx_e)?;
            }
        }
        ws.set_column_width(0, 42).map_err(xlsx_e)?;
        Ok(())
    };
    let none = "—";
    write_sheet(s_rel, &h_ind, &reliability, none)?;
    write_sheet(s_smrp, &h_smrp, &smrp, none)?;
    write_sheet(&truncate_sheet(s_bad), &h_bad, &bad, none)?;
    write_sheet(&truncate_sheet(s_cc), &h_cc, &cc, no_cc)?;
    write_sheet(&truncate_sheet(s_type), &h_type, &by_type, none)?;
    let bytes = wb.save_to_buffer().map_err(xlsx_e)?;
    Ok(ExportedBinaryDocument {
        file_name: format!("{base}.xlsx"),
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        bytes,
    })
}

/// Excel sheet names: at most 31 characters, no `[]:*?/\`.
fn truncate_sheet(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_month_handles_january() {
        let now = Utc.with_ymd_and_hms(2026, 1, 14, 9, 0, 0).unwrap();
        let (w, y, m) = previous_month(now);
        assert_eq!((y, m), (2025, 12));
        assert_eq!(instant(w.from), "2025-12-01T00:00:00Z");
        assert_eq!(instant(w.to), "2026-01-01T00:00:00Z");
        let (w, _, m) = previous_month(Utc.with_ymd_and_hms(2026, 10, 1, 0, 30, 0).unwrap());
        assert_eq!((m, instant(w.from)), (9, "2026-09-01T00:00:00Z".to_string()));
        assert_eq!(month_name(ReportLocale::Fr, 2026, 8), "août 2026");
        assert_eq!(month_name(ReportLocale::En, 2026, 8), "August 2026");
    }

    #[test]
    fn header_html_becomes_text_lines() {
        let html = "<p><strong>ACME Industries</strong></p><p>Zone industrielle&nbsp;12<br>Oran</p>\
                    <table><tr><td>Tél</td><td>+213 41 00 00</td></tr></table>";
        assert_eq!(
            header_lines(html),
            ["ACME Industries", "Zone industrielle 12", "Oran", "Tél +213 41 00 00"]
        );
        assert!(header_lines("").is_empty());
    }

    #[test]
    fn money_groups_thousands_per_language() {
        assert_eq!(money(ReportLocale::Fr, 1_234_567.5, "DZD"), "1 234 567,50 DZD");
        assert_eq!(money(ReportLocale::En, 1_234_567.5, "DZD"), "1,234,567.50 DZD");
        assert_eq!(money(ReportLocale::En, 12.0, ""), "12.00");
        assert_eq!(money(ReportLocale::Fr, -40.0, "EUR"), "-40,00 EUR");
    }

    #[tokio::test]
    async fn monthly_report_builds_and_is_listed() {
        use sea_orm::ConnectionTrait;

        let db = crate::db::test_db::seeded().await;
        db.execute_unprepared(
            "INSERT INTO app_settings (setting_key, setting_scope, setting_value_json, category, setting_risk, validation_status, last_modified_at) VALUES ('documents.default_header', 'tenant', '{\"version\":1,\"html\":\"<p>ACME Industries</p><p>Oran</p>\"}', 'documents', 'low', 'valid', datetime('now')) ON CONFLICT(setting_key, setting_scope) DO UPDATE SET setting_value_json = excluded.setting_value_json",
        )
        .await
        .expect("header");

        let listed = crate::reports::queries::list_report_templates(&db).await.expect("list");
        assert!(listed.iter().any(|t| t.code == TEMPLATE_CODE), "template is available");
        // Idempotent: listing twice keeps one row.
        crate::reports::queries::list_report_templates(&db)
            .await
            .expect("list again");
        let n: i64 = db
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS c FROM report_templates WHERE code = '{TEMPLATE_CODE}'"),
            ))
            .await
            .expect("count")
            .expect("row")
            .try_get_by_index(0)
            .expect("c");
        assert_eq!(n, 1);

        let data = gather(&db, ReportLocale::Fr, Utc::now()).await.expect("gather");
        assert_eq!(data.header, ["ACME Industries", "Oran"]);
        assert!(data.smrp.schedule_compliance.is_some());
        let (_, smrp_rows) = indicator_rows(ReportLocale::Fr, &data);
        assert_eq!(
            smrp_rows.last().expect("row")[1],
            "non mesuré",
            "wrench time is never invented"
        );

        let doc = export_monthly_management(&db, "xlsx", ReportLocale::En)
            .await
            .expect("xlsx");
        assert!(doc.file_name.starts_with("monthly-report-") && doc.file_name.ends_with(".xlsx"));
        assert!(doc.bytes.len() > 1000);
    }

    #[test]
    fn sheet_names_fit_excel_rules() {
        assert_eq!(
            truncate_sheet("Équipements les plus défaillants"),
            "Équipements les plus défaillant"
        );
        assert_eq!(truncate_sheet("Cost / centre"), "Cost  centre");
    }
}
