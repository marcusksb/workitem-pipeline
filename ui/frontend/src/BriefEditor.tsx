import { useState } from "react";
import { api } from "./api";
import { Item } from "./types";
import { IconCheck, IconX } from "./icons";

type Props = { item: Item; base: string; onClose: (saved: boolean) => void };

/** Edits the markdown after the frontmatter. `base` is the body it started from; the backend
 * refuses the save if the file changed since, so an agent's edit is never overwritten. */
export function BriefEditor({ item, base, onClose }: Props) {
  const [text, setText] = useState(base);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

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
    <div className="brief-editor">
      <div className="action-bar">
        <button className="primary" disabled={busy || text === base} onClick={() => void save()}>
          <IconCheck />
          Save
        </button>
        <button disabled={busy} onClick={cancel}>
          <IconX />
          Cancel
        </button>
        {item.session && <span className="muted">An agent session is running; it may edit the brief too.</span>}
        {error && <span className="error">{error}</span>}
      </div>
      <textarea
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
