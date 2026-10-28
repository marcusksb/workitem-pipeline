import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "./api";
import { Item, PrNote } from "./types";
import { usePolling } from "./usePolling";

const LABELS: Record<PrNote["kind"], string> = { comment: "commented", review: "reviewed", inline: "commented inline" };

function Note({ note }: { note: PrNote }) {
  const verdict = note.state?.replace("_", " ").toLowerCase();
  return (
    <article className="pr-note">
      <div className="pr-note-head">
        <strong>{note.author}</strong>
        <span>{verdict && note.state !== "COMMENTED" ? verdict : LABELS[note.kind]}</span>
        {note.path && <code>{note.path}</code>}
        <time dateTime={note.at}>{note.at && new Date(note.at).toLocaleString()}</time>
      </div>
      <Markdown remarkPlugins={[remarkGfm]}>{note.body}</Markdown>
    </article>
  );
}

/** PR description, then comments, review summaries and inline comments, newest first. */
export function PrConversation({ item, onUnauthorized }: { item: Item; onUnauthorized: () => void }) {
  const { data, error } = usePolling(() => api.prConversation(item), 30000, `${item.project}/${item.dir}/${item.pr}`, onUnauthorized);
  if (error) return <p className="error">{error}</p>;
  if (!data) return <p>Loading…</p>;
  return (
    <>
      <h3>Description</h3>
      {data.body.trim() ? <Markdown remarkPlugins={[remarkGfm]}>{data.body}</Markdown> : <p>No description.</p>}
      <h3>Comments ({data.notes.length})</h3>
      {data.notes.map((note, i) => (
        <Note key={`${note.at}-${i}`} note={note} />
      ))}
    </>
  );
}
