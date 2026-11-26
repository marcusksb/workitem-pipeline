use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde_json::json;

use crate::{
    config::{self, Agent, AgentKind, Workspace},
    git,
    locate::{self, Found},
    pipeline::{valid_board_name, Stage},
    tmux,
    workitem,
    AppState,
};

/// What a session works on: one workitem, in its board's repo, or the whole workspace (create
/// and planning sessions).
pub enum Scope<'a> {
    Item(&'a Place<'a>),
    Workspace,
}

/// Where a workitem lives: its folder in the workspace and the repo of its board, if it has one.
pub struct Place<'a> {
    pub dir: &'a str,
    pub workspace: &'a Path,
    pub repo: Option<&'a Path>,
    pub board: &'a str,
}

impl Place<'_> {
    fn file(&self) -> PathBuf {
        workitem::file(self.workspace, self.dir)
    }

    /// Where the item works, for prompts: `in repo <path>`, or a note that its board has none.
    fn in_repo(&self) -> String {
        match self.repo {
            Some(repo) => format!("in repo {}", repo.display()),
            None => "on a board without a repo (no worktree, no code changes; the workspace's repos are added for reading)".into(),
        }
    }
}

/// Starts the workspace's agent (decisions 52, 56) with a prompt in a detached tmux session:
/// `remain-on-exit` keeps a crashed session visible, and the pipeline hooks (or the kind's glue)
/// mark a session that waits for input as blocked. The prompt goes to the agent as one argv
/// element (see `start`), never interpreted by a shell. A
/// workitem's session starts in its worktree when it has one, else in the repo, with the workspace
/// folder added so the agent can edit `workitem.md` (decision 46); on a board without a repo it
/// starts in the item folder with the workspace and every board's repo added. A workspace session
/// starts in the workspace folder with every board's repo added.
pub async fn spawn(state: &AppState, session: &str, workspace: &Workspace, scope: Scope<'_>, prompt: &str) -> Result<(), String> {
    let mut env = vec![format!("WORKITEM_WORKSPACE={}", workspace.dir.display())];
    let mut dirs: Vec<PathBuf> = vec![];
    let mut model = None;
    let cwd = match scope {
        Scope::Item(place) => {
            model = stage_model(state, workspace, place);
            env.push(format!("WORKITEM={}", place.dir));
            env.push(format!("WORKITEM_FILE={}", place.file().display()));
            dirs.push(workspace.dir.clone());
            match place.repo {
                Some(repo) => {
                    let worktree = git::worktree_for(&git::worktrees(repo).await, repo, place.dir).cloned();
                    env.push(format!("WORKITEM_REPO={}", repo.display()));
                    if worktree.is_some() {
                        dirs.push(repo.to_path_buf());
                    }
                    worktree.unwrap_or_else(|| repo.to_path_buf())
                }
                None => {
                    env.push(CLAUDE_MD_OF_ADDED_DIRS.into());
                    dirs.extend(state.repos(workspace));
                    workspace.dir.join(place.dir)
                }
            }
        }
        Scope::Workspace => {
            env.push(format!("WORKITEM_CREATE={session}"));
            env.push(CLAUDE_MD_OF_ADDED_DIRS.into());
            dirs.extend(state.repos(workspace));
            workspace.dir.clone()
        }
    };
    let agent = state.store.agent_for(workspace);
    let program = program(state, &agent)?;
    let (claude_settings, mcp): (OsString, _) = match &scope {
        Scope::Item(place) => (state.skills_dir.join("workitem-pipeline/hooks.json").into(), Some(Mcp::new(state, workspace, place.dir))),
        Scope::Workspace => (create_settings(state).into(), None),
    };
    let launch = Launch { dirs: &dirs, claude_settings, model: model.as_deref(), mcp: mcp.as_ref(), plan: false };
    let mut args = agent_args(state, agent.kind, &launch, &mut env)?;
    if agent.kind == AgentKind::Opencode {
        // The prompt follows as the flag's value.
        args.push("--prompt".into());
    }
    start(state, session, workspace, &cwd, &env, &program, &args, Some(prompt)).await
}

/// What a session gets besides the prompt, in each kind's own flags (decision 56).
struct Launch<'a> {
    /// Folders the agent may reach besides its start folder (46, 49, 54).
    dirs: &'a [PathBuf],
    /// Claude's `--settings` (hooks, permissions); the other kinds get the same from their glue.
    claude_settings: OsString,
    /// The stage's model (decision 60), passed through unchanged.
    model: Option<&'a str>,
    /// The item's `workitem` MCP server (decision 61); item sessions only.
    mcp: Option<&'a Mcp>,
    /// A planning session: opencode asks before shell commands, as Claude does there.
    plan: bool,
}

/// Maps a session's folders, settings, model and MCP server to `kind`'s arguments, before the
/// conversation flags and the prompt; opencode takes its part as `OPENCODE_CONFIG_CONTENT` in `env`.
fn agent_args(state: &AppState, kind: AgentKind, launch: &Launch, env: &mut Vec<String>) -> Result<Vec<OsString>, String> {
    let mut args: Vec<OsString> = vec![];
    match kind {
        AgentKind::Claude => {
            args.extend(add_dirs(launch.dirs));
            if let Some(mcp) = launch.mcp {
                args.extend(["--mcp-config".into(), mcp.claude_config(state)?.into()]);
            }
            args.extend(["--settings".into(), launch.claude_settings.clone()]);
        }
        AgentKind::Opencode => env.push(format!("OPENCODE_CONFIG_CONTENT={}", opencode_config(state, launch)?)),
        AgentKind::Pi | AgentKind::Omp => {
            // pi has no permission prompts, so it reaches the folders without being told.
            if kind == AgentKind::Omp {
                args.extend(launch.dirs.iter().map(|d| prefixed("--add-dir=", d)));
            }
            args.extend(["-e".into(), glue(state, kind).into()]);
            // Neither reads MCP servers from a flag; the glue offers the server's tools instead.
            if let Some(mcp) = launch.mcp {
                env.push(format!("WORKITEM_MCP={}", mcp.claude_config(state)?.display()));
            }
        }
    }
    if let Some(model) = launch.model {
        args.extend(model_args(kind, model));
    }
    Ok(args)
}

/// The stage's model (decision 60) in `kind`'s flag. opencode wants `provider/model` and refuses
/// a Claude alias like `opus`, so it gets none and uses its own default.
fn model_args(kind: AgentKind, model: &str) -> Vec<OsString> {
    match kind {
        AgentKind::Opencode if !model.contains('/') => vec![],
        AgentKind::Opencode => vec!["-m".into(), model.into()],
        AgentKind::Omp => vec![format!("--model={model}").into()],
        AgentKind::Claude | AgentKind::Pi => vec!["--model".into(), model.into()],
    }
}

/// The plugin or extension of `kind` that reports waiting and runs the planning guards.
fn glue(state: &AppState, kind: AgentKind) -> PathBuf {
    state.skills_dir.join("workitem-pipeline/agents").join(kind.name()).join("workitem-notify.ts")
}

/// Config that opencode merges over the user's own: the glue plugin, the item's MCP server, and
/// access outside the session's folder to exactly the session's folders (what Claude gets with
/// `--add-dir`) and the pipeline's own scripts. Everything else keeps the user's setting, so a
/// prompt for it shows as blocked. The env reaches tmux in argv, so the MCP token is only named:
/// opencode reads it from its 0600 file (`{file:...}`).
fn opencode_config(state: &AppState, launch: &Launch) -> Result<String, String> {
    let pipeline = state.skills_dir.join("workitem-pipeline");
    let plugin = pipeline.join("agents/opencode/workitem-notify.ts");
    let mut allowed: Vec<PathBuf> = launch.dirs.to_vec();
    allowed.push(pipeline.clone());
    // The skills folder holds symlinks (`install.sh`); a tool may see either path.
    if let Ok(real) = pipeline.canonicalize() {
        allowed.push(real);
    }
    let rules: serde_json::Map<String, serde_json::Value> = allowed.iter().map(|dir| (format!("{}/**", dir.display()), json!("allow"))).collect();
    let mut config = json!({
        "plugin": [format!("file://{}", plugin.display().to_string().replace('%', "%25").replace(' ', "%20").replace('#', "%23").replace('?', "%3F"))],
        "permission": { "external_directory": rules },
    });
    if launch.plan {
        // The guard covers edit tools only, so a shell command asks first, as in Claude's
        // planning sessions; the record's own helper runs without asking.
        let note = state.skills_dir.join("workitem-pipeline/bin/workitem-plan-note");
        config["permission"]["bash"] = json!({ "*": "ask", format!("python3 {}*", note.display()): "allow" });
    }
    if let Some(mcp) = launch.mcp {
        let token = mcp.token_file(state)?;
        config["mcp"] = json!({ crate::questions::SERVER: {
            "type": "remote", "url": mcp.url, "oauth": false,
            "headers": { "Authorization": format!("Bearer {{file:{}}}", token.display()) },
        } });
    }
    Ok(config.to_string())
}

fn prefixed(flag: &str, path: &Path) -> OsString {
    let mut arg = OsString::from(flag);
    arg.push(path);
    arg
}

/// The agent's executable, resolved here because tmux's `PATH` may differ from ours: its command,
/// else the kind's name, else for Claude `state.claude`.
fn program(state: &AppState, agent: &Agent) -> Result<OsString, String> {
    let name = match (&agent.command, agent.kind) {
        (Some(command), _) => command.as_str(),
        (None, AgentKind::Claude) => return Ok(state.claude.clone().into()),
        (None, kind) => kind.name(),
    };
    config::resolve_executable(name).map(OsString::from).ok_or_else(|| format!("agent '{name}' not found"))
}

/// The `model` of the stage the item is in now (decision 60), so every session of the item,
/// handler, fixer or one started by hand, runs on it.
fn stage_model(state: &AppState, workspace: &Workspace, place: &Place) -> Option<String> {
    let (item, _) = workitem::read(workspace, place.dir)?;
    state.pipeline(workspace, place.board).pipeline?.stage(&item.stage)?.model.clone()
}

/// The backend's `workitem` MCP server of one item (decision 61). Its token goes to the agent only
/// through files in `<root>/mcp/` with mode 0600, because argv and the tmux env are visible to
/// every local user; they are rewritten on each launch.
struct Mcp {
    url: String,
    /// `<workspace>--<dir>`, the name of its files.
    name: String,
}

impl Mcp {
    fn new(state: &AppState, workspace: &Workspace, dir: &str) -> Mcp {
        Mcp { url: format!("http://127.0.0.1:{}/api/mcp/{}/{dir}", state.port, workspace.name), name: format!("{}--{dir}", workspace.name) }
    }

    /// Claude's `--mcp-config` file, which the pi and omp glue read too.
    fn claude_config(&self, state: &AppState) -> Result<PathBuf, String> {
        let config = json!({ "mcpServers": { crate::questions::SERVER: {
            "type": "http", "url": self.url, "headers": { "Authorization": format!("Bearer {}", state.store.token()) },
        } } });
        write_mcp_file(state, &format!("{}.json", self.name), &config.to_string())
    }

    /// The bare token, for opencode's `{file:...}`.
    fn token_file(&self, state: &AppState) -> Result<PathBuf, String> {
        write_mcp_file(state, &format!("{}.token", self.name), &state.store.token())
    }
}

fn write_mcp_file(state: &AppState, name: &str, text: &str) -> Result<PathBuf, String> {
    use std::{io::Write, os::unix::fs::{DirBuilderExt, OpenOptionsExt}};
    let folder = state.store.sibling("mcp");
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(name);
    let _ = fs::remove_file(&path);
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// A workspace, plan or repo-less item session starts outside every repo, so Claude Code would not read the repos'
/// `CLAUDE.md` files; this makes it read them from the `--add-dir` folders too.
const CLAUDE_MD_OF_ADDED_DIRS: &str = "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1";

/// `--add-dir` with every folder, or nothing. It takes several values, so `--settings` must
/// follow it to end the list.
fn add_dirs(dirs: &[PathBuf]) -> Vec<OsString> {
    if dirs.is_empty() {
        return vec![];
    }
    std::iter::once("--add-dir".into()).chain(dirs.iter().map(|d| d.clone().into_os_string())).collect()
}

/// The agent conversation of a planning session: a new one, with the id the launcher picked for
/// kinds that take one (Claude, pi), or the recorded one to resume.
pub enum Conversation<'a> {
    New(Option<&'a str>),
    Resume(&'a str),
}

/// Whether `kind` takes the id of a new conversation from the launcher. The others pick it
/// themselves, and their glue records it with `workitem-plan-note --agent-session`.
pub fn presets_session(kind: AgentKind) -> bool {
    matches!(kind, AgentKind::Claude | AgentKind::Pi)
}

/// Starts a planning session (decisions 36, 49) in the workspace folder with every board's repo
/// added, with `agent`, then `prompt` if any. `record` is its plan record, which the hooks
/// (Claude) or the kind's glue and `workitem-plan-note` update; both guard edits outside the
/// workspace folder.
pub async fn spawn_plan(state: &AppState, session: &str, workspace: &Workspace, record: &Path, agent: &Agent, conversation: Conversation<'_>, prompt: Option<&str>) -> Result<(), String> {
    let mut env = vec![format!("WORKITEM_PLAN={}", record.display()), format!("WORKITEM_WORKSPACE={}", workspace.dir.display()), CLAUDE_MD_OF_ADDED_DIRS.into()];
    let program = program(state, agent)?;
    let dirs = state.repos(workspace);
    let launch = Launch { dirs: &dirs, claude_settings: plan_settings(state).into(), model: None, mcp: None, plan: true };
    let mut args = agent_args(state, agent.kind, &launch, &mut env)?;
    let flags: Vec<OsString> = match (agent.kind, conversation) {
        (AgentKind::Claude, Conversation::New(Some(id))) => vec!["--session-id".into(), id.into()],
        (AgentKind::Claude, Conversation::Resume(id)) => vec!["--resume".into(), id.into()],
        (AgentKind::Opencode, Conversation::Resume(id)) => vec!["--session".into(), id.into()],
        // pi opens the conversation with that id, or creates it.
        (AgentKind::Pi, Conversation::New(Some(id)) | Conversation::Resume(id)) => vec!["--session-id".into(), id.into()],
        (AgentKind::Omp, Conversation::Resume(file)) => vec![prefixed("--resume=", Path::new(file))],
        _ => vec![],
    };
    args.extend(flags);
    if agent.kind == AgentKind::Opencode && prompt.is_some() {
        args.push("--prompt".into());
    }
    start(state, session, workspace, &workspace.dir, &env, &program, &args, prompt).await
}

/// Inline `--settings` of a planning session: the helper and file edits run without asking, and
/// the guard hook refuses every edit outside the workspace folder. `workitem-notify` sets the
/// record `waiting` on a prompt; a typed prompt or a tool run (an approved permission prompt
/// submits nothing) sets it back to `working`.
fn plan_settings(state: &AppState) -> String {
    let script = |name: &str| bin_script(state, name);
    json!({
        "permissions": { "allow": [format!("Bash(python3 {}:*)", script("workitem-plan-note")), "Edit", "Write"] },
        "hooks": {
            "PreToolUse": [{ "matcher": "Edit|Write|MultiEdit|NotebookEdit", "hooks": [{ "type": "command", "command": format!("python3 {}", shell_quote(&script("workitem-plan-guard"))) }] }],
            "Notification": hook(state, "workitem-notify", ""),
            "UserPromptSubmit": hook(state, "workitem-plan-note", " --status working"),
            "PostToolUse": hook(state, "workitem-plan-note", " --status working"),
        }
    })
    .to_string()
}

/// Inline `--settings` of a create session (decision 75): `workitem-notify` sets the session's
/// `@workitem_waiting` while the agent waits for input and unsets it once it goes on.
fn create_settings(state: &AppState) -> String {
    json!({
        "hooks": {
            "Notification": hook(state, "workitem-notify", ""),
            "UserPromptSubmit": hook(state, "workitem-notify", " --resume"),
            "PostToolUse": hook(state, "workitem-notify", " --resume"),
        }
    })
    .to_string()
}

fn bin_script(state: &AppState, name: &str) -> String {
    state.skills_dir.join("workitem-pipeline/bin").join(name).display().to_string()
}

/// A hook entry that runs bundled script `name` with `args`.
fn hook(state: &AppState, name: &str, args: &str) -> serde_json::Value {
    json!([{ "hooks": [{ "type": "command", "command": format!("python3 {}{args}", shell_quote(&bin_script(state, name))) }] }])
}

/// Hook commands run through a shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Runs `program` with `args` and then `prompt` as the last argument.
/// tmux sends a command to its server as one message and refuses one over about 16 KB, so the
/// prompt goes through a private file that `LAUNCH` reads, deletes and appends unchanged. The
/// workspace's GitHub token (decision 67) goes the same way into `GH_TOKEN`, so it is never in
/// an argv; a workspace whose account cannot be read starts nothing.
#[allow(clippy::too_many_arguments)]
async fn start(state: &AppState, session: &str, workspace: &Workspace, cwd: &Path, env: &[String], program: &OsString, args: &[OsString], prompt: Option<&str>) -> Result<(), String> {
    let token = crate::github::token(state, workspace).await?;
    let existing = tmux::sessions(state.tmux_socket.as_deref()).await;
    if let Some(other) = existing.iter().find(|s| s.name == session) {
        let owner = if other.workspace.as_os_str().is_empty() { "started outside the UI".into() } else { format!("workspace {}", other.workspace.display()) };
        return Err(format!("session {session} already exists ({owner})"));
    }
    let mut command = tmux::command(state.tmux_socket.as_deref());
    command.args(["new-session", "-d", "-s", session, "-c"]).arg(cwd);
    for var in env {
        command.arg("-e").arg(var);
    }
    command.arg("--");
    let token_file = token.map(|t| write_private(session, "token", &t)).transpose()?;
    let file = match prompt.map(|text| write_private(session, "prompt", text)).transpose() {
        Ok(file) => file,
        Err(e) => {
            token_file.iter().for_each(|f| drop(fs::remove_file(f)));
            return Err(e);
        }
    };
    let empty = PathBuf::new();
    command.args(["/bin/sh", "-c", LAUNCH, "sh"]).arg(token_file.as_ref().unwrap_or(&empty)).arg(file.as_ref().unwrap_or(&empty));
    command.arg(program).args(args);
    // Without a target, tmux tags the session of `$TMUX_PANE` when the backend runs inside tmux.
    // Not `={session}`: in the same command list that is resolved before the session exists.
    command.args([";", "set-option", "-t", session, tmux::WORKSPACE_OPTION]).arg(&workspace.dir);
    let output = command.output().await.map_err(|e| format!("cannot run tmux: {e}"));
    let failed = match &output {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(format!("tmux new-session failed: {}", String::from_utf8_lossy(&output.stderr).trim())),
        Err(e) => Some(e.clone()),
    };
    if let Some(e) = failed {
        for file in token_file.iter().chain(&file) {
            let _ = fs::remove_file(file);
        }
        return Err(e);
    }
    let _ = tmux::command(state.tmux_socket.as_deref())
        .args(["set-option", "-t", &format!("={session}:"), "remain-on-exit", "on"])
        .output()
        .await;
    Ok(())
}

/// `sh -c LAUNCH sh <token file> <prompt file> <agent> <args>...`, either file possibly empty:
/// exports the token file's content as `GH_TOKEN`, then runs the agent with the prompt file's
/// content as its last argument; each file is deleted once read. The trailing `x` keeps trailing
/// newlines, which `$(...)` would strip.
const LAUNCH: &str = r#"t=$1; f=$2; shift 2; if [ -n "$t" ]; then GH_TOKEN=$(cat -- "$t") || exit 1; rm -f -- "$t"; export GH_TOKEN; fi; [ -z "$f" ] && exec "$@"; p=$(cat -- "$f" && echo x) || exit 1; rm -f -- "$f"; exec "$@" "${p%x}""#;

static PROMPTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Writes `text` to a new file only we can read, in the temp folder.
fn write_private(session: &str, kind: &str, text: &str) -> Result<std::path::PathBuf, String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let n = PROMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("workitem-{kind}-{session}-{}-{n}", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

const PROTOCOL: &str = "agent_status is the agent_status: line in the frontmatter of the workitem's workitem.md.\n\
There is no tool for it: set it by editing that line in the file.\n\
A human may attach to this tmux session ({session}). If you need input,\n\
set agent_status: blocked: <reason>, ask the question here and wait.\n\
After the answer, set agent_status: working and continue.\n\
If the frontmatter has a parent: line, ask with the ask_parent tool of the workitem MCP server\n\
first: it blocks the item for you and returns the answer. If it refuses, ask here.\n\
Permission prompts stay here.\n\
Before you idle on something that needs no human (CI, a deploy), set\n\
agent_status: waiting: <what>. When it is done, set agent_status: working and continue.\n";

fn protocol(session: &str) -> String {
    PROTOCOL.replace("{session}", session)
}

/// The `SKILL.md` of handler `name` for an item on `board`. The first of these wins: the board's
/// `skills/<name>/SKILL.md`, the workspace's, the bundled one.
pub fn handler_file(state: &AppState, workspace: &Workspace, board: &str, name: &str) -> Result<Found, String> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err(format!("'{name}' is not a handler name (lowercase letters, digits and - only)"));
    }
    let places = locate::places(workspace, board, "skills", state.skills_dir.clone());
    locate::find(&places, &Path::new(name).join("SKILL.md"))
        .ok_or_else(|| format!("handler '{name}' not found: no {name}/SKILL.md in {}", locate::looked(&places)))
}

/// Instructions of handler `name` for an item on `board`, without the front matter. Claude does
/// not discover skills in `.workitems/`, so the launcher reads the file and puts it in the prompt.
pub fn instructions(state: &AppState, workspace: &Workspace, board: &str, name: &str) -> Result<String, String> {
    let found = handler_file(state, workspace, board, name)?;
    let text = fs::read_to_string(&found.path).map_err(|e| format!("{}: {e}", found.path.display()))?;
    Ok(strip_frontmatter(&text).to_string())
}

fn strip_frontmatter(text: &str) -> &str {
    match text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n")) {
        Some((_, body)) => body.trim_start_matches('\n'),
        None => text,
    }
}

/// Where the item is and where it may go; added to every handler prompt, skill or inline, so
/// handlers never need to know stage names.
fn stage_context(state: &AppState, place: &Place, stage: &Stage) -> String {
    let next = match stage.next.split_first() {
        Some((first, rest)) => std::iter::once(format!("{first} (default)")).chain(rest.iter().cloned()).collect::<Vec<_>>().join(", "),
        None => "none".into(),
    };
    format!(
        "Workitem {dir} {repo}, on board {board}.\n\
Workitem file: {file} (in the workspace folder, outside the repo)\n\
Current stage: {stage}\n\
Allowed next stages: {next}. The first is the default forward move.\n\
Move it with python3 {mv} {file} <stage>. Only workitem-move changes stage:.\n",
        dir = place.dir,
        repo = place.in_repo(),
        board = place.board,
        file = place.file().display(),
        stage = stage.name,
        mv = state.skills_dir.join("workitem-pipeline/bin/workitem-move").display(),
    )
}

/// The learnings of `board`: rules written down from past reviews, by the review fixers or by hand,
/// in `<workspace>/boards/<board>/learnings.md`. Boards do not share a file. `None` for a board
/// name that is not a plain name.
pub fn learnings_file(workspace: &Path, board: &str) -> Option<PathBuf> {
    valid_board_name(board).then(|| workspace.join("boards").join(board).join("learnings.md"))
}

/// The learnings of `board` as a prompt block, or nothing when the file is missing or holds only
/// headings.
pub fn learnings(workspace: &Path, board: &str) -> String {
    let text = learnings_file(workspace, board).and_then(|f| fs::read_to_string(f).ok()).unwrap_or_default();
    if !text.lines().any(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        return String::new();
    }
    // The file keeps each rule's source links for people; the agent only needs the rule.
    let rules: Vec<&str> = text
        .lines()
        .map(|l| match l.rfind(" (source: ") {
            Some(i) if l.trim_end().ends_with(')') => &l[..i],
            _ => l,
        })
        .collect();
    format!(
        "Rules learned from past reviews in this repo. Follow them like the repo's CLAUDE.md rules.\n\
--- learnings ---\n{}\n--- end of learnings ---\n",
        rules.join("\n").trim_end()
    )
}

/// The step that has a review fixer write down what generalises from the comments it acted on.
fn record_learnings(place: &Place) -> String {
    let Some(file) = learnings_file(place.workspace, place.board) else { return String::new() };
    format!(
        "Record learnings in {file}: for each comment you acted on whose lesson applies beyond this PR and is not \
already covered by the repo's CLAUDE.md/AGENTS.md or an entry in that file, add one bullet written as a rule, ending with \
(source: <comment url>). If an entry already says the same, append the new source link to it instead. \
Create the file with a # Learnings heading if it is missing. Most runs add nothing. \
If the edit fails because the file changed, read it again and retry.",
        file = file.display(),
    )
}

/// Prompt for a stage handler (analyze, development, or an inline prompt): the stage
/// context, then `instructions`.
pub fn handler_prompt(state: &AppState, place: &Place, stage: &Stage, instructions: &str) -> String {
    let session = format!("wi-{}", place.dir);
    // Development writes code, so it does not get the read-only text.
    let rules = if stage.develops() {
        "The workitem is approved. If it is already in the development stage, resume it as is and do not\n\
ask whether to resume or restart.\n"
    } else {
        "Only change the workitem with the instructions' own steps, with workitem-move, and by editing its\n\
agent_status line. Do not change code.\n"
    };
    let kill = if stage.develops() {
        String::new()
    } else {
        format!("\nIf you stop blocked without a question for the human, your last action is\ntmux kill-session -t {session}.")
    };
    format!(
        "{context}\nFollow these instructions for this workitem:\n--- instructions ---\n{instructions}\n--- end of instructions ---\n\
{rules}{learnings}{protocol}\
When you are done, make sure agent_status is ok (a stage move does this).{kill}",
        context = stage_context(state, place, stage),
        learnings = if stage.develops() || stage.learnings { learnings(place.workspace, place.board) } else { String::new() },
        protocol = protocol(&session),
    )
}

/// Prompt for a session the human opens from the detail panel and then drives by hand.
pub fn session_prompt(place: &Place) -> String {
    let session = format!("wi-{}", place.dir);
    let worktree = match place.repo {
        Some(repo) => format!("If the workitem has a worktree {}, work there (the session starts in it when it exists).\n", git::worktree_path(repo, place.dir).display()),
        None => String::new(),
    };
    format!(
        "You are working on workitem {dir} {repo}. Read {file} first (in the workspace folder, outside the repo).\n\
{worktree}Never change stage: and do not run the stage handler skill; a human drives this session and tells you what to do.\n\
{protocol}",
        dir = place.dir,
        file = place.file().display(),
        repo = place.in_repo(),
        protocol = protocol(&session),
    )
}

/// Prompt for a session where the human talks through the brief and plan and has them adjusted.
pub fn brief_prompt(place: &Place) -> String {
    let session = format!("wi-{}", place.dir);
    format!(
        "You are discussing the brief and plan of workitem {dir} {repo} with a human. Read {file} first, \
in particular its ## Review brief (### Plan, ### Risk, ### Test plan, ...).\n\
Start with a two or three line summary of the plan, then wait for the human. Answer questions, check claims against the code, \
and change the brief only when the human asks you to: edit the workitem.md text in place and keep every ### section of the brief non-empty.\n\
Do not change code, never change stage: or approved:, and do not run the stage handler skill.\n\
If the brief changes while the workitem is approved, say so: the human decides whether the approval still holds.\n\
{protocol}",
        dir = place.dir,
        file = place.file().display(),
        repo = place.in_repo(),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer the dispatcher starts when an in-review PR has red CI.
pub fn ci_fix_prompt(place: &Place, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{}", place.dir);
    let worktree = git::worktree_path(repo, place.dir);
    format!(
        "CI is failing on {pr}, the PR of workitem {dir} in repo {repo}.\n\
1. Work in the workitem's worktree {worktree} on the PR branch (git worktree add it if missing).\n\
2. Find the failure: gh pr checks {pr}, then gh run view <run> --log-failed.\n\
3. Fix the cause in code, run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed and the learnings below, commit and push. No force push, no rebase.\n\
4. Pushed a fix: set agent_status: ok and stop.\n\
5. Cannot fix it (flaky or infrastructure failure, failure not caused by this PR, needs a decision or a secret): \
set agent_status: blocked: <one line reason>, explain here and wait.\n\
{learnings}The workitem file is {file} (in the workspace folder, outside the repo): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        dir = place.dir,
        file = place.file().display(),
        worktree = worktree.display(),
        repo = repo.display(),
        learnings = learnings(place.workspace, place.board),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer `pr-review` starts when an in-review PR gets new comments or reviews that ask for a change.
pub fn comments_fix_prompt(place: &Place, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{}", place.dir);
    let worktree = git::worktree_path(repo, place.dir);
    format!(
        "New review comments or a review requesting changes on {pr}, the PR of workitem {dir} in repo {repo}.\n\
1. Work in the workitem's worktree {worktree} on the PR branch (git worktree add it if missing).\n\
2. Read them: gh pr view {pr} --comments (reviews included), and gh api repos/{{owner}}/{{repo}}/pulls/<number>/comments for inline ones. \
Act only on comments and reviews from others and on your user's own comments that contain @change or that start an inline thread (not a reply). \
A review that requests changes: implement what the reviewer asked for.\n\
3. Check each against the actual code before acting, bot findings included. Implement a clear, legitimate change. \
Leave a clearly wrong one untouched. Unsure whether a comment or suggestion is right, or what it asks for: \
do not guess and do not skip it. Finish the rest, then set agent_status: blocked: <your question, one line>, ask it here and wait.\n\
4. Run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed and the learnings below, commit and push. No force push, no rebase.\n\
5. Reply once on the PR to each comment you acted on: \"Fixed in <short sha>\" plus a few words on what changed, \
or \"Not changing this: <reason>\" for one you denied. Inline comment: \
gh api repos/{{owner}}/{{repo}}/pulls/<number>/comments/<id>/replies -f body=<text>. Other PR comment: gh pr comment {pr} --body <text>. \
Do not reply to one you are blocked on.\n\
6. {record} Then set agent_status: ok and stop.\n\
7. Cannot do it (needs a decision, large refactor, database migration): \
set agent_status: blocked: <one line reason>, explain here and wait.\n\
{learnings}The workitem file is {file} (in the workspace folder, outside the repo): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        dir = place.dir,
        file = place.file().display(),
        worktree = worktree.display(),
        repo = repo.display(),
        record = record_learnings(place),
        learnings = learnings(place.workspace, place.board),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer the dispatcher starts when an in-review PR conflicts with its base.
pub fn conflict_fix_prompt(place: &Place, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{}", place.dir);
    let worktree = git::worktree_path(repo, place.dir);
    format!(
        "{pr}, the PR of workitem {dir} in repo {repo}, conflicts with its base branch.\n\
1. Work in the workitem's worktree {worktree} on the PR branch (git worktree add it if missing).\n\
2. Get the base branch name (gh pr view {pr} --json baseRefName), git fetch origin <base>, then git merge origin/<base>, \
never a local copy of it. Do not rebase.\n\
3. Resolve every conflict. Read what each side changed (git log --merge -p <file>) and keep both sides' behavior. \
When it is unclear what someone meant, pick the most likely meaning and go on. \
Generated files (lockfiles, schema dumps, generated docs): take the base version, then rerun the tool that writes them; \
if the tool cannot run, merge both sides by hand. No -X ours/theirs, never git merge --abort.\n\
4. Run the affected tests and the linter, and fix what the merge broke. Commit the merge and git push (no force push). \
If failures remain that you cannot fix, push anyway and name them here in one line: the CI fixer takes over.\n\
5. Set agent_status: ok and stop. Do not block on the conflict.\n\
{learnings}The workitem file is {file} (in the workspace folder, outside the repo): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        dir = place.dir,
        file = place.file().display(),
        worktree = worktree.display(),
        repo = repo.display(),
        learnings = learnings(place.workspace, place.board),
        protocol = protocol(&session),
    )
}

/// The boards of the workspace with their repos, one per line, for sessions that may create
/// items on any of them.
pub fn board_list(state: &AppState, workspace: &Workspace) -> String {
    state
        .board_names(workspace)
        .iter()
        .map(|board| match state.repo_of(workspace, board) {
            Ok(Some(repo)) => format!("- {board}: repo {}\n", repo.display()),
            Ok(None) => format!("- {board}: no repo (items there change no code)\n"),
            Err(e) => format!("- {board}: unusable ({e})\n"),
        })
        .collect()
}

pub fn create_prompt(state: &AppState, workspace: &Workspace, session: &str, input: &str, board: &str) -> Result<String, String> {
    let instructions = instructions(state, workspace, board, "workitem-create")?;
    Ok(format!(
        "Create a workitem with the arguments --workspace {dir} --board {board}. The boards of this workspace:\n{boards}\
Follow these instructions:\n\
--- instructions ---\n{instructions}\n--- end of instructions ---\n\
The task, as typed in the web UI:\n\n{input}\n\n\
{protocol}\
Write created_by: {session} into the frontmatter of every workitem you write, so the web UI can link it.\n\
When the workitem files are written, your last action is tmux kill-session -t {session}.",
        dir = workspace.dir.display(),
        boards = board_list(state, workspace),
        protocol = protocol(session),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place<'a>(workspace: &'a Path, board: &'a str) -> Place<'a> {
        Place { dir: "item-1", workspace, repo: Some(Path::new("/r/proj")), board }
    }

    #[test]
    fn opencode_gets_only_a_provider_model() {
        assert_eq!(model_args(AgentKind::Opencode, "opus"), Vec::<OsString>::new(), "a Claude alias falls back to opencode's default");
        assert_eq!(model_args(AgentKind::Opencode, "anthropic/claude-opus"), ["-m", "anthropic/claude-opus"]);
        assert_eq!(model_args(AgentKind::Omp, "opus"), ["--model=opus"]);
        assert_eq!(model_args(AgentKind::Claude, "opus"), ["--model", "opus"]);
    }

    #[test]
    fn fixer_prompts_name_the_workitem_file_in_the_workspace() {
        let workspace = Path::new("/w/main");
        let want = "/w/main/item-1/workitem.md";
        let place = place(workspace, "dev");
        let repo = Path::new("/r/proj");
        for prompt in [ci_fix_prompt(&place, repo, "#7"), comments_fix_prompt(&place, repo, "#7"), conflict_fix_prompt(&place, repo, "#7"), session_prompt(&place), brief_prompt(&place)] {
            assert!(prompt.contains(want), "missing workitem path in: {prompt}");
            assert!(prompt.contains("/r/proj"), "missing repo in: {prompt}");
        }
    }

    #[test]
    fn the_conflict_fixer_merges_the_base_and_never_blocks() {
        let place = place(Path::new("/w/main"), "dev");
        let prompt = conflict_fix_prompt(&place, Path::new("/r/proj"), "#7");
        assert!(prompt.contains("git merge origin/<base>"), "{prompt}");
        for word in ["rebase onto", "--force", "needs manual rebase", "Give up", "git rebase --abort"] {
            assert!(!prompt.contains(word), "{word} in: {prompt}");
        }
    }

    #[test]
    fn prompts_of_an_item_without_a_repo_say_so_and_name_no_worktree() {
        let place = Place { repo: None, ..place(Path::new("/w/main"), "sdb") };
        for prompt in [session_prompt(&place), brief_prompt(&place)] {
            assert!(prompt.contains("/w/main/item-1/workitem.md"), "{prompt}");
            assert!(prompt.contains("board without a repo"), "{prompt}");
            assert!(!prompt.contains("worktree /"), "{prompt}");
        }
    }

    #[test]
    fn each_board_has_its_own_learnings_file() {
        let workspace = Path::new("/w/main");
        assert_eq!(learnings_file(workspace, "dev").unwrap(), Path::new("/w/main/boards/dev/learnings.md"));
        assert_eq!(learnings_file(workspace, "../x"), None, "a board name never leaves the boards folder");
    }

    #[test]
    fn learnings_are_empty_without_a_file_or_rules() {
        let workspace = tempfile::tempdir().unwrap();
        assert_eq!(learnings(workspace.path(), "dev"), "");
        fs::create_dir_all(workspace.path().join("boards/dev")).unwrap();
        fs::write(workspace.path().join("boards/dev/learnings.md"), "# Learnings\n\n").unwrap();
        assert_eq!(learnings(workspace.path(), "dev"), "", "a header alone adds nothing");
        assert_eq!(learnings(workspace.path(), "../x"), "");
    }

    #[test]
    fn learnings_in_the_prompt_leave_out_source_links() {
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("boards/dev/learnings.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = "- Rule one. (source: https://x/1, https://x/2)\n- Rule two.\n";
        fs::write(&file, text).unwrap();
        let prompt = learnings(workspace.path(), "dev");
        assert!(prompt.contains("--- learnings ---\n- Rule one.\n- Rule two.\n--- end of learnings ---"), "{prompt}");
        assert!(!prompt.contains("source:"), "{prompt}");
        assert_eq!(fs::read_to_string(&file).unwrap(), text, "the file keeps its links");
    }

    #[test]
    fn every_fixer_gets_its_boards_learnings_and_review_fixers_write_them() {
        let workspace = tempfile::tempdir().unwrap();
        for (board, text) in [("dev", "- Dev rule.\n"), ("ops", "# Learnings\n\n- Never mock tmux.\n")] {
            fs::create_dir_all(workspace.path().join("boards").join(board)).unwrap();
            fs::write(workspace.path().join("boards").join(board).join("learnings.md"), text).unwrap();
        }
        let file = workspace.path().join("boards/ops/learnings.md").display().to_string();
        let place = place(workspace.path(), "ops");
        let repo = Path::new("/r/proj");
        let review = [comments_fix_prompt(&place, repo, "#7")];
        let other = [ci_fix_prompt(&place, repo, "#7"), conflict_fix_prompt(&place, repo, "#7")];
        for prompt in review.iter().chain(&other) {
            assert!(prompt.contains("--- learnings ---\n# Learnings\n\n- Never mock tmux.\n--- end of learnings ---"), "{prompt}");
            assert!(!prompt.contains("Dev rule"), "{prompt}");
        }
        for prompt in &review {
            assert!(prompt.contains(&format!("Record learnings in {file}")), "{prompt}");
        }
        for prompt in &other {
            assert!(!prompt.contains("Record learnings"), "{prompt}");
        }
    }
}
