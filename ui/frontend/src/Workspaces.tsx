import { FormEvent, useState } from "react";
import { api } from "./api";
import { Dialog } from "./Dialog";
import { boardLabel } from "./boardState";
import { Pipelines, Workspace } from "./types";

const trackerName = (tracker: string) => (tracker === "github" ? "GitHub Issues" : "Jira");

type Props = {
  workspaces: Workspace[];
  pipelines: Pipelines | undefined;
  /** Board on screen, pre-selected for a new workitem. */
  board: string | undefined;
  /** The workspace on screen; there is always exactly one once any exists. */
  selected: string | undefined;
  onSelect: (name: string) => void;
  onChanged: () => void;
  onLaunched: () => void;
};

export function Workspaces({ workspaces, pipelines, board: shown, selected, onSelect, onChanged, onLaunched }: Props) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState<string>();
  const [creating, setCreating] = useState(false);
  const [editingSettings, setEditingSettings] = useState(false);
  const [agent, setAgent] = useState("");
  const [tracker, setTracker] = useState("jira");
  const [account, setAccount] = useState("");
  const [showingWatchers, setShowingWatchers] = useState(false);
  const [input, setInput] = useState("");
  const boards = (selected ? (pipelines?.[selected] ?? []) : []).map((b) => b.name);
  const onScreen = shown && boards.includes(shown) ? shown : boards[0];
  const [board, setBoard] = useState("");

  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (!selected) return;
    try {
      await api.create(selected, input, board);
      setError(undefined);
      onLaunched();
      setInput("");
      setCreating(false);
    } catch (e) {
      fail(e);
    }
  };

  const add = async (event: FormEvent) => {
    event.preventDefault();
    try {
      const workspace = await api.addWorkspace(name);
      setName("");
      setAdding(false);
      setError(undefined);
      onChanged();
      onSelect(workspace.name);
    } catch (e) {
      fail(e);
    }
  };

  const saveSettings = async (event: FormEvent) => {
    event.preventDefault();
    if (!selected) return;
    try {
      await api.updateWorkspace(selected, { agent, issue_tracker: tracker, github_account: account });
      setEditingSettings(false);
      setError(undefined);
      onChanged();
    } catch (e) {
      fail(e);
    }
  };

  const current = workspaces.find((w) => w.name === selected);

  return (
    <div className="workspaces">
      {workspaces.length > 0 && (
        <select value={selected ?? ""} onChange={(e) => onSelect(e.target.value)} aria-label="Workspace">
          {workspaces.map((workspace) => (
            <option key={workspace.name} value={workspace.name}>
              {workspace.name}
            </option>
          ))}
        </select>
      )}
      {current && (
        <button
          className={current.account_error ? "link error" : "link"}
          title={current.account_error ?? "Agent, issue tracker and GitHub account of this workspace"}
          onClick={() => {
            setError(undefined);
            setAgent(current.agent ?? "");
            setTracker(current.issue_tracker);
            setAccount(current.github_account ?? "");
            setEditingSettings(true);
          }}
        >
          {current.account_error ? "Stopped: GitHub account not logged in" : `Settings: ${current.agent || "default agent"} · ${trackerName(current.issue_tracker)}`}
        </button>
      )}
      {current && (
        <button
          className={current.error ? "link error" : "link"}
          title={current.error ?? "Scripts that run once for the whole workspace, from workspace.toml"}
          onClick={() => setShowingWatchers(true)}
        >
          Watchers{current.error ? ": invalid workspace.toml" : ""}
        </button>
      )}
      <button
        className="link"
        onClick={() => {
          setError(undefined);
          setAdding(true);
        }}
      >
        Add workspace
      </button>
      {selected && onScreen && (
        <button
          className="link"
          onClick={() => {
            setError(undefined);
            setBoard(onScreen);
            setCreating(true);
          }}
        >
          New workitem
        </button>
      )}
      {creating && selected && (
        <Dialog title={`New workitem in ${selected}`} onClose={() => setCreating(false)}>
          <form className="dialog-form" onSubmit={create}>
            <label>
              Task
              <textarea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                placeholder={`${current?.issue_tracker === "github" ? "GitHub issue" : "Jira key"}, Sentry URL or task description`}
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
                      {boardLabel(pipelines, selected, name)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <span className="hint">A task that needs changes in the repos of several boards becomes one linked item per board.</span>
            <button type="submit">Create</button>
            {error && <span className="error">{error}</span>}
          </form>
        </Dialog>
      )}
      {editingSettings && current && (
        <Dialog title={`Settings of ${current.name}`} onClose={() => setEditingSettings(false)}>
          <form className="dialog-form" onSubmit={saveSettings}>
            {current.account_error && <span className="error">{current.account_error}. No sessions or watchers run until it is fixed.</span>}
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
      )}
      {showingWatchers && current && (
        <Dialog title={`Watchers of ${current.name}`} onClose={() => setShowingWatchers(false)}>
          <div className="dialog-form">
            {current.error && <span className="error">{current.error}. The last valid version is in use.</span>}
            {current.watch?.length ? (
              <ul className="workspace-watchers">
                {current.watch.map((watch, index) => (
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
              Edit {current.dir}/workspace.toml to change them, e.g. <code>[[watch]]</code> with <code>every = "5m"</code> and{" "}
              <code>command = "issue-status"</code>. They get every item of the workspace on stdin and run in the workspace folder.
            </span>
          </div>
        </Dialog>
      )}
      {adding && (
        <Dialog title="Add workspace" onClose={() => setAdding(false)}>
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
      )}
    </div>
  );
}
