import { api } from "./api";
import { Item, StageAction } from "./types";
import { useRun } from "./useRun";
import { Button } from "@/components/ui/button";

type Runner = Pick<ReturnType<typeof useRun>, "busy" | "message" | "error" | "run">;

/** Why a session-starting action cannot run now: the backend starts a session only for an idle item that is `ok`. */
const sessionBlock = (item: Item, action: StageAction) =>
  !action.starts_session ? undefined : item.session ? "The agent is already running" : item.agent_status !== "ok" ? `The item is ${item.agent_status}` : undefined;

/** The buttons of `item.actions`, with what the last one reported. `onSession` follows a skill action into its session. */
export function StageActionBar({ item, runner: { busy, message, error, run }, onSession }: { item: Item; runner: Runner; onSession: () => void }) {
  return (
    <div className="flex flex-wrap items-center gap-2 text-[15px]">
      {item.actions.map((action) => {
        const blocked = sessionBlock(item, action);
        return (
          <Button
            key={action.id}
            variant="primary"
            disabled={busy || blocked !== undefined}
            title={blocked}
            onClick={() =>
              run(
                action.label,
                async () => {
                  if (await api.runAction(item, action.id)) onSession();
                },
                action.confirm ?? undefined,
              )
            }
          >
            {action.label}
          </Button>
        );
      })}
      {message && <span>{message}</span>}
      {error && <span className="text-destructive">{error}</span>}
    </div>
  );
}

/** The actions the item's stage offers it now (decision 86); the backend decides which, and checks again on a click.
 * The bar stays while it has something to report, so an action that changed which ones match still shows its outcome. */
export function StageActions({ item, onDone, onSession }: { item: Item; onDone: () => void; onSession: () => void }) {
  const runner = useRun(onDone);
  return item.actions.length > 0 || runner.message || runner.error ? <StageActionBar item={item} runner={runner} onSession={onSession} /> : null;
}
