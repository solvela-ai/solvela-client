# CLAUDE.md

## What This Is

RustyClawClient is a Rust library for making paid LLM API calls through a RustyClawRouter gateway. AI agents use this to hold a Solana wallet, sign USDC-SPL payments, and transparently handle the x402 402-payment handshake.

## Build & Test Commands

```bash
cargo check
cargo test
cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings
```

## Architecture

Single library crate: `crates/rustyclaw-client/`

- `error.rs` — ClientError, WalletError, SignerError (thiserror)
- `config.rs` — ClientConfig + ClientBuilder
- `wallet.rs` — Wallet wrapping solana_sdk::Keypair, BIP39 create/import, zeroize
- `signer.rs` — Build + sign USDC-SPL TransferChecked transactions
- `client.rs` — RustyClawClient with transparent 402 handshake

Depends on `rustyclaw-protocol` (path dep to ../RustyClawRouter/crates/protocol).

## Code Conventions

- Edition 2021 (solana-sdk compatibility)
- thiserror for all error enums
- Never unwrap() in library code — propagate with ?
- Custom Debug redacts all secrets (wallet keys)
- Drop zeros key material
- No Serialize on Wallet — keys never serialized
