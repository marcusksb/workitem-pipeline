import { describe, expect, it } from "vitest";
import { groupPlans, planCounts, planStatus } from "./planState";
import { Plan } from "./types";

const plan = (over: Partial<Plan>): Plan => ({
  workspace: "app",
  id: "a",
  topic: "t",
  summary: "",
  status: "working",
  workitems: [],
  created: 0,
  has_plan: false,
  live: true,
  ...over,
});

describe("planStatus", () => {
  it("is ended without a session, whatever the record says", () => {
    expect(planStatus(plan({ status: "waiting", live: false }))).toBe("ended");
    expect(planStatus(plan({ status: "waiting" }))).toBe("waiting");
    expect(planStatus(plan({ status: "ended" }))).toBe("working");
  });
});

describe("groupPlans", () => {
  it("keeps the shown workspaces, newest first, and sets done plans apart", () => {
    const plans = [
      plan({ id: "old", created: 1 }),
      plan({ id: "new", created: 2 }),
      plan({ id: "other", workspace: "web" }),
      plan({ id: "done", workitems: ["X-1"], live: false }),
      plan({ id: "made-but-running", workitems: ["X-2"] }),
    ];
    const { open, done } = groupPlans(plans, ["app"]);
    expect(open.map((p) => p.id)).toEqual(["new", "old", "made-but-running"]);
    expect(done.map((p) => p.id)).toEqual(["done"]);
  });
});

describe("planCounts", () => {
  it("adds the waiting count only when there is one", () => {
    expect(planCounts([plan({}), plan({ status: "waiting" })])).toBe("2 · 1 needs input");
    expect(planCounts([plan({})])).toBe("1");
  });
});
