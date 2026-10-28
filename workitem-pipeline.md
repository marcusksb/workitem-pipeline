# Workitem pipeline — design

## Goal

Move a workitem (feature request, implementation task, bug report, Sentry error) through stages. Each stage has one handler skill. The user approves before any code is written.

## Decisions

1. **State:** `stage:` in `workitem.md` frontmatter is the source of truth. Jira mirroring is a later add-on.
2. **Stages:** `backlog → analyze → ready_for_development → in_development → in_review → done`. "Blocked" is `agent_status: blocked: <reason>`, not a stage.
3. **Registry:** one `pipeline.json` in `~/.claude/skills/workitem-pipeline/`. It lists each stage with `handler`, `next` (allowed transitions) and `auto`.
4. **Transitions:** only `bin/workitem-move <workitem-dir> <stage>` changes `stage`. It checks the registry and the brief rule.
5. **Runner:** `/workitem-run`, one per repo, meant for `/loop 10m /workitem-run`. It only runs stages with `auto: true` (`backlog`, `analyze`, `in_development`). Pattern from `workitem-watch`: lock file `workitem-<dir>.running`, stale after 20 min, max 3 attempts, then `agent_status: blocked` and a push notification.
6. **Triage rules** (`backlog`). Every item goes to `analyze`; triage only sets `analyze_mode`:
   - concrete steps, named files/layers, no open questions → `validate`
   - bug or Sentry error → `root_cause`
   - feature request → `clarify`
   - too vague → stay in `backlog`, `agent_status: blocked: <what is missing>`
7. **Review brief:** `analyze` writes a `## Review brief` section: root cause or decisions, plan, risk, size, test plan, assumptions, issue and system error links. `bin/workitem-move` refuses `ready_for_development` if a part is missing or if `jira:` is `none`: an item is only ready for development with a linked Jira ticket.
8. **Approval:** `/workitem-approve <dir>` prints the brief, asks to confirm, sets `approved: true` and `approved_at`. Reject with a comment moves the item back to `analyze`. After approval it offers to start development.
9. **Intake:** manual. `workitem-create` accepts a Jira key, text, or a Sentry issue URL. New field `source: issue | system_error | text`.
10. **Existing skills:** `workitem-start` is the `in_development` handler and ends by moving to `in_review`. `workitem-watch` also moves an item to `done` when its PR is merged. Old `status` values migrate: `pending` → `ready_for_development` + `approved: true`, `in_progress` → `in_development`, `in-review` → `in_review`.

## Stage table

| Stage | Handler | auto | Moves on when |
|---|---|---|---|
| `backlog` | `workitem-triage` | yes | routed to `analyze`, or blocked if vague |
| `analyze` | `workitem-analyze` (modes: validate, root_cause, clarify) | yes | brief written → `ready_for_development` |
| `ready_for_development` | none, waits for `/workitem-approve` | no | `approved: true` |
| `in_development` | `workitem-start` | yes | PR opened → `in_review` |
| `in_review` | `workitem-watch` | yes | PR merged → `done` |
| `done` | none | no | terminal |

Allowed backward moves: `analyze → backlog`, `ready_for_development → analyze`.

## Frontmatter

```yaml
stage: backlog
approved: false
analyze_mode: validate | root_cause | clarify   # set by triage
source: issue | system_error | text
parent: <dir of the workitem this was split from>   # optional, set by workitem-create --parent
agent_status: working | ok | blocked: <reason>
```

## Configurable boards (planned)

The stage list above is the bundled default. A project can replace it, and can have several boards. Decisions are in `workitem-pipeline-ui.md` (12 to 27).

- **Files:** `<repo>/.workitems/pipeline.json` is the `default` board, extra boards are `<repo>/.workitems/boards/<name>.json`, items are `<repo>/.workitems/<dir>/workitem.md` with an optional `board: <name>` key. `<repo>/.claude/workitems/` is read when `.workitems/` does not exist. Without a board file the bundled `skills/workitem-pipeline/pipeline.json` applies.
- **Format:** `stages` is an ordered array, and the order is the column order.

```json
{
  "entry": "backlog",
  "stages": [
    { "name": "backlog", "handler": "workitem-triage", "auto": true, "next": ["analyze"] },
    { "name": "analyze", "handler": "workitem-analyze", "auto": true,
      "next": ["ready_for_development", "backlog"], "on_enter": { "approved": "false" } },
    { "name": "ready_for_development", "label": "Ready", "auto": false,
      "next": ["in_development", "analyze"], "on_enter": { "approved": "false" },
      "require": { "issue": "linked", "brief": "complete" },
      "gate": { "label": "Approve", "approve_to": "in_development", "reject_to": "analyze" } },
    { "name": "in_development", "handler": "workitem-start", "auto": true,
      "next": ["in_review"], "require": { "approved": "true" } },
    { "name": "in_review", "handler": "builtin:pr-review", "auto": true,
      "next": ["done"], "on_merge": "done" },
    { "name": "done", "auto": false, "next": [] }
  ]
}
```

- **Per stage:** `name`, optional `label`, one of `handler` (skill or `builtin:pr-review`) or `prompt` (plain string) or neither, `auto`, `next` (first entry is the default forward move), `on_enter`, `require`, optional `gate`, and `on_merge` on the PR column. At most one `builtin:pr-review` per board; any number of gates.
- **Queues and limits:** `limit` (a number) caps how many items of the board can be in a stage; `workitem-move` refuses entry to a full stage. A stage that is `auto` with neither `handler` nor `prompt` is a queue: the UI dispatcher moves its idle items to `next[0]`, oldest `created` first, whenever that stage has room. `once` starts a stage's handler at most once per entry, and `when: {key: value}` only for items whose frontmatter matches; together they suit a final stage that does a job and does not move on. `pr_status: true` shows the PR's approval, merge and CI chips on the cards of a stage that is not the `builtin:pr-review` one; the PR is read only, so no fixer starts and nothing moves.
- **Switching boards:** `bin/workitem-move <workitem.md> --board <name>` puts the item on that board's `entry`. Refused while the item has a live session.
- **Handler prompts** get the workitem path, the current stage and the allowed `next` stages appended by the launcher.
- **Handler files:** a handler name is looked up as `SKILL.md` in `.workitems/boards/<board>/skills/<name>/`, then `.workitems/skills/<name>/`, then the bundled `skills/<name>/`. The launcher reads it and passes the body as the prompt, so nothing depends on Claude's skill discovery. Projects override `workitem-start` (and `create`, `remove`) the same way.

## Components

New: `pipeline.json`, `bin/workitem-move`, `workitem-triage`, `workitem-analyze`, `workitem-approve`, `workitem-run`, `workitem-remove` (manual, deletes a workitem after confirmation; leaves branch and PR alone).
Changed: `workitem-create`, `workitem-start`, `workitem-watch`.

## Non-goals

Pull-based intake (Jira/Sentry polling), Jira status mirroring, cross-repo runner, central workitem store.

## Open points (decided during build)

- `analyze` is read-only for Sentry and the codebase. No production data access by default.
