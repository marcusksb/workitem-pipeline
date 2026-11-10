//! The Chat tab (decisions 57 to 59): an agent session read from its Claude Code transcript and
//! answered through tmux. The JSONL format is known here only; the browser gets small entries.
//! Reading needs no tmux client, so it never resizes the session's window the way an attached
//! terminal does.
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use axum::{
    extract::{Path as UrlPath, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::{api::ApiError, config::Workspace, plans, tmux, workitem, AppState, Shared};

/// Lines of tool output sent to the browser.
const OUTPUT_LINES: usize = 50;
/// Characters of tool output sent to the browser, for output with few, long lines.
const OUTPUT_CHARS: usize = 8000;
/// Characters of a tool call's one-line summary.
const SUMMARY_CHARS: usize = 200;
/// Lines from the bottom of the pane shown in the pending box.
const SCREEN_LINES: usize = 25;
/// Longest message accepted from the reply box.
const MESSAGE_BYTES: usize = 32 * 1024;

#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entry {
    /// The launcher's prompt that opens the session; shown collapsed.
    Prompt { text: String },
    User { text: String },
    Assistant { text: String },
    /// A tool call; its `Result` carries the same `id` and may come in a later poll.
    Tool { id: String, name: String, summary: String },
    Result { id: String, output: String, error: bool, more: usize },
}

#[derive(Serialize)]
pub struct Chat {
    /// The transcript read; empty while the item has none.
    transcript: String,
    /// Byte offset to pass as `after` next time, with this `transcript` as `from`.
    next: u64,
    /// The entries start at the top of the transcript, so the browser drops what it had.
    reset: bool,
    entries: Vec<Entry>,
    /// The tmux session is running, so the reply box and keys work.
    session: bool,
    /// The bottom of the pane as plain text, while the session is running.
    screen: Option<String>,
}

#[derive(Deserialize)]
pub struct Cursor {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    from: String,
}

/// What a chat reads and writes: the session's transcript and its tmux session name.
struct Target {
    transcript: String,
    session: String,
}

fn item_target(state: &AppState, workspace: &str, dir: &str) -> Result<(Workspace, Target), ApiError> {
    let workspace = state.store.workspace(workspace).ok_or(ApiError::not_found("workspace"))?;
    let (_, text) = workitem::read(&workspace, dir).ok_or(ApiError::not_found("workitem"))?;
    let transcript = workitem::frontmatter(&text).remove("transcript").unwrap_or_default();
    Ok((workspace, Target { transcript, session: format!("wi-{dir}") }))
}

fn plan_target(state: &AppState, workspace: &str, id: &str) -> Result<(Workspace, Target), ApiError> {
    let workspace = state.store.workspace(workspace).ok_or(ApiError::not_found("workspace"))?;
    let transcript = plans::transcript(&workspace, id).ok_or(ApiError::not_found("plan"))?;
    Ok((workspace, Target { transcript, session: plans::session_name(id) }))
}

pub async fn item_chat(State(state): State<Shared>, UrlPath((workspace, dir)): UrlPath<(String, String)>, Query(cursor): Query<Cursor>) -> Result<Json<Chat>, ApiError> {
    let (workspace, target) = item_target(&state, &workspace, &dir)?;
    read_chat(&state, &workspace, target, cursor).await
}

pub async fn plan_chat(State(state): State<Shared>, UrlPath((workspace, id)): UrlPath<(String, String)>, Query(cursor): Query<Cursor>) -> Result<Json<Chat>, ApiError> {
    let (workspace, target) = plan_target(&state, &workspace, &id)?;
    read_chat(&state, &workspace, target, cursor).await
}

#[derive(Deserialize)]
pub struct Message {
    text: String,
}

pub async fn item_send(State(state): State<Shared>, UrlPath((workspace, dir)): UrlPath<(String, String)>, Json(body): Json<Message>) -> Result<StatusCode, ApiError> {
    let (workspace, target) = item_target(&state, &workspace, &dir)?;
    send(&state, &workspace, &target.session, &body.text).await
}

pub async fn plan_send(State(state): State<Shared>, UrlPath((workspace, id)): UrlPath<(String, String)>, Json(body): Json<Message>) -> Result<StatusCode, ApiError> {
    let (workspace, target) = plan_target(&state, &workspace, &id)?;
    send(&state, &workspace, &target.session, &body.text).await
}

#[derive(Deserialize)]
pub struct Key {
    key: String,
}

pub async fn item_key(State(state): State<Shared>, UrlPath((workspace, dir)): UrlPath<(String, String)>, Json(body): Json<Key>) -> Result<StatusCode, ApiError> {
    let (workspace, target) = item_target(&state, &workspace, &dir)?;
    press(&state, &workspace, &target.session, &body.key).await
}

pub async fn plan_key(State(state): State<Shared>, UrlPath((workspace, id)): UrlPath<(String, String)>, Json(body): Json<Key>) -> Result<StatusCode, ApiError> {
    let (workspace, target) = plan_target(&state, &workspace, &id)?;
    press(&state, &workspace, &target.session, &body.key).await
}

/// The running session of this workspace named `name`, or 409.
async fn live_session(state: &AppState, workspace: &Workspace, name: &str) -> Result<String, ApiError> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let dead = tmux::dead_sessions(state.tmux_socket.as_deref()).await;
    match tmux::named(&sessions, workspace, name) {
        Some(session) if !dead.contains(&session.name) => Ok(session.name.clone()),
        _ => Err(ApiError(StatusCode::CONFLICT, "the session has ended".into())),
    }
}

async fn read_chat(state: &AppState, workspace: &Workspace, target: Target, cursor: Cursor) -> Result<Json<Chat>, ApiError> {
    let screen = match live_session(state, workspace, &target.session).await {
        Ok(session) => Some(screen(state.tmux_socket.as_deref(), &session).await),
        Err(_) => None,
    };
    let after = if cursor.from == target.transcript { cursor.after } else { 0 };
    let (entries, next, reset) = if transcript_path_ok(&target.transcript) {
        let path = target.transcript.clone();
        tokio::task::spawn_blocking(move || read_from(Path::new(&path), after))
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        (vec![], 0, true)
    };
    Ok(Json(Chat { transcript: target.transcript, next, reset, entries, session: screen.is_some(), screen }))
}

/// A transcript is a `.jsonl` file named by an absolute path. Anything else in the field is ignored,
/// so the backend never reads some other file an agent wrote there.
fn transcript_path_ok(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute() && path.extension().is_some_and(|e| e == "jsonl") && path.is_file()
}

/// Entries of the complete lines after byte `after`, the offset after them, and whether they start
/// at the top (a shorter file than `after` was rewritten, so it is read again from the start).
fn read_from(path: &Path, after: u64) -> (Vec<Entry>, u64, bool) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return (vec![], 0, true);
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = if after <= len { after } else { 0 };
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return (vec![], start, start == 0);
    }
    // A line still being written is left for the next poll.
    let complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let mut entries: Vec<Entry> = bytes[..complete].split(|&b| b == b'\n').filter_map(|line| serde_json::from_slice::<Value>(line).ok()).flat_map(|line| entries(&line)).collect();
    if start == 0 {
        // Every session the UI starts opens with the launcher's prompt (handler skill and stage
        // block, or the planning rules), which the browser shows collapsed. Until it is written,
        // the cursor stays at the top so it is never taken for a reply.
        match entries.iter_mut().find(|e| matches!(e, Entry::User { .. })) {
            Some(first) => {
                if let Entry::User { text } = first {
                    *first = Entry::Prompt { text: std::mem::take(text) };
                }
            }
            None => return (entries, 0, true),
        }
    }
    (entries, start + complete as u64, start == 0)
}

/// The entries of one transcript line. Only the main conversation counts: meta lines (skill bodies,
/// command caveats), subagent lines and bookkeeping lines are left out.
fn entries(line: &Value) -> Vec<Entry> {
    let kind = line["type"].as_str().unwrap_or("");
    if !matches!(kind, "user" | "assistant") || line["isMeta"] == true || line["isSidechain"] == true {
        return vec![];
    }
    let content = &line["message"]["content"];
    if let Some(text) = content.as_str() {
        return user_text(text).map(|text| Entry::User { text }).into_iter().collect();
    }
    let Some(blocks) = content.as_array() else {
        return vec![];
    };
    blocks
        .iter()
        .filter_map(|block| match block["type"].as_str()? {
            "text" => {
                let text = block["text"].as_str()?.trim().to_string();
                if text.is_empty() {
                    None
                } else if kind == "user" {
                    user_text(&text).map(|text| Entry::User { text })
                } else {
                    Some(Entry::Assistant { text })
                }
            }
            "tool_use" => Some(Entry::Tool {
                id: block["id"].as_str()?.into(),
                name: block["name"].as_str().unwrap_or("tool").into(),
                summary: summary(&block["input"]),
            }),
            "tool_result" => {
                let (output, more) = cut(&result_text(&block["content"]));
                Some(Entry::Result { id: block["tool_use_id"].as_str()?.into(), output, error: block["is_error"] == true, more })
            }
            _ => None,
        })
        .collect()
}

/// What a user line shows: typed text as it is, a slash command as `/name args`, and nothing for
/// the command's echoed output.
fn user_text(text: &str) -> Option<String> {
    let tag = |name: &str| {
        let open = format!("<{name}>");
        let start = text.find(&open)? + open.len();
        let end = text[start..].find(&format!("</{name}>"))? + start;
        Some(text[start..end].trim().to_string())
    };
    if let Some(command) = tag("command-name") {
        let args = tag("command-args").unwrap_or_default();
        return Some(if args.is_empty() { command } else { format!("{command} {args}") });
    }
    if text.starts_with("<local-command-") {
        return None;
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// One line saying what a tool call does: its command, file, pattern or description.
fn summary(input: &Value) -> String {
    let text = ["command", "file_path", "pattern", "description", "url", "query", "prompt", "path"]
        .iter()
        .find_map(|key| input[key].as_str())
        .map(String::from)
        .or_else(|| input.as_object()?.values().find_map(|v| v.as_str().map(String::from)))
        .unwrap_or_default();
    let line = text.lines().next().unwrap_or("").trim();
    match line.char_indices().nth(SUMMARY_CHARS) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None if text.trim().lines().count() > 1 => format!("{line} …"),
        None => line.to_string(),
    }
}

/// A tool result's text: a string, or the text blocks of a list (images are left out).
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The first `OUTPUT_LINES` lines (at most `OUTPUT_CHARS` characters) and how many lines were left out.
fn cut(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = lines[..lines.len().min(OUTPUT_LINES)].join("\n");
    if let Some((i, _)) = kept.char_indices().nth(OUTPUT_CHARS) {
        kept.truncate(i);
        kept.push('…');
    }
    (kept, lines.len().saturating_sub(OUTPUT_LINES))
}

/// The bottom of the pane as plain text, with wrapped lines joined and trailing blank lines dropped.
async fn screen(socket: Option<&str>, session: &str) -> String {
    let output = tmux::command(socket).args(["capture-pane", "-p", "-J", "-t", &format!("={session}:")]).output().await;
    let text = output.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
    lines[end.saturating_sub(SCREEN_LINES)..end].join("\n")
}

/// Types `text` into the session as one bracketed paste, so its newlines do not submit early, then
/// presses Enter.
async fn send(state: &AppState, workspace: &Workspace, name: &str, text: &str) -> Result<StatusCode, ApiError> {
    let text = text.trim_end();
    if text.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "the message is empty".into()));
    }
    if text.len() > MESSAGE_BYTES {
        return Err(ApiError(StatusCode::BAD_REQUEST, format!("the message is longer than {} KB", MESSAGE_BYTES / 1024)));
    }
    let session = live_session(state, workspace, name).await?;
    let socket = state.tmux_socket.as_deref();
    let target = format!("={session}:");
    let buffer = format!("chat-{session}");
    let failed = |what: &str, e: String| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("tmux {what} failed: {e}"));
    let mut load = tmux::command(socket).args(["load-buffer", "-b", &buffer, "-"]).stdin(std::process::Stdio::piped()).spawn().map_err(|e| failed("load-buffer", e.to_string()))?;
    let mut stdin = load.stdin.take().expect("piped stdin");
    stdin.write_all(text.as_bytes()).await.map_err(|e| failed("load-buffer", e.to_string()))?;
    drop(stdin);
    run(load.wait_with_output().await, "load-buffer")?;
    run(tmux::command(socket).args(["paste-buffer", "-p", "-d", "-b", &buffer, "-t", &target]).output().await, "paste-buffer")?;
    // Claude Code takes an Enter that arrives with the paste as part of it.
    tokio::time::sleep(Duration::from_millis(150)).await;
    run(tmux::command(socket).args(["send-keys", "-t", &target, "Enter"]).output().await, "send-keys")?;
    Ok(StatusCode::NO_CONTENT)
}

/// Keys the pending box can press; anything else is refused, so no raw input reaches tmux.
const KEYS: &[&str] = &["1", "2", "3", "Enter", "Escape", "Up", "Down"];

async fn press(state: &AppState, workspace: &Workspace, name: &str, key: &str) -> Result<StatusCode, ApiError> {
    if !KEYS.contains(&key) {
        return Err(ApiError(StatusCode::BAD_REQUEST, format!("key '{key}' is not one of {}", KEYS.join(", "))));
    }
    let session = live_session(state, workspace, name).await?;
    run(tmux::command(state.tmux_socket.as_deref()).args(["send-keys", "-t", &format!("={session}:"), key]).output().await, "send-keys")?;
    Ok(StatusCode::NO_CONTENT)
}

fn run(output: std::io::Result<std::process::Output>, what: &str) -> Result<(), ApiError> {
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("tmux {what} failed: {}", String::from_utf8_lossy(&o.stderr).trim()))),
        Err(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("tmux {what} failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user(content: Value) -> Value {
        json!({ "type": "user", "message": { "role": "user", "content": content } })
    }

    #[test]
    fn text_tool_calls_and_results() {
        let assistant = json!({ "type": "assistant", "message": { "content": [
            { "type": "thinking", "thinking": "hm" },
            { "type": "text", "text": "Running tests." },
            { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "cargo test\n--quiet", "description": "Run" } },
        ] } });
        assert_eq!(
            entries(&assistant),
            vec![
                Entry::Assistant { text: "Running tests.".into() },
                Entry::Tool { id: "t1".into(), name: "Bash".into(), summary: "cargo test …".into() },
            ]
        );
        let result = user(json!([{ "type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": [{ "type": "text", "text": "boom" }] }]));
        assert_eq!(entries(&result), vec![Entry::Result { id: "t1".into(), output: "boom".into(), error: true, more: 0 }]);
    }

    #[test]
    fn user_lines() {
        assert_eq!(entries(&user(json!("fix it"))), vec![Entry::User { text: "fix it".into() }]);
        let command = "<command-message>grill-me</command-message>\n<command-name>/grill-me</command-name>\n<command-args>a plan</command-args>";
        assert_eq!(entries(&user(json!(command))), vec![Entry::User { text: "/grill-me a plan".into() }]);
        assert!(entries(&user(json!("<local-command-stdout></local-command-stdout>"))).is_empty());
        let mut meta = user(json!("skill body"));
        meta["isMeta"] = json!(true);
        assert!(entries(&meta).is_empty());
        let mut sidechain = user(json!("subagent prompt"));
        sidechain["isSidechain"] = json!(true);
        assert!(entries(&sidechain).is_empty());
        assert!(entries(&json!({ "type": "attachment" })).is_empty());
    }

    #[test]
    fn long_output_is_cut() {
        let text = (0..60).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let (kept, more) = cut(&text);
        assert_eq!((kept.lines().count(), more), (50, 10));
    }

    #[test]
    fn reads_complete_lines_after_the_cursor_and_marks_the_launch_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        // Before the launch prompt is written, the cursor stays at the top.
        std::fs::write(&path, "").unwrap();
        assert_eq!(read_from(&path, 0), (vec![], 0, true));
        let head = format!("{}\n{}\n", user(json!("handler skill and stage block")), user(json!("one")));
        std::fs::write(&path, format!("{head}{}", user(json!("two")))).unwrap();
        let (entries, next, reset) = read_from(&path, 0);
        let expected = vec![Entry::Prompt { text: "handler skill and stage block".into() }, Entry::User { text: "one".into() }];
        assert_eq!((entries, next, reset), (expected, head.len() as u64, true));
        std::fs::write(&path, format!("{head}{}\n", user(json!("two")))).unwrap();
        let (entries, _, reset) = read_from(&path, next);
        assert_eq!((entries, reset), (vec![Entry::User { text: "two".into() }], false));
        // A file shorter than the cursor was replaced: read again from the top.
        std::fs::write(&path, "\n").unwrap();
        assert!(read_from(&path, next).2);
    }

    #[test]
    fn only_absolute_jsonl_files() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("secret.txt");
        std::fs::write(&other, "x").unwrap();
        assert!(!transcript_path_ok(other.to_str().unwrap()));
        assert!(!transcript_path_ok("t.jsonl"));
        let jsonl = dir.path().join("t.jsonl");
        std::fs::write(&jsonl, "").unwrap();
        assert!(transcript_path_ok(jsonl.to_str().unwrap()));
    }
}
