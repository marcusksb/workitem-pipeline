# PR review board — plan

A second board, `pr-review`, for storyrails. Every eligible open PR becomes an item. One at a time is reviewed by an agent, the findings wait in the brief for your approval, and approving posts them to GitHub.

## Board `<storyrails>/.workitems/boards/pr-review.json`

| Stage | Handler | Rules |
|---|---|---|
| `backlog` | none, `auto` | The dispatcher pulls the oldest item (by `created`, the PR's `createdAt`) into `in_review` when it has room. |
| `in_review` | `pr-review-run`, `auto` | `limit: 1`. Reviews the item's PR, writes the findings into `### Findings` of the brief, moves the item to `reviewed`. |
| `reviewed` | none | `require: {brief: complete}` (`brief_sections: ["Findings"]`), `on_enter: {approved: false}`, gate `Post review`: approve → `posted`, reject → `posted`. |
| `posted` | `pr-review-post`, `auto`, `once`, `when: {approved: "true"}` | Posts `### Findings` as one COMMENT review. A rejected item sits here with `approved: false` and no session is started. |

Decisions taken in the grill: one item per PR; intake is a loop-run watcher skill outside the backend; `limit` is an engine rule; backlog is pulled by the dispatcher, oldest PR first; the brief is the source of truth (you edit `### Findings` before approving); the review is always a plain COMMENT; reject discards; posting refuses a PR whose head commit moved; the watcher deletes items whose PR is closed, merged or has 2 approvals.

## 1. Engine

Three small stage keys. Each is generic, none knows a stage name.

1. `limit` (number). `workitem-move` refuses to enter a stage that already holds `limit` items of the same board in the same workitems dir (`refused: 'in_review' is full (limit 1)`). Also applies to `--board`. The Rust `Stage` gets the field and the API serves it.
2. Pull rule in `dispatch.rs`. A stage that is `auto`, has no handler and no prompt, and has a `next` moves its idle items (status `ok`, no session) to `next[0]`, oldest `created` first, through `workitem-move`, until that stage is full. Runs once per board per pass, under `busy`. A refused move is ignored (the limit is the source of truth).
3. `once` and `when` for a handler stage. `once: true` starts the handler at most once per stage entry (the `attempts` counter already resets on a stage change). `when: {key: value}` starts it only when the item's frontmatter matches. Without these the dispatcher would start `pr-review-post` again every 5s, because a final stage never moves on.

Tests: unittest for `limit` in `test_workitem_move.py`; unit tests for the new keys in `pipeline.rs`; a dispatcher integration test (stub `claude`) for pull order, limit and `once`/`when`.

## 2. Board file and skills

All in `<storyrails>/.workitems/` (project files, not part of this repo, except the sample copies below).

1. `boards/pr-review.json` as in the table.
2. `boards/pr-review/skills/pr-review-run/SKILL.md`: the `review-next-pr` steps 3 and 4 for the PR in the item's `pr:` field. It runs the built-in `review`, writes the result into `## Review brief` / `### Findings` (explanation per finding, file and line where known), records `head_sha:` in the frontmatter, and moves to the default next stage.
3. `boards/pr-review/skills/pr-review-post/SKILL.md`: new.
   - Compares `gh pr view --json headRefOid` with `head_sha`. If it moved, sets `blocked: PR has new commits, drag back to backlog` and stops.
   - Posts `### Findings` as one review (`gh api` pulls review, event `COMMENT`): findings with file and line become inline comments, the rest the review body. It never sets APPROVE or REQUEST_CHANGES.
   - Sets `posted_at`, `agent_status: ok`.
4. `~/.claude/skills/pr-review-intake/SKILL.md` (a global skill, run as `/loop 10m /pr-review-intake`, repo = storyrails):
   - Lists PRs like `review-next-pr` step 1 (open, not draft, not mine, not `storyblok-machine-user` or dependabot, fewer than 2 approvals, not approved by me).
   - Writes `.workitems/pr-<number>/workitem.md` for each PR that has no item: `title`, `type: review`, `pr: <url>`, `repo`, `stage: backlog`, `board: pr-review`, `approved: false`, `agent_status: ok`, `created: <PR createdAt>`, `issue: none`.
   - Deletes items (not in `posted`, no live `wi-<dir>` session) whose PR is closed, merged or has 2 approvals. Items in `posted` are left for the Clear button.

Because the intake watcher writes the files itself and does not use `workitem-create`, `workitem-create` stays unchanged.

## 3. Docs

`workitem-pipeline.md` (per-stage keys), `workitem-pipeline-ui.md` (new decisions 28 to 30: `limit`, pull rule, `once`/`when`).

## Build order

1. `limit` in `workitem-move` + tests.
2. `limit`, `once`, `when` in `pipeline.rs` + tests.
3. Pull rule, `once`, `when` in `dispatch.rs` + integration test.
4. Board file, `pr-review-run`, `pr-review-post` (storyrails).
5. `pr-review-intake`.
6. Docs. Then run the whole `npm test` and `python3 -m unittest test_workitem_move`.

## Risks

- `pr-review-post` is the only step with an outside effect. It is behind the gate, and the stale check stops a review of an old commit.
- The `created` field doubles as sort key, so it holds a full timestamp on this board and a date on the others. Both sort correctly as strings.
- `once` means a posting session that crashes without blocking is not retried; a blocked one can be retried with "clear blocked".
