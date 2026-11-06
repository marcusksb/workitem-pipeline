//! Planning sessions (decision 36): a `claude` conversation about a project with no workitem and
//! no worktree, in tmux session `wi-plan-<id>`. Its record `<workitems dir>/plans/<id>.md` is the
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
    config::Project,
    launch, tmux,
    workitem::{self, frontmatter},
    AppState, Shared,
};

type Result<T> = std::result::Result<T, ApiError>;

const PREFIX: &str = "wi-plan-";
const TOPIC_MAX: usize = 160;

#[derive(Serialize)]
pub struct Plan {
    pub project: String,
    pub id: String,
    pub topic: String,
    pub summary: String,
    /// `working`, `waiting` or `ended`.
    pub status: String,
    /// Directory name of the workitem made from this session, or empty.
    pub workitem: String,
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

fn folder(repo: &Path) -> PathBuf {
    workitem::workitems_dir(repo).join("plans")
}

/// The record of plan `id`, if it exists.
fn record(project: &Project, id: &str) -> Option<PathBuf> {
    let path = folder(&project.repo).join(format!("{id}.md"));
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

fn parse(project: &Project, id: &str, text: &str, live: bool) -> Plan {
    let fields = frontmatter(text);
    let field = |key: &str| fields.get(key).cloned().unwrap_or_default();
    Plan {
        project: project.name.clone(),
        id: id.to_string(),
        topic: field("topic"),
        summary: field("summary"),
        status: if fields.get("status").is_some_and(|s| !s.is_empty()) { field("status") } else { "ended".into() },
        workitem: field("workitem"),
        created: field("created").parse().unwrap_or(0),
        has_plan: section(text, "Plan").is_some_and(|plan| !plan.is_empty()),
        live,
    }
}

/// The project's plans, newest first.
fn list(project: &Project, live: &HashSet<String>) -> Vec<Plan> {
    let Ok(entries) = fs::read_dir(folder(&project.repo)) else {
        return vec![];
    };
    let mut plans: Vec<Plan> = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let id = name.strip_suffix(".md").filter(|id| valid_id(id))?;
            let text = fs::read_to_string(entry.path()).ok()?;
            Some(parse(project, id, &text, live.contains(&session_name(id))))
        })
        .collect();
    plans.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
    plans
}

/// Live `wi-plan-*` session names of this project.
fn live_sessions(sessions: &[tmux::Session], dead: &HashSet<String>, project: &Project) -> HashSet<String> {
    sessions
        .iter()
        .filter(|s| is_session(&s.name) && !dead.contains(&s.name) && tmux::named(sessions, project, &s.name).is_some())
        .map(|s| s.name.clone())
        .collect()
}

/// Dispatcher housekeeping: a plan whose session exited or is gone is `ended`, and an exited
/// session is killed. The caller holds `AppState::busy`, which `create` and `resume` also hold
/// between writing `working` and starting the session.
pub async fn reap(state: &AppState, project: &Project, sessions: &[tmux::Session], dead: &HashSet<String>) {
    let live = live_sessions(sessions, dead, project);
    for plan in list(project, &live) {
        let name = session_name(&plan.id);
        if tmux::named(sessions, project, &name).is_some() && dead.contains(&name) {
            tmux::kill(state.tmux_socket.as_deref(), &name).await;
        }
        if !plan.live && plan.status != "ended" {
            if let Some(path) = record(project, &plan.id) {
                let _ = workitem::set_fields(&path, &[("status", "ended")]);
            }
        }
    }
}

fn project_of(state: &AppState, name: &str) -> Result<Project> {
    state.store.project(name).ok_or(ApiError::not_found("project"))
}

fn internal(message: impl ToString) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
}

fn conflict(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, message.into())
}

/// Plans of every configured project, newest first per project.
pub async fn list_plans(State(state): State<Shared>) -> Json<Vec<Plan>> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    let plans = state
        .store
        .projects()
        .iter()
        .flat_map(|project| list(project, &live_sessions(&sessions, &dead, project)))
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

pub async fn plan(State(state): State<Shared>, UrlPath((project, id)): UrlPath<(String, String)>) -> Result<Json<Detail>> {
    let project = project_of(&state, &project)?;
    let path = record(&project, &id).ok_or(ApiError::not_found("plan"))?;
    let text = fs::read_to_string(&path).map_err(internal)?;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    let live = live_sessions(&sessions, &dead, &project);
    Ok(Json(Detail {
        plan: parse(&project, &id, &text, live.contains(&session_name(&id))),
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
fn prompt(state: &AppState, project: &Project, session: &str, input: &str) -> String {
    let note = state.skills_dir.join("workitem-pipeline/bin/workitem-plan-note");
    format!(
        "This is a planning session in repo {repo}, started from the Workload web UI. A human wants to think something through \
with you: explore the code, answer questions, compare options and work towards a plan. Ask when something is unclear.\n\
You do not change code here. Edits outside {workitems} are refused; prefer read-only shell commands, others ask the human first.\n\
Keep the record of this session current, always with exactly these commands:\n\
- Right after reading the request, and after each meaningful step: python3 {note} \"<one line: what this is about and where it stands>\"\n\
- Only when the human wants a plan saved: python3 {note} --plan <<'PLAN' ... PLAN (the whole plan as markdown on stdin).\n\
- Only when the human asks for a workitem: use the workitem-create skill with --repo {repo}, then python3 {note} --workitem <dir>.\n\
The status (working, waiting) is kept by hooks; do not set it. The tmux session is {session}.\n\
The request, as typed in the web UI:\n\n{input}\n",
        repo = project.repo.display(),
        workitems = workitem::workitems_dir(&project.repo).display(),
        note = note.display(),
    )
}

pub async fn create(
    State(state): State<Shared>,
    UrlPath(project): UrlPath<String>,
    Json(body): Json<NewPlan>,
) -> Result<(StatusCode, Json<Started>)> {
    let project = project_of(&state, &project)?;
    let input = body.input.trim();
    if input.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "describe what you want to plan".into()));
    }
    let id = hex(&rand::rng().random::<[u8; 3]>());
    let session = session_name(&id);
    let claude_session = uuid();
    let folder = folder(&project.repo);
    let path = folder.join(format!("{id}.md"));
    let text = format!(
        "---\ntopic: {topic}\nsummary: \nstatus: working\nclaude_session: {claude_session}\ncreated: {created}\n---\n\n## Prompt\n\n{input}\n",
        topic = topic(input),
        created = now(),
    );
    let prompt = prompt(&state, &project, &session, input);
    let _busy = state.busy.lock().await;
    if path.exists() {
        return Err(conflict(format!("plan {id} already exists, try again")));
    }
    fs::create_dir_all(&folder).map_err(internal)?;
    fs::write(&path, text).map_err(internal)?;
    if let Err(e) = launch::spawn_plan(&state, &session, &project, &path, &["--session-id", &claude_session], Some(&prompt)).await {
        let _ = fs::remove_file(&path);
        return Err(conflict(e));
    }
    Ok((StatusCode::ACCEPTED, Json(Started { id, session })))
}

/// Continues an ended plan's Claude conversation in a new `wi-plan-<id>` session.
pub async fn resume(State(state): State<Shared>, UrlPath((project, id)): UrlPath<(String, String)>) -> Result<(StatusCode, Json<Started>)> {
    let project = project_of(&state, &project)?;
    let path = record(&project, &id).ok_or(ApiError::not_found("plan"))?;
    let text = fs::read_to_string(&path).map_err(internal)?;
    let claude_session = frontmatter(&text).get("claude_session").cloned().unwrap_or_default();
    if claude_session.is_empty() {
        return Err(conflict("the plan has no claude_session to resume"));
    }
    let session = session_name(&id);
    let _busy = state.busy.lock().await;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    if tmux::named(&sessions, &project, &session).is_some() {
        return Err(conflict(format!("{session} is still running")));
    }
    workitem::set_fields(&path, &[("status", "working")]).map_err(internal)?;
    if let Err(e) = launch::spawn_plan(&state, &session, &project, &path, &["--resume", &claude_session], None).await {
        let _ = workitem::set_fields(&path, &[("status", "ended")]);
        return Err(conflict(e));
    }
    Ok((StatusCode::ACCEPTED, Json(Started { id, session })))
}

/// Kills the plan's session, if any, and deletes its record.
pub async fn delete(State(state): State<Shared>, UrlPath((project, id)): UrlPath<(String, String)>) -> Result<StatusCode> {
    let project = project_of(&state, &project)?;
    let path = record(&project, &id).ok_or(ApiError::not_found("plan"))?;
    let session = session_name(&id);
    let _busy = state.busy.lock().await;
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    if tmux::named(&sessions, &project, &session).is_some() {
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
