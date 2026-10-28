//! Column watchers: a script per stage that runs every `watch.every`, whether or not the column
//! has items. The backend only schedules it and hands over the context; what it does (move,
//! delete, create) is the script's business, through `workitem-move` and the workitem files.
//! Costs no tokens. The script is a file run directly, never through a shell.
use std::{collections::HashMap, fs, path::{Path, PathBuf}, process::Stdio, time::{Duration, SystemTime, UNIX_EPOCH}};

use tokio::{io::AsyncWriteExt, process::Command};

use crate::{
    config::Project,
    locate::{self, Found},
    pipeline::{Stage, Watch},
    tmux,
    workitem::{self, Workitem},
    AppState,
};

/// A script that runs longer than this is killed.
const TIMEOUT: Duration = Duration::from_secs(600);

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'))
}

/// The script `name` for a stage on `board`. The first of these wins: the board's
/// `watch/<name>`, the project's `watch/<name>`, the bundled one.
pub fn resolve(state: &AppState, project: &Project, board: &str, name: &str) -> Result<Found, String> {
    let places = locate::places(project, board, "watch", state.skills_dir.join("workitem-pipeline/watch"));
    locate::find(&places, Path::new(name)).ok_or_else(|| format!("watch script '{name}' not found in {}", locate::looked(&places)))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// When each watcher last started (epoch seconds, key `project/board/stage`), kept next to the
/// config so a restart does not run a daily script again.
fn runs_file(state: &AppState) -> PathBuf {
    state.store.sibling("watch.json")
}

fn last_runs(state: &AppState) -> HashMap<String, u64> {
    fs::read_to_string(runs_file(state)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// Makes the watchers of a stage due now, so the next pass runs them.
pub fn forget(state: &AppState, project: &Project, board: &str, stage: &str) {
    let mut runs = last_runs(state);
    let prefix = format!("{}/{board}/{stage}/", project.name);
    runs.retain(|key, _| !key.starts_with(&prefix));
    if let Ok(text) = serde_json::to_string_pretty(&runs) {
        let _ = fs::write(runs_file(state), text);
    }
}

/// Starts every watcher that is due and not still running. Returns at once: a script runs in a
/// task of its own, so a slow one never holds up the dispatcher.
pub async fn pass(state: &AppState) {
    let sessions = tmux::sessions(state.tmux_socket.as_deref()).await;
    let mut runs = last_runs(state);
    let mut started = false;
    for project in state.store.projects() {
        let items = workitem::list(&project);
        for board in state.board_names(&project) {
            let Some(pipeline) = state.pipeline(&project, &board).pipeline else {
                continue;
            };
            for (stage, index, watch) in pipeline.stages.iter().flat_map(|s| s.watch.iter().enumerate().map(move |(i, w)| (s, i, w))) {
                let key = format!("{}/{board}/{}/{index}", project.name, stage.name);
                if runs.get(&key).is_some_and(|last| now().saturating_sub(*last) < watch.every.as_secs()) {
                    continue;
                }
                let script = match resolve(state, &project, &board, &watch.command) {
                    Ok(found) => found.path,
                    Err(e) => {
                        eprintln!("workitem-ui: {key}: {e}");
                        continue;
                    }
                };
                if !state.watching.lock().unwrap().insert(key.clone()) {
                    continue;
                }
                runs.insert(key.clone(), now());
                started = true;
                let here: Vec<&Workitem> = items.iter().filter(|i| i.board == board && i.stage == stage.name).collect();
                let job = Job::new(state, &project, &board, stage, watch, script, &here, &sessions);
                let running = state.watching.clone();
                tokio::spawn(async move {
                    if let Err(e) = job.run().await {
                        eprintln!("workitem-ui: watcher {key}: {e}");
                    }
                    running.lock().unwrap().remove(&key);
                });
            }
        }
    }
    if started {
        if let Ok(text) = serde_json::to_string_pretty(&runs) {
            let _ = fs::write(runs_file(state), text);
        }
    }
}

/// Everything one run needs, owned so it can outlive the pass.
struct Job {
    script: PathBuf,
    args: Vec<String>,
    repo: PathBuf,
    env: Vec<(&'static str, String)>,
    items: String,
}

impl Job {
    #[allow(clippy::too_many_arguments)]
    fn new(state: &AppState, project: &Project, board: &str, stage: &Stage, watch: &Watch, script: PathBuf, items: &[&Workitem], sessions: &[tmux::Session]) -> Job {
        let busy: Vec<&str> = items.iter().filter(|i| tmux::session_for(sessions, project, &i.dir).is_some()).map(|i| i.dir.as_str()).collect();
        let env = vec![
            ("WORKITEM_REPO", project.repo.display().to_string()),
            ("WORKITEM_DIR_ROOT", workitem::workitems_dir(&project.repo).display().to_string()),
            ("WORKITEM_BOARD", board.to_string()),
            ("WORKITEM_STAGE", stage.name.clone()),
            ("WORKITEM_PROJECT", project.name.clone()),
            ("WORKITEM_UI_PORT", state.port.to_string()),
            ("WORKITEM_UI_CONFIG", state.store.path().display().to_string()),
            ("WORKITEM_NEXT", stage.next.join(",")),
            ("WORKITEM_BUSY", busy.join(",")),
            ("WORKITEM_GH", state.gh.clone()),
            ("WORKITEM_ON_MERGE", stage.on_merge.clone().unwrap_or_default()),
            ("WORKITEM_ONCE", if stage.once { "1" } else { "0" }.to_string()),
            ("WORKITEM_WHEN", serde_json::to_string(&stage.when).unwrap_or_else(|_| "{}".into())),
            ("WORKITEM_MOVE", state.skills_dir.join("workitem-pipeline/bin/workitem-move").display().to_string()),
        ];
        Job { script, args: watch.args.clone(), repo: project.repo.clone(), env, items: serde_json::to_string(items).unwrap_or_else(|_| "[]".into()) }
    }

    /// Runs the script in the repo with the context in env vars and the items of the column as a
    /// JSON array on stdin.
    async fn run(self) -> Result<(), String> {
        let mut command = Command::new(&self.script);
        command.args(&self.args).current_dir(&self.repo).envs(self.env.iter().map(|(k, v)| (*k, v))).stdin(Stdio::piped()).kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", self.script.display()))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(self.items.as_bytes()).await;
        }
        let status = tokio::time::timeout(TIMEOUT, child.wait())
            .await
            .map_err(|_| format!("{} ran longer than {}s and was killed", self.script.display(), TIMEOUT.as_secs()))?
            .map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("{} exited with {status}", self.script.display())) }
    }
}
