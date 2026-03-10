use rustyclawclient_cli_args::{GatewayArgs, RpcArgs, WalletArgs};

#[allow(clippy::unused_async)]
pub async fn run(
    _prompt: &str,
    _model: &str,
    _no_stream: bool,
    _max_payment: Option<u64>,
    _wallet_args: &WalletArgs,
    _gateway_args: &GatewayArgs,
    _rpc_args: &RpcArgs,
) -> Result<(), String> {
    Err("chat command not yet implemented".to_string())
}
