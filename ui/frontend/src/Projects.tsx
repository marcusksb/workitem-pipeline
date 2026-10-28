import { FormEvent, useState } from "react";
import { api } from "./api";
import { Dialog } from "./Dialog";
import { DEFAULT_BOARD } from "./boardState";
import { Pipelines, Project } from "./types";

type Props = {
  projects: Project[];
  pipelines: Pipelines | undefined;
  /** Board on screen, pre-selected for a new workitem. */
  board: string | undefined;
  selected: string | null;
  onSelect: (name: string | null) => void;
  onChanged: () => void;
  onLaunched: () => void;
};

export function Projects({ projects, pipelines, board: shown, selected, onSelect, onChanged, onLaunched }: Props) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [repo, setRepo] = useState("");
  const [error, setError] = useState<string>();
  const [creating, setCreating] = useState(false);
  const [input, setInput] = useState("");
  const target = selected ?? projects[0]?.name;
  const boards = (pipelines?.[target] ?? []).map((b) => b.name);
  const [board, setBoard] = useState(DEFAULT_BOARD);

  const launch = async (action: () => Promise<unknown>) => {
    try {
      await action();
      setError(undefined);
      onLaunched();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    }
  };

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (await launch(() => api.create(target, input, board))) {
      setInput("");
      setCreating(false);
    }
  };

  const add = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.addProject(name, repo);
      setName("");
      setRepo("");
      setAdding(false);
      setError(undefined);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const remove = async () => {
    if (!selected || !confirm(`Remove project "${selected}" from the UI? The repo is not touched.`)) return;
    await api.removeProject(selected);
    onSelect(null);
    onChanged();
  };

  return (
    <div className="projects">
      <select value={selected ?? ""} onChange={(e) => onSelect(e.target.value || null)}>
        <option value="">All projects</option>
        {projects.map((project) => (
          <option key={project.name} value={project.name}>
            {project.name}
          </option>
        ))}
      </select>
      {selected && (
        <button className="link" onClick={remove}>
          Remove
        </button>
      )}
      <button
        className="link"
        onClick={() => {
          setError(undefined);
          setAdding(true);
        }}
      >
        Add project
      </button>
      {target && (
        <button
          className="link"
          onClick={() => {
            setError(undefined);
            setBoard(shown && boards.includes(shown) ? shown : DEFAULT_BOARD);
            setCreating(true);
          }}
        >
          New workitem
        </button>
      )}
      {creating && target && (
        <Dialog title={`New workitem in ${target}`} onClose={() => setCreating(false)}>
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
                      {name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <button type="submit">Create</button>
            {error && <span className="error">{error}</span>}
          </form>
        </Dialog>
      )}
      {adding && (
        <Dialog title="Add project" onClose={() => setAdding(false)}>
          <form className="dialog-form" onSubmit={add}>
            <label>
              Name
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name" required />
            </label>
            <label>
              Repository path
              <input value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="/absolute/path/to/repo" required />
            </label>
            <button type="submit">Add</button>
            {error && <span className="error">{error}</span>}
          </form>
        </Dialog>
      )}
    </div>
  );
}
