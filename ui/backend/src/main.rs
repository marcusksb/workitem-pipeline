use std::sync::Arc;

use tokio::net::TcpListener;
use workitem_ui::{config::Store, dispatch, router, AppState};

#[tokio::main]
async fn main() {
    let store = Store::open(Store::default_path()).unwrap_or_else(|e| {
        eprintln!("workload: cannot open config: {e}");
        std::process::exit(1);
    });

    if std::env::args().any(|a| a == "--rotate-token") {
        let token = store.rotate_token().expect("cannot write config");
        println!("new token: {token}");
        return;
    }

    let port = std::env::var("WORKITEM_UI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(store.port());
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap_or_else(|e| {
        eprintln!("workload: cannot bind 127.0.0.1:{port}: {e}");
        std::process::exit(1);
    });
    let port = listener.local_addr().unwrap().port();
    println!("workload: http://127.0.0.1:{port}/#token={}", store.token());

    let state = Arc::new(AppState::new(store, port));
    if std::env::var("WORKITEM_UI_DISPATCH").as_deref() != Ok("off") {
        tokio::spawn(dispatch::run(state.clone()));
    }
    axum::serve(listener, router(state)).await.expect("server error");
}
