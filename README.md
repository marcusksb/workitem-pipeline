# workitem-pipeline

A kanban pipeline for Claude Code agents. A workitem (a Jira ticket, a Sentry error, or a plain task) moves through stages. Each stage has a handler skill that runs as an interactive `claude` session in tmux. A human approves the plan before any code gets written.

```
backlog → analyze → ready_for_development → in_development → in_review → done
 triage    brief        human approves        implement + PR    watch PR
```

Two parts live in this repo:

- **The pipeline:** Claude Code skills plus a few small scripts.
- **Workload (`ui/`):** a local web UI that shows the boards, runs the handlers for you, watches PRs, and opens a browser terminal into any agent session.

Items live in a **workspace**, not in a repo: `~/.workitems-pipeline/workspaces/<name>/`. A workspace has one board per repository (a frontend and a backend board, say), so one feature can be split into linked items, one per repo.

## Requirements

- [Claude Code](https://claude.com/claude-code) (`claude` on your `PATH`)
- `tmux`, `python3`, `git`
- [`gh`](https://cli.github.com/), logged in (for the PR stage)
- For the UI: Rust (stable) and Node.js, plus `cargo install cargo-watch` for `npm run dev`

## Quick start

1. Install the skills (symlinks every `skills/*` folder into `~/.claude/skills`):
   ```sh
   ./install.sh
   ```
2. Build and start the UI:
   ```sh
   cd ui && npm install && npm run build
   ./backend/target/release/workload
   ```
3. Open the printed `http://127.0.0.1:7373/#token=…` URL. Add a workspace, then a board for each repository with **+ Board**, then use **New workitem**.

`./uninstall.sh` removes the symlinks and restores any folders `install.sh` backed up.

## How it works

- **A workitem** is `<workspace>/<dir>/workitem.md` with flat `key: value` frontmatter. `stage:` is the source of truth, and `board:` names its board, whose `repo` is where its code lives.
- **Links:** `depends_on: <dir>, ...` names items (on any board of the workspace) that must be merged first. A stage with `"require": {"depends_on": "merged"}` holds the item until they are.
- **Stage changes** go through `skills/workitem-pipeline/bin/workitem-move` only. It enforces the board's rules: allowed transitions, `require` checks (a linked issue, a complete review brief), stage limits and `on_enter` fields. Never edit `stage:` by hand.
- **Agent status** is `agent_status: working | ok | blocked: <reason>`. "Blocked" is a flag, not a stage. A session that waits for your input is marked blocked through `hooks.json`.
- **Sessions** are tmux sessions named `wi-<dir>`. The session is the lock: one item, one agent. Attach with `tmux attach -t wi-<dir>` or from the UI.

### Stages (bundled board)

| Stage | Handler | What happens |
|---|---|---|
| `backlog` | `workitem-triage` | Picks an analyze mode (`validate`, `root_cause`, `clarify`) or blocks the item as too vague |
| `analyze` | `workitem-analyze` | Writes the `## Review brief`: findings, plan, risk, size, test plan, assumptions, links |
| `ready_for_development` | gate | You approve (→ `in_development`) or reject with a comment (→ `analyze`) |
| `in_development` | `workitem-start` | Sets up the worktree and branch, implements, opens a PR |
| `in_review` | `watch/pr-review` | Reacts to what the workspace's `watch/pr-status` read: fixes conflicts, red CI and review comments, moves merged PRs to `done` |
| `done` | none | Final |

### Custom boards

Each board is `<workspace>/boards/<name>.json` with a `"repo"`; **+ Board** starts it as a copy of the bundled `skills/workitem-pipeline/pipeline.json`. Edit the file to change it. Stages can use a skill, an inline `prompt`, a `gate`, a `limit`, a `require`, or a `watch` script that runs on a timer. Handler skills and watch scripts can be overridden per board (`boards/<name>/skills/`) or per workspace (`skills/`). Details: [`workitem-pipeline.md`](workitem-pipeline.md).

## Skills

| Skill | Use |
|---|---|
| `workitem-create` | Create a workitem from a Jira key, a Sentry URL or text; one linked item per repo when the task spans several |
| `workitem-triage`, `workitem-analyze` | Handlers for `backlog` and `analyze` |
| `workitem-approve` | Review the brief and approve or reject (terminal-only flow; the UI has a button) |
| `workitem-start` | Implement an approved item and open the PR |
| `workitem-remove` | Delete a workitem and its worktree |

The UI runs the pipeline: it starts the handlers and watches the PRs. The skills get the item's path from it.

## Workload UI

A Rust (axum) backend with a React frontend embedded in one binary. It binds to `127.0.0.1` only and guards every request with a token from `~/.workitems-pipeline/config.toml`.

- Kanban per workspace and board, with a chip on cards that wait for another item, with a detail panel: brief, files, PR conversation, actions, and a live terminal (xterm.js) into the item's tmux session
- Runs handlers automatically, retries up to 3 times, then blocks the item
- Watches PRs: CI, conflicts, review comments, merge
- Browser notification when an item becomes blocked

Development (from `ui/`):

```sh
npm install
npm run dev     # frontend watch build + cargo watch, on port 7373
npm test        # cargo test + vitest
npm run build   # release binary at backend/target/release/workload
```

Settings come from env vars: `WORKITEM_UI_HOME` (default `~/.workitems-pipeline`), `WORKITEM_UI_PORT`, `WORKITEM_UI_TMUX_SOCKET`, `WORKITEM_UI_SKILLS`, `WORKITEM_UI_CLAUDE`, `WORKITEM_UI_GH`, `WORKITEM_UI_DISPATCH=off`. Design and decisions: [`workitem-pipeline-ui.md`](workitem-pipeline-ui.md).

## Tests

```sh
python3 -m unittest test_workitem_move test_workitem_run_item test_pr_review
cd ui && npm test
```

Backend integration tests run against a real tmux server on an isolated socket and a stub `claude`.

## Migrating repos to a workspace

Items that live in a repo (`.workitems/` or `.claude/workitems/`) are no longer read. Move each repo into a workspace, once per repo; two repos can share one workspace:

```sh
skills/workitem-pipeline/bin/workitem-migrate --to-workspace <workspace> --board <board> <repo>
```

The repo's default board becomes `<board>`, its other boards `<board>-<name>`. End the repo's `wi-*` sessions first. The first start of the UI takes the token and port over from `~/.config/workitem-ui/config.toml`.
