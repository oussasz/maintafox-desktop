//! Human-readable record numbers that stay unique across devices (Phase 2 leftover 2).
//!
//! Work orders (`OT-…`), intervention requests (`DI-…`), personnel (`PER-…`) and permits
//! (`PTW-…`) are numbered on the device that creates them, offline. Numbering per device alone
//! made two devices hand out the same number. New numbers therefore carry a short code of the
//! device: `OT-7KQ2-0001` is the first work order numbered on device `7KQ2`, and its sequence only
//! counts that device's numbers, so two devices with different codes never produce the same one.
//!
//! - The device code is 4 characters (Crockford base 32: digits and capitals without I, L, O, U)
//!   derived from this database's replica id (`sync_replica_identity`), so it never changes for
//!   the life of the database and needs no network.
//! - Numbers made before this version (`OT-0001`, …) are never changed: printed sheets and QR
//!   codes that carry them keep working.
//! - Two devices whose codes happen to be equal (about one chance in 2 400 for 30 devices) and
//!   numbers that already clashed before this version are detected when the other device's
//!   record arrives (`sync::code_clash`) and shown to a person; nothing is renamed silently.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};

/// Crockford base 32 alphabet (no I, L, O, U: never confused with 1, 0 or V when read aloud).
pub(crate) const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Characters of a device code (5 bits each, 20 bits).
pub const DEVICE_CODE_LEN: usize = 4;

/// The device code of a replica id: the first 20 bits of its SHA-256 in Crockford base 32.
pub fn device_code_for(replica_id: &str) -> String {
    let digest = Sha256::digest(replica_id.trim().as_bytes());
    let value = ((u32::from(digest[0]) << 16) | (u32::from(digest[1]) << 8) | u32::from(digest[2])) >> 4;
    (0..DEVICE_CODE_LEN)
        .map(|i| {
            let shift = 5 * (DEVICE_CODE_LEN - 1 - i);
            char::from(ALPHABET[((value >> shift) & 0x1f) as usize])
        })
        .collect()
}

/// This database's device code.
pub async fn this_device_code(conn: &impl ConnectionTrait) -> AppResult<String> {
    Ok(device_code_for(&crate::sync::queries::local_replica_id(conn).await?))
}

/// SQL expression (and its values) for the next number `{stem}{sequence}` of `table.column`, the
/// sequence counting only numbers that start with `stem` and end in digits. Usable inside an
/// `INSERT … SELECT` so allocation and insert are one statement.
pub fn next_number_sql(table: &str, column: &str, stem: &str, width: usize) -> (String, Vec<sea_orm::Value>) {
    let stem_len = i64::try_from(stem.chars().count()).unwrap_or(i64::MAX);
    let sql = format!(
        "(? || printf('%0{width}d', COALESCE((SELECT MAX(CAST(SUBSTR({column}, ? + 1) AS INTEGER)) FROM {table}
            WHERE SUBSTR({column}, 1, ?) = ? AND SUBSTR({column}, ? + 1) GLOB '[0-9]*'
              AND SUBSTR({column}, ? + 1) NOT GLOB '*[^0-9]*'), 0) + 1))"
    );
    let values = vec![
        stem.into(),
        stem_len.into(),
        stem_len.into(),
        stem.into(),
        stem_len.into(),
        stem_len.into(),
    ];
    (sql, values)
}

/// `{prefix}-{device}-` of this database.
pub async fn device_stem(conn: &impl ConnectionTrait, prefix: &str) -> AppResult<String> {
    Ok(format!("{prefix}-{}-", this_device_code(conn).await?))
}

/// Next number `{prefix}-{device}-{sequence}` for `table.column`, the sequence counting only the
/// numbers of this device with that prefix (`width` digits at least). Pass the transaction that
/// inserts the row, so the read and the insert are serialised by SQLite's write lock.
pub async fn next_device_number(
    conn: &impl ConnectionTrait,
    table: &str,
    column: &str,
    prefix: &str,
    width: usize,
) -> AppResult<String> {
    let stem = device_stem(conn, prefix).await?;
    let (expr, values) = next_number_sql(table, column, &stem, width);
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!("SELECT {expr} AS next_number"),
            values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("{table} number sequence query returned no rows")))?;
    row.try_get("", "next_number")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{table} number sequence decode error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_code_is_stable_short_and_unambiguous() {
        let code = device_code_for("4f0c2b8e-7f53-4a7e-9a61-0f7b8d1e2c3a");
        assert_eq!(code.len(), DEVICE_CODE_LEN);
        assert_eq!(code, device_code_for("4f0c2b8e-7f53-4a7e-9a61-0f7b8d1e2c3a"));
        assert!(code.bytes().all(|c| ALPHABET.contains(&c)));
        assert!(!code.contains(['I', 'L', 'O', 'U']));
        assert_ne!(code, device_code_for("4f0c2b8e-7f53-4a7e-9a61-0f7b8d1e2c3b"));
    }

    #[test]
    fn device_codes_spread_over_the_alphabet() {
        let codes: std::collections::HashSet<String> =
            (0..2000).map(|i| device_code_for(&format!("replica-{i}"))).collect();
        // 2000 random 20-bit values: a handful of repeats at most.
        assert!(codes.len() > 1990, "{} distinct codes", codes.len());
    }
}
