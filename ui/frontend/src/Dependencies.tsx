import { useState } from "react";
import { api } from "./api";
import { boardLabel, isBlocked, isWaiting, openDependencies, shownBoard, stageTitle } from "./boardState";
import { IconX } from "./icons";
import { Item, Pipelines } from "./types";
import { WaitingChip } from "./WaitingChip";
import { ErrorText, Muted } from "./parts";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { NativeSelect } from "@/components/ui/input";

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
    <div className="dependencies grid content-start gap-3">
      {item.depends_on.length === 0 && <Muted className="italic">Depends on nothing.</Muted>}
      <ul className="empty:hidden">
        {item.depends_on.map((dir) => {
          const other = items.find((i) => i.workspace === item.workspace && i.dir === dir);
          return (
            <li key={dir} className="flex flex-col gap-1 border-b border-border py-2.5">
              <div className="child-head flex items-center justify-between gap-2">
                {other ? (
                  <button className="link cursor-pointer text-left text-[15px] font-semibold text-primary hover:underline" onClick={() => onSelect(other)}>
                    {other.title}
                  </button>
                ) : (
                  <span className="font-mono text-[14px]">{dir}</span>
                )}
                <Button variant="ghost" size="icon" disabled={busy} aria-label={`Remove ${dir}`} onClick={() => void save(item.depends_on.filter((d) => d !== dir))}>
                  <IconX />
                </Button>
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                {other ? (
                  <>
                    <Badge>{boardLabel(pipelines, other.workspace, shownBoard(other, pipelines))}</Badge>
                    <Badge>{stageTitle(other, pipelines)}</Badge>
                    {open.includes(other) ? <Badge variant="wait">not merged</Badge> : <Badge variant="good">merged</Badge>}
                    {isBlocked(other) && <Badge variant="danger">blocked</Badge>}
                    {isWaiting(other) && <WaitingChip item={other} />}
                  </>
                ) : (
                  <span className="text-[15px] text-muted-foreground italic">gone, counts as merged</span>
                )}
              </div>
              {other && isBlocked(other) && <p className="text-[15px] whitespace-pre-wrap text-destructive">{other.agent_status.replace(/^blocked:\s*/, "")}</p>}
            </li>
          );
        })}
      </ul>
      {others.length > 0 && (
        <NativeSelect className="w-auto max-w-full" value="" disabled={busy} aria-label="Add a dependency" onChange={(e) => e.target.value && void save([...item.depends_on, e.target.value])}>
          <option value="">Add a dependency…</option>
          {others.map((other) => (
            <option key={other.dir} value={other.dir}>
              {other.dir} ({boardLabel(pipelines, other.workspace, other.board)})
            </option>
          ))}
        </NativeSelect>
      )}
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
