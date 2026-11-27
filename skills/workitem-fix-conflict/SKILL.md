---
name: workitem-fix-conflict
description: Fixer for a workitem whose PR conflicts with its base branch. Merges the base into the PR branch, resolves every conflict keeping both sides' behavior and pushes; never blocks. Started by the UI's `pr-review` watcher, which puts the PR, the worktree and the workitem file above these instructions.
---

1. Work in the workitem's worktree on the PR branch (git worktree add it if missing).
2. Get the base branch name (`python3 ~/.claude/skills/workitem-pipeline/bin/workitem-pr base <workitem file>`), git fetch origin <base>, then git merge origin/<base>, never a local copy of it. Do not rebase.
3. Resolve every conflict. Read what each side changed (git log --merge -p <file>) and keep both sides' behavior. When it is unclear what someone meant, pick the most likely meaning and go on. Generated files (lockfiles, schema dumps, generated docs): take the base version, then rerun the tool that writes them; if the tool cannot run, merge both sides by hand. No -X ours/theirs, never git merge --abort.
4. Run the affected tests and the linter, and fix what the merge broke. Commit the merge and git push (no force push). If failures remain that you cannot fix, push anyway and name them here in one line: the CI fixer takes over.
5. Set agent_status: ok and stop. Do not block on the conflict.

Set agent_status in the workitem file. Never change stage:.
