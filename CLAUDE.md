# CLAUDE.md

## What This Is

Solvela Client is a Rust workspace for making paid LLM API calls through a SolvelaRouter gateway. AI agents use this to hold a Solana wallet, sign USDC-SPL payments, and transparently handle the x402 402-payment handshake.

Phases A–E complete. Core library, CLI, shared args, and HTTP proxy all functional.

## Build & Test Commands

```bash
# Build (requires OpenSSL on Linux)
export OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu
export OPENSSL_INCLUDE_DIR=/usr/include/openssl
cargo check                       # fast type check
cargo build                       # debug build
cargo build --release             # release build

# Test (121 tests total)
cargo test                        # all workspace tests
cargo test -p solvela-client    # 93 unit + 9 integration
cargo test -p solvela-client-cli # 5 tests
cargo test -p solvela-client-cli-args  # 6 tests
cargo test -p solvela-client-proxy     # 7 integration + 1 unit

# Lint
cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check        # CI mode
```

## Architecture

### Workspace Crates (`crates/`)

- **solvela-client** — Core library. Wallet management (BIP39 create/import, zeroize on drop), x402 payment signing, transparent 402 handshake, and 5 smart features (response cache, session sticking, balance monitoring, degraded detection, free tier fallback).
- **solvela-client-cli** (`rcc`) — CLI binary. Wallet ops (create, import, balance, address, export), streaming chat completions, model browsing with filtering, diagnostics (doctor command).
- **solvela-client-cli-args** — Shared clap Args structs (WalletArgs, GatewayArgs, RpcArgs) used by both CLI and proxy.
- **solvela-client-proxy** — HTTP reverse proxy. Localhost x402 interceptor with transparent payment handling for existing LLM tools.

### Core Library Modules (`solvela-client/src/`)

- `error.rs` — ClientError, WalletError, SignerError (thiserror)
- `config.rs` — ClientConfig + ClientBuilder with opt-in smart feature methods
- `wallet.rs` — Wallet wrapping solana_sdk::Keypair, BIP39 create/import, zeroize
- `signer.rs` — Build + sign USDC-SPL TransferChecked transactions
- `client.rs` — Solvela Client with `chat()`, `chat_stream()`, transparent 402 handshake
- `balance.rs` — BalanceMonitor for USDC balance tracking and budget guards
- `cache.rs` — ResponseCache (LRU) for deduplicating identical requests
- `session.rs` — SessionStore with 3-strike model escalation
- `quality.rs` — DegradedReason detection for provider quality issues

### Dependencies

Depends on `rustyclaw-protocol` (path dep to `../SolvelaRouter/crates/protocol`).

## Code Conventions

- Edition 2021 (required for solana-sdk 2.2 compatibility — exception to global 2024 default)
- thiserror for all error enums
- Never unwrap() in library code — propagate with ?
- Custom Debug redacts all secrets (wallet keys)
- Drop zeros key material (zeroize)
- No Serialize on Wallet — keys never serialized
- Async-first: tokio runtime, futures for streaming
- Smart features are opt-in via ClientBuilder (`enable_cache()`, `enable_sessions()`, etc.)
