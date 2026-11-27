import { api } from "./api";
import { Item } from "./types";
import { useRun } from "./useRun";
import { Button } from "@/components/ui/button";

type Runner = Pick<ReturnType<typeof useRun>, "busy" | "message" | "error" | "run">;

/** The buttons of `item.actions`, with what the last one reported. */
export function StageActionBar({ item, runner: { busy, message, error, run } }: { item: Item; runner: Runner }) {
  return (
    <div className="flex flex-wrap items-center gap-2 text-[15px]">
      {item.actions.map((action) => (
        <Button
          key={action.id}
          disabled={busy || (action.starts_session && item.session)}
          title={action.starts_session && item.session ? "The agent is already running" : undefined}
          onClick={() => run(action.label, () => api.runAction(item, action.id), action.confirm ?? undefined)}
        >
          {action.label}
        </Button>
      ))}
      {message && <span>{message}</span>}
      {error && <span className="text-destructive">{error}</span>}
    </div>
  );
}

/** The actions the item's stage offers it now (decision 84); the backend decides which, and checks again on a click. */
export function StageActions({ item, onDone }: { item: Item; onDone: () => void }) {
  const runner = useRun(onDone);
  return item.actions.length > 0 ? <StageActionBar item={item} runner={runner} /> : null;
}
