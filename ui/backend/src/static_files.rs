use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Assets;

/// Serves the embedded frontend; unknown paths fall back to `index.html` for the SPA.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("ws/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    let Some((name, file)) = Assets::get(path).map(|f| (path, f)).or_else(|| Assets::get("index.html").map(|f| ("index.html", f)))
    else {
        return (StatusCode::NOT_FOUND, "frontend not built: run npm run build in ui/frontend").into_response();
    };
    let mime = mime_guess::from_path(name).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.to_string())], file.data).into_response()
}
