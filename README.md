# workitem-pipeline

A kanban pipeline for Claude Code agents. A workitem (a Jira ticket, a Sentry error, or a plain task) moves through stages. Each stage has a handler skill that runs as an interactive `claude` session in tmux. A human approves the plan before any code gets written.

```
backlog → analyze → ready_for_development → in_development → in_review → done
 triage    brief        human approves        implement + PR    watch PR
```

Two parts live in this repo:

- **The pipeline:** Claude Code skills plus a few small scripts. It works on its own, from the terminal.
- **Workload (`ui/`):** a local web UI that shows the board, runs the handlers for you, watches PRs, and opens a browser terminal into any agent session.

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
2. In a repo you want to work on, create a workitem from Claude Code:
   ```
   /workitem-create PROJ-123
   ```
3. Run the board, either with the UI:
   ```sh
   cd ui && npm install && npm run build
   ./backend/target/release/workload
   ```
   Open the printed `http://127.0.0.1:7373/#token=…` URL and add the repo as a project. Or without the UI, from Claude Code in that repo:
   ```
   /loop 10m /workitem-run
   ```

`./uninstall.sh` removes the symlinks and restores any folders `install.sh` backed up.

## How it works

- **A workitem** is `<repo>/.workitems/<dir>/workitem.md` with flat `key: value` frontmatter. `stage:` is the source of truth.
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
| `in_review` | `watch/pr-review` | Polls the PR, fixes red CI, conflicts and review comments, moves merged PRs to `done` |
| `done` | none | Final |

### Custom boards

A project can replace the bundled board (`skills/workitem-pipeline/pipeline.json`) with its own `.workitems/boards/default.json`, and add more boards as `.workitems/boards/<name>.json`. An item picks its board with `board: <name>` in its frontmatter. Stages can use a skill, an inline `prompt`, a `gate`, a `limit`, or a `watch` script that runs on a timer. Handler skills and watch scripts can be overridden per board or per project. Details: [`workitem-pipeline.md`](workitem-pipeline.md).

## Skills

| Skill | Use |
|---|---|
| `workitem-create` | Create a workitem from a Jira key, a Sentry URL or text |
| `workitem-triage`, `workitem-analyze` | Handlers for `backlog` and `analyze` |
| `workitem-approve` | Review the brief and approve or reject (terminal-only flow; the UI has a button) |
| `workitem-start` | Implement an approved item and open the PR |
| `workitem-run` | One pass of the pipeline without the UI, meant for `/loop` |
| `workitem-watch` | Watch PRs without the UI, meant for `/loop` |
| `workitem-remove` | Delete a workitem and its worktree |

Use either the UI or the `/loop` skills on a repo, not both: two runners race each other.

## Workload UI

A Rust (axum) backend with a React frontend embedded in one binary. It binds to `127.0.0.1` only and guards every request with a token from `~/.config/workitem-ui/config.toml`.

- Kanban per project and board, with a detail panel: brief, files, PR conversation, actions, and a live terminal (xterm.js) into the item's tmux session
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

Settings come from env vars: `WORKITEM_UI_PORT`, `WORKITEM_UI_CONFIG`, `WORKITEM_UI_TMUX_SOCKET`, `WORKITEM_UI_SKILLS`, `WORKITEM_UI_CLAUDE`, `WORKITEM_UI_GH`, `WORKITEM_UI_DISPATCH=off`. Design and decisions: [`workitem-pipeline-ui.md`](workitem-pipeline-ui.md).

## Tests

```sh
python3 -m unittest test_workitem_move test_workitem_run_item test_pr_review
cd ui && npm test
```

Backend integration tests run against a real tmux server on an isolated socket and a stub `claude`.

## Migrating older repos

Repos that keep items in `.claude/workitems/` still work. To move them to `.workitems/`:

```sh
skills/workitem-pipeline/bin/workitem-migrate
```
