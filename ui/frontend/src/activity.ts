import { blockedKey, isBlocked, isFinal, itemKey } from "./boardState";
import { Item, Pipelines } from "./types";

export type ActivityKind = "approved" | "blocked";

/** One thing that happened to `item` between two polls; `text` is the plain line of its toast. */
export type ActivityEvent = { kind: ActivityKind; item: Item; text: string };

/**
 * What changed from `before` to `after`, the items of two polls: a PR became approved, or an item became blocked (once per
 * reason). Nothing on the first poll (`before` undefined), and nothing from a PR's first reading, so a restart or a newly
 * linked PR stays quiet.
 */
export function activityEvents(before: Item[] | undefined, after: Item[]): ActivityEvent[] {
  if (!before) return [];
  const old = new Map(before.map((item) => [itemKey(item), item]));
  const events: ActivityEvent[] = [];
  for (const item of after) {
    const was = old.get(itemKey(item));
    if (!was) continue;
    if (was.review && item.review && was.review.approval !== item.review.approval && item.review.approval === "approved") {
      events.push({ kind: "approved", item, text: "PR approved" });
    }
    if (isBlocked(item) && blockedKey(was) !== blockedKey(item)) {
      events.push({ kind: "blocked", item, text: `Blocked: ${item.agent_status.replace(/^blocked:?\s*/, "") || "needs you"}` });
    }
  }
  return events;
}

/** Whether an item moved into a final stage of its board between two polls; it gets confetti, not a toast. Never on the first poll. */
export function anyDone(before: Item[] | undefined, after: Item[], pipelines: Pipelines | undefined): boolean {
  if (!before) return false;
  const old = new Map(before.map((item) => [itemKey(item), item]));
  return after.some((item) => {
    const was = old.get(itemKey(item));
    return !!was && was.stage !== item.stage && isFinal(item, pipelines);
  });
}
