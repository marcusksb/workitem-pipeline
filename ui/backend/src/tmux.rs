use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use tokio::process::Command;

use crate::config::Workspace;

pub struct Session {
    pub name: String,
    /// The folder of the workspace the session belongs to: its `@workitem_workspace` option,
    /// empty for sessions the UI did not start.
    pub workspace: PathBuf,
}

/// Session option `launch::start` sets to the workspace folder.
pub const WORKSPACE_OPTION: &str = "@workitem_workspace";

pub fn command(socket: Option<&str>) -> Command {
    let mut command = Command::new("tmux");
    if let Some(socket) = socket {
        command.args(["-L", socket]);
    }
    command
}

/// All tmux sessions; empty when no server is running.
pub async fn sessions(socket: Option<&str>) -> Vec<Session> {
    let output = command(socket)
        .args(["list-sessions", "-F", &format!("#{{session_name}}\t#{{{WORKSPACE_OPTION}}}")])
        .output()
        .await;
    let Ok(output) = output else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (name, workspace) = line.split_once('\t').unwrap_or((line, ""));
            Some(Session { name: name.to_string(), workspace: workspace.into() })
        })
        .collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The `wi-<dir>` session of this workspace's workitem. A same-named session of another
/// workspace is not a match.
pub fn session_for<'a>(sessions: &'a [Session], workspace: &Workspace, dir: &str) -> Option<&'a Session> {
    named(sessions, workspace, &format!("wi-{dir}"))
}

/// A session of this workspace by exact name.
pub fn named<'a>(sessions: &'a [Session], workspace: &Workspace, name: &str) -> Option<&'a Session> {
    sessions.iter().find(|s| s.name == name && !s.workspace.as_os_str().is_empty() && same_dir(&s.workspace, &workspace.dir))
}

/// Sessions the UI starts that are not tied to one workitem: `wi-new-<id>`.
pub fn is_agent_session(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("wi-new-") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Names of sessions whose pane has exited (`remain-on-exit` keeps them around).
pub async fn dead_sessions(socket: Option<&str>) -> HashSet<String> {
    let output = command(socket)
        .args(["list-panes", "-a", "-F", "#{session_name}\t#{pane_dead}"])
        .output()
        .await;
    let Ok(output) = output else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, dead)| *dead == "1")
        .map(|(name, _)| name.to_string())
        .collect()
}

pub async fn kill(socket: Option<&str>, name: &str) {
    let _ = command(socket).args(["kill-session", "-t", &format!("={name}")]).output().await;
}
