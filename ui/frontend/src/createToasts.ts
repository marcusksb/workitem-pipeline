import { AgentSession, Item } from "./types";

/** Item polls a gone session waits for its items before it counts as ended without one. */
const GRACE_POLLS = 2;

/**
 * What a `wi-new-*` create session's toast shows (decision 75): `running`, `waiting` (the agent waits for an answer), `created` (the session is gone
 * and wrote `items`), `ended` (it died, or it is gone and no item names it after a few item polls).
 */
export type CreateToast = {
  session: AgentSession;
  state: "running" | "waiting" | "created" | "ended";
  items: Item[];
  /** Item polls since the session went away without a matching item. */
  misses: number;
};

const sameSession = (a: AgentSession, b: AgentSession) => a.workspace === b.workspace && a.name === b.name;

/**
 * The create toasts after a poll. `itemsPolled` says `items` is a new item poll, which is what a gone session's grace counts. A `created` or
 * `ended` toast stays as it is; the caller removes it once it is dismissed.
 */
export function nextCreateToasts(previous: CreateToast[], sessions: AgentSession[], items: Item[] | undefined, itemsPolled: boolean): CreateToast[] {
  const next = previous.map((toast): CreateToast => {
    if (toast.state === "created" || toast.state === "ended") return toast;
    const live = sessions.find((s) => sameSession(s, toast.session));
    if (live) return { ...toast, session: live, state: live.dead ? "ended" : live.waiting ? "waiting" : "running", misses: 0 };
    const made = (items ?? []).filter((item) => item.workspace === toast.session.workspace && item.created_by === toast.session.name);
    if (made.length > 0) return { ...toast, state: "created", items: made };
    const misses = toast.misses + (itemsPolled ? 1 : 0);
    return { ...toast, state: misses >= GRACE_POLLS ? "ended" : toast.state, misses };
  });
  const fresh = sessions
    .filter((s) => !previous.some((toast) => sameSession(toast.session, s)))
    .map((session): CreateToast => ({ session, state: session.dead ? "ended" : session.waiting ? "waiting" : "running", items: [], misses: 0 }));
  return [...next, ...fresh];
}

/** Closing a toast with × kills its session only when it ended dead: the countdown never kills, so the terminal link to the crash output survives it. */
export const killsOnClose = (toast: CreateToast) => toast.state === "ended" && toast.session.dead;
