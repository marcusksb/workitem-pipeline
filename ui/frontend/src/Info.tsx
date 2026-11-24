import { useState } from "react";
import { api } from "./api";
import { frontmatterFields } from "./boardState";
import { IssueStatusChip } from "./IssueStatusChip";
import { IconCheck, IconPencil, IconX } from "./icons";
import { ActionBar, ErrorText, Muted } from "./parts";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Item } from "./types";

/** Fields only a stage move may change; the backend refuses them too. */
const READ_ONLY = ["stage", "board"];

const isUrl = (value: string) => /^https?:\/\/\S+$/.test(value);

function Value({ name, value, category }: { name: string; value: string; category: string }) {
  if (!value) return <span className="text-muted-foreground italic">empty</span>;
  if (isUrl(value))
    return (
      <a className="text-primary no-underline hover:underline" href={value} target="_blank" rel="noreferrer">
        {value} ↗
      </a>
    );
  if (name === "agent_status") return <Badge className="h-auto whitespace-normal" variant={value.startsWith("blocked") ? "danger" : value.startsWith("waiting") ? "outline" : "info"}>{value}</Badge>;
  if (name === "issue_status") return <IssueStatusChip status={value} category={category} />;
  if (name === "stage") return <Badge>{value}</Badge>;
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
  if (fields.length === 0) return <Muted className="italic">No frontmatter.</Muted>;

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
      <ActionBar>
        {draft ? (
          <>
            <Button variant="primary" disabled={busy || Object.keys(changed).length === 0} onClick={() => void save()}>
              <IconCheck />
              Save
            </Button>
            <Button disabled={busy} onClick={close}>
              <IconX />
              Cancel
            </Button>
          </>
        ) : (
          <Button onClick={() => setDraft(Object.fromEntries(fields))}>
            <IconPencil />
            Edit
          </Button>
        )}
        {error && <ErrorText>{error}</ErrorText>}
      </ActionBar>
      <dl className="grid text-[15px]">
        {fields.map(([name, value], i) => (
          <div key={`${name}-${i}`} className="grid grid-cols-[minmax(110px,160px)_1fr] items-baseline gap-3 py-1.5">
            <dt className="font-mono text-[14px] text-muted-foreground">{name}</dt>
            <dd className="min-w-0 [overflow-wrap:anywhere]">
              {draft && !READ_ONLY.includes(name) ? (
                <Input
                  className="h-7 px-1.5"
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
