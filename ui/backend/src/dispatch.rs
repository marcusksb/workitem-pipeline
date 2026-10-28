//! The dispatcher loop: reaps finished sessions, then lets the column watchers (`watch.rs`) run.
//! A pass itself costs no tokens.
use std::{path::Path, sync::Arc, time::Duration};

use crate::{
    actions::set_status,
    config::Project,
    launch,
    pipeline::Stage,
    tmux, watch,
    workitem::{self, Workitem},
    AppState,
};

const TICK: Duration = Duration::from_secs(5);

/// What a stage runs as a session. Builtin handlers are not sessions: `review.rs` owns the PR
/// stage and starts a fixer per item instead.
enum Source<'a> {
    Skill(&'a str),
    Prompt(&'a str),
}

fn source_of(stage: &Stage) -> Option<Source<'_>> {
    match (stage.handler.as_deref(), stage.prompt.as_deref()) {
        (Some(handler), _) if !handler.starts_with("builtin:") => Some(Source::Skill(handler)),
        (None, Some(prompt)) => Some(Source::Prompt(prompt)),
        _ => None,
    }
}

pub async fn run(state: Arc<AppState>) {
    loop {
        {
            let _busy = state.busy.lock().await;
            pass(&state).await;
        }
        tokio::select! {
            _ = state.wake.notified() => {}
            _ = tokio::time::sleep(TICK) => {}
        }
    }
}

pub async fn pass(state: &AppState) {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    for project in state.store.projects() {
        for item in workitem::list(&project) {
            let session = tmux::session_for(&sessions, &project, &item.dir).map(|s| s.name.as_str());
            reap(state, &project, &item, session, &dead).await;
        }
    }
    watch::pass(state).await;
}

/// Session housekeeping. `ok` means the handler is done, so its session goes; a dead pane that
/// never said so crashed, so the item is idle again; a `working` item with no session is idle
/// too. Starting handlers and reacting to PRs is the column watchers' job.
async fn reap(state: &AppState, project: &Project, item: &Workitem, session: Option<&str>, dead: &std::collections::HashSet<String>) {
    let Some(path) = workitem::path(project, &item.dir) else {
        return;
    };
    let status = item.agent_status.as_str();
    match session {
        Some(name) if status == "ok" || (dead.contains(name) && !status.starts_with("blocked")) => {
            tmux::kill(state.tmux_socket.as_deref(), name).await;
            if status != "ok" {
                let _ = set_status(&path, "ok");
            }
        }
        None if status == "working" => {
            let _ = set_status(&path, "ok");
        }
        _ => {}
    }
}

fn instructions_of(state: &AppState, project: &Project, item: &Workitem, source: &Source) -> Result<String, String> {
    match source {
        Source::Skill(name) => launch::instructions(state, project, &item.board, name),
        Source::Prompt(text) => Ok(text.to_string()),
    }
}

/// Starts the handler of the item's stage as `wi-<dir>` and marks the item `working`; a failed
/// start puts `ok` back. Shared by the dispatcher and `workitem-run-item`. The caller holds
/// `AppState::busy` and has checked that the item is idle.
pub async fn launch_handler(state: &AppState, project: &Project, item: &Workitem, path: &Path, stage: &Stage, instructions: &str) -> Result<(), String> {
    let _ = set_status(path, "working");
    let prompt = launch::handler_prompt(state, project, item, stage, instructions);
    let name = format!("wi-{}", item.dir);
    launch::spawn(state, &name, &project.repo, Some(&item.dir), &prompt).await.map_err(|e| {
        let _ = set_status(path, "ok");
        format!("cannot start {name}: {e}")
    })
}

/// What `workitem-run-item` does: starts the stage handler of an idle item, with the same
/// instructions the dispatcher would use. Policy (when, how often, attempt caps) is the caller's.
pub async fn run_handler(state: &AppState, project: &Project, item: &Workitem, path: &Path, stage: &Stage) -> Result<(), RunError> {
    let source = source_of(stage).ok_or_else(|| RunError::Refused(format!("stage {} has no handler to run", stage.name)))?;
    if item.agent_status != "ok" {
        return Err(RunError::Refused(format!("{} is {}, not ok", item.dir, item.agent_status)));
    }
    let instructions = instructions_of(state, project, item, &source).map_err(RunError::Handler)?;
    launch_handler(state, project, item, path, stage, &instructions).await.map_err(RunError::Refused)
}

/// Why `run_handler` did not start anything: the stage's handler cannot be used (not found, bad
/// name), which blocks the item, or the item is not in a state to start.
pub enum RunError {
    Handler(String),
    Refused(String),
}
