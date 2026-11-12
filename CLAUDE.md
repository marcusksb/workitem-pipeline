# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Two parts live here: the workitem pipeline itself (Claude skills plus one Python script) and `ui/`, Workload, a local web UI for it (Rust backend, React frontend). Design docs: `workitem-pipeline.md` (pipeline) and `workitem-pipeline-ui.md` (UI, with the numbered decisions). Read the matching doc before changing behavior.

## Commands

Pipeline:
- `./install.sh` symlinks every `skills/*` folder into `~/.claude/skills` (a real folder is moved to `backup/` first); `./uninstall.sh` reverts it.
- `python3 -m unittest test_workitem_move` tests `skills/workitem-pipeline/bin/workitem-move`. Single test: `python3 -m unittest test_workitem_move.WorkitemMoveTest.<name>`.
- `skills/workitem-pipeline/bin/workitem-migrate --to-workspace <name> --board <board> <repo>` moves a repo's `.workitems` (or `.claude/workitems`) into a workspace.

UI (run from `ui/`):
- `npm install` then `npm run dev`: `vite build --watch` plus `cargo watch` (needs `cargo install cargo-watch`) on one port, 7373. The URL with `#token=` is printed on start.
- `npm test`: `cargo test` and vitest. One Rust test: `cargo test --manifest-path backend/Cargo.toml <name>`; one file: `--test pipeline`.
- `cd frontend && npm run build && npm run e2e`: Playwright smoke test. It needs `cargo build` first (it runs `backend/target/debug/workload`) and Chromium (`npx playwright install chromium`).
- `npm run build`: release binary at `backend/target/release/workload`.
- The backend embeds `frontend/dist` at compile time (`rust-embed`), so build the frontend before `cargo build` on a fresh checkout.

## Architecture

**Pipeline.** Items live in workspaces, not repos (decisions 44 to 53): `~/.workitems-pipeline/workspaces/<name>/` (root overridable with `WORKITEM_UI_HOME`) holds `<dir>/workitem.md`, `boards/<board>.json`, `boards/<board>/{skills,watch,learnings.md}`, `skills/`, `watch/`, `plans/` and `workspace.toml` (`agent`). Every board names its `repo`, except one whose items change no code (decision 54: no worktrees, no PRs, sessions start in the item folder with every repo added); every item names its `board:`, and its repo is its board's. There is no default board and no fallback to the bundled `pipeline.json`, which is only the template for a new board. A workitem has flat `key: value` frontmatter. `stage:` is the source of truth. Only `skills/workitem-pipeline/bin/workitem-move` changes `stage`; it enforces `pipeline.json` (allowed transitions, `require` including `issue: linked` and `brief: complete`, and `on_enter`). Never edit `stage:` by hand or reimplement those rules. `pipeline.json` is an ordered `stages` array (the order is the column order) plus `entry`; each stage names its handler skill and whether it is `auto`. Roles are keys, not stage names: `gate` (a human approves or rejects there), `on_merge` and the `pr-review` watcher (the PR stage). The API puts `gate`, `pr_watch` and `develops` on each item, so the frontend knows no stage names. `workitem-move <item> --board <name>` is the only way to change `board:`, and puts the item on that board's `entry`. `depends_on: <dir>, ...` links items across boards; `require: {depends_on: merged}` holds an item until they are merged (`pr_state: merged`, or in their board's `on_merge` stage). `require: {children: merged}` does the same for the items whose `parent:` is this one (and refuses an item without any). `workitem-move` takes a path only; nothing finds items through the current directory. The backend keeps the last valid file per workspace and board (`AppState::pipeline`) and serves them on `/api/pipelines`. `agent_status` (`working`, `waiting: <what>`, `ok`, `blocked: <reason>`) is how agents and humans signal each other; `waiting` means idle on purpose (CI), and a waiting item without a session is reset to `ok` like a `working` one. A handler runs as interactive `claude` in tmux session `wi-<dir>`; the session is the lock, and `hooks.json` plus `bin/workitem-notify` mark a session that waits for input as blocked (a `waiting` one only on a permission or elicitation prompt).

UI backend, frontend and testing notes are in `ui/CLAUDE.md`.
