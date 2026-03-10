use std::io::IsTerminal;

use rustyclaw_client::{ClientBuilder, RustyClawClient, Wallet};
use rustyclawclient_cli_args::GatewayArgs;

pub async fn run(
    provider: Option<&str>,
    json: bool,
    gateway_args: &GatewayArgs,
) -> Result<(), String> {
    // Models endpoint is public — use a throwaway wallet
    let (wallet, _mnemonic) = Wallet::create();
    let config = ClientBuilder::new()
        .gateway_url(&gateway_args.gateway)
        .build_config();

    let client = RustyClawClient::new(wallet, config)
        .map_err(|e| format!("failed to create client: {e}"))?;

    let mut models = client
        .models()
        .await
        .map_err(|e| format!("failed to fetch models: {e}"))?;

    // Apply provider filter (case-insensitive)
    if let Some(filter) = provider {
        let filter_lower = filter.to_lowercase();
        models.retain(|m| m.provider.to_lowercase() == filter_lower);
    }

    // Sort by ID for consistent output
    models.sort_by(|a, b| a.id.cmp(&b.id));

    if json || !std::io::stdout().is_terminal() {
        let output = serde_json::to_string_pretty(&models)
            .map_err(|e| format!("failed to serialize models: {e}"))?;
        println!("{output}");
    } else {
        print_table(&models);
    }

    Ok(())
}

fn print_table(models: &[rustyclaw_protocol::ModelInfo]) {
    const ID_W: usize = 35;
    const PROV_W: usize = 12;
    const COST_W: usize = 10;
    const CTX_W: usize = 8;

    println!(
        "{:<ID_W$} {:<PROV_W$} {:>COST_W$} {:>COST_W$} {:>CTX_W$}",
        "ID", "Provider", "In $/M", "Out $/M", "Context",
    );
    println!("{}", "-".repeat(ID_W + PROV_W + COST_W * 2 + CTX_W + 4));

    for m in models {
        println!(
            "{:<ID_W$} {:<PROV_W$} {:>COST_W$.4} {:>COST_W$.4} {:>CTX_W$}",
            truncate(&m.id, ID_W),
            truncate(&m.provider, PROV_W),
            m.input_cost_per_million,
            m.output_cost_per_million,
            format_context(m.context_window),
        );
    }

    println!();
    println!("{} model(s)", models.len());
}

fn format_context(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        let m = tokens / 1_000_000;
        format!("{m}M")
    } else if tokens >= 1_000 {
        let k = tokens / 1_000;
        format!("{k}K")
    } else {
        tokens.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end = max.saturating_sub(3);
        let prefix: String = s.chars().take(end).collect();
        format!("{prefix}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_context_k() {
        assert_eq!(format_context(128_000), "128K");
        assert_eq!(format_context(4_000), "4K");
    }

    #[test]
    fn test_format_context_m() {
        assert_eq!(format_context(1_000_000), "1M");
        assert_eq!(format_context(2_000_000), "2M");
    }

    #[test]
    fn test_format_context_small() {
        assert_eq!(format_context(512), "512");
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("abcdefghij", 7), "abcd...");
    }
}
