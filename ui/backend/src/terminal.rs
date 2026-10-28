use std::io::{Read, Write};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::Response,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{api::ApiError, config::Project, tmux, workitem, AppState, Shared};

#[derive(Deserialize)]
pub struct Size {
    cols: Option<u16>,
    rows: Option<u16>,
}

/// Text frames from the browser. Binary frames are keystrokes.
#[derive(Deserialize)]
#[serde(untagged)]
enum Control {
    /// `{"cols":80,"rows":24}`
    Resize { cols: u16, rows: u16 },
    /// `{"scroll":12}`: show the pane 12 lines above the bottom of its history (0 = live).
    Scroll { scroll: u32 },
}

/// tmux history position, sent to the browser as a text frame so it can draw a scrollbar.
#[derive(Serialize, PartialEq, Clone, Copy)]
struct Scrollback {
    history: u32,
    position: u32,
}

async fn scrollback(socket: Option<&str>, session: &str) -> Option<Scrollback> {
    let output = tmux::command(socket)
        .args(["display-message", "-p", "-t", &format!("={session}:"), "#{history_size} #{?pane_in_mode,#{scroll_position},0}"])
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (history, position) = text.trim().split_once(' ')?;
    Some(Scrollback { history: history.parse().ok()?, position: position.parse().ok()? })
}

async fn scroll_to(socket: Option<&str>, session: &str, line: u32) {
    let target = format!("={session}:");
    if line == 0 {
        let _ = tmux::command(socket).args(["send-keys", "-t", &target, "-X", "cancel"]).output().await;
        return;
    }
    // One tmux process, not two. copy-mode -e leaves copy mode by itself at the bottom.
    let _ = tmux::command(socket)
        .args(["copy-mode", "-e", "-t", &target, ";", "send-keys", "-t", &target, "-X", "goto-line", &line.to_string()])
        .output()
        .await;
}

struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: mpsc::UnboundedReceiver<Vec<u8>>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

/// Runs `tmux attach-session` on a PTY. Killing this client detaches; the session lives on.
fn spawn(socket: Option<&str>, session: &str, cols: u16, rows: u16) -> Result<Pty, String> {
    let pair = native_pty_system().openpty(size(cols, rows)).map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new("tmux");
    if let Some(socket) = socket {
        command.args(["-L", socket]);
    }
    command.args(["attach-session", "-t", &format!("={session}")]);
    command.env("TERM", "xterm-256color");
    let child = pair.slave.spawn_command(command).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let (sender, output) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 || sender.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Ok(Pty { master: pair.master, child, writer, output })
}

/// Browser terminal for a workitem's `wi-<dir>` session.
pub async fn attach_item(
    upgrade: WebSocketUpgrade,
    State(state): State<Shared>,
    Path((project, dir)): Path<(String, String)>,
    Query(requested): Query<Size>,
) -> Result<Response, ApiError> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    workitem::read(&project, &dir).ok_or(ApiError::not_found("workitem"))?;
    attach(upgrade, &state, &project, &format!("wi-{dir}"), requested).await
}

/// Browser terminal for a session without a workitem: `wi-new-<id>`.
pub async fn attach_agent(
    upgrade: WebSocketUpgrade,
    State(state): State<Shared>,
    Path((project, name)): Path<(String, String)>,
    Query(requested): Query<Size>,
) -> Result<Response, ApiError> {
    let project = state.store.project(&project).ok_or(ApiError::not_found("project"))?;
    if !tmux::is_agent_session(&name) {
        return Err(ApiError::not_found("session"));
    }
    attach(upgrade, &state, &project, &name, requested).await
}

async fn attach(
    upgrade: WebSocketUpgrade,
    state: &AppState,
    project: &Project,
    name: &str,
    requested: Size,
) -> Result<Response, ApiError> {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let session = tmux::named(&sessions, project, name).ok_or(ApiError::not_found("session"))?;
    let pty = spawn(
        state.tmux_socket.as_deref(),
        &session.name,
        requested.cols.unwrap_or(80),
        requested.rows.unwrap_or(24),
    )
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let socket = state.tmux_socket.clone();
    let name = session.name.clone();
    Ok(upgrade.on_upgrade(move |ws| bridge(ws, pty, socket, name)))
}

async fn bridge(mut socket: WebSocket, mut pty: Pty, tmux_socket: Option<String>, session: String) {
    let mut poll = tokio::time::interval(std::time::Duration::from_millis(250));
    let mut sent = None;
    // Scrolling spawns tmux processes. A worker takes only the latest target, so a burst of
    // wheel events costs a few runs and never stalls the pty output below.
    let (scroll_tx, mut scroll_rx) = tokio::sync::watch::channel(None::<u32>);
    let worker = {
        let (tmux_socket, session) = (tmux_socket.clone(), session.clone());
        tokio::spawn(async move {
            while scroll_rx.changed().await.is_ok() {
                let target = *scroll_rx.borrow_and_update();
                if let Some(line) = target {
                    scroll_to(tmux_socket.as_deref(), &session, line).await;
                }
            }
        })
    };
    loop {
        tokio::select! {
            output = pty.output.recv() => match output {
                Some(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Binary(bytes))) => {
                    if pty.writer.write_all(&bytes).is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<Control>(text.as_str()) {
                    Ok(Control::Resize { cols, rows }) => {
                        let _ = pty.master.resize(size(cols, rows));
                    }
                    Ok(Control::Scroll { scroll }) => {
                        let _ = scroll_tx.send(Some(scroll));
                    }
                    Err(_) => {}
                },
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = poll.tick() => {
                let current = scrollback(tmux_socket.as_deref(), &session).await;
                if current.is_some() && current != sent {
                    sent = current;
                    let frame = serde_json::to_string(&current).unwrap_or_default();
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                }
            },
        }
    }
    worker.abort();
    let _ = pty.child.kill();
    let _ = pty.child.wait();
}
