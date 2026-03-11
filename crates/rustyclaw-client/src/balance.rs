use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use solana_sdk::pubkey::Pubkey;
use tracing::{debug, error, warn};

use rustyclaw_protocol::USDC_MINT;

use crate::signer;

/// Default poll interval: 30 seconds.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Default low balance threshold: 0.10 USDC.
const DEFAULT_LOW_BALANCE_THRESHOLD: f64 = 0.10;

/// Background USDC balance monitor that polls at a configurable interval.
///
/// Shares an `Arc<AtomicU64>` with `RustyClawClient` for lock-free balance reads.
/// The monitor writes updated balances; the client reads them.
///
/// # Usage
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use std::sync::atomic::AtomicU64;
/// use rustyclaw_client::BalanceMonitor;
///
/// # async fn example() {
/// // let client = RustyClawClient::new(wallet, config).unwrap();
/// // let monitor = BalanceMonitor::new(
/// //     client.balance_state(),
/// //     "https://api.mainnet-beta.solana.com",
/// //     &client_address,
/// // )
/// // .on_low_balance(|bal| eprintln!("Low balance: {bal} USDC"))
/// // .build();
/// //
/// // tokio::spawn(monitor.run());
/// # }
/// ```
pub struct BalanceMonitor {
    balance_state: Arc<AtomicU64>,
    rpc_url: String,
    wallet_address: String,
    poll_interval: Duration,
    low_balance_threshold: f64,
    on_low_balance: Option<Box<dyn Fn(f64) + Send + Sync + 'static>>,
}

impl BalanceMonitor {
    /// Create a new `BalanceMonitor` with default settings.
    ///
    /// - `balance_state`: shared atomic from `RustyClawClient::balance_state()`
    /// - `rpc_url`: Solana RPC endpoint
    /// - `wallet_address`: base58 Solana address to monitor
    #[must_use]
    pub fn new(balance_state: Arc<AtomicU64>, rpc_url: &str, wallet_address: &str) -> Self {
        Self {
            balance_state,
            rpc_url: rpc_url.to_string(),
            wallet_address: wallet_address.to_string(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            low_balance_threshold: DEFAULT_LOW_BALANCE_THRESHOLD,
            on_low_balance: None,
        }
    }

    /// Set the poll interval (default: 30 seconds).
    #[must_use]
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set the low balance threshold in USDC (default: 0.10).
    #[must_use]
    pub fn low_balance_threshold(mut self, threshold: f64) -> Self {
        self.low_balance_threshold = threshold;
        self
    }

    /// Set a callback that fires when the balance drops below the threshold.
    ///
    /// The callback receives the current balance in USDC.
    #[must_use]
    pub fn on_low_balance<F>(mut self, callback: F) -> Self
    where
        F: Fn(f64) + Send + Sync + 'static,
    {
        self.on_low_balance = Some(Box::new(callback));
        self
    }

    /// Run the monitor loop. This future never completes — spawn it with `tokio::spawn`.
    ///
    /// On each tick:
    /// 1. Fetches the USDC-SPL balance via RPC
    /// 2. Writes the atomic balance (in atomic USDC units) to the shared state
    /// 3. If the balance is below the threshold, fires the low-balance callback
    pub async fn run(self) {
        let http = reqwest::Client::new();
        let mut interval = tokio::time::interval(self.poll_interval);

        loop {
            interval.tick().await;

            match self.fetch_balance(&http).await {
                Ok(ui_amount) => {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let atomic_amount = (ui_amount * 1_000_000.0) as u64;
                    self.balance_state.store(atomic_amount, Ordering::Relaxed);
                    debug!(balance_usdc = ui_amount, "balance poll complete");

                    if ui_amount < self.low_balance_threshold {
                        warn!(
                            balance_usdc = ui_amount,
                            threshold = self.low_balance_threshold,
                            "low balance detected"
                        );
                        if let Some(ref cb) = self.on_low_balance {
                            cb(ui_amount);
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "balance poll failed");
                    // Do not update balance_state on error — keep the last known value
                }
            }
        }
    }

    /// Fetch the USDC-SPL balance from the Solana RPC.
    async fn fetch_balance(&self, http: &reqwest::Client) -> Result<f64, String> {
        let owner: Pubkey = self
            .wallet_address
            .parse()
            .map_err(|e| format!("invalid address: {e}"))?;

        let mint: Pubkey = USDC_MINT
            .parse()
            .map_err(|e| format!("invalid USDC mint: {e}"))?;

        let ata = signer::associated_token_address(&owner, &mint);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountBalance",
            "params": [ata.to_string()]
        });

        let resp = http
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("RPC returned HTTP {}", resp.status()));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        // Account not found -> balance is 0
        if json.get("error").is_some() {
            return Ok(0.0);
        }

        Ok(json["result"]["value"]["uiAmount"].as_f64().unwrap_or(0.0))
    }
}

impl std::fmt::Debug for BalanceMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BalanceMonitor")
            .field("rpc_url", &self.rpc_url)
            .field("wallet_address", &self.wallet_address)
            .field("poll_interval", &self.poll_interval)
            .field("low_balance_threshold", &self.low_balance_threshold)
            .field("has_callback", &self.on_low_balance.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_atomic_balance_read_write() {
        let state = Arc::new(AtomicU64::new(u64::MAX));
        assert_eq!(state.load(Ordering::Relaxed), u64::MAX);

        state.store(1_500_000, Ordering::Relaxed);
        assert_eq!(state.load(Ordering::Relaxed), 1_500_000);
    }

    #[test]
    fn test_atomic_balance_shared_between_threads() {
        let state = Arc::new(AtomicU64::new(u64::MAX));
        let state2 = Arc::clone(&state);

        let handle = std::thread::spawn(move || {
            state2.store(2_000_000, Ordering::Relaxed);
        });
        handle.join().unwrap();

        assert_eq!(state.load(Ordering::Relaxed), 2_000_000);
    }

    #[test]
    fn test_builder_defaults() {
        let state = Arc::new(AtomicU64::new(u64::MAX));
        let monitor = BalanceMonitor::new(state, "http://localhost:8899", "SomeAddress");
        assert_eq!(monitor.poll_interval, DEFAULT_POLL_INTERVAL);
        assert!(
            (monitor.low_balance_threshold - DEFAULT_LOW_BALANCE_THRESHOLD).abs() < f64::EPSILON
        );
        assert!(monitor.on_low_balance.is_none());
    }

    #[test]
    fn test_builder_custom_values() {
        let state = Arc::new(AtomicU64::new(u64::MAX));
        let monitor = BalanceMonitor::new(state, "http://localhost:8899", "SomeAddress")
            .poll_interval(Duration::from_secs(10))
            .low_balance_threshold(1.0)
            .on_low_balance(|_bal| {});
        assert_eq!(monitor.poll_interval, Duration::from_secs(10));
        assert!((monitor.low_balance_threshold - 1.0).abs() < f64::EPSILON);
        assert!(monitor.on_low_balance.is_some());
    }

    #[tokio::test]
    async fn test_poll_updates_balance_state() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "context": { "slot": 100 },
                    "value": {
                        "amount": "5000000",
                        "decimals": 6,
                        "uiAmount": 5.0,
                        "uiAmountString": "5.0"
                    }
                }
            })))
            .mount(&mock_server)
            .await;

        let state = Arc::new(AtomicU64::new(u64::MAX));
        let state_clone = Arc::clone(&state);

        // Use a valid Solana public key for the test
        let wallet_address = solana_sdk::pubkey::Pubkey::new_unique().to_string();

        let monitor = BalanceMonitor::new(state_clone, &mock_server.uri(), &wallet_address)
            .poll_interval(Duration::from_millis(50));

        // Spawn the monitor and let it run for one tick
        let handle = tokio::spawn(monitor.run());

        // Wait for at least one poll cycle
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Balance should have been updated from u64::MAX to 5_000_000
        let balance = state.load(Ordering::Relaxed);
        assert_ne!(balance, u64::MAX, "balance should have been updated");
        assert_eq!(balance, 5_000_000);

        handle.abort();
    }

    #[tokio::test]
    async fn test_low_balance_callback_fires() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "context": { "slot": 100 },
                    "value": {
                        "amount": "50000",
                        "decimals": 6,
                        "uiAmount": 0.05,
                        "uiAmountString": "0.05"
                    }
                }
            })))
            .mount(&mock_server)
            .await;

        let state = Arc::new(AtomicU64::new(u64::MAX));
        let callback_fired = Arc::new(AtomicU64::new(0));
        let callback_fired_clone = Arc::clone(&callback_fired);

        let wallet_address = solana_sdk::pubkey::Pubkey::new_unique().to_string();

        let monitor = BalanceMonitor::new(Arc::clone(&state), &mock_server.uri(), &wallet_address)
            .poll_interval(Duration::from_millis(50))
            .low_balance_threshold(0.10)
            .on_low_balance(move |_bal| {
                callback_fired_clone.store(1, Ordering::Relaxed);
            });

        let handle = tokio::spawn(monitor.run());
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert_eq!(
            callback_fired.load(Ordering::Relaxed),
            1,
            "low balance callback should have fired"
        );

        handle.abort();
    }
}
