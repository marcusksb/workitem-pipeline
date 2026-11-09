use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth,
    config::Workspace,
    git,
    launch,
    locate::{Found, Origin},
    pipeline::{Gate, Pipeline, Resolved},
    review::{self, Conversation},
    tmux,
    workitem::{self, Workitem},
    AppState, Shared,
};

pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn not_found(what: &str) -> Self {
        ApiError(StatusCode::NOT_FOUND, format!("{what} not found"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

#[derive(Deserialize)]
pub struct Login {
    token: String,
}

pub async fn login(State(state): State<Shared>, Json(body): Json<Login>) -> Result<Response, ApiError> {
    let token = state.store.token();
    if !auth::token_matches(&token, &body.token) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong token".into()));
    }
    Ok(([(header::SET_COOKIE, auth::set_cookie(&token))], StatusCode::NO_CONTENT).into_response())
}

/// Reaching this handler means the guard accepted the cookie.
pub async fn me() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub async fn list_workspaces(State(state): State<Shared>) -> Json<Vec<Workspace>> {
    Json(state.store.workspaces())
}

#[derive(Deserialize)]
pub struct NewWorkspace {
    name: String,
}

/// Creates an empty workspace folder (decision 44).
pub async fn add_workspace(
    State(state): State<Shared>,
    Json(body): Json<NewWorkspace>,
) -> Result<(StatusCode, Json<Workspace>), ApiError> {
    let workspace = state.store.add_workspace(&body.name).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

#[derive(Deserialize)]
pub struct NewBoard {
    name: String,
    repo: String,
}

/// Adds a board for a repo: a copy of the bundled board with `repo` set (decision 45).
pub async fn add_board(
    State(state): State<Shared>,
    Path(workspace): Path<String>,
    Json(body): Json<NewBoard>,
) -> Result<StatusCode, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    state.add_board(&workspace, body.name.trim(), &body.repo).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct WorkspaceChange {
    agent: String,
}

/// Sets the workspace's agent (decision 39); an empty one clears it.
pub async fn update_workspace(
    State(state): State<Shared>,
    Path(name): Path<String>,
    Json(body): Json<WorkspaceChange>,
) -> Result<Json<Workspace>, ApiError> {
    match state.store.set_agent(&name, &body.agent) {
        Ok(Some(workspace)) => Ok(Json(workspace)),
        Ok(None) => Err(ApiError::not_found("workspace")),
        Err(e) => Err(ApiError(StatusCode::BAD_REQUEST, e)),
    }
}

#[derive(Serialize)]
pub struct BoardInfo {
    name: String,
    /// Stages with at least one watcher, which can be paused.
    watched: Vec<String>,
    /// Stages whose watchers are paused.
    paused: Vec<String>,
    #[serde(flatten)]
    resolved: Resolved,
}

/// The boards of every workspace, keyed by workspace name, by board name.
pub async fn pipelines(State(state): State<Shared>) -> Json<HashMap<String, Vec<BoardInfo>>> {
    let boards = |workspace: &Workspace| {
        let info = |name: String| {
            let resolved = state.pipeline(workspace, &name);
            let stages = resolved.pipeline.iter().flat_map(|p| &p.stages);
            let watched = stages.filter(|s| !s.watch.is_empty()).map(|s| s.name.clone()).collect();
            BoardInfo { watched, paused: crate::watch::paused_stages(&state, workspace, &name), resolved, name }
        };
        state.board_names(workspace).into_iter().map(info).collect()
    };
    Json(state.store.workspaces().iter().map(|workspace| (workspace.name.clone(), boards(workspace))).collect())
}

/// What the board lets a human do with an item in its current stage, so the UI never needs to
/// know stage names.
#[derive(Serialize, Default)]
pub struct Roles {
    /// The stage is a gate: approve and reject buttons.
    gate: Option<Gate>,
    /// The stage polls PRs: PR actions.
    pr_watch: bool,
    /// An approved item here can be started.
    develops: bool,
}

fn roles_of(board: Option<&Pipeline>, item: &Workitem) -> Roles {
    let Some(stage) = board.and_then(|b| b.stage(&item.stage)) else {
        return Roles::default();
    };
    Roles { gate: stage.gate.clone(), pr_watch: stage.watches_prs(), develops: stage.develops() }
}

#[derive(Serialize)]
pub struct Item {
    #[serde(flatten)]
    item: Workitem,
    session: bool,
    #[serde(flatten)]
    roles: Roles,
    /// Commits in the item's worktree that no remote has; `None` when it has no worktree.
    unpushed: Option<u32>,
}

/// The worktrees git knows for each repo, keyed by repo.
type Worktrees = HashMap<std::path::PathBuf, Vec<std::path::PathBuf>>;

async fn unpushed_of(repo: Option<&std::path::Path>, item: &Workitem, worktrees: &Worktrees) -> Option<u32> {
    let repo = repo?;
    git::unpushed(git::worktree_for(worktrees.get(repo)?, repo, &item.dir)?).await
}

async fn worktrees_of(repos: impl IntoIterator<Item = std::path::PathBuf>) -> Worktrees {
    let repos: Vec<_> = repos.into_iter().collect();
    let lists = futures_util::future::join_all(repos.iter().map(|repo| git::worktrees(repo))).await;
    repos.into_iter().zip(lists).collect()
}

pub async fn list_items(State(state): State<Shared>) -> Json<Vec<Item>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let workspaces = state.store.workspaces();
    let per_workspace = workspaces.iter().map(|workspace| {
        let (state, sessions) = (&state, &sessions);
        async move {
            let mut boards: HashMap<String, Option<Pipeline>> = HashMap::new();
            let items: Vec<_> = workitem::list(workspace)
                .into_iter()
                .map(|item| {
                    let board = boards.entry(item.board.clone()).or_insert_with(|| state.pipeline(workspace, &item.board).pipeline);
                    let session = tmux::session_for(sessions, workspace, &item.dir).is_some();
                    let roles = roles_of(board.as_ref(), &item);
                    let repo = board.as_ref().map(|b| b.repo.clone());
                    (item, session, roles, repo)
                })
                .collect();
            let worktrees = worktrees_of(boards.values().flatten().map(|b| b.repo.clone())).await;
            let worktrees = &worktrees;
            futures_util::future::join_all(items.into_iter().map(|(item, session, roles, repo)| async move {
                let unpushed = unpushed_of(repo.as_deref(), &item, worktrees).await;
                Item { item, session, roles, unpushed }
            }))
            .await
        }
    });
    Json(futures_util::future::join_all(per_workspace).await.into_iter().flatten().collect())
}

#[derive(Serialize)]
pub struct Detail {
    #[serde(flatten)]
    item: Item,
    content: String,
}

pub async fn item(
    State(state): State<Shared>,
    Path((workspace, dir)): Path<(String, String)>,
) -> Result<Json<Detail>, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    let (item, content) = workitem::read(&workspace, &dir).ok_or(ApiError::not_found("workitem"))?;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let session = tmux::session_for(&sessions, &workspace, &dir).is_some();
    let board = state.pipeline(&workspace, &item.board).pipeline;
    let roles = roles_of(board.as_ref(), &item);
    let repo = board.map(|b| b.repo);
    let unpushed = unpushed_of(repo.as_deref(), &item, &worktrees_of(repo.clone()).await).await;
    Ok(Json(Detail { item: Item { item, session, roles, unpushed }, content }))
}

/// The PR description and comments of a workitem, read live with `gh`.
pub async fn pr_conversation(
    State(state): State<Shared>,
    Path((workspace, dir)): Path<(String, String)>,
) -> Result<Json<Conversation>, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    let (item, _) = workitem::read(&workspace, &dir).ok_or(ApiError::not_found("workitem"))?;
    // `gh` takes a URL; anything else (or an option-looking value) is not a PR link.
    if !item.pr.starts_with("http") {
        return Err(ApiError::not_found("pull request"));
    }
    let repo = state.repo_of(&workspace, &item.board).map_err(|e| ApiError(StatusCode::CONFLICT, e))?;
    let conversation = review::conversation(&state, &repo, &item.pr)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(conversation))
}

#[derive(Serialize)]
pub struct AgentSession {
    workspace: String,
    name: String,
}

/// Running `wi-new-*` sessions of the configured workspaces.
pub async fn list_sessions(State(state): State<Shared>) -> Json<Vec<AgentSession>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let found = state
        .store
        .workspaces()
        .iter()
        .flat_map(|workspace| {
            sessions
                .iter()
                .filter(|s| tmux::is_agent_session(&s.name) && tmux::named(&sessions, workspace, &s.name).is_some())
                .map(|s| AgentSession { workspace: workspace.name.clone(), name: s.name.clone() })
                .collect::<Vec<_>>()
        })
        .collect();
    Json(found)
}

/// A file a stage runs, or the inline prompt that stands in for one.
#[derive(Serialize)]
pub struct Source {
    name: String,
    path: Option<String>,
    origin: Option<Origin>,
    content: String,
    /// Why this is not a plain file: `implicit`, `inline in pipeline.json`.
    note: Option<&'static str>,
    /// The file could not be found or read.
    error: Option<String>,
}

#[derive(Serialize)]
pub struct Learnings {
    path: String,
    /// Empty while no fixer has written the file.
    content: String,
}

/// The `learnings.md` of a board that exists in the workspace.
fn learnings_path(state: &AppState, workspace: &str, board: &str) -> Result<std::path::PathBuf, ApiError> {
    let workspace = state.store.workspace(workspace).ok_or(ApiError::not_found("workspace"))?;
    state.pipeline(&workspace, board).pipeline.ok_or(ApiError::not_found("board"))?;
    launch::learnings_file(&workspace.dir, board).ok_or(ApiError::not_found("board"))
}

/// A board's learnings, which the review fixers write and code-writing sessions get.
pub async fn learnings(State(state): State<Shared>, Path((workspace, board)): Path<(String, String)>) -> Result<Json<Learnings>, ApiError> {
    let path = learnings_path(&state, &workspace, &board)?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    Ok(Json(Learnings { path: path.display().to_string(), content }))
}

#[derive(Deserialize)]
pub struct LearningsEdit {
    /// The content the editor started from; empty for a file that did not exist.
    base: String,
    content: String,
}

/// Saves a board's learnings, unless the file changed since the editor opened (a fixer may have
/// added a rule meanwhile).
pub async fn save_learnings(
    State(state): State<Shared>,
    Path((workspace, board)): Path<(String, String)>,
    Json(edit): Json<LearningsEdit>,
) -> Result<StatusCode, ApiError> {
    let path = learnings_path(&state, &workspace, &board)?;
    let internal = |e: std::io::Error| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {e}", path.display()));
    if std::fs::read_to_string(&path).unwrap_or_default() != edit.base {
        return Err(ApiError(StatusCode::CONFLICT, "the learnings changed since you opened the editor; copy your text and reload".into()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(internal)?;
    }
    std::fs::write(&path, edit.content).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct StageSource {
    scripts: Vec<Source>,
    prose: Option<Source>,
}

fn file_source(name: &str, found: Result<Found, String>, note: Option<&'static str>) -> Source {
    let mut source = Source { name: name.into(), path: None, origin: None, content: String::new(), note, error: None };
    match found.and_then(|f| match std::fs::read_to_string(&f.path) {
        Ok(text) => Ok((f, text)),
        Err(e) => Err(format!("{}: {e}", f.path.display())),
    }) {
        Ok((found, content)) => (source.path, source.origin, source.content) = (Some(found.path.display().to_string()), Some(found.origin), content),
        Err(error) => source.error = Some(error),
    }
    source
}

/// What a stage runs, resolved the way the dispatcher resolves it: its watch scripts, and the
/// handler skill or inline prompt.
pub async fn stage_source(
    State(state): State<Shared>,
    Path((workspace, board, stage)): Path<(String, String, String)>,
) -> Result<Json<StageSource>, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    let pipeline = state.pipeline(&workspace, &board).pipeline.ok_or(ApiError::not_found("board"))?;
    let stage = pipeline.stage(&stage).ok_or(ApiError::not_found("stage"))?;
    let scripts = stage
        .watch
        .iter()
        .map(|w| file_source(&w.command, crate::watch::resolve(&state, &workspace, &board, &w.command), w.implicit.then_some("implicit")))
        .collect();
    let prose = match (stage.handler.as_deref(), stage.prompt.as_deref()) {
        (Some(handler), _) if handler.starts_with("builtin:") => None,
        (Some(handler), _) => Some(file_source(handler, launch::handler_file(&state, &workspace, &board, handler), None)),
        (None, Some(prompt)) => Some(Source {
            name: "prompt".into(),
            path: None,
            origin: None,
            content: prompt.into(),
            note: Some("inline in pipeline.json"),
            error: None,
        }),
        (None, None) => None,
    };
    Ok(Json(StageSource { scripts, prose }))
}

#[derive(Deserialize)]
pub struct Pause {
    paused: bool,
}

/// Pauses or resumes the watchers of a stage. Resuming wakes the dispatcher, so a watcher that came
/// due meanwhile runs at once.
pub async fn pause_stage(
    State(state): State<Shared>,
    Path((workspace, board, stage)): Path<(String, String, String)>,
    Json(body): Json<Pause>,
) -> Result<StatusCode, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    let pipeline = state.pipeline(&workspace, &board).pipeline.ok_or(ApiError::not_found("board"))?;
    let stage = pipeline.stage(&stage).ok_or(ApiError::not_found("stage"))?;
    crate::watch::set_paused(&state, &workspace, &board, &stage.name, body.paused).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !body.paused {
        state.wake.notify_one();
    }
    Ok(StatusCode::NO_CONTENT)
}
