use rustyclawclient_cli_args::GatewayArgs;

#[allow(clippy::unused_async)]
pub async fn run(
    _provider: Option<&str>,
    _json: bool,
    _gateway_args: &GatewayArgs,
) -> Result<(), String> {
    Err("models command not yet implemented".to_string())
}
