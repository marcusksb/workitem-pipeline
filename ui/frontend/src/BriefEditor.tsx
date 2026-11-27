import { useEffect, useState } from "react";
import { api } from "./api";
import { Item } from "./types";
import { IconCheck, IconX } from "./icons";
import { ActionBar } from "./parts";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/input";

type Props = { item: Item; base: string; onClose: (saved: boolean) => void; onDirtyChange: (dirty: boolean) => void };

/** Edits the markdown after the frontmatter. `base` is the body it started from; the backend
 * refuses the save if the file changed since, so an agent's edit is never overwritten. */
export function BriefEditor({ item, base, onClose, onDirtyChange }: Props) {
  const [text, setText] = useState(base);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  useEffect(() => {
    onDirtyChange(text !== base);
  }, [text, base, onDirtyChange]);
  // The editor may go away while App still thinks it is dirty.
  useEffect(() => () => onDirtyChange(false), [onDirtyChange]);

  const save = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await api.editBrief(item, base, text);
      onClose(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  const cancel = () => {
    if (text === base || confirm("Discard your changes to the brief?")) onClose(false);
  };

  return (
    <div
      className="flex min-h-0 flex-1 flex-col pb-4"
      onKeyDown={(e) => {
        if (e.key !== "Escape" || e.defaultPrevented || e.nativeEvent.isComposing) return;
        // Escape cancels the edit (asking before discarding) instead of closing the panel.
        e.stopPropagation();
        // A save is not interruptible, like the disabled Cancel button; the outcome is the saved brief.
        if (!busy) cancel();
      }}
    >
      <ActionBar>
        <Button variant="primary" disabled={busy || text === base} onClick={() => void save()}>
          <IconCheck />
          Save
        </Button>
        <Button disabled={busy} onClick={cancel}>
          <IconX />
          Cancel
        </Button>
        {item.session && <span className="text-muted-foreground">An agent session is running; it may edit the brief too.</span>}
        {error && <span className="text-destructive">{error}</span>}
      </ActionBar>
      <Textarea
        className="flex-1 resize-none p-2 font-mono leading-normal"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "s" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            if (!busy && text !== base) void save();
          }
        }}
        spellCheck
        autoFocus
      />
    </div>
  );
}
