use std::sync::Arc;

use axum::Router;

use rustyclaw_client::RustyClawClient;

/// Shared state for the proxy handlers.
pub struct ProxyState {
    pub client: RustyClawClient,
    pub gateway_url: String,
}

/// Build the Axum router for the proxy.
pub fn build_proxy_router(_state: Arc<ProxyState>) -> Router {
    Router::new()
}
