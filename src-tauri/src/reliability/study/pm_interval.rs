//! PM plan intervals in study hours, and the trigger a decision writes back.
//!
//! - meter plans on a run-hours meter count operating hours directly;
//! - calendar plans (`fixed` / `floating`) count calendar time; in an operating-hours study the
//!   interval is converted with the equipment's operating ratio (operating hours per calendar
//!   hour, from its data), and back again when a new interval is written.

use serde_json::{Map, Value};

/// Calendar hours of one PM interval unit (as `pm::queries::parse_interval` counts them).
#[must_use]
pub fn unit_hours(unit: &str) -> Option<f64> {
    match unit.trim().to_lowercase().as_str() {
        "hour" | "hours" => Some(1.0),
        "day" | "days" => Some(24.0),
        "week" | "weeks" => Some(168.0),
        "month" | "months" => Some(720.0),
        "year" | "years" => Some(8760.0),
        _ => None,
    }
}

/// Facts about a plan needed to read or write its interval.
#[derive(Debug, Clone)]
pub struct PlanTrigger {
    pub strategy_type: String,
    pub trigger: Map<String, Value>,
    /// The meter the plan counts is a run-hours meter (`HOURS`) or `operating_hours`.
    pub meter_counts_hours: bool,
}

/// Why an interval cannot be read or written in study hours.
pub const NOTE_NOT_INTERVAL_STRATEGY: &str = "not_interval_strategy";
pub const NOTE_SINGLE_THRESHOLD: &str = "single_threshold";
pub const NOTE_NOT_HOURS_METER: &str = "not_hours_meter";
pub const NOTE_NO_OPERATING_RATIO: &str = "no_operating_ratio";
pub const NOTE_INVALID_TRIGGER: &str = "invalid_trigger";

impl PlanTrigger {
    pub fn parse(strategy_type: &str, trigger_json: &str, meter_counts_hours: bool) -> Result<Self, &'static str> {
        let trigger = serde_json::from_str::<Value>(trigger_json)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .ok_or(NOTE_INVALID_TRIGGER)?;
        Ok(Self {
            strategy_type: strategy_type.to_string(),
            trigger,
            meter_counts_hours,
        })
    }

    fn interval_value(&self) -> Option<f64> {
        self.trigger
            .get("interval_value")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
    }

    /// `(value, unit)` as the plan states it; unit `operating_hours` for meter plans.
    #[must_use]
    pub fn displayed_interval(&self) -> Option<(f64, String)> {
        let value = self.interval_value()?;
        match self.strategy_type.as_str() {
            "meter" => Some((value, "operating_hours".into())),
            "fixed" | "floating" => {
                let unit = self.trigger.get("interval_unit").and_then(Value::as_str)?;
                unit_hours(unit).map(|_| (value, unit.trim().to_lowercase()))
            }
            _ => None,
        }
    }

    /// Current interval in study hours. `ratio` = operating hours per calendar hour of the
    /// equipment; `operating` = the study counts operating hours.
    pub fn interval_hours(&self, operating: bool, ratio: Option<f64>) -> Result<f64, &'static str> {
        let ratio = ratio.filter(|r| r.is_finite() && *r > 0.0);
        match self.strategy_type.as_str() {
            "meter" => {
                if !self.meter_counts_hours {
                    return Err(NOTE_NOT_HOURS_METER);
                }
                let v = self.interval_value().ok_or(NOTE_SINGLE_THRESHOLD)?;
                if operating {
                    Ok(v)
                } else {
                    ratio.map(|r| v / r).ok_or(NOTE_NO_OPERATING_RATIO)
                }
            }
            "fixed" | "floating" => {
                let (v, unit) = self.displayed_interval().ok_or(NOTE_INVALID_TRIGGER)?;
                let cal = v * unit_hours(&unit).ok_or(NOTE_INVALID_TRIGGER)?;
                if operating {
                    ratio.map(|r| cal * r).ok_or(NOTE_NO_OPERATING_RATIO)
                } else {
                    Ok(cal)
                }
            }
            _ => Err(NOTE_NOT_INTERVAL_STRATEGY),
        }
    }

    /// New trigger JSON for an interval of `hours` study hours, keeping every other field.
    /// Returns `(json, value, unit)` as the plan will show it.
    pub fn with_interval_hours(
        &self,
        hours: f64,
        operating: bool,
        ratio: Option<f64>,
    ) -> Result<(String, f64, String), &'static str> {
        if !(hours.is_finite() && hours > 0.0) {
            return Err(NOTE_INVALID_TRIGGER);
        }
        let ratio = ratio.filter(|r| r.is_finite() && *r > 0.0);
        let mut trigger = self.trigger.clone();
        match self.strategy_type.as_str() {
            "meter" => {
                if !self.meter_counts_hours {
                    return Err(NOTE_NOT_HOURS_METER);
                }
                self.interval_value().ok_or(NOTE_SINGLE_THRESHOLD)?;
                let op_hours = if operating {
                    hours
                } else {
                    hours * ratio.ok_or(NOTE_NO_OPERATING_RATIO)?
                };
                let value = op_hours.round().max(1.0);
                trigger.insert("interval_value".into(), Value::from(value));
                Ok((Value::Object(trigger).to_string(), value, "operating_hours".into()))
            }
            "fixed" | "floating" => {
                let (_, unit) = self.displayed_interval().ok_or(NOTE_INVALID_TRIGGER)?;
                let cal = if operating {
                    hours / ratio.ok_or(NOTE_NO_OPERATING_RATIO)?
                } else {
                    hours
                };
                // Keep the plan's unit when it states the interval within 10 %; days otherwise.
                let per_unit = unit_hours(&unit).ok_or(NOTE_INVALID_TRIGGER)?;
                let in_unit = (cal / per_unit).round();
                let (value, unit) = if in_unit >= 1.0 && ((in_unit * per_unit - cal) / cal).abs() <= 0.10 {
                    (in_unit, unit)
                } else {
                    ((cal / 24.0).round().max(1.0), "day".to_string())
                };
                trigger.insert("interval_value".into(), Value::from(value));
                trigger.insert("interval_unit".into(), Value::from(unit.clone()));
                Ok((Value::Object(trigger).to_string(), value, unit))
            }
            _ => Err(NOTE_NOT_INTERVAL_STRATEGY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_plan_reads_and_writes_operating_hours() {
        let p = PlanTrigger::parse(
            "meter",
            r#"{"asset_meter_id":7,"interval_value":6000,"start_reading":0}"#,
            true,
        )
        .unwrap();
        assert_eq!(p.interval_hours(true, None), Ok(6000.0));
        // Calendar study: operating hours / ratio.
        assert_eq!(p.interval_hours(false, Some(0.5)), Ok(12_000.0));
        let (json, value, unit) = p.with_interval_hours(1851.6, true, None).unwrap();
        assert_eq!((value, unit.as_str()), (1852.0, "operating_hours"));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["interval_value"], 1852.0);
        assert_eq!(v["asset_meter_id"], 7, "other fields are kept");
        assert_eq!(v["start_reading"], 0);
    }

    #[test]
    fn single_threshold_and_non_hour_meters_are_refused() {
        let p = PlanTrigger::parse("meter", r#"{"asset_meter_id":7,"threshold_value":5000}"#, true).unwrap();
        assert_eq!(p.interval_hours(true, None), Err(NOTE_SINGLE_THRESHOLD));
        let p = PlanTrigger::parse("meter", r#"{"meter_source":"odometer","interval_value":5000}"#, false).unwrap();
        assert_eq!(p.interval_hours(true, None), Err(NOTE_NOT_HOURS_METER));
        let p = PlanTrigger::parse("event", r#"{"event_code":"X"}"#, false).unwrap();
        assert_eq!(
            p.with_interval_hours(10.0, true, None).map(|t| t.1),
            Err(NOTE_NOT_INTERVAL_STRATEGY)
        );
    }

    #[test]
    fn calendar_plan_converts_with_the_operating_ratio() {
        let p = PlanTrigger::parse("fixed", r#"{"interval_unit":"week","interval_value":36}"#, false).unwrap();
        // 36 weeks = 6 048 calendar hours; running 2/3 of the time → 4 032 operating hours.
        let h = p.interval_hours(true, Some(2.0 / 3.0)).unwrap();
        assert!((h - 4032.0).abs() < 1e-9);
        assert_eq!(p.interval_hours(true, None), Err(NOTE_NO_OPERATING_RATIO));
        assert_eq!(p.interval_hours(false, None), Ok(6048.0));
        // 1 850 operating h at 2/3 → 2 775 calendar h ≈ 16.5 weeks → 17 weeks (within 10 %).
        let (_, value, unit) = p.with_interval_hours(1850.0, true, Some(2.0 / 3.0)).unwrap();
        assert_eq!((value, unit.as_str()), (17.0, "week"));
        // 1 850 calendar h = 2.6 months: 3 months is 17 % off → stated in days (77).
        let monthly = PlanTrigger::parse("floating", r#"{"interval_unit":"month","interval_value":6}"#, false).unwrap();
        let (json, value, unit) = monthly.with_interval_hours(1850.0, false, None).unwrap();
        assert_eq!((value, unit.as_str()), (77.0, "day"));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["interval_unit"], "day");
        // 2 688 calendar h = exactly 16 weeks: the unit is kept.
        let (_, value, unit) = p.with_interval_hours(2688.0, false, None).unwrap();
        assert_eq!((value, unit.as_str()), (16.0, "week"));
    }
}
