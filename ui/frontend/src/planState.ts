import { Plan } from "./types";

export type PlanStatus = Plan["status"];

/** What to show: a plan without a running session has ended, whatever its record still says
 * (the dispatcher writes `ended` on its next pass). */
export function planStatus(plan: Plan): PlanStatus {
  return plan.live ? (plan.status === "ended" ? "working" : plan.status) : "ended";
}

/** Plans of the shown projects: open ones first, newest first, and done ones (a workitem was made)
 * apart, unless their session still runs. */
export function groupPlans(plans: Plan[], projects: string[]) {
  const shown = plans.filter((plan) => projects.includes(plan.project)).sort((a, b) => b.created - a.created);
  const isDone = (plan: Plan) => !!plan.workitem && !plan.live;
  return { open: shown.filter((plan) => !isDone(plan)), done: shown.filter(isDone) };
}

/** One line for the collapsed bar: `3 · 1 waiting`. */
export function planCounts(open: Plan[]): string {
  const waiting = open.filter((plan) => planStatus(plan) === "waiting").length;
  return [String(open.length), ...(waiting ? [`${waiting} waiting`] : [])].join(" · ");
}
