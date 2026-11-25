import confetti from "canvas-confetti";
import { AnimationEvent, useEffect, useRef, useState } from "react";
import { ActivityEvent, ActivityKind, activityEvents } from "./activity";
import { alertsFor } from "./boardState";
import { QUIPS, quipFor } from "./quips";
import { Item, Pipelines } from "./types";

/** Toasts on screen at once; older ones wait behind "+N more". */
const SHOWN = 4;
/** A poll with more events than this becomes one summary toast. */
const BURST = 4;

type Toast = { id: number; events: ActivityEvent[]; quip: string; sticky: boolean; leaving: boolean };

const tone = (kind: ActivityKind) => (kind === "blocked" ? "tone-bad" : "tone-good");

/**
 * A toast for each item done, PR approved or item newly blocked between two polls (see `activityEvents`), bottom left of the board, clear of the detail panel,
 * newest on top, with confetti when an item is done. Toasts leave when their countdown bar runs out (paused while hovered); blocked ones stay. Clicking one calls `onOpen`.
 * The item open in the detail panel (`openKey`) gets no blocked toast while the page has focus.
 */
export function ActivityToasts({ items, pipelines, onOpen, openKey }: { items: Item[] | undefined; pipelines: Pipelines | undefined; onOpen: (item: Item) => void; openKey: string | undefined }) {
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

  const dismiss = (id: number) => setToasts((shown) => shown.map((t) => (t.id === id ? { ...t, leaving: true } : t)));
  const ended = (id: number) => (e: AnimationEvent) => {
    if (e.animationName === "toast-countdown") dismiss(id);
    if (e.animationName === "toast-out") setToasts((shown) => shown.filter((t) => t.id !== id));
  };
  const open = (toast: Toast, item: Item) => {
    dismiss(toast.id);
    onOpen(item);
  };

  if (toasts.length === 0) return null;
  const hidden = toasts.length - SHOWN;
  return (
    <div className="activity-toasts" aria-live="polite">
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
