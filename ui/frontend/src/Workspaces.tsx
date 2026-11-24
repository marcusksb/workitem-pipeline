import { FormEvent, useState } from "react";
import { api } from "./api";
import { Dialog } from "./Dialog";
import { boardLabel } from "./boardState";
import { Pipelines, Workspace } from "./types";

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
      <form className="dialog-form" onSubmit={add}>
        <label>
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="content-delivery" pattern="[A-Za-z0-9_-]+" required autoFocus />
        </label>
        <span className="hint">Creates ~/.workitems-pipeline/workspaces/{name || "<name>"}. Add a board per repository next.</span>
        <button type="submit">Add</button>
        {error && <span className="error">{error}</span>}
      </form>
    </Dialog>
  );
}

export function SettingsDialog({ workspace, onClose, onSaved }: { workspace: Workspace; onClose: () => void; onSaved: () => void }) {
  const [agent, setAgent] = useState(workspace.agent ?? "");
  const [tracker, setTracker] = useState(workspace.issue_tracker);
  const [account, setAccount] = useState(workspace.github_account ?? "");
  const [error, setError] = useState<string>();
  const save = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.updateWorkspace(workspace.name, { agent, issue_tracker: tracker, github_account: account });
      onSaved();
    } catch (e) {
      setError(message(e));
    }
  };
  return (
    <Dialog title={`Settings of ${workspace.name}`} onClose={onClose}>
      <form className="dialog-form" onSubmit={save}>
        {workspace.account_error && <span className="error">{workspace.account_error}. No sessions or watchers run until it is fixed.</span>}
        <label>
          Agent
          <input value={agent} onChange={(e) => setAgent(e.target.value)} placeholder="default (claude)" autoFocus />
        </label>
        <span className="hint">
          One executable on PATH or an absolute path, taking the same arguments as claude. Put flags in a wrapper script. Empty uses
          the default. Applies to sessions started from now on.
        </span>
        <label>
          Issue tracker
          <select value={tracker} onChange={(e) => setTracker(e.target.value)}>
            <option value="jira">Jira</option>
            <option value="github">GitHub Issues</option>
          </select>
        </label>
        <span className="hint">
          Where items link their issue. Boards hold tracker-specific issue states, so pick it before adding boards.
        </span>
        <label>
          GitHub account
          <input value={account} onChange={(e) => setAccount(e.target.value)} placeholder="gh's active account" />
        </label>
        <span className="hint">
          A login gh is signed in to (gh auth login). Its token goes to this workspace's sessions, watchers and PR actions. Empty uses
          gh's active account.
        </span>
        <button type="submit">Save</button>
        {error && <span className="error">{error}</span>}
      </form>
    </Dialog>
  );
}

export function WatchersDialog({ workspace, onClose }: { workspace: Workspace; onClose: () => void }) {
  return (
    <Dialog title={`Watchers of ${workspace.name}`} onClose={onClose}>
      <div className="dialog-form">
        {workspace.error && <span className="error">{workspace.error}. The last valid version is in use.</span>}
        {workspace.watch?.length ? (
          <ul className="workspace-watchers">
            {workspace.watch.map((watch, index) => (
              <li key={index}>
                <code>{[watch.command, ...watch.args].join(" ")}</code> every {watch.every}
                {watch.implicit && <span className="hint"> (implicit: a board reads PRs)</span>}
              </li>
            ))}
          </ul>
        ) : (
          <span className="hint">None.</span>
        )}
        <span className="hint">
          Edit {workspace.dir}/workspace.toml to change them, e.g. <code>[[watch]]</code> with <code>every = "5m"</code> and{" "}
          <code>command = "issue-status"</code>. They get every item of the workspace on stdin and run in the workspace folder.
        </span>
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
      <form className="dialog-form" onSubmit={add}>
        <label>
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="backend" pattern="[A-Za-z0-9_-]+" required autoFocus />
        </label>
        <label>
          Repository path
          <input value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="/absolute/path/to/repo" required />
        </label>
        <label>
          Issue project
          <input value={issueProject} onChange={(e) => setIssueProject(e.target.value)} placeholder="GitHub owner/repo or Jira project key (optional)" />
        </label>
        <label>
          Reviewers
          <input value={reviewers} onChange={(e) => setReviewers(e.target.value)} placeholder="org/team (optional)" />
        </label>
        <span className="hint">
          Starts with the bundled stages and the issue states of the workspace's tracker. On GitHub an empty issue project means the
          repo's own issues; on Jira it is needed to create issues. Edit boards/{name || "<name>"}.json in the workspace folder to change
          any of it.
        </span>
        <button type="submit">Add</button>
        {error && <span className="error">{error}</span>}
      </form>
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
      <form className="dialog-form" onSubmit={create}>
        <label>
          Task
          <textarea
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
        </label>
        {boards.length > 1 && (
          <label>
            Board
            <select value={board} onChange={(e) => setBoard(e.target.value)}>
              {boards.map((name) => (
                <option key={name} value={name}>
                  {boardLabel(pipelines, workspace.name, name)}
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="hint">
          Enter creates, Shift+Enter adds a line. A task that needs changes in the repos of several boards becomes one linked item per board.
        </span>
        <button type="submit">Create</button>
        {error && <span className="error">{error}</span>}
      </form>
    </Dialog>
  );
}
