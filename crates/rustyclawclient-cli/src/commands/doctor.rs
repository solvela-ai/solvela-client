use rustyclawclient_cli_args::{GatewayArgs, RpcArgs, WalletArgs};

#[allow(clippy::unused_async)]
pub async fn run(
    _wallet_args: &WalletArgs,
    _gateway_args: &GatewayArgs,
    _rpc_args: &RpcArgs,
) -> Result<(), String> {
    Err("doctor command not yet implemented".to_string())
}
