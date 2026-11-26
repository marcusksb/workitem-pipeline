---
name: workitem-fix-ci
description: Fixer for a workitem whose PR has failing CI. Finds the failure, fixes it in the workitem's worktree and pushes, or blocks with a reason. Started by the UI's `pr-review` watcher, which puts the PR, the worktree and the workitem file above these instructions.
---

1. Work in the workitem's worktree on the PR branch (git worktree add it if missing).
2. Find the failure: gh pr checks <pr>, then gh run view <run> --log-failed.
3. Fix the cause in code, run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed and the learnings below, commit and push. No force push, no rebase.
4. Pushed a fix: set agent_status: ok and stop.
5. Cannot fix it (flaky or infrastructure failure, failure not caused by this PR, needs a decision or a secret): set agent_status: blocked: <one line reason>, explain here and wait.

Set agent_status in the workitem file. Never change stage:.
