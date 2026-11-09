//! Planning sessions (decisions 36, 49): a `claude` conversation about a workspace with no workitem
//! and no worktree, in tmux session `wi-plan-<id>`. Its record `<workspace>/plans/<id>.md` is the
//! source of truth (topic, summary, status, Claude session id, optional plan); tmux only tells
//! whether it is live. The agent updates the record through `workitem-plan-note` and the hooks.
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Path as UrlPath, State},
    http::StatusCode,
    Json,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    api::ApiError,
    config::Workspace,
    launch, tmux,
    workitem::{self, frontmatter},
    AppState, Shared,
};

type Result<T> = std::result::Result<T, ApiError>;

const PREFIX: &str = "wi-plan-";
const TOPIC_MAX: usize = 160;

#[derive(Serialize)]
pub struct Plan {
    pub workspace: String,
    pub id: String,
    pub topic: String,
    pub summary: String,
    /// `working`, `waiting` or `ended`.
    pub status: String,
    /// Directory names of the workitems made from this session.
    pub workitems: Vec<String>,
    /// Epoch seconds.
    pub created: u64,
    /// A `## Plan` was saved.
    pub has_plan: bool,
    /// Its `wi-plan-<id>` session is running.
    pub live: bool,
}

pub fn session_name(id: &str) -> String {
    format!("{PREFIX}{id}")
}

pub fn is_session(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(valid_id)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn folder(workspace: &Path) -> PathBuf {
    workspace.join("plans")
}

/// The record of plan `id`, if it exists.
fn record(workspace: &Workspace, id: &str) -> Option<PathBuf> {
    let path = folder(&workspace.dir).join(format!("{id}.md"));
    (valid_id(id) && path.is_file()).then_some(path)
}

/// The text between `## <name>` and the next `## ` heading, trimmed.
fn section(text: &str, name: &str) -> Option<String> {
    let heading = format!("## {name}");
    let mut lines = text.lines().skip_while(|line| line.trim_end() != heading);
    lines.next()?;
    let body = lines.take_while(|line| !line.starts_with("## ")).collect::<Vec<_>>().join("\n");
    Some(body.trim().to_string())
}

fn parse(workspace: &Workspace, id: &str, text: &str, live: bool) -> Plan {
    let fields = frontmatter(text);
    let field = |key: &str| fields.get(key).cloned().unwrap_or_default();
    Plan {
        workspace: workspace.name.clone(),
        id: id.to_string(),
        topic: field("topic"),
        summary: field("summary"),
        status: if fields.get("status").is_some_and(|s| !s.is_empty()) { field("status") } else { "ended".into() },
        // `workitem` is the single item of records written before plans could make several.
        workitems: [field("workitems"), field("workitem")].join(",").split(',').map(str::trim).filter(|d| !d.is_empty()).map(String::from).collect(),
        created: field("created").parse().unwrap_or(0),
        has_plan: section(text, "Plan").is_some_and(|plan| !plan.is_empty()),
        live,
    }
}

/// The workspace's plans, newest first.
fn list(workspace: &Workspace, live: &HashSet<String>) -> Vec<Plan> {
    let Ok(entries) = fs::read_dir(folder(&workspace.dir)) else {
        return vec![];
    };
    let mut plans: Vec<Plan> = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let id = name.strip_suffix(".md").filter(|id| valid_id(id))?;
            let text = fs::read_to_string(entry.path()).ok()?;
            Some(parse(workspace, id, &text, live.contains(&session_name(id))))
        })
        .collect();
    plans.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
    plans
}

/// Live `wi-plan-*` session names of this workspace.
fn live_sessions(sessions: &[tmux::Session], dead: &HashSet<String>, workspace: &Workspace) -> HashSet<String> {
    sessions
        .iter()
        .filter(|s| is_session(&s.name) && !dead.contains(&s.name) && tmux::named(sessions, workspace, &s.name).is_some())
        .map(|s| s.name.clone())
        .collect()
}

/// Dispatcher housekeeping: a plan whose session exited or is gone is `ended`, and an exited
/// session is killed. The caller holds `AppState::busy`, which `create` and `resume` also hold
/// between writing `working` and starting the session.
pub async fn reap(state: &AppState, workspace: &Workspace, sessions: &[tmux::Session], dead: &HashSet<String>) {
    let live = live_sessions(sessions, dead, workspace);
    for plan in list(workspace, &live) {
        let name = session_name(&plan.id);
        if tmux::named(sessions, workspace, &name).is_some() && dead.contains(&name) {
            tmux::kill(state.tmux_socket.as_deref(), &name).await;
        }
        if !plan.live && plan.status != "ended" {
            if let Some(path) = record(workspace, &plan.id) {
                let _ = workitem::set_fields(&path, &[("status", "ended")]);
            }
        }
    }
}

fn workspace_of(state: &AppState, name: &str) -> Result<Workspace> {
    state.store.workspace(name).ok_or(ApiError::not_found("workspace"))
}

fn internal(message: impl ToString) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
}

fn conflict(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, message.into())
}

/// Plans of every configured workspace, newest first per workspace.
pub async fn list_plans(State(state): State<Shared>) -> Json<Vec<Plan>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    let plans = state
        .store
        .workspaces()
        .iter()
        .flat_map(|workspace| list(workspace, &live_sessions(&sessions, &dead, workspace)))
        .collect();
    Json(plans)
}

#[derive(Serialize)]
pub struct Detail {
    #[serde(flatten)]
    plan: Plan,
    /// The text typed when the session was started.
    prompt: String,
    /// The saved `## Plan`, if any.
    text: Option<String>,
}

pub async fn plan(State(state): State<Shared>, UrlPath((workspace, id)): UrlPath<(String, String)>) -> Result<Json<Detail>> {
    let workspace = workspace_of(&state, &workspace)?;
    let path = record(&workspace, &id).ok_or(ApiError::not_found("plan"))?;
    let text = fs::read_to_string(&path).map_err(internal)?;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    let live = live_sessions(&sessions, &dead, &workspace);
    Ok(Json(Detail {
        plan: parse(&workspace, &id, &text, live.contains(&session_name(&id))),
        prompt: section(&text, "Prompt").unwrap_or_default(),
        text: section(&text, "Plan").filter(|plan| !plan.is_empty()),
    }))
}

#[derive(Deserialize)]
pub struct NewPlan {
    input: String,
}

#[derive(Serialize)]
pub struct Started {
    id: String,
    session: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A random (version 4) UUID, which `claude --session-id` requires.
fn uuid() -> String {
    let mut bytes: [u8; 16] = rand::rng().random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = hex(&bytes);
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

fn topic(input: &str) -> String {
    let line = input.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    match line.char_indices().nth(TOPIC_MAX - 1) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_string(),
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The first prompt: the planning rules, then what the human typed.
fn prompt(state: &AppState, workspace: &Workspace, session: &str, input: &str) -> String {
    let note = state.skills_dir.join("workitem-pipeline/bin/workitem-plan-note");
    format!(
        "This is a planning session in workspace {workspace}, started from the Workload web UI. A human wants to think something through \
with you: explore the code, answer questions, compare options and work towards a plan. Ask when something is unclear.\n\
The workspace has these boards, each with its repo; all of the repos are added to this session:\n{boards}\
You do not change code here. Edits outside {workspace} are refused; prefer read-only shell commands, others ask the human first.\n\
Keep the record of this session current, always with exactly these commands:\n\
- Right after reading the request, and after each meaningful step: python3 {note} \"<one line: what this is about and where it stands>\"\n\
- Only when the human wants a plan saved: python3 {note} --plan <<'PLAN' ... PLAN (the whole plan as markdown on stdin).\n\
- Only when the human asks for workitems: use the workitem-create skill with --workspace {workspace} --board <board>, once per board \
the work touches (one item per repo, linked with depends_on as that skill describes), then python3 {note} --workitem <dir> for each item.\n\
The status (working, waiting) is kept by hooks; do not set it. The tmux session is {session}.\n\
The request, as typed in the web UI:\n\n{input}\n",
        workspace = workspace.dir.display(),
        boards = launch::board_list(state, workspace),
        note = note.display(),
    )
}

pub async fn create(
    State(state): State<Shared>,
    UrlPath(workspace): UrlPath<String>,
    Json(body): Json<NewPlan>,
) -> Result<(StatusCode, Json<Started>)> {
    let workspace = workspace_of(&state, &workspace)?;
    let input = body.input.trim();
    if input.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "describe what you want to plan".into()));
    }
    let id = hex(&rand::rng().random::<[u8; 3]>());
    let session = session_name(&id);
    let claude_session = uuid();
    let folder = folder(&workspace.dir);
    let path = folder.join(format!("{id}.md"));
    let text = format!(
        "---\ntopic: {topic}\nsummary: \nstatus: working\nclaude_session: {claude_session}\ncreated: {created}\n---\n\n## Prompt\n\n{input}\n",
        topic = topic(input),
        created = now(),
    );
    let prompt = prompt(&state, &workspace, &session, input);
    let _busy = state.busy.lock().await;
    if path.exists() {
        return Err(conflict(format!("plan {id} already exists, try again")));
    }
    fs::create_dir_all(&folder).map_err(internal)?;
    fs::write(&path, text).map_err(internal)?;
    if let Err(e) = launch::spawn_plan(&state, &session, &workspace, &path, &["--session-id", &claude_session], Some(&prompt)).await {
        let _ = fs::remove_file(&path);
        return Err(conflict(e));
    }
    Ok((StatusCode::ACCEPTED, Json(Started { id, session })))
}

/// Continues an ended plan's Claude conversation in a new `wi-plan-<id>` session.
pub async fn resume(State(state): State<Shared>, UrlPath((workspace, id)): UrlPath<(String, String)>) -> Result<(StatusCode, Json<Started>)> {
    let workspace = workspace_of(&state, &workspace)?;
    let path = record(&workspace, &id).ok_or(ApiError::not_found("plan"))?;
    let text = fs::read_to_string(&path).map_err(internal)?;
    let claude_session = frontmatter(&text).get("claude_session").cloned().unwrap_or_default();
    if claude_session.is_empty() {
        return Err(conflict("the plan has no claude_session to resume"));
    }
    let session = session_name(&id);
    let _busy = state.busy.lock().await;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    if tmux::named(&sessions, &workspace, &session).is_some() {
        return Err(conflict(format!("{session} is still running")));
    }
    workitem::set_fields(&path, &[("status", "working")]).map_err(internal)?;
    if let Err(e) = launch::spawn_plan(&state, &session, &workspace, &path, &["--resume", &claude_session], None).await {
        let _ = workitem::set_fields(&path, &[("status", "ended")]);
        return Err(conflict(e));
    }
    Ok((StatusCode::ACCEPTED, Json(Started { id, session })))
}

/// Kills the plan's session, if any, and deletes its record.
pub async fn delete(State(state): State<Shared>, UrlPath((workspace, id)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let workspace = workspace_of(&state, &workspace)?;
    let path = record(&workspace, &id).ok_or(ApiError::not_found("plan"))?;
    let session = session_name(&id);
    let _busy = state.busy.lock().await;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    if tmux::named(&sessions, &workspace, &session).is_some() {
        tmux::kill(state.tmux_socket.as_deref(), &session).await;
    }
    fs::remove_file(&path).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_sessions() {
        assert!(is_session("wi-plan-0a1b2c"));
        assert!(!is_session("wi-plan-"));
        assert!(!is_session("wi-plan-../x"));
        assert!(!is_session("wi-new-0a1b2c"));
    }

    #[test]
    fn uuid_is_version_4() {
        let id = uuid();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn topic_is_the_first_line_capped() {
        assert_eq!(topic("\n  How to split auth?\nmore"), "How to split auth?");
        assert_eq!(topic(&"ä".repeat(200)).chars().count(), TOPIC_MAX);
    }

    #[test]
    fn sections_end_at_the_next_heading() {
        let text = "---\na: b\n---\n\n## Prompt\n\nx\ny\n\n## Plan\n\n1. z\n";
        assert_eq!(section(text, "Prompt").as_deref(), Some("x\ny"));
        assert_eq!(section(text, "Plan").as_deref(), Some("1. z"));
        assert_eq!(section(text, "Other"), None);
    }
}
