import { blockedKey, isBlocked, isFinal, itemKey, stageTitle } from "./boardState";
import { Item, Pipelines } from "./types";

export type ActivityKind =
  | "added"
  | "removed"
  | "moved"
  | "done"
  | "approved"
  | "changes_requested"
  | "ci_red"
  | "ci_green"
  | "merged"
  | "closed"
  | "conflict"
  | "jira"
  | "blocked";

/** One thing that happened to `item` between two polls; `text` is the plain line of its toast. */
export type ActivityEvent = { kind: ActivityKind; item: Item; text: string };

/**
 * What changed from `before` to `after`, the items of two polls. Nothing on the first poll (`before`
 * undefined), and no PR or Jira event from a first reading (no review, empty status) so a restart or
 * a newly linked PR stays quiet.
 */
export function activityEvents(before: Item[] | undefined, after: Item[], pipelines: Pipelines | undefined): ActivityEvent[] {
  if (!before) return [];
  const old = new Map(before.map((item) => [itemKey(item), item]));
  const events: ActivityEvent[] = [];
  const add = (kind: ActivityKind, item: Item, text: string) => events.push({ kind, item, text });
  for (const item of after) {
    const was = old.get(itemKey(item));
    old.delete(itemKey(item));
    if (!was) {
      add("added", item, "New item");
      continue;
    }
    if (was.stage !== item.stage) {
      if (isFinal(item, pipelines)) add("done", item, "Done");
      else add("moved", item, `Moved to ${stageTitle(item, pipelines)}`);
    }
    const [prev, now] = [was.review, item.review];
    if (prev && now) {
      if (prev.state !== now.state && now.state === "merged") add("merged", item, "PR merged");
      if (prev.state !== now.state && now.state === "closed") add("closed", item, "PR closed");
      if (prev.approval !== now.approval && now.approval === "approved") add("approved", item, "PR approved");
      if (prev.approval !== now.approval && now.approval === "changes_requested") add("changes_requested", item, "Changes requested");
      if (prev.ci !== now.ci && now.ci === "failing") add("ci_red", item, now.failed.length ? `CI failed: ${now.failed.join(", ")}` : "CI failed");
      if (prev.ci !== now.ci && now.ci === "passing") add("ci_green", item, "CI green");
      if (prev.merge !== now.merge && now.merge === "conflicting") add("conflict", item, "Merge conflict");
    }
    if (was.issue_status && item.issue_status && was.issue_status !== item.issue_status) add("jira", item, `Jira: ${item.issue_status}`);
    if (isBlocked(item) && blockedKey(was) !== blockedKey(item)) add("blocked", item, `Blocked: ${item.agent_status.replace(/^blocked:?\s*/, "") || "needs you"}`);
  }
  for (const item of old.values()) add("removed", item, "Removed");
  return events;
}
