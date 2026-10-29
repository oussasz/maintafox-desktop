//! A phone joins its plant with the enrolment its administrator issued on the desktop
//! (`commands::mobile_enrolment`): the QR code it scans, or the 8-character code typed instead.
//! Never with the licence key (the licence server refuses a phone that presents one).
//!
//! The QR text is `maintafox-enrol:1?s=<server address>&t=<one-time token>`. It is read strictly
//! (that prefix and version, exactly the two parameters) and refused unless its server address is
//! the licence server this app was built for: the token is never sent anywhere else. A typed code
//! is put in the server's canonical form (case, spaces and dashes ignored; I and L read as 1, O as
//! 0) and refused before any call unless it is 8 characters of the code alphabet.
//!
//! The engine sends the redemption itself, with this phone's own identity (machine fingerprint,
//! label, app version). The answer is a licence claim (signed envelope, device token) plus the
//! person and site the phone is now bound to; it is recorded through the key entry's own path
//! (`record_claim_locked`: licence rule, envelope, tenant bootstrap) with the binding and without
//! any licence key. Only a phone in its first-run state may redeem: an activated phone, one whose
//! plant has an administrator here, or one being erased is refused before the call.
//!
//! The token and the code never reach a log or an error text (`Debug` hides them, every failure
//! text has them replaced), and the device token of the answer stays in Rust.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::{
    answered_token, caller_of, canonical_id, complete_claim_of_answer, invalid_answer, load_state_record_from_db,
    plant_has_administrator, post_activation_call, pre_administered_by_builtin_account, record_claim_locked,
    record_has_valid_activation_claim, running_app_version, ActivationCallFailure, ClaimOrigin, LicenceCaller,
    MobileEnrolmentBinding, LICENCE_RECORD_LOCK, TOKEN_MARKER,
};
use crate::commands::mobile_enrolment::QR_PAYLOAD_PREFIX;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;

/// Redeems an enrolment: the phone's activation bound to the enrolment's person and site.
const REDEEM_PATH: &str = "/api/v1/mobile-enrolments/redeem";

/// What replaces the QR token in a failure text.
const ENROLMENT_TOKEN_MARKER: &str = "[enrolment token]";
/// What replaces the typed code in a failure text.
const ENROLMENT_CODE_MARKER: &str = "[enrolment code]";

/// The typed code: 8 characters of Crockford base32 (the server's `CODE_ALPHABET`).
const CODE_ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_LENGTH: usize = 8;
/// The QR token: base64url, as many characters as the server accepts.
const TOKEN_MIN_CHARS: usize = 16;
const TOKEN_MAX_CHARS: usize = 128;
/// Longest QR text or typed code read at all (a real one is about 100 characters).
const INPUT_MAX_CHARS: usize = 1024;
/// Longest identifier or name of the binding kept from the answer.
const BINDING_FIELD_MAX_CHARS: usize = 256;

/// The name this phone gets in the plant's phone list (the person's name is shown first there).
const PHONE_LABEL: &str = "Mobile (Android)";

// ── Refusal codes (stable; the phone screens translate them) ─────────────────────────────────────

/// Neither or both of the QR text and the code were given.
pub const ENROLMENT_INPUT_REQUIRED: &str = "ENROLMENT_INPUT_REQUIRED";
/// The QR text is not a Maintafox enrolment QR (prefix, version, parameters, token).
pub const ENROLMENT_QR_INVALID: &str = "ENROLMENT_QR_INVALID";
/// The QR names another server than the one this app was built for: nothing was sent.
pub const ENROLMENT_QR_OTHER_SERVER: &str = "ENROLMENT_QR_OTHER_SERVER";
/// The typed code cannot be an enrolment code.
pub const ENROLMENT_CODE_INVALID: &str = "ENROLMENT_CODE_INVALID";
/// A desktop: desktops use the licence key.
pub const ENROLMENT_DESKTOP_REFUSED: &str = "ENROLMENT_DESKTOP_REFUSED";
/// This phone is already activated.
pub const ENROLMENT_ALREADY_ACTIVATED: &str = "ENROLMENT_ALREADY_ACTIVATED";
/// This phone still holds a plant's data (an administrator account): it must be reset first.
pub const ENROLMENT_PHONE_NOT_EMPTY: &str = "ENROLMENT_PHONE_NOT_EMPTY";
/// This phone is uploading its work and erasing itself (`sync::transport::removal`).
pub const ENROLMENT_REMOVAL_IN_PROGRESS: &str = "ENROLMENT_REMOVAL_IN_PROGRESS";
/// The phone's installation identity cannot be read (secure store unavailable).
pub const ENROLMENT_DEVICE_ID_UNAVAILABLE: &str = "ENROLMENT_DEVICE_ID_UNAVAILABLE";
/// Unknown, used, cancelled or expired enrolment (the server does not say which).
pub const ENROLMENT_INVALID: &str = "ENROLMENT_INVALID";
/// This phone is enrolled and active on the server: its administrator must remove it first.
pub const ENROLMENT_ALREADY_ENROLLED: &str = "ENROLMENT_ALREADY_ENROLLED";
/// This phone was removed and has not finished erasing itself on the server.
pub const ENROLMENT_WIPE_PENDING: &str = "ENROLMENT_WIPE_PENDING";
/// Licence refusals, as for a licence claim (`server_code` gives the server's own code).
pub const ENROLMENT_LICENCE_REVOKED: &str = "ENROLMENT_LICENCE_REVOKED";
pub const ENROLMENT_LICENCE_EXPIRED: &str = "ENROLMENT_LICENCE_EXPIRED";
pub const ENROLMENT_LICENCE_INVALID: &str = "ENROLMENT_LICENCE_INVALID";
pub const ENROLMENT_NOT_LICENSED: &str = "ENROLMENT_NOT_LICENSED";
pub const ENROLMENT_NO_FREE_SLOT: &str = "ENROLMENT_NO_FREE_SLOT";
pub const ENROLMENT_DEVICE_CLASS_MISMATCH: &str = "ENROLMENT_DEVICE_CLASS_MISMATCH";
/// Too many attempts: wait before trying again.
pub const ENROLMENT_RATE_LIMITED: &str = "ENROLMENT_RATE_LIMITED";
/// No answer (offline, connection refused, timeout, interrupted).
pub const ENROLMENT_OFFLINE: &str = "ENROLMENT_OFFLINE";
/// The server failed (5xx).
pub const ENROLMENT_SERVER_UNAVAILABLE: &str = "ENROLMENT_SERVER_UNAVAILABLE";
/// Any other refusal (`status`, `server_code` params).
pub const ENROLMENT_SERVER_REFUSED: &str = "ENROLMENT_SERVER_REFUSED";
/// A 2xx answer that is not an enrolment (claim or binding missing or malformed).
pub const ENROLMENT_INVALID_ANSWER: &str = "ENROLMENT_INVALID_ANSWER";

fn refused(code: &str, message: &str) -> AppError {
    crate::errors::org_validation_failed(code, message, std::collections::HashMap::new())
}

fn refused_with(code: &str, message: &str, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(
        code,
        message,
        params.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
    )
}

// ── Input ───────────────────────────────────────────────────────────────────────────────────────

/// What the phone screen gives: the scanned QR text, or the typed code. Exactly one.
#[derive(Deserialize)]
pub struct RedeemMobileEnrolmentInput {
    #[serde(default)]
    pub qr_payload: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

/// Never the QR text (it holds the token) nor the code.
impl std::fmt::Debug for RedeemMobileEnrolmentInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedeemMobileEnrolmentInput")
            .field("qr_payload", &self.qr_payload.as_ref().map(|_| "[redacted]"))
            .field("code", &self.code.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

/// The one-time secret sent to the server.
#[derive(Clone, PartialEq, Eq)]
enum EnrolmentSecret {
    /// The QR token.
    Token(String),
    /// The typed code, canonical.
    Code(String),
}

impl std::fmt::Debug for EnrolmentSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Token(_) => f.write_str("Token([redacted])"),
            Self::Code(_) => f.write_str("Code([redacted])"),
        }
    }
}

impl EnrolmentSecret {
    fn redaction(&self) -> (&str, &'static str) {
        match self {
            Self::Token(token) => (token.as_str(), ENROLMENT_TOKEN_MARKER),
            Self::Code(code) => (code.as_str(), ENROLMENT_CODE_MARKER),
        }
    }
}

/// `%XX` decoding of a QR parameter as the desktop writes it (`mobile_enrolment::percent_encode`:
/// unreserved characters as they are, everything else `%XX`). Anything else (a raw reserved
/// character, a `+`, a bad escape, text that is not UTF-8) is refused.
fn percent_decode_strict(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = raw.get(i + 1..i + 3)?;
                if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b @ (b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~') => {
                out.push(b);
                i += 1;
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

/// A token as the server issues it: base64url without padding, 16 to 128 characters.
fn well_formed_token(token: &str) -> bool {
    (TOKEN_MIN_CHARS..=TOKEN_MAX_CHARS).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The token of an enrolment QR, refused unless the QR is exactly `maintafox-enrol:1?s=…&t=…`
/// (each parameter once, nothing else) and names `api_base`, the server this app was built for
/// (compared after decoding, trailing slashes removed on both sides, otherwise exactly).
fn token_of_qr(payload: &str, api_base: &str) -> AppResult<String> {
    let invalid = || {
        refused(
            ENROLMENT_QR_INVALID,
            "Ce QR code n'est pas un QR code d'inscription Maintafox.",
        )
    };
    let query = payload.trim().strip_prefix(QR_PAYLOAD_PREFIX).ok_or_else(invalid)?;
    let mut server: Option<&str> = None;
    let mut token: Option<&str> = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
        let slot = match key {
            "s" => &mut server,
            "t" => &mut token,
            _ => return Err(invalid()),
        };
        if slot.is_some() || value.is_empty() {
            return Err(invalid());
        }
        *slot = Some(value);
    }
    let (Some(server), Some(token)) = (server, token) else {
        return Err(invalid());
    };
    let server = percent_decode_strict(server).ok_or_else(invalid)?;
    let token = percent_decode_strict(token).ok_or_else(invalid)?;
    if !well_formed_token(&token) {
        return Err(invalid());
    }
    let expected = api_base.trim().trim_end_matches('/');
    if expected.is_empty() || server.trim_end_matches('/') != expected {
        tracing::warn!(
            event = "mobile_enrolment_qr_other_server",
            "Enrolment QR names another server than this app's; nothing was sent"
        );
        return Err(refused(
            ENROLMENT_QR_OTHER_SERVER,
            "Ce QR code désigne un autre serveur que celui de cette application : rien n'a été envoyé.",
        ));
    }
    Ok(token)
}

/// What a person typed, in the server's canonical form (`normaliseEnrolmentCode`): case, white
/// space and dashes ignored, I and L read as 1, O as 0. `None` unless that is exactly 8 characters
/// of the code alphabet (anything outside ASCII is refused).
fn normalise_enrolment_code(typed: &str) -> Option<String> {
    let mut code = String::with_capacity(CODE_LENGTH);
    for c in typed.chars() {
        if c.is_whitespace() || c == '-' {
            continue;
        }
        let c = match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        if !CODE_ALPHABET.contains(c) || code.len() == CODE_LENGTH {
            return None;
        }
        code.push(c);
    }
    (code.len() == CODE_LENGTH).then_some(code)
}

/// The secret to send for `input`, checked before any network call.
fn secret_of_input(input: &RedeemMobileEnrolmentInput, api_base: &str) -> AppResult<EnrolmentSecret> {
    fn given(value: Option<&str>) -> Option<&str> {
        value.filter(|s| !s.trim().is_empty())
    }
    match (given(input.qr_payload.as_deref()), given(input.code.as_deref())) {
        (Some(payload), None) => {
            if payload.chars().count() > INPUT_MAX_CHARS {
                return Err(refused(
                    ENROLMENT_QR_INVALID,
                    "Ce QR code n'est pas un QR code d'inscription Maintafox.",
                ));
            }
            token_of_qr(payload, api_base).map(EnrolmentSecret::Token)
        }
        (None, Some(code)) => Some(code)
            .filter(|c| c.chars().count() <= INPUT_MAX_CHARS)
            .and_then(normalise_enrolment_code)
            .map(EnrolmentSecret::Code)
            .ok_or_else(|| {
                refused(
                    ENROLMENT_CODE_INVALID,
                    "Ce code n'est pas un code d'inscription : il compte 8 caractères (XXXX-XXXX).",
                )
            }),
        _ => Err(refused(
            ENROLMENT_INPUT_REQUIRED,
            "Scannez le QR code ou saisissez le code d'inscription (l'un ou l'autre).",
        )),
    }
}

// ── This phone ──────────────────────────────────────────────────────────────────────────────────

/// What the phone tells the server about itself. Supplied by the engine, never by the webview.
#[derive(Debug, Clone)]
pub(crate) struct PhoneIdentity {
    pub machine_fingerprint: String,
    pub machine_label: String,
    pub app_version: String,
}

/// This phone's identity: the fingerprint of its installation (`auth::device`, the same one sign-in
/// and device trust use), its label and the running version.
fn this_phone() -> AppResult<PhoneIdentity> {
    let machine_fingerprint = crate::auth::device::derive_device_fingerprint().map_err(|e| {
        tracing::warn!(error = %e, "mobile enrolment: installation identity unavailable");
        refused(
            ENROLMENT_DEVICE_ID_UNAVAILABLE,
            "L'identité de ce téléphone est illisible : vérifiez que le verrouillage d'écran est actif.",
        )
    })?;
    Ok(PhoneIdentity {
        machine_fingerprint,
        machine_label: PHONE_LABEL.to_string(),
        app_version: running_app_version().to_string(),
    })
}

fn redeem_body(secret: &EnrolmentSecret, phone: &PhoneIdentity) -> serde_json::Value {
    let mut body = serde_json::json!({
        "machine_fingerprint": phone.machine_fingerprint,
        "machine_label": phone.machine_label,
        "device_class": "mobile",
    });
    match secret {
        EnrolmentSecret::Token(token) => body["token"] = serde_json::Value::from(token.as_str()),
        EnrolmentSecret::Code(code) => body["code"] = serde_json::Value::from(code.as_str()),
    }
    // As every licence call: only a version the server accepts (without one a forced update
    // applies as if this phone were out of date).
    if super::well_formed_app_version(&phone.app_version) {
        body["app_version"] = serde_json::Value::from(phone.app_version.as_str());
    }
    body
}

// ── Answer ──────────────────────────────────────────────────────────────────────────────────────

/// The binding of a redeem answer (`enrolment`), or why there is none: every field a non-empty
/// string without surrounding spaces, at most [`BINDING_FIELD_MAX_CHARS`], and `enrolled_at` a date.
fn binding_of_answer(body: &str) -> Result<MobileEnrolmentBinding, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("The redeem answer is not JSON: {e}"))?;
    let enrolment = value
        .get("enrolment")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "The redeem answer has no enrolment binding.".to_string())?;
    let field = |name: &str| -> Result<String, String> {
        enrolment
            .get(name)
            .and_then(serde_json::Value::as_str)
            .filter(|v| canonical_id(v) && v.chars().count() <= BINDING_FIELD_MAX_CHARS)
            .map(str::to_string)
            .ok_or_else(|| format!("The redeem answer's enrolment binding has no valid {name}."))
    };
    let binding = MobileEnrolmentBinding {
        enrolment_id: field("enrolment_id")?,
        person_sync_id: field("person_sync_id")?,
        person_label: field("person_label")?,
        site_sync_id: field("site_sync_id")?,
        site_label: field("site_label")?,
        enrolled_at: field("enrolled_at")?,
    };
    chrono::DateTime::parse_from_rfc3339(&binding.enrolled_at)
        .map_err(|_| "The redeem answer's enrolment date is not a date.".to_string())?;
    Ok(binding)
}

/// A failed redemption as the phone screen shows it. Texts never carry the secret
/// (`post_activation_call` replaced it).
fn refusal_of(failure: ActivationCallFailure) -> AppError {
    match failure {
        ActivationCallFailure::NetworkError { .. } => refused(
            ENROLMENT_OFFLINE,
            "Le serveur de licences ne répond pas : vérifiez la connexion et réessayez.",
        ),
        ActivationCallFailure::InvalidResponse { .. } => refused(
            ENROLMENT_INVALID_ANSWER,
            "La réponse du serveur de licences est incomplète : rien n'a été enregistré sur ce téléphone.",
        ),
        ActivationCallFailure::HttpError { status, code, .. } => {
            let server_code = code.unwrap_or_default();
            let params = [("status", status.to_string()), ("server_code", server_code.clone())];
            let (rule, message) = match (status, server_code.as_str()) {
                (_, "enrolment_invalid") => (
                    ENROLMENT_INVALID,
                    "Ce QR code ou ce code n'est pas valable : inconnu, déjà utilisé, annulé ou expiré. \
                     Demandez-en un nouveau à votre administrateur.",
                ),
                (_, "already_enrolled") => (
                    ENROLMENT_ALREADY_ENROLLED,
                    "Ce téléphone est déjà inscrit : votre administrateur doit d'abord le retirer.",
                ),
                (_, "wipe_pending") => (
                    ENROLMENT_WIPE_PENDING,
                    "Ce téléphone a été retiré et ne s'est pas encore effacé : ouvrez l'application en ligne \
                     pour qu'il termine, puis réessayez.",
                ),
                (_, "license_revoked" | "activation_revoked") => {
                    (ENROLMENT_LICENCE_REVOKED, "La licence ou ce téléphone a été révoqué.")
                }
                (_, "license_expired") => (ENROLMENT_LICENCE_EXPIRED, "La licence a expiré."),
                (_, "license_not_found" | "tenant_inactive") => {
                    (ENROLMENT_LICENCE_INVALID, "La licence de l'usine n'est pas active.")
                }
                (_, "mobile_not_licensed") => (
                    ENROLMENT_NOT_LICENSED,
                    "La licence n'inclut pas de téléphones : demandez à votre fournisseur de les ajouter.",
                ),
                (_, "slot_limit_reached") => (
                    ENROLMENT_NO_FREE_SLOT,
                    "Tous les téléphones de la licence sont utilisés : un administrateur doit en retirer un.",
                ),
                (_, "device_class_mismatch") => (
                    ENROLMENT_DEVICE_CLASS_MISMATCH,
                    "Cet appareil est enregistré comme poste de travail, pas comme téléphone.",
                ),
                (429, _) => (
                    ENROLMENT_RATE_LIMITED,
                    "Trop de tentatives : patientez quelques minutes avant de réessayer.",
                ),
                (500..=599, _) => (
                    ENROLMENT_SERVER_UNAVAILABLE,
                    "Le serveur de licences est momentanément indisponible : réessayez plus tard.",
                ),
                _ => (
                    ENROLMENT_SERVER_REFUSED,
                    "Le serveur de licences a refusé l'inscription.",
                ),
            };
            refused_with(rule, message, &params)
        }
    }
}

/// What the phone screen gets of a redemption: the plant and who the phone is for. No secret.
#[derive(Debug, Clone, Serialize)]
pub struct MobileEnrolmentRedeemed {
    pub tenant_id: String,
    pub company_display_name: Option<String>,
    pub person_sync_id: String,
    pub person_label: String,
    pub site_sync_id: String,
    pub site_label: String,
    pub enrolled_at: String,
}

/// Refused unless this phone is in its first-run state: not activated, no administrator of a plant
/// here (a claim recorded signed out would be refused after the server spent the enrolment), not
/// being erased.
async fn ensure_first_run(db: &sea_orm::DatabaseConnection) -> AppResult<()> {
    if crate::sync::transport::removal::pending(db).await?.is_some() {
        return Err(refused(
            ENROLMENT_REMOVAL_IN_PROGRESS,
            "Ce téléphone envoie son travail en attente puis s'efface : attendez la fin avant de l'inscrire.",
        ));
    }
    let current = load_state_record_from_db(db).await?;
    if current.as_ref().is_some_and(record_has_valid_activation_claim) {
        return Err(refused(ENROLMENT_ALREADY_ACTIVATED, "Ce téléphone est déjà activé."));
    }
    if plant_has_administrator(db).await? || pre_administered_by_builtin_account(db, current.as_ref()).await? {
        return Err(refused(
            ENROLMENT_PHONE_NOT_EMPTY,
            "Ce téléphone contient encore les données d'une usine : réinitialisez-le avant de l'inscrire.",
        ));
    }
    Ok(())
}

/// Redeems the enrolment of `input` on the licence server at `api_base` for `phone`, then records
/// the activation with its binding for `caller`. See the module comment for every check.
pub(crate) async fn redeem_mobile_enrolment_in(
    db: &sea_orm::DatabaseConnection,
    caller: LicenceCaller,
    api_base: &str,
    input: &RedeemMobileEnrolmentInput,
    phone: &PhoneIdentity,
) -> AppResult<MobileEnrolmentRedeemed> {
    let secret = secret_of_input(input, api_base)?;
    ensure_first_run(db).await?;
    let body = redeem_body(&secret, phone);
    let sent = secret.redaction();
    let answer = match post_activation_call(api_base, REDEEM_PATH, None, &body, &[sent]).await? {
        Ok(answer) => answer,
        // Logged by `post_activation_call`, the secret replaced.
        Err(failure) => return Err(refusal_of(failure)),
    };
    let token = answered_token(&answer);
    let secrets = [sent, (token.as_str(), TOKEN_MARKER)];
    let (claim, envelope) = complete_claim_of_answer(&answer, &phone.machine_fingerprint)
        .map_err(|why| refusal_of(invalid_answer(REDEEM_PATH, why, &secrets)))?;
    let binding = binding_of_answer(&answer).map_err(|why| refusal_of(invalid_answer(REDEEM_PATH, why, &secrets)))?;

    let _record_lock = LICENCE_RECORD_LOCK.lock().await;
    // Again under the lock: nothing may have activated this phone while the call ran.
    ensure_first_run(db).await?;
    let redeemed = MobileEnrolmentRedeemed {
        tenant_id: claim.tenant_id.clone(),
        company_display_name: claim.tenant_display_name.clone(),
        person_sync_id: binding.person_sync_id.clone(),
        person_label: binding.person_label.clone(),
        site_sync_id: binding.site_sync_id.clone(),
        site_label: binding.site_label.clone(),
        enrolled_at: binding.enrolled_at.clone(),
    };
    let enrolment_id = binding.enrolment_id.clone();
    record_claim_locked(
        db,
        caller,
        ClaimOrigin::Enrolment(binding),
        Some(claim),
        envelope,
        Some(phone.machine_fingerprint.clone()),
        Some(phone.app_version.clone()),
    )
    .await?;
    tracing::info!(
        event = "mobile_enrolment_redeemed",
        enrolment_id = enrolment_id.as_str(),
        tenant_id = redeemed.tenant_id.as_str(),
        person_sync_id = redeemed.person_sync_id.as_str(),
        site_sync_id = redeemed.site_sync_id.as_str(),
        "Phone enrolled and activated"
    );
    Ok(redeemed)
}

/// Enrols this phone with its administrator's QR code (`qr_payload`) or typed code (`code`),
/// exactly one. Android only: a desktop is refused (`ENROLMENT_DESKTOP_REFUSED`), it uses the
/// licence key. Needs no session (the phone is not activated yet).
#[tauri::command]
pub async fn redeem_mobile_enrolment(
    input: RedeemMobileEnrolmentInput,
    state: State<'_, AppState>,
) -> AppResult<MobileEnrolmentRedeemed> {
    if !cfg!(target_os = "android") {
        return Err(refused(
            ENROLMENT_DESKTOP_REFUSED,
            "Un poste de travail s'active avec la clé de licence, pas avec un QR code d'inscription.",
        ));
    }
    let phone = this_phone()?;
    let caller = caller_of(&state).await?;
    redeem_mobile_enrolment_in(
        &state.db,
        caller,
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        &input,
        &phone,
    )
    .await
}

#[cfg(test)]
#[path = "phone_enrolment_tests.rs"]
mod tests;
