import { FormEvent, useState } from "react";

import { api } from "./api";
import { blockedByBoard, boardNames, byStage, columnsFor, isWaiting, isBlocked, isWorking, itemKey, KIND_ICONS, boardLabel, openDependencies, orderFor, pauseState, repoName, shownBoard, stageTitle, treeOrder, UNKNOWN_STAGE, waitingFor } from "./boardState";
import { Dialog } from "./Dialog";
import { FollowUpsChip } from "./FollowUpsChip";
import { LinkBadges } from "./LinkBadges";
import { ReviewStrip } from "./ReviewStrip";
import { IconInfo, IconPause, IconPlay, IconTrash } from "./icons";
import { StageSource } from "./StageSource";
import { Item, Pipelines } from "./types";
import { useRun } from "./useRun";

type Props = {
  items: Item[];
  /** Boards of every workspace; `workspaces` are the ones on screen. */
  pipelines: Pipelines | undefined;
  workspaces: string[];
  /** Board asked for by the URL; falls back to the first one that exists. */
  board: string | undefined;
  onBoard: (name: string) => void;
  selected: string | null;
  onSelect: (item: Item) => void;
  showWorkspace: boolean;
  /** Called after items were removed or a column was paused. */
  onChanged: () => void;
};

/** Removes every item of a final column, one by one, the same way as "Remove workitem". */
function ClearColumn({ items, onDone }: { items: Item[]; onDone: () => void }) {
  const { busy, error, run } = useRun(onDone);
  const clear = async () => {
    const problems: string[] = [];
    for (const item of items) {
      try {
        const { worktree_left } = await api.remove(item);
        if (worktree_left) problems.push(`${item.title}: worktree stays: ${worktree_left}`);
      } catch (e) {
        problems.push(`${item.title}: ${e instanceof Error ? e.message : String(e)}`);
      }
    }
    if (problems.length) throw new Error(problems.join("\n"));
  };
  return (
    <>
      <button
        className="link clear"
        disabled={busy}
        title="Remove every workitem in this column"
        onClick={() =>
          run(
            "Clear",
            clear,
            `Remove all ${items.length} workitems in this column?\nTheir agent sessions are stopped, and their worktrees and workitem folders are deleted. This cannot be recovered. Branches and PRs stay.`,
          )
        }
      >
        <IconTrash />
        Clear
      </button>
      {error && <p className="error clear-error">{error}</p>}
    </>
  );
}

/** Pauses or resumes the watchers of a column in every workspace that has them. Running scripts and sessions go on. */
function PauseColumn({ workspaces, board, stage, label, paused, onDone }: { workspaces: string[]; board: string; stage: string; label: string; paused: boolean; onDone: () => void }) {
  const { busy, error, run } = useRun(onDone);
  const toggle = () => run(paused ? "Resume" : "Pause", () => Promise.all(workspaces.map((workspace) => api.pauseStage(workspace, board, stage, !paused))));
  return (
    <>
      {paused && <span className="chip paused">Paused</span>}
      <button
        className="link pause"
        disabled={busy}
        title={paused ? "Resume this column's watchers" : "Pause this column's watchers: nothing new starts here until you resume"}
        aria-label={`${paused ? "Resume" : "Pause"} ${label}`}
        onClick={toggle}
      >
        {paused ? <IconPlay /> : <IconPause />}
      </button>
      {error && <p className="error clear-error">{error}</p>}
    </>
  );
}

/** Adds a board for a repo to the workspace: a copy of the bundled board with that repo. */
function AddBoard({ workspace, onDone }: { workspace: string; onDone: () => void }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [repo, setRepo] = useState("");
  const [error, setError] = useState<string>();
  const add = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.addBoard(workspace, name, repo);
      setName("");
      setRepo("");
      setOpen(false);
      setError(undefined);
      onDone();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <>
      <button className="add-board" title="Add a board for a repository" onClick={() => setOpen(true)}>
        + Board
      </button>
      {open && (
        <Dialog title={`Add a board to ${workspace}`} onClose={() => setOpen(false)}>
          <form className="dialog-form" onSubmit={add}>
            <label>
              Name
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="backend" pattern="[A-Za-z0-9_-]+" required autoFocus />
            </label>
            <label>
              Repository path
              <input value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="/absolute/path/to/repo" required />
            </label>
            <span className="hint">Starts with the bundled stages. Edit boards/{name || "<name>"}.json in the workspace folder to change them.</span>
            <button type="submit">Add</button>
            {error && <span className="error">{error}</span>}
          </form>
        </Dialog>
      )}
    </>
  );
}

/** A dependency that is not merged yet; a click opens it on its own board. */
function DependencyChip({ item, pipelines, onOpen }: { item: Item; pipelines: Pipelines | undefined; onOpen: (item: Item) => void }) {
  return (
    <button
      className="chip waits-on"
      title={`Waits until ${item.dir} is merged`}
      onClick={(event) => {
        event.stopPropagation();
        onOpen(item);
      }}
    >
      waits on {item.dir} · {boardLabel(pipelines, item.workspace, shownBoard(item, pipelines))} · {stageTitle(item, pipelines)}
    </button>
  );
}

function Card({ item, selected, onSelect, showWorkspace, waitsOn, pipelines, onOpen, depth = 0 }: { item: Item; selected: boolean; onSelect: () => void; showWorkspace: boolean; waitsOn: Item[]; pipelines: Pipelines | undefined; onOpen: (item: Item) => void; depth?: number }) {
  const icon = KIND_ICONS[item.kind.trim().toLowerCase()];
  return (
    <div
      style={depth ? { marginLeft: depth * 16 } : undefined}
      className={`card${selected ? " selected" : ""}${isWorking(item) ? " working" : ""}${isBlocked(item) ? " blocked" : ""}${isWaiting(item) ? " waiting" : ""}`}
      title={isWaiting(item) ? `waiting: ${waitingFor(item) || "?"}` : undefined}
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      <div className="title">{item.title}</div>
      <div className="meta">
        {showWorkspace && <span className="badge">{item.workspace}</span>}
        <span className="badge">{item.kind}</span>
        <LinkBadges item={item} />
      </div>
      {waitsOn.map((other) => (
        <DependencyChip key={other.dir} item={other} pipelines={pipelines} onOpen={onOpen} />
      ))}
      {(item.review || icon || !!item.unpushed || item.follow_ups.length > 0) && (
        <div className="card-foot">
          <div className="foot-left">
            <FollowUpsChip followUps={item.follow_ups} />
            {!!item.unpushed && <div className="chip unpushed" title="Commits in the worktree that are not on any remote">{item.unpushed} unpushed</div>}
            <ReviewStrip review={item.review} />
          </div>
          {icon && <span className="kind-icon" title={item.kind}>{icon}</span>}
        </div>
      )}
    </div>
  );
}

type View = "board" | "list";
const VIEW_KEY = "workload.view";

function storedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "board";
  } catch {
    return "board";
  }
}

export function Board({ items, pipelines, workspaces, board, onBoard, selected, onSelect, showWorkspace, onChanged }: Props) {
  const names = boardNames(pipelines, workspaces);
  const active = board && names.includes(board) ? board : (names[0] ?? "");
  const columns = byStage(items, pipelines, active);
  const shown = columnsFor(pipelines, workspaces, active);
  if (pipelines && columns[UNKNOWN_STAGE].length) shown.push({ name: UNKNOWN_STAGE, label: UNKNOWN_STAGE, final: false });
  const blocked = blockedByBoard(items, pipelines);
  const broken = workspaces.flatMap((name) => {
    const found = pipelines?.[name]?.find((b) => b.name === active);
    return found?.error ? [{ workspace: name, pipeline: found.pipeline, error: found.error }] : [];
  });
  const [view, setView] = useState<View>(storedView);
  const chooseView = (next: View) => {
    setView(next);
    try {
      localStorage.setItem(VIEW_KEY, next);
    } catch {
      // the choice just is not remembered
    }
  };
  const [inspected, setInspected] = useState<{ name: string; label: string }>();
  const card = (item: Item, depth = 0) => (
    <Card
      key={itemKey(item)}
      item={item}
      depth={depth}
      selected={selected === itemKey(item)}
      onSelect={() => onSelect(item)}
      showWorkspace={showWorkspace}
      waitsOn={openDependencies(item, items, pipelines)}
      pipelines={pipelines}
      onOpen={onSelect}
    />
  );
  return (
    <>
      <nav className="board-tabs" role="tablist">
        {names.map((name) => {
          const board = workspaces.map((w) => pipelines?.[w]?.find((b) => b.name === name)?.pipeline).find(Boolean);
          const repo = board?.repo;
          return (
            <button key={name} role="tab" aria-selected={name === active} className={name === active ? "active" : ""} onClick={() => onBoard(name)} title={repo}>
              {board?.label || name}
              {repo && !board?.label && <span className="repo-name">{repoName(repo)}</span>}
              {blocked[name] ? <span className="count blocked" title="blocked items">{blocked[name]}</span> : null}
            </button>
          );
        })}
        {workspaces.length === 1 && <AddBoard workspace={workspaces[0]} onDone={onChanged} />}
        <div className="view-toggle" role="group" aria-label="View">
          {(["board", "list"] as const).map((v) => (
            <button key={v} aria-pressed={view === v} className={view === v ? "active" : ""} onClick={() => chooseView(v)}>
              {v}
            </button>
          ))}
        </div>
      </nav>
      {broken.map(({ workspace, pipeline, error }) => (
        <p className="error" key={workspace}>
          {workspace}: board "{active}" file not in use, {pipeline ? "showing the last valid one" : "no valid board yet"}. {error}
        </p>
      ))}
      <div className={`board${view === "list" ? " list" : ""}`}>
        {shown.map(({ name, label, final }) => {
          const inColumn = columns[name] ?? [];
          const pause = pauseState(pipelines, workspaces, active, name);
          return (
            <section className={`column${name === UNKNOWN_STAGE ? " unknown" : ""}${pause.paused ? " paused" : ""}`} data-stage={name} key={name}>
              <h2>
                {label} <span className="count">{inColumn.length}</span>
                {name !== UNKNOWN_STAGE && (
                  <button className="link info" title="What this stage runs" aria-label={`What ${label} runs`} onClick={() => setInspected({ name, label })}>
                    <IconInfo />
                  </button>
                )}
                {pause.workspaces.length > 0 && <PauseColumn workspaces={pause.workspaces} board={active} stage={name} label={label} paused={pause.paused} onDone={onChanged} />}
                {final && inColumn.length > 0 && <ClearColumn items={inColumn} onDone={onChanged} />}
              </h2>
              <div className="cards">{treeOrder(inColumn, orderFor(pipelines, workspaces, active, name)).map(({ item, depth }) => card(item, depth))}</div>
            </section>
          );
        })}
      </div>
      {inspected && <StageSource {...inspected} stage={inspected.name} board={active} workspaces={workspaces} onClose={() => setInspected(undefined)} />}
    </>
  );
}
