//! The files of a workitem folder, for the detail panel's file browser. Read-only, and nothing
//! outside the folder: symlinks are neither listed nor followed.
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use axum::{
    extract::{Path as UrlPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{api::ApiError, workitem, Shared};

/// More than any workitem folder should hold; the rest is left out.
const MAX_ENTRIES: usize = 1000;

#[derive(Serialize, Debug, PartialEq)]
pub struct Entry {
    /// Relative to the workitem folder, `/`-separated.
    pub path: String,
    pub size: u64,
}

/// Regular files under `root`, sorted by path; symlinks are skipped.
pub fn list(root: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for entry in read.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && entries.len() < MAX_ENTRIES {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect();
                entries.push(Entry { path: parts.join("/"), size: entry.metadata().map(|m| m.len()).unwrap_or(0) });
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// `rel` inside `root`, if it names a regular file there and no part of the way is a symlink.
pub fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.as_os_str().is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let mut path = root.to_path_buf();
    for part in rel.components() {
        path.push(part);
        if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    path.is_file().then_some(path)
}

fn item_dir(state: &Shared, project: &str, dir: &str) -> Result<PathBuf, ApiError> {
    let project = state.store.project(project).ok_or(ApiError::not_found("project"))?;
    let file = workitem::path(&project, dir).ok_or(ApiError::not_found("workitem"))?;
    Ok(file.parent().expect("workitem.md has a folder").to_path_buf())
}

pub async fn list_files(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
) -> Result<Json<Vec<Entry>>, ApiError> {
    Ok(Json(list(&item_dir(&state, &project, &dir)?)))
}

/// The raw file. It is served from the UI's origin, so it is sandboxed: an HTML file an agent
/// wrote must not run script with the session cookie.
pub async fn file(
    State(state): State<Shared>,
    UrlPath((project, dir, rel)): UrlPath<(String, String, String)>,
) -> Result<Response, ApiError> {
    let path = resolve(&item_dir(&state, &project, &dir)?, &rel).ok_or(ApiError::not_found("file"))?;
    let bytes = tokio::fs::read(&path).await.map_err(|_| ApiError::not_found("file"))?;
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct Reveal {
    path: String,
}

/// Shows the file in Finder (on Linux, opens its folder), for what the browser cannot play.
pub async fn reveal(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
    Json(body): Json<Reveal>,
) -> Result<StatusCode, ApiError> {
    let path = resolve(&item_dir(&state, &project, &dir)?, &body.path).ok_or(ApiError::not_found("file"))?;
    // `path` is absolute, so it can never read as an option.
    let mut command = if cfg!(target_os = "macos") {
        let mut open = tokio::process::Command::new("open");
        open.arg("-R").arg(&path);
        open
    } else {
        let mut open = tokio::process::Command::new("xdg-open");
        open.arg(path.parent().expect("a file has a folder"));
        open
    };
    let status = command.status().await.map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !status.success() {
        return Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("could not show the file: {status}")));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_nested_files_and_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("notes/deep")).unwrap();
        fs::write(root.join("workitem.md"), "abc").unwrap();
        fs::write(root.join("notes/deep/x.txt"), "").unwrap();
        std::os::unix::fs::symlink("/etc", root.join("etc")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", root.join("hosts")).unwrap();
        assert_eq!(
            list(root),
            vec![Entry { path: "notes/deep/x.txt".into(), size: 0 }, Entry { path: "workitem.md".into(), size: 3 }]
        );
    }

    #[test]
    fn resolve_stays_inside_the_folder() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("item");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/a.md"), "").unwrap();
        fs::write(outer.path().join("secret"), "").unwrap();
        std::os::unix::fs::symlink(outer.path(), root.join("up")).unwrap();
        assert_eq!(resolve(&root, "sub/a.md"), Some(root.join("sub/a.md")));
        for bad in ["", "sub", "../secret", "sub/../../secret", "/etc/hosts", "up/secret", "./sub/a.md", "missing"] {
            assert_eq!(resolve(&root, bad), None, "{bad}");
        }
    }
}
