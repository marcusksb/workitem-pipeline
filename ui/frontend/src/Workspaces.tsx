import { FormEvent, useState } from "react";
import { api } from "./api";
import { Dialog } from "./Dialog";
import { boardLabel } from "./boardState";
import { Pipelines, Workspace } from "./types";

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
  const [editingAgent, setEditingAgent] = useState(false);
  const [agent, setAgent] = useState("");
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

  const saveAgent = async (event: FormEvent) => {
    event.preventDefault();
    if (!selected) return;
    try {
      await api.setAgent(selected, agent);
      setEditingAgent(false);
      setError(undefined);
      onChanged();
    } catch (e) {
      fail(e);
    }
  };

  const current = workspaces.find((w) => w.name === selected);
  const selectedAgent = current?.agent;

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
      {selected && (
        <button
          className="link"
          title="Executable that runs this workspace's agent sessions"
          onClick={() => {
            setError(undefined);
            setAgent(selectedAgent ?? "");
            setEditingAgent(true);
          }}
        >
          Agent: {selectedAgent || "default"}
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
                placeholder="Jira key, Sentry URL or task description"
                rows={4}
                required
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
      {editingAgent && selected && (
        <Dialog title={`Agent for ${selected}`} onClose={() => setEditingAgent(false)}>
          <form className="dialog-form" onSubmit={saveAgent}>
            <label>
              Executable
              <input value={agent} onChange={(e) => setAgent(e.target.value)} placeholder="default (claude)" autoFocus />
            </label>
            <span className="hint">
              One executable on PATH or an absolute path, taking the same arguments as claude. Put flags in a wrapper script. Empty uses
              the default. Applies to sessions started from now on.
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
