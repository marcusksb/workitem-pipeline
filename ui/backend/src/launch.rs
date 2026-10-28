use std::{fs, path::Path};

use crate::{
    config::Project,
    git,
    locate::{self, Found},
    pipeline::{Stage, DEFAULT_BOARD},
    tmux,
    workitem::{self, Workitem},
    AppState,
};

/// Starts `claude` with a prompt in a detached tmux session, the way `workitem-run` does:
/// `remain-on-exit` keeps a crashed session visible, and the pipeline hooks mark a session
/// that waits for input as blocked. The prompt goes to `claude` as one argv element, never
/// through a shell. A workitem's session starts in its worktree when it has one, with the repo
/// added so the agent can edit `workitem.md`; the session is tagged with `repo` in the same
/// tmux call, so no listing sees it untagged.
pub async fn spawn(
    state: &AppState,
    session: &str,
    repo: &Path,
    workitem: Option<&str>,
    prompt: &str,
) -> Result<(), String> {
    let existing = tmux::sessions(state.tmux_socket.as_deref()).await;
    if let Some(other) = existing.iter().find(|s| s.name == session) {
        return Err(format!("session {session} already exists (repo {})", other.repo.display()));
    }
    let hooks = state.skills_dir.join("workitem-pipeline/hooks.json");
    let mut command = tmux::command(state.tmux_socket.as_deref());
    let worktree = match workitem {
        Some(dir) => git::worktree_for(&git::worktrees(repo).await, repo, dir).cloned(),
        None => None,
    };
    command.args(["new-session", "-d", "-s", session, "-c"]).arg(worktree.as_deref().unwrap_or(repo));
    if let Some(dir) = workitem {
        command.arg("-e").arg(format!("WORKITEM={dir}"));
        command.arg("-e").arg(format!("WORKITEM_REPO={}", repo.display()));
    }
    command.arg("--").arg(&state.claude);
    if worktree.is_some() {
        // `--add-dir` takes several values, so `--settings` must follow it to end the list.
        command.arg("--add-dir").arg(repo);
    }
    command.arg("--settings").arg(hooks).arg(prompt);
    command.args([";", "set-option", tmux::REPO_OPTION]).arg(repo);
    let output = command.output().await.map_err(|e| format!("cannot run tmux: {e}"))?;
    if !output.status.success() {
        return Err(format!("tmux new-session failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let _ = tmux::command(state.tmux_socket.as_deref())
        .args(["set-option", "-t", &format!("={session}"), "remain-on-exit", "on"])
        .output()
        .await;
    Ok(())
}

const PROTOCOL: &str = "agent_status is the agent_status: line in the frontmatter of the workitem's workitem.md.\n\
There is no tool for it: set it by editing that line in the file.\n\
A human may attach to this tmux session ({session}). If you need input,\n\
set agent_status: blocked: <reason>, ask the question here and wait.\n\
After the answer, set agent_status: working and continue.\n";

fn protocol(session: &str) -> String {
    PROTOCOL.replace("{session}", session)
}

/// The `SKILL.md` of handler `name` for an item on `board`. The first of these wins: the board's
/// `skills/<name>/SKILL.md`, the project's, the bundled one.
pub fn handler_file(state: &AppState, project: &Project, board: &str, name: &str) -> Result<Found, String> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err(format!("'{name}' is not a handler name (lowercase letters, digits and - only)"));
    }
    let places = locate::places(project, board, "skills", state.skills_dir.clone());
    locate::find(&places, &Path::new(name).join("SKILL.md"))
        .ok_or_else(|| format!("handler '{name}' not found: no {name}/SKILL.md in {}", locate::looked(&places)))
}

/// Instructions of handler `name` for an item on `board`, without the front matter. Claude does
/// not discover skills in `.workitems/`, so the launcher reads the file and puts it in the prompt.
pub fn instructions(state: &AppState, project: &Project, board: &str, name: &str) -> Result<String, String> {
    let found = handler_file(state, project, board, name)?;
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
fn stage_context(state: &AppState, project: &Project, item: &Workitem, stage: &Stage) -> String {
    let next = match stage.next.split_first() {
        Some((first, rest)) => std::iter::once(format!("{first} (default)")).chain(rest.iter().cloned()).collect::<Vec<_>>().join(", "),
        None => "none".into(),
    };
    format!(
        "Workitem {dir} in repo {repo}, on board {board}.\n\
Workitem file: {file}\n\
Current stage: {stage}\n\
Allowed next stages: {next}. The first is the default forward move.\n\
Move it with python3 {mv} {file} <stage>. Only workitem-move changes stage:.\n",
        dir = item.dir,
        repo = project.repo.display(),
        board = item.board,
        file = workitem::file(&project.repo, &item.dir).display(),
        stage = stage.name,
        mv = state.skills_dir.join("workitem-pipeline/bin/workitem-move").display(),
    )
}

/// Prompt for a stage handler (triage, analyze, development, or an inline prompt): the stage
/// context, then `instructions`.
pub fn handler_prompt(state: &AppState, project: &Project, item: &Workitem, stage: &Stage, instructions: &str) -> String {
    let session = format!("wi-{}", item.dir);
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
{rules}{protocol}\
When you are done, make sure agent_status is ok (a stage move does this).{kill}",
        context = stage_context(state, project, item, stage),
        protocol = protocol(&session),
    )
}

/// Prompt for a session the human opens from the detail panel and then drives by hand.
pub fn session_prompt(dir: &str, repo: &Path) -> String {
    let session = format!("wi-{dir}");
    let worktree = git::worktree_path(repo, dir);
    format!(
        "You are working on workitem {dir} in repo {repo}. Read {file} first.\n\
If the workitem has a worktree {worktree}, work there (the session starts in it when it exists).\n\
Never change stage: and do not run the stage handler skill; a human drives this session and tells you what to do.\n\
{protocol}",
        file = workitem::file(repo, dir).display(),
        worktree = worktree.display(),
        repo = repo.display(),
        protocol = protocol(&session),
    )
}

/// Prompt for a session where the human talks through the brief and plan and has them adjusted.
pub fn brief_prompt(dir: &str, repo: &Path) -> String {
    let session = format!("wi-{dir}");
    format!(
        "You are discussing the brief and plan of workitem {dir} in repo {repo} with a human. Read {file} first, \
in particular its ## Review brief (### Plan, ### Risk, ### Test plan, ...).\n\
Start with a two or three line summary of the plan, then wait for the human. Answer questions, check claims against the code, \
and change the brief only when the human asks you to: edit the workitem.md text in place and keep every ### section of the brief non-empty.\n\
Do not change code, never change stage: or approved:, and do not run the stage handler skill.\n\
If the brief changes while the workitem is approved, say so: the human decides whether the approval still holds.\n\
{protocol}",
        file = workitem::file(repo, dir).display(),
        repo = repo.display(),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer the dispatcher starts when an in-review PR has red CI.
pub fn ci_fix_prompt(dir: &str, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{dir}");
    let worktree = git::worktree_path(repo, dir);
    format!(
        "CI is failing on {pr}, the PR of workitem {dir} in repo {repo}.\n\
1. Work in the workitem's worktree {worktree} on the PR branch (git worktree add it if missing).\n\
2. Find the failure: gh pr checks {pr}, then gh run view <run> --log-failed.\n\
3. Fix the cause in code, run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed, commit and push. No force push, no rebase.\n\
4. Pushed a fix: set agent_status: ok and stop.\n\
5. Cannot fix it (flaky or infrastructure failure, failure not caused by this PR, needs a decision or a secret): \
set agent_status: blocked: <one line reason>, explain here and wait.\n\
The workitem file is {file} (in the main repo, not the worktree): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        file = workitem::file(repo, dir).display(),
        worktree = worktree.display(),
        repo = repo.display(),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer the dispatcher starts when an in-review PR has a changes-requested review.
pub fn changes_fix_prompt(dir: &str, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{dir}");
    let worktree = git::worktree_path(repo, dir);
    format!(
        "A reviewer requested changes on {pr}, the PR of workitem {dir} in repo {repo}.\n\
1. Work in the workitem's worktree {worktree} on the PR branch (git worktree add it if missing).\n\
2. Read the review and its comments: gh pr view {pr} --comments, and gh api repos/{{owner}}/{{repo}}/pulls/<number>/comments for inline ones.\n\
3. Implement what the reviewer asked for, run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed, commit and push. No force push, no rebase.\n\
4. Reply once on the PR to each comment you acted on: \"Fixed in <short sha>\" plus a few words on what changed, \
or \"Not changing this: <reason>\" for one you denied. Inline comment: \
gh api repos/{{owner}}/{{repo}}/pulls/<number>/comments/<id>/replies -f body=<text>. Other: gh pr comment {pr} --body <text>. \
Then set agent_status: ok and stop.\n\
5. Cannot do it (unclear request, needs a decision, large refactor, database migration): \
set agent_status: blocked: <one line reason>, explain here and wait.\n\
The workitem file is {file} (in the main repo, not the worktree): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        file = workitem::file(repo, dir).display(),
        worktree = worktree.display(),
        repo = repo.display(),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer the dispatcher starts when an in-review PR gets new comments that ask for a change.
pub fn comments_fix_prompt(dir: &str, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{dir}");
    let worktree = git::worktree_path(repo, dir);
    format!(
        "New review comments on {pr}, the PR of workitem {dir} in repo {repo}.\n\
1. Work in the workitem's worktree {worktree} on the PR branch (git worktree add it if missing).\n\
2. Read them: gh pr view {pr} --comments, and gh api repos/{{owner}}/{{repo}}/pulls/<number>/comments for inline ones. \
Act only on comments from others and on your user's own comments that contain @change or that start an inline thread (not a reply).\n\
3. Check each against the actual code before acting, bot findings included. Implement a clear, legitimate change. \
Leave a clearly wrong one untouched. Unsure whether a comment or suggestion is right, or what it asks for: \
do not guess and do not skip it. Finish the rest, then set agent_status: blocked: <your question, one line>, ask it here and wait.\n\
4. Run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed, commit and push. No force push, no rebase.\n\
5. Reply once on the PR to each comment you acted on: \"Fixed in <short sha>\" plus a few words on what changed, \
or \"Not changing this: <reason>\" for one you denied. Inline comment: \
gh api repos/{{owner}}/{{repo}}/pulls/<number>/comments/<id>/replies -f body=<text>. Other PR comment: gh pr comment {pr} --body <text>. \
Do not reply to one you are blocked on. Then set agent_status: ok and stop.\n\
6. Cannot do it (needs a decision, large refactor, database migration): \
set agent_status: blocked: <one line reason>, explain here and wait.\n\
The workitem file is {file} (in the main repo, not the worktree): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        file = workitem::file(repo, dir).display(),
        worktree = worktree.display(),
        repo = repo.display(),
        protocol = protocol(&session),
    )
}

/// Prompt for the fixer the dispatcher starts when an in-review PR conflicts with its base.
pub fn conflict_fix_prompt(dir: &str, repo: &Path, pr: &str) -> String {
    let session = format!("wi-{dir}");
    let worktree = git::worktree_path(repo, dir);
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
The workitem file is {file} (in the main repo, not the worktree): set agent_status there.\n\
Never change stage:.\n\
{protocol}",
        file = workitem::file(repo, dir).display(),
        worktree = worktree.display(),
        repo = repo.display(),
        protocol = protocol(&session),
    )
}

pub fn create_prompt(state: &AppState, project: &Project, session: &str, input: &str, board: &str) -> Result<String, String> {
    let instructions = instructions(state, project, board, "workitem-create")?;
    let board = if board == DEFAULT_BOARD { String::new() } else { format!(" --board {board}") };
    Ok(format!(
        "Create a workitem with the arguments --repo {repo}{board}. Follow these instructions:\n\
--- instructions ---\n{instructions}\n--- end of instructions ---\n\
The task, as typed in the web UI:\n\n{input}\n\n\
{protocol}\
When the workitem file is written, your last action is tmux kill-session -t {session}.",
        repo = project.repo.display(),
        protocol = protocol(session),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixer_prompts_name_the_workitem_file_in_the_main_repo() {
        let repo = Path::new("/r/proj");
        let want = "/r/proj/.workitems/item-1/workitem.md";
        for prompt in [
            ci_fix_prompt("item-1", repo, "#7"),
            changes_fix_prompt("item-1", repo, "#7"),
            comments_fix_prompt("item-1", repo, "#7"),
            conflict_fix_prompt("item-1", repo, "#7"),
        ] {
            assert!(prompt.contains(want), "missing workitem path in: {prompt}");
        }
    }
}
