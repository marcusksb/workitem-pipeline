import { Item, Pipelines } from "./types";

export const isBlocked = (item: Item) => item.agent_status.startsWith("blocked");

export const isWorking = (item: Item) => item.agent_status === "working";

/** `waiting: <what>`: the agent idles on purpose (CI, a deploy) and needs nobody. */
export const isWaiting = (item: Item) => item.agent_status.startsWith("waiting");

/** What a waiting agent waits for, from `waiting: <what>`. */
export const waitingFor = (item: Item) => item.agent_status.replace(/^waiting:?\s*/, "");

export const blockedKey = (item: Item) => `${itemKey(item)}|${item.agent_status}`;

/** Blocked items that were not blocked with the same reason in `seen` (keys from `blockedKey`). */
export function newlyBlocked(seen: Set<string>, items: Item[]): Item[] {
  return items.filter((item) => isBlocked(item) && !seen.has(blockedKey(item)));
}

/** The PR is open, approved, free of conflicts and green: it only waits for someone to merge it. */
export const readyToMerge = (item: Item) =>
  item.review?.state === "open" && item.review.approval === "approved" && item.review.merge === "mergeable" && item.review.ci === "passing";

export const issueDone = (item: Item) => item.issue_status_category === "done";

/** Items that match now and whose `itemKey` is not in `seen`, the keys of the items that matched before. */
export function newlyMatching(seen: Set<string>, items: Item[], matches: (item: Item) => boolean): Item[] {
  return items.filter((item) => matches(item) && !seen.has(itemKey(item)));
}

/** The item's stage is final on its board: no stage follows it. */
export function isFinal(item: Item, pipelines: Pipelines | undefined): boolean {
  const stage = boardOfWorkspace(pipelines, item.workspace, item.board)?.pipeline?.stages.find((s) => s.name === item.stage);
  return !!stage && stage.next.length === 0;
}

/** Key of the column for items whose stage is not on their board. */
export const UNKNOWN_STAGE = "unknown stage";

/** `final`: no stage follows it on any of the workspaces (`next` is empty), so its items can be cleared. */
export type Column = { name: string; label: string; final: boolean };

const boardOfWorkspace = (pipelines: Pipelines | undefined, workspace: string, board: string) =>
  pipelines?.[workspace]?.find((b) => b.name === board);

/** Column title of the item's stage: its `label` on the item's board, else the name with `_` as a space. */
export function stageTitle(item: Item, pipelines: Pipelines | undefined): string {
  const stage = boardOfWorkspace(pipelines, item.workspace, item.board)?.pipeline?.stages.find((s) => s.name === item.stage);
  return stage?.label ?? stageLabel(item.stage);
}

/**
 * The workspaces among `workspaces` where stage `stage` of board `board` has a watcher, and whether
 * all of them have paused it. A column shows these workspaces merged, so it pauses them together.
 */
export function pauseState(pipelines: Pipelines | undefined, workspaces: string[], board: string, stage: string): { workspaces: string[]; paused: boolean } {
  const watching = workspaces.flatMap((workspace) => {
    const found = boardOfWorkspace(pipelines, workspace, board);
    return found?.watched.includes(stage) ? [{ workspace, paused: found.paused.includes(stage) }] : [];
  });
  return { workspaces: watching.map((w) => w.workspace), paused: watching.length > 0 && watching.every((w) => w.paused) };
}

/** Board names of `workspaces`, by name. */
export function boardNames(pipelines: Pipelines | undefined, workspaces: string[]): string[] {
  const names = new Set<string>();
  for (const workspace of workspaces) for (const board of pipelines?.[workspace] ?? []) names.add(board.name);
  return [...names].sort((a, b) => a.localeCompare(b));
}

/**
 * The board an item is shown on. An item without a board, or whose board has no file, is shown on
 * the workspace's first board, in the unknown column, so it never disappears.
 */
export function shownBoard(item: Item, pipelines: Pipelines | undefined): string {
  if (boardOfWorkspace(pipelines, item.workspace, item.board)) return item.board;
  return pipelines?.[item.workspace]?.[0]?.name ?? item.board;
}

/** How a board is titled: its `label`, else its name. */
export const boardLabel = (pipelines: Pipelines | undefined, workspace: string, board: string) =>
  boardOfWorkspace(pipelines, workspace, board)?.pipeline?.label || board;

/** The last folder of a repo path, which is how a board's repo is shown. */
export const repoName = (repo: string) => repo.replace(/\/+$/, "").split("/").pop() ?? repo;

/**
 * The items `item` depends on that are not merged yet: neither their PR is merged nor are they in
 * the stage their board's `on_merge` names. A dependency that is gone counts as merged, as in
 * `workitem-move`.
 */
export function openDependencies(item: Item, items: Item[], pipelines: Pipelines | undefined): Item[] {
  return item.depends_on.flatMap((dir) => {
    const other = items.find((i) => i.workspace === item.workspace && i.dir === dir);
    if (!other || other.review?.state === "merged") return [];
    const stages = boardOfWorkspace(pipelines, other.workspace, other.board)?.pipeline?.stages ?? [];
    return stages.some((s) => s.on_merge && s.on_merge === other.stage) ? [] : [other];
  });
}

/** Number of blocked items on each board of `items`. */
export function blockedByBoard(items: Item[], pipelines: Pipelines | undefined): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const item of items) if (isBlocked(item)) counts[shownBoard(item, pipelines)] = (counts[shownBoard(item, pipelines)] ?? 0) + 1;
  return counts;
}

/**
 * Columns of board `board` for `workspaces`, left to right: each workspace's stages in order, a stage name
 * seen on an earlier workspace keeping its place. Workspaces without a valid file for it add none.
 */
export function columnsFor(pipelines: Pipelines | undefined, workspaces: string[], board: string): Column[] {
  const columns: Column[] = [];
  for (const workspace of workspaces) {
    for (const stage of boardOfWorkspace(pipelines, workspace, board)?.pipeline?.stages ?? []) {
      const final = stage.next.length === 0;
      const seen = columns.find((c) => c.name === stage.name);
      if (seen) seen.final &&= final;
      else columns.push({ name: stage.name, label: stage.label ?? stageLabel(stage.name), final });
    }
  }
  return columns;
}

/**
 * The items of board `board` by column name. An item whose stage is not on its own board, or whose
 * board has no file, goes to `UNKNOWN_STAGE`.
 */
export function byStage(items: Item[], pipelines: Pipelines | undefined, board: string): Record<string, Item[]> {
  const columns: Record<string, Item[]> = { [UNKNOWN_STAGE]: [] };
  for (const item of items) {
    if (shownBoard(item, pipelines) !== board) continue;
    const known = boardOfWorkspace(pipelines, item.workspace, item.board)?.pipeline?.stages.some((s) => s.name === item.stage);
    (columns[known ? item.stage : UNKNOWN_STAGE] ??= []).push(item);
  }
  return columns;
}

/**
 * How column `stage` of board `board` is sorted: the stage's `order`, else the board's, from the first of
 * `workspaces` whose file has one.
 */
export function orderFor(pipelines: Pipelines | undefined, workspaces: string[], board: string, stage: string): string[] {
  const pipelinesOf = workspaces.map((workspace) => boardOfWorkspace(pipelines, workspace, board)?.pipeline);
  for (const pipeline of pipelinesOf) {
    const order = pipeline?.stages.find((s) => s.name === stage)?.order ?? [];
    if (order.length) return order;
  }
  for (const pipeline of pipelinesOf) if (pipeline?.order.length) return pipeline.order;
  return [];
}

const CATEGORY_RANK: Record<string, number> = { new: 0, indeterminate: 1, done: 2 };

/**
 * Compares two items by one entry of a board's `order`: a field, optionally followed by `asc` (the default)
 * or `desc`. Jira status categories rank to do, in progress, done; other fields compare as text, numbers
 * as numbers ("PR 2" before "PR 10"). Empty values go last in either direction.
 */
function compareBy(a: Item, b: Item, entry: string): number {
  const [field, direction] = entry.trim().split(/\s+/);
  const x = String(a[field as keyof Item] ?? "");
  const y = String(b[field as keyof Item] ?? "");
  if (!x || !y) return Number(!x) - Number(!y);
  const c = field === "issue_status_category"
    ? (CATEGORY_RANK[x] ?? 3) - (CATEGORY_RANK[y] ?? 3)
    : x.localeCompare(y, undefined, { numeric: true, sensitivity: "base" });
  return direction === "desc" ? -c : c;
}

/**
 * Orders one column as a tree: children follow their parent, indented by `depth`.
 * A child whose parent is in another column (or missing) stays a root here.
 * Siblings are sorted by the fields of `order` (the board's `order`), then by title.
 */
export function treeOrder(unsorted: Item[], order: string[] = []): { item: Item; depth: number }[] {
  const entries = [...order, "title"];
  const items = [...unsorted].sort((a, b) => {
    for (const entry of entries) {
      const c = compareBy(a, b, entry);
      if (c) return c;
    }
    return 0;
  });
  const dirs = new Set(items.map((i) => `${i.workspace}/${i.dir}`));
  const hasParent = (i: Item) => i.parent !== "" && i.parent !== i.dir && dirs.has(`${i.workspace}/${i.parent}`);
  const out: { item: Item; depth: number }[] = [];
  const seen = new Set<Item>();
  const visit = (item: Item, depth: number) => {
    if (seen.has(item)) return;
    seen.add(item);
    out.push({ item, depth });
    for (const child of items) {
      if (child.workspace === item.workspace && child.parent === item.dir) visit(child, depth + 1);
    }
  };
  for (const item of items) if (!hasParent(item)) visit(item, 0);
  for (const item of items) visit(item, 0); // parent cycles: show them flat instead of dropping them
  return out;
}

/**
 * The children a parent's card lists: items of its workspace whose `parent:` is it, except those
 * in its own column (`treeOrder` already shows them under it). Blocked first, then by title.
 */
export function cardChildren(parent: Item, items: Item[]): Item[] {
  return items
    .filter((i) => i.workspace === parent.workspace && i.parent === parent.dir && i.dir !== parent.dir && !(i.board === parent.board && i.stage === parent.stage))
    .sort((a, b) => Number(isBlocked(b)) - Number(isBlocked(a)) || a.title.localeCompare(b.title));
}

export const itemKey = (item: Item) => `${item.workspace}/${item.dir}`;

/** `issue:` is `KEY (url)` or `none`. */
export function ticket(issue: string): { key: string; url?: string } | null {
  if (issue === "none" || issue === "") return null;
  const [, key, url] = issue.match(/^(\S+)(?:\s+\((\S+)\))?$/) ?? [];
  return key ? { key, url } : { key: issue };
}

/** Colour group of a Jira status category key, or undefined for an unknown or missing one. */
export function issueCategory(category: string): "todo" | "progress" | "done" | undefined {
  if (category === "new") return "todo";
  if (category === "indeterminate") return "progress";
  if (category === "done") return "done";
}

/** Number of a GitHub PR URL, or undefined. */
export const prNumber = (pr: string) => pr.match(/\/pull\/(\d+)/)?.[1];

/**
 * Items matching every word of `query` in their title, Jira key or PR number (`#123` or `123`).
 * Items whose key or PR number equals the whole query come first, then by title.
 */
export function searchItems(items: Item[], query: string): Item[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const whole = query.trim().toLowerCase().replace(/^#/, "");
  return items
    .map((item) => {
      const key = ticket(item.issue)?.key.toLowerCase() ?? "";
      const pr = prNumber(item.pr) ?? "";
      const text = `${item.title.toLowerCase()} ${key} ${pr && `#${pr}`}`;
      return { item, exact: key === whole || pr === whole, matches: words.every((word) => text.includes(word)) };
    })
    .filter((found) => found.matches)
    .sort((a, b) => Number(b.exact) - Number(a.exact) || a.item.title.localeCompare(b.item.title, undefined, { numeric: true, sensitivity: "base" }))
    .map((found) => found.item);
}

export const stageLabel = (stage: string) => stage.replaceAll("_", " ");

/** Icon per workitem kind; unknown kinds (including the `-` default) have no entry. */
export const KIND_ICONS: Record<string, string> = {
  bug: "🐛",
  feature: "✨",
  enhancement: "⚡",
  improvement: "⚡",
  task: "📋",
  story: "📗",
  chore: "🧹",
  refactor: "♻️",
  perf: "🚀",
  style: "🎨",
  build: "📦",
  ci: "🤖",
  revert: "⏪",
  docs: "📝",
  documentation: "📝",
  test: "🧪",
  spike: "🔬",
  question: "❓",
  review: "👀",
};

/** The `key: value` lines of the leading frontmatter block, in file order. */
export const frontmatterFields = (markdown: string): [string, string][] =>
  (markdown.match(/^---\n([\s\S]*?)\n---\n/)?.[1] ?? "")
    .split("\n")
    .flatMap((line) => {
      const at = line.indexOf(":");
      return at > 0 ? [[line.slice(0, at).trim(), line.slice(at + 1).trim()] as [string, string]] : [];
    });

export const stripFrontmatter = (markdown: string) => markdown.replace(/^---\n[\s\S]*?\n---\n/, "");

/** Flips the checkbox of the task list item that starts at `offset` (its bullet), or returns
 * undefined if no task item starts there. */
export const toggleTask = (markdown: string, offset: number): string | undefined => {
  const match = /(?:[-*+]|\d+[.)])[ \t]+\[([ xX])\]/y;
  match.lastIndex = offset;
  const found = match.exec(markdown);
  if (!found) return undefined;
  const box = offset + found[0].length - 2;
  return markdown.slice(0, box) + (found[1] === " " ? "x" : " ") + markdown.slice(box + 1);
};
