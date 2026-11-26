---
name: workitem-analyze
description: Handler for the `analyze` stage of the workitem pipeline. Reads one workitem, picks its analyze mode (validate, root_cause, clarify) unless one is set, works in that mode, writes the `## Review brief` and moves it to `ready_for_development`. Use when asked to "analyze workitem X"; normally started by the UI's dispatcher.
---

# Workitem Analyze

Handles ONE workitem in stage `analyze`. Input: the workitem file your prompt names ("Workitem file:"), else `$WORKITEM_FILE`, else the path given. Its code is in the repo your prompt names (`$WORKITEM_REPO`).

Read-only: read the codebase, ADRs, the linked issue (`python3 ~/.claude/skills/workitem-pipeline/bin/workitem-issue read <workitem file>`) and Sentry (read tools only). Do not change code, do not query production data, do not create branches or PRs.

## Steps

1. Read `workitem.md`. Confirm the item is in the stage your prompt names as the current stage (`analyze` on the bundled board). Read `analyze_mode`. If it is missing, pick it now with the first rule that matches and write `analyze_mode: <mode>` to the frontmatter:
   1. **`validate`** — the item has concrete steps, names files or layers, and has no open questions.
   2. **`root_cause`** — `type: bug`, or `source: system_error`.
   3. **`clarify`** — a feature request or an idea (`type: feature`, or an ask without a plan; this includes `refactor`, `perf` and the other non-bug types).
   4. **Too vague** — you cannot tell what is wanted, or the description is empty. A bug report without enough detail is too vague, not `root_cause`. Set `agent_status: blocked: <what is missing, one line>` and stop.

   If a `## Rejection` section exists (added by `/workitem-approve`), address it first. If an `## Open questions` section exists, read the answers under each question and treat them as decisions. A question still without an answer stays open (see step 4).
2. Work by mode:
   - **`validate`** — the item already has a plan. Check every file, class and method it names against the current code. Fix small errors in the plan. Find risks the plan missed.
   - **`root_cause`** — find why it fails. For `source: system_error`, fetch the issue with the Sentry tools (stack trace, first/last seen, count). Follow the code path. Name the exact file and line. State how you know (code evidence), not how it might be.
   - **`clarify`** — list every open question about the feature. Answer each one you can from the code, the linked issue or the ADRs. For the rest, see step 4.
3. Write `## Review brief` at the end of the workitem, before `## Follow-ups` if that exists. If the brief exists, replace only that section, up to the next `##` heading. It MUST have these `###` sections, all non-empty, because `workitem-move` checks them:
   - `### Findings` — root cause (bug/Sentry), or the decisions and resolved questions (feature/validate).
   - `### Plan` — numbered steps, with files and layers touched. Follow the layer rules in the repo's `AGENTS.md`.
   - `### Risk` — migrations, auth or policy changes, base-class changes, API contract changes. Write `None found` if there are none.
   - `### Size` — `small`, `medium` or `large`, with one line why.
   - `### Test plan` — the specs to write or change. For a bug, the failing test comes first.
   - `### Assumptions` — what you assumed without confirmation. Write `None` if there are none.
   - `### Links` — the issue and the system error URL (from the frontmatter `issue:` and the source). Write `None` for a missing one.
4. If a `clarify` question needs a human, do not move the item. Write the questions under `## Open questions` (numbered) and set `agent_status: blocked: <N> open questions`. When a human is attached to your tmux session (the runner's prompt says so), ask the questions directly and wait. Write each answer under its question, set `agent_status: working`, and go on to step 3. Without an attached human, just stop; the user can also answer in the file and set `agent_status: ok`, and the next run continues from there.
5. If the frontmatter `issue:` is `none`, read the item's board file (`<workspace>/boards/<board>.json`). Only when the stage you move to in step 6 has `issue: linked` under `require` is a missing issue a problem; otherwise go on to step 6 and do not offer to create one. When it is required, the item cannot be ready for development. Do not move it yet.
   - When a human is attached to your tmux session, ask: "No issue is linked. Create one from this workitem, or give me an existing one?" Only create an issue after an explicit yes: write the description to a file and run `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-issue create <workitem file> --title <title> --body-file <file>`; it files the issue in the board's `issue_project` and sets `issue:` itself (if it says the board needs `issue_project`, ask for it and tell the user to add it to the board file). For an existing one, run `workitem-issue link <workitem file> <ref>`; it sets `issue:` itself. Then update `### Links` in the brief and go on to step 6.
   - Without an attached human, set `agent_status: blocked: no issue linked, needs an issue`, call `PushNotification` with that reason, and stop. The user links or creates an issue, sets `issue:` and `agent_status: ok`.
6. Otherwise move it: `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem file> <next>`, where `<next>` is the default next stage: the one marked `(default)` under "Allowed next stages" in your prompt (`ready_for_development` on the bundled board when run by hand). If it refuses, fix what it names and try once more. Do not edit `stage:` by hand.
7. Report one line: `<dir>: analyze -> ready_for_development (<size>)` or `<dir>: blocked (<reason>)`.

## Rules

- Prove claims with code you read. Label anything else as an assumption.
- If the item is not actionable at all (wrong repo, duplicate, already fixed), do not move it. Set `agent_status: blocked: <reason>` and stop.
- Never remove the user's own text from the workitem.
- If something has to happen after the item is done (a deploy step, a cleanup after a rollout, telling someone), add it as a `- [ ]` line under `## Follow-ups`, the last section of the file; create the section if it is missing. Do not work on follow-ups. Only append: never delete, reword or untick a line, and skip one that is already listed.
- To split the work into smaller workitems, create each with `workitem-create --parent <this workitem's dir> --workspace <workspace> --board <board>` (plus `--issue` as needed). Do not write child `workitem.md` files by hand.
