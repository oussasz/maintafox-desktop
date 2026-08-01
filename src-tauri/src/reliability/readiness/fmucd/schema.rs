use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{NaiveDateTime, Utc};

/// One denormalized FMUCD CSV row (38 columns per Pampana et al., Data in Brief 2024).
#[derive(Debug, Clone)]
pub struct FmucdRow {
    pub line_number: u64,
    pub fields: HashMap<String, String>,
}

impl FmucdRow {
    pub fn from_csv_record(line_number: u64, headers: &[String], record: &csv::StringRecord) -> Self {
        let mut fields = HashMap::new();
        for (i, h) in headers.iter().enumerate() {
            if let Some(v) = record.get(i) {
                fields.insert(h.clone(), v.to_string());
            }
        }
        Self { line_number, fields }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// Parse FMUCD datetime (`M/D/YYYY H:MM:SS` or ISO-like).
pub fn parse_fmucd_datetime(raw: &str) -> Result<chrono::DateTime<Utc>> {
    let s = raw.trim();
    if s.is_empty() {
        anyhow::bail!("empty datetime");
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    const FORMATS: [&str; 4] = ["%m/%d/%Y %H:%M:%S", "%Y-%m-%d %H:%M:%S", "%m/%d/%Y", "%Y-%m-%d"];
    for fmt in FORMATS {
        if let Ok(nd) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(nd.and_utc());
        }
        if let Ok(nd) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Ok(nd.and_hms_opt(0, 0, 0).context("date to datetime")?.and_utc());
        }
    }
    anyhow::bail!("unrecognized datetime format: {s}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_us_datetime() {
        let dt = parse_fmucd_datetime("2012-12-17 07:49:05").expect("parse");
        assert_eq!(dt.format("%Y").to_string(), "2012");
    }
}
