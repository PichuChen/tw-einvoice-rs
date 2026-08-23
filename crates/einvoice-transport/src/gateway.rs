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

/// Metadata required to notify the MOF gateway after an SFTP upload.
///
/// Credentials are deliberately absent from this value object. A concrete HTTP
/// client should receive credentials through a secret-bearing configuration
/// type and use them for both HTTP Basic authentication and the body fields
/// required by the current PFS001 contract.
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayStatusDetail {
    pub code: String,
    pub description: Option<String>,
    pub parameters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayProcessStatus {
    pub status_code: i32,
    pub details: Vec<GatewayStatusDetail>,
}

impl GatewayProcessStatus {
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        self.status_code == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
