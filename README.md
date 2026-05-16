# solvela-client

> **This SDK has moved.** Source now lives in the Solvela monorepo:
> https://github.com/solvela-ai/solvela/tree/main/sdks/rust

**Install (unchanged):**

```bash
cargo install solvela-client-cli         # agent-side payer CLI
cargo install solvela-client-proxy       # localhost x402-signing reverse proxy
```

Library users:

```toml
[dependencies]
solvela-client = "0.2"
```

The crates.io package names — `solvela-client`, `solvela-client-cli`, `solvela-client-cli-args`, `solvela-client-proxy` — are preserved. Existing installs and `Cargo.toml` pins continue to work without changes.

Issues, PRs, and discussion: https://github.com/solvela-ai/solvela/issues

The monorepo's `sdks/rust/` is byte-equivalent to this repo's `main` at archive time, with `Cargo.toml` `repository` rewritten to point at the monorepo and `solvela-protocol` re-linked as a local path dep so wire-format drift between gateway and SDK fails CI in the same PR. See [PR #316](https://github.com/solvela-ai/solvela/pull/316) for the consolidation diff.
