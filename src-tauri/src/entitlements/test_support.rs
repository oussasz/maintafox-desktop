//! TEST-ONLY Ed25519 signing for entitlement envelopes.
//!
//! The key is derived from a public seed and is identical to
//! `maintafox-vendor-console/api/scripts/lib/test-signing-key.mjs`, so Rust tests and the
//! server-generated vector (`testdata/ed25519_vector.json`) use the same key pair.
//! Its public key is trusted only under `#[cfg(test)]` (see `trusted_keys`).

use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};

use crate::entitlements::domain::{EntitlementEnvelopeInput, ENTITLEMENT_SIGNATURE_ALG};
use crate::entitlements::trusted_keys::{ENTITLEMENT_ISSUER, TEST_ENTITLEMENT_KEY_ID};

const TEST_SEED_LABEL: &[u8] = b"maintafox-entitlement-TEST-ONLY-seed-v1";

fn test_key_pair() -> Ed25519KeyPair {
    let seed = Sha256::digest(TEST_SEED_LABEL);
    Ed25519KeyPair::from_seed_unchecked(&seed).expect("32-byte test seed")
}

pub(crate) fn test_public_key_hex() -> String {
    hex::encode(test_key_pair().public_key().as_ref())
}

/// RFC3339 "now": envelopes and activations are checked against a 24h anti-replay window,
/// so fixtures must be signed at test time rather than on a fixed date.
pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Ed25519 signature (hex) over the canonical payload, as the control plane produces it.
pub(crate) fn sign_envelope(input: &EntitlementEnvelopeInput) -> String {
    let message = crate::entitlements::queries::canonical_payload(input);
    hex::encode(test_key_pair().sign(message.as_bytes()).as_ref())
}

/// Sets issuer/key/alg for the test key and signs the envelope.
pub(crate) fn sign_with_test_key(mut input: EntitlementEnvelopeInput) -> EntitlementEnvelopeInput {
    input.issuer = ENTITLEMENT_ISSUER.to_string();
    input.key_id = TEST_ENTITLEMENT_KEY_ID.to_string();
    input.signature_alg = ENTITLEMENT_SIGNATURE_ALG.to_string();
    input.signature = sign_envelope(&input);
    input
}
