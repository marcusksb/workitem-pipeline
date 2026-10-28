---
name: workitem-remove
description: Delete a workitem from the pipeline in the current repo. Shows what will be removed (workitem folder, worktree, branch, open PR), asks for confirmation, then deletes the folder and its pipeline state. Use when asked to "remove workitem X", "delete the workitem", "drop X from the pipeline".
---

# Workitem Remove

Deletes ONE workitem. Input: workitem directory name or Jira key. If none is given, list all workitems with `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move --list` and ask which one.

This is destructive. The workitem file is not in git, so it cannot be recovered. Always ask before deleting, unless the caller states the user already confirmed (the web UI does, and says which parts to remove).

## Steps

1. Find the workitem: `<repo-root>/.workitems/<dir>/` (`.claude/workitems/` if that exists and `.workitems/` does not). Match a Jira key by directory prefix, a name by substring. No match or more than one → say so and stop. Never delete a path outside `.workitems/`.
2. Read the frontmatter and collect what exists:
   - **Folder:** the workitem directory (list its files).
   - **Lock:** `<scratchpad>/workitem-<dir>.running`. If it is younger than 20 minutes, an agent is working on it: say so and stop unless the user insists.
   - **Worktree:** `../<repo-name>-<dir>` (check with `git worktree list`). Note if it has uncommitted changes (`git -C <path> status --short`).
   - **Branch:** the branch checked out in that worktree, or the `pr:` head branch.
   - **PR:** the `pr:` URL and its state (`gh pr view <n> --json state --jq .state`).
   - **Children:** other workitems whose frontmatter has `parent: <dir>` (`grep -l '^parent: <dir>$' <repo-root>/.workitems/*/workitem.md`). They are not deleted. Remove the dangling `parent:` line from each child after the folder is deleted, and report it.
3. Show the user this list, and the item's `stage`. If `stage` is `in_development` or `in_review`, or the PR is open, or the worktree has uncommitted changes, say that clearly first.
4. Ask what to remove, with the workitem folder as the default:
   - **Folder only (default).** Worktree, branch and PR stay.
   - **Folder + worktree.** Run `bin/teardown_worktree` inside the worktree first (drops its test databases, per `AGENTS.md`), then `git worktree remove <path>`. Never use `--force`. If git refuses because of local changes, report it and stop.
   - The local branch and the PR are never removed by this skill. Tell the user what is left so they can close the PR themselves.
5. Delete the folder: `rm -r <repo-root>/.workitems/<dir>` (not `rm -rf`, and only after the user said yes).
6. Clean up state: remove the lock file if it exists, and delete the `<dir>` key from `<scratchpad>/workitem-run.json` and `<scratchpad>/workitem-watch.json` if present.
7. Report one line per thing removed, and one line for anything left behind (branch, PR).

## Rules

- No confirmation, no deletion. A general "remove workitems" request is not a yes for a specific item.
- Do not close PRs, delete remote branches, or touch Jira.
- Remove one workitem per run.
