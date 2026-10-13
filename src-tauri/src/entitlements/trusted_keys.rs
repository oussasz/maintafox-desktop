//! Ed25519 public keys trusted to sign entitlement envelopes.
//!
//! These are PUBLIC keys: embedding them is safe and is what makes forgery impossible (the
//! matching private keys exist only in the control-plane environment, generated with
//! `maintafox-vendor-console/api/scripts/generate-entitlement-signing-key.mjs`).
//!
//! Rotation: add the new `(key_id, public_key_hex)` here and ship it BEFORE the server switches
//! to the new key; remove an old key only once no desktop still depends on envelopes it signed.
//! Operational revocation (compromise) is separate: `licensing_trust_keys.is_compromised`.

use ring::signature::{UnparsedPublicKey, ED25519};

/// Issuer name every entitlement envelope must carry.
pub const ENTITLEMENT_ISSUER: &str = "maintafox-vps";

/// Production keys: `(key_id, raw 32-byte Ed25519 public key as lowercase hex)`.
const PRODUCTION_ENTITLEMENT_KEYS: &[(&str, &str)] = &[(
    "mfx-ed25519-2026-10",
    "dd3df97205c5f0b758cb5d03c729a481035171c855c8c67b83098268829cbea9",
)];

/// TEST-ONLY key derived from a public seed (see `test_support`). Never compiled into builds.
#[cfg(test)]
pub(crate) const TEST_ENTITLEMENT_KEY_ID: &str = "test-ed25519-do-not-trust";

fn all_keys() -> Vec<(String, String)> {
    #[allow(unused_mut)]
    let mut keys: Vec<(String, String)> = PRODUCTION_ENTITLEMENT_KEYS
        .iter()
        .map(|(id, hex_key)| ((*id).to_string(), (*hex_key).to_string()))
        .collect();
    #[cfg(test)]
    keys.push((
        TEST_ENTITLEMENT_KEY_ID.to_string(),
        crate::entitlements::test_support::test_public_key_hex(),
    ));
    keys
}

/// Key ids registered in `licensing_trust_keys` (purpose `entitlement_signature`).
pub fn entitlement_key_ids() -> Vec<String> {
    all_keys().into_iter().map(|(id, _)| id).collect()
}

/// Outcome of cryptographic verification (operational trust is checked separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureCheck {
    Verified,
    InvalidSignature,
    UntrustedKey,
}

impl SignatureCheck {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::InvalidSignature => "invalid_signature",
            Self::UntrustedKey => "untrusted_issuer",
        }
    }
}

/// Verifies a hex Ed25519 `signature_hex` over `message` with the key registered for
/// `(issuer, key_id)`. Malformed hex is reported as an invalid signature, never a panic.
pub fn verify_entitlement_signature(issuer: &str, key_id: &str, message: &[u8], signature_hex: &str) -> SignatureCheck {
    if issuer != ENTITLEMENT_ISSUER {
        return SignatureCheck::UntrustedKey;
    }
    let Some(public_hex) = all_keys()
        .into_iter()
        .find(|(id, _)| id == key_id)
        .map(|(_, hex_key)| hex_key)
    else {
        return SignatureCheck::UntrustedKey;
    };
    let Ok(public_key) = hex::decode(public_hex) else {
        return SignatureCheck::UntrustedKey;
    };
    let Ok(signature) = hex::decode(signature_hex) else {
        return SignatureCheck::InvalidSignature;
    };
    match UnparsedPublicKey::new(&ED25519, public_key).verify(message, &signature) {
        Ok(()) => SignatureCheck::Verified,
        Err(_) => SignatureCheck::InvalidSignature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_keys_are_well_formed() {
        for (id, hex_key) in PRODUCTION_ENTITLEMENT_KEYS {
            assert!(!id.is_empty());
            let bytes = hex::decode(hex_key).expect("public key hex");
            assert_eq!(bytes.len(), 32, "Ed25519 public key for {id} must be 32 bytes");
        }
    }

    #[test]
    fn unknown_issuer_and_key_are_untrusted() {
        assert_eq!(
            verify_entitlement_signature("someone-else", TEST_ENTITLEMENT_KEY_ID, b"x", "00"),
            SignatureCheck::UntrustedKey
        );
        assert_eq!(
            verify_entitlement_signature(ENTITLEMENT_ISSUER, "no-such-key", b"x", "00"),
            SignatureCheck::UntrustedKey
        );
    }

    #[test]
    fn malformed_signature_is_invalid_not_a_panic() {
        assert_eq!(
            verify_entitlement_signature(ENTITLEMENT_ISSUER, TEST_ENTITLEMENT_KEY_ID, b"x", "not-hex"),
            SignatureCheck::InvalidSignature
        );
    }
}
