import { api } from "./api";
import { isBlocked, isWaiting, stageLabel } from "./boardState";
import { Item } from "./types";
import { Tab } from "./url";
import { useRun } from "./useRun";
import { WaitingChip } from "./WaitingChip";
import { IconBot, IconTerminalSquare } from "./icons";

type Props = { items: Item[]; onSelect: (item: Item, tab?: Tab) => void; onDone: () => void };

/** The items whose `parent:` is this one: where each stands, why it is blocked, and a way into
 * its session (starting one first when it has none, which a blocked item allows). Blocked ones
 * come first. */
export function Children({ items, onSelect, onDone }: Props) {
  const { busy, error, run } = useRun(onDone);
  const rows = [...items].sort((a, b) => Number(isBlocked(b)) - Number(isBlocked(a)));

  return (
    <div className="brief children">
      {error && <p className="error">{error}</p>}
      <ul>
        {rows.map((child) => (
          <li key={child.dir} className={isBlocked(child) ? "blocked" : ""}>
            <div className="child-head">
              <button className="link" onClick={() => onSelect(child)}>
                {child.title}
              </button>
              {child.session ? (
                <button disabled={busy} onClick={() => onSelect(child, "terminal")}>
                  <IconTerminalSquare />
                  Open session
                </button>
              ) : (
                isBlocked(child) && (
                  <button
                    disabled={busy}
                    onClick={() => run("Start session", async () => {
                      await api.startSession(child);
                      onSelect(child, "terminal");
                    })}
                  >
                    <IconBot />
                    Start session
                  </button>
                )
              )}
            </div>
            <div className="meta">
              <span className="badge">{child.board}</span>
              <span className="badge">{stageLabel(child.stage)}</span>
              {isBlocked(child) && <span className="chip blocked">blocked</span>}
              {isWaiting(child) && <WaitingChip item={child} />}
              {child.agent_status === "working" && <span className="badge">working</span>}
            </div>
            {isBlocked(child) && <p className="reason">{child.agent_status.replace(/^blocked:\s*/, "")}</p>}
          </li>
        ))}
      </ul>
    </div>
  );
}
