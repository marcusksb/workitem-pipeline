//! bkt identity helpers (decision 82): the Cloud CLI and the workspace's context.

use crate::AppState;
use serde::Deserialize;
use std::time::Duration;

/// Whether bkt knows the context and can act with it: `bkt context list` must carry the
/// name, and an authenticated call (`bkt api /user`) must succeed — `context list` alone
/// needs no credentials. Fails closed: a bkt that cannot run, an unknown context name or an
/// unauthenticated context is an error, like `token_for` is for a GitHub account.
pub async fn context_for(state: &AppState, context: &str) -> Result<(), String> {
    let unknown = |detail: &str| format!("bkt context {context} is not set up: run bkt context create {context} ({detail})");
    let unauthenticated = |detail: &str| format!("bkt context {context} has no working credentials: {detail}");
    let mut command = tokio::process::Command::new(&state.bkt);
    command.args(["context", "list", "--json", "--context", context]).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| unknown("bkt timed out"))?
        .map_err(|e| unknown(&format!("cannot run bkt: {e}")))?;
    if !output.status.success() {
        return Err(unknown(String::from_utf8_lossy(&output.stderr).trim()));
    }
    #[derive(Deserialize)]
    struct Named {
        #[serde(default)]
        name: String,
    }
    #[derive(Deserialize)]
    struct List {
        #[serde(default)]
        contexts: Vec<Named>,
    }
    let list: List = serde_json::from_slice(&output.stdout).map_err(|e| unknown(&format!("unreadable bkt output: {e}")))?;
    if !list.contexts.iter().any(|c| c.name == context) {
        return Err(unknown("it is not in the context list"));
    }
    let mut command = tokio::process::Command::new(&state.bkt);
    command.args(["api", "/user", "--context", context]).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| unauthenticated("bkt timed out"))?
        .map_err(|e| unauthenticated(&format!("cannot run bkt: {e}")))?;
    if !output.status.success() {
        return Err(unauthenticated(String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}
