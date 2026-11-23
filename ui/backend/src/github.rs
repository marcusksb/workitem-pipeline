//! The GitHub account of a workspace (decisions 67, 68). `github_account` in `workspace.toml` names
//! a login gh knows; every process the backend starts for the workspace (its own `gh` calls,
//! watchers, sessions) gets that login's token as `GH_TOKEN`, so workspaces never share gh's active
//! account. A token gh gave is kept in memory for `KEEP`, so the dispatcher, which holds `busy`,
//! does not start a `gh` per workspace on every 5s pass; a failure is never kept. When it cannot
//! be read the workspace stops (no sessions, no watchers) and says why, until a `gh auth login`
//! fixes it.
use std::time::{Duration, Instant};

use crate::{config::Workspace, AppState};

const TIMEOUT: Duration = Duration::from_secs(15);
/// How long a token gh gave is used without asking gh again.
const KEEP: Duration = Duration::from_secs(60);

/// The token of `login`, from gh's own store (never from the cache), which it then refreshes.
pub async fn token_for(state: &AppState, login: &str) -> Result<String, String> {
    let not_logged_in = |detail: &str| format!("GitHub account {login} is not logged in: run gh auth login ({detail})");
    let mut command = tokio::process::Command::new(&state.gh);
    command.args(["auth", "token", "--user", login]).env_remove("GH_TOKEN").kill_on_drop(true);
    let output = tokio::time::timeout(TIMEOUT, command.output())
        .await
        .map_err(|_| not_logged_in("gh timed out"))?
        .map_err(|e| not_logged_in(&format!("cannot run gh: {e}")))?;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || token.is_empty() {
        state.tokens.lock().unwrap().remove(login);
        return Err(not_logged_in(String::from_utf8_lossy(&output.stderr).trim()));
    }
    state.tokens.lock().unwrap().insert(login.to_string(), (Instant::now(), token.clone()));
    Ok(token)
}

fn kept(state: &AppState, login: &str) -> Option<String> {
    let tokens = state.tokens.lock().unwrap();
    tokens.get(login).filter(|(at, _)| at.elapsed() < KEEP).map(|(_, token)| token.clone())
}

/// The workspace's token, `None` when it names no account (gh's active one applies). Remembers a
/// failure for the header (`account_error`) and forgets it once the token can be read again.
pub async fn token(state: &AppState, workspace: &Workspace) -> Result<Option<String>, String> {
    let Some(login) = &workspace.github_account else {
        state.account_errors.lock().unwrap().remove(&workspace.name);
        return Ok(None);
    };
    let result = match kept(state, login) {
        Some(token) => Ok(token),
        None => token_for(state, login).await,
    };
    let mut errors = state.account_errors.lock().unwrap();
    match &result {
        Ok(_) => errors.remove(&workspace.name),
        Err(e) => errors.insert(workspace.name.clone(), e.clone()),
    };
    result.map(Some)
}

/// Why the workspace is stopped, if its account could not be read the last time.
pub fn account_error(state: &AppState, workspace: &Workspace) -> Option<String> {
    state.account_errors.lock().unwrap().get(&workspace.name).cloned()
}

/// `gh` with the workspace's token.
pub async fn command(state: &AppState, workspace: &Workspace) -> Result<tokio::process::Command, String> {
    let mut command = tokio::process::Command::new(&state.gh);
    if let Some(token) = token(state, workspace).await? {
        command.env("GH_TOKEN", token);
    }
    Ok(command)
}
