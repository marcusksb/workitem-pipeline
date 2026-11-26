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

/// The name in a `Host` header or an allowed-hosts entry: `https://foo.bar.com/`, `foo.bar.com:8443`
/// and `foo.bar.com` all name `foo.bar.com`. A bare IPv6 `[::1]` keeps its last colon part.
fn host_name(s: &str) -> &str {
    let s = s.split_once("://").map_or(s, |(_, rest)| rest).trim_end_matches('/');
    match s.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => s,
    }
}

/// DNS rebinding guard on the `Host` header (decisions 79 and 81): loopback with the server's own port,
/// or an allowed host with any port or none, so a reverse proxy that passes `Host: foo.bar.com` gets in.
fn host_allowed(host: &str, port: u16, allowed: &[String]) -> bool {
    let loopback = host.strip_suffix(&format!(":{port}")).is_some_and(|n| n == "127.0.0.1" || n == "localhost");
    loopback || allowed.iter().any(|a| host_name(a).eq_ignore_ascii_case(host_name(host)))
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
        // Same origin; https when a TLS proxy sits in front.
        let origin = header_str(headers, header::ORIGIN);
        if is_ws && origin.and_then(|o| o.strip_prefix("http://").or_else(|| o.strip_prefix("https://"))) != Some(host) {
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
    fn allows_loopback_on_own_port_and_listed_hosts_on_any_port() {
        let none: &[String] = &[];
        let lan = &["my-mac.local".to_string()];
        assert!(host_allowed("127.0.0.1:7373", 7373, none));
        assert!(host_allowed("localhost:7373", 7373, none));
        assert!(!host_allowed("localhost:8080", 7373, none));
        assert!(!host_allowed("localhost", 7373, none));
        assert!(!host_allowed("my-mac.local:7373", 7373, none));
        assert!(host_allowed("my-mac.local:7373", 7373, lan));
        assert!(host_allowed("my-mac.local:8080", 7373, lan));
        assert!(host_allowed("my-mac.local", 7373, lan));
        assert!(host_allowed("My-Mac.Local", 7373, lan));
        assert!(!host_allowed("evil.my-mac.local:7373", 7373, lan));
        assert!(!host_allowed("mac.local:7373", 7373, lan));
        assert!(!host_allowed("my-mac.local.evil:7373", 7373, lan));

        let proxy = &["https://foo.bar.com/".to_string()];
        assert!(host_allowed("foo.bar.com", 7373, proxy));
        assert!(host_allowed("foo.bar.com:443", 7373, proxy));
        assert!(!host_allowed("https", 7373, proxy));

        let v6 = &["[fd00::1]".to_string()];
        assert!(host_allowed("[fd00::1]:7373", 7373, v6));
        assert!(host_allowed("[fd00::1]", 7373, v6));
        assert!(!host_allowed("[fd00::2]:7373", 7373, v6));
    }

    #[test]
    fn reads_the_name_of_a_host_or_entry() {
        assert_eq!(host_name("foo.bar.com"), "foo.bar.com");
        assert_eq!(host_name("foo.bar.com:8443"), "foo.bar.com");
        assert_eq!(host_name("https://foo.bar.com/"), "foo.bar.com");
        assert_eq!(host_name("http://foo.bar.com:8080"), "foo.bar.com");
        assert_eq!(host_name("[::1]:7373"), "[::1]");
        assert_eq!(host_name("[::1]"), "[::1]");
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
