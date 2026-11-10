import { AgentSession, Chat, Detail, FileEntry, Item, Learnings, Pipelines, Plan, PlanDetail, PrConversation, Workspace, StageSource } from "./types";

type Launched = { session: string };

export class Unauthorized extends Error {}

async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  const response = await fetch(url, {
    method,
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (response.status === 401 && url !== "/api/login") {
    throw new Unauthorized();
  }
  if (!response.ok) {
    const error = await response.json().catch(() => null);
    throw new Error(error?.error ?? response.statusText);
  }
  return (response.status === 204 ? undefined : await response.json()) as T;
}

const segment = encodeURIComponent;
const itemUrl = (item: Item) => `/api/workitems/${segment(item.workspace)}/${segment(item.dir)}`;
const planUrl = (workspace: string, id: string) => `/api/plans/${segment(workspace)}/${segment(id)}`;

export const api = {
  login: (token: string) => request<void>("POST", "/api/login", { token }),
  me: () => request<void>("GET", "/api/me"),
  workspaces: () => request<Workspace[]>("GET", "/api/workspaces"),
  addWorkspace: (name: string) => request<Workspace>("POST", "/api/workspaces", { name }),
  setAgent: (name: string, agent: string) => request<Workspace>("PUT", `/api/workspaces/${segment(name)}`, { agent }),
  addBoard: (workspace: string, name: string, repo: string) =>
    request<void>("POST", `/api/workspaces/${segment(workspace)}/boards`, { name, repo }),
  pipelines: () => request<Pipelines>("GET", "/api/pipelines"),
  stageSource: (workspace: string, board: string, stage: string) =>
    request<StageSource>("GET", `/api/workspaces/${segment(workspace)}/boards/${segment(board)}/stages/${segment(stage)}/source`),
  pauseStage: (workspace: string, board: string, stage: string, paused: boolean) =>
    request<void>("PUT", `/api/workspaces/${segment(workspace)}/boards/${segment(board)}/stages/${segment(stage)}/paused`, { paused }),
  learnings: (workspace: string, board: string) =>
    request<Learnings>("GET", `/api/workspaces/${segment(workspace)}/boards/${segment(board)}/learnings`),
  saveLearnings: (workspace: string, board: string, base: string, content: string) =>
    request<void>("PUT", `/api/workspaces/${segment(workspace)}/boards/${segment(board)}/learnings`, { base, content }),
  items: () => request<Item[]>("GET", "/api/workitems"),
  detail: (workspace: string, dir: string) =>
    request<Detail>("GET", `/api/workitems/${segment(workspace)}/${segment(dir)}`),
  files: (item: Item) => request<FileEntry[]>("GET", `${itemUrl(item)}/files`),
  reveal: (item: Item, path: string) => request<void>("POST", `${itemUrl(item)}/reveal`, { path }),
  prConversation: (item: Item) => request<PrConversation>("GET", `${itemUrl(item)}/pr`),
  sessions: () => request<AgentSession[]>("GET", "/api/sessions"),
  approve: (item: Item) => request<void>("POST", `${itemUrl(item)}/approve`),
  reject: (item: Item, comment: string) => request<void>("POST", `${itemUrl(item)}/reject`, { comment }),
  editBrief: (item: Item, base: string, body: string) => request<void>("POST", `${itemUrl(item)}/brief`, { base, body }),
  editFrontmatter: (item: Item, fields: Record<string, string>) =>
    request<void>("POST", `${itemUrl(item)}/frontmatter`, { fields }),
  clearBlocked: (item: Item) => request<void>("POST", `${itemUrl(item)}/clear-blocked`),
  killSession: (item: Item) => request<void>("POST", `${itemUrl(item)}/kill-session`),
  start: (item: Item) => request<Launched>("POST", `${itemUrl(item)}/start`),
  startSession: (item: Item) => request<Launched>("POST", `${itemUrl(item)}/session`),
  discussBrief: (item: Item) => request<Launched>("POST", `${itemUrl(item)}/discuss`),
  merge: (item: Item) => request<void>("POST", `${itemUrl(item)}/merge`),
  rebase: (item: Item) => request<void>("POST", `${itemUrl(item)}/rebase`),
  remove: (item: Item) => request<{ worktree_left: string | null }>("POST", `${itemUrl(item)}/remove`),
  create: (workspace: string, input: string, board: string) =>
    request<Launched>("POST", `/api/workspaces/${segment(workspace)}/create`, { input, board }),
  plans: () => request<Plan[]>("GET", "/api/plans"),
  plan: (workspace: string, id: string) => request<PlanDetail>("GET", planUrl(workspace, id)),
  createPlan: (workspace: string, input: string) =>
    request<{ id: string }>("POST", `/api/workspaces/${segment(workspace)}/plans`, { input }),
  resumePlan: (plan: Plan) => request<void>("POST", `${planUrl(plan.workspace, plan.id)}/resume`),
  deletePlan: (plan: Plan) => request<void>("DELETE", planUrl(plan.workspace, plan.id)),
  chat: (base: string, after: number, from: string) =>
    request<Chat>("GET", `${base}/chat?after=${after}&from=${encodeURIComponent(from)}`),
  sendChat: (base: string, text: string) => request<void>("POST", `${base}/chat`, { text }),
  pressKey: (base: string, key: ChatKey) => request<void>("POST", `${base}/keys`, { key }),
};

/** Keys the Chat tab's pending box can press; the backend refuses any other. */
export const CHAT_KEYS = ["1", "2", "3", "Enter", "Escape", "Up", "Down"] as const;
export type ChatKey = (typeof CHAT_KEYS)[number];

/** API base of the session a Chat tab reads: a workitem's or a plan's. */
export const itemChat = (workspace: string, dir: string) => `/api/workitems/${segment(workspace)}/${segment(dir)}`;
export const planChat = (plan: Plan) => planUrl(plan.workspace, plan.id);

/** URL of one file in a workitem folder; `path` is relative and `/`-separated. */
export const fileUrl = (item: Item, path: string) => `${itemUrl(item)}/files/${path.split("/").map(segment).join("/")}`;

/** Path of a terminal socket: a workitem's session, or a `wi-new-*` or `wi-plan-*` session. */
export const itemTerminal = (workspace: string, dir: string) => `terminal/${segment(workspace)}/${segment(dir)}`;
export const agentTerminal = (workspace: string, name: string) => `session/${segment(workspace)}/${segment(name)}`;
export const planTerminal = (plan: Plan) => agentTerminal(plan.workspace, `wi-plan-${plan.id}`);

export const terminalUrl = (path: string, cols: number, rows: number) => {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${location.host}/ws/${path}?cols=${cols}&rows=${rows}`;
};
