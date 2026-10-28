---
name: workitem-start
version: 1.0.0
description: Pick up a workitem file created by the `workitem-create` skill, move it to the `in_development` stage (only if approved), prepare the worktree/branch it declared, and begin the implementation described in it. Use when asked to "start the workitem", "work on the workitem", "pick up <ticket>", "begin the created workitem".
triggers:
  - "start the workitem"
  - "work on the workitem"
  - "start workitem <ticket or slug>"
allowed-tools:
  - Bash
  - Read
  - Edit
  - Write
  - Agent
  - ListAgents
  - SendMessage
  - PushNotification
  - mcp__claude_ai_Atlassian_Rovo__getAccessibleAtlassianResources
  - mcp__claude_ai_Atlassian_Rovo__getJiraIssue
  - mcp__claude_ai_Atlassian_Rovo__getTransitionsForJiraIssue
  - mcp__claude_ai_Atlassian_Rovo__transitionJiraIssue
---

## When to invoke this skill

Use when a `workitem.md` file (created by the `workitem-create` skill) already exists and the user wants to begin acting on it — not to create a new one.

Examples:
- `/workitem-start SHAPE-17263`
- "start working on the workitem"
- "pick up the extract-story-build-parent-child workitem"

This skill is the handler for the pipeline stage `in_development`. It only starts a workitem that was approved in `ready_for_development` (`approved: true`, set by `/workitem-approve`). It locates the workitem, moves it to `in_development`, sets up the workspace it declared, and then carries out the work using the project's own conventions (already loaded from CLAUDE.md/AGENTS.md) — it does not re-derive those rules.

## Workflow

### Step 1 — Locate the workitem

1. Repo root: `git rev-parse --show-toplevel`.
2. List candidates: `<repo-root>/.workitems/*/workitem.md` (`.claude/workitems/` if that exists and `.workitems/` does not).
3. Resolve the argument:
   - A Jira ticket key (`[A-Z]+-\d+`) → match the directory that starts with it.
   - A directory/slug name → match by substring.
   - Empty:
     - Exactly one workitem with `stage: ready_for_development` and `approved: true` → use it.
     - Multiple → list them (title + stage) and ask which one.
     - None approved → say so, list any `in_development` ones in case the user means "resume", and stop. Point to `/workitem-approve` for items still waiting.
4. If nothing matches the argument, say so and stop — do not guess or create one.

### Step 2 — Read and validate

Read the matched `workitem.md` and parse the frontmatter.

- If `stage` is `backlog`, `analyze` or `ready_for_development` with `approved: false`, stop: it is not approved. Tell the user which stage it is in.
- If `stage` is already `in_development`, `in_review`, or `done`, tell the user and ask whether to resume as-is or restart — do not silently reset it.
  - If it's `in_development` and a `session:` field is present, that's a previously spawned background agent (see Step 4b) — offer to reconnect to it (Step 6) instead of restarting.
- If `repo:` is present and doesn't match the current repo root, warn before continuing — the workitem may belong to a different checkout.

### Step 3 — Move it to in_development

If your prompt says the item is already in the development stage (approving at the gate moves it there), skip this move. If the stage is `ready_for_development`, run `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem-dir> in_development` **before** doing any other work, so the state is durable even if the session is interrupted. It refuses when `approved` is not `true`: report that and stop. Do not edit `stage:` by hand. Then set `agent_status: working` — a sibling field to `status:` that the `workitem-watch` skill later reuses: `working` while an agent is actively on it, `blocked: <reason>` when it's stopped for a human decision, `ok` once it's clean and handed off. This is what lets anyone check a workitem's real-time state without reading this session's scrollback.

Also record who's doing the work, regardless of inline vs. background: call `ListAgents` — its result always states "This session is `<name>` [ref]", even for a plain inline run with no spawned agent. Write that name into the frontmatter as `session:`. This makes every workitem joinable via `SendMessage` to that name, not just the ones launched through Step 4b. (Step 4b overwrites this with the spawned agent's own name once it exists — that agent, not this launching session, is now the one doing the work.)

Then move the linked Jira issue into development, using the Atlassian Rovo MCP tools (not the `jira` CLI). Skip this if `issue:` is `none`.

1. Get the `cloudId` with `getAccessibleAtlassianResources`, then read the issue's current status with `getJiraIssue`. If it is already `In development` or `In Progress`, do nothing.
2. Call `getTransitionsForJiraIssue` and pick the transition whose target status is `In development`; if there is none, use `In Progress`. Run it with `transitionJiraIssue`.
3. If neither transition exists or the call fails, do not stop the workitem: note it, list the available transitions in the Step 8 report, and continue.

### Step 4 — Decide inline vs. background

- Default: run inline, in this session (Step 5 onward, directly).
- If the user asked to run it "in the background", "as a background agent", or similar: go to **Step 4b** instead of Step 5.

### Step 4b — Launch as a background agent

The spawned agent must be self-contained — it does not inherit this session's context, so build its prompt from the workitem file alone, the same way the `workitem-create` skill's own philosophy requires ("another agent can pick up and act on without further context").

1. Build the agent's prompt: the full contents of `workitem.md` (frontmatter + body) inline, plus Steps 5 ("Prepare the workspace") through 8 ("Report") of this skill's workflow, verbatim, as its instructions to execute.
2. Launch it with `Agent`:
   - `subagent_type`: omit (fresh general-purpose agent) — not `fork`. A fork inherits this session's context and is meant to report back here; this task must stand on its own so anyone can join it later.
   - `name`: the workitem's directory name (e.g. `SHAPE-17263-extract-storybuildparentchild-into-service`) — makes it recognizable in `ListAgents`.
   - Do **not** pass `isolation: "worktree"` — the workitem's own Step 4 worktree flow (`bin/setup_worktree`, project-specific naming) already handles isolation; the generic mechanism would create a second, conflicting worktree.
3. The `Agent` call returns that agent's name/id. Immediately overwrite the workitem frontmatter's `session:` field with it (Step 3 already set it to this launching session's own name — the spawned agent is now the one actually doing the work, so it takes over the address).
4. Report to the user per Step 8, noting this ran in the background.

To join or steer a running background agent later (in this session or a future one): `ListAgents` to confirm it's still there, then `SendMessage` to the name recorded in the workitem's `session:` field.

### Step 5 — Prepare the workspace

Read `worktree_needed` from the frontmatter.

**If `true`:**

1. If `issue:` is `none`, stop and ask for a ticket key per this repo's Git Workflow rule — never invent one. If running inline, ask directly. If running as a background agent (Step 4b), instead set `agent_status: blocked: no Jira ticket linked, needs a ticket key`, call `PushNotification` with that reason, and stop — there's no one to ask inline.
2. Reuse the workitem's own directory name (already `<TICKET-ID>-<slug>`) instead of re-deriving a slug. Type prefix: `bug` → `fix`, `feature` → `feature`, any other commitizen type (`refactor`, `docs`, `chore`, …) → the type itself, matching the repo's own `fix/SHAPE-15490-description` example — adjust to match an existing branch pattern in the repo if this guess conflicts.
3. If `../<repo-name>-<workitem-dir-name>` already exists (`git worktree list`), or you are already in it, reuse it: work there and skip to step 4. Otherwise follow this repo's own documented worktree flow (AGENTS.md → Worktrees), not a generic one — it runs `bin/setup_worktree` for isolated test databases. Branch from the freshly fetched remote master, never the local `master`, which may be stale:
   ```
   git fetch origin master
   git worktree add -b <type-prefix>/<workitem-dir-name> ../<repo-name>-<workitem-dir-name> origin/master
   cd ../<repo-name>-<workitem-dir-name>
   bin/setup_worktree
   ```
4. Per stored project memory, `bin/setup_worktree` alone may not be enough: check `.env.development.local` and `.env.test.local` exist in the new worktree; if missing, copy them from the main checkout.
5. Continue the rest of this skill's steps with the worktree as the working directory.

**If `false`:** stay on the current branch. Run `git branch --show-current` and confirm it's sane for this work before making changes.

### Step 6 — Do the work

Follow the workitem's `## Description` and the project's own conventions already loaded (CLAUDE.md/AGENTS.md) for the declared `type`:

- `bug` → write a failing test that reproduces it first, then confirm before fixing: if running inline, ask directly; if running as a background agent, set `agent_status: blocked: failing test written, needs confirmation before fixing`, call `PushNotification`, and wait — a message to this agent's name (see Step 4b) resumes it.
- `feature` → if it changes the API surface, follow the API-First Workflow (schema/contract before implementation); otherwise implement per the layer boundaries in AGENTS.md.
- `refactor` / `perf` → behavior must not change: run the existing tests before and after, and add tests only where coverage is missing.
- `docs` / `style` / `test` / `build` / `ci` / `chore` / `revert` → implement per the description and the project's conventions; keep the change limited to that type's scope.
- `analysis` → no code changes; research and write the findings/decision into the workitem file itself, under a new `## Findings` section.

Run `rubocop -a` and the relevant `bin/rspec` target before treating the change as done, per the repo's "Before Committing" rule.

Then review your own diff (`git diff origin/master...`) against the repo's CLAUDE.md/AGENTS.md before committing: reread the sections that cover the kinds of files you changed (for specs, the test rules) and fix every violation. The rules were loaded at startup but are easy to drift from during a long implementation.

### Step 7 — Open the PR (if applicable)

Read `pr_mode` from the frontmatter.

- `none` → skip this step.
- `draft` or `ready` → this repo's own rule is to **always** create PRs as drafts regardless of `pr_mode`; if `pr_mode: ready`, tell the user about the conflict and default to draft unless they explicitly override.
- As soon as the PR is opened, write its URL into the workitem frontmatter as a new `pr:` field — the `workitem-watch` skill needs it to find this PR later.
- Start `/monitor-pr {pr}` per AGENTS.md. Once it reports CI green and comments addressed, mark the PR ready for review and request the `backend` team as reviewer (`gh pr ready {pr} && gh pr edit {pr} --add-reviewer "$(gh repo view --json owner -q .owner.login)/backend"`; if adding the team fails, say so in your report but still move on), then run `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem-dir> <next>` (the stage marked `(default)` under "Allowed next stages" in your prompt; `in_review` on the bundled board; it also sets `agent_status: ok`) — from here, `workitem-watch` takes over monitoring.
- Right after marking the PR ready, move the linked Jira issue to `In Review` the same way as in Step 3 (Rovo MCP tools; skip if `issue:` is `none`; do nothing if it is already `In Review`). If there is no `In Review` transition or the call fails, do not stop: list the available transitions in the report.

### Step 8 — Report

Tell the user: which workitem was picked up, the branch/worktree it's running in, that stage is now `in_development`, the Jira status it moved to (or why it did not move), and the next concrete action (e.g. "failing test written, confirm before I fix it" / "draft PR #1234 open, monitor-pr running"). If launched in the background (Step 4b), also give the `session:` name and remind them how to join: ask a Claude Code session to `SendMessage` that name.

## Splitting

If asked to split the work into smaller workitems, create each with `workitem-create --parent <this workitem's dir>` (plus `--repo` and `--issue` as needed). Do not write child `workitem.md` files by hand.
