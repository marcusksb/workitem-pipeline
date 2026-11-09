import { describe, expect, it } from "vitest";
import { activityEvents } from "./activity";
import { item, stage } from "./testFixtures";
import { Pipelines, Review } from "./types";

const flow: Pipelines = {
  app: [{ name: "default", pipeline: { repo: "/r/app", order: [], entry: "todo", stages: [{ ...stage("todo"), next: ["in_qa"] }, { ...stage("in_qa", "Ready for QA"), next: ["done"] }, stage("done")] }, error: null, watched: [], paused: [] }],
};
const review = (over: Partial<Review> = {}): Review => ({ state: "open", approval: "review_required", approved_by_me: false, merge: "mergeable", ci: "pending", failed: [], ...over });
const events = (before: Parameters<typeof item>[0], after: Parameters<typeof item>[0]) =>
  activityEvents([item(before)], [item(after)], flow).map((e) => [e.kind, e.text]);

describe("activityEvents", () => {
  it("stays quiet on the first poll and when nothing changed", () => {
    expect(activityEvents(undefined, [item({})], flow)).toEqual([]);
    expect(events({ review: review() }, { review: review() })).toEqual([]);
  });

  it("reports added and removed items", () => {
    const got = activityEvents([item({ dir: "gone" })], [item({ dir: "new" })], flow).map((e) => [e.kind, e.item.dir]);
    expect(got).toEqual([["added", "new"], ["removed", "gone"]]);
  });

  it("reports moves with the column title, and a move into a final stage as done", () => {
    expect(events({ stage: "todo" }, { stage: "in_qa" })).toEqual([["moved", "Moved to Ready for QA"]]);
    expect(events({ stage: "in_qa" }, { stage: "done" })).toEqual([["done", "Done"]]);
  });

  it("reports PR changes, but not the first reading of a PR", () => {
    expect(events({ review: review() }, { review: review({ approval: "approved" }) })).toEqual([["approved", "PR approved"]]);
    expect(events({ review: review() }, { review: review({ approval: "changes_requested" }) })).toEqual([["changes_requested", "Changes requested"]]);
    expect(events({ review: review() }, { review: review({ ci: "failing", failed: ["rspec"] }) })).toEqual([["ci_red", "CI failed: rspec"]]);
    expect(events({ review: review({ ci: "failing" }) }, { review: review({ ci: "passing" }) })).toEqual([["ci_green", "CI green"]]);
    expect(events({ review: review() }, { review: review({ merge: "conflicting" }) })).toEqual([["conflict", "Merge conflict"]]);
    expect(events({ review: review() }, { review: review({ state: "merged" }) })).toEqual([["merged", "PR merged"]]);
    expect(events({ review: review() }, { review: review({ state: "closed" }) })).toEqual([["closed", "PR closed"]]);
    expect(events({ review: null }, { review: review({ approval: "approved", ci: "failing" }) })).toEqual([]);
  });

  it("reports Jira status changes, but not the first sync", () => {
    expect(events({ issue_status: "In Progress" }, { issue_status: "Waiting for Deployment" })).toEqual([["jira", "Jira: Waiting for Deployment"]]);
    expect(events({ issue_status: "" }, { issue_status: "In Progress" })).toEqual([]);
  });

  it("reports a block once per reason", () => {
    expect(events({ agent_status: "working" }, { agent_status: "blocked: needs input" })).toEqual([["blocked", "Blocked: needs input"]]);
    expect(events({ agent_status: "blocked: a" }, { agent_status: "blocked: b" })).toEqual([["blocked", "Blocked: b"]]);
    expect(events({ agent_status: "blocked: a" }, { agent_status: "blocked: a" })).toEqual([]);
    expect(events({ agent_status: "ok" }, { agent_status: "working" })).toEqual([]);
  });
});
