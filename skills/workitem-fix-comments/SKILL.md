---
name: workitem-fix-comments
description: Fixer for a workitem whose PR got new review comments or a review requesting changes. Checks each against the code, implements the legitimate ones, replies on the PR and records learnings, or blocks with a question. Started by the UI's `pr-review` watcher, which puts the PR, the worktree and the workitem file above these instructions.
---

1. Work in the workitem's worktree on the PR branch (git worktree add it if missing).
2. Read them: gh pr view <pr> --comments (reviews included), and gh api repos/{owner}/{repo}/pulls/<number>/comments for inline ones. Act only on comments and reviews from others and on your user's own comments that contain @change or that start an inline thread (not a reply). A review that requests changes: implement what the reviewer asked for.
3. Check each against the actual code before acting, bot findings included. Implement a clear, legitimate change. Leave a clearly wrong one untouched. Unsure whether a comment or suggestion is right, or what it asks for: do not guess and do not skip it. Finish the rest, then set agent_status: blocked: <your question, one line>, ask it here and wait.
4. Run the affected tests locally, check your diff against the repo's CLAUDE.md/AGENTS.md rules for the files you changed and the learnings below, commit and push. No force push, no rebase.
5. Reply once on the PR to each comment you acted on: "Fixed in <short sha>" plus a few words on what changed, or "Not changing this: <reason>" for one you denied. Inline comment: gh api repos/{owner}/{repo}/pulls/<number>/comments/<id>/replies -f body=<text>. Other PR comment: gh pr comment <pr> --body <text>. Do not reply to one you are blocked on.
6. Record learnings as the step below these instructions says, if there is one. Then set agent_status: ok and stop.
7. Cannot do it (needs a decision, large refactor, database migration): set agent_status: blocked: <one line reason>, explain here and wait.

Set agent_status in the workitem file. Never change stage:.
