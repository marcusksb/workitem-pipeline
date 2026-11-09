import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { api } from "./api";
import { Dialog } from "./Dialog";
import { IconCheck, IconX } from "./icons";
import { markdownLinks } from "./markdownLinks";
import { Learnings as Data } from "./types";

type Props = { workspace: string; board: string; onClose: () => void };

const NEW_FILE = "# Learnings\n\n";

/** A board's `learnings.md`: the rules review fixers wrote down, and the human adds or prunes.
 * Saving sends the text the editor started from; the backend refuses if a fixer changed it since. */
export function Learnings({ workspace, board, onClose }: Props) {
  const [data, setData] = useState<Data>();
  const [error, setError] = useState<string>();
  const [text, setText] = useState<string>();
  const [busy, setBusy] = useState(false);

  const load = () =>
    api.learnings(workspace, board).then(setData, (e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  useEffect(() => {
    void load();
  }, [workspace, board]);

  const editing = text !== undefined && data !== undefined;
  const changed = editing && text !== (data.content || NEW_FILE);
  const save = async () => {
    if (!editing) return;
    setBusy(true);
    setError(undefined);
    try {
      await api.saveLearnings(workspace, board, data.content, text);
      setText(undefined);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };
  const close = () => {
    if (!changed || confirm("Discard your changes to the learnings?")) onClose();
  };

  const title = `Learnings in ${workspace} · ${board}`;
  return (
    <Dialog title={title} onClose={close}>
      <div className="source learnings">
        {data && (
          <p className="source-path">
            <code title={data.path}>{data.path}</code>
          </p>
        )}
        {data && (
          <div className="action-bar">
            {editing ? (
              <>
                <button className="primary" disabled={busy || !changed} onClick={() => void save()}>
                  <IconCheck />
                  Save
                </button>
                <button disabled={busy} onClick={() => (!changed || confirm("Discard your changes?")) && setText(undefined)}>
                  <IconX />
                  Cancel
                </button>
              </>
            ) : (
              <button onClick={() => setText(data.content || NEW_FILE)}>Edit</button>
            )}
            {error && <span className="error">{error}</span>}
          </div>
        )}
        {!data && !error && <p className="empty">Loading…</p>}
        {!data && error && <p className="error">{error}</p>}
        {editing ? (
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "s" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                if (!busy && changed) void save();
              }
            }}
            aria-label="Learnings"
            spellCheck
            autoFocus
          />
        ) : data && !data.content.trim() ? (
          <p className="empty">No learnings yet. Review fixers add them, or add your own with Edit.</p>
        ) : (
          data && (
            <div className="file-markdown learnings-body">
              <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{data.content}</Markdown>
            </div>
          )
        )}
      </div>
    </Dialog>
  );
}
