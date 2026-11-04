import { useState } from "react";
import { api } from "./api";
import { frontmatterFields } from "./boardState";
import { IssueStatusChip } from "./IssueStatusChip";
import { IconCheck, IconPencil, IconX } from "./icons";
import { Item } from "./types";

/** Fields only a stage move may change; the backend refuses them too. */
const READ_ONLY = ["stage", "board"];

const isUrl = (value: string) => /^https?:\/\/\S+$/.test(value);

function Value({ name, value, category }: { name: string; value: string; category: string }) {
  if (!value) return <span className="empty">empty</span>;
  if (isUrl(value))
    return (
      <a href={value} target="_blank" rel="noreferrer">
        {value} ↗
      </a>
    );
  if (name === "agent_status") return <span className={`chip ${value.startsWith("blocked") ? "blocked" : value.startsWith("waiting") ? "waiting" : "pr-moved"}`}>{value}</span>;
  if (name === "issue_status") return <IssueStatusChip status={value} category={category} />;
  if (name === "stage") return <span className="badge">{value}</span>;
  return <>{value}</>;
}

/** The frontmatter of a workitem as labelled fields; Edit turns the values into inputs. Only
 * the changed fields are saved, so an agent's edit to another field is never overwritten. */
export function Info({ item, content, onSaved }: { item: Item; content: string; onSaved: () => void }) {
  const fields = frontmatterFields(content);
  const category = fields.find(([name]) => name === "issue_status_category")?.[1] ?? "";
  const [draft, setDraft] = useState<Record<string, string>>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  if (fields.length === 0) return <p className="empty">No frontmatter.</p>;

  const changed = draft ? Object.fromEntries(fields.filter(([name, value]) => name in draft && draft[name] !== value).map(([name]) => [name, draft[name]])) : {};
  const close = () => {
    setDraft(undefined);
    setError(undefined);
  };
  const save = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await api.editFrontmatter(item, changed);
      close();
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  return (
    <>
      <div className="action-bar">
        {draft ? (
          <>
            <button className="primary" disabled={busy || Object.keys(changed).length === 0} onClick={() => void save()}>
              <IconCheck />
              Save
            </button>
            <button disabled={busy} onClick={close}>
              <IconX />
              Cancel
            </button>
          </>
        ) : (
          <button onClick={() => setDraft(Object.fromEntries(fields))}>
            <IconPencil />
            Edit
          </button>
        )}
        {error && <span className="error">{error}</span>}
      </div>
      <dl className="info">
        {fields.map(([name, value], i) => (
          <div key={`${name}-${i}`}>
            <dt>{name}</dt>
            <dd>
              {draft && !READ_ONLY.includes(name) ? (
                <input
                  value={draft[name] ?? value}
                  onChange={(e) => setDraft({ ...draft, [name]: e.target.value })}
                  onKeyDown={(e) => e.key === "Enter" && Object.keys(changed).length > 0 && void save()}
                  spellCheck={false}
                />
              ) : (
                <Value name={name} value={value} category={category} />
              )}
            </dd>
          </div>
        ))}
      </dl>
    </>
  );
}
