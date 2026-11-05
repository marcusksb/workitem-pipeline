/** A column where a human decides: approving moves the item to `approve_to`, rejecting to `reject_to`. */
export type Gate = { label: string; approve_to: string; reject_to: string };

export type StageConfig = {
  name: string;
  label: string | null;
  handler: string | null;
  auto: boolean;
  next: string[];
  gate: Gate | null;
  on_merge: string | null;
};

export type Pipeline = { entry: string; stages: StageConfig[] };

/**
 * A board of a project: `pipeline` is the newest valid file; `error` says why the file on disk is not in use.
 * `watched` are the stages with a watcher, `paused` the ones whose watchers are paused.
 */
export type BoardInfo = { name: string; pipeline: Pipeline | null; error: string | null; watched: string[]; paused: string[] };

/** Boards of every project, by project name; `default` comes first. */
export type Pipelines = Record<string, BoardInfo[]>;

export type Review = {
  approval: "approved" | "changes_requested" | "review_required" | "none";
  /** My latest verdict on the PR is an approval. */
  approved_by_me: boolean;
  merge: "mergeable" | "conflicting" | "unknown";
  ci: "passing" | "failing" | "pending" | "none";
  failed: { name: string; url: string }[];
};

export type Item = {
  project: string;
  dir: string;
  title: string;
  kind: string;
  stage: string;
  board: string;
  approved: boolean;
  agent_status: string;
  issue: string;
  /** Jira status name, or empty. */
  issue_status: string;
  /** Jira status category key: `new`, `indeterminate` or `done`; or empty. */
  issue_status_category: string;
  pr: string;
  parent: string;
  created: string;
  /** PR head commit the item last looked at, or empty. */
  head_sha: string;
  session: boolean;
  /** What the board allows in the item's current stage. */
  gate: Gate | null;
  pr_watch: boolean;
  develops: boolean;
  review: Review | null;
  /** Commits in the item's worktree that no remote has; null without a worktree. */
  unpushed: number | null;
  /** The PR head moved past `head_sha`. */
  pr_moved: boolean;
  /** Open `- [ ]` items under `## Follow-ups`. */
  follow_ups: string[];
};

export type Detail = Item & { content: string };

export type PrNote = {
  kind: "comment" | "review" | "inline";
  author: string;
  body: string;
  at: string;
  state: string | null;
  path: string | null;
};

export type PrConversation = { body: string; notes: PrNote[] };

/** `agent`: executable that runs this project's sessions instead of the default `claude`. */
export type Project = { name: string; repo: string; agent?: string };

export type AgentSession = { project: string; name: string };

/** A planning session's record (`<workitems dir>/plans/<id>.md`). `status` is what the record says;
 * see `planStatus` for what to show. */
export type Plan = {
  project: string;
  id: string;
  topic: string;
  summary: string;
  status: "working" | "waiting" | "ended";
  /** Directory of the workitem made from it, or "". */
  workitem: string;
  /** Epoch seconds. */
  created: number;
  has_plan: boolean;
  live: boolean;
};

export type PlanDetail = Plan & { prompt: string; text: string | null };

/** A script or skill a stage runs, or the inline prompt in its place. */
export type Source = {
  name: string;
  path: string | null;
  origin: "board" | "project" | "bundled" | null;
  content: string;
  note: string | null;
  error: string | null;
};

export type StageSource = { scripts: Source[]; prose: Source | null };

/** A project's `learnings.md`; `content` is empty while the file does not exist. */
export type Learnings = { path: string; content: string };

/** A file in a workitem folder; `path` is relative to it. */
export type FileEntry = { path: string; size: number };
