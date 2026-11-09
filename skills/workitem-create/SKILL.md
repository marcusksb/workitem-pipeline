---
name: workitem-create
version: 1.0.0
description: Generate structured workitem files in a workspace from a Jira ticket or a plain task description — classifies it as bug/analysis/feature, decides whether a worktree and a PR (and which PR mode) are needed, links the Jira ticket if one exists, and splits a task that spans several repos into one linked item per board. Use when asked to "create a workitem", "generate a workitem for X", "make a workitem from this ticket/task".
triggers:
  - "create a workitem"
  - "generate a workitem"
  - "make a workitem for <ticket or task>"
allowed-tools:
  - Bash
  - Write
  - mcp__claude_ai_Atlassian_Rovo__getAccessibleAtlassianResources
  - mcp__claude_ai_Atlassian_Rovo__getJiraIssue
  - mcp__claude_ai_Sentry__get_sentry_resource
---

## When to invoke this skill

Use when the user wants a self-contained, structured workitem file that another agent can pick up and act on without further context — from either a Jira ticket ID or a plain-text task description.

Examples:
- `/workitem-create SBD-123`
- "Create a workitem for: the CDN cache header is missing on 404 responses"
- "Make a workitem for SHAPE-456"

This skill only **generates** the files. It does not implement anything and does not spawn the implementing agent.

## Workflow

### Step 1 — Get the input

If the skill argument matches a Jira ticket key (`[A-Z]+-\d+`), treat it as **Case A**. If it is a Sentry issue URL (contains `sentry.io`), treat it as **Case C**. Otherwise treat the argument as free text — **Case B**.

Items live in a workspace folder, not in a repo. `--workspace <folder>` names it (else use `$WORKITEM_WORKSPACE`; with neither, end your turn asking for the workspace folder and write nothing). `--board <name>` is the board the task was filed on; it must be `<workspace>/boards/<name>.json`, stop and say so if it is not (without `--board`, a workspace with one board uses that one, otherwise ask). Each board file names its repo in `"repo"`: an item belongs to the repo of its board. Read every `<workspace>/boards/*.json` now and note each board's name and repo.

Other optional flags: `--issue <KEY>` links a ticket, `--parent <dir>` marks the new workitem as a child of the workitem with that directory name (use it when splitting a workitem into smaller ones). Callers pass these instead of answering questions.

If no argument was given, end your turn with the question "Give me a Jira ticket ID, a Sentry issue URL, or describe the task." as the final message and write no file.

A new workitem always starts in the `entry` stage of its board (`entry` in `<workspace>/boards/<name>.json`). The UI's dispatcher triages it from there.

### Step 2 — Resolve title, summary, and description

**Case A — Jira ticket given:**

1. `mcp__claude_ai_Atlassian_Rovo__getAccessibleAtlassianResources` to get the cloudId.
2. `mcp__claude_ai_Atlassian_Rovo__getJiraIssue` with that cloudId and the ticket key, `responseContentFormat: "markdown"`.

Extract `fields.summary` and `fields.description`. If the call fails, show the error and stop — do not create a file.

- `title` = the summary, trimmed to ~10 words if needed.
- `summary` = summary, one line.
- `description` = the ticket description, condensed to a short paragraph (3–6 sentences) if long.
- `issue` = ticket key + issue URL.

**Case B — Plain text task:**

- `title` = a short (≤10 words) title derived from the text.
- `summary` = one sentence stating what needs to happen.
- `description` = the user's task text, condensed to a short paragraph if long.
- Do not ask about a Jira ticket. Use `--issue <KEY>`, or a ticket key that appears in the text.
  - If there is one, fetch it as in Case A and merge: keep the user's own framing for `description` unless it conflicts with the ticket, and set `issue` to the ticket key + URL.
  - If none, set `issue: none`.

**Case C — Sentry issue URL given:**

1. Fetch the issue with `mcp__claude_ai_Sentry__get_sentry_resource` (read only).
2. `title` = the error type and message, trimmed to ~10 words. `summary` = one line with the error and where it happens.
3. `description` = short paragraph: error, top stack frames in app code, first seen / last seen, event count.
4. `system_error` = the issue URL. Resolve the Jira ticket as in Case B (flag, key in text, else none).

Set `source` from the input: `issue` for Case A, `system_error` for Case C, `text` for Case B (even if a ticket is linked later).

Never invent or guess a ticket key.

### Step 3 — Classify the type

Decide **one** of `bug`, `feature`, `analysis`, or a commitizen type (`refactor`, `docs`, `style`, `perf`, `test`, `build`, `ci`, `chore`, `revert`) from the actual content (not just keywords):

- `bug` — something is broken, incorrect, or regressed; there's an expected-vs-actual mismatch.
- `feature` — a new capability, endpoint, or behavior is being requested.
- `analysis` — investigate, research, compare options, or produce a decision/report; no code change is implied by the ask itself.
- `refactor` — restructure code without changing behavior.
- `docs` — documentation only.
- `style` — formatting or lint changes with no logic change.
- `perf` — make something faster or lighter without changing behavior.
- `test` — add or fix tests only.
- `build` — build system, packaging, or dependency changes.
- `ci` — CI configuration or pipeline changes.
- `chore` — maintenance that fits none of the above.
- `revert` — undo an earlier change.

State a one-sentence rationale for the choice.

### Step 4 — Decide if a worktree is needed

Default heuristic, then use judgment on the actual content to override it:

- `analysis` → usually `false` (no code changes expected).
- every type except `analysis` → usually `true` (implement in an isolated worktree/branch rather than on the current one).

Override the default when the content clearly says otherwise (e.g. a "bug" that's actually just a one-off data query, or a "feature" ask that's really a config/copy change with no code). State the rationale.

### Step 5 — Decide the PR mode

One of `none`, `draft`, `ready`:

- `analysis` → `none` (nothing to open a PR for).
- every type except `analysis` → `draft` by default — prefer opening PRs as drafts unless the project's own conventions say otherwise.
- `ready` → only pick this if the input explicitly says the change is pre-approved or should skip review. This should be rare; state why when chosen.

Check the board's repo's own CLAUDE.md/AGENTS.md for a stated PR convention (e.g. "always create PRs as drafts") and defer to it over the default above.

State the rationale.

### Step 6 — Decide which boards get an item

Most tasks touch one repo: one item, on the `--board` board. When the task clearly needs changes in the repos of several boards (an API change and the frontend that uses it), create one item per board that needs work, each describing only its repo's part. Look at the repos (read only) when the text alone does not say which parts exist where. Do not split on a guess: when it is unclear whether a second repo is involved, create one item and mention the possible second one in its `## Description`.

When you split, decide the order: an item that another one needs first (the API before its consumer) is a dependency. Each dependent item gets `depends_on: <dir of the item it needs>` (comma-separated for several). Boards hold a dependent item back until its dependencies are merged when they ask for it. Only the last item of the chain (the one no other item depends on) moves the Jira ticket; say so in each other item's `## Jira` section.

State the decision and its rationale in one sentence.

### Step 7 — Derive slug and path

1. Take `title`, lowercase it, replace spaces with `-`, strip all characters that are not `a-z`, `0-9`, or `-`, truncate to 50 characters, strip trailing `-`.
2. Directory name: `<TICKET-ID>-<slug>` if a Jira ticket is linked, otherwise just `<slug>`. When you split, add the board name: `<TICKET-ID>-<slug>-<board>`.
3. Full path: `<workspace>/<dir>/`. Directory names are unique in the workspace: if one exists, stop and say so.
4. `mkdir -p` that directory.

### Step 8 — Write `workitem.md`

One file per item, dependencies first.

```markdown
---
title: <title>
type: bug | feature | analysis | refactor | docs | style | perf | test | build | ci | chore | revert
source: issue | system_error | text
issue: <TICKET-ID> (<url>) | none
system_error: <error url> | none
worktree_needed: true | false
pr_mode: none | draft | ready
stage: <entry stage of the board>
board: <name>
approved: false
agent_status: ok
created: <today's date, YYYY-MM-DD>
parent: <parent workitem dir>   # only with --parent; omit the line otherwise
depends_on: <dir>, <dir>   # only for an item that needs others merged first; omit the line otherwise
---

# <title>

## Summary

<summary — one sentence>

## Description

<description — short paragraph>

## Type rationale

<one sentence from Step 3>

## Worktree

<true/false — one sentence rationale from Step 4>

## PR

<none/draft/ready — one sentence rationale from Step 5>

## Jira

<ticket key + URL, or "None — no ticket linked">

## Sentry

<issue URL, or "None">
```

If `--parent` was given, check that `<workspace>/<parent dir>/workitem.md` exists; if not, stop and write nothing. A child is an independent workitem with its own stage; the parent is not changed.

### Step 9 — Report

Tell the user each file path with its board, and one line each for type, worktree, PR mode, Jira link (or "none") and, when split, which item depends on which.
