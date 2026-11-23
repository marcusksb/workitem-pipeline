import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "./api";
import { markdownLinks } from "./markdownLinks";
import { ErrorText, Muted } from "./parts";
import { Item, PrNote } from "./types";
import { usePolling } from "./usePolling";

const LABELS: Record<PrNote["kind"], string> = { comment: "commented", review: "reviewed", inline: "commented inline" };

function Note({ note }: { note: PrNote }) {
  const verdict = note.state?.replace("_", " ").toLowerCase();
  return (
    <article className="my-2 rounded-md border border-border px-3 py-2 [&>:last-child]:mb-0">
      <div className="flex flex-wrap gap-2 text-[12px] text-muted-foreground">
        <strong>{note.author}</strong>
        <span>{verdict && note.state !== "COMMENTED" ? verdict : LABELS[note.kind]}</span>
        {note.path && <code className="text-[12px]">{note.path}</code>}
        <time dateTime={note.at}>{note.at && new Date(note.at).toLocaleString()}</time>
      </div>
      <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{note.body}</Markdown>
    </article>
  );
}

/** PR description, then comments, review summaries and inline comments, newest first. */
export function PrConversation({ item, onUnauthorized }: { item: Item; onUnauthorized: () => void }) {
  const { data, error } = usePolling(() => api.prConversation(item), 30000, `${item.workspace}/${item.dir}/${item.pr}`, onUnauthorized);
  if (error) return <ErrorText>{error}</ErrorText>;
  if (!data) return <Muted>Loading…</Muted>;
  return (
    <>
      <h3>Description</h3>
      {data.body.trim() ? <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{data.body}</Markdown> : <p>No description.</p>}
      <h3>Comments ({data.notes.length})</h3>
      {data.notes.map((note, i) => (
        <Note key={`${note.at}-${i}`} note={note} />
      ))}
    </>
  );
}
