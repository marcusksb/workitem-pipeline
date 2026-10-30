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
    config::Project,
    git,
    launch,
    locate::{Found, Origin},
    pipeline::{Gate, Pipeline, Resolved},
    review::{self, Conversation, PrStatus},
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

pub async fn list_projects(State(state): State<Shared>) -> Json<Vec<Project>> {
    Json(state.store.projects())
}

#[derive(Deserialize)]
pub struct NewProject {
    name: String,
    repo: String,
}

pub async fn add_project(
    State(state): State<Shared>,
    Json(body): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let project = state
        .store
        .add_project(&body.name, &body.repo)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    Ok((StatusCode::CREATED, Json(project)))
}

pub async fn remove_project(State(state): State<Shared>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    match state.store.remove_project(&name) {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(ApiError::not_found("project")),
        Err(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, e)),
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

/// The boards of every project, keyed by project name; `default` comes first.
pub async fn pipelines(State(state): State<Shared>) -> Json<HashMap<String, Vec<BoardInfo>>> {
    let boards = |project: &Project| {
        let info = |name: String| {
            let resolved = state.pipeline(project, &name);
            let stages = resolved.pipeline.iter().flat_map(|p| &p.stages);
            let watched = stages.filter(|s| !s.watch.is_empty()).map(|s| s.name.clone()).collect();
            BoardInfo { watched, paused: crate::watch::paused_stages(&state, project, &name), resolved, name }
        };
        state.board_names(project).into_iter().map(info).collect()
    };
    Json(state.store.projects().iter().map(|project| (project.name.clone(), boards(project))).collect())
}

/// What the board lets a human do with an item in its current stage, so the UI never needs to
/// know stage names.
#[derive(Serialize, Default)]
pub struct Roles {
    /// The stage is a gate: approve and reject buttons.
    gate: Option<Gate>,
    /// The stage polls PRs: PR actions.
    pr_watch: bool,
    /// The stage only shows PR status.
    #[serde(skip)]
    pr_status: bool,
    /// An approved item here can be started.
    develops: bool,
}

fn roles_of(board: Option<&Pipeline>, item: &Workitem) -> Roles {
    let Some(stage) = board.and_then(|b| b.stage(&item.stage)) else {
        return Roles::default();
    };
    Roles { gate: stage.gate.clone(), pr_watch: stage.watches_prs(), pr_status: stage.shows_pr_status(), develops: stage.develops() }
}

#[derive(Serialize)]
pub struct Item {
    #[serde(flatten)]
    item: Workitem,
    session: bool,
    #[serde(flatten)]
    roles: Roles,
    /// PR approval, merge and CI status; only items in the PR stage have one.
    review: Option<PrStatus>,
    /// Commits in the item's worktree that no remote has; `None` when it has no worktree.
    unpushed: Option<u32>,
    /// The PR head moved past the item's `head_sha`.
    pr_moved: bool,
}

fn pr_moved(item: &Workitem, review: Option<&PrStatus>) -> bool {
    let head = review.and_then(|r| r.head.as_deref());
    !item.head_sha.is_empty() && head.is_some_and(|head| head != item.head_sha)
}

async fn unpushed_of(project: &Project, item: &Workitem, worktrees: &[std::path::PathBuf]) -> Option<u32> {
    git::unpushed(git::worktree_for(worktrees, &project.repo, &item.dir)?).await
}

fn review_of(state: &AppState, item: &Workitem, roles: &Roles) -> Option<PrStatus> {
    if !roles.pr_watch && !roles.pr_status {
        return None;
    }
    state.reviews.lock().unwrap().get(&format!("{}/{}", item.project, item.dir)).map(|r| r.status.clone())
}

pub async fn list_items(State(state): State<Shared>) -> Json<Vec<Item>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let projects = state.store.projects();
    let per_project = projects.iter().map(|project| {
        let (state, sessions) = (&state, &sessions);
        async move {
            let worktrees = git::worktrees(&project.repo).await;
            let mut boards: HashMap<String, Option<Pipeline>> = HashMap::new();
            let items = workitem::list(project).into_iter().map(|item| {
                let board = boards.entry(item.board.clone()).or_insert_with(|| state.pipeline(project, &item.board).pipeline);
                let session = tmux::session_for(sessions, project, &item.dir).is_some();
                let roles = roles_of(board.as_ref(), &item);
                let review = review_of(state, &item, &roles);
                (item, session, roles, review)
            });
            let worktrees = &worktrees;
            futures_util::future::join_all(items.map(|(item, session, roles, review)| async move {
                let unpushed = unpushed_of(project, &item, worktrees).await;
                let pr_moved = pr_moved(&item, review.as_ref());
                Item { item, session, roles, review, unpushed, pr_moved }
            }))
            .await
        }
    });
    Json(futures_util::future::join_all(per_project).await.into_iter().flatten().collect())
}

#[derive(Serialize)]
pub struct Detail {
    #[serde(flatten)]
    item: Item,
    content: String,
}

pub async fn item(
    State(state): State<Shared>,
    Path((project, dir)): Path<(String, String)>,
) -> Result<Json<Detail>, ApiError> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    let (item, content) = workitem::read(&project, &dir).ok_or(ApiError::not_found("workitem"))?;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let session = tmux::session_for(&sessions, &project, &dir).is_some();
    let roles = roles_of(state.pipeline(&project, &item.board).pipeline.as_ref(), &item);
    let review = review_of(&state, &item, &roles);
    let unpushed = unpushed_of(&project, &item, &git::worktrees(&project.repo).await).await;
    let pr_moved = pr_moved(&item, review.as_ref());
    Ok(Json(Detail { item: Item { item, session, roles, review, unpushed, pr_moved }, content }))
}

/// The PR description and comments of a workitem, read live with `gh`.
pub async fn pr_conversation(
    State(state): State<Shared>,
    Path((project, dir)): Path<(String, String)>,
) -> Result<Json<Conversation>, ApiError> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    let (item, _) = workitem::read(&project, &dir).ok_or(ApiError::not_found("workitem"))?;
    // `gh` takes a URL; anything else (or an option-looking value) is not a PR link.
    if !item.pr.starts_with("http") {
        return Err(ApiError::not_found("pull request"));
    }
    let conversation = review::conversation(&state, &project.repo, &item.pr)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(conversation))
}

#[derive(Serialize)]
pub struct AgentSession {
    project: String,
    name: String,
}

/// Running `wi-new-*` sessions of the configured projects.
pub async fn list_sessions(State(state): State<Shared>) -> Json<Vec<AgentSession>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let found = state
        .store
        .projects()
        .iter()
        .flat_map(|project| {
            sessions
                .iter()
                .filter(|s| tmux::is_agent_session(&s.name) && tmux::named(&sessions, project, &s.name).is_some())
                .map(|s| AgentSession { project: project.name.clone(), name: s.name.clone() })
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
    Path((project, board, stage)): Path<(String, String, String)>,
) -> Result<Json<StageSource>, ApiError> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    let pipeline = state.pipeline(&project, &board).pipeline.ok_or(ApiError::not_found("board"))?;
    let stage = pipeline.stage(&stage).ok_or(ApiError::not_found("stage"))?;
    let scripts = stage
        .watch
        .iter()
        .map(|w| file_source(&w.command, crate::watch::resolve(&state, &project, &board, &w.command), w.implicit.then_some("implicit")))
        .collect();
    let prose = match (stage.handler.as_deref(), stage.prompt.as_deref()) {
        (Some(handler), _) if handler.starts_with("builtin:") => None,
        (Some(handler), _) => Some(file_source(handler, launch::handler_file(&state, &project, &board, handler), None)),
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
    Path((project, board, stage)): Path<(String, String, String)>,
    Json(body): Json<Pause>,
) -> Result<StatusCode, ApiError> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    let pipeline = state.pipeline(&project, &board).pipeline.ok_or(ApiError::not_found("board"))?;
    let stage = pipeline.stage(&stage).ok_or(ApiError::not_found("stage"))?;
    crate::watch::set_paused(&state, &project, &board, &stage.name, body.paused).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !body.paused {
        state.wake.notify_one();
    }
    Ok(StatusCode::NO_CONTENT)
}
