//! What the board shows of a PR, and the PR tab. Reading the PR is the `pr-status` watch script
//! (`skills/workitem-pipeline/watch/pr-status`), which writes `pr_*` keys into the workitem's
//! frontmatter; reacting to it is `watch/pr-review`. Here the keys become the board's chips.
use std::{collections::HashMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::AppState;

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

#[derive(Deserialize)]
struct Author {
    login: String,
}

/// A comment on the PR itself, from `gh pr view`.
#[derive(Deserialize)]
struct PrComment {
    author: Option<Author>,
    #[serde(default)]
    body: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

/// An inline review comment, from `gh api repos/{owner}/{repo}/pulls/<number>/comments`.
#[derive(Deserialize)]
struct InlineComment {
    user: Option<Author>,
    #[serde(default)]
    body: String,
    created_at: String,
    path: Option<String>,
}

#[derive(Deserialize)]
struct PrReview {
    state: String,
    author: Option<Author>,
    #[serde(default)]
    body: String,
    #[serde(rename = "submittedAt")]
    submitted_at: Option<String>,
}

async fn gh_json<T: serde::de::DeserializeOwned>(state: &AppState, repo: &Path, args: &[&str]) -> Result<T, String> {
    let mut command = tokio::process::Command::new(&state.gh);
    command.args(args).current_dir(repo).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| "gh timed out".to_string())?
        .map_err(|e| format!("cannot run gh: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("unreadable gh output: {e}"))
}

async fn inline_comments(state: &AppState, repo: &Path, pr: &str) -> Result<Vec<InlineComment>, String> {
    let number = pr.rsplit('/').next().unwrap_or_default();
    let path = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments");
    let pages: Vec<Vec<InlineComment>> = gh_json(state, repo, &["api", &path, "--paginate", "--slurp"]).await?;
    Ok(pages.into_iter().flatten().collect())
}

/// One entry of the PR conversation, as the PR tab shows it.
#[derive(Serialize)]
pub struct Note {
    pub kind: &'static str,
    pub author: String,
    pub body: String,
    pub at: String,
    /// Review verdict, for `kind == "review"`.
    pub state: Option<String>,
    /// File of an inline comment.
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct Conversation {
    pub body: String,
    /// Newest first.
    pub notes: Vec<Note>,
}

#[derive(Deserialize)]
struct RawConversation {
    #[serde(default)]
    body: String,
    comments: Option<Vec<PrComment>>,
    reviews: Option<Vec<PrReview>>,
}

/// The PR description plus its comments, review summaries and inline comments, newest first.
pub async fn conversation(state: &AppState, repo: &Path, pr: &str) -> Result<Conversation, String> {
    let raw: RawConversation = gh_json(state, repo, &["pr", "view", pr, "--json", "body,comments,reviews"]).await?;
    let author = |a: Option<Author>| a.map_or_else(|| "ghost".to_string(), |a| a.login);
    let mut notes: Vec<Note> = raw
        .comments
        .into_iter()
        .flatten()
        .map(|c| Note { kind: "comment", author: author(c.author), body: c.body, at: c.created_at, state: None, path: None })
        .collect();
    notes.extend(raw.reviews.into_iter().flatten().filter_map(|r| {
        let verdict = matches!(r.state.as_str(), "APPROVED" | "CHANGES_REQUESTED");
        (verdict || !r.body.is_empty()).then(|| Note {
            kind: "review",
            author: author(r.author),
            body: r.body,
            at: r.submitted_at.unwrap_or_default(),
            state: Some(r.state),
            path: None,
        })
    }));
    notes.extend(inline_comments(state, repo, pr).await?.into_iter().map(|c| Note {
        kind: "inline",
        author: author(c.user),
        body: c.body,
        at: c.created_at,
        state: None,
        path: c.path,
    }));
    // ISO 8601 timestamps sort as text.
    notes.sort_by(|a, b| b.at.cmp(&a.at));
    Ok(Conversation { body: raw.body, notes })
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
    }
}
