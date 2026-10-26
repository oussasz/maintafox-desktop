//! HTTP client for the control API sync endpoints (reqwest + rustls/ring).
//!
//! The activation token is passed per call by the worker, which reads it in Rust; it never goes
//! through the webview.

use std::time::Duration;

use reqwest::header::{HeaderMap, ACCEPT, CONTENT_TYPE, RETRY_AFTER, USER_AGENT};
use serde::de::DeserializeOwned;

use super::wire::{
    DiagnosticBundleCompleted, DiagnosticBundleCreateRequest, DiagnosticBundleCreated, ErrorBody, ExchangeResponse,
    NotificationEmailAccepted, NotificationEmailRequest, PresignGetRequest, PresignPutRequest, PresignResponse,
    SnapshotRequest, SnapshotResponse,
};
use crate::sync::domain::SYNC_PROTOCOL_VERSION_V2;

/// Longest `Retry-After` the worker honours; a larger value is treated as this one so a
/// misconfigured proxy cannot park sync for days.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

/// Error code of a presigned GET answered 404: the device that owns the file has not uploaded it
/// yet (or it was removed from object storage).
pub const OBJECT_NOT_FOUND: &str = "object_not_found";

/// Longest answer `post_json_raw` reads (a licence answer is a few KiB): a larger one is cut off
/// instead of being held in memory.
pub const RAW_ANSWER_MAX_BYTES: usize = 256 * 1024;

/// Why a request did not produce a usable answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// No connection to the server (DNS failure, refused, unreachable, connect timeout).
    /// Nothing reached the server.
    Offline(String),
    /// The request may have reached the server but no complete answer came back (timeout, reset).
    Interrupted(String),
    /// The server answered with an error status.
    Http {
        status: u16,
        code: Option<String>,
        message: String,
        retry_after: Option<Duration>,
    },
    /// The server answered 2xx with a body that does not follow the contract.
    Contract(String),
}

impl TransportError {
    pub fn code(&self) -> &str {
        match self {
            Self::Offline(_) => "offline",
            Self::Interrupted(_) => "interrupted",
            Self::Http { code: Some(code), .. } => code,
            Self::Http { .. } => "http_error",
            Self::Contract(_) => "invalid_server_response",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Offline(m) | Self::Interrupted(m) | Self::Contract(m) => m.clone(),
            Self::Http { status, message, .. } => format!("HTTP {status}: {message}"),
        }
    }

    pub fn http_status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Timeouts for control API calls.
#[derive(Debug, Clone, Copy)]
pub struct ClientTimeouts {
    pub connect: Duration,
    pub request: Duration,
    /// Whole-request limit for one media upload or download (object storage).
    pub media_transfer: Duration,
}

#[derive(Debug, Clone)]
pub struct ControlPlaneClient {
    http: reqwest::Client,
    base_url: String,
    media_transfer_timeout: Duration,
}

/// `https://…`, or `http://` only for a server on this machine (local development and tests): the
/// bearer token must never cross a network in clear text.
fn validate_base_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(trimmed).map_err(|e| format!("Control API URL '{trimmed}' is invalid: {e}"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "::1"));
    match url.scheme() {
        "https" => Ok(trimmed.to_string()),
        "http" if local => Ok(trimmed.to_string()),
        other => Err(format!(
            "Control API URL '{trimmed}' must use https (scheme '{other}' is only allowed for localhost)."
        )),
    }
}

/// Seconds (`Retry-After: 120`) or an HTTP date.
fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let delay = if let Ok(seconds) = raw.parse::<u64>() {
        Duration::from_secs(seconds)
    } else {
        let at = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
        (at.with_timezone(&chrono::Utc) - chrono::Utc::now())
            .to_std()
            .unwrap_or(Duration::ZERO)
    };
    Some(delay.min(MAX_RETRY_AFTER))
}

fn map_send_error(err: &reqwest::Error) -> TransportError {
    // A connect error (including a connect timeout) means nothing reached the server.
    if err.is_connect() {
        TransportError::Offline(format!("Cannot reach the control API: {err}"))
    } else if err.is_builder() {
        TransportError::Contract(format!("Invalid control API request: {err}"))
    } else {
        TransportError::Interrupted(format!("Control API request interrupted: {err}"))
    }
}

fn http_error(status: u16, text: &str, retry_after: Option<Duration>) -> TransportError {
    let parsed: Option<ErrorBody> = serde_json::from_str(text).ok();
    let code = parsed
        .as_ref()
        .and_then(|b| b.error.clone())
        .filter(|c| !c.trim().is_empty());
    let message = parsed
        .as_ref()
        .and_then(|b| {
            b.message
                .clone()
                .or_else(|| b.details.as_ref().map(serde_json::Value::to_string))
        })
        .unwrap_or_else(|| text.chars().take(800).collect());
    TransportError::Http {
        status,
        code,
        message,
        retry_after,
    }
}

impl ControlPlaneClient {
    pub fn new(base_url: &str, timeouts: ClientTimeouts) -> Result<Self, String> {
        // reqwest is built without a default crypto provider (same as tauri-plugin-updater);
        // install ring once per process. A second install returns Err and is harmless.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let base_url = validate_base_url(base_url)?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, "application/json".parse().map_err(|e| format!("{e}"))?);
        headers.insert(
            USER_AGENT,
            format!("Maintafox-Desktop/{}", env!("CARGO_PKG_VERSION"))
                .parse()
                .map_err(|e| format!("{e}"))?,
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(timeouts.connect)
            .timeout(timeouts.request)
            .build()
            .map_err(|e| format!("Cannot build the sync HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url,
            media_transfer_timeout: timeouts.media_transfer,
        })
    }

    async fn post_json<T: DeserializeOwned>(&self, path: &str, token: &str, body: String) -> Result<T, TransportError> {
        let response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(token)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| map_send_error(&e))?;
        let status = response.status();
        let retry_after = parse_retry_after(response.headers());
        let text = response
            .text()
            .await
            .map_err(|e| TransportError::Interrupted(format!("Control API answer interrupted: {e}")))?;
        if !status.is_success() {
            return Err(http_error(status.as_u16(), &text, retry_after));
        }
        serde_json::from_str(&text)
            .map_err(|e| TransportError::Contract(format!("{path} answer does not follow the sync contract: {e}")))
    }

    /// `POST {path}`, with the bearer token when one is given, answering the status and body as
    /// received, for a caller that reads the server's answer, error bodies included, by its own
    /// contract (the licence activation calls of `commands::product_license`: policy refresh,
    /// preview and claim without a token, tenant initialisation). A request that produced no
    /// answer is an error (`Offline`, `Interrupted`); so is an answer longer than
    /// [`RAW_ANSWER_MAX_BYTES`], cut off there and never read whole (`Contract`).
    pub async fn post_json_raw(
        &self,
        path: &str,
        token: Option<&str>,
        body: String,
    ) -> Result<(u16, String), TransportError> {
        let mut request = self.http.post(format!("{}{path}", self.base_url));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let mut response = request
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| map_send_error(&e))?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| TransportError::Interrupted(format!("Control API answer interrupted: {e}")))?
        {
            if bytes.len() + chunk.len() > RAW_ANSWER_MAX_BYTES {
                return Err(TransportError::Contract(format!(
                    "{path} answer (HTTP {status}) is larger than {RAW_ANSWER_MAX_BYTES} bytes"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
    }

    /// `POST /api/v1/sync/exchange` with an already serialized v2 body (the exact bytes stored
    /// before sending).
    pub async fn exchange(&self, token: &str, body: &str) -> Result<ExchangeResponse, TransportError> {
        let response: ExchangeResponse = self.post_json("/api/v1/sync/exchange", token, body.to_string()).await?;
        if response.protocol_version != SYNC_PROTOCOL_VERSION_V2 {
            return Err(TransportError::Contract(format!(
                "exchange answered protocol '{}' to a v2 request",
                response.protocol_version
            )));
        }
        if response.checkpoint_token.trim().is_empty() || response.server_batch_id.trim().is_empty() {
            return Err(TransportError::Contract(
                "exchange answer has no checkpoint_token or server_batch_id".to_string(),
            ));
        }
        Ok(response)
    }

    /// `POST /api/v1/sync/snapshot` (one page).
    pub async fn snapshot(
        &self,
        token: &str,
        request: &SnapshotRequest<'_>,
    ) -> Result<SnapshotResponse, TransportError> {
        let body = serde_json::to_string(request).map_err(|e| TransportError::Contract(e.to_string()))?;
        let response: SnapshotResponse = self.post_json("/api/v1/sync/snapshot", token, body).await?;
        if response.protocol_version != SYNC_PROTOCOL_VERSION_V2 {
            return Err(TransportError::Contract(format!(
                "snapshot answered protocol '{}' to a v2 request",
                response.protocol_version
            )));
        }
        Ok(response)
    }

    /// `POST /api/v1/sync/media/presign-put`.
    pub async fn presign_put(
        &self,
        token: &str,
        request: &PresignPutRequest<'_>,
    ) -> Result<PresignResponse, TransportError> {
        let body = serde_json::to_string(request).map_err(|e| TransportError::Contract(e.to_string()))?;
        self.post_json("/api/v1/sync/media/presign-put", token, body).await
    }

    /// `POST /api/v1/notifications/email` (contract in `notifications::email`).
    pub async fn send_notification_email(
        &self,
        token: &str,
        request: &NotificationEmailRequest,
    ) -> Result<NotificationEmailAccepted, TransportError> {
        let body = serde_json::to_string(request).map_err(|e| TransportError::Contract(e.to_string()))?;
        self.post_json("/api/v1/notifications/email", token, body).await
    }

    /// `POST /api/v1/sync/media/presign-get`.
    pub async fn presign_get(
        &self,
        token: &str,
        request: &PresignGetRequest<'_>,
    ) -> Result<PresignResponse, TransportError> {
        let body = serde_json::to_string(request).map_err(|e| TransportError::Contract(e.to_string()))?;
        self.post_json("/api/v1/sync/media/presign-get", token, body).await
    }

    /// `POST /api/v1/sync/diagnostic-bundles`: declare a support bundle, get its upload URL.
    pub async fn create_diagnostic_bundle(
        &self,
        token: &str,
        request: &DiagnosticBundleCreateRequest<'_>,
    ) -> Result<DiagnosticBundleCreated, TransportError> {
        let body = serde_json::to_string(request).map_err(|e| TransportError::Contract(e.to_string()))?;
        self.post_json("/api/v1/sync/diagnostic-bundles", token, body).await
    }

    /// `POST /api/v1/sync/diagnostic-bundles/{id}/complete`: the server reads the upload back and
    /// verifies its size and SHA-256.
    pub async fn complete_diagnostic_bundle(
        &self,
        token: &str,
        bundle_id: &str,
    ) -> Result<DiagnosticBundleCompleted, TransportError> {
        if bundle_id.is_empty()
            || !bundle_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(TransportError::Contract(format!("invalid bundle id '{bundle_id}'")));
        }
        self.post_json(
            &format!("/api/v1/sync/diagnostic-bundles/{bundle_id}/complete"),
            token,
            "{}".to_string(),
        )
        .await
    }

    /// Start a download from a presigned object storage URL (no bearer token: the URL is the
    /// credential). The caller reads the body in chunks (`reqwest::Response::chunk`) so a file is
    /// never held whole in memory and an oversized answer can be cut off.
    pub async fn get_object(&self, url: &str) -> Result<reqwest::Response, TransportError> {
        let response = self
            .http
            .get(url)
            .header(ACCEPT, "*/*")
            .timeout(self.media_transfer_timeout)
            .send()
            .await
            .map_err(|e| map_send_error(&e))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let retry_after = parse_retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        Err(TransportError::Http {
            status: status.as_u16(),
            code: Some(if status.as_u16() == 404 {
                OBJECT_NOT_FOUND.to_string()
            } else {
                "object_storage_get_failed".to_string()
            }),
            message: text.chars().take(800).collect(),
            retry_after,
        })
    }

    /// Upload bytes to a presigned object storage URL (no bearer token: the URL is the credential).
    pub async fn put_object(&self, url: &str, bytes: Vec<u8>) -> Result<(), TransportError> {
        let response = self
            .http
            .put(url)
            .timeout(self.media_transfer_timeout)
            .body(bytes)
            .send()
            .await
            .map_err(|e| map_send_error(&e))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let retry_after = parse_retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        Err(TransportError::Http {
            status: status.as_u16(),
            code: Some("object_storage_put_failed".to_string()),
            message: text.chars().take(800).collect(),
            retry_after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_must_be_https_unless_local() {
        assert_eq!(
            validate_base_url("https://api.example.test/").as_deref(),
            Ok("https://api.example.test")
        );
        assert!(validate_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_base_url("http://localhost:3000").is_ok());
        assert!(validate_base_url("http://api.example.test").is_err());
        assert!(validate_base_url("not a url").is_err());
    }

    #[test]
    fn retry_after_seconds_and_cap() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "7".parse().expect("header"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(7)));
        headers.insert(RETRY_AFTER, "999999".parse().expect("header"));
        assert_eq!(parse_retry_after(&headers), Some(MAX_RETRY_AFTER));
        headers.insert(RETRY_AFTER, "soon".parse().expect("header"));
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn error_body_code_and_message() {
        let e = http_error(
            409,
            r#"{"error":"idempotency_key_reused","message":"different body"}"#,
            None,
        );
        assert_eq!(e.code(), "idempotency_key_reused");
        assert_eq!(e.http_status(), Some(409));
        let e = http_error(400, r#"{"error":"validation_error","details":{"x":1}}"#, None);
        assert!(e.message().contains("\"x\":1"));
        let e = http_error(502, "<html>bad gateway</html>", None);
        assert_eq!(e.code(), "http_error");
    }
}
