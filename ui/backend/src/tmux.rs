use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use tokio::process::Command;

use crate::config::Project;

pub struct Session {
    pub name: String,
    /// The repo the session belongs to: its `@workitem_repo` option, else its start directory
    /// (sessions started before the option existed). A session may run in a worktree instead.
    pub repo: PathBuf,
}

/// Session option `launch::spawn` sets to the project's repo.
pub const REPO_OPTION: &str = "@workitem_repo";

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
        .args(["list-sessions", "-F", &format!("#{{session_name}}\t#{{session_path}}\t#{{{REPO_OPTION}}}")])
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
            let mut fields = line.splitn(3, '\t');
            let (name, path, repo) = (fields.next()?, fields.next()?, fields.next().unwrap_or(""));
            Some(Session { name: name.to_string(), repo: if repo.is_empty() { path } else { repo }.into() })
        })
        .collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The `wi-<dir>` session of this project's workitem. A same-named session of another repo
/// is not a match.
pub fn session_for<'a>(sessions: &'a [Session], project: &Project, dir: &str) -> Option<&'a Session> {
    named(sessions, project, &format!("wi-{dir}"))
}

/// A session of this project by exact name.
pub fn named<'a>(sessions: &'a [Session], project: &Project, name: &str) -> Option<&'a Session> {
    sessions.iter().find(|s| s.name == name && same_dir(&s.repo, &project.repo))
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
