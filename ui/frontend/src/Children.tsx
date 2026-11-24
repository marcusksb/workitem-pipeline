import { api } from "./api";
import { isBlocked, isWaiting, stageLabel } from "./boardState";
import { Item } from "./types";
import { Tab } from "./url";
import { useRun } from "./useRun";
import { WaitingChip } from "./WaitingChip";
import { ErrorText } from "./parts";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconBot, IconTerminalSquare } from "./icons";

type Props = { items: Item[]; onSelect: (item: Item, tab?: Tab) => void; onDone: () => void };

/** A list of items (an item's children, a plan's workitems): where each stands, why it is blocked, and a way into
 * its session (starting one first when it has none, which a blocked item allows). Blocked ones
 * come first. */
export function Children({ items, onSelect, onDone }: Props) {
  const { busy, error, run } = useRun(onDone);
  const rows = [...items].sort((a, b) => Number(isBlocked(b)) - Number(isBlocked(a)));

  return (
    <div>
      {error && <ErrorText>{error}</ErrorText>}
      <ul>
        {rows.map((child) => (
          <li key={child.dir} className="flex flex-col gap-1 border-b border-border py-2.5">
            <div className="child-head flex items-center justify-between gap-2">
              <button className="link cursor-pointer text-left text-[15px] font-semibold text-primary hover:underline" onClick={() => onSelect(child)}>
                {child.title}
              </button>
              {child.session ? (
                <Button disabled={busy} onClick={() => onSelect(child, "terminal")}>
                  <IconTerminalSquare />
                  Open session
                </Button>
              ) : (
                isBlocked(child) && (
                  <Button
                    disabled={busy}
                    onClick={() => run("Start session", async () => {
                      await api.startSession(child);
                      onSelect(child, "terminal");
                    })}
                  >
                    <IconBot />
                    Start session
                  </Button>
                )
              )}
            </div>
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge>{child.board}</Badge>
              <Badge>{stageLabel(child.stage)}</Badge>
              {isBlocked(child) && <Badge variant="danger">blocked</Badge>}
              {isWaiting(child) && <WaitingChip item={child} />}
              {child.agent_status === "working" && <Badge variant="wait">working</Badge>}
            </div>
            {isBlocked(child) && <p className="text-[15px] whitespace-pre-wrap text-destructive">{child.agent_status.replace(/^blocked:\s*/, "")}</p>}
          </li>
        ))}
      </ul>
    </div>
  );
}
