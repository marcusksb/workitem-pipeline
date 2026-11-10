import { isBlocked } from "./boardState";
import { planStatus } from "./planState";
import { AgentSession, Item, Plan } from "./types";

/** Something that waits for you, most urgent rule first (decision 60). */
export type InboxEntry =
  | { kind: "item"; rank: number; reason: string; item: Item }
  | { kind: "plan"; rank: number; reason: string; plan: Plan }
  | { kind: "session"; rank: number; reason: string; session: AgentSession };

const workspaceOf = (entry: InboxEntry) =>
  entry.kind === "item" ? entry.item.workspace : entry.kind === "plan" ? entry.plan.workspace : entry.session.workspace;

/** Why an item needs you, with its rank, or nothing. A PR item with a running session is being fixed. */
function itemReason(item: Item): [number, string] | undefined {
  if (isBlocked(item)) return [0, item.agent_status.replace(/^blocked:?\s*/, "") || "blocked"];
  if (item.gate && !item.approved) return [1, item.gate.label || "waiting for approval"];
  const review = item.review;
  if (review?.state === "open" && !item.session) {
    if (review.approval === "changes_requested") return [3, "changes requested"];
    if (review.ci === "failing") return [3, "CI failing"];
  }
  return undefined;
}

/** Every workspace's entries, grouped by workspace (in first-seen order), each group most urgent first. */
export function inbox(items: Item[], plans: Plan[], sessions: AgentSession[]): [string, InboxEntry[]][] {
  const entries: InboxEntry[] = [];
  for (const item of items) {
    const reason = itemReason(item);
    if (reason) entries.push({ kind: "item", rank: reason[0], reason: reason[1], item });
  }
  for (const plan of plans) {
    if (planStatus(plan) === "waiting") entries.push({ kind: "plan", rank: 2, reason: "plan needs input", plan });
  }
  for (const session of sessions) entries.push({ kind: "session", rank: 2, reason: "create session running", session });
  const groups = new Map<string, InboxEntry[]>();
  for (const entry of entries) {
    const workspace = workspaceOf(entry);
    groups.set(workspace, [...(groups.get(workspace) ?? []), entry]);
  }
  return [...groups].map(([workspace, group]) => [workspace, group.sort((a, b) => a.rank - b.rank)]);
}

export const inboxCount = (groups: [string, InboxEntry[]][]) => groups.reduce((n, [, group]) => n + group.length, 0);
