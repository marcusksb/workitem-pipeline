import { AgentSession, Detail, FileEntry, Item, Pipelines, PrConversation, Project, StageSource } from "./types";

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
const itemUrl = (item: Item) => `/api/workitems/${segment(item.project)}/${segment(item.dir)}`;

export const api = {
  login: (token: string) => request<void>("POST", "/api/login", { token }),
  me: () => request<void>("GET", "/api/me"),
  projects: () => request<Project[]>("GET", "/api/projects"),
  addProject: (name: string, repo: string) => request<Project>("POST", "/api/projects", { name, repo }),
  removeProject: (name: string) => request<void>("DELETE", `/api/projects/${segment(name)}`),
  pipelines: () => request<Pipelines>("GET", "/api/pipelines"),
  stageSource: (project: string, board: string, stage: string) =>
    request<StageSource>("GET", `/api/projects/${segment(project)}/boards/${segment(board)}/stages/${segment(stage)}/source`),
  items: () => request<Item[]>("GET", "/api/workitems"),
  detail: (project: string, dir: string) =>
    request<Detail>("GET", `/api/workitems/${segment(project)}/${segment(dir)}`),
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
  create: (project: string, input: string, board: string) =>
    request<Launched>("POST", `/api/projects/${segment(project)}/create`, { input, board }),
};

/** URL of one file in a workitem folder; `path` is relative and `/`-separated. */
export const fileUrl = (item: Item, path: string) => `${itemUrl(item)}/files/${path.split("/").map(segment).join("/")}`;

/** Path of a terminal socket: a workitem's session, or a `wi-new-*` agent session. */
export const itemTerminal = (project: string, dir: string) => `terminal/${segment(project)}/${segment(dir)}`;
export const agentTerminal = (project: string, name: string) => `session/${segment(project)}/${segment(name)}`;

export const terminalUrl = (path: string, cols: number, rows: number) => {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${location.host}/ws/${path}?cols=${cols}&rows=${rows}`;
};
