import { FormEvent, useEffect, useState } from "react";
import { api } from "./api";
import { Dialog, DialogForm, Field } from "./Dialog";
import { boardLabel } from "./boardState";
import { ErrorText, Muted } from "./parts";
import { AGENT_KINDS, Agent, AgentKind, AgentSettings, Pipelines, Workspace } from "./types";
import { Button } from "@/components/ui/button";
import { Input, NativeSelect, Textarea } from "@/components/ui/input";

/** The dialogs of a workspace: the sidebar and the top bar open them. */

export const trackerName = (tracker: string) => (tracker === "github" ? "GitHub Issues" : "Jira");

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function AddWorkspaceDialog({ onClose, onAdded }: { onClose: () => void; onAdded: (name: string) => void }) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string>();
  const add = async (event: FormEvent) => {
    event.preventDefault();
    try {
      const workspace = await api.addWorkspace(name);
      onAdded(workspace.name);
    } catch (e) {
      setError(message(e));
    }
  };
  return (
    <Dialog title="Add workspace" onClose={onClose}>
      <DialogForm onSubmit={add}>
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="content-delivery" pattern="[A-Za-z0-9_-]+" required autoFocus />
        </Field>
        <Muted>Creates ~/.workitems-pipeline/workspaces/{name || "<name>"}. Add a board per repository next.</Muted>
        <Button type="submit" variant="primary">
          Add
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}

export const agentLabel = (agent: Agent) => (agent.command ? `${agent.kind} (${agent.command})` : agent.kind);

/** `kind` and `command` as the API takes them; an empty kind is none. */
const agentOf = (kind: AgentKind | "", command: string): Agent | null => (kind ? { kind, command: command.trim() || undefined } : null);

/** Kind and executable of an agent (decision 56). With `fallback`, an empty kind means that default. */
function AgentFields(props: {
  kind: AgentKind | "";
  command: string;
  onKind: (kind: AgentKind | "") => void;
  onCommand: (command: string) => void;
  settings?: AgentSettings;
  fallback?: string;
}) {
  const installed = (kind: AgentKind) => props.settings?.kinds.find((k) => k.kind === kind)?.installed;
  return (
    <>
      <Field label="Agent">
        <NativeSelect value={props.kind} onChange={(e) => props.onKind(e.target.value as AgentKind | "")} autoFocus>
          {props.fallback !== undefined && <option value="">Default ({props.fallback})</option>}
          {AGENT_KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {installed(kind) === false ? `${kind} (not found on PATH)` : kind}
            </option>
          ))}
        </NativeSelect>
      </Field>
      {props.kind && (
        <Field label="Executable">
          <Input value={props.command} onChange={(e) => props.onCommand(e.target.value)} placeholder={props.kind} />
        </Field>
      )}
    </>
  );
}

export function SettingsDialog({ workspace, onClose, onSaved }: { workspace: Workspace; onClose: () => void; onSaved: () => void }) {
  const [kind, setKind] = useState<AgentKind | "">(workspace.agent?.kind ?? "");
  const [command, setCommand] = useState(workspace.agent?.command ?? "");
  const [settings, setSettings] = useState<AgentSettings>();
  const [editingDefault, setEditingDefault] = useState(false);
  const [tracker, setTracker] = useState(workspace.issue_tracker);
  const [account, setAccount] = useState(workspace.github_account ?? "");
  const [error, setError] = useState<string>();
  useEffect(() => {
    if (!editingDefault) api.agentSettings().then(setSettings, (e) => setError(message(e)));
  }, [editingDefault]);
  const save = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.updateWorkspace(workspace.name, { agent: agentOf(kind, command), issue_tracker: tracker, github_account: account });
      onSaved();
    } catch (e) {
      setError(message(e));
    }
  };
  if (editingDefault && settings) return <DefaultAgentDialog settings={settings} onClose={() => setEditingDefault(false)} />;
  return (
    <Dialog title={`Settings of ${workspace.name}`} onClose={onClose}>
      <DialogForm onSubmit={save}>
        {workspace.account_error && <ErrorText>{workspace.account_error}. No sessions or watchers run until it is fixed.</ErrorText>}
        <AgentFields kind={kind} command={command} onKind={setKind} onCommand={setCommand} settings={settings} fallback={settings?.agent ? agentLabel(settings.agent) : "claude"} />
        <Muted>
          The CLI that runs this workspace's sessions, planning included. The executable is one name on PATH or an absolute path, for
          example a wrapper script that adds flags; empty runs the kind's own CLI. Applies to sessions started from now on.{" "}
          <button type="button" className="underline underline-offset-2 hover:text-foreground" onClick={() => setEditingDefault(true)} disabled={!settings}>
            Edit the default of all workspaces
          </button>
        </Muted>
        <Field label="Issue tracker">
          <NativeSelect value={tracker} onChange={(e) => setTracker(e.target.value)}>
            <option value="jira">Jira</option>
            <option value="github">GitHub Issues</option>
          </NativeSelect>
        </Field>
        <Muted>Where items link their issue. Boards hold tracker-specific issue states, so pick it before adding boards.</Muted>
        <Field label="GitHub account">
          <Input value={account} onChange={(e) => setAccount(e.target.value)} placeholder="gh's active account" />
        </Field>
        <Muted>
          A login gh is signed in to (gh auth login). Its token goes to this workspace's sessions, watchers and PR actions. Empty uses
          gh's active account.
        </Muted>
        <Button type="submit" variant="primary">
          Save
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}

/** The agent of every workspace without its own (`config.toml`). */
function DefaultAgentDialog({ settings, onClose }: { settings: AgentSettings; onClose: () => void }) {
  const [kind, setKind] = useState<AgentKind | "">(settings.agent?.kind ?? "claude");
  const [command, setCommand] = useState(settings.agent?.command ?? "");
  const [error, setError] = useState<string>();
  const save = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.setDefaultAgent(agentOf(kind, command));
      onClose();
    } catch (e) {
      setError(message(e));
    }
  };
  return (
    <Dialog title="Default agent of all workspaces" onClose={onClose}>
      <DialogForm onSubmit={save}>
        <AgentFields kind={kind} command={command} onKind={setKind} onCommand={setCommand} settings={settings} />
        <Muted>Runs the sessions of every workspace without an agent of its own, from now on.</Muted>
        <Button type="submit" variant="primary">
          Save
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}

export function WatchersDialog({ workspace, onClose }: { workspace: Workspace; onClose: () => void }) {
  return (
    <Dialog title={`Watchers of ${workspace.name}`} onClose={onClose}>
      <div className="grid gap-3 text-[15px]">
        {workspace.error && <ErrorText>{workspace.error}. The last valid version is in use.</ErrorText>}
        {workspace.watch?.length ? (
          <ul className="grid list-disc gap-1 pl-5">
            {workspace.watch.map((watch, index) => (
              <li key={index}>
                <code className="font-mono text-[14px]">{[watch.command, ...watch.args].join(" ")}</code> every {watch.every}
                {watch.implicit && <span className="text-muted-foreground"> (implicit: a board reads PRs)</span>}
              </li>
            ))}
          </ul>
        ) : (
          <Muted>None.</Muted>
        )}
        <Muted className="[&_code]:font-mono [&_code]:text-[14px]">
          Edit {workspace.dir}/workspace.toml to change them, e.g. <code>[[watch]]</code> with <code>every = "5m"</code> and{" "}
          <code>command = "issue-status"</code>. They get every item of the workspace on stdin and run in the workspace folder.
        </Muted>
      </div>
    </Dialog>
  );
}

/** Adds a board for a repo to the workspace: a copy of the bundled board with that repo, where its
 * new issues go and the team that reviews its PRs. */
export function AddBoardDialog({ workspace, onClose, onAdded }: { workspace: string; onClose: () => void; onAdded: () => void }) {
  const [name, setName] = useState("");
  const [repo, setRepo] = useState("");
  const [issueProject, setIssueProject] = useState("");
  const [reviewers, setReviewers] = useState("");
  const [error, setError] = useState<string>();
  const add = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.addBoard(workspace, { name, repo, issue_project: issueProject, reviewers });
      onAdded();
    } catch (e) {
      setError(message(e));
    }
  };
  return (
    <Dialog title={`Add a board to ${workspace}`} onClose={onClose}>
      <DialogForm onSubmit={add}>
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="backend" pattern="[A-Za-z0-9_-]+" required autoFocus />
        </Field>
        <Field label="Repository path">
          <Input value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="/absolute/path/to/repo" required />
        </Field>
        <Field label="Issue project">
          <Input value={issueProject} onChange={(e) => setIssueProject(e.target.value)} placeholder="GitHub owner/repo or Jira project key (optional)" />
        </Field>
        <Field label="Reviewers">
          <Input value={reviewers} onChange={(e) => setReviewers(e.target.value)} placeholder="org/team (optional)" />
        </Field>
        <Muted>
          Starts with the bundled stages and the issue states of the workspace's tracker. On GitHub an empty issue project means the
          repo's own issues; on Jira it is needed to create issues. Edit boards/{name || "<name>"}.json in the workspace folder to change
          any of it.
        </Muted>
        <Button type="submit" variant="primary">
          Add
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}

type NewWorkitemProps = {
  workspace: Workspace;
  pipelines: Pipelines | undefined;
  /** The board on screen, pre-selected. */
  board: string;
  /** The task text; it lives with the opener so a closed dialog keeps it. */
  input: string;
  onInput: (input: string) => void;
  onClose: () => void;
  onCreated: () => void;
};

export function NewWorkitemDialog({ workspace, pipelines, board: onScreen, input, onInput, onClose, onCreated }: NewWorkitemProps) {
  const [board, setBoard] = useState(onScreen);
  const [error, setError] = useState<string>();
  const boards = (pipelines?.[workspace.name] ?? []).map((b) => b.name);
  const create = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.create(workspace.name, input, board);
      onCreated();
    } catch (e) {
      setError(message(e));
    }
  };
  return (
    <Dialog title={`New workitem in ${workspace.name}`} onClose={onClose}>
      <DialogForm onSubmit={create}>
        <Field label="Task">
          <Textarea
            value={input}
            onChange={(e) => onInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                e.currentTarget.form?.requestSubmit();
              }
            }}
            placeholder={`${workspace.issue_tracker === "github" ? "GitHub issue" : "Jira key"}, Sentry URL or task description`}
            rows={4}
            required
            autoFocus
          />
        </Field>
        {boards.length > 1 && (
          <Field label="Board">
            <NativeSelect value={board} onChange={(e) => setBoard(e.target.value)}>
              {boards.map((name) => (
                <option key={name} value={name}>
                  {boardLabel(pipelines, workspace.name, name)}
                </option>
              ))}
            </NativeSelect>
          </Field>
        )}
        <Muted>
          Enter creates, Shift+Enter adds a line. A task that needs changes in the repos of several boards becomes one linked item per board.
        </Muted>
        <Button type="submit" variant="primary">
          Create
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}
