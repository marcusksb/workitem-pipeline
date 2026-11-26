use std::sync::Arc;

use tokio::net::TcpListener;
use workitem_ui::{config::Store, dispatch, router, AppState};

#[tokio::main]
async fn main() {
    let store = Store::open(Store::default_root(), Store::legacy_config().as_deref()).unwrap_or_else(|e| {
        eprintln!("workload: cannot open config: {e}");
        std::process::exit(1);
    });

    if std::env::args().any(|a| a == "--rotate-token") {
        let token = store.rotate_token().expect("cannot write config");
        println!("new token: {token}");
        return;
    }

    let port = std::env::var("WORKITEM_UI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(store.port());
    let host = std::env::var("WORKITEM_UI_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let listener = TcpListener::bind((host.as_str(), port)).await.unwrap_or_else(|e| {
        eprintln!("workload: cannot bind {host}:{port}: {e}");
        std::process::exit(1);
    });
    let port = listener.local_addr().unwrap().port();
    let state = AppState::new(store, port);
    let token = state.store.token();
    // The Host check accepts only loopback and the allowed hosts, so those are the URLs that work.
    let local = if listener.local_addr().unwrap().ip().is_unspecified() { "127.0.0.1" } else { host.as_str() };
    for name in std::iter::once(local).chain(state.allowed_hosts.iter().map(String::as_str)) {
        // A URL entry (`https://foo.bar.com`) is a reverse proxy's public address.
        if name.contains("://") {
            println!("workload: {}/#token={token}", name.trim_end_matches('/'));
        } else {
            println!("workload: http://{name}:{port}/#token={token}");
        }
    }

    let state = Arc::new(state);
    if std::env::var("WORKITEM_UI_DISPATCH").as_deref() != Ok("off") {
        tokio::spawn(dispatch::run(state.clone()));
    }
    axum::serve(listener, router(state)).await.expect("server error");
}
