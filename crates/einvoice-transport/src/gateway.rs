use std::{error::Error, fmt, time::Duration};

use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize};

/// Service mode used by the MOF gateway upload-notification API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceType {
    Exchange,
    Store,
}

impl ServiceType {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Exchange => "E",
            Self::Store => "S",
        }
    }
}

/// Whether the object uploaded over SFTP was ZIP-compressed before submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZipMode {
    Plain,
    Zip,
}

impl ZipMode {
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Plain => "0",
            Self::Zip => "1",
        }
    }
}

/// Party + routing metadata duplicated between the invoice envelope and PFS001.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayParty {
    pub party_id: String,
    pub party_description: Option<String>,
    pub routing_id: String,
    pub routing_description: Option<String>,
}

/// Secret-bearing transport credentials used both for HTTP Basic authentication
/// and the current PFS001 JSON body.
#[derive(Clone, PartialEq, Eq)]
pub struct GatewayCredentials {
    login_id: String,
    login_password: String,
}

impl GatewayCredentials {
    #[must_use]
    pub fn new(login_id: impl Into<String>, login_password: impl Into<String>) -> Self {
        Self {
            login_id: login_id.into(),
            login_password: login_password.into(),
        }
    }

    #[must_use]
    pub fn login_id(&self) -> &str {
        &self.login_id
    }
}

impl fmt::Debug for GatewayCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayCredentials")
            .field("login_id", &self.login_id)
            .field("login_password", &"[REDACTED]")
            .finish()
    }
}

/// Metadata required to notify the MOF gateway after an SFTP upload.
///
/// Credentials are deliberately absent from this value object. A concrete HTTP
/// client should receive credentials through [`GatewayCredentials`] and use
/// them for both HTTP Basic authentication and the body fields required by the
/// current PFS001 contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadNotification {
    pub from: GatewayParty,
    pub to: GatewayParty,
    pub service_type: ServiceType,
    pub zip_mode: ZipMode,
    pub message_type: String,
    pub action: String,
    pub quantity: u32,
    pub mig_version: String,
    pub filename: String,
    pub size: u64,
    pub retry: u32,
    pub api_version: String,
    pub turnkey_version: String,
}

impl UploadNotification {
    pub const INITIAL_RETRY: u32 = 0;

    /// Encodes the exact PFS001 JSON shape described by the embedded gateway
    /// `OpenAPI` document.
    ///
    /// The gateway schema uses signed 32-bit integers for `quantity`, `size`,
    /// and `retry`; the public model uses non-negative Rust integers and checks
    /// the conversion here rather than silently truncating them.
    ///
    /// # Errors
    ///
    /// Returns [`GatewayEncodeError::IntegerOutOfRange`] when a numeric value
    /// exceeds the gateway `int32` range, or [`GatewayEncodeError::Json`] when
    /// JSON serialization fails.
    pub fn to_pfs001_json(
        &self,
        credentials: &GatewayCredentials,
    ) -> Result<Vec<u8>, GatewayEncodeError> {
        let quantity = checked_i32("quantity", u64::from(self.quantity))?;
        let size = checked_i32("size", self.size)?;
        let retry = checked_i32("retry", u64::from(self.retry))?;

        let request = WireInvoiceEvent {
            login_id: &credentials.login_id,
            login_password: &credentials.login_password,
            trans_from: WireTrans {
                party_info: WirePartyInfo {
                    party_id: &self.from.party_id,
                    description: self.from.party_description.as_deref(),
                },
                routing_info: WireRoutingInfo {
                    routing_id: &self.from.routing_id,
                    description: self.from.routing_description.as_deref(),
                },
            },
            trans_to: WireTrans {
                party_info: WirePartyInfo {
                    party_id: &self.to.party_id,
                    description: self.to.party_description.as_deref(),
                },
                routing_info: WireRoutingInfo {
                    routing_id: &self.to.routing_id,
                    description: self.to.routing_description.as_deref(),
                },
            },
            service_type: self.service_type.wire_value(),
            message_type: &self.message_type,
            action: &self.action,
            quantity,
            mig_version: &self.mig_version,
            filename: &self.filename,
            size,
            zip: self.zip_mode.wire_value(),
            retry,
            api_version: &self.api_version,
            turnkey_version: &self.turnkey_version,
        };

        serde_json::to_vec(&request).map_err(GatewayEncodeError::Json)
    }
}

fn checked_i32(field: &'static str, value: u64) -> Result<i32, GatewayEncodeError> {
    i32::try_from(value).map_err(|_| GatewayEncodeError::IntegerOutOfRange { field, value })
}

#[derive(Debug)]
pub enum GatewayEncodeError {
    IntegerOutOfRange { field: &'static str, value: u64 },
    Json(serde_json::Error),
}

impl fmt::Display for GatewayEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntegerOutOfRange { field, value } => {
                write!(f, "PFS001 field {field} value {value} exceeds int32 range")
            }
            Self::Json(error) => write!(f, "failed to encode PFS001 JSON: {error}"),
        }
    }
}

impl Error for GatewayEncodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IntegerOutOfRange { .. } => None,
            Self::Json(error) => Some(error),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireInvoiceEvent<'a> {
    login_id: &'a str,
    login_password: &'a str,
    trans_from: WireTrans<'a>,
    trans_to: WireTrans<'a>,
    service_type: &'a str,
    message_type: &'a str,
    action: &'a str,
    quantity: i32,
    mig_version: &'a str,
    filename: &'a str,
    size: i32,
    zip: &'a str,
    retry: i32,
    api_version: &'a str,
    turnkey_version: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireTrans<'a> {
    party_info: WirePartyInfo<'a>,
    routing_info: WireRoutingInfo<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePartyInfo<'a> {
    party_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireRoutingInfo<'a> {
    routing_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GatewayStatusDetail {
    pub code: String,
    pub description: Option<String>,
    #[serde(rename = "parameter", default)]
    pub parameters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GatewayProcessStatus {
    #[serde(rename = "statusCode")]
    pub status_code: i32,
    #[serde(rename = "statusDetail", default)]
    pub details: Vec<GatewayStatusDetail>,
}

impl GatewayProcessStatus {
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        self.status_code == 0
    }

    /// Decodes a PFS gateway `ProcessStatus` response.
    ///
    /// # Errors
    ///
    /// Returns a JSON decoding error if the gateway response does not match the
    /// embedded `OpenAPI` response shape.
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Abstraction used by the native submit pipeline to notify the platform after
/// an SFTP object is durably uploaded.
pub trait GatewayNotifier {
    type Error;

    /// Sends one PFS001 upload notification.
    ///
    /// # Errors
    ///
    /// Returns the concrete notifier error when encoding, HTTP transport, HTTP
    /// status handling, or response decoding fails.
    fn notify(
        &self,
        notification: &UploadNotification,
    ) -> Result<GatewayProcessStatus, Self::Error>;
}

/// Blocking PFS001 client suitable for a dedicated Turnkey-compatible worker.
///
/// The current platform contract requires the same transport ID/password in
/// HTTP Basic authentication and in the JSON body. Credentials are retained in
/// memory but are always redacted from `Debug` output.
pub struct Pfs001HttpClient {
    client: Client,
    endpoint: String,
    credentials: GatewayCredentials,
}

impl Pfs001HttpClient {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
    pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

    /// Builds a client from the configured gateway base URL, for example
    /// `https://tgw.einvoice.nat.gov.tw/gateway/api`.
    ///
    /// # Errors
    ///
    /// Returns [`Pfs001HttpError::ClientBuild`] if the HTTP client cannot be
    /// constructed.
    pub fn new(
        base_url: impl AsRef<str>,
        credentials: GatewayCredentials,
    ) -> Result<Self, Pfs001HttpError> {
        Self::with_timeout(base_url, credentials, Self::DEFAULT_TIMEOUT)
    }

    /// Builds a client with an explicit overall request timeout.
    ///
    /// # Errors
    ///
    /// Returns [`Pfs001HttpError::ClientBuild`] if the HTTP client cannot be
    /// constructed.
    pub fn with_timeout(
        base_url: impl AsRef<str>,
        credentials: GatewayCredentials,
        timeout: Duration,
    ) -> Result<Self, Pfs001HttpError> {
        let endpoint = format!(
            "{}/pfs001i/uploadInvoiceMessage",
            base_url.as_ref().trim_end_matches('/')
        );
        let client = Client::builder()
            .connect_timeout(Self::CONNECT_TIMEOUT)
            .timeout(timeout)
            .build()
            .map_err(Pfs001HttpError::ClientBuild)?;

        Ok(Self {
            client,
            endpoint,
            credentials,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl fmt::Debug for Pfs001HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pfs001HttpClient")
            .field("endpoint", &self.endpoint)
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

impl GatewayNotifier for Pfs001HttpClient {
    type Error = Pfs001HttpError;

    fn notify(
        &self,
        notification: &UploadNotification,
    ) -> Result<GatewayProcessStatus, Self::Error> {
        let body = notification
            .to_pfs001_json(&self.credentials)
            .map_err(Pfs001HttpError::Encode)?;

        let response = self
            .client
            .post(&self.endpoint)
            .basic_auth(
                self.credentials.login_id.as_str(),
                Some(self.credentials.login_password.as_str()),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .map_err(Pfs001HttpError::Request)?;

        let status = response.status();
        let response_body = response.bytes().map_err(Pfs001HttpError::Request)?;
        if !status.is_success() {
            return Err(Pfs001HttpError::HttpStatus {
                status,
                body: String::from_utf8_lossy(&response_body).into_owned(),
            });
        }

        GatewayProcessStatus::from_json(&response_body).map_err(Pfs001HttpError::Decode)
    }
}

#[derive(Debug)]
pub enum Pfs001HttpError {
    ClientBuild(reqwest::Error),
    Encode(GatewayEncodeError),
    Request(reqwest::Error),
    HttpStatus { status: StatusCode, body: String },
    Decode(serde_json::Error),
}

impl fmt::Display for Pfs001HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientBuild(error) => write!(f, "failed to build PFS001 HTTP client: {error}"),
            Self::Encode(error) => write!(f, "failed to encode PFS001 request: {error}"),
            Self::Request(error) => write!(f, "PFS001 HTTP request failed: {error}"),
            Self::HttpStatus { status, body } => {
                write!(f, "PFS001 returned HTTP {status}: {body}")
            }
            Self::Decode(error) => write!(f, "failed to decode PFS001 response: {error}"),
        }
    }
}

impl Error for Pfs001HttpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ClientBuild(error) | Self::Request(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::HttpStatus { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification() -> UploadNotification {
        UploadNotification {
            from: GatewayParty {
                party_id: "12345678".into(),
                party_description: Some("sender".into()),
                routing_id: "FROM-VAC".into(),
                routing_description: None,
            },
            to: GatewayParty {
                party_id: "0000000000".into(),
                party_description: None,
                routing_id: "TO-VAC".into(),
                routing_description: Some("receiver route".into()),
            },
            service_type: ServiceType::Store,
            zip_mode: ZipMode::Zip,
            message_type: "F0401".into(),
            action: "C0401".into(),
            quantity: 3,
            mig_version: "4.1".into(),
            filename: "4.1-F0401-20260824-141623456-550e8400-e29b-41d4-a716-446655440000".into(),
            size: 12_345,
            retry: UploadNotification::INITIAL_RETRY,
            api_version: "3.1.3".into(),
            turnkey_version: "3.2.1".into(),
        }
    }

    #[test]
    fn service_type_wire_values_match_gateway_contract() {
        assert_eq!(ServiceType::Exchange.wire_value(), "E");
        assert_eq!(ServiceType::Store.wire_value(), "S");
    }

    #[test]
    fn zip_wire_values_match_gateway_contract() {
        assert_eq!(ZipMode::Plain.wire_value(), "0");
        assert_eq!(ZipMode::Zip.wire_value(), "1");
    }

    #[test]
    fn credentials_debug_output_redacts_password() {
        let credentials = GatewayCredentials::new("transport-id", "super-secret");
        let rendered = format!("{credentials:?}");
        assert!(rendered.contains("transport-id"));
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("super-secret"));
    }

    #[test]
    fn http_client_debug_output_redacts_password_and_normalizes_endpoint() {
        let client = Pfs001HttpClient::new(
            "https://example.invalid/gateway/api/",
            GatewayCredentials::new("transport-id", "super-secret"),
        )
        .unwrap();
        let rendered = format!("{client:?}");

        assert_eq!(
            client.endpoint(),
            "https://example.invalid/gateway/api/pfs001i/uploadInvoiceMessage"
        );
        assert!(rendered.contains("transport-id"));
        assert!(!rendered.contains("super-secret"));
    }

    #[test]
    fn encodes_pfs001_wire_shape() {
        let credentials = GatewayCredentials::new("transport-id", "transport-password");
        let encoded = notification().to_pfs001_json(&credentials).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(value["loginId"], "transport-id");
        assert_eq!(value["loginPassword"], "transport-password");
        assert_eq!(value["transFrom"]["partyInfo"]["partyId"], "12345678");
        assert_eq!(value["transFrom"]["routingInfo"]["routingId"], "FROM-VAC");
        assert_eq!(value["transTo"]["partyInfo"]["partyId"], "0000000000");
        assert_eq!(value["serviceType"], "S");
        assert_eq!(value["zip"], "1");
        assert_eq!(value["quantity"], 3);
        assert_eq!(value["size"], 12_345);
        assert_eq!(value["retry"], 0);
        assert_eq!(value["apiVersion"], "3.1.3");
        assert_eq!(value["turnkeyVersion"], "3.2.1");
    }

    #[test]
    fn rejects_values_outside_gateway_int32_range() {
        let credentials = GatewayCredentials::new("id", "password");
        let mut request = notification();
        request.size = i32::MAX as u64 + 1;

        assert!(matches!(
            request.to_pfs001_json(&credentials),
            Err(GatewayEncodeError::IntegerOutOfRange { field: "size", .. })
        ));
    }

    #[test]
    fn decodes_process_status() {
        let response = br#"{
            "statusCode": 12,
            "statusDetail": [
                {"code": "E001", "description": "invalid", "parameter": ["filename"]}
            ]
        }"#;
        let status = GatewayProcessStatus::from_json(response).unwrap();

        assert!(!status.is_accepted());
        assert_eq!(status.status_code, 12);
        assert_eq!(status.details[0].code, "E001");
        assert_eq!(status.details[0].parameters, ["filename"]);
    }
}
