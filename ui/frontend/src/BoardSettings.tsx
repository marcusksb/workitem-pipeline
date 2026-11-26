import { FormEvent, useState } from "react";

import { api } from "./api";
import { Dialog, DialogForm, Field } from "./Dialog";
import { ErrorText, Muted } from "./parts";
import { BoardSettings as Settings, Pipeline } from "./types";
import { Button } from "@/components/ui/button";
import { Input, NativeSelect, Textarea } from "@/components/ui/input";

/** The dialog's fields, prefilled from the board: `order` is one entry per line. */
export const formOf = (pipeline: Pipeline) => ({
  label: pipeline.label ?? "",
  entry: pipeline.entry,
  order: pipeline.order.join("\n"),
  issue_project: pipeline.issue_project ?? "",
  reviewers: pipeline.reviewers ?? "",
});

/** What is sent: `order` split into lines, blank ones dropped. */
export const settingsOf = (form: ReturnType<typeof formOf>): Settings => ({
  ...form,
  order: form.order.split("\n").map((line) => line.trim()).filter(Boolean),
});

/** "1 workitem", "3 workitems". */
export const workitemCount = (n: number) => `${n} workitem${n === 1 ? "" : "s"}`;

type Props = {
  workspace: string;
  board: string;
  pipeline: Pipeline;
  /** How many workitems are on the board, for the delete confirmation. */
  items: number;
  onClose: () => void;
  onChanged: () => void;
  /** Called once the board was deleted. */
  onDeleted: () => void;
};

/** A board's top-level settings (decision 73). `repo` is shown but fixed; stages and watchers are
 * edited in the board file. The backend checks the result before it writes. */
export function BoardSettings({ workspace, board, pipeline, items, onClose, onChanged, onDeleted }: Props) {
  const [form, setForm] = useState(() => formOf(pipeline));
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const field = (key: keyof typeof form) => ({ value: form[key], onChange: (e: { target: { value: string } }) => setForm({ ...form, [key]: e.target.value }) });
  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      await api.updateBoard(workspace, board, settingsOf(form));
      onChanged();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };
  /** Deletes the board with its items (decision 77). A worktree git refused to remove is reported
   * after the board is gone, so the dialog stays open to show it. */
  const remove = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      const { worktrees_left } = await api.deleteBoard(workspace, board);
      onDeleted();
      if (worktrees_left.length === 0) {
        onClose();
        return;
      }
      setError(`The board is deleted, but these worktrees stay:\n${worktrees_left.join("\n")}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };
  if (deleting) {
    return (
      <Dialog title={`Delete ${workspace} · ${pipeline.label || board}`} onClose={onClose}>
        <DialogForm onSubmit={remove}>
          <Muted>
            Deletes the board and its {workitemCount(items)}: their agent sessions are stopped, and their worktrees and workitem folders are deleted, as are boards/{board}.json
            and boards/{board}/ (skills, watchers, learnings). This cannot be recovered. Branches and PRs stay.
          </Muted>
          <Button type="submit" variant="danger" disabled={busy}>
            Delete board and {workitemCount(items)}
          </Button>
          {error && <ErrorText>{error}</ErrorText>}
        </DialogForm>
      </Dialog>
    );
  }
  return (
    <Dialog title={`Settings of ${workspace} · ${pipeline.label || board}`} onClose={onClose}>
      <DialogForm onSubmit={save}>
        <Field label="Repository">
          <Input value={pipeline.repo ?? "none"} readOnly disabled />
        </Field>
        <Field label="Label">
          <Input {...field("label")} placeholder={board} autoFocus />
        </Field>
        <Field label="Entry stage">
          <NativeSelect {...field("entry")}>
            {pipeline.stages.map((stage) => (
              <option key={stage.name} value={stage.name}>
                {stage.label || stage.name}
              </option>
            ))}
          </NativeSelect>
        </Field>
        <Field label="Workitem Sort Order">
          <Textarea {...field("order")} rows={3} placeholder={"issue_status desc\ntitle"} spellCheck={false} />
        </Field>
        <Field label="Issue project">
          <Input {...field("issue_project")} placeholder="GitHub owner/repo or Jira project key (optional)" />
        </Field>
        <Field label="Reviewers">
          <Input {...field("reviewers")} placeholder="org/team (optional)" />
        </Field>
        <Muted>
          Workitem Sort Order: one item field per line, optionally followed by asc or desc; title breaks ties. A stage's model and issue requirement are set in its column's stage
          settings; the rest of the stages and the watchers are edited in boards/{board}.json in the workspace folder.
        </Muted>
        <Button type="submit" variant="primary" disabled={busy}>
          Save
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
        <Button type="button" variant="danger" disabled={busy} onClick={() => {
            setError(undefined);
            setDeleting(true);
          }}>
          Delete board
        </Button>
      </DialogForm>
    </Dialog>
  );
}
