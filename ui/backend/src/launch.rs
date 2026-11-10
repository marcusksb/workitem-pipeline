use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde_json::json;

use crate::{
    config::{self, Workspace},
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

/// Starts `claude` with a prompt in a detached tmux session: `remain-on-exit` keeps a crashed
/// session visible, and the pipeline hooks mark a session that waits for input as blocked. The
/// prompt goes to `claude` as one argv element (see `start`), never interpreted by a shell. A
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
            env.push(CLAUDE_MD_OF_ADDED_DIRS.into());
            dirs.extend(state.repos(workspace));
            workspace.dir.clone()
        }
    };
    let mut args = add_dirs(&dirs);
    let hooks = state.skills_dir.join("workitem-pipeline/hooks.json");
    args.extend(["--settings".into(), hooks.into()]);
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    start(state, session, workspace, &cwd, &env, &args, Some(prompt)).await
}

/// The `model` of the stage the item is in now (decision 60), so every session of the item,
/// handler, fixer or one started by hand, runs on it.
fn stage_model(state: &AppState, workspace: &Workspace, place: &Place) -> Option<String> {
    let (item, _) = workitem::read(workspace, place.dir)?;
    state.pipeline(workspace, place.board).pipeline?.stage(&item.stage)?.model.clone()
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

/// Starts a planning session (decisions 36, 49) in the workspace folder with every board's repo
/// added: `claude` with the planning settings and `claude_args` (`--session-id <uuid>`, or
/// `--resume <uuid>`), then `prompt` if any. `record` is its plan record, which the hooks and
/// `workitem-plan-note` update.
pub async fn spawn_plan(state: &AppState, session: &str, workspace: &Workspace, record: &Path, claude_args: &[&str], prompt: Option<&str>) -> Result<(), String> {
    let env = [format!("WORKITEM_PLAN={}", record.display()), format!("WORKITEM_WORKSPACE={}", workspace.dir.display()), CLAUDE_MD_OF_ADDED_DIRS.into()];
    let mut args = add_dirs(&state.repos(workspace));
    args.extend(["--settings".into(), plan_settings(state).into()]);
    args.extend(claude_args.iter().map(OsString::from));
    start(state, session, workspace, &workspace.dir, &env, &args, prompt).await
}

/// Inline `--settings` of a planning session: the helper and file edits run without asking, and
/// the guard hook refuses every edit outside the workspace folder.
fn plan_settings(state: &AppState) -> String {
    let bin = state.skills_dir.join("workitem-pipeline/bin");
    let script = |name: &str| bin.join(name).display().to_string();
    let hook = |name: &str, args: &str| json!([{ "hooks": [{ "type": "command", "command": format!("python3 {}{args}", shell_quote(&script(name))) }] }]);
    json!({
        "permissions": { "allow": [format!("Bash(python3 {}:*)", script("workitem-plan-note")), "Edit", "Write"] },
        "hooks": {
            "PreToolUse": [{ "matcher": "Edit|Write|MultiEdit|NotebookEdit", "hooks": [{ "type": "command", "command": format!("python3 {}", shell_quote(&script("workitem-plan-guard"))) }] }],
            "Notification": hook("workitem-notify", ""),
            "UserPromptSubmit": hook("workitem-plan-note", " --status working"),
        }
    })
    .to_string()
}

/// Hook commands run through a shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Runs the workspace's agent (decisions 39, 52), resolved here because tmux's `PATH` may differ from ours,
/// or `state.claude` when it has none, with `args` and then `prompt` as the last argument.
/// tmux sends a command to its server as one message and refuses one over about 16 KB, so the
/// prompt goes through a private file that `READ_PROMPT` reads, deletes and appends unchanged.
async fn start(state: &AppState, session: &str, workspace: &Workspace, cwd: &Path, env: &[String], args: &[OsString], prompt: Option<&str>) -> Result<(), String> {
    let agent: OsString = match &workspace.agent {
        Some(name) => config::resolve_executable(name).ok_or_else(|| format!("agent '{name}' of workspace {} not found", workspace.name))?.into(),
        None => state.claude.clone().into(),
    };
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
    let file = match prompt {
        Some(text) => {
            let file = write_prompt(session, text)?;
            command.args(["/bin/sh", "-c", READ_PROMPT, "sh"]).arg(&file);
            Some(file)
        }
        None => None,
    };
    command.arg(agent).args(args);
    command.args([";", "set-option", tmux::WORKSPACE_OPTION]).arg(&workspace.dir);
    let output = command.output().await.map_err(|e| format!("cannot run tmux: {e}"));
    let failed = match &output {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(format!("tmux new-session failed: {}", String::from_utf8_lossy(&output.stderr).trim())),
        Err(e) => Some(e.clone()),
    };
    if let Some(e) = failed {
        if let Some(file) = file {
            let _ = fs::remove_file(file);
        }
        return Err(e);
    }
    let _ = tmux::command(state.tmux_socket.as_deref())
        .args(["set-option", "-t", &format!("={session}"), "remain-on-exit", "on"])
        .output()
        .await;
    Ok(())
}

/// `sh -c READ_PROMPT sh <file> <agent> <args>...`: runs the agent with the file's content as its
/// last argument. The trailing `x` keeps trailing newlines, which `$(...)` would strip.
const READ_PROMPT: &str = r#"f=$1; shift; p=$(cat -- "$f" && echo x) || exit 1; rm -f -- "$f"; exec "$@" "${p%x}""#;

static PROMPTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Writes `text` to a new file only we can read, in the temp folder.
fn write_prompt(session: &str, text: &str) -> Result<std::path::PathBuf, String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let n = PROMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("workitem-prompt-{session}-{}-{n}", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

const PROTOCOL: &str = "agent_status is the agent_status: line in the frontmatter of the workitem's workitem.md.\n\
There is no tool for it: set it by editing that line in the file.\n\
A human may attach to this tmux session ({session}). If you need input,\n\
set agent_status: blocked: <reason>, ask the question here and wait.\n\
After the answer, set agent_status: working and continue.\n\
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
    format!(
        "Rules learned from past reviews in this repo. Follow them like the repo's CLAUDE.md rules.\n\
--- learnings ---\n{}\n--- end of learnings ---\n",
        text.trim_end()
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

/// Prompt for a stage handler (triage, analyze, development, or an inline prompt): the stage
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
2. Get the base branch name (gh pr view {pr} --json baseRefName), git fetch origin <base>, then rebase onto origin/<base>, \
never a local copy of it. Resolve a conflict only when the result is obvious \
and keeps both sides' behavior. Lockfiles: take the base version, then rerun the tool. No -X ours/theirs.\n\
3. Give up when a db schema, structure or migration file is in conflict, more than 5 files or about 150 lines conflict, \
or you would have to guess what someone meant: git rebase --abort, then step 5.\n\
4. Run the affected tests and the linter. Only if they pass, git push --force-with-lease, then set agent_status: ok and stop.\n\
5. Cannot resolve it safely: set agent_status: blocked: merge conflict needs manual rebase: <one line reason>, explain here and wait.\n\
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
