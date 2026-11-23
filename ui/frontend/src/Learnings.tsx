import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { api } from "./api";
import { Dialog } from "./Dialog";
import { IconCheck, IconX } from "./icons";
import { markdownLinks } from "./markdownLinks";
import { ActionBar, ErrorText, Muted, prose } from "./parts";
import { Learnings as Data } from "./types";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/** `label`: the board's title. */
type Props = { workspace: string; board: string; label: string; onClose: () => void };

const NEW_FILE = "# Learnings\n\n";

/** A board's `learnings.md`: the rules review fixers wrote down, and the human adds or prunes.
 * Saving sends the text the editor started from; the backend refuses if a fixer changed it since. */
export function Learnings({ workspace, board, label, onClose }: Props) {
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

  const title = `Learnings in ${workspace} · ${label}`;
  return (
    <Dialog title={title} onClose={close} className="max-w-[760px]">
      <div className="grid gap-2">
        {data && (
          <p className="source-path">
            <code className="font-mono text-[12px] [overflow-wrap:anywhere] text-muted-foreground" title={data.path}>
              {data.path}
            </code>
          </p>
        )}
        {data && (
          <ActionBar className="bg-popover pb-1">
            {editing ? (
              <>
                <Button variant="primary" disabled={busy || !changed} onClick={() => void save()}>
                  <IconCheck />
                  Save
                </Button>
                <Button disabled={busy} onClick={() => (!changed || confirm("Discard your changes?")) && setText(undefined)}>
                  <IconX />
                  Cancel
                </Button>
              </>
            ) : (
              <Button onClick={() => setText(data.content || NEW_FILE)}>Edit</Button>
            )}
            {error && <span className="text-destructive">{error}</span>}
          </ActionBar>
        )}
        {!data && !error && <Muted>Loading…</Muted>}
        {!data && error && <ErrorText>{error}</ErrorText>}
        {editing ? (
          <Textarea
            className="h-[55vh] resize-y font-mono leading-normal"
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
          <Muted>No learnings yet. Review fixers add them, or add your own with Edit.</Muted>
        ) : (
          data && (
            <div className={cn(prose, "max-h-[60vh] overflow-auto border-t border-border pt-2")}>
              <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{data.content}</Markdown>
            </div>
          )
        )}
      </div>
    </Dialog>
  );
}
