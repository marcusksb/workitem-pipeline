import { Item } from "./types";

/** An item with every field empty or off, overridden by `over`. */
export const item = (over: Partial<Item>): Item => ({
  workspace: "app",
  dir: "x",
  title: "x",
  kind: "bug",
  stage: "backlog",
  board: "default",
  approved: false,
  agent_status: "ok",
  issue: "none",
  issue_status: "",
  issue_status_category: "",
  pr: "",
  parent: "",
  depends_on: [],
  created: "",
  session: false,
  gate: null,
  pr_watch: false,
  develops: false,
  review: null,
  unpushed: null,
  follow_ups: [],
  ...over,
});

/** A stage with no handler, roles or order. */
export const stage = (name: string, label: string | null = null) => ({ name, label, handler: null, auto: false, next: [], gate: null, on_merge: null, order: [] as string[] });
