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
  /** Replaces the board's `order` for this column when not empty. */
  order: string[];
};

/** `repo`: the repo every item of the board belongs to. `order`: item fields each column is sorted by, first one first; title breaks ties. */
export type Pipeline = { repo?: string; label?: string | null; entry: string; stages: StageConfig[]; order: string[] };

/**
 * A board of a workspace: `pipeline` is the newest valid file; `error` says why the file on disk is not in use.
 * `watched` are the stages with a watcher, `paused` the ones whose watchers are paused.
 */
export type BoardInfo = { name: string; pipeline: Pipeline | null; error: string | null; watched: string[]; paused: string[] };

/** Boards of every workspace, by workspace name, by board name. */
export type Pipelines = Record<string, BoardInfo[]>;

export type Review = {
  state: "open" | "merged" | "closed";
  approval: "approved" | "changes_requested" | "review_required" | "none";
  /** My latest verdict on the PR is an approval. */
  approved_by_me: boolean;
  merge: "mergeable" | "conflicting" | "unknown";
  ci: "passing" | "failing" | "pending" | "none";
  /** Names of the red checks. */
  failed: string[];
};

export type Item = {
  workspace: string;
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
  /** Items that must be merged before this one may enter a stage that requires it. */
  depends_on: string[];
  created: string;
  session: boolean;
  /** What the board allows in the item's current stage. */
  gate: Gate | null;
  pr_watch: boolean;
  develops: boolean;
  review: Review | null;
  /** Commits in the item's worktree that no remote has; null without a worktree. */
  unpushed: number | null;
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

/** A script that runs once for the whole workspace on a timer; `implicit` ones are not in `workspace.toml`. */
export type WorkspaceWatch = { every: string; command: string; args: string[]; implicit: boolean };

/** A folder in `~/.workitems-pipeline/workspaces`. `agent`: executable that runs its sessions instead of the default `claude`.
 * `watch`: its watchers (only in the list). `error`: why `workspace.toml` is not in use; its last valid version is. */
export type Workspace = { name: string; dir: string; agent?: string; watch?: WorkspaceWatch[]; error?: string };

export type AgentSession = { workspace: string; name: string };

/** A planning session's record (`<workspace>/plans/<id>.md`). `status` is what the record says;
 * see `planStatus` for what to show. */
export type Plan = {
  workspace: string;
  id: string;
  topic: string;
  summary: string;
  status: "working" | "waiting" | "ended";
  /** Directories of the workitems made from it. */
  workitems: string[];
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
  origin: "board" | "workspace" | "bundled" | null;
  content: string;
  note: string | null;
  error: string | null;
};

export type StageSource = { scripts: Source[]; prose: Source | null };

/** A workspace's `learnings.md`; `content` is empty while the file does not exist. */
export type Learnings = { path: string; content: string };

/** A file in a workitem folder; `path` is relative to it. */
export type FileEntry = { path: string; size: number };
