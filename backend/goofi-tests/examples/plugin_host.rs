//! Browser fixture with test clocks and no device or native window access.
use goofi_tests::{require_python, Goofi};

#[tokio::main]
async fn main() {
    let _python = require_python();
    let home = tempfile::tempdir().unwrap();
    goofi_tests::fixtures::plugin_package(home.path());
    goofi_tests::fixtures::virtual_cables(home.path());
    goofi_tests::fixtures::latency(home.path());
    let goofi = Goofi::with_plugins(home.path());
    let port = std::env::var("GOOFI_PLUGIN_TEST_PORT").unwrap_or_else(|_| "8599".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    goofi.state.set_bound(listener.local_addr().unwrap());
    goofi_bridge::serve_app(listener, goofi.state.clone(), goofi_bridge::SPA, false)
        .await
        .unwrap();
}
