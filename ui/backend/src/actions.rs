//! Buttons of the detail panel. Stage changes go through `workitem-move`; the rest is
//! either a frontmatter edit or a `wi-*` tmux session that runs the matching skill.
use std::path::Path;

use axum::{
    extract::{Path as UrlPath, State},
    http::StatusCode,
    Json,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    api::ApiError,
    config::Project,
    dispatch::RunError,
    git, launch,
    pipeline::{Stage, DEFAULT_BOARD},
    review::{PrStatus, Review},
    tmux,
    workitem::{self, Workitem},
    AppState, Shared,
};

type Result<T> = std::result::Result<T, ApiError>;

fn conflict(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, message.into())
}

fn internal(message: impl ToString) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
}

fn find(state: &AppState, project: &str, dir: &str) -> Result<(Project, std::path::PathBuf, Workitem)> {
    let project = state.store.project(project).ok_or(ApiError::not_found("project"))?;
    let path = workitem::path(&project, dir).ok_or(ApiError::not_found("workitem"))?;
    let (item, _) = workitem::read(&project, dir).ok_or(ApiError::not_found("workitem"))?;
    Ok((project, path, item))
}

/// The board's config for the stage the item is in. Roles come from there, never from stage names.
fn stage_of(state: &AppState, project: &Project, item: &Workitem) -> Result<Stage> {
    let board = state.pipeline(project, &item.board).pipeline.ok_or(conflict(format!("board {} has no valid file", item.board)))?;
    board.stage(&item.stage).cloned().ok_or_else(|| conflict(format!("stage {} is not on the board", item.stage)))
}

async fn has_session(state: &AppState, project: &Project, dir: &str) -> bool {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    tmux::session_for(&sessions, project, dir).is_some()
}

/// Runs `workitem-move` on the full path, so it never depends on the backend's cwd.
pub fn move_stage(state: &AppState, path: &Path, stage: &str) -> std::result::Result<(), String> {
    let script = state.skills_dir.join("workitem-pipeline/bin/workitem-move");
    let output = std::process::Command::new("python3")
        .arg(script)
        .arg(path)
        .arg(stage)
        .output()
        .map_err(|e| format!("cannot run workitem-move: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn set_status(path: &Path, status: &str) -> std::io::Result<()> {
    workitem::set_fields(path, &[("agent_status", status)])
}

fn today() -> Result<String> {
    let output = std::process::Command::new("date").arg("+%F").output().map_err(internal)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Approves at a gate: sets `approved` first, because the next stage may require it, then moves
/// the item on. A refused move undoes the flag.
pub async fn approve(State(state): State<Shared>, UrlPath((project, dir)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let (project, path, item) = find(&state, &project, &dir)?;
    let gate = stage_of(&state, &project, &item)?.gate.ok_or(conflict(format!("stage {} has no gate", item.stage)))?;
    workitem::set_fields(&path, &[("approved", "true"), ("approved_at", &today()?)]).map_err(internal)?;
    if let Err(e) = move_stage(&state, &path, &gate.approve_to) {
        let _ = workitem::set_fields(&path, &[("approved", "false")]);
        return Err(conflict(e));
    }
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Rejection {
    comment: String,
}

pub async fn reject(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
    Json(body): Json<Rejection>,
) -> Result<StatusCode> {
    let (project, path, item) = find(&state, &project, &dir)?;
    let gate = stage_of(&state, &project, &item)?.gate.ok_or(conflict(format!("stage {} has no gate", item.stage)))?;
    if body.comment.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a rejection needs a comment".into()));
    }
    workitem::set_rejection(&path, &body.comment).map_err(internal)?;
    move_stage(&state, &path, &gate.reject_to).map_err(conflict)?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct BriefEdit {
    /// The body the editor started from.
    base: String,
    body: String,
}

/// Saves the brief (the markdown after the frontmatter); the frontmatter, `stage:` included, is
/// never sent or written.
pub async fn edit_brief(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
    Json(body): Json<BriefEdit>,
) -> Result<StatusCode> {
    let (_, path, _) = find(&state, &project, &dir)?;
    if !workitem::set_body(&path, &body.base, &body.body).map_err(internal)? {
        return Err(conflict("the brief changed since you opened the editor; copy your text and reload"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct FrontmatterEdit {
    /// Only the fields that changed, by key.
    fields: std::collections::HashMap<String, String>,
}

/// Fields that only `workitem-move` may change.
const MOVE_ONLY: [&str; 2] = ["stage", "board"];

/// Sets values of existing frontmatter fields. `stage` and `board` are refused (`workitem-move`
/// enforces the pipeline rules for them), and so are unknown keys and multi-line values.
pub async fn edit_frontmatter(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
    Json(body): Json<FrontmatterEdit>,
) -> Result<StatusCode> {
    let (_, path, _) = find(&state, &project, &dir)?;
    let text = std::fs::read_to_string(&path).map_err(internal)?;
    let current = workitem::frontmatter(&text);
    for (key, value) in &body.fields {
        let bad = |message: String| ApiError(StatusCode::BAD_REQUEST, message);
        if MOVE_ONLY.contains(&key.as_str()) {
            return Err(bad(format!("{key} changes through a stage move, not an edit")));
        }
        if !current.contains_key(key) {
            return Err(bad(format!("{key} is not a field of this workitem")));
        }
        if value.contains(['\n', '\r']) {
            return Err(bad(format!("{key} must be a single line")));
        }
    }
    let fields: Vec<(&str, &str)> = body.fields.iter().map(|(k, v)| (k.as_str(), v.trim())).collect();
    workitem::set_fields(&path, &fields).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// With a live session the agent still waits for its answer, so it goes back to `working`
/// (an `ok` would make the dispatcher kill the session).
pub async fn clear_blocked(State(state): State<Shared>, UrlPath((project, dir)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let (project, path, item) = find(&state, &project, &dir)?;
    if !item.agent_status.starts_with("blocked") {
        return Err(conflict("workitem is not blocked"));
    }
    let status = if has_session(&state, &project, &dir).await { "working" } else { "ok" };
    set_status(&path, status).map_err(internal)?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

/// Ends the agent session and lets the dispatcher retry (or stop at the attempt cap).
pub async fn kill_session(State(state): State<Shared>, UrlPath((project, dir)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let (project, path, _) = find(&state, &project, &dir)?;
    let _busy = state.busy.lock().await;
    if !has_session(&state, &project, &dir).await {
        return Err(conflict("no session is running"));
    }
    tmux::kill(state.tmux_socket.as_deref(), &format!("wi-{dir}")).await;
    set_status(&path, "ok").map_err(internal)?;
    state.wake.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct Launched {
    session: String,
}

pub async fn start(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
) -> Result<(StatusCode, Json<Launched>)> {
    let (project, path, item) = find(&state, &project, &dir)?;
    let stage = stage_of(&state, &project, &item)?;
    if !item.approved || !stage.develops() {
        return Err(conflict("only an approved workitem in a development stage can start"));
    }
    let instructions = launch::instructions(&state, &project, &item.board, stage.handler.as_deref().unwrap_or_default()).map_err(conflict)?;
    let prompt = launch::handler_prompt(&state, &project, &item, &stage, &instructions);
    launch_item_session(&state, &project, &path, &item, &prompt).await
}

/// Starts the handler of the item's stage, as the dispatcher would. For `workitem-run-item`,
/// which column watcher scripts use to decide for themselves when an item gets its handler.
pub async fn run_handler(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
) -> Result<(StatusCode, Json<Launched>)> {
    let (project, path, item) = find(&state, &project, &dir)?;
    let stage = stage_of(&state, &project, &item)?;
    let _busy = state.busy.lock().await;
    if has_session(&state, &project, &dir).await {
        return Err(conflict(format!("{dir} already has a session")));
    }
    crate::dispatch::run_handler(&state, &project, &item, &path, &stage).await.map_err(|e| match e {
        // The handler itself is unusable: not a state that retrying fixes.
        RunError::Handler(message) => ApiError(StatusCode::UNPROCESSABLE_ENTITY, message),
        RunError::Refused(message) => conflict(message),
    })?;
    Ok((StatusCode::ACCEPTED, Json(Launched { session: format!("wi-{dir}") })))
}

#[derive(Deserialize)]
pub struct Fix {
    kind: String,
}

/// Starts the fixer session for an item's PR (`ci`, `conflict`, `changes` or `comments`). For the
/// `pr-review` watcher, which decides when; this only launches, like `run_handler`.
pub async fn fix(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
    Json(Fix { kind }): Json<Fix>,
) -> Result<(StatusCode, Json<Launched>)> {
    let (project, path, item) = find(&state, &project, &dir)?;
    if !item.pr.starts_with("http") {
        return Err(conflict("the workitem has no PR link"));
    }
    if item.agent_status != "ok" {
        return Err(conflict(format!("{dir} is {}, not ok", item.agent_status)));
    }
    let prompt = match kind.as_str() {
        "ci" => launch::ci_fix_prompt(&dir, &project.repo, &item.pr),
        "conflict" => launch::conflict_fix_prompt(&dir, &project.repo, &item.pr),
        "changes" => launch::changes_fix_prompt(&dir, &project.repo, &item.pr),
        "comments" => launch::comments_fix_prompt(&dir, &project.repo, &item.pr),
        other => return Err(ApiError(StatusCode::BAD_REQUEST, format!("'{other}' is not a fixer"))),
    };
    launch_item_session(&state, &project, &path, &item, &prompt).await
}

/// Keeps the PR status the `pr-review` watcher read, for the board's chips.
pub async fn set_review(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
    Json(status): Json<PrStatus>,
) -> Result<StatusCode> {
    let (project, ..) = find(&state, &project, &dir)?;
    state.reviews.lock().unwrap().insert(format!("{}/{dir}", project.name), Review { status });
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_review(State(state): State<Shared>, UrlPath((project, dir)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let (project, ..) = find(&state, &project, &dir)?;
    state.reviews.lock().unwrap().remove(&format!("{}/{dir}", project.name));
    Ok(StatusCode::NO_CONTENT)
}

/// A free-form agent session on the workitem, in any stage: the human drives it from the
/// terminal tab. Same `wi-<dir>` session as the handlers, so it is the item's lock too.
pub async fn start_session(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
) -> Result<(StatusCode, Json<Launched>)> {
    let (project, path, item) = find(&state, &project, &dir)?;
    let prompt = launch::session_prompt(&dir, &project.repo);
    launch_item_session(&state, &project, &path, &item, &prompt).await
}

/// Like `start_session`, but the agent opens on the brief and plan so the human can discuss
/// and adjust them.
pub async fn discuss_brief(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
) -> Result<(StatusCode, Json<Launched>)> {
    let (project, path, item) = find(&state, &project, &dir)?;
    let prompt = launch::brief_prompt(&dir, &project.repo);
    launch_item_session(&state, &project, &path, &item, &prompt).await
}

/// Starts `wi-<dir>` with `prompt`. The status is `working` while it runs: an `ok` item with
/// a live session is reaped by the dispatcher.
async fn launch_item_session(
    state: &AppState,
    project: &Project,
    path: &Path,
    item: &Workitem,
    prompt: &str,
) -> Result<(StatusCode, Json<Launched>)> {
    if item.agent_status.starts_with("blocked") {
        return Err(conflict("clear the blocked status first"));
    }
    let _busy = state.busy.lock().await;
    let session = format!("wi-{}", item.dir);
    set_status(path, "working").map_err(internal)?;
    if let Err(e) = launch::spawn(state, &session, &project.repo, Some(&item.dir), prompt).await {
        let _ = set_status(path, &item.agent_status);
        return Err(conflict(e));
    }
    Ok((StatusCode::ACCEPTED, Json(Launched { session })))
}

/// Runs `gh pr <subcommand> <pr url> <args>` in the project's repo, then forgets the cached
/// PR reading and wakes the stage's watchers so the next pass reads the PR at once.
async fn gh_pr(state: &AppState, project: &Project, dir: &str, item: &Workitem, args: &[&str]) -> Result<()> {
    if !stage_of(state, project, item)?.watches_prs() || !item.pr.starts_with("http") {
        return Err(conflict("only a workitem in the PR stage with a PR link can be changed on GitHub"));
    }
    let mut command = tokio::process::Command::new(&state.gh);
    command.arg("pr").arg(args[0]).arg(&item.pr).args(&args[1..]).current_dir(&project.repo).kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(60), command.output())
        .await
        .map_err(|_| conflict("gh timed out"))?
        .map_err(|e| internal(format!("cannot run gh: {e}")))?;
    if !output.status.success() {
        return Err(conflict(String::from_utf8_lossy(&output.stderr).trim()));
    }
    state.reviews.lock().unwrap().remove(&format!("{}/{}", project.name, dir));
    crate::watch::forget(state, project, &item.board, &item.stage);
    state.wake.notify_one();
    Ok(())
}

/// Squash-merges the PR with `gh`. Moving the item to `done` is left to the dispatcher's
/// review pass: the same path also covers a merge queue that lands the PR later.
pub async fn merge(State(state): State<Shared>, UrlPath((project, dir)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let (project, _, item) = find(&state, &project, &dir)?;
    gh_pr(&state, &project, &dir, &item, &["merge", "--squash"]).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Rebases the PR's remote branch onto its base branch on GitHub (`gh pr update-branch
/// --rebase`), so no local checkout is touched. GitHub refuses when the rebase conflicts.
pub async fn rebase(State(state): State<Shared>, UrlPath((project, dir)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let (project, _, item) = find(&state, &project, &dir)?;
    gh_pr(&state, &project, &dir, &item, &["update-branch", "--rebase"]).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct Removed {
    /// Set when git refused to remove the worktree; the workitem folder is gone regardless.
    worktree_left: Option<String>,
}

/// Path of the workitem's worktree if git has it registered.
async fn worktree_of(repo: &Path, dir: &str) -> Option<std::path::PathBuf> {
    git::worktree_for(&git::worktrees(repo).await, repo, dir).cloned()
}

/// Removal is done here, not by an agent, so it is finished when the request returns and
/// the dispatcher never sees a half-removed item. Stops the session, drops the worktree
/// (never `--force`), then deletes the folder. Branch and PR stay.
pub async fn remove(
    State(state): State<Shared>,
    UrlPath((project, dir)): UrlPath<(String, String)>,
) -> Result<Json<Removed>> {
    let (project, path, _) = find(&state, &project, &dir)?;
    let _busy = state.busy.lock().await;
    // Only this project's session; a same-named session from another repo is not ours.
    let session = format!("wi-{dir}");
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    if tmux::named(&sessions, &project, &session).is_some() {
        tmux::kill(state.tmux_socket.as_deref(), &session).await;
    }

    let mut worktree_left = None;
    if let Some(worktree) = worktree_of(&project.repo, &dir).await {
        let teardown = worktree.join("bin/teardown_worktree");
        if teardown.is_file() {
            let _ = tokio::process::Command::new(&teardown).current_dir(&worktree).output().await;
        }
        let out = tokio::process::Command::new("git")
            .args(["worktree", "remove"])
            .arg(&worktree)
            .current_dir(&project.repo)
            .output()
            .await
            .map_err(internal)?;
        if !out.status.success() {
            worktree_left = Some(format!("{}: {}", worktree.display(), String::from_utf8_lossy(&out.stderr).trim()));
        }
    }

    let folder = path.parent().ok_or_else(|| internal("workitem has no folder"))?;
    std::fs::remove_dir_all(folder).map_err(internal)?;
    state.wake.notify_one();
    Ok(Json(Removed { worktree_left }))
}

#[derive(Deserialize)]
pub struct NewWorkitem {
    input: String,
    /// Board the item starts on; `default` when absent.
    #[serde(default)]
    board: Option<String>,
}

pub async fn create(
    State(state): State<Shared>,
    UrlPath(project): UrlPath<String>,
    Json(body): Json<NewWorkitem>,
) -> Result<(StatusCode, Json<Launched>)> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    if body.input.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "describe the task or paste a Jira key or Sentry URL".into()));
    }
    let board = body.board.unwrap_or_else(|| DEFAULT_BOARD.into());
    if !state.board_names(&project).contains(&board) {
        return Err(ApiError(StatusCode::BAD_REQUEST, format!("the project has no board '{board}'")));
    }
    let id: [u8; 3] = rand::rng().random();
    let session = format!("wi-new-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let prompt = launch::create_prompt(&state, &project, &session, body.input.trim(), &board).map_err(conflict)?;
    let _busy = state.busy.lock().await;
    launch::spawn(&state, &session, &project.repo, None, &prompt).await.map_err(conflict)?;
    Ok((StatusCode::ACCEPTED, Json(Launched { session })))
}
