---
name: workitem-approve
description: Review and approve (or reject) a workitem in `ready_for_development`. Shows the review brief, then either sets `approved: true` and offers to start development, or sends the item back to `analyze` with your comment. Use when asked to "approve workitem X", "review the brief", "reject workitem X".
---

# Workitem Approve

Manual gate before any code is written. Input: workitem directory name or Jira key. If none is given, list items whose stage has a `gate` (title, size from the brief) and ask which one.

## Steps

1. Read `<repo>/.workitems/<dir>/workitem.md` (`.claude/workitems/` if that exists and `.workitems/` does not). The board is `<workitems dir>/boards/default.json`, else `<workitems dir>/pipeline.json`, else the bundled `workitem-pipeline/pipeline.json`. Confirm the item's stage has a `gate` there and note its `approve_to` and `reject_to`. If not, say which stage it is in and stop.
2. Print the brief for the user: title, `source`, links, then `### Findings`, `### Plan`, `### Risk`, `### Size`, `### Test plan`, `### Assumptions`. Put `### Risk` and `### Assumptions` first if they are not `None`.
3. Ask: approve, reject, or leave it. Do not decide for the user.
4. **Approve:** set `approved: true` and `approved_at: <today, YYYY-MM-DD>` in the frontmatter, then run `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem-dir> <approve_to>`. If the move is refused, set `approved: false` again and report the reason. Then offer to start development now (`workitem-start <dir>`). Start only if the user says yes.
5. **Reject:** ask for the comment. Add it to the workitem as `## Rejection` (replace an older one, keep it short). Then run `python3 ~/.claude/skills/workitem-pipeline/bin/workitem-move <workitem-dir> <reject_to>`. The handler of that stage picks it up on the next run.
6. Report one line: `<dir>: approved` or `<dir>: rejected -> <reject_to>`.

## Rules

- Never set `approved: true` without an explicit yes from the user in this session.
- Do not edit the brief. Changes to the plan go through a rejection comment.
