//! Runs the real router against a real tmux server on an isolated socket.
use std::{future::IntoFuture, sync::Arc, time::Duration};

use axum::{body::Body, http::{header, Request, StatusCode}};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::{client::IntoClientRequest, Message}};
use tower::ServiceExt;
use workitem_ui::{config::Store, router, AppState, Shared};

mod common;
use common::Tmux;

struct Fixture {
    _dir: tempfile::TempDir,
    tmux: Tmux,
    state: Shared,
    token: String,
}

async fn fixture() -> (Fixture, TcpListener) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let item = repo.join(".claude/workitems/T-1");
    std::fs::create_dir_all(&item).unwrap();
    std::fs::write(item.join("workitem.md"), "---\ntitle: Test\nstage: analyze\nagent_status: blocked: need input\n---\nbody\n").unwrap();

    let store = Store::open(dir.path().join("config.toml")).unwrap();
    store.add_project("app", repo.to_str().unwrap()).unwrap();
    let token = store.token();

    let tmux = Tmux::new();
    tmux.run(&["new-session", "-d", "-s", "wi-T-1", "-c", repo.to_str().unwrap(), "cat"]);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut state = AppState::new(store, port);
    state.tmux_socket = Some(tmux.0.clone());
    let state = Arc::new(state);
    (Fixture { _dir: dir, tmux, state, token }, listener)
}

async fn get(f: &Fixture, uri: &str, host: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut request = Request::get(uri).header(header::HOST, host);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = router(f.state.clone()).oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn api_requires_host_and_cookie_and_reports_sessions() {
    let (f, _listener) = fixture().await;
    let host = format!("127.0.0.1:{}", f.state.port);
    let cookie = format!("wi_session={}", f.token);

    assert_eq!(get(&f, "/api/workitems", &host, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(get(&f, "/api/workitems", "evil.example:80", Some(&cookie)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(get(&f, "/api/workitems", &host, Some("wi_session=wrong")).await.0, StatusCode::UNAUTHORIZED);

    let (status, body) = get(&f, "/api/workitems", &host, Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let items: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(items[0]["dir"], "T-1");
    assert_eq!(items[0]["agent_status"], "blocked: need input");
    assert_eq!(items[0]["session"], true);

    let (status, body) = get(&f, "/api/workitems/app/T-1", &host, Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"content\""));
    assert_eq!(get(&f, "/api/workitems/app/..", &host, Some(&cookie)).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&f, "/api/nope", &host, Some(&cookie)).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn login_sets_httponly_cookie_only_for_right_token() {
    let (f, _listener) = fixture().await;
    let host = format!("127.0.0.1:{}", f.state.port);
    let login = |token: String| {
        let request = Request::post("/api/login")
            .header(header::HOST, &host)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!("{{\"token\":\"{token}\"}}")))
            .unwrap();
        router(f.state.clone()).oneshot(request)
    };
    let bad = login("nope".into()).await.unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);
    let good = login(f.token.clone()).await.unwrap();
    assert_eq!(good.status(), StatusCode::NO_CONTENT);
    let cookie = good.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
}

fn ws_request(port: u16, path: &str, cookie: Option<&str>, origin: Option<&str>) -> Request<()> {
    let mut request = format!("ws://127.0.0.1:{port}{path}").into_client_request().unwrap();
    if let Some(cookie) = cookie {
        request.headers_mut().insert(header::COOKIE, cookie.parse().unwrap());
    }
    if let Some(origin) = origin {
        request.headers_mut().insert(header::ORIGIN, origin.parse().unwrap());
    }
    request
}

#[tokio::test]
async fn terminal_bridges_input_and_detaches_without_killing_session() {
    let (f, listener) = fixture().await;
    let port = f.state.port;
    tokio::spawn(axum::serve(listener, router(f.state.clone())).into_future());
    let cookie = format!("wi_session={}", f.token);
    let origin = format!("http://127.0.0.1:{port}");
    let path = "/ws/terminal/app/T-1?cols=100&rows=30";

    // No cookie, or a foreign Origin, is refused before the upgrade.
    assert!(connect_async(ws_request(port, path, None, Some(&origin))).await.is_err());
    assert!(connect_async(ws_request(port, path, Some(&cookie), Some("http://evil.example"))).await.is_err());
    assert!(connect_async(ws_request(port, path, Some(&cookie), None)).await.is_err());
    // Unknown workitem has no terminal.
    assert!(connect_async(ws_request(port, "/ws/terminal/app/T-9", Some(&cookie), Some(&origin))).await.is_err());

    let (mut socket, _) = connect_async(ws_request(port, path, Some(&cookie), Some(&origin))).await.unwrap();
    socket.send(Message::Text("{\"cols\":90,\"rows\":20}".into())).await.unwrap();
    socket.send(Message::Binary(b"hello-from-browser\n".to_vec().into())).await.unwrap();

    let mut seen = String::new();
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(bytes) = message {
                seen.push_str(&String::from_utf8_lossy(&bytes));
                if seen.contains("hello-from-browser") {
                    return true;
                }
            }
        }
        false
    })
    .await;
    assert_eq!(found, Ok(true), "terminal output was: {seen:?}");

    socket.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(f.tmux.has("wi-T-1"), "closing the browser must not kill the agent session");
}
