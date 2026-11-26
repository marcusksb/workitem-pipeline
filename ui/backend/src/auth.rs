use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::Shared;

const COOKIE: &str = "wi_session";

pub fn token_matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn set_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=31536000")
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

fn header_str<'a>(headers: &'a HeaderMap, name: header::HeaderName) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// `Authorization: Bearer <token>`, for the scripts the backend starts (`workitem-run-item`).
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, header::AUTHORIZATION)?.strip_prefix("Bearer ")
}

/// The `Host` header names loopback or an allowed host, with the server's own port (DNS rebinding).
fn host_allowed(host: &str, port: u16, allowed: &[String]) -> bool {
    let Some(name) = host.strip_suffix(&format!(":{port}")) else { return false };
    name == "127.0.0.1" || name == "localhost" || allowed.iter().any(|a| a == name)
}

/// Host check, cookie check for `/api` and `/ws`, Origin check for `/ws`.
pub async fn guard(State(state): State<Shared>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = header_str(headers, header::HOST).unwrap_or_default();
    if !host_allowed(host, state.port, &state.allowed_hosts) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = request.uri().path();
    let is_ws = path.starts_with("/ws/");
    let protected = (path.starts_with("/api/") && path != "/api/login") || is_ws;
    if protected {
        let given = cookie_token(headers).or(if is_ws { None } else { bearer_token(headers) });
        let authed = given.is_some_and(|t| token_matches(&state.store.token(), t));
        if !authed {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if is_ws && header_str(headers, header::ORIGIN) != Some(&format!("http://{host}")) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_tokens() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abc", "abd"));
        assert!(!token_matches("abc", "ab"));
    }

    #[test]
    fn allows_loopback_and_listed_hosts_on_own_port() {
        let none: &[String] = &[];
        let lan = &["my-mac.local".to_string()];
        assert!(host_allowed("127.0.0.1:7373", 7373, none));
        assert!(host_allowed("localhost:7373", 7373, none));
        assert!(!host_allowed("my-mac.local:7373", 7373, none));
        assert!(host_allowed("my-mac.local:7373", 7373, lan));
        assert!(!host_allowed("my-mac.local:8080", 7373, lan));
        assert!(!host_allowed("my-mac.local", 7373, lan));
        assert!(!host_allowed("evil.my-mac.local:7373", 7373, lan));
    }

    #[test]
    fn reads_a_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer tok".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("tok"));
        headers.insert(header::AUTHORIZATION, "Basic tok".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn finds_cookie_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; wi_session=tok; b=2".parse().unwrap());
        assert_eq!(cookie_token(&headers), Some("tok"));
        headers.insert(header::COOKIE, "wi_sessionx=tok".parse().unwrap());
        assert_eq!(cookie_token(&headers), None);
    }
}
