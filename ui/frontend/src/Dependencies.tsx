import { useState } from "react";
import { api } from "./api";
import { boardLabel, isBlocked, isWaiting, openDependencies, shownBoard, stageTitle } from "./boardState";
import { IconX } from "./icons";
import { Item, Pipelines } from "./types";
import { WaitingChip } from "./WaitingChip";

type Props = { item: Item; items: Item[]; pipelines: Pipelines | undefined; onSelect: (item: Item) => void; onSaved: () => void };

/** The items this one waits for (`depends_on`): where each stands, whether it is merged, and a
 * picker over the other items of its workspace. A stage that requires it holds the item back
 * until they are merged. */
export function Dependencies({ item, items, pipelines, onSelect, onSaved }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const open = openDependencies(item, items, pipelines);
  const others = items.filter((i) => i.workspace === item.workspace && i.dir !== item.dir && !item.depends_on.includes(i.dir));
  const save = async (next: string[]) => {
    setBusy(true);
    setError(undefined);
    try {
      await api.editFrontmatter(item, { depends_on: next.join(", ") });
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };
  return (
    <div className="brief dependencies">
      {item.depends_on.length === 0 && <p className="empty">Depends on nothing.</p>}
      <ul>
        {item.depends_on.map((dir) => {
          const other = items.find((i) => i.workspace === item.workspace && i.dir === dir);
          return (
            <li key={dir} className={other && isBlocked(other) ? "blocked" : ""}>
              <div className="child-head">
                {other ? (
                  <button className="link" onClick={() => onSelect(other)}>
                    {other.title}
                  </button>
                ) : (
                  <span>{dir}</span>
                )}
                <button className="link" disabled={busy} aria-label={`Remove ${dir}`} onClick={() => void save(item.depends_on.filter((d) => d !== dir))}>
                  <IconX />
                </button>
              </div>
              <div className="meta">
                {other ? (
                  <>
                    <span className="badge">{boardLabel(pipelines, other.workspace, shownBoard(other, pipelines))}</span>
                    <span className="badge">{stageTitle(other, pipelines)}</span>
                    {open.includes(other) ? <span className="chip waiting">not merged</span> : <span className="chip pr-moved">merged</span>}
                    {isBlocked(other) && <span className="chip blocked">blocked</span>}
                    {isWaiting(other) && <WaitingChip item={other} />}
                  </>
                ) : (
                  <span className="empty">gone, counts as merged</span>
                )}
              </div>
              {other && isBlocked(other) && <p className="reason">{other.agent_status.replace(/^blocked:\s*/, "")}</p>}
            </li>
          );
        })}
      </ul>
      {others.length > 0 && (
        <select value="" disabled={busy} aria-label="Add a dependency" onChange={(e) => e.target.value && void save([...item.depends_on, e.target.value])}>
          <option value="">Add a dependency…</option>
          {others.map((other) => (
            <option key={other.dir} value={other.dir}>
              {other.dir} ({boardLabel(pipelines, other.workspace, other.board)})
            </option>
          ))}
        </select>
      )}
      {error && <span className="error">{error}</span>}
    </div>
  );
}
