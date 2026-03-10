use reqwest::StatusCode;
use solana_sdk::pubkey::Pubkey;
use tracing::debug;

use rustyclaw_protocol::{
    ChatMessage, ChatRequest, ChatResponse, CostBreakdown, ModelInfo, PaymentRequired, Role,
    USDC_MINT,
};

use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::signer;
use crate::wallet::Wallet;

/// Client for interacting with a `RustyClawRouter` gateway.
///
/// Handles the x402 payment handshake transparently: sends a probe request,
/// and if the gateway returns 402, signs a payment transaction and retries.
pub struct RustyClawClient {
    wallet: Wallet,
    config: ClientConfig,
    http: reqwest::Client,
}

impl RustyClawClient {
    /// Create a new client with the given wallet and configuration.
    ///
    /// # Errors
    ///
    /// Returns `ClientError::Config` if the HTTP client cannot be built.
    pub fn new(wallet: Wallet, config: ClientConfig) -> Result<Self, ClientError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ClientError::Config(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            wallet,
            config,
            http,
        })
    }

    /// Send a chat completion request with transparent 402 payment handling.
    ///
    /// # Errors
    ///
    /// Returns `ClientError::Gateway` for non-200/402 responses,
    /// `ClientError::Signing` if payment signing fails, or
    /// `ClientError::PaymentRejected` if the gateway rejects the payment.
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ClientError> {
        let url = format!("{}/v1/chat/completions", self.config.gateway_url);

        // Probe with stream: false
        let mut probe_req = req.clone();
        probe_req.stream = false;

        let probe_resp = self.http.post(&url).json(&probe_req).send().await?;
        let status = probe_resp.status();

        match status {
            StatusCode::OK => {
                debug!("gateway returned 200 directly (free/cached model)");
                let body = probe_resp.text().await?;
                serde_json::from_str(&body).map_err(|e| ClientError::ParseError(e.to_string()))
            }
            StatusCode::PAYMENT_REQUIRED => {
                debug!("gateway returned 402, initiating payment");
                let body = probe_resp.text().await?;
                let payment_required: PaymentRequired = serde_json::from_str(&body)
                    .map_err(|e| ClientError::ParseError(format!("invalid 402 body: {e}")))?;
                self.pay_and_resend(&url, &req, &payment_required).await
            }
            _ => {
                let body = probe_resp.text().await.unwrap_or_default();
                Err(ClientError::Gateway {
                    status: status.as_u16(),
                    message: body,
                })
            }
        }
    }

    /// Fetch the list of available models from the gateway.
    ///
    /// # Errors
    ///
    /// Returns `ClientError::Gateway` for non-200 responses or
    /// `ClientError::ParseError` if the response body is malformed.
    pub async fn models(&self) -> Result<Vec<ModelInfo>, ClientError> {
        let url = format!("{}/v1/models", self.config.gateway_url);
        let resp = self.http.get(&url).send().await?;

        let status = resp.status();
        let body = resp.text().await?;

        if status != StatusCode::OK {
            return Err(ClientError::Gateway {
                status: status.as_u16(),
                message: body,
            });
        }
        serde_json::from_str(&body).map_err(|e| ClientError::ParseError(e.to_string()))
    }

    /// Estimate the cost of a chat request by sending a minimal probe.
    ///
    /// Sends a single-message request to trigger a 402 response and extracts
    /// the cost breakdown. The estimate reflects the gateway's per-model pricing,
    /// not a specific token count.
    ///
    /// Returns a zero-cost breakdown if the model is free (200 response).
    ///
    /// # Errors
    ///
    /// Returns `ClientError::Gateway` for unexpected status codes or
    /// `ClientError::ParseError` if the 402 body is malformed.
    pub async fn estimate_cost(&self, model: &str) -> Result<CostBreakdown, ClientError> {
        let url = format!("{}/v1/chat/completions", self.config.gateway_url);

        let probe = ChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "cost estimate probe".to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: None,
            temperature: None,
            top_p: None,
            stream: false,
            tools: None,
            tool_choice: None,
        };

        let resp = self.http.post(&url).json(&probe).send().await?;
        let resp_status = resp.status();
        let body = resp.text().await?;

        match resp_status {
            StatusCode::PAYMENT_REQUIRED => {
                let pr: PaymentRequired = serde_json::from_str(&body)
                    .map_err(|e| ClientError::ParseError(format!("invalid 402 body: {e}")))?;
                Ok(pr.cost_breakdown)
            }
            StatusCode::OK => Ok(CostBreakdown {
                provider_cost: "0.000000".to_string(),
                platform_fee: "0.000000".to_string(),
                total: "0.000000".to_string(),
                currency: "USDC".to_string(),
                fee_percent: 0,
            }),
            _ => Err(ClientError::Gateway {
                status: resp_status.as_u16(),
                message: body,
            }),
        }
    }

    /// Sign a payment for a 402 response and return the encoded
    /// `PAYMENT-SIGNATURE` header value.
    ///
    /// This extracts the signing flow from the 402 handshake so that
    /// external consumers (e.g., the proxy sidecar) can reuse payment
    /// signing without coupling to `ChatRequest`/`ChatResponse` types.
    ///
    /// Applies all security validations (`expected_recipient`, `max_payment_amount`).
    ///
    /// # Errors
    ///
    /// Returns `ClientError::NoCompatibleScheme` if no supported scheme is found,
    /// `ClientError::RecipientMismatch` or `ClientError::AmountExceedsMax` if
    /// security validations fail, or `ClientError::Signing` if transaction
    /// signing fails.
    pub async fn sign_payment_for_402(
        &self,
        payment_required: &PaymentRequired,
    ) -> Result<String, ClientError> {
        let accept = self
            .pick_scheme(&payment_required.accepts)
            .ok_or(ClientError::NoCompatibleScheme)?;

        // Security: validate recipient matches expected (prevents payment redirect)
        if let Some(ref expected) = self.config.expected_recipient {
            if accept.pay_to != *expected {
                return Err(ClientError::RecipientMismatch {
                    expected: expected.clone(),
                    actual: accept.pay_to.clone(),
                });
            }
        }

        let amount_atomic: u64 = accept
            .amount
            .parse()
            .map_err(|e| ClientError::ParseError(format!("invalid amount: {e}")))?;

        // Security: validate amount does not exceed maximum (prevents overcharge)
        if let Some(max) = self.config.max_payment_amount {
            if amount_atomic > max {
                return Err(ClientError::AmountExceedsMax {
                    amount: amount_atomic,
                    max,
                });
            }
        }

        let signed_tx = signer::sign_exact_payment(
            &self.wallet,
            &self.config.rpc_url,
            &self.http,
            &accept.pay_to,
            amount_atomic,
        )
        .await?;

        let payload = signer::build_payment_payload(&payment_required.resource, accept, &signed_tx);
        Ok(signer::encode_payment_header(&payload))
    }

    async fn pay_and_resend(
        &self,
        url: &str,
        req: &ChatRequest,
        payment_required: &PaymentRequired,
    ) -> Result<ChatResponse, ClientError> {
        let payment_header = self.sign_payment_for_402(payment_required).await?;

        debug!("sending paid request");

        let resp = self
            .http
            .post(url)
            .header("PAYMENT-SIGNATURE", &payment_header)
            .json(req)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        match status {
            StatusCode::OK => {
                serde_json::from_str(&body).map_err(|e| ClientError::ParseError(e.to_string()))
            }
            StatusCode::PAYMENT_REQUIRED => Err(ClientError::PaymentRejected(body)),
            _ => Err(ClientError::Gateway {
                status: status.as_u16(),
                message: body,
            }),
        }
    }

    /// Query the USDC-SPL balance of this client's wallet.
    ///
    /// # Errors
    ///
    /// Returns `ClientError::BalanceError` if the RPC call fails.
    pub async fn usdc_balance(&self) -> Result<f64, ClientError> {
        self.usdc_balance_of(&self.wallet.address()).await
    }

    /// Query the USDC-SPL balance of an arbitrary Solana address.
    ///
    /// Returns `0.0` if the associated token account does not exist.
    ///
    /// # Errors
    ///
    /// Returns `ClientError::BalanceError` if the address is invalid or the
    /// RPC call fails for a reason other than "account not found".
    pub async fn usdc_balance_of(&self, address: &str) -> Result<f64, ClientError> {
        let owner: Pubkey = address
            .parse()
            .map_err(|e| ClientError::BalanceError(format!("invalid address: {e}")))?;

        let mint: Pubkey = USDC_MINT
            .parse()
            .map_err(|e| ClientError::BalanceError(format!("invalid USDC mint: {e}")))?;

        let ata = signer::associated_token_address(&owner, &mint);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountBalance",
            "params": [ata.to_string()]
        });

        let resp = self
            .http
            .post(&self.config.rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ClientError::BalanceError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(ClientError::BalanceError(format!(
                "RPC returned HTTP {status}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ClientError::BalanceError(format!("failed to parse RPC response: {e}")))?;

        // Account not found → balance is 0
        if json.get("error").is_some() {
            return Ok(0.0);
        }

        let ui_amount = json["result"]["value"]["uiAmount"]
            .as_f64()
            .unwrap_or(0.0);

        Ok(ui_amount)
    }

    /// Pick the best compatible payment scheme from the 402 accepts list.
    ///
    /// Currently only "exact" (direct transfer) is supported. Escrow signing
    /// is not yet implemented — escrow schemes are filtered out even when
    /// `prefer_escrow` is true. Once `sign_escrow_payment` is added, this
    /// method should respect `config.prefer_escrow` ordering.
    fn pick_scheme<'a>(
        &self,
        accepts: &'a [rustyclaw_protocol::PaymentAccept],
    ) -> Option<&'a rustyclaw_protocol::PaymentAccept> {
        // TODO: respect self.config.prefer_escrow once escrow signing is implemented
        let _ = self.config.prefer_escrow;
        // Only exact scheme is implemented; escrow signing is not yet available
        accepts.iter().find(|a| a.scheme == "exact")
    }
}

impl std::fmt::Debug for RustyClawClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustyClawClient")
            .field("wallet", &self.wallet)
            .field("gateway_url", &self.config.gateway_url)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyclaw_protocol::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_wallet() -> Wallet {
        let (wallet, _) = Wallet::create();
        wallet
    }

    fn sample_chat_request() -> ChatRequest {
        ChatRequest {
            model: "openai/gpt-4o".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "Hello".to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(100),
            temperature: None,
            top_p: None,
            stream: false,
            tools: None,
            tool_choice: None,
        }
    }

    fn sample_payment_required() -> PaymentRequired {
        PaymentRequired {
            x402_version: X402_VERSION,
            resource: Resource {
                url: "/v1/chat/completions".to_string(),
                method: "POST".to_string(),
            },
            accepts: vec![PaymentAccept {
                scheme: "exact".to_string(),
                network: SOLANA_NETWORK.to_string(),
                amount: "2625".to_string(),
                asset: USDC_MINT.to_string(),
                pay_to: solana_sdk::pubkey::Pubkey::new_unique().to_string(),
                max_timeout_seconds: MAX_TIMEOUT_SECONDS,
                escrow_program_id: None,
            }],
            cost_breakdown: CostBreakdown {
                provider_cost: "0.002500".to_string(),
                platform_fee: "0.000125".to_string(),
                total: "0.002625".to_string(),
                currency: "USDC".to_string(),
                fee_percent: PLATFORM_FEE_PERCENT,
            },
            error: "Payment required".to_string(),
        }
    }

    fn sample_chat_response() -> ChatResponse {
        ChatResponse {
            id: "chatcmpl-test123".to_string(),
            object: "chat.completion".to_string(),
            created: 1_234_567_890,
            model: "gpt-4o".to_string(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: Role::Assistant,
                    content: "Hello! How can I help?".to_string(),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 8,
                total_tokens: 18,
            }),
        }
    }

    #[tokio::test]
    async fn test_chat_returns_200_directly_for_free_model() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_chat_response()))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let resp = client.chat(sample_chat_request()).await.unwrap();
        assert_eq!(resp.choices[0].message.content, "Hello! How can I help?");
    }

    #[tokio::test]
    async fn test_chat_handles_402_then_fails_signing_without_rpc() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(402).set_body_json(sample_payment_required()))
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: mock_server.uri(),
                rpc_url: "http://127.0.0.1:1".to_string(),
                timeout: std::time::Duration::from_secs(5),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let result = client.chat(sample_chat_request()).await;
        assert!(result.is_err());
        // Signing fails because the RPC endpoint is unreachable — the reqwest
        // error propagates through SignerError::RpcError → ClientError::Signing.
        assert!(matches!(result.unwrap_err(), ClientError::Signing(_)));
    }

    #[tokio::test]
    async fn test_chat_returns_gateway_error_for_500() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let result = client.chat(sample_chat_request()).await;
        match result.unwrap_err() {
            ClientError::Gateway { status, .. } => assert_eq!(status, 500),
            other => panic!("expected Gateway error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_models_returns_model_list() {
        let mock_server = MockServer::start().await;

        let models = vec![ModelInfo {
            id: "openai/gpt-4o".to_string(),
            provider: "openai".to_string(),
            model_id: "gpt-4o".to_string(),
            display_name: "GPT-4o".to_string(),
            input_cost_per_million: 2.5,
            output_cost_per_million: 10.0,
            context_window: 128_000,
            supports_streaming: true,
            supports_tools: true,
            supports_vision: true,
            reasoning: false,
            supports_structured_output: true,
            supports_batch: false,
            max_output_tokens: Some(16384),
        }];

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&models))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let result = client.models().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "openai/gpt-4o");
    }

    #[tokio::test]
    async fn test_sign_payment_for_402_rejects_unknown_scheme() {
        let mock_server = MockServer::start().await;
        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let mut pr = sample_payment_required();
        pr.accepts[0].scheme = "unknown".to_string();

        let result = client.sign_payment_for_402(&pr).await;
        assert!(matches!(
            result.unwrap_err(),
            ClientError::NoCompatibleScheme
        ));
    }

    #[tokio::test]
    async fn test_usdc_balance_of_parses_rpc_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "context": { "slot": 123_456 },
                    "value": {
                        "amount": "1500000",
                        "decimals": 6,
                        "uiAmount": 1.5,
                        "uiAmountString": "1.5"
                    }
                }
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: "http://unused".to_string(),
                rpc_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let balance = client
            .usdc_balance_of(&client.wallet.address())
            .await
            .unwrap();
        assert!((balance - 1.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_usdc_balance_returns_zero_for_missing_account() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {
                    "code": -32602,
                    "message": "Invalid param: could not find account"
                }
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: "http://unused".to_string(),
                rpc_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let balance = client.usdc_balance().await.unwrap();
        assert!((balance - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_usdc_balance_of_invalid_address() {
        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig::default(),
        )
        .unwrap();

        let result = client.usdc_balance_of("not-a-valid-pubkey").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ClientError::BalanceError(_)));
    }

    #[tokio::test]
    async fn test_estimate_cost_uses_402_probe() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(402).set_body_json(sample_payment_required()))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = RustyClawClient::new(
            test_wallet(),
            ClientConfig {
                gateway_url: mock_server.uri(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let cost = client.estimate_cost("openai/gpt-4o").await.unwrap();
        assert_eq!(cost.total, "0.002625");
        assert_eq!(cost.currency, "USDC");
    }
}
