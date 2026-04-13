//! PIN hashing and validation for idle-lock fast unlock.
//!
//! Security notes:
//! - Uses argon2id with lower memory (16 MiB) because PIN entropy is lower and
//!   unlock UX must remain responsive on field devices.
//! - PIN is ONLY used for unlocking an already-authenticated locked session.
//! - Never used for initial login, step-up verification, or password change.

use crate::errors::{AppError, AppResult};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use rand_core::OsRng;

/// Argon2id memory cost in KiB (16 MiB).
const MEMORY_COST_KIB: u32 = 16 * 1024;
/// Argon2id iteration count.
const TIME_COST: u32 = 3;
/// Argon2id parallelism degree.
const PARALLELISM: u32 = 1;

fn argon2_hasher() -> Argon2<'static> {
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, None)
        .expect("argon2id(pin): invalid parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a PIN using argon2id with reduced memory settings.
pub fn hash_pin(pin: &str) -> AppResult<String> {
    validate_pin_format(pin)?;

    let salt = SaltString::generate(&mut OsRng);
    argon2_hasher()
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("pin hash failed: {e}")))
}

/// Verify a PIN against a stored PHC hash.
pub fn verify_pin(pin: &str, hash: &str) -> AppResult<bool> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("malformed pin hash in DB: {e}")))?;

    Ok(argon2_hasher()
        .verify_password(pin.as_bytes(), &parsed_hash)
        .is_ok())
}

/// Validate PIN format: 4-6 digits.
pub fn validate_pin_format(pin: &str) -> AppResult<()> {
    if !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::ValidationFailed(vec![
            "Le PIN doit contenir uniquement des chiffres.".into(),
        ]));
    }

    if pin.len() < 4 || pin.len() > 6 {
        return Err(AppError::ValidationFailed(vec![
            "Le PIN doit contenir entre 4 et 6 chiffres.".into(),
        ]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_round_trip_hash_verify() {
        let hash = hash_pin("1234").expect("hash pin");
        assert!(hash.starts_with("$argon2id"));

        let ok = verify_pin("1234", &hash).expect("verify pin");
        assert!(ok);

        let bad = verify_pin("9999", &hash).expect("verify bad pin");
        assert!(!bad);
    }

    #[test]
    fn pin_format_validation() {
        assert!(validate_pin_format("1234").is_ok());
        assert!(validate_pin_format("123456").is_ok());
        assert!(validate_pin_format("123").is_err());
        assert!(validate_pin_format("1234567").is_err());
        assert!(validate_pin_format("12ab").is_err());
    }
}
