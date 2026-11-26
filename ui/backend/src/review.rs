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

/// Bitbucket Cloud, from `bkt pr view --json` and `bkt pr comments --details --json`.
#[derive(Deserialize)]
struct BktView {
    #[serde(default)]
    pull_request: Option<BktPullRequest>,
}

#[derive(Deserialize)]
pub struct BktPullRequest {
    #[serde(default)]
    description: String,
    #[serde(default)]
    participants: Vec<BktParticipant>,
    #[serde(default)]
    updated_on: String,
    #[serde(default)]
    pub source: Option<BktRef>,
    #[serde(default)]
    pub destination: Option<BktRef>,
}

#[derive(Deserialize)]
pub struct BktRef {
    #[serde(default)]
    pub branch: BktBranch,
}

#[derive(Default, Deserialize)]
pub struct BktBranch {
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize)]
struct BktParticipant {
    #[serde(default)]
    user: Option<BktAccount>,
    #[serde(default)]
    approved: Option<bool>,
    /// When the participant last acted (approved); the PR's update time would pin an old
    /// approval to a later push.
    #[serde(default)]
    participated_on: String,
}

#[derive(Deserialize)]
struct BktAccount {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
}

#[derive(Deserialize)]
struct BktComments {
    #[serde(default)]
    comments: Option<Vec<BktComment>>,
}

#[derive(Deserialize)]
struct BktComment {
    #[serde(default)]
    content: BktContent,
    #[serde(default)]
    user: Option<BktAccount>,
    #[serde(default)]
    created_on: String,
    #[serde(default)]
    inline: Option<BktInline>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Default, Deserialize)]
struct BktContent {
    #[serde(default)]
    raw: String,
}

#[derive(Deserialize)]
struct BktInline {
    #[serde(default)]
    path: Option<String>,
}

async fn gh_json<T: serde::de::DeserializeOwned>(state: &AppState, workspace: &Workspace, repo: &Path, args: &[&str]) -> Result<T, String> {
    let mut command = crate::github::command(state, workspace).await?;
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

async fn bkt_json<T: serde::de::DeserializeOwned>(state: &AppState, workspace: &Workspace, repo: &Path, args: &[&str]) -> Result<T, String> {
    let mut args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    if let Some(context) = &workspace.bkt_context {
        args.push("--context".into());
        args.push(context.clone());
    }
    let mut command = tokio::process::Command::new(&state.bkt);
    command.args(&args).current_dir(repo).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| "bkt timed out".to_string())?
        .map_err(|e| format!("cannot run bkt: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("unreadable bkt output: {e}"))
}

/// `bitbucket.org/<ws>/<slug>/pull-requests/<id>`, any suffix after the id ignored (`/diff`,
/// a query). A link on another host is None, so GitHub keeps the `gh` paths.
pub fn bb_parts(pr: &str) -> Option<(String, String, String)> {
    let rest = pr.split_once("//")?.1;
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segments.first()?.split('@').next_back()? != "bitbucket.org" {
        return None;
    }
    let i = segments.iter().position(|s| *s == "pull-requests")?;
    if i < 2 || segments.len() <= i + 1 {
        return None;
    }
    let number = segments[i + 1].split('?').next()?;
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((segments[i - 2].to_string(), segments[i - 1].to_string(), number.to_string()))
}

/// The PR's description, participants and branches, from `bkt pr view --json`.
pub async fn bb_view(state: &AppState, workspace: &Workspace, repo: &Path, pr: &str) -> Result<BktPullRequest, String> {
    let (ws, slug, number) = bb_parts(pr).ok_or_else(|| "not a Bitbucket PR".to_string())?;
    let raw: BktView = bkt_json(state, workspace, repo, &["pr", "view", &number, "--json", "--workspace", &ws, "--repo", &slug]).await?;
    raw.pull_request.ok_or_else(|| "no pull request".to_string())
}

fn bb_user(user: Option<BktAccount>) -> String {
    user.map_or_else(|| "ghost".to_string(), |u| {
        u.display_name.filter(|n| !n.is_empty()).or(u.username).or(u.nickname).unwrap_or_else(|| "ghost".to_string())
    })
}

async fn inline_comments(state: &AppState, workspace: &Workspace, repo: &Path, pr: &str) -> Result<Vec<InlineComment>, String> {
    let number = pr.rsplit('/').next().unwrap_or_default();
    let path = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments");
    let pages: Vec<Vec<InlineComment>> = gh_json(state, workspace, repo, &["api", &path, "--paginate", "--slurp"]).await?;
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
pub async fn conversation(state: &AppState, workspace: &Workspace, repo: &Path, pr: &str) -> Result<Conversation, String> {
    if bb_parts(pr).is_some() {
        return bb_conversation(state, workspace, repo, pr).await;
    }
    let raw: RawConversation = gh_json(state, workspace, repo, &["pr", "view", pr, "--json", "body,comments,reviews"]).await?;
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
    notes.extend(inline_comments(state, workspace, repo, pr).await?.into_iter().map(|c| Note {
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

/// The Bitbucket Cloud variant: `bkt pr view --json` for the description and approvals, `bkt pr
/// comments --details --json` for the comments and inline threads.
async fn bb_conversation(state: &AppState, workspace: &Workspace, repo: &Path, pr: &str) -> Result<Conversation, String> {
    let (ws, slug, number) = bb_parts(pr).ok_or_else(|| "not a Bitbucket PR".to_string())?;
    let raw: BktView = bkt_json(state, workspace, repo, &["pr", "view", &number, "--json", "--workspace", &ws, "--repo", &slug]).await?;
    let Some(bb) = raw.pull_request else {
        return Ok(Conversation { body: String::new(), notes: vec![] });
    };
    let mut notes: Vec<Note> = vec![];
    for p in bb.participants {
        if p.approved == Some(true) {
            let at = if p.participated_on.is_empty() { bb.updated_on.clone() } else { p.participated_on };
            notes.push(Note {
                kind: "review",
                author: bb_user(p.user),
                body: String::new(),
                at,
                state: Some("approved".into()),
                path: None,
            });
        }
    }
    let comments: BktComments = bkt_json(state, workspace, repo, &["pr", "comments", &number, "--details", "--json", "--workspace", &ws, "--repo", &slug]).await?;
    for c in comments.comments.unwrap_or_default() {
        if c.deleted { continue; }
        let (kind, path) = c.inline.as_ref().map_or(("comment", None), |i| ("inline", i.path.clone()));
        notes.push(Note {
            kind,
            author: bb_user(c.user),
            body: c.content.raw,
            at: c.created_on,
            state: None,
            path,
        });
    }
    notes.sort_by(|a, b| b.at.cmp(&a.at));
    Ok(Conversation { body: bb.description, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn bb_parts_parses_bitbucket_links_only() {
        let parts = |pr| bb_parts(pr).map(|(ws, slug, n)| (ws, slug, n));
        assert_eq!(parts("https://bitbucket.org/ws/slug/pull-requests/42"), Some(("ws".into(), "slug".into(), "42".into())));
        assert_eq!(parts("https://bitbucket.org/ws/slug/pull-requests/42/diff"), Some(("ws".into(), "slug".into(), "42".into())));
        assert_eq!(parts("https://bitbucket.org/ws/slug/pull-requests/42?email=x"), Some(("ws".into(), "slug".into(), "42".into())));
        assert_eq!(parts("https://github.com/o/r/pull-requests/42"), None, "another host is not Bitbucket");
        assert_eq!(parts("https://bitbucket.org/ws/slug/branch/nope"), None);
        assert_eq!(parts("not a url"), None);
    }

    #[tokio::test]
    async fn a_bitbucket_pr_conversation_comes_from_bkt() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let bkt = dir.path().join("bkt");
        fs::write(&bkt, format!(
            "#!/bin/sh\n\
             echo \"$@\" >> {}/calls\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
             echo '{{\"workspace\": \"ws\", \"repo\": \"repo\", \"pull_request\": {{\"description\": \"The description\", \"updated_on\": \"2026-01-02\", \"participants\": [{{\"user\": {{\"display_name\": \"Me\"}}, \"approved\": true, \"participated_on\": \"2026-01-01\"}}]}}}}'\n\
             else\n\
             echo '{{\"comments\": [{{\"id\": 1, \"content\": {{\"raw\": \"A comment\"}}, \"user\": {{\"display_name\": \"You\"}}, \"created_on\": \"2026-01-02\"}}]}}'\n\
             fi\n",
            dir.path().display()
        )).unwrap();
        fs::set_permissions(&bkt, fs::Permissions::from_mode(0o755)).unwrap();
        let store = crate::config::Store::open(dir.path().to_path_buf(), None).unwrap();
        let mut state = AppState::new(store, 0);
        state.bkt = bkt.display().to_string();
        let workspace = state.store.add_workspace("w").unwrap();
        let conversation = conversation(&state, &workspace, &repo, "https://bitbucket.org/ws/repo/pull-requests/42").await.unwrap();
        assert_eq!(conversation.body, "The description");
        assert_eq!(conversation.notes.len(), 2);
        assert_eq!(conversation.notes[0].body, "A comment");
        assert_eq!(conversation.notes[0].kind, "comment");
        assert_eq!(conversation.notes[1].state, Some("approved".into()));
        assert_eq!(conversation.notes[1].at, "2026-01-01", "the approval keeps its own time");
        let calls = fs::read_to_string(dir.path().join("calls")).unwrap();
        assert_eq!(calls.lines().count(), 2, "pr view and pr comments run");
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
