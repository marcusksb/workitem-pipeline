import confetti from "canvas-confetti";
import { AnimationEvent, useEffect, useRef, useState } from "react";
import { ActivityEvent, ActivityKind, activityEvents } from "./activity";
import { agentTerminal, api } from "./api";
import { alertsFor } from "./boardState";
import { CreateToast, killsOnClose, nextCreateToasts } from "./createToasts";
import { QUIPS, quipFor } from "./quips";
import { AgentSession, Item, Pipelines } from "./types";
import { beep } from "./useAlerts";

/** Toasts on screen at once; older ones wait behind "+N more". */
const SHOWN = 4;
/** A poll with more events than this becomes one summary toast. */
const BURST = 4;

type Toast = { id: number; events: ActivityEvent[]; quip: string; sticky: boolean; leaving: boolean };

const tone = (kind: ActivityKind) => (kind === "blocked" ? "tone-bad" : "tone-good");

const sessionKey = (session: AgentSession) => `${session.workspace}/${session.name}`;

/**
 * A toast for each item done, PR approved or item newly blocked between two polls (see `activityEvents`), bottom left of the board, clear of the detail panel,
 * newest on top, with confetti when an item is done. Toasts leave when their countdown bar runs out (paused while hovered); blocked ones stay. Clicking one calls `onOpen`.
 * The item open in the detail panel (`openKey`) gets no blocked toast while the page has focus.
 * Above them, outside the "+N more" count, one toast per `wi-new-*` create session (decision 75, see `nextCreateToasts`); × on the toast of a dead one kills it (`killsOnClose`).
 */
export function ActivityToasts({ items, sessions, pipelines, onOpen, openKey }: { items: Item[] | undefined; sessions: AgentSession[] | undefined; pipelines: Pipelines | undefined; onOpen: (item: Item) => void; openKey: string | undefined }) {
  const before = useRef<Item[]>(undefined);
  const nextId = useRef(0);
  const [toasts, setToasts] = useState<Toast[]>([]);
  // Read when a poll arrives, so opening another item does not rerun the effect.
  const openKeyRef = useRef(openKey);
  openKeyRef.current = openKey;

  useEffect(() => {
    if (!items) return;
    const alerts = alertsFor(openKeyRef.current, document.hasFocus());
    const events = activityEvents(before.current, items, pipelines).filter((e) => e.kind !== "blocked" || alerts(e.item));
    if (events.some((e) => e.kind === "done")) {
      void confetti({ particleCount: 160, spread: 100, startVelocity: 60, origin: { x: 0.5, y: 1 }, disableForReducedMotion: true });
    }
    before.current = items;
    if (events.length === 0) return;
    const groups = events.length > BURST ? [events] : events.map((e) => [e]);
    const fresh = groups.map((group) => ({
      id: nextId.current++,
      events: group,
      quip: group.length === 1 ? quipFor(group[0].kind) : "A lot happened while you blinked.",
      sticky: group.some((e) => e.kind === "blocked"),
      leaving: false,
    }));
    setToasts((shown) => [...shown, ...fresh]);
  }, [items, pipelines]);

  const [creates, setCreates] = useState<CreateToast[]>([]);
  const createsRef = useRef<CreateToast[]>([]);
  const leftCreates = useRef(new Set<string>());
  const [leavingCreates, setLeavingCreates] = useState<string[]>([]);
  const polledItems = useRef<Item[]>(undefined);

  useEffect(() => {
    if (!sessions) return;
    const itemsPolled = items !== polledItems.current;
    polledItems.current = items;
    const previous = createsRef.current;
    const next = nextCreateToasts(previous, sessions.filter((s) => !leftCreates.current.has(sessionKey(s))), items, itemsPolled);
    const startsWaiting = (toast: CreateToast) => toast.state === "waiting" && previous.find((p) => sessionKey(p.session) === sessionKey(toast.session))?.state !== "waiting";
    if (next.some(startsWaiting)) beep();
    createsRef.current = next;
    setCreates(next);
  }, [sessions, items]);

  const dismissCreate = (key: string) => setLeavingCreates((leaving) => (leaving.includes(key) ? leaving : [...leaving, key]));
  const closeCreate = (toast: CreateToast) => {
    dismissCreate(sessionKey(toast.session));
    if (killsOnClose(toast)) api.killAgentSession(toast.session).catch((e) => console.error("killing the create session failed", e));
  };
  const createEnded = (key: string) => (e: AnimationEvent) => {
    if (e.animationName === "toast-countdown") dismissCreate(key);
    if (e.animationName !== "toast-out") return;
    leftCreates.current.add(key);
    createsRef.current = createsRef.current.filter((t) => sessionKey(t.session) !== key);
    setCreates(createsRef.current);
    setLeavingCreates((leaving) => leaving.filter((k) => k !== key));
  };

  const dismiss = (id: number) => setToasts((shown) => shown.map((t) => (t.id === id ? { ...t, leaving: true } : t)));
  const ended = (id: number) => (e: AnimationEvent) => {
    if (e.animationName === "toast-countdown") dismiss(id);
    if (e.animationName === "toast-out") setToasts((shown) => shown.filter((t) => t.id !== id));
  };
  const open = (toast: Toast, item: Item) => {
    dismiss(toast.id);
    onOpen(item);
  };

  if (toasts.length === 0 && creates.length === 0) return null;
  const hidden = toasts.length - SHOWN;
  return (
    <div className="activity-toasts" aria-live="polite">
      {creates.map((toast) => {
        const key = sessionKey(toast.session);
        return (
          <div key={key} className={`toast-wrap${leavingCreates.includes(key) ? " leaving" : ""}`} onAnimationEnd={createEnded(key)}>
            <CreateToastView toast={toast} leaving={leavingCreates.includes(key)} onDismiss={() => closeCreate(toast)} onOpen={(item) => {
              dismissCreate(key);
              onOpen(item);
            }} />
          </div>
        );
      })}
      {toasts.slice(-SHOWN).reverse().map((toast) => {
        const [first] = toast.events;
        const single = toast.events.length === 1;
        return (
          <div key={toast.id} className={`toast-wrap${toast.leaving ? " leaving" : ""}`} onAnimationEnd={ended(toast.id)}>
            <div className={`toast ${single ? tone(first.kind) : ""}`} role="status">
              <div className="toast-inner">
                <span className={`toast-icon icon-${single ? first.kind : "burst"}`} aria-hidden>
                  {single ? QUIPS[first.kind].icon : "🌪️"}
                </span>
                {single ? (
                  <ToastBody event={first} quip={toast.quip} onOpen={() => open(toast, first.item)} />
                ) : (
                  <div className="toast-body">
                    <span className="toast-text">{toast.events.length} things happened</span>
                    <span className="toast-quip">{toast.quip}</span>
                    <ul className="toast-list">
                      {toast.events.map((event, i) => (
                        <li key={i}>
                          <button type="button" onClick={() => open(toast, event.item)}>
                            {QUIPS[event.kind].icon} {event.text} · {event.item.title}
                          </button>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                <button type="button" className="toast-close" aria-label="Dismiss" onClick={() => dismiss(toast.id)}>
                  ×
                </button>
              </div>
              {!toast.sticky && !toast.leaving && <span className="toast-countdown" />}
            </div>
          </div>
        );
      })}
      {hidden > 0 && <div className="toast-more">+{hidden} more</div>}
    </div>
  );
}

const CREATE_TONES = { running: "", waiting: "tone-wait", created: "tone-good", ended: "" };

/** A create session's toast: running and waiting ones stay (they are the only way to its terminal), created and ended ones count down. */
function CreateToastView({ toast, leaving, onDismiss, onOpen }: { toast: CreateToast; leaving: boolean; onDismiss: () => void; onOpen: (item: Item) => void }) {
  const { session, state, items } = toast;
  const live = state === "running" || state === "waiting";
  const fixes = session.purpose === "fix";
  const terminal = (
    <a className="toast-link" href={`/?terminal=${encodeURIComponent(agentTerminal(session.workspace, session.name))}`} target="_blank" rel="noreferrer">
      Open terminal ↗
    </a>
  );
  return (
    <div className={`toast ${CREATE_TONES[state]}`} role="status">
      <div className="toast-inner">
        {state === "running" ? <span className="toast-spinner" aria-hidden /> : <span className="toast-icon" aria-hidden>{state === "waiting" ? "✋" : state === "created" ? "✨" : "💤"}</span>}
        <div className="toast-body">
          {state === "running" && <span className="toast-text">{fixes ? "Fixing board…" : "Creating workitem…"}</span>}
          {state === "waiting" && <span className="toast-text">Needs your answer</span>}
          {state === "ended" && <span className="toast-text">{fixes ? "Board fix ended" : "Ended without a workitem"}</span>}
          {state === "created" ? (
            <ul className="toast-list">
              {items.map((item) => (
                <li key={item.dir}>
                  <button type="button" onClick={() => onOpen(item)}>
                    Created: {item.title}
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <span className="toast-item">
              <span className="toast-workspace">{session.workspace}</span>
            </span>
          )}
          {state === "ended" && <span className="toast-quip">Check the board{session.dead ? " or the terminal" : ""}.</span>}
          {(live || session.dead) && terminal}
        </div>
        {!live && (
          <button type="button" className="toast-close" aria-label="Dismiss" onClick={onDismiss}>
            ×
          </button>
        )}
      </div>
      {!live && !leaving && <span className="toast-countdown" />}
    </div>
  );
}

function ToastBody({ event, quip, onOpen }: { event: ActivityEvent; quip: string; onOpen?: () => void }) {
  const lines = (
    <>
      <span className="toast-text">{event.text}</span>
      <span className="toast-item">
        {event.item.title} <span className="toast-workspace">· {event.item.workspace}</span>
      </span>
      <span className="toast-quip">{quip}</span>
    </>
  );
  return onOpen ? (
    <button type="button" className="toast-body" onClick={onOpen}>
      {lines}
    </button>
  ) : (
    <div className="toast-body">{lines}</div>
  );
}
