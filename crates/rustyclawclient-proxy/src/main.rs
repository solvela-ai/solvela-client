use rustyclawclient_proxy::proxy;

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use tracing::{error, info};

use rustyclaw_client::{ClientConfig, RustyClawClient, Wallet};

/// Localhost reverse proxy with transparent x402 USDC-SPL payment handling.
///
/// Any OpenAI-compatible client can point at this proxy and get automatic
/// Solana payment handling — no code changes needed.
#[derive(Parser, Debug)]
#[command(name = "rustyclawclient-proxy", version)]
struct Cli {
    /// Gateway URL to forward requests to.
    #[arg(short, long, default_value = "https://rustyclawrouter-gateway.fly.dev")]
    gateway: String,

    /// Port to listen on (binds 127.0.0.1 only).
    #[arg(short, long, default_value_t = 8402)]
    port: u16,

    /// Solana RPC URL.
    #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
    rpc_url: String,

    /// Environment variable containing base58-encoded keypair.
    #[arg(long, default_value = "RUSTYCLAW_WALLET_KEY")]
    wallet_env: String,

    /// Path to wallet keypair file (Solana CLI JSON byte-array format).
    #[arg(long, default_value = "~/.rustyclaw/wallet.json")]
    wallet_file: String,

    /// Maximum payment per request in USDC (e.g., 0.10).
    #[arg(long)]
    max_payment: Option<f64>,

    /// Expected gateway recipient wallet address (security: reject mismatches).
    #[arg(long)]
    expected_recipient: Option<String>,

    /// Enable debug logging.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        "rustyclawclient_proxy=debug,rustyclaw_client=debug,tower_http=debug"
    } else {
        "rustyclawclient_proxy=info,tower_http=info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .init();

    // Load wallet: env var takes priority, then file
    let wallet = match load_wallet(&cli.wallet_env, &cli.wallet_file) {
        Ok(w) => w,
        Err(e) => {
            error!("failed to load wallet: {e}");
            eprintln!("Error: {e}");
            eprintln!();
            eprintln!("Provide a wallet via:");
            eprintln!(
                "  1. Set {} env var with base58-encoded keypair",
                cli.wallet_env
            );
            eprintln!("  2. Create wallet file at {}", cli.wallet_file);
            std::process::exit(1);
        }
    };

    info!(wallet = %wallet.address(), "wallet loaded");

    // Build client config
    let mut config = ClientConfig {
        gateway_url: cli.gateway.trim_end_matches('/').to_string(),
        rpc_url: cli.rpc_url,
        ..ClientConfig::default()
    };

    if let Some(max_usdc) = cli.max_payment {
        // Convert USDC to atomic units (1 USDC = 1_000_000 atomic)
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let atomic = (max_usdc * 1_000_000.0) as u64;
        config.max_payment_amount = Some(atomic);
        info!(max_usdc = %max_usdc, max_atomic = atomic, "payment cap enabled");
    }

    if let Some(ref recipient) = cli.expected_recipient {
        config.expected_recipient = Some(recipient.clone());
        info!(recipient = %recipient, "recipient validation enabled");
    }

    let client = match RustyClawClient::new(wallet, config) {
        Ok(c) => c,
        Err(e) => {
            error!("failed to create client: {e}");
            std::process::exit(1);
        }
    };

    let state = Arc::new(proxy::ProxyState {
        client,
        gateway_url: cli.gateway.trim_end_matches('/').to_string(),
        http: reqwest::Client::new(),
    });

    let app = proxy::build_proxy_router(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], cli.port));
    info!(
        addr = %addr,
        "rustyclawclient-proxy started — forwarding to gateway"
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            error!(addr = %addr, error = %e, "failed to bind");
            std::process::exit(1);
        });

    axum::serve(listener, app).await.unwrap_or_else(|e| {
        error!(error = %e, "server error");
        std::process::exit(1);
    });
}

/// Load wallet from environment variable (priority) or file (fallback).
fn load_wallet(env_var: &str, file_path: &str) -> Result<Wallet, String> {
    // Try env var first
    if let Ok(val) = std::env::var(env_var) {
        if !val.is_empty() {
            return Wallet::from_keypair_b58(&val)
                .map_err(|e| format!("invalid keypair in {env_var}: {e}"));
        }
    }

    // Expand ~ to home directory
    let expanded = if let Some(rest) = file_path.strip_prefix("~/") {
        match dirs_next::home_dir() {
            Some(home) => home.join(rest),
            None => std::path::PathBuf::from(file_path),
        }
    } else {
        std::path::PathBuf::from(file_path)
    };

    // Try wallet file
    if expanded.exists() {
        let contents = std::fs::read_to_string(&expanded)
            .map_err(|e| format!("failed to read {}: {e}", expanded.display()))?;

        // Parse Solana CLI format: JSON array of u8 values [174, 47, ...]
        let bytes: Vec<u8> = serde_json::from_str(&contents)
            .map_err(|e| format!("invalid wallet file format in {}: {e}", expanded.display()))?;

        return Wallet::from_keypair_bytes(&bytes)
            .map_err(|e| format!("invalid keypair in {}: {e}", expanded.display()));
    }

    Err(format!(
        "no wallet found: set {env_var} env var or create {}",
        expanded.display()
    ))
}
