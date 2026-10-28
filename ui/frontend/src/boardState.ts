import { Item, Pipelines } from "./types";

export const isBlocked = (item: Item) => item.agent_status.startsWith("blocked");

export const isWorking = (item: Item) => item.agent_status === "working";

export const blockedKey = (item: Item) => `${itemKey(item)}|${item.agent_status}`;

/** Blocked items that were not blocked with the same reason in `seen` (keys from `blockedKey`). */
export function newlyBlocked(seen: Set<string>, items: Item[]): Item[] {
  return items.filter((item) => isBlocked(item) && !seen.has(blockedKey(item)));
}

export const DEFAULT_BOARD = "default";

/** Key of the column for items whose stage is not on their board. */
export const UNKNOWN_STAGE = "unknown stage";

/** `final`: no stage follows it on any of the projects (`next` is empty), so its items can be cleared. */
export type Column = { name: string; label: string; final: boolean };

const boardOfProject = (pipelines: Pipelines | undefined, project: string, board: string) =>
  pipelines?.[project]?.find((b) => b.name === board);

/** Board names of `projects`, `default` first, then by name. */
export function boardNames(pipelines: Pipelines | undefined, projects: string[]): string[] {
  const names = new Set<string>();
  for (const project of projects) for (const board of pipelines?.[project] ?? []) names.add(board.name);
  return [...names].sort((a, b) => (a === DEFAULT_BOARD ? -1 : b === DEFAULT_BOARD ? 1 : a.localeCompare(b)));
}

/**
 * The board an item is shown on. An item whose board has no file is shown on `default`,
 * in the unknown column; the file may be missing from the project's listing.
 */
export function shownBoard(item: Item, pipelines: Pipelines | undefined): string {
  return boardOfProject(pipelines, item.project, item.board) ? item.board : DEFAULT_BOARD;
}

/** Number of blocked items on each board of `items`. */
export function blockedByBoard(items: Item[], pipelines: Pipelines | undefined): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const item of items) if (isBlocked(item)) counts[shownBoard(item, pipelines)] = (counts[shownBoard(item, pipelines)] ?? 0) + 1;
  return counts;
}

/**
 * Columns of board `board` for `projects`, left to right: each project's stages in order, a stage name
 * seen on an earlier project keeping its place. Projects without a valid file for it add none.
 */
export function columnsFor(pipelines: Pipelines | undefined, projects: string[], board: string): Column[] {
  const columns: Column[] = [];
  for (const project of projects) {
    for (const stage of boardOfProject(pipelines, project, board)?.pipeline?.stages ?? []) {
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
    const known = boardOfProject(pipelines, item.project, item.board)?.pipeline?.stages.some((s) => s.name === item.stage);
    (columns[known ? item.stage : UNKNOWN_STAGE] ??= []).push(item);
  }
  return columns;
}

/**
 * Orders one column as a tree: children follow their parent, indented by `depth`.
 * A child whose parent is in another column (or missing) stays a root here.
 * Siblings are sorted by title, numbers compared as numbers ("PR 2" before "PR 10").
 */
export function treeOrder(unsorted: Item[]): { item: Item; depth: number }[] {
  const items = [...unsorted].sort((a, b) => a.title.localeCompare(b.title, undefined, { numeric: true, sensitivity: "base" }));
  const dirs = new Set(items.map((i) => `${i.project}/${i.dir}`));
  const hasParent = (i: Item) => i.parent !== "" && i.parent !== i.dir && dirs.has(`${i.project}/${i.parent}`);
  const out: { item: Item; depth: number }[] = [];
  const seen = new Set<Item>();
  const visit = (item: Item, depth: number) => {
    if (seen.has(item)) return;
    seen.add(item);
    out.push({ item, depth });
    for (const child of items) {
      if (child.project === item.project && child.parent === item.dir) visit(child, depth + 1);
    }
  };
  for (const item of items) if (!hasParent(item)) visit(item, 0);
  for (const item of items) visit(item, 0); // parent cycles: show them flat instead of dropping them
  return out;
}

export const itemKey = (item: Item) => `${item.project}/${item.dir}`;

/** `issue:` is `KEY (url)` or `none`. */
export function ticket(issue: string): { key: string; url?: string } | null {
  if (issue === "none" || issue === "") return null;
  const [, key, url] = issue.match(/^(\S+)(?:\s+\((\S+)\))?$/) ?? [];
  return key ? { key, url } : { key: issue };
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
