//! Sends a support bundle to Maintafox support (vendor console `deploy/SUPPORT.md`):
//! declare it (size + SHA-256) → upload the bytes to the presigned URL → ask the server to verify
//! the upload. Uses the sync transport client and the device activation token; the bundle is the
//! same sanitized one the diagnostics dialog shows.

use sha2::{Digest, Sha256};

use crate::sync::transport::client::{ControlPlaneClient, TransportError};
use crate::sync::transport::wire::{
    DiagnosticArtifactRef, DiagnosticBundleCompleted, DiagnosticBundleCreateRequest, DiagnosticTicketRequest,
};

/// Redaction applied to every bundle (`diagnostics::sanitize_log_line`).
pub const BUNDLE_REDACTION_PROFILE: &str = "desktop_log_sanitizer_v1";
pub const BUNDLE_LOGICAL_PATH: &str = "support-bundle.json";
pub const BUNDLE_KIND: &str = "support_bundle_json";
/// Module recorded on a ticket opened from the desktop.
pub const TICKET_MODULE: &str = "desktop";
/// Ticket note length accepted by the server.
pub const TICKET_NOTE_MIN_CHARS: usize = 3;
pub const TICKET_NOTE_MAX_CHARS: usize = 2000;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The ticket to open with the bundle: `None` without a note; an error for a note the server
/// would refuse (too short or too long).
pub fn ticket_from_note(note: Option<&str>) -> Result<Option<DiagnosticTicketRequest>, String> {
    let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    let chars = note.chars().count();
    if !(TICKET_NOTE_MIN_CHARS..=TICKET_NOTE_MAX_CHARS).contains(&chars) {
        return Err(format!(
            "GATE_SUPPORT_NOTE_LENGTH:min={TICKET_NOTE_MIN_CHARS},max={TICKET_NOTE_MAX_CHARS}: The note must be {TICKET_NOTE_MIN_CHARS} to {TICKET_NOTE_MAX_CHARS} characters long."
        ));
    }
    Ok(Some(DiagnosticTicketRequest {
        summary: note.to_string(),
        affected_module: TICKET_MODULE.to_string(),
        severity: "warn".to_string(),
    }))
}

/// Uploads `bytes` (the serialized bundle) and returns the server's verification answer.
pub async fn upload_bundle_bytes(
    client: &ControlPlaneClient,
    token: &str,
    bytes: Vec<u8>,
    app_version: &str,
    ticket: Option<DiagnosticTicketRequest>,
) -> Result<DiagnosticBundleCompleted, TransportError> {
    let sha = sha256_hex(&bytes);
    let request = DiagnosticBundleCreateRequest {
        content_sha256: sha.clone(),
        byte_size: bytes.len() as u64,
        redaction_profile: BUNDLE_REDACTION_PROFILE,
        app_version,
        artifacts: vec![DiagnosticArtifactRef {
            logical_path: BUNDLE_LOGICAL_PATH,
            sha256: sha,
            kind: BUNDLE_KIND,
        }],
        ticket,
    };
    let created = client.create_diagnostic_bundle(token, &request).await?;
    if created.upload.method != "PUT" {
        return Err(TransportError::Contract(format!(
            "diagnostic bundle upload method is '{}', expected PUT",
            created.upload.method
        )));
    }
    client
        .put_object(&created.upload.url, bytes, &std::collections::BTreeMap::new())
        .await?;
    client.complete_diagnostic_bundle(token, &created.bundle_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_becomes_a_ticket_only_when_valid() {
        assert_eq!(ticket_from_note(None).expect("none").map(|t| t.summary), None);
        assert_eq!(ticket_from_note(Some("   ")).expect("blank").map(|t| t.summary), None);
        assert!(ticket_from_note(Some("ab")).is_err());
        assert!(ticket_from_note(Some(&"x".repeat(TICKET_NOTE_MAX_CHARS + 1))).is_err());
        let t = ticket_from_note(Some("  Sync fails since the update  "))
            .expect("ok")
            .expect("ticket");
        assert_eq!(t.summary, "Sync fails since the update");
        assert_eq!(t.affected_module, TICKET_MODULE);
        assert_eq!(t.severity, "warn");
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
