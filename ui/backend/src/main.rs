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
    // Only local by default; `WORKITEM_UI_BIND=0.0.0.0` plus `WORKITEM_UI_HOST=<address>` lets a phone on the network in.
    let bind = std::env::var("WORKITEM_UI_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let listener = TcpListener::bind((bind.as_str(), port)).await.unwrap_or_else(|e| {
        eprintln!("workload: cannot bind {bind}:{port}: {e}");
        std::process::exit(1);
    });
    let port = listener.local_addr().unwrap().port();
    println!("workload: http://127.0.0.1:{port}/#token={}", store.token());

    let state = Arc::new(AppState::new(store, port));
    if let Some(host) = &state.extra_host {
        println!("workload: http://{host}:{port}/#token={}", state.store.token());
    }
    if std::env::var("WORKITEM_UI_DISPATCH").as_deref() != Ok("off") {
        tokio::spawn(dispatch::run(state.clone()));
    }
    axum::serve(listener, router(state)).await.expect("server error");
}
