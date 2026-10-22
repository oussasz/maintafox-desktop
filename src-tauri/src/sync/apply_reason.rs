//! Why a received row could not be written here, in words a person can act on (0.9.0 test
//! finding 4, rule 3).
//!
//! A received row refused by the database itself (a unique value already used here, a required
//! field left empty, a link to a record that is not here, a rule of this PC's database) is recorded
//! as a `MATERIALIZE_ERROR` conflict with the database's text (`sync_conflicts.detail_message`).
//! [`classify`] reads that text back into a coded reason with the table and fields it names; the
//! sync centre shows the reason in French or English (`shell:sync.applyReason.*`) and keeps the raw
//! text in the technical details. A text it does not recognise reads [`TECHNICAL`]: the raw text is
//! then the only explanation, never hidden.

use serde::{Deserialize, Serialize};

/// The value is already used here by another record (a unique field or set of fields).
pub const UNIQUE_TAKEN: &str = "UNIQUE_TAKEN";
/// A field this PC requires is empty in the received row (an older app version or old data).
pub const REQUIRED_MISSING: &str = "REQUIRED_MISSING";
/// The received row points to a record this PC does not hold.
pub const LINK_MISSING: &str = "LINK_MISSING";
/// A rule of this PC's database refused the value (a check or a guard).
pub const RULE_REFUSED: &str = "RULE_REFUSED";
/// A value of the received row cannot be read as this version expects it.
pub const INVALID_DATA: &str = "INVALID_DATA";
/// Any other error: the raw text is the explanation.
pub const TECHNICAL: &str = "TECHNICAL";

/// A coded reason with what it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictReason {
    pub code: String,
    /// The table the database named, when it named one.
    pub table: Option<String>,
    /// The fields it named (a unique set, a required field, a check), in its order.
    pub columns: Vec<String>,
    /// The text of a rule that refused the value (a guard's own message), when there is one.
    pub rule: Option<String>,
}

impl ConflictReason {
    fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            table: None,
            columns: Vec::new(),
            rule: None,
        }
    }
}

/// `table.column, table.column` (SQLite's list after a constraint name) → table and columns.
fn table_columns(list: &str) -> (Option<String>, Vec<String>) {
    let mut table = None;
    let mut columns = Vec::new();
    for part in list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('.') {
            Some((t, c)) => {
                table.get_or_insert_with(|| t.trim().to_string());
                columns.push(c.trim().to_string());
            }
            None => columns.push(part.to_string()),
        }
    }
    (table, columns)
}

/// The SQLite extended result code in a database error (`(code: 2067) …`), and the text after it.
fn sqlite_code(message: &str) -> Option<(i64, &str)> {
    let start = message.find("(code: ")? + "(code: ".len();
    let rest = &message[start..];
    let end = rest.find(')')?;
    let code = rest[..end].trim().parse().ok()?;
    Some((code, rest[end + 1..].trim()))
}

/// Text that follows `marker` up to the end of the line.
fn after<'a>(message: &'a str, marker: &str) -> Option<&'a str> {
    message
        .find(marker)
        .map(|i| message[i + marker.len()..].lines().next().unwrap_or("").trim())
}

/// The coded reason of an error text recorded with a sync conflict.
pub fn classify(message: &str) -> ConflictReason {
    if let Some(list) = after(message, "UNIQUE constraint failed:") {
        let (table, columns) = table_columns(list);
        return ConflictReason {
            table,
            columns,
            ..ConflictReason::new(UNIQUE_TAKEN)
        };
    }
    if let Some(list) = after(message, "NOT NULL constraint failed:") {
        let (table, columns) = table_columns(list);
        return ConflictReason {
            table,
            columns,
            ..ConflictReason::new(REQUIRED_MISSING)
        };
    }
    if message.contains("FOREIGN KEY constraint failed") {
        return ConflictReason::new(LINK_MISSING);
    }
    if let Some(name) = after(message, "CHECK constraint failed:") {
        return ConflictReason {
            columns: if name.is_empty() {
                Vec::new()
            } else {
                vec![name.to_string()]
            },
            ..ConflictReason::new(RULE_REFUSED)
        };
    }
    match sqlite_code(message) {
        // SQLITE_CONSTRAINT_TRIGGER: a guard of this PC (RAISE(ABORT, '<rule>')).
        Some((1811, rule)) => {
            return ConflictReason {
                rule: Some(rule.to_string()).filter(|r| !r.is_empty()),
                ..ConflictReason::new(RULE_REFUSED)
            }
        }
        Some((1555 | 2067, _)) => return ConflictReason::new(UNIQUE_TAKEN),
        Some((1299, _)) => return ConflictReason::new(REQUIRED_MISSING),
        Some((787, _)) => return ConflictReason::new(LINK_MISSING),
        Some((275, _)) => return ConflictReason::new(RULE_REFUSED),
        _ => {}
    }
    // The row codec names the table it was reading (`Sync error: <table>: <what>`).
    if let Some(rest) = message.strip_prefix("Sync error: ") {
        if let Some((table, _)) = rest.split_once(": ") {
            if !table.is_empty() && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return ConflictReason {
                    table: Some(table.to_string()),
                    ..ConflictReason::new(INVALID_DATA)
                };
            }
        }
    }
    ConflictReason::new(TECHNICAL)
}

/// The reason shown for a conflict: for a row the database refused (`MATERIALIZE_ERROR`) with its
/// recorded text. Other conflict kinds carry their own explanation.
pub fn conflict_reason(conflict_type: &str, detail_message: Option<&str>) -> Option<ConflictReason> {
    if conflict_type != "MATERIALIZE_ERROR" {
        return None;
    }
    detail_message.map(classify)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_texts_read_as_coded_reasons() {
        let unique = classify(
            "Database error: Execution Error: error returned from database: (code: 2067) UNIQUE constraint failed: \
             pm_plan_versions.pm_plan_id, pm_plan_versions.version_no",
        );
        assert_eq!(unique.code, UNIQUE_TAKEN);
        assert_eq!(unique.table.as_deref(), Some("pm_plan_versions"));
        assert_eq!(unique.columns, vec!["pm_plan_id", "version_no"]);

        let required = classify(
            "Database error: Execution Error: error returned from database: (code: 1299) NOT NULL constraint failed: \
             pm_plan_versions.effective_from",
        );
        assert_eq!(required.code, REQUIRED_MISSING);
        assert_eq!(required.columns, vec!["effective_from"]);

        let guard = classify(
            "Database error: Execution Error: error returned from database: (code: 1811) planned_crew_size must be a \
             whole number from 1 to 50",
        );
        assert_eq!(guard.code, RULE_REFUSED);
        assert_eq!(
            guard.rule.as_deref(),
            Some("planned_crew_size must be a whole number from 1 to 50")
        );

        let link = classify(
            "Database error: Execution Error: error returned from database: (code: 787) FOREIGN KEY constraint failed",
        );
        assert_eq!(link.code, LINK_MISSING);

        let codec = classify("Sync error: pm_plan_versions: trigger_definition_json/asset_meter_id must be a sync id");
        assert_eq!(codec.code, INVALID_DATA);
        assert_eq!(codec.table.as_deref(), Some("pm_plan_versions"));

        assert_eq!(classify("disk I/O error").code, TECHNICAL);
    }

    #[test]
    fn only_a_refused_row_gets_a_database_reason() {
        assert!(conflict_reason("CODE_CLASH", Some("UNIQUE constraint failed: work_orders.code")).is_none());
        assert!(conflict_reason("MATERIALIZE_ERROR", None).is_none());
        assert_eq!(
            conflict_reason("MATERIALIZE_ERROR", Some("UNIQUE constraint failed: pm_plans.code")).map(|r| r.code),
            Some(UNIQUE_TAKEN.to_string())
        );
    }
}
