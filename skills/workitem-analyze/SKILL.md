---
name: workitem-analyze
description: Handler for the `analyze` stage of the workitem pipeline. Reads one workitem, works in the mode set by triage (validate, root_cause, clarify), writes the `## Review brief` and moves it to `ready_for_development`. Use when asked to "analyze workitem X"; normally dispatched by `workitem-run`.
---

# Workitem Analyze

Handles ONE workitem in stage `analyze`. Input: the workitem directory name (or path).

Read-only: read the codebase, ADRs, the Jira ticket and Sentry (read tools only). Do not change code, do not query production data, do not create branches or PRs.

## Steps

1. Read `workitem.md`. Confirm the item is in the stage your prompt names as the current stage (`analyze` on the bundled board). Read `analyze_mode`. If a `## Rejection` section exists (added by `/workitem-approve`), address it first. If an `## Open questions` section exists, read the answers under each question and treat them as decisions. A question still without an answer stays open (see step 4).
2. Work by mode:
   - **`validate`** — the item already has a plan. Check every file, class and method it names against the current code. Fix small errors in the plan. Find risks the plan missed.
   - **`root_cause`** — find why it fails. For `source: system_error`, fetch the issue with the Sentry tools (stack trace, first/last seen, count). Follow the code path. Name the exact file and line. State how you know (code evidence), not how it might be.
   - **`clarify`** — list every open question about the feature. Answer each one you can from the code, the Jira ticket or the ADRs. For the rest, see step 4.
3. Write `## Review brief` at the end of the workitem (replace it if it exists). It MUST have these `###` sections, all non-empty, because `workitem-move` checks them:
   - `### Findings` — root cause (bug/Sentry), or the decisions and resolved questions (feature/validate).
   - `### Plan` — numbered steps, with files and layers touched. Follow the layer rules in the repo's `AGENTS.md`.
   - `### Risk` — migrations, auth or policy changes, base-class changes, API contract changes. Write `None found` if there are none.
   - `### Size` — `small`, `medium` or `large`, with one line why.
   - `### Test plan` — the specs to write or change. For a bug, the failing test comes first.
   - `### Assumptions` — what you assumed without confirmation. Write `None` if there are none.
   - `### Links` — the issue and the system error URL (from the frontmatter `issue:` and the source). Write `None` for a missing one.
4. If a `clarify` question needs a human, do not move the item. Write the questions under `## Open questions` (numbered) and set `agent_status: blocked: <N> open questions`. When a human is attached to your tmux session (the runner's prompt says so), ask the questions directly and wait. Write each answer under its question, set `agent_status: working`, and go on to step 3. Without an attached human, just stop; the user can also answer in the file and set `agent_status: ok`, and the next run continues from there.
5. If the frontmatter `issue:` is `none`, the item cannot be ready for development. Do not move it yet.
   - When a human is attached to your tmux session, ask: "No Jira ticket is linked. Create one from this workitem, or give me an existing key?" Only create a ticket after an explicit yes: use `mcp__claude_ai_Atlassian_Rovo__createJiraIssue` (title and description from the workitem; ask for the project if unknown). Then set `jira: <KEY> (<url>)` in the frontmatter, update `### Links` in the brief, and go on to step 6.
   - Without an attached human, set `agent_status: blocked: no Jira ticket linked, needs a ticket key`, call `PushNotification` with that reason, and stop. The user links or creates a ticket, sets `issue: <KEY> (<url>)` and `agent_status: ok`.
6. Otherwise move it: `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem-dir> <next>`, where `<next>` is the default next stage: the one marked `(default)` under "Allowed next stages" in your prompt (`ready_for_development` on the bundled board when run by hand). If it refuses, fix what it names and try once more. Do not edit `stage:` by hand.
7. Report one line: `<dir>: analyze -> ready_for_development (<size>)` or `<dir>: blocked (<reason>)`.

## Rules

- Prove claims with code you read. Label anything else as an assumption.
- If the item is not actionable at all (wrong repo, duplicate, already fixed), move it back with `workitem-move <dir> <earlier stage>` (the other allowed next stage that sends it back, `backlog` on the bundled board) and set `agent_status: blocked: <reason>`.
- Never remove the user's own text from the workitem.
- To split the work into smaller workitems, create each with `workitem-create --parent <this workitem's dir>` (plus `--repo` and `--issue` as needed). Do not write child `workitem.md` files by hand.
