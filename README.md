# solvela-client

Rust client SDK for [Solvela](https://solvela.ai) — Solana-native AI agent payment gateway.

This workspace holds the client-side primitives that pair with the [`solvela`](https://github.com/solvela-ai/solvela) gateway:

| Crate | Purpose |
|---|---|
| `solvela-client` | Library: wallet management, x402 payment signing, payload assembly |
| `solvela-client-cli` | CLI binary that uses the library |
| `solvela-client-cli-args` | Shared argument parsing for the CLI |
| `solvela-client-proxy` | Local HTTP proxy that signs payments on behalf of unmodified OpenAI-format clients |

## When to use which crate

- **You're building an agent in Rust** → depend on `solvela-client` directly. Sign and send payments yourself.
- **You want a command-line tool** → install `solvela-client-cli` (planned: `cargo install solvela-client-cli`).
- **You have a tool that speaks OpenAI HTTP and you want it to pay automatically** → run `solvela-client-proxy` as a sidecar; point your tool at `http://localhost:<port>` and the proxy signs and forwards.

> **Different from `solvela-cli`**: the [`solvela-cli`](https://github.com/solvela-ai/solvela) (in the main monorepo) is the operator/dev CLI for talking to a Solvela gateway. `solvela-client-cli` here is the agent-side payer CLI. They serve different purposes and may merge in the future.

## Status

Pre-1.0. APIs may shift. The protocol it implements is [x402](https://www.x402.org/) on Solana with USDC-SPL settlement; that part is stable.

## License

[MIT](./LICENSE).
