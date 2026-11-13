import { describe, expect, it } from "vitest";
import { cardChildren, orderFor, blockedByBoard, blockedKey, boardNames, byStage, columnsFor, isBlocked, issueDone, newlyMatching, readyToMerge, isWaiting, issueCategory, newlyBlocked, waitingFor, pauseState, boardLabel, openDependencies, repoName, searchItems, shownBoard, frontmatterFields, stripFrontmatter, toggleTask, treeOrder, UNKNOWN_STAGE } from "./boardState";
import { item, stage } from "./testFixtures";
import { Pipelines, Review } from "./types";


describe("newlyBlocked", () => {
  it("returns blocked items not seen with the same reason, ignoring unblocked ones", () => {
    const old = item({ dir: "old", agent_status: "blocked: a" });
    const changed = item({ dir: "changed", agent_status: "blocked: b" });
    const fresh = item({ dir: "fresh", agent_status: "blocked: c" });
    const seen = new Set([blockedKey(old), blockedKey({ ...changed, agent_status: "blocked: a" })]);
    expect(newlyBlocked(seen, [old, changed, fresh, item({ dir: "ok" })]).map((i) => i.dir)).toEqual(["changed", "fresh"]);
  });
});

const board = (name: string, ...stages: string[]) => ({ name, pipeline: { repo: "/r/app", order: [], entry: stages[0], stages: stages.map((n) => stage(n)) }, error: null, watched: [], paused: [] });
const pipelines: Pipelines = {
  app: [board("default", "todo", "doing", "done"), board("research", "idea", "done")],
  lib: [board("default", "todo", "review", "done")],
};

describe("pauseState", () => {
  const watched = (paused: string[]) => ({ ...board("default", "todo", "doing"), watched: ["doing"], paused });
  it("names the workspaces that watch the stage and is paused only when all of them are", () => {
    const some: Pipelines = { app: [watched(["doing"])], lib: [watched([])], doc: [board("default", "doing")] };
    expect(pauseState(some, ["app", "lib", "doc"], "default", "doing")).toEqual({ workspaces: ["app", "lib"], paused: false });
    expect(pauseState(some, ["app", "doc"], "default", "doing")).toEqual({ workspaces: ["app"], paused: true });
    expect(pauseState(some, ["app", "lib"], "default", "todo")).toEqual({ workspaces: [], paused: false });
  });
});

describe("boardNames", () => {
  it("sorts by name and merges the boards of several workspaces", () => {
    expect(boardNames(pipelines, ["app"])).toEqual(["default", "research"]);
    expect(boardNames(pipelines, ["lib"])).toEqual(["default"]);
    expect(boardNames({ x: [board("zeta", "a"), board("default", "a"), board("alpha", "a")] }, ["x"])).toEqual(["alpha", "default", "zeta"]);
    expect(boardNames(undefined, ["app"])).toEqual([]);
  });
});

describe("columnsFor", () => {
  it("keeps one board's order and merges several workspaces by stage name", () => {
    expect(columnsFor(pipelines, ["app"], "default").map((c) => c.name)).toEqual(["todo", "doing", "done"]);
    expect(columnsFor(pipelines, ["app", "lib"], "default").map((c) => c.name)).toEqual(["todo", "doing", "done", "review"]);
    expect(columnsFor(pipelines, ["app", "lib"], "research").map((c) => c.name)).toEqual(["idea", "done"]);
  });

  it("uses the label, else the name with spaces, and skips workspaces without a valid file", () => {
    const labelled: Pipelines = {
      app: [{ name: "default", pipeline: { repo: "/r/app", order: [], entry: "a_b", stages: [stage("a_b"), stage("c", "See")] }, error: null, watched: [], paused: [] }],
      bad: [{ name: "default", pipeline: null, error: "broken", watched: [], paused: [] }],
    };
    expect(columnsFor(labelled, ["app", "bad", "missing"], "default")).toEqual([
      { name: "a_b", label: "a b", final: true },
      { name: "c", label: "See", final: true },
    ]);
    expect(columnsFor(undefined, ["app"], "default")).toEqual([]);
  });

  it("marks a column final only when no workspace has a next stage for it", () => {
    const flow: Pipelines = {
      app: [{ name: "default", pipeline: { repo: "/r/app", order: [], entry: "a", stages: [{ ...stage("a"), next: ["b"] }, stage("b")] }, error: null, watched: [], paused: [] }],
      lib: [{ name: "default", pipeline: { repo: "/r/app", order: [], entry: "b", stages: [{ ...stage("b"), next: ["c"] }, stage("c")] }, error: null, watched: [], paused: [] }],
    };
    expect(columnsFor(flow, ["app"], "default").map((c) => c.final)).toEqual([false, true]);
    expect(columnsFor(flow, ["app", "lib"], "default").map((c) => c.final)).toEqual([false, false, true]);
  });
});

describe("shownBoard", () => {
  it("is the item's board, or the workspace's first board when it has no such board", () => {
    expect(shownBoard(item({ board: "research" }), pipelines)).toBe("research");
    expect(shownBoard(item({ board: "gone" }), pipelines)).toBe("default");
    expect(shownBoard(item({ board: "" }), pipelines)).toBe("default");
    expect(shownBoard(item({ workspace: "lib", board: "research" }), pipelines)).toBe("default");
    expect(shownBoard(item({ workspace: "x", board: "zeta" }), { x: [board("alpha", "a")] })).toBe("alpha");
  });
});

describe("openDependencies", () => {
  const flow: Pipelines = {
    app: [{ name: "api", pipeline: { repo: "/r/api", order: [], entry: "dev", stages: [{ ...stage("dev"), next: ["pr"] }, { ...stage("pr"), next: ["done"], on_merge: "done" }, stage("done")] }, error: null, watched: [], paused: [] }],
  };
  it("lists the dependencies that are neither merged nor in their board's on_merge stage", () => {
    const items = [
      item({ dir: "A-1", board: "api", stage: "pr" }),
      item({ dir: "A-2", board: "api", stage: "done" }),
      item({ dir: "A-3", board: "api", stage: "pr", review: { state: "merged", approval: "approved", approved_by_me: false, merge: "unknown", ci: "none", failed: [] } }),
      item({ dir: "A-4", workspace: "other", board: "api", stage: "pr" }),
    ];
    const ui = item({ dir: "U-1", depends_on: ["A-1", "A-2", "A-3", "A-4", "A-gone"] });
    expect(openDependencies(ui, items, flow).map((i) => i.dir)).toEqual(["A-1"]);
    expect(openDependencies(item({}), items, flow)).toEqual([]);
  });
});

describe("boardLabel", () => {
  it("is the board's label, else its name", () => {
    const labelled: Pipelines = { x: [{ ...board("dev", "a"), pipeline: { ...board("dev", "a").pipeline, label: "Storyrails - Develop" } }] };
    expect(boardLabel(labelled, "x", "dev")).toBe("Storyrails - Develop");
    expect(boardLabel(labelled, "x", "gone")).toBe("gone");
    expect(boardLabel(pipelines, "app", "research")).toBe("research");
  });
});

describe("repoName", () => {
  it("is the last folder of the path", () => {
    expect(repoName("/Users/me/Projects/storyfront")).toBe("storyfront");
    expect(repoName("/r/api/")).toBe("api");
  });
});

describe("byStage", () => {
  it("groups the board's items by stage and puts a stage missing from their own board in the unknown column", () => {
    const items = [
      item({ dir: "a", stage: "doing" }),
      item({ dir: "b", stage: "weird" }),
      item({ dir: "c", workspace: "lib", stage: "doing" }),
      item({ dir: "d", workspace: "nobody", stage: "todo" }),
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

describe("cardChildren", () => {
  it("lists the parent's children outside its column, blocked first", () => {
    const parent = item({ dir: "p", board: "features", stage: "children" });
    const rows = cardChildren(parent, [
      parent,
      item({ dir: "a", title: "A", parent: "p", board: "fe", stage: "analyze" }),
      item({ dir: "b", title: "B", parent: "p", board: "be", stage: "pr", agent_status: "blocked: ci" }),
      item({ dir: "same", parent: "p", board: "features", stage: "children" }),
      item({ dir: "other", parent: "p", board: "fe", workspace: "lib" }),
      item({ dir: "stranger", parent: "q", board: "fe" }),
    ]);
    expect(rows.map((i) => i.dir)).toEqual(["b", "a"]);
  });
});

describe("orderFor", () => {
  it("uses the stage's order, else the board's", () => {
    const pipelines: Pipelines = {
      app: [{ name: "default", pipeline: { repo: "/r/app", order: ["title desc"], entry: "a", stages: [{ ...stage("a"), order: ["issue_status desc"] }, stage("b")] }, error: null, watched: [], paused: [] }],
    };
    expect(orderFor(pipelines, ["app"], "default", "a")).toEqual(["issue_status desc"]);
    expect(orderFor(pipelines, ["app"], "default", "b")).toEqual(["title desc"]);
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

  it("sorts by the board's order, Jira categories in workflow order, empty values last, title breaking ties", () => {
    const rows = treeOrder([
      item({ dir: "none", title: "A" }),
      item({ dir: "done", title: "B", issue_status_category: "done" }),
      item({ dir: "prog", title: "C", issue_status_category: "indeterminate" }),
      item({ dir: "todo", title: "D", issue_status_category: "new" }),
      item({ dir: "todo2", title: "A", issue_status_category: "new" }),
    ], ["issue_status_category"]);
    expect(rows.map((r) => r.item.dir)).toEqual(["todo2", "todo", "prog", "done", "none"]);
  });

  it("sorts by several fields with a direction each, empty values last either way", () => {
    const rows = treeOrder([
      item({ dir: "none", title: "A" }),
      item({ dir: "deploy", title: "B", issue_status_category: "done", issue_status: "Waiting for Deployment" }),
      item({ dir: "closed", title: "C", issue_status_category: "done", issue_status: "Closed" }),
      item({ dir: "todo", title: "D", issue_status_category: "new", issue_status: "To Do" }),
    ], ["issue_status_category desc", "issue_status desc"]);
    expect(rows.map((r) => r.item.dir)).toEqual(["deploy", "closed", "todo", "none"]);
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

describe("waiting", () => {
  it("is its own state, not blocked", () => {
    const waiting = item({ agent_status: "waiting: CI on PR #12" });
    expect(isWaiting(waiting)).toBe(true);
    expect(isBlocked(waiting)).toBe(false);
    expect(waitingFor(waiting)).toBe("CI on PR #12");
    expect(isWaiting(item({ agent_status: "working" }))).toBe(false);
  });
});

describe("issueCategory", () => {
  it("maps the Jira status category keys and nothing else", () => {
    expect(issueCategory("new")).toBe("todo");
    expect(issueCategory("indeterminate")).toBe("progress");
    expect(issueCategory("done")).toBe("done");
    expect(issueCategory("")).toBeUndefined();
    expect(issueCategory("undefined")).toBeUndefined();
    expect(issueCategory("toString")).toBeUndefined();
  });
});

describe("readyToMerge", () => {
  const review: Review = { state: "open", approval: "approved", approved_by_me: false, merge: "mergeable", ci: "passing", failed: [] };

  it("needs an open, approved, mergeable PR with passing CI", () => {
    expect(readyToMerge(item({ review }))).toBe(true);
    expect(readyToMerge(item({ review: null }))).toBe(false);
    expect(readyToMerge(item({ review: { ...review, state: "merged" } }))).toBe(false);
    expect(readyToMerge(item({ review: { ...review, approval: "review_required" } }))).toBe(false);
    expect(readyToMerge(item({ review: { ...review, merge: "conflicting" } }))).toBe(false);
    expect(readyToMerge(item({ review: { ...review, ci: "pending" } }))).toBe(false);
  });
});

describe("newlyMatching", () => {
  it("returns matching items that did not match before", () => {
    const seen = new Set(["app/old"]);
    const items = [item({ dir: "old", issue_status_category: "done" }), item({ dir: "fresh", issue_status_category: "done" }), item({ dir: "open", issue_status_category: "indeterminate" })];
    expect(newlyMatching(seen, items, issueDone).map((i) => i.dir)).toEqual(["fresh"]);
  });
});
