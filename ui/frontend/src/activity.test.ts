import { describe, expect, it } from "vitest";
import { activityEvents } from "./activity";
import { item, stage } from "./testFixtures";
import { Pipelines, Review } from "./types";

const flow: Pipelines = {
  app: [{ name: "default", pipeline: { repo: "/r/app", order: [], entry: "todo", stages: [{ ...stage("todo"), next: ["done"] }, stage("done")] }, error: null, watched: [], paused: [] }],
};

const review = (over: Partial<Review> = {}): Review => ({ state: "open", approval: "review_required", approved_by_me: false, merge: "mergeable", ci: "pending", failed: [], ...over });
const events = (before: Parameters<typeof item>[0], after: Parameters<typeof item>[0]) =>
  activityEvents([item(before)], [item(after)], flow).map((e) => [e.kind, e.text]);

describe("activityEvents", () => {
  it("stays quiet on the first poll and when nothing changed", () => {
    expect(activityEvents(undefined, [item({})], flow)).toEqual([]);
    expect(events({ review: review() }, { review: review() })).toEqual([]);
  });

  it("stays quiet on everything but done, approvals and blocks", () => {
    expect(activityEvents([item({ dir: "gone" })], [item({ dir: "new", stage: "done" })], flow)).toEqual([]);
    expect(events({ stage: "done" }, { stage: "todo" })).toEqual([]);
    expect(events({ review: review() }, { review: review({ approval: "changes_requested", ci: "failing", merge: "conflicting" }) })).toEqual([]);
    expect(events({ review: review() }, { review: review({ state: "merged" }) })).toEqual([]);
    expect(events({ issue_status: "In Progress" }, { issue_status: "Done" })).toEqual([]);
  });

  it("reports a move into a final stage as done", () => {
    expect(events({ stage: "todo" }, { stage: "done" })).toEqual([["done", "Done"]]);
  });

  it("reports an approved PR, but not the first reading of a PR", () => {
    expect(events({ review: review() }, { review: review({ approval: "approved" }) })).toEqual([["approved", "PR approved"]]);
    expect(events({ review: review({ approval: "approved" }) }, { review: review({ approval: "approved", ci: "passing" }) })).toEqual([]);
    expect(events({ review: null }, { review: review({ approval: "approved" }) })).toEqual([]);
  });

  it("reports a block once per reason", () => {
    expect(events({ agent_status: "working" }, { agent_status: "blocked: needs input" })).toEqual([["blocked", "Blocked: needs input"]]);
    expect(events({ agent_status: "blocked: a" }, { agent_status: "blocked: b" })).toEqual([["blocked", "Blocked: b"]]);
    expect(events({ agent_status: "blocked: a" }, { agent_status: "blocked: a" })).toEqual([]);
    expect(events({ agent_status: "ok" }, { agent_status: "working" })).toEqual([]);
  });
});
