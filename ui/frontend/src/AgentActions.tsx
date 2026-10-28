import { api } from "./api";
import { isBlocked } from "./boardState";
import { Item } from "./types";
import { useRun } from "./useRun";
import { IconBot, IconPlay, IconStop, IconUnlock } from "./icons";

/** Agent controls shown above the terminal; the backend enforces the same rules. */
export function AgentActions({ item, onDone }: { item: Item; onDone: () => void }) {
  const { busy, message, error, run } = useRun(onDone);

  const canStart = item.approved && item.develops && !item.session && !isBlocked(item);

  return (
    <div className="action-bar">
      {canStart && (
        <button className="primary" disabled={busy} onClick={() => run("Start development", () => api.start(item))}>
          <IconPlay />
          Start development
        </button>
      )}
      {!item.session && !isBlocked(item) && (
        <button disabled={busy} onClick={() => run("Start session", () => api.startSession(item))}>
          <IconBot />
          Start agent session
        </button>
      )}
      {isBlocked(item) && (
        <button disabled={busy} onClick={() => run("Clear blocked", () => api.clearBlocked(item))}>
          <IconUnlock />
          Clear blocked
        </button>
      )}
      {item.session && (
        <button
          className="danger"
          disabled={busy}
          onClick={() => run("Kill session", () => api.killSession(item), `Kill the agent session for "${item.title}"? The dispatcher may retry it.`)}
        >
          <IconStop />
          Kill session
        </button>
      )}
      {message && <span>{message}</span>}
      {error && <span className="error">{error}</span>}
    </div>
  );
}
