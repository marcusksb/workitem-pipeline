import { Plan } from "./types";

export type PlanStatus = Plan["status"];

/** What to show: a plan without a running session has ended, whatever its record still says
 * (the dispatcher writes `ended` on its next pass). */
export function planStatus(plan: Plan): PlanStatus {
  return plan.live ? (plan.status === "ended" ? "working" : plan.status) : "ended";
}

/** Plans of the shown workspaces: open ones first, newest first, and done ones (a workitem was made)
 * apart, unless their session still runs. */
export function groupPlans(plans: Plan[], workspaces: string[]) {
  const shown = plans.filter((plan) => workspaces.includes(plan.workspace)).sort((a, b) => b.created - a.created);
  const isDone = (plan: Plan) => plan.workitems.length > 0 && !plan.live;
  return { open: shown.filter((plan) => !isDone(plan)), done: shown.filter(isDone) };
}

/** How a status reads: a plan record's `waiting` means it waits for you, unlike a workitem's. */
export const planLabel = (status: PlanStatus) => (status === "waiting" ? "needs input" : status);

/** One line for the collapsed bar: `3 · 1 needs input`. */
export function planCounts(open: Plan[]): string {
  const waiting = open.filter((plan) => planStatus(plan) === "waiting").length;
  return [String(open.length), ...(waiting ? [`${waiting} ${planLabel("waiting")}`] : [])].join(" · ");
}
