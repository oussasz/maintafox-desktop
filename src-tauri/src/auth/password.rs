//! argon2id password hashing module.
//!
//! Security properties:
//!   - Uses argon2id (hybrid of argon2i and argon2d) for resistance to
//!     both side-channel and GPU attacks.
//!   - OWASP-recommended 2026 parameters: m=65536 (64 MiB), t=3, p=1.
//!   - Salt is 16 random bytes per hash, embedded in the PHC string output.
//!   - Constant-time comparison via `argon2::verify_password`.
//!   - Compile-time constants — parameters cannot be weakened at runtime.

use crate::errors::{AppError, AppResult};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

/// Argon2id memory cost in KiB (64 MiB).
const MEMORY_COST_KIB: u32 = 64 * 1024;
/// Argon2id iteration count.
const TIME_COST: u32 = 3;
/// Argon2id parallelism degree.
const PARALLELISM: u32 = 1;

/// Returns the configured Argon2id hasher.
/// Panics if parameters are out of range — catches misconfigurations at first call.
fn argon2_hasher() -> Argon2<'static> {
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, None)
        .expect("argon2id: invalid parameters — check MEMORY_COST_KIB, TIME_COST, PARALLELISM constants");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a password using argon2id. Returns a PHC string suitable for storage.
///
/// # Security
/// - Salt is generated from OS entropy (`OsRng`). Never reuse salts.
/// - The returned string embeds the salt and parameters.
/// - Typical timing: ~100–400ms depending on CPU. This is intentional.
pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    argon2_hasher()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hash failed: {e}")))
}

/// Verify a plaintext password against a stored argon2id PHC hash.
///
/// Returns `Ok(true)` if the password matches, `Ok(false)` if it does not.
/// Returns `Err` only if the stored hash string is malformed (database corruption).
///
/// # Security
/// This function is constant-time with respect to the password value,
/// preventing timing attacks on password comparison.
pub fn verify_password(password: &str, stored_hash: &str) -> AppResult<bool> {
    let parsed_hash = PasswordHash::new(stored_hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("malformed password hash in DB: {e}")))?;
    Ok(argon2_hasher()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_round_trip() {
        let password = "Correct Horse Battery Staple!";
        let hash = hash_password(password).expect("hash should not fail");
        assert!(hash.starts_with("$argon2id"), "Expected argon2id PHC string");

        let result = verify_password(password, &hash).expect("verify should not fail");
        assert!(result, "Correct password should verify");
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let hash = hash_password("the_right_password").expect("hash should not fail");
        let result = verify_password("the_wrong_password", &hash).expect("verify should not fail on valid hash");
        assert!(!result, "Wrong password must not verify");
    }

    #[test]
    fn two_hashes_of_same_password_are_different() {
        let h1 = hash_password("same").expect("hash 1");
        let h2 = hash_password("same").expect("hash 2");
        assert_ne!(h1, h2, "Same password must produce different hashes (random salt)");
    }

    #[test]
    fn malformed_hash_returns_error() {
        let result = verify_password("password", "not_a_valid_phc_string");
        assert!(result.is_err(), "Malformed hash must return Err");
    }

    #[test]
    fn parameters_are_within_argon2_bounds() {
        let _ = argon2_hasher();
    }
}
