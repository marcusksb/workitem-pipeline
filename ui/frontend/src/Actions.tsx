import { useState } from "react";
import { api } from "./api";
import { Item } from "./types";
import { useRun } from "./useRun";
import { stageLabel } from "./boardState";
import { IconCheck, IconTrash, IconUndo } from "./icons";

type Props = { item: Item; onDone: () => void };

/** Only the actions the workitem's current state allows; the backend enforces the same rules. */
export function Actions({ item, onDone }: Props) {
  const { busy, message, error, run } = useRun(onDone);
  const [comment, setComment] = useState("");

  const gate = item.gate;

  return (
    <div className="actions">
      {gate && (
        <section>
          <h3>Review</h3>
          <button className="primary" disabled={busy} onClick={() => run(gate.label, () => api.approve(item))}>
            <IconCheck />
            {gate.label}
          </button>
          <textarea value={comment} onChange={(e) => setComment(e.target.value)} placeholder="Why is this rejected?" rows={3} />
          <button disabled={busy || !comment.trim()} onClick={() => run("Reject", async () => {
            await api.reject(item, comment);
            setComment("");
          })}>
            <IconUndo />
            Reject → {stageLabel(gate.reject_to)}
          </button>
        </section>
      )}
      <section>
        <h3>Danger</h3>
        <button
          className="danger"
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
        </button>
      </section>
      {message && <p>{message}</p>}
      {error && <p className="error">{error}</p>}
    </div>
  );
}
