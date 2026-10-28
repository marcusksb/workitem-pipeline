//! What the board shows of a PR, and the PR tab. The watching itself (reading the PR, reacting to
//! it) is the `pr-review` watch script (`skills/workitem-pipeline/watch/pr-review`); it pushes the
//! status below through `PUT /api/workitems/{project}/{dir}/review`.
use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    Approved,
    ChangesRequested,
    ReviewRequired,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Merge {
    Mergeable,
    Conflicting,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ci {
    Passing,
    Failing,
    Pending,
    None,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FailedCheck {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PrStatus {
    pub approval: Approval,
    /// The reader's latest verdict on the PR is an approval.
    pub approved_by_me: bool,
    pub merge: Merge,
    /// Every check, as GitHub shows it.
    pub ci: Ci,
    pub failed: Vec<FailedCheck>,
    /// Commit the PR head points to.
    #[serde(default, skip_serializing)]
    pub head: Option<String>,
}

/// The status the `pr-review` script last pushed for an item.
pub struct Review {
    pub status: PrStatus,
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
