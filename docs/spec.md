# Workitem pipeline: spec

How the system works today. The history and rationale are in [`decisions.md`](decisions.md); "(n)" below names a decision there. A behaviour change appends a decision to that log and updates this file to match.

## Parts

- **Pipeline** (`skills/`): the handler skills and `skills/workitem-pipeline/` (board template `pipeline.json`, `bin/`, `watch/`, `hooks.json`, agent glue in `agents/`).
- **Workload** (`ui/`): Rust backend (axum) with the React frontend embedded. It runs the dispatcher and watchers, launches agent sessions in tmux and serves a browser terminal.

The workitem files are the source of truth; tmux only tells whether a session is alive.

## Files

The root is `~/.workitems-pipeline/` (`WORKITEM_UI_HOME`). It holds `config.toml` (token, port, default `agent`; mode 0600), `paused.json`, `watch.json`, `mcp/` and `workspaces/<name>/`. Every folder in `workspaces/` named `[A-Za-z0-9_-]+` is a workspace (44). A workspace is personal (not in git; repo paths are absolute) and holds:

- `<dir>/workitem.md`: one item per folder, flat. The folder may hold other files (shown in the Files tab) and `questions.json` (61).
- `boards/<board>.json`, plus `boards/<board>/{skills/,watch/,learnings.md}`.
- `skills/`, `watch/`: workspace-level overrides.
- `plans/<id>.md`: planning session records.
- `workspace.toml` (optional): `agent`, `issue_tracker`, `github_account`, `bkt_context`, `[[watch]]`. An invalid file keeps its last valid version and shows an error in the header.

Handler skills and watch scripts resolve board first, then workspace, then bundled (`locate.rs`), so the UI's stage view shows the file that runs. Handler names are slugs (`[a-z0-9-]+`) resolved to `<name>/SKILL.md`.

## Boards

A board file is JSON. Top-level keys:

- `repo`: absolute path (`~/` expanded) to an existing directory. Left out only for a board whose items change no code (54): no worktrees, no PRs, sessions start in the item folder.
- `entry`: the stage new items start in. `stages`: ordered array, the column order.
- `brief_sections`: the `## Review brief` subsections that `brief: complete` requires.
- Optional: `label` (tab title), `order` (sort fields, 42), `watch` (board watchers), `issue_project` (64), `reviewers` (`org/team` added when a PR is marked ready, 70).

Stage keys: `name`, `label`, one of `handler` (skill name) or `prompt` (inline string) or neither, `auto`, `next` (first entry is the default forward move), `on_enter` (frontmatter values set on entry), `require`, `gate: {label, approve_to, reject_to}`, `on_merge` (on the PR stage: where a merged PR goes), `limit`, `once`, `when`, `model` (60), `learnings`, `order`, `watch`, `issue_state` (72), `actions` (84), `pr_status` (older files).

The backend and `workitem-move` know no stage names; roles are keys (16). The API puts `gate`, `pr_watch`, `develops` and `actions` on each item. An invalid file is rejected whole: the last valid version stays in use (`AppState::pipeline`) and the board shows the error (20). There is no default board and no fallback to the bundled file; *Add board* copies `pipeline.json` with `repo` set and fills in the `issue_state` defaults of the workspace's tracker. The board settings dialog edits `label`, `entry`, `order`, `issue_project`, `reviewers` (73); the stage dialog edits `model` and `require.issue` (76); everything else is a file edit. Deleting a board removes its items first (77).

The bundled board: `analyze` (auto, `workitem-analyze`) → `ready_for_development` (gate, requires `issue: linked` and `brief: complete`) → `in_development` (auto, `workitem-start`, requires `approved: true`) → `in_review` (PR stage, `pr-review` watcher, `on_merge: done`, actions *Rebase onto base* and *Merge PR*) → `done`.

A stage's `actions: [{label, skill | script, when, confirm, args}]` are buttons in the detail panel for its items whose frontmatter has every `when` value (raw values, as `run-items` compares). The label is the id, unique within the stage; exactly one of `skill` and `script`; `args` only with a `script`. `POST .../action/{id}` checks stage and `when` again (409 otherwise). A `skill` starts the item's `wi-<dir>` session with that skill, like `/run`. A `script` is resolved in `actions/` folders (board, workspace, bundled) and runs once like a column watcher of the stage, with the one item as a JSON object on stdin and a 20 min limit; a failure is a 409 carrying its stderr. Then `pr-status` and the stage's watchers run on the next pass. The bundled `actions/pr merge|rebase` runs `workitem-pr` on the item; the bundled board's Merge needs `pr_state: open`, `pr_approval: approved`, `pr_ci: passing`, `pr_merge: mergeable`, `pr_draft: false`; Rebase needs `pr_state: open`. Boards made before this have neither until they are added (84).

## Workitems

Flat `key: value` frontmatter, then the body (`## Description`, `## Review brief`, `## Follow-ups` last). Keys:

- `title`, `type` (commitizen type or `analysis`), `source`, `created`, `created_by`, `worktree_needed`, `pr_mode`.
- `board`: required. `stage`: the source of truth, changed only by `workitem-move`.
- `approved`, `approved_at`; `analyze_mode` (`validate`, `root_cause`, `clarify`; set by `workitem-analyze` when missing).
- `agent_status`: `working`, `waiting: <what>` (idle on purpose, e.g. CI), `ok`, `blocked: <reason>`. How agents and humans signal each other.
- `session`, `handler_attempts` (dropped on every stage change).
- `issue` (`<ref> (<url>)` or `none`), `issue_status`, `issue_status_category` (from the `issue-status` watcher).
- `pr` (URL), and the `pr_*` facts written by `pr-status` (40).
- `parent`, `depends_on: <dir>, ...` (items of the same workspace, any board), `answers_children` (61).

Open follow-ups are unticked `- [ ]` lines under `## Follow-ups`. Agents only append there and never work on them (P11).

## workitem-move

`bin/workitem-move <path to workitem.md> <stage>` is the only way to change `stage:`; `--board <name>` is the only way to change `board:` (lands on that board's `entry`, refused while the item has a live session). It takes a path only; nothing finds items through the current directory (50). On a move it checks that the target is in the current stage's `next` (an item whose stage is not on its board can only move to `entry`), the target's `limit`, and its `require`: `issue: linked`, `brief: complete`, `depends_on: merged` (every dependency has `pr_state: merged` or sits in its board's `on_merge` stage; a missing folder counts as merged), `children: merged` (same over items whose `parent:` is this one; refuses an item with none), any other key compares a frontmatter value. Then it applies `on_enter`, drops `handler_attempts`, sets `agent_status: ok`, and runs `workitem-issue set-state` with the stage's `issue_state` (best effort: a failure warns and the move stands).

## Issue trackers

`issue_tracker = "jira" | "github"` per workspace (default `jira`). Skills and `workitem-move` reach the tracker only through `bin/workitem-issue` (module `bin/workitem_tracker.py`): `parse`, `link`, `read`, `search`, `create`, `comment`, `set-state`, `pr-line` (66). GitHub refs are `owner/repo#123`; folders are `<repo>-<n>-<slug>`. A bare `#123` uses the board's `issue_project`. GitHub states are only `assigned` and `closed`; the PR body's `Closes owner/repo#123` closes it on merge. Jira uses jira-cli's credentials, global (69). In a `depends_on` chain only the last item moves the shared issue.

`github_account` per workspace: every session, watcher and backend `gh` call gets `GH_TOKEN` from `gh auth token --user <login>`. If that fails the workspace starts nothing and shows an alert; it never falls back to another account (68).

PR hosts: everything that touches a PR goes through `bin/workitem_pr.py` (83): the watchers import it, skills and the backend run its CLI `bin/workitem-pr <verb> <workitem>` (`conversation [--json]`, `checks`, `failed-log`, `base`, `state`, `comment`, `reply <id>`, `ready`, `add-reviewer`, `merge`, `rebase`). Its one link parser picks the provider from the `pr:` host (82): `bitbucket.org/<workspace>/<slug>/pull-requests/<id>` is Bitbucket Cloud, `<owner>/<repo>/pull/<n>` on any other host GitHub; host case, port, query, fragment and suffixes are ignored. GitHub goes through `gh` with the workspace's token; Bitbucket through the `bkt` CLI, whose credentials `bkt_context` in `workspace.toml` selects (or `WORKITEM_BKT_CONTEXT`) — a changed context is probed when it is saved (`bkt context list` names it, then `bkt api /user` succeeds), fail-closed like the account. Both answer in GitHub's vocabulary. Cloud has no changes-requested verdict, so `pr_approval` is only approved | none, and every build status counts as required. Merge is a squash merge; a GitHub rebase is `gh pr update-branch --rebase`, a Bitbucket one runs locally in a temporary detached worktree of the board repo (fetch base, rebase, `git push --force-with-lease`, abort and clean up on failure), never in the board repo's own checkout. Bitbucket Issues are out of scope: `issue_tracker` stays `jira | github`.

## Sessions

Agents run interactively in tmux. The session is the lock: an item has at most one, `wi-<dir>`. Names are global; each session is tagged `@workitem_workspace` when created. Kinds:

- **Item sessions** (`wi-<dir>`): stage handlers, PR fixers, Start session, Discuss brief. They start in the item's worktree (`<workspace>/.worktrees/<repo>-<dir>`, decision 80) when git has one, else in the repo (in the item folder on a board without repo), with `--add-dir` for the workspace (and the repo in a worktree). Env: `WORKITEM`, `WORKITEM_FILE`, `WORKITEM_WORKSPACE`, `WORKITEM_REPO`, and `WORKITEM_WORKTREE` on a board with a repo. A launcher var a session does not set is unset in it, never inherited from the tmux server (83). A stage's `model` becomes `--model`.
- **Create sessions** (`wi-new-<id>`): run `workitem-create` in the workspace folder with every board's repo added. Shown as toasts (75); items they write carry `created_by:`. A dead one is not reaped (its pane keeps the crash output); × on its *Ended* toast kills it (`DELETE /api/sessions/{workspace}/{name}`).
- **Planning sessions** (`wi-plan-<id>`): a conversation with no item, in the workspace folder. The record `plans/<id>.md` (written only through `bin/workitem-plan-note`) is the source of truth; `bin/workitem-plan-guard` refuses edits outside the workspace folder and in its `.worktrees/`. Resume continues the recorded agent session (36, 49).

Prompts: the launcher reads the handler's `SKILL.md` (front matter stripped) or the inline `prompt`, appends the stage block (item path, current stage, allowed next stages) and, for `develops` stages, `learnings: true` stages and fixers, the board's `learnings.md`. The prompt is one argv element, passed through a private temp file because tmux caps command length; never through a shell.

Agents (56): `agent = { kind, command }` in `config.toml` (default) or `workspace.toml` (override); kinds `claude`, `opencode`, `pi`, `omp`. `launch::agent_args` maps folders, model and MCP config per kind; the glue in `agents/<kind>/` does for the others what Claude's hooks do.

Status: `hooks.json` and `bin/workitem-notify` set `blocked` when a session waits for input (for a `waiting` item only on a permission or elicitation prompt). The dispatcher resets a `working` or `waiting` item without a live session to `ok` and reaps dead sessions.

Questions to the parent (61): the backend serves an MCP server per item at `/api/mcp/<workspace>/<dir>` (`ask_parent`, `wait_for_answer`, `list_child_questions`, `answer_child`). A child asks only a parent with `answers_children: true`; the child is `blocked: asking parent: <question>` until answered.

## Dispatcher and watchers

`dispatch.rs` runs every 5s and on every UI action: session housekeeping, then due watchers. A watcher (`{every, command, args}`) is a script run without a shell (one at a time, killed after 10 minutes) with `WORKITEM_*` env vars (no `WORKITEM_REPO` without a repo, 83) and items as JSON on stdin. Rules live in the scripts; they change items only through `workitem-move` and frontmatter writes (`bin/workitem_fields.py`, atomic, retried on a concurrent write). Last starts are in `watch.json`.

- **Column** (a stage's `watch`): gets that column's items, runs in the board's repo. Pausable from the column header (`paused.json`).
- **Board** (top-level `watch`): every item of the board.
- **Workspace** (`[[watch]]` in `workspace.toml`): every item of the workspace, in the workspace folder. `pr-status` and `issue-status` belong only here.

Implicit watchers: `run-items` on an `auto` stage with a handler or prompt; `pull-queue` on an `auto` stage with neither (a queue); `pr-status` every 1m for a workspace with a board that watches PRs.

Bundled scripts: `run-items` (starts handlers via `bin/workitem-run-item` → `POST .../run`; skips items not `ok`, with a session, or missing `when`; honours `once`; blocks after 3 attempts), `pull-queue` (moves idle items to `next[0]`, oldest first, until refused), `pr-status` (reads PRs through `workitem_pr.py`, writes `pr_*` facts), `pr-review` (on the PR stage, reads only facts: merged → `on_merge`, closed → blocked, then one fixer at a time for conflicts, red CI (3 per red streak) and unseen comments), `pr-remind` (opt-in re-review reminder), `issue-status`, `age-out`.

## UI

One workspace in view. The sidebar has the workspace switcher, Search ⌘K, board tabs with blocked counts, the Plans list, *Learnings*, *Watchers* and *Settings*. Columns are the board's stages, plus an unknown-stage column for items whose stage or board is missing. No drag and drop: moves happen only through rule-checked actions.

- **Card:** title, issue, `agent_status` chip, session icon, PR chips (from `pr_*`), follow-ups and `depends on N` badges.
- **Detail panel:** pills, an alert box when blocked (*Start agent*, *Clear*) or at a gate (*Discuss brief*, *Approve*, reject with comment), the stage's actions that match the item (84), and tabs *Brief* (view and edit, refused if the file changed), *Info*, *Files*, *PR*, *Depends on*, *Children*, *Terminal*, *Remove*.
- **Remove** stops the session, removes the worktree (`--force`; uncommitted changes go with it) and deletes the folder; branch and PR stay. A final column's *Clear* removes in bulk.
- **Alerts:** a toast, beep and browser notification when an item becomes blocked (not for the item open while the page has focus); toasts also for an item done and a PR approved (43).
- **URL:** `#<workspace>/<dir>[/<tab>]?board=<name>`, `#?workspace=<w>&board=<b>`, `?plan=<workspace>/<id>`.

## Backend

Binds `127.0.0.1` (`WORKITEM_UI_HOST`), port 7373 (`WORKITEM_UI_PORT`). The Host header must be `127.0.0.1` or `localhost` with the server's port, or a name in `WORKITEM_UI_ALLOWED_HOSTS` (comma list of names, IPs or URLs such as `https://foo.bar.com`; only the name counts) with any port or none; remote access binds `0.0.0.0` and lists the machine's name there, or a reverse proxy's public URL, and the server prints one `#token=` URL per accepted name (a URL entry as given). Auth: a random token in `config.toml`; the printed `#token=` URL is posted once to `/api/login` for an `HttpOnly; SameSite=Strict` cookie. Scripts and the MCP server use `Authorization: Bearer <token>`. WebSockets also check that `Origin` is `http://` or `https://` plus the Host.

The frontend polls; nothing is pushed. Routes (`lib.rs`):

- `/api/workspaces`, `/api/agent`, `/api/pipelines`, boards (`POST`, `PUT`, `DELETE`), stages (`PUT`, `.../source`, `.../paused`), `.../learnings`.
- `/api/workitems` and `/api/workitems/{workspace}/{dir}` with actions `approve`, `reject`, `brief`, `frontmatter`, `clear-blocked`, `kill-session`, `start`, `run`, `fix`, `session`, `discuss`, `action/{id}`, `remove`, plus `pr`, `files`, `reveal`.
- `/api/workspaces/{workspace}/create`, `/api/sessions` (and `DELETE .../{workspace}/{name}`), `/api/plans`, `/api/mcp/{workspace}/{dir}`.
- `/ws/terminal/{workspace}/{dir}`, `/ws/session/{workspace}/{name}`: `tmux attach` on a PTY; closing the socket keeps the session.

`AppState.busy` serialises launches and dispatcher passes. Env: `WORKITEM_UI_HOME`, `_HOST`, `_ALLOWED_HOSTS`, `_PORT`, `_TMUX_SOCKET`, `_SKILLS` (default `~/.claude/skills`), `_CLAUDE`, `_GH`, `_BKT`, `_DISPATCH=off`.

## Non-goals

Remote or multi-user access, drag-and-drop moves, polling issue trackers or Sentry for intake inside the backend (intake is a skill on a loop), a service installer, a desktop shell. Bitbucket Data Center is not a PR host (82 is Cloud only); Bitbucket Issues are out of scope.
