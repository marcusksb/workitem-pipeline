import { describe, expect, it } from "vitest";
import { inbox, inboxCount } from "./inboxState";
import { item } from "./testFixtures";
import { Plan, Review } from "./types";

const gate = { label: "Approve brief", approve_to: "dev", reject_to: "analyze" };
const review = (over: Partial<Review>): Review => ({ state: "open", approval: "review_required", approved_by_me: false, merge: "mergeable", ci: "passing", failed: [], ...over });
const plan = (over: Partial<Plan>): Plan => ({ workspace: "app", id: "1", topic: "t", summary: "", status: "working", workitems: [], created: 0, has_plan: false, live: true, ...over });

describe("inbox", () => {
  it("lists what needs you, most urgent first, grouped by workspace", () => {
    const groups = inbox(
      [
        item({ dir: "pr", review: review({ ci: "failing" }) }),
        item({ dir: "gate", gate }),
        item({ dir: "blocked", agent_status: "blocked: which option?" }),
        item({ dir: "other", workspace: "web", review: review({ approval: "changes_requested" }) }),
      ],
      [plan({ status: "waiting" })],
      [{ workspace: "web", name: "wi-new-1" }],
    );
    const shown = groups.map(([workspace, entries]) => [workspace, entries.map((e) => e.reason)]);
    expect(shown).toEqual([
      ["app", ["which option?", "Approve brief", "plan needs input", "CI failing"]],
      ["web", ["create session running", "changes requested"]],
    ]);
    expect(inboxCount(groups)).toBe(6);
  });

  it("leaves out working, waiting, approved, and PR items a fixer is working on", () => {
    const groups = inbox(
      [
        item({ agent_status: "working" }),
        item({ agent_status: "waiting: CI" }),
        item({ gate, approved: true }),
        item({ review: review({ ci: "failing" }), session: true }),
      ],
      [plan({ status: "waiting", live: false }), plan({ status: "working" })],
      [],
    );
    expect(groups).toEqual([]);
  });
});
