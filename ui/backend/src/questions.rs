//! Questions between items (decision 61): an item's agent asks its parent, and the parent's agent
//! answers, through an MCP server the backend serves to every item session at
//! `/api/mcp/<workspace>/<dir>`, so the path says which item calls. The questions live in the
//! item folder as `questions.json`, so they survive both sessions.
//!
//! Only what this needs of MCP's Streamable HTTP transport: JSON-RPC requests answered with one
//! JSON response each (no SSE stream, so `GET` is 405), notifications answered with 202, and
//! `initialize`, `ping`, `tools/list` and `tools/call`.
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    body::Bytes,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{config::Workspace, tmux, workitem, AppState, Shared};

/// The block an asking item gets, so the parent's watcher notices it.
pub const ASKING: &str = "blocked: asking parent: ";

/// A parent answers its children's questions only with this set to `true`, because something
/// must answer them (on the features board, its `children` watcher starts a resolver).
pub const ANSWERS: &str = "answers_children";

/// Name of the MCP server in the agent's config; its tools are `mcp__workitem__*`.
pub const SERVER: &str = "workitem";

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Question {
    pub id: u32,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
    pub asked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Where the answer came from, as the answering agent says (`the human`, a section, the code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<String>,
}

/// Serialises the read-modify-write of every `questions.json`.
static LOCK: Mutex<()> = Mutex::new(());

fn file(workspace: &Workspace, dir: &str) -> PathBuf {
    workspace.dir.join(dir).join("questions.json")
}

pub fn load(workspace: &Workspace, dir: &str) -> Vec<Question> {
    fs::read_to_string(file(workspace, dir)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save(workspace: &Workspace, dir: &str, questions: &[Question]) -> io::Result<()> {
    let path = file(workspace, dir);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(questions)? + "\n")?;
    fs::rename(tmp, path)
}

fn now() -> String {
    let output = std::process::Command::new("date").arg("+%F %T").output();
    output.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// One line for the item's status: the question without line breaks, cut at 100 characters.
fn summary(question: &str) -> String {
    let line = question.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(100) {
        Some((cut, _)) => format!("{}...", &line[..cut]),
        None => line,
    }
}

/// The MCP endpoint of one item. Requests about an item that does not exist are 404 before any
/// JSON-RPC, like the rest of `/api/workitems`.
pub async fn mcp(State(state): State<Shared>, UrlPath((workspace, dir)): UrlPath<(String, String)>, body: Bytes) -> Response {
    let Some(workspace) = state.store.workspace(&workspace) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if workitem::path(&workspace, &dir).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return Json(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } })).into_response();
    };
    let Some(id) = message.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response(); // a notification, e.g. notifications/initialized
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let reply = match message.get("method").and_then(Value::as_str).unwrap_or_default() {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER, "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Questions between this workitem and its parent workitem. A child asks its parent with ask_parent instead of asking in the terminal; a parent answers its children with list_child_questions and answer_child.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, is_error) = match call(&state, &workspace, &dir, name, &args).await {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        other => Err(json!({ "code": -32601, "message": format!("method not found: {other}") })),
    };
    let response = match reply {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    };
    Json(response).into_response()
}

/// No server-to-client stream.
pub async fn mcp_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

fn tools() -> Value {
    let text = |description: &str| json!({ "type": "string", "description": description });
    let id = json!({ "type": "integer", "description": "The question's id." });
    json!([
        {
            "name": "ask_parent",
            "description": "Ask this workitem's parent workitem a question you would otherwise ask the human, and wait for the answer. Refused when the parent does not answer questions: then ask the human as usual. The item is blocked while it waits, which starts an agent on the parent; that agent answers, or asks the human first. Returns the answer, or, after a few minutes without one, the question's id for wait_for_answer.",
            "inputSchema": { "type": "object", "properties": {
                "question": text("The question, complete enough to answer without your session: what you are deciding, the options, what you found."),
                "recommendation": text("Your recommended answer and one line why."),
            }, "required": ["question"] },
        },
        {
            "name": "wait_for_answer",
            "description": "Keep waiting for the answer to a question asked with ask_parent.",
            "inputSchema": { "type": "object", "properties": { "id": id }, "required": ["id"] },
        },
        {
            "name": "list_child_questions",
            "description": "The unanswered questions of this workitem's children (the items whose parent is this one), as JSON.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "answer_child",
            "description": "Answer a question of one of this workitem's children. The child's waiting ask_parent returns the answer, and its block is lifted once it has no unanswered question left.",
            "inputSchema": { "type": "object", "properties": {
                "child": text("The child's workitem dir."),
                "id": id,
                "answer": text("The answer, complete: the child acts on it as on the human's."),
                "source": text("Where the answer comes from: `the human`, or the section, file or code it is settled in."),
            }, "required": ["child", "id", "answer", "source"] },
        },
    ])
}

async fn call(state: &AppState, workspace: &Workspace, dir: &str, tool: &str, args: &Value) -> Result<String, String> {
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let id = || args.get("id").and_then(Value::as_u64).map(|n| n as u32).ok_or("id is required");
    match tool {
        "ask_parent" => {
            let question = text("question").ok_or("question is required")?;
            let id = ask(state, workspace, dir, question, text("recommendation"))?;
            Ok(wait(state, workspace, dir, id).await)
        }
        "wait_for_answer" => {
            let id = id()?;
            load(workspace, dir).iter().find(|q| q.id == id).ok_or(format!("no question {id}"))?;
            Ok(wait(state, workspace, dir, id).await)
        }
        "list_child_questions" => {
            let open: Vec<Value> = children(workspace, dir)
                .into_iter()
                .flat_map(|child| {
                    load(workspace, &child.dir).into_iter().filter(|q| q.answer.is_none()).map(move |q| {
                        json!({ "child": child.dir, "stage": child.stage, "agent_status": child.agent_status, "id": q.id, "question": q.question, "recommendation": q.recommendation, "asked_at": q.asked_at })
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&open).unwrap_or_default())
        }
        "answer_child" => {
            let child = text("child").ok_or("child is required")?;
            let (id, answer, source) = (id()?, text("answer").ok_or("answer is required")?, text("source").ok_or("source is required")?);
            if !children(workspace, dir).iter().any(|c| c.dir == child) {
                return Err(format!("{child} is not a child of {dir}"));
            }
            answer_question(state, workspace, &child, id, answer, source).await?;
            Ok(format!("answered question {id} of {child}"))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

fn children(workspace: &Workspace, dir: &str) -> Vec<workitem::Workitem> {
    workitem::list(workspace).into_iter().filter(|item| item.parent == dir).collect()
}

/// Records the question and blocks the item with it, so the parent's watcher starts the parent.
fn ask(state: &AppState, workspace: &Workspace, dir: &str, question: String, recommendation: Option<String>) -> Result<u32, String> {
    let (item, _) = workitem::read(workspace, dir).ok_or("workitem not found")?;
    if item.parent.is_empty() {
        return Err("this workitem has no parent: ask the human in your session instead".into());
    }
    let answers = workitem::read(workspace, &item.parent).is_some_and(|(_, text)| workitem::frontmatter(&text).get(ANSWERS).is_some_and(|v| v == "true"));
    if !answers {
        return Err(format!("parent {} does not answer questions ({ANSWERS} is not true): ask the human in your session instead", item.parent));
    }
    let path = workitem::path(workspace, dir).ok_or("workitem not found")?;
    let _lock = LOCK.lock().unwrap();
    let mut questions = load(workspace, dir);
    let id = questions.iter().map(|q| q.id).max().unwrap_or(0) + 1;
    let status = format!("{ASKING}{}", summary(&question));
    questions.push(Question { id, question, recommendation, asked_at: now(), answer: None, source: None, answered_at: None });
    save(workspace, dir, &questions).map_err(|e| e.to_string())?;
    workitem::set_fields(&path, &[("agent_status", &status)]).map_err(|e| e.to_string())?;
    state.wake.notify_one();
    Ok(id)
}

/// Waits up to `AppState::ask_wait` for the answer to question `id`.
async fn wait(state: &AppState, workspace: &Workspace, dir: &str, id: u32) -> String {
    let deadline = Instant::now() + state.ask_wait;
    loop {
        if let Some(Question { answer: Some(answer), source, .. }) = load(workspace, dir).into_iter().find(|q| q.id == id) {
            let source = source.unwrap_or_else(|| "the parent".into());
            return format!("Answer to question {id} (from {source}):\n{answer}");
        }
        if Instant::now() >= deadline {
            return format!("No answer to question {id} yet. Call wait_for_answer with id {id} to keep waiting.");
        }
        tokio::time::sleep(Duration::from_millis(200).min(state.ask_wait)).await;
    }
}

/// Writes the answer, and lifts the child's `asking parent` block once nothing is left open:
/// `working` while its session waits for the answer, `ok` (its handler runs again) without one.
pub async fn answer_question(state: &AppState, workspace: &Workspace, dir: &str, id: u32, answer: String, source: String) -> Result<(), String> {
    let path: PathBuf = workitem::path(workspace, dir).ok_or("workitem not found")?;
    let open = {
        let _lock = LOCK.lock().unwrap();
        let mut questions = load(workspace, dir);
        let question = questions.iter_mut().find(|q| q.id == id).ok_or(format!("{dir} has no question {id}"))?;
        if question.answer.is_some() {
            return Err(format!("question {id} of {dir} is already answered"));
        }
        question.answer = Some(answer);
        question.source = Some(source);
        question.answered_at = Some(now());
        save(workspace, dir, &questions).map_err(|e| e.to_string())?;
        questions.iter().any(|q| q.answer.is_none())
    };
    lift_block(state, workspace, dir, &path, open).await;
    state.wake.notify_one();
    Ok(())
}

async fn lift_block(state: &AppState, workspace: &Workspace, dir: &str, path: &Path, open: bool) {
    let asking = workitem::read(workspace, dir).is_some_and(|(item, _)| item.agent_status.starts_with(ASKING));
    if open || !asking {
        return;
    }
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let status = if tmux::session_for(&sessions, workspace, dir).is_some() { "working" } else { "ok" };
    let _ = workitem::set_fields(path, &[("agent_status", status)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarises_a_question_on_one_line() {
        assert_eq!(summary("Create a doc request\nfor  x?"), "Create a doc request for x?");
        let long = "a".repeat(150);
        assert_eq!(summary(&long), format!("{}...", "a".repeat(100)));
    }
}
