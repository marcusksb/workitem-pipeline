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

type Props = { workspace: string; board: string; pipeline: Pipeline; onClose: () => void; onChanged: () => void };

/** A board's top-level settings (decision 73). `repo` is shown but fixed; stages and watchers are
 * edited in the board file. The backend checks the result before it writes. */
export function BoardSettings({ workspace, board, pipeline, onClose, onChanged }: Props) {
  const [form, setForm] = useState(() => formOf(pipeline));
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
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
          Workitem Sort Order: one item field per line, optionally followed by asc or desc; title breaks ties. Stages and watchers are edited in boards/{board}.json in
          the workspace folder.
        </Muted>
        <Button type="submit" variant="primary" disabled={busy}>
          Save
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}
