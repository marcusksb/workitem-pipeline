import { api } from "./api";
import { isBlocked } from "./boardState";
import { Item } from "./types";
import { useRun } from "./useRun";
import { IconCheck, IconMessage, IconPencil } from "./icons";

type Props = { item: Item; onDone: () => void; onStarted: () => void; onEdit: () => void };

/** Approves the brief, opens an agent session to talk it through (`onStarted` then shows the
 * terminal), or opens the editor. */
export function BriefActions({ item, onDone, onStarted, onEdit }: Props) {
  const { busy, error, run } = useRun(onDone);
  const canDiscuss = !item.session && !isBlocked(item);
  const gate = item.gate;

  return (
    <div className="action-bar">
      {gate && (
        <button className="primary" disabled={busy} onClick={() => run(gate.label, () => api.approve(item))}>
          <IconCheck />
          {gate.label}
        </button>
      )}
      {canDiscuss && (
        <button
          disabled={busy}
          onClick={() =>
            run("Discuss brief", async () => {
              await api.discussBrief(item);
              onStarted();
            })
          }
        >
          <IconMessage />
          Discuss brief
        </button>
      )}
      <button disabled={busy} onClick={onEdit}>
        <IconPencil />
        Edit
      </button>
      {error && <span className="error">{error}</span>}
    </div>
  );
}
