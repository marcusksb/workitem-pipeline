import { blockedKey, isBlocked, isFinal, itemKey } from "./boardState";
import { Item, Pipelines } from "./types";

export type ActivityKind = "done" | "approved" | "blocked";

/** One thing that happened to `item` between two polls; `text` is the plain line of its toast. */
export type ActivityEvent = { kind: ActivityKind; item: Item; text: string };

/**
 * What changed from `before` to `after`, the items of two polls: an item reached a final stage of its board, a PR became approved, or an item became blocked (once per
 * reason). Nothing on the first poll (`before` undefined), and nothing from a PR's first reading, so a restart or a newly
 * linked PR stays quiet.
 */
export function activityEvents(before: Item[] | undefined, after: Item[], pipelines: Pipelines | undefined): ActivityEvent[] {
  if (!before) return [];
  const old = new Map(before.map((item) => [itemKey(item), item]));
  const events: ActivityEvent[] = [];
  for (const item of after) {
    const was = old.get(itemKey(item));
    if (!was) continue;
    if (was.stage !== item.stage && isFinal(item, pipelines)) events.push({ kind: "done", item, text: "Done" });
    if (was.review && item.review && was.review.approval !== item.review.approval && item.review.approval === "approved") {
      events.push({ kind: "approved", item, text: "PR approved" });
    }
    if (isBlocked(item) && blockedKey(was) !== blockedKey(item)) {
      events.push({ kind: "blocked", item, text: `Blocked: ${item.agent_status.replace(/^blocked:?\s*/, "") || "needs you"}` });
    }
  }
  return events;
}

