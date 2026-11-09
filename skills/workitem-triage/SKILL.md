---
name: workitem-triage
description: Handler for the `backlog` stage of the workitem pipeline. Reads one backlog workitem, picks the analyze mode (validate, root_cause, clarify) and moves it to `analyze`, or blocks it if it is too vague. Use when asked to "triage workitem X"; normally started by the UI's dispatcher.
---

# Workitem Triage

Handles ONE workitem in stage `backlog`. Input: the workitem file your prompt names ("Workitem file:"), else `$WORKITEM_FILE`, else the path given. If there is none, ask for the path and stop.

Do not analyze the problem here. Triage only reads the item and decides the mode.

## Steps

1. Read the workitem file. Confirm the item is in the stage your prompt names as the current stage (`backlog` on the bundled board). If not, stop.
2. Pick the mode. Use the first rule that matches:
   1. **`validate`** — the item has concrete steps, names files or layers, and has no open questions.
   2. **`root_cause`** — `type: bug`, or `source: system_error`.
   3. **`clarify`** — a feature request or an idea (`type: feature`, or an ask without a plan; this includes `refactor`, `perf` and the other non-bug types).
   4. **Too vague** — you cannot tell what is wanted, or the description is empty. Do not move it. Set `agent_status: blocked: <what is missing, one line>` in the frontmatter and stop.
3. Set `analyze_mode: <mode>` in the frontmatter (add the line if it is not there).
4. Move it: `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem file> <next>`, where `<next>` is the default next stage: the one marked `(default)` under "Allowed next stages" in your prompt (`analyze` on the bundled board when run by hand).
5. Report one line: `<dir>: backlog -> analyze (<mode>)` or `<dir>: blocked (<reason>)`.

## Rules

- Never guess. A bug report without enough detail is "too vague", not `root_cause`.
- Do not edit the body of the workitem. Only frontmatter fields.
- If `bin/workitem-move` refuses, report its message. Do not edit `stage:` by hand.
