//! Browser fixture with test clocks and no device or native window access.
use goofi_tests::{require_python, Goofi};

#[tokio::main]
async fn main() {
    let python = require_python();
    let home = tempfile::tempdir().unwrap();
    goofi_tests::fixtures::plugin_package(home.path());
    let mut goofi = Goofi::new();
    goofi_bridge::plugins::Plugins::load(
        &mut goofi.state,
        home.path(),
        std::path::Path::new(&python.py),
    )
    .unwrap();
    let port = std::env::var("GOOFI_PLUGIN_TEST_PORT").unwrap_or_else(|_| "8599".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    goofi.state.set_bound(listener.local_addr().unwrap());
    goofi_bridge::serve_app(listener, goofi.state.clone(), goofi_bridge::SPA, false)
        .await
        .unwrap();
}
