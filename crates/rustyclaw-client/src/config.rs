use std::time::Duration;

/// Configuration for the `RustyClawClient`.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Gateway URL (e.g., `http://localhost:8402`).
    pub gateway_url: String,
    /// Solana RPC URL.
    pub rpc_url: String,
    /// Prefer escrow payment scheme over exact (safer for agents).
    /// Currently defaults to `false` because escrow signing is not yet implemented.
    pub prefer_escrow: bool,
    /// Request timeout.
    pub timeout: Duration,
    /// Optional expected recipient wallet address. If set, the client will
    /// reject 402 responses that specify a different `pay_to` address,
    /// preventing payment redirect attacks by malicious gateways.
    pub expected_recipient: Option<String>,
    /// Maximum payment amount in atomic USDC units. If set, the client will
    /// reject 402 responses that request more than this amount, preventing
    /// overcharge attacks by malicious gateways.
    pub max_payment_amount: Option<u64>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            gateway_url: "http://localhost:8402".to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            prefer_escrow: false,
            timeout: Duration::from_secs(180),
            expected_recipient: None,
            max_payment_amount: None,
        }
    }
}

/// Builder for constructing a `ClientConfig`.
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    gateway_url: Option<String>,
    rpc_url: Option<String>,
    prefer_escrow: Option<bool>,
    timeout: Option<Duration>,
    expected_recipient: Option<String>,
    max_payment_amount: Option<u64>,
}

impl ClientBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            gateway_url: None,
            rpc_url: None,
            prefer_escrow: None,
            timeout: None,
            expected_recipient: None,
            max_payment_amount: None,
        }
    }

    #[must_use]
    pub fn gateway_url(mut self, url: &str) -> Self {
        self.gateway_url = Some(url.trim_end_matches('/').to_string());
        self
    }

    #[must_use]
    pub fn rpc_url(mut self, url: &str) -> Self {
        self.rpc_url = Some(url.to_string());
        self
    }

    #[must_use]
    pub fn prefer_escrow(mut self, prefer: bool) -> Self {
        self.prefer_escrow = Some(prefer);
        self
    }

    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    #[must_use]
    pub fn expected_recipient(mut self, recipient: &str) -> Self {
        self.expected_recipient = Some(recipient.to_string());
        self
    }

    #[must_use]
    pub fn max_payment_amount(mut self, max: u64) -> Self {
        self.max_payment_amount = Some(max);
        self
    }

    /// Build a `ClientConfig` from the builder state, using defaults for unset values.
    #[must_use]
    pub fn build_config(self) -> ClientConfig {
        let defaults = ClientConfig::default();
        ClientConfig {
            gateway_url: self.gateway_url.unwrap_or(defaults.gateway_url),
            rpc_url: self.rpc_url.unwrap_or(defaults.rpc_url),
            prefer_escrow: self.prefer_escrow.unwrap_or(defaults.prefer_escrow),
            timeout: self.timeout.unwrap_or(defaults.timeout),
            expected_recipient: self.expected_recipient.or(defaults.expected_recipient),
            max_payment_amount: self.max_payment_amount.or(defaults.max_payment_amount),
        }
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ClientConfig::default();
        assert_eq!(config.gateway_url, "http://localhost:8402");
        assert_eq!(config.rpc_url, "https://api.mainnet-beta.solana.com");
        assert!(!config.prefer_escrow);
        assert_eq!(config.timeout, Duration::from_secs(180));
    }

    #[test]
    fn test_builder_defaults() {
        let config = ClientBuilder::new().build_config();
        assert_eq!(config.gateway_url, "http://localhost:8402");
        assert!(!config.prefer_escrow);
    }

    #[test]
    fn test_builder_custom_values() {
        let config = ClientBuilder::new()
            .gateway_url("https://my-gateway.fly.dev")
            .rpc_url("https://my-rpc.com")
            .prefer_escrow(false)
            .timeout(Duration::from_secs(60))
            .build_config();

        assert_eq!(config.gateway_url, "https://my-gateway.fly.dev");
        assert_eq!(config.rpc_url, "https://my-rpc.com");
        assert!(!config.prefer_escrow);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_builder_gateway_url_trailing_slash_stripped() {
        let config = ClientBuilder::new()
            .gateway_url("https://my-gateway.fly.dev/")
            .build_config();
        assert_eq!(config.gateway_url, "https://my-gateway.fly.dev");
    }

    #[test]
    fn test_builder_expected_recipient() {
        let config = ClientBuilder::new()
            .expected_recipient("RecipientWalletAddress")
            .build_config();
        assert_eq!(
            config.expected_recipient.as_deref(),
            Some("RecipientWalletAddress")
        );
    }

    #[test]
    fn test_builder_max_payment_amount() {
        let config = ClientBuilder::new()
            .max_payment_amount(100_000)
            .build_config();
        assert_eq!(config.max_payment_amount, Some(100_000));
    }

    #[test]
    fn test_default_config_security_fields_none() {
        let config = ClientConfig::default();
        assert!(config.expected_recipient.is_none());
        assert!(config.max_payment_amount.is_none());
    }
}
