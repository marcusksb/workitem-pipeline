use std::collections::{HashMap, HashSet};

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
    config::{self, Agent, AgentKind, Workspace},
    git,
    launch,
    locate::{Found, Origin},
    pipeline::{Gate, Pipeline, Resolved, Stage},
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

#[derive(Serialize)]
pub struct WorkspaceInfo {
    #[serde(flatten)]
    workspace: Workspace,
    /// What runs once for the whole workspace, the implicit `pr-status` included.
    watch: Vec<crate::pipeline::Watch>,
    /// Why the workspace is stopped: its `github_account` could not be read (decision 68).
    #[serde(skip_serializing_if = "Option::is_none")]
    account_error: Option<String>,
}

fn workspace_infos(state: &AppState, workspaces: &[Workspace]) -> Vec<WorkspaceInfo> {
    let info = |workspace: &Workspace| WorkspaceInfo {
        watch: crate::watch::workspace_watchers(state, workspace),
        account_error: crate::github::account_error(state, workspace),
        workspace: workspace.clone(),
    };
    workspaces.iter().map(info).collect()
}

pub async fn list_workspaces(State(state): State<Shared>) -> Json<Vec<WorkspaceInfo>> {
    Json(workspace_infos(&state, &state.store.workspaces()))
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
    /// Where new issues go (decision 64); optional.
    #[serde(default)]
    issue_project: String,
    /// The team a ready PR asks for review (decision 70); optional.
    #[serde(default)]
    reviewers: String,
}

/// Adds a board for a repo: a copy of the bundled board with `repo` set (decision 45).
pub async fn add_board(
    State(state): State<Shared>,
    Path(workspace): Path<String>,
    Json(body): Json<NewBoard>,
) -> Result<StatusCode, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    let board = crate::pipeline::NewBoard { name: body.name.trim(), repo: &body.repo, issue_project: body.issue_project.trim(), reviewers: body.reviewers.trim() };
    state.add_board(&workspace, &board).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

/// What the board settings dialog sends (decision 73). `repo` is not among them, and a body that
/// names it is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardChange {
    #[serde(default)]
    label: String,
    entry: String,
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    issue_project: String,
    #[serde(default)]
    reviewers: String,
}

/// Changes a board's top-level settings; an empty optional one is removed from the file.
pub async fn update_board(
    State(state): State<Shared>,
    Path((workspace, board)): Path<(String, String)>,
    Json(body): Json<BoardChange>,
) -> Result<StatusCode, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    if !state.board_names(&workspace).contains(&board) {
        return Err(ApiError::not_found("board"));
    }
    let settings = crate::pipeline::BoardSettings { label: &body.label, entry: &body.entry, order: &body.order, issue_project: &body.issue_project, reviewers: &body.reviewers };
    state.update_board(&workspace, &board, &settings).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

/// What the stage settings dialog sends (decision 76).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageChange {
    #[serde(default)]
    model: String,
    require_issue: bool,
}

/// Changes a stage's `model` and whether entering it needs a linked issue.
pub async fn update_stage(
    State(state): State<Shared>,
    Path((workspace, board, stage)): Path<(String, String, String)>,
    Json(body): Json<StageChange>,
) -> Result<StatusCode, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    if !state.board_names(&workspace).contains(&board) {
        return Err(ApiError::not_found("board"));
    }
    let pipeline = state.pipeline(&workspace, &board).pipeline.ok_or(ApiError::not_found("board"))?;
    pipeline.stage(&stage).ok_or(ApiError::not_found("stage"))?;
    let settings = crate::pipeline::StageSettings { model: &body.model, require_issue: body.require_issue };
    state.update_stage(&workspace, &board, &stage, &settings).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct WorkspaceChange {
    /// Absent keeps the agent, `null` clears it so the default one applies.
    #[serde(default, deserialize_with = "present")]
    agent: Option<Option<Agent>>,
    issue_tracker: Option<String>,
    github_account: Option<String>,
}

/// A key that is present, even as `null`, is `Some`.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, Serialize)]
pub struct AgentChange {
    agent: Option<Agent>,
}

/// Sets the workspace's agent (decisions 39, 52, 56), issue tracker (62) and GitHub account (67),
/// each only when given; an empty one (a `null` agent) clears it. Everything is checked before
/// anything is written: a new GitHub account must be one gh is logged in to, and a new tracker
/// must suit every board's `issue_project` and `issue_state`.
pub async fn update_workspace(
    State(state): State<Shared>,
    Path(name): Path<String>,
    Json(body): Json<WorkspaceChange>,
) -> Result<Json<Workspace>, ApiError> {
    let bad = |e: String| ApiError(StatusCode::BAD_REQUEST, e);
    let current = state.store.workspace(&name).ok_or(ApiError::not_found("workspace"))?;
    // Only a changed account is checked, so a broken saved one does not block the other settings.
    let login = body.github_account.as_deref().map(str::trim);
    if let Some(login) = login.filter(|l| !l.is_empty() && Some(*l) != current.github_account.as_deref()) {
        if !crate::config::valid_login(login) {
            return Err(bad(format!("'{login}' is not a GitHub login")));
        }
        crate::github::token_for(&state, login).await.map_err(bad)?;
    }
    let tracker = body.issue_tracker.as_deref().map(str::trim);
    let new_tracker = tracker.map(|t| if t.is_empty() { crate::config::ISSUE_TRACKERS[0] } else { t });
    if let Some(tracker) = new_tracker.filter(|t| *t != current.issue_tracker) {
        let conflicts = state.tracker_conflicts(&current, tracker);
        if !conflicts.is_empty() {
            return Err(bad(format!("these boards do not suit {tracker}; change their issue_project and issue_state first: {}", conflicts.join("; "))));
        }
    }
    let change = crate::config::SettingsChange { agent: body.agent, issue_tracker: tracker, github_account: login };
    let workspace = state.store.update(&name, change).map_err(bad)?.ok_or(ApiError::not_found("workspace"))?;
    // A fixed account starts the workspace again at once instead of on its next tick.
    let _ = crate::github::token(&state, &workspace).await;
    state.wake.notify_one();
    Ok(Json(workspace))
}

#[derive(Serialize)]
pub struct AgentKindInfo {
    kind: AgentKind,
    /// Whether the kind's own CLI (for Claude, `AppState::claude`) is found on `PATH`. A kind that
    /// is not can still run through a command naming its executable.
    installed: bool,
}

#[derive(Serialize)]
pub struct AgentSettings {
    agent: Option<Agent>,
    kinds: Vec<AgentKindInfo>,
}

/// The agent of every workspace without its own (decision 56; `null` means Claude), and which
/// kinds are installed.
pub async fn default_agent(State(state): State<Shared>) -> Json<AgentSettings> {
    let kinds = AgentKind::ALL
        .into_iter()
        .map(|kind| {
            let name = if kind == AgentKind::Claude { state.claude.as_str() } else { kind.name() };
            AgentKindInfo { kind, installed: config::resolve_executable(name).is_some() }
        })
        .collect();
    Json(AgentSettings { agent: state.store.default_agent(), kinds })
}

pub async fn set_default_agent(State(state): State<Shared>, Json(body): Json<AgentChange>) -> Result<Json<AgentChange>, ApiError> {
    let agent = state.store.set_default_agent(body.agent).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(AgentChange { agent }))
}

#[derive(Serialize)]
pub struct BoardInfo {
    name: String,
    /// Stages with at least one watcher, which can be paused.
    watched: Vec<String>,
    /// Stages whose watchers are paused.
    paused: Vec<String>,
    /// Per stage, the files it runs that do not resolve (handler, fixer skills, watch scripts);
    /// only stages with problems (decision 83).
    problems: HashMap<String, Vec<String>>,
    #[serde(flatten)]
    resolved: Resolved,
}

/// The boards of every workspace, keyed by workspace name, by board name.
fn boards(state: &AppState, workspaces: &[Workspace]) -> HashMap<String, Vec<BoardInfo>> {
    let boards = |workspace: &Workspace| {
        let info = |name: String| {
            let resolved = state.pipeline(workspace, &name);
            let stages = resolved.pipeline.iter().flat_map(|p| &p.stages);
            let watched = stages.clone().filter(|s| !s.watch.is_empty()).map(|s| s.name.clone()).collect();
            let problems = stages
                .map(|s| (s.name.clone(), StageFiles::of(state, workspace, &name, s).problems()))
                .filter(|(_, problems)| !problems.is_empty())
                .collect();
            BoardInfo { watched, paused: crate::watch::paused_stages(state, workspace, &name), problems, resolved, name }
        };
        state.board_names(workspace).into_iter().map(info).collect()
    };
    workspaces.iter().map(|workspace| (workspace.name.clone(), boards(workspace))).collect()
}

pub async fn pipelines(State(state): State<Shared>) -> Json<HashMap<String, Vec<BoardInfo>>> {
    Json(boards(&state, &state.store.workspaces()))
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
    /// Branch checked out in the item's worktree; `None` without a worktree.
    branch: Option<String>,
    /// Commits in the item's worktree that no remote has; `None` when it has no worktree.
    unpushed: Option<u32>,
}

/// The worktrees git knows for each repo, keyed by repo.
type Worktrees = HashMap<std::path::PathBuf, Vec<std::path::PathBuf>>;

/// Branch and unpushed commits of the item's worktree; both `None` without one.
async fn branch_of(repo: Option<&std::path::Path>, item: &Workitem, worktrees: &Worktrees) -> (Option<String>, Option<u32>) {
    let found = async {
        let repo = repo?;
        git::branch(git::worktree_for(worktrees.get(repo)?, repo, &item.dir)?).await
    };
    match found.await {
        Some((branch, unpushed)) => (Some(branch), unpushed),
        None => (None, None),
    }
}

async fn worktrees_of(repos: impl IntoIterator<Item = std::path::PathBuf>) -> Worktrees {
    let repos: Vec<_> = repos.into_iter().collect();
    let lists = futures_util::future::join_all(repos.iter().map(|repo| git::worktrees(repo))).await;
    repos.into_iter().zip(lists).collect()
}

async fn items(state: &AppState, workspaces: &[Workspace], sessions: &[tmux::Session]) -> Vec<Item> {
    let per_workspace = workspaces.iter().map(|workspace| {
        async move {
            let mut boards: HashMap<String, Option<Pipeline>> = HashMap::new();
            let items: Vec<_> = workitem::list(workspace)
                .into_iter()
                .map(|item| {
                    let board = boards.entry(item.board.clone()).or_insert_with(|| state.pipeline(workspace, &item.board).pipeline);
                    let session = tmux::session_for(sessions, workspace, &item.dir).is_some();
                    let roles = roles_of(board.as_ref(), &item);
                    let repo = board.as_ref().and_then(|b| b.repo.clone());
                    (item, session, roles, repo)
                })
                .collect();
            let worktrees = worktrees_of(boards.values().flatten().filter_map(|b| b.repo.clone())).await;
            let worktrees = &worktrees;
            futures_util::future::join_all(items.into_iter().map(|(item, session, roles, repo)| async move {
                let (branch, unpushed) = branch_of(repo.as_deref(), &item, worktrees).await;
                Item { item, session, roles, branch, unpushed }
            }))
            .await
        }
    });
    futures_util::future::join_all(per_workspace).await.into_iter().flatten().collect()
}

pub async fn list_items(State(state): State<Shared>) -> Json<Vec<Item>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    Json(items(&state, &state.store.workspaces(), &sessions).await)
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
    let repo = board.and_then(|b| b.repo);
    let (branch, unpushed) = branch_of(repo.as_deref(), &item, &worktrees_of(repo.clone()).await).await;
    Ok(Json(Detail { item: Item { item, session, roles, branch, unpushed }, content }))
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
    let repo = state
        .repo_of(&workspace, &item.board)
        .and_then(|repo| repo.ok_or_else(|| format!("board {} has no repo, so no PRs", item.board)))
        .map_err(|e| ApiError(StatusCode::CONFLICT, e))?;
    let conversation = review::conversation(&state, &workspace, &repo, &item.pr)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(conversation))
}

#[derive(Serialize)]
pub struct AgentSession {
    workspace: String,
    name: String,
    /// The agent waits for input (decision 75).
    waiting: bool,
    /// The agent exited and `remain-on-exit` keeps the pane: it ended without killing its session.
    dead: bool,
}

/// `wi-new-*` sessions of the configured workspaces, running or dead.
fn agent_sessions(workspaces: &[Workspace], sessions: &[tmux::Session], dead: &HashSet<String>) -> Vec<AgentSession> {
    workspaces
        .iter()
        .flat_map(|workspace| {
            sessions
                .iter()
                .filter(|s| tmux::is_agent_session(&s.name) && tmux::named(sessions, workspace, &s.name).is_some())
                .map(|s| AgentSession { workspace: workspace.name.clone(), name: s.name.clone(), waiting: s.waiting, dead: dead.contains(&s.name) })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub async fn list_sessions(State(state): State<Shared>) -> Json<Vec<AgentSession>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    Json(agent_sessions(&state.store.workspaces(), &sessions, &dead))
}

/// Everything the board polls, from one list of workspaces and one look at tmux, so its parts
/// agree (decision 78).
#[derive(Serialize)]
pub struct Snapshot {
    workspaces: Vec<WorkspaceInfo>,
    pipelines: HashMap<String, Vec<BoardInfo>>,
    items: Vec<Item>,
    sessions: Vec<AgentSession>,
    plans: Vec<crate::plans::Plan>,
}

pub async fn snapshot(State(state): State<Shared>) -> Json<Snapshot> {
    let workspaces = state.store.workspaces();
    let socket = state.tmux_socket.as_deref();
    let (sessions, dead) = tokio::join!(tmux::sessions(socket), tmux::dead_sessions(socket));
    Json(Snapshot {
        items: items(&state, &workspaces, &sessions).await,
        workspaces: workspace_infos(&state, &workspaces),
        pipelines: boards(&state, &workspaces),
        sessions: agent_sessions(&workspaces, &sessions, &dead),
        plans: crate::plans::all(&workspaces, &sessions, &dead),
    })
}

/// Kills a dead `wi-new-*` session of the workspace, when its *Ended* toast is dismissed. A live
/// one is refused, so a stale toast never kills a running create agent.
pub async fn kill_agent_session(State(state): State<Shared>, Path((workspace, name)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    if !tmux::is_agent_session(&name) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "not a create session".into()));
    }
    let _busy = state.busy.lock().await;
    let socket = state.tmux_socket.as_deref();
    let sessions = tmux::sessions(socket).await;
    tmux::named(&sessions, &workspace, &name).ok_or(ApiError::not_found("session"))?;
    if !tmux::dead_sessions(socket).await.contains(&name) {
        return Err(ApiError(StatusCode::CONFLICT, "the session is still running".into()));
    }
    tmux::kill(socket, &name).await;
    Ok(StatusCode::NO_CONTENT)
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
    /// The PR stage's fixer skills (CI, comments, conflict), which its `pr-review` watcher starts.
    fixers: Vec<Source>,
    /// The stage's `model` (decision 60); `None` means the agent's default.
    model: Option<String>,
    /// Entering the stage needs a linked issue (`require: {"issue": "linked"}`).
    require_issue: bool,
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

/// A file a stage runs, by name, and where it resolved.
type Resolution = (String, Result<Found, String>);

/// The files a stage runs, resolved the way the dispatcher resolves them: its watch scripts, the
/// handler skill (a `builtin:` one has none) and on a PR stage the fixer skills. The stage dialog
/// shows them and the board lists the ones that do not resolve, so both agree (decision 83).
struct StageFiles {
    scripts: Vec<Resolution>,
    handler: Option<Resolution>,
    fixers: Vec<Resolution>,
}

impl StageFiles {
    fn of(state: &AppState, workspace: &Workspace, board: &str, stage: &Stage) -> Self {
        let skill = |name: &str| (name.to_string(), launch::handler_file(state, workspace, board, name));
        StageFiles {
            scripts: stage.watch.iter().map(|w| (w.command.clone(), crate::watch::resolve(state, workspace, board, &w.command))).collect(),
            handler: stage.handler.as_deref().filter(|h| !h.starts_with("builtin:")).map(skill),
            fixers: match stage.watches_prs() {
                true => launch::Fixer::ALL.iter().map(|f| skill(f.skill())).collect(),
                false => Vec::new(),
            },
        }
    }

    fn problems(self) -> Vec<String> {
        self.handler.into_iter().chain(self.fixers).chain(self.scripts).filter_map(|(_, found)| found.err()).collect()
    }
}

/// What a stage runs: its watch scripts, the handler skill or inline prompt, and on the PR stage
/// the fixer skills.
pub async fn stage_source(
    State(state): State<Shared>,
    Path((workspace, board, stage)): Path<(String, String, String)>,
) -> Result<Json<StageSource>, ApiError> {
    let workspace = state.store.workspace(&workspace).ok_or(ApiError::not_found("workspace"))?;
    let pipeline = state.pipeline(&workspace, &board).pipeline.ok_or(ApiError::not_found("board"))?;
    let stage = pipeline.stage(&stage).ok_or(ApiError::not_found("stage"))?;
    let files = StageFiles::of(&state, &workspace, &board, stage);
    let scripts = files.scripts.into_iter().zip(&stage.watch).map(|((name, found), w)| file_source(&name, found, w.implicit.then_some("implicit"))).collect();
    let prose = match (files.handler, stage.prompt.as_deref()) {
        (Some((name, found)), _) => Some(file_source(&name, found, None)),
        (None, Some(prompt)) if stage.handler.is_none() => Some(Source {
            name: "prompt".into(),
            path: None,
            origin: None,
            content: prompt.into(),
            note: Some("inline in pipeline.json"),
            error: None,
        }),
        (None, _) => None,
    };
    let fixers = files.fixers.into_iter().map(|(name, found)| file_source(&name, found, None)).collect();
    Ok(Json(StageSource { scripts, prose, fixers, model: stage.model.clone(), require_issue: stage.requires_issue() }))
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
