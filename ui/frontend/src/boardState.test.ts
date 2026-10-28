import { describe, expect, it } from "vitest";
import { blockedByBoard, blockedKey, boardNames, byStage, columnsFor, newlyBlocked, searchItems, shownBoard, frontmatterFields, stripFrontmatter, toggleTask, treeOrder, UNKNOWN_STAGE } from "./boardState";
import { Item, Pipelines } from "./types";

const item = (over: Partial<Item>): Item => ({
  project: "app",
  dir: "x",
  title: "x",
  kind: "bug",
  stage: "backlog",
  board: "default",
  approved: false,
  agent_status: "ok",
  issue: "none",
  pr: "",
  parent: "",
  created: "",
  session: false,
  gate: null,
  pr_watch: false,
  develops: false,
  review: null,
  unpushed: null,
  pr_moved: false,
  head_sha: "",
  ...over,
});

describe("newlyBlocked", () => {
  it("returns blocked items not seen with the same reason, ignoring unblocked ones", () => {
    const old = item({ dir: "old", agent_status: "blocked: a" });
    const changed = item({ dir: "changed", agent_status: "blocked: b" });
    const fresh = item({ dir: "fresh", agent_status: "blocked: c" });
    const seen = new Set([blockedKey(old), blockedKey({ ...changed, agent_status: "blocked: a" })]);
    expect(newlyBlocked(seen, [old, changed, fresh, item({ dir: "ok" })]).map((i) => i.dir)).toEqual(["changed", "fresh"]);
  });
});

const stage = (name: string, label: string | null = null) => ({ name, label, handler: null, auto: false, next: [], gate: null, on_merge: null });
const board = (name: string, ...stages: string[]) => ({ name, pipeline: { entry: stages[0], stages: stages.map((n) => stage(n)) }, error: null });
const pipelines: Pipelines = {
  app: [board("default", "todo", "doing", "done"), board("research", "idea", "done")],
  lib: [board("default", "todo", "review", "done")],
};

describe("boardNames", () => {
  it("puts default first and merges the boards of several projects", () => {
    expect(boardNames(pipelines, ["app"])).toEqual(["default", "research"]);
    expect(boardNames(pipelines, ["lib"])).toEqual(["default"]);
    expect(boardNames({ x: [board("zeta", "a"), board("default", "a"), board("alpha", "a")] }, ["x"])).toEqual(["default", "alpha", "zeta"]);
    expect(boardNames(undefined, ["app"])).toEqual([]);
  });
});

describe("columnsFor", () => {
  it("keeps one board's order and merges several projects by stage name", () => {
    expect(columnsFor(pipelines, ["app"], "default").map((c) => c.name)).toEqual(["todo", "doing", "done"]);
    expect(columnsFor(pipelines, ["app", "lib"], "default").map((c) => c.name)).toEqual(["todo", "doing", "done", "review"]);
    expect(columnsFor(pipelines, ["app", "lib"], "research").map((c) => c.name)).toEqual(["idea", "done"]);
  });

  it("uses the label, else the name with spaces, and skips projects without a valid file", () => {
    const labelled: Pipelines = {
      app: [{ name: "default", pipeline: { entry: "a_b", stages: [stage("a_b"), stage("c", "See")] }, error: null }],
      bad: [{ name: "default", pipeline: null, error: "broken" }],
    };
    expect(columnsFor(labelled, ["app", "bad", "missing"], "default")).toEqual([
      { name: "a_b", label: "a b", final: true },
      { name: "c", label: "See", final: true },
    ]);
    expect(columnsFor(undefined, ["app"], "default")).toEqual([]);
  });

  it("marks a column final only when no project has a next stage for it", () => {
    const flow: Pipelines = {
      app: [{ name: "default", pipeline: { entry: "a", stages: [{ ...stage("a"), next: ["b"] }, stage("b")] }, error: null }],
      lib: [{ name: "default", pipeline: { entry: "b", stages: [{ ...stage("b"), next: ["c"] }, stage("c")] }, error: null }],
    };
    expect(columnsFor(flow, ["app"], "default").map((c) => c.final)).toEqual([false, true]);
    expect(columnsFor(flow, ["app", "lib"], "default").map((c) => c.final)).toEqual([false, false, true]);
  });
});

describe("shownBoard", () => {
  it("is the item's board, or default when its project has no such board", () => {
    expect(shownBoard(item({ board: "research" }), pipelines)).toBe("research");
    expect(shownBoard(item({ board: "gone" }), pipelines)).toBe("default");
    expect(shownBoard(item({ project: "lib", board: "research" }), pipelines)).toBe("default");
  });
});

describe("byStage", () => {
  it("groups the board's items by stage and puts a stage missing from their own board in the unknown column", () => {
    const items = [
      item({ dir: "a", stage: "doing" }),
      item({ dir: "b", stage: "weird" }),
      item({ dir: "c", project: "lib", stage: "doing" }),
      item({ dir: "d", project: "nobody", stage: "todo" }),
      item({ dir: "e", board: "research", stage: "idea" }),
      item({ dir: "f", board: "gone", stage: "todo" }),
    ];
    const columns = byStage(items, pipelines, "default");
    expect(columns.doing.map((i) => i.dir)).toEqual(["a"]);
    expect(columns[UNKNOWN_STAGE].map((i) => i.dir)).toEqual(["b", "c", "d", "f"]);
    expect(byStage(items, pipelines, "research").idea.map((i) => i.dir)).toEqual(["e"]);
  });
});

describe("blockedByBoard", () => {
  it("counts blocked items on the board they are shown on", () => {
    const items = [
      item({ dir: "a", agent_status: "blocked: x" }),
      item({ dir: "b", board: "research", agent_status: "blocked: y" }),
      item({ dir: "c", board: "gone", agent_status: "blocked: z" }),
      item({ dir: "d", board: "research" }),
    ];
    expect(blockedByBoard(items, pipelines)).toEqual({ default: 2, research: 1 });
  });
});

describe("stripFrontmatter", () => {
  it("removes the leading frontmatter block only", () => {
    expect(stripFrontmatter("---\na: 1\n---\n# T\n---\nx")).toBe("# T\n---\nx");
  });
});

describe("frontmatterFields", () => {
  it("returns the leading block's pairs in order, keeping colons in values", () => {
    expect(frontmatterFields("---\na: 1\nb: x: y\n---\n# T\n---\nz: 2")).toEqual([["a", "1"], ["b", "x: y"]]);
    expect(frontmatterFields("# T")).toEqual([]);
  });
});

describe("treeOrder", () => {
  it("puts children under their parent and keeps orphans as roots", () => {
    const rows = treeOrder([
      item({ dir: "c1", parent: "p" }),
      item({ dir: "p" }),
      item({ dir: "c2", parent: "p" }),
      item({ dir: "orphan", parent: "elsewhere" }),
    ]);
    expect(rows.map((r) => [r.item.dir, r.depth])).toEqual([["p", 0], ["c1", 1], ["c2", 1], ["orphan", 0]]);
  });

  it("does not drop items in a parent cycle", () => {
    const rows = treeOrder([item({ dir: "a", parent: "b" }), item({ dir: "b", parent: "a" })]);
    expect(rows).toHaveLength(2);
  });
});

describe("treeOrder sorting", () => {
  it("sorts roots and children by title with numeric awareness", () => {
    const rows = treeOrder([
      item({ dir: "b", title: "PR 10", parent: "p" }),
      item({ dir: "z", title: "Zeta" }),
      item({ dir: "a", title: "PR 2", parent: "p" }),
      item({ dir: "p", title: "Alpha" }),
    ]);
    expect(rows.map((r) => r.item.dir)).toEqual(["p", "a", "b", "z"]);
  });
});

describe("toggleTask", () => {
  const md = "Intro\n\n- [ ] one\n  1. [x] nested\n* plain\n";
  it("checks and unchecks the item starting at the offset", () => {
    expect(toggleTask(md, md.indexOf("- [ ]"))).toBe(md.replace("- [ ] one", "- [x] one"));
    expect(toggleTask(md, md.indexOf("1. [x]"))).toBe(md.replace("1. [x]", "1. [ ]"));
  });
  it("leaves non-task items alone", () => {
    expect(toggleTask(md, md.indexOf("* plain"))).toBeUndefined();
  });
});

describe("searchItems", () => {
  const items = [
    item({ dir: "a", title: "Fix login redirect", issue: "SHAPE-123 (https://jira/SHAPE-123)" }),
    item({ dir: "b", title: "Upgrade 123 deps", pr: "https://github.com/o/r/pull/77" }),
    item({ dir: "c", title: "Login page copy", pr: "https://github.com/o/r/pull/123" }),
  ];
  const dirs = (query: string) => searchItems(items, query).map((i) => i.dir);

  it("matches every word against title, Jira key and PR number, case-insensitively", () => {
    expect(dirs("login")).toEqual(["a", "c"]);
    expect(dirs("shape-1")).toEqual(["a"]);
    expect(dirs("#77")).toEqual(["b"]);
    expect(dirs("login copy")).toEqual(["c"]);
  });

  it("puts an exact Jira key or PR number match first", () => {
    expect(dirs("123")).toEqual(["c", "a", "b"]);
    expect(dirs("#123")).toEqual(["c"]);
    expect(dirs("SHAPE-123")).toEqual(["a"]);
  });

  it("returns nothing for an empty query", () => {
    expect(dirs("  ")).toEqual([]);
  });
});
