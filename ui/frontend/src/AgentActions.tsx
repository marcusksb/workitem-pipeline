import { api } from "./api";
import { isBlocked } from "./boardState";
import { Item } from "./types";
import { useRun } from "./useRun";
import { IconBot, IconPlay, IconStop, IconUnlock } from "./icons";
import { ActionBar } from "./parts";
import { Button } from "@/components/ui/button";

/** Agent controls shown above the terminal; the backend enforces the same rules. */
export function AgentActions({ item, onDone }: { item: Item; onDone: () => void }) {
  const { busy, message, error, run } = useRun(onDone);

  const canStart = item.approved && item.develops && !item.session && !isBlocked(item);

  return (
    <ActionBar className="empty:hidden">
      {canStart && (
        <Button variant="primary" disabled={busy} onClick={() => run("Start development", () => api.start(item))}>
          <IconPlay />
          Start development
        </Button>
      )}
      {!item.session && (
        <Button disabled={busy} onClick={() => run("Start session", () => api.startSession(item))}>
          <IconBot />
          Start agent session
        </Button>
      )}
      {isBlocked(item) && (
        <Button disabled={busy} onClick={() => run("Clear blocked", () => api.clearBlocked(item))}>
          <IconUnlock />
          Clear blocked
        </Button>
      )}
      {item.session && (
        <Button
          variant="danger"
          disabled={busy}
          onClick={() => run("Kill session", () => api.killSession(item), `Kill the agent session for "${item.title}"? The dispatcher may retry it.`)}
        >
          <IconStop />
          Kill session
        </Button>
      )}
      {message && <span>{message}</span>}
      {error && <span className="text-destructive">{error}</span>}
    </ActionBar>
  );
}
