import { api } from "./api";
import { Item } from "./types";
import { useRun } from "./useRun";
import { IconTrash } from "./icons";
import { ErrorText } from "./parts";
import { Button } from "@/components/ui/button";

type Props = { item: Item; onDone: () => void };

/** Only the actions the workitem's current state allows; the backend enforces the same rules.
 * Approve and send back sit in the gate bar above the tabs. */
export function Actions({ item, onDone }: Props) {
  const { busy, message, error, run } = useRun(onDone);

  return (
    <div className="grid content-start gap-4">
      <section className="grid justify-items-start gap-2">
        <h3 className="text-[13px] font-semibold text-muted-foreground">Danger</h3>
        <Button
          variant="danger"
          disabled={busy}
          onClick={() =>
            run(
              "Remove",
              async () => {
                const { worktree_left } = await api.remove(item);
                if (worktree_left) throw new Error(`Workitem removed, but the worktree stays: ${worktree_left}`);
              },
              `Remove workitem "${item.title}"?\nThe agent session is stopped, and the worktree and workitem folder are deleted. This cannot be recovered. Branch and PR stay.`,
            )
          }
        >
          <IconTrash />
          Remove workitem
        </Button>
      </section>
      {message && <p className="text-[13px]">{message}</p>}
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
