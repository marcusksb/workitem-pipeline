/** A column where a human decides: approving moves the item to `approve_to`, rejecting to `reject_to`. */
export type Gate = { label: string; approve_to: string; reject_to: string };

/** A button the item's stage offers it now (decision 86); `id` is what `api.runAction` sends.
 * `starts_session`: it runs a skill as the item's session, so it cannot start while one runs. */
export type StageAction = { id: string; label: string; confirm: string | null; starts_session: boolean };

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

/** `repo`: the repo every item of the board belongs to. `order`: item fields each column is sorted by, first one first; title breaks ties.
 * `issue_project`: where new issues go (`owner/repo` or a Jira key). `reviewers`: the `org/team` a ready PR asks for review. */
export type Pipeline = { repo?: string; label?: string | null; entry: string; stages: StageConfig[]; order: string[]; issue_project?: string; reviewers?: string };

/** What the board settings dialog changes; an empty value (all but `entry`) removes the key from the file. */
export type BoardSettings = { label: string; entry: string; order: string[]; issue_project: string; reviewers: string };

/**
 * A board of a workspace: `pipeline` is the newest valid file; `error` says why the file on disk is not in use.
 * `watched` are the stages with a watcher, `paused` the ones whose watchers are paused.
 * `problems` lists, per stage, the files it runs that do not resolve (handler, fixer skills, watch scripts); only stages with any.
 */
export type BoardInfo = { name: string; pipeline: Pipeline | null; error: string | null; watched: string[]; paused: string[]; problems: Record<string, string[]> };

/** Boards of every workspace, by workspace name, by board name. */
export type Pipelines = Record<string, BoardInfo[]>;

export type Review = {
  state: "open" | "merged" | "closed";
  approval: "approved" | "changes_requested" | "review_required" | "none";
  /** My latest verdict on the PR is an approval. */
  approved_by_me: boolean;
  merge: "mergeable" | "blocked" | "conflicting" | "unknown";
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
  /** Issue status name, or empty. */
  issue_status: string;
  /** Issue status category key (Jira's): `new`, `indeterminate` or `done`; or empty. */
  issue_status_category: string;
  pr: string;
  parent: string;
  /** The `wi-new-*` session that wrote the item (decision 75), or empty. */
  created_by: string;
  /** Items that must be merged before this one may enter a stage that requires it. */
  depends_on: string[];
  created: string;
  session: boolean;
  /** What the board allows in the item's current stage. */
  gate: Gate | null;
  pr_watch: boolean;
  develops: boolean;
  /** The stage's actions whose conditions the item matches, already filtered by the backend. */
  actions: StageAction[];
  review: Review | null;
  /** Branch checked out in the item's worktree; null without a worktree. */
  branch: string | null;
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
 * `issue_tracker`: `jira` or `github`. `github_account`: the gh login its processes use, else gh's active one.
 * `bkt_context`: the bkt context its Bitbucket calls run with, else bkt's active one.
 * `watch`: its watchers (only in the list). `error`: why `workspace.toml` is not in use; its last valid version is.
 * `account_error`: why the workspace is stopped, its GitHub account not being readable (only in the list). */
export type Workspace = {
  name: string;
  dir: string;
  agent?: Agent;
  issue_tracker: string;
  github_account?: string;
  bkt_context?: string;
  watch?: WorkspaceWatch[];
  error?: string;
  account_error?: string;
};

export const AGENT_KINDS = ["claude", "opencode", "pi", "omp"] as const;
export type AgentKind = (typeof AGENT_KINDS)[number];
/** The agent CLI that runs sessions; `command` defaults to the kind's name. */
export type Agent = { kind: AgentKind; command?: string };
/** The default agent, and which kinds have their own CLI on the backend's PATH. */
export type AgentSettings = { agent: Agent | null; kinds: { kind: AgentKind; installed: boolean }[] };

/** What the workspace settings dialog changes; an empty value clears it. */
export type WorkspaceChange = { agent?: Agent | null; issue_tracker?: string; github_account?: string; bkt_context?: string };

/** A `wi-new-*` create session. `waiting`: the agent waits for an answer. `dead`: it exited without killing its session. */
/** `purpose`: `create` writes workitems, `fix` fixes a board's problems (decision 85). */
export type AgentSession = { workspace: string; name: string; waiting: boolean; dead: boolean; purpose: "create" | "fix" };

/** `GET /api/state`: everything the board polls, read in one go so the parts agree. */
export type UiState = { workspaces: Workspace[]; pipelines: Pipelines; items: Item[]; sessions: AgentSession[]; plans: Plan[] };

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

/** `model`: the `--model` of the stage's item sessions; `null` means the agent's default. */
export type StageSource = { scripts: Source[]; prose: Source | null; fixers: Source[]; model: string | null; require_issue: boolean };

/** What the stage settings dialog saves (decision 76); an empty `model` removes it. */
export type StageSettings = { model: string; require_issue: boolean };

/** A workspace's `learnings.md`; `content` is empty while the file does not exist. */
export type Learnings = { path: string; content: string };

/** A file in a workitem folder; `path` is relative to it. */
export type FileEntry = { path: string; size: number };
