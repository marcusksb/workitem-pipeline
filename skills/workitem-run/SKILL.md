---
name: workitem-run
description: One pass of the workitem pipeline for the current repo. Finds workitems in stages marked `auto` in pipeline.json (backlog, analyze, in_development), and runs each stage's handler in its own tmux session, one per item, so you can attach when one is blocked. Meant to run on a loop, e.g. `/loop 10m /workitem-run`. Use when asked to "run the pipeline", "process the workitems".
---

# Workitem Run

One pass = discover, dispatch, report. The loop is the retry scheduler. Same pattern as `workitem-watch`.

## State

`<scratchpad>/workitem-run.json`, keyed by workitem directory name:
```json
{ "SHAPE-1-foo": { "stage": "analyze", "attempts": 1, "max": 3, "last_notified_status": "ok" } }
```
Read it at the start, write it at the end. `attempts` resets to 0 when the item's stage changes.

**Session:** a handler is in flight while the tmux session `wi-<dir>` exists. It is the lock. The session runs an interactive `claude`, so a blocked item stays alive there until you attach and answer: `tmux attach -t wi-<dir>`, detach with `ctrl-b d`.

## Steps

### 0. Notify
For every workitem, compare `agent_status` with `last_notified_status` in the state file. New `blocked: <reason>` → `PushNotification` with the reason, then store it. If session `wi-<dir>` is alive, add `attach: tmux attach -t wi-<dir>` to the message. Changed away from blocked → just store it.

### 1. Reap
`tmux list-sessions -F '#{session_name} #{pane_dead}'`, only names starting with `wi-`. For each, read `agent_status` of its workitem (`grep -m1 '^agent_status:'` on `workitem.md`):
- `ok` → the handler is done (`workitem-move` sets `ok` on every move). `tmux kill-session -t wi-<dir>`.
- `blocked…` → leave it. It waits for you.
- `working` and pane alive → leave it. Still running.
- pane dead (claude exited or crashed) and status not `ok` → `tmux kill-session`, set `agent_status: ok` so the next pass retries. The attempt already counted.

### 2. Discover
`python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move --list --auto` prints `dir<TAB>stage<TAB>handler<TAB>agent_status`. Nothing listed → "Pipeline idle." and stop.

### 3. Filter
Skip an item when:
- `agent_status` starts with `blocked` (a human has to clear it: fix the item and set `agent_status: ok`).
- Session `wi-<dir>` exists.
- `attempts >= max`. Set `agent_status: blocked: max attempts reached in stage <stage>`, send `PushNotification` now, and skip.

### 4. Dispatch
- Handler `builtin:pr-review` (the PR stage, `in_review` in the bundled board): if any such item exists, run the `workitem-watch` skill once in this session. It handles all its PRs itself. Do not dispatch it per item.
- Any other handler (`workitem-triage`, `workitem-analyze`, `workitem-start`): for each remaining item, increment `attempts`, set `agent_status: working`, and start a session:
  Run `tmux new-session -d -s wi-<dir> -c <repo-root> -e WORKITEM=<dir> -e WORKITEM_REPO=<repo-root> "claude --settings ~/.claude/skills/workitem-pipeline/hooks.json \"<prompt>\""`, then `tmux set-option -t wi-<dir> remain-on-exit on`. The settings file adds a Notification hook (`bin/workitem-notify`): when the session waits for input while `agent_status` is `working`, it sets `blocked` and shows a desktop notification, so a forgotten flag or a permission prompt is still caught. Start all items in one message. The prompt must stand alone and contain no double quotes:
  ```
  Run the <handler> skill for workitem <dir> in repo <repo-root>.
  Only change the workitem with the skill's own steps, with
  python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move,
  and by editing its agent_status line.
  Do not change code.
  agent_status is the agent_status: line in the frontmatter of the workitem's workitem.md.
  There is no tool for it: set it by editing that line in the file.
  A human may attach to this tmux session (wi-<dir>). If you need input,
  set agent_status: blocked: <reason>, ask the question here and wait.
  After the answer, set agent_status: working and continue.
  When you are done, make sure agent_status is ok (a stage move does this).
  If you stop blocked without a question for the human, your last action is
  tmux kill-session -t wi-<dir>.
  ```
  For `workitem-start`, drop the line `Do not change code.` and the `Only change the workitem…` sentence, and add: `The workitem is approved. If it is already in_development, resume it as is and do not ask whether to resume or restart.`

  Nothing to wait for: the next pass reaps finished sessions (step 1). An item that ends in the same stage with `agent_status: ok` counts as a used attempt.

### 5. Report
```
## Workitem pipeline
SHAPE-1-foo     backlog -> analyze        triage started
SHAPE-2-bar     analyze                   blocked: 2 open questions
SHAPE-3-baz     analyze                   attempt 2/3
```

## Rules

- Do not attach to or type into `wi-*` sessions from the runner. Only you answer there.
- Never dispatch for stages with `auto: false` (the gate stage, `ready_for_development` in the bundled board). That one waits for the user.
- Never move stages by editing `stage:`. Only `workitem-move`.
- Keep the orchestrator small: read only the list output and `agent_status`, not workitem bodies.
