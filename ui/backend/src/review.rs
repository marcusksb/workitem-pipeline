//! What the board shows of a PR, and the PR tab. Reading the PR is the `pr-status` watch script
//! (`skills/workitem-pipeline/watch/pr-status`), which writes `pr_*` keys into the workitem's
//! frontmatter; reacting to it is `watch/pr-review`. Here the keys become the board's chips.
use std::{collections::HashMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{config::Workspace, AppState};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    Approved,
    ChangesRequested,
    ReviewRequired,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Merge {
    Mergeable,
    Blocked,
    Conflicting,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ci {
    Passing,
    Failing,
    Pending,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrStatus {
    pub state: State,
    pub approval: Approval,
    /// The reader's latest verdict on the PR is an approval.
    pub approved_by_me: bool,
    pub merge: Merge,
    /// Every check, as GitHub shows it.
    pub ci: Ci,
    /// Names of the red checks.
    pub failed: Vec<String>,
}

impl PrStatus {
    /// The status `pr-status` last wrote into the frontmatter; `None` before its first reading.
    /// Values it does not know read as the neutral one.
    pub fn from_fields(fields: &HashMap<String, String>) -> Option<PrStatus> {
        let get = |key: &str| fields.get(key).map(String::as_str).unwrap_or_default();
        let state = match get("pr_state") {
            "open" => State::Open,
            "merged" => State::Merged,
            "closed" => State::Closed,
            _ => return None,
        };
        Some(PrStatus {
            state,
            approval: match get("pr_approval") {
                "approved" => Approval::Approved,
                "changes_requested" => Approval::ChangesRequested,
                "review_required" => Approval::ReviewRequired,
                _ => Approval::None,
            },
            approved_by_me: get("pr_approved_by_me") == "true",
            merge: match get("pr_merge") {
                "mergeable" => Merge::Mergeable,
                "blocked" => Merge::Blocked,
                "conflicting" => Merge::Conflicting,
                _ => Merge::Unknown,
            },
            ci: match get("pr_ci") {
                "passing" => Ci::Passing,
                "failing" => Ci::Failing,
                "pending" => Ci::Pending,
                _ => Ci::None,
            },
            failed: get("pr_ci_failed").split(',').filter(|n| !n.is_empty()).map(String::from).collect(),
        })
    }
}

/// One entry of the PR conversation, as the PR tab shows it.
#[derive(Serialize, Deserialize)]
pub struct Note {
    pub kind: String,
    pub author: String,
    pub body: String,
    pub at: String,
    /// Review verdict, for `kind == "review"`.
    pub state: Option<String>,
    /// File of an inline comment.
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Conversation {
    pub body: String,
    /// Newest first.
    pub notes: Vec<Note>,
}

/// Runs `workitem-pr <verb> <item> <args>` (decision 83), the one way the backend reaches a PR
/// host: the workspace's GitHub token as `GH_TOKEN` (decision 67), its bkt context as
/// `WORKITEM_BKT_CONTEXT` (82) and the backend's `gh` and `bkt` binaries. Its stdout, or its
/// message.
pub async fn workitem_pr(state: &AppState, workspace: &Workspace, item: &Path, verb: &str, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let script = state.skills_dir.join("workitem-pipeline/bin/workitem-pr");
    let mut command = tokio::process::Command::new("python3");
    command.arg(script).arg(verb).arg(item).args(args).current_dir(&workspace.dir).env("WORKITEM_GH", &state.gh).env("WORKITEM_BKT", &state.bkt).kill_on_drop(true);
    if let Some(token) = crate::github::token(state, workspace).await? {
        command.env("GH_TOKEN", token);
    }
    match &workspace.bkt_context {
        Some(context) => command.env("WORKITEM_BKT_CONTEXT", context),
        None => command.env_remove("WORKITEM_BKT_CONTEXT"),
    };
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| format!("workitem-pr {verb} timed out after {}s", timeout.as_secs()))?
        .map_err(|e| format!("cannot run workitem-pr: {e}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if message.is_empty() { format!("workitem-pr {verb} exited with {}", output.status) } else { message });
    }
    Ok(output.stdout)
}

/// The PR description plus its comments, review summaries and inline comments, newest first.
/// The limit covers several host calls of 30 s each (the token, the PR, the comment pages).
pub async fn conversation(state: &AppState, workspace: &Workspace, item: &Path) -> Result<Conversation, String> {
    let out = workitem_pr(state, workspace, item, "conversation", &["--json"], Duration::from_secs(150)).await?;
    serde_json::from_slice(&out).map_err(|e| format!("unreadable workitem-pr output: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn status_comes_from_the_keys_pr_status_wrote() {
        assert_eq!(PrStatus::from_fields(&fields(&[("pr_ci", "failing")])), None, "no reading yet");
        let open = PrStatus::from_fields(&fields(&[
            ("pr_state", "open"),
            ("pr_approval", "changes_requested"),
            ("pr_approved_by_me", "true"),
            ("pr_merge", "conflicting"),
            ("pr_ci", "failing"),
            ("pr_ci_failed", "lint,rspec"),
            ("pr_head", "abc"),
        ]))
        .unwrap();
        assert_eq!(
            open,
            PrStatus {
                state: State::Open,
                approval: Approval::ChangesRequested,
                approved_by_me: true,
                merge: Merge::Conflicting,
                ci: Ci::Failing,
                failed: vec!["lint".into(), "rspec".into()],
            }
        );
        let merged = PrStatus::from_fields(&fields(&[("pr_state", "merged"), ("pr_ci", "odd")])).unwrap();
        assert_eq!((merged.state, merged.ci, merged.merge, merged.failed.len()), (State::Merged, Ci::None, Merge::Unknown, 0));
        let blocked = PrStatus::from_fields(&fields(&[("pr_state", "open"), ("pr_merge", "blocked")])).unwrap();
        assert_eq!(blocked.merge, Merge::Blocked);
    }
}
